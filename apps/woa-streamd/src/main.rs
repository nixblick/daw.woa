//! Stream-Prozess mit Notfall-Playlist.
//!
//! `woa-streamd [--config <config.json>] [--data-dir <ordner>]`
//!
//! Liest den Schlüssel `stream` aus `config.json`, meldet sich beim Shoutcast-Server an und
//! sendet, bis die Oberfläche „Sendung beenden“ schickt. Läuft getrennt von der
//! Oberfläche weiter, wenn diese abstürzt (dann Notfall-Playlist).

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use woa_stream::StreamConfig;

fn main() -> ExitCode {
    let mut config = woa_radio::config_dir().join("config.json");
    let mut data_dir = woa_radio::data_dir();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--config" => match args.next() {
                Some(p) => config = PathBuf::from(p),
                None => return usage(),
            },
            "--data-dir" => match args.next() {
                Some(p) => data_dir = PathBuf::from(p),
                None => return usage(),
            },
            "--version" => {
                println!("woa-streamd {}", env!("CARGO_PKG_VERSION"));
                return ExitCode::SUCCESS;
            }
            _ => return usage(),
        }
    }
    let _telemetry = match woa_telemetry::init(&data_dir.join("logs").join("streamd"), None) {
        Ok(t) => Some(t),
        Err(e) => {
            eprintln!("woa-streamd: Log nicht verfügbar: {e}");
            None
        }
    };
    let cfg = match StreamConfig::load_from(&config) {
        Ok(c) => c,
        Err(e) => {
            tracing::error!(error = %e, "config unreadable");
            eprintln!("woa-streamd: {e}");
            return ExitCode::from(2);
        }
    };
    tracing::info!(version = env!("CARGO_PKG_VERSION"), server = %cfg.label(), "woa-streamd start");
    match woa_stream::daemon::run(cfg, &data_dir, Arc::new(AtomicBool::new(false))) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            tracing::error!(error = %e, "woa-streamd failed");
            eprintln!("woa-streamd: {e}");
            ExitCode::from(1)
        }
    }
}

fn usage() -> ExitCode {
    eprintln!("Aufruf: woa-streamd [--config <config.json>] [--data-dir <ordner>]");
    ExitCode::from(2)
}
