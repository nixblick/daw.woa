//! Party-Layout (Paket 35): große Now-Playing-Anzeige, nächster Titel, wenige große Tasten,
//! Wunschliste, Vollbild. Zustand und Befehle stehen in `woa_radio::party` (ohne Fenster
//! testbar); hier wird nur gezeichnet, und die Effekte gehen an die laufende Sendung.

use std::path::{Path, PathBuf};

use eframe::egui;
use woa_io::controller::Bus;
use woa_radio::party::{PartyCommand, PartyLayout, PartyState};

use crate::radio::RadioDesk;
use crate::theme;

/// Der Party-Modus.
#[derive(Default)]
pub struct PartyView {
    pub state: PartyState,
    /// Fernbedienung im WLAN (Paket 36).
    pub remote: crate::remote::RemoteView,
}

/// Alle lesbaren Audiodateien in `dir` (bis `depth` Ebenen tief), nach Pfad sortiert.
pub fn audio_files_in(dir: &Path, depth: u32) -> Vec<PathBuf> {
    let mut out = Vec::new();
    collect(dir, depth, &mut out);
    out.sort();
    out
}

fn collect(dir: &Path, depth: u32, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        tracing::warn!(dir = %dir.display(), "party: folder not readable");
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            if depth > 0 {
                collect(&p, depth - 1, out);
            }
        } else if woa_engine::decode::is_readable(&p) {
            out.push(p);
        }
    }
}

impl PartyView {
    /// Tasten im Party-Modus (nur, wenn kein Textfeld den Fokus hat).
    pub fn shortcuts(ctx: &egui::Context) -> Vec<PartyCommand> {
        if ctx.egui_wants_keyboard_input() {
            return Vec::new();
        }
        let n = egui::Modifiers::NONE;
        ctx.input_mut(|i| {
            let mut v = Vec::new();
            if i.consume_key(n, egui::Key::Space) {
                v.push(PartyCommand::PlayPause);
            }
            if i.consume_key(n, egui::Key::N) || i.consume_key(n, egui::Key::ArrowRight) {
                v.push(PartyCommand::Next);
            }
            if i.consume_key(n, egui::Key::P) || i.consume_key(n, egui::Key::ArrowLeft) {
                v.push(PartyCommand::Previous);
            }
            if i.consume_key(n, egui::Key::M) {
                v.push(PartyCommand::Mute);
            }
            if i.consume_key(n, egui::Key::R) {
                v.push(PartyCommand::CycleRepeat);
            }
            if i.consume_key(n, egui::Key::S) {
                v.push(PartyCommand::ToggleShuffle);
            }
            if i.consume_key(n, egui::Key::ArrowUp) {
                v.push(PartyCommand::VolumeUp);
            }
            if i.consume_key(n, egui::Key::ArrowDown) {
                v.push(PartyCommand::VolumeDown);
            }
            if i.consume_key(n, egui::Key::F) || i.consume_key(n, egui::Key::F11) {
                v.push(PartyCommand::ToggleFullscreen);
            }
            v
        })
    }

    /// Führt Befehle aus und reicht die Effekte an die Sendung bzw. das Fenster.
    pub fn run(&mut self, cmds: Vec<PartyCommand>, desk: &mut RadioDesk, ctx: &egui::Context) {
        for cmd in cmds {
            for e in self.state.apply(cmd, desk.is_on_air()) {
                if let woa_radio::party::Effect::Fullscreen(on) = e {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(on));
                } else {
                    desk.party_effect(e);
                }
            }
        }
    }

    /// Holt Befehle der Handys ab, führt sie über dieselbe Schicht wie die Tasten aus und
    /// veröffentlicht den Zustand. Jeden Frame aufrufen; ohne Fernbedienung kostet das nichts.
    pub fn pump_remote(&mut self, desk: &mut RadioDesk, ctx: &egui::Context) {
        if !self.remote.is_on() {
            return;
        }
        self.state.level_db = desk.level_db(Bus::Music);
        let cmds = self.remote.pump(&self.state, desk, ctx);
        if !cmds.is_empty() {
            self.run(cmds, desk, ctx);
        }
    }

    /// Zeichnet das ganze Layout; gibt die gedrückten Befehle zurück.
    pub fn ui(&mut self, ui: &mut egui::Ui, desk: &mut RadioDesk) -> Vec<PartyCommand> {
        self.state.level_db = desk.level_db(Bus::Music);
        let layout = woa_radio::party::layout(desk.view(), &self.state);
        let mut cmds = Vec::new();
        let full = ui.available_height();
        let mut folder = false;
        ui.columns(2, |cols| {
            // Links: jetzt, nächster, Tasten.
            let ui = &mut cols[0];
            now_card(ui, &layout, full);
            ui.add_space(10.0);
            folder = buttons_row(ui, &layout, &mut cmds);
            // Rechts: Wunschliste.
            let ui = &mut cols[1];
            self.wish_card(ui, &mut cmds);
            ui.add_space(10.0);
            self.remote.ui(ui);
        });
        if folder {
            Self::load_folder(desk);
        }
        cmds
    }

    fn wish_card(&mut self, ui: &mut egui::Ui, cmds: &mut Vec<PartyCommand>) {
        theme::card_frame().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("WUNSCHLISTE")
                        .size(11.5)
                        .strong()
                        .color(theme::text_dim()),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("+ Wunsch aus Datei").clicked()
                        && let Some(files) = rfd::FileDialog::new()
                            .add_filter(
                                woa_engine::decode::FILTER_NAME,
                                &woa_engine::decode::dialog_extensions(),
                            )
                            .pick_files()
                    {
                        for path in files {
                            let title = woa_radio::show::title_of(&path);
                            cmds.push(PartyCommand::Wish { path, title });
                        }
                    }
                });
            });
            ui.add_space(6.0);
            let items: Vec<_> = self.state.wishes.items().to_vec();
            if items.is_empty() {
                ui.label(
                    egui::RichText::new("Noch keine Wünsche.")
                        .size(18.0)
                        .color(theme::text_dim()),
                );
            }
            for (i, w) in items.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(format!("{}.", i + 1))
                            .size(20.0)
                            .monospace()
                            .color(theme::accent()),
                    );
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(&w.title)
                                .size(20.0)
                                .color(theme::text()),
                        )
                        .truncate(),
                    );
                    if w.votes > 1 {
                        theme::status_pill(ui, &format!("×{}", w.votes), theme::cue(), true);
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .button(theme::ic::X)
                            .on_hover_text("Wunsch verwerfen")
                            .clicked()
                        {
                            cmds.push(PartyCommand::DropWish(w.path.clone()));
                        }
                        if ui
                            .button("Als Nächstes")
                            .on_hover_text("Hinter dem laufenden Titel einreihen")
                            .clicked()
                        {
                            cmds.push(PartyCommand::PlayWish(w.path.clone()));
                        }
                    });
                });
            }
        });
    }

    /// Musikordner laden (Dialog); hängt alle Titel hinten an die Playlist.
    pub fn load_folder(desk: &RadioDesk) {
        if let Some(dir) = rfd::FileDialog::new().pick_folder() {
            let files = audio_files_in(&dir, 3);
            if files.is_empty() {
                tracing::info!(dir = %dir.display(), "party: no audio files in folder");
            } else {
                desk.append_files(files);
            }
        }
    }
}

fn now_card(ui: &mut egui::Ui, l: &PartyLayout, _height: f32) {
    theme::lcd_show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.set_min_height(260.0);
        let (badge, col) = match &l.now {
            Some(n) if n.playing => ("JETZT", theme::accent()),
            Some(n) if n.paused => ("PAUSE", theme::on_air()),
            _ => ("BEREIT", theme::cue()),
        };
        ui.label(
            egui::RichText::new(badge)
                .size(13.0)
                .strong()
                .monospace()
                .color(col),
        );
        match &l.now {
            Some(n) => {
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(&n.title)
                            .size(40.0)
                            .strong()
                            .color(theme::text()),
                    )
                    .truncate(),
                );
                if !n.artist.is_empty() {
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(&n.artist)
                                .size(26.0)
                                .color(theme::text_dim()),
                        )
                        .truncate(),
                    );
                }
                ui.add_space(6.0);
                let urgent = n.playing && n.remaining.len() <= 4 && n.remaining.starts_with("0:0");
                ui.label(
                    egui::RichText::new(&n.remaining)
                        .size(84.0)
                        .strong()
                        .monospace()
                        .color(if urgent {
                            theme::on_air()
                        } else {
                            theme::accent()
                        }),
                );
                ui.add(
                    egui::ProgressBar::new(n.progress)
                        .desired_height(12.0)
                        .fill(theme::accent()),
                );
            }
            None => {
                ui.label(
                    egui::RichText::new("Noch kein Titel – Ordner laden")
                        .size(28.0)
                        .color(theme::text_dim()),
                );
            }
        }
        ui.add_space(8.0);
        ui.label(
            egui::RichText::new(format!(
                "NÄCHSTER: {}",
                l.next
                    .as_deref()
                    .unwrap_or("– nichts mehr in der Playlist –")
            ))
            .size(20.0)
            .color(theme::cue()),
        );
    });
}

/// Zeichnet die Tasten; `true`, wenn „Ordner laden“ gedrückt wurde.
fn buttons_row(ui: &mut egui::Ui, l: &PartyLayout, cmds: &mut Vec<PartyCommand>) -> bool {
    let mut folder = false;
    let w = (ui.available_width() - 3.0 * 8.0) / 4.0;
    for (n, row) in l.buttons.iter().enumerate() {
        // Die erste Reihe (Transport) ist groß, die anderen etwas flacher.
        let (h, size) = if n == 0 { (90.0, 28.0) } else { (56.0, 20.0) };
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            for b in row {
                let primary = matches!(b.command, PartyCommand::PlayPause);
                let lit = primary || b.active;
                let btn = egui::Button::new(
                    egui::RichText::new(b.label)
                        .size(size)
                        .strong()
                        .color(if lit { theme::well() } else { theme::text() }),
                )
                .fill(if primary {
                    theme::ok()
                } else if b.active {
                    theme::accent()
                } else {
                    theme::surface()
                })
                .min_size(egui::vec2(w, h));
                if ui.add(btn).clicked() {
                    cmds.push(b.command.clone());
                }
            }
        });
        ui.add_space(4.0);
    }
    ui.add_space(2.0);
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(format!("Lautstärke {:+.0} dB", l.level_db))
                .size(16.0)
                .color(theme::text_dim()),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let t = if l.fullscreen {
                "Vollbild aus"
            } else {
                "Vollbild an"
            };
            if ui.button(t).on_hover_text("Taste F oder F11").clicked() {
                cmds.push(PartyCommand::ToggleFullscreen);
            }
            if ui.button("Ordner laden …").clicked() {
                folder = true;
            }
        });
    });
    folder
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_scan_finds_audio_sorted_and_depth_limited() {
        let tmp = tempfile::tempdir().unwrap();
        let d = tmp.path();
        std::fs::write(d.join("b.mp3"), b"x").unwrap();
        std::fs::write(d.join("a.WAV"), b"x").unwrap();
        std::fs::write(d.join("notes.txt"), b"x").unwrap();
        std::fs::create_dir_all(d.join("s/t")).unwrap();
        std::fs::write(d.join("s/c.flac"), b"x").unwrap();
        std::fs::write(d.join("s/t/d.ogg"), b"x").unwrap();
        let names = |depth| -> Vec<String> {
            audio_files_in(d, depth)
                .iter()
                .map(|p| {
                    p.strip_prefix(d)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/")
                })
                .collect()
        };
        assert_eq!(names(0), ["a.WAV", "b.mp3"]);
        assert_eq!(names(1), ["a.WAV", "b.mp3", "s/c.flac"]);
        assert_eq!(names(2).len(), 4);
        assert!(audio_files_in(&d.join("fehlt"), 3).is_empty());
    }
}
