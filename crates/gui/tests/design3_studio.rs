//! Design-Runde 3, Studio: Beats-Reiter mit Presets und Kits, Trim getrennt vom Fader, kein
//! MIDI-Badge im Spurkopf, Transport mit Export und Takte/Zeit, Dock-Reiter. Echte Oberfläche
//! bei 1366 × 768, ohne Fenster und ohne Ton.

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

impl Rig {
    fn has_exact(&self, text: &str) -> bool {
        self.nodes
            .iter()
            .any(|(l, r)| r.width() > 0.0 && l.trim() == text)
    }

    fn count_exact(&self, text: &str) -> usize {
        self.nodes
            .iter()
            .filter(|(l, r)| r.width() > 0.0 && l.trim() == text)
            .count()
    }
}

#[test]
fn beats_tab_lists_every_preset_and_the_kits() {
    let mut rig = Rig::new();
    rig.press("Beats");
    for p in woa_model::DRUM_PRESETS {
        assert!(rig.has_exact(p.name), "Preset {} fehlt", p.name);
    }
    for k in woa_gui::beats::kits() {
        assert!(rig.find(k.name).is_some(), "Kit {} fehlt", k.name);
    }
    assert!(rig.find("Leerer MIDI-Clip").is_some(), "leerer Clip bleibt");
}

#[test]
fn preset_click_adds_drum_track_with_kit_and_one_undo_removes_it() {
    let mut rig = Rig::new();
    rig.press("Beats");
    rig.press("Elektro-Kit");
    let preset = &woa_model::DRUM_PRESETS[0];
    rig.press(preset.name);
    let project = rig.app.state().project().clone();
    assert_eq!(project.tracks.len(), 1);
    let t = &project.tracks[0];
    assert_eq!(t.kind, woa_model::TrackKind::Midi);
    let kit = woa_gui::beats::kits()
        .find(|k| k.name == "Elektro-Kit")
        .unwrap();
    assert_eq!(t.sound, Some(kit.sound));
    let clip = &t.clips[0];
    let want = woa_model::drums::apply_preset(
        &woa_model::MidiData::default(),
        f64::from(woa_gui::beats::BEAT_BARS) * 4.0,
        preset,
        0,
    );
    assert_eq!(clip.midi.as_ref().unwrap().notes, want.notes);
    rig.app.state_mut().undo();
    assert_eq!(rig.tracks(), 0, "ein Undo nimmt Spur, Kit und Clip zurück");
}

#[test]
fn trim_is_its_own_value_and_undoable() {
    let mut rig = Rig::new();
    rig.press(theme::ic::PLUS);
    rig.press("Audio-Spur");
    let id = rig.app.state().project().tracks[0].id;
    assert!(rig.app.state_mut().set_trim(id, 6.0));
    let t = &rig.app.state().project().tracks[0];
    assert_eq!((t.trim_db, t.gain_db), (6.0, 0.0));
    rig.app.state_mut().undo();
    assert_eq!(rig.app.state().project().tracks[0].trim_db, 0.0);
}

#[test]
fn midi_track_header_has_no_midi_badge() {
    let mut rig = Rig::new();
    // „MIDI“ steht auch in der Kopfleiste (Status); die neue Spur darf keins dazubringen.
    let before = rig.count_exact("MIDI");
    rig.press(theme::ic::PLUS);
    rig.press("MIDI-Spur");
    rig.step(Vec::new());
    assert_eq!(
        rig.count_exact("MIDI"),
        before,
        "MIDI-Badge noch im Spurkopf"
    );
}

#[test]
fn transport_has_export_and_ruler_segments() {
    let rig = Rig::new();
    for label in ["EXPORT", "Takte", "Zeit"] {
        let r = rig
            .find(label)
            .unwrap_or_else(|| panic!("„{label}“ fehlt in der Transportleiste"));
        assert!(r.right() <= 1366.0, "„{label}“ abgeschnitten: {r:?}");
    }
}
