//! Ducking-Auslöser im Pult-Modus (#596).
//!
//! Im Pult-Modus hängt das Mikro am externen Mischpult. Der Pult-Eingang trägt die Stimme
//! **und** die Musik, die WOA ans Pult schickt; eine einfache Pegelschwelle am Pult-Eingang
//! würde sich also selbst auslösen. Zwei Wege:
//!
//! - [`TriggerMode::Input`]: ein eigener Eingang nur zum Messen (der einzelne Mikro-Kanal
//!   eines mehrkanaligen USB-Pults oder ein zweites Gerät). Liegt er über der Schwelle, spricht
//!   jemand. Er geht nie auf Sendung.
//! - [`TriggerMode::Auto`]: [`EchoGate`] erkennt Stimme im Pult-Eingang, obwohl dort WOAs
//!   eigene Musik mitläuft. WOA kennt das Signal, das es ans Pult schickt (Referenz), und
//!   schätzt, wie laut es zurückkommt. Liegt der Pult-Eingang deutlich darüber, spricht jemand.
//!
//! Alles hier läuft im Audio-Thread: feste Felder, keine Allokation, kein Lock.

/// Woran der Pult-Modus erkennt, dass jemand spricht.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TriggerMode {
    /// Kein Ducking im Pult-Modus (Telefon, Gäste und Carts ducken weiter).
    #[default]
    Off,
    /// Eigener Eingang, nur gemessen.
    Input,
    /// Aus dem Pult-Eingang, WOAs eigene Musik herausgerechnet.
    Auto,
}

/// Einstellungen des Auslösers, wie der Mixer sie braucht.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TriggerCfg {
    pub mode: TriggerMode,
    /// Stimm-Schwelle (dBFS): darunter gilt der Eingang als still.
    pub threshold_db: f32,
    /// Nur `Auto`: so viel (dB) muss der Pult-Eingang über dem liegen, was von WOAs Musik
    /// zurückkommt.
    pub margin_db: f32,
}

impl Default for TriggerCfg {
    fn default() -> Self {
        Self {
            mode: TriggerMode::Off,
            threshold_db: DEFAULT_THRESHOLD_DB,
            margin_db: DEFAULT_MARGIN_DB,
        }
    }
}

/// Vorgabe der Stimm-Schwelle (dBFS).
pub const DEFAULT_THRESHOLD_DB: f32 = -40.0;
/// Vorgabe der Spanne für `Auto` (dB).
pub const DEFAULT_MARGIN_DB: f32 = 6.0;

/// Länge eines Messabschnitts (Sekunden).
const HOP_SECONDS: f32 = 0.005;
/// Abschnitte im Laufzeitfenster: 96 × 5 ms = 480 ms. Musik kommt höchstens so spät zurück.
pub const WINDOW_HOPS: usize = 96;
/// Glättung der Hüllkurven (Sekunden).
const ENV_TAU: f32 = 0.03;
/// Unter diesem Pegel (dBFS) gilt die Referenz als zu leise zum Lernen.
const REF_FLOOR_DB: f32 = -50.0;
/// So viele Abschnitte mit lauter Referenz, bis die Schätzung gilt (1 s).
const LEARN_HOPS: u32 = 200;
/// Nachführen nach oben je Abschnitt (Anteil des Abstands; τ ≈ 100 ms).
const RISE: f32 = 0.05;
/// Absinken der Schätzung (dB je Sekunde), solange die Musik leiser zurückkommt.
const DECAY_DB_PER_S: f32 = 1.0;
/// Hysterese: einmal erkannt, gilt Stimme bis `margin - HYSTERESIS_DB`.
const HYSTERESIS_DB: f32 = 3.0;
/// Hält „Stimme“ länger als das (Abschnitte, 1,5 s) und schwankt der Abstand dabei kaum, ist
/// es keine Stimme, sondern die Musik kommt lauter zurück (Regler am Pult hochgezogen).
const STEADY_HOPS: u32 = 300;
/// Höchste Schwankung (dB, Standardabweichung) für „gleichmäßig“.
const STEADY_DB: f32 = 2.5;
/// Glättung der Schwankungsmessung (Sekunden).
const STEADY_TAU: f32 = 0.5;

fn db(p: f32) -> f32 {
    10.0 * (p + 1e-12).log10()
}

/// Stimme im Pult-Eingang erkennen, obwohl WOAs eigene Musik darin zurückkommt.
///
/// Gerechnet wird mit geglätteten Leistungshüllkurven in Abschnitten von 5 ms, nicht mit
/// Samples. Laufzeit und Verstärkung des Rückwegs sind unbekannt und ändern sich mit den
/// Reglern am Pult:
///
/// - **Laufzeit:** Erwartet wird das Lauteste der Referenz in den letzten 480 ms. Damit liegt
///   die zurückkommende Musik nie über der Erwartung, egal wie spät sie kommt. Auch beim
///   Absenken (die Referenz fällt sofort, die Musik im Pult erst eine Laufzeit später) hält
///   das Fenster die Erwartung oben; es gibt keinen Selbsthalt.
/// - **Verstärkung:** `gain_db` folgt dem Abstand Eingang minus Erwartung nach oben (τ 100 ms)
///   und sinkt langsam (1 dB/s). Nachgeführt wird nur, wenn keine Stimme erkannt ist und die
///   Referenz laut genug ist.
/// - **Regler hochgezogen:** Hält „Stimme“ länger als 1,5 s bei gleichmäßigem Abstand
///   (Schwankung unter 2,5 dB), ist es Musik mit neuer Verstärkung: die Schätzung springt nach.
///   Sprache schwankt im Silbentakt viel stärker.
/// - **Rückkopplung:** Die Referenz ist das, was WOA wirklich ans Pult schickt, also schon
///   geduckt. Duckt WOA, sinken Referenz und zurückkommende Musik gemeinsam; die Erkennung
///   rechnet mit der geduckten Referenz.
#[derive(Debug, Clone)]
pub struct EchoGate {
    hop: usize,
    n: usize,
    acc_in: f32,
    acc_ref: f32,
    env_in: f32,
    env_ref: f32,
    a_env: f32,
    hist: [f32; WINDOW_HOPS],
    pos: usize,
    gain_db: f32,
    learned: u32,
    decay: f32,
    voice: bool,
    voice_hops: u32,
    a_steady: f32,
    mean: f32,
    mean_sq: f32,
    /// Letzter Stand für Anzeige und Tests.
    pub last_in_db: f32,
    pub last_expected_db: f32,
}

impl EchoGate {
    pub fn new(rate: u32) -> Self {
        let r = rate.max(1) as f32;
        let hop = ((HOP_SECONDS * r) as usize).max(1);
        let hop_s = hop as f32 / r;
        Self {
            hop,
            n: 0,
            acc_in: 0.0,
            acc_ref: 0.0,
            env_in: 0.0,
            env_ref: 0.0,
            a_env: 1.0 - (-hop_s / ENV_TAU).exp(),
            hist: [-120.0; WINDOW_HOPS],
            pos: 0,
            gain_db: 0.0,
            learned: 0,
            decay: DECAY_DB_PER_S * hop_s,
            voice: false,
            voice_hops: 0,
            a_steady: 1.0 - (-hop_s / STEADY_TAU).exp(),
            mean: 0.0,
            mean_sq: 0.0,
            last_in_db: -120.0,
            last_expected_db: -120.0,
        }
    }

    /// Vergisst alles Gelernte (neuer Pult-Eingang, Modus gewechselt).
    pub fn reset(&mut self) {
        let a = self.a_env;
        let d = self.decay;
        let s = self.a_steady;
        let hop = self.hop;
        *self = Self {
            hop,
            a_env: a,
            decay: d,
            a_steady: s,
            ..Self::new(1)
        };
    }

    /// Vergisst den Signalverlauf (Hüllkurven, Laufzeitfenster, Stimme), behält die gelernte
    /// Verstärkung. Nach einem ungültigen Abschnitt (NaN/Inf, #605). Ohne Allokation.
    fn drop_signal(&mut self) {
        self.env_in = 0.0;
        self.env_ref = 0.0;
        self.hist = [-120.0; WINDOW_HOPS];
        self.pos = 0;
        self.voice = false;
        self.voice_hops = 0;
        self.mean = 0.0;
        self.mean_sq = 0.0;
        self.last_in_db = -120.0;
        self.last_expected_db = -120.0;
    }

    /// Ist die Schätzung schon eingelernt?
    pub fn learned(&self) -> bool {
        self.learned >= LEARN_HOPS
    }

    /// Geschätzte Verstärkung des Rückwegs (dB).
    pub fn gain_db(&self) -> f32 {
        self.gain_db
    }

    /// Ein Frame: `input` Pult-Eingang, `reference` das, was WOA gerade ans Pult schickt
    /// (je Stereo-Paar). Gibt zurück, ob gerade Stimme erkannt ist.
    #[inline]
    pub fn tick(&mut self, input: (f32, f32), reference: (f32, f32), cfg: &TriggerCfg) -> bool {
        self.acc_in += 0.5 * (input.0 * input.0 + input.1 * input.1);
        self.acc_ref += 0.5 * (reference.0 * reference.0 + reference.1 * reference.1);
        self.n += 1;
        if self.n >= self.hop {
            let k = 1.0 / self.n as f32;
            let (pi, pr) = (self.acc_in * k, self.acc_ref * k);
            self.acc_in = 0.0;
            self.acc_ref = 0.0;
            self.n = 0;
            self.step(pi, pr, cfg);
        }
        self.voice
    }

    fn step(&mut self, p_in: f32, p_ref: f32, cfg: &TriggerCfg) {
        // NaN/Inf (#605): einmal je Abschnitt geprüft, nicht je Sample. Hüllkurven und
        // Historie zurück, die gelernte Verstärkung bleibt; nichts bleibt NaN hängen.
        if !(p_in.is_finite() && p_ref.is_finite()) {
            self.drop_signal();
            return;
        }
        self.env_in += (p_in - self.env_in) * self.a_env;
        self.env_ref += (p_ref - self.env_ref) * self.a_env;
        let in_db = db(self.env_in);
        let ref_db = db(self.env_ref);
        self.hist[self.pos] = ref_db;
        self.pos = (self.pos + 1) % WINDOW_HOPS;
        let ref_max = self.hist.iter().fold(-120.0f32, |a, &b| a.max(b));
        let expected = ref_max + self.gain_db;
        let excess = in_db - expected;
        self.last_in_db = in_db;
        self.last_expected_db = expected;

        let loud_ref = ref_max > REF_FLOOR_DB;
        let above = in_db > cfg.threshold_db - if self.voice { HYSTERESIS_DB } else { 0.0 };
        let voice = if !above {
            false
        } else if !loud_ref {
            // WOA schickt (fast) nichts: alles über der Schwelle ist Stimme.
            true
        } else if !self.learned() {
            false
        } else if expected < cfg.threshold_db - 6.0 {
            // Von WOAs Musik kommt kaum etwas zurück.
            true
        } else {
            let m = cfg.margin_db - if self.voice { HYSTERESIS_DB } else { 0.0 };
            excess > m
        };

        // Gleichmäßigkeit des Abstands (Regler hochgezogen statt Stimme).
        let r = in_db - ref_max;
        self.mean += (r - self.mean) * self.a_steady;
        self.mean_sq += (r * r - self.mean_sq) * self.a_steady;
        if voice && loud_ref {
            self.voice_hops = self.voice_hops.saturating_add(1);
            let var = (self.mean_sq - self.mean * self.mean).max(0.0);
            if self.voice_hops > STEADY_HOPS && var.sqrt() < STEADY_DB {
                self.gain_db = self.mean + var.sqrt();
                self.voice_hops = 0;
                self.voice = false;
                return;
            }
        } else {
            self.voice_hops = 0;
        }

        if !voice && loud_ref {
            if !self.learned() {
                // Einlernen: schnell in beide Richtungen.
                self.gain_db += (r - self.gain_db) * 0.1;
                self.learned += 1;
            } else if r > self.gain_db {
                self.gain_db += (r - self.gain_db) * RISE;
            } else {
                self.gain_db -= self.decay;
            }
        }
        self.voice = voice;
    }
}

/// Pegel (linear) zu dBFS.
pub fn lin_to_db(v: f32) -> f32 {
    20.0 * (v.abs() + 1e-9).log10()
}

/// dBFS zu linear.
pub fn db_to_lin(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: u32 = 48_000;

    /// Rauschen als „Musik“, Spitze 0,3.
    struct Noise(u32);

    impl Noise {
        fn next(&mut self) -> f32 {
            self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            0.3 * ((self.0 >> 8) as f32 / (1u32 << 24) as f32 * 2.0 - 1.0)
        }
    }

    fn auto() -> TriggerCfg {
        TriggerCfg {
            mode: TriggerMode::Auto,
            ..TriggerCfg::default()
        }
    }

    /// `seconds` lang; `f(k)` liefert (Eingang, Referenz). Zählt die Frames mit Stimme.
    fn feed(g: &mut EchoGate, seconds: f32, f: &mut dyn FnMut(usize) -> (f32, f32)) -> usize {
        let n = (seconds * SR as f32) as usize;
        let cfg = auto();
        (0..n)
            .filter(|&k| {
                let (i, r) = f(k);
                g.tick((i, i), (r, r), &cfg)
            })
            .count()
    }

    /// #605: Ein NaN im Pult-Eingang machte die Hüllkurve für immer NaN: taub.
    #[test]
    fn nan_in_the_input_does_not_leave_the_gate_deaf() {
        let mut g = EchoGate::new(SR);
        feed(&mut g, 0.1, &mut |_| (f32::NAN, 0.0));
        feed(&mut g, 0.1, &mut |_| (f32::INFINITY, 0.0));
        assert_eq!(feed(&mut g, 0.3, &mut |_| (0.0, 0.0)), 0, "Stille");
        assert!(g.last_in_db.is_finite(), "{}", g.last_in_db);
        let mut s = Noise(7);
        let voiced = feed(&mut g, 0.3, &mut |_| (s.next(), 0.0));
        assert!(voiced > 0, "Stimme ohne Musik wird wieder erkannt");
    }

    /// #605: Ein NaN in der Referenz ließ `loud_ref` für immer falsch: Musik allein galt
    /// als Stimme (Dauer-Ducking).
    #[test]
    fn nan_in_the_reference_does_not_duck_for_ever() {
        let mut g = EchoGate::new(SR);
        let mut m = Noise(3);
        let mut music = move |_| {
            let r = m.next();
            (0.5 * r, r)
        };
        // Einlernen mit Musik, die halb so laut zurückkommt.
        feed(&mut g, 3.0, &mut music);
        assert!(g.learned());
        assert_eq!(feed(&mut g, 1.0, &mut music), 0, "Musik allein vorher");
        feed(&mut g, 0.01, &mut |_| (0.0, f32::NAN));
        feed(&mut g, 0.01, &mut |_| (0.0, f32::NEG_INFINITY));
        assert_eq!(feed(&mut g, 3.0, &mut music), 0, "Musik allein danach");
        assert!(g.last_expected_db.is_finite(), "{}", g.last_expected_db);
        assert!(g.gain_db().is_finite());
        // Stimme deutlich über der Musik wird weiter erkannt.
        let mut v = Noise(11);
        let mut m2 = Noise(5);
        let voiced = feed(&mut g, 1.0, &mut |_| {
            let r = 0.1 * m2.next();
            (0.5 * r + 3.0 * v.next(), r)
        });
        assert!(voiced > 0, "Stimme nach dem NaN");
    }
}
