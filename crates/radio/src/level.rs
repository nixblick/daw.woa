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
    woa_dsp::analysis::track::gain_to_target_db(lufs, peak_db, target, CEILING_DB)
        .clamp(MIN_GAIN_DB, MAX_GAIN_DB)
}

/// Crossfade mit konstanter Leistung: bei `t` (0..=1) die Faktoren (abgehend, kommend).
/// `out² + in² = 1` an jeder Stelle.
pub fn crossfade(t: f32) -> (f32, f32) {
    let x = t.clamp(0.0, 1.0) * std::f32::consts::FRAC_PI_2;
    (x.cos(), x.sin())
}

/// Crossfader der Party (0 = links, 1 = rechts): Constant-Power wie [`crossfade`], aber mit
/// exakten Endpunkten (ganz links ist rechts wirklich stumm und umgekehrt).
pub fn fader_gains(x: f32) -> (f32, f32) {
    if x <= 0.0 {
        (1.0, 0.0)
    } else if x >= 1.0 {
        (0.0, 1.0)
    } else {
        crossfade(x)
    }
}

/// So lange braucht der Fader von Hand höchstens für den ganzen Weg (gegen Knacken).
pub const FADER_HAND_SECONDS: f32 = 0.01;

/// Stellung des Party-Crossfaders im Audio-Thread: fährt linear und sample-genau auf ihr Ziel,
/// je Frame höchstens um `step`. So springt die Stellung nie, auch wenn die Hand ruckt, und das
/// Ziel wird exakt erreicht (anders als beim [`Smoother`]).
#[derive(Debug, Clone, Copy)]
pub struct Fader {
    pos: f32,
    target: f32,
    step: f32,
    /// Schritt einer Handbewegung (ganzer Weg in [`FADER_HAND_SECONDS`]).
    hand_step: f32,
}

impl Fader {
    pub fn new(pos: f32, rate: f32) -> Self {
        let pos = pos.clamp(0.0, 1.0);
        let hand_step = 1.0 / (FADER_HAND_SECONDS * rate).max(1.0);
        Self {
            pos,
            target: pos,
            step: hand_step,
            hand_step,
        }
    }

    /// Ziel setzen. `frames == 0`: von Hand (fährt schnell, aber ohne Sprung); sonst fährt der
    /// Fader in genau so vielen Frames hin (Automatik). Ein neues Ziel ersetzt das alte.
    pub fn set(&mut self, target: f32, frames: usize) {
        self.target = target.clamp(0.0, 1.0);
        self.step = if frames == 0 {
            self.hand_step
        } else {
            ((self.target - self.pos).abs() / frames as f32).max(f32::MIN_POSITIVE)
        };
    }

    pub fn pos(&self) -> f32 {
        self.pos
    }

    pub fn target(&self) -> f32 {
        self.target
    }

    /// Ein Frame weiter; gibt die Stellung zurück.
    #[inline]
    pub fn tick(&mut self) -> f32 {
        let d = self.target - self.pos;
        // Kleine Rundungsreste der Fahrt nicht als eigenen Schritt stehen lassen.
        if d.abs() <= self.step + 1e-6 {
            self.pos = self.target;
        } else {
            self.pos += self.step.copysign(d);
        }
        self.pos
    }
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
    fn fader_gains_constant_power_with_exact_ends() {
        assert_eq!(fader_gains(0.0), (1.0, 0.0));
        assert_eq!(fader_gains(1.0), (0.0, 1.0));
        assert_eq!(fader_gains(-0.5), (1.0, 0.0));
        assert_eq!(fader_gains(2.0), (0.0, 1.0));
        for i in 0..=1000 {
            let (a, b) = fader_gains(i as f32 / 1000.0);
            assert!((a * a + b * b - 1.0).abs() < 1e-5, "power at {i}");
        }
        let (a, b) = fader_gains(0.5);
        assert!((a - b).abs() < 1e-6, "no hole in the middle: both -3 dB");
    }

    #[test]
    fn fader_moves_without_jumps_and_reaches_target() {
        let rate = 48_000.0;
        let mut f = Fader::new(0.0, rate);
        // Hand reißt von ganz links nach ganz rechts: kein Sprung, Ende exakt.
        f.set(1.0, 0);
        let max = 1.0 / (FADER_HAND_SECONDS * rate);
        let mut last = f.pos();
        let mut gl = fader_gains(last);
        for _ in 0..2000 {
            let p = f.tick();
            assert!((p - last).abs() <= max + 1e-6, "step too big");
            let g = fader_gains(p);
            assert!(
                (g.0 - gl.0).abs() < 0.01 && (g.1 - gl.1).abs() < 0.01,
                "gain jump"
            );
            last = p;
            gl = g;
        }
        assert_eq!(f.pos(), 1.0);
        // Automatik: in genau 100 Frames zurück.
        f.set(0.0, 100);
        for _ in 0..98 {
            f.tick();
        }
        assert!(f.pos() > 0.0);
        f.tick();
        f.tick();
        assert_eq!(f.pos(), 0.0);
    }

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
