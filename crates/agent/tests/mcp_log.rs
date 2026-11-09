//! #584: Jeder MCP-Aufruf im Live-Betrieb steht mit Werkzeug und Dauer im Log.
//! Eigene Testdatei, weil der Server auf fremden Threads loggt (globaler Subscriber).

#![allow(clippy::unwrap_used, clippy::expect_used)] // Tests duerfen abbrechen (Regel 6 gilt fuer Produktivcode).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use tracing_subscriber::Layer;
use tracing_subscriber::layer::SubscriberExt;
use woa_agent::live::{self, LiveServer};
use woa_model::Session;
use woa_telemetry::{Journal, JournalLayer, journal_filter};

#[test]
fn every_mcp_call_is_logged_with_tool_and_duration() {
    static J: OnceLock<Journal> = OnceLock::new();
    let j = J.get_or_init(Journal::default);
    tracing::subscriber::set_global_default(
        tracing_subscriber::registry().with(JournalLayer::new(j).with_filter(journal_filter())),
    )
    .unwrap();

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

    let texts: Vec<String> = j.snapshot().into_iter().map(|e| e.text).collect();
    let call = texts.iter().find(|t| t.starts_with("mcp call"));
    let done = texts.iter().find(|t| t.starts_with("mcp done"));
    assert!(
        call.is_some_and(|t| t.contains("add_track") && t.contains("kind") && t.contains("name")),
        "{texts:?}"
    );
    assert!(
        done.is_some_and(|t| t.contains("add_track") && t.contains("ms=")),
        "{texts:?}"
    );
    assert!(
        texts.iter().all(|t| !t.contains("Geheimer Name")),
        "keine Argumentwerte: {texts:?}"
    );
}
