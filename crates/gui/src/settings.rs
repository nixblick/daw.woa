//! Einstellungen (ScreenSettings + SettingsFilter, Design Fassung 7): Bereiche links, Filter
//! oben findet jede Einstellung, Enter springt zum ersten Treffer und markiert ihn 2 s. Eine
//! Liste ([`REGISTRY`]) speist Filter und Anzeige; jede Zeile öffnet denselben Dialog wie das
//! Menü Einstellungen.

/// Bereich links.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Area {
    Audio,
    Sendung,
    Midi,
    Ki,
    Ablage,
    Datenschutz,
    Entwickler,
}

impl Area {
    pub const ALL: [Area; 7] = [
        Area::Audio,
        Area::Sendung,
        Area::Midi,
        Area::Ki,
        Area::Ablage,
        Area::Datenschutz,
        Area::Entwickler,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Area::Audio => "Audio",
            Area::Sendung => "Sendung",
            Area::Midi => "MIDI",
            Area::Ki => "KI",
            Area::Ablage => "Speicherorte",
            Area::Datenschutz => "Datenschutz",
            Area::Entwickler => "Entwickler",
        }
    }

    pub fn icon(self) -> &'static str {
        use egui_phosphor::regular as ic;
        match self {
            Area::Audio => ic::SPEAKER_HIGH,
            Area::Sendung => ic::BROADCAST,
            Area::Midi => ic::PIANO_KEYS,
            Area::Ki => ic::SPARKLE,
            Area::Ablage => ic::FOLDER,
            Area::Datenschutz => ic::SHIELD_CHECK,
            Area::Entwickler => ic::CODE,
        }
    }
}

/// Was eine Zeile öffnet (der Aufrufer führt es aus).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SettingId {
    Assistant,
    ReconnectOutput,
    Outputs,
    Pfl,
    Ki,
    Stream,
    Phone,
    Folders,
    Controls,
    MidiInput,
    Telemetry,
    Developer,
}

/// Eine Einstellung: Bereich, Name (wie im Menü), Beschreibung, Suchwörter.
#[derive(Debug, Clone, Copy)]
pub struct Entry {
    pub id: SettingId,
    pub area: Area,
    pub name: &'static str,
    pub desc: &'static str,
    pub keys: &'static str,
}

/// Alle Einstellungen. Neue Menüeinträge unter „Einstellungen“ gehören hier hinein (Test).
pub const REGISTRY: &[Entry] = &[
    Entry {
        id: SettingId::Outputs,
        area: Area::Audio,
        name: "Ausgänge (Soundkarten)",
        desc: "Welche Soundkarte Player, Sendeausgänge und Pult nutzen",
        keys: "ausgang gerät kopfhörer lautsprecher audio interface pult",
    },
    Entry {
        id: SettingId::ReconnectOutput,
        area: Area::Audio,
        name: "Ausgabe neu verbinden",
        desc: "Öffnet alle Ausgänge neu auf dem aktuellen Standardgerät, z. B. nach dem Einstecken der Kopfhörer",
        keys: "kopfhörer neu verbinden ton weg",
    },
    Entry {
        id: SettingId::Pfl,
        area: Area::Audio,
        name: "Vorhören (PFL)",
        desc: "Wohin das Vorhören geht",
        keys: "pfl cue vorhören kopfhörer",
    },
    Entry {
        id: SettingId::Phone,
        area: Area::Audio,
        name: "Telefon (Eingang für den Anrufer)",
        desc: "Eingang und Pegel für Anrufer in der Sendung",
        keys: "anruf telefon hybrid gast",
    },
    Entry {
        id: SettingId::Stream,
        area: Area::Sendung,
        name: "Sendung: Stream-Server und Kennwort",
        desc: "Shoutcast oder Icecast: Adresse, Port, Kennwort, Verbindung testen",
        keys: "stream shoutcast icecast server port senden",
    },
    Entry {
        id: SettingId::Controls,
        area: Area::Midi,
        name: "Controller und MIDI-Learn",
        desc: "Tasten und Regler eines Controllers zuordnen",
        keys: "controller learn fader tasten zuordnen",
    },
    Entry {
        id: SettingId::MidiInput,
        area: Area::Midi,
        name: "MIDI-Eingang",
        desc: "Gerät, das die scharf geschaltete MIDI-Spur spielt",
        keys: "keyboard klaviatur gerät midi",
    },
    Entry {
        id: SettingId::Ki,
        area: Area::Ki,
        name: "KI",
        desc: "Anbieter wählen: Claude über MCP oder ein Sprachmodell; Schlüssel nur im Schlüsselbund",
        keys: "ki ai schlüssel api key elevenlabs anbieter modell",
    },
    Entry {
        id: SettingId::Assistant,
        area: Area::Ki,
        name: "Assistent (Chat)",
        desc: "Chat mit einem Sprachmodell, das WOA bedienen kann",
        keys: "chat assistent sprachmodell",
    },
    Entry {
        id: SettingId::Folders,
        area: Area::Ablage,
        name: "Speicherorte",
        desc: "Wo WOA Projekte, Mitschnitte und Exporte ablegt",
        keys: "ordner pfad projekte mitschnitt export",
    },
    Entry {
        id: SettingId::Telemetry,
        area: Area::Datenschutz,
        name: "Fehlercodes anonym senden (ab nächstem Start)",
        desc: "Nur Fehlercodes, keine Inhalte; ab dem nächsten Start",
        keys: "telemetrie fehler anonym datenschutz",
    },
    Entry {
        id: SettingId::Developer,
        area: Area::Entwickler,
        name: "Entwickler-Modus",
        desc: "Für alle, die mitentwickeln: per Kennwort freischalten",
        keys: "entwickler kennwort abnahme fragen",
    },
];

/// Für die Suche: klein, Umlaute wie Grundbuchstabe (auch „ae“, „oe“, „ue“), ß wie ss.
pub fn fold(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars().flat_map(char::to_lowercase) {
        match c {
            'ä' => out.push('a'),
            'ö' => out.push('o'),
            'ü' => out.push('u'),
            'ß' => out.push_str("ss"),
            c => out.push(c),
        }
    }
    out.replace("ae", "a").replace("oe", "o").replace("ue", "u")
}

/// Treffer für `query` (ohne Groß/Klein, Umlaute wie Grundbuchstabe), leer = alle.
pub fn filter(query: &str) -> Vec<&'static Entry> {
    let q = fold(query.trim());
    REGISTRY
        .iter()
        .filter(|e| {
            q.is_empty()
                || [e.name, e.desc, e.keys, e.area.name()]
                    .iter()
                    .any(|t| fold(t).contains(&q))
        })
        .collect()
}

use eframe::egui;

use crate::theme;

/// Dauer der Markierung nach Enter (s).
pub const MARK_SECONDS: f64 = 2.0;

/// Fenster „Einstellungen“.
#[derive(Debug)]
pub struct SettingsWindow {
    pub open: bool,
    pub maximized: bool,
    pub area: Area,
    pub query: String,
    /// Markierte Zeile und Startzeit (egui-Zeit).
    pub marked: Option<(SettingId, f64)>,
}

impl Default for SettingsWindow {
    fn default() -> Self {
        Self {
            open: false,
            maximized: false,
            area: Area::Audio,
            query: String::new(),
            marked: None,
        }
    }
}

impl SettingsWindow {
    /// Enter im Filter: zum ersten Treffer springen (Bereich wechseln) und ihn markieren.
    pub fn jump(&mut self, now: f64) -> Option<SettingId> {
        let e = filter(&self.query).first().copied()?;
        self.area = e.area;
        self.marked = Some((e.id, now));
        Some(e.id)
    }

    /// Ist `id` gerade markiert?
    pub fn is_marked(&self, id: SettingId, now: f64) -> bool {
        self.marked
            .is_some_and(|(m, t0)| m == id && (0.0..MARK_SECONDS).contains(&(now - t0)))
    }

    /// Zeichnet das Fenster. `row` zeichnet das Bedienelement einer Einstellung (Aufrufer).
    pub fn show(&mut self, ctx: &egui::Context, mut row: impl FnMut(&mut egui::Ui, SettingId)) {
        if !self.open {
            return;
        }
        let mut open = true;
        let now = ctx.input(|i| i.time);
        let mut max = self.maximized;
        crate::fragen::fenster(ctx, "Einstellungen", max).show(ctx, |ui| {
            crate::fragen::kopf(
                ui,
                egui_phosphor::regular::GEAR_SIX,
                "Einstellungen",
                "Filter findet jede Einstellung · Enter springt hin",
                &mut open,
                &mut max,
            );
            let r = ui.add(
                egui::TextEdit::singleline(&mut self.query)
                    .hint_text(format!(
                        "{} Einstellung suchen",
                        egui_phosphor::regular::MAGNIFYING_GLASS
                    ))
                    .desired_width(f32::INFINITY)
                    .min_size(egui::vec2(0.0, 32.0)),
            );
            r.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, "Einstellung suchen")
            });
            if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                self.jump(now);
            }
            ui.add_space(theme::SPACE_2);
            let hits = filter(&self.query);
            let filtering = !self.query.trim().is_empty();
            ui.horizontal_top(|ui| {
                // Bereiche links, mit Trefferzahl beim Filtern.
                ui.vertical(|ui| {
                    ui.set_width(180.0);
                    for a in Area::ALL {
                        let n = hits.iter().filter(|e| e.area == a).count();
                        let text = if filtering {
                            format!("{} {}  ({n})", a.icon(), a.name())
                        } else {
                            format!("{} {}", a.icon(), a.name())
                        };
                        let b = egui::Button::selectable(self.area == a, text)
                            .min_size(egui::vec2(180.0, 30.0));
                        if ui.add_enabled(!filtering || n > 0, b).clicked() {
                            self.area = a;
                        }
                    }
                });
                ui.separator();
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    let shown: Vec<_> = if filtering {
                        hits.clone()
                    } else {
                        REGISTRY.iter().filter(|e| e.area == self.area).collect()
                    };
                    if shown.is_empty() {
                        ui.weak("Keine Einstellung gefunden");
                    }
                    for e in shown {
                        let marked = self.is_marked(e.id, now);
                        let fr = egui::Frame::new()
                            .fill(if marked {
                                theme::mix(theme::cue(), theme::panel(), 0.82)
                            } else {
                                theme::surface()
                            })
                            .stroke(egui::Stroke::new(
                                if marked { 1.5 } else { 1.0 },
                                if marked {
                                    theme::cue()
                                } else {
                                    theme::border()
                                },
                            ))
                            .corner_radius(theme::RADIUS_MD)
                            .inner_margin(egui::Margin::same(10))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.horizontal(|ui| {
                                    ui.vertical(|ui| {
                                        ui.label(
                                            egui::RichText::new(e.name)
                                                .strong()
                                                .color(theme::text()),
                                        );
                                        ui.label(
                                            egui::RichText::new(e.desc)
                                                .small()
                                                .color(theme::text_dim()),
                                        );
                                    });
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| row(ui, e.id),
                                    );
                                });
                            });
                        if marked {
                            fr.response.scroll_to_me(Some(egui::Align::Center));
                            ctx.request_repaint();
                        }
                        ui.add_space(theme::SPACE_1);
                    }
                });
            });
        });
        self.maximized = max;
        if !open {
            self.open = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(q: &str) -> Vec<&'static str> {
        filter(q).iter().map(|e| e.name).collect()
    }

    #[test]
    fn enter_jumps_to_the_first_hit_and_marks_it_two_seconds() {
        let mut w = SettingsWindow {
            query: "icecast".into(),
            ..Default::default()
        };
        assert_eq!(w.jump(10.0), Some(SettingId::Stream));
        assert_eq!(w.area, Area::Sendung);
        assert!(w.is_marked(SettingId::Stream, 11.9));
        assert!(!w.is_marked(SettingId::Stream, 12.0), "nach 2 s aus");
        w.query = "xyzzy".into();
        assert_eq!(w.jump(20.0), None);
    }

    #[test]
    fn empty_filter_lists_everything() {
        assert_eq!(filter("").len(), REGISTRY.len());
        assert!(REGISTRY.len() >= 12);
    }

    #[test]
    fn filter_finds_by_name_description_keyword_and_area() {
        assert!(names("kennwort").contains(&"Sendung: Stream-Server und Kennwort"));
        assert_eq!(names("icecast"), ["Sendung: Stream-Server und Kennwort"]);
        assert!(names("soundkarte").contains(&"Ausgänge (Soundkarten)"));
        assert!(
            names("KOPFHOERER").contains(&"Ausgabe neu verbinden"),
            "Umlaute egal"
        );
        assert!(names("midi").contains(&"Controller und MIDI-Learn"));
        assert!(names("datenschutz").contains(&"Fehlercodes anonym senden (ab nächstem Start)"));
        assert!(filter("xyzzy").is_empty());
    }

    #[test]
    fn every_settings_menu_entry_is_in_the_registry() {
        for (group, items) in crate::app::MENU_GROUPS {
            if !group.starts_with("Einstellungen") {
                continue;
            }
            for (label, _) in *items {
                assert!(
                    REGISTRY.iter().any(|e| e.name == *label),
                    "{group} › {label} fehlt in settings::REGISTRY"
                );
            }
        }
    }
}
