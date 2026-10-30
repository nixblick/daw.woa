//! Spektrogramm: Spektren über der Zeit, Farbskala und Bild (RGB), für Fenster und Agent.

use super::spectrum::{FLOOR_DB, SpectrumAnalyzer, hz_bin};

/// Untere Grenze der Farbskala in dB.
pub const MIN_DB: f32 = -90.0;
/// Kleinste dargestellte Frequenz in Hz.
pub const MIN_HZ: f32 = 20.0;

/// Spektren aufeinanderfolgender Fenster, Bin für Bin in dBFS.
pub struct Spectrogram {
    /// Fenstergröße der FFT in Samples.
    pub size: usize,
    /// Abtastrate in Hz.
    pub rate: u32,
    /// Schrittweite zwischen Fenstern in Samples.
    pub hop: usize,
    /// Bins je Fenster (`size / 2 + 1`).
    pub bins: usize,
    /// Anzahl der Fenster.
    pub frames: usize,
    /// `frames * bins` Werte, Fenster für Fenster.
    pub db: Vec<f32>,
}

impl Spectrogram {
    /// Rechnet das Spektrogramm eines Mono-Signals. `hop` darf nicht 0 sein.
    pub fn compute(mono: &[f32], rate: u32, size: usize, hop: usize) -> Self {
        let mut a = SpectrumAnalyzer::new(size);
        let size = a.size();
        let hop = hop.max(1);
        let bins = a.bins();
        let frames = if mono.len() < size {
            usize::from(!mono.is_empty())
        } else {
            (mono.len() - size) / hop + 1
        };
        let mut db = Vec::with_capacity(frames * bins);
        for f in 0..frames {
            let start = f * hop;
            let end = (start + size).min(mono.len());
            db.extend_from_slice(a.analyze(&mono[start..end]));
        }
        Self {
            size,
            rate,
            hop,
            bins,
            frames,
            db,
        }
    }

    /// Fenster `f` als Slice.
    pub fn frame(&self, f: usize) -> &[f32] {
        &self.db[f * self.bins..(f + 1) * self.bins]
    }

    /// Mittleres Spektrum über alle Fenster (Leistungsmittel je Bin, in dBFS).
    pub fn mean_db(&self) -> Vec<f32> {
        if self.frames == 0 {
            return vec![FLOOR_DB; self.bins];
        }
        (0..self.bins)
            .map(|k| {
                let p: f64 = (0..self.frames)
                    .map(|f| 10f64.powf(f64::from(self.db[f * self.bins + k]) / 10.0))
                    .sum::<f64>()
                    / self.frames as f64;
                (10.0 * p.log10()).max(f64::from(FLOOR_DB)) as f32
            })
            .collect()
    }

    /// Mischt zwei Kanäle zu Mono und rechnet das Spektrogramm.
    pub fn of_stereo(left: &[f32], right: &[f32], rate: u32, size: usize, hop: usize) -> Self {
        let mono: Vec<f32> = left.iter().zip(right).map(|(l, r)| (l + r) * 0.5).collect();
        Self::compute(&mono, rate, size, hop)
    }

    /// RGB-Bild (`width * height * 3` Bytes, Zeile 0 oben = höchste Frequenz): waagrecht die
    /// Zeit, senkrecht die Frequenz logarithmisch von [`MIN_HZ`] bis zur halben Abtastrate.
    /// Je Pixel der größte Pegel aus den zugehörigen Fenstern und Bins.
    pub fn render_rgb(&self, width: usize, height: usize) -> Vec<u8> {
        let (width, height) = (width.max(1), height.max(1));
        let mut img = vec![0u8; width * height * 3];
        if self.frames == 0 {
            return img;
        }
        let top = self.rate as f32 / 2.0;
        let ratio = (top / MIN_HZ).max(1.0001);
        // Bin-Bereich je Bildzeile (von unten nach oben wachsend).
        let rows: Vec<(usize, usize)> = (0..height)
            .map(|y| {
                let t0 = y as f32 / height as f32;
                let t1 = (y + 1) as f32 / height as f32;
                let lo = hz_bin(MIN_HZ * ratio.powf(t0), self.size, self.rate).max(1);
                let hi = hz_bin(MIN_HZ * ratio.powf(t1), self.size, self.rate).max(lo);
                (lo, hi)
            })
            .collect();
        for x in 0..width {
            let f0 = x * self.frames / width;
            let f1 = ((x + 1) * self.frames)
                .div_ceil(width)
                .clamp(f0 + 1, self.frames);
            for (y, &(lo, hi)) in rows.iter().enumerate() {
                let mut v = FLOOR_DB;
                for f in f0..f1 {
                    let frame = self.frame(f);
                    for &d in &frame[lo..=hi.min(self.bins - 1)] {
                        v = v.max(d);
                    }
                }
                let row = height - 1 - y;
                let o = (row * width + x) * 3;
                img[o..o + 3].copy_from_slice(&colour(v));
            }
        }
        img
    }
}

/// Farbe zu einem Pegel: von schwarz über blau, rot und orange zu weiß (−90 … 0 dB).
pub fn colour(db: f32) -> [u8; 3] {
    let t = ((db - MIN_DB) / -MIN_DB).clamp(0.0, 1.0);
    const STOPS: [(f32, [f32; 3]); 5] = [
        (0.0, [8.0, 10.0, 16.0]),
        (0.35, [40.0, 30.0, 120.0]),
        (0.6, [200.0, 50.0, 70.0]),
        (0.85, [255.0, 160.0, 40.0]),
        (1.0, [255.0, 250.0, 230.0]),
    ];
    for w in STOPS.windows(2) {
        let (a, b) = (w[0], w[1]);
        if t <= b.0 {
            let k = (t - a.0) / (b.0 - a.0);
            let m = |i: usize| (a.1[i] + (b.1[i] - a.1[i]) * k).round() as u8;
            return [m(0), m(1), m(2)];
        }
    }
    [255, 250, 230]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(freq: f32, secs: f32, rate: u32) -> Vec<f32> {
        (0..(rate as f32 * secs) as usize)
            .map(|i| 0.8 * (std::f32::consts::TAU * freq * i as f32 / rate as f32).sin())
            .collect()
    }

    /// Helligkeitsstufen als Zeichen, Bild auf `cols × rows` verkleinert (größter Wert).
    fn ascii(img: &[u8], w: usize, h: usize, cols: usize, rows: usize) -> String {
        let ramp = [' ', '.', ':', '+', '#', '@'];
        let mut out = String::new();
        for r in 0..rows {
            for c in 0..cols {
                let mut best = 0u32;
                for y in r * h / rows..(r + 1) * h / rows {
                    for x in c * w / cols..(c + 1) * w / cols {
                        let o = (y * w + x) * 3;
                        let lum = u32::from(img[o]) + u32::from(img[o + 1]) + u32::from(img[o + 2]);
                        best = best.max(lum);
                    }
                }
                out.push(ramp[(best as usize * ramp.len() / 766).min(ramp.len() - 1)]);
            }
            out.push('\n');
        }
        out
    }

    #[test]
    fn shape_and_frame_count() {
        let s = Spectrogram::compute(&tone(1000.0, 1.0, 48_000), 48_000, 2048, 1024);
        assert_eq!(s.bins, 1025);
        assert_eq!(s.frames, (48_000 - 2048) / 1024 + 1);
        assert_eq!(s.db.len(), s.frames * s.bins);
        let empty = Spectrogram::compute(&[], 48_000, 2048, 1024);
        assert_eq!(empty.frames, 0);
        assert_eq!(empty.render_rgb(4, 4).len(), 48);
    }

    #[test]
    fn tone_draws_a_horizontal_line_at_its_frequency() {
        let s = Spectrogram::of_stereo(
            &tone(1000.0, 2.0, 48_000),
            &tone(1000.0, 2.0, 48_000),
            48_000,
            2048,
            1024,
        );
        let (w, h) = (64, 128);
        let img = s.render_rgb(w, h);
        let lum = |x: usize, y: usize| {
            let o = (y * w + x) * 3;
            u32::from(img[o]) + u32::from(img[o + 1]) + u32::from(img[o + 2])
        };
        // Hellste Zeile bestimmen und mit 1 kHz auf der Log-Achse vergleichen.
        let row_sum = |y: usize| (0..w).map(|x| lum(x, y)).sum::<u32>();
        let brightest = (0..h).max_by_key(|&y| row_sum(y)).unwrap();
        let t = (1000.0f32 / MIN_HZ).ln() / (24_000.0f32 / MIN_HZ).ln();
        let expect = h - 1 - (t * h as f32) as usize;
        assert!(
            brightest.abs_diff(expect) <= 2,
            "{brightest} statt {expect}"
        );
        // Die Linie läuft über die ganze Breite.
        assert!(lum(1, brightest) > 600 && lum(w - 2, brightest) > 600);
    }

    #[test]
    fn colour_scale_is_monotone_in_brightness() {
        let sum = |db: f32| colour(db).iter().map(|&v| u32::from(v)).sum::<u32>();
        let mut last = 0;
        for db in (-90..=0).step_by(5) {
            let s = sum(db as f32);
            assert!(s >= last, "{db}");
            last = s;
        }
        assert_eq!(colour(-200.0), colour(MIN_DB));
    }

    /// Schnappschuss: Tonfolge 440 Hz, 2 kHz, 8 kHz als grobes Textbild.
    #[test]
    fn snapshot_three_tones() {
        let mut sig = tone(440.0, 1.0, 48_000);
        sig.extend(tone(2000.0, 1.0, 48_000));
        sig.extend(tone(8000.0, 1.0, 48_000));
        let s = Spectrogram::compute(&sig, 48_000, 2048, 1024);
        let (w, h) = (96, 96);
        let img = s.render_rgb(w, h);
        insta::assert_snapshot!(ascii(&img, w, h, 24, 16));
    }
}
