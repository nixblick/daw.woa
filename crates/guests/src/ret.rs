//! Rückweg ins Netz: Die Bank (Audio-Thread) schreibt je Gast den Mix-Minus in einen `rtrb`-Ring,
//! der Netz-Thread liest ihn, kodiert Opus und schickt ihn zum Browser.
//!
//! Der Ring ist wie bei den Gästen Stereo verschachtelt (L, R, L, R ...) in der Rate der Engine.
//! Schreibseite: keine Allokation, kein Lock. Ist der Ring voll (Netz-Thread hängt), werden Frames
//! verworfen und gezählt; der Audio-Thread wartet nie.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

/// Größe des Rückweg-Rings in Sekunden.
pub const RETURN_RING_SECONDS: f32 = 1.0;

/// Zähler des Rückwegs eines Kanals.
#[derive(Debug, Default)]
pub struct ReturnStats {
    /// Verworfene Frames, weil der Ring voll war.
    pub dropped_frames: AtomicU64,
}

/// Schreibseite im Audio-Thread.
pub struct ReturnTx {
    producer: rtrb::Producer<f32>,
    stats: Arc<ReturnStats>,
}

impl ReturnTx {
    /// Ein Frame. Kein Platz: verwerfen und zählen.
    #[inline]
    pub fn push_frame(&mut self, l: f32, r: f32) {
        if self.producer.slots() >= 2 {
            let _ = self.producer.push(l);
            let _ = self.producer.push(r);
        } else {
            self.stats.dropped_frames.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// Leseseite im Netz-Thread.
pub struct ReturnRx {
    consumer: rtrb::Consumer<f32>,
    stats: Arc<ReturnStats>,
}

impl ReturnRx {
    /// Verfügbare Frames.
    pub fn frames(&self) -> usize {
        self.consumer.slots() / 2
    }

    /// Liest bis zu `out.len() / 2` Frames; gibt die Zahl der Frames zurück.
    pub fn pop_into(&mut self, out: &mut [f32]) -> usize {
        let want = (out.len() / 2).min(self.frames()) * 2;
        let mut n = 0;
        while n < want {
            match self.consumer.pop() {
                Ok(v) => {
                    out[n] = v;
                    n += 1;
                }
                Err(_) => break,
            }
        }
        n / 2
    }

    /// Wirft alles Gepufferte weg (bei neuem Gast, damit nichts Altes zu hören ist).
    pub fn flush(&mut self) {
        let avail = self.consumer.slots();
        if let Ok(chunk) = self.consumer.read_chunk(avail) {
            chunk.commit_all();
        }
    }

    /// Frames, die verworfen wurden, weil dieser Leser zu langsam war.
    pub fn dropped_frames(&self) -> u64 {
        self.stats.dropped_frames.load(Ordering::Relaxed)
    }
}

/// Legt Schreib- und Leseseite für `rate` an.
pub fn pair(rate: u32) -> (ReturnTx, ReturnRx) {
    let cap = crate::jitter::samples_for(RETURN_RING_SECONDS, rate);
    let (producer, consumer) = rtrb::RingBuffer::new(cap);
    let stats = Arc::new(ReturnStats::default());
    (
        ReturnTx {
            producer,
            stats: Arc::clone(&stats),
        },
        ReturnRx { consumer, stats },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_arrive_in_order_and_overflow_is_counted() {
        let (mut tx, mut rx) = pair(100);
        for i in 0..100 {
            tx.push_frame(i as f32, -(i as f32));
        }
        // Ring fasst eine Sekunde = 100 Frames; weitere werden verworfen.
        tx.push_frame(9.0, 9.0);
        assert_eq!(rx.dropped_frames(), 1);
        let mut out = [0.0f32; 6];
        assert_eq!(rx.pop_into(&mut out), 3);
        assert_eq!(out, [0.0, 0.0, 1.0, -1.0, 2.0, -2.0]);
        rx.flush();
        assert_eq!(rx.frames(), 0);
    }
}
