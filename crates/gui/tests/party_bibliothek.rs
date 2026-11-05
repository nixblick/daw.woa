//! #386 (André 03.11.2026): „Wenn man auf Bibliothek klickt, fehlt Rückmeldung.“ In der
//! Party schaltete Ansicht → Bibliothek nur den Haken um, das Fenster kam nie. Ohne Fenster
//! und ohne Ton (AccessKit).

#![allow(clippy::unwrap_used, clippy::expect_used)] // Tests duerfen abbrechen (Regel 6 gilt fuer Produktivcode).

use eframe::egui;
use woa_gui::{GuiConfig, WoaApp};

const H: f32 = 900.0;

struct Rig {
    ctx: egui::Context,
    app: WoaApp,
    frame: eframe::Frame,
    w: f32,
    n: u32,
    nodes: Vec<(String, egui::Rect)>,
    _tmp: tempfile::TempDir,
}

impl Rig {
    fn new(w: f32) -> Self {
        // Mitschnitte und radio.json in einen Wegwerf-Ordner, nie in Andrés Datenordner.
        static DATA: std::sync::OnceLock<tempfile::TempDir> = std::sync::OnceLock::new();
        let data = DATA.get_or_init(|| tempfile::tempdir().expect("data"));
        // SAFETY: einmal je Prozess vor dem ersten App-Start; alle Tests setzen denselben Wert.
        unsafe { std::env::set_var("XDG_DATA_HOME", data.path()) };
        let tmp = tempfile::tempdir().expect("tmp");
        let config = GuiConfig {
            telemetry_enabled: Some(false),
            ..GuiConfig::default()
        };
        let app = WoaApp::without_audio(config, tmp.path().join("rec"));
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let mut rig = Self {
            ctx,
            app,
            frame: eframe::Frame::_new_kittest(),
            w,
            n: 0,
            nodes: Vec::new(),
            _tmp: tmp,
        };
        rig.idle();
        rig.idle();
        rig
    }

    fn step(&mut self, events: Vec<egui::Event>) {
        self.n += 1;
        let mut input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(self.w, H),
            )),
            time: Some(f64::from(self.n) * 0.05),
            events,
            ..Default::default()
        };
        input.viewports.entry(egui::ViewportId::ROOT).or_default();
        let app = &mut self.app;
        let frame = &mut self.frame;
        let out = self.ctx.run_ui(input, |ui| {
            eframe::App::logic(app, ui.ctx(), frame);
            eframe::App::ui(app, ui, frame);
        });
        self.nodes.clear();
        if let Some(update) = out.platform_output.accesskit_update {
            for (_, node) in update.nodes {
                let label = node
                    .label()
                    .or_else(|| node.value())
                    .map(str::to_string)
                    .unwrap_or_default();
                if let Some(b) = node.bounds() {
                    let r = egui::Rect::from_min_max(
                        egui::pos2(b.x0 as f32, b.y0 as f32),
                        egui::pos2(b.x1 as f32, b.y1 as f32),
                    );
                    self.nodes.push((label, r));
                }
            }
        }
    }

    fn idle(&mut self) {
        self.step(Vec::new());
    }

    fn top(&self, text: &str) -> egui::Rect {
        self.nodes
            .iter()
            .find(|(l, r)| l.starts_with(text) && r.center().y < 40.0)
            .map(|(_, r)| *r)
            .unwrap_or_else(|| {
                let top: Vec<_> = self
                    .nodes
                    .iter()
                    .filter(|(_, r)| r.center().y < 40.0)
                    .collect();
                panic!("{text:?} not in the top bar: {top:?}")
            })
    }

    fn click_top(&mut self, text: &str) {
        let at = self.top(text).center();
        self.click_at(at);
    }

    fn click_at(&mut self, at: egui::Pos2) {
        let ev = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        self.step(vec![egui::Event::PointerMoved(at)]);
        self.step(vec![ev(true)]);
        self.step(vec![ev(false)]);
        self.idle();
        self.idle();
    }
}

#[test]
fn library_window_opens_in_party_mode() {
    let mut rig = Rig::new(1600.0);
    rig.click_top("Party");
    rig.click_top("Ansicht");
    let item = rig
        .nodes
        .iter()
        .find(|(l, _)| l == "Bibliothek")
        .map(|(_, r)| r.center())
        .expect("Ansicht → Bibliothek im Menü");
    rig.click_at(item);
    for _ in 0..3 {
        rig.idle();
    }
    assert!(
        rig.nodes.iter().any(|(l, _)| l == "Bibliothek"),
        "Party: das Fenster Bibliothek erscheint nach dem Klick"
    );
}
