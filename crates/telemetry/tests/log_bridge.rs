//! Meldungen aus dem `log`-Crate (Abhängigkeiten): eigener Prozess, weil der `LogTracer`
//! global ist.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::Value;
use tracing_subscriber::layer::SubscriberExt;
use woa_telemetry::{LOG_BURST, UploadConfig, Uploader, noise_filter};

fn mock() -> (String, Arc<Mutex<Vec<Value>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/ingest", listener.local_addr().unwrap());
    let bodies = Arc::new(Mutex::new(Vec::new()));
    let seen = bodies.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut len = 0usize;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" || line.is_empty() {
                    break;
                }
                if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    len = v.trim().parse().unwrap();
                }
            }
            let mut body = vec![0u8; len];
            reader.read_exact(&mut body).unwrap();
            seen.lock()
                .unwrap()
                .push(serde_json::from_slice(&body).unwrap_or(Value::Null));
            let _ = stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}");
        }
    });
    (url, bodies)
}

#[test]
fn log_crate_messages_carry_the_crate_are_throttled_and_mp3_noise_is_silenced() {
    let (url, bodies) = mock();
    let up = Uploader::start(UploadConfig {
        enabled: true,
        url,
        install_id: "0f8e-test".into(),
        app_version: "0.1.0".into(),
        os: "windows".into(),
        interval: Duration::from_secs(3600),
        ..UploadConfig::default()
    })
    .unwrap();
    tracing_log::LogTracer::init().unwrap();
    let subscriber = tracing_subscriber::registry()
        .with(noise_filter())
        .with(up.layer());
    tracing::subscriber::with_default(subscriber, || {
        for i in 0..1084 {
            log::warn!(target: "symphonia_bundle_mp3::layer3", "mpa: invalid main_data_begin, underflow by {i} bytes");
            log::warn!(target: "symphonia_bundle_mp3::demuxer", "skipping junk at {i} bytes in C:\\Users\\nutzer\\Musik\\geheim.mp3");
        }
        log::error!(target: "symphonia_core::formats::probe", "reached probe limit of 1048576 bytes");
    });
    assert!(up.flush());
    let bodies = bodies.lock().unwrap();
    let raw = serde_json::to_string(&*bodies).unwrap();
    assert!(!raw.contains("geheim") && !raw.contains("nutzer") && !raw.contains("junk"));
    let ev: Vec<&Value> = bodies
        .iter()
        .flat_map(|b| b["ereignisse"].as_array().unwrap().iter())
        .collect();
    // Unterflow-Rauschen kommt gar nicht an, die Junk-Warnung nur gedrosselt, der Fehler einmal.
    let mods: Vec<&str> = ev.iter().map(|e| e["modul"].as_str().unwrap()).collect();
    assert_eq!(
        mods.iter()
            .filter(|m| **m == "symphonia_bundle_mp3")
            .count(),
        LOG_BURST as usize,
        "{raw}"
    );
    assert!(!mods.contains(&"log"), "{raw}");
    let err: Vec<_> = ev.iter().filter(|e| e["stufe"] == "error").collect();
    assert_eq!(err.len(), 1);
    assert_eq!(err[0]["modul"], "symphonia_core");
    assert!(err[0]["art"].as_str().unwrap().starts_with("log.error"));
}
