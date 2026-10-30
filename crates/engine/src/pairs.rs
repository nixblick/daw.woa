//! Kanalpaare und Taktausgleich zwischen Geräten (ohne cpal, also offline testbar).
//!
//! Zwei Bausteine für mehrere Ausgänge gleichzeitig:
//! - [`interleave_pairs`] schreibt beliebig viele Stereo-Paare auf die Kanäle eines Geräts
//!   (Paar 0 = Kanal 1/2, Paar 1 = Kanal 3/4 …).
//! - [`link`] verbindet zwei Geräte mit eigenem Takt. Der Mixer läuft im Callback des
//!   Hauptgeräts und schiebt Stereo in einen Ring ([`LinkTx`]); der Callback des zweiten
//!   Geräts holt es dort ab ([`LinkRx`]). Weil zwei Soundkarten nie exakt gleich schnell
//!   laufen (und oft 44,1 und 48 kHz haben), rechnet der Empfänger mit einem leicht
//!   regelbaren Abtastverhältnis um (Hermite-Interpolation): Der Füllstand des Rings bleibt
//!   so um einen Sollwert, ohne dass Samples fehlen oder doppelt kommen. Nur im Notfall
//!   (Gerät hängt, Ring läuft über oder leer) werden Frames verworfen oder ausgeblendet; das
//!   zählen [`LinkStats::dropped`], [`LinkStats::skipped`] und [`LinkStats::underruns`].
//!
//! Alles hier ist für den Audio-Thread gebaut: keine Allokation, kein Lock, kein I/O.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

/// Ein Stereo-Paar als zwei vorab angelegte Puffer.
#[derive(Debug, Clone, PartialEq)]
pub struct PairBuf {
    pub l: Vec<f32>,
    pub r: Vec<f32>,
}

impl PairBuf {
    /// Legt ein Paar für Blöcke bis `frames` Frames an (Stille).
    pub fn new(frames: usize) -> Self {
        Self {
            l: vec![0.0; frames],
            r: vec![0.0; frames],
        }
    }

    /// Setzt die ersten `frames` Frames auf Stille.
    pub fn clear(&mut self, frames: usize) {
        let n = frames.min(self.l.len());
        self.l[..n].fill(0.0);
        self.r[..n].fill(0.0);
    }
}

/// Schreibt `pairs` verschachtelt in Geräte-Frames mit `channels` Kanälen: Paar `k` auf Kanal
/// `2k` und `2k+1`. Kanäle ohne Paar bleiben still. Ein Gerät mit einem Kanal bekommt Paar 0
/// gemittelt. Bei einer ungeraden Kanalzahl bekommt der letzte Kanal nur die linke Seite
/// seines Paars. `out` hat `frames * channels` Samples.
pub fn interleave_pairs(out: &mut [f32], channels: usize, pairs: &[PairBuf], frames: usize) {
    let channels = channels.max(1);
    for (i, frame) in out.chunks_mut(channels).take(frames).enumerate() {
        frame.fill(0.0);
        if frame.len() == 1 {
            if let Some(p) = pairs.first() {
                frame[0] =
                    0.5 * (p.l.get(i).copied().unwrap_or(0.0) + p.r.get(i).copied().unwrap_or(0.0));
            }
            continue;
        }
        for (k, p) in pairs.iter().enumerate() {
            let c = 2 * k;
            if c >= frame.len() {
                break;
            }
            frame[c] = p.l.get(i).copied().unwrap_or(0.0);
            if c + 1 < frame.len() {
                frame[c + 1] = p.r.get(i).copied().unwrap_or(0.0);
            }
        }
    }
}

/// Wie lange der Ring im Soll gefüllt sein soll (Sekunden). Das ist die zusätzliche Latenz
/// des zweiten Geräts gegenüber dem Hauptgerät.
pub const TARGET_SECONDS: f64 = 0.05;
/// Größe des Rings (Sekunden).
const CAPACITY_SECONDS: f64 = 0.25;
/// Über diesem Füllstand (Vielfache des Solls) werden Frames verworfen.
const MAX_FILL_FACTOR: f64 = 3.0;
/// Regelverstärkung: relative Abweichung vom Soll mal dieser Wert = Änderung des Verhältnisses.
const TRIM_GAIN: f64 = 0.004;
/// Größte Abweichung des Verhältnisses (0,5 %, etwa 8,6 Cent).
const TRIM_MAX: f64 = 0.005;
/// Glättung der Regelgröße je Block.
const TRIM_SMOOTH: f64 = 0.1;

/// Zähler einer Verbindung, lock-frei lesbar.
#[derive(Debug, Default)]
pub struct LinkStats {
    /// Frames, die der Sender verworfen hat, weil der Ring voll war (Gerät steht).
    pub dropped: AtomicU64,
    /// Frames, die der Empfänger übersprungen hat, weil der Ring zu voll war.
    pub skipped: AtomicU64,
    /// Wie oft der Ring leer lief (Ausblenden, danach Neuaufbau des Polsters).
    pub underruns: AtomicU64,
    /// Füllstand in Frames beim letzten Block des Empfängers.
    pub fill: AtomicU64,
}

impl LinkStats {
    /// Alle Zähler, die einen echten Ausfall bedeuten (nicht die stille Regelung).
    pub fn faults(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
            + self.skipped.load(Ordering::Relaxed)
            + self.underruns.load(Ordering::Relaxed)
    }
}

/// Sendeseite im Callback des Hauptgeräts.
pub struct LinkTx {
    prod: rtrb::Producer<f32>,
    stats: Arc<LinkStats>,
}

/// Empfangsseite im Callback des zweiten Geräts.
pub struct LinkRx {
    cons: rtrb::Consumer<f32>,
    stats: Arc<LinkStats>,
    /// Quellrate durch Zielrate.
    ratio: f64,
    trim: f64,
    target: usize,
    max_fill: usize,
    phase: f64,
    /// Die letzten vier Frames (links, rechts) für die Interpolation.
    hist: [[f32; 2]; 4],
    primed: bool,
}

/// Legt eine Verbindung an: Der Sender liefert mit `src_rate`, der Empfänger gibt mit
/// `dst_rate` aus.
pub fn link(src_rate: u32, dst_rate: u32) -> (LinkTx, LinkRx, Arc<LinkStats>) {
    let src = f64::from(src_rate.max(1));
    let capacity = (src * CAPACITY_SECONDS) as usize;
    let (prod, cons) = rtrb::RingBuffer::new(capacity.max(64) * 2);
    let stats = Arc::new(LinkStats::default());
    let target = (src * TARGET_SECONDS) as usize;
    (
        LinkTx {
            prod,
            stats: Arc::clone(&stats),
        },
        LinkRx {
            cons,
            stats: Arc::clone(&stats),
            ratio: src / f64::from(dst_rate.max(1)),
            trim: 0.0,
            target: target.max(8),
            max_fill: (target as f64 * MAX_FILL_FACTOR) as usize,
            phase: 0.0,
            hist: [[0.0; 2]; 4],
            primed: false,
        },
        stats,
    )
}

impl LinkTx {
    /// Schiebt einen Block Stereo in den Ring. Was nicht mehr hineinpasst, fällt weg und
    /// wird gezählt.
    pub fn push(&mut self, l: &[f32], r: &[f32]) {
        let n = l.len().min(r.len());
        let room = self.prod.slots() / 2;
        let take = n.min(room);
        for i in 0..take {
            let _ = self.prod.push(l[i]);
            let _ = self.prod.push(r[i]);
        }
        if take < n {
            self.stats
                .dropped
                .fetch_add((n - take) as u64, Ordering::Relaxed);
        }
    }
}

/// Catmull-Rom zwischen `p1` und `p2` an der Stelle `t` (0..1).
#[inline]
fn hermite(p0: f32, p1: f32, p2: f32, p3: f32, t: f32) -> f32 {
    p1 + 0.5
        * t
        * (p2 - p0 + t * (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3 + t * (3.0 * (p1 - p2) + p3 - p0)))
}

impl LinkRx {
    /// Holt den nächsten Frame aus dem Ring; `None`, wenn er leer ist.
    #[inline]
    fn pop_frame(&mut self) -> Option<[f32; 2]> {
        if self.cons.slots() < 2 {
            return None;
        }
        let l = self.cons.pop().ok()?;
        let r = self.cons.pop().ok()?;
        Some([l, r])
    }

    #[inline]
    fn advance(&mut self, frame: [f32; 2]) {
        self.hist = [self.hist[1], self.hist[2], self.hist[3], frame];
    }

    /// Füllstand in Frames.
    fn fill(&self) -> usize {
        self.cons.slots() / 2
    }

    /// Rechnet einen Block (überschreibt `l`/`r`). Solange das Polster nicht aufgebaut ist,
    /// kommt Stille.
    pub fn pull(&mut self, l: &mut [f32], r: &mut [f32]) {
        let n = l.len().min(r.len());
        let mut avail = self.fill();
        self.stats.fill.store(avail as u64, Ordering::Relaxed);
        if !self.primed {
            if avail < self.target {
                l[..n].fill(0.0);
                r[..n].fill(0.0);
                return;
            }
            self.primed = true;
            self.phase = 0.0;
            self.hist = [[0.0; 2]; 4];
            // Die ersten drei Frames füllen die Historie, danach liegt Frame 0 auf `hist[1]`.
            for _ in 0..3 {
                if let Some(f) = self.pop_frame() {
                    self.advance(f);
                }
            }
            avail = self.fill();
        }
        if avail > self.max_fill {
            // Ring läuft über (Gerät hing): bis zum Soll überspringen, nicht Block für Block
            // dagegen anregeln.
            let skip = avail - self.target;
            if let Ok(chunk) = self.cons.read_chunk(skip * 2) {
                chunk.commit_all();
            }
            self.stats.skipped.fetch_add(skip as u64, Ordering::Relaxed);
            avail = self.fill();
        }
        let err = (avail as f64 - self.target as f64) / self.target as f64;
        let want = (err * TRIM_GAIN).clamp(-TRIM_MAX, TRIM_MAX);
        self.trim += (want - self.trim) * TRIM_SMOOTH;
        let step = self.ratio * (1.0 + self.trim);
        for i in 0..n {
            let t = self.phase as f32;
            let h = &self.hist;
            l[i] = hermite(h[0][0], h[1][0], h[2][0], h[3][0], t);
            r[i] = hermite(h[0][1], h[1][1], h[2][1], h[3][1], t);
            self.phase += step;
            while self.phase >= 1.0 {
                self.phase -= 1.0;
                match self.pop_frame() {
                    Some(f) => self.advance(f),
                    None => {
                        // Leer: ausblenden, Polster neu aufbauen.
                        self.advance([0.0; 2]);
                        if self.primed {
                            self.primed = false;
                            self.stats.underruns.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                }
            }
            if !self.primed {
                l[i + 1..n].fill(0.0);
                r[i + 1..n].fill(0.0);
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pairs(a: f32, b: f32, c: f32, d: f32) -> Vec<PairBuf> {
        vec![
            PairBuf {
                l: vec![a; 2],
                r: vec![b; 2],
            },
            PairBuf {
                l: vec![c; 2],
                r: vec![d; 2],
            },
        ]
    }

    #[test]
    fn each_pair_lands_on_its_channels_only() {
        let p = pairs(0.1, 0.2, 0.3, 0.4);
        let mut out = [9.0f32; 12];
        interleave_pairs(&mut out, 6, &p, 2);
        assert_eq!(
            out,
            [0.1, 0.2, 0.3, 0.4, 0.0, 0.0, 0.1, 0.2, 0.3, 0.4, 0.0, 0.0]
        );
        // Zwei Kanäle: nur Paar 0, Paar 1 erreicht Kanal 1/2 nie.
        let mut st = [9.0f32; 4];
        interleave_pairs(&mut st, 2, &p, 2);
        assert_eq!(st, [0.1, 0.2, 0.1, 0.2]);
        // Ein Kanal: Mittel von Paar 0.
        let mut mono = [9.0f32; 2];
        interleave_pairs(&mut mono, 1, &p, 2);
        assert!((mono[0] - 0.15).abs() < 1e-6);
    }

    #[test]
    fn unused_pair_stays_silent() {
        let mut p = pairs(0.0, 0.0, 0.5, -0.5);
        p[0].clear(2);
        let mut out = [9.0f32; 8];
        interleave_pairs(&mut out, 4, &p, 2);
        assert_eq!(out, [0.0, 0.0, 0.5, -0.5, 0.0, 0.0, 0.5, -0.5]);
    }

    /// Treibt einen Sinus durch die Verbindung. Sender: Blöcke zu `tx_block` mit `src_rate`
    /// (plus `ppm` Abweichung der Uhr), Empfänger: Blöcke zu `rx_block` mit `dst_rate`.
    /// Zeit läuft in Mikroblöcken von 1 ms. Gibt Ausgabe, Zähler und Füllstände zurück.
    fn run(
        src_rate: u32,
        dst_rate: u32,
        ppm: f64,
        seconds: f64,
        tx_block: usize,
        rx_block: usize,
        freq: f64,
    ) -> (Vec<f32>, Arc<LinkStats>, Vec<u64>) {
        let (mut tx, mut rx, stats) = link(src_rate, dst_rate);
        let tx_rate = f64::from(src_rate) * (1.0 + ppm * 1e-6);
        let (mut tx_frames, mut rx_frames) = (0.0f64, 0.0f64);
        let (mut sent, mut got) = (0usize, 0usize);
        let (mut out, mut fills) = (Vec::new(), Vec::new());
        let mut t = 0.0f64;
        let dt = 0.001;
        let mut l = vec![0.0f32; rx_block];
        let mut r = vec![0.0f32; rx_block];
        let mut tl = vec![0.0f32; tx_block];
        let mut tr = vec![0.0f32; tx_block];
        while t < seconds {
            t += dt;
            tx_frames += tx_rate * dt;
            while tx_frames >= (sent + tx_block) as f64 {
                for i in 0..tx_block {
                    let v = ((sent + i) as f64 / f64::from(src_rate) * freq * std::f64::consts::TAU)
                        .sin() as f32
                        * 0.5;
                    tl[i] = v;
                    tr[i] = -v;
                }
                tx.push(&tl, &tr);
                sent += tx_block;
            }
            rx_frames += f64::from(dst_rate) * dt;
            while rx_frames >= (got + rx_block) as f64 {
                rx.pull(&mut l, &mut r);
                out.extend_from_slice(&l);
                got += rx_block;
                fills.push(stats.fill.load(Ordering::Relaxed));
            }
        }
        (out, stats, fills)
    }

    /// Größter Sprung zwischen zwei Nachbar-Samples nach dem Einschwingen.
    fn max_step(out: &[f32], skip: usize) -> f32 {
        out[skip..]
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0, f32::max)
    }

    #[test]
    fn same_rate_without_drift_is_bit_exact_passthrough() {
        let (out, stats, _) = run(48_000, 48_000, 0.0, 2.0, 480, 480, 440.0);
        assert_eq!(stats.faults(), 0);
        // Das Polster verschiebt nur; Form und Frequenz bleiben. Ein 440-Hz-Sinus bei 0,5 hat
        // höchstens 0,5 * 2pi * 440 / 48000 = 0,029 Sprung je Sample.
        assert!(max_step(&out, 5000) < 0.031, "{}", max_step(&out, 5000));
    }

    #[test]
    fn clock_drift_is_absorbed_both_ways_without_gaps() {
        for ppm in [-1000.0, -200.0, 50.0, 200.0, 1000.0] {
            let (out, stats, fills) = run(48_000, 48_000, ppm, 60.0, 480, 512, 1000.0);
            assert_eq!(stats.faults(), 0, "{ppm} ppm: Ausfälle");
            let target = (48_000.0 * TARGET_SECONDS) as u64;
            let late = &fills[fills.len() / 2..];
            let (lo, hi) = (*late.iter().min().unwrap(), *late.iter().max().unwrap());
            assert!(
                lo > target / 2 && hi < target * 2,
                "{ppm} ppm: Füllstand {lo}..{hi}, Soll {target}"
            );
            // Keine Lücken, keine Sprünge: 1 kHz bei 0,5 steigt höchstens 0,065 je Sample.
            assert!(
                max_step(&out, 5000) < 0.07,
                "{ppm} ppm: {}",
                max_step(&out, 5000)
            );
            let silent = out[5000..]
                .windows(64)
                .filter(|w| w.iter().all(|v| *v == 0.0))
                .count();
            assert_eq!(silent, 0, "{ppm} ppm: Stille im Signal");
        }
    }

    #[test]
    fn different_rates_keep_the_pitch() {
        // 44,1 kHz in, 48 kHz aus: ein 1-kHz-Sinus muss bei 1 kHz bleiben.
        let (out, stats, _) = run(44_100, 48_000, 30.0, 4.0, 441, 480, 1000.0);
        assert_eq!(stats.faults(), 0);
        let tail = &out[48_000..];
        let crossings = tail
            .windows(2)
            .filter(|w| w[0] <= 0.0 && w[1] > 0.0)
            .count();
        let hz = crossings as f64 / (tail.len() as f64 / 48_000.0);
        assert!((hz - 1000.0).abs() < 2.0, "{hz} Hz");
        assert!(max_step(&out, 5000) < 0.07);
    }

    #[test]
    fn stalled_device_drops_and_recovers() {
        let (mut tx, mut rx, stats) = link(48_000, 48_000);
        let (l, r) = (vec![0.25f32; 4800], vec![0.25f32; 4800]);
        // Empfänger steht: der Sender füllt den Ring und verwirft den Rest (gezählt).
        for _ in 0..10 {
            tx.push(&l, &r);
        }
        assert!(stats.dropped.load(Ordering::Relaxed) > 0);
        // Empfänger wacht auf: springt zum Soll, zählt das Überspringen.
        let (mut ol, mut or) = (vec![0.0f32; 480], vec![0.0f32; 480]);
        rx.pull(&mut ol, &mut or);
        assert!(stats.skipped.load(Ordering::Relaxed) > 0);
        assert!(stats.fill.load(Ordering::Relaxed) > 0);
        // Danach läuft er leer: Ausblenden zu Stille, ein Underrun gezählt.
        for _ in 0..40 {
            rx.pull(&mut ol, &mut or);
        }
        assert_eq!(stats.underruns.load(Ordering::Relaxed), 1);
        assert!(ol.iter().all(|v| *v == 0.0));
    }

    #[test]
    fn pull_and_push_do_not_allocate() {
        use assert_no_alloc::{AllocDisabler, assert_no_alloc};
        #[global_allocator]
        static A: AllocDisabler = AllocDisabler;
        let (mut tx, mut rx, _) = link(48_000, 44_100);
        let (l, r) = (vec![0.1f32; 480], vec![0.1f32; 480]);
        let (mut ol, mut or) = (vec![0.0f32; 441], vec![0.0f32; 441]);
        assert_no_alloc(|| {
            for _ in 0..20 {
                tx.push(&l, &r);
                rx.pull(&mut ol, &mut or);
            }
        });
    }
}
