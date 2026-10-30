//! Einfacher Stereo-Resampler für den Netz-Thread (linear, mit Zustand über Blockgrenzen).
//!
//! Opus läuft immer mit 48 kHz. Läuft die Engine mit einer anderen Rate (zum Beispiel 44,1 kHz),
//! wird im Netz-Thread umgerechnet, nie im Audio-Thread. Für Sprache reicht linear; bei 48 kHz
//! Engine-Rate ist der Resampler ein Durchreicher ohne Rechnung.

/// Stereo verschachtelt, lineare Interpolation, stetig über Blöcke.
#[derive(Debug, Clone)]
pub struct StreamResampler {
    /// Eingangsframes je Ausgangsframe.
    step: f64,
    /// Position im aktuellen Block, gemessen ab dem letzten Frame des Vorblocks.
    pos: f64,
    prev: (f32, f32),
    passthrough: bool,
}

impl StreamResampler {
    pub fn new(from: u32, to: u32) -> Self {
        let from = from.max(1);
        let to = to.max(1);
        Self {
            step: f64::from(from) / f64::from(to),
            pos: 0.0,
            prev: (0.0, 0.0),
            passthrough: from == to,
        }
    }

    /// Hängt die umgerechneten Frames an `out` an (Stereo verschachtelt).
    pub fn process(&mut self, input: &[f32], out: &mut Vec<f32>) {
        let n = input.len() / 2;
        if n == 0 {
            return;
        }
        if self.passthrough {
            out.extend_from_slice(&input[..n * 2]);
            return;
        }
        let limit = n as f64;
        while self.pos < limit {
            let i = self.pos as usize;
            let frac = (self.pos - i as f64) as f32;
            let (al, ar) = if i == 0 {
                self.prev
            } else {
                (input[(i - 1) * 2], input[(i - 1) * 2 + 1])
            };
            let (bl, br) = (input[i * 2], input[i * 2 + 1]);
            out.push(al + (bl - al) * frac);
            out.push(ar + (br - ar) * frac);
            self.pos += self.step;
        }
        self.pos -= limit;
        self.prev = (input[(n - 1) * 2], input[(n - 1) * 2 + 1]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(freq: f32, rate: u32, frames: usize, start: usize) -> Vec<f32> {
        (0..frames)
            .flat_map(|i| {
                let v = (std::f32::consts::TAU * freq * (start + i) as f32 / rate as f32).sin();
                [v, v]
            })
            .collect()
    }

    #[test]
    fn same_rate_is_passthrough() {
        let mut r = StreamResampler::new(48_000, 48_000);
        let x = sine(440.0, 48_000, 100, 0);
        let mut out = Vec::new();
        r.process(&x, &mut out);
        assert_eq!(out, x);
    }

    #[test]
    fn length_follows_ratio_across_blocks() {
        let mut r = StreamResampler::new(48_000, 44_100);
        let mut out = Vec::new();
        for b in 0..100 {
            r.process(&sine(440.0, 48_000, 480, b * 480), &mut out);
        }
        let frames = out.len() / 2;
        // 48000 Frames rein -> 44100 raus (±2).
        assert!((frames as i64 - 44_100).abs() <= 2, "{frames}");
    }

    #[test]
    fn tone_survives_the_conversion() {
        let mut r = StreamResampler::new(44_100, 48_000);
        let mut out = Vec::new();
        for b in 0..50 {
            r.process(&sine(1000.0, 44_100, 441, b * 441), &mut out);
        }
        // Nulldurchgänge zählen: 1 kHz über 0,5 s = 1000 Durchgänge.
        let l: Vec<f32> = out.chunks(2).map(|f| f[0]).collect();
        let crossings = l.windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0).count();
        assert!((crossings as i64 - 500).abs() <= 2, "{crossings}");
    }
}
