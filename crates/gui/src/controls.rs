//! Controller und MIDI-Learn (Paket 14) in der Oberfläche: Gerät wählen, Queue je Bild
//! leeren, Aktionen an die Oberfläche weitergeben, Zuordnung je Gerät in `config.json`,
//! LEDs nachführen, Fenster mit der Zuordnungsliste.
//!
//! Der Audio-Thread sieht davon nichts: Was ein Fader oder Pad auslöst, geht denselben Weg
//! wie ein Mausklick (Projekt-Befehl oder Sendemixer-Befehl über `rtrb`).

use std::time::{Duration, Instant};

use eframe::egui;
use rtrb::Consumer;
use woa_io::controller::{
    Action, Bus, CtrlMsg, Layout, LedOut, Mapper, Mapping, Target, control_queue,
};
use woa_io::midi::{MidiIn, MidiStatus, event_sink, stable_name};
use woa_model::{MAX_GAIN_DB, MIN_GAIN_DB};

use crate::feedback::GuiConfig;
use crate::theme;

/// Wie oft ein fehlender LED-Ausgang neu gesucht wird.
const LED_RETRY: Duration = Duration::from_secs(2);

/// Was die Oberfläche für den Controller können muss. Die App setzt es um; Tests setzen
/// es mit Zählern um.
pub trait Surface {
    /// Wert, den die Oberfläche für ein stetiges Ziel zeigt (0..1), für den Takeover.
    fn current(&self, target: &Target) -> Option<f32>;
    /// Fader/Regler.
    fn set(&mut self, target: Target, value: f32);
    /// Taster/Pad.
    fn press(&mut self, target: Target);
    /// LED an (Cart läuft, Mikro offen, Spur stumm …).
    fn lit(&self, target: &Target) -> bool;
}

/// Gibt die Aktionen eines Bilds an die Oberfläche. Liefert gelernte Zuordnungen zurück
/// (zum Speichern).
pub fn dispatch(actions: Vec<Action>, surface: &mut impl Surface) -> Vec<Mapping> {
    let mut learned = Vec::new();
    for a in actions {
        match a {
            Action::Set { target, value } => surface.set(target, value),
            Action::Press(target) => surface.press(target),
            Action::Learned(m) => learned.push(m),
        }
    }
    learned
}

/// Spur-Fader: 0..1 linear über den dB-Bereich der Spur.
pub fn gain_from_norm(v: f32) -> f64 {
    MIN_GAIN_DB + f64::from(v.clamp(0.0, 1.0)) * (MAX_GAIN_DB - MIN_GAIN_DB)
}

pub fn gain_to_norm(db: f64) -> f32 {
    ((db - MIN_GAIN_DB) / (MAX_GAIN_DB - MIN_GAIN_DB)).clamp(0.0, 1.0) as f32
}

/// Pan: 0 = links, 0,5 = Mitte, 1 = rechts. Um die Mitte rastet er ein (± 1 Schritt).
pub fn pan_from_norm(v: f32) -> f64 {
    let p = f64::from(v.clamp(0.0, 1.0)) * 2.0 - 1.0;
    if p.abs() < 1.5 / 127.0 { 0.0 } else { p }
}

pub fn pan_to_norm(pan: f64) -> f32 {
    ((pan.clamp(-1.0, 1.0) + 1.0) / 2.0) as f32
}

/// Pegel im Sendemixer: 0..1 linear über −60..+6 dB (unten = aus).
pub fn level_from_norm(v: f32) -> f64 {
    use woa_radio::mixer::{LEVEL_MAX_DB, LEVEL_MIN_DB};
    LEVEL_MIN_DB + f64::from(v.clamp(0.0, 1.0)) * (LEVEL_MAX_DB - LEVEL_MIN_DB)
}

pub fn level_to_norm(db: f64) -> f32 {
    use woa_radio::mixer::{LEVEL_MAX_DB, LEVEL_MIN_DB};
    ((db - LEVEL_MIN_DB) / (LEVEL_MAX_DB - LEVEL_MIN_DB)).clamp(0.0, 1.0) as f32
}

/// Der Controller-Eingang der App samt Zuordnung.
pub struct ControlDesk {
    input: MidiIn,
    rx: Consumer<CtrlMsg>,
    pub mapper: Mapper,
    led: Option<LedOut>,
    led_checked: Option<Instant>,
    /// Fenster „Controller“ offen.
    pub window: bool,
}

impl ControlDesk {
    pub fn new(config: &GuiConfig) -> Self {
        let (sink, rx) = control_queue();
        let input = MidiIn::new(event_sink()).with_controls(sink);
        let mut desk = Self {
            input,
            rx,
            mapper: Mapper::default(),
            led: None,
            led_checked: None,
            window: false,
        };
        if let Some(name) = config.controller_input.clone() {
            desk.mapper.set_mappings(maps_for(config, &name));
            desk.input.select(Some(name));
        }
        desk
    }

    /// Weckt die Oberfläche, sobald der Controller etwas schickt (einmal setzen).
    pub fn set_waker(&self, ctx: &egui::Context) {
        let ctx = ctx.clone();
        self.input
            .set_waker(Box::new(move || ctx.request_repaint()));
    }

    /// Gewähltes Gerät.
    pub fn wanted(&self) -> Option<&str> {
        self.input.wanted()
    }

    pub fn status(&self) -> MidiStatus {
        self.input.status()
    }

    pub fn ports(&self) -> &[String] {
        self.input.ports()
    }

    /// Gerät wählen (`None` = aus); die Zuordnung dieses Geräts wird geladen.
    pub fn select(&mut self, name: Option<String>, config: &mut GuiConfig) {
        self.mapper.set_mappings(
            name.as_deref()
                .map_or_else(Vec::new, |n| maps_for(config, n)),
        );
        self.led = None;
        self.led_checked = None;
        config.controller_input = name.clone();
        self.input.select(name);
    }

    /// Geräteabfrage (Hotplug) und LED-Ausgang suchen.
    pub fn poll(&mut self, now: Instant) {
        self.input.poll(now);
        let connected = matches!(self.input.status(), MidiStatus::Connected(_));
        if !connected {
            self.led = None;
            return;
        }
        if self.led.is_none()
            && self
                .led_checked
                .is_none_or(|t| now.duration_since(t) >= LED_RETRY)
        {
            self.led_checked = Some(now);
            if let MidiStatus::Connected(name) = self.input.status() {
                self.led = LedOut::open(&name);
            }
        }
    }

    /// Ein Bild: Queue leeren, Aktionen ausführen, LEDs nachführen. `true` = es kam
    /// etwas an (neu zeichnen); gelernte Zuordnungen stehen schon in `config`.
    pub fn frame(&mut self, surface: &mut impl Surface, config: &mut GuiConfig) -> bool {
        let actions = self.mapper.pump(&mut self.rx, |t| surface.current(t));
        let any = !actions.is_empty();
        let learned = dispatch(actions, surface);
        for m in &learned {
            tracing::info!(
                source = %m.source.label(),
                target = %m.target.label(),
                "controller mapping learned"
            );
        }
        if !learned.is_empty() {
            self.store(config);
        }
        if let Some(led) = self.led.as_mut() {
            led.update(&self.mapper.leds(|t| surface.lit(t)));
        }
        any
    }

    /// Schreibt die Zuordnung des gewählten Geräts in `config`.
    pub fn store(&self, config: &mut GuiConfig) {
        let Some(name) = self.input.wanted() else {
            return;
        };
        let key = stable_name(name).to_string();
        if self.mapper.mappings().is_empty() {
            config.controller_maps.remove(&key);
        } else {
            config
                .controller_maps
                .insert(key, self.mapper.mappings().to_vec());
        }
    }

    /// Kurzer Text für das Lernen, `None` = es wird nicht gelernt.
    pub fn learning_text(&self) -> Option<String> {
        self.mapper.learning().map(|t| {
            format!(
                "MIDI-Learn: {} – jetzt Regler bewegen oder Pad drücken",
                t.label()
            )
        })
    }

    /// Fenster mit Geräteauswahl und Zuordnungsliste. `true` = Einstellungen geändert
    /// (speichern).
    pub fn window(&mut self, ctx: &egui::Context, config: &mut GuiConfig) -> bool {
        if !self.window {
            return false;
        }
        let mut open = true;
        let mut changed = false;
        egui::Window::new("Controller und MIDI-Learn")
            .open(&mut open)
            .default_width(460.0)
            .show(ctx, |ui| changed = self.window_ui(ui, config));
        self.window = open;
        changed
    }

    fn window_ui(&mut self, ui: &mut egui::Ui, config: &mut GuiConfig) -> bool {
        let mut changed = false;
        let wanted = self.input.wanted().map(str::to_string);
        let mut ports: Vec<String> = self.input.ports().to_vec();
        if let Some(w) = &wanted
            && !ports.contains(w)
        {
            ports.push(w.clone());
        }
        let mut choice = wanted.clone();
        ui.horizontal(|ui| {
            ui.label("Gerät");
            egui::ComboBox::from_id_salt("controller-device")
                .selected_text(wanted.as_deref().map_or("aus", stable_name))
                .width(300.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut choice, None, "aus");
                    for p in &ports {
                        ui.selectable_value(&mut choice, Some(p.clone()), stable_name(p));
                    }
                });
        });
        if choice != wanted {
            self.select(choice, config);
            changed = true;
        }
        let (text, color) = match self.input.status() {
            MidiStatus::Off => ("Kein Controller gewählt".to_string(), theme::text_dim()),
            MidiStatus::Waiting(n) => (
                format!("{} nicht da, suche alle 2 s", stable_name(&n)),
                theme::on_air(),
            ),
            MidiStatus::Connected(_) => (
                if self.led.is_some() {
                    "Verbunden, LEDs der Pads werden nachgeführt".to_string()
                } else {
                    "Verbunden (Gerät ohne LED-Eingang)".to_string()
                },
                theme::accent(),
            ),
        };
        ui.label(egui::RichText::new(text).small().color(color));
        ui.label(
            egui::RichText::new(
                "Lernen: Rechtsklick auf Fader, Stumm-Knopf, Start, Stopp, Mikro, Pegel oder \
                 Cart, dort „MIDI lernen“, dann am Gerät bewegen oder drücken. Fader springen \
                 nicht: sie greifen erst, wenn sie den Wert auf dem Bildschirm erreichen.",
            )
            .small()
            .color(theme::text_dim()),
        );
        ui.separator();
        if let Some(t) = self.learning_text() {
            ui.horizontal(|ui| {
                ui.colored_label(theme::accent(), t);
                if ui.small_button("Abbrechen").clicked() {
                    self.mapper.cancel_learn();
                }
            });
        }
        let mut remove = None;
        if self.mapper.mappings().is_empty() {
            ui.label(egui::RichText::new("Noch nichts zugeordnet.").color(theme::text_dim()));
        } else {
            egui::Grid::new("controller-maps")
                .num_columns(3)
                .striped(true)
                .spacing(egui::vec2(16.0, 6.0))
                .show(ui, |ui| {
                    ui.label(egui::RichText::new("Quelle").strong());
                    ui.label(egui::RichText::new("Ziel").strong());
                    ui.label("");
                    ui.end_row();
                    for (i, m) in self.mapper.mappings().iter().enumerate() {
                        ui.label(egui::RichText::new(m.source.label()).monospace());
                        ui.label(m.target.label());
                        if ui
                            .small_button("Löschen")
                            .on_hover_text("Zuordnung entfernen")
                            .clicked()
                        {
                            remove = Some(i);
                        }
                        ui.end_row();
                    }
                });
        }
        if let Some(i) = remove {
            self.mapper.remove(i);
            self.store(config);
            changed = true;
        }
        ui.separator();
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new("Ohne Knopf im Fenster:").small());
            for (label, t) in [
                ("Layout umschalten lernen", Target::Mode(Layout::Toggle)),
                ("Musik-Pegel lernen", Target::Level(Bus::Music)),
            ] {
                if ui.small_button(label).clicked() {
                    self.mapper.learn(t);
                }
            }
        });
        changed
    }
}

/// Zuordnung des Geräts `name` aus der Konfiguration.
fn maps_for(config: &GuiConfig, name: &str) -> Vec<Mapping> {
    config
        .controller_maps
        .get(stable_name(name))
        .cloned()
        .unwrap_or_default()
}

/// Kontextmenü-Eintrag „MIDI lernen“ unter einem Widget; zeigt die Quelle, wenn schon
/// zugeordnet. Liefert das Ziel, wenn gewählt.
pub fn learn_menu(ui: &mut egui::Ui, target: Target, bound: Option<String>) -> Option<Target> {
    let mut chosen = None;
    let text = match bound {
        Some(src) => format!("MIDI lernen: {} (jetzt {src})", target.label()),
        None => format!("MIDI lernen: {}", target.label()),
    };
    if ui.button(text).clicked() {
        chosen = Some(target);
        ui.close();
    }
    chosen
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn norm_round_trips() {
        for db in [MIN_GAIN_DB, -12.0, 0.0, MAX_GAIN_DB] {
            assert!((gain_from_norm(gain_to_norm(db)) - db).abs() < 1e-4);
        }
        assert_eq!(pan_from_norm(0.5), 0.0);
        assert_eq!(pan_from_norm(64.0 / 127.0), 0.0, "Mitte rastet");
        assert_eq!(pan_from_norm(0.0), -1.0);
        assert!((level_from_norm(level_to_norm(-6.0)) + 6.0).abs() < 1e-4);
        assert_eq!(level_from_norm(0.0), woa_radio::mixer::LEVEL_MIN_DB);
    }
}
