//! „Spur exportieren …“ im Kontextmenü des Spurkopfs (Ticket 229): nur diese Spur als WAV
//! oder MP3, mit Dateiname, Titel, Interpret und Kommentar. Gerechnet wird wie ein Stem
//! ([`woa_engine::stems::render_track`]) in einem eigenen Thread, nicht in der Oberfläche
//! und nicht im Audio-Thread.

use std::path::PathBuf;
use std::sync::mpsc;

use eframe::egui;

use woa_engine::export::{ExportFormat, ExportTags};
use woa_model::TrackId;

use crate::theme;

/// Formate, die der Dialog anbietet.
pub const FORMATS: [ExportFormat; 2] = [ExportFormat::Wav, ExportFormat::Mp3];

/// Was der Dialog am Ende liefert.
#[derive(Debug, Clone, PartialEq)]
pub struct TrackExportRequest {
    pub track: TrackId,
    pub file_name: String,
    pub format: ExportFormat,
    pub tags: ExportTags,
}

/// Fenster „Spur exportieren“.
#[derive(Debug, Clone, PartialEq)]
pub struct TrackExportDialog {
    pub track: TrackId,
    pub file_name: String,
    pub format: ExportFormat,
    pub tags: ExportTags,
}

impl TrackExportDialog {
    /// Vorgaben: Dateiname und Titel sind der Spurname, Format WAV.
    pub fn new(track: TrackId, name: &str) -> Self {
        Self {
            track,
            file_name: woa_engine::stems::sanitize_name(name),
            format: ExportFormat::Wav,
            tags: ExportTags {
                title: name.to_string(),
                ..Default::default()
            },
        }
    }

    /// Anfrage mit bereinigtem Dateinamen (ohne Endung, ohne Pfadteile).
    pub fn request(&self) -> TrackExportRequest {
        let stem = self
            .file_name
            .trim()
            .strip_suffix(&format!(".{}", self.format.extension()))
            .unwrap_or(self.file_name.trim());
        TrackExportRequest {
            track: self.track,
            file_name: format!(
                "{}.{}",
                woa_engine::stems::sanitize_name(stem),
                self.format.extension()
            ),
            format: self.format,
            tags: self.tags.clone(),
        }
    }

    /// Zeichnet das Fenster. `Ok(Some)` bei „Exportieren“, `Err(())` bei Schließen.
    pub(crate) fn show(&mut self, ctx: &egui::Context) -> Result<Option<TrackExportRequest>, ()> {
        let mut open = true;
        let mut result = None;
        egui::Window::new("Spur exportieren")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .frame(theme::card_frame())
            .show(ctx, |ui| {
                egui::Grid::new("track_export_grid")
                    .num_columns(2)
                    .spacing([8.0, 6.0])
                    .show(ui, |ui| {
                        ui.label("Dateiname");
                        ui.text_edit_singleline(&mut self.file_name);
                        ui.end_row();
                        ui.label("Format");
                        ui.horizontal(|ui| {
                            for f in FORMATS {
                                if ui
                                    .add(egui::Button::selectable(self.format == f, f.label()))
                                    .clicked()
                                {
                                    self.format = f;
                                }
                            }
                        });
                        ui.end_row();
                        ui.label("Titel");
                        ui.text_edit_singleline(&mut self.tags.title);
                        ui.end_row();
                        ui.label("Interpret");
                        ui.text_edit_singleline(&mut self.tags.artist);
                        ui.end_row();
                        ui.label("Kommentar");
                        ui.text_edit_singleline(&mut self.tags.comment);
                        ui.end_row();
                    });
                ui.add_space(6.0);
                if ui
                    .button(format!("{} Exportieren …", theme::ic::EXPORT))
                    .clicked()
                {
                    result = Some(self.request());
                }
            });
        if !open {
            return Err(());
        }
        Ok(result)
    }
}

/// Ein laufender Export im Hintergrund.
pub struct TrackExportJob {
    pub out: PathBuf,
    pub rx: mpsc::Receiver<Result<f64, String>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_come_from_the_track_name_and_the_name_is_cleaned() {
        let mut d = TrackExportDialog::new(TrackId(7), "Gitarre/Solo");
        assert_eq!(d.tags.title, "Gitarre/Solo");
        assert_eq!(d.format, ExportFormat::Wav);
        assert_eq!(d.request().file_name, "Gitarre_Solo.wav");
        d.format = ExportFormat::Mp3;
        d.file_name = "../Mein Song.mp3".into();
        let r = d.request();
        assert!(r.file_name.ends_with("Mein Song.mp3") && !r.file_name.contains('/'));
        assert_eq!(r.track, TrackId(7));
    }
}
