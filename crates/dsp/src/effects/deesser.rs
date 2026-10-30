//! De-Esser: senkt nur das Band um die S-Laute ab (Split-Band). Ein Bandpass auf `freq_hz`
//! liefert das Zischband; überschreitet dessen Pegel die Schwelle, wird nur dieser Anteil
//! vom Signal abgezogen. Alles außerhalb des Bandes bleibt unberührt.

use crate::dynamics::{Biquad, coef, lin_to_db};

/// Güte des Zischbandes (breit genug für Sprecherstimmen).
const BAND_Q: f64 = 1.2;

/// De-Esser für einen oder zwei Kanäle (verkoppelt).
#[derive(Debug, Clone)]
pub struct DeEsser {
    band: [Biquad; 2],
    threshold_db: f32,
    ratio: f32,
    max_cut_db: f32,
    env: f32,
    attack: f32,
    release: f32,
    gain: f32,
    gain_coef: f32,
    reduction_db: f32,
}

impl DeEsser {
    /// Legt den De-Esser mit den Reglern an.
    pub fn new(p: &[f32], rate: f32) -> Self {
        let mut d = Self {
            band: [Biquad::identity(); 2],
            threshold_db: -30.0,
            ratio: 4.0,
            max_cut_db: 12.0,
            env: 0.0,
            attack: coef(0.0005, rate),
            release: coef(0.03, rate),
            gain: 1.0,
            gain_coef: coef(0.001, rate),
            reduction_db: 0.0,
        };
        d.band = [d.design(p, rate); 2];
        d.set(p, rate);
        d
    }

    fn design(&self, p: &[f32], rate: f32) -> Biquad {
        let rate = f64::from(rate.max(8000.0));
        Biquad::bandpass(f64::from(p[0]).clamp(1000.0, rate * 0.45), BAND_Q, rate)
    }

    /// Neue Reglerwerte: freq_hz, threshold_db, ratio, max_cut_db.
    pub fn set(&mut self, p: &[f32], rate: f32) {
        let new = self.design(p, rate);
        for b in &mut self.band {
            b.retune(&new);
        }
        self.threshold_db = p[1];
        self.ratio = p[2].max(1.0);
        self.max_cut_db = p[3].max(0.0);
    }

    /// Aktuelle Absenkung in dB.
    pub fn reduction_db(&self) -> f32 {
        self.reduction_db
    }

    /// Ein Sample-Paar; bei `stereo = false` zählt nur `l`.
    #[inline]
    pub fn step(&mut self, l: f32, r: f32, stereo: bool) -> (f32, f32) {
        let bl = self.band[0].run(l);
        let br = if stereo { self.band[1].run(r) } else { 0.0 };
        let level = bl.abs().max(br.abs());
        if level > self.env {
            self.env += (level - self.env) * self.attack;
        } else {
            self.env += (level - self.env) * self.release;
        }
        let over = lin_to_db(self.env) - self.threshold_db;
        let cut = if over > 0.0 {
            (over * (1.0 - 1.0 / self.ratio)).min(self.max_cut_db)
        } else {
            0.0
        };
        self.reduction_db = cut;
        let target = (-cut * std::f32::consts::LN_10 / 20.0).exp();
        self.gain += (target - self.gain) * self.gain_coef;
        let k = 1.0 - self.gain;
        (l - k * bl, r - k * br)
    }
}
