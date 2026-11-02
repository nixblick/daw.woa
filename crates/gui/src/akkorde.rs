//! Akkordfolgen im Browser (Paket J3, Issue #261; seit Design System 2 kein Fenster mehr): Folge und Tonart wählen, drei MIDI-Spuren
//! entstehen. Ein Tonartwechsel danach transponiert die drei zuletzt erzeugten Clips.

use eframe::egui;
use woa_model::ClipId;
use woa_model::chords::{CHORD_PROGRESSIONS, KEYS, chord_names, key_shift};

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
                        // Vorhören über der Zeile (später gezeichnet, bekommt den Klick).
                        let sp = egui::Rect::from_center_size(
                            resp.rect.right_top() + egui::vec2(-16.0, 13.0),
                            egui::vec2(22.0, 20.0),
                        );
                        if vorhoeren(ui, sp, q.hint).clicked() {
                            preview(state, q, self.key);
                        } else if resp.clicked() {
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

/// Lautsprecher-Knopf „Vorhören“ einer Zeile.
fn vorhoeren(ui: &mut egui::Ui, rect: egui::Rect, what: &str) -> egui::Response {
    let label = format!("{what} vorhören");
    let r = ui
        .put(
            rect,
            egui::Button::new(egui::RichText::new(egui_phosphor::regular::SPEAKER_HIGH).size(12.0))
                .frame(false),
        )
        .on_hover_text("Vorhören: erster Akkord auf der ersten MIDI-Spur (ohne Kit)");
    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
    r
}

/// Spielt den ersten Akkord der Folge kurz als Probeton (Note an/aus auf die erste MIDI-Spur
/// ohne Kit). Ohne solche Spur bleibt es still.
fn preview(state: &mut AppState, q: &woa_model::chords::ChordProgression, key: u8) {
    let Some(track) = state
        .project()
        .tracks
        .iter()
        .find(|t| t.kind == woa_model::TrackKind::Midi && !t.sound.is_some_and(|s| s.drums))
        .map(|t| t.id)
    else {
        return;
    };
    let notes = woa_model::chords::generate(q, key, 1).chords.notes;
    let mut pitches: Vec<u8> = notes
        .iter()
        .filter(|n| n.beat < 1e-6)
        .map(|n| n.pitch)
        .collect();
    pitches.dedup();
    state.probeton.tap(
        track,
        &pitches,
        96,
        std::time::Instant::now(),
        std::time::Duration::ZERO,
    );
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

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::kittest::Queryable;

    #[test]
    fn chord_row_preview_sends_note_on_to_a_midi_track() {
        let tmp = tempfile::tempdir().expect("tmp");
        let mut state = AppState::new(tmp.path().join("rec"), std::time::Instant::now());
        let track = state.add_midi_track().expect("track");
        let hint = CHORD_PROGRESSIONS[0].hint;
        let mut h = egui_kittest::Harness::builder()
            .with_size(egui::vec2(400.0, 900.0))
            .build_ui_state(
                |ui, s: &mut (AppState, AkkordFenster)| {
                    if !crate::theme::install_fonts(ui.ctx()) {
                        s.1.body(ui, &mut s.0, 0.0);
                    }
                },
                (state, AkkordFenster::default()),
            );
        h.run_steps(2);
        h.get_by_label(&format!("{hint} vorhören")).click();
        h.run_steps(1);
        let got = h.state_mut().0.probeton.take(std::time::Instant::now());
        let on: Vec<_> = got
            .iter()
            .filter(|p| matches!(p, crate::probeton::Probe::NoteOn { .. }))
            .collect();
        assert_eq!(on.len(), 3, "Dreiklang: {got:?}");
        assert!(on.iter().all(|p| p.track() == track));
    }
}
