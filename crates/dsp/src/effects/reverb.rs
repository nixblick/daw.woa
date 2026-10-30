//! Hall nach dem Schroeder-Freeverb-Prinzip: acht parallele Kammfilter mit Dämpfung, vier
//! Allpässe hintereinander, je Kanal leicht versetzt. Die Rückkopplung jedes Kammfilters wird
//! aus der gewünschten Nachhallzeit (RT60) berechnet, so dass `decay_s` die gemessene Zeit
//! bis −60 dB ist (bei der Mittenfrequenz; die Dämpfung verkürzt die Höhen).

use super::DelayLine;
use crate::{DEFAULT_RAMP_SECONDS, Smoother};

/// Kammfilter-Längen bei 44,1 kHz (Freeverb).
const COMB: [usize; 8] = [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617];
/// Allpass-Längen bei 44,1 kHz.
const ALLPASS: [usize; 4] = [556, 441, 341, 225];
/// Versatz des rechten Kanals bei 44,1 kHz.
const SPREAD: usize = 23;
/// Längste vorgesehene Vorverzögerung in Sekunden.
pub const MAX_PREDELAY_S: f32 = 0.2;

#[derive(Debug, Clone)]
struct Comb {
    buf: Vec<f32>,
    pos: usize,
    filt: f32,
    feedback: f32,
}

impl Comb {
    fn new(len: usize) -> Self {
        Self {
            buf: vec![0.0; len.max(2)],
            pos: 0,
            filt: 0.0,
            feedback: 0.0,
        }
    }

    #[inline]
    fn run(&mut self, x: f32, damp: f32) -> f32 {
        let y = self.buf[self.pos];
        self.filt = y * (1.0 - damp) + self.filt * damp;
        if self.filt.abs() < 1e-20 {
            self.filt = 0.0;
        }
        self.buf[self.pos] = x + self.filt * self.feedback;
        self.pos += 1;
        if self.pos == self.buf.len() {
            self.pos = 0;
        }
        y
    }
}

#[derive(Debug, Clone)]
struct Allpass {
    buf: Vec<f32>,
    pos: usize,
}

impl Allpass {
    fn new(len: usize) -> Self {
        Self {
            buf: vec![0.0; len.max(2)],
            pos: 0,
        }
    }

    #[inline]
    fn run(&mut self, x: f32) -> f32 {
        // Schroeder-Allpass mit Verstärkung 1 bei jeder Frequenz: (b - g*w), w = x + g*b.
        const G: f32 = 0.5;
        let b = self.buf[self.pos];
        let w = x + b * G;
        let y = b - w * G;
        self.buf[self.pos] = w;
        self.pos += 1;
        if self.pos == self.buf.len() {
            self.pos = 0;
        }
        y
    }
}

/// Hall für einen oder zwei Kanäle.
#[derive(Debug, Clone)]
pub struct Reverb {
    combs: [[Comb; 8]; 2],
    allpass: [[Allpass; 4]; 2],
    predelay: DelayLine,
    predelay_samples: f32,
    damp: f32,
    norm: f32,
    mix: Smoother,
}

impl Reverb {
    /// Legt Puffer für die Abtastrate an (Control-Thread) und stellt die Regler ein.
    pub fn new(p: &[f32], rate: f32) -> Self {
        let k = f64::from(rate.max(8000.0)) / 44_100.0;
        let scaled = |n: usize, extra: usize| (((n + extra) as f64) * k).round() as usize;
        let comb = |extra: usize| COMB.map(|n| Comb::new(scaled(n, extra)));
        let allpass = |extra: usize| ALLPASS.map(|n| Allpass::new(scaled(n, extra)));
        let mut mix = Smoother::new(0.0, DEFAULT_RAMP_SECONDS);
        mix.prepare(rate);
        let mut r = Self {
            combs: [comb(0), comb(SPREAD)],
            allpass: [allpass(0), allpass(SPREAD)],
            predelay: DelayLine::new((MAX_PREDELAY_S * rate) as usize + 2),
            predelay_samples: 0.0,
            damp: 0.0,
            norm: 1.0,
            mix,
        };
        r.set(p, rate);
        r.mix.reset(p[3] / 100.0);
        r
    }

    /// Neue Reglerwerte: decay_s, predelay_ms, damping_pct, mix_pct.
    pub fn set(&mut self, p: &[f32], rate: f32) {
        let decay = f64::from(p[0].max(0.05));
        let mut energy = 0.0f64;
        for c in self.combs.iter_mut().flatten() {
            let g = 10f64.powf(-3.0 * c.buf.len() as f64 / (decay * f64::from(rate)));
            c.feedback = g as f32;
            energy += 1.0 / (1.0 - g * g);
        }
        // Weißes Rauschen hinein ergibt etwa gleich viel Rauschen heraus (zwei Kanäle).
        self.norm = (2.0 / energy).sqrt() as f32;
        self.predelay_samples = p[1].max(0.0) * 0.001 * rate;
        self.damp = (p[2] / 100.0).clamp(0.0, 1.0) * 0.7;
        self.mix.set_target(p[3] / 100.0);
    }

    /// Ein Sample-Paar; bei `stereo = false` zählt nur `l`.
    #[inline]
    pub fn step(&mut self, l: f32, r: f32, stereo: bool) -> (f32, f32) {
        let m = self.mix.next_value();
        let input = if stereo { 0.5 * (l + r) } else { l };
        let x = if self.predelay_samples >= 1.0 {
            let d = self.predelay.read(self.predelay_samples);
            self.predelay.write(input);
            d
        } else {
            self.predelay.write(input);
            input
        };
        let mut out = [0.0f32; 2];
        let channels = if stereo { 2 } else { 1 };
        for (c, o) in out.iter_mut().enumerate().take(channels) {
            let mut acc = 0.0;
            for comb in &mut self.combs[c] {
                acc += comb.run(x, self.damp);
            }
            let mut y = acc * self.norm;
            for ap in &mut self.allpass[c] {
                y = ap.run(y);
            }
            *o = y;
        }
        let wet_l = out[0];
        let wet_r = if stereo { out[1] } else { l };
        (l + (wet_l - l) * m, r + (wet_r - r) * m)
    }
}
