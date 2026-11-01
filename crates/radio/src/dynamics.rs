//! Sendebus- und Mikrofon-Processing (Paket 9). Alles echtzeitsicher: keine Allokation,
//! kein Lock, kein I/O; die Zustände sind feste Felder, angelegt im Control-Thread.
//!
//! **Sendebus** ([`BusProcessor`]), Stereo gekoppelt:
//! 1. Lautheitsmessung am Ausgang (K-Filter nach BS.1770, gleitend über etwa 3 s, also
//!    „kurzzeitig“), nur für die Anzeige. Sie regelt nicht nach (#297): Jeder Titel hat
//!    schon seine feste Verstärkung zum Ziel (Auto-Gain beim Bereitlegen), eine gleitende
//!    Regelung würde leise und laute Passagen innerhalb eines Titels angleichen und nach
//!    jedem Ducking nachpumpen.
//! 2. Kompressor (RMS, 2,5 : 1, Schwelle 4 dB über dem Ziel), fängt kurze Lautheitsspitzen ab.
//! 3. Limiter ohne Vorausschau mit sofortigem Angriff: Spitze nie über [`CEILING_DB`].
//!    Ohne Vorausschau entsteht keine Latenz; Mitschnitt, Sendung und Abhören bleiben
//!    sample-genau mit der Uhr des Mixers.
//!
//! **Mikrofon** ([`MicChain`]): Hochpass 80 Hz, Präsenz +3 dB bei 3 kHz, Gate (−50 dBFS,
//! schließt um 30 dB) und Kompressor (4 : 1 ab −24 dBFS, +6 dB Aufholverstärkung).
//! Ein De-Esser kommt erst in M5.

use crate::level::db_to_gain;

/// Höchste Spitze am Sendeausgang (dBFS).
pub const CEILING_DB: f64 = -1.0;
/// Messfenster der Lautheit (Zeitkonstante, Sekunden).
const AGC_TAU: f32 = 3.0;

pub use woa_dsp::dynamics::{Biquad, Compressor, Gate, Limiter, coef, lin_to_db};

/// Lautheitsmessung am Ausgang (K-gefiltert, gleitend). Sie regelt nicht (#297).
#[derive(Debug, Clone, Copy)]
struct Agc {
    kl: [Biquad; 2],
    kr: [Biquad; 2],
    ms: f32,
    ms_coef: f32,
}

impl Agc {
    fn new(rate: f32) -> Self {
        let r = f64::from(rate);
        let k = [Biquad::k_shelf(r), Biquad::k_highpass(r)];
        Self {
            kl: k,
            kr: k,
            ms: 0.0,
            ms_coef: coef(AGC_TAU, rate),
        }
    }

    /// Gemessene Lautheit (LUFS, gleitend).
    fn lufs(&self) -> f32 {
        -0.691 + 10.0 * self.ms.max(1e-18).log10()
    }

    /// Misst ein Ausgangs-Sample.
    #[inline]
    fn observe(&mut self, l: f32, r: f32) {
        let a = self.kl[0].run(l);
        let a = self.kl[1].run(a);
        let b = self.kr[0].run(r);
        let b = self.kr[1].run(b);
        self.ms += (a * a + b * b - self.ms) * self.ms_coef;
    }
}

/// Werte für die Anzeige, aus dem Audio-Thread über Atomics.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct BusMeter {
    /// Verstärkung der Lautheitsregelung (dB). Seit #297 immer 0: der Sendebus regelt
    /// innerhalb eines Titels nicht nach.
    pub agc_db: f32,
    /// Absenkung durch Kompressor (dB, ≥ 0).
    pub comp_db: f32,
    /// Absenkung durch Limiter (dB, ≥ 0).
    pub limit_db: f32,
    /// Gemessene Kurzzeit-Lautheit am Ausgang (LUFS).
    pub lufs: f32,
}

/// Sendebus: Lautheitsmessung, Kompressor, Limiter. Stereo gekoppelt.
#[derive(Debug, Clone, Copy)]
pub struct BusProcessor {
    agc: Agc,
    comp: Compressor,
    limit: Limiter,
}

impl BusProcessor {
    pub fn new(target_lufs: f64, rate: u32) -> Self {
        let r = rate.max(1) as f32;
        let t = target_lufs as f32;
        Self {
            agc: Agc::new(r),
            // RMS eines Sinus liegt ~3 dB über seinem LUFS-Wert; Schwelle 4 dB über dem Ziel.
            comp: Compressor::new(t + 4.0, 2.5, 0.01, 0.25, r),
            limit: Limiter::new(CEILING_DB, 0.08, r),
        }
    }

    /// Neues Ziel (LUFS) für die Kompressor-Schwelle.
    pub fn set_target(&mut self, target_lufs: f64) {
        self.comp.set_threshold_db(target_lufs as f32 + 4.0);
    }

    /// Rechnet den Faktor für ein Stereo-Sample `(l, r)`. Der Aufrufer multipliziert damit
    /// alle Wege (Sendung und Abhören), damit beide gleich geregelt sind.
    #[inline]
    pub fn gain(&mut self, l: f32, r: f32) -> f32 {
        let gc = self.comp.gain((l * l + r * r) * 0.5);
        let (cl, cr) = (l * gc, r * gc);
        let gl = self.limit.gain(cl.abs().max(cr.abs()));
        let g = gc * gl;
        let c = self.limit.ceiling();
        self.agc.observe((l * g).clamp(-c, c), (r * g).clamp(-c, c));
        g
    }

    /// Wendet [`Self::gain`] an und sichert hart auf die Decke.
    #[inline]
    pub fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        let g = self.gain(l, r);
        let c = self.limit.ceiling();
        ((l * g).clamp(-c, c), (r * g).clamp(-c, c))
    }

    /// Hart auf die Decke (für weitere Wege mit demselben Faktor).
    #[inline]
    pub fn clip(&self, x: f32) -> f32 {
        let c = self.limit.ceiling();
        x.clamp(-c, c)
    }

    pub fn meter(&self) -> BusMeter {
        BusMeter {
            agc_db: 0.0,
            comp_db: self.comp.reduction_db(),
            limit_db: self.limit.reduction_db(),
            lufs: self.agc.lufs(),
        }
    }
}

/// Mikrofon-Kette: EQ (Hochpass, Präsenz), Gate, Kompressor mit Aufholverstärkung.
#[derive(Debug, Clone, Copy)]
pub struct MicChain {
    hp: [Biquad; 2],
    presence: [Biquad; 2],
    gate: Gate,
    comp: Compressor,
    makeup: f32,
}

impl MicChain {
    pub fn new(rate: u32) -> Self {
        let r = f64::from(rate.max(1));
        let hp = Biquad::highpass(80.0, std::f64::consts::FRAC_1_SQRT_2, r);
        let pr = Biquad::peaking(3_000.0, 1.0, 3.0, r);
        Self {
            hp: [hp, hp],
            presence: [pr, pr],
            gate: Gate::new(-50.0, 30.0, r as f32),
            comp: Compressor::new(-24.0, 4.0, 0.005, 0.12, r as f32),
            makeup: db_to_gain(6.0),
        }
    }

    #[inline]
    pub fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        let l = self.presence[0].run(self.hp[0].run(l));
        let r = self.presence[1].run(self.hp[1].run(r));
        let g = self.gate.gain(l.abs().max(r.abs()));
        let (l, r) = (l * g, r * g);
        let c = self.comp.gain((l * l + r * r) * 0.5) * self.makeup;
        (l * c, r * c)
    }

    pub fn gate_open(&self) -> bool {
        self.gate.is_open()
    }

    pub fn reduction_db(&self) -> f32 {
        self.comp.reduction_db()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: u32 = 48_000;

    fn sine(i: usize, f: f32, a: f32) -> f32 {
        (i as f32 * f * std::f32::consts::TAU / SR as f32).sin() * a
    }

    #[test]
    fn limiter_never_exceeds_ceiling() {
        let mut bus = BusProcessor::new(-16.0, SR);
        let ceil = db_to_gain(CEILING_DB);
        for i in 0..SR as usize * 2 {
            // Laut (+6 dBFS) mit harten Sprüngen.
            let a = if (i / 1000) % 2 == 0 { 2.0 } else { 0.1 };
            let (l, r) = bus.process(sine(i, 440.0, a), sine(i, 445.0, a));
            assert!(
                l.abs() <= ceil + 1e-6 && r.abs() <= ceil + 1e-6,
                "{i}: {l} {r}"
            );
        }
    }

    #[test]
    fn bus_does_not_ride_quiet_and_loud_passages() {
        // #297: leise und laute Passagen unter der Kompressor-Schwelle behalten ihren Abstand.
        let mut bus = BusProcessor::new(-16.0, SR);
        let mut peaks = [0.0f32; 2];
        for (k, amp) in [0.03f32, 0.15].into_iter().enumerate() {
            for i in 0..SR as usize * 6 {
                let (l, _) = bus.process(sine(i, 1_000.0, amp), sine(i, 1_000.0, amp));
                if i > SR as usize * 5 {
                    peaks[k] = peaks[k].max(l.abs() / amp);
                }
            }
        }
        assert!((peaks[0] - 1.0).abs() < 0.01, "{peaks:?}");
        assert!((peaks[1] - 1.0).abs() < 0.01, "{peaks:?}");
        assert_eq!(bus.meter().agc_db, 0.0);
        assert!((bus.meter().lufs - (-0.691 + 20.0 * 0.15f32.log10())).abs() < 0.5);
    }

    #[test]
    fn gate_closes_on_noise_and_opens_on_speech() {
        let mut mic = MicChain::new(SR);
        let mut quiet = 0.0f32;
        for i in 0..SR as usize {
            let (l, _) = mic.process(sine(i, 1000.0, 0.001), 0.0);
            if i > SR as usize / 2 {
                quiet = quiet.max(l.abs());
            }
        }
        assert!(!mic.gate_open());
        let mut loud = 0.0f32;
        for i in 0..SR as usize / 2 {
            let (l, _) = mic.process(sine(i, 1000.0, 0.1), 0.0);
            if i > SR as usize / 4 {
                loud = loud.max(l.abs());
            }
        }
        assert!(mic.gate_open());
        // Ohne Gate wäre das Verhältnis 40 dB, mit Gate (30 dB Bereich) deutlich mehr.
        let ratio = lin_to_db(loud / quiet.max(1e-12));
        assert!(ratio > 55.0, "{ratio} dB");
    }

    #[test]
    fn mic_eq_cuts_rumble_and_compressor_evens_out() {
        let mut mic = MicChain::new(SR);
        let mut rumble = 0.0f32;
        for i in 0..SR as usize {
            let (l, _) = mic.process(sine(i, 30.0, 0.2) + sine(i, 1000.0, 0.05), 0.0);
            if i > SR as usize / 2 {
                rumble = rumble.max(l.abs());
            }
        }
        // 30 Hz weitgehend weg: Spitze bleibt in der Nähe des 1-kHz-Anteils.
        assert!(rumble < 0.2, "{rumble}");
        let level = |amp: f32| {
            let mut m = MicChain::new(SR);
            let mut p = 0.0f32;
            for i in 0..SR as usize {
                let (l, _) = m.process(sine(i, 1000.0, amp), 0.0);
                if i > SR as usize / 2 {
                    p = p.max(l.abs());
                }
            }
            lin_to_db(p)
        };
        // 20 dB Unterschied am Eingang werden deutlich kleiner.
        let spread = level(0.5) - level(0.05);
        assert!(spread < 12.0 && spread > 3.0, "{spread} dB");
    }

    #[test]
    fn compressor_curve() {
        let c = Compressor::new(-20.0, 4.0, 0.01, 0.1, 48_000.0);
        assert_eq!(c.curve(-40.0), 0.0);
        assert!((c.curve(0.0) - 15.0).abs() < 1e-4);
        assert!(c.curve(-20.0) > 0.0 && c.curve(-20.0) < 1.0);
    }
}
