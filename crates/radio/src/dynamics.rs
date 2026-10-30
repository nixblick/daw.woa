//! Sendebus- und Mikrofon-Processing (Paket 9). Alles echtzeitsicher: keine Allokation,
//! kein Lock, kein I/O; die Zustände sind feste Felder, angelegt im Control-Thread.
//!
//! **Sendebus** ([`BusProcessor`]), Stereo gekoppelt:
//! 1. Lautheitsregelung (AGC): misst die Lautheit *am Ausgang* (K-Filter nach BS.1770,
//!    gleitend über etwa 3 s, also „kurzzeitig“) und führt eine Verstärkung langsam
//!    (höchstens [`AGC_DB_PER_SECOND`]) Richtung Ziel, z. B. −16 LUFS. In Pausen
//!    (unter [`AGC_GATE_LUFS`]) bleibt sie stehen, damit Stille nicht hochgezogen wird.
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
/// Regelbereich der Lautheitsregelung (dB, ±).
pub const AGC_RANGE_DB: f32 = 12.0;
/// Schnellste Änderung der Lautheitsregelung.
pub const AGC_DB_PER_SECOND: f32 = 3.0;
/// Darunter gilt der Ausgang als Pause, die Regelung hält still.
pub const AGC_GATE_LUFS: f32 = -45.0;
/// Messfenster der Lautheit (Zeitkonstante, Sekunden).
const AGC_TAU: f32 = 3.0;
/// Die Regelung rechnet alle so viele Samples neu.
const AGC_STEP: u32 = 64;

pub use woa_dsp::dynamics::{Biquad, Compressor, Gate, Limiter, coef, lin_to_db};

/// Lautheitsmessung am Ausgang (K-gefiltert, gleitend) und langsame Regelung.
#[derive(Debug, Clone, Copy)]
struct Agc {
    kl: [Biquad; 2],
    kr: [Biquad; 2],
    ms: f32,
    ms_coef: f32,
    target: f32,
    gain_db: f32,
    max_step_db: f32,
    count: u32,
    gain: f32,
}

impl Agc {
    fn new(target_lufs: f32, rate: f32) -> Self {
        let r = f64::from(rate);
        let k = [Biquad::k_shelf(r), Biquad::k_highpass(r)];
        Self {
            kl: k,
            kr: k,
            ms: 0.0,
            ms_coef: coef(AGC_TAU, rate),
            target: target_lufs,
            gain_db: 0.0,
            max_step_db: AGC_DB_PER_SECOND * AGC_STEP as f32 / rate.max(1.0),
            count: 0,
            gain: 1.0,
        }
    }

    /// Gemessene Lautheit (LUFS, gleitend).
    fn lufs(&self) -> f32 {
        -0.691 + 10.0 * self.ms.max(1e-18).log10()
    }

    /// Misst ein Ausgangs-Sample; alle [`AGC_STEP`] Samples wird die Verstärkung nachgeführt.
    #[inline]
    fn observe(&mut self, l: f32, r: f32) {
        let a = self.kl[0].run(l);
        let a = self.kl[1].run(a);
        let b = self.kr[0].run(r);
        let b = self.kr[1].run(b);
        self.ms += (a * a + b * b - self.ms) * self.ms_coef;
        self.count += 1;
        if self.count >= AGC_STEP {
            self.count = 0;
            let lufs = self.lufs();
            if lufs > AGC_GATE_LUFS {
                let want = (self.gain_db + (self.target - lufs)).clamp(-AGC_RANGE_DB, AGC_RANGE_DB);
                let step = (want - self.gain_db).clamp(-self.max_step_db, self.max_step_db);
                self.gain_db += step;
                self.gain = db_to_gain(f64::from(self.gain_db));
            }
        }
    }
}

/// Werte für die Anzeige, aus dem Audio-Thread über Atomics.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct BusMeter {
    /// Verstärkung der Lautheitsregelung (dB).
    pub agc_db: f32,
    /// Absenkung durch Kompressor (dB, ≥ 0).
    pub comp_db: f32,
    /// Absenkung durch Limiter (dB, ≥ 0).
    pub limit_db: f32,
    /// Gemessene Kurzzeit-Lautheit am Ausgang (LUFS).
    pub lufs: f32,
}

/// Sendebus: Lautheitsregelung, Kompressor, Limiter. Stereo gekoppelt.
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
            agc: Agc::new(t, r),
            // RMS eines Sinus liegt ~3 dB über seinem LUFS-Wert; Schwelle 4 dB über dem Ziel.
            comp: Compressor::new(t + 4.0, 2.5, 0.01, 0.25, r),
            limit: Limiter::new(CEILING_DB, 0.08, r),
        }
    }

    /// Neues Ziel (LUFS); die Regelung fährt weich dorthin.
    pub fn set_target(&mut self, target_lufs: f64) {
        let t = target_lufs as f32;
        self.agc.target = t;
        self.comp.set_threshold_db(t + 4.0);
    }

    /// Rechnet den Faktor für ein Stereo-Sample `(l, r)`. Der Aufrufer multipliziert damit
    /// alle Wege (Sendung und Abhören), damit beide gleich geregelt sind.
    #[inline]
    pub fn gain(&mut self, l: f32, r: f32) -> f32 {
        let g0 = self.agc.gain;
        let (al, ar) = (l * g0, r * g0);
        let gc = self.comp.gain((al * al + ar * ar) * 0.5);
        let (cl, cr) = (al * gc, ar * gc);
        let gl = self.limit.gain(cl.abs().max(cr.abs()));
        let g = g0 * gc * gl;
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
            agc_db: self.agc.gain_db,
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
    fn agc_pulls_quiet_and_loud_material_to_target() {
        for amp in [0.05f32, 0.6] {
            let mut bus = BusProcessor::new(-16.0, SR);
            let mut out_l = Vec::new();
            let mut out_r = Vec::new();
            for i in 0..SR as usize * 20 {
                let (l, r) = bus.process(sine(i, 440.0, amp), sine(i, 440.0, amp));
                if i > SR as usize * 12 {
                    out_l.push(l);
                    out_r.push(r);
                }
            }
            let lufs = woa_engine::loudness::integrated_lufs(&out_l, &out_r, SR).unwrap();
            assert!((lufs + 16.0).abs() < 1.0, "amp {amp}: {lufs} LUFS");
        }
    }

    #[test]
    fn agc_holds_still_in_silence() {
        let mut bus = BusProcessor::new(-16.0, SR);
        for _ in 0..SR as usize * 10 {
            bus.process(0.0, 0.0);
        }
        assert_eq!(bus.meter().agc_db, 0.0);
        // Rauschen weit unter der Pausenschwelle zieht nicht hoch.
        for i in 0..SR as usize * 5 {
            bus.process(sine(i, 300.0, 1e-4), 0.0);
        }
        assert_eq!(bus.meter().agc_db, 0.0);
    }

    #[test]
    fn agc_moves_slowly() {
        let mut bus = BusProcessor::new(-16.0, SR);
        for i in 0..SR as usize {
            bus.process(sine(i, 440.0, 0.05), sine(i, 440.0, 0.05));
        }
        let m = bus.meter();
        assert!(
            m.agc_db > 0.5 && m.agc_db <= AGC_DB_PER_SECOND + 0.1,
            "{m:?}"
        );
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
