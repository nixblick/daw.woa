//! Transport: Play/Stop, Position in Samples, Loop-Bereich, Tempo.
//!
//! Der Transport ist eine Uhr, kein Stummschalter: Der Graph rechnet auch im Stopp
//! weiter (Live-Quellen im Radio-Betrieb), nur die Position bleibt stehen.
//! Der Audio-Thread schreibt Position und Zustand in Atomics; Leser (GUI, Agent)
//! lesen sie über [`TransportHandle`] ohne Sperre.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// Loop-Bereich in Samples, `start < end`. `end` ist exklusiv.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoopRange {
    pub start: u64,
    pub end: u64,
}

impl LoopRange {
    /// Prüft `start < end`.
    pub fn new(start: u64, end: u64) -> Option<Self> {
        (start < end).then_some(Self { start, end })
    }
}

/// Rechnet Beats bei `tempo_bpm` in Samples um (abgerundet, nie negativ).
pub fn beats_to_samples(beats: f64, tempo_bpm: f64, sample_rate: f32) -> u64 {
    if !(beats.is_finite() && tempo_bpm.is_finite() && tempo_bpm > 0.0 && beats > 0.0) {
        return 0;
    }
    let samples = beats * 60.0 / tempo_bpm * f64::from(sample_rate);
    // `as` sättigt bei f64 -> u64.
    samples.floor() as u64
}

/// Rechnet Samples in Beats um.
pub fn samples_to_beats(samples: u64, tempo_bpm: f64, sample_rate: f32) -> f64 {
    if sample_rate <= 0.0 {
        return 0.0;
    }
    samples as f64 / f64::from(sample_rate) * tempo_bpm / 60.0
}

/// Geteilter, lock-free lesbarer Zustand.
#[derive(Debug, Default)]
pub(crate) struct Shared {
    pub(crate) position: AtomicU64,
    pub(crate) playing: AtomicBool,
    pub(crate) tempo_bits: AtomicU64,
    pub(crate) underruns: AtomicU64,
}

/// Lesezugriff auf Transport und Zähler, beliebig oft klonbar.
#[derive(Debug, Clone)]
pub struct TransportHandle {
    pub(crate) shared: Arc<Shared>,
}

impl TransportHandle {
    /// Aktuelle Position in Samples.
    pub fn position(&self) -> u64 {
        self.shared.position.load(Ordering::Acquire)
    }

    /// Läuft der Transport?
    pub fn is_playing(&self) -> bool {
        self.shared.playing.load(Ordering::Acquire)
    }

    /// Tempo in BPM, wie zuletzt an die Engine geschickt.
    pub fn tempo_bpm(&self) -> f64 {
        f64::from_bits(self.shared.tempo_bits.load(Ordering::Acquire))
    }

    /// Bisher gezählte Unter- bzw. Überläufe des Geräts.
    pub fn underruns(&self) -> u64 {
        self.shared.underruns.load(Ordering::Relaxed)
    }

    /// Zählt einen Unterlauf (aufgerufen vom Geräte-Fehlerpfad). Echtzeitsicher.
    pub fn count_underrun(&self) {
        self.shared.underruns.fetch_add(1, Ordering::Relaxed);
    }
}

/// Transport-Zustand im Audio-Thread.
#[derive(Debug)]
pub(crate) struct Transport {
    position: u64,
    playing: bool,
    looping: Option<LoopRange>,
    shared: Arc<Shared>,
}

impl Transport {
    pub(crate) fn new(shared: Arc<Shared>) -> Self {
        Self {
            position: 0,
            playing: false,
            looping: None,
            shared,
        }
    }

    pub(crate) fn play(&mut self) {
        self.playing = true;
        self.shared.playing.store(true, Ordering::Release);
    }

    pub(crate) fn stop(&mut self) {
        self.playing = false;
        self.shared.playing.store(false, Ordering::Release);
    }

    pub(crate) fn seek(&mut self, position: u64) {
        self.position = position;
        self.publish();
    }

    pub(crate) fn set_loop(&mut self, range: Option<LoopRange>) {
        self.looping = range;
    }

    /// Länge des nächsten Abschnitts ohne Loop-Sprung, höchstens `max`.
    pub(crate) fn segment(&self, max: usize) -> usize {
        match self.looping {
            Some(l) if self.playing && self.position < l.end => {
                let left = l.end - self.position;
                usize::try_from(left).map_or(max, |left| left.min(max))
            }
            _ => max,
        }
    }

    /// Schiebt die Position um `frames` weiter, springt am Loop-Ende exakt zurück.
    pub(crate) fn advance(&mut self, frames: usize) {
        if !self.playing {
            return;
        }
        let before = self.position;
        self.position = self.position.saturating_add(frames as u64);
        if let Some(l) = self.looping
            && before < l.end
            && self.position >= l.end
        {
            self.position = l.start + (self.position - l.end);
        }
        self.publish();
    }

    fn publish(&self) {
        self.shared.position.store(self.position, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn beats_conversion() {
        assert_eq!(beats_to_samples(1.0, 120.0, 48_000.0), 24_000);
        assert_eq!(beats_to_samples(-1.0, 120.0, 48_000.0), 0);
        assert_eq!(beats_to_samples(1.0, 0.0, 48_000.0), 0);
        assert!((samples_to_beats(24_000, 120.0, 48_000.0) - 1.0).abs() < 1e-12);
    }

    #[test]
    fn loop_range_requires_order() {
        assert!(LoopRange::new(5, 5).is_none());
        assert!(LoopRange::new(5, 4).is_none());
        assert!(LoopRange::new(4, 5).is_some());
    }

    #[test]
    fn advance_wraps_exactly() {
        let mut t = Transport::new(Arc::default());
        t.play();
        t.set_loop(LoopRange::new(10, 20));
        t.seek(15);
        assert_eq!(t.segment(128), 5);
        t.advance(5);
        assert_eq!(t.position, 10);
    }

    #[test]
    fn past_loop_end_plays_through() {
        let mut t = Transport::new(Arc::default());
        t.play();
        t.seek(30);
        t.set_loop(LoopRange::new(10, 20));
        assert_eq!(t.segment(128), 128);
        t.advance(128);
        assert_eq!(t.position, 158);
    }

    #[test]
    fn stopped_does_not_advance() {
        let mut t = Transport::new(Arc::default());
        t.advance(100);
        assert_eq!(t.position, 0);
    }
}
