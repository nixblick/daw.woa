//! Sampler: vorab geladener Mono-Puffer, Tonhöhe per linearer Interpolation,
//! optionaler Loop, ADSR, feste Stimmenzahl.

use std::sync::Arc;

use crate::instrument::{
    Adsr, AdsrParams, AdsrStage, Instrument, NoteEvent, NoteEventKind, pick_voice,
};
use crate::{DEFAULT_RAMP_SECONDS, Smoother};

/// Parameter-IDs des Samplers.
pub mod sampler_params {
    pub const ATTACK: u32 = 1;
    pub const DECAY: u32 = 2;
    pub const SUSTAIN: u32 = 3;
    pub const RELEASE: u32 = 4;
    pub const GAIN: u32 = 5;
}

/// Fehler beim Anlegen eines Samples.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SampleError {
    Empty,
    InvalidLoop {
        start: usize,
        end: usize,
        len: usize,
    },
}

impl std::fmt::Display for SampleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => write!(f, "sample is empty"),
            Self::InvalidLoop { start, end, len } => {
                write!(f, "invalid loop {start}..{end} for {len} frames")
            }
        }
    }
}

impl std::error::Error for SampleError {}

/// Unveränderlicher Sample-Puffer (Mono, f32), im Control-Thread geladen.
#[derive(Debug)]
pub struct SampleData {
    frames: Box<[f32]>,
    sample_rate: f32,
    root_note: u8,
    loop_range: Option<(usize, usize)>,
}

impl SampleData {
    /// `loop_range` = Start (inklusive) bis Ende (exklusive) in Frames.
    pub fn new(
        frames: Vec<f32>,
        sample_rate: f32,
        root_note: u8,
        loop_range: Option<(usize, usize)>,
    ) -> Result<Self, SampleError> {
        let len = frames.len();
        if len == 0 {
            return Err(SampleError::Empty);
        }
        if let Some((start, end)) = loop_range
            && (start >= end || end > len)
        {
            return Err(SampleError::InvalidLoop { start, end, len });
        }
        Ok(Self {
            frames: frames.into_boxed_slice(),
            sample_rate: sample_rate.max(1.0),
            root_note,
            loop_range,
        })
    }

    pub fn len(&self) -> usize {
        self.frames.len()
    }

    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }
}

#[derive(Debug, Clone, Default)]
struct SamplerVoice {
    note: u8,
    pos: f64,
    rate: f64,
    velocity: f32,
    env: Adsr,
    age: u64,
}

/// Sampler-Instrument.
pub struct Sampler {
    sample: Arc<SampleData>,
    voices: Box<[SamplerVoice]>,
    sample_rate: f32,
    adsr: AdsrParams,
    gain: Smoother,
    counter: u64,
}

impl Sampler {
    pub fn new(sample: Arc<SampleData>, voices: usize) -> Self {
        Self {
            sample,
            voices: vec![SamplerVoice::default(); voices.max(1)].into_boxed_slice(),
            sample_rate: 48_000.0,
            adsr: AdsrParams {
                attack: 0.001,
                decay: 0.0,
                sustain: 1.0,
                release: 0.05,
            },
            gain: Smoother::new(1.0, DEFAULT_RAMP_SECONDS),
            counter: 0,
        }
    }

    pub fn with_adsr(mut self, adsr: AdsrParams) -> Self {
        self.adsr = adsr;
        self
    }

    pub fn active_voices(&self) -> usize {
        self.voices.iter().filter(|v| v.env.is_active()).count()
    }

    fn rate_for(&self, note: u8) -> f64 {
        let semis = f64::from(note) - f64::from(self.sample.root_note);
        2f64.powf(semis / 12.0) * f64::from(self.sample.sample_rate) / f64::from(self.sample_rate)
    }

    fn note_on(&mut self, note: u8, velocity: u8) {
        self.counter += 1;
        let age = self.counter;
        let rate = self.rate_for(note);
        let (adsr, sr) = (self.adsr, self.sample_rate);
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
            v.env.kill();
        }
        v.pos = 0.0;
        v.note = note;
        v.rate = rate;
        v.velocity = f32::from(velocity.min(127)) / 127.0;
        v.age = age;
        v.env.trigger(&adsr, sr);
    }
}

impl Instrument for Sampler {
    fn prepare(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate.max(1.0);
        self.gain.prepare(self.sample_rate);
    }

    fn set_param(&mut self, param: u32, value: f32) {
        match param {
            sampler_params::ATTACK => self.adsr.attack = value.max(0.0),
            sampler_params::DECAY => self.adsr.decay = value.max(0.0),
            sampler_params::SUSTAIN => self.adsr.sustain = value.clamp(0.0, 1.0),
            sampler_params::RELEASE => self.adsr.release = value.max(0.0),
            sampler_params::GAIN => self.gain.set_target(value),
            _ => {}
        }
    }

    fn handle_event(&mut self, event: &NoteEvent) {
        match event.kind {
            NoteEventKind::NoteOn { note, velocity: 0 } | NoteEventKind::NoteOff { note } => {
                for v in self.voices.iter_mut() {
                    if v.note == note && v.env.is_active() && v.env.stage() != AdsrStage::Release {
                        v.env.release();
                    }
                }
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
        let data = &self.sample.frames;
        let len = data.len();
        let lp = self.sample.loop_range;
        for v in self.voices.iter_mut() {
            if !v.env.is_active() {
                continue;
            }
            for s in out.iter_mut() {
                let idx = v.pos as usize;
                if idx >= len {
                    v.env.kill();
                    break;
                }
                let frac = (v.pos - idx as f64) as f32;
                let a = data[idx];
                let next = idx + 1;
                let b = match lp {
                    Some((ls, le)) if next >= le => data[ls],
                    _ => data.get(next).copied().unwrap_or(0.0),
                };
                let env = v.env.next_value();
                *s += (a + (b - a) * frac) * env * v.velocity;
                v.pos += v.rate;
                if let Some((ls, le)) = lp {
                    let (ls, le) = (ls as f64, le as f64);
                    while v.pos >= le {
                        v.pos -= le - ls;
                    }
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

    fn ramp(n: usize) -> Vec<f32> {
        (0..n).map(|i| i as f32 / n as f32).collect()
    }

    #[test]
    fn invalid_loop_is_rejected() {
        assert!(SampleData::new(ramp(10), 48_000.0, 60, Some((5, 11))).is_err());
        assert!(SampleData::new(ramp(10), 48_000.0, 60, Some((5, 5))).is_err());
        assert!(SampleData::new(vec![], 48_000.0, 60, None).is_err());
    }

    #[test]
    fn one_shot_ends_after_sample() {
        let data = Arc::new(SampleData::new(vec![1.0; 100], 48_000.0, 60, None).expect("ok"));
        let mut s = Sampler::new(data, 2);
        s.prepare(48_000.0);
        s.handle_event(&NoteEvent::note_on(60, 127));
        let mut buf = [0.0f32; 128];
        s.render(&mut buf);
        assert_eq!(s.active_voices(), 0);
        assert_eq!(buf[110], 0.0);
    }

    #[test]
    fn octave_up_plays_twice_as_fast() {
        let data = Arc::new(SampleData::new(ramp(1000), 48_000.0, 60, None).expect("ok"));
        let mut s = Sampler::new(data, 1);
        s.prepare(48_000.0);
        s.handle_event(&NoteEvent::note_on(72, 127));
        let mut buf = [0.0f32; 400];
        s.render(&mut buf);
        assert_eq!(s.active_voices(), 1);
        let mut buf2 = [0.0f32; 128];
        s.render(&mut buf2);
        assert_eq!(s.active_voices(), 0);
    }

    #[test]
    fn loop_sustains() {
        let data = Arc::new(SampleData::new(ramp(100), 48_000.0, 60, Some((50, 100))).expect("ok"));
        let mut s = Sampler::new(data, 1);
        s.prepare(48_000.0);
        s.handle_event(&NoteEvent::note_on(60, 127));
        let mut buf = [0.0f32; 4800];
        s.render(&mut buf);
        assert_eq!(s.active_voices(), 1);
        assert!(buf[4000..].iter().all(|x| *x >= 0.49 && *x < 1.0));
    }
}
