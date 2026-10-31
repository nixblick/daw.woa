//! Live-Modus: Ein MCP-Aufruf über die stdio-Weiterleitung landet im Projekt der „App“
//! (hier: eine Sitzung, die der Test wie der GUI-Thread herleiht), ist ein Rückgängig-Schritt
//! „Agent: add_track“ und verschwindet mit Undo. Ohne Ton, ohne echte App.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use woa_agent::live::{self, LiveServer};
use woa_model::Session;

const TOKEN: &str = "test-token-live-1";

const LINES: &str = concat!(
    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"test","version":"1"}}}"#,
    "\n",
    r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
    "\n",
    r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"add_track","arguments":{"name":"Von Claude","kind":"audio"}}}"#,
    "\n",
    r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"save","arguments":{}}}"#,
    "\n",
);

#[test]
fn agent_call_lands_in_the_app_session_and_undo_removes_it() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tmp.path().join("daten");
    let server = LiveServer::start(&data, TOKEN.into(), 0, Arc::new(|| {})).unwrap();

    // Die Lock-Datei verrät die App; stdio würde jetzt durchreichen.
    assert_eq!(live::running_app(&data), Some(server.port()));
    let lock = std::fs::read_to_string(data.join(live::LOCK_FILE)).unwrap();
    assert!(!lock.contains(TOKEN), "Token nie in der Lock-Datei");

    let port = server.port();
    let done = Arc::new(AtomicBool::new(false));
    let client = {
        let done = done.clone();
        std::thread::spawn(move || {
            let mut out = Vec::new();
            let r = live::forward(port, TOKEN, LINES.as_bytes(), &mut out);
            done.store(true, Ordering::SeqCst);
            r.map(|()| String::from_utf8(out).unwrap())
        })
    };

    // Der „GUI-Thread“: bedient die Leihwünsche zwischen zwei Bildern.
    let mut session = Session::new("App-Projekt");
    let dir = tmp.path().join("wiederherstellung");
    std::fs::create_dir_all(&dir).unwrap();
    let start = Instant::now();
    let mut served = 0;
    while !done.load(Ordering::SeqCst) {
        served += server.serve_pending(&mut session, &dir);
        assert!(
            start.elapsed() < Duration::from_secs(20),
            "Zeitüberschreitung"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    let out = client.join().unwrap().unwrap();
    assert_eq!(served, 2, "{out}");
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 3, "initialize, add_track, save: {out}");
    assert!(lines[0].contains("\"woa\""), "{out}");
    assert!(lines[1].contains("\\\"track\\\""), "{out}");
    // Ungespeichertes App-Projekt: save legt keinen fremden Ordner an.
    assert!(lines[2].contains("Speichern unter"), "{out}");
    assert!(session.path().is_none());

    assert_eq!(session.project().meta.name, "App-Projekt");
    assert!(
        session
            .project()
            .tracks
            .iter()
            .any(|t| t.name == "Von Claude"),
        "Spur sichtbar im Projekt der App"
    );
    assert_eq!(session.history().undo_label(), Some("Agent: add_track"));
    session.undo().unwrap();
    assert!(
        session
            .project()
            .tracks
            .iter()
            .all(|t| t.name != "Von Claude")
    );

    drop(server);
    assert!(!data.join(live::LOCK_FILE).exists(), "Lock-Datei weg");
    assert_eq!(live::running_app(&data), None);
}

#[test]
fn wrong_token_reaches_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let server = LiveServer::start(tmp.path(), TOKEN.into(), 0, Arc::new(|| {})).unwrap();
    let mut out = Vec::new();
    live::forward(
        server.port(),
        "falsch",
        LINES.lines().next().unwrap().as_bytes(),
        &mut out,
    )
    .unwrap();
    let out = String::from_utf8(out).unwrap();
    assert!(out.contains("HTTP 401"), "{out}");
    assert!(LiveServer::start(tmp.path(), String::new(), 0, Arc::new(|| {})).is_err());
}
