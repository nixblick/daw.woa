//! Wellenform während der Aufnahme (#43): Spitzenwerte je [`PEAK_BLOCK`] Frames, vom
//! Aufnahme-Thread über einen lock-freien Ring (`rtrb`) an die Oberfläche.
//!
//! [`PeakCollector`] sammelt verschachtelte Samples beliebig zerstückelt (der Schreib-Thread
//! holt sie einzeln aus der Queue) und schiebt je Block den Betrag des lautesten Samples über
//! alle Kanäle in den Ring: dieselbe Einheit wie [`crate::clip::peaks`] auf der Mono-Datei.
//! Kein Lock, keine Allokation nach dem Anlegen. Ist der Ring voll, zählt er die verlorenen
//! Blöcke; [`LivePeaks::drain_into`] füllt sie als Stille auf, damit die Zeitachse stimmt.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::clip::PEAK_BLOCK;

/// Platz im Ring: rund drei Minuten bei 48 kHz, falls die Oberfläche länger nicht abholt.
pub const RING_BLOCKS: usize = 1 << 15;

/// Erzeugerseite, lebt im Aufnahme-Thread.
pub struct PeakCollector {
    producer: rtrb::Producer<f32>,
    dropped: Arc<AtomicU64>,
    channels: usize,
    /// Kanal des nächsten Samples im laufenden Frame.
    channel: usize,
    /// Frames im laufenden Block.
    frames: usize,
    peak: f32,
}

/// Abholerseite für die Oberfläche.
pub struct LivePeaks {
    consumer: rtrb::Consumer<f32>,
    dropped: Arc<AtomicU64>,
}

/// Legt Sammler und Abholer für `channels` verschachtelte Kanäle an.
pub fn live_peaks(channels: u16) -> (PeakCollector, LivePeaks) {
    let (producer, consumer) = rtrb::RingBuffer::new(RING_BLOCKS);
    let dropped = Arc::new(AtomicU64::new(0));
    (
        PeakCollector {
            producer,
            dropped: Arc::clone(&dropped),
            channels: usize::from(channels.max(1)),
            channel: 0,
            frames: 0,
            peak: 0.0,
        },
        LivePeaks { consumer, dropped },
    )
}

impl PeakCollector {
    /// Nimmt ein Sample (verschachtelt, Kanal für Kanal).
    #[inline]
    pub fn push(&mut self, sample: f32) {
        self.peak = self.peak.max(sample.abs());
        self.channel += 1;
        if self.channel < self.channels {
            return;
        }
        self.channel = 0;
        self.frames += 1;
        if self.frames == PEAK_BLOCK {
            if self.producer.push(self.peak).is_err() {
                self.dropped.fetch_add(1, Ordering::Relaxed);
            }
            self.frames = 0;
            self.peak = 0.0;
        }
    }

    /// Nimmt einen Abschnitt verschachtelter Samples.
    pub fn push_slice(&mut self, samples: &[f32]) {
        for &s in samples {
            self.push(s);
        }
    }
}

impl LivePeaks {
    /// Hängt alle bisher fertigen Blöcke an `out` an; verlorene Blöcke als 0.
    pub fn drain_into(&mut self, out: &mut Vec<f32>) {
        let lost = self.dropped.swap(0, Ordering::Relaxed);
        out.extend(std::iter::repeat_n(0.0, lost as usize));
        while let Ok(p) = self.consumer.pop() {
            out.push(p);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clip::peaks;

    #[test]
    fn matches_peaks_of_the_mono_file() {
        // Stereo, rechts lauter als links, zerstückelt angeliefert.
        let frames = PEAK_BLOCK * 3 + 17;
        let mut inter = Vec::new();
        let mut mono = Vec::new();
        for i in 0..frames {
            let l = ((i as f32) * 0.01).sin() * 0.3;
            let r = if i == PEAK_BLOCK + 5 { -0.9 } else { l * 0.5 };
            inter.extend([l, r]);
            mono.push(l.abs().max(r.abs()));
        }
        let (mut c, mut live) = live_peaks(2);
        for chunk in inter.chunks(37) {
            c.push_slice(chunk);
        }
        let mut got = Vec::new();
        live.drain_into(&mut got);
        let want = peaks(&mono);
        // Der angefangene letzte Block kommt erst, wenn er voll ist.
        assert_eq!(got.len(), 3);
        assert_eq!(got[..], want[..3]);
        assert_eq!(got[1], 0.9);
    }

    #[test]
    fn mono_and_incremental_drain() {
        let (mut c, mut live) = live_peaks(1);
        let mut got = Vec::new();
        c.push_slice(&vec![0.25; PEAK_BLOCK]);
        live.drain_into(&mut got);
        assert_eq!(got, [0.25]);
        c.push_slice(&vec![-0.5; PEAK_BLOCK]);
        live.drain_into(&mut got);
        assert_eq!(got, [0.25, 0.5]);
    }

    #[test]
    fn full_ring_keeps_the_timeline() {
        let (mut c, mut live) = live_peaks(1);
        let blocks = RING_BLOCKS + 5;
        c.push_slice(&vec![0.1; PEAK_BLOCK * blocks]);
        let mut got = Vec::new();
        live.drain_into(&mut got);
        assert_eq!(got.len(), blocks);
        assert_eq!(got[..5], [0.0; 5]);
    }
}
