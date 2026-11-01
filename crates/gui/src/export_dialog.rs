//! Export-Fenster (Paket 26): Format, Wortbreite oder Bitrate, Stems und Mixdown.
//!
//! Formate, die ein externes ffmpeg brauchen (Opus, Ogg, AAC), sind ausgegraut, wenn es fehlt
//! oder den Encoder nicht kann; der Hinweis steht daneben. Die Suche nach ffmpeg startet einen
//! Prozess und läuft deshalb einmal beim Öffnen, nie je Bild. Der Zustand ist ohne Fenster
//! testbar.

use std::path::PathBuf;

use eframe::egui;
use woa_engine::export::{BitDepth, ExportFormat, ExportSpec};
use woa_engine::ffmpeg::{Encoders, Ffmpeg};
use woa_engine::stems::StemSelect;

use crate::theme;

/// Was der Nutzer abgeschickt hat.
#[derive(Debug, Clone, PartialEq)]
pub struct ExportRequest {
    pub spec: ExportSpec,
    /// Stems: eine Datei je Spur in einen Ordner; sonst eine Datei.
    pub stems: Option<StemOptions>,
    /// Als Jingle (J6): WAV mit Ziel-Lautheit und Limiter in den Jingle-Ordner, in die
    /// Bibliothek und auf Wunsch auf eine Cart ([`woa_agent::jingle`], derselbe Weg wie MCP).
    pub jingle: Option<JingleOptions>,
}

/// Optionen für „Als Jingle“.
#[derive(Debug, Clone, PartialEq)]
pub struct JingleOptions {
    /// Dateiname ohne Endung.
    pub name: String,
    /// `None` = Ziel-Lautheit der Sendung.
    pub target_lufs: Option<f64>,
    /// Cart 1 bis 16; `None` = keine.
    pub cart: Option<usize>,
}

impl Default for JingleOptions {
    fn default() -> Self {
        Self {
            name: "Jingle".into(),
            target_lufs: None,
            cart: None,
        }
    }
}

/// Optionen für Stems.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StemOptions {
    pub select: StemSelect,
    /// Mixdown als zusätzliche Datei.
    pub mixdown: bool,
}

/// Ergebnis der ffmpeg-Suche.
#[derive(Debug, Clone, PartialEq)]
enum Probe {
    /// Noch nicht gesucht.
    Pending,
    Found(PathBuf, Encoders),
    /// Nicht gefunden oder nicht lauffähig: Text für den Nutzer.
    Missing(String),
}

/// Zustand des Fensters.
pub struct ExportDialog {
    pub open: bool,
    format: ExportFormat,
    depth: Option<BitDepth>,
    kbps: u32,
    stems: bool,
    buses: bool,
    mixdown: bool,
    jingle: Option<JingleOptions>,
    probe: Probe,
}

impl Default for ExportDialog {
    fn default() -> Self {
        Self::new()
    }
}

impl ExportDialog {
    pub fn new() -> Self {
        Self {
            open: false,
            format: ExportFormat::Wav,
            depth: None,
            kbps: 0,
            stems: false,
            buses: true,
            mixdown: true,
            jingle: None,
            probe: Probe::Pending,
        }
    }

    /// Öffnet das Fenster und sucht ffmpeg (einmal).
    pub fn open_now(&mut self) {
        self.open = true;
        if self.probe == Probe::Pending {
            self.probe = Self::search();
        }
    }

    fn search() -> Probe {
        match Ffmpeg::locate() {
            None => Probe::Missing(woa_engine::ffmpeg::MISSING.to_owned()),
            Some(ff) => match ff.encoders() {
                Ok(enc) => Probe::Found(ff.path().to_path_buf(), enc),
                Err(e) => Probe::Missing(e),
            },
        }
    }

    /// Setzt das Ergebnis der Suche von außen (Tests).
    pub fn set_ffmpeg(&mut self, found: Option<(PathBuf, Encoders)>) {
        self.probe = match found {
            Some((p, e)) => Probe::Found(p, e),
            None => Probe::Missing(woa_engine::ffmpeg::MISSING.to_owned()),
        };
    }

    /// Geht das Format? `Err` mit dem Grund, wenn nicht.
    pub fn available(&self, format: ExportFormat) -> Result<(), String> {
        let Some(codec) = format.codec() else {
            return Ok(());
        };
        match &self.probe {
            Probe::Pending => Err("ffmpeg wird noch gesucht".into()),
            Probe::Missing(why) => Err(why.clone()),
            Probe::Found(_, enc) if enc.supports(codec) => Ok(()),
            Probe::Found(..) => Err(format!(
                "Dieses ffmpeg kann {} nicht schreiben (Encoder fehlt)",
                codec.label()
            )),
        }
    }

    /// Wählt ein Format, wenn es geht, und setzt Wortbreite und Bitrate auf dessen Vorgabe.
    pub fn select(&mut self, format: ExportFormat) -> bool {
        if self.available(format).is_err() {
            return false;
        }
        if format != self.format {
            self.format = format;
            self.depth = None;
            self.kbps = 0;
        }
        true
    }

    pub fn format(&self) -> ExportFormat {
        self.format
    }

    pub fn set_stems(&mut self, on: bool) {
        self.stems = on;
    }

    pub fn set_depth(&mut self, depth: BitDepth) {
        if self.format.depths().contains(&depth) {
            self.depth = Some(depth);
        }
    }

    /// Bitrate in kbit/s, in den erlaubten Grenzen; 0 ist die Vorgabe des Formats.
    pub fn set_kbps(&mut self, kbps: u32) {
        self.kbps = kbps.clamp(32, 512);
    }

    /// Die Anfrage aus dem aktuellen Zustand, wenn das Format geht.
    /// Schaltet „Als Jingle“ ein (`Some`) oder aus.
    pub fn set_jingle(&mut self, jingle: Option<JingleOptions>) {
        self.jingle = jingle;
    }

    pub fn request(&self) -> Result<ExportRequest, String> {
        if let Some(j) = &self.jingle {
            woa_agent::jingle::check_name(&j.name)?;
            return Ok(ExportRequest {
                spec: ExportSpec::new(ExportFormat::Wav),
                stems: None,
                jingle: Some(j.clone()),
            });
        }
        self.available(self.format)?;
        let mut spec = ExportSpec::new(self.format);
        spec.depth = self.depth;
        spec.kbps = self.kbps;
        if let Probe::Found(p, _) = &self.probe
            && self.format.codec().is_some()
        {
            spec.ffmpeg = Some(p.clone());
        }
        Ok(ExportRequest {
            spec,
            stems: self.stems.then_some(StemOptions {
                select: StemSelect {
                    tracks: true,
                    buses: self.buses,
                },
                mixdown: self.mixdown,
            }),
            jingle: None,
        })
    }

    /// Zeichnet das Fenster. Gibt die Anfrage zurück, wenn „Exportieren“ geklickt wurde
    /// (das Fenster schließt sich dann).
    pub fn show(&mut self, ctx: &egui::Context) -> Option<ExportRequest> {
        if !self.open {
            return None;
        }
        let mut open = true;
        let mut result = None;
        egui::Window::new("Exportieren")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .frame(theme::card_frame())
            .show(ctx, |ui| result = self.body(ui));
        self.open = open && result.is_none();
        result
    }

    fn body(&mut self, ui: &mut egui::Ui) -> Option<ExportRequest> {
        self.jingle_ui(ui);
        let mut out = None;
        ui.add_enabled_ui(self.jingle.is_none(), |ui| out = self.file_ui(ui));
        if self.jingle.is_some() {
            ui.add_space(8.0);
            let go = ui.add(
                egui::Button::new(
                    egui::RichText::new("Als Jingle exportieren")
                        .color(theme::bg())
                        .strong(),
                )
                .fill(theme::accent()),
            );
            if go.clicked() {
                match self.request() {
                    Ok(r) => out = Some(r),
                    Err(e) => tracing::warn!(error = %e, "jingle name refused"),
                }
            }
        }
        out
    }

    /// Abschnitt „Als Jingle“ (J6): Name, Ziel-Lautheit, Cart.
    fn jingle_ui(&mut self, ui: &mut egui::Ui) {
        let mut on = self.jingle.is_some();
        if ui
            .checkbox(
                &mut on,
                "Als Jingle: Lautheit und Limiter, in den Jingle-Ordner und die Bibliothek",
            )
            .changed()
        {
            self.jingle = on.then(JingleOptions::default);
        }
        let Some(j) = self.jingle.as_mut() else {
            ui.separator();
            return;
        };
        ui.indent("jingle_opts", |ui| {
            ui.horizontal(|ui| {
                ui.label("Name");
                ui.text_edit_singleline(&mut j.name);
            });
            if let Err(e) = woa_agent::jingle::check_name(&j.name) {
                ui.label(egui::RichText::new(e).small().color(theme::warn()));
            }
            ui.horizontal(|ui| {
                let mut own = j.target_lufs.is_some();
                ui.checkbox(&mut own, "Eigene Lautheit");
                if own {
                    let mut t = j.target_lufs.unwrap_or(-16.0);
                    ui.add(
                        egui::DragValue::new(&mut t)
                            .range(-30.0..=-6.0)
                            .speed(0.1)
                            .suffix(" LUFS"),
                    );
                    j.target_lufs = Some(t);
                } else {
                    j.target_lufs = None;
                    ui.label(
                        egui::RichText::new("wie die Sendung (Auto-Gain)")
                            .small()
                            .color(theme::text_dim()),
                    );
                }
            });
            ui.horizontal(|ui| {
                ui.label("Cart");
                let shown = j.cart.map_or("keine".to_string(), |c| c.to_string());
                egui::ComboBox::from_id_salt("jingle_cart")
                    .selected_text(shown)
                    .show_ui(ui, |ui| {
                        if ui.selectable_label(j.cart.is_none(), "keine").clicked() {
                            j.cart = None;
                        }
                        for c in 1..=woa_radio::CARTS {
                            if ui
                                .selectable_label(j.cart == Some(c), c.to_string())
                                .clicked()
                            {
                                j.cart = Some(c);
                            }
                        }
                    });
            });
        });
        ui.separator();
    }

    fn file_ui(&mut self, ui: &mut egui::Ui) -> Option<ExportRequest> {
        ui.label(theme::panel_label("FORMAT"));
        ui.horizontal_wrapped(|ui| {
            for f in ExportFormat::ALL {
                let ok = self.available(f);
                let resp = ui.add_enabled(
                    ok.is_ok(),
                    egui::Button::selectable(self.format == f, f.label()),
                );
                let resp = match &ok {
                    Err(why) => resp.on_disabled_hover_text(why),
                    Ok(()) => resp,
                };
                if resp.clicked() {
                    self.select(f);
                }
            }
        });
        if let Probe::Missing(why) = &self.probe {
            ui.label(egui::RichText::new(why).small().color(theme::text_dim()));
        }
        ui.add_space(6.0);
        let depths = self.format.depths();
        if !depths.is_empty() {
            ui.horizontal(|ui| {
                ui.label("Wortbreite");
                let shown = self.depth.unwrap_or(if self.format == ExportFormat::Flac {
                    BitDepth::Int24
                } else {
                    BitDepth::Float32
                });
                egui::ComboBox::from_id_salt("export_depth")
                    .selected_text(shown.label())
                    .show_ui(ui, |ui| {
                        for d in depths {
                            if ui.selectable_label(shown == *d, d.label()).clicked() {
                                self.set_depth(*d);
                            }
                        }
                    });
                if self.format == ExportFormat::Flac {
                    ui.label(
                        egui::RichText::new("FLAC kennt kein Float")
                            .small()
                            .color(theme::text_dim()),
                    );
                }
            });
        }
        if self.format.codec().is_some() {
            ui.horizontal(|ui| {
                ui.label("Bitrate");
                let mut k = if self.kbps == 0 {
                    self.format.default_kbps()
                } else {
                    self.kbps
                };
                if ui
                    .add(
                        egui::DragValue::new(&mut k)
                            .range(32..=512)
                            .speed(1.0)
                            .suffix(" kbit/s"),
                    )
                    .changed()
                {
                    self.set_kbps(k);
                }
            });
        }
        ui.separator();
        ui.checkbox(&mut self.stems, "Stems: eine Datei je Spur in einen Ordner");
        ui.add_enabled_ui(self.stems, |ui| {
            ui.indent("stem_opts", |ui| {
                ui.checkbox(&mut self.buses, "Busse dazu");
                ui.checkbox(&mut self.mixdown, "Mixdown dazu (00 Mixdown)");
            });
        });
        if self.stems {
            ui.label(
                egui::RichText::new(
                    "Stumme Spuren fehlen. Spuren, die in einen Bus laufen, stecken auch im Bus-Stem.",
                )
                .small()
                .color(theme::text_dim()),
            );
        }
        ui.add_space(8.0);
        let mut out = None;
        ui.horizontal(|ui| {
            let label = if self.stems {
                "Ordner wählen und exportieren …"
            } else {
                "Datei wählen und exportieren …"
            };
            let go = ui.add(
                egui::Button::new(egui::RichText::new(label).color(theme::bg()).strong())
                    .fill(theme::accent()),
            );
            if go.clicked() {
                match self.request() {
                    Ok(r) => out = Some(r),
                    Err(e) => tracing::warn!(error = %e, "export format unavailable"),
                }
            }
        });
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all() -> Encoders {
        Encoders {
            opus: true,
            vorbis: true,
            aac: true,
        }
    }

    #[test]
    fn without_ffmpeg_only_the_builtin_formats_work() {
        let mut d = ExportDialog::new();
        d.set_ffmpeg(None);
        for f in [ExportFormat::Wav, ExportFormat::Flac, ExportFormat::Mp3] {
            assert!(d.available(f).is_ok());
        }
        for f in [ExportFormat::Opus, ExportFormat::Ogg, ExportFormat::Aac] {
            let why = d.available(f).unwrap_err();
            assert!(why.contains("ffmpeg"), "{why}");
            assert!(!d.select(f), "ausgegraut: nicht wählbar");
        }
        assert_eq!(d.format(), ExportFormat::Wav);
    }

    #[test]
    fn encoder_missing_in_a_found_ffmpeg_greys_only_that_format() {
        let mut d = ExportDialog::new();
        d.set_ffmpeg(Some((
            "/usr/bin/ffmpeg".into(),
            Encoders {
                opus: true,
                vorbis: false,
                aac: true,
            },
        )));
        assert!(d.available(ExportFormat::Opus).is_ok());
        assert!(
            d.available(ExportFormat::Ogg)
                .unwrap_err()
                .contains("Ogg Vorbis")
        );
        assert!(d.select(ExportFormat::Aac));
    }

    #[test]
    fn request_carries_choices_and_resets_on_format_change() {
        let mut d = ExportDialog::new();
        d.set_ffmpeg(Some(("/usr/bin/ffmpeg".into(), all())));
        d.set_depth(BitDepth::Int16);
        d.set_depth(BitDepth::Int24);
        let r = d.request().unwrap();
        assert_eq!(r.spec.format, ExportFormat::Wav);
        assert_eq!(r.spec.depth, Some(BitDepth::Int24));
        assert!(r.spec.ffmpeg.is_none() && r.stems.is_none());
        // FLAC kennt kein Float.
        assert!(d.select(ExportFormat::Flac));
        d.set_depth(BitDepth::Float32);
        assert_eq!(d.request().unwrap().spec.depth, None, "Wahl zurückgesetzt");
        // Opus: ffmpeg-Pfad und Bitrate in den Grenzen.
        assert!(d.select(ExportFormat::Opus));
        d.set_kbps(5);
        d.set_stems(true);
        let r = d.request().unwrap();
        assert_eq!(r.spec.kbps, 32);
        assert_eq!(r.spec.ffmpeg, Some(PathBuf::from("/usr/bin/ffmpeg")));
        let s = r.stems.unwrap();
        assert!(s.mixdown && s.select.tracks && s.select.buses);
    }

    #[test]
    fn request_fails_when_the_format_is_not_available() {
        let mut d = ExportDialog::new();
        d.set_ffmpeg(Some(("/usr/bin/ffmpeg".into(), all())));
        assert!(d.select(ExportFormat::Opus));
        d.set_ffmpeg(None);
        assert!(d.request().unwrap_err().contains("ffmpeg"));
    }

    #[test]
    fn dialog_draws_without_a_window() {
        let ctx = crate::theme::test_context();
        let mut d = ExportDialog::new();
        d.set_ffmpeg(None);
        d.open = true;
        for _ in 0..2 {
            let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
                assert!(d.show(ui.ctx()).is_none());
            });
        }
        assert!(d.open);
    }

    #[test]
    fn jingle_request_is_wav_without_stems_and_checks_the_name() {
        let mut d = ExportDialog::new();
        d.set_ffmpeg(Some(("/usr/bin/ffmpeg".into(), all())));
        assert!(d.select(ExportFormat::Opus));
        d.set_stems(true);
        d.set_jingle(Some(JingleOptions {
            name: "Station Ident".into(),
            target_lufs: Some(-14.0),
            cart: Some(3),
        }));
        let r = d.request().unwrap();
        assert_eq!(r.spec.format, ExportFormat::Wav, "Jingle ist immer WAV");
        assert!(r.stems.is_none());
        let j = r.jingle.unwrap();
        assert_eq!(
            (j.name.as_str(), j.target_lufs, j.cart),
            ("Station Ident", Some(-14.0), Some(3))
        );
        d.set_jingle(Some(JingleOptions {
            name: "../weg".into(),
            ..JingleOptions::default()
        }));
        assert!(d.request().is_err());
        d.set_jingle(None);
        assert!(d.request().unwrap().jingle.is_none());
    }
}
