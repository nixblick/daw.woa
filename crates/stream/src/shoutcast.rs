//! Quellclient für Shoutcast DNAS v2, v1-kompatibler Legacy-Modus.
//!
//! Warum Legacy statt Uvox2: DNAS v2 nimmt beide Quellprotokolle an. Der Legacy-Weg ist
//! eine Textzeile (Kennwort), eine Antwort (`OK2`), ein paar `icy-*`-Kopfzeilen und dann
//! rohes MP3. Uvox2 braucht Binär-Rahmen und einen verschlüsselten Anmelde-Schritt (XTEA)
//! – viel mehr Code und Fehlerfläche für dasselbe Ergebnis. Auf dem eigenen Server ist zudem nur der
//! v1-Quellport (8001) offen (bretteins.infra#150).
//!
//! Die Quelle meldet sich bei Shoutcast auf `port + 1` an ([`StreamConfig::source_port`]).
//! Stream-ID 1 braucht keinen Zusatz; ab 2 hängt sie am Kennwort (`kennwort:#sid`, nur
//! Shoutcast v2). Das
//! v1-Protokoll hat keinen Weg für Titel in der Quellverbindung (die `icy-*`-Kopfzeilen
//! gelten nur einmal beim Anmelden). Titel gehen per HTTP an `admin.cgi?mode=updinfo` mit
//! dem Kennwort in der URL. Deshalb nur bei Loopback-Host und Admin-Port. Bei einem
//! entfernten Server geht der Titel per HTTPS an den eigenen Sendeserver (`POST /api/titel`,
//! [`title_url`]); der ruft `admin.cgi` im Docker-Netz mit dem Admin-Kennwort auf, das den
//! Server nie verlässt. Die App weist sich dort mit dem Quell-Kennwort aus.
//!
//! Alles mit `std::net`, blockierend mit Zeitgrenzen; der Aufrufer ist ein eigener Thread.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::time::Duration;

use crate::config::StreamConfig;
use crate::kennwort::Kennwort;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const IO_TIMEOUT: Duration = Duration::from_secs(10);
/// Manche DNAS-Seiten antworten nur einem Browser.
const USER_AGENT: &str = "Mozilla/5.0 (WOA woa-streamd)";

#[derive(Debug)]
pub enum StreamError {
    /// Server nicht erreichbar (DNS, Verbindung, Zeitgrenze).
    Connect(String),
    /// Server lehnt das Kennwort ab.
    AuthFailed,
    /// Kennwort stimmt, aber die Quelle ist schon belegt (Icecast: Mountpoint in Benutzung).
    InUse,
    /// Server lehnt aus anderem Grund ab (Antwortzeile).
    Refused(String),
    /// Verbindung während der Sendung abgerissen.
    Io(String),
}

impl std::fmt::Display for StreamError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Connect(e) => write!(f, "Server nicht erreichbar: {e}"),
            Self::AuthFailed => f.write_str(
                "Server lehnt das Quell-Kennwort ab (Kennwort, Benutzer und Stream-ID prüfen)",
            ),
            Self::InUse => f.write_str("Die Quelle ist schon belegt (es sendet bereits jemand)"),
            Self::Refused(l) => write!(f, "Server lehnt ab: {l}"),
            Self::Io(e) => write!(f, "Verbindung abgerissen: {e}"),
        }
    }
}

impl std::error::Error for StreamError {}

fn open(host: &str, port: u16) -> Result<TcpStream, StreamError> {
    let addrs: Vec<SocketAddr> = (host, port)
        .to_socket_addrs()
        .map_err(|e| StreamError::Connect(format!("{host}:{port}: {e}")))?
        .collect();
    let mut last = format!("{host}:{port}: keine Adresse");
    // Höchstens CONNECT_TIMEOUT für alle Adressen zusammen (IPv6 und IPv4 kosteten
    // sonst je 5 s; die Sendung hing 10 s pro Versuch).
    let deadline = std::time::Instant::now() + CONNECT_TIMEOUT;
    for a in addrs {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        if left.is_zero() {
            break;
        }
        match TcpStream::connect_timeout(&a, left) {
            Ok(s) => {
                let _ = s.set_read_timeout(Some(IO_TIMEOUT));
                let _ = s.set_write_timeout(Some(IO_TIMEOUT));
                let _ = s.set_nodelay(true);
                return Ok(s);
            }
            Err(e) => last = format!("{a}: {e}"),
        }
    }
    Err(StreamError::Connect(last))
}

/// Liest bis Leerzeile oder Verbindungsende (höchstens 4 KiB).
fn read_head(s: &mut TcpStream) -> std::io::Result<String> {
    let mut buf = Vec::new();
    let mut byte = [0u8; 1];
    while buf.len() < 4096 {
        match s.read(&mut byte)? {
            0 => break,
            _ => buf.push(byte[0]),
        }
        if buf.ends_with(b"\r\n\r\n") || buf.ends_with(b"\n\n") {
            break;
        }
    }
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

/// Shoutcast: Anmeldezeile senden und die Antwort prüfen. Danach ist die Verbindung bereit
/// für die `icy-*`-Kopfzeilen. Genutzt vom Senden und vom Verbindungstest.
pub(crate) fn login_shoutcast(cfg: &StreamConfig, kw: &Kennwort) -> Result<TcpStream, StreamError> {
    let mut s = open(&cfg.host, cfg.source_port())?;
    let io = |e: std::io::Error| StreamError::Io(e.to_string());
    let line = cfg.login_line(kw.expose());
    s.write_all(line.as_bytes()).map_err(io)?;
    let head = read_head(&mut s).map_err(io)?;
    let first = head.lines().next().unwrap_or("").trim().to_string();
    if first.starts_with("OK") {
        return Ok(s);
    }
    let low = first.to_ascii_lowercase();
    if low.contains("password") || low.contains("invalid") {
        return Err(StreamError::AuthFailed);
    }
    if low.starts_with("http/") || low.starts_with("<") {
        return Err(StreamError::Refused(format!(
            "Der Server antwortet wie ein Webserver ({first}). Ist hier Icecast oder ein \
             anderer Port gemeint?"
        )));
    }
    Err(StreamError::Refused(if first.is_empty() {
        "keine Antwort (Server-Typ und Port prüfen)".into()
    } else {
        first
    }))
}

/// Angemeldete Quellverbindung.
pub struct Source {
    stream: TcpStream,
}

impl std::fmt::Debug for Source {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Source")
    }
}

impl Source {
    /// Meldet sich auf dem Quell-Port an und schickt die Kopfzeilen.
    pub fn connect(cfg: &StreamConfig, kw: &Kennwort) -> Result<Self, StreamError> {
        if cfg.protocol == crate::config::Protocol::Icecast {
            return Self::connect_icecast(cfg, kw);
        }
        let mut s = login_shoutcast(cfg, kw)?;
        let io = |e: std::io::Error| StreamError::Io(e.to_string());
        let headers = format!(
            "content-type:audio/mpeg\r\nicy-name:{}\r\nicy-genre:Radio\r\nicy-pub:0\r\n\
             icy-br:{}\r\nicy-url:\r\n\r\n",
            one_line(&cfg.name),
            cfg.bitrate_kbps
        );
        s.write_all(headers.as_bytes()).map_err(io)?;
        Ok(Self { stream: s })
    }

    /// Icecast 2: `PUT <mount>` mit Basic-Auth (`source:<kennwort>`), dann rohes MP3.
    /// Das Kennwort steht nur im Kopf der Anfrage, nie in einer URL oder im Log.
    fn connect_icecast(cfg: &StreamConfig, kw: &Kennwort) -> Result<Self, StreamError> {
        let mut s = open(&cfg.host, cfg.source_port())?;
        let io = |e: std::io::Error| StreamError::Io(e.to_string());
        let auth = base64(format!("{}:{}", cfg.source_user(), kw.expose()).as_bytes());
        let req = format!(
            "PUT {} HTTP/1.1\r\nHost: {}:{}\r\nAuthorization: Basic {auth}\r\n\
             User-Agent: {USER_AGENT}\r\nContent-Type: audio/mpeg\r\nIce-Public: 0\r\n\
             Ice-Name: {}\r\nIce-Bitrate: {}\r\nExpect: 100-continue\r\n\r\n",
            cfg.mount,
            crate::config::host_for_url(&cfg.host),
            cfg.source_port(),
            one_line(&cfg.name),
            cfg.bitrate_kbps
        );
        s.write_all(req.as_bytes()).map_err(io)?;
        let head = read_head(&mut s).map_err(io)?;
        let code: u16 = head
            .lines()
            .next()
            .and_then(|l| l.split_whitespace().nth(1))
            .and_then(|c| c.parse().ok())
            .unwrap_or(0);
        match code {
            100 | 200 => Ok(Self { stream: s }),
            401 => Err(StreamError::AuthFailed),
            403 => {
                // Icecast meldet „Mountpoint in use“ mit 403; das Kennwort stimmt dann.
                let mut rest = Vec::new();
                let _ = s.take(512).read_to_end(&mut rest);
                let rest = format!("{head}{}", String::from_utf8_lossy(&rest)).to_ascii_lowercase();
                if rest.contains("in use") {
                    Err(StreamError::InUse)
                } else {
                    Err(StreamError::AuthFailed)
                }
            }
            0 => Err(StreamError::Refused(
                "keine HTTP-Antwort (Server-Typ prüfen: ist es Icecast?)".into(),
            )),
            c => Err(StreamError::Refused(format!("HTTP {c}"))),
        }
    }

    /// Schickt MP3-Bytes.
    pub fn send(&mut self, bytes: &[u8]) -> Result<(), StreamError> {
        self.stream
            .write_all(bytes)
            .map_err(|e| StreamError::Io(e.to_string()))
    }
}

fn one_line(s: &str) -> String {
    s.chars().filter(|c| !c.is_control()).collect()
}

/// Prozent-Kodierung für Query-Werte.
pub fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Einfacher HTTP-GET auf `portbase`; liefert Statuscode und Rumpf. Der Pfad kann das
/// Kennwort enthalten und wird darum nie geloggt oder in Fehler geschrieben.
fn http_get(cfg: &StreamConfig, path: &str) -> Result<(u16, String), StreamError> {
    let port = cfg
        .admin_port
        .ok_or_else(|| StreamError::Refused("kein Admin-Port eingetragen".into()))?;
    let mut s = open(&cfg.host, port)?;
    let io = |e: std::io::Error| StreamError::Io(e.to_string());
    let req = format!(
        "GET {path} HTTP/1.0\r\nHost: {}:{}\r\nUser-Agent: {USER_AGENT}\r\nConnection: close\r\n\r\n",
        crate::config::host_for_url(&cfg.host),
        port
    );
    s.write_all(req.as_bytes()).map_err(io)?;
    let mut buf = Vec::new();
    s.take(64 * 1024).read_to_end(&mut buf).map_err(io)?;
    let text = String::from_utf8_lossy(&buf);
    let code = text
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    let body = text
        .split_once("\r\n\r\n")
        .map(|(_, b)| b.to_string())
        .unwrap_or_default();
    Ok((code, body))
}

/// Base64 (Standard-Alphabet, mit `=`), für die Basic-Auth bei Icecast.
pub fn base64(data: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(A[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Ist der Host Loopback (`localhost`, 127.0.0.0/8, `::1`)?
pub fn is_loopback(host: &str) -> bool {
    let h = host.trim_start_matches('[').trim_end_matches(']');
    h.eq_ignore_ascii_case("localhost")
        || h.parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
}

/// Können Titel gesetzt werden? Nur mit Admin-Port und nur auf Loopback: `admin.cgi`
/// trägt das Quell-Kennwort im Klartext in der URL, das darf nicht übers Netz.
pub fn titles_possible(cfg: &StreamConfig) -> bool {
    cfg.protocol.is_shoutcast() && cfg.admin_port.is_some() && is_loopback(&cfg.host)
}

/// Titel setzen (`admin.cgi?mode=updinfo`). Nur für Loopback-Hosts, siehe
/// [`titles_possible`].
pub fn update_title(cfg: &StreamConfig, kw: &Kennwort, title: &str) -> Result<(), StreamError> {
    if !is_loopback(&cfg.host) {
        return Err(StreamError::Refused(
            "Titel nur bei lokalem Server (Kennwort stünde sonst im Klartext in der URL)".into(),
        ));
    }
    let path = format!(
        "/admin.cgi?sid={}&pass={}&mode=updinfo&song={}",
        cfg.stream_id,
        url_encode(kw.expose()),
        url_encode(&one_line(title))
    );
    match http_get(cfg, &path)? {
        (200, _) => Ok(()),
        (401 | 403, _) => Err(StreamError::AuthFailed),
        (code, _) => Err(StreamError::Refused(format!("Titel: HTTP {code}"))),
    }
}

/// Höchstlänge eines Titels in Zeichen (der Server kürzt ebenso).
pub const TITLE_MAX: usize = 200;

/// Titel-Endpunkt für einen entfernten Shoutcast-Server, sonst `None` (lokal, Icecast,
/// IP-Adresse ohne eingetragene URL). Nur `https://`, `http://` nur auf Loopback (Tests).
pub fn title_url(cfg: &StreamConfig) -> Option<String> {
    let host = cfg.host.trim();
    if !cfg.protocol.is_shoutcast() || host.is_empty() || is_loopback(host) {
        return None;
    }
    let u = cfg.title_url.trim();
    if !u.is_empty() {
        return title_url_ok(u).then(|| u.to_string());
    }
    // Eine IP hat kein Zertifikat auf der Edge; dann muss die URL eingetragen sein.
    let h = host.trim_start_matches('[').trim_end_matches(']');
    if h.parse::<std::net::IpAddr>().is_ok() {
        return None;
    }
    Some(format!("https://{host}/api/titel"))
}

fn title_url_ok(u: &str) -> bool {
    if u.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return false;
    }
    if u.starts_with("https://") {
        return true;
    }
    u.strip_prefix("http://")
        .and_then(|r| r.split(['/', '?']).next())
        .map(|hp| {
            let host = hp.rsplit_once(':').map_or(hp, |(h, p)| {
                if p.chars().all(|c| c.is_ascii_digit()) {
                    h
                } else {
                    hp
                }
            });
            is_loopback(host)
        })
        .unwrap_or(false)
}

/// Kann ein Titel gesetzt werden, lokal oder über den Sendeserver?
pub fn titles_reachable(cfg: &StreamConfig) -> bool {
    titles_possible(cfg) || title_url(cfg).is_some()
}

/// Titel kürzen und einzeilig machen.
pub fn clean_title(title: &str) -> String {
    one_line(title).trim().chars().take(TITLE_MAX).collect()
}

/// JSON-Rumpf für `POST /api/titel`. Die Stream-ID kennt der Server selbst.
pub fn title_body(title: &str) -> String {
    serde_json::json!({ "titel": clean_title(title) }).to_string()
}

/// Titel über den Sendeserver setzen (`POST <title_url>`, `Authorization: Bearer <Quell-Kennwort>`).
/// Keine Weiterleitungen (sonst ginge der Kopf an eine fremde Adresse). Der Kennwort-Kopf
/// steht in keinem Fehler und keinem Log.
pub fn update_title_via_server(url: &str, kw: &Kennwort, title: &str) -> Result<(), StreamError> {
    if !title_url_ok(url) {
        return Err(StreamError::Refused(
            "Titel-URL muss https:// sein (http nur lokal)".into(),
        ));
    }
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(IO_TIMEOUT))
        .max_redirects(0)
        .http_status_as_error(false)
        .build()
        .into();
    let resp = agent
        .post(url)
        .header("Authorization", &format!("Bearer {}", kw.expose()))
        .header("Content-Type", "application/json")
        .header("User-Agent", USER_AGENT)
        .send(title_body(title).as_bytes())
        .map_err(|e| StreamError::Connect(format!("Titel-Server: {e}")))?;
    match resp.status().as_u16() {
        200 | 204 => Ok(()),
        401 | 403 => Err(StreamError::AuthFailed),
        404 => Err(StreamError::Refused(
            "Titel: Server kennt /api/titel nicht".into(),
        )),
        code => Err(StreamError::Refused(format!("Titel: HTTP {code}"))),
    }
}

/// Titel setzen: lokal per `admin.cgi`, entfernt über den Sendeserver.
pub fn set_title(cfg: &StreamConfig, kw: &Kennwort, title: &str) -> Result<(), StreamError> {
    if titles_possible(cfg) {
        return update_title(cfg, kw, title);
    }
    match title_url(cfg) {
        Some(u) => update_title_via_server(&u, kw, title),
        None => Err(StreamError::Refused(
            "Titel: kein Weg (lokal ohne Admin-Port oder IP ohne Titel-URL)".into(),
        )),
    }
}

/// Hörerzahlen aus der öffentlichen Kurzstatistik `/7.html?sid=n` (ohne Kennwort).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ListenerStats {
    pub current: u32,
    /// Spitze laut Server (seit dessen Start, nicht je Sendung).
    pub peak: Option<u32>,
    pub unique: Option<u32>,
}

impl ListenerStats {
    /// Eindeutige Hörer; `current` zählt Verbindungen und gilt nur, wenn `unique` fehlt.
    pub fn listeners(&self) -> u32 {
        self.unique.unwrap_or(self.current)
    }
}

/// Liest `aktuell,status,spitze,max,eindeutig,bitrate,titel` aus dem Text von `/7.html`.
pub fn parse_seven(body: &str) -> Option<ListenerStats> {
    let inner = body
        .split_once("<body>")
        .map_or(body, |(_, b)| b)
        .split("</body>")
        .next()
        .unwrap_or("");
    let f: Vec<&str> = inner.split(',').map(str::trim).collect();
    let num = |i: usize| f.get(i).and_then(|v| v.parse::<u32>().ok());
    Some(ListenerStats {
        current: num(0)?,
        peak: num(2),
        unique: num(4),
    })
}

/// Hörerzahlen über `/7.html?sid=n`. `None`, wenn nicht abfragbar.
pub fn listener_stats(cfg: &StreamConfig) -> Option<ListenerStats> {
    let (code, body) = http_get(cfg, &format!("/7.html?sid={}", cfg.stream_id)).ok()?;
    if code != 200 {
        return None;
    }
    parse_seven(&body)
}

/// Nur die Hörerzahl (eindeutige Hörer). `None`, wenn nicht abfragbar.
pub fn listeners(cfg: &StreamConfig) -> Option<u32> {
    listener_stats(cfg).map(|s| s.listeners())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seven_html_gives_current_peak_unique() {
        let s =
            parse_seven("<html><body>12,1,18,100,9,128,Titel, mit Komma</body></html>").unwrap();
        assert_eq!((s.current, s.peak, s.unique), (12, Some(18), Some(9)));
        assert_eq!(s.listeners(), 9);
        // 04.11.2026: ein iPhone, zwei Verbindungen = ein Hörer.
        assert_eq!(parse_seven("<body>2,1,2,50,1,128,X</body>").unwrap().listeners(), 1);
        assert_eq!(parse_seven("4").unwrap().listeners(), 4);
        assert_eq!(parse_seven("<html><body>kaputt</body></html>"), None);
        assert_eq!(
            parse_seven("4").map(|s| (s.current, s.peak)),
            Some((4, None))
        );
    }

    #[test]
    fn title_url_only_for_remote_shoutcast_names() {
        let mut c = StreamConfig {
            host: "stream.wironair.de".into(),
            ..StreamConfig::default()
        };
        assert_eq!(
            title_url(&c).as_deref(),
            Some("https://stream.wironair.de/api/titel")
        );
        c.host = "127.0.0.1".into();
        assert_eq!(title_url(&c), None);
        c.host = "203.0.113.5".into();
        assert_eq!(title_url(&c), None);
        c.title_url = "https://feedback.wironair.de/api/titel".into();
        assert!(title_url(&c).is_some());
        c.title_url = "http://203.0.113.5/api/titel".into();
        assert_eq!(title_url(&c), None, "kein Klartext übers Netz");
        c.title_url = "http://127.0.0.1:9/api/titel".into();
        assert!(title_url(&c).is_some());
        c.protocol = crate::Protocol::Icecast;
        assert_eq!(title_url(&c), None);
    }

    #[test]
    fn title_body_is_short_and_one_line() {
        let long = "x".repeat(500);
        let v: serde_json::Value = serde_json::from_str(&title_body(&long)).unwrap();
        assert_eq!(v["titel"].as_str().unwrap().chars().count(), TITLE_MAX);
        assert_eq!(clean_title(" a\r\nb "), "ab");
    }

    #[test]
    fn encodes_query_values() {
        assert_eq!(url_encode("Ä b&c=d"), "%C3%84%20b%26c%3Dd");
        assert_eq!(one_line("a\r\nb"), "ab");
    }

    #[test]
    fn base64_matches_rfc4648() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64(b"source:hackme"), "c291cmNlOmhhY2ttZQ==");
    }
}
