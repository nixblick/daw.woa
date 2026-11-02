//! Drum-Raster (Design `DrumGrid`, Paket 13 / Design-Runde 3): auf Kit-Spuren statt der
//! Notenfläche. Kick oben, 16tel in Vierergruppen mit Zählzeiten, Zeilen mit Farbkante, Name und
//! Vorhören, Muster aus dem Beat-Katalog, Länge, Dichte, Würfel, Lauflicht. Klick laut (nochmal
//! aus), Umschalt+Klick leise, Rechtsklick löschen. Es bearbeitet denselben Clip wie die
//! [`PianoRoll`] und gehört zu ihrem Zustand; jede Änderung ist ein `SetNotes`.

use eframe::egui;
use egui::{Color32, Rect, Sense, Stroke, pos2, vec2};
use woa_model::MidiData;

use super::piano_roll::{Palette, PianoRoll, icon_button, seg};
use crate::state::AppState;
use crate::theme;

/// Drum-Raster: Was ein Klick aus dem Schritt macht. Klick: leer/leise → laut, laut → leer;
/// Umschalt+Klick: leise (leise → leer); Rechtsklick: leer.
pub fn drum_click(now: Option<u8>, shift: bool, right: bool) -> Option<u8> {
    use woa_model::drums::{LOUD, SOFT};
    match (right, shift, now) {
        (true, _, _) => None,
        (false, true, Some(v)) if v <= SOFT => None,
        (false, true, _) => Some(SOFT),
        (false, false, Some(v)) if v > SOFT => None,
        (false, false, _) => Some(LOUD),
    }
}

/// Spurfarben-Index je Drum-Zeile nach Design `DrumGrid` (Kick 1, Becken 2, Clap 3, …).
fn drum_color(pitch: u8) -> Color32 {
    let i = match pitch {
        36 => 0,
        49 => 1,
        39 => 2,
        42 => 3,
        46 => 4,
        _ => 5,
    };
    theme::track_color(i)
}

impl PianoRoll {
    /// Noten | Raster auf Kit-Spuren.
    pub(super) fn kit_switch(&mut self, ui: &mut egui::Ui, grid: bool) {
        ui.spacing_mut().item_spacing.x = 0.0;
        if seg(ui, !grid, "Noten", "Noten in der Piano-Roll bearbeiten") {
            self.drum_grid = Some(false);
        }
        if seg(ui, grid, "Raster", "16 Schritte je Drum, dieselben Noten") {
            self.drum_grid = Some(true);
        }
        ui.spacing_mut().item_spacing.x = 6.0;
    }

    /// Kopf des Drum-Rasters (Design `DrumGrid`): Noten | Raster, Muster, Länge, Dichte, Würfel.
    pub(super) fn drum_head(
        &mut self,
        ui: &mut egui::Ui,
        state: &mut AppState,
        midi: &MidiData,
        length: f64,
    ) {
        use egui_phosphor::regular as ic;
        use woa_model::{DRUM_PRESETS, drums};
        ui.horizontal(|ui| {
            self.kit_switch(ui, true);
            ui.separator();
            let name = self
                .preset
                .and_then(|i| DRUM_PRESETS.get(i))
                .map_or("Muster", |p| p.name);
            let mut chosen = None;
            egui::ComboBox::from_id_salt(("drum_preset", self.clip.0))
                .selected_text(format!("{} {name}", ic::GRID_FOUR))
                .width(150.0)
                .show_ui(ui, |ui| {
                    for (i, p) in DRUM_PRESETS.iter().enumerate() {
                        if ui
                            .selectable_label(self.preset == Some(i), p.name)
                            .clicked()
                        {
                            chosen = Some(i);
                        }
                    }
                })
                .response
                .on_hover_text("Muster aus dem Beat-Katalog");
            let bars = (length / 4.0).round().max(1.0) as u32;
            let mut new_bars = bars;
            egui::ComboBox::from_id_salt(("drum_bars", self.clip.0))
                .selected_text(if bars == 1 {
                    "1 Takt".to_owned()
                } else {
                    format!("{bars} Takte")
                })
                .width(80.0)
                .show_ui(ui, |ui| {
                    for b in [1u32, 2, 4, 8] {
                        let t = if b == 1 {
                            "1 Takt".to_owned()
                        } else {
                            format!("{b} Takte")
                        };
                        ui.selectable_value(&mut new_bars, b, t);
                    }
                })
                .response
                .on_hover_text("Länge des Clips in Takten");
            let mut length = length;
            if new_bars != bars
                && let Some((_, c)) = state.project().clip(self.clip)
            {
                let (start, offset) = (c.start_beats, c.offset_beats);
                length = f64::from(new_bars) * 4.0;
                state.trim_clip(self.clip, start, length, offset);
                if chosen.is_none() {
                    chosen = self.preset;
                }
            }
            ui.separator();
            ui.spacing_mut().item_spacing.x = 0.0;
            let before = self.density;
            for (d, label, tip) in [
                (-1i8, ic::MINUS, "Weniger: nur laute Schläge"),
                (0, ic::EQUALS, "Wie das Muster"),
                (1, ic::PLUS, "Mehr: dichte Hi-Hat, leise ergänzt"),
            ] {
                if icon_button(ui, self.density == d, label, tip, tip).clicked() {
                    self.density = d;
                }
            }
            ui.spacing_mut().item_spacing.x = 6.0;
            if before != self.density && chosen.is_none() {
                chosen = self.preset;
            }
            if let Some(i) = chosen
                && let Some(p) = DRUM_PRESETS.get(i)
            {
                self.preset = Some(i);
                let now = Self::notes(state, self.clip).map_or_else(|| midi.clone(), |n| n.0);
                state.set_notes(
                    self.clip,
                    drums::apply_preset(&now, length, p, self.density),
                );
            }
            if icon_button(
                ui,
                false,
                ic::DICE_FIVE,
                "Würfeln",
                "Würfeln: zufälliger Beat aus einem Muster mit kleinen Abweichungen",
            )
            .clicked()
            {
                self.dice_seed = self.dice_seed.wrapping_add(1);
                let seed = self.dice_seed
                    ^ (std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map_or(0, |d| d.as_nanos() as u64));
                state.set_notes(self.clip, drums::dice(midi, length, seed));
                self.preset = None;
            }
        });
    }

    /// Das Raster (Design `DrumGrid`): Kick oben, 16tel in Vierergruppen, Zählzeiten darüber,
    /// laut/leise, Lauflicht. Jede Änderung ist ein `SetNotes`.
    pub(super) fn drum_editor(
        &mut self,
        ui: &mut egui::Ui,
        state: &mut AppState,
        midi: &MidiData,
        length: f64,
    ) {
        use woa_model::{DRUM_ROWS, drums};
        const NAME_W: f32 = 128.0;
        const HEAD_H: f32 = 16.0;
        let pal = Palette::of(ui, self.color);
        let total = drums::steps(length);
        let avail = ui.available_width() - NAME_W;
        let cell = (avail / total.min(drums::STEPS_PER_BAR * 2) as f32).clamp(16.0, 120.0);
        let row_h = 30.0;
        // Kick oben: DRUM_ROWS umgekehrt (die Reihenfolge dort gilt für Muster und MCP).
        let order: Vec<&woa_model::DrumRow> = DRUM_ROWS.iter().rev().collect();
        let run = self.playhead.and_then(|ph| {
            let rel = ph - self.origin;
            (rel >= 0.0 && rel < length).then(|| (rel * 4.0).floor() as usize)
        });
        egui::ScrollArea::horizontal()
            .id_salt(("drum_grid", self.clip.0))
            .show(ui, |ui| {
                // Zählzeiten.
                let (head, _) = ui.allocate_exact_size(
                    vec2(NAME_W + cell * total as f32, HEAD_H),
                    Sense::hover(),
                );
                for step in 0..total {
                    let x = head.left() + NAME_W + (step as f32 + 0.5) * cell;
                    let beat = step % 4 == 0;
                    ui.painter().text(
                        pos2(x, head.center().y),
                        egui::Align2::CENTER_CENTER,
                        if beat {
                            format!("{}", (step / 4) % 4 + 1)
                        } else {
                            "·".to_owned()
                        },
                        egui::FontId::monospace(10.0),
                        if beat { theme::text() } else { pal.text },
                    );
                }
                for row in &order {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                        let c = drum_color(row.pitch);
                        let (name, _) = ui.allocate_exact_size(vec2(NAME_W, row_h), Sense::hover());
                        let painter = ui.painter();
                        let nr = name.shrink2(vec2(0.0, 2.0));
                        painter.rect_filled(nr, 3.0, theme::surface());
                        painter.rect_filled(
                            Rect::from_min_size(nr.min, vec2(3.0, nr.height())),
                            1.0,
                            c,
                        );
                        painter.text(
                            pos2(nr.left() + 10.0, nr.center().y),
                            egui::Align2::LEFT_CENTER,
                            row.name,
                            egui::FontId::new(12.0, egui::FontFamily::Name("ui-bold".into())),
                            theme::text(),
                        );
                        let sp = Rect::from_center_size(
                            pos2(nr.right() - 14.0, nr.center().y),
                            vec2(22.0, 22.0),
                        );
                        let listen = ui
                            .put(
                                sp,
                                egui::Button::new(
                                    egui::RichText::new(egui_phosphor::regular::SPEAKER_HIGH)
                                        .size(12.0),
                                )
                                .frame(false),
                            )
                            .on_hover_text(format!("{} vorhören", row.name));
                        let label = format!("{} vorhören", row.name);
                        listen.widget_info(|| {
                            egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label)
                        });
                        if listen.clicked()
                            && let Some(t) = self.track
                        {
                            state.probeton.tap(
                                t,
                                &[row.pitch],
                                drums::LOUD,
                                std::time::Instant::now(),
                                std::time::Duration::ZERO,
                            );
                        }
                        let (steps, resp) = ui
                            .allocate_exact_size(vec2(cell * total as f32, row_h), Sense::click());
                        let painter = ui.painter_at(steps);
                        for step in 0..total {
                            let x = steps.left() + step as f32 * cell;
                            let r = Rect::from_min_size(pos2(x, steps.top()), vec2(cell, row_h))
                                .shrink(2.0);
                            let group = (step / 4) % 2 == 1;
                            let empty = if group {
                                theme::mix(theme::surface(), pal.bg, 0.35)
                            } else {
                                theme::surface()
                            };
                            match drums::step_velocity(midi, row.pitch, step) {
                                Some(v) if v > drums::SOFT => {
                                    painter.rect_filled(r.expand(1.0), 4.0, c.gamma_multiply(0.35));
                                    painter.rect_filled(r, 3.0, c);
                                }
                                Some(_) => {
                                    painter.rect_filled(r, 3.0, theme::mix(empty, c, 0.5));
                                    painter.rect_stroke(
                                        r,
                                        3.0,
                                        Stroke::new(1.0, c),
                                        egui::StrokeKind::Inside,
                                    );
                                }
                                None => {
                                    painter.rect_filled(r, 3.0, empty);
                                    painter.rect_stroke(
                                        r,
                                        3.0,
                                        Stroke::new(1.0, theme::border()),
                                        egui::StrokeKind::Inside,
                                    );
                                }
                            }
                            if run == Some(step) {
                                painter.rect_stroke(
                                    r.expand(1.0),
                                    3.0,
                                    Stroke::new(2.0, theme::text()),
                                    egui::StrokeKind::Outside,
                                );
                            }
                        }
                        let resp = resp.on_hover_text(
                            "Klick: laut (nochmal: aus) · Umschalt+Klick: leise · \
                             Rechtsklick: löschen · Strg+Z: zurück",
                        );
                        let right = resp.secondary_clicked();
                        if (resp.clicked() || right)
                            && let Some(p) = resp.interact_pointer_pos()
                        {
                            let step = ((p.x - steps.left()) / cell).floor();
                            if step >= 0.0 && (step as usize) < total {
                                let step = step as usize;
                                let shift = ui.input(|i| i.modifiers.shift);
                                let now = drums::step_velocity(midi, row.pitch, step);
                                let next = drum_click(now, shift, right);
                                state.set_notes(
                                    self.clip,
                                    drums::set_step(midi, row.pitch, step, next),
                                );
                                if self.probe
                                    && let (Some(v), Some(t)) = (next, self.track)
                                {
                                    state.probeton.tap(
                                        t,
                                        &[row.pitch],
                                        v,
                                        std::time::Instant::now(),
                                        std::time::Duration::ZERO,
                                    );
                                }
                            }
                        }
                    });
                }
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drum_clicks_follow_the_design() {
        use woa_model::drums::{LOUD, SOFT};
        assert_eq!(drum_click(None, false, false), Some(LOUD));
        assert_eq!(drum_click(Some(LOUD), false, false), None);
        assert_eq!(drum_click(Some(SOFT), false, false), Some(LOUD));
        assert_eq!(drum_click(None, true, false), Some(SOFT));
        assert_eq!(drum_click(Some(LOUD), true, false), Some(SOFT));
        assert_eq!(drum_click(Some(SOFT), true, false), None);
        assert_eq!(drum_click(Some(LOUD), false, true), None);
    }
}
