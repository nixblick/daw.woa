//! Protokoll-Fenster (Hilfe → Protokoll): Info, Warnungen und Fehler der laufenden
//! Sitzung aus dem Ringpuffer [`woa_telemetry::journal`]. Nur lokal; der Upload bekommt
//! davon nichts. Dazu das Warnsymbol für beschädigte Dateien in Playlist und Bibliothek.

use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use eframe::egui;
use woa_engine::decode::Damage;
use woa_engine::localtime::Zone;
use woa_telemetry::{Entry, Stufe};

use crate::theme;

/// Zustand des Fensters.
#[derive(Debug)]
pub struct ProtokollWindow {
    pub open: bool,
    info: bool,
    warn: bool,
    error: bool,
}

impl Default for ProtokollWindow {
    fn default() -> Self {
        Self {
            open: false,
            info: true,
            warn: true,
            error: true,
        }
    }
}

/// Uhrzeit einer Zeile (mitteleuropäische Zeit).
fn time_text(e: &Entry) -> String {
    let unix = e
        .zeit
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    crate::radio::time_of_day(unix, Zone::CENTRAL_EUROPE)
}

/// Eine Zeile als Text (für „Kopieren“).
pub fn line_text(e: &Entry) -> String {
    let mut s = format!("{} {:<7} {}", time_text(e), e.stufe.label(), e.text);
    if e.anzahl > 1 {
        s.push_str(&format!(" ×{}", e.anzahl));
    }
    if let Some(d) = &e.datei {
        s.push_str(&format!(" [{d}]"));
    }
    s
}

fn icon(s: Stufe) -> (&'static str, egui::Color32) {
    match s {
        Stufe::Info => (theme::ic::INFO, theme::text_dim()),
        Stufe::Warnung => (theme::ic::WARNING, theme::warn()),
        Stufe::Fehler => (theme::ic::WARNING_OCTAGON, theme::on_air()),
    }
}

/// Öffnet einen Ordner im Dateimanager des Systems.
fn open_folder(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    #[cfg(windows)]
    let prog = "explorer";
    #[cfg(target_os = "macos")]
    let prog = "open";
    #[cfg(not(any(windows, target_os = "macos")))]
    let prog = "xdg-open";
    std::process::Command::new(prog)
        .arg(dir)
        .spawn()
        .map(|_| ())
}

impl ProtokollWindow {
    fn shows(&self, s: Stufe) -> bool {
        match s {
            Stufe::Info => self.info,
            Stufe::Warnung => self.warn,
            Stufe::Fehler => self.error,
        }
    }

    /// Zeilen nach Filter, neueste zuerst.
    pub fn visible(&self, all: Vec<Entry>) -> Vec<Entry> {
        all.into_iter()
            .rev()
            .filter(|e| self.shows(e.stufe))
            .collect()
    }

    /// Zeichnet das Fenster; `log_dir` ist der Ordner der Logdateien.
    pub fn show(&mut self, ctx: &egui::Context, log_dir: &Path) {
        if !self.open {
            return;
        }
        let mut open = self.open;
        let rows = self.visible(woa_telemetry::journal().snapshot());
        egui::Window::new(format!("{} Protokoll", theme::ic::LIST_BULLETS))
            .open(&mut open)
            .default_size([640.0, 380.0])
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.toggle_value(&mut self.info, format!("{} Info", theme::ic::INFO));
                    ui.toggle_value(&mut self.warn, format!("{} Warnungen", theme::ic::WARNING));
                    ui.toggle_value(
                        &mut self.error,
                        format!("{} Fehler", theme::ic::WARNING_OCTAGON),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .button(format!("{} Logordner öffnen", theme::ic::FOLDER_OPEN))
                            .on_hover_text(log_dir.display().to_string())
                            .clicked()
                            && let Err(e) = open_folder(log_dir)
                        {
                            tracing::warn!("open log folder: {e}");
                        }
                        if ui
                            .button(format!("{} Kopieren", theme::ic::COPY))
                            .on_hover_text("Die angezeigten Zeilen in die Zwischenablage")
                            .clicked()
                        {
                            let text: Vec<String> = rows.iter().map(line_text).collect();
                            ui.ctx().copy_text(text.join("\n"));
                        }
                    });
                });
                ui.label(
                    egui::RichText::new(
                        "Nur auf diesem Rechner. Die anonymen Fehlercodes enthalten keinen Text \
                         und keine Dateinamen.",
                    )
                    .small()
                    .color(theme::text_dim()),
                );
                ui.separator();
                if rows.is_empty() {
                    ui.label(egui::RichText::new("Keine Meldungen.").color(theme::text_dim()));
                    return;
                }
                egui::ScrollArea::vertical().auto_shrink(false).show_rows(
                    ui,
                    18.0,
                    rows.len(),
                    |ui, range| {
                        for e in &rows[range] {
                            ui.horizontal(|ui| {
                                let (ic, col) = icon(e.stufe);
                                ui.label(egui::RichText::new(ic).color(col));
                                ui.label(
                                    egui::RichText::new(time_text(e))
                                        .monospace()
                                        .small()
                                        .color(theme::text_dim()),
                                );
                                if e.anzahl > 1 {
                                    ui.label(
                                        egui::RichText::new(format!("×{}", e.anzahl))
                                            .small()
                                            .strong()
                                            .color(col),
                                    );
                                }
                                let r = ui.add(
                                    egui::Label::new(
                                        egui::RichText::new(&e.text).color(theme::text()),
                                    )
                                    .truncate(),
                                );
                                let mut hover = format!("{}\n{}", e.text, e.ziel);
                                if let Some(d) = &e.datei {
                                    hover.push('\n');
                                    hover.push_str(d);
                                }
                                r.on_hover_text(hover);
                            });
                        }
                    },
                );
            });
        self.open = open;
    }
}

/// Tooltip-Text für eine beschädigte oder unlesbare Datei.
pub fn damage_hint(path: &Path) -> Option<String> {
    let d = woa_engine::decode::damage(path)?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    Some(format!("{}\nDetails: Hilfe → Protokoll", d.text(&name)))
}

/// Kleines Warnsymbol mit Tooltip, wenn `path` beim Lesen beschädigt oder unlesbar war.
/// Kein Pop-up: stört keine Sendung.
pub fn damage_badge(ui: &mut egui::Ui, path: &Path) {
    let Some(d) = woa_engine::decode::damage(path) else {
        return;
    };
    let col = match d {
        Damage::Damaged(_) => theme::warn(),
        Damage::Unreadable(_) => theme::on_air(),
    };
    if let Some(hint) = damage_hint(path) {
        ui.label(egui::RichText::new(theme::ic::WARNING).color(col))
            .on_hover_text(hint);
    }
}

/// Logordner zum Datenverzeichnis (wie `apps/woa`: `<daten>/logs`).
pub fn log_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("logs")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::SystemTime;

    fn entry(stufe: Stufe, text: &str, n: u64) -> Entry {
        Entry {
            zeit: SystemTime::UNIX_EPOCH,
            stufe,
            ziel: "t".into(),
            text: text.into(),
            datei: None,
            anzahl: n,
        }
    }

    #[test]
    fn filter_hides_levels_and_lists_newest_first() {
        let mut w = ProtokollWindow::default();
        let all = vec![
            entry(Stufe::Info, "a", 1),
            entry(Stufe::Warnung, "b", 1084),
            entry(Stufe::Fehler, "c", 1),
        ];
        let v = w.visible(all.clone());
        assert_eq!(
            v.iter().map(|e| e.text.as_str()).collect::<Vec<_>>(),
            ["c", "b", "a"]
        );
        w.info = false;
        assert_eq!(w.visible(all).len(), 2);
        assert!(line_text(&entry(Stufe::Warnung, "b", 1084)).ends_with("b ×1084"));
    }

    #[test]
    fn window_renders_without_entries_and_with_badge() {
        let ctx = theme::test_context();
        let mut w = ProtokollWindow {
            open: true,
            ..Default::default()
        };
        let _ = ctx.run_ui(Default::default(), |ui| {
            w.show(ui.ctx(), Path::new("/nicht/da"));
            damage_badge(ui, Path::new("/nicht/da.mp3"));
        });
        assert!(w.open);
    }
}
