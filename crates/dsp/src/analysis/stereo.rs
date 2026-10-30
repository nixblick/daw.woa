//! Stereo-Bild: Korrelation, Balance und Breite.

/// Korrelation nach Pearson ohne Mittelwertabzug (wie das Phasenkorrelations-Messgerät):
/// +1 Mono, 0 unabhängig, −1 gegenphasig. `None` bei Stille auf einem Kanal.
pub fn correlation(left: &[f32], right: &[f32]) -> Option<f64> {
    let mut m = StereoMeter::new(f64::INFINITY);
    m.push(left, right);
    m.correlation()
}

/// Laufende Stereo-Messung mit exponentiellem Gedächtnis.
#[derive(Debug, Clone)]
pub struct StereoMeter {
    /// Zeitkonstante in Samples (unendlich: alles gleich gewichtet).
    tau: f64,
    ll: f64,
    rr: f64,
    lr: f64,
    mm: f64,
    ss: f64,
}

impl StereoMeter {
    /// Gedächtnis von `tau_samples` Samples; `f64::INFINITY` für ein ganzes Stück.
    pub fn new(tau_samples: f64) -> Self {
        Self {
            tau: tau_samples,
            ll: 0.0,
            rr: 0.0,
            lr: 0.0,
            mm: 0.0,
            ss: 0.0,
        }
    }

    /// Nimmt Stereo-Samples.
    pub fn push(&mut self, left: &[f32], right: &[f32]) {
        let decay = if self.tau.is_finite() {
            (-1.0 / self.tau.max(1.0)).exp()
        } else {
            1.0
        };
        for (&l, &r) in left.iter().zip(right) {
            let (l, r) = (f64::from(l), f64::from(r));
            let (m, s) = ((l + r) * 0.5, (l - r) * 0.5);
            self.ll = self.ll * decay + l * l;
            self.rr = self.rr * decay + r * r;
            self.lr = self.lr * decay + l * r;
            self.mm = self.mm * decay + m * m;
            self.ss = self.ss * decay + s * s;
        }
    }

    /// Korrelation −1 … +1; `None` bei Stille.
    pub fn correlation(&self) -> Option<f64> {
        let d = (self.ll * self.rr).sqrt();
        (d > 1e-12).then(|| (self.lr / d).clamp(-1.0, 1.0))
    }

    /// Balance in dB (rechts minus links); `None`, wenn ein Kanal still ist.
    pub fn balance_db(&self) -> Option<f64> {
        (self.ll > 1e-12 && self.rr > 1e-12).then(|| 10.0 * (self.rr / self.ll).log10())
    }

    /// Breite als Seiten- zu Mittenpegel in dB: −∞ reines Mono, 0 dB gleich viel Seite wie
    /// Mitte, positiv sehr breit. `None` bei Stille.
    pub fn width_db(&self) -> Option<f64> {
        if self.mm + self.ss < 1e-12 {
            return None;
        }
        // Reines Seitensignal (Gegenphase) hat keine Mitte: sehr breit.
        Some(10.0 * ((self.ss + 1e-12) / (self.mm + 1e-12)).log10())
    }
}

/// Zeigerposition eines Sample-Paares im Stereo-Scope: x nach rechts (Seite), y nach oben
/// (Mitte). Mono liegt auf der senkrechten Achse, nur links auf der Diagonalen links oben.
pub fn scope_point(left: f32, right: f32) -> (f32, f32) {
    let k = std::f32::consts::FRAC_1_SQRT_2;
    ((right - left) * k, (left + right) * k)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn noise(n: usize, seed: u32) -> Vec<f32> {
        let mut x = seed;
        (0..n)
            .map(|_| {
                x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (x >> 8) as f32 / (1u32 << 23) as f32 - 1.0
            })
            .collect()
    }

    #[test]
    fn mono_is_plus_one_and_phase_flip_is_minus_one() {
        let l = noise(10_000, 1);
        let inv: Vec<f32> = l.iter().map(|v| -v).collect();
        assert!((correlation(&l, &l).unwrap() - 1.0).abs() < 1e-9);
        assert!((correlation(&l, &inv).unwrap() + 1.0).abs() < 1e-9);
    }

    #[test]
    fn independent_noise_is_near_zero_and_silence_is_none() {
        let (l, r) = (noise(50_000, 1), noise(50_000, 7));
        assert!(correlation(&l, &r).unwrap().abs() < 0.05);
        assert!(correlation(&[0.0; 100], &l[..100]).is_none());
    }

    #[test]
    fn width_and_balance() {
        let l = noise(20_000, 3);
        let mut m = StereoMeter::new(f64::INFINITY);
        m.push(&l, &l);
        assert!(m.width_db().unwrap() < -100.0);
        assert!(m.balance_db().unwrap().abs() < 1e-9);
        let half: Vec<f32> = l.iter().map(|v| v * 0.5).collect();
        let mut m = StereoMeter::new(f64::INFINITY);
        m.push(&l, &half);
        assert!((m.balance_db().unwrap() + 6.02).abs() < 0.01);
        let inv: Vec<f32> = l.iter().map(|v| -v).collect();
        let mut m = StereoMeter::new(f64::INFINITY);
        m.push(&l, &inv);
        assert!(m.width_db().unwrap() > 100.0);
    }

    #[test]
    fn running_meter_forgets() {
        let l = noise(48_000, 5);
        let inv: Vec<f32> = l.iter().map(|v| -v).collect();
        let mut m = StereoMeter::new(4800.0);
        m.push(&l, &l);
        m.push(&l, &inv);
        assert!(m.correlation().unwrap() < -0.99);
    }

    #[test]
    fn scope_points() {
        let (x, y) = scope_point(0.5, 0.5);
        assert!(x.abs() < 1e-6 && y > 0.0);
        let (x, y) = scope_point(0.5, -0.5);
        assert!(y.abs() < 1e-6 && x < 0.0);
        let (x, y) = scope_point(0.5, 0.0);
        assert!(x < 0.0 && y > 0.0);
    }
}
