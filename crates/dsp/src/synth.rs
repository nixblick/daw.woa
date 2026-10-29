//! Polyphoner Synthesizer: bandbegrenzte Oszillatoren (PolyBLEP), ADSR,
//! feste Stimmenzahl, Voice-Stealing.

use crate::instrument::{
    Adsr, AdsrParams, AdsrStage, Instrument, NoteEvent, NoteEventKind, note_to_hz, pick_voice,
};
use crate::{DEFAULT_RAMP_SECONDS, Smoother};

/// Parameter-IDs des Synthesizers.
pub mod synth_params {
    /// 0 = Sinus, 1 = Säge, 2 = Rechteck.
    pub const WAVEFORM: u32 = 0;
    /// Sekunden.
    pub const ATTACK: u32 = 1;
    /// Sekunden.
    pub const DECAY: u32 = 2;
    /// Pegel 0..=1.
    pub const SUSTAIN: u32 = 3;
    /// Sekunden.
    pub const RELEASE: u32 = 4;
    /// Ausgangspegel linear, geglättet.
    pub const GAIN: u32 = 5;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Waveform {
    Sine,
    Saw,
    Square,
}

impl Waveform {
    fn from_param(v: f32) -> Self {
        match v.round() as i32 {
            1 => Self::Saw,
            2 => Self::Square,
            _ => Self::Sine,
        }
    }
}

/// PolyBLEP-Korrektur für eine Unstetigkeit bei Phase 0.
#[inline]
fn poly_blep(t: f64, dt: f64) -> f64 {
    if t < dt {
        let x = t / dt;
        x + x - x * x - 1.0
    } else if t > 1.0 - dt {
        let x = (t - 1.0) / dt;
        x * x + x + x + 1.0
    } else {
        0.0
    }
}

/// Ein Sample des Oszillators bei Phase `t` (0..1) und Schrittweite `dt`.
#[inline]
pub fn osc_sample(wave: Waveform, t: f64, dt: f64) -> f64 {
    match wave {
        Waveform::Sine => (t * std::f64::consts::TAU).sin(),
        Waveform::Saw => 2.0 * t - 1.0 - poly_blep(t, dt),
        Waveform::Square => {
            let naive = if t < 0.5 { 1.0 } else { -1.0 };
            let t2 = (t + 0.5).fract();
            naive + poly_blep(t, dt) - poly_blep(t2, dt)
        }
    }
}

#[derive(Debug, Clone, Default)]
struct SynthVoice {
    note: u8,
    phase: f64,
    inc: f64,
    velocity: f32,
    env: Adsr,
    age: u64,
}

/// Polyphoner Synthesizer mit fester, vorab angelegter Stimmenzahl.
pub struct PolySynth {
    voices: Box<[SynthVoice]>,
    sample_rate: f32,
    waveform: Waveform,
    adsr: AdsrParams,
    gain: Smoother,
    counter: u64,
}

impl PolySynth {
    /// `voices` wird hier (Control-Thread) einmal angelegt, mindestens 1.
    pub fn new(voices: usize, waveform: Waveform) -> Self {
        Self {
            voices: vec![SynthVoice::default(); voices.max(1)].into_boxed_slice(),
            sample_rate: 48_000.0,
            waveform,
            adsr: AdsrParams::default(),
            gain: Smoother::new(0.5, DEFAULT_RAMP_SECONDS),
            counter: 0,
        }
    }

    /// Setzt die Hüllkurve direkt (Control-Thread, vor dem Spielen).
    pub fn with_adsr(mut self, adsr: AdsrParams) -> Self {
        self.adsr = adsr;
        self
    }

    pub fn voice_count(&self) -> usize {
        self.voices.len()
    }

    /// Anzahl klingender Stimmen.
    pub fn active_voices(&self) -> usize {
        self.voices.iter().filter(|v| v.env.is_active()).count()
    }

    fn note_on(&mut self, note: u8, velocity: u8) {
        self.counter += 1;
        let age = self.counter;
        let sr = self.sample_rate;
        let adsr = self.adsr;
        let inc = note_to_hz(note) / f64::from(sr);
        let Some(v) = pick_voice(&mut self.voices, note, |v| {
            (
                v.env.is_active(),
                v.note,
                v.env.stage(),
                v.env.level(),
                v.age,
            )
        }) else {
            return;
        };
        if v.env.is_active() && v.note != note {
            // Gestohlene Stimme: neu ansetzen.
            v.env.kill();
            v.phase = 0.0;
        } else if !v.env.is_active() {
            v.phase = 0.0;
        }
        v.note = note;
        v.inc = inc;
        v.velocity = f32::from(velocity.min(127)) / 127.0;
        v.age = age;
        v.env.trigger(&adsr, sr);
    }

    fn note_off(&mut self, note: u8) {
        for v in self.voices.iter_mut() {
            if v.note == note && v.env.is_active() && v.env.stage() != AdsrStage::Release {
                v.env.release();
            }
        }
    }
}

impl Instrument for PolySynth {
    fn prepare(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate.max(1.0);
        self.gain.prepare(self.sample_rate);
    }

    fn set_param(&mut self, param: u32, value: f32) {
        match param {
            synth_params::WAVEFORM => self.waveform = Waveform::from_param(value),
            synth_params::ATTACK => self.adsr.attack = value.max(0.0),
            synth_params::DECAY => self.adsr.decay = value.max(0.0),
            synth_params::SUSTAIN => self.adsr.sustain = value.clamp(0.0, 1.0),
            synth_params::RELEASE => self.adsr.release = value.max(0.0),
            synth_params::GAIN => self.gain.set_target(value),
            _ => {}
        }
    }

    fn handle_event(&mut self, event: &NoteEvent) {
        match event.kind {
            NoteEventKind::NoteOn { note, velocity: 0 } | NoteEventKind::NoteOff { note } => {
                self.note_off(note)
            }
            NoteEventKind::NoteOn { note, velocity } => self.note_on(note, velocity),
            NoteEventKind::AllNotesOff => {
                for v in self.voices.iter_mut() {
                    v.env.release();
                }
            }
            NoteEventKind::ProgramChange { .. } => {}
        }
    }

    fn render(&mut self, out: &mut [f32]) {
        out.fill(0.0);
        let wave = self.waveform;
        for v in self.voices.iter_mut() {
            if !v.env.is_active() {
                continue;
            }
            for s in out.iter_mut() {
                let env = v.env.next_value();
                let x = osc_sample(wave, v.phase, v.inc) as f32;
                *s += x * env * v.velocity;
                v.phase += v.inc;
                if v.phase >= 1.0 {
                    v.phase -= 1.0;
                }
                if !v.env.is_active() {
                    break;
                }
            }
        }
        for s in out.iter_mut() {
            *s *= self.gain.next_value();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synth(voices: usize) -> PolySynth {
        let mut s = PolySynth::new(voices, Waveform::Saw);
        s.prepare(48_000.0);
        s
    }

    #[test]
    fn voice_stealing_keeps_fixed_count() {
        let mut s = synth(4);
        for n in 60..70 {
            s.handle_event(&NoteEvent::note_on(n, 100));
        }
        assert_eq!(s.active_voices(), 4);
        let notes: Vec<u8> = s.voices.iter().map(|v| v.note).collect();
        // Die vier jüngsten Noten bleiben.
        for n in 66..70 {
            assert!(notes.contains(&n), "{notes:?}");
        }
    }

    #[test]
    fn released_voice_is_stolen_first() {
        let mut s = synth(2);
        s.handle_event(&NoteEvent::note_on(60, 100));
        s.handle_event(&NoteEvent::note_on(64, 100));
        s.handle_event(&NoteEvent::note_off(64));
        s.handle_event(&NoteEvent::note_on(67, 100));
        let notes: Vec<u8> = s.voices.iter().map(|v| v.note).collect();
        assert!(notes.contains(&60) && notes.contains(&67), "{notes:?}");
    }

    #[test]
    fn polyblep_saw_stays_bounded_and_is_band_limited() {
        let dt = 5000.0 / 48_000.0;
        let mut t = 0.0;
        let mut max_jump = 0.0f64;
        let mut prev = osc_sample(Waveform::Saw, t, dt);
        for _ in 0..4800 {
            t += dt;
            if t >= 1.0 {
                t -= 1.0;
            }
            let x = osc_sample(Waveform::Saw, t, dt);
            assert!(x.abs() <= 1.0 + 1e-9);
            max_jump = max_jump.max((x - prev).abs());
            prev = x;
        }
        // Die naive Säge springt um 2.0; PolyBLEP glättet die Kante.
        assert!(max_jump < 1.5, "max jump {max_jump}");
    }

    #[test]
    fn note_off_releases_to_silence() {
        let mut s = synth(2);
        s.set_param(synth_params::RELEASE, 0.01);
        s.handle_event(&NoteEvent::note_on(60, 127));
        let mut buf = [0.0f32; 480];
        s.render(&mut buf);
        s.handle_event(&NoteEvent::note_off(60));
        s.render(&mut buf);
        s.render(&mut buf);
        assert_eq!(s.active_voices(), 0);
        assert!(buf.iter().all(|x| *x == 0.0));
    }
}
