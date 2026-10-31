//! Kopfleiste, Über-WOA und Feedback-Fenster (André 31.10.2026): ON AIR steht rechts neben
//! MIDI, die grüne Feedback-Taste öffnet ein großes Fenster mittig, und der Über-Dialog
//! öffnet Andrés echte Spendenlinks. Alles über die echte Oberfläche, ohne Fenster und Ton.

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
    urls: Vec<String>,
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
            urls: Vec::new(),
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
        for c in &out.platform_output.commands {
            if let egui::OutputCommand::OpenUrl(u) = c {
                self.urls.push(u.url.clone());
            }
        }
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
}

impl Rig {
    /// Das erste Element in der Kopfleiste (oberste 40 px), dessen Beschriftung `text` enthält.
    fn top(&self, text: &str) -> egui::Rect {
        self.find(text)
            .into_iter()
            .find(|r| r.center().y < 40.0)
            .unwrap_or_else(|| panic!("{text:?} not in the top bar"))
    }

    fn click_text(&mut self, text: &str) {
        let r = *self
            .find(text)
            .first()
            .unwrap_or_else(|| panic!("{text:?} not found"));
        self.click(r.center(), egui::PointerButton::Primary);
    }
}

#[test]
fn on_air_sits_right_next_to_midi_in_the_top_bar() {
    let rig = Rig::new();
    let midi = rig.top("MIDI");
    let lamp = rig.top("ON AIR");
    assert!((midi.center().y - lamp.center().y).abs() < 6.0, "same row");
    let gap = (lamp.left() - midi.right()).max(midi.left() - lamp.right());
    assert!(gap < 40.0, "ON AIR next to MIDI, gap {gap}");
    // Rechte Gruppe: Feedback ganz rechts, rechts von ON AIR und MIDI.
    let fb = rig.top("Feedback");
    assert!(fb.left() > lamp.right() && fb.left() > midi.right());
    assert!(
        fb.right() > W - 40.0,
        "Feedback is the last item on the right"
    );
    assert!(rig.find("Feedback …").is_empty(), "no ellipsis");
}

#[test]
fn feedback_opens_large_and_centred() {
    let mut rig = Rig::new();
    let fb = rig.top("Feedback");
    rig.click(fb.center(), egui::PointerButton::Primary);
    // Das Bildschirmfoto, das die Taste anfordert, kommt wie vom Backend zurück.
    let image = std::sync::Arc::new(egui::ColorImage::new([2, 2], vec![egui::Color32::BLACK; 4]));
    rig.step(vec![egui::Event::Screenshot {
        viewport_id: egui::ViewportId::ROOT,
        user_data: egui::UserData::default(),
        image,
    }]);
    rig.idle();
    rig.idle();
    let title = *rig
        .find("Schön, dass du dich meldest")
        .first()
        .expect("friendly title");
    let send = *rig.find("Abschicken").first().expect("send button");
    assert!(
        !rig.find("Bildschirmfoto mitsenden").is_empty(),
        "attachment option kept"
    );
    assert!(!rig.find("Lieber später").is_empty(), "cancel kept");
    // Fensterbreite aus linker Titelkante (Symbol davor) und rechter Senden-Kante.
    let width = send.right() - (title.left() - 60.0);
    assert!(width >= 520.0, "dialog is large, width {width}");
    let centre_x = (title.left() - 60.0 + send.right()) / 2.0;
    assert!(
        (centre_x - W / 2.0).abs() < 40.0,
        "centred horizontally: {centre_x}"
    );
    assert!(
        title.top() > 150.0 && send.bottom() < H - 150.0,
        "centred vertically"
    );
    assert!(send.bottom() > title.bottom());
}

#[test]
fn about_shows_tagline_thanks_and_real_donation_links() {
    assert_eq!(
        woa_gui::app::DONATE_PAYPAL,
        "https://www.paypal.com/qrcodes/p2pqrc/KUND7JXYHRGDN"
    );
    assert_eq!(woa_gui::app::DONATE_KOFI, "https://ko-fi.com/nixblick");
    let mut rig = Rig::new();
    rig.click_text("Hilfe");
    rig.click_text("Über WOA");
    assert!(!rig.find("built with love, coffee and AI").is_empty());
    assert!(!rig.find("Danke an alle Lizenzgeber").is_empty());
    assert!(
        !rig.find("SIL Open Font License").is_empty(),
        "licence list kept"
    );
    rig.click_text("PayPal");
    rig.click_text("Ko-fi");
    assert!(
        rig.urls.iter().any(|u| u == woa_gui::app::DONATE_PAYPAL),
        "{:?}",
        rig.urls
    );
    assert!(
        rig.urls.iter().any(|u| u == woa_gui::app::DONATE_KOFI),
        "{:?}",
        rig.urls
    );
}

#[test]
fn every_former_menu_entry_is_still_reachable() {
    let mut rig = Rig::new();
    let menus: [(&str, &[&str]); 5] = [
        (
            "Datei",
            &[
                "Neu",
                "Öffnen",
                "Speichern (Strg+S)",
                "Speichern unter",
                "Importieren",
                "Exportieren",
            ],
        ),
        (
            "Bearbeiten",
            &["Rückgängig", "Wiederholen", "Leeren MIDI-Clip anlegen"],
        ),
        (
            "Ansicht",
            &[
                "Bibliothek",
                "Stundenuhr",
                "Zeit-Events",
                "Erscheinungsbild",
            ],
        ),
        (
            "Einstellungen",
            &[
                "Ausgänge (Soundkarten)",
                "Vorhören (PFL)",
                "Ausgabe neu verbinden",
                "Telefon",
                "MIDI-Eingang",
                "Controller und MIDI-Learn",
                "Fehlercodes anonym senden",
            ],
        ),
        (
            "Hilfe",
            &[
                "Beim Start nach Updates suchen",
                "Jetzt nach Updates suchen",
                "Über WOA",
            ],
        ),
    ];
    for (menu, items) in menus {
        let m = rig.top(menu);
        rig.click(m.center(), egui::PointerButton::Primary);
        for it in items {
            assert!(!rig.find(it).is_empty(), "{menu} → {it}");
        }
        rig.click(m.center(), egui::PointerButton::Primary);
    }
    for (sub, items) in [
        (
            "Importieren",
            ["Audio-Datei", "MIDI-Datei", "DAWproject öffnen"],
        ),
        (
            "Exportieren",
            ["Mixdown exportieren", "Als DAWproject", "MIDI-Clip (.mid)"],
        ),
    ] {
        let m = rig.top("Datei");
        rig.click(m.center(), egui::PointerButton::Primary);
        let s = *rig.find(sub).first().expect("submenu");
        rig.step(vec![egui::Event::PointerMoved(s.center())]);
        rig.click(s.center(), egui::PointerButton::Primary);
        for it in items {
            assert!(!rig.find(it).is_empty(), "Datei → {sub} → {it}");
        }
        rig.click(egui::pos2(W / 2.0, H / 2.0), egui::PointerButton::Primary);
    }
}
