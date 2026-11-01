//! Browser links im Produzenten-Modus (Design System 2, `Browser`): eine Stelle für alles, was
//! man ins Projekt holt. Sechs Reiter (Klänge, Beats, Akkorde, FX, Samples, Jingles), ersetzt die
//! Knopfreihe unter den Spuren. Akkorde und FX erzeugen direkt hier; die übrigen Reiter führen
//! zu den bestehenden Wegen (Import, Produktionen, Piano-Roll, KI-Chat, Jingle-Assistent).

use eframe::egui;
use woa_model::TrackId;

use crate::akkorde::AkkordFenster;
use crate::jingle_fx::FxFenster;
use crate::state::AppState;
use crate::theme;

/// Breite des Browsers laut Entwurf.
pub const WIDTH: f32 = 280.0;

/// Reiter des Browsers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Sounds,
    Beats,
    Chords,
    Fx,
    Samples,
    Jingles,
}

impl Tab {
    pub const ALL: [(Self, &'static str); 6] = [
        (Self::Sounds, "Klänge"),
        (Self::Beats, "Beats"),
        (Self::Chords, "Akkorde"),
        (Self::Fx, "FX"),
        (Self::Samples, "Samples"),
        (Self::Jingles, "Jingles"),
    ];
}

/// Was der Browser von der App will (Wege, die App-Zustand brauchen).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wish {
    ImportAudio,
    Produktionen,
    MidiClip,
    OpenChat,
    JingleWizard,
}

/// Zustand des Browsers.
pub struct Browser {
    pub open: bool,
    pub tab: Tab,
}

impl Default for Browser {
    fn default() -> Self {
        Self {
            open: true,
            tab: Tab::Chords,
        }
    }
}

/// Beschriftung der Einfüge-Taste: Takt am Abspielkopf (4/4, ab 1).
pub fn insert_label(at_beats: f64) -> String {
    format!(
        "Einfügen · Takt {}",
        (at_beats / 4.0).floor().max(0.0) as u64 + 1
    )
}

/// Hauptaktion (44 px, `accent`), z. B. „Einfügen · Takt n“.
pub fn primary(ui: &mut egui::Ui, text: &str) -> egui::Response {
    ui.add_sized(
        [ui.available_width(), 44.0],
        egui::Button::new(
            egui::RichText::new(text.to_uppercase())
                .font(theme::panel_label_font())
                .size(15.0)
                .color(theme::on_accent()),
        )
        .fill(theme::accent()),
    )
}

/// Segment-Wahl (2 · 4 · 8 …), eingeschaltet in `accent-soft`.
pub fn chips<T: PartialEq + Copy + std::fmt::Display>(
    ui: &mut egui::Ui,
    value: &mut T,
    choices: &[T],
) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        for &c in choices {
            let on = *value == c;
            let b =
                egui::Button::new(egui::RichText::new(c.to_string()).monospace().color(if on {
                    theme::text()
                } else {
                    theme::text_dim()
                }))
                .fill(if on {
                    theme::accent_soft()
                } else {
                    theme::well()
                })
                .min_size(egui::vec2(28.0, 24.0));
            if ui.add(b).clicked() {
                *value = c;
            }
        }
    });
}

/// Zweitrangige Taste mit Symbol und Wort (32 px, volle Breite).
fn entry(ui: &mut egui::Ui, icon: &str, text: &str, hint: &str) -> bool {
    ui.add_sized(
        [ui.available_width(), 32.0],
        egui::Button::new(egui::RichText::new(format!("{icon}  {text}")).color(theme::text()))
            .fill(theme::surface()),
    )
    .on_hover_text(hint)
    .clicked()
}

fn note(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text).small().color(theme::text_dim()));
}

impl Browser {
    /// Zeichnet den Browser in `ui`. `at_beats` ist der Taktanfang am Abspielkopf.
    #[allow(clippy::too_many_arguments)]
    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        state: &mut AppState,
        chords: &mut AkkordFenster,
        fx: &mut FxFenster,
        track: Option<TrackId>,
        at_beats: f64,
    ) -> Option<Wish> {
        let mut wish = None;
        ui.set_max_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            ui.spacing_mut().button_padding = egui::vec2(1.0, 2.0);
            let w = (ui.available_width() - 2.0) / Tab::ALL.len() as f32;
            for (t, name) in Tab::ALL {
                let on = self.tab == t;
                let b = egui::Button::new(
                    egui::RichText::new(name.to_uppercase())
                        .font(theme::panel_label_font())
                        .size(10.0)
                        .color(if on { theme::text() } else { theme::text_dim() }),
                )
                .fill(if on {
                    theme::surface()
                } else {
                    egui::Color32::TRANSPARENT
                })
                .stroke(if on {
                    egui::Stroke::new(1.0, theme::border_strong())
                } else {
                    egui::Stroke::NONE
                });
                if ui.add_sized([w, 24.0], b).clicked() {
                    self.tab = t;
                }
            }
        });
        ui.add_space(8.0);
        match self.tab {
            Tab::Sounds => {
                ui.label(theme::panel_label("AUDIO"));
                if entry(
                    ui,
                    theme::ic::FOLDER_OPEN,
                    "Audio importieren …",
                    "WAV, FLAC, MP3, OGG oder M4A als Clip am Abspielkopf einfügen",
                ) {
                    wish = Some(Wish::ImportAudio);
                }
                if entry(
                    ui,
                    theme::ic::WAVEFORM,
                    "Produktionen",
                    "Audio-Dateien des Projekts (Aufnahmen, Exporte, von Claude erzeugt), \
                     die neuesten oben; auf eine Spur ziehen",
                ) {
                    wish = Some(Wish::Produktionen);
                }
                ui.add_space(8.0);
                note(
                    ui,
                    "Instrument-Klänge wählst du im Rack der MIDI-Spur (Mixer-Reiter rechts).",
                );
            }
            Tab::Beats => {
                ui.label(theme::panel_label("MUSTER"));
                if entry(
                    ui,
                    theme::ic::PIANO_KEYS,
                    "Leerer MIDI-Clip (4 Takte)",
                    "Leeren 4-Takt-MIDI-Clip am Abspielkopf anlegen und in der Piano-Roll öffnen \
                     (oder Doppelklick in eine MIDI-Spur)",
                ) {
                    wish = Some(Wish::MidiClip);
                }
                note(
                    ui,
                    "Beats baust du im Drum-Raster der Piano-Roll; der Würfel dort setzt einen \
                     zufälligen Beat aus den Presets.",
                );
            }
            Tab::Chords => chords.body(ui, state, at_beats),
            Tab::Fx => fx.body(ui, state, track, at_beats),
            Tab::Samples => {
                ui.label(theme::panel_label("FREESOUND"));
                note(
                    ui,
                    "Samples von Freesound sucht die KI für dich (eigener Schlüssel unter \
                     Einstellungen → KI). Treffer landen in der Bibliothek, mit Lizenz-Hinweis.",
                );
                if entry(
                    ui,
                    theme::ic::CHAT_CIRCLE,
                    "Im KI-Chat suchen",
                    "Öffnet den KI-Chat im Dock, z. B. „Such mir Applaus auf Freesound“",
                ) {
                    wish = Some(Wish::OpenChat);
                }
                ui.add_space(8.0);
                note(
                    ui,
                    "CC BY: Urheber steht im Titel-Tag und in der Exportliste.",
                );
            }
            Tab::Jingles => {
                ui.label(theme::panel_label("ASSISTENT"));
                if entry(
                    ui,
                    theme::ic::WAVEFORM,
                    "Jingle bauen …",
                    "Assistent: Länge und Stimmung wählen, Beat, Akkorde, Stimme, Ausgabe",
                ) {
                    wish = Some(Wish::JingleWizard);
                }
                note(
                    ui,
                    "Länge, Stimmung, Beat, Akkorde, Stimme, Ausgabe – Schritt für Schritt.",
                );
            }
        }
        wish
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_label_names_the_bar_at_the_playhead() {
        assert_eq!(insert_label(0.0), "Einfügen · Takt 1");
        assert_eq!(insert_label(16.0), "Einfügen · Takt 5");
        assert_eq!(insert_label(17.5), "Einfügen · Takt 5");
    }
}
