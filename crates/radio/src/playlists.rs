//! Mehrere benannte Playlisten einer Sendung (neu, umbenennen, duplizieren, löschen, leeren,
//! wechseln) und M3U/M3U8-Import und -Export.
//!
//! Die aktive Playlist liegt wie bisher in [`RadioShow::playlist`]; alle anderen Stellen
//! (Mixer, AutoDJ, Voice-Tracking, Agent-Werkzeuge) sehen nur sie. [`RadioShow::playlists`]
//! hält die Namen in fester Reihenfolge, die Titel der nicht aktiven Listen und für die
//! aktive nur den Namen. Was gerade läuft, entscheidet der Controller
//! ([`crate::Controller::select_playlist`]); hier steht nur das Datenmodell.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::show::{PlaylistEntry, RadioShow};

/// Name der ersten Playlist (auch für alte Sendungen mit nur einer Liste).
pub const FIRST_NAME: &str = "Playlist 1";

/// Eine benannte Playlist.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NamedPlaylist {
    pub name: String,
    /// Titel. Leer für die aktive Playlist, deren Titel in [`RadioShow::playlist`] stehen.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entries: Vec<PlaylistEntry>,
}

impl NamedPlaylist {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            entries: Vec::new(),
        }
    }
}

/// Eine gelöschte Playlist, zum Zurückholen.
#[derive(Debug, Clone, PartialEq)]
pub struct Deleted {
    pub index: usize,
    pub playlist: NamedPlaylist,
    /// War sie die aktive?
    pub was_active: bool,
    /// Sie war die letzte; an ihrer Stelle steht eine leere Ersatzliste.
    pub replaced: bool,
}

impl RadioShow {
    /// Hält `playlists` und `active` stimmig (alte Dateien: „Playlist 1“).
    pub(crate) fn sanitize_playlists(&mut self) {
        if self.playlists.is_empty() {
            self.playlists.push(NamedPlaylist::new(FIRST_NAME));
        }
        if self.active >= self.playlists.len() {
            self.active = 0;
        }
        for (i, p) in self.playlists.iter_mut().enumerate() {
            if p.name.trim().is_empty() {
                p.name = format!("Playlist {}", i + 1);
            }
        }
        // Titel der aktiven Liste gehören nach `playlist`.
        let stray = std::mem::take(&mut self.playlists[self.active].entries);
        if self.playlist.is_empty() {
            self.playlist = stray;
        }
    }

    /// Namen aller Playlisten in ihrer Reihenfolge.
    pub fn playlist_names(&self) -> Vec<String> {
        self.playlists.iter().map(|p| p.name.clone()).collect()
    }

    /// Name der aktiven Playlist.
    pub fn active_name(&self) -> &str {
        self.playlists
            .get(self.active)
            .map_or(FIRST_NAME, |p| p.name.as_str())
    }

    /// Titel von Playlist `i` (die aktive aus `playlist`).
    pub fn playlist_entries(&self, i: usize) -> Option<&[PlaylistEntry]> {
        if i == self.active {
            return Some(&self.playlist);
        }
        self.playlists.get(i).map(|p| p.entries.as_slice())
    }

    /// Veränderliche Titel von Playlist `i`.
    pub fn playlist_entries_mut(&mut self, i: usize) -> Option<&mut Vec<PlaylistEntry>> {
        if i == self.active {
            return Some(&mut self.playlist);
        }
        self.playlists.get_mut(i).map(|p| &mut p.entries)
    }

    /// Ein freier Name: `base`, sonst `base 2`, `base 3` …
    pub fn unique_name(&self, base: &str) -> String {
        let base = base.trim();
        let base = if base.is_empty() { "Playlist" } else { base };
        let taken = |n: &str| self.playlists.iter().any(|p| p.name == n);
        if !taken(base) {
            return base.to_string();
        }
        (2..)
            .map(|k| format!("{base} {k}"))
            .find(|n| !taken(n))
            .unwrap_or_default()
    }

    /// Legt eine leere Playlist an (ohne zu wechseln). Gibt ihren Index zurück.
    pub fn new_playlist(&mut self, name: &str) -> usize {
        let base = if name.trim().is_empty() {
            format!("Playlist {}", self.playlists.len() + 1)
        } else {
            name.to_string()
        };
        let name = self.unique_name(&base);
        self.playlists.push(NamedPlaylist::new(name));
        self.playlists.len() - 1
    }

    /// Legt eine Playlist mit Titeln an (etwa aus M3U). Gibt ihren Index zurück.
    pub fn add_playlist(&mut self, name: &str, entries: Vec<PlaylistEntry>) -> usize {
        let i = self.new_playlist(name);
        self.playlists[i].entries = entries;
        i
    }

    /// Benennt Playlist `i` um. Leere oder vergebene Namen werden abgelehnt.
    pub fn rename_playlist(&mut self, i: usize, name: &str) -> Result<(), String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("Der Name darf nicht leer sein".into());
        }
        if i >= self.playlists.len() {
            return Err(format!("Playlist {} gibt es nicht", i + 1));
        }
        if self
            .playlists
            .iter()
            .enumerate()
            .any(|(k, p)| k != i && p.name == name)
        {
            return Err(format!("Eine Playlist „{name}“ gibt es schon"));
        }
        self.playlists[i].name = name.to_string();
        Ok(())
    }

    /// Kopiert Playlist `i` direkt dahinter. Gibt den Index der Kopie zurück.
    pub fn duplicate_playlist(&mut self, i: usize) -> Option<usize> {
        let entries = self.playlist_entries(i)?.to_vec();
        let name = self.unique_name(&format!("{} (Kopie)", self.playlists[i].name));
        let at = i + 1;
        self.playlists.insert(
            at,
            NamedPlaylist {
                name,
                entries: entries
                    .into_iter()
                    .map(|mut e| {
                        e.uid = 0;
                        e
                    })
                    .collect(),
            },
        );
        if self.active >= at {
            self.active += 1;
        }
        Some(at)
    }

    /// Leert Playlist `i`. Gibt die alten Titel zurück.
    pub fn clear_playlist(&mut self, i: usize) -> Option<Vec<PlaylistEntry>> {
        self.playlist_entries_mut(i).map(std::mem::take)
    }

    /// Macht Playlist `i` zur aktiven: ihre Titel wandern nach `playlist`, die der bisherigen
    /// zurück in ihr Feld. Die Laufzeit-Kennungen vergibt der Controller.
    pub fn switch_playlist(&mut self, i: usize) -> bool {
        if i >= self.playlists.len() {
            return false;
        }
        if i == self.active {
            return true;
        }
        let old = std::mem::take(&mut self.playlist);
        self.playlists[self.active].entries = old;
        self.playlist = std::mem::take(&mut self.playlists[i].entries);
        self.active = i;
        true
    }

    /// Löscht Playlist `i`. Ist sie die aktive, wird die Nachbarin aktiv; war es die
    /// letzte, bleibt eine leere „Playlist 1“. Zum Zurückholen: [`Self::restore_playlist`].
    pub fn delete_playlist(&mut self, i: usize) -> Option<Deleted> {
        if i >= self.playlists.len() {
            return None;
        }
        let was_active = i == self.active;
        if was_active {
            self.playlists[i].entries = std::mem::take(&mut self.playlist);
        }
        let playlist = self.playlists.remove(i);
        let replaced = self.playlists.is_empty();
        if replaced {
            self.playlists.push(NamedPlaylist::new(FIRST_NAME));
            self.active = 0;
        } else if was_active {
            let next = i.min(self.playlists.len() - 1);
            self.playlist = std::mem::take(&mut self.playlists[next].entries);
            self.active = next;
        } else if self.active > i {
            self.active -= 1;
        }
        Some(Deleted {
            index: i,
            playlist,
            was_active,
            replaced,
        })
    }

    /// Holt eine gelöschte Playlist an ihre Stelle zurück (wieder aktiv, wenn sie es war).
    /// Ein inzwischen vergebener Name bekommt eine Nummer.
    pub fn restore_playlist(&mut self, d: Deleted) -> usize {
        // Die leere Ersatzliste nach dem Löschen der letzten fällt wieder weg.
        if d.replaced
            && self.playlists.len() == 1
            && self.playlist.is_empty()
            && self.playlists[0].name == FIRST_NAME
        {
            self.playlists.clear();
            self.active = 0;
        }
        let mut p = d.playlist;
        p.name = self.unique_name(&p.name);
        let at = d.index.min(self.playlists.len());
        self.playlists.insert(at, p);
        if self.playlists.len() == 1 {
            self.active = 0;
            self.playlist = std::mem::take(&mut self.playlists[0].entries);
        } else {
            if self.active >= at {
                self.active += 1;
            }
            if d.was_active {
                self.switch_playlist(at);
            }
        }
        at
    }
}

// ---- M3U / M3U8 ------------------------------------------------------------------------

/// Liest eine M3U/M3U8-Liste: Zeilen mit `#` sind Kommentare, relative Pfade gelten ab dem
/// Ordner der Datei, `file://`-Adressen werden zu Pfaden. Nur lokale Dateien.
pub fn parse_m3u(text: &str, base: &Path) -> Vec<PathBuf> {
    text.trim_start_matches('\u{feff}')
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter(|l| !l.contains("://") || l.starts_with("file://"))
        .map(|l| {
            let l = l.strip_prefix("file://").unwrap_or(l);
            let p = PathBuf::from(l.replace('\\', std::path::MAIN_SEPARATOR_STR));
            if p.is_absolute() { p } else { base.join(p) }
        })
        .collect()
}

/// Liest `path` als M3U/M3U8. Gibt Name (Dateiname ohne Endung) und Titel zurück.
pub fn import_m3u(path: &Path) -> Result<(String, Vec<PlaylistEntry>), String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let text = String::from_utf8_lossy(&bytes);
    let base = path.parent().unwrap_or(Path::new("."));
    let entries: Vec<PlaylistEntry> = parse_m3u(&text, base)
        .into_iter()
        .map(PlaylistEntry::new)
        .collect();
    let name = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Import".into());
    Ok((name, entries))
}

/// M3U8-Text (UTF-8, absolute Pfade) mit `#EXTINF` für Länge und Titel.
pub fn m3u_text(entries: &[PlaylistEntry]) -> String {
    let mut out = String::from("#EXTM3U\n");
    for e in entries {
        let secs = e.seconds.map_or(-1, |s| s.round() as i64);
        out.push_str(&format!("#EXTINF:{secs},{}\n", e.title()));
        out.push_str(&e.path.to_string_lossy());
        out.push('\n');
    }
    out
}

/// Schreibt `entries` als M3U8 nach `path`.
pub fn export_m3u(path: &Path, entries: &[PlaylistEntry]) -> Result<(), String> {
    std::fs::write(path, m3u_text(entries)).map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn show_with(names: &[&str]) -> RadioShow {
        let mut s = RadioShow::default();
        s.playlists.clear();
        for n in names {
            s.playlists.push(NamedPlaylist::new(*n));
        }
        s.active = 0;
        s
    }

    #[test]
    fn old_file_becomes_playlist_1() {
        let json = r#"{"version":1,"playlist":[{"path":"/m/a.wav"},{"path":"/m/b.wav"}]}"#;
        let s = RadioShow::from_json(json).unwrap();
        assert_eq!(s.playlist_names(), vec![FIRST_NAME]);
        assert_eq!(s.active, 0);
        assert_eq!(s.playlist.len(), 2);
        assert_eq!(s.playlist_entries(0).unwrap().len(), 2);
    }

    #[test]
    fn new_rename_and_unique_names() {
        let mut s = RadioShow::default();
        let i = s.new_playlist("");
        assert_eq!(s.playlists[i].name, "Playlist 2");
        let j = s.new_playlist("Morgen");
        let k = s.new_playlist("Morgen");
        assert_eq!(s.playlists[k].name, "Morgen 2");
        assert!(s.rename_playlist(k, "Morgen").is_err());
        assert!(s.rename_playlist(k, "  ").is_err());
        s.rename_playlist(k, "Abend").unwrap();
        assert_eq!(
            s.playlist_names(),
            vec!["Playlist 1", "Playlist 2", "Morgen", "Abend"]
        );
        assert_eq!(j, 2);
    }

    #[test]
    fn switch_keeps_entries_and_roundtrips() {
        let mut s = show_with(&["A", "B"]);
        s.playlist.push(PlaylistEntry::new("/m/a.wav"));
        s.playlists[1].entries.push(PlaylistEntry::new("/m/b.wav"));
        assert!(s.switch_playlist(1));
        assert_eq!(s.active_name(), "B");
        assert_eq!(s.playlist[0].path, PathBuf::from("/m/b.wav"));
        assert_eq!(s.playlists[0].entries[0].path, PathBuf::from("/m/a.wav"));
        let back = RadioShow::from_json(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn duplicate_and_clear() {
        let mut s = show_with(&["A"]);
        s.playlist.push(PlaylistEntry::new("/m/a.wav"));
        let d = s.duplicate_playlist(0).unwrap();
        assert_eq!(s.playlists[d].name, "A (Kopie)");
        assert_eq!(s.playlist_entries(d).unwrap().len(), 1);
        assert_eq!(s.clear_playlist(d).unwrap().len(), 1);
        assert!(s.playlist_entries(d).unwrap().is_empty());
        assert_eq!(s.playlist.len(), 1);
    }

    #[test]
    fn delete_and_undo() {
        let mut s = show_with(&["A", "B", "C"]);
        s.playlists[1].entries.push(PlaylistEntry::new("/m/b.wav"));
        let d = s.delete_playlist(1).unwrap();
        assert_eq!(s.playlist_names(), vec!["A", "C"]);
        s.restore_playlist(d);
        assert_eq!(s.playlist_names(), vec!["A", "B", "C"]);
        assert_eq!(s.playlist_entries(1).unwrap().len(), 1);
        assert_eq!(s.active, 0);
    }

    #[test]
    fn delete_active_switches_and_undo_restores() {
        let mut s = show_with(&["A", "B"]);
        s.playlist.push(PlaylistEntry::new("/m/a.wav"));
        s.playlists[1].entries.push(PlaylistEntry::new("/m/b.wav"));
        let d = s.delete_playlist(0).unwrap();
        assert!(d.was_active);
        assert_eq!(s.active_name(), "B");
        assert_eq!(s.playlist[0].path, PathBuf::from("/m/b.wav"));
        s.restore_playlist(d);
        assert_eq!(s.active_name(), "A");
        assert_eq!(s.playlist[0].path, PathBuf::from("/m/a.wav"));
        assert_eq!(s.playlist_entries(1).unwrap().len(), 1);
    }

    #[test]
    fn delete_last_leaves_empty_and_undo_brings_it_back() {
        let mut s = show_with(&["Nur"]);
        s.playlist.push(PlaylistEntry::new("/m/a.wav"));
        let d = s.delete_playlist(0).unwrap();
        assert_eq!(s.playlist_names(), vec![FIRST_NAME]);
        assert!(s.playlist.is_empty());
        s.restore_playlist(d);
        assert_eq!(s.playlist_names(), vec!["Nur"]);
        assert_eq!(s.playlist.len(), 1);
    }

    #[test]
    fn undo_keeps_a_real_empty_playlist_1() {
        let mut s = RadioShow::default();
        let b = s.new_playlist("B");
        let d = s.delete_playlist(b).unwrap();
        s.restore_playlist(d);
        assert_eq!(s.playlist_names(), vec![FIRST_NAME, "B"]);
    }

    #[test]
    fn m3u_roundtrip_and_relative_paths() {
        let tmp = tempfile::tempdir().unwrap();
        let mut a = PlaylistEntry::new(tmp.path().join("a b.mp3"));
        a.seconds = Some(61.4);
        let p = tmp.path().join("Morgen.m3u8");
        export_m3u(&p, &[a.clone()]).unwrap();
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(text.starts_with("#EXTM3U\n#EXTINF:61,a b\n"));
        let (name, back) = import_m3u(&p).unwrap();
        assert_eq!(name, "Morgen");
        assert_eq!(back[0].path, a.path);
        let rel = parse_m3u(
            "\u{feff}#EXTM3U\nsub/x.wav\n\nhttp://radio/stream\n",
            Path::new("/m"),
        );
        assert_eq!(rel, vec![PathBuf::from("/m/sub/x.wav")]);
    }
}
