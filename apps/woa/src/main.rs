//! Desktop-App.
//!
//! Ordner: Konfiguration `<config>/woa/config.json` (Feld `feedback_url`), Logs und
//! Wiederherstellung unter `<data>/woa/`. Unter Linux XDG, unter Windows `%APPDATA%`.

use std::process::ExitCode;

use woa_gui::{GuiConfig, WoaApp};

fn main() -> ExitCode {
    let config_dir = woa_radio::config_dir();
    let data_dir = woa_radio::data_dir();

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
            app.set_data_dir(data_dir);
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
