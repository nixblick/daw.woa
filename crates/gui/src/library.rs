//! Bibliotheks-Fenster (Paket 24): Suchen, Tags, Ordner-Import, Vorhören und Ziehen in
//! Timeline, Playlist und Cart-Wall. Die Logik (Suche, Import, Tags) steckt in `woa-library`;
//! dieses Fenster hält nur den Zustand der Anzeige und ist in allen Modi dasselbe.
//!
//! Läuft komplett im UI-Thread und im Import-Hintergrund-Thread von `woa-library`; mit dem
//! Audio-Thread hat es nichts zu tun (Vorhören geht über den vorhandenen PFL-Weg der App).

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use eframe::egui;
use woa_library::import::{self, Import, Progress};
use woa_library::{Kind, Library, Query, Track};

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
    results: Vec<Track>,
    total: usize,
    tags: Vec<(String, usize)>,
    dirty: bool,
    refreshed: Instant,
    selected: Option<i64>,
    new_tag: String,
    dragging: Option<PathBuf>,
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
            results: Vec::new(),
            total: 0,
            tags: Vec::new(),
            dirty: true,
            refreshed: Instant::now(),
            selected: None,
            new_tag: String::new(),
            dragging: None,
        }
    }

    /// Anderer Datenordner: Datenbank neu öffnen.
    pub fn set_db_path(&mut self, db_path: PathBuf) {
        if db_path != self.db_path {
            self.db_path = db_path;
            self.import = None;
            self.lib = None;
            self.results.clear();
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
        };
        match lib.search(&q) {
            Ok(r) => self.results = r,
            Err(e) => {
                tracing::warn!(error = %e, "library search failed");
                self.note = Some(format!("Suche: {e}"));
            }
        }
        self.total = lib.count().unwrap_or(0);
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

    /// Zeichnet das Fenster, wenn es offen ist.
    pub fn show(&mut self, ctx: &egui::Context, pfl_available: bool) -> Output {
        let mut out = Output::default();
        if !self.open {
            return out;
        }
        self.ensure_open();
        let running = self.poll();
        if self.dirty {
            self.refresh();
        }
        if running {
            ctx.request_repaint_after(Duration::from_millis(150));
        }
        let mut open = true;
        let resp = egui::Window::new("Bibliothek")
            .open(&mut open)
            .default_size([680.0, 540.0])
            .min_size([420.0, 320.0])
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
            ui.colored_label(theme::ON_AIR, e);
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
                    .color(theme::DIM),
            );
        });
        // Import
        ui.horizontal(|ui| {
            let busy = self.importing();
            if ui
                .add_enabled(
                    !busy,
                    egui::Button::new(egui::RichText::new("Ordner importieren …").strong())
                        .fill(theme::mix(theme::ACCENT, theme::SURFACE, 0.72))
                        .stroke(egui::Stroke::new(1.0, theme::ACCENT)),
                )
                .clicked()
                && let Some(dir) = rfd::FileDialog::new()
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
                    .color(theme::ACCENT),
                );
                if ui.small_button("Abbrechen").clicked() {
                    imp.cancel();
                }
            } else if let Some(n) = &self.note {
                ui.label(egui::RichText::new(n).small().color(theme::TEXT_BRIGHT));
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
                    .fill(theme::WELL)
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
                .color(theme::DIM),
            );
            return;
        }
        let n = self.results.len();
        let mut clicked = None;
        let mut started = None;
        let mut stopped = None;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show_rows(ui, ROW_H, n, |ui, range| {
                for i in range {
                    let t = &self.results[i];
                    let (rect, resp) = ui.allocate_exact_size(
                        egui::vec2(ui.available_width(), ROW_H),
                        egui::Sense::click_and_drag(),
                    );
                    let sel = self.selected == Some(t.id);
                    let painter = ui.painter_at(rect);
                    if sel {
                        painter.rect_filled(rect, 3, theme::mix(theme::ACCENT, theme::WELL, 0.78));
                    } else if resp.hovered() {
                        painter.rect_filled(rect, 3, theme::PANEL);
                    }
                    let mid = rect.center().y;
                    let text_col = if sel {
                        theme::TEXT_BRIGHT
                    } else {
                        theme::mix(theme::TEXT_BRIGHT, theme::DIM, 0.2)
                    };
                    painter.text(
                        egui::pos2(rect.left() + 8.0, mid),
                        egui::Align2::LEFT_CENTER,
                        &t.title,
                        egui::FontId::proportional(13.0),
                        text_col,
                    );
                    painter.text(
                        egui::pos2(rect.right() - 8.0, mid),
                        egui::Align2::RIGHT_CENTER,
                        duration_text(t.duration),
                        egui::FontId::monospace(12.0),
                        theme::DIM,
                    );
                    let kind_x = rect.right() - 62.0;
                    painter.text(
                        egui::pos2(kind_x, mid),
                        egui::Align2::RIGHT_CENTER,
                        t.kind.label(),
                        egui::FontId::proportional(11.0),
                        theme::CUE_CYAN,
                    );
                    if !t.tags.is_empty() {
                        painter.text(
                            egui::pos2(kind_x - 70.0, mid),
                            egui::Align2::RIGHT_CENTER,
                            t.tags.join(" · "),
                            egui::FontId::proportional(11.0),
                            theme::DIM,
                        );
                    }
                    if resp.clicked() || resp.drag_started() {
                        clicked = Some(t.id);
                    }
                    if resp.drag_started() {
                        started = Some(PathBuf::from(&t.path));
                    }
                    if resp.drag_stopped() {
                        stopped = Some(PathBuf::from(&t.path));
                    }
                    resp.on_hover_text(&t.path);
                }
            });
        if let Some(id) = clicked {
            self.selected = Some(id);
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
                    format!("♪ {name}"),
                    egui::FontId::proportional(13.0),
                    theme::WELL,
                );
                let r = egui::Rect::from_min_size(
                    pos + egui::vec2(14.0, 10.0),
                    galley.size() + egui::vec2(14.0, 8.0),
                );
                painter.rect_filled(r, 4, theme::ACCENT);
                painter.galley(r.min + egui::vec2(7.0, 4.0), galley, theme::WELL);
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
                egui::RichText::new("Titel anklicken: Tags vergeben, vorhören. In die Timeline, Playlist oder auf ein Cart-Pad ziehen.")
                    .small()
                    .color(theme::DIM),
            );
            return;
        };
        ui.horizontal_wrapped(|ui| {
            ui.label(
                egui::RichText::new(&t.title)
                    .strong()
                    .color(theme::TEXT_BRIGHT),
            );
            if ui
                .add_enabled(
                    pfl_available,
                    egui::Button::new("▶ Vorhören").fill(theme::SURFACE),
                )
                .on_disabled_hover_text(
                    "Kein Kopfhörer-Ausgang: Radio-Modus und Einstellungen → Vorhören (PFL)",
                )
                .clicked()
            {
                out.preview = Some(PathBuf::from(&t.path));
            }
            if ui
                .add_enabled(pfl_available, egui::Button::new("■").fill(theme::SURFACE))
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
            ui.label(egui::RichText::new("TAGS").size(10.0).color(theme::DIM));
            let mut remove = None;
            for tag in &t.tags {
                let b = ui.add(
                    egui::Button::new(egui::RichText::new(format!("{tag}  ✕")).small())
                        .fill(theme::mix(theme::CUE_CYAN, theme::SURFACE, 0.8)),
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
        let ctx = egui::Context::default();
        w.open = true;
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            let _ = w.show(ui.ctx(), false);
        });
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
        let ctx = egui::Context::default();
        for _ in 0..3 {
            let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
                let out = w.show(ui.ctx(), true);
                assert!(out.preview.is_none() && out.drop.is_none());
            });
        }
        assert!(w.open);
    }

    #[test]
    fn duration_formatting() {
        assert_eq!(duration_text(0.0), "0:00");
        assert_eq!(duration_text(83.4), "1:23");
        assert_eq!(duration_text(3725.0), "1:02:05");
        assert_eq!(duration_text(-4.0), "0:00");
    }
}
