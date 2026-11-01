//! Der lokale Webserver der Fernbedienung: HTTP für die Seite und die Anmeldung, WebSocket für
//! Zustand und Befehle. Gebaut auf `std::net` und `tungstenite` (ohne Async-Stapel); jede
//! Verbindung hat einen eigenen Thread mit kurzem Lese-Timeout, damit ein langsames Handy nie
//! einen anderen oder die Oberfläche aufhält.
//!
//! Läuft nie im Audio-Thread. Die Oberfläche übergibt den Zustand mit [`Remote::publish`] und
//! holt Befehle mit [`Remote::poll`] ab.

use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tungstenite::protocol::{Role as WsRole, WebSocketConfig};
use tungstenite::{Message, WebSocket};

use crate::auth::{Auth, Limits, LoginError, Role};
use crate::command::{CmdError, RemoteCommand, resolve};
use crate::snapshot::{CatalogItem, PartySnapshot, WishSnap, delta, short_id};

const PAGE: &str = include_str!("page.html");
/// Größter Kopf und größter Körper einer Anfrage.
const MAX_HEAD: usize = 8 * 1024;
const MAX_BODY: usize = 4 * 1024;
/// So viele Suchtreffer gehen ans Handy.
const MAX_RESULTS: usize = 20;
/// Warteschlange der Befehle an die Oberfläche.
const QUEUE: usize = 256;

/// Einstellungen der Fernbedienung.
#[derive(Clone)]
pub struct RemoteConfig {
    /// Adresse und Port des Webservers. Port 0: das System sucht einen freien.
    pub bind: SocketAddr,
    /// OSC-Eingang (UDP), aus wenn `None`. Ohne Anmeldung, siehe [`crate::osc`].
    pub osc: Option<SocketAddr>,
    pub limits: Limits,
    /// Wird nach jedem eingegangenen Befehl gerufen (zum Aufwecken der Oberfläche).
    pub wake: Option<Arc<dyn Fn() + Send + Sync>>,
    /// Feste PINs (Gast, DJ), nur für Tests; sonst zufällig.
    pub pins: Option<(String, String)>,
}

impl Default for RemoteConfig {
    fn default() -> Self {
        Self {
            bind: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
            osc: None,
            limits: Limits::default(),
            wake: None,
            pins: None,
        }
    }
}

/// Was beim Starten schiefgehen kann.
#[derive(Debug)]
pub enum RemoteError {
    /// Port nicht zu öffnen (belegt, keine Berechtigung).
    Bind(io::Error),
    /// Das System liefert keinen Zufall für die PINs.
    NoRandom,
    /// Thread nicht zu starten.
    Thread(io::Error),
}

impl std::fmt::Display for RemoteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RemoteError::Bind(e) => write!(f, "Port nicht zu öffnen: {e}"),
            RemoteError::NoRandom => write!(f, "kein Zufall für die PIN"),
            RemoteError::Thread(e) => write!(f, "Thread nicht zu starten: {e}"),
        }
    }
}

impl std::error::Error for RemoteError {}

/// Die Adresse dieses Rechners im lokalen Netz. Es wird kein Paket gesendet: ein UDP-Socket
/// ohne Verbindungsaufbau verrät nur, über welche Schnittstelle der Weg ins Netz führt.
/// (192.0.2.0/24 ist ein Dokumentationsbereich.)
pub fn lan_ip() -> Option<IpAddr> {
    let s = UdpSocket::bind("0.0.0.0:0").ok()?;
    s.connect("192.0.2.1:9").ok()?;
    let ip = s.local_addr().ok()?.ip();
    (!ip.is_unspecified() && !ip.is_loopback()).then_some(ip)
}

#[derive(Default)]
struct Shared {
    public: Value,
    rev: u64,
    wishes: Vec<WishSnap>,
    catalog: HashMap<String, CatalogItem>,
}

struct Inner {
    stop: Arc<AtomicBool>,
    conns: AtomicUsize,
    ws_clients: AtomicUsize,
    shared: Mutex<Shared>,
    auth: Mutex<Auth>,
    tx: SyncSender<RemoteCommand>,
    wake: Option<Arc<dyn Fn() + Send + Sync>>,
    max_conns: usize,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

struct ConnGuard(Arc<Inner>);

impl ConnGuard {
    fn new(i: &Arc<Inner>) -> Self {
        i.conns.fetch_add(1, Ordering::SeqCst);
        Self(Arc::clone(i))
    }
}

impl Drop for ConnGuard {
    fn drop(&mut self) {
        self.0.conns.fetch_sub(1, Ordering::SeqCst);
    }
}

struct WsGuard(Arc<Inner>);

impl WsGuard {
    fn new(i: &Arc<Inner>) -> Self {
        i.ws_clients.fetch_add(1, Ordering::SeqCst);
        Self(Arc::clone(i))
    }
}

impl Drop for WsGuard {
    fn drop(&mut self) {
        self.0.ws_clients.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Die laufende Fernbedienung. Beim Beenden oder Wegwerfen wird der Port geschlossen und
/// jede offene Verbindung bekommt ein Close.
pub struct Remote {
    inner: Arc<Inner>,
    rx: Receiver<RemoteCommand>,
    addr: SocketAddr,
    osc_addr: Option<SocketAddr>,
    threads: Vec<JoinHandle<()>>,
}

impl Remote {
    /// Öffnet den Port und startet den Server.
    pub fn start(cfg: RemoteConfig) -> Result<Self, RemoteError> {
        let listener = TcpListener::bind(cfg.bind).map_err(RemoteError::Bind)?;
        let addr = listener.local_addr().map_err(RemoteError::Bind)?;
        listener.set_nonblocking(true).map_err(RemoteError::Bind)?;
        let auth = match &cfg.pins {
            Some((g, d)) => Auth::with_pins(cfg.limits.clone(), g, d),
            None => Auth::new(cfg.limits.clone()).ok_or(RemoteError::NoRandom)?,
        };
        let (tx, rx) = sync_channel(QUEUE);
        let inner = Arc::new(Inner {
            stop: Arc::new(AtomicBool::new(false)),
            conns: AtomicUsize::new(0),
            ws_clients: AtomicUsize::new(0),
            shared: Mutex::new(Shared {
                public: PartySnapshot::default().to_public(),
                ..Shared::default()
            }),
            auth: Mutex::new(auth),
            tx: tx.clone(),
            wake: cfg.wake.clone(),
            max_conns: cfg.limits.max_connections,
        });
        let mut threads = Vec::new();
        let i2 = Arc::clone(&inner);
        threads.push(
            std::thread::Builder::new()
                .name("woa-remote-accept".into())
                .spawn(move || accept_loop(listener, i2))
                .map_err(RemoteError::Thread)?,
        );
        let mut osc_addr = None;
        if let Some(a) = cfg.osc {
            match crate::osc::spawn(
                a,
                tx,
                Arc::clone(&inner.stop),
                cfg.wake.clone(),
                cfg.limits.commands_per_second,
            ) {
                Ok((h, local)) => {
                    osc_addr = Some(local);
                    threads.push(h);
                }
                Err(e) => tracing::warn!(error = %e, "remote: osc port not available"),
            }
        }
        tracing::info!(%addr, "remote: started");
        Ok(Self {
            inner,
            rx,
            addr,
            osc_addr,
            threads,
        })
    }

    /// Adresse des Webservers (mit dem echten Port).
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// Adresse des OSC-Eingangs, wenn er läuft.
    pub fn osc_addr(&self) -> Option<SocketAddr> {
        self.osc_addr
    }

    pub fn guest_pin(&self) -> String {
        lock(&self.inner.auth).guest_pin().to_string()
    }

    pub fn dj_pin(&self) -> String {
        lock(&self.inner.auth).dj_pin().to_string()
    }

    /// Neue PINs. Alle angemeldeten Handys fliegen raus (ihr Token gilt nicht mehr).
    pub fn renew_pins(&self) -> bool {
        let ok = lock(&self.inner.auth).renew();
        if ok {
            tracing::info!("remote: pins renewed, sessions dropped");
        }
        ok
    }

    /// Adresse zum Eintippen oder für den QR-Code. `host`: unter welcher Adresse Handys den
    /// Rechner erreichen (bei `0.0.0.0` als Bindung die LAN-Adresse).
    pub fn join_url(&self, pin: &str) -> String {
        let ip = if self.addr.ip().is_unspecified() {
            lan_ip().unwrap_or(IpAddr::V4(Ipv4Addr::LOCALHOST))
        } else {
            self.addr.ip()
        };
        let host = match ip {
            IpAddr::V4(v) => v.to_string(),
            IpAddr::V6(v) => format!("[{v}]"),
        };
        format!("http://{host}:{}/?pin={pin}", self.addr.port())
    }

    /// Wie viele Handys gerade per WebSocket verbunden sind.
    pub fn clients(&self) -> usize {
        self.inner.ws_clients.load(Ordering::SeqCst)
    }

    /// Setzt den Zustand, den alle Handys sehen. Sendet nur, wenn sich etwas ändert.
    pub fn publish(&self, snap: &PartySnapshot) {
        let public = snap.to_public();
        let mut sh = lock(&self.inner.shared);
        sh.wishes = snap.wishes.clone();
        if sh.public != public {
            sh.public = public;
            sh.rev += 1;
        }
    }

    /// Die Titel, die Gäste suchen und wünschen dürfen.
    pub fn set_catalog(&self, items: &[CatalogItem]) {
        let map: HashMap<String, CatalogItem> = items
            .iter()
            .map(|c| (short_id(&c.path), c.clone()))
            .collect();
        lock(&self.inner.shared).catalog = map;
    }

    /// Holt alle eingegangenen Befehle ab (ohne zu warten).
    pub fn poll(&self) -> Vec<RemoteCommand> {
        self.rx.try_iter().collect()
    }

    /// Beendet den Server: der Port ist danach zu, offene WebSockets bekommen ein Close.
    pub fn stop(&mut self) {
        if self.threads.is_empty() {
            return;
        }
        self.inner.stop.store(true, Ordering::SeqCst);
        for t in self.threads.drain(..) {
            let _ = t.join();
        }
        // Verbindungen enden spätestens nach einem Lesetakt; nicht ewig warten.
        let until = Instant::now() + Duration::from_secs(3);
        while self.inner.conns.load(Ordering::SeqCst) > 0 && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(10));
        }
        tracing::info!("remote: stopped");
    }
}

impl Drop for Remote {
    fn drop(&mut self) {
        self.stop();
    }
}

fn accept_loop(listener: TcpListener, inner: Arc<Inner>) {
    while !inner.stop.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((mut stream, peer)) => {
                // Auf manchen Systemen erbt die Verbindung den nicht blockierenden Modus.
                let _ = stream.set_nonblocking(false);
                if inner.conns.load(Ordering::SeqCst) >= inner.max_conns {
                    tracing::warn!(%peer, "remote: too many connections");
                    let _ = respond(&mut stream, 503, "text/plain", b"zu viele Verbindungen", "");
                    continue;
                }
                let guard = ConnGuard::new(&inner);
                let i = Arc::clone(&inner);
                let spawned = std::thread::Builder::new()
                    .name("woa-remote-conn".into())
                    .stack_size(512 * 1024)
                    .spawn(move || {
                        let _guard = guard;
                        handle(stream, peer, &i);
                    });
                if let Err(e) = spawned {
                    tracing::error!(error = %e, "remote: cannot start connection thread");
                }
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(e) => {
                tracing::warn!(error = %e, "remote: accept failed");
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    }
    // Hier endet der Thread und mit ihm der Listener: der Port ist zu.
}

// ---------------------------------------------------------------- HTTP

struct Request {
    method: String,
    path: String,
    query: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Request {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    fn param(&self, key: &str) -> Option<String> {
        self.query.split('&').find_map(|kv| {
            let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
            (k == key).then(|| percent_decode(v))
        })
    }

    /// Token aus `Authorization: Bearer …` oder `?token=…` (für den WebSocket).
    fn token(&self) -> Option<String> {
        if let Some(a) = self.header("authorization")
            && let Some(t) = a.strip_prefix("Bearer ")
        {
            return Some(t.trim().to_string());
        }
        self.param("token")
    }
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < b.len() => {
                let hex = std::str::from_utf8(&b[i + 1..i + 3]).ok();
                match hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                    Some(v) => {
                        out.push(v);
                        i += 2;
                    }
                    None => out.push(b'%'),
                }
            }
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[derive(Debug)]
enum ReadError {
    Closed,
    Timeout,
    TooLarge,
    Bad,
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

fn read_request(stream: &mut TcpStream) -> Result<Request, ReadError> {
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut buf: Vec<u8> = Vec::with_capacity(1024);
    let mut tmp = [0u8; 1024];
    let mut read_more = |buf: &mut Vec<u8>| -> Result<(), ReadError> {
        if Instant::now() > deadline {
            return Err(ReadError::Timeout);
        }
        match stream.read(&mut tmp) {
            Ok(0) => Err(ReadError::Closed),
            Ok(n) => {
                buf.extend_from_slice(&tmp[..n]);
                Ok(())
            }
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) =>
            {
                Err(ReadError::Timeout)
            }
            Err(_) => Err(ReadError::Closed),
        }
    };
    let head_end = loop {
        if let Some(p) = find(&buf, b"\r\n\r\n") {
            break p + 4;
        }
        if buf.len() > MAX_HEAD {
            return Err(ReadError::TooLarge);
        }
        read_more(&mut buf)?;
    };
    let mut headers = [httparse::EMPTY_HEADER; 32];
    let mut req = httparse::Request::new(&mut headers);
    match req.parse(&buf[..head_end]) {
        Ok(httparse::Status::Complete(_)) => {}
        _ => return Err(ReadError::Bad),
    }
    let method = req.method.ok_or(ReadError::Bad)?.to_string();
    let target = req.path.ok_or(ReadError::Bad)?;
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let (path, query) = (path.to_string(), query.to_string());
    let hs: Vec<(String, String)> = req
        .headers
        .iter()
        .map(|h| {
            (
                h.name.to_string(),
                String::from_utf8_lossy(h.value).into_owned(),
            )
        })
        .collect();
    let len = hs
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("content-length"))
        .map(|(_, v)| v.trim().parse::<usize>().map_err(|_| ReadError::Bad))
        .transpose()?
        .unwrap_or(0);
    if len > MAX_BODY {
        return Err(ReadError::TooLarge);
    }
    let mut body = buf[head_end..].to_vec();
    while body.len() < len {
        read_more(&mut body)?;
    }
    body.truncate(len);
    Ok(Request {
        method,
        path,
        query,
        headers: hs,
        body,
    })
}

fn status_text(code: u16) -> &'static str {
    match code {
        101 => "Switching Protocols",
        200 => "OK",
        204 => "No Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        408 => "Request Timeout",
        413 => "Payload Too Large",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        _ => "Service Unavailable",
    }
}

fn respond(
    stream: &mut TcpStream,
    code: u16,
    ctype: &str,
    body: &[u8],
    extra: &str,
) -> io::Result<()> {
    let head = format!(
        "HTTP/1.1 {code} {}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\n\
         Cache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\n\
         Referrer-Policy: no-referrer\r\n\
         Content-Security-Policy: default-src 'self'; script-src 'unsafe-inline'; \
         style-src 'unsafe-inline'; connect-src 'self' ws: wss:\r\n\
         Connection: close\r\n{extra}\r\n",
        status_text(code),
        body.len()
    );
    stream.write_all(head.as_bytes())?;
    stream.write_all(body)?;
    stream.flush()
}

fn respond_json(stream: &mut TcpStream, code: u16, v: &Value, extra: &str) {
    let body = v.to_string();
    if let Err(e) = respond(
        stream,
        code,
        "application/json; charset=utf-8",
        body.as_bytes(),
        extra,
    ) {
        tracing::debug!(error = %e, "remote: response not sent");
    }
}

fn fail(stream: &mut TcpStream, code: u16, msg: &str) {
    respond_json(stream, code, &json!({"error": msg}), "");
}

/// Prüft das Token der Anfrage.
fn authed(inner: &Inner, req: &Request) -> Option<(String, Role)> {
    let t = req.token()?;
    let role = lock(&inner.auth).role_of(&t)?;
    Some((t, role))
}

fn handle(mut stream: TcpStream, peer: SocketAddr, inner: &Arc<Inner>) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
    let _ = stream.set_nodelay(true);
    let req = match read_request(&mut stream) {
        Ok(r) => r,
        Err(ReadError::Closed) => return,
        Err(ReadError::Timeout) => return fail(&mut stream, 408, "Zeitüberschreitung"),
        Err(ReadError::TooLarge) => return fail(&mut stream, 413, "Anfrage zu groß"),
        Err(ReadError::Bad) => return fail(&mut stream, 400, "Anfrage unlesbar"),
    };
    let ip = peer.ip();
    match (req.method.as_str(), req.path.as_str()) {
        ("GET", "/" | "/index.html") => {
            let _ = respond(
                &mut stream,
                200,
                "text/html; charset=utf-8",
                PAGE.as_bytes(),
                "",
            );
        }
        ("GET", "/favicon.ico") => {
            let _ = respond(&mut stream, 204, "text/plain", b"", "");
        }
        ("POST", "/api/login") => login(&mut stream, &req, ip, inner),
        ("GET", "/api/state") => match authed(inner, &req) {
            Some((_, role)) => {
                let (rev, state) = {
                    let sh = lock(&inner.shared);
                    (sh.rev, sh.public.clone())
                };
                respond_json(
                    &mut stream,
                    200,
                    &json!({"role": role.as_str(), "rev": rev, "state": state}),
                    "",
                );
            }
            None => fail(&mut stream, 401, "nicht angemeldet"),
        },
        ("GET", "/api/search") => match authed(inner, &req) {
            Some(_) => {
                let q = req.param("q").unwrap_or_default();
                respond_json(&mut stream, 200, &json!({"results": search(inner, &q)}), "");
            }
            None => fail(&mut stream, 401, "nicht angemeldet"),
        },
        ("POST", "/api/cmd") => match authed(inner, &req) {
            Some((token, _)) => {
                let body = String::from_utf8_lossy(&req.body).into_owned();
                match execute(inner, &token, ip, &body) {
                    Ok(()) => respond_json(&mut stream, 200, &json!({"ok": true}), ""),
                    Err((code, msg)) => fail(&mut stream, code, &msg),
                }
            }
            None => fail(&mut stream, 401, "nicht angemeldet"),
        },
        ("GET", "/ws") => websocket(stream, &req, ip, inner),
        (_, "/" | "/api/login" | "/api/state" | "/api/search" | "/api/cmd" | "/ws") => {
            fail(&mut stream, 405, "Methode nicht erlaubt");
        }
        _ => fail(&mut stream, 404, "nicht gefunden"),
    }
}

fn login(stream: &mut TcpStream, req: &Request, ip: IpAddr, inner: &Inner) {
    let pin = serde_json::from_slice::<Value>(&req.body)
        .ok()
        .and_then(|v| v.get("pin").and_then(Value::as_str).map(str::to_string))
        .unwrap_or_default();
    if pin.len() > 32 {
        return fail(stream, 400, "PIN zu lang");
    }
    let r = lock(&inner.auth).login_with_token(ip, &pin, req.token().as_deref(), Instant::now());
    match r {
        Ok((token, role)) => {
            tracing::info!(%ip, role = role.as_str(), "remote: login");
            respond_json(
                stream,
                200,
                &json!({"token": token, "role": role.as_str()}),
                "",
            );
        }
        Err(LoginError::WrongPin) => {
            tracing::warn!(%ip, "remote: wrong pin");
            fail(stream, 401, "PIN falsch");
        }
        Err(LoginError::Locked(s)) => {
            tracing::warn!(%ip, seconds = s, "remote: login locked");
            respond_json(
                stream,
                429,
                &json!({"error": format!("Zu viele Versuche, bitte {s} Sekunden warten"), "retry_after": s}),
                &format!("Retry-After: {s}\r\n"),
            );
        }
        Err(LoginError::NoRandom) => fail(stream, 500, "kein Zufall"),
    }
}

/// Einfache Suche: alle Wörter müssen im Titel vorkommen. Mindestens zwei Zeichen.
fn search(inner: &Inner, q: &str) -> Vec<Value> {
    let words: Vec<String> = q.split_whitespace().map(str::to_lowercase).collect();
    if words.is_empty() || q.trim().chars().count() < 2 {
        return Vec::new();
    }
    let sh = lock(&inner.shared);
    let mut hits: Vec<(&String, &CatalogItem)> = sh
        .catalog
        .iter()
        .filter(|(_, c)| {
            let t = c.title.to_lowercase();
            words.iter().all(|w| t.contains(w.as_str()))
        })
        .collect();
    hits.sort_by(|a, b| a.1.title.cmp(&b.1.title).then(a.0.cmp(b.0)));
    hits.into_iter()
        .take(MAX_RESULTS)
        .map(|(id, c)| json!({"id": id, "title": c.title}))
        .collect()
}

/// Führt eine Nachricht vom Handy aus. `Err`: HTTP-Status und Text für das Handy.
fn execute(inner: &Inner, token: &str, ip: IpAddr, body: &str) -> Result<(), (u16, String)> {
    let now = Instant::now();
    let role = {
        let mut a = lock(&inner.auth);
        let Some(role) = a.role_of(token) else {
            return Err((401, "nicht angemeldet".into()));
        };
        if !a.command_allowed(token, now) {
            tracing::warn!(%ip, "remote: command rate limit");
            return Err((429, "Zu viele Befehle, bitte kurz warten".into()));
        }
        role
    };
    let command = {
        let sh = lock(&inner.shared);
        resolve(role, body, &sh.catalog, &sh.wishes)
    };
    let command = match command {
        Ok(c) => c,
        Err(CmdError::Forbidden) => {
            tracing::warn!(%ip, "remote: guest tried to control");
            return Err((403, "Das darf nur der DJ".into()));
        }
        Err(CmdError::Bad(m)) => return Err((400, m.into())),
        Err(CmdError::NotFound) => return Err((404, "Titel oder Wunsch nicht gefunden".into())),
    };
    if role == Role::Guest
        && let Err(wait) = lock(&inner.auth).wish_allowed(ip, now)
    {
        tracing::info!(%ip, "remote: wish rate limit");
        return Err((
            429,
            format!("Nur ein Wunsch alle paar Sekunden, bitte {wait} s warten"),
        ));
    }
    tracing::info!(%ip, role = role.as_str(), cmd = %command.name(), "remote: command");
    match inner.tx.try_send(RemoteCommand { role, command }) {
        Ok(()) => {}
        Err(TrySendError::Full(_)) => return Err((503, "Rechner ist beschäftigt".into())),
        Err(TrySendError::Disconnected(_)) => return Err((503, "Fernbedienung beendet".into())),
    }
    if let Some(w) = &inner.wake {
        w();
    }
    Ok(())
}

// ---------------------------------------------------------------- WebSocket

fn websocket(mut stream: TcpStream, req: &Request, ip: IpAddr, inner: &Arc<Inner>) {
    let Some((token, role)) = authed(inner, req) else {
        tracing::warn!(%ip, "remote: websocket without valid token");
        return fail(&mut stream, 401, "nicht angemeldet");
    };
    let upgrade = req
        .header("upgrade")
        .is_some_and(|v| v.eq_ignore_ascii_case("websocket"));
    let Some(key) = req.header("sec-websocket-key").filter(|_| upgrade) else {
        return fail(&mut stream, 400, "kein WebSocket");
    };
    let accept = tungstenite::handshake::derive_accept_key(key.trim().as_bytes());
    let head = format!(
        "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\
         Sec-WebSocket-Accept: {accept}\r\n\r\n"
    );
    if stream.write_all(head.as_bytes()).is_err() {
        return;
    }
    // Kurzer Lesetakt: so kann derselbe Thread auch Änderungen senden und das Ende bemerken.
    let _ = stream.set_read_timeout(Some(Duration::from_millis(100)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
    let cfg = WebSocketConfig::default()
        .max_message_size(Some(MAX_BODY))
        .max_frame_size(Some(MAX_BODY));
    let mut ws = WebSocket::from_raw_socket(stream, WsRole::Server, Some(cfg));
    let _g = WsGuard::new(inner);
    ws_loop(&mut ws, &token, role, ip, inner);
    let _ = ws.close(None);
    let _ = ws.flush();
}

fn send_json(ws: &mut WebSocket<TcpStream>, v: &Value) -> bool {
    match ws.send(Message::text(v.to_string())) {
        Ok(()) => true,
        Err(e) => {
            tracing::debug!(error = %e, "remote: websocket send failed");
            false
        }
    }
}

fn ws_loop(ws: &mut WebSocket<TcpStream>, token: &str, role: Role, ip: IpAddr, inner: &Arc<Inner>) {
    // Vollstand beim Verbinden.
    let (mut seen_rev, mut last) = {
        let sh = lock(&inner.shared);
        (sh.rev, sh.public.clone())
    };
    if !send_json(
        ws,
        &json!({"type": "full", "role": role.as_str(), "rev": seen_rev, "state": last}),
    ) {
        return;
    }
    loop {
        if inner.stop.load(Ordering::Relaxed) || lock(&inner.auth).role_of(token).is_none() {
            return;
        }
        match ws.read() {
            Ok(Message::Text(t)) => {
                let reply = match execute(inner, token, ip, t.as_str()) {
                    Ok(()) => json!({"type": "ack"}),
                    Err((status, message)) => {
                        json!({"type": "error", "status": status, "message": message})
                    }
                };
                if !send_json(ws, &reply) {
                    return;
                }
            }
            Ok(Message::Close(_)) => return,
            Ok(_) => {}
            Err(tungstenite::Error::Io(e))
                if matches!(
                    e.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) => {}
            Err(_) => return,
        }
        let (rev, now_state) = {
            let sh = lock(&inner.shared);
            if sh.rev == seen_rev {
                continue;
            }
            (sh.rev, sh.public.clone())
        };
        let changes = delta(&last, &now_state);
        seen_rev = rev;
        if changes.is_empty() {
            continue;
        }
        if !send_json(
            ws,
            &json!({"type": "delta", "rev": rev, "changes": changes}),
        ) {
            return;
        }
        last = now_state;
    }
}
