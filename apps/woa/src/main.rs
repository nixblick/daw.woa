//! Desktop-App.
//!
//! Ordner: Konfiguration `<config>/woa/config.json` (Feld `feedback_url`), Logs und
//! Wiederherstellung unter `<data>/woa/`. Unter Linux XDG, unter Windows `%APPDATA%`.

// Unter Windows ohne Konsolenfenster starten (blieb schwarz im Hintergrund offen, André 30.10.).
#![cfg_attr(windows, windows_subsystem = "windows")]

use std::process::ExitCode;

use woa_gui::{GuiConfig, WoaApp};

/// Fenstergröße beim Start: 900×600, mit `WOA_WINDOW=1366x768` eine andere (für Screenshots
/// in fester Größe, #303).
fn window_size(spec: Option<&str>) -> [f32; 2] {
    spec.and_then(|s| s.split_once(['x', 'X']))
        .and_then(|(w, h)| Some([w.trim().parse().ok()?, h.trim().parse().ok()?]))
        .filter(|[w, h]: &[f32; 2]| *w >= 200.0 && *h >= 200.0)
        .unwrap_or([900.0, 600.0])
}

fn main() -> ExitCode {
    // Als Plugin-Prozess gerufen (Absturzschutz, Paket 33): nur das Plugin bedienen, keine Oberfläche.
    if let Some(code) = woa_plugins::child::dispatch() {
        return ExitCode::from(u8::try_from(code).unwrap_or(1));
    }
    // `woa --version` (Pakete prüfen sich damit, ohne Fenster): Version ausgeben und beenden.
    if let Some(text) = info_text(std::env::args_os().nth(1).as_deref()) {
        println!("{text}");
        return ExitCode::SUCCESS;
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
    // WOA-Ordner (#258): fehlende Ordner still anlegen, Fehler nur im Log.
    woa_gui::ordner::apply(&config.ordner);
    let (feedback, telemetry) = (
        !config.feedback_url.is_empty(),
        config.telemetry_enabled == Some(true),
    );

    let mut viewport = eframe::egui::ViewportBuilder::default()
        .with_title("WOA")
        .with_inner_size(window_size(std::env::var("WOA_WINDOW").ok().as_deref()));
    if let Some(icon) = woa_gui::theme::app_icon() {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions {
        viewport,
        event_loop_builder: event_loop_hook(),
        ..Default::default()
    };
    let recovery = data_dir.join("recovery.woa");
    match eframe::run_native(
        "WOA",
        options,
        Box::new(move |cc| {
            // Schriften vor dem ersten Bild setzen (kein leeres erstes Bild).
            let _ = woa_gui::theme::install_fonts(&cc.egui_ctx);
            // Eine kurze Startzeile; die lange wgpu-Adapterliste steht nur auf DEBUG.
            let gpu = cc.wgpu_render_state.as_ref().map(|r| r.adapter.get_info());
            let line = start_line(
                gpu.as_ref().map(|i| format!("{:?}", i.backend)).as_deref(),
                gpu.as_ref().map(|i| i.name.as_str()),
                gpu.as_ref()
                    .map(|i| format!("{} {}", i.driver, i.driver_info))
                    .as_deref(),
            );
            tracing::info!(feedback, telemetry, "{line}");
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

/// Build-Kennung: kurzer Commit aus der CI (`GITHUB_SHA`), lokal „dev“.
fn build_id() -> &'static str {
    option_env!("GITHUB_SHA").map_or("dev", |s| s.get(..8).unwrap_or(s))
}

/// Startzeile: Version, Build, Betriebssystem, Grafik-Backend, Adapter und Treiber.
fn start_line(backend: Option<&str>, adapter: Option<&str>, driver: Option<&str>) -> String {
    let driver = driver
        .map(str::trim)
        .filter(|d| !d.is_empty())
        .unwrap_or("-");
    format!(
        "woa start {} ({}) {}/{} gpu={} \"{}\" driver=\"{}\"",
        env!("CARGO_PKG_VERSION"),
        build_id(),
        std::env::consts::OS,
        std::env::consts::ARCH,
        backend.unwrap_or("-"),
        adapter.unwrap_or("-"),
        driver,
    )
}

/// Text für `--version` und `--help`; bei jedem anderen ersten Argument (ein Projektordner) `None`.
fn info_text(arg: Option<&std::ffi::OsStr>) -> Option<String> {
    let v = env!("CARGO_PKG_VERSION");
    match arg?.to_str()? {
        "--version" | "-V" => Some(format!("woa {v}")),
        "--help" | "-h" => Some(format!(
            "woa {v}: Radio-DAW\nAufruf: woa [PROJEKTORDNER]\n  --version  Version ausgeben"
        )),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{info_text, window_size};
    use std::ffi::OsStr;

    #[test]
    fn window_size_reads_env_spec() {
        assert_eq!(window_size(Some("1366x768")), [1366.0, 768.0]);
        assert_eq!(window_size(Some("1920X1080")), [1920.0, 1080.0]);
        assert_eq!(window_size(Some("kaputt")), [900.0, 600.0]);
        assert_eq!(window_size(None), [900.0, 600.0]);
    }

    #[test]
    fn version_flag_prints_package_version() {
        let t = info_text(Some(OsStr::new("--version"))).unwrap_or_default();
        assert_eq!(t, format!("woa {}", env!("CARGO_PKG_VERSION")));
    }

    #[test]
    fn start_line_names_version_os_and_gpu() {
        let l = super::start_line(Some("Vulkan"), Some("Intel UHD 620"), Some("Mesa 24.1"));
        assert!(l.contains(env!("CARGO_PKG_VERSION")), "{l}");
        assert!(
            l.contains(std::env::consts::OS) && l.contains("Vulkan"),
            "{l}"
        );
        assert!(
            l.contains("Intel UHD 620") && l.contains("Mesa 24.1"),
            "{l}"
        );
        assert!(!l.contains('\n'));
        assert!(super::start_line(None, None, None).contains("gpu=-"));
    }

    #[test]
    fn project_path_is_not_info() {
        assert!(info_text(Some(OsStr::new("jingle.woa"))).is_none());
        assert!(info_text(None).is_none());
    }
}
