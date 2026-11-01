//! WOA-Ordner (#258): Pfad-Logik ohne Oberfläche, damit App und MCP (J6) dieselben Ordner
//! nutzen. Fenster und Dialoge liegen in `woa_gui::ordner`.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Name des Basisordners im Musikordner.
pub const WOA_DIR: &str = "WOA";

/// Was die App ablegt oder öffnet. Reihenfolge = Anzeige in den Einstellungen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Art {
    /// Projektordner (`.woa`), auch aus DAWproject geöffnete.
    Projekte,
    /// Sprachaufnahmen für Voice-Tracks und Testgäste.
    Aufnahmen,
    /// Mitschnitte der Sendesumme (REC), ohne Dialog.
    Mitschnitte,
    /// Mixdown, Spur-Export, Stems, DAWproject-Export.
    Exporte,
    /// Jingles und Einspieler für die Cart-Wall.
    Jingles,
    /// MIDI-Import und -Export.
    Midi,
    /// Playlists der Sendung (M3U).
    Sendungen,
}

impl Art {
    pub const ALL: [Art; 7] = [
        Art::Projekte,
        Art::Aufnahmen,
        Art::Mitschnitte,
        Art::Exporte,
        Art::Jingles,
        Art::Midi,
        Art::Sendungen,
    ];

    /// Unterordner im Basisordner, so benannt wie in der Oberfläche.
    pub fn folder(self) -> &'static str {
        match self {
            Art::Projekte => "Projekte",
            Art::Aufnahmen => "Aufnahmen",
            Art::Mitschnitte => "Mitschnitte",
            Art::Exporte => "Exporte",
            Art::Jingles => "Jingles",
            Art::Midi => "MIDI",
            Art::Sendungen => "Sendungen",
        }
    }
}

/// Abschnitt `ordner` in `config.json`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default)]
pub struct Ordner {
    /// Eigener Basisordner; `None` = `<Musik>/WOA`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub basis: Option<PathBuf>,
    /// Eigener Ordner je Art; fehlt = `<Basis>/<Unterordner>`.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub eigene: BTreeMap<Art, PathBuf>,
}

impl Ordner {
    pub fn is_default(&self) -> bool {
        *self == Ordner::default()
    }

    /// Basisordner bei gegebenem Musikordner.
    pub fn base_in(&self, music: &Path) -> PathBuf {
        self.basis
            .clone()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| music.join(WOA_DIR))
    }

    /// Ordner einer Art bei gegebenem Musikordner.
    pub fn dir_in(&self, art: Art, music: &Path) -> PathBuf {
        match self.eigene.get(&art).filter(|p| !p.as_os_str().is_empty()) {
            Some(p) => p.clone(),
            None => self.base_in(music).join(art.folder()),
        }
    }

    /// Legt alle Ordner an, die fehlen. Idempotent, nichts wird überschrieben oder verschoben;
    /// Fehler werden nur geloggt. Gibt die neu angelegten Ordner zurück.
    pub fn ensure_in(&self, music: &Path) -> Vec<PathBuf> {
        let mut made = Vec::new();
        for art in Art::ALL {
            let dir = self.dir_in(art, music);
            if dir.is_dir() {
                continue;
            }
            match std::fs::create_dir_all(&dir) {
                Ok(()) => made.push(dir),
                Err(e) => tracing::warn!(error = %e, dir = %dir.display(), "folder not created"),
            }
        }
        if !made.is_empty() {
            tracing::info!(count = made.len(), "woa folders created");
        }
        made
    }
}

/// Musikordner aus Home, dem Inhalt von `user-dirs.dirs` (XDG) und der Plattform.
/// Rückfall: unter Windows `<Home>\Music`, sonst `<Home>/Musik`.
pub fn music_dir_from(home: &Path, user_dirs: Option<&str>, windows: bool) -> PathBuf {
    if windows {
        return home.join("Music");
    }
    let xdg = user_dirs.and_then(|text| {
        text.lines().find_map(|line| {
            let v = line.trim().strip_prefix("XDG_MUSIC_DIR=")?;
            let v = v.trim().trim_matches('"');
            let p = match v.strip_prefix("$HOME") {
                Some(rest) => home.join(rest.trim_start_matches('/')),
                None if v.starts_with('/') => PathBuf::from(v),
                None => return None,
            };
            // `$HOME` allein heißt in XDG „abgeschaltet“.
            (p != home).then_some(p)
        })
    });
    xdg.unwrap_or_else(|| home.join("Musik"))
}

/// Musikordner dieses Rechners.
pub fn music_dir() -> PathBuf {
    let home = std::env::var_os("HOME")
        .filter(|_| !cfg!(windows))
        .or_else(|| std::env::var_os("USERPROFILE"))
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".config"));
    let user_dirs = std::fs::read_to_string(config.join("user-dirs.dirs")).ok();
    music_dir_from(&home, user_dirs.as_deref(), cfg!(windows))
}

/// Abschnitt `ordner` aus `config.json` der App; fehlt die Datei oder der Abschnitt, gilt die
/// Vorgabe. Damit schreibt der MCP-Weg in denselben Ordner wie die Oberfläche.
pub fn from_config(config_json: &Path) -> Ordner {
    #[derive(Deserialize, Default)]
    struct Section {
        #[serde(default)]
        ordner: Ordner,
    }
    std::fs::read_to_string(config_json)
        .ok()
        .and_then(|t| serde_json::from_str::<Section>(&t).ok())
        .map(|s| s.ordner)
        .unwrap_or_default()
}

/// Ordner einer Art nach `config.json` im Konfigurationsordner.
pub fn configured_dir(art: Art) -> PathBuf {
    from_config(&crate::config_dir().join("config.json")).dir_in(art, &music_dir())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_section_is_read_and_missing_means_default() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("config.json");
        assert!(from_config(&p).is_default());
        std::fs::write(
            &p,
            r#"{"mode":"radio","ordner":{"eigene":{"jingles":"/j"}}}"#,
        )
        .unwrap();
        let o = from_config(&p);
        assert_eq!(o.dir_in(Art::Jingles, Path::new("/m")), PathBuf::from("/j"));
        std::fs::write(&p, "kaputt").unwrap();
        assert!(from_config(&p).is_default());
    }
}
