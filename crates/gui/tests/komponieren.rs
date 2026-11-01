//! Design System 2, Komponieren (`ChordTrack`, `SoundPicker`): Akkordspur unter dem Lineal,
//! Abschnitt „Instrument“ im Rack, MIDI-Eingang im Spurkopf. Über die echte Oberfläche,
//! ohne Fenster und ohne Ton.

use eframe::egui;
use woa_gui::{GuiConfig, WoaApp};

#[allow(dead_code)]
struct Rig {
    ctx: egui::Context,
    app: WoaApp,
    frame: eframe::Frame,
    n: u32,
    mods: egui::Modifiers,
    nodes: Vec<(String, egui::Rect)>,
    shapes: Vec<egui::epaint::RectShape>,
    tmp: tempfile::TempDir,
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
            mods: egui::Modifiers::NONE,
            nodes: Vec::new(),
            shapes: Vec::new(),
            tmp,
        };
        // Das Dock legt sich in den ersten Bildern noch zurecht.
        rig.wait(10);
        rig
    }

    fn step(&mut self, events: Vec<egui::Event>) {
        self.n += 1;
        let mut input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1600.0, 1000.0),
            )),
            time: Some(f64::from(self.n) * 0.05),
            events,
            modifiers: self.mods,
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
        fn walk(s: &egui::Shape, out: &mut Vec<egui::epaint::RectShape>) {
            match s {
                egui::Shape::Rect(r) => out.push(r.clone()),
                egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
                _ => {}
            }
        }
        self.shapes.clear();
        for c in &out.shapes {
            walk(&c.shape, &mut self.shapes);
        }
    }

    fn idle(&mut self) {
        self.step(Vec::new());
    }

    fn click_with(&mut self, at: egui::Pos2, modifiers: egui::Modifiers) {
        let ev = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers,
        };
        self.mods = modifiers;
        self.step(vec![egui::Event::PointerMoved(at)]);
        self.step(vec![ev(true)]);
        self.step(vec![ev(false)]);
        self.mods = egui::Modifiers::NONE;
        self.idle();
    }

    fn click(&mut self, at: egui::Pos2) {
        self.click_with(at, egui::Modifiers::NONE);
    }

    fn find(&self, text: &str) -> Vec<egui::Rect> {
        self.nodes
            .iter()
            .filter(|(l, _)| l.contains(text))
            .map(|(_, r)| *r)
            .collect()
    }

    fn wait(&mut self, n: u32) {
        for _ in 0..n {
            self.idle();
        }
    }
}

fn chords(rig: &mut Rig, key: u8) -> Vec<woa_model::TrackId> {
    let p = woa_model::chords::progression("I–V–vi–IV").expect("pop");
    let made = rig
        .app
        .state_mut()
        .add_chord_tracks(p, key, 4, 0.0)
        .expect("made");
    rig.wait(4);
    made.iter().map(|m| m.0).collect()
}

fn top_of(rig: &Rig, text: &str) -> f32 {
    rig.find(text)
        .first()
        .unwrap_or_else(|| panic!("{text} fehlt"))
        .top()
}

#[test]
fn chord_lane_names_key_and_chords_and_follows_key_change() {
    let mut rig = Rig::new();
    assert!(
        rig.find("Akkorde · ").is_empty(),
        "ohne MIDI keine Akkordspur"
    );
    let tracks = chords(&mut rig, 0);
    let lane = rig.find("Akkorde · C Dur");
    assert_eq!(lane.len(), 1, "Kopf der Akkordspur");
    let first_row = rig
        .app
        .track_rects()
        .iter()
        .find(|(t, _)| *t == tracks[0])
        .map(|(_, r)| *r)
        .expect("row");
    assert!(
        lane[0].bottom() <= first_row.top(),
        "Akkordspur über den Spuren"
    );
    let xs: Vec<f32> = ["C I", "G V", "Am vi", "F IV"]
        .iter()
        .map(|c| {
            let r = rig.find(c);
            assert!(!r.is_empty(), "Chip {c}");
            assert!(
                r[0].bottom() <= first_row.top(),
                "Chip {c} in der Akkordspur"
            );
            r[0].left()
        })
        .collect();
    assert!(xs.windows(2).all(|w| w[0] < w[1]), "Chips am Takt: {xs:?}");

    let clips: Vec<_> = rig
        .app
        .state()
        .project()
        .tracks
        .iter()
        .flat_map(|t| t.clips.iter().map(|c| c.id))
        .collect();
    assert!(rig.app.state_mut().transpose_clips(&clips, 2));
    rig.wait(2);
    assert_eq!(
        rig.find("Akkorde · D Dur").len(),
        1,
        "folgt dem Tonartwechsel"
    );
    assert!(!rig.find("D I").is_empty());
}

#[test]
fn instrument_is_its_own_rack_section_before_the_inserts() {
    let mut rig = Rig::new();
    let tracks = chords(&mut rig, 0);
    // Ohne Auswahl zeigt das Rack die erste Spur (Akkorde, Pad, Streicher).
    let instrument = top_of(&rig, "INSTRUMENT");
    assert!(
        instrument < top_of(&rig, "INSERTS"),
        "Instrument vor den Inserts"
    );
    assert!(
        instrument > top_of(&rig, "EINGANG"),
        "Instrument nach dem Eingang"
    );
    assert!(!rig.find("Pad · GeneralUser GS").is_empty(), "Rolle · Bank");
    let open = rig.find("Klang wählen")[0];
    rig.click(open.center());
    for role in ["Lead", "Bass", "Pad", "Kit"] {
        assert!(!rig.find(role).is_empty(), "Rolle {role}");
    }
    // Rolle Bass, Klang E-Bass.
    let bass = rig.find(&format!("{} Bass", woa_gui::theme::ic::WAVE_SINE))[0];
    assert!(bass.top() > instrument, "Rollen im Abschnitt Instrument");
    rig.click(bass.center());
    let tile = rig.find("E-Bass")[0];
    rig.click(tile.center());
    let sound = |rig: &Rig| {
        rig.app
            .state()
            .project()
            .track(tracks[0])
            .and_then(|t| t.sound)
    };
    assert_eq!(sound(&rig).map(|s| (s.program, s.drums)), Some((33, false)));
    // Das Rack scrollt; den Synth-Knopf ins Bild holen.
    let at = rig.find("INSTRUMENT")[0].center();
    rig.step(vec![
        egui::Event::PointerMoved(at),
        egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, -300.0),
            phase: egui::TouchPhase::Move,
            modifiers: egui::Modifiers::NONE,
        },
    ]);
    rig.wait(30);
    let synth = rig.find("Eingebauter Synth")[0];
    rig.click(synth.center());
    assert_eq!(sound(&rig), None, "eingebauter Synth bleibt erreichbar");
}

#[test]
fn midi_track_input_names_the_midi_device_not_the_microphone() {
    let mut rig = Rig::new();
    chords(&mut rig, 0);
    assert!(
        rig.find("Mikrofon").is_empty(),
        "MIDI-Spur zeigt nie „Mikrofon“"
    );
    assert!(!rig.find("Kein MIDI-Gerät").is_empty());
}
