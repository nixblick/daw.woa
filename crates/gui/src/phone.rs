//! Telefon: Eingang für den Anrufer (Gerät und Kanalpaar) und das Fenster dazu.
//!
//! Der Anrufer kommt über ein Handy am Pult. Der Eingang gilt nur, wenn er hier gewählt ist
//! (nie das Standardmikrofon). Der Rückweg zum Anrufer wird unter „Ausgänge (Soundkarten)“
//! gewählt (Quelle „Telefon-Rückweg“).

use eframe::egui;
use serde::{Deserialize, Serialize};
use woa_engine::device::OutputDeviceInfo;
use woa_engine::record::{InputConfig, list_input_devices, probe_input};

use crate::outputs_ui::pair_label;
use crate::theme;

/// Gewählter Telefon-Eingang, gespeichert in `config.json`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PhoneInput {
    /// Kennung des Geräts, `None` = kein Telefon-Eingang.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_id: Option<String>,
    /// Name bei der Wahl, für verständliche Meldungen, wenn das Gerät fehlt.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub device_name: String,
    /// Kanalpaar: 0 = Kanal 1/2, 1 = Kanal 3/4 …
    #[serde(skip_serializing_if = "is_zero")]
    pub pair: u8,
}

fn is_zero(v: &u8) -> bool {
    *v == 0
}

impl PhoneInput {
    pub fn is_default(&self) -> bool {
        *self == PhoneInput::default()
    }

    /// Name für Meldungen.
    pub fn label(&self) -> String {
        if self.device_name.is_empty() {
            self.device_id.clone().unwrap_or_default()
        } else {
            self.device_name.clone()
        }
    }

    /// Wie `start_pair_input` das Gerät öffnet; `None` = kein Eingang gewählt.
    pub fn input_config(&self) -> Option<InputConfig> {
        self.device_id.as_ref().map(|id| InputConfig {
            device_id: Some(id.clone()),
            pair: self.pair,
        })
    }
}

#[derive(Default)]
pub struct PhoneUi {
    open: bool,
    draft: PhoneInput,
    devices: Vec<OutputDeviceInfo>,
    list_error: Option<String>,
    /// Kanäle des gewählten Geräts (aus der Prüfung), `None` = unbekannt.
    channels: Option<u16>,
    probe_error: Option<String>,
}

impl PhoneUi {
    pub fn open(&mut self, saved: &PhoneInput) {
        self.open = true;
        self.draft = saved.clone();
        self.refresh();
    }

    fn refresh(&mut self) {
        match list_input_devices() {
            Ok(list) => {
                self.devices = list;
                self.list_error = None;
            }
            Err(e) => {
                self.devices.clear();
                self.list_error = Some(format!("Geräte nicht lesbar: {e}"));
            }
        }
        self.probe();
    }

    fn probe(&mut self) {
        self.channels = None;
        self.probe_error = None;
        let Some(cfg) = self.draft.input_config() else {
            return;
        };
        match probe_input(&cfg) {
            Ok(p) => self.channels = Some(p.channels),
            Err(e) => {
                self.probe_error = Some(format!(
                    "{} lässt sich nicht öffnen oder fehlt ({e}). Bitte anstecken und \
                     „Geräte neu lesen“ drücken.",
                    self.draft.label()
                ));
            }
        }
    }

    /// Zeichnet das Fenster. `status`: Meldung der laufenden Ausgabe zum Eingang. Gibt die
    /// Wahl zurück, wenn „Übernehmen“ gedrückt wurde.
    pub fn show(&mut self, ctx: &egui::Context, status: &[String]) -> Option<PhoneInput> {
        if !self.open {
            return None;
        }
        let mut open = true;
        let mut applied = None;
        let mut refresh = false;
        let mut changed = false;
        egui::Window::new("Telefon")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label(
                    egui::RichText::new(
                        "Der Anrufer kommt über ein Handy am Pult in einen Eingang der \
                         Soundkarte. Den Rückweg zum Anrufer (die Sendung ohne seine Stimme) \
                         wählst du unter „Ausgänge (Soundkarten)“ bei „Telefon-Rückweg“, auf \
                         einem eigenen Kanalpaar.",
                    )
                    .small()
                    .color(theme::text_dim()),
                );
                ui.add_space(6.0);
                egui::Grid::new("phone-grid")
                    .num_columns(2)
                    .spacing([12.0, 6.0])
                    .show(ui, |ui| {
                        ui.label("Eingang");
                        let known = self
                            .draft
                            .device_id
                            .as_ref()
                            .is_none_or(|id| self.devices.iter().any(|d| d.id == *id));
                        let shown = match (&self.draft.device_id, known) {
                            (None, _) => "Kein Telefon-Eingang".to_string(),
                            (Some(_), true) => self.draft.label(),
                            (Some(_), false) => format!("{} (fehlt)", self.draft.label()),
                        };
                        egui::ComboBox::from_id_salt("phone-device")
                            .width(280.0)
                            .selected_text(shown)
                            .show_ui(ui, |ui| {
                                if ui
                                    .selectable_label(
                                        self.draft.device_id.is_none(),
                                        "Kein Telefon-Eingang",
                                    )
                                    .clicked()
                                {
                                    self.draft = PhoneInput::default();
                                    changed = true;
                                }
                                for d in &self.devices {
                                    let on = self.draft.device_id.as_deref() == Some(d.id.as_str());
                                    if ui.selectable_label(on, &d.name).clicked() {
                                        self.draft.device_id = Some(d.id.clone());
                                        self.draft.device_name = d.name.clone();
                                        changed = true;
                                    }
                                }
                            });
                        ui.end_row();
                        ui.label("Kanalpaar");
                        ui.add_enabled_ui(self.draft.device_id.is_some(), |ui| {
                            let pairs = self.channels.map_or(4, |c| (c / 2).max(1) as u8);
                            egui::ComboBox::from_id_salt("phone-pair")
                                .selected_text(pair_label(self.draft.pair))
                                .show_ui(ui, |ui| {
                                    for p in 0..pairs {
                                        ui.selectable_value(&mut self.draft.pair, p, pair_label(p));
                                    }
                                });
                        });
                        ui.end_row();
                    });
                if let Some(c) = self.channels {
                    ui.label(
                        egui::RichText::new(format!("Das Gerät hat {c} Eingangskanäle."))
                            .small()
                            .color(theme::text_dim()),
                    );
                }
                for e in [&self.list_error, &self.probe_error].into_iter().flatten() {
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
                if !status.is_empty() {
                    ui.separator();
                    for l in status {
                        ui.label(egui::RichText::new(l).small());
                    }
                }
            });
        if refresh {
            self.refresh();
        } else if changed {
            self.probe();
        }
        if !open || applied.is_some() {
            self.open = false;
        }
        applied
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_off_and_not_saved() {
        let p = PhoneInput::default();
        assert!(p.is_default() && p.input_config().is_none());
        assert_eq!(serde_json::to_string(&p).unwrap(), "{}");
    }

    #[test]
    fn choice_round_trips_with_the_pair() {
        let p = PhoneInput {
            device_id: Some("karte-1".into()),
            device_name: "USB Audio".into(),
            pair: 1,
        };
        let back: PhoneInput = serde_json::from_str(&serde_json::to_string(&p).unwrap()).unwrap();
        assert_eq!(back, p);
        let cfg = p.input_config().unwrap();
        assert_eq!((cfg.device_id.as_deref(), cfg.pair), (Some("karte-1"), 1));
    }
}
