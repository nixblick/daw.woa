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

/// Mini-Shoutcast-1-Quelle: erwartet `want` als Anmeldezeile, zählt MP3-Bytes.
fn shoutcast_fake(want: &'static str) -> (u16, mpsc::Receiver<usize>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let Ok((stream, _)) = listener.accept() else {
            return;
        };
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut w = stream;
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        if line.trim_end() != want {
            let _ = w.write_all(b"invalid password\r\n");
            return;
        }
        w.write_all(b"OK2\r\nicy-caps:11\r\n\r\n").unwrap();
        loop {
            let mut h = String::new();
            if reader.read_line(&mut h).unwrap_or(0) == 0 || h == "\r\n" {
                break;
            }
        }
        let mut rest = Vec::new();
        let _ = reader.read_to_end(&mut rest);
        let _ = tx.send(rest.len());
    });
    (port, rx)
}

#[test]
fn stream_test_sends_mp3_and_rejects_wrong_password() {
    let tmp = tempfile::tempdir().unwrap();
    let kw_file = tmp.path().join("source.kw");
    std::fs::write(&kw_file, "probe-kw\n").unwrap();

    let (port, rx) = shoutcast_fake("probe-kw");
    bin()
        .args([
            "stream-test",
            "--host",
            "127.0.0.1",
            "--seconds",
            "1",
            "--port",
        ])
        .arg(port.to_string())
        .arg("--pass-file")
        .arg(&kw_file)
        .assert()
        .success()
        .stdout(contains("Titel ausgelassen"))
        .stdout(contains("1 s gesendet"));
    let bytes = rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
    assert!(bytes > 10_000, "{bytes}");

    let (port, _rx) = shoutcast_fake("anderes");
    let out = bin()
        .args([
            "stream-test",
            "--host",
            "127.0.0.1",
            "--seconds",
            "1",
            "--port",
        ])
        .arg(port.to_string())
        .arg("--pass-file")
        .arg(&kw_file)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(3));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("Kennwort") && !err.contains("probe-kw"),
        "{err}"
    );
}
