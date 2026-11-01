//! Produzent nach Design System 2 (ScreenStudio, Browser): ein Browser links mit sechs Reitern
//! statt der Knopfreihe unter den Spuren, neue Spur über „+“ im Spurenkopf. Alle Erzeuger bleiben
//! erreichbar. Echte Oberfläche bei 1366 × 768, ohne Fenster und ohne Ton.

use eframe::egui;
use woa_gui::{GuiConfig, WoaApp, theme};

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
        for _ in 0..10 {
            rig.step(Vec::new());
        }
        rig
    }

    fn step(&mut self, events: Vec<egui::Event>) {
        self.n += 1;
        let mut input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1366.0, 768.0),
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

    fn click(&mut self, at: egui::Pos2) {
        let ev = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        self.step(vec![egui::Event::PointerMoved(at)]);
        self.step(vec![ev(true)]);
        self.step(vec![ev(false)]);
        self.step(Vec::new());
        self.step(Vec::new());
    }

    /// Erst genau (ohne Groß-/Kleinschreibung, Reiter stehen in Versalien), sonst enthalten.
    fn find(&self, text: &str) -> Option<egui::Rect> {
        let t = text.to_lowercase();
        let hit = |exact: bool| {
            self.nodes
                .iter()
                .find(|(l, r)| {
                    let l = l.to_lowercase();
                    r.width() > 0.0 && if exact { l.trim() == t } else { l.contains(&t) }
                })
                .map(|(_, r)| *r)
        };
        hit(true).or_else(|| hit(false))
    }

    fn press(&mut self, text: &str) {
        let r = self
            .find(text)
            .unwrap_or_else(|| panic!("„{text}“ nicht gefunden"));
        assert!(
            r.right() <= 1366.0 && r.bottom() <= 768.0,
            "„{text}“ liegt außerhalb: {r:?}"
        );
        self.click(r.center());
    }

    fn tracks(&self) -> usize {
        self.app.state().project().tracks.len()
    }
}

#[test]
fn browser_replaces_the_button_row_under_the_tracks() {
    let rig = Rig::new();
    for old in [
        "Akkordfolge …",
        "Jingle-FX …",
        "+ MIDI-Clip (4 T.)",
        "+  Audio-Spur",
    ] {
        assert!(rig.find(old).is_none(), "alte Knopfreihe noch da: {old}");
    }
    for tab in ["Klänge", "Beats", "Akkorde", "FX", "Samples", "Jingles"] {
        let r = rig
            .find(tab)
            .unwrap_or_else(|| panic!("Reiter {tab} fehlt"));
        assert!(r.left() < 300.0, "Reiter {tab} steht nicht links: {r:?}");
    }
}

#[test]
fn plus_in_track_header_adds_audio_and_midi_tracks() {
    let mut rig = Rig::new();
    rig.press(theme::ic::PLUS);
    rig.press("Audio-Spur");
    assert_eq!(rig.tracks(), 1);
    rig.press(theme::ic::PLUS);
    rig.press("MIDI-Spur");
    assert_eq!(rig.tracks(), 2);
}

#[test]
fn chords_and_fx_insert_from_the_browser_at_the_playhead() {
    let mut rig = Rig::new();
    rig.press("Akkorde");
    rig.press("Einfügen · Takt 1");
    assert_eq!(rig.tracks(), 3, "Akkorde, Bass, Arpeggio");
    rig.press("FX");
    rig.press("Einfügen · Takt 1");
    assert_eq!(rig.tracks(), 4, "FX-Clip auf neuer Audio-Spur");
}

#[test]
fn other_generators_stay_reachable() {
    let mut rig = Rig::new();
    rig.press("Beats");
    rig.press("Leerer MIDI-Clip");
    assert_eq!(rig.tracks(), 1, "MIDI-Clip legt eine Spur an");
    rig.press("Klänge");
    assert!(rig.find("Audio importieren").is_some());
    assert!(rig.find("Produktionen").is_some());
    rig.press("Samples");
    assert!(rig.find("Freesound").is_some());
    rig.press("Jingles");
    rig.press("Jingle bauen");
}
