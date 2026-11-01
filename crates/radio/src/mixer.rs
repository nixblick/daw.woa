//! Der Sendemixer im Audio-Thread: Musik mit Crossfade, Carts, Mikro mit Ducking.
//!
//! Keine Allokation, kein Lock, kein I/O. Alles, was hereinkommt, kommt über die
//! `rtrb`-Queue [`Cmd`]; dekodierte Puffer stecken in `basedrop::Shared`, sodass ihr
//! Freigeben nur einen Eintrag beim Collector einreiht (abgeholt vom Control-Thread).
//! Zustand für Oberfläche und Controller steht in [`Status`] (Atomics).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

use basedrop::{Owned, Shared};
use woa_guests::GuestBank;

use crate::audio::Stereo;
use crate::cues::CueFrames;
use crate::dynamics::{BusProcessor, MicChain};
use crate::level::{Fader, Smoother, crossfade, fader_gains};
use crate::voicetrack::{EDGE_SECONDS, VoiceFrames, duck_gain, edge_gain};
use crate::{CARTS, DECKS, PARTY_SIDES};

/// Kennung „kein Titel“.
pub const NONE: u32 = u32::MAX;
/// Deck ohne Titel.
pub const DECK_EMPTY: u32 = 0;
/// Deck mit Titel, steht.
pub const DECK_READY: u32 = 1;
/// Deck spielt.
pub const DECK_PLAYING: u32 = 2;
/// Ausblenden bei Stopp (Sekunden).
const STOP_SECONDS: f32 = 0.03;
/// Glättung des Ducking (Sekunden, Zeitkonstante).
const DUCK_TAU: f32 = 0.08;
/// Glättung von Mikro auf/zu.
const MIC_TAU: f32 = 0.01;
/// Mehr Mikro-Vorlauf als das (Sekunden) wird verworfen, damit die Latenz klein bleibt.
const MIC_BACKLOG: f32 = 0.08;
/// Frames, die der Telefon-Eingang auf einmal aus seiner Taktverbindung holt.
const PHONE_CHUNK: usize = 512;

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
    /// Decks A, B, C zusammen.
    Decks,
    /// Ausgang „Wiedergabe Notebook“ (Abhörmix im Hauptausgang, #96).
    Monitor,
    /// Ausgang „Stream senden“ (nur der Ring an woa-streamd, #96).
    Stream,
}

impl LevelBus {
    fn index(self) -> usize {
        match self {
            LevelBus::Music => 0,
            LevelBus::Carts => 1,
            LevelBus::Mic => 2,
            LevelBus::Decks => 3,
            LevelBus::Monitor => 4,
            LevelBus::Stream => 5,
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
    /// Voice-Track zum bereitgelegten Titel `uid` (Paket 30): die Stimme beginnt `frames.offset`
    /// Frames nach dem Beginn der Überblendung zu diesem Titel, die Musik wird davor abgesenkt.
    VoiceOver {
        uid: u32,
        buf: Shared<Stereo>,
        gain: f32,
        frames: VoiceFrames,
    },
    /// Bereitgelegten Titel verwerfen.
    ClearNext,
    /// Sendung starten: der bereitgelegte Titel beginnt.
    Play,
    /// Musik kurz ausblenden und anhalten; Carts und Mikro laufen weiter. Verwirft auch
    /// einen pausierten Titel.
    Stop,
    /// Pause: Musik kurz ausblenden und an der Stelle festhalten (laufender und
    /// bereitgelegter Titel bleiben); [`Cmd::Resume`] macht dort weiter.
    Pause,
    /// Nach [`Cmd::Pause`] an der gehaltenen Stelle weiter (ohne neu zu laden).
    Resume,
    /// Wie [`Cmd::Pause`], aber über so viele Frames ausgeblendet (Deck löst die Playlist ab).
    PauseFade(usize),
    /// Die Musik blendet über so viele Frames ein, sobald sie (wieder) läuft; vor
    /// [`Cmd::Resume`] oder [`Cmd::Play`] geschickt (Rückkehr vom Deck zur Playlist).
    FadeIn(usize),
    /// Jetzt zum nächsten Titel überblenden (oder ausblenden, wenn keiner bereitliegt).
    Next,
    /// Im laufenden Titel um so viele Frames springen (negativ: zurück); bleibt zwischen
    /// Cue-In und Cue-Out.
    Seek(i64),
    /// Crossfade-Länge in Frames.
    SetCrossfade(usize),
    /// Stopp mit Ausblenden (#227): so viele Frames blenden [`Cmd::Stop`] und
    /// [`Cmd::DeckStop`] aus; 0 = aus (kurzer harter Stopp).
    SetStopFade(usize),
    /// Absenkung der Musik bei offenem Mikro (linear).
    SetDuck(f32),
    /// Musik auch absenken, solange ein Cart spielt (#149).
    SetCartDuck(bool),
    /// Mikro auf/zu.
    MicOpen(bool),
    /// Mikro im eigenen Ausgang hören (Sendung enthält es immer).
    MicMonitor(bool),
    /// Mikro-Eingang: Stereo verschachtelt in Mixer-Rate, `None` = keiner.
    MicInput(Option<Owned<rtrb::Consumer<f32>>>),
    /// Telefon-Eingang (Anrufer) über eine Taktverbindung in Mixer-Rate, `None` = keiner.
    PhoneInput(Option<Owned<woa_engine::pairs::LinkRx>>),
    /// Telefon auf Sendung: Der Anrufer liegt auf der Sendung und die Musik wird abgesenkt.
    PhoneOn(bool),
    /// Cart abspielen (von vorn, wenn er schon läuft).
    Cart {
        slot: usize,
        buf: Shared<Stereo>,
        gain: f32,
        /// Start und Ende im Puffer (Frames, Cue-In und Cue-Out).
        start: usize,
        end: usize,
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
    /// Ziele der Quellen (Ausgänge je Gerät); Ziel 0 ist der Hauptausgang.
    SetRoutes(crate::routing::Routes),
    /// Titel auf ein Deck legen (Cue-In bis Cue-Out in Frames); das Deck steht dann am Anfang.
    DeckLoad {
        deck: usize,
        buf: Shared<Stereo>,
        gain: f32,
        start: usize,
        end: usize,
    },
    /// Cue-In und Cue-Out eines geladenen Decks ändern (ein stehendes Deck springt an den Anfang).
    DeckCues {
        deck: usize,
        start: usize,
        end: usize,
    },
    /// Deck starten (am Ende: von vorn).
    DeckPlay(usize),
    /// Deck starten und über so viele Frames einblenden (0 = sofort voll).
    DeckPlayFade(usize, usize),
    /// Deck über so viele Frames ausblenden und anhalten (Übergabe an die Playlist).
    DeckFadeOut(usize, usize),
    /// Deck kurz ausblenden und anhalten; die Stelle bleibt.
    DeckStop(usize),
    /// Deck anhalten und an den Anfang (Cue-In) stellen.
    DeckCue(usize),
    /// Deck leeren.
    DeckEject(usize),
    /// Party: Titel auf Seite `side` (0 links, 1 rechts) legen; die Seite steht am Cue-In.
    PartyLoad {
        side: usize,
        buf: Shared<Stereo>,
        gain: f32,
        start: usize,
        end: usize,
    },
    /// Party: Seite starten (am Ende: von vorn).
    PartyPlay(usize),
    /// Party: Seite kurz ausblenden und anhalten; die Stelle bleibt.
    PartyStop(usize),
    /// Party: Seite anhalten und an den Anfang stellen.
    PartyCue(usize),
    /// Party: Seite leeren.
    PartyEject(usize),
    /// Party: auf Seite `side` um so viele Frames springen.
    PartySeek(usize, i64),
    /// Party-Crossfader (0 = links, 1 = rechts). `frames == 0`: von Hand (kurz geglättet),
    /// sonst fährt er in so vielen Frames hin. Eine Handbewegung ersetzt jede Fahrt.
    PartyFader { to: f32, frames: usize },
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
    /// Zählt jeden Titelwechsel (auch wenn derselbe Titel noch einmal beginnt).
    pub plays: AtomicU32,
    pub playing: AtomicBool,
    /// Bit je laufendem Cart.
    pub carts: AtomicU32,
    /// Restliche Frames je Cart (0 = steht).
    pub cart_rest: [AtomicU64; CARTS],
    /// Zustand je Deck: [`DECK_EMPTY`], [`DECK_READY`] oder [`DECK_PLAYING`].
    pub deck_state: [AtomicU32; DECKS],
    /// Stelle im Deck (Frames).
    pub deck_pos: [AtomicU64; DECKS],
    /// Ende im Deck (Frames, Cue-Out).
    pub deck_end: [AtomicU64; DECKS],
    /// Ereignis „Deck fertig“: zählt je Deck, wie oft es bis Cue-Out durchgelaufen ist
    /// (nicht beim Anhalten). Der Controller vergleicht mit seinem letzten Stand.
    pub deck_done: [AtomicU32; DECKS],
    pub mic_open: AtomicBool,
    /// Telefon auf Sendung.
    pub phone_on: AtomicBool,
    /// Spitze des Telefon-Eingangs seit dem letzten Abholen (auch, wenn es nicht auf Sendung ist).
    pub phone_peak: AtomicU32,
    /// Ein Voice-Track läuft (Stimme oder Absenkung davor und danach).
    pub voice: AtomicBool,
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
    /// Spitzen nach dem Fader je Zweig Playlist, Carts, Decks seit letztem Abholen (f32-Bits,
    /// für die LED-Meter neben den Fadern, #216).
    pub bus_peaks: [AtomicU32; 3],
    /// Sendebus: Verstärkung der Lautheitsregelung (dB, f32-Bits).
    pub bus_agc_db: AtomicU32,
    /// Sendebus: größte Absenkung Kompressor + Limiter seit letztem Abholen (dB, f32-Bits).
    pub bus_reduction_db: AtomicU32,
    /// Sendebus: Kurzzeit-Lautheit am Ausgang (LUFS, f32-Bits).
    pub bus_lufs: AtomicU32,
    /// Mikro-Gate offen.
    pub mic_gate_open: AtomicBool,
    /// Party-Seiten: Zustand, Stelle, Ende und „fertig“-Zähler wie bei den Decks.
    pub party_state: [AtomicU32; PARTY_SIDES],
    pub party_pos: [AtomicU64; PARTY_SIDES],
    pub party_end: [AtomicU64; PARTY_SIDES],
    pub party_done: [AtomicU32; PARTY_SIDES],
    /// Stellung des Party-Crossfaders, wie der Mixer sie gerade fährt (f32-Bits).
    pub party_fader: AtomicU32,
}

impl Default for Status {
    fn default() -> Self {
        Self {
            current: AtomicU32::new(NONE),
            next: AtomicU32::new(NONE),
            position: AtomicU64::new(0),
            end: AtomicU64::new(0),
            plays: AtomicU32::new(0),
            playing: AtomicBool::new(false),
            carts: AtomicU32::new(0),
            cart_rest: std::array::from_fn(|_| AtomicU64::new(0)),
            deck_state: std::array::from_fn(|_| AtomicU32::new(DECK_EMPTY)),
            deck_pos: std::array::from_fn(|_| AtomicU64::new(0)),
            deck_end: std::array::from_fn(|_| AtomicU64::new(0)),
            deck_done: std::array::from_fn(|_| AtomicU32::new(0)),
            mic_open: AtomicBool::new(false),
            phone_on: AtomicBool::new(false),
            phone_peak: AtomicU32::new(0),
            voice: AtomicBool::new(false),
            clock: AtomicU64::new(0),
            music_end: AtomicU64::new(0),
            record_dropped: AtomicU64::new(0),
            stream_dropped: AtomicU64::new(0),
            mic_underruns: AtomicU64::new(0),
            peak: AtomicU32::new(0),
            mic_peak: AtomicU32::new(0),
            bus_peaks: std::array::from_fn(|_| AtomicU32::new(0)),
            bus_agc_db: AtomicU32::new(0),
            bus_reduction_db: AtomicU32::new(0),
            bus_lufs: AtomicU32::new((-70.0f32).to_bits()),
            mic_gate_open: AtomicBool::new(false),
            party_state: std::array::from_fn(|_| AtomicU32::new(DECK_EMPTY)),
            party_pos: std::array::from_fn(|_| AtomicU64::new(0)),
            party_end: std::array::from_fn(|_| AtomicU64::new(0)),
            party_done: std::array::from_fn(|_| AtomicU32::new(0)),
            party_fader: AtomicU32::new(0),
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

    /// Spitzen der Zweige Playlist, Carts, Decks seit dem letzten Aufruf.
    pub fn take_bus_peaks(&self) -> [f32; 3] {
        std::array::from_fn(|k| f32::from_bits(self.bus_peaks[k].swap(0, Ordering::Relaxed)))
    }

    /// Spitze des Telefon-Eingangs seit dem letzten Aufruf.
    pub fn take_phone_peak(&self) -> f32 {
        f32::from_bits(self.phone_peak.swap(0, Ordering::Relaxed))
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

/// Ein Deck (A, B, C): ein Titel, der auf Knopfdruck startet und anhält.
struct DeckPlay {
    buf: Shared<Stereo>,
    pos: usize,
    start: usize,
    end: usize,
    gain: f32,
    playing: bool,
    /// Restliche Frames der Ausblendung beim Anhalten.
    stopping: Option<usize>,
    /// Länge der laufenden Ausblendung (Frames).
    stop_total: usize,
    /// Einblenden: (gespielte Frames, Länge).
    fade_in: Option<(usize, usize)>,
}

impl DeckPlay {
    fn new(buf: Shared<Stereo>, gain: f32, start: usize, end: usize) -> Self {
        let end = end.min(buf.frames());
        let start = start.min(end);
        Self {
            buf,
            pos: start,
            start,
            end,
            gain,
            playing: false,
            stopping: None,
            stop_total: 1,
            fade_in: None,
        }
    }

    /// Starten; `fade` Frames einblenden (0 = sofort voll).
    fn play(&mut self, fade: usize) {
        if self.pos >= self.end {
            self.pos = self.start;
        }
        self.stopping = None;
        self.fade_in = (fade > 0).then_some((0, fade));
        self.playing = self.pos < self.end;
    }

    /// Über `frames` ausblenden und anhalten (die Stelle bleibt).
    fn fade_out(&mut self, frames: usize) {
        if self.playing && self.stopping.is_none() {
            let f = frames.max(1);
            self.stopping = Some(f);
            self.stop_total = f;
        }
    }

    /// Anhalten und an den Anfang.
    fn cue(&mut self) {
        self.playing = false;
        self.stopping = None;
        self.pos = self.start;
    }

    /// Ein Frame: das Sample (mit Titel-Gain und Blenden) und ob das Deck gerade bis Cue-Out
    /// durchgelaufen ist. Steht das Deck, kommt Stille.
    #[inline]
    fn tick(&mut self) -> ((f32, f32), bool) {
        if !self.playing {
            return ((0.0, 0.0), false);
        }
        let mut out = (0.0, 0.0);
        if self.pos < self.end {
            let mut g = self.gain;
            if let Some(n) = self.stopping {
                g *= n as f32 / self.stop_total.max(1) as f32;
                if n <= 1 {
                    self.playing = false;
                    self.stopping = None;
                } else {
                    self.stopping = Some(n - 1);
                }
            }
            if let Some((p, len)) = self.fade_in {
                g *= crossfade(p as f32 / len as f32).1;
                self.fade_in = (p + 1 < len).then_some((p + 1, len));
            }
            out = (self.buf.left[self.pos] * g, self.buf.right[self.pos] * g);
            self.pos += 1;
        }
        if self.pos >= self.end {
            self.playing = false;
            self.stopping = None;
            self.fade_in = None;
            return (out, true);
        }
        (out, false)
    }

    /// (Zustand, Stelle, Ende) für [`Status`].
    fn state(d: Option<&Self>) -> (u32, u64, u64) {
        match d {
            None => (DECK_EMPTY, 0, 0),
            Some(d) => (
                if d.playing { DECK_PLAYING } else { DECK_READY },
                d.pos as u64,
                d.end as u64,
            ),
        }
    }
}

struct Voice {
    buf: Shared<Stereo>,
    pos: usize,
    /// Ende (Frames, Cue-Out).
    end: usize,
    gain: f32,
}

/// Ein Voice-Track, der auf seinen Titel wartet.
struct VoicePending {
    uid: u32,
    buf: Shared<Stereo>,
    gain: f32,
    frames: VoiceFrames,
}

/// Ein laufender Voice-Track. `pos` ist negativ, solange nur die Musik abgesenkt wird.
struct VoicePlay {
    buf: Shared<Stereo>,
    gain: f32,
    frames: VoiceFrames,
    pos: i64,
    /// Weiches Ein- und Ausblenden der Ränder (Frames).
    fade: i64,
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
    /// Ausblenden beim Stopp (Frames), 0 = aus: dann gilt `stop_len`.
    stop_fade: usize,
    stopping: Option<usize>,
    /// Länge der laufenden Musik-Ausblendung (Frames).
    stop_total: usize,
    /// Einblenden der Musik: (gespielte Frames, Länge).
    fade_in: Option<(usize, usize)>,
    /// Die laufende Ausblendung ist eine Pause: am Ende bleibt der Titel stehen.
    pausing: bool,
    /// Pausiert: der Titel steht und wird nicht weitergespielt.
    held: bool,
    carts: [Option<Voice>; CARTS],
    decks: [Option<DeckPlay>; DECKS],
    /// Party: zwei eigene Seiten über den Crossfader, im Musikzweig.
    party: [Option<DeckPlay>; PARTY_SIDES],
    party_fader: Fader,
    voice_pending: Option<VoicePending>,
    voice: Option<VoicePlay>,
    mic: Option<Owned<rtrb::Consumer<f32>>>,
    mic_open: bool,
    mic_monitor: bool,
    mic_gain: Smoother,
    /// Telefon: Anrufer als Taktverbindung, auf Sendung ja/nein, Blende, Zwischenblock.
    phone: Option<Owned<woa_engine::pairs::LinkRx>>,
    phone_on: bool,
    phone_gain: Smoother,
    phone_l: [f32; PHONE_CHUNK],
    phone_r: [f32; PHONE_CHUNK],
    duck_lin: f32,
    /// Carts senken die Musik ab (#149).
    cart_duck: bool,
    /// Beim letzten Blockanfang spielte mindestens ein Cart.
    carts_on: bool,
    duck: Smoother,
    mic_backlog: usize,
    rec: Option<Owned<rtrb::Producer<f32>>>,
    stream: Option<Owned<rtrb::Producer<f32>>>,
    clock: u64,
    bus: BusProcessor,
    bus_on: bool,
    mic_chain: MicChain,
    mic_chain_on: bool,
    /// Pegel Musik, Carts, Mikro, Decks, Abhören, Stream (geglättet).
    levels: [Smoother; 6],
    /// Gast-Kanäle mit Mix-Minus (Paket 37).
    guests: GuestBank,
    /// Wohin die Cart-Wall geht (0 = in den Hauptausgang).
    routes: crate::routing::Routes,
}

impl Mixer {
    /// Länge eines Stopps in Frames: mit Ausblenden (#227) die eingestellte Dauer, sonst kurz.
    #[inline]
    fn stop_frames(&self) -> usize {
        if self.stop_fade > 0 {
            self.stop_fade
        } else {
            self.stop_len
        }
    }

    /// Legt den Mixer an (im Control-Thread, danach in den Audio-Thread verschieben).
    pub fn new(
        cmds: rtrb::Consumer<Cmd>,
        status: Arc<Status>,
        rate: u32,
        guests: GuestBank,
    ) -> Self {
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
            stop_fade: 0,
            stopping: None,
            stop_total: (STOP_SECONDS * r).max(1.0) as usize,
            fade_in: None,
            pausing: false,
            held: false,
            carts: std::array::from_fn(|_| None),
            decks: std::array::from_fn(|_| None),
            party: std::array::from_fn(|_| None),
            party_fader: Fader::new(0.0, r),
            voice_pending: None,
            voice: None,
            mic: None,
            mic_open: false,
            mic_monitor: false,
            mic_gain: Smoother::new(0.0, MIC_TAU, r),
            phone: None,
            phone_on: false,
            phone_gain: Smoother::new(0.0, MIC_TAU, r),
            phone_l: [0.0; PHONE_CHUNK],
            phone_r: [0.0; PHONE_CHUNK],
            duck_lin: 0.25,
            cart_duck: true,
            carts_on: false,
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
            guests,
            routes: crate::routing::Routes::default(),
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
                Cmd::VoiceOver {
                    uid,
                    buf,
                    gain,
                    frames,
                } => {
                    self.voice_pending = Some(VoicePending {
                        uid,
                        buf,
                        gain,
                        frames,
                    });
                }
                Cmd::ClearNext => {
                    self.next = None;
                    self.voice_pending = None;
                }
                Cmd::Play => {
                    if self.held {
                        // Neuer Start nach einer Pause: der gehaltene Titel ist vorbei, der
                        // eben bereitgelegte beginnt.
                        self.current = None;
                        self.outgoing = None;
                        self.held = false;
                    }
                    self.playing = true;
                    self.stopping = None;
                    self.pausing = false;
                }
                Cmd::Resume => {
                    self.held = false;
                    self.playing = true;
                    self.stopping = None;
                    self.pausing = false;
                }
                Cmd::Stop => {
                    let len = self.stop_frames();
                    if self.held {
                        self.clear_music();
                    } else if self.playing && self.stopping.is_none() {
                        self.stopping = Some(len);
                        self.stop_total = len;
                    }
                    self.pausing = false;
                }
                Cmd::Pause => {
                    if self.playing && self.stopping.is_none() {
                        self.stopping = Some(self.stop_len);
                        self.stop_total = self.stop_len;
                        self.pausing = true;
                    }
                }
                Cmd::PauseFade(frames) => {
                    if self.playing && self.stopping.is_none() {
                        let f = frames.max(1);
                        self.stopping = Some(f);
                        self.stop_total = f;
                        self.pausing = true;
                    }
                }
                Cmd::FadeIn(frames) => {
                    self.fade_in = (frames > 0).then_some((0, frames));
                }
                Cmd::Next => self.skip(),
                Cmd::Seek(delta) => self.seek(delta),
                Cmd::SetCrossfade(f) => self.xf_frames = f,
                Cmd::SetStopFade(f) => self.stop_fade = f,
                Cmd::SetDuck(g) => {
                    self.duck_lin = g.clamp(0.0, 1.0);
                    self.retarget_duck();
                }
                Cmd::SetCartDuck(on) => {
                    self.cart_duck = on;
                    self.retarget_duck();
                }
                Cmd::MicOpen(on) => {
                    self.mic_open = on;
                    self.mic_gain.set_target(if on { 1.0 } else { 0.0 });
                    self.retarget_duck();
                }
                Cmd::PhoneInput(c) => self.phone = c,
                Cmd::PhoneOn(on) => {
                    self.phone_on = on;
                    self.phone_gain.set_target(if on { 1.0 } else { 0.0 });
                    self.retarget_duck();
                }
                Cmd::MicMonitor(on) => self.mic_monitor = on,
                Cmd::MicInput(c) => self.mic = c,
                Cmd::Cart {
                    slot,
                    buf,
                    gain,
                    start,
                    end,
                } => {
                    if let Some(v) = self.carts.get_mut(slot) {
                        let end = end.min(buf.frames());
                        *v = Some(Voice {
                            buf,
                            pos: start.min(end),
                            end,
                            gain,
                        });
                    }
                }
                Cmd::CartStop(slot) => {
                    if let Some(v) = self.carts.get_mut(slot) {
                        *v = None;
                    }
                }
                Cmd::DeckLoad {
                    deck,
                    buf,
                    gain,
                    start,
                    end,
                } => {
                    if let Some(slot) = self.decks.get_mut(deck) {
                        *slot = Some(DeckPlay::new(buf, gain, start, end));
                    }
                }
                Cmd::DeckCues { deck, start, end } => {
                    if let Some(Some(d)) = self.decks.get_mut(deck) {
                        d.end = end.min(d.buf.frames());
                        d.start = start.min(d.end);
                        if !d.playing {
                            d.pos = d.start;
                        }
                    }
                }
                Cmd::DeckPlay(deck) => {
                    if let Some(Some(d)) = self.decks.get_mut(deck) {
                        d.play(0);
                    }
                }
                Cmd::DeckStop(deck) => {
                    let len = self.stop_frames();
                    if let Some(Some(d)) = self.decks.get_mut(deck) {
                        d.fade_out(len);
                    }
                }
                Cmd::DeckPlayFade(deck, frames) => {
                    if let Some(Some(d)) = self.decks.get_mut(deck) {
                        d.play(frames);
                    }
                }
                Cmd::DeckFadeOut(deck, frames) => {
                    if let Some(Some(d)) = self.decks.get_mut(deck) {
                        d.fade_out(frames);
                    }
                }
                Cmd::DeckCue(deck) => {
                    if let Some(Some(d)) = self.decks.get_mut(deck) {
                        d.cue();
                    }
                }
                Cmd::DeckEject(deck) => {
                    if let Some(slot) = self.decks.get_mut(deck) {
                        *slot = None;
                    }
                }
                Cmd::PartyLoad {
                    side,
                    buf,
                    gain,
                    start,
                    end,
                } => {
                    if let Some(slot) = self.party.get_mut(side) {
                        *slot = Some(DeckPlay::new(buf, gain, start, end));
                    }
                }
                Cmd::PartyPlay(side) => {
                    if let Some(Some(d)) = self.party.get_mut(side) {
                        d.play(0);
                    }
                }
                Cmd::PartyStop(side) => {
                    if let Some(Some(d)) = self.party.get_mut(side) {
                        d.fade_out(self.stop_len);
                    }
                }
                Cmd::PartyCue(side) => {
                    if let Some(Some(d)) = self.party.get_mut(side) {
                        d.cue();
                    }
                }
                Cmd::PartyEject(side) => {
                    if let Some(slot) = self.party.get_mut(side) {
                        *slot = None;
                    }
                }
                Cmd::PartySeek(side, delta) => {
                    if let Some(Some(d)) = self.party.get_mut(side) {
                        let last = d.end.saturating_sub(1).max(d.start);
                        let to = (d.pos as i64).saturating_add(delta).max(d.start as i64);
                        d.pos = (to as usize).min(last);
                    }
                }
                Cmd::PartyFader { to, frames } => self.party_fader.set(to, frames),
                Cmd::Record(p) => self.rec = p,
                Cmd::Stream(p) => self.stream = p,
                Cmd::SetProcessing { bus, mic } => {
                    self.bus_on = bus;
                    self.mic_chain_on = mic;
                }
                Cmd::SetTarget(t) => self.bus.set_target(t),
                Cmd::SetRoutes(r) => self.routes = r,
                Cmd::SetLevel { bus, gain } => {
                    self.levels[bus.index()].set_target(gain.clamp(0.0, 4.0));
                }
            }
        }
    }

    /// Springt im laufenden Titel (kein Sprung während einer Überblendung oder Ausblendung).
    fn seek(&mut self, delta: i64) {
        if !self.playing || self.outgoing.is_some() {
            return;
        }
        if let Some(c) = self.current.as_mut() {
            let last = c.end.saturating_sub(1).max(c.start);
            let to = (c.pos as i64).saturating_add(delta).max(c.start as i64) as usize;
            c.pos = to.min(last);
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

    /// Startet einen wartenden Voice-Track, sobald der Übergang zu seinem Titel nah genug ist:
    /// Die Stimme beginnt `offset` Frames nach dem Beginn der Überblendung, die Absenkung
    /// `attack` Frames vor der Stimme. Kommt der Übergang früher als geplant (etwa „Weiter“),
    /// beginnt die Stimme sofort ganz von vorn, nie mittendrin.
    fn voice_arm(&mut self) {
        let Some(p) = self.voice_pending.as_ref() else {
            return;
        };
        let f = p.frames;
        let pos = match (&self.current, &self.next) {
            // Der Titel läuft schon (auch: erster Titel der Sendung).
            (Some(c), _) if c.uid == p.uid => -f.offset - (c.pos.saturating_sub(c.start)) as i64,
            (Some(c), Some(n)) if n.uid == p.uid && self.outgoing.is_none() => {
                let until = c.remaining() as i64 - self.xf_for(c, n) as i64;
                if until > f.attack - f.offset {
                    return;
                }
                -f.offset - until
            }
            _ => return,
        };
        if let Some(p) = self.voice_pending.take() {
            self.voice = Some(VoicePlay {
                buf: p.buf,
                gain: p.gain,
                frames: p.frames,
                pos: pos.min(-p.frames.attack),
                fade: ((EDGE_SECONDS * f64::from(self.rate_hz)) as i64).max(1),
            });
        }
    }

    /// Laufenden, abgehenden und bereitgelegten Titel verwerfen (Stopp).
    fn clear_music(&mut self) {
        self.current = None;
        self.outgoing = None;
        self.next = None;
        self.voice_pending = None;
        self.held = false;
    }

    fn transitions(&mut self) {
        if !self.playing || self.stopping.is_some() {
            return;
        }
        self.voice_arm();
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
            self.status.plays.fetch_add(1, Ordering::Release);
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
            self.status.plays.fetch_add(1, Ordering::Release);
        }
    }

    /// Rechnet einen Block. `left`/`right` bekommen den Abhörmix (Sendung, Mikro nur mit
    /// [`Cmd::MicMonitor`]); der Mitschnitt bekommt immer die volle Sendung.
    pub fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        self.run(left, right, &mut []);
    }

    /// Wie [`Mixer::process`], aber mit getrennten Ausgängen: `outs[0]` ist der Hauptausgang
    /// (der Abhörmix wie bei `process`), `outs[d]` das Ziel `d` aus [`crate::routing::Routes`].
    /// Eine Quelle mit Ziel `d > 0` fehlt im Hauptausgang und liegt vor dem Sendebus (nach den
    /// Reglern der Oberfläche) auf `outs[d]`; Mitschnitt und Sendung bekommen weiter alles.
    /// Gibt es nur `outs[0]`, ist das `process`. Rechnet `frames` Frames.
    pub fn process_multi(&mut self, outs: &mut [crate::PairBuf], frames: usize) {
        let Some((main, rest)) = outs.split_first_mut() else {
            return;
        };
        let n = frames.min(main.l.len()).min(main.r.len());
        self.run(&mut main.l[..n], &mut main.r[..n], rest);
    }

    fn run(&mut self, left: &mut [f32], right: &mut [f32], extra: &mut [crate::PairBuf]) {
        self.drain();
        let n = left.len().min(right.len());
        for e in extra.iter_mut() {
            e.clear(n);
        }
        // Ziel je Quelle: Carts, Deck A, B, C (Ziel 0 = Hauptausgang, bleibt `None`).
        let dests: [Option<usize>; 1 + DECKS] = std::array::from_fn(|k| {
            match self.routes.dest[crate::routing::Source::Carts.index() + k] as usize {
                d if (1..=extra.len()).contains(&d) => Some(d - 1),
                _ => None,
            }
        });
        self.trim_mic_backlog();
        // Carts ducken die Musik (#149); geprüft je Block, das reicht bei 80 ms Glättung.
        let carts_on = self.carts.iter().any(Option::is_some);
        if carts_on != self.carts_on {
            self.carts_on = carts_on;
            self.retarget_duck();
        }
        self.guests.begin_block();
        let mut peak = 0.0f32;
        let mut mic_peak = 0.0f32;
        let mut phone_peak = 0.0f32;
        let mut bus_peaks = [0.0f32; 3];
        let ret_dest = match self.routes.dest[crate::routing::Source::PhoneReturn.index()] as usize
        {
            d if (1..=extra.len()).contains(&d) => Some(d - 1),
            _ => None,
        };
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
            if let Some(c) = self.current.as_mut().filter(|_| !self.held) {
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
            if let Some((p, len)) = self.fade_in
                && self.playing
                && !self.held
                && self.current.is_some()
            {
                // Rückkehr vom Deck: die Musik blendet ein.
                let g = crossfade(p as f32 / len as f32).1;
                ml *= g;
                mr *= g;
                self.fade_in = (p + 1 < len).then_some((p + 1, len));
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
                let g = k as f32 / self.stop_total.max(1) as f32;
                ml *= g;
                mr *= g;
                if k <= 1 {
                    if self.pausing {
                        // Pause: der Titel bleibt an dieser Stelle stehen.
                        self.outgoing = None;
                        self.held = self.current.is_some();
                        self.pausing = false;
                    } else {
                        self.clear_music();
                    }
                    self.playing = false;
                    self.stopping = None;
                } else {
                    self.stopping = Some(k - 1);
                }
            }

            // Party: zwei Seiten über den Crossfader (Constant-Power), im Musikzweig.
            let (pa, pb) = fader_gains(self.party_fader.tick());
            for (k, (slot, g)) in self.party.iter_mut().zip([pa, pb]).enumerate() {
                if let Some(d) = slot {
                    let ((l, r), ended) = d.tick();
                    ml += l * g;
                    mr += r * g;
                    if ended {
                        self.status.party_done[k].fetch_add(1, Ordering::Release);
                    }
                }
            }

            // Carts.
            let (mut cl, mut cr) = (0.0f32, 0.0f32);
            for slot in self.carts.iter_mut() {
                if let Some(v) = slot {
                    if v.pos < v.end {
                        cl += v.buf.left[v.pos] * v.gain;
                        cr += v.buf.right[v.pos] * v.gain;
                        v.pos += 1;
                    } else {
                        *slot = None;
                    }
                }
            }

            let (cart_l, cart_r) = (cl, cr);

            // Decks.
            let mut deck_out = [(0.0f32, 0.0f32); DECKS];
            let ldg = self.levels[3].tick();
            for (k, (slot, out)) in self.decks.iter_mut().zip(deck_out.iter_mut()).enumerate() {
                if let Some(d) = slot {
                    let ((l, r), ended) = d.tick();
                    *out = (l * ldg, r * ldg);
                    if ended {
                        // Ereignis „Deck fertig“: nur ein Zähler, keine Allokation.
                        self.status.deck_done[k].fetch_add(1, Ordering::Release);
                    }
                }
            }
            let (dl, dr) = deck_out
                .iter()
                .fold((0.0f32, 0.0f32), |a, b| (a.0 + b.0, a.1 + b.1));

            // Voice-Track: Stimme auf den Carts-Zweig, Musik wird abgesenkt.
            let mut vduck = 1.0f32;
            if let Some(v) = self.voice.as_mut() {
                let f = v.frames;
                if v.pos >= 0 && v.pos < f.len {
                    let k = v.pos as usize;
                    let g = v.gain * edge_gain(v.pos, f.len, v.fade);
                    cl += v.buf.left.get(k).copied().unwrap_or(0.0) * g;
                    cr += v.buf.right.get(k).copied().unwrap_or(0.0) * g;
                }
                vduck = duck_gain(v.pos, f.len, f.attack, f.release, f.depth);
                v.pos += 1;
                if v.pos >= f.len + f.release {
                    self.voice = None;
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

            // Telefon: der Anrufer. Der Pegel zeigt auch, wenn er nicht auf Sendung ist.
            let (mut pl, mut pr) = (0.0f32, 0.0f32);
            if let Some(p) = self.phone.as_mut() {
                let k = i % PHONE_CHUNK;
                if k == 0 {
                    let m = (n - i).min(PHONE_CHUNK);
                    p.pull(&mut self.phone_l[..m], &mut self.phone_r[..m]);
                }
                (pl, pr) = (self.phone_l[k], self.phone_r[k]);
            }
            phone_peak = phone_peak.max(pl.abs()).max(pr.abs());
            let pg = self.phone_gain.tick();
            pl *= pg;
            pr *= pg;
            let [lm, lc, li, _, lo, ls] = &mut self.levels;
            let (out_mon, out_stream) = (lo.tick(), ls.tick());
            let (lm, lc) = (lm.tick(), lc.tick());
            let mg = self.mic_gain.tick() * li.tick();
            let duck = self.duck.tick() * lm * vduck;
            cl *= lc;
            let (cart_l, cart_r) = (cart_l * lc, cart_r * lc);
            cr *= lc;
            il *= mg;
            ir *= mg;
            mic_peak = mic_peak.max(il.abs()).max(ir.abs());
            bus_peaks[0] = bus_peaks[0].max((ml * duck).abs()).max((mr * duck).abs());
            bus_peaks[1] = bus_peaks[1].max(cl.abs()).max(cr.abs());
            bus_peaks[2] = bus_peaks[2].max(dl.abs()).max(dr.abs());

            // Basis ohne Gäste: Musik, Carts, Moderator. Daraus entstehen die Rückwege.
            let (bl, br) = (ml * duck + cl + il + dl, mr * duck + cr + ir + dr);
            let (gl, gr) = self.guests.frame();
            // Rückweg zum Anrufer (Mix-Minus): alles, was auf Sendung ist, außer dem Anrufer.
            let (mut tl, mut tr) = (bl + gl, br + gr);
            let mut sl = tl + pl;
            let mut sr = tr + pr;
            // Abhören ohne Mikro: derselbe Weg ohne `il`/`ir`, mit demselben Faktor.
            let (mut ql, mut qr) = (sl - il, sr - ir);
            // Rückweg eines Gastes im Kopfhörer: Basis plus alle anderen Gäste (Mix-Minus).
            let mut gain_out = 1.0f32;
            let mut ret = self
                .guests
                .monitor()
                .map(|g| self.guests.return_bus(g, (bl, br)));
            if self.bus_on {
                let g = self.bus.gain(sl, sr);
                gain_out = g;
                sl = self.bus.clip(sl * g);
                sr = self.bus.clip(sr * g);
                ql = self.bus.clip(ql * g);
                qr = self.bus.clip(qr * g);
                tl = self.bus.clip(tl * g);
                tr = self.bus.clip(tr * g);
                if let Some((rl, rr)) = ret.as_mut() {
                    *rl = self.bus.clip(*rl * g);
                    *rr = self.bus.clip(*rr * g);
                }
                // Rückwege ins Netz: dieselbe Summenbearbeitung wie die Sendung.
                let bus = &self.bus;
                self.guests
                    .send_returns((bl, br), |l, r| (bus.clip(l * g), bus.clip(r * g)));
                let m = self.bus.meter();
                reduction = reduction.max(m.comp_db + m.limit_db);
            } else {
                self.guests.send_returns((bl, br), |l, r| (l, r));
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
                    let _ = p.push(sl * out_stream);
                    let _ = p.push(sr * out_stream);
                } else {
                    self.status.stream_dropped.fetch_add(2, Ordering::Relaxed);
                }
            }
            let (ol, or) = match ret {
                Some(r) => r,
                None if self.mic_monitor => (sl, sr),
                None => (ql, qr),
            };
            left[i] = ol;
            right[i] = or;
            // Getrennte Quellen: vor dem Sendebus auf das eigene Ziel; im Hauptausgang fehlt das
            // Stück, das der Hauptausgang mit demselben Faktor geführt hätte.
            let srcs = [(cart_l, cart_r), deck_out[0], deck_out[1], deck_out[2]];
            for (dest, (xl, xr)) in dests.iter().zip(srcs) {
                if let Some(d) = *dest {
                    extra[d].l[i] += xl;
                    extra[d].r[i] += xr;
                    left[i] -= xl * gain_out;
                    right[i] -= xr * gain_out;
                }
            }
            left[i] *= out_mon;
            right[i] *= out_mon;
            if let Some(d) = ret_dest {
                extra[d].l[i] += tl;
                extra[d].r[i] += tr;
            }
            peak = peak.max(sl.abs()).max(sr.abs());
            self.clock += 1;
        }
        self.guests.end_block();
        self.publish(peak, mic_peak, reduction);
        store_max(&self.status.phone_peak, phone_peak);
        for (cell, v) in self.status.bus_peaks.iter().zip(bus_peaks) {
            store_max(cell, v);
        }
    }

    /// Die Musik wird abgesenkt, solange das Mikro offen ist, das Telefon auf Sendung oder
    /// (abschaltbar) ein Cart spielt.
    fn retarget_duck(&mut self) {
        let down = self.mic_open || self.phone_on || (self.cart_duck && self.carts_on);
        self.duck.set_target(if down { self.duck_lin } else { 1.0 });
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
            let rest = v.as_ref().map_or(0, |v| v.end.saturating_sub(v.pos) as u64);
            s.cart_rest[i].store(rest, Ordering::Release);
        }
        s.carts.store(mask, Ordering::Release);
        for (k, d) in self.decks.iter().enumerate() {
            let (state, pos, end) = DeckPlay::state(d.as_ref());
            s.deck_state[k].store(state, Ordering::Release);
            s.deck_pos[k].store(pos, Ordering::Release);
            s.deck_end[k].store(end, Ordering::Release);
        }
        for (k, d) in self.party.iter().enumerate() {
            let (state, pos, end) = DeckPlay::state(d.as_ref());
            s.party_state[k].store(state, Ordering::Release);
            s.party_pos[k].store(pos, Ordering::Release);
            s.party_end[k].store(end, Ordering::Release);
        }
        s.party_fader
            .store(self.party_fader.pos().to_bits(), Ordering::Release);
        s.mic_open.store(self.mic_open, Ordering::Release);
        s.phone_on.store(self.phone_on, Ordering::Release);
        s.voice.store(self.voice.is_some(), Ordering::Release);
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
