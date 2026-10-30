//! Latenzausgleich (Paket 33): Hat ein Plugin eine Latenz, kommt seine Spur später an als die
//! anderen. Der Graph-Bau hängt dann an jede kürzere Spur eine feste Verzögerung (größte Latenz
//! minus eigene), damit alles zeitgleich in die Summe läuft.
//!
//! Echtzeit: Der Speicher wird beim Anlegen reserviert, der Block rechnet ohne Allokation.

use woa_dsp::{Node, ProcessBuffers, StereoBuffers, StereoNode};

/// Ein Ring aus `len` Abtastwerten: Was hineingeht, kommt `len` Werte später heraus.
pub struct DelayLine {
    buf: Vec<f32>,
    pos: usize,
}

impl DelayLine {
    /// Verzögerung um `samples` Abtastwerte (0 reicht durch).
    pub fn new(samples: u32) -> Self {
        Self {
            buf: vec![0.0; samples as usize],
            pos: 0,
        }
    }

    /// Verzögert `input` nach `output` (gleich lang). Echtzeitsicher.
    pub fn run(&mut self, input: &[f32], output: &mut [f32]) {
        if self.buf.is_empty() {
            output.copy_from_slice(input);
            return;
        }
        for (o, i) in output.iter_mut().zip(input) {
            *o = std::mem::replace(&mut self.buf[self.pos], *i);
            self.pos += 1;
            if self.pos == self.buf.len() {
                self.pos = 0;
            }
        }
    }

    /// Leert den Ring (nach einem Sprung im Transport).
    pub fn clear(&mut self) {
        self.buf.fill(0.0);
        self.pos = 0;
    }
}

/// Verzögerung für eine Mono-Spur.
pub struct DelayNode {
    line: DelayLine,
}

impl DelayNode {
    /// Neuer Knoten.
    pub fn new(samples: u32) -> Self {
        Self {
            line: DelayLine::new(samples),
        }
    }
}

impl Node for DelayNode {
    fn prepare(&mut self, _sample_rate: f32) {}

    fn set_param(&mut self, _param: u32, _value: f32) {}

    fn process(&mut self, b: ProcessBuffers<'_>) {
        self.line.run(b.input, b.output);
    }
}

/// Verzögerung für eine Stereo-Spur.
pub struct StereoDelayNode {
    left: DelayLine,
    right: DelayLine,
}

impl StereoDelayNode {
    /// Neuer Knoten.
    pub fn new(samples: u32) -> Self {
        Self {
            left: DelayLine::new(samples),
            right: DelayLine::new(samples),
        }
    }
}

impl StereoNode for StereoDelayNode {
    fn prepare(&mut self, _sample_rate: f32) {}

    fn set_param(&mut self, _param: u32, _value: f32) {}

    fn process(&mut self, b: StereoBuffers<'_>) {
        self.left.run(b.in_left, b.out_left);
        self.right.run(b.in_right, b.out_right);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delays_by_exactly_n_samples_across_blocks() {
        let mut d = DelayLine::new(5);
        let mut input = [0.0f32; 16];
        input[0] = 1.0;
        let mut out = [0.0f32; 16];
        let (a, b) = out.split_at_mut(7);
        d.run(&input[..7], a);
        d.run(&input[7..], b);
        let peak = out.iter().position(|v| *v == 1.0);
        assert_eq!(peak, Some(5));
    }

    #[test]
    fn zero_delay_passes_through() {
        let mut d = DelayLine::new(0);
        let mut out = [0.0f32; 3];
        d.run(&[1.0, 2.0, 3.0], &mut out);
        assert_eq!(out, [1.0, 2.0, 3.0]);
    }
}
