//! #212: Der Spurkopf ragt nicht in die Zeitleiste, auch nicht mit langem Gerätenamen.
//! Ohne Fenster, ohne Ton, Daten nur im Wegwerf-Ordner.

#![allow(clippy::unwrap_used, clippy::expect_used)] // Tests duerfen abbrechen (Regel 6 gilt fuer Produktivcode).

use eframe::egui;
use woa_gui::{GuiConfig, WoaApp};

fn step(ctx: &egui::Context, app: &mut WoaApp, frame: &mut eframe::Frame, n: u32) {
    let mut input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1600.0, 1000.0),
        )),
        time: Some(f64::from(n) * 0.05),
        ..Default::default()
    };
    input.viewports.entry(egui::ViewportId::ROOT).or_default();
    let _ = ctx.run_ui(input, |ui| {
        eframe::App::logic(app, ui.ctx(), frame);
        eframe::App::ui(app, ui, frame);
    });
}

#[test]
fn track_header_ends_before_bar_one() {
    let tmp = tempfile::tempdir().expect("tmp");
    let config = GuiConfig {
        telemetry_enabled: Some(false),
        ..GuiConfig::default()
    };
    let mut app = WoaApp::without_audio(config, tmp.path().join("rec"));
    let short = app.state_mut().add_track().expect("track");
    let long = app.state_mut().add_track().expect("track");
    app.set_track_source(
        long,
        "app:Mikrofon (USB Audio CODEC mit einem wirklich sehr langen Gerätenamen) Kanal 1+2",
    );
    let ctx = egui::Context::default();
    ctx.set_fonts(woa_gui::theme::font_definitions());
    let mut frame = eframe::Frame::_new_kittest();
    for n in 1..=4 {
        step(&ctx, &mut app, &mut frame, n);
    }
    for id in [short, long] {
        let head = app
            .header_rects()
            .iter()
            .find(|(t, _)| *t == id)
            .map(|(_, r)| *r)
            .expect("Spurkopf gezeichnet");
        // Takt 1 liegt am linken Rand der Zeitleiste (Beat 0).
        let bar_one = app
            .track_rects()
            .iter()
            .find(|(t, _)| *t == id)
            .map(|(_, r)| r.left())
            .expect("Zeitleiste gezeichnet");
        assert!(
            head.right() <= bar_one,
            "Spurkopf endet bei {} hinter Takt 1 bei {bar_one}",
            head.right()
        );
    }
}

/// #334: Mit offener Piano-Roll waren die Spuren nur 36 statt 64 px hoch, die zweite Zeile des
/// Spurkopfs (Eingang, Pegel) war abgeschnitten. Die Spurhöhe bleibt jetzt gleich.
#[test]
fn tracks_keep_their_height_with_the_piano_roll_open() {
    let tmp = tempfile::tempdir().expect("tmp");
    let config = GuiConfig {
        telemetry_enabled: Some(false),
        ..GuiConfig::default()
    };
    let mut app = WoaApp::without_audio(config, tmp.path().join("rec"));
    let prog = woa_model::chords::CHORD_PROGRESSIONS
        .first()
        .expect("Akkordfolge");
    app.state_mut()
        .add_chord_tracks(prog, 0, 4, 0.0)
        .expect("Akkordspuren");
    let clip = app
        .state_mut()
        .project()
        .tracks
        .iter()
        .flat_map(|t| &t.clips)
        .find(|c| c.midi.is_some())
        .map(|c| c.id)
        .expect("MIDI-Clip");
    let ctx = egui::Context::default();
    ctx.set_fonts(woa_gui::theme::font_definitions());
    let mut frame = eframe::Frame::_new_kittest();
    for n in 1..=3 {
        step(&ctx, &mut app, &mut frame, n);
    }
    let closed: Vec<f32> = app.track_rects().iter().map(|(_, r)| r.height()).collect();
    app.open_piano_roll_docked(clip);
    for n in 4..=6 {
        step(&ctx, &mut app, &mut frame, n);
    }
    assert!(app.piano_roll_open());
    let open: Vec<f32> = app.track_rects().iter().map(|(_, r)| r.height()).collect();
    assert!(!open.is_empty());
    assert!(
        open.iter().all(|h| (h - closed[0]).abs() < 0.5) && closed[0] >= 60.0,
        "Spurhöhe mit Piano-Roll {open:?}, ohne {closed:?}"
    );
}
