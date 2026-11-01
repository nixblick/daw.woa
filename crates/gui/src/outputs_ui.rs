//! Fenster „Ausgänge“: Gerät und Kanalpaar je Quelle (Playlist, Cart-Wall …).

use eframe::egui;
use woa_engine::device::{OutputDeviceInfo, list_output_devices};
use woa_engine::record::list_input_devices;
use woa_radio::routing::{OutputMap, Pult, Route, Source};

use crate::theme;

/// Kanalpaare, die zur Wahl stehen (Kanal 1/2 bis 7/8).
const PAIRS: u8 = 4;

#[derive(Default)]
pub struct OutputsUi {
    open: bool,
    draft: OutputMap,
    /// Pult-Modus (#301) samt Eingang.
    pult: Pult,
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

impl OutputsUi {
    /// Öffnet das Fenster mit der gespeicherten Zuordnung und liest die Geräte neu.
    pub fn open(&mut self, saved: &OutputMap, pult: &Pult) {
        self.open = true;
        self.draft = saved.clone();
        self.pult = pult.clone();
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
        self.inputs = list_input_devices().unwrap_or_default();
    }

    /// Zeichnet das Fenster. `lines`: Meldungen und Zustand der Ausgänge der laufenden
    /// Ausgabe. Gibt die Zuordnung zurück, wenn „Übernehmen“ gedrückt wurde.
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        lines: &[String],
        on_air: bool,
    ) -> Option<(OutputMap, Pult)> {
        if !self.open {
            return None;
        }
        let mut applied = None;
        let mut open = true;
        let mut refresh = false;
        egui::Window::new("Ausgänge (Soundkarten)")
            .open(&mut open)
            .collapsible(false)
            .default_width(520.0)
            .show(ctx, |ui| {
                ui.label(
                    egui::RichText::new(
                        "Jede Quelle bekommt ein Gerät und ein Kanalpaar. Mehrere Soundkarten \
                         laufen gleichzeitig. Den Pegel fährst du am Pult. Die Playlist gibt \
                         den Takt vor.",
                    )
                    .small()
                    .color(theme::text_dim()),
                );
                ui.add_space(6.0);
                egui::Grid::new("outputs-grid")
                    .num_columns(3)
                    .spacing([12.0, 6.0])
                    .show(ui, |ui| {
                        for s in Source::ALL {
                            ui.label(s.label());
                            let mut route = self.draft.get(s);
                            let mut follow =
                                s != Source::Playlist && self.draft.explicit(s).is_none();
                            self.device_combo(ui, s, &mut route, &mut follow);
                            ui.add_enabled_ui(!follow, |ui| {
                                egui::ComboBox::from_id_salt(("pair", s))
                                    .selected_text(pair_label(route.pair))
                                    .show_ui(ui, |ui| {
                                        for p in 0..PAIRS {
                                            ui.selectable_value(&mut route.pair, p, pair_label(p));
                                        }
                                    });
                            });
                            ui.end_row();
                            if follow {
                                self.draft.clear(s);
                            } else {
                                self.draft.set(s, route);
                            }
                        }
                    });
                if let Some(e) = &self.list_error {
                    ui.colored_label(theme::on_air(), e);
                }
                ui.separator();
                self.pult_section(ui);
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Geräte neu lesen").clicked() {
                        refresh = true;
                    }
                    if ui.button("Übernehmen").clicked() {
                        applied = Some((self.draft.clone(), self.pult.clone()));
                    }
                });
                if on_air {
                    ui.label(
                        egui::RichText::new(
                            "Die Sendung läuft: Die neue Zuordnung gilt, wenn die Ausgabe das \
                             nächste Mal öffnet (Programm neu starten).",
                        )
                        .small()
                        .color(theme::text_dim()),
                    );
                }
                if !lines.is_empty() {
                    ui.separator();
                    for l in lines {
                        ui.label(egui::RichText::new(l).small());
                    }
                }
            });
        if refresh {
            self.refresh();
        }
        if !open || applied.is_some() {
            self.open = false;
        }
        applied
    }

    /// Pult-Modus (#301): ein Schalter und der Eingang vom Pult (Gerät, Kanalpaar).
    fn pult_section(&mut self, ui: &mut egui::Ui) {
        ui.checkbox(&mut self.pult.on, "Sendung kommt vom Pult");
        ui.label(
            egui::RichText::new(
                "An: Playlist, Carts und Decks gehen auf die Ausgänge oben (Richtung Pult). \
                 Stream und Mitschnitt bekommen nur den Eingang vom Pult, der Sendebus läuft \
                 darauf. Das Mikro hängt am Pult (WOAs Mikro ist aus). Vorhören braucht ein \
                 eigenes Gerät.",
            )
            .small()
            .color(theme::text_dim()),
        );
        ui.add_enabled_ui(self.pult.on, |ui| {
            ui.horizontal(|ui| {
                ui.label("Eingang vom Pult");
                let r = &mut self.pult.input;
                let shown = if r.device_id.is_none() {
                    "Kein Eingang".to_string()
                } else {
                    r.device_label()
                };
                egui::ComboBox::from_id_salt("pult-device")
                    .width(240.0)
                    .selected_text(shown)
                    .show_ui(ui, |ui| {
                        for d in &self.inputs {
                            let on = r.device_id.as_deref() == Some(d.id.as_str());
                            if ui.selectable_label(on, &d.name).clicked() {
                                r.device_id = Some(d.id.clone());
                                r.device_name = d.name.clone();
                            }
                        }
                    });
                egui::ComboBox::from_id_salt("pult-pair")
                    .selected_text(pair_label(r.pair))
                    .show_ui(ui, |ui| {
                        for p in 0..PAIRS {
                            ui.selectable_value(&mut r.pair, p, pair_label(p));
                        }
                    });
            });
        });
    }

    fn device_combo(&self, ui: &mut egui::Ui, s: Source, route: &mut Route, follow: &mut bool) {
        let known = route
            .device_id
            .as_ref()
            .is_none_or(|id| self.devices.iter().any(|d| d.id == *id));
        let shown = match (&route.device_id, known) {
            _ if *follow => follow_label(s).to_string(),
            (None, _) => "Standardgerät".to_string(),
            (Some(_), true) => route.device_label(),
            (Some(_), false) => format!("{} (fehlt)", route.device_label()),
        };
        egui::ComboBox::from_id_salt(("device", s))
            .width(280.0)
            .selected_text(shown)
            .show_ui(ui, |ui| {
                if s != Source::Playlist && ui.selectable_label(*follow, follow_label(s)).clicked()
                {
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
                for d in &self.devices {
                    let on = !*follow && route.device_id.as_deref() == Some(d.id.as_str());
                    if ui.selectable_label(on, &d.name).clicked() {
                        *follow = false;
                        route.device_id = Some(d.id.clone());
                        route.device_name = d.name.clone();
                    }
                }
            });
    }
}
