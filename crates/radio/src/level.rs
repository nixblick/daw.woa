//! Pegelrechnung: Auto-Gain, Crossfade-Hüllkurve, Glättung. Alles echtzeitsicher.

/// Ziel-Lautheit im Radio-Modus.
pub const TARGET_LUFS: f64 = -16.0;
/// Höchste Spitze nach dem Auto-Gain (dBFS), wie beim Export.
pub const CEILING_DB: f64 = -1.0;
/// Grenzen für das Auto-Gain, gegen Ausreißer (fast stille Titel).
pub const MIN_GAIN_DB: f64 = -30.0;
pub const MAX_GAIN_DB: f64 = 20.0;

/// dB in linearen Faktor.
pub fn db_to_gain(db: f64) -> f32 {
    10f64.powf(db / 20.0) as f32
}

/// Verstärkung (dB), die einen Titel mit `lufs` und Spitze `peak_db` auf `target` bringt,
/// ohne dass die Spitze über [`CEILING_DB`] kommt. Ohne Messung: 0 dB.
pub fn auto_gain_db(lufs: Option<f64>, peak_db: Option<f64>, target: f64) -> f64 {
    let Some(l) = lufs.filter(|l| l.is_finite()) else {
        return 0.0;
    };
    let mut g = target - l;
    if let Some(p) = peak_db.filter(|p| p.is_finite()) {
        g = g.min(CEILING_DB - p);
    }
    g.clamp(MIN_GAIN_DB, MAX_GAIN_DB)
}

/// Crossfade mit konstanter Leistung: bei `t` (0..=1) die Faktoren (abgehend, kommend).
/// `out² + in² = 1` an jeder Stelle.
pub fn crossfade(t: f32) -> (f32, f32) {
    let x = t.clamp(0.0, 1.0) * std::f32::consts::FRAC_PI_2;
    (x.cos(), x.sin())
}

/// Einpoliger Glätter gegen Zipper-Noise: nähert sich dem Ziel mit Zeitkonstante `tau`.
#[derive(Debug, Clone, Copy)]
pub struct Smoother {
    value: f32,
    target: f32,
    coef: f32,
}

impl Smoother {
    /// Startet bei `value`; `tau_seconds` bei `rate` Hz.
    pub fn new(value: f32, tau_seconds: f32, rate: f32) -> Self {
        let n = (tau_seconds * rate).max(1.0);
        Self {
            value,
            target: value,
            coef: 1.0 - (-1.0 / n).exp(),
        }
    }

    pub fn set_target(&mut self, target: f32) {
        self.target = target;
    }

    pub fn target(&self) -> f32 {
        self.target
    }

    pub fn value(&self) -> f32 {
        self.value
    }

    /// Ein Sample weiter.
    #[inline]
    pub fn tick(&mut self) -> f32 {
        self.value += (self.target - self.value) * self.coef;
        if (self.target - self.value).abs() < 1e-4 {
            self.value = self.target;
        }
        self.value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crossfade_keeps_constant_power() {
        for i in 0..=100 {
            let (o, n) = crossfade(i as f32 / 100.0);
            assert!((o * o + n * n - 1.0).abs() < 1e-5, "at {i}");
        }
        assert_eq!(crossfade(0.0), (1.0, 0.0));
        let (o, n) = crossfade(1.0);
        assert!(o.abs() < 1e-6 && (n - 1.0).abs() < 1e-6);
        let (o, n) = crossfade(0.5);
        assert!((o - n).abs() < 1e-6, "symmetric in the middle");
        assert!(
            (20.0 * o.log10() + 3.01).abs() < 0.01,
            "-3 dB in the middle"
        );
    }

    #[test]
    fn auto_gain_hits_target_and_respects_ceiling() {
        assert_eq!(auto_gain_db(Some(-20.0), Some(-10.0), -16.0), 4.0);
        assert_eq!(auto_gain_db(Some(-10.0), Some(0.0), -16.0), -6.0);
        // Quiet but peaky: the ceiling wins.
        assert_eq!(auto_gain_db(Some(-30.0), Some(-3.0), -16.0), 2.0);
        assert_eq!(auto_gain_db(None, Some(-3.0), -16.0), 0.0);
        assert_eq!(auto_gain_db(Some(-90.0), None, -16.0), MAX_GAIN_DB);
        assert!((db_to_gain(-6.0) - 0.501).abs() < 0.001);
    }

    #[test]
    fn smoother_glides_without_jump() {
        let rate = 48_000.0;
        let mut s = Smoother::new(1.0, 0.05, rate);
        s.set_target(db_to_gain(-12.0));
        let mut last = 1.0;
        let mut max_step = 0.0f32;
        for _ in 0..48_000 {
            let v = s.tick();
            max_step = max_step.max((last - v).abs());
            assert!(v <= last + 1e-7, "monotonic");
            last = v;
        }
        // One time constant: about 63 % of the way.
        let mut s2 = Smoother::new(1.0, 0.05, rate);
        s2.set_target(0.0);
        for _ in 0..2400 {
            s2.tick();
        }
        assert!(
            (s2.value() - (-1.0f32).exp()).abs() < 0.01,
            "{}",
            s2.value()
        );
        assert!(max_step < 0.001, "no zipper step: {max_step}");
        assert!((last - db_to_gain(-12.0)).abs() < 1e-5, "arrives");
    }
}
