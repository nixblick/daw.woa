//! Fenster „Produktionen“ und Live-Modus (André 31.10.2026): Der Knopf in der Clip-Leiste
//! öffnet das Fenster, und ein MCP-Aufruf über die stdio-Weiterleitung legt im Projekt der
//! App eine Spur an, die mit Undo wieder verschwindet. Ohne Fenster, ohne Ton.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use eframe::egui;
use woa_gui::state::AppState;
use woa_gui::{GuiConfig, WoaApp};

const W: f32 = 1600.0;
const H: f32 = 1000.0;

fn run(
    ctx: &egui::Context,
    app: &mut WoaApp,
    frame: &mut eframe::Frame,
    n: u32,
    events: Vec<egui::Event>,
) -> Vec<(String, egui::Rect)> {
    let mut input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(W, H),
        )),
        time: Some(f64::from(n) * 0.05),
        events,
        ..Default::default()
    };
    input.viewports.entry(egui::ViewportId::ROOT).or_default();
    let out = ctx.run_ui(input, |ui| {
        eframe::App::logic(app, ui.ctx(), frame);
        eframe::App::ui(app, ui, frame);
    });
    let mut nodes = Vec::new();
    if let Some(update) = out.platform_output.accesskit_update {
        for (_, node) in update.nodes {
            let label = node.label().map(str::to_string).unwrap_or_default();
            if let Some(b) = node.bounds() {
                nodes.push((
                    label,
                    egui::Rect::from_min_max(
                        egui::pos2(b.x0 as f32, b.y0 as f32),
                        egui::pos2(b.x1 as f32, b.y1 as f32),
                    ),
                ));
            }
        }
    }
    nodes
}

#[test]
fn button_opens_the_produktionen_window() {
    let tmp = tempfile::tempdir().unwrap();
    let config = GuiConfig {
        telemetry_enabled: Some(false),
        ..GuiConfig::default()
    };
    let mut app = WoaApp::without_audio(config, tmp.path().join("rec"));
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let mut frame = eframe::Frame::_new_kittest();
    let mut n = 0;
    let mut nodes = Vec::new();
    for _ in 0..2 {
        n += 1;
        nodes = run(&ctx, &mut app, &mut frame, n, Vec::new());
    }
    assert!(!app.produktionen_open());
    let at = nodes
        .iter()
        .find(|(l, _)| l.contains("Produktionen"))
        .map(|(_, r)| r.center())
        .expect("Knopf Produktionen");
    let press = |pressed| egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    for ev in [
        vec![egui::Event::PointerMoved(at)],
        vec![press(true)],
        vec![press(false)],
        vec![],
    ] {
        n += 1;
        nodes = run(&ctx, &mut app, &mut frame, n, ev);
    }
    assert!(app.produktionen_open(), "Fenster offen");
    assert!(
        nodes.iter().any(|(l, _)| l.contains("Neu einlesen")),
        "Fensterinhalt sichtbar"
    );
}

const TOKEN: &str = "test-token-gui-live";

#[test]
fn mcp_call_shows_up_in_the_app_project_and_undo_removes_it() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tmp.path().join("daten");
    let server =
        woa_agent::live::LiveServer::start(&data, TOKEN.into(), 0, Arc::new(|| {})).unwrap();
    let app = woa_agent::live::running_app(&data).expect("App erkannt");
    assert_eq!(app.port, server.port());
    let mut state = AppState::new(tmp.path().join("rec"), Instant::now());
    std::fs::create_dir_all(state.project_dir()).unwrap();
    let before = state.project().tracks.len();

    let port = server.port();
    let key = app.key;
    let done = Arc::new(AtomicBool::new(false));
    let client = {
        let done = done.clone();
        std::thread::spawn(move || {
            let key = key;
            let lines = concat!(
                r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"t","version":"1"}}}"#,
                "\n",
                r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
                "\n",
                r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"add_track","arguments":{"name":"Claude-Spur","kind":"audio"}}}"#,
                "\n",
            );
            let mut out = Vec::new();
            woa_agent::live::forward(port, &key, lines.as_bytes(), &mut out).unwrap();
            done.store(true, Ordering::SeqCst);
            String::from_utf8(out).unwrap()
        })
    };
    let start = Instant::now();
    while !done.load(Ordering::SeqCst) {
        let dir = state.project_dir();
        server.serve_pending(state.lend_session(), &dir);
        assert!(start.elapsed() < Duration::from_secs(20));
        std::thread::sleep(Duration::from_millis(5));
    }
    let out = client.join().unwrap();
    assert!(out.contains("track"), "{out}");
    assert_eq!(state.project().tracks.len(), before + 1);
    assert!(
        state
            .project()
            .tracks
            .iter()
            .any(|t| t.name == "Claude-Spur")
    );
    assert_eq!(
        state.session().history().undo_label(),
        Some("Agent: add_track")
    );
    state.undo();
    assert_eq!(state.project().tracks.len(), before);
}
