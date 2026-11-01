//! Jingle-FX im Browser (Paket J8, Issue #283; seit Design System 2 kein Fenster mehr): Riser, Whoosh oder Downlifter aus Rauschen und
//! Filter-Sweep, als Audio-Clip am Abspielkopf. Derselbe Klang wie `generate_fx` über MCP.

use eframe::egui;
use woa_agent::audio::AgentFxKind;
use woa_model::TrackId;

use crate::state::AppState;
use crate::theme;

/// Zustand des FX-Reiters.
pub struct FxFenster {
    kind: AgentFxKind,
    beats: u32,
}

impl Default for FxFenster {
    fn default() -> Self {
        Self {
            kind: AgentFxKind::Riser,
            beats: 4,
        }
    }
}

/// Hüllkurve der Form an Stelle `t` (0..1), für die Formkarte.
fn shape(kind: AgentFxKind, t: f32) -> f32 {
    match kind {
        AgentFxKind::Riser => t * t,
        AgentFxKind::Whoosh => (-((t - 0.5) * 4.0).powi(2)).exp(),
        AgentFxKind::Downlifter => (1.0 - t) * (1.0 - t),
    }
}

impl FxFenster {
    /// Inhalt des Browser-Reiters „FX“; `track` ist die gewählte Spur, `at_beats` der
    /// Taktanfang am Abspielkopf.
    pub fn body(
        &mut self,
        ui: &mut egui::Ui,
        state: &mut AppState,
        track: Option<TrackId>,
        at_beats: f64,
    ) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            let w = (ui.available_width() - 8.0) / 3.0;
            for (k, name) in AgentFxKind::ALL {
                let hint = match k {
                    AgentFxKind::Riser => "steigt an, vor dem Einsatz",
                    AgentFxKind::Whoosh => "vorbei, für Übergänge",
                    AgentFxKind::Downlifter => "fällt ab, nach dem Einsatz",
                };
                let on = self.kind == k;
                let (r, resp) = ui.allocate_exact_size(egui::vec2(w, 96.0), egui::Sense::click());
                let p = ui.painter();
                p.rect_filled(
                    r,
                    4.0,
                    if on {
                        theme::accent_soft()
                    } else {
                        theme::well()
                    },
                );
                p.rect_stroke(
                    r,
                    4.0,
                    egui::Stroke::new(1.0, if on { theme::accent() } else { theme::border() }),
                    egui::StrokeKind::Inside,
                );
                let curve = egui::Rect::from_min_size(
                    r.min + egui::vec2(6.0, 8.0),
                    egui::vec2(r.width() - 12.0, 34.0),
                );
                let pts: Vec<egui::Pos2> = (0..=24)
                    .map(|i| {
                        let t = i as f32 / 24.0;
                        egui::pos2(
                            curve.left() + t * curve.width(),
                            curve.bottom() - shape(k, t) * curve.height(),
                        )
                    })
                    .collect();
                p.add(egui::Shape::line(
                    pts,
                    egui::Stroke::new(2.0, theme::accent()),
                ));
                p.text(
                    r.left_top() + egui::vec2(7.0, 50.0),
                    egui::Align2::LEFT_TOP,
                    name,
                    egui::FontId::proportional(12.0),
                    theme::text(),
                );
                p.text(
                    r.left_top() + egui::vec2(7.0, 68.0),
                    egui::Align2::LEFT_TOP,
                    hint,
                    egui::FontId::proportional(9.5),
                    theme::text_dim(),
                );
                if resp.on_hover_text(hint).clicked() {
                    self.kind = k;
                }
            }
        });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label(theme::panel_label("LÄNGE"));
            crate::browser::chips(ui, &mut self.beats, &[1, 2, 4, 8]);
            ui.add(egui::DragValue::new(&mut self.beats).range(1..=32))
                .on_hover_text("Andere Länge, 1 bis 32 Schläge");
        });
        let secs = f64::from(self.beats) * 60.0 / state.project().tempo_bpm.max(1.0);
        ui.label(
            egui::RichText::new(format!("Schläge · {secs:.1} s"))
                .small()
                .color(theme::text_dim()),
        );
        ui.add_space(4.0);
        if crate::browser::primary(ui, &crate::browser::insert_label(at_beats))
            .on_hover_text(
                "Audio-Clip am Abspielkopf auf der gewählten Audio-Spur (sonst neue Spur)",
            )
            .clicked()
        {
            let _ = state.add_fx(track, self.kind, f64::from(self.beats), at_beats);
        }
        ui.label(
            egui::RichText::new("Länge passt zum Tempo · Strg+Z: zurück")
                .small()
                .color(theme::text_dim()),
        );
    }
}
