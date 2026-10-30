//! Abgriff fürs Analyse-Fenster (Paket 19): ein Stereo-Node, der mithört und die Samples über
//! einen lock-freien Ring (`rtrb`) an die Oberfläche gibt. Das Signal im Graphen bleibt
//! unverändert (Durchgang). Ist der Abgriff nicht eingeschaltet, kostet er einen
//! Atomic-Zugriff je Block. Ein voller Ring zählt verlorene Frames statt zu warten.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use woa_dsp::{StereoBuffers, StereoNode};

/// Frames, die der Ring fasst: gut eine Sekunde bei 48 kHz, falls die Oberfläche stockt.
pub const TAP_FRAMES: usize = 1 << 16;

/// Seite im Audio-Thread.
pub struct TapSink {
    producer: rtrb::Producer<f32>,
    enabled: Arc<AtomicBool>,
    dropped: Arc<AtomicU64>,
}

/// Seite der Oberfläche.
pub struct TapHandle {
    consumer: rtrb::Consumer<f32>,
    enabled: Arc<AtomicBool>,
    dropped: Arc<AtomicU64>,
    seen_dropped: u64,
}

/// Legt einen Abgriff an (zunächst ausgeschaltet).
pub fn tap() -> (TapSink, TapHandle) {
    let (producer, consumer) = rtrb::RingBuffer::new(TAP_FRAMES * 2);
    let enabled = Arc::new(AtomicBool::new(false));
    let dropped = Arc::new(AtomicU64::new(0));
    (
        TapSink {
            producer,
            enabled: Arc::clone(&enabled),
            dropped: Arc::clone(&dropped),
        },
        TapHandle {
            consumer,
            enabled,
            dropped,
            seen_dropped: 0,
        },
    )
}

impl StereoNode for TapSink {
    fn prepare(&mut self, _sample_rate: f32) {}

    fn set_param(&mut self, _param: u32, _value: f32) {}

    fn process(&mut self, b: StereoBuffers<'_>) {
        b.out_left.copy_from_slice(b.in_left);
        b.out_right.copy_from_slice(b.in_right);
        if !self.enabled.load(Ordering::Relaxed) {
            return;
        }
        let frames = b.in_left.len().min(b.in_right.len());
        // Ganze Frames oder gar nichts, damit links und rechts nie auseinanderlaufen.
        if self.producer.slots() < frames * 2 {
            self.dropped.fetch_add(frames as u64, Ordering::Relaxed);
            return;
        }
        for i in 0..frames {
            // Platz ist geprüft; ein Fehler wäre nur ein verlorenes Sample.
            let _ = self.producer.push(b.in_left[i]);
            let _ = self.producer.push(b.in_right[i]);
        }
    }
}

impl TapHandle {
    /// Schaltet das Mitschneiden ein oder aus. Beim Einschalten wird Altes verworfen.
    pub fn set_enabled(&mut self, on: bool) {
        if on && !self.enabled.load(Ordering::Relaxed) {
            self.discard();
        }
        self.enabled.store(on, Ordering::Relaxed);
    }

    /// Ist der Abgriff eingeschaltet?
    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    fn discard(&mut self) {
        let n = self.consumer.slots();
        if let Ok(chunk) = self.consumer.read_chunk(n) {
            chunk.commit_all();
        }
    }

    /// Holt alles Vorhandene in `left` und `right` (hängt an, ein Frame je Eintrag).
    /// Liefert die Zahl der Frames, die seit dem letzten Abholen verloren gingen.
    pub fn drain(&mut self, left: &mut Vec<f32>, right: &mut Vec<f32>) -> u64 {
        let n = self.consumer.slots() & !1;
        if n > 0
            && let Ok(chunk) = self.consumer.read_chunk(n)
        {
            let (a, b) = chunk.as_slices();
            // Die Teilung kann mitten in einem Frame liegen: einzeln paaren.
            let mut it = a.iter().chain(b.iter());
            while let (Some(&l), Some(&r)) = (it.next(), it.next()) {
                left.push(l);
                right.push(r);
            }
            chunk.commit_all();
        }
        let total = self.dropped.load(Ordering::Relaxed);
        let lost = total - self.seen_dropped;
        self.seen_dropped = total;
        lost
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(sink: &mut TapSink, l: &[f32], r: &[f32]) -> (Vec<f32>, Vec<f32>) {
        let (mut ol, mut or) = (vec![0.0; l.len()], vec![0.0; r.len()]);
        sink.process(StereoBuffers {
            in_left: l,
            in_right: r,
            out_left: &mut ol,
            out_right: &mut or,
        });
        (ol, or)
    }

    #[test]
    fn passes_signal_through_and_delivers_pairs() {
        let (mut sink, mut handle) = tap();
        let (l, r) = ([0.1, 0.2, 0.3], [-0.1, -0.2, -0.3]);
        // Ausgeschaltet: Durchgang, aber nichts im Ring.
        let (ol, or) = run(&mut sink, &l, &r);
        assert_eq!((ol, or), (l.to_vec(), r.to_vec()));
        let (mut a, mut b) = (Vec::new(), Vec::new());
        assert_eq!(handle.drain(&mut a, &mut b), 0);
        assert!(a.is_empty());
        handle.set_enabled(true);
        let (ol, _) = run(&mut sink, &l, &r);
        assert_eq!(ol, l.to_vec());
        handle.drain(&mut a, &mut b);
        assert_eq!((a, b), (l.to_vec(), r.to_vec()));
    }

    #[test]
    fn full_ring_counts_dropped_frames_and_keeps_pairs() {
        let (mut sink, mut handle) = tap();
        handle.set_enabled(true);
        let block = vec![0.5f32; 1024];
        for _ in 0..(TAP_FRAMES / 1024 + 2) {
            run(&mut sink, &block, &block);
        }
        let (mut a, mut b) = (Vec::new(), Vec::new());
        let lost = handle.drain(&mut a, &mut b);
        assert_eq!(a.len(), TAP_FRAMES);
        assert_eq!(a.len(), b.len());
        assert_eq!(lost, 2048);
        // Danach passt wieder etwas hinein.
        run(&mut sink, &block, &block);
        assert_eq!(handle.drain(&mut a, &mut b), 0);
        assert_eq!(a.len(), TAP_FRAMES + 1024);
    }

    #[test]
    fn enabling_discards_stale_samples() {
        let (mut sink, mut handle) = tap();
        handle.set_enabled(true);
        run(&mut sink, &[1.0; 8], &[1.0; 8]);
        handle.set_enabled(false);
        handle.set_enabled(true);
        let (mut a, mut b) = (Vec::new(), Vec::new());
        handle.drain(&mut a, &mut b);
        assert!(a.is_empty());
    }
}
