//! Desktop-App.
//!
//! Ordner: Konfiguration `<config>/woa/config.json` (Feld `feedback_url`), Logs und
//! Wiederherstellung unter `<data>/woa/`. Unter Linux XDG, unter Windows `%APPDATA%`.

use std::path::PathBuf;
use std::process::ExitCode;

use woa_gui::{GuiConfig, WoaApp};

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
}

fn base_dir(xdg: &str, fallback: &str) -> PathBuf {
    if let Some(p) = std::env::var_os("APPDATA") {
        return PathBuf::from(p).join("woa");
    }
    std::env::var_os(xdg)
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(fallback))
        .join("woa")
}

fn main() -> ExitCode {
    let config_dir = base_dir("XDG_CONFIG_HOME", ".config");
    let data_dir = base_dir("XDG_DATA_HOME", ".local/share");

    let _telemetry = match woa_telemetry::init(&data_dir.join("logs"), None) {
        Ok(t) => Some(t),
        Err(e) => {
            eprintln!("woa: logging disabled: {e}");
            None
        }
    };
    let config = GuiConfig::load(&config_dir.join("config.json"));
    tracing::info!(feedback = !config.feedback_url.is_empty(), "woa start");

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("WOA")
            .with_inner_size([900.0, 600.0]),
        ..Default::default()
    };
    let recovery = data_dir.join("recovery.woa");
    match eframe::run_native(
        "WOA",
        options,
        Box::new(move |_cc| Ok(Box::new(WoaApp::new(config, recovery)))),
    ) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            tracing::error!(error = %e, "window failed");
            eprintln!("woa: {e}");
            ExitCode::FAILURE
        }
    }
}
