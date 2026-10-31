//! Jitter-Puffer für den Sendemix in `woa-streamd`.
//!
//! Die Oberfläche liefert den Sendemix im Takt ihrer Soundkarte und in Häppchen (Feeder alle
//! 20 ms, dazu Puffergröße des Treibers und Planer-Zittern, auf alten Windows-Rechnern
//! leicht 30 bis 100 ms). `woa-streamd` kodiert im Takt der Systemuhr. Beide Uhren laufen
//! nie ganz gleich. Früher (0,1 s Vorlauf, Kürzung bei 0,5 s) riss das alle ein bis zwei
//! Minuten ein Loch in den Stream: Unterlauf = 0,1 s Stille, Überlauf = 0,4 s Ton
//! weggeschnitten (30.10.2026: 28 Unterläufe in 36 min, `streamd-status.json`).
//!
//! Jetzt: Zielstand [`TARGET_SECONDS`] (0,5 s; der Stream hat ohnehin Sekunden Vorlauf beim Hörer). Verlässt der Stand das Band ± [`BAND_SECONDS`],
//! wird je Block genau ein Frame weggelassen oder wiederholt (bis ≈ 1100 ppm Ausgleich bei
//! 20-ms-Blöcken, unhörbar, keine Rechenlast). Gekürzt wird nur noch als Notbremse über
//! [`MAX_SECONDS`].

use std::collections::VecDeque;

/// Zielstand des Puffers.
pub const TARGET_SECONDS: f32 = 0.5;
/// Innerhalb von Ziel ± Band wird nichts nachgeregelt.
pub const BAND_SECONDS: f32 = 0.1;
/// Darüber wird hart auf das Ziel gekürzt (nur Notbremse, z. B. nach Standby).
pub const MAX_SECONDS: f32 = 2.0;

/// Stereo verschachtelt (`l, r, l, r, …`).
#[derive(Debug)]
pub struct LiveBuffer {
    q: VecDeque<f32>,
    target: usize,
    band: usize,
    max: usize,
    buffering: bool,
    last: (f32, f32),
    /// Unterläufe bei verbundener Oberfläche.
    pub underruns: u32,
    /// Harte Kürzungen (Notbremse).
    pub trims: u32,
    /// Weggelassene bzw. wiederholte Frames (Uhrenausgleich).
    pub dropped: u64,
    pub repeated: u64,
}

impl LiveBuffer {
    pub fn new(rate: u32) -> Self {
        let frames = |s: f32| (s * rate as f32) as usize;
        Self {
            q: VecDeque::with_capacity(frames(MAX_SECONDS) * 2 + 4096),
            target: frames(TARGET_SECONDS),
            band: frames(BAND_SECONDS),
            max: frames(MAX_SECONDS),
            buffering: true,
            last: (0.0, 0.0),
            underruns: 0,
            trims: 0,
            dropped: 0,
            repeated: 0,
        }
    }

    /// Gepufferte Frames.
    pub fn frames(&self) -> usize {
        self.q.len() / 2
    }

    /// Gepufferte Zeit in Millisekunden bei `rate`.
    pub fn millis(&self, rate: u32) -> u64 {
        (self.frames() as u64 * 1000) / u64::from(rate.max(1))
    }

    /// Hängt Samples an (Stereo verschachtelt; ein ungerader Rest wird verworfen).
    pub fn push(&mut self, samples: &[f32]) {
        let n = samples.len() & !1;
        self.q.extend(samples[..n].iter().copied());
        if self.frames() > self.max {
            let cut = (self.frames() - self.target) * 2;
            self.q.drain(..cut);
            self.trims = self.trims.saturating_add(1);
        }
    }

    /// Füllt einen Block. `count`: Unterlauf zählen (nur bei verbundener Oberfläche).
    /// Gibt zurück, ob Sendemix geliefert wurde (sonst Stille im Block).
    pub fn pull(&mut self, l: &mut [f32], r: &mut [f32], count: bool) -> bool {
        let n = l.len().min(r.len());
        if self.buffering {
            if self.frames() >= self.target {
                self.buffering = false;
            } else {
                l.fill(0.0);
                r.fill(0.0);
                return false;
            }
        }
        if self.frames() < n {
            // Unterlauf: was da ist, dann Stille, und wieder bis zum Ziel vorlaufen.
            let have = self.frames();
            for i in 0..n {
                if i < have {
                    l[i] = self.q.pop_front().unwrap_or(0.0);
                    r[i] = self.q.pop_front().unwrap_or(0.0);
                } else {
                    l[i] = 0.0;
                    r[i] = 0.0;
                }
            }
            self.buffering = true;
            if count {
                self.underruns = self.underruns.saturating_add(1);
            }
            return have > 0;
        }
        let level = self.frames();
        let repeat = level + self.band < self.target;
        let drop = level > self.target + self.band;
        // Wiederholen: der erste Frame des Blocks kommt doppelt (ein Frame weniger verbraucht).
        let (start, (mut pl, mut pr)) = if repeat {
            self.repeated += 1;
            (1, self.last)
        } else {
            (0, self.last)
        };
        if repeat && n > 0 {
            l[0] = pl;
            r[0] = pr;
        }
        for i in start..n {
            pl = self.q.pop_front().unwrap_or(0.0);
            pr = self.q.pop_front().unwrap_or(0.0);
            l[i] = pl;
            r[i] = pr;
        }
        if drop && self.frames() > 0 {
            // Weglassen: ein Frame mehr verbraucht.
            self.q.pop_front();
            self.q.pop_front();
            self.dropped += 1;
        }
        self.last = (pl, pr);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 44_100;
    const BLOCK: usize = 882; // 20 ms
    const RAMP: u64 = 1 << 20;

    /// Fake-Quelle: Rampe (Wert = Frame-Nummer), in unregelmäßigen Häppchen, mit
    /// Uhrenabweichung `ppm` und Hängern. Fake-Senke: Blöcke im Takt der Systemuhr.
    /// Prüft, dass nach dem Vorlauf keine Lücke und kein Sprung entsteht.
    fn simulate(ppm: f64, seconds: u32, stall_every_s: u32, stall_ms: u32) -> LiveBuffer {
        let mut b = LiveBuffer::new(RATE);
        let mut next_frame: u64 = 0; // Quelle
        let mut src_due = 0.0f64; // Frames, die die Quelle bis jetzt geliefert haben sollte
        let mut pending: Vec<f32> = Vec::new();
        let mut seen: Option<f32> = None;
        let mut started = false;
        let (mut l, mut r) = (vec![0.0; BLOCK], vec![0.0; BLOCK]);
        let ms_total = seconds * 1000;
        let mut stalled_until = 0u32;
        // Häppchengrößen in ms: Feeder 20 ms plus Zittern bis 100 ms (Pseudozufall).
        let mut lcg: u32 = 12345;
        let mut next_push = 0u32;
        for ms in 0..ms_total {
            // Quelle erzeugt im eigenen Takt.
            src_due += f64::from(RATE) * (1.0 + ppm / 1e6) / 1000.0;
            while (next_frame as f64) < src_due {
                // Unter 2^20 bleiben, damit f32 die Rampe exakt trägt.
                let v = (next_frame % RAMP) as f32;
                pending.push(v);
                pending.push(-v);
                next_frame += 1;
            }
            if stall_every_s > 0 && ms > 0 && ms % (stall_every_s * 1000) == 0 {
                stalled_until = ms + stall_ms;
            }
            if ms >= next_push && ms >= stalled_until {
                b.push(&pending);
                pending.clear();
                lcg = lcg.wrapping_mul(1_103_515_245).wrapping_add(12345);
                next_push = ms + 20 + (lcg >> 16) % 80;
            }
            // Senke: alle 20 ms ein Block.
            if ms % 20 == 19 {
                let live = b.pull(&mut l, &mut r, true);
                if !live {
                    assert!(!started, "Lücke bei {ms} ms (Stand {} ms)", b.millis(RATE));
                    continue;
                }
                started = true;
                for i in 0..BLOCK {
                    assert_eq!(l[i], -r[i], "Kanäle vertauscht");
                    if let Some(p) = seen {
                        let step = (l[i] - p).rem_euclid(RAMP as f32);
                        assert!(
                            (0.0..=2.0).contains(&step),
                            "Sprung {step} bei {ms} ms (Stand {} ms)",
                            b.millis(RATE)
                        );
                    }
                    seen = Some(l[i]);
                }
            }
        }
        b
    }

    #[test]
    fn no_gap_with_jitter_stalls_and_fast_source_clock() {
        let b = simulate(1000.0, 600, 30, 200);
        assert_eq!(b.underruns, 0);
        assert_eq!(b.trims, 0);
        assert!(b.dropped > 0, "Uhrenausgleich greift");
    }

    #[test]
    fn no_gap_with_jitter_stalls_and_slow_source_clock() {
        let b = simulate(-1000.0, 600, 30, 200);
        assert_eq!(b.underruns, 0);
        assert_eq!(b.trims, 0);
        assert!(b.repeated > 0, "Uhrenausgleich greift");
    }

    #[test]
    fn no_gap_with_matching_clocks() {
        let b = simulate(0.0, 120, 10, 250);
        assert_eq!((b.underruns, b.trims), (0, 0));
        // Nur bei Hängern kurz nachgeregelt: weit unter 1 % der Blöcke.
        assert!(
            b.dropped + b.repeated < 120,
            "{} / {}",
            b.dropped,
            b.repeated
        );
    }

    #[test]
    fn underrun_counts_and_rebuffers() {
        let mut b = LiveBuffer::new(RATE);
        let (mut l, mut r) = (vec![0.0; BLOCK], vec![0.0; BLOCK]);
        assert!(!b.pull(&mut l, &mut r, true), "Vorlauf");
        b.push(&vec![0.5; (b.target + 10) * 2]);
        while b.pull(&mut l, &mut r, true) && b.underruns == 0 {}
        assert_eq!(b.underruns, 1);
        assert!(!b.pull(&mut l, &mut r, true), "wieder Vorlauf");
        assert!(l.iter().all(|x| *x == 0.0));
    }

    #[test]
    fn overflow_trims_to_target() {
        let mut b = LiveBuffer::new(RATE);
        b.push(&vec![0.1; (b.max + 1) * 2]);
        assert_eq!(b.trims, 1);
        assert_eq!(b.frames(), b.target);
    }
}
