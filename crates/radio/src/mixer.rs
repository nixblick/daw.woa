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
use crate::cues::CueFrames;
use crate::dynamics::{BusProcessor, MicChain};
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

/// Glättung der Pegelregler (Sekunden, Zeitkonstante), gegen Zipper-Noise beim Fader.
const LEVEL_TAU: f32 = 0.02;

/// Unterste Stellung der Pegelregler (dB) = aus.
pub const LEVEL_MIN_DB: f64 = -60.0;
/// Oberste Stellung der Pegelregler (dB).
pub const LEVEL_MAX_DB: f64 = 6.0;

/// Pegelregler im Sendemixer (Paket 14, auch per Controller).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LevelBus {
    Music,
    Carts,
    Mic,
}

impl LevelBus {
    fn index(self) -> usize {
        match self {
            LevelBus::Music => 0,
            LevelBus::Carts => 1,
            LevelBus::Mic => 2,
        }
    }
}

/// Kommandos vom Control- an den Audio-Thread.
pub enum Cmd {
    /// Nächsten Titel bereitlegen (ersetzt einen schon bereitgelegten).
    Queue {
        uid: u32,
        buf: Shared<Stereo>,
        gain: f32,
        /// Cue-Punkte in Frames (Paket 15).
        cues: CueFrames,
        /// Überblendlänge vom laufenden Titel in diesen (Paket 29, Mixxx-Übergang aus
        /// Intro/Outro); `None`: die eingestellte Überblendzeit.
        lead: Option<usize>,
        /// Zeit-Event: beginnt nie von selbst, sondern erst mit [`Cmd::StartAt`].
        hold: bool,
    },
    /// Das bereitgelegte Zeit-Event beginnt, sobald die Mixer-Uhr diesen Frame erreicht
    /// (Paket 29). Der laufende Titel wird darunter ausgeblendet.
    StartAt(u64),
    /// Nimmt [`Cmd::StartAt`] zurück.
    ClearStartAt,
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
    /// Processing an/aus: Sendebus (Lautheitsregelung, Kompressor, Limiter) und Mikro-Kette.
    SetProcessing { bus: bool, mic: bool },
    /// Ziel-Lautheit des Sendebusses (LUFS).
    SetTarget(f64),
    /// Pegel eines Zweigs (linear, 1.0 = unverändert), geglättet.
    SetLevel { bus: LevelBus, gain: f32 },
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
    /// Sendebus: Verstärkung der Lautheitsregelung (dB, f32-Bits).
    pub bus_agc_db: AtomicU32,
    /// Sendebus: größte Absenkung Kompressor + Limiter seit letztem Abholen (dB, f32-Bits).
    pub bus_reduction_db: AtomicU32,
    /// Sendebus: Kurzzeit-Lautheit am Ausgang (LUFS, f32-Bits).
    pub bus_lufs: AtomicU32,
    /// Mikro-Gate offen.
    pub mic_gate_open: AtomicBool,
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
            bus_agc_db: AtomicU32::new(0),
            bus_reduction_db: AtomicU32::new(0),
            bus_lufs: AtomicU32::new((-70.0f32).to_bits()),
            mic_gate_open: AtomicBool::new(false),
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

    /// Größte Absenkung des Sendebusses (dB) seit dem letzten Aufruf.
    pub fn take_bus_reduction(&self) -> f32 {
        f32::from_bits(self.bus_reduction_db.swap(0, Ordering::Relaxed))
    }

    /// Verstärkung der Lautheitsregelung (dB).
    pub fn bus_agc_db(&self) -> f32 {
        f32::from_bits(self.bus_agc_db.load(Ordering::Relaxed))
    }

    /// Kurzzeit-Lautheit am Sendeausgang (LUFS).
    pub fn bus_lufs(&self) -> f32 {
        f32::from_bits(self.bus_lufs.load(Ordering::Relaxed))
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
    /// Cue-In (Frames).
    start: usize,
    /// Cue-Out, bei „Weiter“ vorgezogen.
    end: usize,
    /// Überblendlänge aus dem Fade-Out-Marker.
    fade: Option<usize>,
    /// „Weiter“ wurde gedrückt: die Restlänge ist die Überblendung.
    skipped: bool,
    gain: f32,
    /// Ausblenden über die letzten so vielen Frames (0 = keins), bei „Weiter“ ohne Nachfolger.
    tail: usize,
    /// Überblendlänge vom Vorgänger (Paket 29).
    lead: Option<usize>,
    /// Zeit-Event: wartet auf [`Cmd::StartAt`].
    hold: bool,
}

impl Deck {
    fn new(uid: u32, buf: Shared<Stereo>, gain: f32, cues: CueFrames) -> Self {
        let total = buf.frames();
        let end = cues.end.min(total);
        let start = cues.start.min(end);
        Self {
            uid,
            buf,
            pos: start,
            start,
            end,
            fade: cues.fade,
            skipped: false,
            gain,
            tail: 0,
            lead: None,
            hold: false,
        }
    }

    /// Gespielte Länge (Cue-In bis Cue-Out).
    fn len(&self) -> usize {
        self.end - self.start
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
    /// Schritt von `xf_pos` je Frame; größer als 1 beschleunigt eine laufende Überblendung
    /// vor einem harten Zeit-Event.
    xf_step: usize,
    /// Frame der Mixer-Uhr, an dem das bereitgelegte Zeit-Event beginnt.
    start_at: Option<u64>,
    rate_hz: u32,
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
    bus: BusProcessor,
    bus_on: bool,
    mic_chain: MicChain,
    mic_chain_on: bool,
    /// Pegel Musik, Carts, Mikro (geglättet).
    levels: [Smoother; 3],
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
            xf_step: 1,
            start_at: None,
            rate_hz: rate.max(1),
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
            bus: BusProcessor::new(crate::level::TARGET_LUFS, rate),
            bus_on: true,
            mic_chain: MicChain::new(rate),
            mic_chain_on: true,
            levels: std::array::from_fn(|_| Smoother::new(1.0, LEVEL_TAU, r)),
        }
    }

    fn drain(&mut self) {
        while let Ok(cmd) = self.cmds.pop() {
            match cmd {
                Cmd::Queue {
                    uid,
                    buf,
                    gain,
                    cues,
                    lead,
                    hold,
                } => {
                    let mut d = Deck::new(uid, buf, gain, cues);
                    d.lead = lead;
                    d.hold = hold;
                    self.next = Some(d);
                }
                Cmd::StartAt(frame) => self.start_at = Some(frame),
                Cmd::ClearStartAt => self.start_at = None,
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
                Cmd::SetProcessing { bus, mic } => {
                    self.bus_on = bus;
                    self.mic_chain_on = mic;
                }
                Cmd::SetTarget(t) => self.bus.set_target(t),
                Cmd::SetLevel { bus, gain } => {
                    self.levels[bus.index()].set_target(gain.clamp(0.0, 4.0));
                }
            }
        }
    }

    /// „Weiter“: das aktuelle Stück endet nach einer Überblendlänge.
    fn skip(&mut self) {
        if !self.playing || self.outgoing.is_some() {
            return;
        }
        let next_len = self.next.as_ref().map(Deck::len);
        if let Some(c) = self.current.as_mut() {
            let mut xf = self.xf_frames.min(c.remaining());
            if let Some(n) = next_len {
                xf = xf.min(n / 2);
            } else {
                c.tail = xf;
            }
            c.end = c.pos + xf;
            c.skipped = true;
        }
    }

    /// Überblendlänge zwischen `cur` und dem nächsten Titel.
    fn xf_for(&self, cur: &Deck, next: &Deck) -> usize {
        if cur.skipped {
            // Vorgezogenes Ende („Weiter“): die Restlänge ist die Überblendung.
            return cur.remaining();
        }
        if let Some(l) = next.lead {
            // Übergang aus Intro/Outro (Controller gerechnet, `transition.rs`).
            return l.min(cur.len()).min(next.len() / 2);
        }
        match cur.fade {
            // Fade-Out-Marker: Überblendung von dort bis Cue-Out (wie `crossfade_entries`).
            Some(f) => f.min(cur.len()).min(next.len() / 2),
            None => self.xf_frames.min(cur.len() / 2).min(next.len() / 2),
        }
    }

    fn transitions(&mut self) {
        if !self.playing || self.stopping.is_some() {
            return;
        }
        let due = self.start_at.is_some_and(|t| self.clock >= t);
        let held = self.next.as_ref().is_some_and(|n| n.hold);
        if due && self.outgoing.is_some() && held {
            // Läuft noch eine Überblendung: in rund 0,2 s zu Ende bringen, nicht warten.
            let fast = (self.rate_hz as usize / 5).max(1);
            let left = self.xf_len.saturating_sub(self.xf_pos);
            self.xf_step = self.xf_step.max(left.div_ceil(fast).max(1));
        }
        if self.current.is_none() && self.outgoing.is_none() && self.next.is_some() {
            if held && !due {
                return;
            }
            self.current = self.next.take();
            self.start_at = None;
            return;
        }
        if self.outgoing.is_some() {
            return;
        }
        let start = match (&self.current, &self.next) {
            (Some(c), Some(n)) if n.hold => due && !c.done(),
            (Some(c), Some(n)) => c.remaining() <= self.xf_for(c, n),
            _ => false,
        };
        if start && held {
            // Hartes Zeit-Event: der laufende Titel wird unter dem Event ausgeblendet.
            self.start_at = None;
            if let (Some(c), Some(n)) = (self.current.as_mut(), self.next.as_ref()) {
                // Kurz (250 ms): das Event soll auf die Zeit sitzen, nicht langsam einblenden.
                let fade = (self.rate_hz as usize / 4)
                    .min(self.xf_frames)
                    .min(c.remaining())
                    .min(n.len() / 2);
                c.end = c.pos + fade;
            }
        }
        if start {
            let out = self.current.take();
            self.xf_len = out.as_ref().map_or(0, Deck::remaining);
            self.xf_pos = 0;
            self.xf_step = 1;
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
        let mut reduction = 0.0f32;
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
                self.xf_pos += self.xf_step;
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
            if self.mic_chain_on {
                (il, ir) = self.mic_chain.process(il, ir);
            }
            let [lm, lc, li] = &mut self.levels;
            let (lm, lc) = (lm.tick(), lc.tick());
            let mg = self.mic_gain.tick() * li.tick();
            let duck = self.duck.tick() * lm;
            cl *= lc;
            cr *= lc;
            il *= mg;
            ir *= mg;
            mic_peak = mic_peak.max(il.abs()).max(ir.abs());

            let mut sl = ml * duck + cl + il;
            let mut sr = mr * duck + cr + ir;
            // Abhören ohne Mikro: derselbe Weg ohne `il`/`ir`, mit demselben Faktor.
            let (mut ql, mut qr) = (sl - il, sr - ir);
            if self.bus_on {
                let g = self.bus.gain(sl, sr);
                sl = self.bus.clip(sl * g);
                sr = self.bus.clip(sr * g);
                ql = self.bus.clip(ql * g);
                qr = self.bus.clip(qr * g);
                let m = self.bus.meter();
                reduction = reduction.max(m.comp_db + m.limit_db);
            }
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
            let (ol, or) = if self.mic_monitor { (sl, sr) } else { (ql, qr) };
            left[i] = ol;
            right[i] = or;
            peak = peak.max(sl.abs()).max(sr.abs());
            self.clock += 1;
        }
        self.publish(peak, mic_peak, reduction);
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

    fn publish(&self, peak: f32, mic_peak: f32, reduction: f32) {
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
        store_max(&s.bus_reduction_db, reduction);
        let m = self.bus.meter();
        let agc = if self.bus_on { m.agc_db } else { 0.0 };
        s.bus_agc_db.store(agc.to_bits(), Ordering::Relaxed);
        s.bus_lufs.store(m.lufs.to_bits(), Ordering::Relaxed);
        s.mic_gate_open.store(
            self.mic_chain_on && self.mic_chain.gate_open(),
            Ordering::Relaxed,
        );
    }
}
