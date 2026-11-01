//! MCP über HTTP (Streamable HTTP) für Clients, die keinen Prozess starten können
//! (Claude Desktop über einen Vermittler, Claude Code, eigene Werkzeuge).
//!
//! Sicherheit: MCP-Werkzeuge schreiben Dateien und lesen Pfade, deshalb
//!
//! - hört der Server vorgabegemäß nur auf `127.0.0.1`; jede andere Adresse ist eine
//!   ausdrückliche Wahl des Nutzers,
//! - verlangt jede Anfrage `Authorization: Bearer <Token>`; der Vergleich läuft in
//!   gleichbleibender Zeit,
//! - prüft rmcp den `Host` (nur Loopback, solange der Server nur dort hört) und wir den
//!   `Origin` (nur lokale Seiten), gegen DNS-Rebinding aus dem Browser,
//! - steht weder das Token noch der Kopf je in einem Protokolleintrag.

use std::net::SocketAddr;
use std::sync::Arc;

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::service::service_fn;
use hyper_util::rt::TokioIo;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;
use tower_service::Service;

use crate::{AgentError, WoaAgent};

/// Pfad, unter dem der Server antwortet.
pub const MCP_PATH: &str = "/mcp";

/// Vergleich in gleichbleibender Zeit (Länge ist kein Geheimnis, der Inhalt schon).
pub fn tokens_equal(a: &[u8], b: &[u8]) -> bool {
    let mut diff = (a.len() ^ b.len()) as u32;
    let n = a.len().max(b.len());
    for i in 0..n {
        let x = a.get(i).copied().unwrap_or(0);
        let y = b.get(i).copied().unwrap_or(0);
        diff |= u32::from(x ^ y);
    }
    diff == 0
}

/// Gehört der `Authorization`-Kopf zu `token`? Ein leeres Token lässt nie etwas durch.
pub fn bearer_matches(header: Option<&str>, token: &str) -> bool {
    if token.is_empty() {
        return false;
    }
    let Some(h) = header else { return false };
    let Some((scheme, value)) = h.split_once(' ') else {
        return false;
    };
    scheme.eq_ignore_ascii_case("bearer") && tokens_equal(value.trim().as_bytes(), token.as_bytes())
}

type Body = http_body_util::combinators::UnsyncBoxBody<Bytes, std::convert::Infallible>;

fn plain(status: u16, text: &'static str) -> http::Response<Body> {
    let body = Full::new(Bytes::from_static(text.as_bytes()))
        .map_err(|never| match never {})
        .boxed_unsync();
    let mut r = http::Response::new(body);
    *r.status_mut() = http::StatusCode::from_u16(status).unwrap_or(http::StatusCode::BAD_REQUEST);
    r.headers_mut().insert(
        http::header::CONTENT_TYPE,
        http::HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    // Abgewiesen: die Verbindung endet nach der Antwort.
    r.headers_mut().insert(
        http::header::CONNECTION,
        http::HeaderValue::from_static("close"),
    );
    if status == 401 {
        r.headers_mut().insert(
            http::header::WWW_AUTHENTICATE,
            http::HeaderValue::from_static("Bearer"),
        );
    }
    r
}

/// Wie [`serve_http`] eingestellt wird.
#[derive(Clone)]
pub struct HttpOptions {
    /// Zugriffstoken; leer = der Server startet nicht.
    pub token: String,
    /// `true`, wenn der Server auf mehr als der eigenen Maschine hört. Dann prüft rmcp den
    /// `Host` nicht mehr (der Client nennt die Adresse, unter der er uns erreicht).
    pub reachable_from_network: bool,
    /// Zweites, kurzlebiges Geheimnis je App-Start (Live-Modus, `woa-headless mcp` reicht
    /// damit durch statt mit dem dauerhaften Token). `None`: nur `token`.
    pub live_secret: Option<String>,
    /// Beendet den Server.
    pub cancel: CancellationToken,
}

impl std::fmt::Debug for HttpOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HttpOptions")
            .field("token", &"***")
            .field("reachable_from_network", &self.reachable_from_network)
            .finish()
    }
}

/// Bedient `agent` auf `listener`, bis `opts.cancel` gesetzt wird. Alle Sitzungen teilen
/// sich das eine Projekt von `agent` ([`WoaAgent`] oder der Live-Agent der App).
pub async fn serve_http<S>(
    listener: TcpListener,
    agent: S,
    opts: HttpOptions,
) -> Result<(), AgentError>
where
    S: rmcp::ServerHandler + Clone + Send + Sync + 'static,
{
    if opts.token.is_empty() {
        return Err(AgentError::Transport(
            "Ohne Zugriffstoken startet der HTTP-Server nicht".into(),
        ));
    }
    let mut config = StreamableHttpServerConfig::default().with_allowed_origins([
        "http://localhost:*",
        "http://127.0.0.1:*",
        "http://[::1]:*",
    ]);
    config.cancellation_token = opts.cancel.child_token();
    if opts.reachable_from_network {
        config = config.disable_allowed_hosts();
    }
    let service = StreamableHttpService::new(
        move || Ok(agent.clone()),
        Arc::new(LocalSessionManager::default()),
        config,
    );
    let token: Arc<(String, Option<String>)> = Arc::new((
        opts.token.clone(),
        opts.live_secret.clone().filter(|s| !s.is_empty()),
    ));
    loop {
        let (stream, _peer) = tokio::select! {
            () = opts.cancel.cancelled() => return Ok(()),
            r = listener.accept() => match r {
                Ok(x) => x,
                Err(e) => {
                    tracing_warn(&format!("accept failed: {e}"));
                    continue;
                }
            },
        };
        let service = service.clone();
        let token = token.clone();
        let cancel = opts.cancel.clone();
        tokio::spawn(async move {
            let svc = service_fn(move |req: http::Request<Incoming>| {
                let mut service = service.clone();
                let token = token.clone();
                async move {
                    let resp: http::Response<Body> = if req.uri().path() != MCP_PATH {
                        drain(req.into_body()).await;
                        plain(404, "nicht gefunden")
                    } else if !{
                        let h = req
                            .headers()
                            .get(http::header::AUTHORIZATION)
                            .and_then(|v| v.to_str().ok());
                        bearer_matches(h, &token.0)
                            || token.1.as_deref().is_some_and(|s| bearer_matches(h, s))
                    } {
                        drain(req.into_body()).await;
                        plain(401, "Zugriffstoken fehlt oder stimmt nicht")
                    } else {
                        match service.call(req).await {
                            Ok(r) => r.map(|b| b.boxed_unsync()),
                            Err(never) => match never {},
                        }
                    };
                    Ok::<_, std::convert::Infallible>(resp)
                }
            });
            let conn = hyper::server::conn::http1::Builder::new()
                .serve_connection(TokioIo::new(stream), svc)
                .with_upgrades();
            tokio::select! {
                () = cancel.cancelled() => {}
                _ = conn => {}
            }
        });
    }
}

/// Höchstens so viele Bytes liest der Server von einer abgewiesenen Anfrage weg.
const DRAIN_LIMIT: u64 = 1024 * 1024;
/// Höchstens so viele abgewiesene Anfragen werden gleichzeitig weggelesen; jede weitere
/// bekommt die Antwort sofort.
static DRAINS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(16);

/// Liest den Body einer abgewiesenen Anfrage weg und verwirft jedes Stück sofort (höchstens
/// [`DRAIN_LIMIT`] Bytes, höchstens 2 s, höchstens 16 gleichzeitig). Schließt der Server mit
/// ungelesenen Bytes, wird unter Windows aus der Antwort ein RST, und der Client sieht statt
/// der 401 „connection aborted (os error 10053)“ (#256). Gibt die gelesenen Bytes zurück.
async fn drain<B>(body: B) -> u64
where
    B: hyper::body::Body + Unpin,
{
    let Ok(_permit) = DRAINS.try_acquire() else {
        return 0;
    };
    let mut body = body;
    let mut read = 0u64;
    let _ = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while read < DRAIN_LIMIT {
            match body.frame().await {
                Some(Ok(f)) => {
                    if let Some(d) = f.data_ref() {
                        read += bytes::Buf::remaining(d) as u64;
                    }
                }
                _ => break,
            }
        }
    })
    .await;
    read
}

// Der Crate hat kein `tracing`; Verbindungsfehler gehören auf stderr (stdout bleibt MCP).
fn tracing_warn(msg: &str) {
    eprintln!("woa mcp http: {msg}");
}

/// Öffnet das Projekt `dir` und bedient MCP über HTTP auf `addr`, bis der Prozess endet.
/// Blockiert. Schreibt die Adresse nach stderr.
pub fn run_http(
    dir: impl AsRef<std::path::Path>,
    addr: SocketAddr,
    token: String,
) -> Result<(), AgentError> {
    let agent = WoaAgent::open_or_create(dir)?;
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| AgentError::Transport(e.to_string()))?;
    rt.block_on(async move {
        let listener = TcpListener::bind(addr)
            .await
            .map_err(|e| AgentError::Transport(format!("Adresse {addr} nicht nutzbar: {e}")))?;
        let local = listener
            .local_addr()
            .map_err(|e| AgentError::Transport(e.to_string()))?;
        eprintln!("WOA MCP: http://{local}{MCP_PATH} (Zugriffstoken als Bearer-Kopf)");
        serve_http(
            listener,
            agent,
            HttpOptions {
                token,
                reachable_from_network: !addr.ip().is_loopback(),
                live_secret: None,
                cancel: CancellationToken::new(),
            },
        )
        .await
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ein Body aus `n` Stücken zu 64 KiB; zählt, wie viele gelesen wurden.
    struct Chunks {
        left: usize,
        polled: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    }

    impl hyper::body::Body for Chunks {
        type Data = Bytes;
        type Error = std::convert::Infallible;
        fn poll_frame(
            mut self: std::pin::Pin<&mut Self>,
            _: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Option<Result<hyper::body::Frame<Bytes>, Self::Error>>> {
            if self.left == 0 {
                return std::task::Poll::Ready(None);
            }
            self.left -= 1;
            self.polled
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            std::task::Poll::Ready(Some(Ok(hyper::body::Frame::data(Bytes::from(vec![
                0u8;
                64 * 1024
            ])))))
        }
    }

    #[test]
    fn drain_stops_at_the_limit_and_reads_small_bodies_fully() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let polled = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let big = Chunks {
            left: 48,
            polled: polled.clone(),
        };
        let read = rt.block_on(drain(big));
        assert_eq!(read, DRAIN_LIMIT);
        assert_eq!(polled.load(std::sync::atomic::Ordering::SeqCst), 16);
        let small = Chunks {
            left: 2,
            polled: polled.clone(),
        };
        assert_eq!(rt.block_on(drain(small)), 128 * 1024);
        let r = plain(401, "x");
        assert_eq!(r.headers()[http::header::CONNECTION], "close");
    }

    #[test]
    fn bearer_check() {
        assert!(bearer_matches(Some("Bearer abc"), "abc"));
        assert!(bearer_matches(Some("bearer abc"), "abc"));
        assert!(!bearer_matches(Some("Bearer abd"), "abc"));
        assert!(!bearer_matches(Some("Bearer abcd"), "abc"));
        assert!(!bearer_matches(Some("Bearer ab"), "abc"));
        assert!(!bearer_matches(Some("Basic abc"), "abc"));
        assert!(!bearer_matches(Some("abc"), "abc"));
        assert!(!bearer_matches(None, "abc"));
        assert!(!bearer_matches(Some("Bearer "), ""));
        assert!(!bearer_matches(Some("Bearer"), ""));
    }
}
