//! Fenster „Produktionen“ (André 31.10.2026): alle Audio-Dateien des Projektordners
//! (`aufnahmen/`, Exporte, was Claude erzeugt hat), die neuesten oben. Vorhören über den
//! PFL-Weg der Bibliothek, Ziehen auf eine Spur legt die Datei dort ab (derselbe Weg wie aus
//! der Bibliothek). Gelesen wird nur beim Öffnen, auf „Neu einlesen“ und nach Agent-Schritten.

use eframe::egui;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::library::Output;
use crate::theme;

/// So tief steigt die Suche in Unterordner.
const MAX_DEPTH: usize = 3;
/// Höchstens so viele Dateien.
const MAX_FILES: usize = 500;

/// Eine Datei der Liste.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub path: PathBuf,
    pub name: String,
    pub seconds: Option<f64>,
    pub modified: SystemTime,
}

/// Alle Audio-Dateien unter `root` (ohne versteckte Ordner), die neuesten zuerst.
pub fn scan(root: &Path) -> Vec<Entry> {
    let mut out = Vec::new();
    walk(root, 0, &mut out);
    out.sort_by(|a, b| {
        b.modified
            .cmp(&a.modified)
            .then_with(|| a.name.cmp(&b.name))
    });
    out.truncate(MAX_FILES);
    for e in &mut out {
        e.seconds = woa_library::tags::read_header(&e.path)
            .ok()
            .map(|h| h.duration)
            .filter(|d| *d > 0.0);
    }
    out
}

fn walk(dir: &Path, depth: usize, out: &mut Vec<Entry>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        // Keine Verknüpfungen folgen: nur, was wirklich im Projektordner liegt.
        let Ok(ft) = e.file_type() else { continue };
        let path = e.path();
        if ft.is_dir() {
            if depth < MAX_DEPTH {
                walk(&path, depth + 1, out);
            }
        } else if ft.is_file() && woa_engine::decode::is_readable(&path) {
            let modified = e
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH);
            out.push(Entry {
                path,
                name,
                seconds: None,
                modified,
            });
        }
    }
}

/// „vor 3 min“, ohne Zeitzone.
fn age(t: SystemTime) -> String {
    let s = SystemTime::now()
        .duration_since(t)
        .map_or(0, |d| d.as_secs());
    match s {
        0..60 => "gerade eben".into(),
        60..3600 => format!("vor {} min", s / 60),
        3600..86_400 => format!("vor {} h", s / 3600),
        _ => format!("vor {} Tagen", s / 86_400),
    }
}

fn length(s: Option<f64>) -> String {
    match s {
        Some(s) => {
            let s = s.round() as u64;
            format!("{}:{:02}", s / 60, s % 60)
        }
        None => "–".into(),
    }
}

/// Zustand des Fensters.
#[derive(Default)]
pub struct ProduktionenWindow {
    pub open: bool,
    root: Option<PathBuf>,
    entries: Vec<Entry>,
    stale: bool,
    dragging: Option<PathBuf>,
}

impl ProduktionenWindow {
    /// Öffnet das Fenster und liest den Ordner neu.
    pub fn open_window(&mut self) {
        self.open = true;
        self.stale = true;
    }

    /// Nach Agent-Schritten neu lesen (nur, wenn das Fenster offen ist).
    pub fn mark_stale(&mut self) {
        self.stale = true;
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// Zeichnet das Fenster für den Projektordner `root`. `pfl`: Vorhören ist möglich.
    pub fn show(&mut self, ctx: &egui::Context, root: &Path, pfl: bool) -> Output {
        let mut out = Output::default();
        if !self.open {
            self.dragging = None;
            return out;
        }
        if self.stale || self.root.as_deref() != Some(root) {
            self.entries = scan(root);
            self.root = Some(root.to_path_buf());
            self.stale = false;
        }
        let mut open = true;
        let resp = egui::Window::new(format!("{} Produktionen", theme::ic::WAVEFORM))
            .id(egui::Id::new("produktionen"))
            .open(&mut open)
            .default_size([520.0, 420.0])
            .min_size([360.0, 200.0])
            .frame(theme::card_frame())
            .show(ctx, |ui| self.body(ui, pfl, &mut out));
        self.open = open;
        if let (Some((_, pos)), Some(r)) = (&out.drop, &resp)
            && r.response.rect.contains(*pos)
        {
            out.drop = None;
        }
        out
    }

    fn body(&mut self, ui: &mut egui::Ui, pfl: bool, out: &mut Output) {
        ui.horizontal(|ui| {
            if ui
                .button(format!("{} Neu einlesen", theme::ic::ARROWS_CLOCKWISE))
                .clicked()
            {
                self.stale = true;
            }
            if pfl
                && ui
                    .button(format!("{} Vorhören stoppen", theme::ic::STOP))
                    .clicked()
            {
                out.stop_preview = true;
            }
            ui.label(
                egui::RichText::new("Auf eine Spur ziehen legt die Datei dort ab")
                    .small()
                    .color(theme::text_dim()),
            );
        });
        ui.separator();
        if self.entries.is_empty() {
            ui.label(
                egui::RichText::new("Noch keine Audio-Dateien im Projektordner")
                    .color(theme::text_dim()),
            );
            return;
        }
        let mut started = None;
        let mut stopped = None;
        egui::ScrollArea::vertical().show(ui, |ui| {
            for e in &self.entries {
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(pfl, egui::Button::new(theme::ic::HEADPHONES).small())
                        .on_hover_text("Vorhören im Kopfhörer (PFL, nie auf Sendung)")
                        .on_disabled_hover_text(
                            "Vorhören braucht einen Kopfhörer-Ausgang (Einstellungen → Vorhören)",
                        )
                        .clicked()
                    {
                        out.preview = Some(e.path.clone());
                    }
                    let resp = ui
                        .add(
                            egui::Label::new(format!("{} {}", theme::ic::MUSIC_NOTE, e.name))
                                .sense(egui::Sense::click_and_drag())
                                .truncate(),
                        )
                        .on_hover_text(e.path.display().to_string());
                    if resp.drag_started() {
                        started = Some(e.path.clone());
                    }
                    if resp.drag_stopped() {
                        stopped = Some(e.path.clone());
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            egui::RichText::new(age(e.modified))
                                .small()
                                .color(theme::text_dim()),
                        );
                        ui.label(egui::RichText::new(length(e.seconds)).small().monospace());
                    });
                });
            }
        });
        if let Some(p) = started {
            self.dragging = Some(p);
        }
        if let Some(p) = &self.dragging {
            let ctx = ui.ctx().clone();
            ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
            if let Some(pos) = ctx.pointer_latest_pos() {
                let name = p
                    .file_name()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let painter = ctx.layer_painter(egui::LayerId::new(
                    egui::Order::Tooltip,
                    egui::Id::new("prod_drag_ghost"),
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn touch(path: &Path, ago_secs: u64) {
        std::fs::write(path, b"RIFF").unwrap();
        let f = std::fs::File::options().write(true).open(path).unwrap();
        f.set_modified(SystemTime::now() - Duration::from_secs(ago_secs))
            .unwrap();
    }

    #[test]
    fn newest_first_and_only_audio() {
        let tmp = tempfile::tempdir().unwrap();
        let takes = tmp.path().join("aufnahmen");
        std::fs::create_dir_all(&takes).unwrap();
        std::fs::create_dir_all(tmp.path().join(".versteckt")).unwrap();
        touch(&takes.join("alt.wav"), 3600);
        touch(&tmp.path().join("export.flac"), 10);
        touch(&takes.join("mitte.wav"), 600);
        touch(&tmp.path().join("project.json"), 0);
        touch(&tmp.path().join(".versteckt/neu.wav"), 0);
        let names: Vec<String> = scan(tmp.path()).into_iter().map(|e| e.name).collect();
        assert_eq!(names, ["export.flac", "mitte.wav", "alt.wav"]);
    }

    #[test]
    fn age_and_length_texts() {
        assert_eq!(length(Some(75.4)), "1:15");
        assert_eq!(length(None), "–");
        assert_eq!(age(SystemTime::now()), "gerade eben");
        assert_eq!(
            age(SystemTime::now() - Duration::from_secs(7200)),
            "vor 2 h"
        );
    }
}
