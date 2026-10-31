//! Control-Seite des Sendeablaufs: Playlist-Zeiger, Vorladen, Carts, Mitschnitt.
//!
//! Läuft nie im Audio-Thread. Dekodiert Dateien, legt den nächsten Titel beim [`Mixer`]
//! bereit, schreibt den Mitschnitt aus dem Ring in eine WAV-Datei und gibt verworfene
//! Puffer über den `basedrop`-Collector frei ([`Controller::pump`] regelmäßig aufrufen).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering;

use basedrop::{Collector, Owned, Shared};

use crate::audio::{Stereo, load_stereo, measure};
use crate::cues::{CueEdit, CueHistory, Cues};
use crate::events::{Action, EventClock, EventTable, MissedWhy};
use crate::level::db_to_gain;
use crate::mixer::{Cmd, Mixer, NONE, Status};
use crate::pfl::{PflCmd, PflHandle};
use crate::show::{CartEntry, PlaylistEntry, RadioShow};
use crate::{CARTS, RECORDINGS_DIR};

/// Kommandos im Flug zum Audio-Thread.
const QUEUE: usize = 256;
/// Sekunden Mitschnitt-Puffer zwischen Audio- und Control-Thread.
const RECORD_QUEUE_SECONDS: usize = 8;
/// So lang zählt ein noch ungemessener Titel bei der Planung eines harten Events.
const UNMEASURED_SECONDS: f64 = 180.0;
/// Eine Mitschnitt-Datei je so vielen Sekunden (WAV-Grenze 4 GiB).
pub const RECORD_FILE_SECONDS: u64 = 3600;

/// Wiederholen nach Art von Apple Music: aus, ganze Playlist, ein Titel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Repeat {
    #[default]
    Off,
    /// Nach dem letzten Titel beginnt die Playlist von vorn.
    All,
    /// Am Ende beginnt derselbe Titel neu; „Weiter“ springt trotzdem.
    One,
}

impl Repeat {
    /// Der nächste Zustand beim Klick: aus, Playlist, Titel, aus.
    pub fn cycled(self) -> Self {
        match self {
            Repeat::Off => Repeat::All,
            Repeat::All => Repeat::One,
            Repeat::One => Repeat::Off,
        }
    }

    /// Name für Anzeige-Zustand und Fernbedienung.
    pub fn name(self) -> &'static str {
        match self {
            Repeat::Off => "off",
            Repeat::All => "all",
            Repeat::One => "one",
        }
    }
}

type WavOut = hound::WavWriter<std::io::BufWriter<std::fs::File>>;

/// Ein laufender Mitschnitt.
struct Recording {
    dir: PathBuf,
    consumer: rtrb::Consumer<f32>,
    writer: Option<(PathBuf, WavOut)>,
    frames_in_file: u64,
    rate: u32,
    files: Vec<PathBuf>,
}

impl Recording {
    fn open_file(&mut self) -> Result<(), String> {
        std::fs::create_dir_all(&self.dir).map_err(|e| format!("{}: {e}", self.dir.display()))?;
        let stamp = crate::utc_now_stamp();
        let mut path = self.dir.join(format!("sendung-{stamp}.wav"));
        let mut n = 1;
        while path.exists() {
            path = self.dir.join(format!("sendung-{stamp}-{n}.wav"));
            n += 1;
        }
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: self.rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        let w = hound::WavWriter::new(std::io::BufWriter::new(file), spec)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        tracing::info!(file = %path.display(), "mitschnitt file opened");
        self.files.push(path.clone());
        self.writer = Some((path, w));
        self.frames_in_file = 0;
        Ok(())
    }

    fn close_file(&mut self) -> Result<(), String> {
        if let Some((p, w)) = self.writer.take() {
            w.finalize().map_err(|e| format!("{}: {e}", p.display()))?;
        }
        Ok(())
    }

    /// Schreibt alles, was im Ring liegt.
    fn drain(&mut self) -> Result<(), String> {
        let limit = RECORD_FILE_SECONDS * u64::from(self.rate);
        while self.consumer.slots() >= 2 {
            if self.writer.is_none() || self.frames_in_file >= limit {
                self.close_file()?;
                self.open_file()?;
            }
            let (Ok(l), Ok(r)) = (self.consumer.pop(), self.consumer.pop()) else {
                break;
            };
            if let Some((p, w)) = self.writer.as_mut() {
                for s in [l, r] {
                    let v = (s.clamp(-1.0, 1.0) * 32_767.0).round() as i16;
                    w.write_sample(v)
                        .map_err(|e| format!("{}: {e}", p.display()))?;
                }
            }
            self.frames_in_file += 1;
        }
        if let Some((p, w)) = self.writer.as_mut() {
            w.flush().map_err(|e| format!("{}: {e}", p.display()))?;
        }
        Ok(())
    }
}

/// Steuert einen [`Mixer`].
pub struct Controller {
    collector: Collector,
    cmds: rtrb::Producer<Cmd>,
    status: Arc<Status>,
    rate: u32,
    show: RadioShow,
    next_uid: u32,
    /// Bereitgelegter, vom Mixer noch nicht begonnener Titel.
    pending: Option<u32>,
    /// Titelwechsel-Zähler des Mixers, als `pending` bereitgelegt wurde: so ist erkennbar,
    /// dass auch derselbe Titel (Wiederholen) wirklich neu begonnen hat.
    pending_plays: u32,
    /// Pausiert: Titel (uid), Stelle und Ende in Frames. `None`, wenn nicht pausiert.
    paused: Option<(u32, u64, u64)>,
    /// Nach dem Beginn des Titels (uid) einmal an diese Stelle springen; dazu der Stand des
    /// Titelwechsel-Zählers beim Start.
    resume_at: Option<(u32, u64, u32)>,
    /// Wiederholen und Zufall (laufen nur, nicht in der gespeicherten Sendung).
    repeat: Repeat,
    shuffle: bool,
    /// Bei Zufall schon gespielte Titel (uid).
    played: Vec<u32>,
    rng: u64,
    /// Titel, mit dem „Start“ beginnt (uid), falls nichts läuft.
    cue: Option<u32>,
    running: bool,
    carts: Vec<Option<Shared<Stereo>>>,
    /// Dekodierte Titel der Decks A bis C.
    decks: Vec<Option<Shared<Stereo>>>,
    recording: Option<Recording>,
    message: Option<String>,
    /// Titel, die sich nicht laden ließen (werden übersprungen).
    failed: Vec<u32>,
    /// Zuletzt geladener Titel und wie lange das Dekodieren dauerte (für das Log, #51).
    last_load: Option<(PathBuf, std::time::Duration)>,
    /// Undo/Redo der Cue-Punkte (Paket 15).
    cue_history: CueHistory,
    /// Vorhören (PFL), falls ein Kopfhörer-Ausgang eingehängt ist.
    pfl: Option<PflHandle>,
    /// Feste Zeit-Events (Paket 29).
    events: EventClock,
    /// Vorab geladene Event-Dateien (nach Event-Kennung).
    event_files: HashMap<u32, Shared<Stereo>>,
    /// Playlist-Einträge, die Zeit-Events sind (nach uid).
    event_runs: HashMap<u32, EventRun>,
    /// Letztes gespieltes Event und seine Abweichung in Sekunden (positiv: zu spät).
    last_event: Option<(String, f64)>,
    /// Verpasste Events seit dem Start.
    events_missed: u32,
    /// Vorhör-Rendering eines Voice-Tracks läuft im Hintergrund (Paket 30).
    preview: Option<std::sync::mpsc::Receiver<Result<Stereo, String>>>,
    /// Gast-Kanäle (Paket 37).
    guests: woa_guests::GuestHub,
}

/// Ein Zeit-Event in der Playlist.
#[derive(Debug, Clone)]
struct EventRun {
    id: u32,
    name: String,
    at: i64,
    hard: bool,
    started: bool,
}

impl Controller {
    /// Zuletzt geladener Titel und Dauer des Dekodierens.
    pub fn last_load(&self) -> Option<&(PathBuf, std::time::Duration)> {
        self.last_load.as_ref()
    }

    /// Legt Controller und Mixer an. Der Mixer gehört danach in den Audio-Callback.
    pub fn new(show: RadioShow, rate: u32) -> (Controller, Mixer) {
        let (producer, consumer) = rtrb::RingBuffer::new(QUEUE);
        let status = Arc::new(Status::default());
        let (guests, bank) = woa_guests::channel(rate);
        let mixer = Mixer::new(consumer, Arc::clone(&status), rate, bank);
        let mut c = Controller {
            collector: Collector::new(),
            cmds: producer,
            status,
            rate: rate.max(1),
            show: RadioShow::default(),
            next_uid: 1,
            pending: None,
            pending_plays: 0,
            paused: None,
            resume_at: None,
            repeat: Repeat::Off,
            shuffle: false,
            played: Vec::new(),
            rng: 0x9e37_79b9_7f4a_7c15 ^ wall_now().to_bits(),
            cue: None,
            running: false,
            carts: vec![None; CARTS],
            decks: vec![None; crate::DECKS],
            recording: None,
            message: None,
            failed: Vec::new(),
            last_load: None,
            cue_history: CueHistory::default(),
            pfl: None,
            events: EventClock::default(),
            event_files: HashMap::new(),
            event_runs: HashMap::new(),
            last_event: None,
            events_missed: 0,
            preview: None,
            guests,
        };
        c.replace_show(show);
        (c, mixer)
    }

    fn send(&mut self, cmd: Cmd) {
        if self.cmds.push(cmd).is_err() {
            tracing::warn!("radio command queue full");
            self.message = Some("Befehl verloren (Warteschlange voll)".into());
        }
    }

    /// Ersetzt Playlist, Carts und Einstellungen (etwa nach Laden von radio.json).
    /// Was gerade läuft, läuft weiter, wenn es noch in der Playlist steht.
    pub fn replace_show(&mut self, mut show: RadioShow) {
        show.sanitize();
        let old: Vec<(PathBuf, u32)> = self
            .show
            .playlist
            .iter()
            .map(|e| (e.path.clone(), e.uid))
            .collect();
        for e in &mut show.playlist {
            // Laufzeit-Kennung behalten, damit der laufende Titel erkannt bleibt.
            e.uid = match old.iter().position(|(p, _)| *p == e.path) {
                Some(i) => old[i].1,
                None => {
                    let u = self.next_uid;
                    self.next_uid += 1;
                    u
                }
            };
        }
        let carts_changed = show.carts != self.show.carts;
        let decks_changed = show.decks != self.show.decks;
        self.show = show;
        if decks_changed {
            for deck in 0..crate::DECKS {
                self.load_deck(deck);
            }
        }
        if carts_changed {
            self.carts = vec![None; CARTS];
            for slot in 0..CARTS {
                self.load_cart(slot);
            }
        }
        self.apply_settings();
        self.requeue();
    }

    fn apply_settings(&mut self) {
        let frames = (self.show.crossfade_seconds * f64::from(self.rate)) as usize;
        self.send(Cmd::SetCrossfade(frames));
        self.send(Cmd::SetDuck(db_to_gain(self.show.duck_db)));
        self.send(Cmd::SetTarget(self.show.target_lufs));
        self.send(Cmd::SetProcessing {
            bus: self.show.bus_processing,
            mic: self.show.mic_processing,
        });
    }

    pub fn show(&self) -> &RadioShow {
        &self.show
    }

    pub fn status(&self) -> &Arc<Status> {
        &self.status
    }

    pub fn rate(&self) -> u32 {
        self.rate
    }

    /// Meldung für die Oberfläche hinterlegen (etwa ein Fehler aus einem Auftrag).
    pub fn report(&mut self, message: String) {
        tracing::warn!(%message, "radio");
        self.message = Some(message);
    }

    /// Letzte Meldung für die Oberfläche (einmal abholen).
    pub fn take_message(&mut self) -> Option<String> {
        self.message.take()
    }

    fn index_of(&self, uid: u32) -> Option<usize> {
        self.show.playlist.iter().position(|e| e.uid == uid)
    }

    /// Index des laufenden Titels.
    pub fn current_index(&self) -> Option<usize> {
        let cur = self.status.current.load(Ordering::Acquire);
        if cur == NONE {
            return None;
        }
        self.index_of(cur)
    }

    /// Index, mit dem „Start“ beginnt.
    pub fn cue_index(&self) -> usize {
        self.cue.and_then(|u| self.index_of(u)).unwrap_or(0)
    }

    /// Wählt den Titel, mit dem „Start“ beginnt.
    pub fn set_cue(&mut self, index: usize) {
        self.cue = self.show.playlist.get(index).map(|e| e.uid);
    }

    pub fn is_running(&self) -> bool {
        self.running
    }

    // ---- Playlist bearbeiten ---------------------------------------------------------

    /// Hängt Dateien an (oder fügt bei `at` ein). Gibt die Indizes zurück.
    #[tracing::instrument(level = "info", skip_all, name = "playlist_add")]
    pub fn add(&mut self, paths: &[PathBuf], at: Option<usize>) -> Vec<usize> {
        let mut at = at.unwrap_or(self.show.playlist.len());
        let mut out = Vec::new();
        for p in paths {
            let mut e = PlaylistEntry::new(p.clone());
            e.uid = self.next_uid;
            self.next_uid += 1;
            at = at.min(self.show.playlist.len());
            self.show.playlist.insert(at, e);
            out.push(at);
            at += 1;
        }
        self.requeue();
        out
    }

    pub fn remove(&mut self, index: usize) -> bool {
        if index >= self.show.playlist.len() {
            return false;
        }
        self.show.playlist.remove(index);
        self.requeue();
        true
    }

    pub fn move_entry(&mut self, from: usize, to: usize) -> bool {
        let ok = self.show.move_entry(from, to);
        if ok {
            self.requeue();
        }
        ok
    }

    /// Übernimmt eine Messung (aus dem Hintergrund-Thread) für alle Einträge mit `path`.
    pub fn set_measured(&mut self, path: &Path, m: crate::audio::Measured) {
        for e in &mut self.show.playlist {
            if e.path == path {
                e.set_measured(m);
            }
        }
    }

    /// Misst alle Titel ohne gültige Messung sofort (blockiert; für Offline und Agent).
    pub fn measure_missing(&mut self) {
        let rate = self.rate;
        for e in &mut self.show.playlist {
            if !e.is_measured() {
                match load_stereo(&e.path, rate) {
                    Ok(s) => e.set_measured(measure(&s, rate)),
                    Err(err) => tracing::warn!(error = %err, "measure failed"),
                }
            }
        }
    }

    // ---- Einstellungen ---------------------------------------------------------------

    pub fn set_crossfade(&mut self, seconds: f64) {
        self.show.crossfade_seconds = seconds;
        self.show.sanitize();
        self.apply_settings();
    }

    pub fn set_duck(&mut self, db: f64) {
        self.show.duck_db = db;
        self.show.sanitize();
        self.apply_settings();
    }

    /// Sendebus- und Mikro-Processing an/aus.
    pub fn set_processing(&mut self, bus: bool, mic: bool) {
        self.show.bus_processing = bus;
        self.show.mic_processing = mic;
        self.apply_settings();
    }

    /// Pegel von Musik, Carts oder Mikro in dB (Paket 14, Fader am Controller);
    /// −60 dB und weniger = aus, höchstens +6 dB.
    pub fn set_level(&mut self, bus: crate::mixer::LevelBus, db: f64) {
        let db = db.clamp(crate::mixer::LEVEL_MIN_DB, crate::mixer::LEVEL_MAX_DB);
        let gain = if db <= crate::mixer::LEVEL_MIN_DB {
            0.0
        } else {
            10f64.powf(db / 20.0) as f32
        };
        self.send(Cmd::SetLevel { bus, gain });
    }

    // ---- Transport -------------------------------------------------------------------

    /// Wiederholen-Modus.
    pub fn repeat(&self) -> Repeat {
        self.repeat
    }

    pub fn set_repeat(&mut self, r: Repeat) {
        if self.repeat != r {
            self.repeat = r;
            self.requeue_choice();
        }
    }

    /// Ein Knopf, drei Zustände: aus, Playlist, Titel, aus.
    pub fn cycle_repeat(&mut self) {
        self.set_repeat(self.repeat.cycled());
    }

    /// Zufällige Reihenfolge.
    pub fn shuffle(&self) -> bool {
        self.shuffle
    }

    pub fn set_shuffle(&mut self, on: bool) {
        if self.shuffle != on {
            self.shuffle = on;
            self.played.clear();
            if on && let Some(i) = self.current_index() {
                // Was gerade läuft, zählt als gespielt.
                let uid = self.show.playlist[i].uid;
                self.played.push(uid);
            }
            self.requeue_choice();
        }
    }

    pub fn toggle_shuffle(&mut self) {
        self.set_shuffle(!self.shuffle);
    }

    /// Die Wahl des nächsten Titels hat sich geändert: den bereitgelegten verwerfen, der Takt
    /// legt einen neuen bereit.
    fn requeue_choice(&mut self) {
        self.requeue();
    }

    fn rand(&mut self) -> u64 {
        // xorshift64: reicht für eine Zufallsfolge von Titeln.
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.rng = x;
        x
    }

    /// Titel, die als Nächstes infrage kommen, in Reihenfolge der Wahl (nur Musik, keine
    /// Voice-Tracks). `after`: erster Index der normalen Reihenfolge. `honor_one`: bei
    /// „Titel wiederholen“ kommt zuerst derselbe Titel (aus bei „Weiter“).
    fn candidates(&mut self, cur: Option<usize>, after: usize, honor_one: bool) -> Vec<usize> {
        let n = self.show.playlist.len();
        let music = |s: &Self, i: usize| s.show.playlist[i].voice.is_none();
        let mut out = Vec::new();
        if honor_one
            && self.repeat == Repeat::One
            && let Some(i) = cur.filter(|&i| i < n && music(self, i))
        {
            out.push(i);
        }
        if self.shuffle {
            let mut pool: Vec<usize> = (0..n)
                .filter(|&i| music(self, i) && Some(i) != cur)
                .filter(|&i| !self.played.contains(&self.show.playlist[i].uid))
                .collect();
            if pool.is_empty() && self.repeat == Repeat::All {
                // Alle durch: eine neue Runde.
                self.played.clear();
                pool = (0..n)
                    .filter(|&i| music(self, i) && Some(i) != cur)
                    .collect();
            }
            if !pool.is_empty() {
                let k = (self.rand() % pool.len() as u64) as usize;
                out.push(pool[k]);
            }
        } else {
            out.extend((after..n).filter(|&i| music(self, i)));
            if self.repeat == Repeat::All {
                let upto = cur.map_or(0, |c| c + 1).min(n);
                out.extend((0..upto).filter(|&i| music(self, i)));
            }
        }
        out
    }

    /// Spielt Titel `index` jetzt: Überblendung dorthin (oder Start, wenn nichts läuft).
    fn jump_to(&mut self, index: usize) {
        if self.running && self.status.current.load(Ordering::Acquire) != NONE {
            self.send(Cmd::ClearNext);
            self.pending = None;
            if self.queue_index(index) {
                self.send(Cmd::Next);
            }
        } else {
            self.set_cue(index);
            self.start();
        }
    }

    /// Vorheriger Titel; am Anfang der Playlist beginnt der laufende Titel neu.
    pub fn previous(&mut self) {
        let cur = self.current_index().unwrap_or_else(|| self.cue_index());
        let to = self.show.prev_music(cur).unwrap_or(cur);
        self.jump_to(to);
    }

    /// Der laufende (bzw. bereitliegende) Titel beginnt von vorn.
    pub fn restart(&mut self) {
        if self.running && self.status.current.load(Ordering::Acquire) != NONE {
            // Ohne Überblendung: zurück auf Cue-In (der Mixer begrenzt den Sprung).
            self.send(Cmd::Seek(-1_000_000_000_000));
        } else {
            let cur = self.current_index().unwrap_or_else(|| self.cue_index());
            self.set_cue(cur);
            self.start();
        }
    }

    /// Im laufenden Titel um `seconds` springen (negativ: zurück).
    pub fn seek(&mut self, seconds: f64) {
        if self.status.current.load(Ordering::Acquire) == NONE || !seconds.is_finite() {
            return;
        }
        let frames = (seconds * f64::from(self.rate)).round() as i64;
        self.send(Cmd::Seek(frames));
    }

    /// Start (wenn nichts läuft) oder Weiter (Überblenden zum nächsten Titel). Bei „Titel
    /// wiederholen“ springt Weiter trotzdem zum nächsten Titel.
    pub fn start_or_next(&mut self) {
        if self.paused.is_some() {
            self.play();
            return;
        }
        if self.running && self.status.current.load(Ordering::Acquire) != NONE {
            if self.repeat == Repeat::One {
                let cur = self.current_index();
                let after = cur.map_or(0, |i| i + 1);
                let target = self.candidates(cur, after, false).into_iter().next();
                if let Some(t) = target {
                    self.jump_to(t);
                    return;
                }
            }
            self.send(Cmd::Next);
        } else {
            self.start();
        }
    }

    /// Startet ab dem Cue-Titel.
    pub fn start(&mut self) {
        // Ein Voice-Track beginnt nie eine Sendung: ab dem ersten Titel dahinter.
        let from = self
            .show
            .first_music_from(self.cue_index())
            .unwrap_or(usize::MAX);
        if from >= self.show.playlist.len() {
            self.message = Some("Playlist ist leer".into());
            return;
        }
        if self.paused.take().is_some() {
            // Der Mixer hält den pausierten Titel noch: verwerfen, dann neu starten.
            self.send(Cmd::Stop);
        }
        self.send(Cmd::ClearNext);
        self.pending = None;
        self.failed.clear();
        self.played.clear();
        self.running = true;
        if self.queue_index(from) {
            if self.shuffle {
                let uid = self.show.playlist[from].uid;
                self.played.push(uid);
            }
            self.send(Cmd::Play);
        } else {
            self.running = false;
        }
    }

    /// Blendet die Musik aus und hält an. „Start“ beginnt danach mit dem Titel, der lief.
    pub fn stop(&mut self) {
        self.paused = None;
        self.resume_at = None;
        if let Some(i) = self.current_index() {
            self.set_cue(i);
        }
        self.running = false;
        self.pending = None;
        self.send(Cmd::Stop);
    }

    /// Pause: blendet aus und hält den Titel an der Stelle fest (der Mixer behält ihn samt
    /// dem bereitgelegten nächsten). [`Self::play`] macht dort ohne Neuladen weiter.
    pub fn pause(&mut self) {
        let cur = self.status.current.load(Ordering::Acquire);
        if !self.running || cur == NONE {
            return;
        }
        let pos = self.status.position.load(Ordering::Acquire);
        let end = self.status.end.load(Ordering::Acquire);
        self.resume_at = None;
        if let Some(i) = self.current_index() {
            self.set_cue(i);
        }
        self.running = false;
        self.send(Cmd::Pause);
        self.paused = Some((cur, pos, end));
    }

    /// Pausiert (Stelle gemerkt)?
    pub fn is_paused(&self) -> bool {
        self.paused.is_some()
    }

    /// Restzeit des pausierten Titels (Sekunden) und seine Länge bis Cue-Out, wenn pausiert.
    pub fn paused_times(&self) -> Option<(f64, f64)> {
        let (_, pos, end) = self.paused?;
        let r = f64::from(self.rate);
        Some((end.saturating_sub(pos) as f64 / r, end as f64 / r))
    }

    /// Play: nach einer Pause an der gemerkten Stelle weiter, sonst starten (wie
    /// [`Self::start_or_next`]).
    pub fn play(&mut self) {
        let Some((uid, pos, _)) = self.paused else {
            self.start_or_next();
            return;
        };
        let Some(i) = self.index_of(uid) else {
            // Der Titel ist nicht mehr in der Playlist: normal starten.
            self.start();
            return;
        };
        if self.status.current.load(Ordering::Acquire) == uid {
            // Der Mixer hält den Titel (oder blendet ihn noch aus): dort weiter, ohne Neuladen.
            self.paused = None;
            self.running = true;
            self.send(Cmd::Resume);
            return;
        }
        let plays = self.status.plays.load(Ordering::Acquire);
        self.set_cue(i);
        self.start();
        if self.running {
            self.resume_at = Some((uid, pos, plays));
        }
    }

    /// Nach dem Beginn des pausierten Titels an die gemerkte Stelle springen.
    fn resume_tick(&mut self) {
        let Some((uid, pos, plays)) = self.resume_at else {
            return;
        };
        if self.status.current.load(Ordering::Acquire) != uid
            || self.status.plays.load(Ordering::Acquire) == plays
        {
            return;
        }
        self.resume_at = None;
        let now = self.status.position.load(Ordering::Acquire);
        self.send(Cmd::Seek(pos as i64 - now as i64));
    }

    /// Legt Titel `index` beim Mixer bereit. `false` bei Fehler (Titel wird übersprungen
    /// gemeldet).
    fn queue_index(&mut self, index: usize) -> bool {
        let Some(e) = self.show.playlist.get(index) else {
            return false;
        };
        let (uid, path) = (e.uid, e.path.clone());
        let event = self.event_runs.get(&uid).cloned();
        let t0 = std::time::Instant::now();
        let loaded: Result<Shared<Stereo>, String> =
            match event.as_ref().and_then(|r| self.event_files.get(&r.id)) {
                // Zeit-Event: schon geladen und gemessen.
                Some(buf) => Ok(buf.clone()),
                None => load_stereo(&path, self.rate).map(|s| {
                    // Nur messen, was noch keine Messung hat: das läuft im Control-Thread
                    // direkt vor dem Abspielen (#120).
                    if let Some(e) = self.show.playlist.get_mut(index)
                        && !e.is_measured()
                    {
                        e.set_measured(measure(&s, self.rate));
                    }
                    Shared::new(&self.collector.handle(), s)
                }),
            };
        let took = t0.elapsed();
        tracing::debug!(file = %path.display(), ms = took.as_millis() as u64, "track decoded");
        self.last_load = Some((path.clone(), took));
        match loaded {
            Ok(buf) => {
                let gain = self
                    .show
                    .playlist
                    .get(index)
                    .map_or(0.0, |e| e.gain_db(self.show.target_lufs));
                let cues = self
                    .show
                    .playlist
                    .get(index)
                    .map(|e| e.cues)
                    .unwrap_or_default()
                    .frames(self.rate, buf.frames());
                // Übergang vom laufenden Titel (Mixxx-Logik aus Intro/Outro, `transition.rs`).
                let lead = self
                    .show
                    .prev_music(index)
                    .filter(|&p| self.current_index() == Some(p))
                    .and_then(|p| {
                        let (a, b) = (self.show.playlist.get(p)?, self.show.playlist.get(index)?);
                        (a.seconds.is_some() && b.seconds.is_some())
                            .then(|| self.show.crossfade_entries(a, b))
                    })
                    .map(|s| (s * f64::from(self.rate)) as usize);
                self.send(Cmd::Queue {
                    uid,
                    buf,
                    gain: db_to_gain(gain),
                    cues,
                    lead,
                    hold: event.is_some_and(|r| r.hard),
                });
                self.pending = Some(uid);
                self.pending_plays = self.status.plays.load(Ordering::Acquire);
                self.queue_voice(index, uid);
                true
            }
            Err(err) => {
                tracing::warn!(error = %err, "track load failed");
                self.message = Some(err);
                false
            }
        }
    }

    /// Nach einer Änderung der Playlist: den bereitgelegten Titel neu bestimmen.
    fn requeue(&mut self) {
        if self.pending.is_some() && self.running {
            self.send(Cmd::ClearNext);
            self.pending = None;
        }
    }

    // ---- Decks A, B, C ---------------------------------------------------------------

    /// Lädt den Titel von Deck `deck` aus `show.decks` und legt ihn in den Mixer.
    fn load_deck(&mut self, deck: usize) {
        let Some(Some(e)) = self.show.decks.get(deck) else {
            self.decks[deck] = None;
            self.send(Cmd::DeckEject(deck));
            return;
        };
        let (path, cues) = (e.path.clone(), e.cues);
        match load_stereo(&path, self.rate) {
            Ok(s) => {
                let buf = Shared::new(&self.collector.handle(), s);
                self.decks[deck] = Some(buf.clone());
                let f = cues.frames(self.rate, buf.frames());
                self.send(Cmd::DeckLoad {
                    deck,
                    buf,
                    gain: 1.0,
                    start: f.start,
                    end: f.end,
                });
            }
            Err(err) => {
                tracing::warn!(error = %err, "deck load failed");
                self.show.decks[deck] = None;
                self.decks[deck] = None;
                self.send(Cmd::DeckEject(deck));
                self.message = Some(err);
            }
        }
    }

    /// Legt eine Datei auf Deck `deck` (steht danach am Cue-In). Ein laufendes Deck wird
    /// ersetzt.
    pub fn deck_load(&mut self, deck: usize, path: PathBuf, cues: Cues) -> Result<(), String> {
        if deck >= crate::DECKS {
            return Err(format!("Deck {} gibt es nicht", deck + 1));
        }
        let mut entry = CartEntry::new(path);
        entry.cues = Cues {
            cue_in: cues.cue_in,
            cue_out: cues.cue_out,
            ..Cues::default()
        };
        self.show.decks[deck] = Some(entry);
        self.load_deck(deck);
        if self.show.decks[deck].is_none() {
            return Err(self
                .message
                .take()
                .unwrap_or_else(|| "Deck: Laden fehlgeschlagen".into()));
        }
        Ok(())
    }

    /// Legt Playlist-Titel `index` auf Deck `deck` (mit seinen Cue-Punkten).
    pub fn deck_load_title(&mut self, deck: usize, index: usize) -> Result<(), String> {
        let (path, cues) = self
            .show
            .playlist
            .get(index)
            .map(|e| (e.path.clone(), e.cues))
            .ok_or_else(|| format!("Titel {} gibt es nicht", index + 1))?;
        self.deck_load(deck, path, cues)
    }

    /// Deck starten.
    pub fn deck_play(&mut self, deck: usize) {
        if self.decks.get(deck).is_some_and(Option::is_some) {
            self.send(Cmd::DeckPlay(deck));
        }
    }

    /// Deck anhalten (die Stelle bleibt).
    pub fn deck_stop(&mut self, deck: usize) {
        self.send(Cmd::DeckStop(deck));
    }

    /// Deck anhalten und an den Anfang stellen.
    pub fn deck_cue(&mut self, deck: usize) {
        self.send(Cmd::DeckCue(deck));
    }

    /// Deck leeren.
    pub fn deck_eject(&mut self, deck: usize) {
        if let Some(e) = self.show.decks.get_mut(deck) {
            *e = None;
        }
        if let Some(d) = self.decks.get_mut(deck) {
            *d = None;
        }
        self.send(Cmd::DeckEject(deck));
    }

    /// Setzt Cue-In und Cue-Out von Deck `deck`; gilt gleich (ein stehendes Deck springt an
    /// den neuen Anfang).
    pub fn set_deck_cues(&mut self, deck: usize, cues: Cues) -> Result<(), String> {
        let secs = self
            .decks
            .get(deck)
            .and_then(Option::as_ref)
            .map(|b| b.frames() as f64 / f64::from(self.rate));
        cues.validate(secs)?;
        self.show.set_deck_cues(deck, cues)?;
        let (rate, frames) = (
            self.rate,
            self.decks[deck].as_ref().map_or(0, |b| b.frames()),
        );
        let f = self.show.decks[deck]
            .as_ref()
            .map(|e| e.cues.frames(rate, frames))
            .unwrap_or_else(|| Cues::default().frames(rate, frames));
        self.send(Cmd::DeckCues {
            deck,
            start: f.start,
            end: f.end,
        });
        Ok(())
    }

    /// Hört Deck `deck` ab `from` Sekunden vor.
    pub fn pfl_deck_at(&mut self, deck: usize, from: f64) -> Result<(), String> {
        let path = match self.show.decks.get(deck) {
            Some(Some(c)) => c.path.clone(),
            _ => return Err(format!("Deck {} ist leer", crate::show::deck_name(deck))),
        };
        self.pfl_play(&path, from)
    }

    /// Hört Deck `deck` vor, ab seinem Cue-In.
    pub fn pfl_deck(&mut self, deck: usize) -> Result<(), String> {
        let from = self
            .show
            .decks
            .get(deck)
            .and_then(Option::as_ref)
            .map_or(0.0, |c| c.cues.start());
        self.pfl_deck_at(deck, from)
    }

    // ---- Carts -----------------------------------------------------------------------

    fn load_cart(&mut self, slot: usize) {
        let Some(Some(c)) = self.show.carts.get(slot) else {
            return;
        };
        let path = c.path.clone();
        match load_stereo(&path, self.rate) {
            Ok(s) => self.carts[slot] = Some(Shared::new(&self.collector.handle(), s)),
            Err(e) => {
                tracing::warn!(error = %e, "cart load failed");
                self.message = Some(e);
            }
        }
    }

    /// Belegt eine Cart-Taste (`None` = frei).
    pub fn assign_cart(&mut self, slot: usize, path: Option<PathBuf>) -> Result<(), String> {
        if slot >= CARTS {
            return Err(format!("Cart {slot} gibt es nicht"));
        }
        match path {
            None => {
                self.show.carts[slot] = None;
                self.carts[slot] = None;
                Ok(())
            }
            Some(p) => {
                let s = load_stereo(&p, self.rate)?;
                self.carts[slot] = Some(Shared::new(&self.collector.handle(), s));
                self.show.carts[slot] = Some(CartEntry::new(p));
                Ok(())
            }
        }
    }

    /// Spielt Cart `slot` (überlagert die Musik); läuft er schon, hält er an.
    pub fn trigger_cart(&mut self, slot: usize) {
        if self.status.carts.load(Ordering::Acquire) & (1 << slot) != 0 {
            self.send(Cmd::CartStop(slot));
            return;
        }
        if let Some(Some(buf)) = self.carts.get(slot) {
            let buf = buf.clone();
            let cues = self
                .show
                .carts
                .get(slot)
                .and_then(Option::as_ref)
                .map(|c| c.cues)
                .unwrap_or_default();
            let f = cues.frames(self.rate, buf.frames());
            self.send(Cmd::Cart {
                slot,
                buf,
                gain: 1.0,
                start: f.start,
                end: f.end,
            });
        }
    }

    /// Setzt Start und Ende von Cart `slot`; gilt ab dem nächsten Start des Carts.
    pub fn set_cart_cues(&mut self, slot: usize, cues: Cues) -> Result<(), String> {
        let secs = self
            .carts
            .get(slot)
            .and_then(Option::as_ref)
            .map(|b| b.frames() as f64 / f64::from(self.rate));
        cues.validate(secs)?;
        self.show.set_cart_cues(slot, cues)
    }

    /// Legt die Taste von Cart `slot` fest (`None` = keine).
    pub fn set_cart_key(&mut self, slot: usize, key: Option<&str>) -> Result<(), String> {
        self.show.set_cart_key(slot, key)
    }

    // ---- Mikro -----------------------------------------------------------------------

    /// Hängt einen Mikro-Ring ein (Stereo verschachtelt in Mixer-Rate) oder aus.
    pub fn set_mic_input(&mut self, consumer: Option<rtrb::Consumer<f32>>) {
        let owned = consumer.map(|c| Owned::new(&self.collector.handle(), c));
        self.send(Cmd::MicInput(owned));
    }

    pub fn set_mic_open(&mut self, open: bool) {
        self.send(Cmd::MicOpen(open));
    }

    /// Hängt den Telefon-Eingang (Anrufer) ein oder aus. Er kommt über eine Taktverbindung
    /// ([`woa_engine::pairs::link`]) in der Rate des Mixers an.
    pub fn set_phone_input(&mut self, rx: Option<woa_engine::pairs::LinkRx>) {
        let owned = rx.map(|c| Owned::new(&self.collector.handle(), c));
        self.send(Cmd::PhoneInput(owned));
    }

    /// Telefon auf Sendung: Der Anrufer liegt auf der Sendung, die Musik wird abgesenkt.
    pub fn set_phone_on(&mut self, on: bool) {
        self.send(Cmd::PhoneOn(on));
    }

    pub fn set_mic_monitor(&mut self, on: bool) {
        self.send(Cmd::MicMonitor(on));
    }

    /// Ziele der Quellen (Ausgänge je Gerät, [`crate::routing::Plan`]).
    pub fn set_routes(&mut self, routes: crate::routing::Routes) {
        self.send(Cmd::SetRoutes(routes));
    }

    // ---- Gäste (Paket 37) ------------------------------------------------------------

    /// Gast-Kanäle: anlegen, Fader, Rückweg im Kopfhörer.
    pub fn guests(&mut self) -> &mut woa_guests::GuestHub {
        &mut self.guests
    }

    /// Anzeige der Gäste (holt die Pegelspitzen ab).
    pub fn guests_view(&self) -> Vec<woa_guests::GuestView> {
        self.guests.view()
    }

    /// Schaltet den Zugang für Browser-Gäste ein oder aus (Paket 38). `ip` ist die LAN-Adresse
    /// dieses Rechners; ohne sie gibt es keinen Link, den ein Handy erreicht.
    pub fn guests_web_set(&mut self, on: bool, ip: Option<std::net::IpAddr>) {
        if !on {
            self.guests.web_stop();
            return;
        }
        let Some(ip) = ip else {
            self.message = Some("Browser-Gäste: kein Netzwerk gefunden (WLAN verbunden?)".into());
            return;
        };
        let cfg = woa_guests::WebConfig {
            ip,
            udp_port: woa_guests::web::PREFERRED_UDP_PORT,
            https_port: woa_guests::web::PREFERRED_HTTPS_PORT,
            engine_rate: self.rate,
        };
        if let Err(e) = self.guests.web_start(&cfg) {
            tracing::warn!(error = %e, "guest web start failed");
            self.message = Some(format!("Browser-Gäste: {e}"));
        }
    }

    /// Einladungen und Warteraum der Browser-Gäste.
    pub fn guests_web_view(&self) -> woa_guests::WebView {
        self.guests.web_view()
    }

    // ---- Sendung (woa-streamd) -------------------------------------------------------

    /// Hängt den Sende-Ring ein (Sendeausgang Stereo verschachtelt in Mixer-Rate) oder aus.
    /// Den Ring liest der Feeder aus `woa-stream` in einem eigenen Thread.
    pub fn set_stream_output(&mut self, producer: Option<rtrb::Producer<f32>>) {
        let owned = producer.map(|p| Owned::new(&self.collector.handle(), p));
        self.send(Cmd::Stream(owned));
    }

    // ---- Mitschnitt ------------------------------------------------------------------

    /// Startet den Mitschnitt nach `<data_dir>/mitschnitte/`.
    pub fn start_recording(&mut self, data_dir: &Path) {
        if self.recording.is_some() {
            return;
        }
        let cap = self.rate as usize * 2 * RECORD_QUEUE_SECONDS;
        let (producer, consumer) = rtrb::RingBuffer::new(cap);
        self.recording = Some(Recording {
            dir: data_dir.join(RECORDINGS_DIR),
            consumer,
            writer: None,
            frames_in_file: 0,
            rate: self.rate,
            files: Vec::new(),
        });
        let owned = Owned::new(&self.collector.handle(), producer);
        self.send(Cmd::Record(Some(owned)));
    }

    /// Beendet den Mitschnitt; liefert die geschriebenen Dateien.
    pub fn stop_recording(&mut self) -> Vec<PathBuf> {
        self.send(Cmd::Record(None));
        let Some(mut r) = self.recording.take() else {
            return Vec::new();
        };
        if let Err(e) = r.drain().and_then(|()| r.close_file()) {
            tracing::warn!(error = %e, "mitschnitt failed");
            self.message = Some(e);
        }
        r.files
    }

    /// Datei, in die gerade mitgeschnitten wird.
    pub fn recording_file(&self) -> Option<&Path> {
        self.recording
            .as_ref()
            .and_then(|r| r.writer.as_ref().map(|(p, _)| p.as_path()))
    }

    pub fn is_recording(&self) -> bool {
        self.recording.is_some()
    }

    // ---- Takt des Control-Threads ----------------------------------------------------

    /// Regelmäßig aufrufen (alle paar Millisekunden): nächsten Titel bereitlegen,
    /// Mitschnitt schreiben, alte Puffer freigeben.
    pub fn pump(&mut self) {
        self.pump_at(wall_now());
    }

    /// Wie [`Self::pump`] mit vorgegebener Uhrzeit (Unix-Sekunden). Die Tests treiben so die
    /// Zeit-Events mit einer simulierten Uhr.
    pub fn pump_at(&mut self, now: f64) {
        self.sync_voices();
        self.pump_queue();
        self.resume_tick();
        self.event_tick(now);
    }

    fn pump_queue(&mut self) {
        self.collector.collect();
        if let Some(m) = self.guests.poll() {
            self.message = Some(m);
        }
        if let Some(r) = self.recording.as_mut()
            && let Err(e) = r.drain()
        {
            tracing::warn!(error = %e, "mitschnitt failed, stopped");
            self.message = Some(format!("Mitschnitt angehalten: {e}"));
            self.stop_recording();
        }
        if !self.running {
            return;
        }
        let cur = self.status.current.load(Ordering::Acquire);
        if let Some(p) = self.pending {
            if cur != p {
                // Noch bereitgelegt oder unterwegs: warten.
                return;
            }
            if self.status.plays.load(Ordering::Acquire) == self.pending_plays {
                // Derselbe Titel läuft noch (Wiederholen): der neue hat noch nicht begonnen.
                return;
            }
            // Mixer hat ihn begonnen.
            self.pending = None;
        }
        // Nichts bereit: den Titel nach dem laufenden bereitlegen.
        let after = match self.index_of(cur) {
            Some(i) => i + 1,
            None if cur == NONE => {
                if self.status.playing.load(Ordering::Acquire) {
                    // Musik zu Ende, nichts mehr bereit: Sendung endet.
                    self.running = false;
                    self.cue = None;
                    self.send(Cmd::Stop);
                }
                return;
            }
            // Laufender Titel wurde aus der Playlist entfernt: mit dem Cue weiter.
            None => self.cue_index(),
        };
        let cur_idx = self.index_of(cur);
        for i in self.candidates(cur_idx, after, true) {
            let uid = self.show.playlist[i].uid;
            if self.failed.contains(&uid) {
                continue;
            }
            if self.queue_index(i) {
                if self.shuffle {
                    self.played.push(uid);
                }
                break;
            }
            // Kaputte Datei: überspringen (Meldung steht in `message`).
            self.failed.push(uid);
        }
    }

    /// Restzeit des laufenden Titels (Sekunden).
    pub fn remaining_title(&self) -> f64 {
        if self.status.current.load(Ordering::Acquire) == NONE {
            return 0.0;
        }
        let end = self.status.end.load(Ordering::Acquire);
        let pos = self.status.position.load(Ordering::Acquire);
        end.saturating_sub(pos) as f64 / f64::from(self.rate)
    }

    /// Restzeit bis zum Ende der Playlist (Sekunden): Rest des laufenden Titels plus die
    /// folgenden, abzüglich Überblendungen.
    pub fn remaining_total(&self) -> f64 {
        match self.current_index() {
            Some(i) => {
                let rest = self.remaining_title();
                let after = self.show.total_seconds_from(i + 1);
                let next = self
                    .show
                    .next_music(i)
                    .and_then(|k| self.show.playlist.get(k));
                let fade = match (self.show.playlist.get(i), next) {
                    (Some(a), Some(b)) => self.show.crossfade_entries(a, b).min(rest),
                    _ => 0.0,
                };
                rest + after - fade
            }
            None => self.show.total_seconds_from(self.cue_index()),
        }
    }

    /// Sekunden bis zum Ramp-Punkt (Intro-Ende) des laufenden Titels; `None`, wenn keiner
    /// gesetzt oder schon vorbei.
    pub fn ramp_remaining(&self) -> Option<f64> {
        let i = self.current_index()?;
        let ramp = self.show.playlist.get(i)?.cues.ramp?;
        let pos = self.status.position.load(Ordering::Acquire) as f64 / f64::from(self.rate);
        let left = ramp - pos;
        (left > 0.0).then_some(left)
    }

    // ---- Cue-Punkte (Paket 15) -------------------------------------------------------

    /// Setzt die Cue-Punkte von Titel `index` als Undo-Schritt. Ein schon bereitgelegter
    /// Titel wird mit den neuen Punkten neu bereitgelegt; der laufende behält seine.
    pub fn set_cues(&mut self, index: usize, cues: Cues) -> Result<(), String> {
        let before = self.show.set_cues(index, cues)?;
        if before == cues {
            return Ok(());
        }
        let uid = self.show.playlist[index].uid;
        tracing::info!(index, ?cues, "cues set");
        self.cue_history.push(CueEdit {
            uid,
            before,
            after: cues,
        });
        self.cues_changed(uid);
        Ok(())
    }

    fn cues_changed(&mut self, uid: u32) {
        // Der bereitgelegte Titel selbst oder sein Vorgänger (Übergang hängt an beiden).
        let prev_of_pending = self
            .pending
            .and_then(|p| self.index_of(p))
            .and_then(|i| i.checked_sub(1))
            .and_then(|i| self.show.playlist.get(i))
            .is_some_and(|e| e.uid == uid);
        if self.pending == Some(uid) || prev_of_pending {
            self.requeue();
        }
    }

    fn apply_cue_edit(&mut self, uid: u32, cues: Cues) -> bool {
        let Some(i) = self.index_of(uid) else {
            return false;
        };
        if self.show.set_cues(i, cues).is_err() {
            return false;
        }
        self.cues_changed(uid);
        true
    }

    /// Nimmt die letzte Cue-Bearbeitung zurück.
    pub fn undo_cues(&mut self) -> bool {
        match self.cue_history.take_undo() {
            Some(e) => self.apply_cue_edit(e.uid, e.before),
            None => false,
        }
    }

    /// Stellt die zurückgenommene Cue-Bearbeitung wieder her.
    pub fn redo_cues(&mut self) -> bool {
        match self.cue_history.take_redo() {
            Some(e) => self.apply_cue_edit(e.uid, e.after),
            None => false,
        }
    }

    pub fn can_undo_cues(&self) -> bool {
        self.cue_history.can_undo()
    }

    pub fn can_redo_cues(&self) -> bool {
        self.cue_history.can_redo()
    }

    // ---- Vorhören (PFL) --------------------------------------------------------------

    /// Hängt einen Kopfhörer-Ausgang ein oder aus.
    pub fn set_pfl(&mut self, pfl: Option<PflHandle>) {
        self.pfl = pfl;
    }

    pub fn has_pfl(&self) -> bool {
        self.pfl.is_some()
    }

    pub fn pfl_playing(&self) -> bool {
        self.pfl.as_ref().is_some_and(PflHandle::is_playing)
    }

    fn pfl_play(&mut self, path: &Path, from: f64) -> Result<(), String> {
        let Some(rate) = self.pfl.as_ref().map(|h| h.rate) else {
            return Err("Kein Kopfhörer-Ausgang für das Vorhören eingestellt".into());
        };
        let s = load_stereo(path, rate)?;
        let start = ((from.max(0.0) * f64::from(rate)) as usize).min(s.frames());
        let buf = Shared::new(&self.collector.handle(), s);
        if let Some(h) = self.pfl.as_mut()
            && h.cmds.push(PflCmd::Play { buf, start }).is_err()
        {
            return Err("Vorhören: Warteschlange voll".into());
        }
        tracing::info!(file = %path.display(), from, "pfl play");
        Ok(())
    }

    /// Hört Titel `index` vor, ab Hook (falls `hook`), sonst ab Cue-In.
    pub fn pfl_title(&mut self, index: usize, hook: bool) -> Result<(), String> {
        let e = self
            .show
            .playlist
            .get(index)
            .ok_or_else(|| format!("Titel {} gibt es nicht", index + 1))?;
        let from = if hook {
            e.cues.hook.unwrap_or(e.cues.start())
        } else {
            e.cues.start()
        };
        let path = e.path.clone();
        self.pfl_play(&path, from)
    }

    /// Hört Titel `index` ab Sekunde `from` vor (Cue-Editor).
    pub fn pfl_title_at(&mut self, index: usize, from: f64) -> Result<(), String> {
        let path = self
            .show
            .playlist
            .get(index)
            .map(|e| e.path.clone())
            .ok_or_else(|| format!("Titel {} gibt es nicht", index + 1))?;
        self.pfl_play(&path, from)
    }

    /// Hört eine Datei (z. B. aus der Bibliothek) im Kopfhörer vor, von Anfang an.
    pub fn pfl_file(&mut self, path: &Path) -> Result<(), String> {
        self.pfl_play(path, 0.0)
    }

    /// Hört Cart `slot` ab `from` Sekunden vor (Cart-Schnitt).
    pub fn pfl_cart_at(&mut self, slot: usize, from: f64) -> Result<(), String> {
        let path = match self.show.carts.get(slot) {
            Some(Some(c)) => c.path.clone(),
            _ => return Err(format!("Cart {} ist frei", slot + 1)),
        };
        self.pfl_play(&path, from)
    }

    /// Hört Cart `slot` vor, ab seinem Cue-In.
    pub fn pfl_cart(&mut self, slot: usize) -> Result<(), String> {
        let path = match self.show.carts.get(slot) {
            Some(Some(c)) => c.path.clone(),
            _ => return Err(format!("Cart {} ist frei", slot + 1)),
        };
        let from = self.show.carts[slot]
            .as_ref()
            .map_or(0.0, |c| c.cues.start());
        self.pfl_play(&path, from)
    }

    /// Hält das Vorhören an.
    pub fn pfl_stop(&mut self) {
        if let Some(h) = self.pfl.as_mut()
            && h.cmds.push(PflCmd::Stop).is_err()
        {
            tracing::warn!("pfl queue full");
        }
    }

    /// Ist die Sendung durch (für Offline-Render)?
    pub fn finished(&self) -> bool {
        !self.running
            && self.pending.is_none()
            && self.status.current.load(Ordering::Acquire) == NONE
    }
}

/// Rendert die Playlist von `show` offline mit Crossfades und Auto-Gain (dieselben
/// Klassen wie live). Gibt Stereo bis zum Ende des letzten Titels zurück.
#[tracing::instrument(level = "info", skip_all, name = "show_render")]
pub fn render(show: &RadioShow, rate: u32) -> Result<Stereo, String> {
    const BLOCK: usize = 512;
    if show.playlist.is_empty() {
        return Err("Playlist ist leer".into());
    }
    let (mut c, mut mixer) = Controller::new(show.clone(), rate);
    c.measure_missing();
    let total: f64 = c.show().total_seconds_from(0);
    let limit = ((total + 10.0) * f64::from(rate)) as usize;
    c.start();
    if let Some(m) = c.take_message() {
        return Err(m);
    }
    let mut out = Stereo::default();
    let mut l = vec![0.0f32; BLOCK];
    let mut r = vec![0.0f32; BLOCK];
    loop {
        c.pump();
        mixer.process(&mut l, &mut r);
        out.left.extend_from_slice(&l);
        out.right.extend_from_slice(&r);
        c.pump();
        if c.finished() && !c.status().playing.load(Ordering::Acquire) {
            break;
        }
        if out.left.len() > limit {
            return Err("Render läuft über die erwartete Länge".into());
        }
    }
    let end = c.status().music_end.load(Ordering::Acquire) as usize;
    out.left.truncate(end);
    out.right.truncate(end);
    if let Some(m) = c.take_message() {
        tracing::warn!(message = %m, "radio render");
    }
    Ok(out)
}

/// Aktuelle Unix-Zeit in Sekunden.
pub fn wall_now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64())
}

// ---- Feste Zeit-Events (Paket 29) ----------------------------------------------------

impl Controller {
    /// Setzt die Event-Tabelle (gleiche Tabelle: nichts ändert sich).
    pub fn set_events(&mut self, table: EventTable) {
        if *self.events.table() != table {
            tracing::info!(events = table.events.len(), "event table set");
            self.events.set_table(table);
        }
    }

    pub fn events(&self) -> &EventTable {
        self.events.table()
    }

    /// Nächstes Event: Name, Sekunden bis dahin, hart.
    pub fn next_event(&self, now: f64) -> Option<(String, f64, bool)> {
        let u = self.events.upcoming()?;
        let name = self.events.table().get(u.id)?.name.clone();
        Some((name, u.at as f64 - now, u.hard))
    }

    /// Letztes gespieltes Event mit Abweichung (Sekunden, positiv = zu spät).
    pub fn last_event(&self) -> Option<&(String, f64)> {
        self.last_event.as_ref()
    }

    pub fn events_missed(&self) -> u32 {
        self.events_missed
    }

    fn event_uid(&self, id: u32, at: i64) -> Option<u32> {
        self.event_runs
            .iter()
            .find(|(_, r)| r.id == id && r.at == at)
            .map(|(u, _)| *u)
    }

    /// Fügt das Event als Playlist-Eintrag bei `at_index` ein.
    fn insert_event(&mut self, id: u32, at: i64, hard: bool, at_index: usize) -> Option<u32> {
        let ev = self.events.table().get(id)?.clone();
        let buf = self.event_files.get(&id)?.clone();
        let mut e = PlaylistEntry::new(ev.path.clone());
        e.uid = self.next_uid;
        self.next_uid += 1;
        let m = measure(&buf, self.rate);
        e.set_measured(m);
        let uid = e.uid;
        let at_index = at_index.min(self.show.playlist.len());
        self.show.playlist.insert(at_index, e);
        self.event_runs.insert(
            uid,
            EventRun {
                id,
                name: ev.name,
                at,
                hard,
                started: false,
            },
        );
        Some(uid)
    }

    /// Stelle in der Playlist, vor die ein hartes Event um `at` gehört: vor den ersten Titel,
    /// der erst nach `at - 1 s` beginnen würde. So läuft davor ein Titel bis zur Zeit und
    /// wird abgeblendet, statt dass das Event zu früh kommt.
    fn hard_index(&self, now: f64, at: i64) -> usize {
        let Some(cur) = self.current_index() else {
            return self.cue_index();
        };
        let target = at as f64 - 1.0 - now;
        let mut end = self.remaining_title();
        let mut k = cur + 1;
        while let Some(b) = self.show.playlist.get(k) {
            if b.voice.is_some() {
                // Voice-Tracks haben keine eigene Zeit.
                k += 1;
                continue;
            }
            let Some(a) = self
                .show
                .prev_music(k)
                .and_then(|p| self.show.playlist.get(p))
            else {
                break;
            };
            let start = end - self.show.crossfade_entries(a, b);
            if start >= target {
                // Nicht zwischen einen Voice-Track und seinen Titel schneiden.
                return if k > 0 && self.show.playlist[k - 1].voice.is_some() {
                    k - 1
                } else {
                    k
                };
            }
            end = start + b.play_seconds().unwrap_or(UNMEASURED_SECONDS);
            k += 1;
        }
        k.min(self.show.playlist.len())
    }

    fn event_tick(&mut self, now: f64) {
        let cur = self.status.current.load(Ordering::Acquire);
        let on_air = self.running && cur != NONE;
        // Gestartete Events: Abweichung festhalten, Erledigtes vergessen.
        let started: Vec<u32> = self
            .event_runs
            .iter()
            .filter(|(u, r)| !r.started && **u == cur)
            .map(|(u, _)| *u)
            .collect();
        for u in started {
            if let Some(r) = self.event_runs.get_mut(&u) {
                r.started = true;
                let dev = now - r.at as f64;
                tracing::info!(event = %r.name, hard = r.hard, deviation_s = dev, "time event started");
                self.last_event = Some((r.name.clone(), dev));
            }
        }
        self.event_runs.retain(|u, r| !r.started || *u == cur);
        let mut keep: Vec<u32> = self.event_runs.values().map(|r| r.id).collect();
        keep.extend(self.events.waiting_ids());
        self.event_files.retain(|id, _| keep.contains(id));

        for a in self.events.tick(now, on_air) {
            match a {
                Action::Predecode { id, at, hard } => self.event_predecode(id, at, hard, now),
                Action::Fire {
                    id,
                    at,
                    hard,
                    in_secs,
                } => self.event_fire(id, at, hard, in_secs),
                Action::Missed { id, at, why } => {
                    let name = self
                        .events
                        .table()
                        .get(id)
                        .map_or_else(|| id.to_string(), |e| e.name.clone());
                    self.events_missed += 1;
                    let why = match why {
                        MissedWhy::NotOnAir => "Sendung lief nicht".to_string(),
                        MissedWhy::TooLate(s) => format!("{s:.0} s zu spät erkannt"),
                    };
                    tracing::warn!(event = %name, at, %why, "time event missed");
                    self.message = Some(format!("Event „{name}“ verpasst: {why}"));
                }
            }
        }
    }

    fn event_predecode(&mut self, id: u32, at: i64, hard: bool, now: f64) {
        let Some(ev) = self.events.table().get(id).cloned() else {
            return;
        };
        if !self.event_files.contains_key(&id) {
            match load_stereo(&ev.path, self.rate) {
                Ok(s) => {
                    let sh = Shared::new(&self.collector.handle(), s);
                    self.event_files.insert(id, sh);
                }
                Err(err) => {
                    tracing::warn!(event = %ev.name, error = %err, "event file load failed");
                    self.message = Some(format!("Event „{}“: {err}", ev.name));
                    self.events_missed += 1;
                    return;
                }
            }
        }
        if hard && self.event_uid(id, at).is_none() {
            let idx = self.hard_index(now, at);
            self.insert_event(id, at, true, idx);
            if self
                .pending
                .and_then(|p| self.index_of(p))
                .is_some_and(|i| i >= idx)
            {
                self.requeue();
            }
            tracing::info!(event = %ev.name, index = idx, "hard event planned");
        }
    }

    fn event_fire(&mut self, id: u32, at: i64, hard: bool, in_secs: f64) {
        if !self.event_files.contains_key(&id) {
            // Datei ließ sich nicht laden (schon gemeldet).
            return;
        }
        let Some(cur) = self.current_index() else {
            return;
        };
        let uid = match self.event_uid(id, at) {
            Some(u) => u,
            None => match self.insert_event(id, at, hard, cur + 1) {
                Some(u) => u,
                None => return,
            },
        };
        // Das Event muss das nächste sein: gleich hinter den laufenden Titel.
        if let Some(i) = self.index_of(uid)
            && i != cur + 1
        {
            let e = self.show.playlist.remove(i);
            let to = (cur + 1).min(self.show.playlist.len());
            self.show.playlist.insert(to, e);
        }
        if self.pending != Some(uid) {
            self.send(Cmd::ClearNext);
            self.pending = None;
            self.pump_queue();
        }
        if hard {
            let cut = (self.remaining_title() - in_secs).max(0.0);
            let frame =
                self.status.clock.load(Ordering::Acquire) + (in_secs * f64::from(self.rate)) as u64;
            self.send(Cmd::StartAt(frame));
            tracing::info!(id, in_secs, cut_s = cut, "hard event armed");
        } else {
            tracing::info!(id, "soft event queued after the running title");
        }
    }
}

// ---- Voice-Tracking (Paket 30) --------------------------------------------------------

impl Controller {
    /// Gleicht die Voice-Tracks mit ihren Nachbarn ab; was sich geändert hat, steht auf
    /// „prüfen“ und wird nicht gesendet.
    fn sync_voices(&mut self) {
        let flagged = self.show.relink_voices();
        for i in flagged {
            let name = self
                .show
                .playlist
                .get(i)
                .map(PlaylistEntry::title)
                .unwrap_or_default();
            tracing::warn!(index = i, voice = %name, "voice track neighbour changed, needs check");
            self.message = Some(format!(
                "Voice-Track „{name}“ (Eintrag {}): ein Nachbar hat sich geändert, bitte prüfen",
                i + 1
            ));
            // Einen schon bereitgelegten Titel ohne diesen Voice-Track neu bereitlegen, aber
            // nie den Start selbst (noch kein Titel am Laufen) wegräumen.
            if self.status.current.load(Ordering::Acquire) != NONE {
                self.requeue();
            }
        }
        self.poll_preview();
    }

    /// Legt den Voice-Track vor dem Titel `index` (Kennung `uid`) beim Mixer bereit.
    fn queue_voice(&mut self, index: usize, uid: u32) {
        let Some(vi) = index.checked_sub(1) else {
            return;
        };
        let Some(ve) = self.show.playlist.get(vi).filter(|e| e.voice.is_some()) else {
            return;
        };
        let Some(v) = ve.voice.clone() else { return };
        let (path, name) = (ve.path.clone(), ve.title());
        if v.check {
            tracing::warn!(voice = %name, "voice track skipped: needs check");
            self.message = Some(format!(
                "Voice-Track „{name}“ übersprungen: Nachbar geändert, erst bestätigen"
            ));
            return;
        }
        let stereo = match load_stereo(&path, self.rate) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(error = %e, "voice track load failed");
                self.message = Some(format!("Voice-Track „{name}“: {e}"));
                return;
            }
        };
        if stereo.frames() == 0 {
            self.message = Some(format!("Voice-Track „{name}“ ist leer"));
            return;
        }
        let m = measure(&stereo, self.rate);
        let auto = crate::level::auto_gain_db(m.lufs, m.peak_db, self.show.target_lufs);
        let gain = db_to_gain(auto + v.params.gain_db);
        let frames = v.params.frames(self.rate, stereo.frames());
        let buf = Shared::new(&self.collector.handle(), stereo);
        tracing::info!(
            voice = %name,
            offset_s = v.params.offset,
            duck_db = v.params.duck_db,
            gain_db = auto + v.params.gain_db,
            "voice track queued"
        );
        self.send(Cmd::VoiceOver {
            uid,
            buf,
            gain,
            frames,
        });
    }

    /// Legt eine Sprachdatei als Voice-Track bei `at` in die Playlist (gemessen, an die jetzigen
    /// Nachbarn gebunden, Beginn der Stimme vorgeschlagen). Gibt den Index zurück.
    #[tracing::instrument(level = "info", skip_all, name = "voice_add")]
    pub fn add_voice(&mut self, path: &Path, at: Option<usize>) -> Result<usize, String> {
        let s = load_stereo(path, self.rate)?;
        let m = measure(&s, self.rate);
        if m.seconds < 0.2 {
            return Err(format!("{}: zu kurz für eine Moderation", path.display()));
        }
        if m.seconds > crate::voicetrack::MAX_VOICE_SECONDS {
            return Err(format!(
                "{}: länger als {} s",
                path.display(),
                crate::voicetrack::MAX_VOICE_SECONDS
            ));
        }
        let mut e = PlaylistEntry::new(path);
        e.uid = self.next_uid;
        self.next_uid += 1;
        e.set_measured(m);
        // Die Cue-Vorschläge der Titel-Analyse gelten nicht für eine Moderation.
        e.cues = Cues::default();
        let at = at.unwrap_or(self.show.playlist.len());
        let i = self.show.insert_voice(at, e);
        self.requeue();
        tracing::info!(file = %path.display(), index = i, "voice track added");
        Ok(i)
    }

    /// Ändert die Werte von Voice-Track `index`.
    pub fn set_voice(
        &mut self,
        index: usize,
        params: crate::voicetrack::VoiceParams,
    ) -> Result<(), String> {
        self.show.set_voice_params(index, params)?;
        self.requeue();
        Ok(())
    }

    /// Bestätigt einen Voice-Track auf „prüfen“.
    pub fn confirm_voice(&mut self, index: usize) -> Result<(), String> {
        self.show.confirm_voice(index)?;
        self.requeue();
        Ok(())
    }

    /// Rendert A, Stimme, B im Hintergrund und spielt das Ergebnis im Kopfhörer (PFL), nie auf
    /// Sendung. Das Ergebnis holt [`Self::pump_at`] ab.
    pub fn pfl_voice(&mut self, index: usize) -> Result<(), String> {
        let Some(rate) = self.pfl.as_ref().map(|h| h.rate) else {
            return Err("Kein Kopfhörer-Ausgang für das Vorhören eingestellt".into());
        };
        let show = self.show.voice_preview(index)?;
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::Builder::new()
            .name("woa-voice-preview".into())
            .spawn(move || {
                let _ = tx.send(render(&show, rate));
            })
            .map_err(|e| format!("Vorhören: {e}"))?;
        self.preview = Some(rx);
        Ok(())
    }

    fn poll_preview(&mut self) {
        let Some(rx) = self.preview.as_ref() else {
            return;
        };
        match rx.try_recv() {
            Ok(Ok(s)) => {
                self.preview = None;
                if let Err(e) = self.pfl_stereo(s) {
                    self.report(e);
                }
            }
            Ok(Err(e)) => {
                self.preview = None;
                self.report(format!("Vorhören: {e}"));
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(std::sync::mpsc::TryRecvError::Disconnected) => self.preview = None,
        }
    }

    fn pfl_stereo(&mut self, s: Stereo) -> Result<(), String> {
        let buf = Shared::new(&self.collector.handle(), s);
        let Some(h) = self.pfl.as_mut() else {
            return Err("Kein Kopfhörer-Ausgang für das Vorhören eingestellt".into());
        };
        h.cmds
            .push(PflCmd::Play { buf, start: 0 })
            .map_err(|_| "Vorhören: Warteschlange voll".to_string())
    }
}
