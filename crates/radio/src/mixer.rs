//! Der Sendemixer im Audio-Thread: Musik mit Crossfade, Carts, Mikro mit Ducking.
//!
//! Keine Allokation, kein Lock, kein I/O. Alles, was hereinkommt, kommt über die
//! `rtrb`-Queue [`Cmd`]; dekodierte Puffer stecken in `basedrop::Shared`, sodass ihr
//! Freigeben nur einen Eintrag beim Collector einreiht (abgeholt vom Control-Thread).
//! Zustand für Oberfläche und Controller steht in [`Status`] (Atomics).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

use basedrop::{Owned, Shared};

use crate::CARTS;
use crate::audio::Stereo;
use crate::level::{Smoother, crossfade};

/// Kennung „kein Titel“.
pub const NONE: u32 = u32::MAX;
/// Ausblenden bei Stopp (Sekunden).
const STOP_SECONDS: f32 = 0.03;
/// Glättung des Ducking (Sekunden, Zeitkonstante).
const DUCK_TAU: f32 = 0.08;
/// Glättung von Mikro auf/zu.
const MIC_TAU: f32 = 0.01;
/// Mehr Mikro-Vorlauf als das (Sekunden) wird verworfen, damit die Latenz klein bleibt.
const MIC_BACKLOG: f32 = 0.08;

/// Kommandos vom Control- an den Audio-Thread.
pub enum Cmd {
    /// Nächsten Titel bereitlegen (ersetzt einen schon bereitgelegten).
    Queue {
        uid: u32,
        buf: Shared<Stereo>,
        gain: f32,
    },
    /// Bereitgelegten Titel verwerfen.
    ClearNext,
    /// Sendung starten: der bereitgelegte Titel beginnt.
    Play,
    /// Musik kurz ausblenden und anhalten; Carts und Mikro laufen weiter.
    Stop,
    /// Jetzt zum nächsten Titel überblenden (oder ausblenden, wenn keiner bereitliegt).
    Next,
    /// Crossfade-Länge in Frames.
    SetCrossfade(usize),
    /// Absenkung der Musik bei offenem Mikro (linear).
    SetDuck(f32),
    /// Mikro auf/zu.
    MicOpen(bool),
    /// Mikro im eigenen Ausgang hören (Sendung enthält es immer).
    MicMonitor(bool),
    /// Mikro-Eingang: Stereo verschachtelt in Mixer-Rate, `None` = keiner.
    MicInput(Option<Owned<rtrb::Consumer<f32>>>),
    /// Cart abspielen (von vorn, wenn er schon läuft).
    Cart {
        slot: usize,
        buf: Shared<Stereo>,
        gain: f32,
    },
    /// Cart anhalten.
    CartStop(usize),
    /// Mitschnitt: Sendeausgang Stereo verschachtelt in diesen Ring, `None` = aus.
    Record(Option<Owned<rtrb::Producer<f32>>>),
    /// Sendung an woa-streamd: Sendeausgang Stereo verschachtelt in diesen Ring, `None` = aus.
    Stream(Option<Owned<rtrb::Producer<f32>>>),
}

/// Lock-frei lesbarer Zustand des Mixers.
#[derive(Debug)]
pub struct Status {
    pub current: AtomicU32,
    pub next: AtomicU32,
    /// Position im aktuellen Titel (Frames).
    pub position: AtomicU64,
    /// Ende des aktuellen Titels (Frames), bei „Weiter“ vorgezogen.
    pub end: AtomicU64,
    pub playing: AtomicBool,
    /// Bit je laufendem Cart.
    pub carts: AtomicU32,
    pub mic_open: AtomicBool,
    /// Gerechnete Frames insgesamt.
    pub clock: AtomicU64,
    /// Frame, an dem zuletzt die Musik endete (Ende der Playlist).
    pub music_end: AtomicU64,
    /// Verlorene Mitschnitt-Samples (Ring voll).
    pub record_dropped: AtomicU64,
    /// Verlorene Sende-Samples (Ring voll, woa-streamd hängt).
    pub stream_dropped: AtomicU64,
    /// Mikro-Ring leer gelaufen (Frames).
    pub mic_underruns: AtomicU64,
    /// Spitzenpegel Sendeausgang seit letztem Abholen (f32-Bits).
    pub peak: AtomicU32,
    /// Spitzenpegel Mikro seit letztem Abholen (f32-Bits).
    pub mic_peak: AtomicU32,
}

impl Default for Status {
    fn default() -> Self {
        Self {
            current: AtomicU32::new(NONE),
            next: AtomicU32::new(NONE),
            position: AtomicU64::new(0),
            end: AtomicU64::new(0),
            playing: AtomicBool::new(false),
            carts: AtomicU32::new(0),
            mic_open: AtomicBool::new(false),
            clock: AtomicU64::new(0),
            music_end: AtomicU64::new(0),
            record_dropped: AtomicU64::new(0),
            stream_dropped: AtomicU64::new(0),
            mic_underruns: AtomicU64::new(0),
            peak: AtomicU32::new(0),
            mic_peak: AtomicU32::new(0),
        }
    }
}

impl Status {
    /// Spitzenpegel seit dem letzten Aufruf.
    pub fn take_peak(&self) -> f32 {
        f32::from_bits(self.peak.swap(0, Ordering::Relaxed))
    }

    pub fn take_mic_peak(&self) -> f32 {
        f32::from_bits(self.mic_peak.swap(0, Ordering::Relaxed))
    }
}

fn store_max(cell: &AtomicU32, v: f32) {
    let mut cur = cell.load(Ordering::Relaxed);
    while v > f32::from_bits(cur) {
        match cell.compare_exchange_weak(cur, v.to_bits(), Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => break,
            Err(seen) => cur = seen,
        }
    }
}

struct Deck {
    uid: u32,
    buf: Shared<Stereo>,
    pos: usize,
    end: usize,
    gain: f32,
    /// Ausblenden über die letzten so vielen Frames (0 = keins), bei „Weiter“ ohne Nachfolger.
    tail: usize,
}

impl Deck {
    fn new(uid: u32, buf: Shared<Stereo>, gain: f32) -> Self {
        let end = buf.frames();
        Self {
            uid,
            buf,
            pos: 0,
            end,
            gain,
            tail: 0,
        }
    }

    fn remaining(&self) -> usize {
        self.end.saturating_sub(self.pos)
    }

    fn done(&self) -> bool {
        self.pos >= self.end
    }

    /// Nächstes Sample (l, r) mit Titel-Gain und ggf. Ausblendung.
    #[inline]
    fn next(&mut self) -> (f32, f32) {
        if self.done() {
            return (0.0, 0.0);
        }
        let mut g = self.gain;
        if self.tail > 0 {
            let left = self.remaining();
            if left < self.tail {
                g *= crossfade(1.0 - left as f32 / self.tail as f32).0;
            }
        }
        let i = self.pos;
        self.pos += 1;
        (self.buf.left[i] * g, self.buf.right[i] * g)
    }
}

struct Voice {
    buf: Shared<Stereo>,
    pos: usize,
    gain: f32,
}

/// Der Mixer im Audio-Thread.
pub struct Mixer {
    cmds: rtrb::Consumer<Cmd>,
    status: Arc<Status>,
    current: Option<Deck>,
    outgoing: Option<Deck>,
    next: Option<Deck>,
    xf_frames: usize,
    xf_len: usize,
    xf_pos: usize,
    playing: bool,
    stop_len: usize,
    stopping: Option<usize>,
    carts: [Option<Voice>; CARTS],
    mic: Option<Owned<rtrb::Consumer<f32>>>,
    mic_open: bool,
    mic_monitor: bool,
    mic_gain: Smoother,
    duck_lin: f32,
    duck: Smoother,
    mic_backlog: usize,
    rec: Option<Owned<rtrb::Producer<f32>>>,
    stream: Option<Owned<rtrb::Producer<f32>>>,
    clock: u64,
}

impl Mixer {
    /// Legt den Mixer an (im Control-Thread, danach in den Audio-Thread verschieben).
    pub fn new(cmds: rtrb::Consumer<Cmd>, status: Arc<Status>, rate: u32) -> Self {
        let r = rate.max(1) as f32;
        Self {
            cmds,
            status,
            current: None,
            outgoing: None,
            next: None,
            xf_frames: (3.0 * r) as usize,
            xf_len: 0,
            xf_pos: 0,
            playing: false,
            stop_len: (STOP_SECONDS * r).max(1.0) as usize,
            stopping: None,
            carts: std::array::from_fn(|_| None),
            mic: None,
            mic_open: false,
            mic_monitor: false,
            mic_gain: Smoother::new(0.0, MIC_TAU, r),
            duck_lin: 0.25,
            duck: Smoother::new(1.0, DUCK_TAU, r),
            mic_backlog: (MIC_BACKLOG * r) as usize * 2,
            rec: None,
            stream: None,
            clock: 0,
        }
    }

    fn drain(&mut self) {
        while let Ok(cmd) = self.cmds.pop() {
            match cmd {
                Cmd::Queue { uid, buf, gain } => self.next = Some(Deck::new(uid, buf, gain)),
                Cmd::ClearNext => self.next = None,
                Cmd::Play => {
                    self.playing = true;
                    self.stopping = None;
                }
                Cmd::Stop => {
                    if self.playing && self.stopping.is_none() {
                        self.stopping = Some(self.stop_len);
                    }
                }
                Cmd::Next => self.skip(),
                Cmd::SetCrossfade(f) => self.xf_frames = f,
                Cmd::SetDuck(g) => {
                    self.duck_lin = g.clamp(0.0, 1.0);
                    if self.mic_open {
                        self.duck.set_target(self.duck_lin);
                    }
                }
                Cmd::MicOpen(on) => {
                    self.mic_open = on;
                    self.mic_gain.set_target(if on { 1.0 } else { 0.0 });
                    self.duck.set_target(if on { self.duck_lin } else { 1.0 });
                }
                Cmd::MicMonitor(on) => self.mic_monitor = on,
                Cmd::MicInput(c) => self.mic = c,
                Cmd::Cart { slot, buf, gain } => {
                    if let Some(v) = self.carts.get_mut(slot) {
                        *v = Some(Voice { buf, pos: 0, gain });
                    }
                }
                Cmd::CartStop(slot) => {
                    if let Some(v) = self.carts.get_mut(slot) {
                        *v = None;
                    }
                }
                Cmd::Record(p) => self.rec = p,
                Cmd::Stream(p) => self.stream = p,
            }
        }
    }

    /// „Weiter“: das aktuelle Stück endet nach einer Überblendlänge.
    fn skip(&mut self) {
        if !self.playing || self.outgoing.is_some() {
            return;
        }
        let next_len = self.next.as_ref().map(|n| n.buf.frames());
        if let Some(c) = self.current.as_mut() {
            let mut xf = self.xf_frames.min(c.remaining());
            if let Some(n) = next_len {
                xf = xf.min(n / 2);
            } else {
                c.tail = xf;
            }
            c.end = c.pos + xf;
        }
    }

    /// Überblendlänge zwischen `cur` und dem nächsten Titel.
    fn xf_for(&self, cur: &Deck, next: &Deck) -> usize {
        if cur.end < cur.buf.frames() {
            // Vorgezogenes Ende („Weiter“): die Restlänge ist die Überblendung.
            return cur.remaining();
        }
        self.xf_frames
            .min(cur.buf.frames() / 2)
            .min(next.buf.frames() / 2)
    }

    fn transitions(&mut self) {
        if !self.playing || self.stopping.is_some() {
            return;
        }
        if self.current.is_none() && self.outgoing.is_none() && self.next.is_some() {
            self.current = self.next.take();
            return;
        }
        if self.outgoing.is_some() {
            return;
        }
        let start = match (&self.current, &self.next) {
            (Some(c), Some(n)) => c.remaining() <= self.xf_for(c, n),
            _ => false,
        };
        if start {
            let out = self.current.take();
            self.xf_len = out.as_ref().map_or(0, Deck::remaining);
            self.xf_pos = 0;
            self.outgoing = if self.xf_len > 0 { out } else { None };
            self.current = self.next.take();
        }
    }

    /// Rechnet einen Block. `left`/`right` bekommen den Abhörmix (Sendung, Mikro nur mit
    /// [`Cmd::MicMonitor`]); der Mitschnitt bekommt immer die volle Sendung.
    pub fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        self.drain();
        let n = left.len().min(right.len());
        self.trim_mic_backlog();
        let mut peak = 0.0f32;
        let mut mic_peak = 0.0f32;
        for i in 0..n {
            self.transitions();

            // Musik.
            let (mut ml, mut mr) = (0.0f32, 0.0f32);
            let fading = self.outgoing.is_some();
            let (g_out, g_in) = if fading && self.xf_len > 0 {
                crossfade(self.xf_pos as f32 / self.xf_len as f32)
            } else {
                (0.0, 1.0)
            };
            if let Some(c) = self.current.as_mut() {
                let (l, r) = c.next();
                ml += l * g_in;
                mr += r * g_in;
            }
            if let Some(o) = self.outgoing.as_mut() {
                let (l, r) = o.next();
                ml += l * g_out;
                mr += r * g_out;
                self.xf_pos += 1;
                if self.xf_pos >= self.xf_len || o.done() {
                    self.outgoing = None;
                }
            }
            if self.current.as_ref().is_some_and(Deck::done) {
                self.current = None;
                if self.next.is_none() {
                    self.status
                        .music_end
                        .store(self.clock + 1, Ordering::Release);
                }
            }
            if let Some(k) = self.stopping {
                let g = k as f32 / self.stop_len as f32;
                ml *= g;
                mr *= g;
                if k <= 1 {
                    self.current = None;
                    self.outgoing = None;
                    self.next = None;
                    self.playing = false;
                    self.stopping = None;
                } else {
                    self.stopping = Some(k - 1);
                }
            }

            // Carts.
            let (mut cl, mut cr) = (0.0f32, 0.0f32);
            for slot in self.carts.iter_mut() {
                if let Some(v) = slot {
                    if v.pos < v.buf.frames() {
                        cl += v.buf.left[v.pos] * v.gain;
                        cr += v.buf.right[v.pos] * v.gain;
                        v.pos += 1;
                    } else {
                        *slot = None;
                    }
                }
            }

            // Mikro.
            let (mut il, mut ir) = (0.0f32, 0.0f32);
            if let Some(m) = self.mic.as_mut() {
                if m.slots() >= 2 {
                    il = m.pop().unwrap_or(0.0);
                    ir = m.pop().unwrap_or(0.0);
                } else if self.mic_open {
                    self.status.mic_underruns.fetch_add(1, Ordering::Relaxed);
                }
            }
            let mg = self.mic_gain.tick();
            let duck = self.duck.tick();
            il *= mg;
            ir *= mg;
            mic_peak = mic_peak.max(il.abs()).max(ir.abs());

            let sl = ml * duck + cl + il;
            let sr = mr * duck + cr + ir;
            if let Some(p) = self.rec.as_mut() {
                if p.slots() >= 2 {
                    let _ = p.push(sl);
                    let _ = p.push(sr);
                } else {
                    self.status.record_dropped.fetch_add(2, Ordering::Relaxed);
                }
            }
            if let Some(p) = self.stream.as_mut() {
                if p.slots() >= 2 {
                    let _ = p.push(sl);
                    let _ = p.push(sr);
                } else {
                    self.status.stream_dropped.fetch_add(2, Ordering::Relaxed);
                }
            }
            let (ol, or) = if self.mic_monitor {
                (sl, sr)
            } else {
                (sl - il, sr - ir)
            };
            left[i] = ol;
            right[i] = or;
            peak = peak.max(sl.abs()).max(sr.abs());
            self.clock += 1;
        }
        self.publish(peak, mic_peak);
    }

    /// Wirft zu viel Mikro-Vorlauf weg (etwa nach einem Hänger des Eingangs).
    fn trim_mic_backlog(&mut self) {
        if let Some(m) = self.mic.as_mut() {
            let avail = m.slots();
            if avail > self.mic_backlog {
                let drop = (avail - self.mic_backlog / 2) & !1;
                if let Ok(chunk) = m.read_chunk(drop) {
                    chunk.commit_all();
                }
            }
        }
    }

    fn publish(&self, peak: f32, mic_peak: f32) {
        let s = &self.status;
        let (cur, pos, end) = self
            .current
            .as_ref()
            .map_or((NONE, 0, 0), |c| (c.uid, c.pos as u64, c.end as u64));
        s.current.store(cur, Ordering::Release);
        s.position.store(pos, Ordering::Release);
        s.end.store(end, Ordering::Release);
        s.next.store(
            self.next.as_ref().map_or(NONE, |d| d.uid),
            Ordering::Release,
        );
        s.playing.store(self.playing, Ordering::Release);
        let mut mask = 0u32;
        for (i, v) in self.carts.iter().enumerate() {
            if v.is_some() {
                mask |= 1 << i;
            }
        }
        s.carts.store(mask, Ordering::Release);
        s.mic_open.store(self.mic_open, Ordering::Release);
        s.clock.store(self.clock, Ordering::Release);
        store_max(&s.peak, peak);
        store_max(&s.mic_peak, mic_peak);
    }
}
