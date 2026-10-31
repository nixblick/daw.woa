//! Editor für feste Zeit-Events (Paket 29): Nachrichten, Werbung, Stundenjingle zu einer
//! Uhrzeit oder nach einem Muster, hart oder weich.
//!
//! Der Editor pflegt die Tabelle (`events.json`) im UI-Thread und reicht sie an den
//! Sende-Thread weiter ([`EventEditor::take_push`]); die Auslösung selbst macht der Controller
//! (`woa_radio::events`). Ortszeit und Sommerzeit übernimmt der Editor von der Stundenuhr, damit
//! es nur eine Einstellung gibt.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use eframe::egui;
use woa_radio::events::{
    self, EventTable, Kind, MAX_EVENTS, Pattern, TimeEvent, Timing, format_hours, format_weekdays,
    parse_hours, parse_weekdays,
};
use woa_radio::station::View;

use crate::theme;

/// Änderungen werden so lange gesammelt, dann gespeichert.
const SAVE_AFTER: Duration = Duration::from_millis(700);
/// So oft wird die Tabelle erneut an den Sende-Thread gereicht (gleiche Tabelle ändert dort
/// nichts; so überlebt sie auch einen Neustart der Station).
const PUSH_EVERY: Duration = Duration::from_secs(5);

/// Zustand des Event-Editors.
pub struct EventEditor {
    pub open: bool,
    dir: PathBuf,
    table: EventTable,
    loaded: bool,
    pub error: Option<String>,
    dirty_since: Option<Instant>,
    pushed_at: Option<Instant>,
    need_push: bool,
    /// Zähler für Änderungen an der Tabelle.
    revision: u64,
    /// Eingabetexte je Event (Wochentage, Stunden), solange sie nicht lesbar sind.
    texts: HashMap<u32, (String, String)>,
    bad: HashMap<u32, String>,
}

impl EventEditor {
    pub fn new(dir: PathBuf) -> Self {
        Self {
            open: false,
            dir,
            table: EventTable::default(),
            loaded: false,
            error: None,
            dirty_since: None,
            pushed_at: None,
            need_push: false,
            revision: 0,
            texts: HashMap::new(),
            bad: HashMap::new(),
        }
    }

    fn file(&self) -> PathBuf {
        self.dir.join(events::FILE)
    }

    pub fn set_dir(&mut self, dir: PathBuf) {
        if dir != self.dir {
            self.flush();
            self.dir = dir;
            self.loaded = false;
            self.error = None;
        }
    }

    /// Lädt die Tabelle beim ersten Gebrauch. Ein Fehler steht im Fenster; die Datei wird dann
    /// nicht überschrieben (`loaded` bleibt aus).
    pub fn ensure_loaded(&mut self) {
        if self.loaded || self.error.is_some() {
            return;
        }
        match EventTable::load(&self.file()) {
            Ok(t) => {
                self.table = t;
                self.loaded = true;
                self.need_push = true;
                self.revision += 1;
                self.texts.clear();
                self.bad.clear();
            }
            Err(e) => {
                tracing::error!(error = %e, "events not loaded");
                self.error = Some(format!("Zeit-Events: {e}"));
            }
        }
    }

    pub fn table(&self) -> &EventTable {
        &self.table
    }

    pub fn is_loaded(&self) -> bool {
        self.loaded
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Übernimmt Ortszeit und Sommerzeit von der Stundenuhr.
    pub fn set_zone(&mut self, utc_offset_min: i32, auto_dst: bool) {
        if self.loaded
            && (self.table.utc_offset_min, self.table.auto_dst) != (utc_offset_min, auto_dst)
        {
            self.table.utc_offset_min = utc_offset_min;
            self.table.auto_dst = auto_dst;
            self.changed();
        }
    }

    fn changed(&mut self) {
        self.revision += 1;
        self.need_push = true;
        if self.dirty_since.is_none() {
            self.dirty_since = Some(Instant::now());
        }
    }

    /// Die Tabelle für den Sende-Thread, wenn sie geändert wurde oder Zeit ist.
    pub fn take_push(&mut self) -> Option<EventTable> {
        if !self.loaded {
            return None;
        }
        let due = self.need_push || self.pushed_at.is_none_or(|t| t.elapsed() >= PUSH_EVERY);
        if !due {
            return None;
        }
        self.need_push = false;
        self.pushed_at = Some(Instant::now());
        Some(self.table.clone())
    }

    /// Speichert sofort, wenn etwas geändert wurde.
    pub fn flush(&mut self) {
        if self.dirty_since.is_none() || !self.loaded {
            return;
        }
        if let Err(e) = self.table.save(&self.file()) {
            tracing::error!(error = %e, "events not saved");
            self.error = Some(format!("Zeit-Events nicht gespeichert: {e}"));
        }
        self.dirty_since = None;
    }

    pub fn tick_save(&mut self) {
        if self.dirty_since.is_some_and(|t| t.elapsed() >= SAVE_AFTER) {
            self.flush();
        }
    }

    // ---- Bearbeiten ----------------------------------------------------------------------

    /// Neues Event: Stundenjingle zur vollen Stunde, hart, ohne Datei (noch nicht lauffähig).
    pub fn add_event(&mut self, path: &Path) -> bool {
        if self.table.events.len() >= MAX_EVENTS {
            self.error = Some(format!("Höchstens {MAX_EVENTS} Events"));
            return false;
        }
        let id = self.table.events.iter().map(|e| e.id).max().unwrap_or(0) + 1;
        let name = path
            .file_stem()
            .map_or_else(|| "Event".to_string(), |s| s.to_string_lossy().into_owned());
        self.table.events.push(TimeEvent {
            id,
            name: name.chars().take(events::MAX_NAME_CHARS).collect(),
            kind: Kind::Jingle,
            path: path.to_path_buf(),
            pattern: Pattern::hourly(0),
            timing: Timing::Hard,
            enabled: true,
        });
        self.changed();
        true
    }

    pub fn remove_event(&mut self, id: u32) {
        self.table.events.retain(|e| e.id != id);
        self.texts.remove(&id);
        self.bad.remove(&id);
        self.changed();
    }

    fn with_event(&mut self, id: u32, f: impl FnOnce(&mut TimeEvent)) {
        if let Some(e) = self.table.events.iter_mut().find(|e| e.id == id) {
            f(e);
            self.changed();
        }
    }

    pub fn set_timing(&mut self, id: u32, timing: Timing) {
        self.with_event(id, |e| e.timing = timing);
    }

    pub fn set_enabled(&mut self, id: u32, on: bool) {
        self.with_event(id, |e| e.enabled = on);
    }

    pub fn set_time(&mut self, id: u32, minute: u8, second: u8) {
        self.with_event(id, |e| {
            e.pattern.minute = minute.min(59);
            e.pattern.second = second.min(59);
        });
    }

    /// Wochentage und Stunden aus Text (`Mo-Fr`, `7,8,12-14`, `*`). Bei einem Fehler bleibt das
    /// Event unverändert und der Fehler steht in der Zeile.
    pub fn set_pattern_text(&mut self, id: u32, days: &str, hours: &str) -> bool {
        let parsed = parse_weekdays(days).and_then(|d| parse_hours(hours).map(|h| (d, h)));
        match parsed {
            Ok((d, h)) => {
                self.bad.remove(&id);
                self.texts.remove(&id);
                self.with_event(id, |e| {
                    e.pattern.weekdays = d;
                    e.pattern.hours = h;
                });
                true
            }
            Err(msg) => {
                self.bad.insert(id, msg);
                self.texts.insert(id, (days.to_string(), hours.to_string()));
                false
            }
        }
    }

    // ---- Fenster -------------------------------------------------------------------------

    pub fn toggle(&mut self) {
        self.open = !self.open;
        if self.open {
            self.ensure_loaded();
        }
    }

    pub fn show(&mut self, ctx: &egui::Context, view: Option<&View>) {
        if !self.open {
            return;
        }
        let mut open = true;
        egui::Window::new("Zeit-Events")
            .open(&mut open)
            .collapsible(false)
            .default_width(760.0)
            .frame(theme::card_frame())
            .show(ctx, |ui| self.body(ui, view));
        if !open {
            self.open = false;
            self.flush();
        }
    }

    fn body(&mut self, ui: &mut egui::Ui, view: Option<&View>) {
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
        ui.label(
            egui::RichText::new(
                "Hart: das Event beginnt zur genauen Zeit (der laufende Titel blendet darunter aus). \
                 Weich: es kommt direkt nach dem laufenden Titel. Stunden: * oder z. B. 7,8,12-14; \
                 Wochentage: * oder z. B. Mo-Fr.",
            )
            .small()
            .color(theme::text_dim()),
        );
        ui.horizontal(|ui| {
            if ui
                .button("+ Event …")
                .on_hover_text("Audiodatei wählen: Jingle, Nachrichten, Werbung")
                .clicked()
                && let Some(p) = rfd::FileDialog::new()
                    .add_filter("Audio", &["wav", "mp3", "flac", "ogg"])
                    .pick_file()
            {
                self.add_event(&p);
            }
            if let Some(v) = view {
                if let Some((name, secs, hard)) = &v.next_event {
                    ui.label(
                        egui::RichText::new(format!(
                            "Nächstes: {name} in {} ({})",
                            crate::radio::clock(*secs),
                            if *hard { "hart" } else { "weich" }
                        ))
                        .color(theme::accent()),
                    );
                }
                if let Some((name, dev)) = &v.last_event {
                    ui.label(
                        egui::RichText::new(format!("Zuletzt: {name} {dev:+.1} s"))
                            .color(theme::text_dim()),
                    );
                }
            }
        });
        ui.separator();
        if self.table.events.is_empty() {
            ui.label(egui::RichText::new("Noch keine Events").color(theme::text_dim()));
        }
        let ids: Vec<u32> = self.table.events.iter().map(|e| e.id).collect();
        let mut remove = None;
        for id in ids {
            if self.row(ui, id) {
                remove = Some(id);
            }
            if let Some(msg) = self.bad.get(&id) {
                ui.colored_label(theme::on_air(), msg);
            }
        }
        if let Some(id) = remove {
            self.remove_event(id);
        }
    }

    /// Eine Zeile. `true`: löschen.
    fn row(&mut self, ui: &mut egui::Ui, id: u32) -> bool {
        let Some(e) = self.table.events.iter().find(|e| e.id == id).cloned() else {
            return false;
        };
        let (mut days, mut hours) = self.texts.get(&id).cloned().unwrap_or_else(|| {
            (
                format_weekdays(e.pattern.weekdays),
                format_hours(e.pattern.hours),
            )
        });
        let (mut on, mut name, mut kind, mut timing) =
            (e.enabled, e.name.clone(), e.kind, e.timing);
        let (mut min, mut sec) = (e.pattern.minute, e.pattern.second);
        let mut delete = false;
        let mut text_changed = false;
        ui.horizontal(|ui| {
            ui.checkbox(&mut on, "")
                .on_hover_text("Event ein- oder ausschalten");
            ui.add(egui::TextEdit::singleline(&mut name).desired_width(130.0));
            egui::ComboBox::from_id_salt(("ev-kind", id))
                .selected_text(kind.label())
                .width(100.0)
                .show_ui(ui, |ui| {
                    for k in Kind::ALL {
                        ui.selectable_value(&mut kind, k, k.label());
                    }
                });
            ui.label("Tage");
            text_changed |= ui
                .add(egui::TextEdit::singleline(&mut days).desired_width(60.0))
                .changed();
            ui.label("Stunden");
            text_changed |= ui
                .add(egui::TextEdit::singleline(&mut hours).desired_width(80.0))
                .changed();
            ui.label(":");
            ui.add(
                egui::DragValue::new(&mut min)
                    .range(0..=59)
                    .custom_formatter(|v, _| format!("{v:02}")),
            )
            .on_hover_text("Minute");
            ui.add(
                egui::DragValue::new(&mut sec)
                    .range(0..=59)
                    .custom_formatter(|v, _| format!("{v:02}")),
            )
            .on_hover_text("Sekunde");
            egui::ComboBox::from_id_salt(("ev-timing", id))
                .selected_text(timing.label())
                .width(60.0)
                .show_ui(ui, |ui| {
                    for t in [Timing::Hard, Timing::Soft] {
                        ui.selectable_value(&mut timing, t, t.label());
                    }
                });
            if ui
                .small_button(theme::ic::TRASH)
                .on_hover_text("Event entfernen")
                .clicked()
            {
                delete = true;
            }
        });
        ui.label(
            egui::RichText::new(e.path.to_string_lossy())
                .small()
                .color(theme::text_dim()),
        );
        if on != e.enabled {
            self.set_enabled(id, on);
        }
        if name != e.name && !name.trim().is_empty() {
            self.with_event(id, |x| {
                x.name = name.chars().take(events::MAX_NAME_CHARS).collect();
            });
        }
        if kind != e.kind {
            self.with_event(id, |x| x.kind = kind);
        }
        if timing != e.timing {
            self.set_timing(id, timing);
        }
        if (min, sec) != (e.pattern.minute, e.pattern.second) {
            self.set_time(id, min, sec);
        }
        if text_changed {
            self.set_pattern_text(id, &days, &hours);
        }
        delete
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn editor() -> (EventEditor, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        (EventEditor::new(dir.path().to_path_buf()), dir)
    }

    #[test]
    fn edit_save_reload_and_push() {
        let (mut ed, dir) = editor();
        ed.ensure_loaded();
        assert!(ed.is_loaded());
        // Nach dem Laden geht die (leere) Tabelle an den Sende-Thread, dann erst wieder später.
        assert!(ed.take_push().is_some());
        assert!(ed.take_push().is_none());
        assert!(ed.add_event(Path::new("/x/Stundenjingle.wav")));
        let id = ed.table().events[0].id;
        assert_eq!(ed.table().events[0].name, "Stundenjingle");
        assert!(ed.take_push().is_some());
        ed.set_timing(id, Timing::Soft);
        assert!(ed.set_pattern_text(id, "Mo-Fr", "7,8,12-14"));
        ed.set_time(id, 55, 30);
        assert!(!ed.set_pattern_text(id, "Xx", "7"));
        // Der Fehler lässt das Event unverändert.
        assert_eq!(
            ed.table().events[0].pattern.hours,
            (1 << 7) | (1 << 8) | (7 << 12)
        );
        ed.flush();
        let mut again = EventEditor::new(dir.path().to_path_buf());
        again.ensure_loaded();
        assert_eq!(again.table(), ed.table());
        ed.remove_event(id);
        assert!(ed.table().events.is_empty());
    }

    #[test]
    fn broken_file_is_reported_and_not_overwritten() {
        let (mut ed, dir) = editor();
        std::fs::write(dir.path().join(events::FILE), "{ kaputt").unwrap();
        ed.ensure_loaded();
        assert!(ed.error.is_some() && !ed.is_loaded());
        ed.flush();
        assert_eq!(
            std::fs::read_to_string(dir.path().join(events::FILE)).unwrap(),
            "{ kaputt"
        );
    }

    #[test]
    fn zone_follows_the_schedule() {
        let (mut ed, _d) = editor();
        ed.ensure_loaded();
        let r = ed.revision();
        ed.set_zone(60, true);
        assert_eq!(ed.revision(), r, "same zone changes nothing");
        ed.set_zone(0, false);
        assert!(ed.revision() > r);
        assert!(!ed.table().auto_dst);
    }

    #[test]
    fn window_draws_without_a_display() {
        let (mut ed, _d) = editor();
        ed.toggle();
        ed.add_event(Path::new("/x/a.wav"));
        let ctx = crate::theme::test_context();
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            ed.show(ui.ctx(), Some(&View::default()));
        });
        assert!(ed.open);
    }
}
