//! `radio.json`: Playlist, Cart-Belegung und Einstellungen, versioniert.
//!
//! GUI und Agent lesen und schreiben dieselbe Datei. Geschrieben wird atomar über eine
//! neu angelegte Temp-Datei im selben Ordner plus `rename`; ein Symlink als Ziel wird
//! abgelehnt. Änderungen an der Playlist sind kein Undo-Schritt des Projekts.

use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::audio::{Measured, file_stamp};
use crate::cues::Cues;
use crate::level::{TARGET_LUFS, auto_gain_db};
use crate::{CARTS, DECKS};

/// Aktuelle Formatversion von `radio.json`.
pub const VERSION: u32 = 1;

/// Ein Titel der Playlist. Die Messwerte sind ein Cache, gültig solange `stamp` passt.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlaylistEntry {
    pub path: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lufs: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peak_db: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seconds: Option<f64>,
    /// Dateistand (Größe, Änderungszeit) zur Zeit der Messung.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stamp: Option<String>,
    /// Cue-Punkte (Paket 15).
    #[serde(default, skip_serializing_if = "Cues::is_empty")]
    pub cues: Cues,
    /// Voice-Track (Paket 30): gesetzt, wenn der Eintrag eine Moderation ist, die zwischen
    /// ihren Nachbarn über die Musik gelegt wird. `path` ist dann die Sprachdatei.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voice: Option<crate::voicetrack::VoiceTrack>,
    /// Laufzeit-Kennung (nicht gespeichert), vom Controller vergeben.
    #[serde(skip)]
    pub uid: u32,
}

impl PlaylistEntry {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            lufs: None,
            peak_db: None,
            seconds: None,
            stamp: None,
            cues: Cues::default(),
            voice: None,
            uid: 0,
        }
    }

    /// Anzeigename: Dateiname ohne Endung.
    pub fn title(&self) -> String {
        title_of(&self.path)
    }

    /// Ist die gecachte Messung noch gültig?
    pub fn is_measured(&self) -> bool {
        self.seconds.is_some() && self.stamp.is_some() && self.stamp == file_stamp(&self.path)
    }

    /// Übernimmt eine Messung samt aktuellem Dateistand.
    pub fn set_measured(&mut self, m: Measured) {
        self.lufs = m.lufs;
        self.peak_db = m.peak_db;
        self.seconds = Some(m.seconds);
        self.stamp = file_stamp(&self.path);
        // Cue-Vorschlag nur, wo noch nichts gesetzt ist: von Hand gesetzte Punkte bleiben.
        if self.cues.is_empty() && m.auto_cues.validate(Some(m.seconds)).is_ok() {
            self.cues = m.auto_cues;
        }
    }

    /// Gespielte Länge (Cue-In bis Cue-Out), falls gemessen.
    pub fn play_seconds(&self) -> Option<f64> {
        self.seconds.map(|s| self.cues.play_seconds(s))
    }

    /// Auto-Gain dieses Titels in dB.
    pub fn gain_db(&self, target: f64) -> f64 {
        auto_gain_db(self.lufs, self.peak_db, target)
    }
}

/// Dateiname ohne Endung.
pub fn title_of(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// Belegung einer Cart-Taste.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CartEntry {
    pub path: PathBuf,
    #[serde(default)]
    pub label: String,
    /// Start- und Endpunkt des Carts (nur Cue-In und Cue-Out zählen).
    #[serde(default, skip_serializing_if = "Cues::is_empty")]
    pub cues: Cues,
}

impl CartEntry {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let label = title_of(&path);
        Self {
            path,
            label,
            cues: Cues::default(),
        }
    }
}

/// Buchstabe eines Decks: 0 = A.
pub fn deck_name(deck: usize) -> char {
    (b'A' + (deck.min(25)) as u8) as char
}

/// Kein Cart auf dieser Taste: Wert in [`RadioShow::cart_keys`].
pub const NO_KEY: &str = "-";

/// Tastenname der Standardbelegung: Cart 1–9 auf 1–9, Cart 10 auf 0 (Namen wie `egui::Key::name`).
pub fn default_cart_key(slot: usize) -> Option<&'static str> {
    const KEYS: [&str; 10] = ["1", "2", "3", "4", "5", "6", "7", "8", "9", "0"];
    KEYS.get(slot).copied()
}

fn decks_empty(v: &[Option<CartEntry>]) -> bool {
    v.iter().all(Option::is_none)
}

fn all_false(v: &[bool]) -> bool {
    !v.contains(&true)
}

fn is_true(v: &bool) -> bool {
    *v
}

fn all_empty(v: &[String]) -> bool {
    v.iter().all(String::is_empty)
}

fn default_crossfade() -> f64 {
    3.0
}

fn default_stop_fade() -> f64 {
    3.0
}

fn is_default_stop_fade(v: &f64) -> bool {
    *v == default_stop_fade()
}

fn default_duck() -> f64 {
    -12.0
}

fn yes() -> bool {
    true
}

fn default_target() -> f64 {
    TARGET_LUFS
}

/// Inhalt von `radio.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RadioShow {
    pub version: u32,
    #[serde(default = "default_crossfade")]
    pub crossfade_seconds: f64,
    #[serde(default = "default_duck")]
    pub duck_db: f64,
    /// Musik auch absenken, solange ein Cart spielt (#149).
    #[serde(default = "yes")]
    pub cart_duck: bool,
    #[serde(default = "default_target")]
    pub target_lufs: f64,
    /// Sendebus-Processing (Lautheitsregelung auf `target_lufs`, Kompressor, Limiter).
    #[serde(default = "yes")]
    pub bus_processing: bool,
    /// Mikro-Kette (EQ, Gate, Kompressor).
    #[serde(default = "yes")]
    pub mic_processing: bool,
    /// Die aktive Playlist (die, die auf Sendung geht).
    #[serde(default)]
    pub playlist: Vec<PlaylistEntry>,
    /// Alle Playlisten der Sendung in fester Reihenfolge; im Feld `active` steht nur der
    /// Name, die Titel liegen in `playlist`. Alte Dateien ohne Feld: „Playlist 1“.
    #[serde(default)]
    pub playlists: Vec<crate::playlists::NamedPlaylist>,
    /// Index der aktiven Playlist in `playlists`.
    #[serde(default)]
    pub active: usize,
    /// Genau [`CARTS`] Felder, `null` = frei.
    #[serde(default)]
    pub carts: Vec<Option<CartEntry>>,
    /// Taste je Cart (Name wie `egui::Key::name`), leer = Standard ([`default_cart_key`]),
    /// [`NO_KEY`] = keine. Genau [`CARTS`] Felder.
    #[serde(default, skip_serializing_if = "all_empty")]
    pub cart_keys: Vec<String>,
    /// Genau [`DECKS`] Decks (A, B, C), `null` = leer. Nur Cue-In und Cue-Out zählen.
    #[serde(default, skip_serializing_if = "decks_empty")]
    pub decks: Vec<Option<CartEntry>>,
    /// „Zurück zur Playlist“ je Deck (A, B, C): endet das Deck, läuft die Playlist weiter.
    /// Vorgabe aus. Genau [`DECKS`] Felder.
    #[serde(default, skip_serializing_if = "all_false")]
    pub deck_return: Vec<bool>,
    /// Party: Auto-Übergang zwischen den zwei Decks (Vorgabe an).
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub party_auto: bool,
    /// Gespielte Titel in der Sende-Playlist ausblenden (#215). Nur Anzeige, gelöscht wird
    /// nichts. Vorgabe aus.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hide_played: bool,
    /// Stopp mit Ausblenden (#227): Stopp am Sende-Player und an den Decks blendet über
    /// [`Self::stop_fade_seconds`] aus statt hart abzubrechen. Vorgabe aus.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub stop_fade: bool,
    /// Dauer des Ausblendens beim Stopp (Sekunden).
    #[serde(
        default = "default_stop_fade",
        skip_serializing_if = "is_default_stop_fade"
    )]
    pub stop_fade_seconds: f64,
    /// Auto-Weiter in der Sende-Playlist (#148). Aus: nach jedem Titel hält die Playlist an,
    /// der nächste steht als Cue bereit und startet mit „Play“. Vorgabe an.
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub auto_next: bool,
}

impl Default for RadioShow {
    fn default() -> Self {
        Self {
            version: VERSION,
            crossfade_seconds: default_crossfade(),
            duck_db: default_duck(),
            cart_duck: true,
            target_lufs: default_target(),
            bus_processing: true,
            mic_processing: true,
            playlist: Vec::new(),
            playlists: vec![crate::playlists::NamedPlaylist::new(
                crate::playlists::FIRST_NAME,
            )],
            active: 0,
            carts: vec![None; CARTS],
            cart_keys: vec![String::new(); CARTS],
            decks: vec![None; DECKS],
            deck_return: vec![false; DECKS],
            party_auto: true,
            hide_played: false,
            stop_fade: false,
            stop_fade_seconds: default_stop_fade(),
            auto_next: true,
        }
    }
}

/// Längste einstellbare Überblendung.
pub const MAX_CROSSFADE: f64 = 15.0;

impl RadioShow {
    /// Bringt Werte in gültige Bereiche und die Cart-Liste auf [`CARTS`] Felder.
    pub fn sanitize(&mut self) {
        if !self.crossfade_seconds.is_finite() {
            self.crossfade_seconds = default_crossfade();
        }
        self.crossfade_seconds = self.crossfade_seconds.clamp(0.0, MAX_CROSSFADE);
        if !self.stop_fade_seconds.is_finite() {
            self.stop_fade_seconds = default_stop_fade();
        }
        self.stop_fade_seconds = self.stop_fade_seconds.clamp(0.1, MAX_CROSSFADE);
        if !self.duck_db.is_finite() {
            self.duck_db = default_duck();
        }
        self.duck_db = self.duck_db.clamp(-40.0, 0.0);
        if !self.target_lufs.is_finite() {
            self.target_lufs = default_target();
        }
        self.target_lufs = self.target_lufs.clamp(-40.0, -5.0);
        self.carts.resize(CARTS, None);
        self.decks.resize(DECKS, None);
        self.deck_return.resize(DECKS, false);
        for e in self.decks.iter_mut().flatten() {
            if e.cues.validate(None).is_err() {
                e.cues = Cues::default();
            }
        }
        self.cart_keys.resize(CARTS, String::new());
        for e in self.carts.iter_mut().flatten() {
            if e.cues.validate(None).is_err() {
                e.cues = Cues::default();
            }
        }
        self.sanitize_playlists();
        for e in &mut self.playlist {
            if let Some(v) = e.voice.as_mut() {
                v.params = v.params.sanitized();
            }
        }
    }

    /// Die Taste von Cart `slot`: eigene Belegung, sonst die Standardtaste.
    pub fn cart_key(&self, slot: usize) -> Option<String> {
        match self.cart_keys.get(slot).map(String::as_str) {
            Some(NO_KEY) => None,
            Some(k) if !k.is_empty() => Some(k.to_string()),
            _ => default_cart_key(slot).map(str::to_string),
        }
    }

    /// Legt die Taste von Cart `slot` fest (`None` = keine). Hatte ein anderes Cart diese Taste,
    /// verliert es sie: eine Taste startet immer genau ein Cart.
    pub fn set_cart_key(&mut self, slot: usize, key: Option<&str>) -> Result<(), String> {
        if slot >= CARTS {
            return Err(format!("Cart {slot} gibt es nicht"));
        }
        self.cart_keys.resize(CARTS, String::new());
        if let Some(k) = key {
            if k.is_empty()
                || k == NO_KEY
                || k.len() > 16
                || !k.chars().all(|c| c.is_ascii_alphanumeric())
            {
                return Err(format!("Taste „{k}“ ist ungültig"));
            }
            for other in 0..CARTS {
                if other != slot && self.cart_key(other).as_deref() == Some(k) {
                    self.cart_keys[other] = NO_KEY.to_string();
                }
            }
        }
        self.cart_keys[slot] = key.map_or(NO_KEY.to_string(), str::to_string);
        // Wieder die Standardtaste: Eintrag leer lassen, damit die Datei schlicht bleibt.
        if self.cart_keys[slot] == default_cart_key(slot).unwrap_or("") {
            self.cart_keys[slot].clear();
        }
        Ok(())
    }

    /// Das Cart, das die Taste `key` startet.
    pub fn cart_for_key(&self, key: &str) -> Option<usize> {
        (0..CARTS).find(|&s| self.cart_key(s).as_deref() == Some(key))
    }

    /// Setzt Start und Ende von Cart `slot` (geprüft; andere Marken fallen weg).
    pub fn set_cart_cues(&mut self, slot: usize, cues: Cues) -> Result<(), String> {
        let c = Cues {
            cue_in: cues.cue_in,
            cue_out: cues.cue_out,
            ..Cues::default()
        };
        c.validate(None)?;
        match self.carts.get_mut(slot) {
            Some(Some(e)) => {
                e.cues = c;
                Ok(())
            }
            _ => Err(format!("Cart {} ist frei", slot + 1)),
        }
    }

    /// Setzt Cue-In und Cue-Out von Deck `deck`.
    pub fn set_deck_cues(&mut self, deck: usize, cues: Cues) -> Result<(), String> {
        let c = Cues {
            cue_in: cues.cue_in,
            cue_out: cues.cue_out,
            ..Cues::default()
        };
        c.validate(None)?;
        match self.decks.get_mut(deck) {
            Some(Some(e)) => {
                e.cues = c;
                Ok(())
            }
            _ => Err(format!("Deck {} ist leer", deck_name(deck))),
        }
    }

    /// Liest `path`. Fehlt die Datei, gibt es eine leere Sendung.
    #[tracing::instrument(level = "info", skip_all, name = "show_load")]
    pub fn load(path: &Path) -> Result<Self, String> {
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(format!("{}: {e}", path.display())),
        };
        Self::from_json(&text).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// Aus JSON; eine neuere Formatversion wird abgelehnt.
    pub fn from_json(text: &str) -> Result<Self, String> {
        let mut show: Self = serde_json::from_str(text).map_err(|e| e.to_string())?;
        if show.version > VERSION || show.version == 0 {
            return Err(format!(
                "radio.json Version {} wird nicht unterstützt (kann {VERSION})",
                show.version
            ));
        }
        show.version = VERSION;
        show.sanitize();
        Ok(show)
    }

    /// Schreibt atomar nach `path` (Endung `.json`, kein Symlink). Legt den Ordner an.
    #[tracing::instrument(level = "info", skip_all, name = "show_save")]
    pub fn save(&self, path: &Path) -> Result<(), String> {
        let is_json = path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("json"));
        if !is_json {
            return Err(format!("{}: Endung muss .json sein", path.display()));
        }
        if let Ok(m) = std::fs::symlink_metadata(path)
            && (m.file_type().is_symlink() || !m.is_file())
        {
            return Err(format!(
                "{}: Ziel ist ein Symlink oder keine Datei",
                path.display()
            ));
        }
        let dir = match path.parent() {
            Some(d) if !d.as_os_str().is_empty() => d.to_path_buf(),
            _ => PathBuf::from("."),
        };
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let mut show = self.clone();
        show.version = VERSION;
        let text = serde_json::to_string_pretty(&show).map_err(|e| e.to_string())?;
        let mut opened = None;
        for n in 0..100u32 {
            let p = dir.join(format!(".radio-{}-{n}.json", std::process::id()));
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&p)
            {
                Ok(f) => {
                    opened = Some((p, f));
                    break;
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(format!("{}: {e}", p.display())),
            }
        }
        let (tmp, mut file) = opened.ok_or("keine Temp-Datei frei")?;
        let r = file
            .write_all(text.as_bytes())
            .and_then(|()| file.sync_all())
            .and_then(|()| std::fs::rename(&tmp, path))
            .map_err(|e| format!("{}: {e}", path.display()));
        drop(file);
        if r.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        r
    }

    /// Effektive Überblendung zwischen zwei Titeln (Sekunden), aus ihren gespielten
    /// Längen: höchstens die halbe Länge jedes der beiden. Genau so rechnet der Mixer.
    pub fn crossfade_between(&self, a_seconds: f64, b_seconds: f64) -> f64 {
        self.crossfade_seconds
            .min(a_seconds / 2.0)
            .min(b_seconds / 2.0)
            .max(0.0)
    }

    /// Überblendung von Eintrag `a` nach `b` nach der Mixxx-Logik aus Outro von `a` und Intro
    /// von `b` (`transition.rs`); ohne Marken [`Self::crossfade_between`].
    /// Ungemessene Titel zählen 0.
    pub fn crossfade_entries(&self, a: &PlaylistEntry, b: &PlaylistEntry) -> f64 {
        let (Some(sa), Some(sb)) = (a.seconds, b.seconds) else {
            return 0.0;
        };
        crate::transition::overlap_seconds(self.crossfade_seconds, &a.cues, sa, &b.cues, sb)
    }

    /// Gesamtlänge der Playlist ab Titel `from` (Sekunden): gespielte Längen minus
    /// Überblendungen. Ungemessene Titel zählen 0.
    pub fn total_seconds_from(&self, from: usize) -> f64 {
        // Voice-Tracks liegen über der Musik und verlängern die Sendung nicht.
        let rest: Vec<&PlaylistEntry> = self
            .playlist
            .get(from..)
            .unwrap_or(&[])
            .iter()
            .filter(|e| e.voice.is_none())
            .collect();
        let sum: f64 = rest.iter().map(|e| e.play_seconds().unwrap_or(0.0)).sum();
        let fades: f64 = rest
            .windows(2)
            .map(|w| self.crossfade_entries(w[0], w[1]))
            .sum();
        sum - fades
    }

    /// Setzt die Cue-Punkte von Titel `index` (geprüft). Gibt die alten zurück.
    pub fn set_cues(&mut self, index: usize, cues: Cues) -> Result<Cues, String> {
        let e = self
            .playlist
            .get_mut(index)
            .ok_or_else(|| format!("Titel {} gibt es nicht", index + 1))?;
        cues.validate(e.seconds)?;
        Ok(std::mem::replace(&mut e.cues, cues))
    }

    /// Verschiebt einen Titel von `from` nach `to`.
    pub fn move_entry(&mut self, from: usize, to: usize) -> bool {
        if from >= self.playlist.len() || to >= self.playlist.len() {
            return false;
        }
        let e = self.playlist.remove(from);
        self.playlist.insert(to, e);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_load_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("sub/radio.json");
        let mut show = RadioShow::default();
        show.playlist.push(PlaylistEntry::new("/musik/a.wav"));
        let mut b = PlaylistEntry::new("/musik/b.wav");
        b.lufs = Some(-12.5);
        b.seconds = Some(180.0);
        show.playlist.push(b);
        show.carts[3] = Some(CartEntry::new("/jingles/Station ID.wav"));
        show.crossfade_seconds = 5.0;
        show.save(&p).unwrap();
        let back = RadioShow::load(&p).unwrap();
        assert_eq!(back, show);
        assert_eq!(back.carts[3].as_ref().unwrap().label, "Station ID");
        assert_eq!(std::fs::read_dir(p.parent().unwrap()).unwrap().count(), 1);
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(text.contains("\"version\": 1"), "{text}");
    }

    #[test]
    fn missing_file_is_empty_and_versions_are_checked() {
        let tmp = tempfile::tempdir().unwrap();
        let show = RadioShow::load(&tmp.path().join("fehlt.json")).unwrap();
        assert_eq!(show.carts.len(), CARTS);
        assert_eq!(show.crossfade_seconds, 3.0);
        assert!(RadioShow::from_json(r#"{"version": 2}"#).is_err());
        assert!(RadioShow::from_json(r#"{"version": 0}"#).is_err());
        // Old/short files get defaults and 16 carts.
        let s = RadioShow::from_json(r#"{"version": 1, "crossfade_seconds": 99}"#).unwrap();
        assert_eq!(s.crossfade_seconds, MAX_CROSSFADE);
        assert_eq!(s.carts.len(), CARTS);
        assert!(show.save(&tmp.path().join("radio.txt")).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn refuses_symlink_target() {
        let tmp = tempfile::tempdir().unwrap();
        let victim = tmp.path().join("victim.json");
        std::fs::write(&victim, "x").unwrap();
        let link = tmp.path().join("radio.json");
        std::os::unix::fs::symlink(&victim, &link).unwrap();
        assert!(RadioShow::default().save(&link).is_err());
        assert_eq!(std::fs::read_to_string(&victim).unwrap(), "x");
    }

    #[test]
    fn total_length_subtracts_crossfades() {
        let mut show = RadioShow::default();
        for s in [10.0, 20.0, 4.0] {
            let mut e = PlaylistEntry::new("x.wav");
            e.seconds = Some(s);
            show.playlist.push(e);
        }
        // 3 s between 10 and 20, then for the 4 s title Mixxx keeps the transition time before its own end: 4 - 3 = 1 s.
        assert!((show.total_seconds_from(0) - (34.0 - 3.0 - 1.0)).abs() < 1e-9);
        assert!((show.total_seconds_from(1) - 23.0).abs() < 1e-9);
        assert!(show.move_entry(2, 0));
        assert_eq!(show.playlist[0].seconds, Some(4.0));
        assert!(!show.move_entry(5, 0));
    }

    #[test]
    fn measuring_proposes_cues_but_never_overrides_manual_ones() {
        let auto = Cues {
            cue_in: Some(1.0),
            fade_out: Some(8.0),
            cue_out: Some(10.0),
            ..Cues::default()
        };
        let m = Measured {
            lufs: Some(-20.0),
            peak_db: Some(-3.0),
            seconds: 12.0,
            auto_cues: auto,
        };
        let mut fresh = PlaylistEntry::new("/musik/a.wav");
        fresh.set_measured(m);
        assert_eq!(fresh.cues, auto);
        assert_eq!(fresh.play_seconds(), Some(9.0));

        let manual = Cues {
            cue_out: Some(11.0),
            ..Cues::default()
        };
        let mut hand = PlaylistEntry::new("/musik/b.wav");
        hand.cues = manual;
        hand.set_measured(m);
        assert_eq!(hand.cues, manual, "Handarbeit hat Vorrang");

        // Ein Vorschlag, der nicht zur Länge passt, wird verworfen.
        let mut bad = PlaylistEntry::new("/musik/c.wav");
        bad.set_measured(Measured { seconds: 5.0, ..m });
        assert!(bad.cues.is_empty());
    }

    #[test]
    fn cues_shorten_and_set_the_crossfade() {
        let mut show = RadioShow::default();
        for s in [10.0, 20.0] {
            let mut e = PlaylistEntry::new("x.wav");
            e.seconds = Some(s);
            show.playlist.push(e);
        }
        let old = show
            .set_cues(
                0,
                Cues {
                    cue_in: Some(1.0),
                    cue_out: Some(9.0),
                    fade_out: Some(8.0),
                    ..Cues::default()
                },
            )
            .unwrap();
        assert!(old.is_empty());
        // 8 s + 20 s - 1 s Überblendung ab Fade-Out-Marker.
        assert!((show.total_seconds_from(0) - 27.0).abs() < 1e-9);
        assert!(
            show.set_cues(
                1,
                Cues {
                    cue_in: Some(30.0),
                    ..Cues::default()
                }
            )
            .is_err()
        );
        assert!(show.set_cues(5, Cues::default()).is_err());
        // Roundtrip über JSON, Version bleibt 1.
        let text = serde_json::to_string(&show).unwrap();
        assert!(text.contains("cue_in"));
        assert_eq!(RadioShow::from_json(&text).unwrap(), show);
    }

    #[test]
    fn cart_keys_default_change_and_stay_unique() {
        let mut show = RadioShow::default();
        assert_eq!(show.cart_key(0).as_deref(), Some("1"));
        assert_eq!(show.cart_key(9).as_deref(), Some("0"));
        assert_eq!(show.cart_key(10), None, "ab Cart 11 keine Standardtaste");
        assert_eq!(show.cart_for_key("3"), Some(2));
        // Cart 11 bekommt F5; Cart 1 bekommt Cart 3s Taste: Cart 3 verliert sie.
        show.set_cart_key(10, Some("F5")).unwrap();
        assert_eq!(show.cart_for_key("F5"), Some(10));
        show.set_cart_key(0, Some("3")).unwrap();
        assert_eq!(show.cart_for_key("3"), Some(0));
        assert_eq!(show.cart_key(2), None);
        assert_eq!(show.cart_key(0).as_deref(), Some("3"));
        // Zurück auf die Standardtaste hält die Datei schlicht; keine Taste geht auch.
        show.set_cart_key(2, Some("3")).unwrap();
        assert_eq!(show.cart_key(0), None);
        assert!(show.cart_keys[2].is_empty());
        show.set_cart_key(1, None).unwrap();
        assert_eq!(show.cart_key(1), None);
        assert!(show.set_cart_key(1, Some("a b")).is_err());
        assert!(show.set_cart_key(CARTS, Some("A")).is_err());
        // Überlebt Speichern und Laden; alte Dateien ohne Tasten bekommen die Standardtasten.
        let back: RadioShow = serde_json::from_str(&serde_json::to_string(&show).unwrap()).unwrap();
        assert_eq!(back.cart_key(10).as_deref(), Some("F5"));
        let mut old: RadioShow = serde_json::from_str(r#"{"version":1}"#).unwrap();
        old.sanitize();
        assert_eq!(old.cart_key(4).as_deref(), Some("5"));
    }

    #[test]
    fn cart_cues_are_validated_and_kept() {
        let mut show = RadioShow::default();
        let cues = Cues {
            cue_in: Some(0.5),
            cue_out: Some(2.0),
            hook: Some(1.0),
            ..Cues::default()
        };
        assert!(show.set_cart_cues(3, cues).is_err(), "leeres Cart");
        show.carts[3] = Some(CartEntry::new("/jingles/Station ID.wav"));
        show.set_cart_cues(3, cues).unwrap();
        let e = show.carts[3].as_ref().unwrap();
        assert_eq!(
            (e.cues.cue_in, e.cues.cue_out, e.cues.hook),
            (Some(0.5), Some(2.0), None)
        );
        let bad = Cues {
            cue_in: Some(3.0),
            cue_out: Some(2.0),
            ..Cues::default()
        };
        assert!(show.set_cart_cues(3, bad).is_err());
        let back: RadioShow = serde_json::from_str(&serde_json::to_string(&show).unwrap()).unwrap();
        assert_eq!(back.carts[3].as_ref().unwrap().cues.cue_out, Some(2.0));
    }
}
