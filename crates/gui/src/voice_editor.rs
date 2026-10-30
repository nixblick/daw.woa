//! Voice-Track-Editor (Paket 30): die Wellenformen von Titel A (Ausklang), Stimme und Titel B
//! (Anfang) auf einer Zeitachse, deren Nullpunkt der Beginn der Überblendung ist. Die Stimme
//! lässt sich mit der Maus verschieben, darunter steht die Absenkungskurve der Musik. Vorhören
//! rendert A, Stimme, B offline und spielt das Ergebnis nur im Kopfhörer (PFL), nie auf Sendung.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use eframe::egui;
use woa_radio::Cues;
use woa_radio::voicetrack::{MAX_OFFSET, VoiceParams, duck_curve, suggest_offset};

use crate::theme;

/// Rate, in der die Wellenformen gelesen werden (reicht für die Anzeige).
const PEAK_RATE: u32 = 8_000;
/// Balken je Sekunde.
const BINS_PER_SECOND: f64 = 20.0;
/// So viele Sekunden von B (ab dem Beginn) werden gelesen.
const B_HEAD_SECONDS: f64 = 60.0;

/// Was der Editor nach einem Bild vom Radio-Layout will.
#[derive(Debug, Clone, PartialEq)]
pub enum VoiceAction {
    Apply(usize, VoiceParams),
    Confirm(usize),
    Preview(usize),
    StopPreview,
}

/// Was der Editor über die drei Dateien weiß (aus der Ansicht des Radios).
#[derive(Debug, Clone)]
pub struct VoiceInput {
    pub index: usize,
    pub title: String,
    pub voice_path: PathBuf,
    pub voice_seconds: Option<f64>,
    pub a_path: Option<PathBuf>,
    pub a_title: String,
    pub a_seconds: Option<f64>,
    pub a_cues: Cues,
    pub b_path: Option<PathBuf>,
    pub b_title: String,
    pub b_seconds: Option<f64>,
    pub b_cues: Cues,
    pub crossfade: f64,
    pub params: VoiceParams,
    pub check: bool,
}

/// Balken der Wellenform, Dauer in Sekunden.
#[derive(Debug, Clone, Default)]
struct Wave {
    peaks: Vec<f32>,
    seconds: f64,
}

type Loaded = Result<[Wave; 3], String>;

fn wave_of(path: &std::path::Path, max_seconds: Option<f64>) -> Result<Wave, String> {
    let s = woa_radio::audio::load_stereo(path, PEAK_RATE)?;
    let seconds = s.frames() as f64 / f64::from(PEAK_RATE);
    let keep = max_seconds
        .map_or(s.frames(), |m| (m * f64::from(PEAK_RATE)) as usize)
        .min(s.frames());
    let bins = (keep as f64 / f64::from(PEAK_RATE) * BINS_PER_SECOND).ceil() as usize;
    Ok(Wave {
        peaks: crate::cue_editor::peaks(&s.left[..keep], &s.right[..keep], bins),
        seconds,
    })
}

/// Ein offener Editor.
pub struct VoiceEditor {
    pub input: VoiceInput,
    /// Entwurf; erst „Übernehmen“ schickt ihn an die Sendung.
    pub draft: VoiceParams,
    waves: Option<[Wave; 3]>,
    loading: Option<Receiver<Loaded>>,
    error: Option<String>,
}

impl VoiceEditor {
    pub fn open(input: VoiceInput) -> Self {
        let (tx, rx) = channel();
        let (v, a, b) = (
            input.voice_path.clone(),
            input.a_path.clone(),
            input.b_path.clone(),
        );
        let spawned = std::thread::Builder::new()
            .name("woa-voice-peaks".into())
            .spawn(move || {
                let r = (|| {
                    let voice = wave_of(&v, None)?;
                    let a = a.as_ref().map(|p| wave_of(p, None)).transpose()?;
                    let b = b
                        .as_ref()
                        .map(|p| wave_of(p, Some(B_HEAD_SECONDS)))
                        .transpose()?;
                    Ok([a.unwrap_or_default(), voice, b.unwrap_or_default()])
                })();
                let _ = tx.send(r);
            });
        let error = spawned.err().map(|e| format!("Wellenform: {e}"));
        let draft = input.params;
        Self {
            input,
            draft,
            waves: None,
            loading: Some(rx),
            error,
        }
    }

    /// Länge der Stimme in Sekunden (gemessen, sonst aus der Datei).
    pub fn voice_len(&self) -> f64 {
        self.input
            .voice_seconds
            .or_else(|| self.waves.as_ref().map(|w| w[1].seconds))
            .unwrap_or(0.0)
    }

    fn xf(&self) -> f64 {
        self.input.crossfade.max(0.0)
    }

    /// Sekunden von Beginn der Überblendung bis zum Ramp-Punkt in B.
    fn ramp_rel(&self) -> Option<f64> {
        self.input
            .b_cues
            .ramp
            .map(|r| r - self.input.b_cues.start())
    }

    /// Schiebt die Stimme auf `offset` Sekunden (auf 0,05 s gerundet, im erlaubten Bereich).
    pub fn set_offset(&mut self, offset: f64) {
        self.draft.offset = ((offset * 20.0).round() / 20.0).clamp(-MAX_OFFSET, MAX_OFFSET);
    }

    /// Setzt den Beginn der Stimme so, dass sie am Ramp-Punkt von B (sonst 2 s nach dem
    /// Beginn von B) endet.
    pub fn suggest(&mut self) {
        let o = suggest_offset(self.voice_len(), self.ramp_rel());
        self.set_offset(o);
    }

    /// Weicht der Entwurf von den gespeicherten Werten ab?
    pub fn dirty(&self) -> bool {
        self.draft != self.input.params
    }

    /// Nach einem Eingriff von außen (Undo, anderer Wert) den Stand der Sendung übernehmen.
    pub fn sync(&mut self, params: VoiceParams, check: bool) {
        if params != self.input.params {
            self.draft = params;
            self.input.params = params;
        }
        self.input.check = check;
    }

    fn poll(&mut self) {
        let Some(rx) = &self.loading else { return };
        match rx.try_recv() {
            Ok(Ok(w)) => {
                self.waves = Some(w);
                self.loading = None;
            }
            Ok(Err(e)) => {
                self.error = Some(e);
                self.loading = None;
            }
            Err(TryRecvError::Disconnected) => self.loading = None,
            Err(TryRecvError::Empty) => {}
        }
    }

    /// Sichtbarer Ausschnitt der Zeitachse (Sekunden ab Beginn der Überblendung).
    fn window(&self) -> (f64, f64) {
        let d = self.draft;
        let from = (d.offset - 3.0).min(-12.0);
        let to = (d.offset + self.voice_len() + d.release + 2.0)
            .max(self.xf() + 12.0)
            .min(from + 150.0);
        (from, to)
    }

    pub fn ui(
        &mut self,
        ctx: &egui::Context,
        open: &mut bool,
        pfl: (bool, bool),
    ) -> Vec<VoiceAction> {
        self.poll();
        if self.loading.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        let mut out = Vec::new();
        egui::Window::new(format!("Voice-Track: {}", self.input.title))
            .id(egui::Id::new("woa-voice-editor"))
            .open(open)
            .default_width(720.0)
            .show(ctx, |ui| self.body(ui, pfl, &mut out));
        out
    }

    fn lane(
        painter: &egui::Painter,
        rect: egui::Rect,
        wave: &Wave,
        t_of_sec: impl Fn(f64) -> f64,
        x_of: &impl Fn(f64) -> f32,
        color: egui::Color32,
    ) {
        let w = (rect.width() / 400.0).max(1.0);
        for (i, v) in wave.peaks.iter().enumerate() {
            let t = t_of_sec((i as f64 + 0.5) / BINS_PER_SECOND);
            let x = x_of(t);
            if x < rect.left() || x > rect.right() {
                continue;
            }
            let h = v * rect.height() * 0.46;
            painter.line_segment(
                [
                    egui::pos2(x, rect.center().y - h),
                    egui::pos2(x, rect.center().y + h),
                ],
                egui::Stroke::new(w, color),
            );
        }
    }

    fn body(
        &mut self,
        ui: &mut egui::Ui,
        (pfl_available, pfl_playing): (bool, bool),
        out: &mut Vec<VoiceAction>,
    ) {
        let (from, to) = self.window();
        let span = (to - from).max(1.0);
        let width = ui.available_width().max(300.0);
        let (rect, resp) =
            ui.allocate_exact_size(egui::vec2(width, 230.0), egui::Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 4.0, theme::WELL);
        let x_of = |t: f64| rect.left() + ((t - from) / span) as f32 * rect.width();
        let lane_h = 64.0;
        let a_lane = egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), lane_h));
        let v_lane = egui::Rect::from_min_size(
            rect.min + egui::vec2(0.0, lane_h + 6.0),
            egui::vec2(rect.width(), lane_h),
        );
        let d_lane = egui::Rect::from_min_size(
            rect.min + egui::vec2(0.0, 2.0 * (lane_h + 6.0)),
            egui::vec2(rect.width(), rect.height() - 2.0 * (lane_h + 6.0)),
        );
        for (r, name) in [
            (a_lane, "Titel A  ·  Titel B"),
            (v_lane, "Stimme (ziehen)"),
            (d_lane, "Musik-Pegel (Ducking)"),
        ] {
            painter.rect_stroke(
                r,
                2.0,
                egui::Stroke::new(1.0, theme::GRID),
                egui::StrokeKind::Inside,
            );
            painter.text(
                r.left_top() + egui::vec2(4.0, 2.0),
                egui::Align2::LEFT_TOP,
                name,
                egui::FontId::proportional(10.0),
                theme::DIM,
            );
        }
        // Sekundenraster, 0 = Beginn der Überblendung.
        let mut t = from.ceil();
        while t <= to {
            let x = x_of(t);
            let zero = t == 0.0;
            painter.line_segment(
                [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
                egui::Stroke::new(
                    if zero { 1.5 } else { 0.5 },
                    if zero { theme::ACCENT } else { theme::GRID },
                ),
            );
            if (t as i64) % 5 == 0 {
                painter.text(
                    egui::pos2(x + 2.0, rect.bottom() - 1.0),
                    egui::Align2::LEFT_BOTTOM,
                    format!("{t:.0}"),
                    egui::FontId::proportional(9.0),
                    theme::DIM,
                );
            }
            t += 1.0;
        }
        // Überblendung A → B.
        let xf = self.xf();
        painter.rect_filled(
            egui::Rect::from_x_y_ranges(x_of(0.0)..=x_of(xf), a_lane.y_range()),
            0.0,
            theme::mix(theme::ACCENT, theme::WELL, 0.82),
        );
        if let Some(w) = &self.waves {
            let a_end = self
                .input
                .a_seconds
                .map_or(w[0].seconds, |s| self.input.a_cues.end(s));
            Self::lane(
                &painter,
                a_lane,
                &w[0],
                |p| p - a_end + xf,
                &x_of,
                theme::CUE_CYAN,
            );
            let b_start = self.input.b_cues.start();
            Self::lane(
                &painter,
                a_lane,
                &w[2],
                |p| p - b_start,
                &x_of,
                theme::OK_GREEN,
            );
            Self::lane(
                &painter,
                v_lane,
                &w[1],
                |p| p + self.draft.offset,
                &x_of,
                theme::ACCENT,
            );
        } else if self.loading.is_some() {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "Wellenformen werden gelesen …",
                egui::FontId::proportional(13.0),
                theme::DIM,
            );
        }
        // Ramp-Punkt von B.
        if let Some(r) = self.ramp_rel() {
            let x = x_of(r);
            painter.line_segment(
                [egui::pos2(x, a_lane.top()), egui::pos2(x, a_lane.bottom())],
                egui::Stroke::new(1.5, theme::OK_GREEN),
            );
            painter.text(
                egui::pos2(x + 3.0, a_lane.bottom() - 2.0),
                egui::Align2::LEFT_BOTTOM,
                "Ramp B",
                egui::FontId::proportional(10.0),
                theme::OK_GREEN,
            );
        }
        // Stimmenband und Ziehen.
        let vlen = self.voice_len();
        let band = egui::Rect::from_x_y_ranges(
            x_of(self.draft.offset)..=x_of(self.draft.offset + vlen),
            v_lane.y_range(),
        );
        painter.rect_stroke(
            band,
            2.0,
            egui::Stroke::new(1.5, theme::ACCENT),
            egui::StrokeKind::Inside,
        );
        if resp.dragged()
            && resp
                .interact_pointer_pos()
                .is_some_and(|p| p.y < d_lane.top())
        {
            let dt = f64::from(resp.drag_delta().x / rect.width()) * span;
            self.set_offset(self.draft.offset + dt);
        }
        // Absenkungskurve.
        let curve = duck_curve(&self.draft, vlen, from, to, span / 400.0);
        let pts: Vec<egui::Pos2> = curve
            .iter()
            .map(|&(t, g)| {
                egui::pos2(
                    x_of(t),
                    d_lane.bottom() - 4.0 - g.clamp(0.0, 1.0) * (d_lane.height() - 10.0),
                )
            })
            .collect();
        painter.add(egui::Shape::line(
            pts,
            egui::Stroke::new(2.0, theme::ON_AIR),
        ));

        ui.add_space(4.0);
        egui::Grid::new("woa-voice-values")
            .num_columns(4)
            .show(ui, |ui| {
                let d = &mut self.draft;
                ui.label("Beginn der Stimme");
                ui.add(
                    egui::DragValue::new(&mut d.offset)
                        .speed(0.05)
                        .range(-MAX_OFFSET..=MAX_OFFSET)
                        .suffix(" s")
                        .max_decimals(2),
                )
                .on_hover_text("Negativ: im Ausklang von Titel A, null: Beginn der Überblendung");
                ui.label("Stimme lauter/leiser");
                ui.add(
                    egui::DragValue::new(&mut d.gain_db)
                        .speed(0.1)
                        .range(-24.0..=12.0)
                        .suffix(" dB")
                        .max_decimals(1),
                );
                ui.end_row();
                ui.label("Musik absenken um");
                ui.add(
                    egui::DragValue::new(&mut d.duck_db)
                        .speed(0.1)
                        .range(-40.0..=0.0)
                        .suffix(" dB")
                        .max_decimals(1),
                );
                ui.label("Rampen vor / nach");
                ui.horizontal(|ui| {
                    ui.add(
                        egui::DragValue::new(&mut d.attack)
                            .speed(0.01)
                            .range(0.0..=10.0)
                            .suffix(" s")
                            .max_decimals(2),
                    );
                    ui.add(
                        egui::DragValue::new(&mut d.release)
                            .speed(0.01)
                            .range(0.0..=10.0)
                            .suffix(" s")
                            .max_decimals(2),
                    );
                });
                ui.end_row();
            });
        ui.label(
            egui::RichText::new(format!(
                "A: {}  →  B: {}   ·   Überblendung {:.1} s   ·   Stimme {:.1} s",
                self.input.a_title, self.input.b_title, xf, vlen
            ))
            .small()
            .color(theme::DIM),
        );
        if self.input.check {
            ui.colored_label(
                theme::ON_AIR,
                "prüfen: ein Nachbar hat sich geändert. Der Voice-Track geht erst nach \
                 „Prüfung bestätigen“ auf Sendung.",
            );
        }
        if let Some(e) = &self.error {
            ui.colored_label(theme::ON_AIR, e);
        }
        ui.separator();
        let index = self.input.index;
        ui.horizontal(|ui| {
            if ui
                .button("Vorschlag")
                .on_hover_text("Stimme endet am Ramp-Punkt von B (sonst 2 s nach dem Beginn von B)")
                .clicked()
            {
                self.suggest();
            }
            if ui
                .add_enabled(self.dirty(), egui::Button::new("Übernehmen"))
                .clicked()
            {
                out.push(VoiceAction::Apply(index, self.draft.sanitized()));
            }
            if ui
                .add_enabled(self.input.check, egui::Button::new("Prüfung bestätigen"))
                .on_hover_text("Die jetzigen Nachbarn gelten als geprüft")
                .clicked()
            {
                out.push(VoiceAction::Confirm(index));
            }
            ui.separator();
            let hint = if pfl_available {
                "A, Stimme und B im Kopfhörer (PFL), nie auf Sendung. Zuerst „Übernehmen“."
            } else {
                "Kein Kopfhörer-Ausgang: Einstellungen → Vorhören (PFL)"
            };
            if ui
                .add_enabled(
                    pfl_available && !self.dirty(),
                    egui::Button::new("Vorhören"),
                )
                .on_hover_text(hint)
                .on_disabled_hover_text(hint)
                .clicked()
            {
                out.push(VoiceAction::Preview(index));
            }
            if ui
                .add_enabled(pfl_playing, egui::Button::new("Vorhören stopp"))
                .clicked()
            {
                out.push(VoiceAction::StopPreview);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn tone(path: &Path, secs: f64, hz: f32) {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 8000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut w = hound::WavWriter::create(path, spec).unwrap();
        for i in 0..(secs * 8000.0) as usize {
            let v = (i as f32 * hz * std::f32::consts::TAU / 8000.0).sin() * 0.5;
            w.write_sample((v * 32767.0) as i16).unwrap();
        }
        w.finalize().unwrap();
    }

    fn input(dir: &Path) -> VoiceInput {
        let (a, v, b) = (dir.join("a.wav"), dir.join("v.wav"), dir.join("b.wav"));
        tone(&a, 20.0, 440.0);
        tone(&v, 4.0, 1000.0);
        tone(&b, 20.0, 1500.0);
        VoiceInput {
            index: 1,
            title: "Ansage".into(),
            voice_path: v,
            voice_seconds: Some(4.0),
            a_path: Some(a),
            a_title: "A".into(),
            a_seconds: Some(20.0),
            a_cues: Cues::default(),
            b_path: Some(b),
            b_title: "B".into(),
            b_seconds: Some(20.0),
            b_cues: Cues {
                ramp: Some(9.0),
                ..Cues::default()
            },
            crossfade: 3.0,
            params: VoiceParams::default(),
            check: true,
        }
    }

    fn loaded(dir: &Path) -> VoiceEditor {
        let mut ed = VoiceEditor::open(input(dir));
        let t0 = std::time::Instant::now();
        while ed.loading.is_some() {
            assert!(t0.elapsed().as_secs() < 30, "loading hangs");
            std::thread::sleep(std::time::Duration::from_millis(10));
            ed.poll();
        }
        ed
    }

    #[test]
    fn suggest_ends_the_voice_at_the_ramp_of_b() {
        let dir = tempfile::tempdir().unwrap();
        let mut ed = loaded(dir.path());
        assert!(ed.error.is_none(), "{:?}", ed.error);
        ed.suggest();
        // 4 s Stimme, Ramp 9 s nach Beginn von B: Beginn bei 5 s.
        assert!((ed.draft.offset - 5.0).abs() < 1e-9, "{}", ed.draft.offset);
        assert!(ed.dirty());
        ed.set_offset(1e9);
        assert_eq!(ed.draft.offset, MAX_OFFSET);
        ed.set_offset(-2.037);
        assert!((ed.draft.offset + 2.05).abs() < 1e-9);
    }

    #[test]
    fn peaks_load_and_window_covers_the_voice() {
        let dir = tempfile::tempdir().unwrap();
        let mut ed = loaded(dir.path());
        let w = ed.waves.as_ref().unwrap();
        assert_eq!(w[1].peaks.len(), 80, "20 Balken je Sekunde");
        assert!((w[1].seconds - 4.0).abs() < 0.01);
        ed.set_offset(-30.0);
        let (from, to) = ed.window();
        assert!(from <= -33.0 && to > ed.xf());
        ed.set_offset(10.0);
        let (_, to) = ed.window();
        assert!(to >= 10.0 + 4.0);
    }

    #[test]
    fn draws_and_reports_actions_headless() {
        let dir = tempfile::tempdir().unwrap();
        let mut ed = loaded(dir.path());
        ed.suggest();
        let ctx = egui::Context::default();
        let mut open = true;
        let mut actions = Vec::new();
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            actions = ed.ui(ui.ctx(), &mut open, (false, false));
        });
        assert!(open);
        assert!(actions.is_empty(), "ohne Klick keine Aufträge");
        // Nach dem Abgleich mit der Sendung ist der Entwurf sauber.
        let p = ed.draft;
        ed.sync(p, false);
        assert!(!ed.dirty() && !ed.input.check);
    }
}
