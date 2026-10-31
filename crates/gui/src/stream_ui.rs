//! Fenster „Sendung: Stream-Server“: Servertyp, Adresse, Port, Kennwort, Format und
//! „Verbindung testen“. Das Kennwort kommt in den Schlüsselbund des Systems, nie in die
//! `config.json` ([`woa_stream::kennwort`]).

use std::path::Path;
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use eframe::egui;
use woa_stream::kennwort::{self, Ort};
use woa_stream::probe::{self, Ausgang, Befund};
use woa_stream::{Kennwort, Protocol, StreamConfig};

use crate::theme;

const BITRATES: [u32; 7] = [64, 96, 128, 160, 192, 256, 320];
const RATES: [u32; 2] = [44_100, 48_000];

/// Was „Speichern“ liefert.
#[derive(Debug, Clone, PartialEq)]
pub struct Applied {
    pub config: StreamConfig,
    /// Neues Kennwort; `None` = unverändert.
    pub neues_kennwort: Option<String>,
}

#[derive(Default)]
pub struct StreamUi {
    open: bool,
    draft: StreamConfig,
    /// Inhalt des Feldes „Server“ (kann Port und Pfad enthalten, bis er übernommen wird).
    server: String,
    kw_text: String,
    /// Der Nutzer hat im Kennwortfeld getippt.
    kw_dirty: bool,
    kw_show: bool,
    stored: Option<Ort>,
    admin_on: bool,
    testing: Option<Receiver<Befund>>,
    result: Option<Befund>,
    note: Option<String>,
}

impl StreamUi {
    /// Öffnet das Fenster mit dem gespeicherten Stand.
    pub fn open(&mut self, saved: &StreamConfig, data_dir: &Path) {
        self.open = true;
        self.draft = saved.clone();
        self.server = saved.host.clone();
        self.kw_text.clear();
        self.kw_dirty = false;
        self.kw_show = false;
        self.stored = kennwort::stored(data_dir);
        self.admin_on = saved.admin_port.is_some();
        self.testing = None;
        self.result = None;
        self.note = None;
    }

    fn close(&mut self) {
        self.open = false;
        // Das Kennwort bleibt nicht im Fensterzustand liegen.
        self.kw_text.clear();
        self.kw_dirty = false;
        self.kw_show = false;
    }

    /// Übernimmt das Feld „Server“ in den Entwurf.
    fn take_server(&mut self) {
        self.draft.set_server(&self.server);
        self.server = self.draft.host.clone();
        if !self.admin_on {
            self.draft.admin_port = None;
        }
    }

    fn start_test(&mut self, ctx: &egui::Context, data_dir: &Path) {
        self.take_server();
        let wert = if self.kw_dirty {
            Some(self.kw_text.clone())
        } else {
            kennwort::reveal(data_dir)
        };
        let cfg = self.draft.clone();
        let (tx, rx) = channel();
        let ctx = ctx.clone();
        self.result = None;
        self.testing = Some(rx);
        std::thread::spawn(move || {
            let befund = match wert {
                Some(w) if !w.is_empty() => probe::run(&cfg, &Kennwort::from_value(w)),
                _ => Befund {
                    ausgang: Ausgang::Eingabe,
                    text: "Trage zuerst das Kennwort ein (das Quell-Kennwort des Servers).".into(),
                    millis: 0,
                },
            };
            let _ = tx.send(befund);
            ctx.request_repaint();
        });
    }

    /// Zeichnet das Fenster. `streaming`: die Sendung läuft gerade. Gibt den Stand zurück,
    /// wenn „Speichern“ gedrückt wurde und die Werte gültig sind.
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        data_dir: &Path,
        streaming: bool,
    ) -> Option<Applied> {
        if !self.open {
            return None;
        }
        if let Some(rx) = &self.testing {
            match rx.try_recv() {
                Ok(b) => {
                    self.result = Some(b);
                    self.testing = None;
                }
                Err(TryRecvError::Disconnected) => self.testing = None,
                Err(TryRecvError::Empty) => ctx.request_repaint(),
            }
        }
        let mut applied = None;
        let mut open = true;
        let mut test = false;
        let mut save = false;
        egui::Window::new("Sendung: Stream-Server")
            .open(&mut open)
            .collapsible(false)
            .default_width(480.0)
            .show(ctx, |ui| {
                ui.label(
                    egui::RichText::new(
                        "Hierhin sendet WOA. Es geht mit jedem Shoutcast- oder Icecast-Server; \
                         die Angaben bekommst du vom Betreiber deines Servers.",
                    )
                    .small()
                    .color(theme::text_dim()),
                );
                ui.add_space(6.0);
                egui::Grid::new("stream-grid")
                    .num_columns(2)
                    .spacing([12.0, 8.0])
                    .show(ui, |ui| self.fields(ui, data_dir));
                ui.add_space(4.0);
                egui::CollapsingHeader::new("Erweitert").show(ui, |ui| self.advanced(ui));
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    let busy = self.testing.is_some();
                    let t = ui.add_enabled(
                        !busy && !streaming,
                        egui::Button::new(if busy {
                            "Teste …"
                        } else {
                            "Verbindung testen"
                        }),
                    );
                    if streaming {
                        t.on_disabled_hover_text(
                            "Während der Sendung nicht möglich: ein Test könnte die laufende \
                             Quelle stören",
                        );
                    } else if t.clicked() {
                        test = true;
                    }
                    if ui.button("Speichern").clicked() {
                        save = true;
                    }
                });
                if let Some(b) = &self.result {
                    ui.add_space(6.0);
                    let (mark, color) = if b.ausgang.gut() {
                        ("OK", theme::ok())
                    } else {
                        ("Fehler", theme::on_air())
                    };
                    ui.horizontal_wrapped(|ui| {
                        ui.label(egui::RichText::new(mark).strong().color(color));
                        ui.label(&b.text);
                    });
                }
                if let Some(n) = &self.note {
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new(n).small().color(theme::warn()));
                }
                if streaming {
                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new(
                            "Die Sendung läuft. Neue Angaben gelten ab der nächsten Sendung \
                             (Sendung beenden, dann wieder „Auf Sendung“).",
                        )
                        .small()
                        .color(theme::text_dim()),
                    );
                }
            });
        if test {
            self.start_test(ctx, data_dir);
        }
        if save {
            self.take_server();
            match self.draft.validate() {
                Ok(()) => {
                    applied = Some(Applied {
                        config: self.draft.clone(),
                        neues_kennwort: self.kw_dirty.then(|| self.kw_text.clone()),
                    });
                }
                Err(e) => self.note = Some(e),
            }
        }
        if !open || applied.is_some() {
            self.close();
        }
        applied
    }

    fn fields(&mut self, ui: &mut egui::Ui, data_dir: &Path) {
        ui.label("Servertyp");
        egui::ComboBox::from_id_salt("stream-protocol")
            .selected_text(self.draft.protocol.label())
            .show_ui(ui, |ui| {
                for p in Protocol::ALL {
                    if ui
                        .selectable_value(&mut self.draft.protocol, p, p.label())
                        .clicked()
                    {
                        self.result = None;
                    }
                }
            });
        ui.end_row();

        ui.label("Server");
        let r = ui
            .add(
                egui::TextEdit::singleline(&mut self.server)
                    .hint_text("Name oder IP-Adresse, z. B. radio.example")
                    .desired_width(260.0),
            )
            .on_hover_text(
                "Name oder IP-Adresse. Eine ganze Adresse wie http://1.2.3.4:8000/live \
                 wird in Server, Port und Mountpoint zerlegt.",
            );
        if r.lost_focus() {
            self.take_server();
        }
        ui.end_row();

        ui.label("Port");
        ui.horizontal(|ui| {
            ui.add(egui::DragValue::new(&mut self.draft.port).range(1..=65_534));
            if self.draft.protocol.is_shoutcast() {
                ui.label(
                    egui::RichText::new(format!(
                        "Port des Servers; WOA meldet sich auf {} an",
                        self.draft.source_port()
                    ))
                    .small()
                    .color(theme::text_dim()),
                );
            }
        });
        ui.end_row();

        match self.draft.protocol {
            Protocol::Icecast => {
                ui.label("Mountpoint");
                ui.add(
                    egui::TextEdit::singleline(&mut self.draft.mount)
                        .hint_text("/stream")
                        .desired_width(160.0),
                );
                ui.end_row();
                ui.label("Benutzer");
                ui.add(
                    egui::TextEdit::singleline(&mut self.draft.user)
                        .hint_text("source")
                        .desired_width(160.0),
                );
                ui.end_row();
            }
            Protocol::ShoutcastV2 => {
                ui.label("Stream-ID");
                ui.add(egui::DragValue::new(&mut self.draft.stream_id).range(1..=99));
                ui.end_row();
            }
            Protocol::Shoutcast => {}
        }

        ui.label("Kennwort");
        ui.horizontal(|ui| {
            let hint = match self.stored {
                Some(_) if !self.kw_dirty => "gespeichert (leer lassen = behalten)",
                _ => "Quell-Kennwort des Servers",
            };
            let masked = !self.kw_show;
            let r = ui.add(
                egui::TextEdit::singleline(&mut self.kw_text)
                    .password(masked)
                    .hint_text(hint)
                    .desired_width(200.0),
            );
            if r.changed() {
                self.kw_dirty = true;
            }
            let label = if self.kw_show {
                "Verbergen"
            } else {
                "Anzeigen"
            };
            if ui.button(label).clicked() {
                self.kw_show = !self.kw_show;
                // Beim Anzeigen das gespeicherte Kennwort holen, solange nichts getippt ist.
                if self.kw_show
                    && !self.kw_dirty
                    && let Some(w) = kennwort::reveal(data_dir)
                {
                    self.kw_text = w;
                }
                if !self.kw_show && !self.kw_dirty {
                    self.kw_text.clear();
                }
            }
        });
        ui.end_row();
        if let Some(ort) = self.stored {
            ui.label("");
            ui.label(
                egui::RichText::new(format!("Kennwort liegt {}", ort.label()))
                    .small()
                    .color(theme::text_dim()),
            );
            ui.end_row();
        }

        ui.label("Sendername");
        ui.add(egui::TextEdit::singleline(&mut self.draft.name).desired_width(260.0));
        ui.end_row();

        ui.label("Format");
        ui.horizontal(|ui| {
            ui.label("MP3");
            egui::ComboBox::from_id_salt("stream-bitrate")
                .selected_text(format!("{} kbit/s", self.draft.bitrate_kbps))
                .show_ui(ui, |ui| {
                    for b in BITRATES {
                        ui.selectable_value(&mut self.draft.bitrate_kbps, b, format!("{b} kbit/s"));
                    }
                });
            egui::ComboBox::from_id_salt("stream-rate")
                .selected_text(format!("{} Hz", self.draft.sample_rate))
                .show_ui(ui, |ui| {
                    for r in RATES {
                        ui.selectable_value(&mut self.draft.sample_rate, r, format!("{r} Hz"));
                    }
                });
        });
        ui.end_row();
    }

    fn advanced(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.admin_on, "Admin-Port").on_hover_text(
                "Für die Hörerzahl in der Sendeanzeige. Ohne Admin-Port sendet WOA trotzdem.",
            );
            if self.admin_on {
                let mut p = self.draft.admin_port.unwrap_or(self.draft.port);
                ui.add(egui::DragValue::new(&mut p).range(1..=65_535));
                self.draft.admin_port = Some(p);
            }
        });
        ui.label(
            egui::RichText::new(format!("Ziel: {}", self.draft.source_url()))
                .small()
                .color(theme::text_dim()),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_draws_for_every_server_type_without_a_display() {
        let tmp = tempfile::tempdir().unwrap();
        let ctx = crate::theme::test_context();
        for p in Protocol::ALL {
            let mut ui = StreamUi::default();
            ui.open(
                &StreamConfig {
                    protocol: p,
                    ..StreamConfig::default()
                },
                tmp.path(),
            );
            let _ = ctx.run_ui(egui::RawInput::default(), |u| {
                assert!(ui.show(u.ctx(), tmp.path(), false).is_none());
            });
            assert!(ui.open);
        }
    }

    #[test]
    fn server_field_is_split_and_closing_forgets_the_typed_secret() {
        let tmp = tempfile::tempdir().unwrap();
        let mut ui = StreamUi::default();
        ui.open(&StreamConfig::default(), tmp.path());
        ui.draft.protocol = Protocol::Icecast;
        ui.server = "http://10.1.2.3:8010/live".into();
        ui.take_server();
        assert_eq!(
            (ui.server.as_str(), ui.draft.port, ui.draft.mount.as_str()),
            ("10.1.2.3", 8010, "/live")
        );
        ui.kw_text = "tippfehler".into();
        ui.kw_dirty = true;
        ui.close();
        assert!(ui.kw_text.is_empty() && !ui.kw_dirty);
    }
}
