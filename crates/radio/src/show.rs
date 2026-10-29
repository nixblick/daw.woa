//! `radio.json`: Playlist, Cart-Belegung und Einstellungen, versioniert.
//!
//! GUI und Agent lesen und schreiben dieselbe Datei. Geschrieben wird atomar über eine
//! neu angelegte Temp-Datei im selben Ordner plus `rename`; ein Symlink als Ziel wird
//! abgelehnt. Änderungen an der Playlist sind kein Undo-Schritt des Projekts.

use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::CARTS;
use crate::audio::{Measured, file_stamp};
use crate::level::{TARGET_LUFS, auto_gain_db};

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
}

impl CartEntry {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let label = title_of(&path);
        Self { path, label }
    }
}

fn default_crossfade() -> f64 {
    3.0
}

fn default_duck() -> f64 {
    -12.0
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
    #[serde(default = "default_target")]
    pub target_lufs: f64,
    #[serde(default)]
    pub playlist: Vec<PlaylistEntry>,
    /// Genau [`CARTS`] Felder, `null` = frei.
    #[serde(default)]
    pub carts: Vec<Option<CartEntry>>,
}

impl Default for RadioShow {
    fn default() -> Self {
        Self {
            version: VERSION,
            crossfade_seconds: default_crossfade(),
            duck_db: default_duck(),
            target_lufs: default_target(),
            playlist: Vec::new(),
            carts: vec![None; CARTS],
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
        if !self.duck_db.is_finite() {
            self.duck_db = default_duck();
        }
        self.duck_db = self.duck_db.clamp(-40.0, 0.0);
        if !self.target_lufs.is_finite() {
            self.target_lufs = default_target();
        }
        self.target_lufs = self.target_lufs.clamp(-40.0, -5.0);
        self.carts.resize(CARTS, None);
    }

    /// Liest `path`. Fehlt die Datei, gibt es eine leere Sendung.
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

    /// Effektive Überblendung zwischen zwei Titeln (Sekunden): höchstens die halbe Länge
    /// jedes der beiden. Genau so rechnet auch der Mixer.
    pub fn crossfade_between(&self, a_seconds: f64, b_seconds: f64) -> f64 {
        self.crossfade_seconds
            .min(a_seconds / 2.0)
            .min(b_seconds / 2.0)
            .max(0.0)
    }

    /// Gesamtlänge der Playlist ab Titel `from` (Sekunden): Summe minus Überblendungen.
    /// Ungemessene Titel zählen 0.
    pub fn total_seconds_from(&self, from: usize) -> f64 {
        let lens: Vec<f64> = self
            .playlist
            .iter()
            .skip(from)
            .map(|e| e.seconds.unwrap_or(0.0))
            .collect();
        let sum: f64 = lens.iter().sum();
        let fades: f64 = lens
            .windows(2)
            .map(|w| self.crossfade_between(w[0], w[1]))
            .sum();
        sum - fades
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
        // 3 s between 10 and 20, then at most half of 4 s = 2 s.
        assert!((show.total_seconds_from(0) - (34.0 - 3.0 - 2.0)).abs() < 1e-9);
        assert!((show.total_seconds_from(1) - 22.0).abs() < 1e-9);
        assert!(show.move_entry(2, 0));
        assert_eq!(show.playlist[0].seconds, Some(4.0));
        assert!(!show.move_entry(5, 0));
    }
}
