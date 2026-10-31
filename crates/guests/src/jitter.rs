//! Jitterpuffer je Gast: Vorlauf vor dem Start, Obergrenze gegen wachsende Verzögerung,
//! Aussetzer werden gezählt. Idee: `buffer` und `max` im Harbor-Eingang von Liquidsoap
//! (`src/core/sources/harbor_input.ml`, GPL-2.0-or-later); kein Code übernommen.
//!
//! Der Puffer ist ein `rtrb`-Ring mit Stereo verschachtelt (L, R, L, R ...). Das Netz schreibt
//! über [`GuestFeed`], der Audio-Thread liest über [`JitterReader`].

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

/// Vorlauf: so viel muss gepuffert sein, bevor die Wiedergabe beginnt (und nach einem Aussetzer
/// wieder beginnt).
pub const TARGET_SECONDS: f32 = 0.060;
/// Obergrenze: mehr wird nie gepuffert (sonst hörte der Gast sich immer später).
pub const MAX_SECONDS: f32 = 0.400;
/// Größe des Rings, immer größer als die Obergrenze.
pub const RING_SECONDS: f32 = 1.0;

/// Geteilte Zähler eines Kanals (lock-frei).
#[derive(Debug)]
pub struct SlotStats {
    pub active: AtomicBool,
    /// Wartet auf den Vorlauf (Start oder nach Aussetzer).
    pub prebuffering: AtomicBool,
    /// Gepufferte Frames beim letzten Block.
    pub buffered: AtomicU32,
    /// Spitze seit dem letzten [`SlotStats::take_peak`] (f32-Bits, nach dem Fader).
    peak: AtomicU32,
    /// Spitze vor dem Fader (f32-Bits): zeigt, ob der Gast spricht, auch stumm oder im Warteraum.
    input_peak: AtomicU32,
    /// Aussetzer: der Puffer war leer, obwohl die Wiedergabe lief.
    pub underruns: AtomicU64,
    /// Überläufe: Daten zurückgewiesen oder abgeschnitten, weil über der Obergrenze.
    pub overflows: AtomicU64,
    /// Dabei verworfene Frames.
    pub dropped_frames: AtomicU64,
}

impl Default for SlotStats {
    fn default() -> Self {
        Self {
            active: AtomicBool::new(false),
            prebuffering: AtomicBool::new(true),
            buffered: AtomicU32::new(0),
            peak: AtomicU32::new(0),
            input_peak: AtomicU32::new(0),
            underruns: AtomicU64::new(0),
            overflows: AtomicU64::new(0),
            dropped_frames: AtomicU64::new(0),
        }
    }
}

impl SlotStats {
    /// Spitze seit dem letzten Aufruf (linear) und zurücksetzen.
    pub fn take_input_peak(&self) -> f32 {
        f32::from_bits(self.input_peak.swap(0, Ordering::Relaxed))
    }

    pub(crate) fn store_input_peak(&self, v: f32) {
        self.input_peak
            .fetch_max(v.max(0.0).to_bits(), Ordering::Relaxed);
    }

    pub fn take_peak(&self) -> f32 {
        f32::from_bits(self.peak.swap(0, Ordering::Relaxed))
    }

    pub(crate) fn store_peak(&self, v: f32) {
        // Nur nichtnegative Werte: die f32-Bits ordnen sich dann wie die Zahlen.
        self.peak.fetch_max(v.max(0.0).to_bits(), Ordering::Relaxed);
    }

    pub(crate) fn reset(&self) {
        self.peak.store(0, Ordering::Relaxed);
        self.input_peak.store(0, Ordering::Relaxed);
        self.buffered.store(0, Ordering::Relaxed);
        self.prebuffering.store(true, Ordering::Relaxed);
        self.underruns.store(0, Ordering::Relaxed);
        self.overflows.store(0, Ordering::Relaxed);
        self.dropped_frames.store(0, Ordering::Relaxed);
    }
}

/// Zahl der `f32` (Stereo verschachtelt, gerade) für `seconds` bei `rate`.
pub fn samples_for(seconds: f32, rate: u32) -> usize {
    (((seconds * rate.max(1) as f32) as usize) * 2).max(2)
}

/// Schreibseite (Netz- oder Quell-Thread). Nimmt nie mehr an, als die Obergrenze erlaubt.
pub struct GuestFeed {
    producer: rtrb::Producer<f32>,
    capacity: usize,
    max_samples: usize,
    stats: Arc<SlotStats>,
}

impl GuestFeed {
    /// Schreibt Stereo verschachtelt; gibt die Zahl der angenommenen Frames zurück. Was über
    /// die Obergrenze hinausgeht, wird verworfen und als Überlauf gezählt.
    pub fn push(&mut self, interleaved: &[f32]) -> usize {
        let want = interleaved.len() & !1;
        let occupied = self.capacity - self.producer.slots();
        let room = self.max_samples.saturating_sub(occupied) & !1;
        let n = want.min(room);
        let mut written = 0;
        for &v in &interleaved[..n] {
            if self.producer.push(v).is_err() {
                break;
            }
            written += 1;
        }
        let written = written & !1;
        if written < want {
            self.stats.overflows.fetch_add(1, Ordering::Relaxed);
            self.stats
                .dropped_frames
                .fetch_add(((want - written) / 2) as u64, Ordering::Relaxed);
        }
        written / 2
    }

    /// Gepufferte Frames.
    pub fn buffered_frames(&self) -> usize {
        (self.capacity - self.producer.slots()) / 2
    }
}

/// Leseseite im Audio-Thread.
pub struct JitterReader {
    consumer: rtrb::Consumer<f32>,
    target: usize,
    max: usize,
    prebuffering: bool,
    stats: Arc<SlotStats>,
}

impl JitterReader {
    /// Am Blockanfang: schneidet zu viel Vorlauf auf den Vorlaufwert ab und meldet den Füllstand.
    pub fn begin_block(&mut self) {
        let avail = self.consumer.slots();
        if avail > self.max {
            let drop = (avail - self.target) & !1;
            if let Ok(chunk) = self.consumer.read_chunk(drop) {
                chunk.commit_all();
            }
            self.stats.overflows.fetch_add(1, Ordering::Relaxed);
            self.stats
                .dropped_frames
                .fetch_add((drop / 2) as u64, Ordering::Relaxed);
        }
        self.stats
            .buffered
            .store((self.consumer.slots() / 2) as u32, Ordering::Relaxed);
        self.stats
            .prebuffering
            .store(self.prebuffering, Ordering::Relaxed);
    }

    /// Ein Frame; Stille, solange der Vorlauf fehlt oder der Puffer leer ist.
    #[inline]
    pub fn pop_frame(&mut self) -> (f32, f32) {
        let avail = self.consumer.slots();
        if self.prebuffering {
            if avail < self.target {
                return (0.0, 0.0);
            }
            self.prebuffering = false;
        }
        if avail < 2 {
            self.stats.underruns.fetch_add(1, Ordering::Relaxed);
            self.prebuffering = true;
            return (0.0, 0.0);
        }
        let l = self.consumer.pop().unwrap_or(0.0);
        let r = self.consumer.pop().unwrap_or(0.0);
        (l, r)
    }

    pub fn is_prebuffering(&self) -> bool {
        self.prebuffering
    }

    /// Wirft alles weg und beginnt wieder beim Vorlauf (Kanal frei gegeben).
    pub fn flush(&mut self) {
        let avail = self.consumer.slots();
        if let Ok(chunk) = self.consumer.read_chunk(avail) {
            chunk.commit_all();
        }
        self.prebuffering = true;
    }
}

/// Legt Schreib- und Leseseite für `rate` an.
pub fn pair(rate: u32, stats: Arc<SlotStats>) -> (GuestFeed, JitterReader) {
    let capacity = samples_for(RING_SECONDS, rate);
    let (producer, consumer) = rtrb::RingBuffer::new(capacity);
    let max = samples_for(MAX_SECONDS, rate);
    let target = samples_for(TARGET_SECONDS, rate);
    (
        GuestFeed {
            producer,
            capacity,
            max_samples: max,
            stats: Arc::clone(&stats),
        },
        JitterReader {
            consumer,
            target,
            max,
            prebuffering: true,
            stats,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 48_000;

    fn mk() -> (GuestFeed, JitterReader, Arc<SlotStats>) {
        let st = Arc::new(SlotStats::default());
        let (f, r) = pair(RATE, Arc::clone(&st));
        (f, r, st)
    }

    fn ramp(frames: usize) -> Vec<f32> {
        vec![0.5; frames * 2]
    }

    #[test]
    fn silent_until_prebuffer_is_full() {
        let (mut f, mut r, st) = mk();
        let target_frames = (TARGET_SECONDS * RATE as f32) as usize;
        f.push(&ramp(target_frames - 10));
        r.begin_block();
        assert_eq!(r.pop_frame(), (0.0, 0.0));
        assert!(r.is_prebuffering());
        f.push(&ramp(20));
        assert_eq!(r.pop_frame(), (0.5, 0.5));
        assert!(!r.is_prebuffering());
        assert_eq!(st.underruns.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn overflow_is_rejected_at_the_cap_and_counted() {
        let (mut f, _r, st) = mk();
        let max_frames = (MAX_SECONDS * RATE as f32) as usize;
        assert_eq!(f.push(&ramp(max_frames)), max_frames);
        assert_eq!(f.push(&ramp(100)), 0);
        assert_eq!(st.overflows.load(Ordering::Relaxed), 1);
        assert_eq!(st.dropped_frames.load(Ordering::Relaxed), 100);
        assert!(f.buffered_frames() <= max_frames);
    }

    #[test]
    fn reader_trims_excess_back_to_target() {
        let (mut f, mut r, st) = mk();
        let max_frames = (MAX_SECONDS * RATE as f32) as usize;
        f.push(&ramp(max_frames));
        // Der Leser stand lange still, dann schreibt das Netz bis zur Obergrenze; mit einer
        // kleineren Obergrenze auf der Leseseite würde er abschneiden. Hier prüft der Test die
        // Leseseite direkt.
        r.max = max_frames / 2;
        r.begin_block();
        let target_frames = (TARGET_SECONDS * RATE as f32) as usize;
        assert_eq!(st.buffered.load(Ordering::Relaxed) as usize, target_frames);
        assert_eq!(st.overflows.load(Ordering::Relaxed), 1);
        assert_eq!(
            st.dropped_frames.load(Ordering::Relaxed) as usize,
            max_frames - target_frames
        );
    }

    #[test]
    fn underrun_gives_silence_counts_once_and_rebuffers() {
        let (mut f, mut r, st) = mk();
        let target_frames = (TARGET_SECONDS * RATE as f32) as usize;
        f.push(&ramp(target_frames));
        for _ in 0..target_frames {
            assert_eq!(r.pop_frame(), (0.5, 0.5));
        }
        // Leer: viele Frames Stille, aber nur ein Aussetzer.
        for _ in 0..1000 {
            assert_eq!(r.pop_frame(), (0.0, 0.0));
        }
        assert_eq!(st.underruns.load(Ordering::Relaxed), 1);
        // Wenig Nachschub genügt nicht, erst der Vorlauf startet wieder.
        f.push(&ramp(100));
        assert_eq!(r.pop_frame(), (0.0, 0.0));
        f.push(&ramp(target_frames));
        assert_eq!(r.pop_frame(), (0.5, 0.5));
    }

    #[test]
    fn flush_empties_and_restarts_prebuffering() {
        let (mut f, mut r, _st) = mk();
        f.push(&ramp(4000));
        r.flush();
        assert!(r.is_prebuffering());
        assert_eq!(r.pop_frame(), (0.0, 0.0));
        assert_eq!(f.buffered_frames(), 0);
    }
}
