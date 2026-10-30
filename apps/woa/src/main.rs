//! Desktop-App.
//!
//! Ordner: Konfiguration `<config>/woa/config.json` (Feld `feedback_url`), Logs und
//! Wiederherstellung unter `<data>/woa/`. Unter Linux XDG, unter Windows `%APPDATA%`.

// Unter Windows ohne Konsolenfenster starten (blieb schwarz im Hintergrund offen, André 30.10.).
#![cfg_attr(windows, windows_subsystem = "windows")]

use std::process::ExitCode;

use woa_gui::{GuiConfig, WoaApp};

fn main() -> ExitCode {
    // Als Plugin-Prozess gerufen (Absturzschutz, Paket 33): nur das Plugin bedienen, keine Oberfläche.
    if let Some(code) = woa_plugins::child::dispatch() {
        return ExitCode::from(u8::try_from(code).unwrap_or(1));
    }
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
        event_loop_builder: event_loop_hook(),
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

/// Unter Wayland läuft die Ereignisschleife von eframe 0.35 heiß. Ein fälliger Repaint setzt
/// `ControlFlow::Poll`, und `RedrawRequested` kommt erst mit dem nächsten Frame-Callback des
/// Compositors. Ein verdecktes Fenster bekommt keinen, dann frisst die Oberfläche einen
/// ganzen Kern (egui#8326, behoben in eframe 0.37, das rustc 1.95 braucht). Gemessen am
/// 29.10.2026 mit `woa` 0.3.4 in Ruhe: Wayland 24–91 %, X11 1 %. Deshalb nimmt WOA unter
/// Linux X11 (XWayland), solange `DISPLAY` gesetzt ist. `WOA_WAYLAND=1` erzwingt Wayland.
#[cfg(target_os = "linux")]
fn event_loop_hook() -> Option<eframe::EventLoopBuilderHook> {
    let wayland_wanted = std::env::var_os("WOA_WAYLAND").is_some_and(|v| v == "1");
    if wayland_wanted || std::env::var_os("DISPLAY").is_none() {
        return None;
    }
    tracing::info!("event loop: x11 (WOA_WAYLAND=1 for wayland)");
    Some(Box::new(|b| {
        use winit::platform::x11::EventLoopBuilderExtX11 as _;
        b.with_x11();
    }))
}

#[cfg(not(target_os = "linux"))]
fn event_loop_hook() -> Option<eframe::EventLoopBuilderHook> {
    None
}
