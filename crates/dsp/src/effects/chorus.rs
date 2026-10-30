//! Chorus: je Kanal zwei Stimmen mit sinusförmig schwankender Verzögerung (um eine halbe
//! Periode versetzt), der rechte Kanal um eine Viertelperiode gegen den linken.

use super::DelayLine;
use crate::{DEFAULT_RAMP_SECONDS, Smoother};

/// Größte Grundverzögerung plus Tiefe in Millisekunden (Puffergröße).
const MAX_MS: f32 = 45.0;

/// Chorus für einen oder zwei Kanäle.
#[derive(Debug, Clone)]
pub struct Chorus {
    line: [DelayLine; 2],
    phase: f32,
    inc: f32,
    base: f32,
    depth: Smoother,
    mix: Smoother,
}

impl Chorus {
    /// Legt die Puffer an (Control-Thread) und stellt die Regler ein.
    pub fn new(p: &[f32], rate: f32) -> Self {
        let size = (MAX_MS * 0.001 * rate) as usize + 2;
        let mut depth = Smoother::new(0.0, DEFAULT_RAMP_SECONDS);
        depth.prepare(rate);
        let mut mix = Smoother::new(0.0, DEFAULT_RAMP_SECONDS);
        mix.prepare(rate);
        let mut c = Self {
            line: [DelayLine::new(size), DelayLine::new(size)],
            phase: 0.0,
            inc: 0.0,
            base: 0.0,
            depth,
            mix,
        };
        c.set(p, rate);
        c.depth.reset(p[1] * 0.001 * rate);
        c.mix.reset(p[3] / 100.0);
        c
    }

    /// Neue Reglerwerte: rate_hz, depth_ms, delay_ms, mix_pct.
    pub fn set(&mut self, p: &[f32], rate: f32) {
        self.inc = p[0].max(0.0) / rate;
        self.base = p[2] * 0.001 * rate;
        self.depth.set_target(p[1] * 0.001 * rate);
        self.mix.set_target(p[3] / 100.0);
    }

    #[inline]
    fn voice(&self, ch: usize, phase: f32, depth: f32) -> f32 {
        let lfo = (std::f32::consts::TAU * phase.fract()).sin();
        self.line[ch].read((self.base + depth * lfo).max(1.0))
    }

    /// Ein Sample-Paar; bei `stereo = false` zählt nur `l`.
    #[inline]
    pub fn step(&mut self, l: f32, r: f32, stereo: bool) -> (f32, f32) {
        let m = self.mix.next_value();
        let depth = self.depth.next_value();
        let p = self.phase;
        let wl = 0.5 * (self.voice(0, p, depth) + self.voice(0, p + 0.5, depth));
        self.line[0].write(l);
        let out = if stereo {
            let wr = 0.5 * (self.voice(1, p + 0.25, depth) + self.voice(1, p + 0.75, depth));
            self.line[1].write(r);
            (l + (wl - l) * m, r + (wr - r) * m)
        } else {
            (l + (wl - l) * m, r)
        };
        self.phase += self.inc;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }
        out
    }
}
