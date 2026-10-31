//! Live-Modus: Läuft die WOA-App, arbeitet der MCP-Server auf dem Projekt, das in der App
//! offen ist (docs/konzept.md: GUI und Agent schicken Commands an dieselbe Schicht).
//!
//! Die App hostet dafür einen MCP-HTTP-Server nur auf `127.0.0.1`, mit dem Zugriffstoken
//! aus dem Schlüsselbund und der Origin-Prüfung aus [`crate::http`]. Port, Prozessnummer und
//! ein zufälliges **Live-Geheimnis je Start** schreibt sie in die Lock-Datei [`LOCK_FILE`]
//! im Datenordner (unter Unix Modus 0600), nie das dauerhafte Token. `woa-headless mcp`
//! (stdio, so startet Claude Desktop) findet die Datei, prüft Prozess und Port und reicht
//! jede Zeile mit dem Live-Geheimnis an die App durch ([`forward`]). Ein fremder Prozess
//! auf dem Port bekommt so höchstens ein Geheimnis, das mit der App stirbt. Sonst arbeitet
//! `woa-headless` wie bisher auf einem eigenen Projekt.
//!
//! Es gibt nur einen Projektzustand: Für jeden Werkzeugaufruf **leiht** der GUI-Thread seine
//! [`Session`] her ([`LiveDesk::serve_pending`]), wartet, bis das Werkzeug fertig ist, und
//! nimmt sie zurück. Aufrufe laufen so nacheinander und zwischen zwei Bildern, ohne Race.
//! Rendern und Exportieren klonen das Projekt wie bisher; der Audio-Thread ist davon frei.
//! Jeder Aufruf ist ein Rückgängig-Schritt „Agent: <werkzeug>“.

use std::io::{BufRead, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, ListToolsResult,
    PaginatedRequestParams, ServerConfig, Tool,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData as McpError, RoleServer, ServerHandler};
use tokio_util::sync::CancellationToken;
use woa_model::Session;

use crate::http::{HttpOptions, MCP_PATH, serve_http};
use crate::{AGENT_LABEL, AgentError, WoaAgent};

/// Lock-Datei im Datenordner: `{"port": …, "pid": …}` der laufenden App.
pub const LOCK_FILE: &str = "mcp-live.json";
/// So lange wartet ein Werkzeugaufruf auf die App (sie zeichnet z. B. minimiert nicht).
const LOAN_WAIT: Duration = Duration::from_secs(20);
/// Höchste Antwortgröße beim Durchreichen (Spektrogramme als PNG).
const MAX_REPLY: u64 = 64 * 1024 * 1024;

/// Die geliehene Sitzung. Geht sie verloren (Abbruch, Panik), bringt `Drop` sie zurück.
pub struct Loan {
    session: Option<Session>,
    dir: PathBuf,
    back: mpsc::Sender<Session>,
}

impl Drop for Loan {
    fn drop(&mut self) {
        if let Some(s) = self.session.take() {
            let _ = self.back.send(s);
        }
    }
}

struct LoanRequest {
    give: tokio::sync::oneshot::Sender<Loan>,
}

/// GUI-Seite: nimmt die Leihwünsche der Werkzeugaufrufe an.
pub struct LiveDesk {
    rx: mpsc::Receiver<LoanRequest>,
    activity: Arc<AtomicU64>,
}

impl LiveDesk {
    /// Bedient alle wartenden Aufrufe mit `session`; `dir` ist der Projektordner (bei einem
    /// ungespeicherten Projekt der Wiederherstellungsordner). Blockiert je Aufruf, bis das
    /// Werkzeug fertig ist. Gibt die Zahl der bedienten Aufrufe zurück.
    pub fn serve_pending(&self, session: &mut Session, dir: &Path) -> usize {
        let mut served = 0;
        while let Ok(req) = self.rx.try_recv() {
            let (back, back_rx) = mpsc::channel();
            let loan = Loan {
                session: Some(std::mem::replace(session, Session::new(""))),
                dir: dir.to_path_buf(),
                back,
            };
            if let Err(mut loan) = req.give.send(loan) {
                // Der Aufruf hat aufgegeben: Sitzung sofort zurück.
                if let Some(s) = loan.session.take() {
                    *session = s;
                }
                continue;
            }
            match back_rx.recv() {
                Ok(s) => *session = s,
                // Kann nicht passieren: `Loan::drop` schickt immer zurück.
                Err(_) => eprintln!("woa live: Sitzung nicht zurückgekommen"),
            }
            served += 1;
        }
        served
    }

    /// Sekunden seit dem letzten Kontakt eines KI-Clients, `None`: noch keiner.
    pub fn idle_seconds(&self) -> Option<u64> {
        let t = self.activity.load(Ordering::Relaxed);
        (t > 0).then(|| unix_now().saturating_sub(t))
    }
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

type Wake = Arc<dyn Fn() + Send + Sync>;

/// MCP-Server, der für jeden Aufruf die Sitzung der App leiht und dann [`WoaAgent`] fragt.
#[derive(Clone)]
pub struct LiveAgent {
    tx: mpsc::Sender<LoanRequest>,
    wake: Wake,
    activity: Arc<AtomicU64>,
    /// Werkzeugliste und Server-Info, ohne eigenen Projektzustand.
    catalog: WoaAgent,
    /// Ein Agent je Projektordner, bleibt bestehen (seine Aufräumarbeit läuft erst am Ende).
    current: Arc<Mutex<Option<(PathBuf, WoaAgent)>>>,
}

impl LiveAgent {
    /// Neuer Live-Agent und sein Gegenstück für den GUI-Thread. `wake` weckt die App
    /// (z. B. `ctx.request_repaint()`), damit sie den Aufruf sofort bedient.
    pub fn new(wake: Wake) -> (Self, LiveDesk) {
        let (tx, rx) = mpsc::channel();
        let activity = Arc::new(AtomicU64::new(0));
        (
            Self {
                tx,
                wake,
                activity: activity.clone(),
                catalog: WoaAgent::with_session(Session::new(""), PathBuf::new()),
                current: Arc::new(Mutex::new(None)),
            },
            LiveDesk { rx, activity },
        )
    }

    fn touch(&self) {
        self.activity.store(unix_now(), Ordering::Relaxed);
    }

    fn agent_for(&self, dir: &Path) -> WoaAgent {
        let mut cur = self.current.lock().unwrap_or_else(|e| e.into_inner());
        match &*cur {
            Some((d, a)) if d == dir => a.clone(),
            _ => {
                let a = WoaAgent::with_session(Session::new(""), dir.to_path_buf());
                *cur = Some((dir.to_path_buf(), a.clone()));
                a
            }
        }
    }

    async fn borrow(&self) -> Result<Loan, String> {
        let (give, got) = tokio::sync::oneshot::channel();
        self.tx
            .send(LoanRequest { give })
            .map_err(|_| "Die WOA-App ist geschlossen".to_string())?;
        (self.wake)();
        match tokio::time::timeout(LOAN_WAIT, got).await {
            Ok(Ok(loan)) => Ok(loan),
            Ok(Err(_)) => Err("Die WOA-App ist geschlossen".into()),
            Err(_) => {
                Err("Die WOA-App antwortet nicht (Fenster minimiert oder beschäftigt)".into())
            }
        }
    }
}

/// Die geliehene Sitzung steckt im Agenten; `Drop` holt sie heraus und gibt sie zurück.
struct Lent {
    loan: Loan,
    agent: WoaAgent,
}

impl Lent {
    fn new(mut loan: Loan, agent: WoaAgent) -> Self {
        if let Some(s) = loan.session.take() {
            let shared = agent.session();
            let mut g = shared.lock().unwrap_or_else(|e| e.into_inner());
            *g = s;
        }
        Self { loan, agent }
    }

    fn with<T>(&self, f: impl FnOnce(&mut Session) -> T) -> T {
        let shared = self.agent.session();
        let mut g = shared.lock().unwrap_or_else(|e| e.into_inner());
        f(&mut g)
    }
}

impl Drop for Lent {
    fn drop(&mut self) {
        let s = self.with(|s| std::mem::replace(s, Session::new("")));
        self.loan.session = Some(s);
    }
}

fn text_result(ok: bool, text: String) -> CallToolResponse {
    let content = vec![ContentBlock::text(text)];
    if ok {
        CallToolResult::success(content)
    } else {
        CallToolResult::error(content)
    }
    .into()
}

impl ServerHandler for LiveAgent {
    fn get_info(&self) -> ServerConfig {
        self.touch();
        self.catalog.get_info()
    }

    async fn list_tools(
        &self,
        request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        self.touch();
        self.catalog.list_tools(request, context).await
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        self.catalog.get_tool(name)
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        self.touch();
        let loan = match self.borrow().await {
            Ok(l) => l,
            Err(e) => return Ok(text_result(false, e)),
        };
        let dir = loan.dir.clone();
        let lent = Lent::new(loan, self.agent_for(&dir));
        let name = request.name.to_string();
        if name == "save" {
            // Gespeichert wird der Ordner der App, nie ein anderer.
            return Ok(lent.with(|s| match s.path().map(Path::to_path_buf) {
                None => text_result(
                    false,
                    "Das Projekt ist in der App noch nicht gespeichert: dort einmal \
                     „Speichern unter …“ wählen"
                        .into(),
                ),
                Some(p) => match s.save() {
                    Ok(()) => text_result(true, serde_json::json!({ "saved": p }).to_string()),
                    Err(e) => text_result(false, e.to_string()),
                },
            }));
        }
        // undo/redo laufen nicht in einer Gruppe (sie sind selbst Verlaufsschritte).
        let grouped = !matches!(name.as_str(), "undo" | "redo");
        if grouped {
            lent.with(|s| s.begin_group(format!("{AGENT_LABEL}: {name}")));
        }
        let agent = lent.agent.clone();
        let r = agent.call_tool(request, context).await;
        if grouped {
            lent.with(|s| {
                let _ = s.end_group();
            });
        }
        drop(lent);
        r
    }
}

/// Der MCP-Server der App. Endet mit `Drop` (Lock-Datei weg).
pub struct LiveServer {
    desk: LiveDesk,
    port: u16,
    lock: PathBuf,
    cancel: CancellationToken,
}

impl std::fmt::Debug for LiveServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LiveServer")
            .field("port", &self.port)
            .finish()
    }
}

impl LiveServer {
    /// Startet den Server auf `127.0.0.1:port` (0 = freier Port) in einem eigenen Thread
    /// und schreibt die Lock-Datei nach `data_dir`. Ohne Token startet nichts.
    pub fn start(
        data_dir: &Path,
        token: String,
        port: u16,
        wake: Wake,
    ) -> Result<Self, AgentError> {
        if token.is_empty() {
            return Err(AgentError::Transport(
                "Ohne Zugriffstoken kein Live-Modus".into(),
            ));
        }
        let std_listener = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, port))
            .map_err(|e| AgentError::Transport(format!("Port {port}: {e}")))?;
        std_listener
            .set_nonblocking(true)
            .map_err(|e| AgentError::Transport(e.to_string()))?;
        let port = std_listener
            .local_addr()
            .map_err(|e| AgentError::Transport(e.to_string()))?
            .port();
        let (agent, desk) = LiveAgent::new(wake);
        let cancel = CancellationToken::new();
        let secret = new_secret()?;
        let opts = HttpOptions {
            token,
            reachable_from_network: false,
            live_secret: Some(secret.clone()),
            cancel: cancel.clone(),
        };
        std::thread::Builder::new()
            .name("woa-mcp-live".into())
            .spawn(move || {
                let rt = match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(rt) => rt,
                    Err(e) => return eprintln!("woa live: {e}"),
                };
                rt.block_on(async move {
                    match tokio::net::TcpListener::from_std(std_listener) {
                        Ok(l) => {
                            if let Err(e) = serve_http(l, agent, opts).await {
                                eprintln!("woa live: {e}");
                            }
                        }
                        Err(e) => eprintln!("woa live: {e}"),
                    }
                });
            })
            .map_err(|e| AgentError::Transport(e.to_string()))?;
        let lock = data_dir.join(LOCK_FILE);
        let body = serde_json::json!({
            "port": port,
            "pid": std::process::id(),
            "secret": secret,
        })
        .to_string();
        let tmp = data_dir.join(format!("{LOCK_FILE}.tmp"));
        let written = std::fs::create_dir_all(data_dir)
            .and_then(|()| write_private(&tmp, body.as_bytes()))
            .and_then(|()| std::fs::rename(&tmp, &lock));
        if let Err(e) = written {
            cancel.cancel();
            return Err(AgentError::Transport(format!("{}: {e}", lock.display())));
        }
        Ok(Self {
            desk,
            port,
            lock,
            cancel,
        })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    /// Siehe [`LiveDesk::serve_pending`].
    pub fn serve_pending(&self, session: &mut Session, dir: &Path) -> usize {
        self.desk.serve_pending(session, dir)
    }

    /// Siehe [`LiveDesk::idle_seconds`].
    pub fn idle_seconds(&self) -> Option<u64> {
        self.desk.idle_seconds()
    }
}

impl Drop for LiveServer {
    fn drop(&mut self) {
        self.cancel.cancel();
        // Nur die eigene Lock-Datei entfernen, nicht die einer zweiten App.
        if lock_port(&self.lock) == Some(self.port) {
            let _ = std::fs::remove_file(&self.lock);
        }
    }
}

/// 32 Zufallsbytes als Hex, je App-Start neu.
pub fn new_secret() -> Result<String, AgentError> {
    let mut buf = [0u8; 32];
    getrandom::fill(&mut buf)
        .map_err(|e| AgentError::Transport(format!("Zufallsgenerator: {e}")))?;
    Ok(buf.iter().map(|b| format!("{b:02x}")).collect())
}

/// Schreibt eine Datei, die nur der Benutzer lesen darf (Unix 0600; unter Windows liegt der
/// Datenordner im Benutzerprofil).
fn write_private(path: &Path, data: &[u8]) -> std::io::Result<()> {
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600);
    }
    let mut f = o.open(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        f.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    f.write_all(data)
}

fn read_lock(lock: &Path) -> Option<serde_json::Value> {
    serde_json::from_str(&std::fs::read_to_string(lock).ok()?).ok()
}

fn lock_port(lock: &Path) -> Option<u16> {
    u16::try_from(read_lock(lock)?.get("port")?.as_u64()?).ok()
}

/// Lebt der Prozess noch? Unter Linux über `/proc`, sonst nicht prüfbar (dann `true`, der
/// Port muss trotzdem antworten).
fn pid_alive(pid: u64) -> bool {
    if cfg!(target_os = "linux") {
        Path::new(&format!("/proc/{pid}")).exists()
    } else {
        true
    }
}

/// Die laufende App: Port und Live-Geheimnis dieses Starts.
#[derive(Clone, PartialEq, Eq)]
pub struct RunningApp {
    pub port: u16,
    pub key: String,
}

impl std::fmt::Debug for RunningApp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RunningApp")
            .field("port", &self.port)
            .field("key", &"***")
            .finish()
    }
}

/// Die laufende App, wenn ihre Lock-Datei in `data_dir` liegt, ihr Prozess lebt und der
/// Port antwortet. Eine verwaiste Datei (App abgestürzt, PID tot) ergibt `None`.
pub fn running_app(data_dir: &Path) -> Option<RunningApp> {
    let v = read_lock(&data_dir.join(LOCK_FILE))?;
    let port = u16::try_from(v.get("port")?.as_u64()?).ok()?;
    let secret = v.get("secret")?.as_str()?.to_string();
    if secret.is_empty() || !pid_alive(v.get("pid")?.as_u64()?) {
        return None;
    }
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    TcpStream::connect_timeout(&addr, Duration::from_millis(500)).ok()?;
    Some(RunningApp { port, key: secret })
}

/// Reicht JSON-RPC-Zeilen von `input` mit dem Bearer `token` (das Live-Geheimnis aus
/// [`running_app`]) an die App auf `port` durch und schreibt ihre
/// Antworten zeilenweise nach `output`, bis `input` endet. So spricht ein stdio-Client
/// (Claude Desktop) mit der offenen App, ohne dass er etwas umstellen muss.
pub fn forward(
    port: u16,
    token: &str,
    input: impl BufRead,
    mut output: impl Write,
) -> Result<(), AgentError> {
    let io = |e: std::io::Error| AgentError::Transport(e.to_string());
    let http: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .build()
        .into();
    let url = format!("http://127.0.0.1:{port}{MCP_PATH}");
    let auth = format!("Bearer {token}");
    let mut session_id: Option<String> = None;
    let mut protocol: Option<String> = None;
    for line in input.lines() {
        let line = line.map_err(io)?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let id = serde_json::from_str::<serde_json::Value>(line)
            .ok()
            .and_then(|v| v.get("id").cloned());
        let mut req = http
            .post(&url)
            .header("Authorization", &auth)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json, text/event-stream");
        if let Some(s) = &session_id {
            req = req.header("Mcp-Session-Id", s);
        }
        if let Some(p) = &protocol {
            req = req.header("MCP-Protocol-Version", p);
        }
        let replies = match req.send(line) {
            Err(e) => Err(format!("WOA-App nicht erreichbar: {e}")),
            Ok(mut resp) => {
                if let Some(s) = resp
                    .headers()
                    .get("mcp-session-id")
                    .and_then(|v| v.to_str().ok())
                {
                    session_id = Some(s.to_string());
                }
                let status = resp.status().as_u16();
                let sse = resp
                    .headers()
                    .get("content-type")
                    .and_then(|v| v.to_str().ok())
                    .is_some_and(|c| c.starts_with("text/event-stream"));
                let body = resp
                    .body_mut()
                    .with_config()
                    .limit(MAX_REPLY)
                    .read_to_string()
                    .unwrap_or_default();
                match status {
                    202 => Ok(Vec::new()),
                    200 => Ok(json_messages(&body, sse)),
                    s => Err(format!("WOA-App antwortet mit HTTP {s}")),
                }
            }
        };
        let replies = match replies {
            Ok(r) => r,
            Err(msg) => match &id {
                // Eine Anfrage bekommt einen Fehler, damit der Client nicht wartet.
                Some(id) => vec![
                    serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "error": { "code": -32603, "message": msg },
                    })
                    .to_string(),
                ],
                None => {
                    eprintln!("woa mcp: {msg}");
                    Vec::new()
                }
            },
        };
        for r in replies {
            if protocol.is_none()
                && let Ok(v) = serde_json::from_str::<serde_json::Value>(&r)
                && let Some(p) = v
                    .pointer("/result/protocolVersion")
                    .and_then(|p| p.as_str())
            {
                protocol = Some(p.to_string());
            }
            writeln!(output, "{r}").map_err(io)?;
        }
        output.flush().map_err(io)?;
    }
    Ok(())
}

/// Die JSON-Nachrichten einer Antwort: der Körper selbst oder die `data:`-Zeilen eines
/// Event-Streams (leere Zeilen sind Verbindungs-Pings).
fn json_messages(body: &str, sse: bool) -> Vec<String> {
    if !sse {
        let b = body.trim();
        return if b.is_empty() {
            Vec::new()
        } else {
            vec![b.to_string()]
        };
    }
    body.lines()
        .filter_map(|l| l.strip_prefix("data:"))
        .map(str::trim)
        .filter(|d| !d.is_empty())
        .map(str::to_string)
        .collect()
}

/// [`forward`] über stdin/stdout. Blockiert, bis der Client stdin schließt.
pub fn forward_stdio(port: u16, token: &str) -> Result<(), AgentError> {
    forward(
        port,
        token,
        std::io::stdin().lock(),
        std::io::stdout().lock(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sse_and_json_bodies() {
        assert_eq!(json_messages(" {\"a\":1}\n", false), vec!["{\"a\":1}"]);
        assert!(json_messages("", false).is_empty());
        let sse = "id: 0\nretry: 3000\ndata:\n\nevent: message\ndata: {\"id\":1}\n\n";
        assert_eq!(json_messages(sse, true), vec!["{\"id\":1}"]);
    }

    fn lock(dir: &Path, port: u16, pid: u64, secret: &str) {
        let v = serde_json::json!({ "port": port, "pid": pid, "secret": secret });
        std::fs::write(dir.join(LOCK_FILE), v.to_string()).unwrap();
    }

    #[test]
    fn stale_lock_is_not_a_running_app() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(running_app(tmp.path()), None);
        let me = u64::from(std::process::id());
        // Port, auf dem niemand hört: erst binden, Port merken, wieder freigeben.
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        lock(tmp.path(), port, me, "s");
        assert_eq!(running_app(tmp.path()), None);
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        // Ohne Geheimnis (alte Lock-Datei) nie.
        std::fs::write(
            tmp.path().join(LOCK_FILE),
            format!("{{\"port\":{port},\"pid\":{me}}}"),
        )
        .unwrap();
        assert_eq!(running_app(tmp.path()), None);
        if cfg!(target_os = "linux") {
            // Toter Prozess: veraltet, auch wenn jemand auf dem Port hört.
            lock(tmp.path(), port, 4_000_000_000, "s");
            assert_eq!(running_app(tmp.path()), None);
        }
        lock(tmp.path(), port, me, "s");
        assert_eq!(
            running_app(tmp.path()),
            Some(RunningApp {
                port,
                key: "s".into()
            })
        );
    }

    #[test]
    fn foreign_listener_gets_only_the_live_secret() {
        use std::io::Read;
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let seen = std::thread::spawn(move || {
            let (mut c, _) = l.accept().unwrap();
            c.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
            // Bis der Body da ist lesen: ungelesene Bytes beim Schließen
            // werden unter Windows zu einem RST statt der Antwort.
            let mut got = Vec::new();
            let mut buf = [0u8; 4096];
            while !String::from_utf8_lossy(&got).contains("\"ping\"") {
                match c.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => got.extend_from_slice(&buf[..n]),
                }
            }
            let _ =
                c.write_all(b"HTTP/1.1 500 x\r\ncontent-length: 0\r\nconnection: close\r\n\r\n");
            let _ = c.flush();
            String::from_utf8_lossy(&got).into_owned()
        });
        let mut out = Vec::new();
        forward(
            port,
            "nur-live",
            &b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}\n"[..],
            &mut out,
        )
        .unwrap();
        let req = seen.join().unwrap();
        assert!(req.contains("Bearer nur-live"), "{req}");
        assert!(String::from_utf8(out).unwrap().contains("HTTP 500"));
    }
}
