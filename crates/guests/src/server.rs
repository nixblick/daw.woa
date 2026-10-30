//! Der kleine HTTPS-Server für Gäste: liefert die Gast-Seite und nimmt die Anmeldung
//! (Signalisierung) entgegen. HTTPS, weil Browser das Mikrofon nur auf sicheren Seiten
//! freigeben; das Zertifikat ist selbst ausgestellt und wird bei jedem Start neu erzeugt (kein
//! Schlüssel auf der Platte). Das Handy zeigt deshalb einmal eine Zertifikatswarnung.
//!
//! Ein Thread nimmt Verbindungen an, jede Verbindung bekommt einen kurzlebigen eigenen Thread
//! mit Zeitgrenzen (eine Anfrage, dann zu). Die Routen stehen in [`route`], rein und ohne Netz
//! testbar. Läuft nie im Audio-Thread.
//!
//! Routen (alle unter `/gast`): `GET /gast` die Seite, `GET /gast/check` Einladung gültig,
//! `POST /gast/join` beitreten, `GET /gast/state` Stand abfragen, `POST /gast/leave` auflegen.

use std::io::{Read, Write};
use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use rustls::{ServerConfig, ServerConnection, StreamOwned};
use serde_json::{Value, json};

use crate::gate::{Gate, MAX_SDP, TicketState};

const PAGE: &str = include_str!("gast.html");
const MAX_HEAD: usize = 8 * 1024;
const MAX_BODY: usize = MAX_SDP + 2 * 1024;
const MAX_CONNS: usize = 32;
const IO_TIMEOUT: Duration = Duration::from_secs(10);

/// Eine fertige Antwort.
#[derive(Debug, Clone, PartialEq)]
pub struct Response {
    pub status: u16,
    pub content_type: &'static str,
    pub body: Vec<u8>,
}

impl Response {
    fn json(status: u16, v: &Value) -> Self {
        Self {
            status,
            content_type: "application/json; charset=utf-8",
            body: v.to_string().into_bytes(),
        }
    }

    fn error(status: u16, msg: &str) -> Self {
        Self::json(status, &json!({ "error": msg }))
    }
}

/// Hex-Kennung (Einladung, Ticket) aus dem Query-Text, sonst leer.
fn query_id<'a>(query: &'a str, key: &str) -> &'a str {
    for pair in query.split('&') {
        if let Some((k, v)) = pair.split_once('=')
            && k == key
            && v.len() <= 64
            && v.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return v;
        }
    }
    ""
}

/// Bearbeitet eine Anfrage. Rein: keine Uhr, kein Netz.
pub fn route(
    gate: &Gate,
    method: &str,
    path: &str,
    query: &str,
    body: &[u8],
    peer: &str,
    now: Instant,
) -> Response {
    match (method, path) {
        ("GET", "/gast") | ("GET", "/gast/") => Response {
            status: 200,
            content_type: "text/html; charset=utf-8",
            body: PAGE.as_bytes().to_vec(),
        },
        ("GET", "/gast/check") => {
            let ok = gate.invite_valid(query_id(query, "i"), now);
            Response::json(200, &json!({ "ok": ok }))
        }
        ("GET", "/gast/state") => {
            let st = gate.state(query_id(query, "t"), now);
            let v = match st {
                TicketState::Waiting => json!({ "state": "waiting" }),
                TicketState::Accepted(sdp) => json!({ "state": "accepted", "sdp": sdp }),
                TicketState::Rejected => json!({ "state": "rejected" }),
                TicketState::Ended => json!({ "state": "ended" }),
            };
            Response::json(200, &v)
        }
        ("POST", "/gast/join") => {
            let Ok(v) = serde_json::from_slice::<Value>(body) else {
                return Response::error(400, "Ungültige Anfrage");
            };
            let text = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or("");
            match gate.join(text("invite"), text("name"), text("sdp"), peer, now) {
                Ok(ticket) => Response::json(200, &json!({ "ticket": ticket })),
                Err(e) => Response::error(403, e),
            }
        }
        ("POST", "/gast/leave") => {
            if let Ok(v) = serde_json::from_slice::<Value>(body)
                && let Some(t) = v.get("t").and_then(Value::as_str)
            {
                gate.leave(t);
            }
            Response::json(200, &json!({ "ok": true }))
        }
        ("GET" | "POST", _) => Response::error(404, "Nicht gefunden"),
        _ => Response::error(405, "Methode nicht erlaubt"),
    }
}

fn status_text(code: u16) -> &'static str {
    match code {
        200 => "OK",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        413 => "Payload Too Large",
        _ => "Error",
    }
}

fn write_response<W: Write>(w: &mut W, r: &Response) -> std::io::Result<()> {
    let head = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nCache-Control: no-store\r\n\
         X-Content-Type-Options: nosniff\r\nReferrer-Policy: no-referrer\r\n\
         Content-Security-Policy: default-src 'none'; script-src 'unsafe-inline'; \
         style-src 'unsafe-inline'; connect-src 'self'; media-src blob: mediastream:; \
         frame-ancestors 'none'; base-uri 'none'; form-action 'none'\r\n\
         Connection: close\r\n\r\n",
        r.status,
        status_text(r.status),
        r.content_type,
        r.body.len()
    );
    w.write_all(head.as_bytes())?;
    w.write_all(&r.body)?;
    w.flush()
}

struct Request {
    method: String,
    path: String,
    query: String,
    body: Vec<u8>,
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// Liest eine Anfrage (Kopf und Körper) mit Größen- und Zeitgrenze.
fn read_request<R: Read>(r: &mut R) -> Result<Request, u16> {
    let deadline = Instant::now() + IO_TIMEOUT;
    let mut buf: Vec<u8> = Vec::with_capacity(2048);
    let mut tmp = [0u8; 2048];
    let head_end = loop {
        if let Some(p) = find(&buf, b"\r\n\r\n") {
            break p + 4;
        }
        if buf.len() > MAX_HEAD {
            return Err(413);
        }
        if Instant::now() > deadline {
            return Err(400);
        }
        match r.read(&mut tmp) {
            Ok(0) | Err(_) => return Err(400),
            Ok(n) => buf.extend_from_slice(&tmp[..n]),
        }
    };
    if head_end > MAX_HEAD {
        return Err(413);
    }
    let mut headers = [httparse::EMPTY_HEADER; 32];
    let mut req = httparse::Request::new(&mut headers);
    if !matches!(
        req.parse(&buf[..head_end]),
        Ok(httparse::Status::Complete(_))
    ) {
        return Err(400);
    }
    let method = req.method.ok_or(400u16)?.to_string();
    let target = req.path.ok_or(400u16)?;
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let len = req
        .headers
        .iter()
        .find(|h| h.name.eq_ignore_ascii_case("content-length"))
        .map(|h| {
            std::str::from_utf8(h.value)
                .ok()
                .and_then(|s| s.trim().parse::<usize>().ok())
                .ok_or(400u16)
        })
        .transpose()?
        .unwrap_or(0);
    if len > MAX_BODY {
        return Err(413);
    }
    let (path, query) = (path.to_string(), query.to_string());
    let mut body = buf[head_end..].to_vec();
    while body.len() < len {
        if Instant::now() > deadline {
            return Err(400);
        }
        match r.read(&mut tmp) {
            Ok(0) | Err(_) => return Err(400),
            Ok(n) => body.extend_from_slice(&tmp[..n]),
        }
    }
    body.truncate(len);
    Ok(Request {
        method,
        path,
        query,
        body,
    })
}

fn serve_one<S: Read + Write>(stream: &mut S, gate: &Gate, peer: &str) {
    let resp = match read_request(stream) {
        Ok(req) => route(
            gate,
            &req.method,
            &req.path,
            &req.query,
            &req.body,
            peer,
            Instant::now(),
        ),
        Err(code) => Response::error(code, "Ungültige Anfrage"),
    };
    let _ = write_response(stream, &resp);
}

/// Erzeugt ein selbst ausgestelltes Zertifikat für die Adresse und `localhost`.
fn make_tls(ip: IpAddr) -> Result<Arc<ServerConfig>, String> {
    let certified =
        rcgen::generate_simple_self_signed(vec!["localhost".to_string(), ip.to_string()])
            .map_err(|e| format!("Zertifikat: {e}"))?;
    let cert: CertificateDer<'static> = certified.cert.der().clone();
    let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(
        certified.signing_key.serialize_der(),
    ));
    let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    let cfg = ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| format!("TLS: {e}"))?
        .with_no_client_auth()
        .with_single_cert(vec![cert], key)
        .map_err(|e| format!("TLS-Schlüssel: {e}"))?;
    Ok(Arc::new(cfg))
}

/// Der laufende Server. Beim Ausschalten (oder Wegwerfen) ist der Port zu.
pub struct WebServer {
    addr: SocketAddr,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl WebServer {
    /// Öffnet `ip:preferred` (sonst einen freien Port) und startet den Thread.
    pub fn start(ip: IpAddr, preferred: u16, gate: Gate) -> Result<Self, String> {
        let tls = make_tls(ip)?;
        let listener = TcpListener::bind(SocketAddr::new(ip, preferred))
            .or_else(|_| TcpListener::bind(SocketAddr::new(ip, 0)))
            .map_err(|e| format!("Port nicht zu öffnen: {e}"))?;
        let addr = listener.local_addr().map_err(|e| e.to_string())?;
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let thread = std::thread::Builder::new()
            .name("woa-guest-web".into())
            .spawn(move || accept_loop(listener, tls, gate, flag))
            .map_err(|e| e.to_string())?;
        tracing::info!(%addr, "guest web: listening (https)");
        Ok(Self {
            addr,
            stop,
            thread: Some(thread),
        })
    }

    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for WebServer {
    fn drop(&mut self) {
        self.stop();
    }
}

fn accept_loop(listener: TcpListener, tls: Arc<ServerConfig>, gate: Gate, stop: Arc<AtomicBool>) {
    let conns = Arc::new(AtomicUsize::new(0));
    while !stop.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((stream, peer)) => {
                if conns.load(Ordering::Relaxed) >= MAX_CONNS {
                    tracing::warn!(%peer, "guest web: too many connections");
                    continue;
                }
                conns.fetch_add(1, Ordering::Relaxed);
                let (tls, gate, conns2) = (Arc::clone(&tls), gate.clone(), Arc::clone(&conns));
                let spawned = std::thread::Builder::new()
                    .name("woa-guest-conn".into())
                    .spawn(move || {
                        handle_conn(stream, peer, tls, &gate);
                        conns2.fetch_sub(1, Ordering::Relaxed);
                    });
                if spawned.is_err() {
                    conns.fetch_sub(1, Ordering::Relaxed);
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(e) => {
                tracing::warn!(error = %e, "guest web: accept failed");
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    }
    tracing::info!("guest web: stopped");
}

fn handle_conn(stream: TcpStream, peer: SocketAddr, tls: Arc<ServerConfig>, gate: &Gate) {
    // Der Port der Gegenseite ist kein Merkmal; nur die Adresse zeigen.
    let peer_ip = peer.ip().to_string();
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
    let conn = match ServerConnection::new(tls) {
        Ok(c) => c,
        Err(e) => {
            tracing::debug!(error = %e, "guest web: tls setup failed");
            return;
        }
    };
    let mut tls_stream = StreamOwned::new(conn, stream);
    serve_one(&mut tls_stream, gate, &peer_ip);
    tls_stream.conn.send_close_notify();
    let _ = tls_stream.flush();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    const OFFER: &str = "v=0\r\no=- 1 1 IN IP4 0.0.0.0\r\n";

    fn body(r: &Response) -> Value {
        serde_json::from_slice(&r.body).expect("json")
    }

    #[test]
    fn page_is_served_and_unknown_paths_404() {
        let g = Gate::new();
        let now = Instant::now();
        let r = route(&g, "GET", "/gast", "i=abc", b"", "x", now);
        assert_eq!(r.status, 200);
        assert!(r.content_type.starts_with("text/html"));
        assert!(String::from_utf8_lossy(&r.body).contains("getUserMedia"));
        assert_eq!(
            route(&g, "GET", "/etc/passwd", "", b"", "x", now).status,
            404
        );
        assert_eq!(route(&g, "DELETE", "/gast", "", b"", "x", now).status, 405);
    }

    #[test]
    fn join_state_leave_roundtrip() {
        let g = Gate::new();
        let now = Instant::now();
        let tok = g.create_invite(now).expect("invite");
        let q = format!("i={tok}");
        assert_eq!(
            body(&route(&g, "GET", "/gast/check", &q, b"", "x", now))["ok"],
            true
        );
        let req = json!({ "invite": tok, "name": "Anna", "sdp": OFFER }).to_string();
        let r = route(
            &g,
            "POST",
            "/gast/join",
            "",
            req.as_bytes(),
            "10.0.0.5",
            now,
        );
        assert_eq!(r.status, 200);
        let ticket = body(&r)["ticket"].as_str().expect("ticket").to_string();
        // Einladung ist verbraucht.
        assert_eq!(
            body(&route(&g, "GET", "/gast/check", &q, b"", "x", now))["ok"],
            false
        );
        let r = route(
            &g,
            "POST",
            "/gast/join",
            "",
            req.as_bytes(),
            "10.0.0.5",
            now,
        );
        assert_eq!(r.status, 403);
        let st = |now| {
            body(&route(
                &g,
                "GET",
                "/gast/state",
                &format!("t={ticket}"),
                b"",
                "x",
                now,
            ))
        };
        assert_eq!(st(now)["state"], "waiting");
        g.take_for_accept(&ticket).expect("take");
        g.set_answer(&ticket, "v=0 antwort".into());
        let s = st(now);
        assert_eq!(s["state"], "accepted");
        assert_eq!(s["sdp"], "v=0 antwort");
        let leave = json!({ "t": ticket }).to_string();
        assert_eq!(
            route(&g, "POST", "/gast/leave", "", leave.as_bytes(), "x", now).status,
            200
        );
        assert_eq!(g.take_left(), vec![ticket]);
    }

    #[test]
    fn garbage_is_refused_without_panic() {
        let g = Gate::new();
        let now = Instant::now();
        assert_eq!(
            route(&g, "POST", "/gast/join", "", b"{kaputt", "x", now).status,
            400
        );
        assert_eq!(
            route(&g, "POST", "/gast/join", "", b"[]", "x", now).status,
            403
        );
        // Kennungen mit Sonderzeichen werden nicht übernommen.
        assert_eq!(query_id("t=../../etc", "t"), "");
        assert_eq!(query_id("x=1&t=ab12", "t"), "ab12");
        assert_eq!(query_id(&format!("t={}", "a".repeat(65)), "t"), "");
        assert_eq!(
            body(&route(&g, "GET", "/gast/state", "t=zz", b"", "x", now))["state"],
            "ended"
        );
    }

    #[test]
    fn requests_are_parsed_with_limits() {
        let ok = b"POST /gast/join?a=b HTTP/1.1\r\nHost: x\r\nContent-Length: 5\r\n\r\nhello";
        let r = read_request(&mut Cursor::new(&ok[..])).expect("request");
        assert_eq!(
            (r.method.as_str(), r.path.as_str(), r.query.as_str()),
            ("POST", "/gast/join", "a=b")
        );
        assert_eq!(r.body, b"hello");
        let big = format!(
            "POST / HTTP/1.1\r\nContent-Length: {}\r\n\r\n",
            MAX_BODY + 1
        );
        assert_eq!(
            read_request(&mut Cursor::new(big.into_bytes())).err(),
            Some(413)
        );
        let long = format!("GET /{} HTTP/1.1\r\n\r\n", "a".repeat(MAX_HEAD + 10));
        assert_eq!(
            read_request(&mut Cursor::new(long.into_bytes())).err(),
            Some(413)
        );
        assert_eq!(
            read_request(&mut Cursor::new(&b"GET / HTTP/1.1\r\n"[..])).err(),
            Some(400)
        );
        let short = b"POST / HTTP/1.1\r\nContent-Length: 10\r\n\r\nabc";
        assert_eq!(read_request(&mut Cursor::new(&short[..])).err(), Some(400));
    }

    #[test]
    fn tls_config_can_be_built() {
        assert!(make_tls("127.0.0.1".parse().expect("ip")).is_ok());
    }
}
