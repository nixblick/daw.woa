//! Eingangs-Trim einer laufenden Aufnahme (Produzent). Die Datei bleibt roh; der Trim der Spur
//! wirkt beim Abspielen (vor den Inserts). Damit Aufnahme-Pegel und Mithören schon beim
//! Aufnehmen so klingen wie später die Spur, laufen sie durch denselben Trim.
//!
//! Der Wert steht als f32-Bits in einem Atomic: Der Audio-Callback liest ihn ohne Lock und
//! ohne Allokation, die Oberfläche ändert ihn jederzeit (Trim während der Aufnahme).

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

/// Linearer Faktor zu `db` (nicht endlich = 1, also unverändert).
pub fn db_to_gain(db: f64) -> f32 {
    if db.is_finite() {
        10f64.powf(db / 20.0) as f32
    } else {
        1.0
    }
}

/// Geteilter Trim-Faktor einer Aufnahme.
#[derive(Debug, Clone)]
pub struct TrimCell(Arc<AtomicU32>);

impl Default for TrimCell {
    fn default() -> Self {
        Self(Arc::new(AtomicU32::new(1.0f32.to_bits())))
    }
}

impl TrimCell {
    /// Setzt den Trim in dB.
    pub fn set_db(&self, db: f64) {
        self.0.store(db_to_gain(db).to_bits(), Ordering::Relaxed);
    }

    /// Aktueller linearer Faktor.
    pub fn gain(&self) -> f32 {
        f32::from_bits(self.0.load(Ordering::Relaxed))
    }
}

/// Schreibt `src` (f32 little endian, verschachtelt) mal `gain` nach `out` (wiederverwendet,
/// wächst nur, wenn ein Block größer ist als alle davor).
pub fn scale_f32le(src: &[u8], gain: f32, out: &mut Vec<u8>) {
    out.clear();
    let (samples, rest) = src.as_chunks::<4>();
    for b in samples {
        out.extend_from_slice(&(f32::from_le_bytes(*b) * gain).to_le_bytes());
    }
    out.extend_from_slice(rest);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn db_to_gain_matches_the_track_trim() {
        assert_eq!(db_to_gain(0.0), 1.0);
        assert!((db_to_gain(-6.0) - 0.501).abs() < 1e-3);
        assert!((db_to_gain(12.0) - 3.981).abs() < 1e-3);
        assert_eq!(db_to_gain(f64::NAN), 1.0);
    }

    #[test]
    fn trim_cell_starts_at_unity_and_follows_set_db() {
        let c = TrimCell::default();
        assert_eq!(c.gain(), 1.0);
        let shared = c.clone();
        shared.set_db(-6.0);
        assert!((c.gain() - 0.501).abs() < 1e-3, "geteilt");
    }

    /// Bekannter Pegel rein, getrimmter Pegel raus (Mithören).
    #[test]
    fn scale_f32le_applies_the_gain() {
        let src: Vec<u8> = [0.5f32, -0.25, 1.0]
            .iter()
            .flat_map(|s| s.to_le_bytes())
            .collect();
        let mut out = Vec::new();
        scale_f32le(&src, 0.5, &mut out);
        let got: Vec<f32> = out
            .as_chunks::<4>()
            .0
            .iter()
            .map(|b| f32::from_le_bytes(*b))
            .collect();
        assert_eq!(got, vec![0.25, -0.125, 0.5]);
        let cap = out.capacity();
        scale_f32le(&src, 2.0, &mut out);
        assert_eq!(out.capacity(), cap, "Puffer wird wiederverwendet");
    }
}
