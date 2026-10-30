//! Spektrum: Hann-Fenster, FFT (`realfft`), Pegel in dBFS je Bin.
//!
//! Ein Sinus mit Vollaussteuerung (Amplitude 1) auf einer Bin-Mitte zeigt 0 dB.

use std::sync::Arc;

use realfft::num_complex::Complex;
use realfft::{RealFftPlanner, RealToComplex};

/// Untergrenze der Anzeige in dB (Stille).
pub const FLOOR_DB: f32 = -120.0;

/// Spektrum-Rechner mit vorab angelegten Puffern.
pub struct SpectrumAnalyzer {
    size: usize,
    window: Vec<f32>,
    fft: Arc<dyn RealToComplex<f32>>,
    input: Vec<f32>,
    output: Vec<Complex<f32>>,
    scratch: Vec<Complex<f32>>,
    db: Vec<f32>,
}

impl SpectrumAnalyzer {
    /// Neuer Rechner für `size` Samples je Fenster (gerade, mindestens 16).
    pub fn new(size: usize) -> Self {
        let size = size.max(16) & !1;
        let fft = RealFftPlanner::<f32>::new().plan_fft_forward(size);
        let window = (0..size)
            .map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / size as f32).cos())
            .collect();
        Self {
            size,
            window,
            input: fft.make_input_vec(),
            output: fft.make_output_vec(),
            scratch: fft.make_scratch_vec(),
            db: vec![FLOOR_DB; size / 2 + 1],
            fft,
        }
    }

    /// Fenstergröße in Samples.
    pub fn size(&self) -> usize {
        self.size
    }

    /// Anzahl der Bins (`size / 2 + 1`).
    pub fn bins(&self) -> usize {
        self.size / 2 + 1
    }

    /// Rechnet das Spektrum der letzten `size` Samples (kürzere Eingabe wird vorne mit
    /// Stille aufgefüllt). Liefert dBFS je Bin.
    pub fn analyze(&mut self, samples: &[f32]) -> &[f32] {
        let n = self.size;
        let tail = &samples[samples.len().saturating_sub(n)..];
        let pad = n - tail.len();
        for (i, v) in self.input.iter_mut().enumerate() {
            *v = if i < pad {
                0.0
            } else {
                tail[i - pad] * self.window[i]
            };
        }
        // Längen stimmen durch `make_*_vec`; ein Fehler hieße, dass die Puffer nicht passen.
        if self
            .fft
            .process_with_scratch(&mut self.input, &mut self.output, &mut self.scratch)
            .is_err()
        {
            self.db.fill(FLOOR_DB);
            return &self.db;
        }
        // Hann: ein Sinus der Amplitude A auf einer Bin-Mitte ergibt |X| = A · N / 4.
        let norm = 4.0 / n as f32;
        for (d, c) in self.db.iter_mut().zip(&self.output) {
            let mag = c.norm() * norm;
            *d = if mag > 1e-6 {
                20.0 * mag.log10()
            } else {
                FLOOR_DB
            };
        }
        &self.db
    }
}

/// Mittenfrequenz eines Bins in Hz.
pub fn bin_hz(bin: usize, size: usize, rate: u32) -> f32 {
    bin as f32 * rate as f32 / size as f32
}

/// Bin zu einer Frequenz (gerundet, auf den gültigen Bereich begrenzt).
pub fn hz_bin(hz: f32, size: usize, rate: u32) -> usize {
    ((hz * size as f32 / rate as f32).round().max(0.0) as usize).min(size / 2)
}

/// Bin mit dem größten Pegel (ohne Gleichanteil); `None` bei leerer Eingabe.
pub fn peak_bin(db: &[f32]) -> Option<usize> {
    db.iter()
        .enumerate()
        .skip(1)
        .max_by(|a, b| a.1.total_cmp(b.1))
        .map(|(i, _)| i)
}

/// Kleinste Frequenz der logarithmischen Anzeige in Hz.
pub const DISPLAY_MIN_HZ: f32 = 20.0;

/// Größte Frequenz der logarithmischen Anzeige in Hz (oder die halbe Abtastrate, wenn kleiner).
pub const DISPLAY_MAX_HZ: f32 = 20_000.0;

/// Bringt ein Spektrum auf `n` logarithmisch verteilte Anzeigebänder von [`DISPLAY_MIN_HZ`] bis
/// [`DISPLAY_MAX_HZ`]: je Band der größte Pegel seiner Bins; wo ein Band zwischen zwei Bins liegt
/// (Bässe), wird linear zwischen ihnen gemischt.
pub fn log_bands(db: &[f32], size: usize, rate: u32, n: usize) -> Vec<f32> {
    let top = DISPLAY_MAX_HZ.min(rate as f32 / 2.0);
    let ratio = top / DISPLAY_MIN_HZ;
    let res = rate as f32 / size as f32;
    (0..n)
        .map(|i| {
            let lo = DISPLAY_MIN_HZ * ratio.powf(i as f32 / n as f32);
            let hi = DISPLAY_MIN_HZ * ratio.powf((i + 1) as f32 / n as f32);
            let (b0, b1) = ((lo / res).ceil() as usize, (hi / res).floor() as usize);
            if b1 >= b0 && b0 < db.len() {
                db[b0..=b1.min(db.len() - 1)]
                    .iter()
                    .copied()
                    .fold(FLOOR_DB, f32::max)
            } else {
                let x = ((lo * hi).sqrt() / res).max(0.0);
                let k = (x.floor() as usize).min(db.len().saturating_sub(2));
                let f = (x - k as f32).clamp(0.0, 1.0);
                db.get(k)
                    .zip(db.get(k + 1))
                    .map_or(FLOOR_DB, |(a, b)| a + (b - a) * f)
            }
        })
        .collect()
}

/// Mittenfrequenzen der Oktavbänder (ISO), von 31,5 Hz bis 16 kHz.
pub const OCTAVE_CENTRES: [f32; 10] = [
    31.5, 63.0, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0,
];

/// Pegel je Oktavband in dBFS aus einem Spektrum von [`SpectrumAnalyzer::analyze`]: die
/// Leistung der Bins im Band, durch 1,5 (Rauschbandbreite des Hann-Fensters) geteilt, damit
/// ein Sinus im Band seinen eigenen Pegel zeigt. Bänder über der halben Abtastrate fehlen.
pub fn octave_bands(db: &[f32], size: usize, rate: u32) -> Vec<(f32, f32)> {
    let nyquist = rate as f32 / 2.0;
    OCTAVE_CENTRES
        .iter()
        .filter(|&&c| c / 2f32.sqrt() < nyquist)
        .map(|&c| {
            let lo = hz_bin(c / 2f32.sqrt(), size, rate).max(1);
            let hi = hz_bin((c * 2f32.sqrt()).min(nyquist), size, rate);
            let power: f64 = (lo..hi.max(lo))
                .filter_map(|k| db.get(k))
                .map(|&d| 10f64.powf(f64::from(d) / 10.0))
                .sum();
            let level = if power > 0.0 {
                (10.0 * (power / 1.5).log10()) as f32
            } else {
                FLOOR_DB
            };
            (c, level.max(FLOOR_DB))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine_at_bin(bin: usize, size: usize, amp: f32, len: usize) -> Vec<f32> {
        (0..len)
            .map(|i| amp * (std::f32::consts::TAU * bin as f32 * i as f32 / size as f32).sin())
            .collect()
    }

    #[test]
    fn sine_lands_on_its_bin_with_its_level() {
        let size = 4096;
        let mut a = SpectrumAnalyzer::new(size);
        for bin in [10usize, 93, 512, 1500] {
            let s = sine_at_bin(bin, size, 1.0, size);
            let db = a.analyze(&s).to_vec();
            assert_eq!(peak_bin(&db), Some(bin));
            assert!(db[bin].abs() < 0.1, "bin {bin}: {} dB", db[bin]);
            // Weit weg vom Sinus ist es still (Hann-Seitenzipfel).
            assert!(db[(bin + 300) % 2048] < -80.0);
        }
        // Halbe Amplitude: −6 dB.
        let s = sine_at_bin(100, size, 0.5, size);
        assert!((a.analyze(&s)[100] + 6.02).abs() < 0.1);
    }

    #[test]
    fn bin_frequency_maps_back() {
        // 48 kHz, 4096: Bin 93 ist 1089,84 Hz.
        assert!((bin_hz(93, 4096, 48_000) - 1089.84).abs() < 0.01);
        assert_eq!(hz_bin(1089.84, 4096, 48_000), 93);
        assert_eq!(hz_bin(1e9, 4096, 48_000), 2048);
    }

    #[test]
    fn silence_and_short_input_are_safe() {
        let mut a = SpectrumAnalyzer::new(1024);
        assert!(a.analyze(&[]).iter().all(|&d| d == FLOOR_DB));
        assert!(a.analyze(&[0.0; 10]).iter().all(|&d| d == FLOOR_DB));
        // Kürzere Eingabe als Fenster: Ton ist trotzdem zu sehen.
        let s = sine_at_bin(50, 1024, 1.0, 600);
        assert_eq!(peak_bin(a.analyze(&s)), Some(50));
    }

    #[test]
    fn log_bands_put_the_tone_at_its_log_position() {
        let size = 4096;
        let mut a = SpectrumAnalyzer::new(size);
        let s = sine_at_bin(93, size, 1.0, size);
        let bands = log_bands(a.analyze(&s), size, 48_000, 200);
        assert_eq!(bands.len(), 200);
        let best = bands
            .iter()
            .enumerate()
            .max_by(|x, y| x.1.total_cmp(y.1))
            .unwrap()
            .0;
        let pos = (1089.84f32 / 20.0).ln() / (20_000.0f32 / 20.0).ln();
        assert!((best as f32 / 200.0 - pos).abs() < 0.01, "{best}");
        // Bässe zwischen zwei Bins bleiben ohne Loch.
        assert!(bands[..20].iter().all(|&d| d.is_finite()));
    }

    #[test]
    fn octave_band_shows_the_tone() {
        let size = 4096;
        let mut a = SpectrumAnalyzer::new(size);
        // Bin 93 ≈ 1090 Hz liegt im Band um 1 kHz.
        let s = sine_at_bin(93, size, 0.5, size);
        let bands = octave_bands(a.analyze(&s), size, 48_000);
        let (best, level) = bands
            .iter()
            .copied()
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .unwrap();
        assert_eq!(best, 1000.0);
        assert!((level + 6.02).abs() < 0.5, "{level}");
        // Die anderen Bänder liegen weit darunter.
        assert!(bands.iter().filter(|b| b.0 != 1000.0).all(|b| b.1 < -50.0));
    }
}
