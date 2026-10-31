//! Design D1–D6 durften keine Bedienung kosten (André 31.10.2026). Diese Tests spielen die
//! Wege über die echte Oberfläche ohne Fenster und ohne Ton durch: Rechtsklick auf den
//! Spurkopf oder Kanalzug → „Spur löschen“ für Audio, MIDI und Bus, der eine sichtbare
//! Papierkorb je Spur, das Insert-Rack mit Effekten für jede Spur, das eine Auswahlmodell
//! (Spurkopf und Kanalzug) und Entf für Clip und Spur.

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

impl Rig {
    fn key(&mut self, key: egui::Key) {
        let ev = |pressed| egui::Event::Key {
            key,
            physical_key: Some(key),
            pressed,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        self.step(vec![ev(true)]);
        self.step(vec![ev(false)]);
        self.idle();
    }

    /// Unterkante der letzten Spurzeile: darunter liegt das Dock (Mixer, Rack, Inspektor).
    fn rows_bottom(&self) -> f32 {
        self.app
            .track_rects()
            .iter()
            .map(|(_, r)| r.bottom())
            .fold(0.0, f32::max)
    }

    /// Der Namensstreifen (Scribble-Strip) des Kanalzugs einer Spur im Mixer.
    fn strip(&self, t: TrackId) -> egui::Rect {
        let name = self.app.state().project().track(t).expect("t").name.clone();
        let below = self.rows_bottom();
        self.nodes
            .iter()
            .filter(|(l, r)| *l == name && r.top() > below && r.left() < 1000.0)
            .map(|(_, r)| *r)
            .min_by(|a, b| a.left().total_cmp(&b.left()))
            .unwrap_or_else(|| panic!("strip of {name}"))
    }

    /// Zeigt das Insert-Rack (rechts) den Namen dieser Spur?
    fn rack_shows(&self, t: TrackId) -> bool {
        let name = self.app.state().project().track(t).expect("t").name.clone();
        self.nodes
            .iter()
            .any(|(l, r)| *l == name && r.left() > 1000.0)
    }
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
    assert!(rig.rack_shows(tracks[0]), "rack shows the first track");
    // Kein eigener Knopf mehr je Kanalzug: ein Klick auf den Kanalzug holt ihn ins Rack.
    assert!(rig.find("Effekte ").is_empty(), "no extra effects button");
    for (i, t) in tracks.into_iter().enumerate() {
        let s = rig.strip(t);
        rig.click(s.center(), egui::PointerButton::Primary);
        assert_eq!(rig.app.selected_track(), Some(t));
        assert!(rig.rack_shows(t), "rack shows track {i}");
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
    }
}

#[test]
fn right_click_on_mixer_strip_deletes_each_kind() {
    let mut rig = Rig::new();
    let tracks = three_kinds(&mut rig);
    for t in tracks {
        let s = rig.strip(t);
        rig.click(s.center(), egui::PointerButton::Secondary);
        let item = menu_item(&rig, "Spur löschen", s.center());
        rig.click(item.center(), egui::PointerButton::Primary);
        assert!(!rig.has(t), "deleted via the strip menu");
    }
}

#[test]
fn trash_is_shown_once_per_track() {
    let mut rig = Rig::new();
    let _ = three_kinds(&mut rig);
    let below = rig.rows_bottom();
    let n = rig
        .nodes
        .iter()
        .filter(|(l, r)| l == TRASH && r.top() < below)
        .count();
    assert_eq!(n, 3, "one trash per track header");
    let strips = rig
        .nodes
        .iter()
        .filter(|(l, r)| l == TRASH && r.top() > below)
        .count();
    assert_eq!(strips, 0, "no second trash in the mixer (no clip selected)");
}

#[test]
fn one_selection_for_header_and_strip() {
    let mut rig = Rig::new();
    let [a, m, b] = three_kinds(&mut rig);
    assert_eq!(rig.app.selected_track(), None);
    // Spurkopf wählt, Rack folgt.
    rig.click(rig.header_bg(m), egui::PointerButton::Primary);
    assert_eq!(rig.app.selected_track(), Some(m));
    assert!(rig.rack_shows(m));
    // Kanalzug wählt, Spurkopf und Rack folgen derselben Auswahl.
    let s = rig.strip(b);
    rig.click(s.center(), egui::PointerButton::Primary);
    assert_eq!(rig.app.selected_track(), Some(b));
    assert!(rig.rack_shows(b));
    rig.click(rig.header_bg(a), egui::PointerButton::Primary);
    assert_eq!(rig.app.selected_track(), Some(a));
    assert!(rig.rack_shows(a));
}

#[test]
fn delete_key_removes_selected_clip_then_track() {
    let mut rig = Rig::new();
    let [a, m, _b] = three_kinds(&mut rig);
    let c = rig
        .app
        .state_mut()
        .create_empty_midi_clip(Some(m), 0.0, 16.0)
        .expect("clip");
    rig.idle();
    // Ohne Auswahl tut Entf nichts.
    rig.key(egui::Key::Delete);
    assert_eq!(rig.app.state().project().tracks.len(), 3);
    assert!(rig.app.state().project().clip(c).is_some());
    // Clip anklicken, Entf: nur der Clip ist weg, die Spur bleibt.
    let row = rig.row(m);
    let at = egui::pos2(row.left() + 2.0 * rig.app.zoom(), row.center().y);
    rig.click(at, egui::PointerButton::Primary);
    assert_eq!(rig.app.selected_clip(), Some(c));
    rig.key(egui::Key::Delete);
    assert!(rig.app.state().project().clip(c).is_none(), "clip deleted");
    assert!(rig.has(m), "track stays");
    // Noch einmal Entf: die Spur war nicht angeklickt, also bleibt sie.
    rig.key(egui::Key::Delete);
    assert!(rig.has(m));
    // Spurkopf anklicken, Entf: die Spur ist weg.
    rig.click(rig.header_bg(a), egui::PointerButton::Primary);
    rig.key(egui::Key::Delete);
    assert!(!rig.has(a), "selected track deleted with Entf");
}

#[test]
fn inspector_trash_icon_deletes_the_clip() {
    let mut rig = Rig::new();
    let [_a, m, _b] = three_kinds(&mut rig);
    let c = rig
        .app
        .state_mut()
        .create_empty_midi_clip(Some(m), 0.0, 16.0)
        .expect("clip");
    rig.idle();
    let row = rig.row(m);
    rig.click(
        egui::pos2(row.left() + 2.0 * rig.app.zoom(), row.center().y),
        egui::PointerButton::Primary,
    );
    // Der Inspektor steht über der Zeitleiste, oberhalb der ersten Spurzeile.
    let top = rig.row(rig.app.state().project().tracks[0].id).top();
    let btn = rig
        .nodes
        .iter()
        .find(|(l, r)| l == TRASH && r.bottom() < top)
        .map(|(_, r)| *r)
        .expect("trash icon in the clip inspector");
    rig.click(btn.center(), egui::PointerButton::Primary);
    assert!(rig.app.state().project().clip(c).is_none());
    assert!(rig.has(m));
}

// André 31.10.2026: „Entf darf nur aktiv sein wenn es sinnvoll was zu löschen gibt. Also eine
// Spur markiert ist.“ Markierung (Hervorhebung) und Ziel für Entf sind ein Zustand.

#[test]
fn entf_without_marking_does_nothing_even_though_rack_shows_first_track() {
    let mut rig = Rig::new();
    let [a, _m, _b] = three_kinds(&mut rig);
    assert!(rig.rack_shows(a), "rack shows the first track");
    assert_eq!(rig.app.marked_track(), None, "first track is not marked");
    rig.key(egui::Key::Delete);
    assert_eq!(rig.app.state().project().tracks.len(), 3);
}

#[test]
fn entf_target_is_exactly_the_marked_track() {
    let mut rig = Rig::new();
    let [a, m, _b] = three_kinds(&mut rig);
    rig.click(rig.header_bg(m), egui::PointerButton::Primary);
    assert_eq!(rig.app.marked_track(), Some(m));
    // Klick in die leere Zeitleiste hebt die Markierung auf, Entf tut dann nichts.
    let row = rig.row(a);
    rig.click(
        egui::pos2(row.left() + 8.0 * rig.app.zoom(), row.center().y),
        egui::PointerButton::Primary,
    );
    assert_eq!(rig.app.marked_track(), None, "timeline click unmarks");
    rig.key(egui::Key::Delete);
    assert_eq!(rig.app.state().project().tracks.len(), 3);
    // Wieder markieren: Entf löscht genau die markierte Spur.
    rig.click(rig.header_bg(m), egui::PointerButton::Primary);
    rig.key(egui::Key::Delete);
    assert!(!rig.has(m));
    assert!(rig.has(a));
    assert_eq!(rig.app.marked_track(), None);
}

#[test]
fn entf_ignored_while_track_menu_is_open() {
    let mut rig = Rig::new();
    let [a, _m, _b] = three_kinds(&mut rig);
    rig.click(rig.header_bg(a), egui::PointerButton::Primary);
    rig.click(rig.header_bg(a), egui::PointerButton::Secondary);
    assert!(!rig.find("Spur löschen").is_empty(), "menu open");
    rig.key(egui::Key::Delete);
    assert!(rig.has(a), "Entf with open menu keeps the track");
}

#[test]
fn entf_ignored_while_renaming_a_track() {
    let mut rig = Rig::new();
    let [a, _m, _b] = three_kinds(&mut rig);
    rig.click(rig.header_bg(a), egui::PointerButton::Primary);
    let at = rig.header_bg(a);
    rig.click(at, egui::PointerButton::Secondary);
    let item = menu_item(&rig, "Spur umbenennen", at);
    rig.click(item.center(), egui::PointerButton::Primary);
    rig.idle();
    assert!(
        rig.ctx.egui_wants_keyboard_input(),
        "rename field has focus"
    );
    rig.key(egui::Key::Delete);
    assert!(rig.has(a), "Entf in the rename field keeps the track");
}

#[test]
fn entf_ignored_while_feedback_dialog_is_open() {
    let mut rig = Rig::new();
    let [a, _m, _b] = three_kinds(&mut rig);
    rig.click(rig.header_bg(a), egui::PointerButton::Primary);
    let btn = *rig.find("Feedback").first().expect("feedback button");
    rig.click(btn.center(), egui::PointerButton::Primary);
    rig.key(egui::Key::Delete);
    assert!(rig.has(a), "Entf with feedback dialog keeps the track");
}
