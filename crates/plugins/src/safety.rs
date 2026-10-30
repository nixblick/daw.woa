//! Sperrliste und Logs je Plugin (Paket 34).
//!
//! **Sperrliste:** Eine Plugin-Datei, die beim Suchen abstürzt oder hängt, wird gesperrt und beim
//! nächsten Suchen nicht mehr geladen (sonst kostet sie bei jedem Start 20 Sekunden oder
//! mehr). Der Nutzer kann eine Datei auch selbst sperren („Sperren“ beim abgestürzten Platz) und
//! sie wieder freigeben; „Neu suchen“ prüft gesperrte Dateien nicht, „Freigeben“ schon beim
//! nächsten Suchen. Die Idee (Ergebnis je Plugin merken: neu, in Ordnung, Fehler,
//! Zeitüberschreitung, gesperrt; Sperre löschbar) stammt aus dem Plugin-Verwalter von Ardour
//! (`libs/ardour/plugin_manager.cc`, GPL-2.0-or-later, `docs/herkunft.md`). Kein Code
//! übernommen.
//!
//! **Logs:** Je Plugin eine Datei `<Kennung>.log` im Log-Ordner: Start, Ausfall (mit Grund und
//! verspäteten Blöcken), Neuladen, dazu alles, was der Plugin-Prozess selbst auf `stderr`
//! schreibt. `scan.log` hält die Ergebnisse der Suche. Geschrieben wird nur im Control-Thread
//! oder in Hilfsthreads, nie im Audio-Thread. Jede Datei wird beim Öffnen auf
//! [`MAX_LOG_BYTES`] begrenzt (die alte Hälfte wandert nach `.1`).

use std::io::Write;
use std::path::{Path, PathBuf};

/// Ab dieser Größe wird eine Log-Datei beim nächsten Öffnen weggeschoben.
pub const MAX_LOG_BYTES: u64 = 512 * 1024;
/// Höchste Zahl gesperrter Dateien (Schutz vor einer wuchernden Datei).
pub const MAX_BLOCKED: usize = 1000;

/// Eine gesperrte Plugin-Datei.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockEntry {
    /// Die Plugin-Datei.
    pub file: PathBuf,
    /// Warum (ein Satz, ohne Tabulator und Zeilenumbruch).
    pub reason: String,
}

/// Die Liste der gesperrten Plugin-Dateien, gespeichert als Textdatei (`Datei<TAB>Grund`).
#[derive(Debug, Clone, Default)]
pub struct Blocklist {
    entries: Vec<BlockEntry>,
    path: Option<PathBuf>,
}

fn clean(s: &str) -> String {
    s.replace(['\t', '\n', '\r'], " ")
}

impl Blocklist {
    /// Eine leere Liste ohne Datei (nichts wird gespeichert).
    pub fn in_memory() -> Self {
        Self::default()
    }

    /// Liest die Liste aus `path`. Fehlt die Datei, ist die Liste leer; jede Änderung
    /// wird dorthin geschrieben. Zeilen ohne Tabulator werden übergangen.
    pub fn load(path: &Path) -> Self {
        let mut entries = Vec::new();
        match std::fs::read_to_string(path) {
            Ok(text) => {
                for line in text.lines().take(MAX_BLOCKED) {
                    let mut f = line.splitn(2, '\t');
                    if let (Some(file), Some(reason)) = (f.next(), f.next())
                        && !file.is_empty()
                    {
                        entries.push(BlockEntry {
                            file: PathBuf::from(file),
                            reason: reason.to_string(),
                        });
                    }
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                tracing::warn!(file = %path.display(), error = %e, "plugin blocklist unreadable")
            }
        }
        Self {
            entries,
            path: Some(path.to_path_buf()),
        }
    }

    /// Wo die Liste liegt: `plugin-blocklist.tsv` im Konfigurationsordner.
    pub fn default_path(config_dir: &Path) -> PathBuf {
        config_dir.join("plugin-blocklist.tsv")
    }

    /// Alle Einträge.
    pub fn entries(&self) -> &[BlockEntry] {
        &self.entries
    }

    /// Ob diese Datei gesperrt ist.
    pub fn is_blocked(&self, file: &Path) -> bool {
        self.entries.iter().any(|e| e.file == file)
    }

    /// Der Grund einer Sperre.
    pub fn reason(&self, file: &Path) -> Option<&str> {
        self.entries
            .iter()
            .find(|e| e.file == file)
            .map(|e| e.reason.as_str())
    }

    /// Sperrt eine Datei (hat sie schon eine Sperre, bleibt der erste Grund). Liefert `true`,
    /// wenn neu gesperrt wurde.
    pub fn block(&mut self, file: &Path, reason: &str) -> bool {
        if self.is_blocked(file) || self.entries.len() >= MAX_BLOCKED {
            return false;
        }
        self.entries.push(BlockEntry {
            file: file.to_path_buf(),
            reason: clean(reason),
        });
        self.save();
        true
    }

    /// Gibt eine Datei frei. Liefert `true`, wenn sie gesperrt war.
    pub fn unblock(&mut self, file: &Path) -> bool {
        let before = self.entries.len();
        self.entries.retain(|e| e.file != file);
        let changed = self.entries.len() != before;
        if changed {
            self.save();
        }
        changed
    }

    /// Hebt alle Sperren auf.
    pub fn clear(&mut self) {
        if !self.entries.is_empty() {
            self.entries.clear();
            self.save();
        }
    }

    fn save(&self) {
        let Some(path) = &self.path else { return };
        let mut text = String::new();
        for e in &self.entries {
            text.push_str(&format!(
                "{}\t{}\n",
                clean(&e.file.to_string_lossy()),
                e.reason
            ));
        }
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let tmp = path.with_extension("tsv.tmp");
        let r = std::fs::write(&tmp, text).and_then(|()| std::fs::rename(&tmp, path));
        if let Err(e) = r {
            tracing::warn!(file = %path.display(), error = %e, "plugin blocklist not saved");
        }
    }
}

/// Der Log-Ordner für Plugins.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginLogs {
    dir: PathBuf,
}

/// Aus einer Kennung einen sicheren Dateinamen machen (keine Pfadzeichen, begrenzte Länge).
pub fn log_name(key: &str) -> String {
    let s: String = key
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .take(120)
        .collect();
    let s = s.trim_start_matches('.').to_string();
    if s.is_empty() { "plugin".into() } else { s }
}

impl PluginLogs {
    /// Logs unter `dir` (wird beim ersten Schreiben angelegt).
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    /// Der Ordner.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Die Log-Datei eines Plugins.
    pub fn path_for(&self, clap_id: &str) -> PathBuf {
        self.dir.join(format!("{}.log", log_name(clap_id)))
    }

    /// Die Log-Datei der Suche.
    pub fn scan_path(&self) -> PathBuf {
        self.dir.join("scan.log")
    }

    /// Öffnet die Datei zum Anhängen; ist sie zu groß, wandert sie nach `.1`.
    fn open(&self, path: &Path) -> std::io::Result<std::fs::File> {
        std::fs::create_dir_all(&self.dir)?;
        if std::fs::metadata(path).is_ok_and(|m| m.len() > MAX_LOG_BYTES) {
            let old = path.with_extension("log.1");
            let _ = std::fs::rename(path, old);
        }
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
    }

    /// Hängt eine Zeile an das Log eines Plugins. Fehler werden nur gemeldet, nie weitergegeben.
    pub fn line(&self, clap_id: &str, text: &str) {
        self.write(&self.path_for(clap_id), text);
    }

    /// Hängt eine Zeile an das Suchlog.
    pub fn scan_line(&self, text: &str) {
        self.write(&self.scan_path(), text);
    }

    fn write(&self, path: &Path, text: &str) {
        match self.open(path) {
            Ok(mut f) => {
                let _ = writeln!(f, "{} {}", utc_now(), clean(text));
            }
            Err(e) => tracing::warn!(file = %path.display(), error = %e, "plugin log not writable"),
        }
    }

    /// Die Datei, in die der Plugin-Prozess seine Ausgabe schreibt (`stderr`).
    pub fn child_output(&self, clap_id: &str) -> Option<std::fs::File> {
        self.open(&self.path_for(clap_id)).ok()
    }

    /// Die letzten `n` Zeilen des Logs eines Plugins (für die Oberfläche).
    pub fn tail(&self, clap_id: &str, n: usize) -> Vec<String> {
        use std::io::{Read, Seek, SeekFrom};
        // Nur das Ende lesen (höchstens 16 KiB), die Oberfläche ruft es jedes Bild.
        const TAIL_BYTES: u64 = 16 * 1024;
        let Ok(mut f) = std::fs::File::open(self.path_for(clap_id)) else {
            return Vec::new();
        };
        let len = f.metadata().map_or(0, |m| m.len());
        let start = len.saturating_sub(TAIL_BYTES);
        let mut bytes = Vec::new();
        if f.seek(SeekFrom::Start(start)).is_err()
            || f.take(TAIL_BYTES).read_to_end(&mut bytes).is_err()
        {
            return Vec::new();
        }
        let text = String::from_utf8_lossy(&bytes);
        let mut lines: Vec<&str> = text.lines().collect();
        if start > 0 && !lines.is_empty() {
            lines.remove(0); // angeschnittene erste Zeile
        }
        let from = lines.len().saturating_sub(n);
        lines[from..].iter().map(|s| (*s).to_string()).collect()
    }
}

/// Jetzt als `2026-10-30T15:04:05Z` (UTC).
fn utc_now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    utc_text(secs)
}

/// Sekunden seit 1970 als `JJJJ-MM-TTThh:mm:ssZ` (bürgerlicher Kalender, Verfahren von Howard
/// Hinnant, `days_from_civil` rückwärts).
pub fn utc_text(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_text_is_right() {
        assert_eq!(utc_text(0), "1970-01-01T00:00:00Z");
        assert_eq!(utc_text(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(utc_text(1_793_372_645), "2026-10-30T15:04:05Z");
    }

    #[test]
    fn blocklist_roundtrip_and_unblock() {
        let d = tempfile::tempdir().unwrap();
        let p = Blocklist::default_path(d.path());
        let mut b = Blocklist::load(&p);
        assert!(b.entries().is_empty());
        assert!(b.block(Path::new("/x/a.clap"), "abgestürzt\tmit Tab"));
        assert!(!b.block(Path::new("/x/a.clap"), "zweiter Grund"));
        assert!(b.block(Path::new("/x/b.clap"), "hängt"));
        let b2 = Blocklist::load(&p);
        assert_eq!(b2.entries().len(), 2);
        assert!(b2.is_blocked(Path::new("/x/a.clap")));
        assert_eq!(
            b2.reason(Path::new("/x/a.clap")),
            Some("abgestürzt mit Tab")
        );
        let mut b2 = b2;
        assert!(b2.unblock(Path::new("/x/a.clap")));
        assert!(!b2.unblock(Path::new("/x/a.clap")));
        assert_eq!(Blocklist::load(&p).entries().len(), 1);
        b2.clear();
        assert!(Blocklist::load(&p).entries().is_empty());
    }

    #[test]
    fn broken_blocklist_lines_are_ignored() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("l.tsv");
        std::fs::write(&p, "ohne tab\n\n/ok.clap\tgrund\n\tleer\n").unwrap();
        let b = Blocklist::load(&p);
        assert_eq!(b.entries().len(), 1);
    }

    #[test]
    fn log_names_are_safe() {
        assert_eq!(log_name("org.surge/../x y"), "org.surge_.._x_y");
        assert_eq!(log_name("../../etc"), "_.._etc");
        assert_eq!(log_name(""), "plugin");
        assert!(log_name(&"a".repeat(500)).len() <= 120);
    }

    #[test]
    fn logs_append_rotate_and_tail() {
        let d = tempfile::tempdir().unwrap();
        let logs = PluginLogs::new(d.path().join("plugins"));
        logs.line("de.x", "eins");
        logs.line("de.x", "zwei\nmit Umbruch");
        let t = logs.tail("de.x", 10);
        assert_eq!(t.len(), 2);
        assert!(t[1].ends_with("zwei mit Umbruch"), "{t:?}");
        assert_eq!(logs.tail("de.x", 1).len(), 1);
        assert!(logs.tail("fehlt", 5).is_empty());
        // Zu groß: beim nächsten Schreiben wandert die Datei nach .1.
        let big = vec![b'x'; (MAX_LOG_BYTES + 10) as usize];
        std::fs::write(logs.path_for("de.x"), big).unwrap();
        logs.line("de.x", "neu");
        assert_eq!(logs.tail("de.x", 10).len(), 1);
        assert!(logs.dir().join("de.x.log.1").exists());
        logs.scan_line("a.clap: ok");
        assert!(logs.scan_path().exists());
    }
}
