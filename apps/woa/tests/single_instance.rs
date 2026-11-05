//! Fassung 8, Punkt 0a: ein zweiter Start lässt den laufenden WOA in Ruhe.
//! Der Test hält die Sperre selbst (wie ein laufendes WOA) und startet das echte Binary.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::process::Command;
use std::time::{Duration, Instant};

use woa_gui::instance::{Instance, acquire};

#[test]
fn second_start_wakes_first_and_opens_nothing() {
    let base = std::env::temp_dir().join(format!("woa-single-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let data_dir = base.join("woa");
    let Instance::Primary(first) = acquire(&data_dir) else {
        panic!("test must hold the lock");
    };
    let mut child = Command::new(env!("CARGO_BIN_EXE_woa"))
        .env("XDG_DATA_HOME", &base)
        .env("XDG_CONFIG_HOME", &base)
        .env("APPDATA", &base)
        .spawn()
        .expect("start woa");
    // Auf das Ende warten, nicht auf eine Uhrzeit; die Grenze fängt nur ein Hängen.
    let start = Instant::now();
    let status = loop {
        if let Some(s) = child.try_wait().unwrap() {
            break s;
        }
        if start.elapsed() > Duration::from_secs(60) {
            let _ = child.kill();
            panic!("second start did not exit");
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    assert!(status.success());
    let t = Instant::now();
    while !first.poll_show() {
        assert!(
            t.elapsed() < Duration::from_secs(30),
            "first instance got the show request"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    // Kein Log, keine Konfiguration: der zweite Start kam nicht bis Telemetrie, Audio, Stream.
    assert!(!data_dir.join("logs").exists(), "no logging set up");
    assert!(!data_dir.join("config.json").exists());
    drop(first);
    let _ = std::fs::remove_dir_all(&base);
}
