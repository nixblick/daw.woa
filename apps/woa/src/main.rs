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

    // Config first: the upload (opt-in) depends on it. Warnings while loading go to stderr
    // only, the log is not set up yet.
    let config_path = config_dir.join("config.json");
    let mut config = GuiConfig::load(&config_path);
    let save_err = if config.ensure_install_id() {
        config.save(&config_path).err()
    } else {
        None
    };
    let _telemetry = match woa_telemetry::init(&data_dir.join("logs"), Some(config.upload_config()))
    {
        Ok(t) => Some(t),
        Err(e) => {
            eprintln!("woa: logging disabled: {e}");
            None
        }
    };
    if let Some(e) = save_err {
        tracing::warn!(error = %e, "config not saved");
    }
    tracing::info!(
        feedback = !config.feedback_url.is_empty(),
        telemetry = config.telemetry_enabled == Some(true),
        "woa start"
    );

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
        Box::new(move |_cc| {
            let mut app = WoaApp::new(config, recovery);
            app.set_config_path(config_path);
            // `woa <projektordner>` öffnet das Projekt gleich beim Start.
            if let Some(dir) = std::env::args_os().nth(1) {
                app.open_project(std::path::Path::new(&dir));
            }
            Ok(Box::new(app))
        }),
    ) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            tracing::error!(error = %e, "window failed");
            eprintln!("woa: {e}");
            ExitCode::FAILURE
        }
    }
}
