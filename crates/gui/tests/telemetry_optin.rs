//! Opt-in end to end: GuiConfig -> UploadConfig -> Uploader against a local fake server
//! (std::net, no extra crate).

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::mpsc;
use std::time::Duration;

use woa_gui::GuiConfig;
use woa_telemetry::{Level, Uploader};

/// Answers every request with 200 and reports (path, body).
fn fake_server() -> (String, mpsc::Receiver<(String, String)>) {
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
            let _ = tx.send((path, String::from_utf8_lossy(&body).into_owned()));
            let answer = r#"{"accepted":1}"#;
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{answer}",
                answer.len()
            );
            stream.write_all(resp.as_bytes()).unwrap();
        }
    });
    (format!("http://{addr}"), rx)
}

#[test]
fn without_opt_in_nothing_is_sent() {
    let (url, rx) = fake_server();
    for choice in [None, Some(false)] {
        let mut c = GuiConfig {
            telemetry_enabled: choice,
            telemetry_url: format!("{url}/api/logs/ingest"),
            ..GuiConfig::default()
        };
        c.ensure_install_id();
        assert!(Uploader::start(c.upload_config()).is_none());
    }
    assert!(rx.recv_timeout(Duration::from_millis(300)).is_err());
}

#[test]
fn with_opt_in_codes_reach_server_and_buffer_empties() {
    let (url, rx) = fake_server();
    let mut c = GuiConfig {
        telemetry_enabled: Some(true),
        telemetry_url: format!("{url}/api/logs/ingest"),
        ..GuiConfig::default()
    };
    c.ensure_install_id();
    let up = Uploader::start(c.upload_config()).expect("opt-in starts uploader");
    assert!(up.record(Level::Warn, "woa_engine", "xrun", 1));
    assert!(up.flush(), "2xx accepted");
    let (path, body) = rx.recv_timeout(Duration::from_secs(10)).unwrap();
    assert_eq!(path, "/api/logs/ingest");
    assert!(body.contains("\"art\":\"xrun\""), "{body}");
    assert!(body.contains(&c.install_id), "{body}");
    // Buffer was emptied: the next flush sends nothing.
    assert!(up.flush());
    assert!(rx.recv_timeout(Duration::from_millis(300)).is_err());
}
