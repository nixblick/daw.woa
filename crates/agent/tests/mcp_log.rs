//! #584: Jeder MCP-Aufruf im Live-Betrieb steht mit Werkzeug und Dauer im Log, und ein
//! abgebrochenes `analyze` hört auf zu rendern.
//! Eigene Testdatei, weil der Server auf fremden Threads loggt (globaler Subscriber).

#![allow(clippy::unwrap_used, clippy::expect_used)] // Tests duerfen abbrechen (Regel 6 gilt fuer Produktivcode).

use std::io::BufRead;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Once, OnceLock};
use std::time::{Duration, Instant};

use tracing_subscriber::Layer;
use tracing_subscriber::layer::SubscriberExt;
use woa_agent::http::MCP_PATH;
use woa_agent::live::{self, LiveServer};
use woa_model::{Clip, Command, Session, TrackKind};
use woa_telemetry::{Journal, JournalLayer, journal_filter};

/// Das Protokoll aller Tests dieser Datei; den globalen Subscriber gibt es nur einmal.
fn journal() -> &'static Journal {
    static J: OnceLock<Journal> = OnceLock::new();
    static INIT: Once = Once::new();
    let j = J.get_or_init(Journal::default);
    INIT.call_once(|| {
        let layer = JournalLayer::new(j).with_filter(journal_filter());
        let subscriber = tracing_subscriber::registry().with(layer);
        tracing::subscriber::set_global_default(subscriber).unwrap();
    });
    j
}

fn texts() -> Vec<String> {
    journal().snapshot().into_iter().map(|e| e.text).collect()
}

#[test]
fn every_mcp_call_is_logged_with_tool_and_duration() {
    journal();
    let tmp = tempfile::tempdir().unwrap();
    let data = tmp.path().join("daten");
    let server = LiveServer::start(&data, "test-token-log".into(), 0, Arc::new(|| {})).unwrap();
    let app = live::running_app(&data).unwrap();
    let lines = concat!(
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"test","version":"1"}}}"#,
        "\n",
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        "\n",
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"add_track","arguments":{"name":"Geheimer Name","kind":"audio"}}}"#,
        "\n",
    );
    let (port, key) = (server.port(), app.key.clone());
    let done = Arc::new(AtomicBool::new(false));
    let client = {
        let done = done.clone();
        std::thread::spawn(move || {
            let mut out = Vec::new();
            let r = live::forward(port, &key, lines.as_bytes(), &mut out);
            done.store(true, Ordering::SeqCst);
            r
        })
    };
    let mut session = Session::new("p");
    let dir = tmp.path().join("p");
    std::fs::create_dir_all(&dir).unwrap();
    let start = Instant::now();
    while !done.load(Ordering::SeqCst) {
        server.serve_pending(&mut session, &dir);
        assert!(
            start.elapsed() < Duration::from_secs(20),
            "Zeitüberschreitung"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    client.join().unwrap().unwrap();

    // Der Test zum Abbruch schreibt ins selbe Protokoll: nur die Zeilen von add_track.
    let texts = texts();
    let call = texts
        .iter()
        .find(|t| t.starts_with("mcp call") && t.contains("add_track"));
    let done = texts
        .iter()
        .find(|t| t.starts_with("mcp done") && t.contains("add_track"));
    assert!(
        call.is_some_and(|t| t.contains("kind") && t.contains("name")),
        "{texts:?}"
    );
    // Gesamtdauer und die Zeit, in der die App ohne ihre Sitzung war (UI-Thread).
    assert!(
        done.is_some_and(|t| t.contains(" ms=") && t.contains(" ui_ms=")),
        "{texts:?}"
    );
    assert!(
        texts.iter().all(|t| !t.contains("Geheimer Name")),
        "keine Argumentwerte: {texts:?}"
    );
}

const INIT: &str = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"test","version":"1"}}}"#;
const INITIALIZED: &str = r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;
const ANALYZE: &str =
    r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"analyze","arguments":{}}}"#;
const CANCEL: &str = r#"{"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":2,"reason":"test"}}"#;

/// So viele Spuren hat das lange Projekt; `analyze` rendert den Mix und jede Spur einzeln.
const TRACKS: u32 = 6;

/// Ein Projekt aus stillen Platzhalter-Clips, `seconds` lang.
fn long_project(seconds: f64) -> Session {
    let mut s = Session::new("lang");
    let beats = seconds * s.project().tempo_bpm / 60.0;
    for i in 0..TRACKS {
        let t = s.new_track_id();
        s.apply(Command::add_track(t, format!("Spur {i}"), TrackKind::Audio))
            .unwrap();
        let c = s.new_clip_id();
        s.apply(Command::AddClip {
            track: t,
            clip: Clip::new(c, "still", 0.0, beats, None),
            index: None,
        })
        .unwrap();
    }
    s
}

/// Schickt eine JSON-RPC-Nachricht an die App und liest bis zur ersten Antwort (ein
/// Event-Stream darf danach offen bleiben). Status und Sitzungs-Id, `None` ohne Verbindung.
fn post(port: u16, key: &str, session: Option<&str>, body: &str) -> Option<(u16, String)> {
    let http: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs(60)))
        .build()
        .into();
    let url = format!("http://127.0.0.1:{port}{MCP_PATH}");
    let auth = format!("Bearer {key}");
    let mut req = http
        .post(url.as_str())
        .header("Authorization", auth.as_str())
        .header("Content-Type", "application/json")
        .header("Accept", "application/json, text/event-stream");
    if let Some(s) = session {
        req = req.header("Mcp-Session-Id", s);
    }
    let mut resp = req.send(body).ok()?;
    let status = resp.status().as_u16();
    let sid = resp
        .headers()
        .get("mcp-session-id")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    let reader = std::io::BufReader::new(resp.body_mut().with_config().reader());
    for line in reader.lines() {
        match line {
            Ok(l) if !l.contains("\"result\"") => {}
            _ => break,
        }
    }
    Some((status, sid))
}

/// Eine Benachrichtigung (ohne Antwort) an die Sitzung `sid`.
fn notify(port: u16, key: &str, sid: &str, body: &str) {
    let (status, _) = post(port, key, Some(sid), body).unwrap();
    assert!(status < 300, "HTTP {status}");
}

/// Die erste Log-Zeile von `analyze`, die mit einem der `prefixes` beginnt.
fn analyze_line(prefixes: &[&str]) -> Option<String> {
    texts()
        .into_iter()
        .find(|t| t.contains("tool=analyze") && prefixes.iter().any(|p| t.starts_with(*p)))
}

/// Zahlenwert des Feldes `name` in einer Log-Zeile (`… ms=12 …`).
fn field(text: &str, name: &str) -> u64 {
    let key = format!(" {name}=");
    let at = text.find(&key).unwrap() + key.len();
    text[at..].split(' ').next().unwrap().parse().unwrap()
}

/// Bricht der Client ein laufendes `analyze` ab, endet der Render sofort, und das Log sagt es.
#[test]
fn cancelled_analyze_stops_the_render() {
    journal();
    let tmp = tempfile::tempdir().unwrap();
    let data = tmp.path().join("daten");
    let server = LiveServer::start(&data, "test-token-stop".into(), 0, Arc::new(|| {})).unwrap();
    let app = live::running_app(&data).unwrap();
    let dir = tmp.path().join("p");
    std::fs::create_dir_all(&dir).unwrap();

    // So lang, dass ein Render spürbar dauert; Rechner sind verschieden schnell.
    let mut seconds = 30.0;
    let (mut session, one_render) = loop {
        let s = long_project(seconds);
        let t = Instant::now();
        woa_agent::audio::render(s.project(), &dir).unwrap();
        let took = t.elapsed();
        if took >= Duration::from_millis(500) || seconds >= 480.0 {
            break (s, took);
        }
        seconds *= 2.0;
    };
    // Ein voller Lauf rendert den Mix und jede Spur einzeln.
    let full = one_render * (TRACKS + 1);

    let (port, key) = (server.port(), app.key.clone());
    let (status, sid) = post(port, &key, None, INIT).unwrap();
    assert_eq!(status, 200);
    assert!(!sid.is_empty(), "keine Sitzungs-Id");
    notify(port, &key, &sid, INITIALIZED);
    {
        // Die Antwort kommt nach dem Abbruch nie; der Faden endet mit dem Server.
        let (key, sid) = (key.clone(), sid.clone());
        std::thread::spawn(move || post(port, &key, Some(&sid), ANALYZE));
    }

    // Der „GUI-Thread“ leiht die Sitzung her; sobald analyze läuft, bricht der Client ab.
    let start = Instant::now();
    let mut cancel_sent = false;
    let done = loop {
        server.serve_pending(&mut session, &dir);
        if !cancel_sent && analyze_line(&["mcp call"]).is_some() {
            notify(port, &key, &sid, CANCEL);
            cancel_sent = true;
        }
        if let Some(done) = analyze_line(&["mcp done", "mcp slow"]) {
            break done;
        }
        assert!(
            start.elapsed() < Duration::from_secs(120),
            "analyze endet nicht"
        );
        std::thread::sleep(Duration::from_millis(5));
    };
    // Die Meldung zum Abbruch ist ein eigener Eintrag direkt nach `mcp done`.
    let start = Instant::now();
    let stop = loop {
        if let Some(stop) = analyze_line(&["mcp cancelled"]) {
            break stop;
        }
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "keine Abbruch-Meldung: {done}"
        );
        std::thread::sleep(Duration::from_millis(5));
    };
    assert!(stop.contains("render stopped"), "{stop}");
    assert!(done.contains("cancelled=true"), "{done}");
    // Grobe Schranke: Ohne Abbruch liefe analyze den vollen Lauf.
    let took = Duration::from_millis(field(&done, "ms"));
    assert!(
        took * 2 < full,
        "abgebrochen nach {took:?}, ein voller Lauf dauert etwa {full:?}"
    );
}
