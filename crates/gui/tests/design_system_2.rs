//! Design System Fassung 2 (31.10.2026): Rückgängig-Hinweis nach dem Löschen einer Spur, eine
//! Auswahlfarbe an drei Stellen, die vier Zustände des Clip-Inspektors und die Leer-Zustände
//! des Insert-Racks. Alles über die echte Oberfläche, ohne Fenster und ohne Ton.

use eframe::egui;
use woa_gui::{GuiConfig, WoaApp, theme};
use woa_model::{ClipId, TrackId};

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

    fn key(&mut self, key: egui::Key, modifiers: egui::Modifiers) {
        let ev = |pressed| egui::Event::Key {
            key,
            physical_key: Some(key),
            pressed,
            repeat: false,
            modifiers,
        };
        self.step(vec![ev(true)]);
        self.step(vec![ev(false)]);
        self.idle();
    }

    fn find(&self, text: &str) -> Vec<egui::Rect> {
        self.nodes
            .iter()
            .filter(|(l, _)| l.contains(text))
            .map(|(_, r)| *r)
            .collect()
    }

    fn row(&self, track: TrackId) -> egui::Rect {
        self.app
            .track_rects()
            .iter()
            .find(|(t, _)| *t == track)
            .map(|(_, r)| *r)
            .expect("track row drawn")
    }

    fn rows_bottom(&self) -> f32 {
        self.app
            .track_rects()
            .iter()
            .map(|(_, r)| r.bottom())
            .fold(0.0, f32::max)
    }

    fn header_bg(&self, track: TrackId) -> egui::Pos2 {
        let r = self.row(track);
        egui::pos2(r.left() - 30.0, r.bottom() - 3.0)
    }

    /// Punkt auf einem Clip `beat` Beats nach Spurbeginn.
    fn on_clip(&self, track: TrackId, beat: f32) -> egui::Pos2 {
        let row = self.row(track);
        egui::pos2(row.left() + beat * self.app.zoom(), row.center().y + 8.0)
    }

    /// Das Kopf-Zeilenstück über der ersten Spur: dort steht der Clip-Inspektor.
    fn inspector_has(&self, text: &str) -> bool {
        let top = self.row(self.app.state().project().tracks[0].id).top();
        self.nodes
            .iter()
            .any(|(l, r)| l.contains(text) && r.bottom() < top)
    }

    /// Drückt `n` Bilder ohne Eingabe (je 50 ms).
    fn wait(&mut self, n: u32) {
        for _ in 0..n {
            self.idle();
        }
    }
}

fn ctrl() -> egui::Modifiers {
    egui::Modifiers {
        ctrl: true,
        command: true,
        ..Default::default()
    }
}

const UNDO_HINT: &str = "Rückgängig (Strg+Z)";
const TRASH: &str = theme::ic::TRASH;

fn header_trash(rig: &Rig, t: TrackId) -> egui::Rect {
    let row = rig.row(t);
    rig.nodes
        .iter()
        .find(|(l, r)| {
            l == TRASH
                && r.center().y > row.top()
                && r.center().y < row.bottom()
                && r.right() < row.left()
        })
        .map(|(_, r)| *r)
        .expect("trash in header")
}

#[test]
fn deleting_a_track_shows_undo_hint_and_ctrl_z_restores_routing() {
    let mut rig = Rig::new();
    let st = rig.app.state_mut();
    let a = st.add_track().expect("audio");
    let bus = st.add_bus("Hall").expect("bus");
    st.set_send(a, bus, -6.0, false);
    rig.wait(2);
    assert!(rig.find(UNDO_HINT).is_empty(), "no hint before deleting");

    let trash = header_trash(&rig, bus);
    rig.click(trash.center());
    assert!(
        rig.app.state().project().track(bus).is_none(),
        "bus deleted"
    );
    assert!(!rig.find(UNDO_HINT).is_empty(), "undo hint visible");
    assert!(
        !rig.find("Spur „Hall“ gelöscht").is_empty(),
        "hint names the track"
    );

    rig.key(egui::Key::Z, ctrl());
    let p = rig.app.state().project();
    assert!(p.track(bus).is_some(), "Strg+Z brings the bus back");
    let sends = &p.track(a).expect("a").sends;
    assert_eq!(sends.len(), 1, "send restored");
    assert_eq!(sends[0].target, bus);
    assert!(rig.find(UNDO_HINT).is_empty(), "hint gone after undo");
}

#[test]
fn undo_hint_counts_and_disappears_after_eight_seconds() {
    let mut rig = Rig::new();
    let st = rig.app.state_mut();
    let a = st.add_track().expect("a");
    let b = st.add_track().expect("b");
    rig.wait(2);
    rig.click(header_trash(&rig, a).center());
    rig.click(header_trash(&rig, b).center());
    assert!(
        !rig.find("2 Spuren gelöscht").is_empty(),
        "counts deletions"
    );
    // 8 s = 160 Bilder zu 50 ms.
    rig.wait(120);
    assert!(!rig.find(UNDO_HINT).is_empty(), "still there after 6 s");
    rig.wait(60);
    assert!(rig.find(UNDO_HINT).is_empty(), "gone after 8 s");
}

#[test]
fn selection_looks_the_same_in_header_strip_and_rack() {
    let mut rig = Rig::new();
    let st = rig.app.state_mut();
    let _a = st.add_track().expect("a");
    let m = st.add_midi_track().expect("m");
    rig.wait(2);
    rig.click(rig.header_bg(m));
    assert_eq!(rig.app.selected_track(), Some(m));
    rig.idle();
    let color = theme::track_color(1);
    let fill = theme::selection_fill(color);
    let stroke = theme::selection_stroke(color);
    let hits: Vec<egui::Rect> = rig
        .shapes
        .iter()
        .filter(|s| s.fill == fill && s.stroke == stroke)
        .map(|s| s.rect)
        .collect();
    let row = rig.row(m);
    let below = rig.rows_bottom();
    let header = hits
        .iter()
        .any(|r| r.right() <= row.left() + 1.0 && r.top() < below);
    let strip = hits.iter().any(|r| r.top() > below && r.left() < 1000.0);
    let rack = hits.iter().any(|r| r.top() > below && r.left() > 1000.0);
    assert!(header, "header uses the selection style: {hits:?}");
    assert!(strip, "channel strip uses the selection style: {hits:?}");
    assert!(rack, "rack head uses the selection style: {hits:?}");
    // Die andere Spur trägt den Stil nicht.
    let other = theme::selection_fill(theme::track_color(0));
    assert!(!rig.shapes.iter().any(|s| s.fill == other
        && s.stroke.width == 1.0
        && s.stroke.color == theme::track_color(0)));
}

fn audio_clip(rig: &mut Rig, t: TrackId, start: f64) -> ClipId {
    let rate = 48_000u32;
    let s: Vec<f32> = (0..rate * 2)
        .map(|i| (i as f32 * 0.05).sin() * 0.5)
        .collect();
    let st = rig.app.state_mut();
    let dir = st.takes_dir();
    std::fs::create_dir_all(&dir).expect("dir");
    let path = dir.join(format!("c{start}.wav"));
    woa_engine::write_wav_stereo(&path, &s, &s, rate).expect("wav");
    let c = st.add_take(Some(t), &path, start, 4.0, 0.0).expect("clip");
    let _ = &rig.tmp;
    c
}

#[test]
fn inspector_states_empty_audio_midi_and_several() {
    let mut rig = Rig::new();
    let st = rig.app.state_mut();
    let a = st.add_track().expect("a");
    let m = st.add_midi_track().expect("m");
    rig.wait(2);
    let ca = audio_clip(&mut rig, a, 0.0);
    let _cb = audio_clip(&mut rig, a, 8.0);
    let _cc = audio_clip(&mut rig, a, 16.0);
    let cm = rig
        .app
        .state_mut()
        .create_empty_midi_clip(Some(m), 0.0, 16.0)
        .expect("midi");
    rig.wait(2);
    let top0 = rig.row(a).top();

    // Leer
    assert!(rig.inspector_has("Kein Clip gewählt"), "empty state");
    assert!(!rig.inspector_has(TRASH), "no tools without a clip");

    // Ein Audio-Clip: Fade, Gain, Zeit, Werkzeuge.
    rig.click(rig.on_clip(a, 1.0));
    assert_eq!(rig.app.selected_clip(), Some(ca));
    assert_eq!(rig.app.selected_track(), Some(a), "clip selects its track");
    for t in ["FADE", "GAIN", "ZEIT", "1.1.1", "dB", TRASH] {
        assert!(rig.inspector_has(t), "audio inspector shows {t:?}");
    }
    assert!(!rig.inspector_has("NOTEN"));
    assert_eq!(rig.row(a).top(), top0, "no jump");

    // MIDI: Noten (Raster, Q, Velocity), Piano-Roll.
    rig.click(rig.on_clip(m, 1.0));
    assert_eq!(rig.app.selected_clip(), Some(cm));
    for t in ["NOTEN", "1/16", "Q", theme::ic::PIANO_KEYS, TRASH] {
        assert!(rig.inspector_has(t), "midi inspector shows {t:?}");
    }
    assert!(!rig.inspector_has("GAIN"));
    assert_eq!(rig.row(a).top(), top0, "no jump");

    // Mehrere: drei Audio-Clips, unterschiedliche Gains stehen als „—“.
    let ids: Vec<ClipId> = rig
        .app
        .state()
        .project()
        .track(a)
        .expect("a")
        .clips
        .iter()
        .map(|c| c.id)
        .collect();
    rig.app.state_mut().set_clip_gain(ids[1], -6.0);
    rig.click(rig.on_clip(a, 1.0));
    rig.click_with(rig.on_clip(a, 9.0), egui::Modifiers::SHIFT);
    rig.click_with(rig.on_clip(a, 17.0), egui::Modifiers::SHIFT);
    assert!(rig.inspector_has("3 Clips"), "several clips");
    assert!(rig.inspector_has("—"), "mixed values as a dash");
    assert!(rig.inspector_has(TRASH));
    assert_eq!(rig.row(a).top(), top0, "no jump");

    // Eintippen in das gemischte Gain-Feld setzt alle Clips (ein Undo-Schritt).
    let dash = rig
        .nodes
        .iter()
        .filter(|(l, r)| l == "—" && r.bottom() < top0)
        .map(|(_, r)| *r)
        .max_by(|x, y| x.left().total_cmp(&y.left()))
        .expect("gain field");
    rig.click(dash.center());
    rig.step(vec![egui::Event::Key {
        key: egui::Key::A,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: ctrl(),
    }]);
    rig.step(vec![egui::Event::Text("-3".into())]);
    rig.key(egui::Key::Enter, egui::Modifiers::NONE);
    let gains: Vec<f64> = rig
        .app
        .state()
        .project()
        .track(a)
        .expect("a")
        .clips
        .iter()
        .map(|c| c.gain_db)
        .collect();
    assert_eq!(
        gains,
        vec![-3.0, -3.0, -3.0],
        "typed value set on all clips"
    );
    rig.key(egui::Key::Z, ctrl());
    let gains: Vec<f64> = rig
        .app
        .state()
        .project()
        .track(a)
        .expect("a")
        .clips
        .iter()
        .map(|c| c.gain_db)
        .collect();
    assert_eq!(gains, vec![0.0, -6.0, 0.0], "one undo step");
}

#[test]
fn rack_empty_states() {
    let mut rig = Rig::new();
    // Ohne Spur: Symbol, „Keine Spur“, die Knöpfe zum Anlegen.
    assert!(!rig.find("Keine Spur").is_empty(), "rack empty state");
    let add = rig.find("+ Audio-Spur");
    assert!(!add.is_empty());
    rig.click(add[0].center());
    assert_eq!(
        rig.app.state().project().tracks.len(),
        1,
        "+ Audio-Spur adds a track"
    );
    rig.wait(2);
    // Spur ohne Effekte: nur „+ Effekt“ (und „+ Plugin“), keine Slots, keine Sends-Zeile.
    assert!(rig.find("Keine Spur").is_empty());
    assert!(
        !rig.find("INSERTS · 0").is_empty(),
        "inserts group with count"
    );
    assert!(!rig.find("+ Effekt").is_empty());
    assert!(!rig.find("+ Plugin").is_empty());
    assert!(rig.find("SENDS").is_empty(), "no empty sends title");
    assert!(rig.find("Regler auf").is_empty(), "no effect slot");
    // „+ Bus“ als Platzhalter im Mixer legt einen Bus an.
    let bus = rig.find("+ Bus");
    assert_eq!(bus.len(), 1, "one + Bus");
    rig.click(bus[0].center());
    let p = rig.app.state().project();
    assert_eq!(p.tracks.len(), 2);
    assert_eq!(p.tracks[1].kind, woa_model::TrackKind::Bus);
}
