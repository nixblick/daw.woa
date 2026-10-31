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

/// Eigener Shoutcast auf dem eigenen Server. Hören: `https://stream.wironair.de/stream`.
pub const DEFAULT_HOST: &str = "stream.wironair.de";

/// Stand des Dateiformats von `stream` in `config.json`. Fehlt `schema`, ist es ein Stand
/// vor 0.34.25 und [`StreamConfig::migrate`] rechnet ihn um.
pub const SCHEMA: u32 = 2;

/// Quellprotokoll des Servers.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Protocol {
    /// Shoutcast v1: Kennwort, `OK2`, `icy-*`-Kopfzeilen. Ein Stream je Server.
    #[default]
    #[serde(rename = "shoutcast")]
    Shoutcast,
    /// Shoutcast v2 (DNAS) über den Legacy-Quellport: wie v1, dazu Stream-ID (`kennwort:#sid`).
    #[serde(rename = "shoutcast2")]
    ShoutcastV2,
    /// Icecast 2 (HTTP `PUT` mit Basic-Auth, Benutzer meist `source`).
    #[serde(rename = "icecast")]
    Icecast,
}

impl Protocol {
    pub const ALL: [Protocol; 3] = [Self::Shoutcast, Self::ShoutcastV2, Self::Icecast];

    /// Name für die Auswahl in den Einstellungen.
    pub fn label(self) -> &'static str {
        match self {
            Self::Shoutcast => "Shoutcast v1",
            Self::ShoutcastV2 => "Shoutcast v2 (DNAS)",
            Self::Icecast => "Icecast 2",
        }
    }

    /// Kennung für die Kommandozeile und `config.json`.
    pub fn id(self) -> &'static str {
        match self {
            Self::Shoutcast => "shoutcast",
            Self::ShoutcastV2 => "shoutcast2",
            Self::Icecast => "icecast",
        }
    }

    /// Aus der Kennung ([`Protocol::id`]); Groß-/Kleinschreibung egal.
    pub fn from_id(s: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|p| p.id().eq_ignore_ascii_case(s.trim()))
    }

    pub fn is_shoutcast(self) -> bool {
        !matches!(self, Self::Icecast)
    }
}

/// Was der Nutzer im Feld „Server“ eingibt, zerlegt: `http://1.2.3.4:8000/stream`,
/// `radio.example:8000`, `[::1]:8000` oder nur `radio.example`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ServerInput {
    pub host: String,
    pub port: Option<u16>,
    /// Pfad mit führendem `/`, wenn eingegeben (Icecast-Mountpoint).
    pub mount: Option<String>,
}

impl ServerInput {
    /// Zerlegt die Eingabe. Schema (`http://`, `icecast://` …), Benutzer und Kennwort vor dem
    /// `@` fallen weg (ein Kennwort gehört nie in dieses Feld), ebenso Anfrage und Anker.
    pub fn parse(input: &str) -> Self {
        let mut s = input.trim();
        if let Some((_, rest)) = s.split_once("://") {
            s = rest;
        }
        let (auth_host, path) = match s.find('/') {
            Some(i) => (&s[..i], Some(&s[i..])),
            None => (s, None),
        };
        let auth_host = auth_host.rsplit('@').next().unwrap_or(auth_host).trim();
        let (host, port) = if let Some(rest) = auth_host.strip_prefix('[') {
            match rest.split_once(']') {
                Some((h, after)) => (h, after.strip_prefix(':').and_then(|p| p.parse().ok())),
                None => (rest, None),
            }
        } else if auth_host.matches(':').count() == 1 {
            let (h, p) = auth_host.split_once(':').unwrap_or((auth_host, ""));
            (h, p.parse().ok())
        } else {
            // Kein oder mehrere Doppelpunkte: Name oder nackte IPv6-Adresse.
            (auth_host, None)
        };
        let mount = path
            .map(|p| p.split(['?', '#']).next().unwrap_or(p).trim())
            .filter(|p| p.len() > 1)
            .map(str::to_string);
        Self {
            host: host.trim().to_string(),
            port,
            mount,
        }
    }
}

/// Host für eine URL oder den `Host:`-Kopf: IPv6-Adressen in eckigen Klammern.
pub fn host_for_url(host: &str) -> String {
    if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]")
    } else {
        host.to_string()
    }
}

fn schema_old() -> u32 {
    1
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
    /// Port des Servers, wie im Server eingestellt (Shoutcast: `portbase`, Icecast: `port`).
    /// Shoutcast nimmt die Quelle auf `port + 1` an, siehe [`StreamConfig::source_port`].
    pub port: u16,
    /// Benutzer der Quelle. Icecast: meist `source`. Shoutcast: ohne Bedeutung.
    pub user: String,
    /// Admin-Port (Shoutcast: meist `portbase`) für Titel (`admin.cgi?mode=updinfo`) und
    /// Hörerzahl (`/7.html`). `None` = nicht erreichbar: dann keine Titel und keine Hörerzahl.
    pub admin_port: Option<u16>,
    /// Stream-ID im DNAS (`streamid`), meist 1.
    pub stream_id: u32,
    /// MP3-Bitrate (CBR) in kbit/s.
    pub bitrate_kbps: u32,
    /// Abtastrate des Streams: 44100 oder 48000.
    pub sample_rate: u32,
    /// Alte Kennwortdatei (eine Zeile) für Skripte und die Kommandozeile. Leer = das
    /// Kennwort liegt im Schlüsselbund ([`crate::kennwort`]). Das Kennwort selbst steht nie hier.
    pub password_file: PathBuf,
    /// Stand des Formats, siehe [`SCHEMA`]. Fehlt er, ist es ein alter Stand.
    #[serde(default = "schema_old")]
    pub schema: u32,
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
            port: 8000,
            user: "source".to_string(),
            admin_port: None,
            stream_id: 1,
            bitrate_kbps: 128,
            sample_rate: 44_100,
            password_file: PathBuf::new(),
            schema: SCHEMA,
            name: "WOA – Wir on Air".to_string(),
            emergency: Vec::new(),
            silence_seconds: 2.0,
        }
    }
}

impl StreamConfig {
    /// Rechnet einen alten Stand auf [`SCHEMA`] um. Bis 0.34.24 stand bei Shoutcast der
    /// Quell-Port (`portbase + 1`) in `port`; jetzt steht dort der Port des Servers.
    /// Mehrfacher Aufruf ändert nichts mehr.
    pub fn migrate(mut self) -> Self {
        if self.schema < SCHEMA {
            if self.protocol.is_shoutcast() && self.port > 1 {
                self.port -= 1;
            }
            self.schema = SCHEMA;
        }
        self
    }

    /// Trägt keine alte Kennwortdatei mehr ein (nachdem ihr Inhalt im Schlüsselbund liegt).
    pub fn forget_legacy_file(&mut self) {
        self.password_file = PathBuf::new();
    }

    /// Port, auf dem die Quelle sich anmeldet: Shoutcast `port + 1`, Icecast `port`.
    pub fn source_port(&self) -> u16 {
        if self.protocol.is_shoutcast() {
            self.port.saturating_add(1)
        } else {
            self.port
        }
    }

    /// Benutzer der Quelle (Icecast); leer = `source`.
    pub fn source_user(&self) -> &str {
        let u = self.user.trim();
        if u.is_empty() { "source" } else { u }
    }

    /// Prüft die Werte; Fehlertext auf Deutsch.
    pub fn validate(&self) -> Result<(), String> {
        let host = self.host.trim();
        if host.is_empty() {
            return Err("Kein Server eingetragen (Adresse oder Name)".into());
        }
        if host
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || matches!(c, '/' | '?' | '#' | '@'))
        {
            return Err(format!(
                "Serveradresse {host:?} ungültig (nur Name oder IP, ohne http:// und Pfad)"
            ));
        }
        if self.port == 0 || (self.protocol.is_shoutcast() && self.port == u16::MAX) {
            return Err(format!("Port {} ungültig", self.port));
        }
        if self.protocol == Protocol::Icecast {
            if !self.mount.starts_with('/')
                || self.mount.len() > 200
                || self
                    .mount
                    .chars()
                    .any(|c| c.is_whitespace() || c.is_control() || c == '?' || c == '#')
            {
                return Err(format!(
                    "Mountpoint {:?} ungültig (beginnt mit /, ohne Leerzeichen)",
                    self.mount
                ));
            }
            if self.user.chars().any(|c| c.is_control() || c == ':') {
                return Err("Benutzername darf keinen Doppelpunkt enthalten".into());
            }
        }
        if self.protocol == Protocol::ShoutcastV2 && self.stream_id == 0 {
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

    /// Übernimmt eine Eingabe im Feld „Server“ (siehe [`ServerInput::parse`]): Name oder IP,
    /// Port und Mountpoint, soweit eingegeben.
    pub fn set_server(&mut self, input: &str) {
        let p = ServerInput::parse(input);
        self.host = p.host;
        if let Some(port) = p.port {
            self.port = port;
        }
        if let Some(m) = p.mount
            && self.protocol == Protocol::Icecast
        {
            self.mount = m;
        }
    }

    /// Adresse der Quelle für Anzeige und Log, ohne Kennwort: `shoutcast://host:8001`,
    /// `icecast://host:8000/stream`.
    pub fn source_url(&self) -> String {
        let h = host_for_url(self.host.trim());
        match self.protocol {
            Protocol::Icecast => {
                format!("icecast://{h}:{}{}", self.source_port(), self.mount)
            }
            _ => format!("shoutcast://{h}:{}", self.source_port()),
        }
    }

    /// Adresse für den Verwaltungs-Zugang (Hörerzahl, Titel), wenn ein Admin-Port eingetragen ist.
    pub fn admin_url(&self) -> Option<String> {
        self.admin_port
            .map(|p| format!("http://{}:{p}", host_for_url(self.host.trim())))
    }

    /// `host:port (sid n)` für Anzeige und Log, ohne Kennwort.
    pub fn label(&self) -> String {
        match self.protocol {
            Protocol::Shoutcast => format!("{}:{}", self.host, self.source_port()),
            Protocol::ShoutcastV2 => format!(
                "{}:{} (Stream {})",
                self.host,
                self.source_port(),
                self.stream_id
            ),
            Protocol::Icecast => format!("{}:{}{} (Icecast)", self.host, self.port, self.mount),
        }
    }

    /// Erste Zeile der Anmeldung: Kennwort, bei Shoutcast v2 ab Stream-ID 2 mit `:#sid`.
    pub(crate) fn login_line(&self, kw: &str) -> String {
        if self.protocol == Protocol::ShoutcastV2 && self.stream_id > 1 {
            format!("{kw}:#{}\r\n", self.stream_id)
        } else {
            format!("{kw}\r\n")
        }
    }

    /// Liest den Schlüssel `stream` aus einer `config.json` und rechnet alte Stände um.
    /// Fehlt die Datei oder der Schlüssel, gelten die Vorgaben.
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
            .map(|f| f.stream.migrate())
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
            r#"{"mode":"radio","stream":{"schema":2,"host":"radio.example","port":8010,"stream_id":2}}"#,
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
            ("stream.wironair.de", 8000, 1)
        );
        assert_eq!(d.source_port(), 8001);
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
        // Shoutcast v1 kennt keine Stream-ID, v2 hängt sie ab 2 an.
        assert_eq!(
            StreamConfig {
                stream_id: 3,
                ..d.clone()
            }
            .login_line("k"),
            "k\r\n"
        );
        assert_eq!(
            StreamConfig {
                protocol: Protocol::ShoutcastV2,
                stream_id: 3,
                ..d
            }
            .login_line("k"),
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

    #[test]
    fn old_config_is_migrated_once() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("config.json");
        // Stand bis 0.34.24: Quell-Port 8001, Kennwortdatei, Protokoll `shoutcast`.
        std::fs::write(
            &p,
            r#"{"stream":{"host":"radio.example","port":8001,"stream_id":2,"password_file":"/x/kw.pass","name":"Mein Sender"}}"#,
        )
        .unwrap();
        let c = StreamConfig::load_from(&p).unwrap();
        assert_eq!((c.port, c.source_port(), c.schema), (8000, 8001, SCHEMA));
        assert_eq!(c.password_file, PathBuf::from("/x/kw.pass"));
        assert_eq!(c.name, "Mein Sender");
        assert_eq!(c.user, "source");
        assert_eq!(c.clone().migrate(), c, "zweites Umrechnen ändert nichts");
        // Gespeichert und neu geladen bleibt es stabil.
        let again: StreamConfig =
            serde_json::from_str(&serde_json::to_string(&c).unwrap()).unwrap();
        assert_eq!(again.migrate().port, 8000);
        // Icecast: der Port war schon der des Servers.
        std::fs::write(
            &p,
            r#"{"stream":{"protocol":"icecast","host":"h","port":8000,"mount":"/live"}}"#,
        )
        .unwrap();
        let ice = StreamConfig::load_from(&p).unwrap();
        assert_eq!((ice.port, ice.source_port()), (8000, 8000));
        // Kein Schlüssel `stream`: aktuelle Vorgaben, nicht umgerechnet.
        std::fs::write(&p, r#"{"mode":"radio"}"#).unwrap();
        assert_eq!(StreamConfig::load_from(&p).unwrap().port, 8000);
    }

    #[test]
    fn server_input_is_taken_apart() {
        let p = ServerInput::parse;
        assert_eq!(p("radio.example").host, "radio.example");
        assert_eq!(p("  radio.example:8010 ").port, Some(8010));
        let u = p("http://user:pw@10.0.0.5:8000/live?x=1#y");
        assert_eq!(
            (u.host.as_str(), u.port, u.mount.as_deref()),
            ("10.0.0.5", Some(8000), Some("/live"))
        );
        let v6 = p("[2001:db8::1]:8000");
        assert_eq!((v6.host.as_str(), v6.port), ("2001:db8::1", Some(8000)));
        assert_eq!(p("2001:db8::1").host, "2001:db8::1");
        assert_eq!(p("host/").mount, None);
        let mut c = StreamConfig {
            protocol: Protocol::Icecast,
            ..StreamConfig::default()
        };
        c.set_server("https://ice.example:8443/radio.mp3");
        assert_eq!(
            (c.host.as_str(), c.port, c.mount.as_str()),
            ("ice.example", 8443, "/radio.mp3")
        );
        assert!(c.validate().is_ok());
    }

    #[test]
    fn urls_per_server_type() {
        let sc = StreamConfig {
            host: "1.2.3.4".into(),
            port: 9000,
            admin_port: Some(9000),
            ..StreamConfig::default()
        };
        assert_eq!(sc.source_url(), "shoutcast://1.2.3.4:9001");
        assert_eq!(sc.admin_url().as_deref(), Some("http://1.2.3.4:9000"));
        let ice = StreamConfig {
            protocol: Protocol::Icecast,
            host: "::1".into(),
            port: 8000,
            mount: "/radio".into(),
            ..StreamConfig::default()
        };
        assert_eq!(ice.source_url(), "icecast://[::1]:8000/radio");
        assert_eq!(ice.admin_url(), None);
        assert_eq!(host_for_url("[::1]"), "[::1]");
        // Das Kennwort steht in keiner dieser Adressen.
        assert!(!sc.label().contains("pass") && !ice.label().contains("pass"));
    }

    #[test]
    fn validation_catches_typical_typing_errors() {
        let d = StreamConfig::default();
        let bad = |f: &dyn Fn(&mut StreamConfig)| {
            let mut c = d.clone();
            f(&mut c);
            c.validate().is_err()
        };
        assert!(bad(&|c| c.host = "http://x.de".into()));
        assert!(bad(&|c| c.host = "a b".into()));
        assert!(bad(&|c| c.port = 0));
        assert!(bad(&|c| c.port = 65535));
        assert!(bad(&|c| {
            c.protocol = Protocol::Icecast;
            c.mount = "live".into();
        }));
        assert!(bad(&|c| {
            c.protocol = Protocol::Icecast;
            c.user = "a:b".into();
        }));
        assert!(!bad(&|c| c.protocol = Protocol::Icecast));
        assert_eq!(Protocol::from_id("Shoutcast2"), Some(Protocol::ShoutcastV2));
        assert_eq!(Protocol::from_id("x"), None);
    }
}
