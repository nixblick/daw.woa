//! Titelinfos bearbeiten: Titel, Interpret, Album, Jahr, Genre, Tracknummer, Kommentar und
//! Cover, für einen oder mehrere Titel. Eigenständiges Widget (`TagEditor::ui`), damit es in
//! jedes Fenster passt (Bibliothek, später „Verwalten → Titel“).
//!
//! Lesen und Schreiben laufen in einem Hintergrund-Thread; die Oberfläche wartet nie auf die
//! Platte. Geschrieben wird über eine Zwischendatei mit atomarem Ersetzen
//! ([`woa_library::tags::write_fields`]); die Wiedergabe läuft aus dem Speicher und merkt
//! nichts. Hält ein anderes Programm die Datei (Windows), versucht WOA es selbst wieder.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

use eframe::egui;
use woa_library::tags::{
    Cover, CoverEdit, Edit, FIELD_NAMES, Fields, WriteError, read_fields, read_header, write_fields,
};
use woa_library::{Library, file_hash, import::display_title};

use crate::theme;

/// So oft wird eine gesperrte Datei erneut versucht.
const RETRY_EVERY: Duration = Duration::from_secs(10);
/// Kurze Wiederholungen im Hintergrund, bevor eine Datei als gesperrt gilt.
const QUICK_TRIES: u32 = 5;

/// Ein Titel zum Bearbeiten.
#[derive(Debug, Clone)]
pub struct Item {
    pub id: i64,
    pub path: PathBuf,
}

/// Gelesener Stand eines Titels.
type Loaded = Vec<(Item, Result<(Fields, Option<Cover>), String>)>;
/// Ergebnis eines Speicherlaufs: je Titel Erfolg (mit dem alten Stand für Rückgängig) oder Fehler.
type Saved = Vec<(Item, Edit, Result<Edit, WriteError>)>;

enum CoverView {
    /// Alle gleich (oder ein Titel): dieses Bild, oder keins.
    Same(Option<Cover>),
    /// Mehrere Titel mit verschiedenen Bildern.
    Mixed,
}

/// Zustand des Editors.
pub struct TagEditor {
    db_path: PathBuf,
    items: Vec<Item>,
    loading: Option<Receiver<Loaded>>,
    /// Werte in den Feldern.
    fields: Fields,
    /// Feld unterscheidet sich zwischen den gewählten Titeln (bleibt ohne Eingabe, wie es ist).
    mixed: [bool; 7],
    /// Vom Nutzer geändert.
    touched: [bool; 7],
    cover: CoverView,
    cover_edit: CoverEdit,
    texture: Option<egui::TextureHandle>,
    texture_dirty: bool,
    saving: Option<Receiver<Saved>>,
    /// Gesperrte Dateien mit ihrer Änderung; werden selbst erneut versucht.
    retry: Vec<(Item, Edit)>,
    retry_at: Option<Instant>,
    undo: Vec<(Item, Edit)>,
    note: Option<String>,
    error: Option<String>,
    /// Hat ein Speicherlauf Dateien geändert (die Liste soll neu suchen)?
    changed: bool,
}

impl TagEditor {
    /// Öffnet den Editor für `items` und liest deren Titelinfos im Hintergrund.
    pub fn open(db_path: PathBuf, items: Vec<Item>) -> Self {
        let (tx, rx) = channel();
        let todo = items.clone();
        let spawned = std::thread::Builder::new()
            .name("woa-tags-read".into())
            .spawn(move || {
                let out: Loaded = todo
                    .into_iter()
                    .map(|it| {
                        let r = read_fields(&it.path);
                        (it, r)
                    })
                    .collect();
                let _ = tx.send(out);
            });
        let error = spawned.err().map(|e| format!("Titelinfos: {e}"));
        Self {
            db_path,
            items,
            loading: error.is_none().then_some(rx),
            fields: Fields::default(),
            mixed: [false; 7],
            touched: [false; 7],
            cover: CoverView::Same(None),
            cover_edit: CoverEdit::Keep,
            texture: None,
            texture_dirty: true,
            saving: None,
            retry: Vec::new(),
            retry_at: None,
            undo: Vec::new(),
            note: None,
            error,
            changed: false,
        }
    }

    /// Titel, die dieser Editor bearbeitet.
    pub fn ids(&self) -> Vec<i64> {
        self.items.iter().map(|i| i.id).collect()
    }

    /// Läuft noch Arbeit (Speichern, gesperrte Dateien)? Dann darf der Editor nicht weg.
    pub fn busy(&self) -> bool {
        self.saving.is_some() || !self.retry.is_empty()
    }

    /// Gibt einmal `true` zurück, nachdem Dateien geändert wurden.
    pub fn take_changed(&mut self) -> bool {
        std::mem::take(&mut self.changed)
    }

    /// Ob die Felder fertig gelesen sind.
    pub fn loaded(&self) -> bool {
        self.loading.is_none()
    }

    pub fn fields(&self) -> &Fields {
        &self.fields
    }

    pub fn note(&self) -> Option<&str> {
        self.note.as_deref().or(self.error.as_deref())
    }

    /// Setzt ein Feld wie eine Eingabe (für Tests und Agenten).
    pub fn set_field(&mut self, i: usize, value: &str) {
        *self.fields.get_mut(i) = value.to_string();
        self.touched[i] = true;
    }

    fn take_loaded(&mut self, loaded: Loaded) {
        let mut first: Option<(Fields, Option<Cover>)> = None;
        let mut mixed = [false; 7];
        let mut cover_mixed = false;
        let mut failed = 0usize;
        for (_, r) in &loaded {
            let Ok((f, c)) = r else {
                failed += 1;
                continue;
            };
            match &first {
                None => first = Some((f.clone(), c.clone())),
                Some((f0, c0)) => {
                    for (i, m) in mixed.iter_mut().enumerate() {
                        *m |= f0.get(i) != f.get(i);
                    }
                    cover_mixed |= c0 != c;
                }
            }
        }
        let (mut fields, cover) = first.unwrap_or_default();
        for (i, m) in mixed.iter().enumerate() {
            if *m {
                fields.get_mut(i).clear();
            }
        }
        self.fields = fields;
        self.mixed = mixed;
        self.cover = if cover_mixed {
            CoverView::Mixed
        } else {
            CoverView::Same(cover)
        };
        self.texture_dirty = true;
        if failed > 0 {
            tracing::warn!(failed, "tag editor: files not readable");
            self.error = Some(format!(
                "{failed} Datei(en) nicht lesbar, sie bleiben unverändert"
            ));
        }
    }

    /// Die Änderung aus den Feldern: nur angefasste Felder, sonst bleibt alles.
    fn edit(&self) -> Edit {
        let mut e = Edit::default();
        for (i, slot) in e.fields.iter_mut().enumerate() {
            if self.touched[i] {
                *slot = Some(self.fields.get(i).trim().to_string());
            }
        }
        e.cover = self.cover_edit.clone();
        e
    }

    /// Startet das Speichern im Hintergrund.
    pub fn save(&mut self) {
        if self.saving.is_some() {
            return;
        }
        if let Err(e) = self.fields_checked() {
            self.error = Some(e);
            return;
        }
        let edit = self.edit();
        if edit.is_empty() {
            self.note = Some("Nichts geändert".into());
            return;
        }
        let jobs = self
            .items
            .iter()
            .map(|i| (i.clone(), edit.clone()))
            .collect();
        self.run(jobs, true);
    }

    /// Prüft nur Felder, die gespeichert werden.
    fn fields_checked(&self) -> Result<(), String> {
        let mut f = Fields::default();
        for i in [3usize, 5] {
            if self.touched[i] {
                *f.get_mut(i) = self.fields.get(i).to_string();
            }
        }
        f.check()
    }

    /// Nimmt den letzten Speicherlauf zurück (schreibt den alten Stand in die Dateien).
    pub fn undo(&mut self) {
        if self.saving.is_some() || self.undo.is_empty() {
            return;
        }
        let jobs = std::mem::take(&mut self.undo);
        self.run(jobs, false);
    }

    fn run(&mut self, jobs: Vec<(Item, Edit)>, keep_undo: bool) {
        let (tx, rx) = channel();
        let db = self.db_path.clone();
        let spawned = std::thread::Builder::new()
            .name("woa-tags-write".into())
            .spawn(move || {
                let lib = Library::open(&db).ok();
                let out: Saved = jobs
                    .into_iter()
                    .map(|(it, edit)| {
                        let r = save_one(lib.as_ref(), &it, &edit);
                        let r = r.map(|old| if keep_undo { old } else { Edit::default() });
                        (it, edit, r)
                    })
                    .collect();
                let _ = tx.send(out);
            });
        match spawned {
            Ok(_) => {
                self.saving = Some(rx);
                self.note = Some("Wird gespeichert …".into());
                self.error = None;
            }
            Err(e) => self.error = Some(format!("Speichern: {e}")),
        }
    }

    fn take_saved(&mut self, saved: Saved) {
        let mut ok = 0usize;
        let mut undo = Vec::new();
        let mut errors: Vec<String> = Vec::new();
        for (it, edit, r) in saved {
            match r {
                Ok(old) => {
                    ok += 1;
                    if !old.is_empty() {
                        undo.push((it, old));
                    }
                }
                Err(WriteError::Locked) => {
                    self.retry.retain(|(i, _)| i.id != it.id);
                    self.retry.push((it, edit));
                }
                Err(e) => {
                    let name = it
                        .path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    tracing::warn!(error = %e, "tag editor: file not written");
                    errors.push(format!("{name}: {e}"));
                }
            }
        }
        if ok > 0 {
            self.changed = true;
            self.touched = [false; 7];
            self.cover_edit = CoverEdit::Keep;
            if !undo.is_empty() {
                self.undo = undo;
            }
        }
        self.retry_at = (!self.retry.is_empty()).then(|| Instant::now() + RETRY_EVERY);
        self.note = Some(match (ok, self.retry.len()) {
            (n, 0) => format!("Gespeichert ({n} Datei{})", if n == 1 { "" } else { "en" }),
            (_, r) => format!(
                "{r} Datei(en) gesperrt (ein anderes Programm nutzt sie), WOA versucht es alle {} s wieder",
                RETRY_EVERY.as_secs()
            ),
        });
        self.error = (!errors.is_empty()).then(|| errors.join("\n"));
        // Neu lesen, damit Felder und Cover den Stand der Dateien zeigen.
        if ok > 0 {
            let fresh = Self::open(self.db_path.clone(), self.items.clone());
            self.loading = fresh.loading;
        }
    }

    /// Hintergrundarbeit abholen; auch ohne sichtbares Fenster aufrufen.
    pub fn tick(&mut self, ctx: &egui::Context) {
        if let Some(rx) = &self.loading
            && let Ok(l) = rx.try_recv()
        {
            self.loading = None;
            self.take_loaded(l);
        }
        if let Some(rx) = &self.saving
            && let Ok(s) = rx.try_recv()
        {
            self.saving = None;
            self.take_saved(s);
        }
        if self.saving.is_none()
            && let Some(at) = self.retry_at
            && Instant::now() >= at
        {
            self.retry_at = None;
            let jobs = std::mem::take(&mut self.retry);
            self.run(jobs, true);
        }
        if self.loading.is_some() || self.saving.is_some() {
            ctx.request_repaint_after(Duration::from_millis(100));
        } else if let Some(at) = self.retry_at {
            ctx.request_repaint_after(at.saturating_duration_since(Instant::now()));
        }
    }

    fn shown_cover(&self) -> Option<&[u8]> {
        match (&self.cover_edit, &self.cover) {
            (CoverEdit::Set(d), _) => Some(d),
            (CoverEdit::Remove, _) | (CoverEdit::Keep, CoverView::Mixed) => None,
            (CoverEdit::Keep, CoverView::Same(c)) => c.as_ref().map(|c| c.data.as_slice()),
        }
    }

    /// Ersetzt das Cover durch die Bilddatei `path` (JPEG oder PNG).
    pub fn set_cover_file(&mut self, path: &Path) {
        match std::fs::read(path) {
            Ok(b) if b.len() <= woa_engine::cover::MAX_COVER_BYTES => {
                if crate::covers::decode_image(&b).is_none() {
                    self.error = Some("Bild nicht lesbar".into());
                    return;
                }
                self.cover_edit = CoverEdit::Set(b);
                self.texture_dirty = true;
            }
            Ok(_) => self.error = Some("Bild größer als 10 MB".into()),
            Err(e) => self.error = Some(format!("Bild: {e}")),
        }
    }

    /// Zeichnet die Felder und das Cover. Gibt zurück, ob Dateien geändert wurden.
    pub fn ui(&mut self, ui: &mut egui::Ui) -> bool {
        self.tick(ui.ctx());
        let n = self.items.len();
        ui.label(
            egui::RichText::new(if n == 1 {
                self.items[0]
                    .path
                    .file_name()
                    .map(|f| f.to_string_lossy().into_owned())
                    .unwrap_or_default()
            } else {
                format!("{n} Titel gewählt: nur geänderte Felder werden gesetzt")
            })
            .color(theme::text_dim()),
        );
        if self.loading.is_some() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Lese Titelinfos …");
            });
            return self.take_changed();
        }
        ui.horizontal_top(|ui| {
            egui::Grid::new("tag_fields")
                .num_columns(2)
                .spacing([8.0, 6.0])
                .show(ui, |ui| {
                    for (i, name) in FIELD_NAMES.iter().enumerate() {
                        ui.label(*name);
                        let hint = if self.mixed[i] && !self.touched[i] {
                            "(verschieden, bleibt)"
                        } else {
                            ""
                        };
                        let width = if i == 3 || i == 5 { 80.0 } else { 260.0 };
                        let r = ui.add(
                            egui::TextEdit::singleline(self.fields.get_mut(i))
                                .hint_text(hint)
                                .desired_width(width),
                        );
                        if r.changed() {
                            self.touched[i] = true;
                        }
                        if r.lost_focus() && ui.input(|inp| inp.key_pressed(egui::Key::Enter)) {
                            self.save();
                        }
                        ui.end_row();
                    }
                });
            ui.add_space(12.0);
            ui.vertical(|ui| self.cover_ui(ui));
        });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let dirty = self.touched.iter().any(|t| *t) || self.cover_edit != CoverEdit::Keep;
            if ui
                .add_enabled(
                    dirty && self.saving.is_none(),
                    egui::Button::new("Speichern"),
                )
                .clicked()
            {
                self.save();
            }
            if ui
                .add_enabled(
                    !self.undo.is_empty() && self.saving.is_none(),
                    egui::Button::new("Rückgängig"),
                )
                .on_hover_text("Schreibt den Stand vor dem letzten Speichern zurück")
                .clicked()
            {
                self.undo();
            }
            if self.saving.is_some() {
                ui.spinner();
            }
        });
        if let Some(n) = &self.note {
            ui.label(egui::RichText::new(n).color(theme::text_dim()));
        }
        if let Some(e) = &self.error {
            ui.colored_label(theme::on_air(), e);
        }
        self.take_changed()
    }

    fn cover_ui(&mut self, ui: &mut egui::Ui) {
        const EDGE: f32 = 140.0;
        if self.texture_dirty {
            self.texture_dirty = false;
            self.texture = self
                .shown_cover()
                .and_then(crate::covers::decode_image)
                .map(|img| {
                    ui.ctx()
                        .load_texture("tag-editor-cover", img.big, egui::TextureOptions::LINEAR)
                });
        }
        let (rect, _) = ui.allocate_exact_size(egui::vec2(EDGE, EDGE), egui::Sense::hover());
        match &self.texture {
            Some(t) => {
                let s = t.size_vec2();
                let k = (EDGE / s.x.max(s.y)).min(1.0);
                let r = egui::Rect::from_center_size(rect.center(), s * k);
                ui.painter().image(
                    t.id(),
                    r,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
            }
            None => {
                ui.painter().rect_filled(rect, 6.0, theme::surface());
                let text = if matches!(self.cover, CoverView::Mixed)
                    && self.cover_edit == CoverEdit::Keep
                {
                    "verschiedene Cover"
                } else {
                    "kein Cover"
                };
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    text,
                    egui::FontId::proportional(12.0),
                    theme::text_dim(),
                );
            }
        }
        ui.horizontal(|ui| {
            if ui.button("Bild …").on_hover_text("JPEG oder PNG").clicked()
                && let Some(p) = rfd::FileDialog::new()
                    .add_filter("Bild", &["jpg", "jpeg", "png"])
                    .pick_file()
            {
                self.set_cover_file(&p);
            }
            let has = self.shown_cover().is_some() || matches!(self.cover, CoverView::Mixed);
            if ui
                .add_enabled(has, egui::Button::new("Entfernen"))
                .clicked()
            {
                self.cover_edit = CoverEdit::Remove;
                self.texture_dirty = true;
            }
        });
    }
}

/// Schreibt eine Datei (mit kurzen Wiederholungen, wenn sie gesperrt ist) und zieht die
/// Bibliothek nach. Gibt den alten Stand für Rückgängig zurück.
fn save_one(lib: Option<&Library>, it: &Item, edit: &Edit) -> Result<Edit, WriteError> {
    let old_hash = file_hash(&it.path).map_err(|e| WriteError::Other(e.to_string()))?;
    let (old_fields, old_cover) = read_fields(&it.path).map_err(WriteError::Other)?;
    let mut tries = 0;
    loop {
        match write_fields(&it.path, edit) {
            Ok(()) => break,
            Err(WriteError::Locked) if tries + 1 < QUICK_TRIES => {
                tries += 1;
                std::thread::sleep(Duration::from_millis(300));
            }
            Err(e) => return Err(e),
        }
    }
    let new_hash = file_hash(&it.path).map_err(|e| WriteError::Other(e.to_string()))?;
    let title = read_header(&it.path)
        .map(|h| display_title(&it.path, &h))
        .map_err(WriteError::Other)?;
    if let Some(lib) = lib
        && let Err(e) = lib.retagged(it.id, &old_hash, &new_hash, &title)
    {
        tracing::warn!(error = %e, "tag editor: library not updated");
    }
    Ok(Edit::exact(&old_fields, old_cover.as_ref()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use woa_library::{AddOutcome, Kind, NewTrack};

    fn wait(ed: &mut TagEditor, ctx: &egui::Context) {
        let t0 = Instant::now();
        while ed.loading.is_some() || ed.saving.is_some() {
            assert!(t0.elapsed() < Duration::from_secs(30), "hängt");
            ed.tick(ctx);
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn wav(path: &Path, hz: f32) {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 8000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut w = hound::WavWriter::create(path, spec).unwrap();
        for i in 0..4000 {
            let s = (i as f32 * hz * std::f32::consts::TAU / 8000.0).sin();
            w.write_sample((s * 9000.0) as i16).unwrap();
        }
        w.finalize().unwrap();
    }

    #[test]
    fn edit_two_titles_save_and_undo() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("l.sqlite3");
        let lib = Library::open(&db).unwrap();
        let mut items = Vec::new();
        for (n, hz) in [(0, 300.0), (1, 500.0)] {
            let p = dir.path().join(format!("t{n}.wav"));
            wav(&p, hz);
            let AddOutcome::Added(id) = lib
                .add_track(&NewTrack {
                    path: p.to_str().unwrap().into(),
                    hash: file_hash(&p).unwrap(),
                    title: format!("t{n}"),
                    duration: 0.5,
                    kind: Kind::Music,
                })
                .unwrap()
            else {
                panic!()
            };
            items.push(Item { id, path: p });
        }
        let ctx = crate::theme::test_context();
        let mut ed = TagEditor::open(db.clone(), items.clone());
        wait(&mut ed, &ctx);
        ed.set_field(1, "Gemeinsame Band");
        ed.set_field(0, "Gleicher Titel");
        ed.save();
        wait(&mut ed, &ctx);
        assert!(ed.take_changed());
        assert!(ed.error.is_none(), "{:?}", ed.error);
        for it in &items {
            let (f, _) = read_fields(&it.path).unwrap();
            assert_eq!(f.artist, "Gemeinsame Band");
            let t = lib.get(it.id).unwrap().unwrap();
            assert_eq!(t.title, "Gemeinsame Band - Gleicher Titel");
            assert_eq!(t.hash, file_hash(&it.path).unwrap());
        }
        wait(&mut ed, &ctx);
        assert_eq!(ed.fields().artist, "Gemeinsame Band");
        ed.undo();
        wait(&mut ed, &ctx);
        for it in &items {
            let (f, _) = read_fields(&it.path).unwrap();
            assert_eq!(f.artist, "");
        }
        // Zeichnen geht in jedem Zustand.
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            ed.ui(ui);
        });
    }

    #[test]
    fn bad_year_is_refused_before_writing() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.wav");
        wav(&p, 440.0);
        let before = std::fs::read(&p).unwrap();
        let ctx = crate::theme::test_context();
        let mut ed = TagEditor::open(
            dir.path().join("l.sqlite3"),
            vec![Item {
                id: 1,
                path: p.clone(),
            }],
        );
        wait(&mut ed, &ctx);
        ed.set_field(3, "zwanzig");
        ed.save();
        assert!(ed.saving.is_none());
        assert!(ed.note().is_some());
        assert_eq!(std::fs::read(&p).unwrap(), before);
    }
}
