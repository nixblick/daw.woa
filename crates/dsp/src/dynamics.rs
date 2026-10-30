//! Dynamik und Filter: Biquad, Kompressor, Limiter, Gate. Aus dem Radio-Crate hierher
//! verschoben (Paket 16), damit Radio-Sendebus und Mischpult-Inserts dieselben Bausteine
//! nutzen. Alles echtzeitsicher: keine Allokation, kein Lock, kein I/O; die Zustände sind
//! feste Felder, angelegt im Control-Thread.

/// dB in linearen Faktor.
pub fn db_to_gain(db: f64) -> f32 {
    10f64.powf(db / 20.0) as f32
}

/// Biquad (direkte Form I, transponiert nicht nötig), f32, ein Kanal.
#[derive(Debug, Clone, Copy)]
pub struct Biquad {
    b: [f32; 3],
    a: [f32; 2],
    x: [f32; 2],
    y: [f32; 2],
}

impl Biquad {
    /// Aus Zähler `b`, Nenner `a0`/`a` (unnormiert).
    pub fn new(b: [f64; 3], a0: f64, a: [f64; 2]) -> Self {
        Self {
            b: [(b[0] / a0) as f32, (b[1] / a0) as f32, (b[2] / a0) as f32],
            a: [(a[0] / a0) as f32, (a[1] / a0) as f32],
            x: [0.0; 2],
            y: [0.0; 2],
        }
    }

    /// Durchgang ohne Wirkung (Zähler = Nenner).
    pub fn identity() -> Self {
        Self::new([1.0, 0.0, 0.0], 1.0, [0.0, 0.0])
    }

    /// Übernimmt die Koeffizienten von `other`, behält den eigenen Zustand (kein Sprung im
    /// Filtergedächtnis, Grundlage für sanftes Umstimmen im Betrieb).
    pub fn retune(&mut self, other: &Biquad) {
        self.b = other.b;
        self.a = other.a;
    }

    /// Hochpass 2. Ordnung (RBJ).
    pub fn highpass(freq: f64, q: f64, rate: f64) -> Self {
        let w = std::f64::consts::TAU * freq / rate;
        let (s, c) = w.sin_cos();
        let alpha = s / (2.0 * q);
        Self::new(
            [(1.0 + c) / 2.0, -(1.0 + c), (1.0 + c) / 2.0],
            1.0 + alpha,
            [-2.0 * c, 1.0 - alpha],
        )
    }

    /// Glockenfilter (RBJ).
    pub fn peaking(freq: f64, q: f64, gain_db: f64, rate: f64) -> Self {
        let a = 10f64.powf(gain_db / 40.0);
        let w = std::f64::consts::TAU * freq / rate;
        let (s, c) = w.sin_cos();
        let alpha = s / (2.0 * q);
        Self::new(
            [1.0 + alpha * a, -2.0 * c, 1.0 - alpha * a],
            1.0 + alpha / a,
            [-2.0 * c, 1.0 - alpha / a],
        )
    }

    /// Höhen-Shelf des K-Filters (BS.1770), für beliebige Raten.
    pub fn k_shelf(rate: f64) -> Self {
        let (f0, g, q) = (
            1_681.974_450_955_533,
            3.999_843_853_973_347,
            0.707_175_236_955_419_6,
        );
        let k = (std::f64::consts::PI * f0 / rate).tan();
        let vh = 10f64.powf(g / 20.0);
        let vb = vh.powf(0.499_666_774_154_541_6);
        Self::new(
            [
                vh + vb * k / q + k * k,
                2.0 * (k * k - vh),
                vh - vb * k / q + k * k,
            ],
            1.0 + k / q + k * k,
            [2.0 * (k * k - 1.0), 1.0 - k / q + k * k],
        )
    }

    /// Hochpass des K-Filters (BS.1770).
    pub fn k_highpass(rate: f64) -> Self {
        let (f0, q) = (38.135_470_876_024_44, 0.500_327_037_323_877_3);
        let k = (std::f64::consts::PI * f0 / rate).tan();
        Self::new(
            [1.0, -2.0, 1.0],
            1.0 + k / q + k * k,
            [2.0 * (k * k - 1.0), 1.0 - k / q + k * k],
        )
    }

    #[inline]
    pub fn run(&mut self, x: f32) -> f32 {
        let y = self.b[0] * x + self.b[1] * self.x[0] + self.b[2] * self.x[1]
            - self.a[0] * self.y[0]
            - self.a[1] * self.y[1];
        self.x = [x, self.x[0]];
        // Denormale vermeiden: sehr kleine Werte zu 0.
        let y = if y.abs() < 1e-20 { 0.0 } else { y };
        self.y = [y, self.y[0]];
        y
    }
}

/// Koeffizient eines einpoligen Glätters mit Zeitkonstante `tau` (Sekunden).
pub fn coef(tau: f32, rate: f32) -> f32 {
    1.0 - (-1.0 / (tau * rate).max(1.0)).exp()
}

pub fn lin_to_db(x: f32) -> f32 {
    20.0 * x.max(1e-9).log10()
}

/// RMS-Kompressor mit weichem Knie, Hüllkurve in dB mit Angriff und Rückstellung.
#[derive(Debug, Clone, Copy)]
pub struct Compressor {
    threshold_db: f32,
    ratio: f32,
    knee_db: f32,
    rms_coef: f32,
    attack: f32,
    release: f32,
    ms: f32,
    reduction_db: f32,
}

impl Compressor {
    /// Setzt die Schwelle (dB) im laufenden Betrieb.
    pub fn set_threshold_db(&mut self, threshold_db: f32) {
        self.threshold_db = threshold_db;
    }

    /// Stellt alle Parameter im laufenden Betrieb um (ohne Allokation, Zustand bleibt).
    pub fn set(&mut self, threshold_db: f32, ratio: f32, attack_s: f32, release_s: f32, rate: f32) {
        self.threshold_db = threshold_db;
        self.ratio = ratio.max(1.0);
        self.attack = coef(attack_s, rate);
        self.release = coef(release_s, rate);
    }

    pub fn new(threshold_db: f32, ratio: f32, attack_s: f32, release_s: f32, rate: f32) -> Self {
        Self {
            threshold_db,
            ratio: ratio.max(1.0),
            knee_db: 6.0,
            rms_coef: coef(0.01, rate),
            attack: coef(attack_s, rate),
            release: coef(release_s, rate),
            ms: 0.0,
            reduction_db: 0.0,
        }
    }

    /// Statische Kennlinie: Absenkung (dB, ≥ 0) für Eingangspegel `x_db`.
    pub fn curve(&self, x_db: f32) -> f32 {
        let over = x_db - self.threshold_db;
        let slope = 1.0 - 1.0 / self.ratio;
        let h = self.knee_db / 2.0;
        if over <= -h {
            0.0
        } else if over >= h {
            over * slope
        } else {
            slope * (over + h) * (over + h) / (2.0 * self.knee_db)
        }
    }

    /// Nimmt den Pegel eines Samples (Summe der Quadrate je Kanal gemittelt) und gibt den
    /// Verstärkungsfaktor zurück.
    #[inline]
    pub fn gain(&mut self, power: f32) -> f32 {
        self.ms += (power - self.ms) * self.rms_coef;
        let level_db = 10.0 * self.ms.max(1e-18).log10();
        let target = self.curve(level_db);
        let c = if target > self.reduction_db {
            self.attack
        } else {
            self.release
        };
        self.reduction_db += (target - self.reduction_db) * c;
        db_to_gain(-f64::from(self.reduction_db))
    }

    pub fn reduction_db(&self) -> f32 {
        self.reduction_db
    }
}

/// Spitzenbegrenzer: sofortiger Angriff, weiche Rückstellung, harte Sicherung.
#[derive(Debug, Clone, Copy)]
pub struct Limiter {
    ceiling: f32,
    release: f32,
    gain: f32,
}

impl Limiter {
    pub fn new(ceiling_db: f64, release_s: f32, rate: f32) -> Self {
        Self {
            ceiling: db_to_gain(ceiling_db),
            release: coef(release_s, rate),
            gain: 1.0,
        }
    }

    /// Stellt Decke und Rückstellung im laufenden Betrieb um (Zustand bleibt).
    pub fn set(&mut self, ceiling_db: f64, release_s: f32, rate: f32) {
        self.ceiling = db_to_gain(ceiling_db);
        self.release = coef(release_s, rate);
    }

    /// Faktor für ein Sample mit Spitze `peak` (Betrag, beide Kanäle).
    #[inline]
    pub fn gain(&mut self, peak: f32) -> f32 {
        self.gain += (1.0 - self.gain) * self.release;
        if peak * self.gain > self.ceiling {
            self.gain = self.ceiling / peak;
        }
        self.gain
    }

    pub fn ceiling(&self) -> f32 {
        self.ceiling
    }

    pub fn reduction_db(&self) -> f32 {
        -lin_to_db(self.gain)
    }
}

/// Mikrofon-Gate: offen über der Schwelle, mit Haltezeit, schließt um `range_db`.
#[derive(Debug, Clone, Copy)]
pub struct Gate {
    threshold: f32,
    floor: f32,
    hold: u32,
    held: u32,
    env: f32,
    env_coef: f32,
    attack: f32,
    release: f32,
    gain: f32,
}

impl Gate {
    pub fn new(threshold_db: f64, range_db: f64, rate: f32) -> Self {
        Self {
            threshold: db_to_gain(threshold_db),
            floor: db_to_gain(-range_db.abs()),
            hold: (0.1 * rate) as u32,
            held: 0,
            env: 0.0,
            env_coef: coef(0.005, rate),
            attack: coef(0.001, rate),
            release: coef(0.15, rate),
            gain: db_to_gain(-range_db.abs()),
        }
    }

    pub fn is_open(&self) -> bool {
        self.held > 0
    }

    #[inline]
    pub fn gain(&mut self, peak: f32) -> f32 {
        // Spitzen-Hüllkurve: steigt sofort, fällt mit 5 ms.
        if peak > self.env {
            self.env = peak;
        } else {
            self.env += (peak - self.env) * self.env_coef;
        }
        if self.env >= self.threshold {
            self.held = self.hold.max(1);
        } else if self.held > 0 {
            self.held -= 1;
        }
        let (target, c) = if self.held > 0 {
            (1.0, self.attack)
        } else {
            (self.floor, self.release)
        };
        self.gain += (target - self.gain) * c;
        self.gain
    }
}
