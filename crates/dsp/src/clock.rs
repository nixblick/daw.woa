//! Sample-Uhr (Paket 12): Der Audio-Thread meldet je Block Transportposition und
//! Uhrzeit, der MIDI-Thread rechnet damit die Ankunftszeit eines Events in eine
//! Transportposition in Samples um. So hängt die Aufnahme nicht am Takt der Oberfläche
//! und nicht an der Blockgröße.
//!
//! Lock-free: ein Sequenzzähler (Seqlock) schützt das Paar Position/Zeit. Der Schreiber
//! (Audio-Thread) wartet nie, der Leser wiederholt höchstens.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering, fence};
use std::time::Instant;

/// Gemeinsame Uhr zwischen Audio-Thread (schreibt) und MIDI-Thread (liest).
#[derive(Debug)]
pub struct SampleClock {
    epoch: Instant,
    seq: AtomicU64,
    position: AtomicU64,
    nanos: AtomicU64,
    playing: AtomicBool,
    rate_bits: AtomicU32,
}

impl Default for SampleClock {
    fn default() -> Self {
        Self::new()
    }
}

/// Ein gelesener Stand der Uhr.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClockReading {
    /// Transportposition am Blockanfang (Samples).
    pub position: u64,
    /// Zeitpunkt des Blockanfangs in Nanosekunden seit dem Anlegen der Uhr.
    pub nanos: u64,
    /// Läuft der Transport?
    pub playing: bool,
    /// Abtastrate des Graphen.
    pub rate: f32,
}

impl SampleClock {
    pub fn new() -> Self {
        Self {
            epoch: Instant::now(),
            seq: AtomicU64::new(0),
            position: AtomicU64::new(0),
            nanos: AtomicU64::new(0),
            playing: AtomicBool::new(false),
            rate_bits: AtomicU32::new(0),
        }
    }

    /// Nanosekunden seit dem Anlegen, für `t` (vor dem Anlegen: 0).
    pub fn nanos_of(&self, t: Instant) -> u64 {
        u64::try_from(t.saturating_duration_since(self.epoch).as_nanos()).unwrap_or(u64::MAX)
    }

    /// Audio-Thread, am Blockanfang: Position, Zustand und Rate zur Zeit `now`.
    /// Echtzeitsicher (nur Atomics).
    pub fn publish_at(&self, position: u64, playing: bool, rate: f32, now: Instant) {
        let nanos = self.nanos_of(now);
        self.seq.fetch_add(1, Ordering::AcqRel);
        fence(Ordering::Release);
        self.position.store(position, Ordering::Relaxed);
        self.nanos.store(nanos, Ordering::Relaxed);
        self.playing.store(playing, Ordering::Relaxed);
        self.rate_bits.store(rate.to_bits(), Ordering::Relaxed);
        self.seq.fetch_add(1, Ordering::Release);
    }

    /// Wie [`SampleClock::publish_at`] mit der aktuellen Zeit.
    pub fn publish(&self, position: u64, playing: bool, rate: f32) {
        self.publish_at(position, playing, rate, Instant::now());
    }

    /// Letzter vollständiger Stand; `None`, wenn noch nie gemeldet oder der Schreiber
    /// dauernd dazwischenkam.
    pub fn read(&self) -> Option<ClockReading> {
        for _ in 0..64 {
            let s1 = self.seq.load(Ordering::Acquire);
            if s1 == 0 {
                return None;
            }
            if s1 % 2 == 1 {
                std::hint::spin_loop();
                continue;
            }
            let r = ClockReading {
                position: self.position.load(Ordering::Relaxed),
                nanos: self.nanos.load(Ordering::Relaxed),
                playing: self.playing.load(Ordering::Relaxed),
                rate: f32::from_bits(self.rate_bits.load(Ordering::Relaxed)),
            };
            fence(Ordering::Acquire);
            if self.seq.load(Ordering::Relaxed) == s1 {
                return Some(r);
            }
        }
        None
    }

    /// Transportposition (Samples) zum Zeitpunkt `t`: letzte Blockposition plus die seit
    /// dem Blockanfang vergangene Zeit. `None`, wenn der Transport steht oder die Uhr
    /// noch nicht läuft.
    pub fn sample_at(&self, t: Instant) -> Option<u64> {
        let r = self.read()?;
        if !(r.playing && r.rate.is_finite() && r.rate > 0.0) {
            return None;
        }
        let now = self.nanos_of(t);
        let delta = now as f64 - r.nanos as f64;
        let s = r.position as f64 + delta * 1e-9 * f64::from(r.rate);
        Some(s.round().max(0.0) as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn interpolates_between_blocks() {
        let c = SampleClock::new();
        let t0 = Instant::now();
        assert_eq!(c.sample_at(t0), None, "not published yet");
        c.publish_at(48_000, true, 48_000.0, t0);
        assert_eq!(c.sample_at(t0), Some(48_000));
        assert_eq!(
            c.sample_at(t0 + Duration::from_millis(10)),
            Some(48_480),
            "10 ms later = 480 samples"
        );
        c.publish_at(48_000, false, 48_000.0, t0);
        assert_eq!(c.sample_at(t0), None, "stopped");
    }
}
