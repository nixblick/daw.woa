//! Zustand von `woa-streamd` als JSON im Datenordner (`streamd-status.json`). Die
//! Oberfläche und das Agent-Werkzeug `stream_status` lesen ihn; ein alter Zeitstempel
//! heißt: woa-streamd läuft nicht.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const STATUS_FILE: &str = "streamd-status.json";
/// Älter als das: woa-streamd gilt als beendet.
pub const STALE_SECONDS: u64 = 5;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StreamStatus {
    /// Sekunden seit 1970, zuletzt geschrieben.
    pub updated_unix: u64,
    pub pid: u32,
    /// `host:port (Stream n)`.
    pub server: String,
    /// `verbinde`, `verbunden`, `verbinde neu`, `beendet`.
    pub connection: String,
    pub connected: bool,
    /// Was gerade gesendet wird: `Sendemix` oder `Notfall-Playlist`.
    pub source: String,
    pub emergency: bool,
    /// Oberfläche verbunden (lokale Verbindung steht).
    pub ui_connected: bool,
    pub listeners: Option<u32>,
    pub bitrate_kbps: u32,
    pub sample_rate: u32,
    /// Seit wann die Server-Verbindung steht (Sekunden).
    pub connected_seconds: u64,
    /// Seit wann woa-streamd sendet (Sekunden).
    pub on_air_seconds: u64,
    pub title: String,
    pub last_error: Option<String>,
    pub reconnects: u32,
    /// Sekunden bis zum nächsten Verbindungsversuch, solange keine Verbindung steht.
    pub retry_in_seconds: Option<u64>,
    pub bytes_sent: u64,
    pub bytes_dropped: u64,
    /// So oft war bei verbundener Oberfläche kein Sendemix im Puffer (Unterlauf).
    pub live_underruns: u32,
}

impl StreamStatus {
    /// Frisch geschrieben und nicht beendet.
    pub fn is_running(&self, now_unix: u64) -> bool {
        self.connection != "beendet" && now_unix.saturating_sub(self.updated_unix) <= STALE_SECONDS
    }

    /// Verständliche Fehlerzeile, z. B. „Server nicht erreichbar (Port 8001 zu?), neuer
    /// Versuch in 30 s“. `None`, solange alles läuft.
    pub fn problem(&self) -> Option<String> {
        let err = self.last_error.as_deref()?;
        if self.connected {
            return None;
        }
        let mut s = if err.starts_with("Server nicht erreichbar") {
            let port = self.server.rsplit_once(':').map_or("", |(_, p)| p);
            let port = port.split_whitespace().next().unwrap_or("");
            format!("Server nicht erreichbar (Port {port} zu?)")
        } else {
            err.to_string()
        };
        if let Some(n) = self.retry_in_seconds {
            s.push_str(&format!(", neuer Versuch in {n} s"));
        }
        Some(s)
    }

    /// Eine Zeile für die Oberfläche.
    pub fn summary(&self) -> String {
        let mut s = format!("{} · {}", self.connection, self.source);
        if let Some(n) = self.listeners {
            s.push_str(&format!(" · {n} Hörer"));
        }
        s.push_str(&format!(
            " · {} kbit/s · {}",
            self.bitrate_kbps,
            clock(self.on_air_seconds)
        ));
        s
    }
}

/// `h:mm:ss`.
pub fn clock(seconds: u64) -> String {
    format!(
        "{}:{:02}:{:02}",
        seconds / 3600,
        seconds % 3600 / 60,
        seconds % 60
    )
}

pub fn status_path(data_dir: &Path) -> PathBuf {
    data_dir.join(STATUS_FILE)
}

/// Schreibt über eine Zwischendatei (der Leser sieht nie eine halbe Datei).
pub fn write(path: &Path, status: &StreamStatus) -> std::io::Result<()> {
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d)?;
    }
    let tmp = path.with_extension("json.tmp");
    let text = serde_json::to_string_pretty(status).map_err(std::io::Error::other)?;
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)
}

/// Liest den Zustand; `None`, wenn keiner da ist.
pub fn read(path: &Path) -> Option<StreamStatus> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_staleness() {
        let tmp = tempfile::tempdir().unwrap();
        let p = status_path(tmp.path());
        assert!(read(&p).is_none());
        let s = StreamStatus {
            updated_unix: 100,
            connection: "verbunden".into(),
            source: "Sendemix".into(),
            listeners: Some(3),
            bitrate_kbps: 128,
            on_air_seconds: 3725,
            ..StreamStatus::default()
        };
        write(&p, &s).unwrap();
        let r = read(&p).unwrap();
        assert_eq!(r, s);
        assert!(r.is_running(104));
        assert!(!r.is_running(200));
        assert_eq!(
            r.summary(),
            "verbunden · Sendemix · 3 Hörer · 128 kbit/s · 1:02:05"
        );
        assert_eq!(r.problem(), None);
    }

    #[test]
    fn problem_is_readable() {
        let s = StreamStatus {
            server: "stream.wironair.de:8001 (Stream 1)".into(),
            last_error: Some("Server nicht erreichbar: [::1]:8001: connection timed out".into()),
            retry_in_seconds: Some(30),
            ..StreamStatus::default()
        };
        assert_eq!(
            s.problem().as_deref(),
            Some("Server nicht erreichbar (Port 8001 zu?), neuer Versuch in 30 s")
        );
    }
}
