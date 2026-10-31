//! Tests gegen einen lokalen Mock-HTTP-Server (std::net, ohne Fremd-Crate).

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::Engine;
use serde_json::Value;
use tracing_subscriber::layer::SubscriberExt;
use woa_telemetry::{
    FeedbackError, FeedbackMeta, FeedbackType, Level, UploadConfig, Uploader, submit_feedback,
};

struct Mock {
    url: String,
    bodies: Arc<Mutex<Vec<(String, Value)>>>,
}

/// Startet einen Server, der der Reihe nach mit `answers` (Status, Body) antwortet;
/// danach immer mit dem letzten Eintrag.
fn mock(answers: Vec<(u16, &'static str)>) -> Mock {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let bodies = Arc::new(Mutex::new(Vec::new()));
    let seen = bodies.clone();
    std::thread::spawn(move || {
        for (i, stream) in listener.incoming().enumerate() {
            let Ok(mut stream) = stream else { continue };
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut request_line = String::new();
            reader.read_line(&mut request_line).unwrap();
            let path = request_line
                .split_whitespace()
                .nth(1)
                .unwrap_or("")
                .to_string();
            let mut len = 0usize;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" || line.is_empty() {
                    break;
                }
                let lower = line.to_ascii_lowercase();
                if let Some(v) = lower.strip_prefix("content-length:") {
                    len = v.trim().parse().unwrap();
                }
            }
            let mut body = vec![0u8; len];
            reader.read_exact(&mut body).unwrap();
            let json: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
            seen.lock().unwrap().push((path, json));
            let (status, answer) = answers[i.min(answers.len() - 1)];
            let resp = format!(
                "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{answer}",
                answer.len()
            );
            stream.write_all(resp.as_bytes()).unwrap();
        }
    });
    Mock {
        url: format!("http://{addr}"),
        bodies,
    }
}

fn upload_cfg(url: String) -> UploadConfig {
    UploadConfig {
        enabled: true,
        url,
        install_id: "0f8e-test".into(),
        app_version: "0.1.0".into(),
        os: "linux".into(),
        interval: Duration::from_secs(3600),
        ..UploadConfig::default()
    }
}

#[test]
fn upload_is_off_without_opt_in_or_url() {
    let mut cfg = upload_cfg("http://127.0.0.1:1/ingest".into());
    cfg.enabled = false;
    assert!(Uploader::start(cfg).is_none());
    assert!(Uploader::start(upload_cfg(String::new())).is_none());
    assert!(Uploader::start(UploadConfig::default()).is_none());
}

#[test]
fn upload_batch_matches_ingest_contract() {
    let m = mock(vec![(200, r#"{"accepted":2}"#)]);
    let up = Uploader::start(upload_cfg(format!("{}/ingest", m.url))).unwrap();
    assert!(up.record(Level::Warn, "woa_engine", "xrun", 3));
    assert!(up.record(Level::Info, "woa_io", "perf.load.ms", 120));
    // Freitext und Pfade passieren den Scrubber nicht.
    assert!(!up.record(Level::Error, "woa_io", "/home/x/song.wav", 1));
    assert!(!up.record(Level::Error, "woa io", "crash", 1));
    assert!(up.flush());
    let bodies = m.bodies.lock().unwrap();
    assert_eq!(bodies.len(), 1);
    let (path, b) = &bodies[0];
    assert_eq!(path, "/ingest");
    assert_eq!(b["install_id"], "0f8e-test");
    assert_eq!(b["app_version"], "0.1.0");
    assert_eq!(b["os"], "linux");
    let ev = b["ereignisse"].as_array().unwrap();
    assert_eq!(ev.len(), 2);
    assert_eq!(ev[0]["stufe"], "warn");
    assert_eq!(ev[0]["modul"], "woa_engine");
    assert_eq!(ev[0]["art"], "xrun");
    assert_eq!(ev[0]["wert"], 3);
    assert!(ev[0]["zeit_ms"].as_u64().unwrap() > 946_684_800_000);
    let mut keys: Vec<_> = ev[0].as_object().unwrap().keys().cloned().collect();
    keys.sort();
    assert_eq!(keys, ["art", "modul", "stufe", "wert", "zeit_ms"]);
}

#[test]
fn upload_keeps_buffer_on_server_error() {
    let m = mock(vec![
        (503, r#"{"error":"storage unavailable"}"#),
        (200, r#"{"accepted":1}"#),
    ]);
    let up = Uploader::start(upload_cfg(format!("{}/ingest", m.url))).unwrap();
    assert!(up.record(Level::Error, "woa_stream", "stream.reconnect", 1));
    assert!(!up.flush());
    assert!(up.flush());
    let bodies = m.bodies.lock().unwrap();
    assert_eq!(bodies.len(), 2);
    assert_eq!(bodies[0].1["ereignisse"], bodies[1].1["ereignisse"]);
}

#[test]
fn upload_sends_rest_on_drop() {
    let m = mock(vec![(200, "{}")]);
    let up = Uploader::start(upload_cfg(format!("{}/ingest", m.url))).unwrap();
    assert!(up.record(Level::Info, "woa", "start.ms", 900));
    drop(up);
    assert_eq!(m.bodies.lock().unwrap().len(), 1);
}

#[test]
fn layer_forwards_codes_but_never_text() {
    let m = mock(vec![(200, "{}")]);
    let up = Uploader::start(upload_cfg(format!("{}/ingest", m.url))).unwrap();
    let subscriber = tracing_subscriber::registry().with(up.layer());
    tracing::subscriber::with_default(subscriber, || {
        tracing::warn!(target: "woa_engine::graph", "could not open /home/user/geheim.wav");
        tracing::info!(target: "woa_io", art = "perf.load.ms", wert = 42u64, "loaded /x/y.wav");
        tracing::info!(target: "woa_io", "plain info is not uploaded");
        tracing::error!(target: "woa_io", art = "Freitext mit Leerzeichen", "nope");
    });
    assert!(up.flush());
    let bodies = m.bodies.lock().unwrap();
    let raw = bodies[0].1.to_string();
    assert!(!raw.contains("geheim"));
    assert!(!raw.contains("/x/y"));
    let ev = bodies[0].1["ereignisse"].as_array().unwrap().clone();
    assert_eq!(ev.len(), 2);
    // Ohne eigenes `art` steht der Code-Ort da, nie die Nachricht.
    let art = ev[0]["art"].as_str().unwrap();
    assert!(art.starts_with("log.warn.mock_server."), "{art}");
    assert_eq!(ev[0]["modul"], "woa_engine.graph");
    assert_eq!(ev[1]["art"], "perf.load.ms");
    assert_eq!(ev[1]["wert"], 42);
}

#[test]
fn slow_spans_are_uploaded_with_their_duration() {
    let m = mock(vec![(200, "{}")]);
    let up = Uploader::start(upload_cfg(format!("{}/ingest", m.url))).unwrap();
    let subscriber = tracing_subscriber::registry().with(up.layer());
    tracing::subscriber::with_default(subscriber, || {
        let _fast = tracing::info_span!(target: "woa_radio", "fast_step").entered();
        drop(_fast);
        let _slow = tracing::info_span!(target: "woa_radio", "playlist_add").entered();
        std::thread::sleep(std::time::Duration::from_millis(30));
    });
    assert!(up.flush());
    let bodies = m.bodies.lock().unwrap();
    let ev = bodies[0].1["ereignisse"].as_array().unwrap().clone();
    assert_eq!(ev.len(), 1, "{ev:?}");
    assert_eq!(ev[0]["art"], "span.playlist_add");
    assert_eq!(ev[0]["modul"], "woa_radio");
    assert!(ev[0]["wert"].as_i64().unwrap() >= 30);
}

#[test]
fn feedback_with_screenshot() {
    let m = mock(vec![(201, r#"{"ok":true,"issue":42}"#)]);
    let meta = FeedbackMeta {
        url: format!("{}/api/feedback", m.url),
        kind: FeedbackType::Bug,
        install_id: "0f8e-test".into(),
        app_version: "0.1.0".into(),
        os: "windows".into(),
        ..FeedbackMeta::default()
    };
    let png = b"\x89PNG\r\n\x1a\nrest-of-image".to_vec();
    let issue = submit_feedback(
        "Mixer knackt\nDetails",
        vec![("screenshot.png".into(), png.clone())],
        &meta,
    )
    .unwrap();
    assert_eq!(issue, Some(42));
    let bodies = m.bodies.lock().unwrap();
    let (path, b) = &bodies[0];
    assert_eq!(path, "/api/feedback");
    assert_eq!(b["kind"], "feedback");
    assert_eq!(b["title"], "Mixer knackt");
    assert_eq!(b["type"], "bug");
    assert_eq!(b["trust"], "L0");
    assert_eq!(b["installId"], "0f8e-test");
    assert_eq!(b["appVersion"], "0.1.0");
    assert_eq!(b["os"], "windows");
    let img = &b["images"][0];
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(img["content_b64"].as_str().unwrap())
        .unwrap();
    assert_eq!(decoded, png);
}

#[test]
fn feedback_errors() {
    let m = mock(vec![(503, r#"{"error":"feedback backend off"}"#)]);
    let meta = FeedbackMeta {
        url: format!("{}/api/feedback", m.url),
        ..FeedbackMeta::default()
    };
    assert!(matches!(
        submit_feedback("x", vec![], &meta),
        Err(FeedbackError::Status(503))
    ));
    assert!(matches!(
        submit_feedback("x", vec![], &FeedbackMeta::default()),
        Err(FeedbackError::NoEndpoint)
    ));
}

#[test]
fn daily_log_file_is_written() {
    let dir = tempfile::tempdir().unwrap();
    let appender = woa_telemetry::file_appender(dir.path()).unwrap();
    let (writer, guard) = tracing_appender::non_blocking(appender);
    let subscriber = tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer().json().with_writer(writer));
    tracing::subscriber::with_default(subscriber, || {
        tracing::info!(art = "start.ms", wert = 5u64, "hello");
    });
    drop(guard);
    let files: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
        .collect();
    assert_eq!(files.len(), 1);
    assert!(
        files[0].starts_with("woa.") && files[0].ends_with(".log"),
        "{files:?}"
    );
    let content = std::fs::read_to_string(dir.path().join(&files[0])).unwrap();
    assert!(content.contains("\"art\":\"start.ms\""));
}

#[test]
fn install_id_is_random_uuid_v4() {
    let a = woa_telemetry::new_install_id();
    let b = woa_telemetry::new_install_id();
    assert_ne!(a, b);
    assert_eq!(a.len(), 36);
    let parts: Vec<&str> = a.split('-').collect();
    assert_eq!(
        parts.iter().map(|p| p.len()).collect::<Vec<_>>(),
        [8, 4, 4, 4, 12]
    );
    assert!(parts[2].starts_with('4'), "{a}");
    assert!(a.chars().all(|c| c == '-' || c.is_ascii_hexdigit()));
}

#[test]
fn default_ingest_url_is_own_server() {
    assert_eq!(
        woa_telemetry::DEFAULT_INGEST_URL,
        "https://feedback.wironair.de/api/logs/ingest"
    );
}

/// Fenster zu, Prozess muss enden: Auch wenn noch ein Layer (globaler Subscriber) einen
/// Sender hält, kehrt `Drop` zurück (André 30.10., Windows-Prozess lief weiter).
#[test]
fn drop_returns_while_a_layer_still_holds_a_sender() {
    let up = Uploader::start(upload_cfg("http://127.0.0.1:9/ingest".into())).expect("enabled");
    let layer = up.layer();
    let t = std::time::Instant::now();
    drop(up);
    assert!(t.elapsed() < std::time::Duration::from_secs(5));
    drop(layer);
}
