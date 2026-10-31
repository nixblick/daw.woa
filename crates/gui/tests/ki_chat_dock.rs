//! KI-Chat im Studio-Dock (#217): der Produzent hat neben Mixer, Piano-Roll und Automation
//! einen Reiter „KI-Chat“. Ohne eingerichtete KI steht dort ein Satz und der Weg zur
//! Einstellungsseite. Echte Oberfläche, ohne Fenster, Ton und KI-Aufruf.

use eframe::egui;
use woa_gui::{GuiConfig, WoaApp};

const W: f32 = 1600.0;
const H: f32 = 1000.0;

struct Rig {
    ctx: egui::Context,
    app: WoaApp,
    frame: eframe::Frame,
    n: u32,
    nodes: Vec<(String, egui::Rect)>,
    _tmp: tempfile::TempDir,
}

impl Rig {
    fn new() -> Self {
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
                egui::vec2(W, H),
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
        // AccessKit meldet je Bild nur Geändertes: alles sammeln.
        if self.nodes.len() > 20_000 {
            self.nodes.clear();
        }
        if let Some(update) = out.platform_output.accesskit_update {
            for (_, node) in update.nodes {
                let label = node
                    .label()
                    .map(str::to_string)
                    .or_else(|| node.value().map(str::to_string))
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

    fn click(&mut self, at: egui::Pos2, button: egui::PointerButton) {
        let ev = |pressed| egui::Event::PointerButton {
            pos: at,
            button,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        self.step(vec![egui::Event::PointerMoved(at)]);
        self.step(vec![ev(true)]);
        self.step(vec![ev(false)]);
        self.idle();
    }

    /// Alle sichtbaren Bedienelemente, deren Beschriftung `text` enthält.
    fn find(&self, text: &str) -> Vec<egui::Rect> {
        self.nodes
            .iter()
            .filter(|(l, _)| l.contains(text))
            .map(|(_, r)| *r)
            .collect()
    }
}

#[test]
fn chat_tab_sits_in_the_studio_dock_and_says_what_is_missing() {
    let mut rig = Rig::new();
    for _ in 0..6 {
        rig.idle();
    }
    assert!(
        !rig.find("KI-Chat").is_empty(),
        "Reiter KI-Chat fehlt im Dock"
    );
    // Die Lage setzt sich erst nach ein paar Bildern; die letzte Meldung gilt.
    let r = *rig.find("KI-Chat").last().unwrap();
    assert!(r.center().y > H / 2.0, "Reiter liegt nicht unten: {r:?}");
    rig.click(r.center(), egui::PointerButton::Primary);
    assert!(
        !rig.find("Noch keine KI eingerichtet.").is_empty(),
        "Hinweis ohne Anbieter fehlt"
    );
    assert!(!rig.find("Einstellungen → KI öffnen").is_empty());
}
