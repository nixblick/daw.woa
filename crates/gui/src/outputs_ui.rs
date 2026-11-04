//! Fenster „Audio-Geräte“ (André 03.11.2026): der eine Ort, an dem WOA Soundkarten wählt.
//! Ausgänge der Sendung (Gerät und Kanalpaar je Quelle), Vorhören (PFL) und die Eingänge
//! (Mikrofon, Telefon, Pult). Menü, Einstellungen und Meldungen verweisen nur hierher.
//! Die Aufnahmequelle einer Spur im Produzenten bleibt an der Spur (sie gehört zum Projekt).

use eframe::egui;
use serde::{Deserialize, Serialize};
use woa_engine::device::{OutputDeviceInfo, list_output_devices};
use woa_engine::record::{InputConfig, list_input_devices};
use woa_radio::routing::{OutputMap, Pult, Route, Source};

use crate::PflOutput;
use crate::phone::PhoneInput;
use crate::theme;

/// Kanalpaare, die zur Wahl stehen (Kanal 1/2 bis 7/8).
const PAIRS: u8 = 4;

/// Name des Fensters, auf den Menü, Einstellungen und Meldungen verweisen.
pub const WINDOW: &str = "Audio-Geräte";

/// Gewähltes Mikrofon des Radios, gespeichert in `config.json` (Feld `mic`, seit 0.41.7).
/// Ohne Wahl gilt das Standard-Eingabegerät des Systems (wie vor 0.41.7).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct MicInput {
    /// Kennung des Geräts, `None` = Systemstandard.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_id: Option<String>,
    /// Name bei der Wahl, für verständliche Meldungen, wenn das Gerät fehlt.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub device_name: String,
    /// Kanalpaar: 0 = Kanal 1/2, 1 = Kanal 3/4 …
    #[serde(skip_serializing_if = "is_zero")]
    pub pair: u8,
}

/// Höhe der Rollfläche im Fenster „Audio-Geräte“: höchstens 560 px, sonst Fensterhöhe minus
/// Titelleiste, Ränder und Menü (#443), nie unter 200 px.
pub(crate) fn window_scroll_height(screen_h: f32) -> f32 {
    (screen_h - 120.0).clamp(200.0, 560.0)
}

fn is_zero(v: &u8) -> bool {
    *v == 0
}

impl MicInput {
    pub fn is_default(&self) -> bool {
        *self == MicInput::default()
    }

    /// Name für Meldungen.
    pub fn label(&self) -> String {
        match &self.device_id {
            None => "Systemstandard".to_string(),
            Some(id) if self.device_name.is_empty() => id.clone(),
            Some(_) => self.device_name.clone(),
        }
    }

    /// Wie `start_live_input` das Gerät öffnet (`device_id: None` = Systemstandard).
    pub fn input_config(&self) -> InputConfig {
        InputConfig {
            device_id: self.device_id.clone(),
            pair: if self.device_id.is_some() {
                self.pair
            } else {
                0
            },
        }
    }
}

/// Alles, was das Fenster wählt. „Übernehmen“ gibt es zurück; der Aufrufer speichert und
/// wendet nur an, was sich geändert hat.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AudioChoice {
    pub outputs: OutputMap,
    pub pult: Pult,
    pub pfl: PflOutput,
    pub mic: MicInput,
    pub phone: PhoneInput,
}

#[derive(Default)]
pub struct OutputsUi {
    open: bool,
    draft: AudioChoice,
    devices: Vec<OutputDeviceInfo>,
    inputs: Vec<OutputDeviceInfo>,
    list_error: Option<String>,
}

/// Name eines Kanalpaars: 0 = „Kanal 1/2“.
pub fn pair_label(pair: u8) -> String {
    format!(
        "Kanal {}/{}",
        2 * u16::from(pair) + 1,
        2 * u16::from(pair) + 2
    )
}

/// Was „nicht zugeordnet“ bedeutet: die Quellen laufen mit der Playlist, der Rückweg zum
/// Anrufer gibt es dann nicht.
fn follow_label(s: Source) -> &'static str {
    if s == Source::PhoneReturn {
        "Kein Rückweg"
    } else {
        "Wie Playlist"
    }
}

/// Ein kurzer Satz je Ausgangszeile: wofür sie ist.
pub fn source_hint(s: Source) -> &'static str {
    match s {
        Source::Playlist => "Die Sendung. Gibt den Takt vor.",
        Source::Carts => "Jingles und Einspieler der Cart-Wall.",
        Source::DeckA | Source::DeckB | Source::DeckC => "Von Hand gestartete Titel.",
        Source::PhoneReturn => "Was der Anrufer hört: die Sendung ohne seine Stimme.",
    }
}

/// Kurze Sätze zu Vorhören und Eingängen.
pub const MIC_HINT: &str = "Dein Mikrofon für die Sendung (Knopf „Mikro“). Systemstandard ist \
                            das in Windows eingestellte Eingabegerät.";
pub const PHONE_HINT: &str =
    "Der Anrufer über ein Handy am Pult. Den Rückweg wählst du oben bei „Telefon-Rückweg“.";
pub const PFL_HINT: &str = "Kopfhörer zum Vorhören, nie auf Sendung.";

/// Name der Vorhören-Wahl.
fn pfl_label(p: &PflOutput, devices: &[OutputDeviceInfo]) -> String {
    match p {
        PflOutput::Aus => "Aus".to_string(),
        PflOutput::Kanal34 => "Kanal 3/4 des Sendegeräts".to_string(),
        PflOutput::Geraet(id) => devices
            .iter()
            .find(|d| d.id == *id)
            .map_or_else(|| format!("{id} (fehlt)"), |d| d.name.clone()),
    }
}

fn hint(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text).small().color(theme::text_dim()));
}

/// Gerät und Kanalpaar eines Eingangs. `none`: Name für „kein Gerät“ (`device_id: None`).
fn input_row(
    ui: &mut egui::Ui,
    salt: &str,
    inputs: &[OutputDeviceInfo],
    none: &str,
    device_id: &mut Option<String>,
    device_name: &mut String,
    pair: &mut u8,
) {
    let known = device_id
        .as_ref()
        .is_none_or(|id| inputs.iter().any(|d| d.id == *id));
    let label = if device_name.is_empty() {
        device_id.clone().unwrap_or_default()
    } else {
        device_name.clone()
    };
    let shown = match (&device_id, known) {
        (None, _) => none.to_string(),
        (Some(_), true) => label,
        (Some(_), false) => format!("{label} (fehlt)"),
    };
    egui::ComboBox::from_id_salt((salt, "device"))
        .width(260.0)
        .selected_text(shown)
        .show_ui(ui, |ui| {
            if ui.selectable_label(device_id.is_none(), none).clicked() {
                *device_id = None;
                device_name.clear();
                *pair = 0;
            }
            for d in inputs {
                let on = device_id.as_deref() == Some(d.id.as_str());
                if ui.selectable_label(on, &d.name).clicked() {
                    *device_id = Some(d.id.clone());
                    device_name.clone_from(&d.name);
                }
            }
        });
    ui.add_enabled_ui(device_id.is_some(), |ui| {
        egui::ComboBox::from_id_salt((salt, "pair"))
            .selected_text(pair_label(*pair))
            .show_ui(ui, |ui| {
                for p in 0..PAIRS {
                    ui.selectable_value(pair, p, pair_label(p));
                }
            });
    });
}

impl OutputsUi {
    /// Öffnet das Fenster mit der gespeicherten Wahl und liest die Geräte neu.
    pub fn open(&mut self, saved: AudioChoice) {
        self.open = true;
        self.draft = saved;
        self.refresh();
    }

    fn refresh(&mut self) {
        match list_output_devices() {
            Ok(list) => {
                self.devices = list;
                self.list_error = None;
            }
            Err(e) => {
                self.devices.clear();
                self.list_error = Some(format!("Geräte nicht lesbar: {e}"));
            }
        }
        match list_input_devices() {
            Ok(list) => self.inputs = list,
            Err(e) => {
                self.inputs.clear();
                self.list_error = Some(format!("Eingänge nicht lesbar: {e}"));
            }
        }
    }

    /// Zeichnet das Fenster. `lines`: Meldungen der laufenden Ausgabe (Ausgänge, Mikrofon,
    /// Telefon). Gibt die Wahl zurück, wenn „Übernehmen“ gedrückt wurde.
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        lines: &[String],
        on_air: bool,
    ) -> Option<AudioChoice> {
        if !self.open {
            return None;
        }
        let mut applied = None;
        let mut open = true;
        let mut refresh = false;
        // #443: Das Fenster bleibt im Bild. Oben beginnen, auf die Fensterhöhe begrenzen, Rest rollt.
        let screen = ctx.content_rect();
        let scroll_h = window_scroll_height(screen.height());
        egui::Window::new(WINDOW)
            .open(&mut open)
            .collapsible(false)
            .default_width(640.0_f32.min((screen.width() - 40.0).max(320.0)))
            .default_pos(egui::pos2(
                (screen.center().x - 320.0).max(screen.left() + 8.0),
                screen.top() + 40.0,
            ))
            .constrain(true)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .max_height(scroll_h)
                    .show(ui, |ui| {
                        hint(
                            ui,
                            "Alle Soundkarten an einer Stelle. Die Aufnahmequelle einer Spur im \
                         Produzenten wählst du an der Spur.",
                        );
                        ui.add_space(6.0);
                        ui.heading("Ausgänge");
                        hint(
                            ui,
                            "Jede Quelle der Sendung bekommt ein Gerät und ein Kanalpaar. Mehrere \
                         Soundkarten laufen gleichzeitig. Produzent und Party spielen auf dem \
                         Standardgerät des Systems.",
                        );
                        self.outputs_grid(ui);
                        ui.separator();
                        ui.heading("Vorhören (PFL)");
                        hint(ui, PFL_HINT);
                        self.pfl_row(ui);
                        ui.separator();
                        ui.heading("Eingänge");
                        self.inputs_grid(ui);
                        ui.add_space(4.0);
                        self.pult_section(ui);
                        if let Some(e) = &self.list_error {
                            ui.colored_label(theme::on_air(), e);
                        }
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            if ui.button("Geräte neu lesen").clicked() {
                                refresh = true;
                            }
                            if ui.button("Übernehmen").clicked() {
                                applied = Some(self.draft.clone());
                            }
                        });
                        if on_air {
                            hint(
                                ui,
                                "Die Sendung läuft: Mikrofon und Telefon gelten sofort. Ausgänge, \
                             Pult und Vorhören gelten, sobald nichts mehr läuft.",
                            );
                        }
                        if !lines.is_empty() {
                            ui.separator();
                            for l in lines {
                                ui.label(egui::RichText::new(l).small());
                            }
                        }
                    });
            });
        if refresh {
            self.refresh();
        }
        if !open || applied.is_some() {
            self.open = false;
        }
        applied
    }

    fn outputs_grid(&mut self, ui: &mut egui::Ui) {
        egui::Grid::new("outputs-grid")
            .num_columns(4)
            .spacing([12.0, 6.0])
            .show(ui, |ui| {
                for s in Source::ALL {
                    ui.label(s.label());
                    let map = &mut self.draft.outputs;
                    let mut route = map.get(s);
                    let mut follow = s != Source::Playlist && map.explicit(s).is_none();
                    device_combo(&self.devices, ui, s, &mut route, &mut follow);
                    ui.add_enabled_ui(!follow, |ui| {
                        egui::ComboBox::from_id_salt(("pair", s))
                            .selected_text(pair_label(route.pair))
                            .show_ui(ui, |ui| {
                                for p in 0..PAIRS {
                                    ui.selectable_value(&mut route.pair, p, pair_label(p));
                                }
                            });
                    });
                    hint(ui, source_hint(s));
                    ui.end_row();
                    if follow {
                        map.clear(s);
                    } else {
                        map.set(s, route);
                    }
                }
            });
    }

    fn pfl_row(&mut self, ui: &mut egui::Ui) {
        let p = &mut self.draft.pfl;
        egui::ComboBox::from_id_salt("pfl-device")
            .width(280.0)
            .selected_text(pfl_label(p, &self.devices))
            .show_ui(ui, |ui| {
                ui.selectable_value(p, PflOutput::Aus, "Aus");
                ui.selectable_value(p, PflOutput::Kanal34, "Kanal 3/4 des Sendegeräts");
                for d in &self.devices {
                    ui.selectable_value(p, PflOutput::Geraet(d.id.clone()), &d.name);
                }
            });
    }

    fn inputs_grid(&mut self, ui: &mut egui::Ui) {
        let pult_on = self.draft.pult.on;
        egui::Grid::new("inputs-grid")
            .num_columns(3)
            .spacing([12.0, 6.0])
            .show(ui, |ui| {
                ui.label("Mikrofon");
                ui.add_enabled_ui(!pult_on, |ui| {
                    ui.horizontal(|ui| {
                        let m = &mut self.draft.mic;
                        input_row(
                            ui,
                            "mic",
                            &self.inputs,
                            "Systemstandard",
                            &mut m.device_id,
                            &mut m.device_name,
                            &mut m.pair,
                        );
                    });
                });
                hint(
                    ui,
                    if pult_on {
                        "Im Pult-Modus hängt das Mikro am Pult (WOAs Mikro ist aus)."
                    } else {
                        MIC_HINT
                    },
                );
                ui.end_row();
                ui.label("Telefon");
                ui.horizontal(|ui| {
                    let ph = &mut self.draft.phone;
                    input_row(
                        ui,
                        "phone",
                        &self.inputs,
                        "Kein Telefon-Eingang",
                        &mut ph.device_id,
                        &mut ph.device_name,
                        &mut ph.pair,
                    );
                });
                hint(ui, PHONE_HINT);
                ui.end_row();
            });
    }

    /// Pult-Modus (#301): ein Schalter und der Eingang vom Pult (Gerät, Kanalpaar).
    fn pult_section(&mut self, ui: &mut egui::Ui) {
        ui.checkbox(&mut self.draft.pult.on, "Sendung kommt vom Pult");
        hint(
            ui,
            "An: Playlist, Carts und Decks gehen auf die Ausgänge oben (Richtung Pult). Stream \
             und Mitschnitt bekommen nur den Eingang vom Pult. Vorhören braucht ein eigenes Gerät.",
        );
        ui.add_enabled_ui(self.draft.pult.on, |ui| {
            ui.horizontal(|ui| {
                ui.label("Eingang vom Pult");
                let r = &mut self.draft.pult.input;
                input_row(
                    ui,
                    "pult",
                    &self.inputs,
                    "Kein Eingang",
                    &mut r.device_id,
                    &mut r.device_name,
                    &mut r.pair,
                );
            });
        });
    }
}

fn device_combo(
    devices: &[OutputDeviceInfo],
    ui: &mut egui::Ui,
    s: Source,
    route: &mut Route,
    follow: &mut bool,
) {
    let known = route
        .device_id
        .as_ref()
        .is_none_or(|id| devices.iter().any(|d| d.id == *id));
    let shown = match (&route.device_id, known) {
        _ if *follow => follow_label(s).to_string(),
        (None, _) => "Standardgerät".to_string(),
        (Some(_), true) => route.device_label(),
        (Some(_), false) => format!("{} (fehlt)", route.device_label()),
    };
    egui::ComboBox::from_id_salt(("device", s))
        .width(240.0)
        .selected_text(shown)
        .show_ui(ui, |ui| {
            if s != Source::Playlist && ui.selectable_label(*follow, follow_label(s)).clicked() {
                *follow = true;
            }
            if ui
                .selectable_label(!*follow && route.device_id.is_none(), "Standardgerät")
                .clicked()
            {
                *follow = false;
                route.device_id = None;
                route.device_name.clear();
            }
            for d in devices {
                let on = !*follow && route.device_id.as_deref() == Some(d.id.as_str());
                if ui.selectable_label(on, &d.name).clicked() {
                    *follow = false;
                    route.device_id = Some(d.id.clone());
                    route.device_name = d.name.clone();
                }
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mic_default_is_system_default_and_not_saved() {
        let m = MicInput::default();
        assert!(m.is_default());
        assert_eq!(m.input_config(), InputConfig::default());
        assert_eq!(m.label(), "Systemstandard");
        assert_eq!(serde_json::to_string(&m).unwrap(), "{}");
    }

    #[test]
    fn mic_choice_round_trips_with_the_pair() {
        let m = MicInput {
            device_id: Some("usb-1".into()),
            device_name: "USB Audio CODEC".into(),
            pair: 1,
        };
        let back: MicInput = serde_json::from_str(&serde_json::to_string(&m).unwrap()).unwrap();
        assert_eq!(back, m);
        let cfg = m.input_config();
        assert_eq!((cfg.device_id.as_deref(), cfg.pair), (Some("usb-1"), 1));
    }

    #[test]
    fn every_row_has_a_sentence() {
        for s in Source::ALL {
            assert!(source_hint(s).ends_with('.'), "{s:?}");
        }
        for h in [MIC_HINT, PHONE_HINT, PFL_HINT] {
            assert!(h.ends_with('.'));
        }
    }

    /// Nur dieses Fenster wählt Geräte (André 03.11.: „doppelt gemoppelt“). Ausnahme: die
    /// Aufnahmequelle je Spur (`sources.rs`), die zum Projekt gehört.
    /// #443: Auch im 1280×720-Fenster passt die Rollfläche samt Titelleiste und Menü ins Bild.
    #[test]
    fn audio_device_window_fits_small_screens() {
        assert!(window_scroll_height(720.0) + 120.0 <= 720.0);
        assert_eq!(window_scroll_height(1440.0), 560.0);
        assert!(window_scroll_height(100.0) >= 200.0);
    }

    #[test]
    fn devices_are_listed_only_here() {
        for (name, src) in [
            ("app.rs", include_str!("app.rs")),
            ("phone.rs", include_str!("phone.rs")),
            ("radio.rs", include_str!("radio.rs")),
            ("settings.rs", include_str!("settings.rs")),
        ] {
            for f in ["list_output_devices(", "list_input_devices("] {
                assert!(!src.contains(f), "{name} listet Geräte selbst ({f})");
            }
        }
        let radio = include_str!("radio.rs").replace("\r\n", "\n");
        let toggle = radio
            .split("fn toggle_mic(")
            .nth(1)
            .and_then(|s| s.split("\n    fn ").next())
            .expect("toggle_mic");
        assert!(
            !toggle.contains("InputConfig::default()"),
            "Mikro nimmt die Wahl"
        );
    }
}
