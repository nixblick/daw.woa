//! Kopfleiste aus einem Guss (André 31.10.2026): MIDI, ON AIR, REC, Modus-Umschalter und
//! Feedback gleich hoch, mit Abständen aus den Tokens, ohne Überlappen bei 1280 und 900 px.
//! REC ist der eine Mitschnitt-Knopf für alle Modi. Ohne Fenster und ohne Ton (AccessKit).

use eframe::egui;
use woa_gui::widgets::topbar::{TOPBAR_GAP, TOPBAR_ITEM_H, TOPBAR_SEGMENT_GAP};
use woa_gui::{GuiConfig, WoaApp};

const H: f32 = 800.0;
const ITEMS: [&str; 7] = [
    "MIDI",
    "ON AIR",
    "REC",
    "Produzent",
    "Party",
    "Radio",
    "Feedback",
];

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

fn check_row(w: f32) {
    let rig = Rig::new(w);
    let mut rects: Vec<(&str, egui::Rect)> = ITEMS.iter().map(|t| (*t, rig.top(t))).collect();
    for (t, r) in &rects {
        assert!(
            (r.height() - TOPBAR_ITEM_H).abs() < 0.5,
            "{w}px: {t} height {}",
            r.height()
        );
        assert!(r.right() <= w + 0.5, "{w}px: {t} off screen");
    }
    rects.sort_by(|a, b| a.1.left().total_cmp(&b.1.left()));
    let segs = ["Produzent", "Party", "Radio"];
    for pair in rects.windows(2) {
        let ((a, ra), (b, rb)) = (pair[0], pair[1]);
        let gap = rb.left() - ra.right();
        let min = if segs.contains(&a) && segs.contains(&b) {
            TOPBAR_SEGMENT_GAP
        } else {
            TOPBAR_GAP
        };
        assert!(gap >= min - 0.5, "{w}px: gap {a}→{b} = {gap}, token {min}");
    }
    // Rechts der Menüs, nichts überlappt das letzte Menü.
    let hilfe = rig.top("Hilfe");
    assert!(
        rects[0].1.left() > hilfe.right(),
        "{w}px: overlaps the menu"
    );
}

#[test]
fn top_bar_items_share_height_and_token_gaps_at_1280() {
    check_row(1280.0);
}

#[test]
fn top_bar_items_do_not_overlap_at_900() {
    check_row(900.0);
}

#[test]
fn rec_toggles_in_every_mode_and_survives_mode_switch() {
    let mut rig = Rig::new(1280.0);
    for mode in ["Party", "Radio", "Produzent"] {
        rig.click_top(mode);
        assert!(!rig.app.mitschnitt_laeuft(), "{mode}: idle");
        rig.click_top("REC");
        for _ in 0..20 {
            rig.idle();
        }
        assert!(rig.app.mitschnitt_laeuft(), "{mode}: REC starts");
        rig.click_top("REC");
        for _ in 0..20 {
            rig.idle();
        }
        assert!(!rig.app.mitschnitt_laeuft(), "{mode}: REC stops");
    }
    rig.click_top("REC");
    for _ in 0..20 {
        rig.idle();
    }
    assert!(rig.app.mitschnitt_laeuft(), "running");
    for mode in ["Radio", "Party", "Produzent", "Radio"] {
        rig.click_top(mode);
        assert!(rig.app.mitschnitt_laeuft(), "{mode} keeps REC");
    }
    rig.click_top("REC");
}

#[test]
fn no_mitschnitt_button_left_in_the_views() {
    let mut rig = Rig::new(1600.0);
    for mode in ["Radio", "Party", "Produzent"] {
        rig.click_top(mode);
        assert!(
            !rig.nodes.iter().any(|(l, _)| l.contains("Mitschnitt")),
            "{mode}: old Mitschnitt button still there"
        );
    }
}
