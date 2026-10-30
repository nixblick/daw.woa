//! Instrumente, Effekte, Analyse
//!
//! Grundbausteine für den Audio-Graphen: das [`Node`]-Trait, Parameter-Glättung
//! ([`Smoother`]) und die ersten Nodes ([`SineOsc`], [`Gain`]).
//!
//! Echtzeitregel: `process` und `set_param` allokieren nie, sperren nie und
//! machen kein I/O. Alle Puffer gehören dem Graphen und werden vorab angelegt.
//!
//! Instrumente (M1): [`synth::PolySynth`], [`sampler::Sampler`],
//! [`soundfont::SoundFontPlayer`], per [`instrument::InstrumentNode`] im Graphen,
//! Note-Events über [`instrument::note_queue`] (`rtrb`).

pub mod analysis;
pub mod automation;
pub mod clock;
pub mod dynamics;
pub mod effects;
pub mod fx;
pub mod generate;
pub mod instrument;
pub mod sampler;
pub mod soundfont;
pub mod synth;

/// Puffer für einen Process-Aufruf. `input` ist die Summe aller Eingänge
/// (leer bzw. Stille bei Quellen), `output` hat dieselbe Länge.
pub struct ProcessBuffers<'a> {
    pub input: &'a [f32],
    pub output: &'a mut [f32],
}

/// Ein Knoten im Audio-Graphen (mono).
pub trait Node: Send {
    /// Wird im Control-Thread vor dem ersten `process` aufgerufen.
    fn prepare(&mut self, sample_rate: f32);
    /// Setzt einen Parameter; Wertsprünge werden intern geglättet. Echtzeitsicher.
    fn set_param(&mut self, param: u32, value: f32);
    /// Meldet vor jedem Abschnitt die Transportposition (Samples) und ob sie weiterläuft.
    /// Nur Knoten mit Automation brauchen das; sie zählen die Position im Block selbst weiter.
    /// Echtzeitsicher.
    fn locate(&mut self, _position: u64, _playing: bool) {}
    /// Rechnet einen Block. Darf nicht allokieren.
    fn process(&mut self, buffers: ProcessBuffers<'_>);
}

/// Puffer eines Stereo-Process-Aufrufs; alle vier Slices sind gleich lang.
pub struct StereoBuffers<'a> {
    pub in_left: &'a [f32],
    pub in_right: &'a [f32],
    pub out_left: &'a mut [f32],
    pub out_right: &'a mut [f32],
}

/// Ein Stereo-Knoten im Audio-Graphen (zwei Ein-, zwei Ausgänge), z. B. ein verkoppelter
/// Kompressor oder eine Panorama-Stufe. Gleiche Echtzeitregeln wie [`Node`].
pub trait StereoNode: Send {
    /// Wird im Control-Thread vor dem ersten `process` aufgerufen.
    fn prepare(&mut self, sample_rate: f32);
    /// Setzt einen Parameter; Wertsprünge werden intern geglättet. Echtzeitsicher.
    fn set_param(&mut self, param: u32, value: f32);
    /// Wie [`Node::locate`].
    fn locate(&mut self, _position: u64, _playing: bool) {}
    /// Rechnet einen Block. Darf nicht allokieren.
    fn process(&mut self, buffers: StereoBuffers<'_>);
}

/// Lineare Rampe gegen Zipper-Noise: Ein neues Ziel wird über eine feste
/// Anzahl Samples angefahren, ohne Sprung.
#[derive(Debug, Clone)]
pub struct Smoother {
    current: f32,
    target: f32,
    step: f32,
    remaining: u32,
    ramp_samples: u32,
    ramp_seconds: f32,
}

impl Smoother {
    /// Neuer Smoother mit Startwert und Rampendauer in Sekunden.
    pub fn new(value: f32, ramp_seconds: f32) -> Self {
        Self {
            current: value,
            target: value,
            step: 0.0,
            remaining: 0,
            ramp_samples: 1,
            ramp_seconds: ramp_seconds.max(0.0),
        }
    }

    /// Rechnet die Rampenlänge für die Abtastrate aus.
    pub fn prepare(&mut self, sample_rate: f32) {
        let n = (self.ramp_seconds * sample_rate.max(1.0)).round();
        self.ramp_samples = if n >= 1.0 { n as u32 } else { 1 };
    }

    /// Neues Ziel; die Rampe startet am aktuellen Wert.
    pub fn set_target(&mut self, target: f32) {
        self.target = target;
        self.remaining = self.ramp_samples;
        self.step = (target - self.current) / self.ramp_samples as f32;
    }

    /// Springt ohne Rampe (nur außerhalb des Spielbetriebs verwenden).
    pub fn reset(&mut self, value: f32) {
        self.current = value;
        self.target = value;
        self.remaining = 0;
        self.step = 0.0;
    }

    pub fn target(&self) -> f32 {
        self.target
    }

    pub fn is_smoothing(&self) -> bool {
        self.remaining > 0
    }

    /// Nächster Wert der Rampe.
    #[inline]
    pub fn next_value(&mut self) -> f32 {
        if self.remaining > 0 {
            self.remaining -= 1;
            if self.remaining == 0 {
                self.current = self.target;
            } else {
                self.current += self.step;
            }
        }
        self.current
    }
}

/// Parameter-IDs des Sinus-Oszillators.
pub mod sine_params {
    pub const FREQUENCY: u32 = 0;
    pub const AMPLITUDE: u32 = 1;
}

/// Parameter-IDs des Gain-Nodes.
pub mod gain_params {
    pub const GAIN: u32 = 0;
}

/// Standard-Rampendauer: 20 ms.
pub const DEFAULT_RAMP_SECONDS: f32 = 0.02;

/// Einfacher Sinus-Oszillator (Quelle, ignoriert den Eingang).
#[derive(Debug, Clone)]
pub struct SineOsc {
    phase: f64,
    sample_rate: f64,
    frequency: Smoother,
    amplitude: Smoother,
}

impl SineOsc {
    pub fn new(frequency: f32, amplitude: f32) -> Self {
        Self {
            phase: 0.0,
            sample_rate: 48_000.0,
            frequency: Smoother::new(frequency, DEFAULT_RAMP_SECONDS),
            amplitude: Smoother::new(amplitude, DEFAULT_RAMP_SECONDS),
        }
    }
}

impl Node for SineOsc {
    fn prepare(&mut self, sample_rate: f32) {
        self.sample_rate = f64::from(sample_rate.max(1.0));
        self.frequency.prepare(sample_rate);
        self.amplitude.prepare(sample_rate);
    }

    fn set_param(&mut self, param: u32, value: f32) {
        match param {
            sine_params::FREQUENCY => self.frequency.set_target(value.max(0.0)),
            sine_params::AMPLITUDE => self.amplitude.set_target(value),
            _ => {}
        }
    }

    fn process(&mut self, buffers: ProcessBuffers<'_>) {
        let tau = std::f64::consts::TAU;
        for out in buffers.output.iter_mut() {
            let freq = f64::from(self.frequency.next_value());
            let amp = self.amplitude.next_value();
            *out = (self.phase * tau).sin() as f32 * amp;
            self.phase += freq / self.sample_rate;
            if self.phase >= 1.0 {
                self.phase -= self.phase.floor();
            }
        }
    }
}

/// Lautstärke (linear), geglättet.
#[derive(Debug, Clone)]
pub struct Gain {
    gain: Smoother,
}

impl Gain {
    pub fn new(gain: f32) -> Self {
        Self {
            gain: Smoother::new(gain, DEFAULT_RAMP_SECONDS),
        }
    }
}

impl Node for Gain {
    fn prepare(&mut self, sample_rate: f32) {
        self.gain.prepare(sample_rate);
    }

    fn set_param(&mut self, param: u32, value: f32) {
        if param == gain_params::GAIN {
            self.gain.set_target(value);
        }
    }

    fn process(&mut self, buffers: ProcessBuffers<'_>) {
        for (out, inp) in buffers.output.iter_mut().zip(buffers.input.iter()) {
            *out = *inp * self.gain.next_value();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smoother_reaches_target_without_jump() {
        let mut s = Smoother::new(0.0, 0.01);
        s.prepare(48_000.0);
        s.set_target(1.0);
        let max_step = 1.0 / 480.0 + 1e-6;
        let mut prev = 0.0f32;
        for _ in 0..480 {
            let v = s.next_value();
            assert!((v - prev).abs() <= max_step, "jump {prev} -> {v}");
            prev = v;
        }
        assert_eq!(prev, 1.0);
        assert!(!s.is_smoothing());
    }

    #[test]
    fn retarget_mid_ramp_is_continuous() {
        let mut s = Smoother::new(0.0, 0.01);
        s.prepare(1000.0); // 10 samples
        s.set_target(1.0);
        let mut prev = 0.0;
        for _ in 0..5 {
            prev = s.next_value();
        }
        s.set_target(-1.0);
        let first = s.next_value();
        assert!((first - prev).abs() < 0.2 + 1e-6);
    }
}
