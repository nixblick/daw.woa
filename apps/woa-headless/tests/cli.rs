//! Integrationstests für `woa-headless` (assert_cmd + tempfile, Mock-HTTP über std::net).

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::mpsc;

use assert_cmd::Command;
use predicates::str::contains;
use serde_json::Value;

fn bin() -> Command {
    let mut c = Command::cargo_bin("woa-headless").unwrap();
    c.env_remove("WOA_TELEMETRY_URL")
        .env_remove("WOA_FEEDBACK_URL");
    c
}

/// Ein-Anfrage-Mock: antwortet mit `status`/`answer` und liefert (Pfad, JSON-Body).
fn mock(status: u16, answer: &'static str) -> (String, mpsc::Receiver<(String, Value)>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            let path = line.split_whitespace().nth(1).unwrap_or("").to_string();
            let mut len = 0usize;
            loop {
                let mut h = String::new();
                reader.read_line(&mut h).unwrap();
                if h == "\r\n" || h.is_empty() {
                    break;
                }
                if let Some(v) = h.to_ascii_lowercase().strip_prefix("content-length:") {
                    len = v.trim().parse().unwrap();
                }
            }
            let mut body = vec![0u8; len];
            reader.read_exact(&mut body).unwrap();
            let json = serde_json::from_slice(&body).unwrap_or(Value::Null);
            let _ = tx.send((path, json));
            let resp = format!(
                "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{answer}",
                answer.len()
            );
            stream.write_all(resp.as_bytes()).unwrap();
        }
    });
    (format!("http://{addr}"), rx)
}

#[test]
fn new_add_info_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("jingle.woa");
    bin().arg("new").arg(&p).assert().success();
    assert!(p.join("project.json").exists());
    bin().arg("new").arg(&p).assert().failure();

    bin()
        .arg("add-track")
        .arg(&p)
        .arg("Stimme")
        .assert()
        .success();
    bin()
        .args(["add-track", "--kind", "bus"])
        .arg(&p)
        .arg("Summe")
        .assert()
        .success();

    bin()
        .arg("info")
        .arg(&p)
        .assert()
        .success()
        .stdout(contains("name: jingle"))
        .stdout(contains("tracks: 2"))
        .stdout(contains("\"Stimme\""))
        .stdout(contains("Bus \"Summe\""));

    let out = bin().args(["info", "--json"]).arg(&p).output().unwrap();
    let json: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["project"]["tracks"].as_array().map(Vec::len), Some(2));

    bin()
        .arg("roundtrip")
        .arg(&p)
        .assert()
        .success()
        .stdout(contains("gleich"));
}

#[test]
fn roundtrip_missing_project_fails() {
    let dir = tempfile::tempdir().unwrap();
    bin()
        .arg("roundtrip")
        .arg(dir.path().join("fehlt.woa"))
        .assert()
        .code(1);
}

#[test]
fn undo_demo_passes() {
    bin()
        .arg("undo-demo")
        .assert()
        .success()
        .stdout(contains("alles gleich"));
}

#[test]
fn bad_usage_is_exit_2() {
    bin().arg("gibtsnicht").assert().code(2);
}

#[test]
fn log_test_sends_event() {
    let (url, rx) = mock(200, "{}");
    bin()
        .arg("log-test")
        .arg("--url")
        .arg(format!("{url}/ingest"))
        .assert()
        .success();
    let (path, body) = rx.recv().unwrap();
    assert_eq!(path, "/ingest");
    assert!(body.to_string().contains("log_test"), "{body}");
}

#[test]
fn log_test_reads_env_and_reports_rejection() {
    let (url, _rx) = mock(500, "{}");
    bin()
        .arg("log-test")
        .env("WOA_TELEMETRY_URL", format!("{url}/ingest"))
        .assert()
        .code(1);
}

#[test]
fn log_test_defaults_to_own_server() {
    bin()
        .args(["log-test", "--help"])
        .assert()
        .success()
        .stdout(contains("https://feedback.wironair.de/api/logs/ingest"));
}

#[test]
fn log_test_with_empty_url_fails() {
    bin()
        .args(["log-test", "--url", ""])
        .assert()
        .code(1)
        .stderr(contains("keine URL"));
}

#[test]
fn feedback_test_reports_issue() {
    let (url, rx) = mock(200, r#"{"issue": 42}"#);
    bin()
        .arg("feedback-test")
        .arg("--url")
        .arg(format!("{url}/api/feedback"))
        .assert()
        .success()
        .stdout(contains("#42"));
    let (path, body) = rx.recv().unwrap();
    assert_eq!(path, "/api/feedback");
    assert_eq!(body["kind"], "feedback");
}

#[cfg(not(feature = "device"))]
#[test]
fn play_and_devices_explain_missing_feature() {
    for args in [
        &["play", "--seconds", "1"][..],
        &["devices"][..],
        &["record", "x.wav"][..],
    ] {
        bin()
            .args(args)
            .assert()
            .code(1)
            .stderr(contains("ohne Feature `device`"));
    }
}

#[test]
fn play_help_names_underruns() {
    bin()
        .args(["play", "--help"])
        .assert()
        .success()
        .stdout(contains("Underruns"))
        .stdout(contains("--buffer"));
}
