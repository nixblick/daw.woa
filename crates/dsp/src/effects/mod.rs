//! Die weiteren eingebauten Effekte (Paket 17): De-Esser, Reverb, Delay, Chorus, Sättigung,
//! Rauschunterdrückung. Das Gate steckt in [`crate::dynamics`], der volle EQ in [`crate::fx`].
//!
//! Jeder Effekt legt seine Puffer in `new` an (Control-Thread). `set` und `step` sind
//! echtzeitsicher: keine Allokation, kein Lock, kein I/O. Sprünge der Regler werden geglättet
//! (Mix, Pegel, Verzögerungszeit) oder wirken nur auf Koeffizienten, nie auf Puffer.

pub mod chorus;
pub mod deesser;
pub mod delay;
pub mod denoise;
pub mod reverb;
pub mod saturation;

/// Verzögerungsleitung mit linearer Interpolation (Ringpuffer fester Größe).
#[derive(Debug, Clone)]
pub struct DelayLine {
    buf: Vec<f32>,
    w: usize,
}

impl DelayLine {
    /// Leitung für Verzögerungen bis `max_samples` (Control-Thread: allokiert).
    pub fn new(max_samples: usize) -> Self {
        Self {
            buf: vec![0.0; max_samples.max(2) + 3],
            w: 0,
        }
    }

    /// Größte lesbare Verzögerung in Samples.
    pub fn max_delay(&self) -> f32 {
        (self.buf.len() - 3) as f32
    }

    /// Schreibt das nächste Sample.
    #[inline]
    pub fn write(&mut self, x: f32) {
        self.buf[self.w] = x;
        self.w += 1;
        if self.w == self.buf.len() {
            self.w = 0;
        }
    }

    #[inline]
    fn at(&self, back: usize) -> f32 {
        let len = self.buf.len();
        self.buf[(self.w + len - back) % len]
    }

    /// Liest `delay` Samples zurück (1 = das zuletzt geschriebene Sample), vor dem nächsten
    /// `write`. Bruchteile werden linear interpoliert.
    #[inline]
    pub fn read(&self, delay: f32) -> f32 {
        let d = delay.clamp(1.0, self.max_delay());
        let di = d as usize;
        let f = d - di as f32;
        if f == 0.0 {
            self.at(di)
        } else {
            self.at(di) * (1.0 - f) + self.at(di + 1) * f
        }
    }
}

/// Einpoliger Tiefpass mit Koeffizient aus der Grenzfrequenz.
#[derive(Debug, Clone, Copy, Default)]
pub struct OnePole {
    a: f32,
    z: f32,
}

impl OnePole {
    /// Tiefpass mit Grenzfrequenz `hz`.
    pub fn lowpass(hz: f32, rate: f32) -> Self {
        let mut p = Self::default();
        p.set_lowpass(hz, rate);
        p
    }

    /// Ändert die Grenzfrequenz, behält den Zustand.
    pub fn set_lowpass(&mut self, hz: f32, rate: f32) {
        let hz = hz.clamp(10.0, rate * 0.49);
        self.a = 1.0 - (-std::f32::consts::TAU * hz / rate).exp();
    }

    /// Ein Sample.
    #[inline]
    pub fn run(&mut self, x: f32) -> f32 {
        self.z += (x - self.z) * self.a;
        if self.z.abs() < 1e-20 {
            self.z = 0.0;
        }
        self.z
    }
}
