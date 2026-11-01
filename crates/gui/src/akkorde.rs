//! Akkordfolgen im Browser (Paket J3, Issue #261; seit Design System 2 kein Fenster mehr): Folge und Tonart wählen, drei MIDI-Spuren
//! entstehen. Ein Tonartwechsel danach transponiert die drei zuletzt erzeugten Clips.

use eframe::egui;
use woa_model::ClipId;
use woa_model::chords::{CHORD_PROGRESSIONS, ChordProgression, KEYS, key_shift};

use crate::state::AppState;
use crate::theme;

/// Zustand des Akkord-Reiters.
pub struct AkkordFenster {
    folge: usize,
    key: u8,
    bars: u32,
    /// Die zuletzt erzeugten Clips und ihre Tonart.
    last: Option<(Vec<ClipId>, u8)>,
}

impl Default for AkkordFenster {
    fn default() -> Self {
        Self {
            folge: 0,
            key: 0,
            bars: 4,
            last: None,
        }
    }
}

/// Dur-Leiter und natürliches Moll in Halbtönen.
const MAJOR: [u8; 7] = [0, 2, 4, 5, 7, 9, 11];
const MINOR: [u8; 7] = [0, 2, 3, 5, 7, 8, 10];

/// Akkordnamen der Folge `p` in Tonart `key` (z. B. „C G Am F“), leitereigene Dreiklänge.
pub fn chord_names(p: &ChordProgression, key: u8) -> Vec<String> {
    let scale = if p.minor { MINOR } else { MAJOR };
    p.degrees
        .iter()
        .map(|&d| {
            let i = usize::from(d.clamp(1, 7) - 1);
            let root = scale[i];
            let third = (scale[(i + 2) % 7] + 12 - root) % 12;
            let fifth = (scale[(i + 4) % 7] + 12 - root) % 12;
            let name = KEYS[usize::from((key + root) % 12)];
            match (third, fifth) {
                (3, 6) => format!("{name}°"),
                (3, _) => format!("{name}m"),
                _ => name.to_string(),
            }
        })
        .collect()
}

/// Akkord-Chips (dunkle Kästchen mit Namen, darunter optional die Stufe).
fn chord_chips(ui: &mut egui::Ui, names: &[String], degrees: Option<&[&str]>, big: bool) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 3.0;
        for (i, n) in names.iter().enumerate() {
            let size = if big {
                egui::vec2(36.0, 34.0)
            } else {
                egui::vec2(24.0, 18.0)
            };
            let (r, _) = ui.allocate_exact_size(size, egui::Sense::hover());
            let p = ui.painter();
            p.rect_filled(r, 3.0, theme::lcd_bg());
            p.rect_stroke(
                r,
                3.0,
                egui::Stroke::new(1.0, theme::border()),
                egui::StrokeKind::Inside,
            );
            let deg = degrees.and_then(|d| d.get(i));
            let top = if deg.is_some() {
                r.center_top() + egui::vec2(0.0, 12.0)
            } else {
                r.center()
            };
            p.text(
                top,
                egui::Align2::CENTER_CENTER,
                n,
                egui::FontId::proportional(if big { 13.0 } else { 10.0 }),
                theme::text(),
            );
            if let Some(d) = deg {
                p.text(
                    r.center_bottom() - egui::vec2(0.0, 8.0),
                    egui::Align2::CENTER_CENTER,
                    *d,
                    egui::FontId::monospace(9.0),
                    theme::text_dim(),
                );
            }
        }
    });
}

impl AkkordFenster {
    /// Inhalt des Browser-Reiters „Akkorde“; `at_beats` ist der Taktanfang am Abspielkopf.
    pub fn body(&mut self, ui: &mut egui::Ui, state: &mut AppState, at_beats: f64) {
        let list_h = (ui.available_height() - 375.0).max(80.0);
        egui::ScrollArea::vertical()
            .id_salt("akkord_liste")
            .max_height(list_h)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                for minor in [false, true] {
                    ui.label(theme::panel_label(if minor { "MOLL" } else { "DUR" }));
                    for (i, q) in CHORD_PROGRESSIONS.iter().enumerate() {
                        if q.minor != minor {
                            continue;
                        }
                        let on = self.folge == i;
                        let resp = egui::Frame::new()
                            .fill(if on {
                                theme::accent_soft()
                            } else {
                                egui::Color32::TRANSPARENT
                            })
                            .stroke(if on {
                                egui::Stroke::new(1.0, theme::accent())
                            } else {
                                egui::Stroke::NONE
                            })
                            .corner_radius(4)
                            .inner_margin(egui::Margin::symmetric(10, 4))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.label(egui::RichText::new(q.hint).strong().color(theme::text()));
                                chord_chips(ui, &chord_names(q, self.key), None, false);
                            })
                            .response
                            .interact(egui::Sense::click())
                            .on_hover_text(q.name);
                        if resp.clicked() {
                            self.folge = i;
                        }
                    }
                }
            });
        ui.separator();
        let Some(p) = CHORD_PROGRESSIONS.get(self.folge) else {
            return;
        };
        let degrees: Vec<&str> = p.name.split('–').collect();
        chord_chips(ui, &chord_names(p, self.key), Some(&degrees), true);
        let mode = if p.minor { "Moll" } else { "Dur" };
        ui.horizontal(|ui| {
            ui.label(theme::panel_label("TONART"));
            ui.label(
                egui::RichText::new(format!("{} {mode}", KEYS[usize::from(self.key % 12)]))
                    .small()
                    .color(theme::text_dim()),
            );
        });
        let before = self.key;
        let key_w = ((ui.available_width() - 11.0 * 2.0) / 12.0).floor();
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(2.0, 2.0);
            ui.spacing_mut().button_padding = egui::vec2(0.0, 2.0);
            for (i, k) in KEYS.iter().enumerate() {
                let on = usize::from(self.key) == i;
                let b = egui::Button::new(egui::RichText::new(*k).size(9.5).color(if on {
                    theme::on_accent()
                } else {
                    theme::text()
                }))
                .fill(if on { theme::accent() } else { theme::well() });
                if ui
                    .add_sized([key_w, 24.0], b)
                    .on_hover_text("Nach dem Erzeugen verschiebt ein Wechsel alle drei Clips")
                    .clicked()
                {
                    self.key = i as u8;
                }
            }
        });
        if before != self.key
            && let Some((clips, from)) = &self.last
        {
            if state.transpose_clips(clips, key_shift(*from, self.key)) {
                self.last = Some((clips.clone(), self.key));
            } else {
                self.last = None;
            }
        }
        ui.horizontal(|ui| {
            ui.label(theme::panel_label("TAKTE"));
            crate::browser::chips(ui, &mut self.bars, &[2, 4, 8]);
            ui.add(egui::DragValue::new(&mut self.bars).range(1..=16))
                .on_hover_text("Andere Länge, 1 bis 16 Takte");
        });
        note_row(ui, theme::track_color(0), "Akkorde");
        note_row(ui, theme::track_color(1), "Bass");
        note_row(ui, theme::track_color(2), "Arpeggio");
        if crate::browser::primary(ui, &crate::browser::insert_label(at_beats))
            .on_hover_text("Drei MIDI-Spuren: Akkorde, Bass, Arpeggio, ab dem Takt am Abspielkopf")
            .clicked()
            && let Some(made) = state.add_chord_tracks(p, self.key, self.bars, at_beats)
        {
            self.last = Some((made.iter().map(|m| m.1).collect(), self.key));
        }
        ui.label(
            egui::RichText::new("Alle Töne passen zur Tonart · Strg+Z: zurück")
                .small()
                .color(theme::text_dim()),
        );
    }
}

/// Eine Ausgabe-Zeile mit Spurfarbe.
fn note_row(ui: &mut egui::Ui, color: egui::Color32, name: &str) {
    egui::Frame::new()
        .fill(theme::well())
        .stroke(egui::Stroke::new(1.0, theme::border()))
        .corner_radius(3)
        .inner_margin(egui::Margin::symmetric(8, 3))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(egui::vec2(9.0, 9.0), egui::Sense::hover());
                ui.painter().rect_filled(r, 2.0, color);
                ui.label(egui::RichText::new(name).small().color(theme::text()));
            });
        });
}
