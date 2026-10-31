//! Lokale Verbindung Oberfläche → `woa-streamd`.
//!
//! - Unix: Unix-Socket `link.sock` in `<datenordner>/streamd/` (Modus 0700), also nur
//!   für den eigenen Benutzer erreichbar.
//! - Windows: TCP auf 127.0.0.1. Bekannter Rest: Ein lokaler Prozess desselben Rechners
//!   kann dort das Verbinden der Oberfläche stören (Verbindungen fluten), aber ohne Token
//!   nicht senden. Eine Named Pipe mit ACL ginge nur mit neuer Abhängigkeit.
//!
//! Dazu in beiden Fällen ein zufälliges Token. Port, Token und Socket stehen in
//! `streamd.link` im Datenordner (unter Unix Modus 0600). Wer das Token nicht binnen
//! [`HANDSHAKE`] schickt, wird getrennt. Rahmen: `art: u8`, `länge: u32 LE`, Nutzdaten.
//!
//! - `Hello`: Rate (u32 LE) und Token. Eine neue Verbindung löst die alte ab (Neustart der
//!   Oberfläche nach einem Absturz).
//! - `Audio`: f32 LE, Stereo verschachtelt, in der gemeldeten Rate.
//! - `Title`: UTF-8.
//! - `Stop`: Sendung beenden; woa-streamd meldet sich ab und endet.

use std::io::{Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

pub const LINK_FILE: &str = "streamd.link";
/// Unterordner (0700) mit dem Unix-Socket.
pub const SOCKET_DIR: &str = "streamd";
pub const SOCKET_FILE: &str = "link.sock";
/// Frist für das Hello. Die Oberfläche schickt es sofort nach dem Verbinden.
pub const HANDSHAKE: Duration = Duration::from_millis(200);
/// Größter Rahmen nach der Anmeldung (1 MiB; der Feeder schickt höchstens
/// [`MAX_AUDIO_SAMPLES`] Samples je Rahmen).
pub const MAX_FRAME: usize = 1024 * 1024;
/// Samples je Audio-Rahmen höchstens (64 Ki × 4 Byte = 256 KiB).
pub const MAX_AUDIO_SAMPLES: usize = 64 * 1024;
/// Länge des Tokens (Hex-Zeichen, siehe [`crate::random_token`]).
pub const TOKEN_LEN: usize = 64;
/// Hello-Nutzdaten: Rate (4 Byte) plus Token.
pub const HELLO_LEN: usize = 4 + TOKEN_LEN;

pub const KIND_HELLO: u8 = 1;
pub const KIND_AUDIO: u8 = 2;
pub const KIND_TITLE: u8 = 3;
pub const KIND_STOP: u8 = 4;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LinkInfo {
    /// TCP-Port (Windows); 0 unter Unix.
    pub port: u16,
    /// Unix-Socket; leer unter Windows.
    #[serde(default)]
    pub socket: PathBuf,
    pub token: String,
    pub pid: u32,
}

pub fn link_path(data_dir: &Path) -> PathBuf {
    data_dir.join(LINK_FILE)
}

impl LinkInfo {
    pub fn write(&self, path: &Path) -> std::io::Result<()> {
        if let Some(d) = path.parent() {
            std::fs::create_dir_all(d)?;
        }
        let text = serde_json::to_string(self).map_err(std::io::Error::other)?;
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut f = opts.open(path)?;
        f.write_all(text.as_bytes())
    }

    pub fn read(path: &Path) -> Option<Self> {
        let text = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&text).ok()
    }
}

/// Eine lokale Verbindung (Unix-Socket oder TCP).
#[derive(Debug)]
pub enum LocalStream {
    Tcp(TcpStream),
    #[cfg(unix)]
    Unix(std::os::unix::net::UnixStream),
}

impl LocalStream {
    pub fn set_read_timeout(&self, d: Option<Duration>) -> std::io::Result<()> {
        match self {
            Self::Tcp(s) => s.set_read_timeout(d),
            #[cfg(unix)]
            Self::Unix(s) => s.set_read_timeout(d),
        }
    }

    pub fn set_write_timeout(&self, d: Option<Duration>) -> std::io::Result<()> {
        match self {
            Self::Tcp(s) => s.set_write_timeout(d),
            #[cfg(unix)]
            Self::Unix(s) => s.set_write_timeout(d),
        }
    }

    pub fn set_nonblocking(&self, on: bool) -> std::io::Result<()> {
        match self {
            Self::Tcp(s) => s.set_nonblocking(on),
            #[cfg(unix)]
            Self::Unix(s) => s.set_nonblocking(on),
        }
    }

    pub fn try_clone(&self) -> std::io::Result<Self> {
        Ok(match self {
            Self::Tcp(s) => Self::Tcp(s.try_clone()?),
            #[cfg(unix)]
            Self::Unix(s) => Self::Unix(s.try_clone()?),
        })
    }

    pub fn shutdown(&self) {
        let _ = match self {
            Self::Tcp(s) => s.shutdown(Shutdown::Both),
            #[cfg(unix)]
            Self::Unix(s) => s.shutdown(Shutdown::Both),
        };
    }
}

impl Read for LocalStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Self::Tcp(s) => s.read(buf),
            #[cfg(unix)]
            Self::Unix(s) => s.read(buf),
        }
    }
}

impl Write for LocalStream {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            Self::Tcp(s) => s.write(buf),
            #[cfg(unix)]
            Self::Unix(s) => s.write(buf),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            Self::Tcp(s) => s.flush(),
            #[cfg(unix)]
            Self::Unix(s) => s.flush(),
        }
    }
}

/// Der Empfänger in woa-streamd.
pub enum LocalListener {
    Tcp(TcpListener),
    #[cfg(unix)]
    Unix(std::os::unix::net::UnixListener, PathBuf),
}

impl LocalListener {
    /// Unix: Socket in `<data_dir>/streamd/` (Ordner 0700, alter Socket wird ersetzt).
    /// Sonst: 127.0.0.1 mit freiem Port.
    pub fn bind(data_dir: &Path) -> std::io::Result<Self> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let dir = data_dir.join(SOCKET_DIR);
            std::fs::create_dir_all(&dir)?;
            // Kein Symlink, und chmod gelingt nur dem Eigentümer: danach gehört der Ordner
            // uns und ist nur für uns offen.
            if std::fs::symlink_metadata(&dir)?.file_type().is_symlink() {
                return Err(std::io::Error::other("streamd dir is a symlink"));
            }
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
            let mode = std::fs::metadata(&dir)?.permissions().mode() & 0o777;
            if mode != 0o700 {
                return Err(std::io::Error::other("streamd dir not 0700"));
            }
            let path = dir.join(SOCKET_FILE);
            let _ = std::fs::remove_file(&path);
            let l = std::os::unix::net::UnixListener::bind(&path)?;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
            l.set_nonblocking(true)?;
            Ok(Self::Unix(l, path))
        }
        #[cfg(not(unix))]
        {
            let _ = data_dir;
            let l = TcpListener::bind(("127.0.0.1", 0))?;
            l.set_nonblocking(true)?;
            Ok(Self::Tcp(l))
        }
    }

    /// Was in `streamd.link` steht (ohne Token).
    pub fn info(&self, token: String) -> std::io::Result<LinkInfo> {
        let pid = std::process::id();
        Ok(match self {
            Self::Tcp(l) => LinkInfo {
                port: l.local_addr()?.port(),
                socket: PathBuf::new(),
                token,
                pid,
            },
            #[cfg(unix)]
            Self::Unix(_, p) => LinkInfo {
                port: 0,
                socket: p.clone(),
                token,
                pid,
            },
        })
    }

    /// Nicht blockierend: `WouldBlock`, wenn niemand wartet.
    pub fn accept(&self) -> std::io::Result<LocalStream> {
        match self {
            Self::Tcp(l) => l.accept().map(|(s, _)| LocalStream::Tcp(s)),
            #[cfg(unix)]
            Self::Unix(l, _) => l.accept().map(|(s, _)| LocalStream::Unix(s)),
        }
    }

    /// Socket-Datei entfernen (Unix).
    pub fn cleanup(&self) {
        #[cfg(unix)]
        if let Self::Unix(_, p) = self {
            let _ = std::fs::remove_file(p);
        }
    }
}

pub fn write_frame(w: &mut impl Write, kind: u8, payload: &[u8]) -> std::io::Result<()> {
    let len = u32::try_from(payload.len()).map_err(std::io::Error::other)?;
    let mut head = [0u8; 5];
    head[0] = kind;
    head[1..].copy_from_slice(&len.to_le_bytes());
    w.write_all(&head)?;
    w.write_all(payload)
}

/// Liest einen Rahmen; die Länge wird gegen `max` geprüft, bevor Speicher angelegt wird.
pub fn read_frame(r: &mut impl Read, buf: &mut Vec<u8>, max: usize) -> std::io::Result<u8> {
    let mut head = [0u8; 5];
    r.read_exact(&mut head)?;
    let len = u32::from_le_bytes([head[1], head[2], head[3], head[4]]) as usize;
    if len > max {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "frame too large",
        ));
    }
    buf.resize(len, 0);
    r.read_exact(buf)?;
    Ok(head[0])
}

/// Wie [`read_frame`], aber für einen Leser mit Lese-Frist: Läuft die Frist ab, bevor das
/// erste Byte da ist, kommt der Fehler zurück (der Aufrufer prüft dann z. B. `stop`). Ist
/// der Rahmen schon angelesen, wird weitergelesen, solange `keep_going` gilt. Sonst ginge
/// der Rest des Rahmens verloren, der nächste Kopf wäre Audio-Bytes und die Verbindung
/// risse ab („ui disconnected“ alle paar Sekunden, Paket 8, 29.10.2026).
pub fn read_frame_patient(
    r: &mut impl Read,
    buf: &mut Vec<u8>,
    max: usize,
    keep_going: &dyn Fn() -> bool,
) -> std::io::Result<u8> {
    let mut head = [0u8; 5];
    read_full(r, &mut head, true, keep_going)?;
    let len = u32::from_le_bytes([head[1], head[2], head[3], head[4]]) as usize;
    if len > max {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "frame too large",
        ));
    }
    buf.resize(len, 0);
    read_full(r, buf, false, keep_going)?;
    Ok(head[0])
}

fn read_full(
    r: &mut impl Read,
    buf: &mut [u8],
    may_idle: bool,
    keep_going: &dyn Fn() -> bool,
) -> std::io::Result<()> {
    let mut got = 0;
    while got < buf.len() {
        match r.read(&mut buf[got..]) {
            Ok(0) => return Err(std::io::ErrorKind::UnexpectedEof.into()),
            Ok(n) => got += n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                if (may_idle && got == 0) || !keep_going() {
                    return Err(e);
                }
            }
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

/// Vergleich in konstanter Zeit (bei gleicher Länge).
pub fn token_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Liest genau `buf.len()` Bytes, aber insgesamt nicht länger als bis `deadline`
/// (gegen Gegenüber, die Byte für Byte tröpfeln).
pub fn read_exact_until(
    s: &mut LocalStream,
    buf: &mut [u8],
    deadline: Instant,
) -> std::io::Result<()> {
    let mut got = 0;
    while got < buf.len() {
        let left = deadline
            .checked_duration_since(Instant::now())
            .filter(|d| !d.is_zero())
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::TimedOut, "handshake"))?;
        s.set_read_timeout(Some(left))?;
        match s.read(&mut buf[got..]) {
            Ok(0) => return Err(std::io::ErrorKind::UnexpectedEof.into()),
            Ok(n) => got += n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

/// Liest das Hello innerhalb von `deadline`: Rate, wenn das Token stimmt. Größe wird vor
/// jeder Allokation geprüft (genau [`HELLO_LEN`]).
pub fn read_hello(s: &mut LocalStream, token: &str, deadline: Instant) -> Option<u32> {
    let mut head = [0u8; 5];
    read_exact_until(s, &mut head, deadline).ok()?;
    let len = u32::from_le_bytes([head[1], head[2], head[3], head[4]]) as usize;
    if head[0] != KIND_HELLO || len != HELLO_LEN {
        return None;
    }
    let mut p = [0u8; HELLO_LEN];
    read_exact_until(s, &mut p, deadline).ok()?;
    if !token_eq(&p[4..], token.as_bytes()) {
        return None;
    }
    Some(u32::from_le_bytes([p[0], p[1], p[2], p[3]]))
}

pub fn hello_payload(rate: u32, token: &str) -> Vec<u8> {
    let mut p = rate.to_le_bytes().to_vec();
    p.extend_from_slice(token.as_bytes());
    p
}

pub fn audio_payload(samples: &[f32], out: &mut Vec<u8>) {
    out.clear();
    out.reserve(samples.len() * 4);
    for s in samples {
        out.extend_from_slice(&s.to_le_bytes());
    }
}

pub fn decode_audio(payload: &[u8], out: &mut Vec<f32>) {
    out.clear();
    out.extend(
        payload
            .as_chunks::<4>()
            .0
            .iter()
            .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])),
    );
}

/// Öffnet die lokale Verbindung laut Link-Datei, ohne Anmeldung.
pub fn open_raw(link: &Path) -> std::io::Result<(LocalStream, LinkInfo)> {
    let info = LinkInfo::read(link)
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no streamd.link"))?;
    #[cfg(unix)]
    if !info.socket.as_os_str().is_empty() {
        let s = std::os::unix::net::UnixStream::connect(&info.socket)?;
        return Ok((LocalStream::Unix(s), info));
    }
    let addr = SocketAddr::from(([127, 0, 0, 1], info.port));
    let s = TcpStream::connect_timeout(&addr, Duration::from_millis(300))?;
    let _ = s.set_nodelay(true);
    Ok((LocalStream::Tcp(s), info))
}

/// Verbindet sich mit dem laufenden woa-streamd und meldet sich an.
pub fn connect(link: &Path, rate: u32) -> std::io::Result<LocalStream> {
    let (mut s, info) = open_raw(link)?;
    s.set_write_timeout(Some(Duration::from_millis(500)))?;
    write_frame(&mut s, KIND_HELLO, &hello_payload(rate, &info.token))?;
    Ok(s)
}

/// Läuft woa-streamd (Link-Datei da und Verbindung geht auf)?
pub fn is_alive(link: &Path) -> bool {
    open_raw(link).is_ok()
}

/// Neuer Versuch zum lokalen woa-streamd: 1 s, verdoppelt bis 10 s.
const FEED_BACKOFF_MIN: Duration = Duration::from_secs(1);
const FEED_BACKOFF_MAX: Duration = Duration::from_secs(10);
/// So viel Sendemix hält der Feeder ohne Verbindung zurück und schickt ihn nach dem
/// Wiederverbinden nach. Kürzer als der Jitter-Puffer von woa-streamd, dann geht ein kurzer
/// Abriss ohne Lücke durch.
const FEED_BACKLOG_SECONDS: f32 = 0.4;

enum FeedMsg {
    Title(String),
    Stop,
    Quit,
}

/// Auf Seite der Oberfläche: liest den Sendemix aus dem `rtrb`-Ring des Mixers und schickt
/// ihn an woa-streamd. Eigener Thread, damit Socket-I/O weder Audio- noch Control-Thread
/// aufhält. Ohne Verbindung wird der Ring trotzdem geleert und alle Sekunde neu verbunden.
pub struct Feeder {
    tx: Sender<FeedMsg>,
    connected: Arc<AtomicBool>,
    reconnects: Arc<AtomicU32>,
    thread: Option<JoinHandle<()>>,
}

impl Feeder {
    pub fn spawn(consumer: rtrb::Consumer<f32>, rate: u32, link: PathBuf) -> Result<Self, String> {
        let (tx, rx) = channel();
        let connected = Arc::new(AtomicBool::new(false));
        let reconnects = Arc::new(AtomicU32::new(0));
        let c = Arc::clone(&connected);
        let n = Arc::clone(&reconnects);
        let thread = std::thread::Builder::new()
            .name("woa-stream-feeder".into())
            .spawn(move || feed(consumer, rate, link, rx, c, n))
            .map_err(|e| e.to_string())?;
        Ok(Self {
            tx,
            connected,
            reconnects,
            thread: Some(thread),
        })
    }

    pub fn set_title(&self, title: &str) {
        let _ = self.tx.send(FeedMsg::Title(title.to_string()));
    }

    pub fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Relaxed)
    }

    /// Wie oft die Verbindung zu woa-streamd abriss und wieder stand.
    pub fn reconnects(&self) -> u32 {
        self.reconnects.load(Ordering::Relaxed)
    }

    /// Beendet die Sendung (woa-streamd meldet sich ab) und den Feeder.
    pub fn stop_broadcast(mut self) {
        let _ = self.tx.send(FeedMsg::Stop);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for Feeder {
    /// Nur den Feeder beenden: woa-streamd sendet weiter (Notfall-Playlist).
    fn drop(&mut self) {
        let _ = self.tx.send(FeedMsg::Quit);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn feed(
    mut consumer: rtrb::Consumer<f32>,
    rate: u32,
    link: PathBuf,
    rx: Receiver<FeedMsg>,
    connected: Arc<AtomicBool>,
    reconnects: Arc<AtomicU32>,
) {
    let mut conn: Option<LocalStream> = None;
    // Seit wann die Verbindung fehlt (nach einem Abriss, nicht beim ersten Verbinden).
    let mut lost_at: Option<Instant> = None;
    let backlog_max = (FEED_BACKLOG_SECONDS * rate as f32) as usize * 2;
    let mut next_try = Instant::now();
    let mut backoff = FEED_BACKOFF_MIN;
    let mut title: Option<String> = None;
    let mut samples: Vec<f32> = Vec::with_capacity(rate as usize);
    let mut bytes: Vec<u8> = Vec::with_capacity(rate as usize * 4);
    let title_frame = |s: &mut LocalStream, t: &str| write_frame(s, KIND_TITLE, t.as_bytes());
    loop {
        loop {
            match rx.try_recv() {
                Ok(FeedMsg::Title(t)) => {
                    if let Some(s) = conn.as_mut()
                        && title_frame(s, &t).is_err()
                    {
                        conn = None;
                        lost_at = Some(Instant::now());
                    }
                    title = Some(t);
                }
                Ok(FeedMsg::Stop) => {
                    if conn.is_none() {
                        conn = connect(&link, rate).ok();
                    }
                    if let Some(s) = conn.as_mut() {
                        let _ = write_frame(s, KIND_STOP, &[]);
                    }
                    connected.store(false, Ordering::Relaxed);
                    return;
                }
                Ok(FeedMsg::Quit) | Err(TryRecvError::Disconnected) => {
                    connected.store(false, Ordering::Relaxed);
                    return;
                }
                Err(TryRecvError::Empty) => break,
            }
        }
        if conn.is_none() && Instant::now() >= next_try {
            match connect(&link, rate) {
                Ok(mut s) => {
                    let ok = title
                        .as_deref()
                        .is_none_or(|t| title_frame(&mut s, t).is_ok());
                    if ok {
                        conn = Some(s);
                        backoff = FEED_BACKOFF_MIN;
                        if let Some(t) = lost_at.take() {
                            let n = reconnects.fetch_add(1, Ordering::Relaxed) + 1;
                            tracing::info!(
                                down_ms = t.elapsed().as_millis() as u64,
                                reconnects = n,
                                "streamd link restored"
                            );
                        }
                    }
                }
                Err(e) => tracing::debug!(error = %e, "streamd not reachable"),
            }
            if conn.is_none() {
                next_try = Instant::now() + backoff;
                backoff = (backoff * 2).min(FEED_BACKOFF_MAX);
            }
        }
        connected.store(conn.is_some(), Ordering::Relaxed);

        // Ring leeren (auch ohne Verbindung, damit er nicht vollläuft). Was nicht gesendet
        // werden konnte, bleibt in `samples`, höchstens `backlog_max` (das Neueste).
        let n = consumer.slots() & !1;
        if n > 0
            && let Ok(chunk) = consumer.read_chunk(n)
        {
            let (a, b) = chunk.as_slices();
            samples.extend_from_slice(a);
            samples.extend_from_slice(b);
            chunk.commit_all();
        }
        if let Some(s) = conn.as_mut() {
            let mut sent = 0;
            for part in samples.chunks(MAX_AUDIO_SAMPLES) {
                audio_payload(part, &mut bytes);
                if let Err(e) = write_frame(s, KIND_AUDIO, &bytes) {
                    tracing::warn!(error = %e, "streamd link lost");
                    conn = None;
                    lost_at = Some(Instant::now());
                    connected.store(false, Ordering::Relaxed);
                    // Sofort neu versuchen: woa-streamd läuft meist noch.
                    next_try = Instant::now();
                    break;
                }
                sent += part.len();
            }
            samples.drain(..sent);
        }
        if samples.len() > backlog_max {
            let cut = (samples.len() - backlog_max) & !1;
            samples.drain(..cut);
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Liefert Bytes in Häppchen, dazwischen eine abgelaufene Lese-Frist.
    struct Choppy(Vec<Vec<u8>>);
    impl Read for Choppy {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            if self.0.is_empty() {
                return Err(std::io::ErrorKind::UnexpectedEof.into());
            }
            let mut part = self.0.remove(0);
            if part.is_empty() {
                return Err(std::io::ErrorKind::WouldBlock.into());
            }
            let n = part.len().min(buf.len());
            buf[..n].copy_from_slice(&part[..n]);
            if n < part.len() {
                self.0.insert(0, part.split_off(n));
            }
            Ok(n)
        }
    }

    #[test]
    fn timeout_mid_frame_keeps_the_stream_in_sync() {
        let mut wire = Vec::new();
        write_frame(&mut wire, KIND_AUDIO, &[1, 2, 3, 4, 5, 6, 7, 8]).unwrap();
        write_frame(&mut wire, KIND_TITLE, b"x").unwrap();
        // Frist läuft im Kopf und mitten in den Nutzdaten ab.
        let mut r = Choppy(vec![
            vec![],
            wire[..3].to_vec(),
            vec![],
            wire[3..9].to_vec(),
            vec![],
            wire[9..].to_vec(),
        ]);
        let mut buf = Vec::new();
        let go = || true;
        let e = read_frame_patient(&mut r, &mut buf, 64, &go).unwrap_err();
        assert_eq!(
            e.kind(),
            std::io::ErrorKind::WouldBlock,
            "Leerlauf meldet sich"
        );
        assert_eq!(
            read_frame_patient(&mut r, &mut buf, 64, &go).unwrap(),
            KIND_AUDIO
        );
        assert_eq!(buf, [1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(
            read_frame_patient(&mut r, &mut buf, 64, &go).unwrap(),
            KIND_TITLE
        );
        assert_eq!(buf, b"x");
    }

    #[test]
    fn frames_roundtrip() {
        let mut buf = Vec::new();
        write_frame(&mut buf, KIND_HELLO, &hello_payload(48_000, "tok")).unwrap();
        let mut bytes = Vec::new();
        audio_payload(&[0.5, -0.25], &mut bytes);
        write_frame(&mut buf, KIND_AUDIO, &bytes).unwrap();
        let mut r = std::io::Cursor::new(buf);
        let mut p = Vec::new();
        assert_eq!(read_frame(&mut r, &mut p, MAX_FRAME).unwrap(), KIND_HELLO);
        assert_eq!(&p[..4], &48_000u32.to_le_bytes());
        assert_eq!(&p[4..], b"tok");
        assert_eq!(read_frame(&mut r, &mut p, MAX_FRAME).unwrap(), KIND_AUDIO);
        let mut s = Vec::new();
        decode_audio(&p, &mut s);
        assert_eq!(s, vec![0.5, -0.25]);
        assert!(read_frame(&mut r, &mut p, MAX_FRAME).is_err());
        // Oversize is refused before allocating.
        let mut big = std::io::Cursor::new(vec![KIND_AUDIO, 0xFF, 0xFF, 0xFF, 0x7F]);
        let mut p = Vec::new();
        assert!(read_frame(&mut big, &mut p, MAX_FRAME).is_err());
        assert_eq!(p.capacity(), 0);
        assert!(token_eq(b"abc", b"abc"));
        assert!(!token_eq(b"abc", b"abd"));
        assert!(!token_eq(b"abc", b"ab"));
    }
}
