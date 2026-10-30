//! Stundenuhr-Editor und AutoDJ (Paket 28).
//!
//! Der Editor bearbeitet die Stundenuhren und die Wochenzuordnung (`stundenuhr.json`), der
//! AutoDJ-Teil hält die Playlist des Radio-Modus gefüllt: Er trägt gespielte Titel in die
//! Historie ein und hängt hinten neue Titel an, bis `lookahead` Titel bereitliegen. Von Hand
//! gesetzte Titel werden nie angefasst. Die Wahl selbst steckt in `woa_library::rotation`.
//!
//! Alles läuft im UI-Thread und ist ohne Fenster testbar ([`ClockEditor::autodj_tick`]); mit
//! dem Audio-Thread hat nichts davon zu tun.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use eframe::egui;
use woa_library::Library;
use woa_library::rotation::{
    self, Ahead, AutoDjState, Deadline, HourClock, MAX_CLOCKS, MAX_SLOTS, Schedule, Slot,
    WEEK_HOURS,
};
use woa_radio::station::View;

use crate::theme;

const DAYS: [&str; 7] = ["Mo", "Di", "Mi", "Do", "Fr", "Sa", "So"];
/// So oft schaut der AutoDJ nach, ob die Playlist aufgefüllt werden muss.
/// Kategorie, deren Auswahl als Voice-Track zwischen zwei Titel kommt (Paket 30).
pub const VOICE_CATEGORY: &str = "moderation";
const PLAN_EVERY: Duration = Duration::from_secs(3);
/// Änderungen werden so lange gesammelt, dann gespeichert.
const SAVE_AFTER: Duration = Duration::from_millis(700);
/// Zeilen im Auswahl-Protokoll.
const LOG_LINES: usize = 12;

pub fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// Zustand von Editor und AutoDJ.
pub struct ClockEditor {
    pub open: bool,
    dir: PathBuf,
    pub schedule: Schedule,
    loaded: bool,
    pub error: Option<String>,
    selected: usize,
    dirty_since: Option<Instant>,
    lib: Option<Library>,
    tags: Vec<String>,
    state: AutoDjState,
    /// Kategorie je angehängtem Titel (Pfad), bis er gespielt wird.
    queued: HashMap<String, String>,
    /// Zuletzt als gespielt eingetragen: Position und Pfad.
    last_played: Option<(usize, PathBuf)>,
    planned_at: Option<Instant>,
    /// Zeit-Events (Paket 29): der AutoDJ plant rückwärts auf das nächste harte.
    events: woa_radio::events::EventTable,
    /// Letzte Auswahlen, neueste zuletzt.
    pub log: Vec<String>,
}

impl ClockEditor {
    pub fn new(dir: PathBuf) -> Self {
        Self {
            open: false,
            dir,
            schedule: Schedule::default(),
            loaded: false,
            error: None,
            selected: 0,
            dirty_since: None,
            lib: None,
            tags: Vec::new(),
            state: AutoDjState::new(0),
            queued: HashMap::new(),
            last_played: None,
            planned_at: None,
            events: woa_radio::events::EventTable::default(),
            log: Vec::new(),
        }
    }

    fn file(&self) -> PathBuf {
        self.dir.join(rotation::FILE)
    }

    /// Anderer Datenordner: alles neu laden.
    pub fn set_dir(&mut self, dir: PathBuf) {
        if dir != self.dir {
            self.flush();
            self.dir = dir;
            self.loaded = false;
            self.lib = None;
            self.error = None;
        }
    }

    /// Lädt die Uhren beim ersten Gebrauch. Fehler stehen im Fenster; die Datei wird dann
    /// nicht überschrieben (`loaded` bleibt aus).
    pub fn ensure_loaded(&mut self) {
        if self.loaded || self.error.is_some() {
            return;
        }
        let path = self.file();
        let fresh = !path.exists();
        match Schedule::load(&path) {
            Ok(mut s) => {
                if fresh {
                    // Neue Installation: eigener Startwert, sonst beginnt jede mit derselben Folge.
                    s.seed = now_unix() as u64;
                }
                self.state = AutoDjState::new(s.seed);
                self.schedule = s;
                self.loaded = true;
                self.selected = 0;
            }
            Err(e) => {
                tracing::error!(error = %e, "schedule not loaded");
                self.error = Some(format!("Stundenuhr: {e}"));
            }
        }
    }

    fn library(&mut self) -> Option<&Library> {
        if self.lib.is_none() {
            let db = self.dir.join(crate::library::DB_FILE);
            if let Err(e) = std::fs::create_dir_all(&self.dir) {
                tracing::error!(error = %e, "schedule folder not created");
                return None;
            }
            match Library::open(&db) {
                Ok(l) => self.lib = Some(l),
                Err(e) => {
                    tracing::error!(error = %e, "library not opened for autodj");
                    return None;
                }
            }
        }
        self.lib.as_ref()
    }

    fn refresh_tags(&mut self) {
        self.tags = self
            .library()
            .and_then(|l| l.all_tags().ok())
            .map(|t| t.into_iter().map(|(n, _)| n).collect())
            .unwrap_or_default();
    }

    fn touch(&mut self) {
        if self.dirty_since.is_none() {
            self.dirty_since = Some(Instant::now());
        }
    }

    /// Speichert sofort, wenn etwas geändert wurde.
    pub fn flush(&mut self) {
        if self.dirty_since.is_none() || !self.loaded {
            return;
        }
        if let Err(e) = self.schedule.save(&self.file()) {
            tracing::error!(error = %e, "schedule not saved");
            self.error = Some(format!("Stundenuhr nicht gespeichert: {e}"));
        }
        self.dirty_since = None;
    }

    // ---- Bearbeiten (alles über diese Funktionen, damit es testbar bleibt) --------------

    pub fn selected(&self) -> usize {
        self.selected
    }

    pub fn select(&mut self, i: usize) {
        if i < self.schedule.clocks.len() {
            self.selected = i;
        }
    }

    pub fn add_clock(&mut self) -> bool {
        if self.schedule.clocks.len() >= MAX_CLOCKS {
            return false;
        }
        let mut n = self.schedule.clocks.len() + 1;
        let name = loop {
            let c = format!("Uhr {n}");
            if !self.schedule.clocks.iter().any(|k| k.name == c) {
                break c;
            }
            n += 1;
        };
        self.schedule.clocks.push(HourClock {
            name,
            slots: vec![Slot::new("hit")],
        });
        self.selected = self.schedule.clocks.len() - 1;
        self.touch();
        true
    }

    /// Entfernt eine Uhr; ihre Stunden in der Woche werden leer.
    pub fn remove_clock(&mut self, i: usize) {
        if i >= self.schedule.clocks.len() {
            return;
        }
        let c = self.schedule.clocks.remove(i);
        for w in &mut self.schedule.week {
            if *w == c.name {
                w.clear();
            }
        }
        self.selected = self
            .selected
            .min(self.schedule.clocks.len().saturating_sub(1));
        self.touch();
    }

    /// Benennt eine Uhr um (Name nicht leer, nicht doppelt); die Woche folgt.
    pub fn rename_clock(&mut self, i: usize, name: &str) -> bool {
        let name = name.trim();
        if name.is_empty()
            || name.chars().count() > rotation::MAX_NAME_CHARS
            || name.chars().any(char::is_control)
            || self
                .schedule
                .clocks
                .iter()
                .enumerate()
                .any(|(k, c)| k != i && c.name == name)
        {
            return false;
        }
        let Some(c) = self.schedule.clocks.get_mut(i) else {
            return false;
        };
        let old = std::mem::replace(&mut c.name, name.to_string());
        for w in &mut self.schedule.week {
            if *w == old {
                *w = name.to_string();
            }
        }
        self.touch();
        true
    }

    pub fn add_slot(&mut self, clock: usize) -> bool {
        match self.schedule.clocks.get_mut(clock) {
            Some(c) if c.slots.len() < MAX_SLOTS => {
                c.slots.push(Slot::new("hit"));
                self.touch();
                true
            }
            _ => false,
        }
    }

    pub fn remove_slot(&mut self, clock: usize, slot: usize) {
        if let Some(c) = self.schedule.clocks.get_mut(clock)
            && slot < c.slots.len()
        {
            c.slots.remove(slot);
            self.touch();
        }
    }

    pub fn move_slot(&mut self, clock: usize, from: usize, to: usize) {
        if let Some(c) = self.schedule.clocks.get_mut(clock)
            && from < c.slots.len()
            && to < c.slots.len()
            && from != to
        {
            let s = c.slots.remove(from);
            c.slots.insert(to, s);
            self.touch();
        }
    }

    /// Setzt Kategorie und Ersatzkategorie eines Slots (Tag-Form wird geprüft).
    pub fn set_slot(&mut self, clock: usize, slot: usize, category: &str, fallback: &str) -> bool {
        let Ok(cat) = woa_library::normalize_tag(category) else {
            return false;
        };
        let fb = if fallback.trim().is_empty() {
            String::new()
        } else {
            match woa_library::normalize_tag(fallback) {
                Ok(f) => f,
                Err(_) => return false,
            }
        };
        match self
            .schedule
            .clocks
            .get_mut(clock)
            .and_then(|c| c.slots.get_mut(slot))
        {
            Some(s) => {
                s.category = cat;
                s.fallback = fb;
                self.touch();
                true
            }
            None => false,
        }
    }

    /// Ordnet der Stunde die gewählte Uhr zu (`None` leert sie).
    pub fn paint(&mut self, weekday: u8, hour: u8, clear: bool) {
        let name = self
            .schedule
            .clocks
            .get(self.selected)
            .map(|c| c.name.clone());
        let r = if clear {
            self.schedule.set_hour(weekday, hour, None)
        } else {
            self.schedule.set_hour(weekday, hour, name.as_deref())
        };
        match r {
            Ok(()) => self.touch(),
            Err(e) => tracing::warn!(error = %e, "hour not assigned"),
        }
    }

    /// Ordnet der gewählten Uhr alle Stunden der Woche zu.
    pub fn paint_all(&mut self) {
        for d in 0..7 {
            for h in 0..24 {
                self.paint(d, h, false);
            }
        }
    }

    pub fn set_autodj(&mut self, on: bool) {
        if self.schedule.autodj != on {
            self.schedule.autodj = on;
            self.planned_at = None;
            tracing::info!(on, "autodj switched");
            self.touch();
        }
    }

    /// Regeln ändern (Grenzen werden eingehalten).
    pub fn set_rules(
        &mut self,
        lookahead: usize,
        track_min: u32,
        artist_min: u32,
        offset_min: i32,
    ) {
        self.schedule.lookahead = lookahead;
        self.schedule.track_lockout_min = track_min;
        self.schedule.artist_lockout_min = artist_min;
        self.schedule.utc_offset_min = offset_min;
        if let Err(e) = self.schedule.sanitize() {
            tracing::warn!(error = %e, "rules rejected");
        }
        self.touch();
    }

    // ---- AutoDJ -------------------------------------------------------------------------

    /// Die Zeit-Events, auf deren hartes nächstes der AutoDJ rückwärts plant.
    pub fn set_events(&mut self, table: woa_radio::events::EventTable) {
        self.events = table;
    }

    fn note(&mut self, line: String) {
        self.log.push(line);
        if self.log.len() > LOG_LINES {
            self.log.remove(0);
        }
    }

    /// War `path` eine Auswahl aus der Kategorie `moderation`? Dann gehört er als Voice-Track
    /// zwischen zwei Titel (Paket 30). Der Eintrag wird dabei vergessen: eine Moderation wird
    /// nie als Titel gespielt und käme sonst nie aus der Warteliste.
    pub fn take_voice_pick(&mut self, path: &std::path::Path) -> bool {
        let key = path.to_string_lossy().into_owned();
        match self.queued.get(&key) {
            Some(c) if c.trim().eq_ignore_ascii_case(VOICE_CATEGORY) => {
                self.queued.remove(&key);
                true
            }
            _ => false,
        }
    }

    /// Ein Takt des AutoDJ für den Schnappschuss `v` der Sendung zur Zeit `now`: trägt neu
    /// gestartete Titel in die Historie ein und gibt die Pfade zurück, die hinten an die
    /// Playlist gehängt werden sollen. Gespeichert wird hier nichts an der Playlist.
    pub fn autodj_tick(&mut self, v: &View, now: i64) -> Vec<PathBuf> {
        self.ensure_loaded();
        if !self.loaded {
            return Vec::new();
        }
        self.record_played(v, now);
        if !self.schedule.autodj {
            return Vec::new();
        }
        let first_ahead = v.current.map_or(v.cue, |c| c + 1);
        // Voice-Tracks sind keine Titel: sie zählen nicht zum Vorlauf.
        let ahead_entries: Vec<_> = v
            .playlist
            .iter()
            .skip(first_ahead)
            .filter(|e| e.voice.is_none())
            .collect();
        let need = self.schedule.lookahead.saturating_sub(ahead_entries.len());
        if need == 0 {
            return Vec::new();
        }
        // Nicht öfter als nötig planen (jeder Lauf liest die Historie).
        if self.planned_at.is_some_and(|t| t.elapsed() < PLAN_EVERY) {
            return Vec::new();
        }
        self.planned_at = Some(Instant::now());
        let start = now
            + if v.running {
                v.remaining_title.ceil() as i64
            } else {
                0
            };
        let schedule = self.schedule.clone();
        let mut state = self.state.clone();
        let deadline = self.events.next_hard(start).map(|(at, _)| Deadline {
            at,
            overlap_s: v.crossfade_seconds.round() as i64,
        });
        let Some(lib) = self.library() else {
            self.error = Some("AutoDJ: Bibliothek nicht zu öffnen".into());
            return Vec::new();
        };
        let ahead: Vec<Ahead> = ahead_entries
            .iter()
            .map(|e| {
                let track = lib.by_path(&e.path.to_string_lossy()).ok().flatten();
                Ahead {
                    track_id: track.as_ref().map(|t| t.id),
                    artist: track
                        .as_ref()
                        .map(|t| rotation::artist_of(&t.title))
                        .unwrap_or_default(),
                    seconds: e.seconds.unwrap_or(200.0),
                }
            })
            .collect();
        let picks =
            match rotation::plan_until(lib, &schedule, &mut state, start, &ahead, need, deadline) {
                Ok(p) => p,
                Err(e) => {
                    tracing::error!(error = %e, "autodj plan failed");
                    self.error = Some(format!("AutoDJ: {e}"));
                    return Vec::new();
                }
            };
        self.state = state;
        if picks.is_empty() {
            self.note("Nichts zu spielen: Kategorie, Ersatz und Notfall sind leer".into());
        }
        let mut out = Vec::new();
        for p in picks {
            self.note(p.describe());
            self.queued.insert(p.path.clone(), p.category.clone());
            out.push(PathBuf::from(p.path));
        }
        out
    }

    /// Hat ein neuer Titel begonnen, kommt er in die Spielhistorie.
    fn record_played(&mut self, v: &View, now: i64) {
        let Some(i) = v.current.filter(|_| v.running) else {
            return;
        };
        let Some(e) = v.playlist.get(i) else { return };
        if self
            .last_played
            .as_ref()
            .is_some_and(|(k, p)| *k == i && *p == e.path)
        {
            return;
        }
        self.last_played = Some((i, e.path.clone()));
        let key = e.path.to_string_lossy().into_owned();
        let category = self.queued.remove(&key).unwrap_or_default();
        let Some(lib) = self.library() else { return };
        match lib.by_path(&key) {
            Ok(Some(t)) => {
                let artist = rotation::artist_of(&t.title);
                if let Err(err) = lib.record_play(t.id, &artist, &category, now) {
                    tracing::error!(error = %err, "play not recorded");
                }
            }
            Ok(None) => tracing::debug!("played title is not in the library, no history"),
            Err(err) => tracing::error!(error = %err, "play lookup failed"),
        }
    }

    // ---- Fenster ------------------------------------------------------------------------

    pub fn toggle(&mut self) {
        self.open = !self.open;
        if self.open {
            self.ensure_loaded();
            self.refresh_tags();
        }
    }

    /// Speichert nach kurzer Ruhe.
    pub fn tick_save(&mut self) {
        if self.dirty_since.is_some_and(|t| t.elapsed() >= SAVE_AFTER) {
            self.flush();
        }
    }

    /// Zeichnet das Fenster.
    pub fn show(&mut self, ctx: &egui::Context) {
        if !self.open {
            return;
        }
        let mut open = true;
        egui::Window::new("Stundenuhr und AutoDJ")
            .open(&mut open)
            .collapsible(false)
            .default_width(620.0)
            .frame(theme::card_frame())
            .show(ctx, |ui| self.body(ui));
        if !open {
            self.open = false;
            self.flush();
        }
    }

    fn body(&mut self, ui: &mut egui::Ui) {
        if let Some(e) = self.error.clone() {
            ui.colored_label(theme::on_air(), &e);
            if ui.button("Neu laden").clicked() {
                self.error = None;
                self.loaded = false;
                self.ensure_loaded();
            }
            return;
        }
        self.ensure_loaded();
        self.top_row(ui);
        ui.separator();
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(250.0);
                self.clock_list(ui);
            });
            ui.separator();
            ui.vertical(|ui| self.week_grid(ui));
        });
        ui.separator();
        ui.label(
            egui::RichText::new("LETZTE AUSWAHLEN")
                .small()
                .color(theme::text_dim()),
        );
        if self.log.is_empty() {
            ui.label(egui::RichText::new("Noch nichts gewählt").color(theme::text_dim()));
        }
        for l in self.log.iter().rev() {
            ui.label(egui::RichText::new(l).small());
        }
    }

    fn top_row(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            let mut on = self.schedule.autodj;
            let label = if on { "AutoDJ an" } else { "AutoDJ aus" };
            if ui
                .add(egui::Button::selectable(on, label))
                .on_hover_text(
                    "Hält die Playlist des Radio-Modus nach der Stundenuhr gefüllt. \
                     Von Hand gesetzte Titel bleiben stehen.",
                )
                .clicked()
            {
                on = !on;
                self.set_autodj(on);
            }
            let s = &self.schedule;
            let (mut ahead, mut tl, mut al, mut off) = (
                s.lookahead,
                s.track_lockout_min,
                s.artist_lockout_min,
                s.utc_offset_min,
            );
            let before = (ahead, tl, al, off);
            ui.label("im Voraus");
            ui.add(egui::DragValue::new(&mut ahead).range(1..=rotation::MAX_LOOKAHEAD));
            ui.label("Titel gesperrt für");
            ui.add(
                egui::DragValue::new(&mut tl)
                    .range(0..=rotation::MAX_LOCKOUT_MIN)
                    .suffix(" min"),
            )
            .on_hover_text("Derselbe Titel kommt frühestens nach dieser Zeit wieder");
            ui.label("Interpret");
            ui.add(
                egui::DragValue::new(&mut al)
                    .range(0..=rotation::MAX_LOCKOUT_MIN)
                    .suffix(" min"),
            )
            .on_hover_text("Derselbe Interpret („Interpret - Titel“) frühestens nach dieser Zeit");
            ui.label("UTC+");
            ui.add(
                egui::DragValue::new(&mut off)
                    .range(-840..=840)
                    .suffix(" min"),
            )
            .on_hover_text("Ortszeit gegenüber UTC im Winter (Mitteleuropa 60)");
            let mut dst = self.schedule.auto_dst;
            if ui
                .checkbox(&mut dst, "Sommerzeit")
                .on_hover_text(
                    "EU-Sommerzeit automatisch (letzter Sonntag im März bis Oktober). \
                     Aus: der Versatz gilt das ganze Jahr.",
                )
                .changed()
            {
                self.schedule.auto_dst = dst;
                self.touch();
            }
            if (ahead, tl, al, off) != before {
                self.set_rules(ahead, tl, al, off);
            }
        });
        ui.horizontal(|ui| {
            ui.label("Notfall-Tag");
            let mut em = self.schedule.emergency.clone();
            if ui
                .add(egui::TextEdit::singleline(&mut em).desired_width(100.0))
                .on_hover_text("Kommt, wenn Slot und Ersatz nichts liefern")
                .changed()
            {
                self.schedule.emergency = em.to_lowercase();
                self.touch();
            }
        });
    }

    fn clock_list(&mut self, ui: &mut egui::Ui) {
        ui.label(
            egui::RichText::new("UHREN")
                .small()
                .color(theme::text_dim()),
        );
        let mut pick = None;
        for (i, c) in self.schedule.clocks.iter().enumerate() {
            let col = theme::track_color(i);
            ui.horizontal(|ui| {
                ui.colored_label(col, "■");
                if ui
                    .add(egui::Button::selectable(i == self.selected, &c.name))
                    .clicked()
                {
                    pick = Some(i);
                }
            });
        }
        if let Some(i) = pick {
            self.select(i);
        }
        ui.horizontal(|ui| {
            if ui.button("+ Uhr").clicked() {
                self.add_clock();
            }
            if ui.button("Uhr löschen").clicked() {
                self.remove_clock(self.selected);
            }
        });
        let i = self.selected;
        if i >= self.schedule.clocks.len() {
            ui.label(egui::RichText::new("Noch keine Uhr").color(theme::text_dim()));
            return;
        }
        ui.add_space(6.0);
        let mut name = self.schedule.clocks[i].name.clone();
        if ui.text_edit_singleline(&mut name).changed() {
            self.rename_clock(i, &name);
        }
        ui.label(
            egui::RichText::new("SLOTS (Kategorie, Ersatz)")
                .small()
                .color(theme::text_dim()),
        );
        let slots = self.schedule.clocks[i].slots.clone();
        let tags = self.tags.clone();
        let mut act: Option<(usize, i8)> = None;
        for (k, s) in slots.iter().enumerate() {
            let (mut cat, mut fb) = (s.category.clone(), s.fallback.clone());
            let mut changed = false;
            ui.horizontal(|ui| {
                ui.label(format!("{}", k + 1));
                changed |= tag_field(ui, &mut cat, &tags, 70.0);
                changed |= tag_field(ui, &mut fb, &tags, 70.0);
                if ui.small_button("↑").clicked() {
                    act = Some((k, -1));
                }
                if ui.small_button("↓").clicked() {
                    act = Some((k, 1));
                }
                if ui.small_button("✕").clicked() {
                    act = Some((k, 0));
                }
            });
            if changed {
                self.set_slot(i, k, &cat, &fb);
            }
        }
        match act {
            Some((k, 0)) => self.remove_slot(i, k),
            Some((k, d)) => self.move_slot(i, k, (k as i64 + i64::from(d)).max(0) as usize),
            None => {}
        }
        if ui.button("+ Slot").clicked() {
            self.add_slot(i);
        }
    }

    fn week_grid(&mut self, ui: &mut egui::Ui) {
        ui.label(
            egui::RichText::new("WOCHE: Klick ordnet die gewählte Uhr zu, Rechtsklick leert")
                .small()
                .color(theme::text_dim()),
        );
        let cell = egui::vec2(15.0, 17.0);
        let now = rotation::local_hour(now_unix(), self.schedule.zone());
        let mut changes: Vec<(u8, u8, bool)> = Vec::new();
        for (d, day) in DAYS.iter().enumerate() {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(1.0, 1.0);
                ui.label(egui::RichText::new(*day).small().color(theme::text_dim()));
                for h in 0..24 {
                    let name = &self.schedule.week[d * 24 + h];
                    let idx = self.schedule.clocks.iter().position(|c| &c.name == name);
                    let fill = idx.map_or(theme::well(), theme::track_color);
                    let (rect, resp) = ui.allocate_exact_size(cell, egui::Sense::click());
                    ui.painter().rect_filled(rect, 2.0, fill);
                    let here = now.weekday as usize == d && now.hour as usize == h;
                    ui.painter().rect_stroke(
                        rect,
                        2.0,
                        egui::Stroke::new(
                            if here { 2.0 } else { 1.0 },
                            if here { theme::text() } else { theme::border() },
                        ),
                        egui::StrokeKind::Inside,
                    );
                    resp.clone().on_hover_text(format!(
                        "{} {:02}:00 – {}",
                        day,
                        h,
                        if name.is_empty() { "keine Uhr" } else { name }
                    ));
                    if resp.clicked() {
                        changes.push((d as u8, h as u8, false));
                    }
                    if resp.secondary_clicked() {
                        changes.push((d as u8, h as u8, true));
                    }
                }
            });
        }
        for (d, h, clear) in changes {
            self.paint(d, h, clear);
        }
        if ui.button("Gewählte Uhr für die ganze Woche").clicked() {
            self.paint_all();
        }
        debug_assert_eq!(self.schedule.week.len(), WEEK_HOURS);
    }
}

/// Textfeld für einen Tag mit Auswahl der bekannten Tags der Bibliothek. Gibt zurück, ob sich
/// der Wert geändert hat.
fn tag_field(ui: &mut egui::Ui, value: &mut String, tags: &[String], width: f32) -> bool {
    let mut changed = ui
        .add(egui::TextEdit::singleline(value).desired_width(width))
        .changed();
    if !tags.is_empty() {
        ui.menu_button("▾", |ui| {
            for t in tags {
                if ui.button(t).clicked() {
                    *value = t.clone();
                    changed = true;
                    ui.close();
                }
            }
        });
    }
    changed
}

/// Pfad der Datei im Datenordner (für Anzeigen und Tests).
pub fn schedule_path(dir: &Path) -> PathBuf {
    dir.join(rotation::FILE)
}

#[cfg(test)]
mod tests {
    use super::*;
    use woa_library::{AddOutcome, Kind, NewTrack};
    use woa_radio::station::ViewEntry;

    fn editor() -> (ClockEditor, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let mut e = ClockEditor::new(dir.path().to_path_buf());
        e.ensure_loaded();
        (e, dir)
    }

    fn entry(path: &str, secs: f64) -> ViewEntry {
        ViewEntry {
            title: path.into(),
            path: PathBuf::from(path),
            seconds: Some(secs),
            lufs: None,
            gain_db: 0.0,
            cues: woa_radio::Cues::default(),
            voice: None,
        }
    }

    fn seed_library(dir: &Path) {
        let lib = Library::open(&dir.join(crate::library::DB_FILE)).unwrap();
        for (i, tag) in ["hit", "hit", "hit", "hit", "neu", "neu", "neu", "jingle"]
            .iter()
            .enumerate()
        {
            let r = lib
                .add_track(&NewTrack {
                    path: format!("/m/{i}.wav"),
                    hash: format!("h{i}"),
                    title: format!("A{i} - T{i}"),
                    duration: 180.0,
                    kind: Kind::Music,
                })
                .unwrap();
            let AddOutcome::Added(id) = r else { panic!() };
            lib.add_tag(id, tag).unwrap();
        }
    }

    #[test]
    fn editing_state_roundtrip() {
        let (mut e, dir) = editor();
        assert_eq!(e.schedule.clocks.len(), 1);
        assert!(e.add_clock());
        assert_eq!(e.selected(), 1);
        assert!(e.rename_clock(1, "Nacht"));
        assert!(!e.rename_clock(1, "Standard"), "doppelter Name");
        assert!(!e.rename_clock(1, "  "), "leerer Name");
        assert!(e.add_slot(1));
        assert!(e.set_slot(1, 1, "Jingle", "hit"));
        assert!(!e.set_slot(1, 1, "", ""));
        e.move_slot(1, 1, 0);
        assert_eq!(e.schedule.clocks[1].slots[0].category, "jingle");
        e.paint(0, 5, false);
        assert_eq!(e.schedule.week[5], "Nacht");
        assert!(e.rename_clock(1, "Spät"));
        assert_eq!(e.schedule.week[5], "Spät", "Umbenennen nimmt die Woche mit");
        e.paint(0, 5, true);
        assert_eq!(e.schedule.week[5], "");
        e.paint(6, 23, false);
        e.set_rules(99, 5, 0, 900);
        assert_eq!(e.schedule.lookahead, rotation::MAX_LOOKAHEAD);
        assert_eq!(e.schedule.utc_offset_min, 840);
        e.set_autodj(true);
        e.remove_slot(1, 0);
        e.flush();
        let back = Schedule::load(&schedule_path(dir.path())).unwrap();
        assert_eq!(back, e.schedule);
        assert!(back.autodj);
        e.remove_clock(1);
        assert_eq!(
            e.schedule.week[6 * 24 + 23],
            "",
            "Stunden einer gelöschten Uhr werden leer"
        );
        assert!(!e.add_slot(9) && e.schedule.clocks.len() == 1);
    }

    #[test]
    fn broken_file_is_reported_and_not_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let f = schedule_path(dir.path());
        std::fs::write(&f, "{kaputt").unwrap();
        let mut e = ClockEditor::new(dir.path().to_path_buf());
        e.ensure_loaded();
        assert!(e.error.is_some());
        e.touch();
        e.flush();
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "{kaputt");
    }

    #[test]
    fn autodj_fills_follows_the_clock_keeps_hand_tracks_and_records_plays() {
        let (mut e, dir) = editor();
        seed_library(dir.path());
        e.schedule.utc_offset_min = 0;
        e.schedule.auto_dst = false;
        e.schedule.lookahead = 4;
        e.schedule.track_lockout_min = 0;
        e.schedule.artist_lockout_min = 0;
        e.schedule.clocks[0].slots = vec![Slot::new("hit"), Slot::new("neu"), Slot::new("jingle")];
        for h in 0..WEEK_HOURS {
            e.schedule.week[h] = "Standard".into();
        }
        e.set_autodj(true);
        // Von Hand gesetzt, nicht in der Bibliothek: bleibt, zählt als belegte Zeit.
        let mut v = View {
            playlist: vec![entry("/fremd/hand.wav", 100.0)],
            current: None,
            cue: 0,
            running: false,
            ..View::default()
        };
        let now = 1_000_000;
        let add = e.autodj_tick(&v, now);
        assert_eq!(add.len(), 3, "lookahead 4, einer liegt schon bereit");
        let cats: Vec<String> = add
            .iter()
            .map(|p| e.queued[&p.to_string_lossy().into_owned()].clone())
            .collect();
        assert_eq!(cats, ["hit", "neu", "jingle"]);
        assert_eq!(e.log.len(), 3);
        assert!(!add.contains(&PathBuf::from("/fremd/hand.wav")));
        // Playlist wächst um die Wahl, die Handtitel bleiben vorn.
        for p in &add {
            v.playlist.push(entry(&p.to_string_lossy(), 180.0));
        }
        assert_eq!(v.playlist[0].path, PathBuf::from("/fremd/hand.wav"));
        // Bereit gefüllt: nichts mehr zu tun.
        assert!(e.autodj_tick(&v, now + 1).is_empty());
        // Der erste Titel beginnt: Historie bekommt die Zeile (nur Titel der Bibliothek).
        v.running = true;
        v.current = Some(1);
        e.autodj_tick(&v, now + 2);
        let lib = Library::open(&dir.path().join(crate::library::DB_FILE)).unwrap();
        let plays = lib.plays_since(0).unwrap();
        assert_eq!(plays.len(), 1);
        assert_eq!(plays[0].played, now + 2);
        assert_eq!(plays[0].category, cats[0].as_str());
        // Dasselbe Bild noch einmal trägt nichts doppelt ein.
        e.autodj_tick(&v, now + 3);
        assert_eq!(lib.plays_since(0).unwrap().len(), 1);
    }

    #[test]
    fn moderation_picks_become_voice_tracks_once() {
        let (mut e, _dir) = editor();
        e.queued
            .insert("/m/ansage.wav".into(), " Moderation ".into());
        e.queued.insert("/m/song.wav".into(), "hit".into());
        assert!(e.take_voice_pick(Path::new("/m/ansage.wav")));
        assert!(!e.queued.contains_key("/m/ansage.wav"), "wird vergessen");
        assert!(!e.take_voice_pick(Path::new("/m/ansage.wav")));
        assert!(!e.take_voice_pick(Path::new("/m/song.wav")));
        assert!(
            e.queued.contains_key("/m/song.wav"),
            "Titel bleibt in der Warteliste"
        );
    }

    #[test]
    fn autodj_off_does_nothing_but_history() {
        let (mut e, dir) = editor();
        seed_library(dir.path());
        let v = View::default();
        assert!(e.autodj_tick(&v, 1000).is_empty());
        assert!(e.log.is_empty());
    }

    #[test]
    fn window_draws_without_a_display() {
        let (mut e, _dir) = editor();
        e.toggle();
        let ctx = egui::Context::default();
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| e.show(ui.ctx()));
        assert!(e.open);
    }
}
