//! Party-Layout (Paket 35): Zustand und Befehle ohne Fenster.
//!
//! Der Party-Modus spielt über denselben Sendemixer wie das Radio (AutoDJ, Crossfade,
//! Lautheitsangleich aus [`crate::control`]). Hier steht nur, was die Oberfläche zeigt und
//! welche Befehle die großen Tasten auslösen, damit beides ohne Display testbar ist:
//!
//! - [`PartyState::apply`] nimmt einen [`PartyCommand`] und liefert die [`Effect`]e für die
//!   Sendung; jeder Befehl landet im Protokoll (auch später für Fernbedienung und MIDI, die
//!   dieselben Befehle schicken sollen).
//! - [`Wishlist`] ordnet Wünsche nach Stimmen, dann nach Eingang, und entfernt Doppelte.
//! - [`layout`] baut aus dem Schnappschuss der Sendung eine [`PartyLayout`]; ihre Textform
//!   ist der Schnappschuss für Tests.
//!
//! Alles läuft im Control-Thread bzw. in der Oberfläche, nie im Audio-Thread.

use std::path::PathBuf;

use crate::control::Repeat;
use crate::mixer::{LEVEL_MAX_DB, LEVEL_MIN_DB};
use crate::station::View;

/// Schritt für Lauter/Leiser (dB).
pub const VOLUME_STEP_DB: f64 = 2.0;
/// Sprung der Spultasten (Sekunden).
pub const SEEK_STEP_SECONDS: f64 = 10.0;
/// So viele Protokollzeilen bleiben stehen.
pub const LOG_CAP: usize = 200;

/// Ein Wunsch der Gäste.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Wish {
    /// Datei des Wunsches; freier Text hat den Platzhalter `wunsch:<text>`.
    pub path: PathBuf,
    pub title: String,
    /// Wie oft er gewünscht wurde.
    pub votes: u32,
    /// Reihenfolge des ersten Eingangs.
    #[serde(default)]
    order: u64,
    /// Erledigt: bleibt stehen, durchgestrichen und abgeblendet.
    #[serde(default)]
    pub done: bool,
    /// Per „→ Playlist“ eingereiht: diese Datei. Läuft sie, ist der Wunsch erledigt.
    #[serde(default)]
    pub queued: Option<PathBuf>,
}

/// Freier Text ohne Datei (Platzhalter-Pfad wie bei der Fernbedienung).
pub fn free_wish_path(text: &str) -> PathBuf {
    PathBuf::from(format!("wunsch:{}", text.trim().to_lowercase()))
}

/// Ist das ein freier Wunsch ohne Datei?
pub fn is_free_wish(path: &std::path::Path) -> bool {
    path.to_string_lossy().starts_with("wunsch:")
}

/// Vergleichsschlüssel: Titel ohne Groß-/Kleinschreibung und doppelte Leerzeichen; fehlt der
/// Titel, der Pfad.
fn key_of(path: &std::path::Path, title: &str) -> String {
    let t: Vec<String> = title.split_whitespace().map(|w| w.to_lowercase()).collect();
    if t.is_empty() {
        path.to_string_lossy().to_lowercase()
    } else {
        t.join(" ")
    }
}

/// Wunschliste: sortiert (Stimmen absteigend, dann der Eingang), ohne Doppelte.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Wishlist {
    items: Vec<Wish>,
    #[serde(default)]
    counter: u64,
}

impl Wishlist {
    /// Nimmt einen Wunsch auf. Gibt es ihn schon, zählt nur seine Stimme. `true`, wenn neu.
    pub fn add(&mut self, path: PathBuf, title: &str) -> bool {
        let key = key_of(&path, title);
        if let Some(w) = self
            .items
            .iter_mut()
            .find(|w| key_of(&w.path, &w.title) == key)
        {
            w.votes = w.votes.saturating_add(1);
            w.done = false;
            self.sort();
            return false;
        }
        self.counter += 1;
        self.items.push(Wish {
            path,
            title: title.trim().to_string(),
            votes: 1,
            order: self.counter,
            done: false,
            queued: None,
        });
        self.sort();
        true
    }

    fn sort(&mut self) {
        // Offene zuerst, dann Stimmen, dann Eingang.
        self.items.sort_by(|a, b| {
            a.done
                .cmp(&b.done)
                .then(b.votes.cmp(&a.votes))
                .then(a.order.cmp(&b.order))
        });
    }

    /// Wunsch erledigt (bzw. wieder offen). `true`, wenn es ihn gibt.
    pub fn set_done(&mut self, path: &std::path::Path, done: bool) -> bool {
        let Some(w) = self.items.iter_mut().find(|w| w.path == path) else {
            return false;
        };
        w.done = done;
        self.sort();
        true
    }

    /// Wunsch `path` wurde mit der Datei `file` eingereiht.
    pub fn set_queued(&mut self, path: &std::path::Path, file: PathBuf) -> bool {
        let Some(w) = self.items.iter_mut().find(|w| w.path == path) else {
            return false;
        };
        w.queued = Some(file);
        true
    }

    /// Die Datei `playing` läuft: eingereihte Wünsche mit ihr sind erledigt. `true` bei Änderung.
    pub fn mark_played(&mut self, playing: &std::path::Path) -> bool {
        let mut changed = false;
        for w in &mut self.items {
            if !w.done && w.queued.as_deref() == Some(playing) {
                w.done = true;
                changed = true;
            }
        }
        if changed {
            self.sort();
        }
        changed
    }

    /// Liest die Liste; fehlt die Datei oder ist sie kaputt, eine leere.
    pub fn load(path: &std::path::Path) -> Self {
        let Ok(text) = std::fs::read_to_string(path) else {
            return Self::default();
        };
        match serde_json::from_str::<Self>(&text) {
            Ok(mut l) => {
                l.counter = l.items.iter().map(|w| w.order).max().unwrap_or(0);
                l.sort();
                l
            }
            Err(e) => {
                tracing::warn!(error = %e, "party: wishes not readable");
                Self::default()
            }
        }
    }

    /// Schreibt die Liste (über eine Zwischendatei).
    pub fn save(&self, path: &std::path::Path) -> std::io::Result<()> {
        if let Some(d) = path.parent() {
            std::fs::create_dir_all(d)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(
            &tmp,
            serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?,
        )?;
        std::fs::rename(tmp, path)
    }

    /// Wünsche in Anzeigereihenfolge (nach jedem Eingriff sortiert).
    pub fn items(&self) -> &[Wish] {
        &self.items
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Nimmt den Wunsch mit diesem Pfad heraus.
    pub fn take(&mut self, path: &std::path::Path) -> Option<Wish> {
        let i = self.items.iter().position(|w| w.path == path)?;
        Some(self.items.remove(i))
    }
}

/// Was eine große Taste (oder später Handy und MIDI) auslöst.
#[derive(Debug, Clone, PartialEq)]
pub enum PartyCommand {
    /// Läuft die Musik, anhalten; sonst starten.
    PlayPause,
    /// Starten, wenn nichts läuft; sonst nichts.
    Play,
    /// Stopp: ausblenden und die gemerkte Pausenstelle vergessen.
    Stop,
    /// Weiter: Crossfade zum nächsten Titel (startet, wenn nichts läuft). Auch bei
    /// „Titel wiederholen“ geht es zum nächsten Titel.
    Next,
    /// Zurück: vorheriger Titel.
    Previous,
    /// Titel von vorn.
    Restart,
    /// 10 s zurück.
    SeekBack,
    /// 10 s vor.
    SeekForward,
    /// Stumm schalten bzw. wieder auf den alten Pegel.
    Mute,
    /// Wiederholen weiterschalten: aus, Playlist, Titel, aus.
    CycleRepeat,
    /// Zufall ein/aus.
    ToggleShuffle,
    VolumeUp,
    VolumeDown,
    /// Wunsch eintragen (doppelte zählen als Stimme).
    Wish {
        path: PathBuf,
        title: String,
    },
    /// Diesen Wunsch als Nächstes spielen: hinter dem laufenden Titel einreihen; der Wunsch
    /// bleibt stehen und ist erledigt, sobald der Titel läuft.
    PlayWish(PathBuf),
    /// Wunsch `wish` mit der Datei `file` (Treffer der Bibliothek) als Nächstes einreihen.
    QueueWish {
        wish: PathBuf,
        file: PathBuf,
    },
    /// Wunsch erledigt bzw. wieder offen.
    MarkWish {
        path: PathBuf,
        done: bool,
    },
    /// Wunsch verwerfen.
    DropWish(PathBuf),
    ToggleFullscreen,
}

impl PartyCommand {
    /// Kurzform für das Befehlsprotokoll.
    pub fn name(&self) -> String {
        match self {
            PartyCommand::PlayPause => "play_pause".into(),
            PartyCommand::Play => "play".into(),
            PartyCommand::Stop => "stop".into(),
            PartyCommand::Next => "next".into(),
            PartyCommand::Previous => "previous".into(),
            PartyCommand::Restart => "restart".into(),
            PartyCommand::SeekBack => "seek_back".into(),
            PartyCommand::SeekForward => "seek_forward".into(),
            PartyCommand::Mute => "mute".into(),
            PartyCommand::CycleRepeat => "repeat".into(),
            PartyCommand::ToggleShuffle => "shuffle".into(),
            PartyCommand::VolumeUp => "volume_up".into(),
            PartyCommand::VolumeDown => "volume_down".into(),
            PartyCommand::Wish { title, .. } => format!("wish {title}"),
            PartyCommand::PlayWish(p) => format!("play_wish {}", crate::show::title_of(p)),
            PartyCommand::DropWish(p) => format!("drop_wish {}", crate::show::title_of(p)),
            PartyCommand::QueueWish { file, .. } => {
                format!("queue_wish {}", crate::show::title_of(file))
            }
            PartyCommand::MarkWish { path, done } => {
                format!("mark_wish {} {done}", crate::show::title_of(path))
            }
            PartyCommand::ToggleFullscreen => "toggle_fullscreen".into(),
        }
    }
}

/// Was die Oberfläche mit der Sendung tun soll.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    /// `Controller::start_or_next`.
    StartOrNext,
    /// `Controller::stop`.
    Stop,
    /// `Controller::pause`.
    Pause,
    /// `Controller::play`: nach einer Pause an der Stelle weiter, sonst starten.
    Play,
    /// `Controller::previous`.
    Previous,
    /// `Controller::restart`.
    Restart,
    /// `Controller::seek` um so viele Sekunden.
    Seek(f64),
    /// `Controller::cycle_repeat`.
    CycleRepeat,
    /// `Controller::toggle_shuffle`.
    ToggleShuffle,
    /// Pegel des Musikzweigs (dB).
    MusicLevel(f64),
    /// Datei hinter dem laufenden (bzw. Cue-)Titel einfügen.
    InsertNext(PathBuf),
    Fullscreen(bool),
}

/// Zustand des Party-Modus.
#[derive(Debug, Clone, PartialEq)]
pub struct PartyState {
    pub wishes: Wishlist,
    /// Musikpegel (dB), die Quelle für Lauter/Leiser.
    pub level_db: f64,
    /// Pegel vor dem Stummschalten.
    saved_db: f64,
    pub fullscreen: bool,
    log: Vec<String>,
}

impl Default for PartyState {
    fn default() -> Self {
        Self {
            wishes: Wishlist::default(),
            level_db: 0.0,
            saved_db: 0.0,
            fullscreen: false,
            log: Vec::new(),
        }
    }
}

impl PartyState {
    /// Befehlsprotokoll, älteste zuerst (höchstens [`LOG_CAP`]).
    pub fn log(&self) -> &[String] {
        &self.log
    }

    /// Führt einen Befehl aus. `running`: die Musik läuft gerade.
    pub fn apply(&mut self, cmd: PartyCommand, running: bool) -> Vec<Effect> {
        if self.log.len() >= LOG_CAP {
            self.log.remove(0);
        }
        self.log.push(cmd.name());
        match cmd {
            PartyCommand::PlayPause if running => vec![Effect::Pause],
            PartyCommand::PlayPause => vec![Effect::Play],
            PartyCommand::Next => vec![Effect::StartOrNext],
            PartyCommand::Play if !running => vec![Effect::Play],
            PartyCommand::Play => Vec::new(),
            PartyCommand::Stop => vec![Effect::Stop],
            PartyCommand::Previous => vec![Effect::Previous],
            PartyCommand::Restart => vec![Effect::Restart],
            PartyCommand::SeekBack => vec![Effect::Seek(-SEEK_STEP_SECONDS)],
            PartyCommand::SeekForward => vec![Effect::Seek(SEEK_STEP_SECONDS)],
            PartyCommand::Mute => self.toggle_mute(),
            PartyCommand::CycleRepeat => vec![Effect::CycleRepeat],
            PartyCommand::ToggleShuffle => vec![Effect::ToggleShuffle],
            PartyCommand::VolumeUp => self.step_level(VOLUME_STEP_DB),
            PartyCommand::VolumeDown => self.step_level(-VOLUME_STEP_DB),
            PartyCommand::Wish { path, title } => {
                self.wishes.add(path, &title);
                Vec::new()
            }
            PartyCommand::PlayWish(path) => {
                // Freier Text hat keine Datei: erst über die Bibliothek einen Titel wählen.
                if is_free_wish(&path) || !self.wishes.set_queued(&path, path.clone()) {
                    return Vec::new();
                }
                vec![Effect::InsertNext(path)]
            }
            PartyCommand::QueueWish { wish, file } => {
                self.wishes.set_queued(&wish, file.clone());
                vec![Effect::InsertNext(file)]
            }
            PartyCommand::MarkWish { path, done } => {
                self.wishes.set_done(&path, done);
                Vec::new()
            }
            PartyCommand::DropWish(path) => {
                self.wishes.take(&path);
                Vec::new()
            }
            PartyCommand::ToggleFullscreen => {
                self.fullscreen = !self.fullscreen;
                vec![Effect::Fullscreen(self.fullscreen)]
            }
        }
    }

    /// Stumm, wenn der Pegel am unteren Anschlag steht.
    pub fn muted(&self) -> bool {
        self.level_db <= LEVEL_MIN_DB
    }

    fn toggle_mute(&mut self) -> Vec<Effect> {
        if self.muted() {
            self.level_db = self.saved_db;
        } else {
            self.saved_db = self.level_db;
            self.level_db = LEVEL_MIN_DB;
        }
        vec![Effect::MusicLevel(self.level_db)]
    }

    fn step_level(&mut self, step: f64) -> Vec<Effect> {
        if self.muted() {
            // Lauter/Leiser hebt die Stummschaltung auf und geht vom alten Pegel aus.
            self.level_db = self.saved_db;
        }
        let next = (self.level_db + step).clamp(LEVEL_MIN_DB, LEVEL_MAX_DB);
        if next == self.level_db {
            return Vec::new();
        }
        self.level_db = next;
        vec![Effect::MusicLevel(next)]
    }
}

/// `m:ss`, ab einer Stunde `h:mm:ss`; Sekundenbruchteile werden aufgerundet (die Anzeige
/// springt erst auf 0:00, wenn der Titel wirklich aus ist).
pub fn clock(seconds: f64) -> String {
    let s = seconds.max(0.0).ceil() as u64;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

/// Zerlegt „Interpret - Titel“ (Dateiname); ohne Trenner bleibt der Interpret leer.
pub fn split_title(name: &str) -> (String, String) {
    match name.split_once(" - ") {
        Some((a, t)) if !a.trim().is_empty() && !t.trim().is_empty() => {
            (a.trim().to_string(), t.trim().to_string())
        }
        _ => (String::new(), name.trim().to_string()),
    }
}

/// Der große Titel.
#[derive(Debug, Clone, PartialEq)]
pub struct NowPlaying {
    pub title: String,
    pub artist: String,
    /// Restzeit als Text.
    pub remaining: String,
    /// 0..1; 0 vor dem Start.
    pub progress: f32,
    /// Läuft, sonst „bereit“.
    pub playing: bool,
    /// Pausiert: die Stelle ist gemerkt, Play macht dort weiter.
    pub paused: bool,
}

/// Eine große Taste.
#[derive(Debug, Clone, PartialEq)]
pub struct PartyButton {
    pub label: &'static str,
    pub command: PartyCommand,
    /// Funktion ist an (Zufall, Wiederholen, Stumm): Knopf hervorheben.
    pub active: bool,
}

/// Alles, was das Party-Layout anzeigt, ohne Fenster.
#[derive(Debug, Clone, PartialEq)]
pub struct PartyLayout {
    pub now: Option<NowPlaying>,
    /// „Interpret – Titel“ des nächsten Musiktitels.
    pub next: Option<String>,
    /// Die Tasten in Reihen (Transport, Spulen, Pegel, Modi).
    pub buttons: Vec<Vec<PartyButton>>,
    pub repeat: Repeat,
    pub shuffle: bool,
    pub muted: bool,
    /// „Titel (×Stimmen)“ in Reihenfolge.
    pub wishes: Vec<String>,
    pub level_db: f64,
    pub fullscreen: bool,
}

/// Die Tasten in festen Reihen: Transport, Spulen, Pegel, Modi. App und Fernbedienung
/// zeigen dieselben Befehle; die Namen dort sind die aus [`PartyCommand::name`].
pub fn buttons(running: bool, repeat: Repeat, shuffle: bool, muted: bool) -> Vec<Vec<PartyButton>> {
    let b = |label, command, active| PartyButton {
        label,
        command,
        active,
    };
    vec![
        vec![
            b("Zurück", PartyCommand::Previous, false),
            b(
                if running { "Pause" } else { "Play" },
                PartyCommand::PlayPause,
                false,
            ),
            b("Stopp", PartyCommand::Stop, false),
            b("Weiter", PartyCommand::Next, false),
        ],
        vec![
            b("Von vorn", PartyCommand::Restart, false),
            b("−10 s", PartyCommand::SeekBack, false),
            b("+10 s", PartyCommand::SeekForward, false),
            b("Stumm", PartyCommand::Mute, muted),
        ],
        vec![
            b("Leiser", PartyCommand::VolumeDown, false),
            b("Lauter", PartyCommand::VolumeUp, false),
            b("Zufall", PartyCommand::ToggleShuffle, shuffle),
            b(
                match repeat {
                    Repeat::One => "Titel",
                    Repeat::All => "Playlist",
                    Repeat::Off => "Wdh. aus",
                },
                PartyCommand::CycleRepeat,
                repeat != Repeat::Off,
            ),
        ],
    ]
}

/// Baut das Layout aus dem Schnappschuss der Sendung.
pub fn layout(view: &View, state: &PartyState) -> PartyLayout {
    // Läuft ein Titel, ist es der; sonst der, der als Nächstes startet (Cue).
    let idx = view.current.or((!view.running).then_some(view.cue));
    let entry = idx.and_then(|i| view.playlist.get(i));
    let now = entry.map(|e| {
        let (artist, title) = split_title(&e.title);
        let paused = view.paused;
        let playing = view.current.is_some() && paused.is_none();
        let progress = match (playing, paused, e.seconds) {
            (_, Some((rest, len)), _) if len > 0.0 => (1.0 - rest / len).clamp(0.0, 1.0) as f32,
            (true, None, Some(len)) if len > 0.0 => {
                (1.0 - view.remaining_title / len).clamp(0.0, 1.0) as f32
            }
            _ => 0.0,
        };
        let remaining = if let Some((rest, _)) = paused {
            clock(rest)
        } else if playing {
            clock(view.remaining_title)
        } else {
            clock(e.seconds.unwrap_or(0.0))
        };
        NowPlaying {
            title,
            artist,
            remaining,
            progress,
            playing,
            paused: paused.is_some(),
        }
    });
    let next = idx.and_then(|i| {
        view.playlist
            .iter()
            .skip(i + 1)
            .find(|e| e.voice.is_none())
            .map(|e| {
                let (a, t) = split_title(&e.title);
                if a.is_empty() {
                    t
                } else {
                    format!("{a} – {t}")
                }
            })
    });
    PartyLayout {
        now,
        next,
        buttons: buttons(view.running, view.repeat, view.shuffle, state.muted()),
        repeat: view.repeat,
        shuffle: view.shuffle,
        muted: state.muted(),
        wishes: state
            .wishes
            .items()
            .iter()
            .map(|w| {
                if w.votes > 1 {
                    format!("{} (×{})", w.title, w.votes)
                } else {
                    w.title.clone()
                }
            })
            .collect(),
        level_db: state.level_db,
        fullscreen: state.fullscreen,
    }
}

impl PartyLayout {
    /// Textform für Schnappschuss-Tests.
    pub fn to_text(&self) -> String {
        let mut s = String::new();
        match &self.now {
            Some(n) => {
                s += &format!(
                    "jetzt: {}{} [{}] rest {} fortschritt {:.0}%\n",
                    if n.artist.is_empty() {
                        String::new()
                    } else {
                        format!("{} – ", n.artist)
                    },
                    n.title,
                    if n.playing {
                        "läuft"
                    } else if n.paused {
                        "pausiert"
                    } else {
                        "bereit"
                    },
                    n.remaining,
                    n.progress * 100.0
                );
            }
            None => s += "jetzt: –\n",
        }
        s += &format!("nächster: {}\n", self.next.as_deref().unwrap_or("–"));
        for row in &self.buttons {
            let labels: Vec<String> = row
                .iter()
                .map(|b| format!("{}{}", b.label, if b.active { "*" } else { "" }))
                .collect();
            s += &format!("tasten: {}\n", labels.join(" | "));
        }
        s += &format!(
            "pegel: {:+.0} dB, vollbild: {}\n",
            self.level_db,
            if self.fullscreen { "an" } else { "aus" }
        );
        s += "wünsche:\n";
        for (i, w) in self.wishes.iter().enumerate() {
            s += &format!("  {}. {w}\n", i + 1);
        }
        s
    }
}

/// Seite eines Party-Decks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Side {
    #[default]
    Left,
    Right,
}

impl Side {
    pub fn other(self) -> Self {
        match self {
            Side::Left => Side::Right,
            Side::Right => Side::Left,
        }
    }

    /// Stellung des Crossfaders ganz auf dieser Seite (0 = links, 1 = rechts).
    pub fn pos(self) -> f32 {
        match self {
            Side::Left => 0.0,
            Side::Right => 1.0,
        }
    }
}

/// Zwei Decks über der einen Sende-Playlist: Die Playlist bleibt das Modell (laufender und
/// nächster Titel); hier steht nur, auf welcher Seite der laufende liegt. Jeder Titelwechsel
/// legt den neuen Titel auf die andere Seite, der nächste ist auf der freien Seite vorgeladen.
/// Der Mixer wechselt den laufenden Titel zu Beginn der Überblendung, darum läuft der
/// Crossfader ab diesem Wechsel über die eingestellte Crossfade-Länge.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Decks {
    /// Seite des laufenden (bzw. als Nächstes startenden) Titels.
    pub live: Side,
    last: Option<PathBuf>,
    /// Der vorige Titel (blendet nach einem Wechsel noch aus).
    pub previous: Option<PathBuf>,
    /// Zeitpunkt des letzten Wechsels (Sekunden), `None` vor dem ersten.
    since: Option<f64>,
}

impl Decks {
    /// Nimmt den laufenden Titel aus dem Schnappschuss. `true`, wenn die Seite gewechselt hat.
    pub fn track(&mut self, current: Option<&std::path::Path>, now: f64) -> bool {
        let Some(cur) = current else {
            // Stopp: keine Überblendung mehr, die Seite bleibt.
            if self.last.take().is_some() {
                self.since = None;
            }
            return false;
        };
        let flip = self.last.as_deref().is_some_and(|l| l != cur);
        if flip {
            self.live = self.live.other();
            self.previous = self.last.take();
            self.since = Some(now);
        }
        self.last = Some(cur.to_path_buf());
        flip
    }

    /// Crossfader-Stellung (0 = links, 1 = rechts) zur Zeit `now` bei `xf` Sekunden
    /// Überblendung: gleichmäßig von der alten zur neuen Seite.
    pub fn fader(&self, now: f64, xf: f64) -> f32 {
        crossfader_pos(self.live, self.since.map(|s| now - s), xf)
    }

    /// Blendet gerade über (der vorige Titel ist noch zu hören)?
    pub fn fading(&self, now: f64, xf: f64) -> bool {
        self.since.is_some_and(|s| xf > 0.0 && now - s < xf)
    }

    /// Dreht der Plattenteller dieser Seite? Nur das hörbare Deck: das laufende, und während
    /// der Überblendung noch das ausblendende. Pausiert oder vorgeladen steht er still.
    pub fn spinning(&self, side: Side, playing: bool, now: f64, xf: f64) -> bool {
        playing && (side == self.live || self.fading(now, xf))
    }
}

/// Crossfader-Kurve beim Auto-Übergang: `elapsed` Sekunden nach dem Wechsel auf `to`, linear
/// über `xf` Sekunden. Ohne Wechsel (oder ohne Überblendung) steht er auf `to`.
pub fn crossfader_pos(to: Side, elapsed: Option<f64>, xf: f64) -> f32 {
    let target = to.pos();
    match elapsed {
        Some(e) if xf > 0.0 => {
            let t = (e / xf).clamp(0.0, 1.0) as f32;
            let from = 1.0 - target;
            from + (target - from) * t
        }
        _ => target,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::station::ViewEntry;

    fn entry(title: &str, seconds: f64) -> ViewEntry {
        ViewEntry {
            title: title.into(),
            path: PathBuf::from(format!("/m/{title}.wav")),
            seconds: Some(seconds),
            lufs: None,
            gain_db: 0.0,
            cues: Default::default(),
            voice: None,
        }
    }

    #[test]
    fn wishlist_is_sorted_and_deduplicated() {
        let mut w = Wishlist::default();
        assert!(w.add("/a/one.mp3".into(), "One"));
        assert!(w.add("/a/two.mp3".into(), "Two"));
        assert!(w.add("/a/three.mp3".into(), "Three"));
        // Doppelter Wunsch (anderer Pfad, Schreibweise, Leerzeichen): nur eine Stimme.
        assert!(!w.add("/b/two copy.mp3".into(), "  two "));
        assert!(!w.add("/a/three.mp3".into(), "THREE"));
        assert!(!w.add("/a/three.mp3".into(), "Three"));
        let titles: Vec<(&str, u32)> = w.items().iter().map(|x| (&*x.title, x.votes)).collect();
        assert_eq!(titles, vec![("Three", 3), ("Two", 2), ("One", 1)]);
        // Gleichstand: wer zuerst kam.
        w.add("/a/four.mp3".into(), "Four");
        w.add("/a/four.mp3".into(), "Four");
        let t: Vec<&str> = w.items().iter().map(|x| &*x.title).collect();
        assert_eq!(t, vec!["Three", "Two", "Four", "One"]);
    }

    #[test]
    fn buttons_emit_the_expected_commands() {
        let mut s = PartyState::default();
        assert_eq!(s.apply(PartyCommand::PlayPause, false), vec![Effect::Play]);
        assert_eq!(s.apply(PartyCommand::PlayPause, true), vec![Effect::Pause]);
        assert_eq!(s.apply(PartyCommand::Next, true), vec![Effect::StartOrNext]);
        assert_eq!(
            s.apply(PartyCommand::VolumeDown, true),
            vec![Effect::MusicLevel(-2.0)]
        );
        assert_eq!(
            s.apply(PartyCommand::VolumeUp, true),
            vec![Effect::MusicLevel(0.0)]
        );
        assert_eq!(
            s.apply(PartyCommand::ToggleFullscreen, true),
            vec![Effect::Fullscreen(true)]
        );
        assert_eq!(
            s.apply(PartyCommand::ToggleFullscreen, true),
            vec![Effect::Fullscreen(false)]
        );
        s.apply(
            PartyCommand::Wish {
                path: "/m/x.wav".into(),
                title: "X".into(),
            },
            true,
        );
        assert_eq!(
            s.apply(PartyCommand::PlayWish("/m/x.wav".into()), true),
            vec![Effect::InsertNext("/m/x.wav".into())]
        );
        assert_eq!(s.wishes.len(), 1, "bleibt stehen, bis der Titel läuft");
        assert!(s.wishes.items()[0].queued.is_some());
        assert_eq!(
            s.log(),
            [
                "play_pause",
                "play_pause",
                "next",
                "volume_down",
                "volume_up",
                "toggle_fullscreen",
                "toggle_fullscreen",
                "wish X",
                "play_wish x"
            ]
        );
    }

    #[test]
    fn volume_stops_at_the_limits_and_log_is_capped() {
        let mut s = PartyState::default();
        for _ in 0..10 {
            s.apply(PartyCommand::VolumeUp, true);
        }
        assert_eq!(s.level_db, LEVEL_MAX_DB);
        assert!(s.apply(PartyCommand::VolumeUp, true).is_empty());
        for _ in 0..LOG_CAP + 20 {
            s.apply(PartyCommand::Next, true);
        }
        assert_eq!(s.log().len(), LOG_CAP);
    }

    #[test]
    fn layout_snapshot_without_window() {
        let view = View {
            playlist: vec![
                entry("Abba - Waterloo", 200.0),
                entry("Queen - Radio Ga Ga", 180.0),
                entry("Solo", 60.0),
            ],
            current: Some(0),
            running: true,
            remaining_title: 50.0,
            ..View::default()
        };
        let mut state = PartyState::default();
        state.wishes.add("/m/a.wav".into(), "Wonderwall");
        state.wishes.add("/m/b.wav".into(), "Mr. Brightside");
        state.wishes.add("/m/b.wav".into(), "mr. brightside");
        let l = layout(&view, &state);
        assert_eq!(
            l.to_text(),
            "jetzt: Abba – Waterloo [läuft] rest 0:50 fortschritt 75%\n\
             nächster: Queen – Radio Ga Ga\n\
             tasten: Zurück | Pause | Stopp | Weiter\n\
             tasten: Von vorn | −10 s | +10 s | Stumm\n\
             tasten: Leiser | Lauter | Zufall | Wdh. aus\n\
             pegel: +0 dB, vollbild: aus\n\
             wünsche:\n  1. Mr. Brightside (×2)\n  2. Wonderwall\n"
        );
        // Vor dem Start: der Cue-Titel ist „bereit“, die Taste heißt Play.
        let idle = View {
            running: false,
            current: None,
            cue: 1,
            ..view
        };
        let t = layout(&idle, &PartyState::default()).to_text();
        assert!(t.starts_with("jetzt: Queen – Radio Ga Ga [bereit] rest 3:00 fortschritt 0%\n"));
        assert!(t.contains("nächster: Solo\n") && t.contains("tasten: Zurück | Play |"));
        assert!(
            layout(&View::default(), &PartyState::default())
                .to_text()
                .starts_with("jetzt: –\nnächster: –\n")
        );
    }

    #[test]
    fn clock_rounds_up() {
        assert_eq!(clock(0.0), "0:00");
        assert_eq!(clock(0.2), "0:01");
        assert_eq!(clock(59.0), "0:59");
        assert_eq!(clock(61.5), "1:02");
        assert_eq!(clock(3600.0), "1:00:00");
    }

    #[test]
    fn three_titles_alternate_left_right_left() {
        let mut d = Decks::default();
        let p = |n: &str| PathBuf::from(n);
        assert!(!d.track(Some(&p("1.mp3")), 0.0));
        assert_eq!(d.live, Side::Left);
        assert!(d.track(Some(&p("2.mp3")), 10.0));
        assert_eq!(d.live, Side::Right);
        assert_eq!(d.previous, Some(p("1.mp3")));
        assert!(d.track(Some(&p("3.mp3")), 20.0));
        assert_eq!(d.live, Side::Left);
        // Gleicher Titel im nächsten Bild: kein Wechsel.
        assert!(!d.track(Some(&p("3.mp3")), 21.0));
        assert_eq!(d.live, Side::Left);
    }

    #[test]
    fn crossfader_moves_over_the_crossfade_length() {
        assert_eq!(crossfader_pos(Side::Right, None, 5.0), 1.0);
        assert_eq!(crossfader_pos(Side::Right, Some(0.0), 5.0), 0.0);
        assert!((crossfader_pos(Side::Right, Some(2.5), 5.0) - 0.5).abs() < 1e-6);
        assert_eq!(crossfader_pos(Side::Right, Some(9.0), 5.0), 1.0);
        assert!((crossfader_pos(Side::Left, Some(1.0), 4.0) - 0.75).abs() < 1e-6);
        assert_eq!(crossfader_pos(Side::Left, Some(1.0), 0.0), 0.0);
        let mut d = Decks::default();
        d.track(Some(std::path::Path::new("a")), 0.0);
        d.track(Some(std::path::Path::new("b")), 100.0);
        let mut last = -1.0;
        for k in 0..=10 {
            let f = d.fader(100.0 + f64::from(k) * 0.5, 5.0);
            assert!(f >= last, "monoton");
            last = f;
        }
        assert_eq!(last, 1.0);
    }

    #[test]
    fn only_the_audible_deck_spins() {
        let mut d = Decks::default();
        d.track(Some(std::path::Path::new("a")), 0.0);
        assert!(d.spinning(Side::Left, true, 1.0, 5.0));
        assert!(!d.spinning(Side::Right, true, 1.0, 5.0), "vorgeladen steht");
        assert!(!d.spinning(Side::Left, false, 1.0, 5.0), "pausiert steht");
        d.track(Some(std::path::Path::new("b")), 10.0);
        assert!(d.spinning(Side::Right, true, 11.0, 5.0));
        assert!(d.spinning(Side::Left, true, 11.0, 5.0), "blendet noch aus");
        assert!(
            !d.spinning(Side::Left, true, 16.0, 5.0),
            "nach der Blende still"
        );
    }

    #[test]
    fn wishes_queue_done_drop_and_save() {
        let tmp = tempfile::tempdir().unwrap();
        let mut s = PartyState::default();
        let free = free_wish_path("Tina: Queen");
        s.apply(
            PartyCommand::Wish {
                path: free.clone(),
                title: "Tina: Queen".into(),
            },
            true,
        );
        // Freier Text lässt sich ohne Titel nicht direkt einreihen.
        assert!(
            s.apply(PartyCommand::PlayWish(free.clone()), true)
                .is_empty()
        );
        let file = PathBuf::from("/m/Queen - Radio.mp3");
        let fx = s.apply(
            PartyCommand::QueueWish {
                wish: free.clone(),
                file: file.clone(),
            },
            true,
        );
        assert_eq!(fx, vec![Effect::InsertNext(file.clone())]);
        assert_eq!(s.wishes.len(), 1, "bleibt stehen");
        assert!(!s.wishes.items()[0].done);
        assert!(s.wishes.mark_played(&file));
        assert!(s.wishes.items()[0].done);
        s.apply(
            PartyCommand::MarkWish {
                path: free.clone(),
                done: false,
            },
            true,
        );
        assert!(!s.wishes.items()[0].done);
        s.apply(
            PartyCommand::MarkWish {
                path: free.clone(),
                done: true,
            },
            true,
        );
        let f = tmp.path().join("w/party_wishes.json");
        s.wishes.save(&f).unwrap();
        let back = Wishlist::load(&f);
        assert_eq!(back, s.wishes);
        s.apply(PartyCommand::DropWish(free), true);
        assert!(s.wishes.is_empty());
        assert!(Wishlist::load(&tmp.path().join("fehlt.json")).is_empty());
    }
}
