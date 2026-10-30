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
#[derive(Debug, Clone, PartialEq)]
pub struct Wish {
    pub path: PathBuf,
    pub title: String,
    /// Wie oft er gewünscht wurde.
    pub votes: u32,
    /// Reihenfolge des ersten Eingangs.
    order: u64,
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
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Wishlist {
    items: Vec<Wish>,
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
            self.sort();
            return false;
        }
        self.counter += 1;
        self.items.push(Wish {
            path,
            title: title.trim().to_string(),
            votes: 1,
            order: self.counter,
        });
        self.sort();
        true
    }

    fn sort(&mut self) {
        self.items
            .sort_by(|a, b| b.votes.cmp(&a.votes).then(a.order.cmp(&b.order)));
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
    /// Diesen Wunsch als Nächstes spielen: aus der Liste nehmen, hinter dem laufenden Titel einreihen.
    PlayWish(PathBuf),
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
            PartyCommand::PlayWish(path) => match self.wishes.take(&path) {
                Some(w) => vec![Effect::InsertNext(w.path)],
                None => Vec::new(),
            },
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
            b("🔀 Zufall", PartyCommand::ToggleShuffle, shuffle),
            b(
                match repeat {
                    Repeat::One => "🔂 Titel",
                    Repeat::All => "🔁 Playlist",
                    Repeat::Off => "🔁 Aus",
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
        assert!(s.wishes.is_empty());
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
             tasten: Leiser | Lauter | 🔀 Zufall | 🔁 Aus\n\
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
}
