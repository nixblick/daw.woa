//! Fenster „Akkordfolge“ (Paket J3, Issue #261): Folge und Tonart wählen, drei MIDI-Spuren
//! entstehen. Ein Tonartwechsel danach transponiert die drei zuletzt erzeugten Clips.

use eframe::egui;
use woa_model::ClipId;
use woa_model::chords::{CHORD_PROGRESSIONS, KEYS, key_shift};

use crate::state::AppState;

/// Zustand des Fensters.
pub struct AkkordFenster {
    pub open: bool,
    folge: usize,
    key: u8,
    bars: u32,
    /// Die zuletzt erzeugten Clips und ihre Tonart.
    last: Option<(Vec<ClipId>, u8)>,
}

impl Default for AkkordFenster {
    fn default() -> Self {
        Self {
            open: false,
            folge: 0,
            key: 0,
            bars: 4,
            last: None,
        }
    }
}

impl AkkordFenster {
    /// Zeigt das Fenster; `at_beats` ist der Taktanfang am Abspielkopf.
    pub fn show(&mut self, ctx: &egui::Context, state: &mut AppState, at_beats: f64) {
        let mut open = self.open;
        egui::Window::new("Akkordfolge")
            .open(&mut open)
            .resizable(false)
            .show(ctx, |ui| self.body(ui, state, at_beats));
        self.open = open;
    }

    fn body(&mut self, ui: &mut egui::Ui, state: &mut AppState, at_beats: f64) {
        let Some(p) = CHORD_PROGRESSIONS.get(self.folge) else {
            return;
        };
        egui::ComboBox::from_id_salt("akkord_folge")
            .selected_text(format!("{} · {}", p.name, p.hint))
            .show_ui(ui, |ui| {
                for (i, q) in CHORD_PROGRESSIONS.iter().enumerate() {
                    ui.selectable_value(&mut self.folge, i, format!("{} · {}", q.name, q.hint));
                }
            });
        let mode = if p.minor { "Moll" } else { "Dur" };
        let before = self.key;
        egui::ComboBox::from_id_salt("akkord_tonart")
            .selected_text(format!("{} {mode}", KEYS[usize::from(self.key % 12)]))
            .show_ui(ui, |ui| {
                for (i, k) in KEYS.iter().enumerate() {
                    ui.selectable_value(&mut self.key, i as u8, format!("{k} {mode}"));
                }
            })
            .response
            .on_hover_text("Nach dem Erzeugen verschiebt ein Wechsel alle drei Clips");
        if before != self.key
            && let Some((clips, from)) = &self.last
        {
            if state.transpose_clips(clips, key_shift(*from, self.key)) {
                self.last = Some((clips.clone(), self.key));
            } else {
                self.last = None;
            }
        }
        ui.add(egui::Slider::new(&mut self.bars, 1..=16).text("Takte"));
        if ui
            .button("Erzeugen")
            .on_hover_text("Drei MIDI-Spuren: Akkorde, Bass, Arpeggio, ab dem Takt am Abspielkopf")
            .clicked()
            && let Some(made) = state.add_chord_tracks(p, self.key, self.bars, at_beats)
        {
            self.last = Some((made.iter().map(|m| m.1).collect(), self.key));
        }
        ui.label(
            egui::RichText::new("Alle Töne passen zur Tonart · Strg+Z: zurück")
                .small()
                .weak(),
        );
    }
}
