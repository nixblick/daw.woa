//! Bibliotheks-Fenster (Paket 24): Suchen, Tags, Ordner-Import, Vorhören und Ziehen in
//! Timeline, Playlist und Cart-Wall. Die Logik (Suche, Import, Tags) steckt in `woa-library`;
//! dieses Fenster hält nur den Zustand der Anzeige und ist in allen Modi dasselbe.
//!
//! Läuft komplett im UI-Thread und im Import-Hintergrund-Thread von `woa-library`; mit dem
//! Audio-Thread hat es nichts zu tun (Vorhören geht über den vorhandenen PFL-Weg der App).

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use eframe::egui;
use woa_library::analysis::Job as AnalysisJob;
use woa_library::import::{self, Import, Progress};
use woa_library::{Analysis, Kind, Library, Query, SortBy, Track};

use crate::tag_editor::{Item as TagItem, TagEditor};
use crate::theme;

/// Datei der Bibliothek im Datenordner.
pub const DB_FILE: &str = "bibliothek.sqlite3";
/// Zeilenhöhe der Trefferliste.
const ROW_H: f32 = 24.0;
/// Während eines Imports wird die Liste höchstens so oft neu gesucht.
const REFRESH_EVERY: Duration = Duration::from_millis(1000);
/// So viele Treffer zeigt das Fenster.
const SHOW_LIMIT: usize = 500;

/// Was das Fenster im letzten Bild von der App will.
#[derive(Debug, Default)]
pub struct Output {
    /// Datei im Kopfhörer vorhören.
    pub preview: Option<PathBuf>,
    pub stop_preview: bool,
    /// Eine Datei wurde losgelassen: Pfad und Zeigerposition (nicht im Fenster selbst).
    pub drop: Option<(PathBuf, egui::Pos2)>,
}

/// Zustand des Fensters.
pub struct LibraryWindow {
    pub open: bool,
    db_path: PathBuf,
    lib: Option<Library>,
    error: Option<String>,
    note: Option<String>,
    query: String,
    kind_filter: Option<Kind>,
    tag_filter: Option<String>,
    import_kind: Kind,
    import: Option<Import>,
    /// Hintergrundanalyse (Paket 25) und wie viele Titel noch darauf warten.
    analysis: Option<AnalysisJob>,
    pending: usize,
    analysis_blocked: bool,
    results: Vec<Track>,
    total: usize,
    tags: Vec<(String, usize)>,
    dirty: bool,
    refreshed: Instant,
    selected: Option<i64>,
    new_tag: String,
    dragging: Option<PathBuf>,
    /// Sortierung der Liste (Klick auf einen Spaltenkopf, #124).
    sort: SortBy,
    desc: bool,
    /// Titelinfos bearbeiten (Doppelklick oder Kontextmenü); bleibt nach dem Schließen,
    /// solange gesperrte Dateien noch gespeichert werden.
    tag_editor: Option<TagEditor>,
    tag_editor_open: bool,
    /// Auswahl im Reiter „Verwalten → Titel“ (Ids) und Anker für Umschalt-Klick.
    marked: Vec<i64>,
    /// Anker für Umschalt-Klick als Track-ID: Indizes gelten nach einer neuen Suche nicht mehr.
    mark_anchor: Option<i64>,
    /// Der Reiter hat den Editor seit dem letzten Bild benutzt (dann bleibt er).
    tab_seen: bool,
}

impl LibraryWindow {
    pub fn new(db_path: PathBuf) -> Self {
        Self {
            open: false,
            db_path,
            lib: None,
            error: None,
            note: None,
            query: String::new(),
            kind_filter: None,
            tag_filter: None,
            import_kind: Kind::Music,
            import: None,
            analysis: None,
            pending: 0,
            analysis_blocked: false,
            results: Vec::new(),
            total: 0,
            tags: Vec::new(),
            dirty: true,
            refreshed: Instant::now(),
            selected: None,
            new_tag: String::new(),
            dragging: None,
            sort: SortBy::Relevance,
            desc: false,
            tag_editor: None,
            tag_editor_open: false,
            marked: Vec::new(),
            mark_anchor: None,
            tab_seen: false,
        }
    }

    /// Anderer Datenordner: Datenbank neu öffnen.
    pub fn set_db_path(&mut self, db_path: PathBuf) {
        if db_path != self.db_path {
            self.db_path = db_path;
            self.import = None;
            self.analysis = None;
            self.analysis_blocked = false;
            self.pending = 0;
            self.lib = None;
            // Ein Fehler des alten Ordners darf den neuen nicht sperren (ensure_open).
            self.error = None;
            self.note = None;
            self.results.clear();
            self.marked.clear();
            self.mark_anchor = None;
            self.selected = None;
            self.dirty = true;
        }
    }

    /// Öffnet die Datenbank beim ersten Gebrauch. Fehler stehen im Fenster, nie ein Absturz.
    fn ensure_open(&mut self) {
        if self.lib.is_some() || self.error.is_some() {
            return;
        }
        if let Some(dir) = self.db_path.parent()
            && let Err(e) = std::fs::create_dir_all(dir)
        {
            tracing::error!(error = %e, "library folder not created");
            self.error = Some(format!("Bibliothek: Ordner nicht anlegbar ({e})"));
            return;
        }
        match Library::open(&self.db_path) {
            Ok(l) => self.lib = Some(l),
            Err(e) => {
                tracing::error!(error = %e, "library not opened");
                self.error = Some(format!("Bibliothek: {e}"));
            }
        }
    }

    /// Sucht neu mit den aktuellen Filtern.
    pub fn refresh(&mut self) {
        self.ensure_open();
        let Some(lib) = &self.lib else { return };
        let q = Query {
            text: self.query.clone(),
            tag: self.tag_filter.clone(),
            kind: self.kind_filter,
            limit: SHOW_LIMIT,
            sort: self.sort,
            desc: self.desc,
        };
        match lib.search(&q) {
            Ok(r) => {
                self.results = r;
                // Auswahl auf die neuen Treffer beschränken (einmal je Suche, nicht je Frame).
                let ids: std::collections::HashSet<i64> =
                    self.results.iter().map(|t| t.id).collect();
                self.marked.retain(|id| ids.contains(id));
            }
            Err(e) => {
                tracing::warn!(error = %e, "library search failed");
                self.note = Some(format!("Suche: {e}"));
            }
        }
        self.total = lib.count().unwrap_or(0);
        self.pending = lib.pending_analysis_count().unwrap_or(0);
        self.tags = lib.all_tags().unwrap_or_default();
        self.dirty = false;
        self.refreshed = Instant::now();
    }

    pub fn set_query(&mut self, text: &str) {
        self.query = text.to_string();
        self.dirty = true;
    }

    pub fn set_tag_filter(&mut self, tag: Option<&str>) {
        self.tag_filter = tag.map(str::to_string);
        self.dirty = true;
    }

    pub fn results(&self) -> &[Track] {
        &self.results
    }

    pub fn total(&self) -> usize {
        self.total
    }

    pub fn select(&mut self, id: Option<i64>) {
        self.selected = id;
    }

    /// Startet den Import eines Ordners im Hintergrund (läuft schon einer, passiert nichts).
    pub fn start_import(&mut self, dir: &Path) {
        if self
            .import
            .as_ref()
            .is_some_and(|i| !i.progress.is_finished())
        {
            return;
        }
        self.ensure_open();
        if self.lib.is_none() {
            return;
        }
        self.note = None;
        self.import = Some(import::start(&self.db_path, dir, self.import_kind));
        self.dirty = true;
    }

    /// Ob ein Import läuft.
    pub fn importing(&self) -> bool {
        self.import
            .as_ref()
            .is_some_and(|i| !i.progress.is_finished())
    }

    /// Vergibt einen Tag am gewählten Titel.
    pub fn tag_selected(&mut self, tag: &str) {
        let Some(id) = self.selected else { return };
        let Some(lib) = &self.lib else { return };
        match lib.add_tag(id, tag) {
            Ok(_) => self.note = None,
            Err(e) => self.note = Some(e.to_string()),
        }
        self.dirty = true;
    }

    /// Öffnet die Titelinfos der Titel `ids` (läuft noch ein Speichern, bleibt der alte Editor).
    /// Titel außerhalb der aktuellen Trefferliste holt es aus der Datenbank.
    pub fn edit_tags(&mut self, ids: &[i64]) {
        if self.tag_editor.as_ref().is_some_and(TagEditor::busy) {
            self.tag_editor_open = true;
            return;
        }
        self.ensure_open();
        let items: Vec<TagItem> = ids
            .iter()
            .filter_map(|id| {
                self.results
                    .iter()
                    .find(|t| t.id == *id)
                    .map(|t| t.path.clone())
                    .or_else(|| {
                        let lib = self.lib.as_ref()?;
                        lib.get(*id).ok().flatten().map(|t| t.path)
                    })
                    .map(|path| TagItem {
                        id: *id,
                        path: PathBuf::from(path),
                    })
            })
            .collect();
        if items.is_empty() {
            return;
        }
        self.tag_editor = Some(TagEditor::open(self.db_path.clone(), items));
        self.tag_editor_open = true;
    }

    /// Titelinfos der Datei `path` (Radio-Playlist). Steht sie nicht in der Bibliothek, sagt
    /// das die Bibliothek, statt still nichts zu tun.
    pub fn edit_tags_for_path(&mut self, path: &Path) {
        self.ensure_open();
        let found = self
            .lib
            .as_ref()
            .and_then(|l| l.by_path(&path.to_string_lossy()).ok().flatten());
        match found {
            Some(t) => self.edit_tags(&[t.id]),
            None => {
                self.open = true;
                self.note = Some(format!(
                    "Titelinfos: {} steht nicht in der Bibliothek (erst importieren)",
                    path.display()
                ));
            }
        }
    }

    /// Suchtext der Trefferliste.
    pub fn query(&self) -> &str {
        &self.query
    }

    /// Gewählte Titel im Reiter „Verwalten → Titel“.
    pub fn marked(&self) -> &[i64] {
        &self.marked
    }

    /// Klick auf Treffer `i`: allein wählen, mit `toggle` hinzu/heraus, mit `range` von der
    /// letzten Wahl bis hier.
    pub fn mark_click(&mut self, i: usize, toggle: bool, range: bool) {
        let Some(id) = self.results.get(i).map(|t| t.id) else {
            return;
        };
        let anchor = self
            .mark_anchor
            .and_then(|a| self.results.iter().position(|t| t.id == a));
        match (range, anchor) {
            (true, Some(a)) => {
                let (lo, hi) = (a.min(i), a.max(i).min(self.results.len() - 1));
                for t in &self.results[lo..=hi] {
                    if !self.marked.contains(&t.id) {
                        self.marked.push(t.id);
                    }
                }
            }
            _ if toggle => {
                if let Some(k) = self.marked.iter().position(|m| *m == id) {
                    self.marked.remove(k);
                } else {
                    self.marked.push(id);
                }
                self.mark_anchor = Some(id);
            }
            _ => {
                self.marked = vec![id];
                self.mark_anchor = Some(id);
            }
        }
    }

    /// Editor für die Auswahl des Reiters (derselbe wie im Fenster „Titelinfos“). Wechselt
    /// die Auswahl, liest er neu; solange er noch speichert, bleibt der alte.
    pub fn tab_editor(&mut self) -> Option<&mut TagEditor> {
        self.tab_seen = true;
        if self.marked.is_empty() {
            return None;
        }
        let same = self
            .tag_editor
            .as_ref()
            .is_some_and(|e| e.ids() == self.marked);
        if !same && !self.tag_editor.as_ref().is_some_and(TagEditor::busy) {
            let open = self.tag_editor_open;
            let ids = self.marked.clone();
            self.edit_tags(&ids);
            self.tag_editor_open = open;
        }
        self.tag_editor.as_mut()
    }

    /// Fenster der Titelinfos zeichnen und Hintergrundarbeit abholen.
    fn tag_editor_window(&mut self, ctx: &egui::Context) {
        let Some(ed) = self.tag_editor.as_mut() else {
            return;
        };
        let mut changed = false;
        if self.tag_editor_open {
            let mut open = true;
            egui::Window::new("Titelinfos")
                .open(&mut open)
                .default_width(460.0)
                .resizable(false)
                .frame(theme::card_frame())
                .show(ctx, |ui| changed = ed.ui(ui));
            self.tag_editor_open = open;
        } else {
            ed.tick(ctx);
            changed = ed.take_changed();
        }
        if changed {
            self.dirty = true;
        }
        if !self.tag_editor_open && !ed.busy() && !std::mem::take(&mut self.tab_seen) {
            self.tag_editor = None;
        }
    }

    /// Klick auf einen Spaltenkopf: dieselbe Spalte dreht die Richtung, eine neue sortiert
    /// aufsteigend (Messwerte und Dauer zuerst absteigend: das Größte oben).
    fn sort_by(&mut self, col: SortBy) {
        if self.sort == col {
            self.desc = !self.desc;
        } else {
            self.sort = col;
            self.desc = matches!(col, SortBy::Bpm | SortBy::Lufs | SortBy::Duration);
        }
        self.dirty = true;
    }

    /// Nimmt einen Titel aus der Bibliothek; die Datei bleibt auf der Platte (#124).
    fn remove(&mut self, id: i64) {
        let Some(lib) = &self.lib else { return };
        match lib.remove_track(id) {
            Ok(()) => {
                let title = self
                    .results
                    .iter()
                    .find(|t| t.id == id)
                    .map(|t| t.title.clone());
                self.note = Some(format!(
                    "„{}“ aus der Bibliothek entfernt (Datei bleibt)",
                    title.unwrap_or_default()
                ));
            }
            Err(e) => self.note = Some(e.to_string()),
        }
        if self.selected == Some(id) {
            self.selected = None;
        }
        self.dirty = true;
    }

    fn untag(&mut self, id: i64, tag: &str) {
        let Some(lib) = &self.lib else { return };
        if let Err(e) = lib.remove_tag(id, tag) {
            self.note = Some(e.to_string());
        }
        if self.tag_filter.as_deref() == Some(tag)
            && !self.tags.iter().any(|(t, n)| t == tag && *n > 1)
        {
            self.tag_filter = None;
        }
        self.dirty = true;
    }

    /// Fortschritt des Imports abholen; gibt zurück, ob ein Bild nachgezogen werden muss.
    fn poll(&mut self) -> bool {
        let Some(imp) = &self.import else {
            return false;
        };
        let p = &imp.progress;
        let finished = p.is_finished();
        if finished {
            let fatal = p.fatal.load(std::sync::atomic::Ordering::Acquire);
            self.note = Some(if fatal {
                "Import fehlgeschlagen (Ordner nicht lesbar?)".into()
            } else {
                import_summary(p)
            });
            self.import = None;
            self.dirty = true;
            return false;
        }
        if self.refreshed.elapsed() >= REFRESH_EVERY {
            self.dirty = true;
        }
        true
    }

    /// Startet die Analyse im Hintergrund, sobald Titel darauf warten (auch während eines
    /// Imports: der liest nur die Datei-Köpfe, die Analyse misst hinterher),
    /// und holt ihren Fortschritt ab. Gibt zurück, ob sie läuft.
    fn poll_analysis(&mut self) -> bool {
        if let Some(job) = &self.analysis {
            let p = &job.progress;
            if p.is_finished() {
                if p.fatal.load(std::sync::atomic::Ordering::Acquire) {
                    self.note = Some("Analyse fehlgeschlagen (siehe Protokoll)".into());
                    self.analysis_blocked = true;
                }
                self.analysis = None;
                self.dirty = true;
                return false;
            }
            if self.refreshed.elapsed() >= REFRESH_EVERY {
                self.dirty = true;
            }
            return true;
        }
        if self.pending > 0 && !self.analysis_blocked && self.lib.is_some() {
            self.analysis = Some(woa_library::analysis::start(&self.db_path));
            return true;
        }
        false
    }

    /// Ob die Hintergrundanalyse läuft.
    pub fn analysing(&self) -> bool {
        self.analysis.is_some()
    }

    /// Import und Analyse weiterführen, auch bei geschlossenem Fenster (#123): Ordner wählen,
    /// Fenster zu, die Arbeit läuft im Hintergrund weiter. Gibt zurück, ob etwas läuft.
    pub fn tick(&mut self, ctx: &egui::Context) -> bool {
        self.ensure_open();
        let running = self.poll();
        let analysing = self.poll_analysis();
        if self.dirty && (self.open || self.refreshed.elapsed() >= REFRESH_EVERY) {
            self.refresh();
        }
        let running = running || analysing;
        if running {
            // Offen flüssig, geschlossen reicht ein Blick je Sekunde.
            ctx.request_repaint_after(if self.open {
                Duration::from_millis(150)
            } else {
                REFRESH_EVERY
            });
        }
        running
    }

    /// Zeichnet das Fenster, wenn es offen ist; führt die Hintergrundarbeit immer weiter.
    pub fn show(&mut self, ctx: &egui::Context, pfl_available: bool) -> Output {
        let mut out = Output::default();
        self.tick(ctx);
        self.tag_editor_window(ctx);
        if !self.open {
            return out;
        }
        let mut open = true;
        let resp = egui::Window::new("Bibliothek")
            .open(&mut open)
            .default_size([820.0, 540.0])
            .min_size([560.0, 320.0])
            .frame(theme::card_frame())
            .show(ctx, |ui| self.body(ui, pfl_available, &mut out));
        self.open = open;
        // Loslassen im Fenster selbst legt nichts ab.
        if let (Some((_, pos)), Some(r)) = (&out.drop, &resp)
            && r.response.rect.contains(*pos)
        {
            out.drop = None;
        }
        out
    }

    fn body(&mut self, ui: &mut egui::Ui, pfl_available: bool, out: &mut Output) {
        if let Some(e) = &self.error {
            ui.colored_label(theme::on_air(), e);
            return;
        }
        // Suche und Filter
        ui.horizontal(|ui| {
            let r = ui.add(
                egui::TextEdit::singleline(&mut self.query)
                    .hint_text("Suchen: Titel, Tag, Ordner …")
                    .desired_width(220.0),
            );
            if r.changed() {
                self.dirty = true;
            }
            let mut kind = self.kind_filter;
            egui::ComboBox::from_id_salt("lib_kind_filter")
                .selected_text(kind.map_or("Alle Arten", Kind::label))
                .width(110.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut kind, None, "Alle Arten");
                    for k in Kind::ALL {
                        ui.selectable_value(&mut kind, Some(k), k.label());
                    }
                });
            if kind != self.kind_filter {
                self.kind_filter = kind;
                self.dirty = true;
            }
            let mut tag = self.tag_filter.clone();
            egui::ComboBox::from_id_salt("lib_tag_filter")
                .selected_text(tag.clone().unwrap_or_else(|| "Alle Tags".into()))
                .width(130.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut tag, None, "Alle Tags");
                    for (t, n) in &self.tags {
                        ui.selectable_value(&mut tag, Some(t.clone()), format!("{t} ({n})"));
                    }
                });
            if tag != self.tag_filter {
                self.tag_filter = tag;
                self.dirty = true;
            }
            ui.label(
                egui::RichText::new(format!("{} von {}", self.results.len(), self.total))
                    .small()
                    .color(theme::text_dim()),
            );
        });
        // Import
        ui.horizontal(|ui| {
            let busy = self.importing();
            if ui
                .add_enabled(
                    !busy,
                    egui::Button::new(egui::RichText::new("Ordner importieren …").strong())
                        .fill(theme::mix(theme::accent(), theme::surface(), 0.72))
                        .stroke(egui::Stroke::new(1.0, theme::accent())),
                )
                .clicked()
                && let Some(dir) = crate::ordner::dialog_musik()
                    .set_title("Ordner mit Musik, Samples oder Jingles")
                    .pick_folder()
            {
                self.start_import(&dir);
            }
            ui.add_enabled_ui(!busy, |ui| {
                egui::ComboBox::from_id_salt("lib_import_kind")
                    .selected_text(format!("als {}", self.import_kind.label()))
                    .width(110.0)
                    .show_ui(ui, |ui| {
                        for k in Kind::ALL {
                            ui.selectable_value(&mut self.import_kind, k, k.label());
                        }
                    });
            });
            if let Some(imp) = &self.import {
                let p = &imp.progress;
                let total = Progress::get(&p.total);
                let done = Progress::get(&p.done);
                ui.spinner();
                ui.label(
                    egui::RichText::new(if total == 0 {
                        "sucht Dateien …".to_string()
                    } else {
                        format!("{done} von {total}")
                    })
                    .small()
                    .color(theme::accent()),
                );
                if ui.small_button("Abbrechen").clicked() {
                    imp.cancel();
                }
            } else if let Some(job) = &self.analysis {
                let p = &job.progress;
                ui.spinner();
                ui.label(
                    egui::RichText::new(format!(
                        "analysiert: {} von {}",
                        Progress::get(&p.done),
                        Progress::get(&p.total)
                    ))
                    .small()
                    .color(theme::accent()),
                )
                .on_hover_text("BPM, Tonart, Lautheit und Stille im Hintergrund");
            } else if let Some(n) = &self.note {
                ui.label(egui::RichText::new(n).small().color(theme::text()));
            }
        });
        ui.add_space(4.0);

        // Detailzeile des gewählten Titels (unten), Liste füllt den Rest.
        let selected = self
            .selected
            .and_then(|id| self.results.iter().find(|t| t.id == id))
            .cloned();
        egui::Panel::bottom("lib_detail")
            .frame(egui::Frame::new().inner_margin(egui::Margin::symmetric(0, 6)))
            .show(ui, |ui| {
                self.detail(ui, selected.as_ref(), pfl_available, out)
            });
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(theme::well())
                    .corner_radius(4)
                    .inner_margin(egui::Margin::same(4)),
            )
            .show(ui, |ui| self.list(ui, out));
    }

    fn list(&mut self, ui: &mut egui::Ui, out: &mut Output) {
        if self.results.is_empty() {
            ui.add_space(12.0);
            ui.label(
                egui::RichText::new(if self.total == 0 {
                    "Noch leer. „Ordner importieren …“ liest Musik, Samples oder Jingles ein."
                } else {
                    "Nichts gefunden."
                })
                .color(theme::text_dim()),
            );
            return;
        }
        let n = self.results.len();
        // Spaltenköpfe über der Liste; ein Klick sortiert, ein zweiter dreht die Richtung (#124).
        {
            let (rect, _) = ui
                .allocate_exact_size(egui::vec2(ui.available_width(), 16.0), egui::Sense::hover());
            let cols = Columns::at(rect.right());
            let mut sort_click = None;
            for (col, x, align, label) in [
                (
                    SortBy::Title,
                    rect.left() + 8.0,
                    egui::Align2::LEFT_CENTER,
                    "TITEL",
                ),
                (SortBy::Bpm, cols.bpm, egui::Align2::RIGHT_CENTER, "BPM"),
                (SortBy::Key, cols.key, egui::Align2::RIGHT_CENTER, "TONART"),
                (SortBy::Lufs, cols.lufs, egui::Align2::RIGHT_CENTER, "LUFS"),
                (SortBy::Kind, cols.kind, egui::Align2::RIGHT_CENTER, "ART"),
                (
                    SortBy::Duration,
                    cols.duration,
                    egui::Align2::RIGHT_CENTER,
                    "DAUER",
                ),
            ] {
                let active = self.sort == col;
                let text = if active {
                    let arrow = if self.desc {
                        theme::ic::CARET_DOWN
                    } else {
                        theme::ic::CARET_UP
                    };
                    format!("{label} {arrow}")
                } else {
                    label.to_string()
                };
                let galley = ui.painter().layout_no_wrap(
                    text,
                    egui::FontId::proportional(10.0),
                    if active {
                        theme::accent()
                    } else {
                        theme::text_dim()
                    },
                );
                let r = align.anchor_size(egui::pos2(x, rect.center().y), galley.size());
                let resp = ui
                    .interact(
                        r.expand(3.0),
                        ui.id().with(("lib_head", label)),
                        egui::Sense::click(),
                    )
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text("Sortieren (noch einmal: andere Richtung)");
                let color = if active || resp.hovered() {
                    theme::accent()
                } else {
                    theme::text_dim()
                };
                ui.painter().galley(r.min, galley, color);
                if resp.clicked() {
                    sort_click = Some(col);
                }
            }
            if let Some(col) = sort_click {
                self.sort_by(col);
            }
        }
        let mut clicked = None;
        let mut remove = None;
        let mut edit_tags = None;
        let mut started = None;
        let mut stopped = None;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show_rows(ui, ROW_H, n, |ui, range| {
                for i in range {
                    let t = &self.results[i];
                    let (rect, _) = ui.allocate_exact_size(
                        egui::vec2(ui.available_width(), ROW_H),
                        egui::Sense::hover(),
                    );
                    // Feste Kennung je Titel: die Tastatur-Markierung überlebt das Scrollen.
                    let resp = ui.interact(rect, row_id(t.id), egui::Sense::click_and_drag());
                    let sel = self.selected == Some(t.id);
                    let painter = ui.painter_at(rect);
                    if sel {
                        painter.rect_filled(
                            rect,
                            3,
                            theme::mix(theme::accent(), theme::well(), 0.78),
                        );
                    } else if resp.hovered() {
                        painter.rect_filled(rect, 3, theme::panel());
                    }
                    let mid = rect.center().y;
                    let text_col = if sel {
                        theme::text()
                    } else {
                        theme::mix(theme::text(), theme::text_dim(), 0.2)
                    };
                    let hint = crate::protokoll::damage_hint(Path::new(&t.path));
                    let mut title_x = rect.left() + 8.0;
                    if hint.is_some() {
                        let r = painter.text(
                            egui::pos2(title_x, mid),
                            egui::Align2::LEFT_CENTER,
                            theme::ic::WARNING,
                            egui::FontId::proportional(13.0),
                            theme::warn(),
                        );
                        title_x = r.right() + 4.0;
                    }
                    painter.text(
                        egui::pos2(title_x, mid),
                        egui::Align2::LEFT_CENTER,
                        &t.title,
                        egui::FontId::proportional(13.0),
                        text_col,
                    );
                    let cols = Columns::at(rect.right());
                    painter.text(
                        egui::pos2(cols.duration, mid),
                        egui::Align2::RIGHT_CENTER,
                        duration_text(t.duration),
                        egui::FontId::monospace(12.0),
                        theme::text_dim(),
                    );
                    painter.text(
                        egui::pos2(cols.kind, mid),
                        egui::Align2::RIGHT_CENTER,
                        t.kind.label(),
                        egui::FontId::proportional(11.0),
                        theme::cue(),
                    );
                    // Analyse (Paket 25): „…“ solange der Titel noch nicht dran war.
                    let a = t.analysis.as_ref();
                    for (x, text) in [
                        (cols.lufs, lufs_text(a)),
                        (cols.key, key_text(a)),
                        (cols.bpm, bpm_text(a)),
                    ] {
                        painter.text(
                            egui::pos2(x, mid),
                            egui::Align2::RIGHT_CENTER,
                            text,
                            egui::FontId::monospace(12.0),
                            if a.is_some() {
                                text_col
                            } else {
                                theme::text_dim()
                            },
                        );
                    }
                    if !t.tags.is_empty() {
                        painter.text(
                            egui::pos2(cols.tags, mid),
                            egui::Align2::RIGHT_CENTER,
                            t.tags.join(" · "),
                            egui::FontId::proportional(11.0),
                            theme::text_dim(),
                        );
                    }
                    if resp.clicked() || resp.drag_started() || resp.secondary_clicked() {
                        clicked = Some(t.id);
                        // Fokus auf die Zeile: Entf gilt dann ihr, nicht dem Studio.
                        resp.request_focus();
                    }
                    if theme::double_clicked(&resp) {
                        edit_tags = Some(t.id);
                    }
                    resp.context_menu(|ui| {
                        if ui.button("Titelinfos bearbeiten …").clicked() {
                            edit_tags = Some(t.id);
                            ui.close();
                        }
                        if ui
                            .button(format!("{} Aus der Bibliothek entfernen", theme::ic::TRASH))
                            .on_hover_text("Die Datei auf der Platte bleibt (Taste: Entf)")
                            .clicked()
                        {
                            remove = Some(t.id);
                            ui.close();
                        }
                    });
                    if resp.drag_started() {
                        started = Some(PathBuf::from(&t.path));
                    }
                    if resp.drag_stopped() {
                        stopped = Some(PathBuf::from(&t.path));
                    }
                    match hint {
                        Some(h) => resp.on_hover_text(format!("{}\n{h}", t.path)),
                        None => resp.on_hover_text(&t.path),
                    };
                }
            });
        if let Some(id) = clicked {
            self.selected = Some(id);
        }
        // Entf nur auf den markierten Eintrag, wenn die Zeile den Fokus hat (nie im Suchfeld).
        if let Some(id) = self.selected
            && ui.ctx().memory(|m| m.has_focus(row_id(id)))
            && !ui.ctx().text_edit_focused()
            && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Delete))
        {
            remove = Some(id);
        }
        if let Some(id) = remove {
            self.remove(id);
        }
        if let Some(id) = edit_tags {
            self.edit_tags(&[id]);
        }
        if let Some(p) = started {
            self.dragging = Some(p);
        }
        if let Some(p) = &self.dragging {
            let ctx = ui.ctx().clone();
            ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
            if let Some(pos) = ctx.pointer_latest_pos() {
                let name = p
                    .file_stem()
                    .map(|s| s.to_string_lossy().replace('_', " "))
                    .unwrap_or_default();
                let painter = ctx.layer_painter(egui::LayerId::new(
                    egui::Order::Tooltip,
                    egui::Id::new("lib_drag_ghost"),
                ));
                let galley = painter.layout_no_wrap(
                    format!("{} {name}", theme::ic::MUSIC_NOTE),
                    egui::FontId::proportional(13.0),
                    theme::well(),
                );
                let r = egui::Rect::from_min_size(
                    pos + egui::vec2(14.0, 10.0),
                    galley.size() + egui::vec2(14.0, 8.0),
                );
                painter.rect_filled(r, 4, theme::accent());
                painter.galley(r.min + egui::vec2(7.0, 4.0), galley, theme::well());
            }
        }
        if let Some(path) = stopped {
            self.dragging = None;
            if let Some(pos) = ui.ctx().pointer_latest_pos() {
                out.drop = Some((path, pos));
            }
        }
    }

    fn detail(
        &mut self,
        ui: &mut egui::Ui,
        sel: Option<&Track>,
        pfl_available: bool,
        out: &mut Output,
    ) {
        let Some(t) = sel else {
            ui.label(
                egui::RichText::new("Titel anklicken: Tags vergeben, vorhören. In die Timeline, Playlist oder auf ein Cart-Pad ziehen. Rechtsklick oder Entf: entfernen.")
                    .small()
                    .color(theme::text_dim()),
            );
            return;
        };
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new(&t.title).strong().color(theme::text()));
            if ui
                .add_enabled(
                    pfl_available,
                    egui::Button::new(theme::fill_label(
                        theme::icf::PLAY,
                        "Vorhören",
                        14.0,
                        theme::text(),
                    ))
                    .fill(theme::surface()),
                )
                .on_disabled_hover_text(
                    "Kein Kopfhörer-Ausgang: Radio-Modus und Einstellungen → Vorhören (PFL)",
                )
                .clicked()
            {
                out.preview = Some(PathBuf::from(&t.path));
            }
            if ui
                .add_enabled(
                    pfl_available,
                    egui::Button::new(theme::fill_label(theme::icf::STOP, "", 14.0, theme::text()))
                        .fill(theme::surface()),
                )
                .on_hover_text("Vorhören stoppen")
                .clicked()
            {
                out.stop_preview = true;
            }
            let mut kind = t.kind;
            egui::ComboBox::from_id_salt("lib_sel_kind")
                .selected_text(kind.label())
                .width(90.0)
                .show_ui(ui, |ui| {
                    for k in Kind::ALL {
                        ui.selectable_value(&mut kind, k, k.label());
                    }
                });
            if kind != t.kind
                && let Some(lib) = &self.lib
            {
                if let Err(e) = lib.set_kind(t.id, kind) {
                    self.note = Some(e.to_string());
                }
                self.dirty = true;
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.label(theme::panel_label("TAGS"));
            let mut remove = None;
            for tag in &t.tags {
                let b = ui.add(
                    egui::Button::new(
                        egui::RichText::new(format!("{tag}  {}", theme::ic::X)).small(),
                    )
                    .fill(theme::mix(theme::cue(), theme::surface(), 0.8)),
                );
                if b.on_hover_text("Tag entfernen").clicked() {
                    remove = Some(tag.clone());
                }
            }
            if let Some(tag) = remove {
                self.untag(t.id, &tag);
            }
            let r = ui.add(
                egui::TextEdit::singleline(&mut self.new_tag)
                    .hint_text("neuer Tag")
                    .desired_width(110.0),
            );
            let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if (ui.button("+ Tag").clicked() || enter) && !self.new_tag.trim().is_empty() {
                let tag = std::mem::take(&mut self.new_tag);
                self.tag_selected(&tag);
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.label(theme::panel_label("ANALYSE"));
            match &t.analysis {
                Some(a) => {
                    ui.label(
                        egui::RichText::new(analysis_summary(a))
                            .small()
                            .color(theme::text()),
                    );
                    if ui
                        .small_button("Neu analysieren")
                        .on_hover_text("Misst den Titel noch einmal")
                        .clicked()
                        && let Some(lib) = &self.lib
                    {
                        if let Err(e) = lib.clear_analysis(t.id) {
                            self.note = Some(e.to_string());
                        }
                        self.analysis_blocked = false;
                        self.dirty = true;
                    }
                }
                None => {
                    ui.label(
                        egui::RichText::new("wartet auf die Hintergrundanalyse …")
                            .small()
                            .color(theme::text_dim()),
                    );
                }
            }
        });
    }
}

/// Kennung einer Listenzeile (für Fokus und Entf).
fn row_id(track: i64) -> egui::Id {
    egui::Id::new(("lib_row", track))
}

/// Rechte Kanten der Spalten (x-Koordinaten, rechtsbündig) einer Zeile, die bei `right` endet.
#[derive(Debug, Clone, Copy)]
struct Columns {
    duration: f32,
    kind: f32,
    lufs: f32,
    key: f32,
    bpm: f32,
    tags: f32,
}

impl Columns {
    fn at(right: f32) -> Self {
        let duration = right - 8.0;
        let kind = right - 62.0;
        let lufs = kind - 56.0;
        let key = lufs - 62.0;
        let bpm = key - 50.0;
        Self {
            duration,
            kind,
            lufs,
            key,
            bpm,
            tags: bpm - 56.0,
        }
    }
}

/// Tempo für die Spalte: „128“ oder „–“, „…“ vor der Analyse.
fn bpm_text(a: Option<&Analysis>) -> String {
    match a {
        None => "…".into(),
        Some(a) => a.bpm.map_or("–".into(), |b| format!("{b:.0}")),
    }
}

fn key_text(a: Option<&Analysis>) -> String {
    match a {
        None => "…".into(),
        Some(a) => a.key.clone().unwrap_or_else(|| "–".into()),
    }
}

fn lufs_text(a: Option<&Analysis>) -> String {
    match a {
        None => "…".into(),
        Some(a) => a.lufs.map_or("–".into(), |l| format!("{l:.1}")),
    }
}

/// Eine Zeile für die Detailansicht: alle Messwerte und der Cue-Vorschlag.
fn analysis_summary(a: &Analysis) -> String {
    let mut parts = Vec::new();
    if let Some(b) = a.bpm {
        parts.push(format!("{b:.1} BPM"));
    }
    if let Some(k) = &a.key {
        parts.push(k.clone());
    }
    if let Some(l) = a.lufs {
        parts.push(format!("{l:.1} LUFS"));
    }
    if let Some(p) = a.true_peak_db {
        parts.push(format!("True Peak {p:.1} dBTP"));
    }
    if let (Some(lead), Some(tail)) = (a.lead_silence, a.tail_silence)
        && (lead >= 0.25 || tail >= 0.5)
    {
        parts.push(format!("Stille vorn {lead:.1} s, hinten {tail:.1} s"));
    }
    let cues: Vec<String> = [
        ("Cue-In", a.cue_in),
        ("Fade-Out", a.fade_out),
        ("Cue-Out", a.cue_out),
    ]
    .into_iter()
    .filter_map(|(n, v)| v.map(|v| format!("{n} {v:.2} s")))
    .collect();
    if !cues.is_empty() {
        parts.push(format!("Cue-Vorschlag: {}", cues.join(", ")));
    }
    if parts.is_empty() {
        "Analyse ohne Ergebnis (Stille oder nicht lesbar)".into()
    } else {
        parts.join(" · ")
    }
}

fn duration_text(secs: f64) -> String {
    let s = secs.max(0.0).round() as u64;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

fn import_summary(p: &Progress) -> String {
    let get = Progress::get;
    let mut s = format!("{} neu", get(&p.added));
    if get(&p.duplicates) > 0 {
        s.push_str(&format!(", {} schon vorhanden", get(&p.duplicates)));
    }
    if get(&p.updated) > 0 {
        s.push_str(&format!(", {} aktualisiert", get(&p.updated)));
    }
    if get(&p.failed) > 0 {
        s.push_str(&format!(", {} nicht lesbar", get(&p.failed)));
    }
    if p.cancel.load(std::sync::atomic::Ordering::Relaxed) {
        s.push_str(" (abgebrochen)");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(id: i64) -> Track {
        Track {
            id,
            path: format!("/t/{id}.wav"),
            hash: String::new(),
            title: format!("T{id}"),
            duration: 1.0,
            kind: Kind::Music,
            added: 0,
            tags: Vec::new(),
            analysis: None,
        }
    }

    #[test]
    fn shift_anchor_follows_track_after_new_search() {
        let mut w = LibraryWindow::new(PathBuf::from("/nonexistent/lib.db"));
        w.results = (1..=5).map(track).collect();
        w.mark_click(3, false, false); // Anker: Titel 4
        // Neue Suche: Titel 4 steht jetzt vorn.
        w.results = vec![track(4), track(9), track(8), track(7)];
        w.mark_click(2, false, true);
        assert_eq!(w.marked(), &[4, 9, 8]);
        // Anker nicht mehr in den Treffern: Umschalt-Klick wählt nur den Titel.
        w.results = vec![track(20), track(21), track(22)];
        w.marked = vec![21];
        w.mark_anchor = Some(4);
        w.mark_click(2, false, true);
        assert_eq!(w.marked(), &[22]);
    }

    fn write_wav(path: &Path, hz: f32) {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 8000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut w = hound::WavWriter::create(path, spec).unwrap();
        for i in 0..800 {
            let s = (i as f32 * hz * std::f32::consts::TAU / 8000.0).sin();
            w.write_sample((s * 8000.0) as i16).unwrap();
        }
        w.finalize().unwrap();
    }

    fn wait_import(w: &mut LibraryWindow) {
        let t0 = Instant::now();
        while w.importing() {
            assert!(t0.elapsed() < Duration::from_secs(60));
            std::thread::sleep(Duration::from_millis(10));
        }
        w.poll();
        w.refresh();
    }

    #[test]
    fn set_db_path_clears_error_of_old_folder() {
        let dir = tempfile::tempdir().unwrap();
        let blocker = dir.path().join("datei");
        std::fs::write(&blocker, b"x").unwrap();
        let mut w = LibraryWindow::new(blocker.join("sub").join(DB_FILE));
        w.ensure_open();
        assert!(w.error.is_some());
        w.set_db_path(dir.path().join("gut").join(DB_FILE));
        w.ensure_open();
        assert!(w.error.is_none(), "{:?}", w.error);
        assert!(w.lib.is_some());
    }

    #[test]
    fn import_search_tag_filter_through_the_window_state() {
        let dir = tempfile::tempdir().unwrap();
        let music = dir.path().join("musik");
        std::fs::create_dir_all(&music).unwrap();
        write_wav(&music.join("Sommer_Hit.wav"), 300.0);
        write_wav(&music.join("Winter_Song.wav"), 500.0);
        let mut w = LibraryWindow::new(dir.path().join("daten").join(DB_FILE));
        w.start_import(&music);
        assert!(w.importing() || w.dirty);
        wait_import(&mut w);
        assert_eq!(w.total(), 2);
        assert!(
            w.note.as_deref().is_some_and(|n| n.starts_with("2 neu")),
            "{:?}",
            w.note
        );

        w.set_query("somm");
        w.refresh();
        assert_eq!(w.results().len(), 1);
        assert_eq!(w.results()[0].title, "Sommer Hit");

        w.set_query("");
        w.refresh();
        let id = w.results()[1].id;
        w.select(Some(id));
        w.tag_selected("Lieblingslied");
        w.refresh();
        w.set_tag_filter(Some("lieblingslied"));
        w.refresh();
        assert_eq!(w.results().len(), 1);
        assert_eq!(w.results()[0].id, id);
        w.untag(id, "lieblingslied");
        w.refresh();
        assert!(w.tag_filter.is_none());
        assert_eq!(w.results().len(), 2);

        // Zweiter Import desselben Ordners: nur Doppelte.
        w.start_import(&music);
        wait_import(&mut w);
        assert_eq!(w.total(), 2);
        assert!(
            w.note
                .as_deref()
                .is_some_and(|n| n.contains("2 schon vorhanden")),
            "{:?}",
            w.note
        );
    }

    #[test]
    fn unreadable_database_path_shows_an_error_instead_of_crashing() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("kein-ordner");
        std::fs::write(&file, b"x").unwrap();
        let mut w = LibraryWindow::new(file.join(DB_FILE));
        w.refresh();
        assert!(w.error.is_some());
        assert!(w.results().is_empty());
        // Auch das Zeichnen übersteht den Fehlerfall.
        let ctx = crate::theme::test_context();
        w.open = true;
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            let _ = w.show(ui.ctx(), false);
        });
    }

    use egui_kittest::kittest::Queryable;

    /// Bibliothek mit einem Titel, offen in einem kittest-Harness.
    fn harness_with_one_track(dir: &Path) -> (egui_kittest::Harness<'static, LibraryWindow>, i64) {
        let music = dir.join("m");
        std::fs::create_dir_all(&music).unwrap();
        write_wav(&music.join("a.wav"), 300.0);
        let mut w = LibraryWindow::new(dir.join(DB_FILE));
        w.start_import(&music);
        wait_import(&mut w);
        w.open = true;
        let id = w.results()[0].id;
        let mut h = egui_kittest::Harness::builder()
            .with_size(egui::vec2(1024.0, 600.0))
            .with_step_dt(1.0 / 60.0)
            .build_ui_state(
                |ui, w: &mut LibraryWindow| {
                    let ctx = ui.ctx();
                    crate::theme::install_fonts(ctx);
                    let _ = w.show(ctx, true);
                },
                w,
            );
        h.run_steps(3);
        (h, id)
    }

    fn row_center(h: &egui_kittest::Harness<'_, LibraryWindow>, id: i64) -> egui::Pos2 {
        h.ctx
            .read_response(row_id(id))
            .expect("Zeile gezeichnet")
            .rect
            .center()
    }

    fn click(
        h: &mut egui_kittest::Harness<'_, LibraryWindow>,
        at: egui::Pos2,
        button: egui::PointerButton,
    ) {
        h.event(egui::Event::PointerMoved(at));
        h.step();
        for pressed in [true, false] {
            h.event(egui::Event::PointerButton {
                pos: at,
                button,
                pressed,
                modifiers: egui::Modifiers::NONE,
            });
            h.step();
        }
    }

    fn tag_window_open(h: &egui_kittest::Harness<'_, LibraryWindow>) -> bool {
        if !h.state().tag_editor_open || h.query_by_label("Titelinfos").is_none() {
            return false;
        }
        // Auch sichtbar: oben, nicht hinter der Bibliothek, und mit Fläche.
        let id = egui::Id::new(Some("Titelinfos"));
        let Some(rect) = h.ctx.memory(|m| m.area_rect(id)) else {
            return false;
        };
        let screen = h.ctx.content_rect();
        screen.contains_rect(rect)
            && rect.width() > 100.0
            && rect.height() > 100.0
            && h.ctx.layer_id_at(rect.center()).map(|l| l.id) == Some(id)
    }

    #[test]
    fn tags_open_by_path_and_outside_the_hits() {
        let dir = tempfile::tempdir().unwrap();
        let music = dir.path().join("m");
        std::fs::create_dir_all(&music).unwrap();
        write_wav(&music.join("a.wav"), 300.0);
        let mut w = LibraryWindow::new(dir.path().join(DB_FILE));
        w.start_import(&music);
        wait_import(&mut w);
        let t = w.results()[0].clone();
        // Radio-Playlist: nach Pfad.
        w.edit_tags_for_path(Path::new(&t.path));
        assert!(w.tag_editor_open && w.tag_editor.as_ref().unwrap().ids() == vec![t.id]);
        // Nicht in der Trefferliste (andere Suche): trotzdem aus der Datenbank.
        w.tag_editor = None;
        w.tag_editor_open = false;
        w.results.clear();
        w.edit_tags(&[t.id]);
        assert!(w.tag_editor_open, "Titel außerhalb der Treffer");
        // Nicht in der Bibliothek: Hinweis statt Stille.
        w.tag_editor = None;
        w.tag_editor_open = false;
        w.edit_tags_for_path(&dir.path().join("fremd.mp3"));
        assert!(w.tag_editor.is_none());
        assert!(
            w.note
                .as_deref()
                .is_some_and(|n| n.contains("nicht in der Bibliothek"))
        );
    }

    #[test]
    fn double_click_on_library_row_opens_tag_window() {
        let dir = tempfile::tempdir().unwrap();
        let (mut h, id) = harness_with_one_track(dir.path());
        let at = row_center(&h, id);
        click(&mut h, at, egui::PointerButton::Primary);
        click(&mut h, at, egui::PointerButton::Primary);
        h.run_steps(3);
        assert!(
            tag_window_open(&h),
            "Doppelklick öffnet die Titelinfos nicht"
        );
    }

    #[test]
    fn context_menu_on_library_row_opens_tag_window() {
        let dir = tempfile::tempdir().unwrap();
        let (mut h, id) = harness_with_one_track(dir.path());
        let at = row_center(&h, id);
        click(&mut h, at, egui::PointerButton::Secondary);
        h.run_steps(2);
        h.get_by_label("Titelinfos bearbeiten …").click();
        h.run_steps(3);
        assert!(
            tag_window_open(&h),
            "Kontextmenü öffnet die Titelinfos nicht"
        );
    }

    #[test]
    fn window_draws_with_results_and_selection() {
        let dir = tempfile::tempdir().unwrap();
        let music = dir.path().join("m");
        std::fs::create_dir_all(&music).unwrap();
        write_wav(&music.join("a.wav"), 300.0);
        let mut w = LibraryWindow::new(dir.path().join(DB_FILE));
        w.start_import(&music);
        wait_import(&mut w);
        let id = w.results()[0].id;
        w.select(Some(id));
        w.open = true;
        // Titelinfos öffnen sich und zeichnen mit (Doppelklick oder Kontextmenü).
        w.edit_tags(&[id]);
        assert!(w.tag_editor.is_some());
        let ctx = crate::theme::test_context();
        for _ in 0..3 {
            let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
                let out = w.show(ui.ctx(), true);
                assert!(out.preview.is_none() && out.drop.is_none());
            });
        }
        assert!(w.open);
    }

    #[test]
    fn window_analyses_after_import_and_shows_the_columns() {
        let dir = tempfile::tempdir().unwrap();
        let music = dir.path().join("m");
        std::fs::create_dir_all(&music).unwrap();
        // 2 s Sinus, damit eine Lautheit messbar ist.
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 8000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut wr = hound::WavWriter::create(music.join("ton.wav"), spec).unwrap();
        for i in 0..16_000 {
            let s = (i as f32 * 440.0 * std::f32::consts::TAU / 8000.0).sin();
            wr.write_sample((s * 8000.0) as i16).unwrap();
        }
        wr.finalize().unwrap();
        let mut w = LibraryWindow::new(dir.path().join(DB_FILE));
        w.open = true;
        w.start_import(&music);
        wait_import(&mut w);
        assert!(w.results()[0].analysis.is_none(), "noch nicht analysiert");
        let ctx = crate::theme::test_context();
        let t0 = Instant::now();
        loop {
            let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
                let _ = w.show(ui.ctx(), false);
            });
            if !w.analysing() && w.pending == 0 && w.results()[0].analysis.is_some() {
                break;
            }
            assert!(t0.elapsed() < Duration::from_secs(60), "analysis hangs");
            std::thread::sleep(Duration::from_millis(20));
        }
        let a = w.results()[0].analysis.clone().unwrap();
        assert!(a.lufs.is_some());
        assert!(lufs_text(Some(&a)).contains('.'));
        assert_eq!(bpm_text(Some(&a)), "–");
        assert_eq!(bpm_text(None), "…");
        assert!(analysis_summary(&a).contains("LUFS"));
        assert!(analysis_summary(&Analysis::default()).contains("ohne Ergebnis"));
        // Mit Auswahl zeichnen (Detailzeile mit Analyse).
        let id = w.results()[0].id;
        w.select(Some(id));
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            let _ = w.show(ui.ctx(), false);
        });
    }

    /// #123: Import und Analyse laufen weiter, wenn das Fenster zu ist.
    #[test]
    fn closed_window_still_imports_and_analyses() {
        let dir = tempfile::tempdir().unwrap();
        let music = dir.path().join("m");
        std::fs::create_dir_all(&music).unwrap();
        write_wav(&music.join("a.wav"), 300.0);
        write_wav(&music.join("b.wav"), 500.0);
        let mut w = LibraryWindow::new(dir.path().join(DB_FILE));
        w.start_import(&music);
        assert!(!w.open);
        let ctx = crate::theme::test_context();
        let t0 = Instant::now();
        loop {
            let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
                let out = w.show(ui.ctx(), false);
                assert!(out.drop.is_none());
            });
            // Geschlossen wird höchstens einmal je Sekunde neu gezählt.
            if !w.importing() && !w.analysing() && w.total() == 2 && w.pending == 0 {
                break;
            }
            assert!(
                t0.elapsed() < Duration::from_secs(60),
                "background work stalls"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(!w.open);
        let lib = Library::open(&dir.path().join(DB_FILE)).unwrap();
        assert_eq!(lib.pending_analysis_count().unwrap(), 0);
    }

    /// #124: Entfernen (Datei bleibt) und Sortieren per Spaltenkopf.
    #[test]
    fn remove_and_sort_through_the_window_state() {
        let dir = tempfile::tempdir().unwrap();
        let music = dir.path().join("m");
        std::fs::create_dir_all(&music).unwrap();
        write_wav(&music.join("Alpha.wav"), 300.0);
        write_wav(&music.join("Beta.wav"), 500.0);
        let mut w = LibraryWindow::new(dir.path().join(DB_FILE));
        w.start_import(&music);
        wait_import(&mut w);
        let titles = |w: &LibraryWindow| -> Vec<String> {
            w.results().iter().map(|t| t.title.clone()).collect()
        };
        assert_eq!(titles(&w), ["Alpha", "Beta"]);
        w.sort_by(SortBy::Title);
        w.refresh();
        assert_eq!(titles(&w), ["Alpha", "Beta"]);
        w.sort_by(SortBy::Title);
        w.refresh();
        assert_eq!(titles(&w), ["Beta", "Alpha"], "zweiter Klick dreht");
        w.sort_by(SortBy::Duration);
        assert!(w.desc, "Dauer zuerst absteigend");

        let id = w.results().iter().find(|t| t.title == "Beta").unwrap().id;
        w.select(Some(id));
        w.remove(id);
        w.refresh();
        assert_eq!(w.total(), 1);
        assert_eq!(titles(&w), ["Alpha"]);
        assert!(w.selected.is_none());
        assert!(music.join("Beta.wav").exists(), "Datei bleibt");
        assert!(w.note.as_deref().is_some_and(|n| n.contains("Beta")));
    }

    #[test]
    fn duration_formatting() {
        assert_eq!(duration_text(0.0), "0:00");
        assert_eq!(duration_text(83.4), "1:23");
        assert_eq!(duration_text(3725.0), "1:02:05");
        assert_eq!(duration_text(-4.0), "0:00");
    }
}
