//! Was der Ausgabetreiber zuletzt gemeldet hat. Ohne Geräte-Feature lesbar, damit auch der
//! Sendeablauf (`woa-radio`) die Werte loggen kann (#595).

use std::sync::atomic::{AtomicU64, Ordering};

/// Zuletzt vom Treiber gemeldete Ausgangslatenz (Callback bis Lautsprecher) in Mikrosekunden,
/// 0 wenn unbekannt. Für den Latenzausgleich bei Aufnahmen.
pub static OUTPUT_LATENCY_US: AtomicU64 = AtomicU64::new(0);

/// Größe des letzten Ausgabe-Callbacks in Frames (was der Treiber wirklich liefert).
pub static OUTPUT_CALLBACK_FRAMES: AtomicU64 = AtomicU64::new(0);

/// [`OUTPUT_LATENCY_US`] als Dauer.
pub fn output_latency() -> std::time::Duration {
    std::time::Duration::from_micros(OUTPUT_LATENCY_US.load(Ordering::Relaxed))
}
