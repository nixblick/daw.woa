//! Einstellungen der Sendung. Liegen als Schlüssel `stream` in `config.json` (neben den
//! Einstellungen der Oberfläche); `woa-streamd` liest nur diesen Schlüssel.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Vorgabe-Pfad der Kennwortdatei: `~/.secrets/daw.woa/shoutcast-source.pass`.
pub fn default_password_file() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_default()
        .join(".secrets")
        .join("daw.woa")
        .join("shoutcast-source.pass")
}

/// Eigener Shoutcast auf lfs001 (bretteins.infra#150). Hören: `https://stream.wironair.de/stream`.
pub const DEFAULT_HOST: &str = "stream.wironair.de";

/// Quellprotokoll des Servers.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Protocol {
    /// Shoutcast 1 (Legacy-Quelle), auch DNAS v2.
    #[default]
    Shoutcast,
    /// Icecast 2 (HTTP `PUT` mit Basic-Auth, Benutzer `source`).
    Icecast,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StreamConfig {
    /// Shoutcast (Vorgabe) oder Icecast.
    pub protocol: Protocol,
    /// Icecast-Mountpoint, z. B. `/stream`. Bei Shoutcast ohne Bedeutung.
    pub mount: String,
    /// Shoutcast-Server (Hostname oder IP). Leer = nicht eingerichtet.
    pub host: String,
    /// Quell-Port (Shoutcast-1-Protokoll, beim DNAS `portbase + 1`).
    pub port: u16,
    /// Admin-Port (`portbase`) für Titel (`admin.cgi?mode=updinfo`) und Hörerzahl
    /// (`/7.html`). `None` = nicht erreichbar: dann keine Titel und keine Hörerzahl.
    pub admin_port: Option<u16>,
    /// Stream-ID im DNAS (`streamid`), meist 1.
    pub stream_id: u32,
    /// MP3-Bitrate (CBR) in kbit/s.
    pub bitrate_kbps: u32,
    /// Abtastrate des Streams: 44100 oder 48000.
    pub sample_rate: u32,
    /// Datei mit dem Quell-Kennwort (eine Zeile). Das Kennwort selbst steht nie hier.
    pub password_file: PathBuf,
    /// Name des Senders (`icy-name`).
    pub name: String,
    /// Notfall-Playlist: Dateien oder Ordner, der Reihe nach, endlos.
    pub emergency: Vec<PathBuf>,
    /// Nach so vielen Sekunden ohne Ton von der Oberfläche übernimmt die Notfall-Playlist.
    pub silence_seconds: f64,
}

impl Default for StreamConfig {
    fn default() -> Self {
        Self {
            protocol: Protocol::Shoutcast,
            mount: "/stream".to_string(),
            host: DEFAULT_HOST.to_string(),
            port: 8001,
            admin_port: None,
            stream_id: 1,
            bitrate_kbps: 128,
            sample_rate: 44_100,
            password_file: default_password_file(),
            name: "WOA – Wir on Air".to_string(),
            emergency: Vec::new(),
            silence_seconds: 2.0,
        }
    }
}

impl StreamConfig {
    /// Prüft die Werte; Fehlertext auf Deutsch.
    pub fn validate(&self) -> Result<(), String> {
        if self.host.trim().is_empty() {
            return Err("Kein Shoutcast-Server eingetragen (stream.host)".into());
        }
        if self.port == 0 {
            return Err(format!("Port {} ungültig", self.port));
        }
        if self.protocol == Protocol::Icecast
            && (!self.mount.starts_with('/')
                || self.mount.len() > 200
                || self
                    .mount
                    .chars()
                    .any(|c| c.is_whitespace() || c.is_control() || c == '?' || c == '#'))
        {
            return Err(format!(
                "Mountpoint {:?} ungültig (beginnt mit /, ohne Leerzeichen)",
                self.mount
            ));
        }
        if self.stream_id == 0 {
            return Err("Stream-ID muss ab 1 zählen".into());
        }
        if !matches!(self.sample_rate, 44_100 | 48_000) {
            return Err(format!(
                "Abtastrate {} nicht möglich (44100 oder 48000)",
                self.sample_rate
            ));
        }
        crate::mp3::bitrate(self.bitrate_kbps)?;
        Ok(())
    }

    /// `host:port (sid n)` für Anzeige und Log, ohne Kennwort.
    pub fn label(&self) -> String {
        match self.protocol {
            Protocol::Shoutcast => {
                format!("{}:{} (Stream {})", self.host, self.port, self.stream_id)
            }
            Protocol::Icecast => format!("{}:{}{} (Icecast)", self.host, self.port, self.mount),
        }
    }

    /// Erste Zeile der Anmeldung: Kennwort, ab Stream-ID 2 mit `:#sid`.
    pub(crate) fn login_line(&self, kw: &str) -> String {
        if self.stream_id <= 1 {
            format!("{kw}\r\n")
        } else {
            format!("{kw}:#{}\r\n", self.stream_id)
        }
    }

    /// Liest den Schlüssel `stream` aus einer `config.json`. Fehlt die Datei oder der
    /// Schlüssel, gelten die Vorgaben.
    pub fn load_from(config_json: &Path) -> Result<Self, String> {
        #[derive(Deserialize, Default)]
        struct File {
            #[serde(default)]
            stream: StreamConfig,
        }
        let text = match std::fs::read_to_string(config_json) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(format!("{}: {e}", config_json.display())),
        };
        serde_json::from_str::<File>(&text)
            .map(|f| f.stream)
            .map_err(|e| format!("{}: {e}", config_json.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_stream_key_and_validates() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("config.json");
        assert_eq!(
            StreamConfig::load_from(&p).unwrap(),
            StreamConfig::default()
        );
        std::fs::write(
            &p,
            r#"{"mode":"radio","stream":{"host":"radio.example","port":8010,"stream_id":2}}"#,
        )
        .unwrap();
        let c = StreamConfig::load_from(&p).unwrap();
        assert_eq!(c.port, 8010);
        assert_eq!(c.stream_id, 2);
        assert_eq!(c.bitrate_kbps, 128);
        assert!(c.validate().is_ok());
        let d = StreamConfig::default();
        assert_eq!(
            (d.host.as_str(), d.port, d.stream_id),
            ("stream.wironair.de", 8001, 1)
        );
        assert_eq!(d.admin_port, None);
        assert!(d.validate().is_ok());
        assert!(
            StreamConfig {
                host: String::new(),
                ..d.clone()
            }
            .validate()
            .is_err()
        );
        assert_eq!(d.login_line("k"), "k\r\n");
        assert_eq!(
            StreamConfig { stream_id: 3, ..d }.login_line("k"),
            "k:#3\r\n"
        );
        let bad = StreamConfig {
            sample_rate: 22_050,
            ..c
        };
        assert!(bad.validate().is_err());
        assert!(
            default_password_file().ends_with(".secrets/daw.woa/shoutcast-source.pass")
                || cfg!(windows)
        );
    }
}
