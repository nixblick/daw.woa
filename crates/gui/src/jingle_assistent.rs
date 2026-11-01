//! Assistent „Jingle bauen“ (Paket J4, Issue #261): sechs Schritte über die vorhandenen
//! Bausteine. Vorlage (Länge, Stimmung) → Beat (J2) → Akkorde (J3) → Stimme (J5) →
//! Schluss (Stab, J8-Riser) → Fertig (Ausgabe J6). Jeder Schritt ist ein Undo-Schritt,
//! das Ergebnis ist ein normales Projekt. Neue Funktionen gibt es hier keine.

use eframe::egui;
use woa_model::chords::{CHORD_PROGRESSIONS, KEYS};
use woa_model::jingle::{LENGTHS, MOODS, seconds};
use woa_model::{ClipId, DRUM_PRESETS, MidiData, drums};

use crate::state::{AppState, JingleParts};

/// Was das App-Fenster nach einem Klick tun soll (braucht App-Zustand außerhalb von `AppState`).
#[derive(Debug, Clone, PartialEq)]
pub enum Wunsch {
    /// Neues Projekt (fragt nach dem Speichern), dann `AppState::build_jingle`.
    Anlegen { mood: usize, seconds: u32 },
    /// Clip im Drum-Raster öffnen.
    Raster(ClipId),
    /// Export-Fenster mit „Als Jingle“ öffnen.
    Export(String),
}

const SCHRITTE: [&str; 6] = ["Vorlage", "Beat", "Akkorde", "Stimme", "Schluss", "Fertig"];

/// Zustand des Assistenten.
pub struct JingleAssistent {
    pub open: bool,
    schritt: usize,
    laenge: usize,
    mood: usize,
    preset: usize,
    folge: usize,
    /// Was Schritt 1 angelegt hat; `None` nach Undo bis vor das Anlegen.
    parts: Option<JingleParts>,
    name: String,
}

impl Default for JingleAssistent {
    fn default() -> Self {
        Self {
            open: false,
            schritt: 0,
            laenge: 1,
            mood: 0,
            preset: 0,
            folge: 0,
            parts: None,
            name: "Jingle".into(),
        }
    }
}

impl JingleAssistent {
    /// Nach `AppState::build_jingle`: Gerüst merken, Beat und Folge der Stimmung vorwählen.
    pub fn angelegt(&mut self, parts: JingleParts) {
        let m = &MOODS[self.mood.min(MOODS.len() - 1)];
        self.preset = DRUM_PRESETS
            .iter()
            .position(|p| p.name == m.drums)
            .unwrap_or(0);
        self.folge = CHORD_PROGRESSIONS
            .iter()
            .position(|p| p.name == m.progression)
            .unwrap_or(0);
        self.name = format!("Jingle {}", m.name);
        self.parts = Some(parts);
        self.schritt = 1;
    }

    /// Zeigt das Fenster.
    pub fn show(&mut self, ctx: &egui::Context, state: &mut AppState) -> Option<Wunsch> {
        if self.parts.is_some_and(|p| !state.jingle_alive(&p)) {
            self.parts = None;
            self.schritt = 0;
        }
        let mut open = self.open;
        let mut out = None;
        egui::Window::new("Jingle bauen")
            .open(&mut open)
            .resizable(false)
            .default_width(380.0)
            .show(ctx, |ui| out = self.body(ui, state));
        self.open = open;
        out
    }

    fn body(&mut self, ui: &mut egui::Ui, state: &mut AppState) -> Option<Wunsch> {
        ui.horizontal_wrapped(|ui| {
            for (i, s) in SCHRITTE.iter().enumerate() {
                let enabled = i == 0 || self.parts.is_some();
                if ui
                    .add_enabled(
                        enabled,
                        egui::Button::selectable(self.schritt == i, format!("{} {s}", i + 1)),
                    )
                    .clicked()
                {
                    self.schritt = i;
                }
            }
        });
        ui.separator();
        let out = match (self.schritt, self.parts) {
            (0, _) | (_, None) => self.vorlage(ui),
            (1, Some(p)) => self.beat(ui, state, p),
            (2, Some(p)) => {
                self.akkorde(ui, state, p);
                None
            }
            (3, Some(p)) => {
                self.stimme(ui, state, p);
                None
            }
            (4, Some(p)) => {
                self.schluss(ui, state, p);
                None
            }
            (_, Some(_)) => self.fertig(ui),
        };
        ui.separator();
        ui.horizontal(|ui| {
            if ui
                .add_enabled(self.schritt > 0, egui::Button::new("Zurück"))
                .clicked()
            {
                self.schritt -= 1;
            }
            if ui
                .add_enabled(
                    self.parts.is_some() && self.schritt + 1 < SCHRITTE.len(),
                    egui::Button::new("Weiter"),
                )
                .clicked()
            {
                self.schritt += 1;
            }
            ui.label(
                egui::RichText::new("Leertaste: anhören (Loop) · Strg+Z: zurück")
                    .small()
                    .weak(),
            );
        });
        out
    }

    fn vorlage(&mut self, ui: &mut egui::Ui) -> Option<Wunsch> {
        ui.label("Wie lang?");
        ui.horizontal(|ui| {
            for (i, s) in LENGTHS.iter().enumerate() {
                ui.selectable_value(&mut self.laenge, i, format!("{s} s"));
            }
        });
        ui.label("Welche Stimmung?");
        for (i, m) in MOODS.iter().enumerate() {
            ui.selectable_value(&mut self.mood, i, format!("{} · {}", m.name, m.hint));
        }
        let m = &MOODS[self.mood.min(MOODS.len() - 1)];
        let secs = LENGTHS[self.laenge.min(LENGTHS.len() - 1)];
        let (bars, bpm) = woa_model::jingle::fit(f64::from(secs), f64::from(m.bpm));
        let minor = woa_model::chords::progression(m.progression).is_some_and(|p| p.minor);
        ui.label(
            egui::RichText::new(format!(
                "{bars} Takte bei {bpm:.1} BPM = {:.1} s · {} {} · Beat „{}“",
                seconds(bars, bpm),
                KEYS[usize::from(m.key % 12)],
                if minor { "Moll" } else { "Dur" },
                m.drums
            ))
            .small()
            .weak(),
        );
        ui.button("Jingle anlegen")
            .on_hover_text("Neues Projekt mit Drums, Akkorden, Bass, Arpeggio und Stimm-Spur")
            .clicked()
            .then_some(Wunsch::Anlegen {
                mood: self.mood,
                seconds: secs,
            })
    }

    fn beat(&mut self, ui: &mut egui::Ui, state: &mut AppState, p: JingleParts) -> Option<Wunsch> {
        let before = self.preset;
        egui::ComboBox::from_id_salt("jingle_beat")
            .selected_text(DRUM_PRESETS.get(self.preset).map_or("", |d| d.name))
            .show_ui(ui, |ui| {
                for (i, d) in DRUM_PRESETS.iter().enumerate() {
                    ui.selectable_value(&mut self.preset, i, d.name);
                }
            });
        let length = f64::from(p.bars) * 4.0;
        if before != self.preset
            && let Some(d) = DRUM_PRESETS.get(self.preset)
        {
            let midi = drums::apply_preset(&MidiData::default(), length, d, 0);
            state.jingle_notes(&p, Some(midi), None);
        }
        let mut out = None;
        ui.horizontal(|ui| {
            if ui
                .button("Würfeln")
                .on_hover_text("Zufälliger Beat aus den Presets")
                .clicked()
            {
                let seed = ui.input(|i| i.time.to_bits());
                let midi = drums::dice(&MidiData::default(), length, seed);
                state.jingle_notes(&p, Some(midi), None);
            }
            if ui
                .button("Im Raster bearbeiten")
                .on_hover_text("Schritte selbst setzen: Klick laut, Rechtsklick leise")
                .clicked()
            {
                out = Some(Wunsch::Raster(p.drums));
            }
        });
        out
    }

    fn akkorde(&mut self, ui: &mut egui::Ui, state: &mut AppState, p: JingleParts) {
        let mood = MOODS[self.mood.min(MOODS.len() - 1)].name;
        let before = self.folge;
        egui::ScrollArea::vertical()
            .max_height(220.0)
            .show(ui, |ui| {
                // Erst die Folgen der Stimmung, dann die übrigen.
                for passt in [true, false] {
                    for (i, f) in CHORD_PROGRESSIONS.iter().enumerate() {
                        if (f.mood == mood) == passt {
                            ui.selectable_value(
                                &mut self.folge,
                                i,
                                format!("{} · {}", f.name, f.hint),
                            );
                        }
                    }
                }
            });
        if before != self.folge {
            state.jingle_notes(&p, None, CHORD_PROGRESSIONS.get(self.folge));
        }
        ui.label(
            egui::RichText::new(
                "Bass und Arpeggio folgen den Akkorden; alle Töne passen zur Tonart",
            )
            .small()
            .weak(),
        );
    }

    fn stimme(&mut self, ui: &mut egui::Ui, state: &mut AppState, p: JingleParts) {
        ui.label(
            "Auf der Spur „Stimme“ aufnehmen (R) oder eine Datei aus der Bibliothek daraufziehen.",
        );
        if ui
            .button("Stimme über Bett")
            .on_hover_text("Sprachkette auf die Stimme, die Musik wird darunter leiser")
            .clicked()
        {
            state.voice_over(p.voice);
        }
    }

    fn schluss(&mut self, ui: &mut egui::Ui, state: &mut AppState, mut p: JingleParts) {
        let mut on = p.stab;
        if ui
            .checkbox(&mut on, "Schluss-Stab")
            .on_hover_text("Der letzte Takt wird ein einzelner Schlag, nichts klingt abgeschnitten")
            .changed()
        {
            let beat = state
                .project()
                .clip(p.drums)
                .and_then(|(_, c)| c.midi.clone())
                .unwrap_or_default();
            p.stab = on;
            let (drums, folge) = if on {
                (Some(beat), CHORD_PROGRESSIONS.get(self.folge))
            } else {
                // Aus: Beat und Folge neu, ohne Stab.
                let length = f64::from(p.bars) * 4.0;
                let d = DRUM_PRESETS
                    .get(self.preset)
                    .map(|d| drums::apply_preset(&MidiData::default(), length, d, 0));
                (d, CHORD_PROGRESSIONS.get(self.folge))
            };
            if state.jingle_notes(&p, drums, folge) {
                self.parts = Some(p);
            }
        }
        if ui
            .button("Riser davor")
            .on_hover_text("Rauschen, das zum Schluss hin anschwillt (Jingle-FX)")
            .clicked()
        {
            let start = f64::from(p.bars.saturating_sub(2)) * 4.0;
            state.add_fx(None, woa_agent::audio::AgentFxKind::Riser, 4.0, start);
        }
    }

    fn fertig(&mut self, ui: &mut egui::Ui) -> Option<Wunsch> {
        ui.label("Anhören, dann als Jingle ausgeben: Lautheit und Limiter, Jingle-Ordner, Bibliothek, wahlweise Cart.");
        ui.horizontal(|ui| {
            ui.label("Name");
            ui.text_edit_singleline(&mut self.name);
        });
        ui.button("Als Jingle exportieren …")
            .clicked()
            .then(|| Wunsch::Export(self.name.trim().to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::kittest::Queryable;

    struct Probe {
        state: AppState,
        w: JingleAssistent,
        out: Vec<Wunsch>,
    }

    fn harness(dir: &std::path::Path) -> egui_kittest::Harness<'static, Probe> {
        let w = JingleAssistent {
            open: true,
            ..Default::default()
        };
        let probe = Probe {
            state: AppState::new(dir.join("recovery"), std::time::Instant::now()),
            w,
            out: Vec::new(),
        };
        egui_kittest::Harness::builder()
            .with_size(egui::vec2(800.0, 700.0))
            .build_ui_state(
                |ui, p: &mut Probe| {
                    if let Some(x) = p.w.show(ui.ctx(), &mut p.state) {
                        p.out.push(x);
                    }
                },
                probe,
            )
    }

    #[test]
    fn walks_all_six_steps_to_the_jingle_export() {
        let tmp = tempfile::tempdir().unwrap();
        let mut h = harness(tmp.path());
        h.run_steps(2);
        h.get_by_label("Party · treibend, für Abend und Wochenende")
            .click();
        h.run_steps(2);
        h.get_by_label("Jingle anlegen").click();
        h.run_steps(2);
        let Some(Wunsch::Anlegen { mood, seconds }) = h.state_mut().out.pop() else {
            panic!("kein Anlegen-Wunsch");
        };
        assert_eq!((mood, seconds), (3, 10));
        let p = h.state_mut();
        let parts = p.state.build_jingle(&MOODS[mood], seconds).unwrap();
        p.w.angelegt(parts);
        h.run_steps(2);
        assert_eq!(h.state().w.schritt, 1, "weiter zum Beat");
        assert_eq!(DRUM_PRESETS[h.state().w.preset].name, "Four on the Floor");
        for _ in 0..4 {
            h.get_by_label("Weiter").click();
            h.run_steps(2);
        }
        assert_eq!(h.state().w.schritt, 5);
        h.get_by_label("Als Jingle exportieren …").click();
        h.run_steps(2);
        assert_eq!(h.state().out, vec![Wunsch::Export("Jingle Party".into())]);
    }

    #[test]
    fn undo_before_the_build_goes_back_to_step_one() {
        let tmp = tempfile::tempdir().unwrap();
        let mut h = harness(tmp.path());
        let p = h.state_mut();
        let parts = p.state.build_jingle(&MOODS[0], 5).unwrap();
        p.w.angelegt(parts);
        p.state.undo();
        h.run_steps(2);
        assert_eq!(h.state().w.schritt, 0);
        assert!(h.state().w.parts.is_none());
    }
}
