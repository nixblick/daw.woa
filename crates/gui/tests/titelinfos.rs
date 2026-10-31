//! Titelinfos über die echte Oberfläche (André 31.10.2026: „titelinfos ändert geht nicht.
//! egal wo ich doppelt klicke“, „rechtsklick funktioniert auch nicht“): Doppelklick und
//! Kontextmenü auf einen Bibliothekstitel öffnen das Fenster „Titelinfos“, sichtbar oben.

use std::time::{Duration, Instant};

use eframe::egui;
use woa_gui::{GuiConfig, Mode, WoaApp};

const W: f32 = 1366.0;
const H: f32 = 700.0;

struct Rig {
    ctx: egui::Context,
    app: WoaApp,
    frame: eframe::Frame,
    n: u32,
    nodes: Vec<(String, egui::Rect)>,
    track: i64,
    _tmp: tempfile::TempDir,
}

fn write_wav(path: &std::path::Path) {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 8000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut w = hound::WavWriter::create(path, spec).unwrap();
    for i in 0..800 {
        w.write_sample(((i as f32 * 0.2).sin() * 8000.0) as i16)
            .unwrap();
    }
    w.finalize().unwrap();
}

impl Rig {
    fn new(mode: Mode) -> Self {
        let tmp = tempfile::tempdir().expect("tmp");
        let data = tmp.path().join("data");
        let music = tmp.path().join("m");
        std::fs::create_dir_all(&data).unwrap();
        std::fs::create_dir_all(&music).unwrap();
        write_wav(&music.join("Erster Titel.wav"));
        let db = data.join(woa_gui::library::DB_FILE);
        let imp = woa_library::import::start(&db, &music, woa_library::Kind::Music);
        let t0 = Instant::now();
        while !imp.progress.is_finished() {
            assert!(t0.elapsed() < Duration::from_secs(30), "import hangs");
            std::thread::sleep(Duration::from_millis(10));
        }
        let lib = woa_library::Library::open(&db).unwrap();
        let track = lib
            .search(&woa_library::Query::default())
            .unwrap()
            .first()
            .expect("imported")
            .id;
        let config = GuiConfig {
            telemetry_enabled: Some(false),
            mode,
            ..GuiConfig::default()
        };
        let mut app = WoaApp::without_audio(config, tmp.path().join("rec"));
        app.set_data_dir(data);
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let mut rig = Self {
            ctx,
            app,
            frame: eframe::Frame::_new_kittest(),
            n: 0,
            nodes: Vec::new(),
            track,
            _tmp: tmp,
        };
        rig.idle();
        rig.idle();
        rig.click_text("Ansicht");
        rig.click_text("Bibliothek");
        // Menü zu: Escape, dann ein paar Bilder.
        rig.step(vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }]);
        // Eine Sekunde Ruhe: die Menüklicks zählen nicht mehr in die Klickfolge.
        rig.wait(1.0);
        rig
    }

    /// Lässt `secs` Sekunden ohne Eingabe vergehen.
    fn wait(&mut self, secs: f64) {
        for _ in 0..(secs / 0.02).round() as u32 {
            self.idle();
        }
    }

    fn step(&mut self, events: Vec<egui::Event>) {
        self.n += 1;
        let mut input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(W, H),
            )),
            time: Some(f64::from(self.n) * 0.02),
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
                let label = node.label().map(str::to_string).unwrap_or_default();
                if let Some(b) = node.bounds() {
                    self.nodes.push((
                        label,
                        egui::Rect::from_min_max(
                            egui::pos2(b.x0 as f32, b.y0 as f32),
                            egui::pos2(b.x1 as f32, b.y1 as f32),
                        ),
                    ));
                }
            }
        }
    }

    fn idle(&mut self) {
        self.step(Vec::new());
    }

    fn press(&mut self, at: egui::Pos2, button: egui::PointerButton) {
        let ev = |pressed| egui::Event::PointerButton {
            pos: at,
            button,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        self.step(vec![ev(true)]);
        self.step(vec![ev(false)]);
    }

    fn click(&mut self, at: egui::Pos2, button: egui::PointerButton) {
        self.step(vec![egui::Event::PointerMoved(at)]);
        self.press(at, button);
        self.idle();
    }

    fn find(&self, text: &str) -> Vec<egui::Rect> {
        self.nodes
            .iter()
            .filter(|(l, _)| l == text)
            .map(|(_, r)| *r)
            .collect()
    }

    fn click_text(&mut self, text: &str) {
        let r = *self
            .find(text)
            .first()
            .unwrap_or_else(|| panic!("{text:?} not found"));
        self.click(r.center(), egui::PointerButton::Primary);
    }

    /// Mitte der Bibliothekszeile des Titels.
    fn row(&self) -> egui::Pos2 {
        self.ctx
            .read_response(egui::Id::new(("lib_row", self.track)))
            .expect("library row drawn")
            .rect
            .center()
    }

    /// Wartet bis zu 10 s echte Zeit auf das Fenster: die Tags liest ein eigener Thread,
    /// und der Windows-Runner der CI ist dafür manchmal zu langsam (flaky, 31.10.2026).
    fn tag_window_appears(&mut self) -> bool {
        let t0 = Instant::now();
        while t0.elapsed() < Duration::from_secs(10) {
            if self.tag_window_visible() {
                return true;
            }
            self.idle();
            std::thread::sleep(Duration::from_millis(5));
        }
        false
    }

    /// Fenster „Titelinfos“ offen, ganz auf dem Bildschirm und zuoberst.
    fn tag_window_visible(&self) -> bool {
        let id = egui::Id::new(Some("Titelinfos"));
        let Some(rect) = self.ctx.memory(|m| m.area_rect(id)) else {
            return false;
        };
        let shown = self
            .ctx
            .memory(|m| m.areas().visible_layer_ids())
            .iter()
            .any(|l| l.id == id);
        shown
            && self.ctx.content_rect().contains_rect(rect)
            && self.ctx.layer_id_at(rect.center()).map(|l| l.id) == Some(id)
    }
}

/// Klickt auf die Zeile; vor jedem weiteren Klick die Pause aus `gaps` (Sekunden).
fn clicks_on_row(mode: Mode, gaps: &[f64]) -> Rig {
    let mut rig = Rig::new(mode);
    let at = rig.row();
    rig.step(vec![egui::Event::PointerMoved(at)]);
    rig.press(at, egui::PointerButton::Primary);
    for gap in gaps {
        rig.wait(*gap);
        rig.press(at, egui::PointerButton::Primary);
    }
    rig.wait(0.1);
    rig
}

fn double_click_opens(mode: Mode) {
    let mut rig = clicks_on_row(mode, &[0.0]);
    assert!(
        rig.tag_window_appears(),
        "{mode:?}: Doppelklick öffnet die Titelinfos nicht"
    );
}

/// Erst wählen, kurz darauf doppelklicken: egui zählt den zweiten Klick des Doppelklicks
/// als Dreifachklick, und `double_clicked()` ist falsch.
#[test]
fn select_then_double_click_opens_tag_window() {
    let mut rig = clicks_on_row(Mode::Produzent, &[0.36, 0.0]);
    assert!(
        rig.tag_window_appears(),
        "Klick + Doppelklick öffnet die Titelinfos nicht"
    );
}

/// Windows erlaubt 500 ms zwischen den Klicks eines Doppelklicks, egui nur 300 ms.
#[test]
fn slow_windows_double_click_opens_tag_window() {
    let mut rig = clicks_on_row(Mode::Produzent, &[0.4]);
    assert!(
        rig.tag_window_appears(),
        "langsamer Doppelklick öffnet die Titelinfos nicht"
    );
}

fn context_menu_opens(mode: Mode) {
    let mut rig = Rig::new(mode);
    let at = rig.row();
    rig.click(at, egui::PointerButton::Secondary);
    rig.idle();
    rig.click_text("Titelinfos bearbeiten …");
    for _ in 0..4 {
        rig.idle();
    }
    assert!(
        rig.tag_window_appears(),
        "{mode:?}: Kontextmenü öffnet die Titelinfos nicht"
    );
}

#[test]
fn double_click_in_library_opens_tag_window_studio() {
    double_click_opens(Mode::Produzent);
}

#[test]
fn double_click_in_library_opens_tag_window_radio() {
    double_click_opens(Mode::Radio);
}

#[test]
fn context_menu_in_library_opens_tag_window_studio() {
    context_menu_opens(Mode::Produzent);
}

#[test]
fn context_menu_in_library_opens_tag_window_radio() {
    context_menu_opens(Mode::Radio);
}
