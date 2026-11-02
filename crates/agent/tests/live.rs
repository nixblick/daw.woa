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
    let app = live::running_app(&data).expect("App erkannt");
    assert_eq!(app.port, server.port());
    let lock = std::fs::read_to_string(data.join(live::LOCK_FILE)).unwrap();
    assert!(!lock.contains(TOKEN), "Token nie in der Lock-Datei");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(data.join(live::LOCK_FILE))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    let port = server.port();
    let key = app.key.clone();
    let done = Arc::new(AtomicBool::new(false));
    let client = {
        let done = done.clone();
        std::thread::spawn(move || {
            let mut out = Vec::new();
            let r = live::forward(port, &key, LINES.as_bytes(), &mut out);
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
    assert!(live::running_app(&data).is_none());
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

#[test]
fn after_token_change_the_old_token_gets_401() {
    let tmp = tempfile::tempdir().unwrap();
    let init = LINES.lines().next().unwrap();
    let old = LiveServer::start(tmp.path(), "alt-token-1".into(), 0, Arc::new(|| {})).unwrap();
    let old_key = live::running_app(tmp.path()).unwrap().key;
    drop(old);
    let new = LiveServer::start(tmp.path(), "neu-token-2".into(), 0, Arc::new(|| {})).unwrap();
    for (bearer, ok) in [
        ("alt-token-1", false),
        (old_key.as_str(), false),
        ("neu-token-2", true),
    ] {
        let mut out = Vec::new();
        live::forward(new.port(), bearer, init.as_bytes(), &mut out).unwrap();
        let out = String::from_utf8(out).unwrap();
        assert_eq!(!out.contains("HTTP 401"), ok, "{out}");
    }
}

/// Eine abgewiesene Anfrage liest der Server trotzdem zu Ende. Sonst schließt er mit
/// ungelesenen Bytes, Windows schickt dann ein RST statt der 401, und der Client sieht
/// „connection aborted (os error 10053)“ (#256).
#[test]
fn rejected_request_is_read_before_the_answer() {
    use std::io::{Read, Write};
    let tmp = tempfile::tempdir().unwrap();
    let server = LiveServer::start(tmp.path(), TOKEN.into(), 0, Arc::new(|| {})).unwrap();
    let body = LINES.lines().next().unwrap();
    let mut c = std::net::TcpStream::connect(("127.0.0.1", server.port())).unwrap();
    write!(
        c,
        "POST {} HTTP/1.1\r\nhost: 127.0.0.1\r\nauthorization: Bearer falsch\r\n\
         content-type: application/json\r\ncontent-length: {}\r\n\r\n",
        woa_agent::http::MCP_PATH,
        body.len()
    )
    .unwrap();
    c.set_read_timeout(Some(Duration::from_millis(300)))
        .unwrap();
    let mut buf = [0u8; 4096];
    assert!(
        c.read(&mut buf).is_err(),
        "401 kam, bevor der Body gelesen war"
    );
    c.write_all(body.as_bytes()).unwrap();
    c.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let n = c.read(&mut buf).unwrap();
    let head = String::from_utf8_lossy(&buf[..n]);
    assert!(head.starts_with("HTTP/1.1 401"), "{head}");
    assert!(head.contains("connection: close"), "{head}");
}

/// Ausgangspegel per MCP (André 02.11.2026): nur lesend, ohne Leihe der Sitzung und ohne
/// Rückgängig-Schritt. Die Werte setzt hier der Test, wie sonst die App je Bild. Ohne Ton.
#[test]
fn output_levels_are_read_only_and_listed() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tmp.path().join("daten");
    let server = LiveServer::start(&data, TOKEN.into(), 0, Arc::new(|| {})).unwrap();
    server.output_levels().publish(true, [0.5, 0.25, 0.0]);
    let app = live::running_app(&data).expect("App erkannt");
    let lines = concat!(
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"test","version":"1"}}}"#,
        "\n",
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        "\n",
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}"#,
        "\n",
        r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"output_levels","arguments":{}}}"#,
        "\n",
    );
    let mut out = Vec::new();
    live::forward(app.port, &app.key, lines.as_bytes(), &mut out).unwrap();
    let out = String::from_utf8(out).unwrap();
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 3, "{out}");
    assert!(
        lines[1].contains("\"output_levels\""),
        "Werkzeug gelistet: {out}"
    );
    let reply = lines[2];
    assert!(reply.contains("\\\"radio\\\":true"), "{reply}");
    assert!(reply.contains("\\\"output\\\":\\\"notebook\\\""), "{reply}");
    assert!(
        reply.contains("\\\"peak_dbfs\\\":-6.0"),
        "Notebook −6 dBFS: {reply}"
    );
    assert!(
        reply.contains("\\\"peak_dbfs\\\":-12.0"),
        "Stream −12 dBFS: {reply}"
    );
    assert!(
        reply.contains("\\\"peak_dbfs\\\":null"),
        "Vorhören still: {reply}"
    );
    // Keine Leihe: kein Aufruf wartet auf den GUI-Thread.
    let mut session = woa_model::Session::new("App");
    assert_eq!(server.serve_pending(&mut session, tmp.path()), 0);
    assert!(session.history().undo_label().is_none());
}
