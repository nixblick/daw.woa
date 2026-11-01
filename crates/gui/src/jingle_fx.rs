//! Fenster „Jingle-FX“ (Paket J8, Issue #283): Riser, Whoosh oder Downlifter aus Rauschen und
//! Filter-Sweep, als Audio-Clip am Abspielkopf. Derselbe Klang wie `generate_fx` über MCP.

use eframe::egui;
use woa_agent::audio::AgentFxKind;
use woa_model::TrackId;

use crate::state::AppState;

/// Zustand des Fensters.
pub struct FxFenster {
    pub open: bool,
    kind: AgentFxKind,
    beats: u32,
}

impl Default for FxFenster {
    fn default() -> Self {
        Self {
            open: false,
            kind: AgentFxKind::Riser,
            beats: 4,
        }
    }
}

impl FxFenster {
    /// Zeigt das Fenster; `track` ist die gewählte Spur, `at_beats` der Taktanfang am Abspielkopf.
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        state: &mut AppState,
        track: Option<TrackId>,
        at_beats: f64,
    ) {
        let mut open = self.open;
        egui::Window::new("Jingle-FX")
            .open(&mut open)
            .resizable(false)
            .show(ctx, |ui| {
                for (k, name) in AgentFxKind::ALL {
                    let hint = match k {
                        AgentFxKind::Riser => "schwillt an, vor den Einsatz legen",
                        AgentFxKind::Whoosh => "Übergang, laut in der Mitte",
                        AgentFxKind::Downlifter => "fällt ab, nach dem Einsatz",
                    };
                    ui.radio_value(&mut self.kind, k, name).on_hover_text(hint);
                }
                ui.add(egui::Slider::new(&mut self.beats, 1..=32).text("Beats"));
                if ui
                    .button("Erzeugen")
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
                        .weak(),
                );
            });
        self.open = open;
    }
}
