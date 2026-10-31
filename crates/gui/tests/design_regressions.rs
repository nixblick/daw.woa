//! Design D1–D6 durften keine Bedienung kosten (André 31.10.2026). Diese Tests spielen die
//! Wege über die echte Oberfläche ohne Fenster und ohne Ton durch: Rechtsklick auf den
//! Spurkopf → „Spur löschen“ für Audio, MIDI und Bus, der sichtbare Papierkorb im Spurkopf
//! und im Kanalzug, und das Insert-Rack mit Effekten für jede Spur ohne vorherige Auswahl.

use eframe::egui;
use woa_gui::{GuiConfig, WoaApp};
use woa_model::TrackId;

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
                egui::vec2(1600.0, 1000.0),
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

    fn row(&self, track: TrackId) -> egui::Rect {
        self.app
            .track_rects()
            .iter()
            .find(|(t, _)| *t == track)
            .map(|(_, r)| *r)
            .expect("track row drawn")
    }

    /// Ein Punkt im Spurkopf links neben der Zeitleiste, unten auf freiem Hintergrund.
    fn header_bg(&self, track: TrackId) -> egui::Pos2 {
        let r = self.row(track);
        egui::pos2(r.left() - 30.0, r.bottom() - 3.0)
    }

    fn has(&self, t: TrackId) -> bool {
        self.app.state().project().track(t).is_some()
    }
}

fn three_kinds(rig: &mut Rig) -> [TrackId; 3] {
    let st = rig.app.state_mut();
    let a = st.add_track().expect("audio");
    let m = st.add_midi_track().expect("midi");
    let b = st.add_bus("Bus Test").expect("bus");
    rig.idle();
    rig.idle();
    [a, m, b]
}

const TRASH: &str = woa_gui::theme::ic::TRASH;

/// Der Menüeintrag `text` im gerade geöffneten Kontextmenü: der nächste zum Klickpunkt.
fn menu_item(rig: &Rig, text: &str, near: egui::Pos2) -> egui::Rect {
    rig.find(text)
        .into_iter()
        .filter(|r| r.center().distance(near) < 300.0)
        .min_by(|a, b| {
            a.center()
                .distance(near)
                .total_cmp(&b.center().distance(near))
        })
        .unwrap_or_else(|| panic!("menu item {text:?} near {near:?}"))
}

#[test]
fn right_click_on_track_header_deletes_audio_midi_and_bus() {
    let mut rig = Rig::new();
    let tracks = three_kinds(&mut rig);
    for t in tracks {
        // Rechtsklick auf den freien Hintergrund des Spurkopfs öffnet das Menü.
        let at = rig.header_bg(t);
        rig.click(at, egui::PointerButton::Secondary);
        let item = menu_item(&rig, "Spur löschen", at);
        rig.click(item.center(), egui::PointerButton::Primary);
        assert!(!rig.has(t), "track deleted via context menu");
    }
    assert!(rig.app.state().project().tracks.is_empty());
}

#[test]
fn right_click_on_track_name_opens_the_menu_too() {
    let mut rig = Rig::new();
    let [a, m, b] = three_kinds(&mut rig);
    for t in [m, b, a] {
        let name = rig.app.state().project().track(t).expect("t").name.clone();
        let row = rig.row(t);
        let n = rig
            .find(&name)
            .into_iter()
            .find(|r| {
                r.center().y > row.top() && r.center().y < row.bottom() && r.right() < row.left()
            })
            .expect("name in header");
        rig.click(n.center(), egui::PointerButton::Secondary);
        let item = menu_item(&rig, "Spur löschen", n.center());
        rig.click(item.center(), egui::PointerButton::Primary);
        assert!(!rig.has(t), "deleted via right click on the name");
    }
}

#[test]
fn visible_trash_in_track_header_deletes() {
    let mut rig = Rig::new();
    let [a, m, b] = three_kinds(&mut rig);
    for t in [m, b, a] {
        let row = rig.row(t);
        let btn = rig
            .nodes
            .iter()
            .find(|(l, r)| {
                l == TRASH
                    && r.center().y > row.top()
                    && r.center().y < row.bottom()
                    && r.right() < row.left()
            })
            .map(|(_, r)| *r)
            .expect("trash button in header");
        rig.click(btn.center(), egui::PointerButton::Primary);
        assert!(!rig.has(t));
    }
}

/// Die Knöpfe „Effekte n“ der Kanalzüge, von links nach rechts.
fn fx_buttons(rig: &Rig) -> Vec<egui::Rect> {
    let mut v: Vec<_> = rig
        .nodes
        .iter()
        .filter(|(l, _)| l.contains("Effekte "))
        .map(|(_, r)| *r)
        .collect();
    v.sort_by(|x, y| x.left().total_cmp(&y.left()));
    v.dedup_by(|a, b| (a.left() - b.left()).abs() < 1.0);
    v
}

#[test]
fn effects_are_reachable_for_every_track_without_selecting() {
    let mut rig = Rig::new();
    let tracks = three_kinds(&mut rig);
    // Ohne Auswahl zeigt das Rack sofort einen Kanal samt „+ Effekt“ und „+ Plugin“.
    assert!(
        !rig.find("+ Effekt").is_empty(),
        "effects without selecting"
    );
    assert!(
        !rig.find("+ Plugin").is_empty(),
        "plugins without selecting"
    );
    assert_eq!(fx_buttons(&rig).len(), 3, "one effects button per strip");
    for (i, t) in tracks.into_iter().enumerate() {
        let fx = fx_buttons(&rig)[i];
        rig.click(fx.center(), egui::PointerButton::Primary);
        let rack = rig.app.state().project().track(t).expect("t").name.clone();
        assert!(
            rig.nodes
                .iter()
                .any(|(l, r)| *l == rack && r.left() > 1000.0),
            "rack shows {rack}"
        );
        // Effekt hinzufügen: „+ Effekt“ öffnet die Liste, der erste Eintrag legt ihn an.
        let add = rig.find("+ Effekt")[0];
        // Sichtbar ohne Scrollen: über der Statuszeile, nicht unter dem Dock-Rand.
        assert!(add.bottom() < 960.0, "„+ Effekt“ below the fold: {add:?}");
        rig.click(add.center(), egui::PointerButton::Primary);
        let label = woa_model::EffectKind::ALL[0].label();
        let item = *rig
            .find(label)
            .first()
            .unwrap_or_else(|| panic!("effect {label:?} in the menu"));
        rig.click(item.center(), egui::PointerButton::Primary);
        let n = rig.app.state().project().track(t).expect("t").inserts.len();
        assert_eq!(n, 1, "effect added on track {i}");
        assert_eq!(fx_buttons(&rig).len(), 3);
    }
}

#[test]
fn mixer_strip_trash_deletes_each_kind() {
    let mut rig = Rig::new();
    let _ = three_kinds(&mut rig);
    for _ in 0..3 {
        let fx = fx_buttons(&rig)[0];
        let btn = rig
            .nodes
            .iter()
            .filter(|(l, r)| l == TRASH && r.bottom() < fx.top() && r.top() > fx.top() - 200.0)
            .filter(|(_, r)| r.left() >= fx.left() - 1.0 && r.right() <= fx.right() + 1.0)
            .map(|(_, r)| *r)
            .next()
            .expect("trash in first strip");
        let before = rig.app.state().project().tracks.len();
        rig.click(btn.center(), egui::PointerButton::Primary);
        assert_eq!(rig.app.state().project().tracks.len(), before - 1);
    }
}
