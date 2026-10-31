//! Fenster „Verwalten“ des Radio-Studios: Reiter **Playlisten** (links die Listen, rechts die
//! Titel der gewählten) und **Titel** (Rahmen für den Tag-Editor, [`crate::titel_tab`]).
//!
//! Das Fenster rechnet nichts selbst: Es liefert [`Action`]s, die [`crate::radio::RadioDesk`]
//! an den Controller schickt (`woa_radio::playlists`). So bleibt die Regel an einer Stelle:
//! Die laufende Playlist lässt sich auf Sendung nicht löschen, ein Wechsel lässt den
//! laufenden Titel und die Decks weiterspielen.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use eframe::egui;

use woa_radio::RadioShow;
use woa_radio::playlists::Deleted;

use crate::theme;

/// Wie lange der Hinweis „gelöscht · Rückgängig“ steht (wie beim Löschen einer Spur).
pub const UNDO_SECS: f64 = 8.0;

/// Ein Auftrag aus Studio-Kopf oder Fenster.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Select(usize),
    New,
    Rename(usize, String),
    Duplicate(usize),
    Clear(usize),
    Delete(usize),
    Undo,
    Import(PathBuf),
    Export(usize, PathBuf),
    Add(usize, Vec<PathBuf>),
    Remove(usize, usize),
    Move(usize, usize, usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tab {
    #[default]
    Playlists,
    Titles,
}

/// Zuletzt gelöschte Playlist, solange der Hinweis steht.
pub struct UndoSlot {
    pub deleted: Deleted,
    /// egui-Zeit, bis zu der der Hinweis steht (wird beim ersten Zeichnen gesetzt).
    pub until: Option<f64>,
}

/// Zustand von Fenster und Studio-Kopf.
#[derive(Default)]
pub struct ManageWindow {
    pub open: bool,
    pub tab: Tab,
    /// Im Fenster gewählte Playlist (nicht zwingend die aktive).
    pub selected: usize,
    /// Markierter Titel rechts (Entf nimmt ihn heraus).
    pub marked: Option<usize>,
    /// Umbenennen läuft: Index, Text und ob der Fokus schon gesetzt ist.
    pub renaming: Option<(usize, String, bool)>,
    /// Wechsel während der Sendung wartet auf Bestätigung.
    pub confirm_switch: Option<usize>,
    /// Vom Controller-Thread gefüllt, wenn eine Playlist gelöscht wurde.
    pub undo: Arc<Mutex<Option<UndoSlot>>>,
    /// Zuletzt gesehene aktive Playlist (zum Nachmessen nach einem Wechsel).
    pub seen_active: Option<usize>,
}

impl ManageWindow {
    /// Wählt eine Playlist als aktive; während der Sendung erst nach Rückfrage.
    pub fn request_select(&mut self, i: usize, on_air: bool, out: &mut Vec<Action>) {
        if on_air {
            self.confirm_switch = Some(i);
        } else {
            out.push(Action::Select(i));
        }
    }

    /// Schlanke Auswahl im Kopf der Sende-Playlist: Liste der Playlisten und „⋯“.
    pub fn header(
        &mut self,
        ui: &mut egui::Ui,
        names: &[String],
        active: usize,
        on_air: bool,
    ) -> Vec<Action> {
        let mut out = Vec::new();
        let current = names.get(active).cloned().unwrap_or_default();
        egui::ComboBox::from_id_salt("radio_playlist_pick")
            .selected_text(egui::RichText::new(current).strong())
            .width(170.0)
            .show_ui(ui, |ui| {
                for (i, n) in names.iter().enumerate() {
                    if ui.selectable_label(i == active, n).clicked() && i != active {
                        self.request_select(i, on_air, &mut out);
                    }
                }
            })
            .response
            .on_hover_text("Aktive Playlist wählen");
        ui.menu_button(theme::ic::DOTS_THREE, |ui| {
            if ui.button("Neue Playlist").clicked() {
                out.push(Action::New);
                ui.close();
            }
            if ui.button("Playlisten verwalten …").clicked() {
                self.open = true;
                self.tab = Tab::Playlists;
                self.selected = active;
                ui.close();
            }
        })
        .response
        .on_hover_text("Playlisten");
        out
    }

    /// Rückfrage beim Wechsel während der Sendung.
    pub fn confirm_ui(&mut self, ctx: &egui::Context, names: &[String]) -> Option<Action> {
        let i = self.confirm_switch?;
        let name = names.get(i).cloned().unwrap_or_default();
        let mut act = None;
        let mut keep = true;
        egui::Window::new("Playlist wechseln?")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label(format!(
                    "Die Sendung läuft. Der laufende Titel und die Decks spielen weiter, danach geht es mit „{name}“ weiter."
                ));
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.button(egui::RichText::new("Wechseln").strong()).clicked() {
                        act = Some(Action::Select(i));
                        keep = false;
                    }
                    if ui.button("Abbrechen").clicked()
                        || ui.input(|i| i.key_pressed(egui::Key::Escape))
                    {
                        keep = false;
                    }
                });
            });
        if !keep {
            self.confirm_switch = None;
        }
        act
    }

    /// Das Fenster. `show` ist der aktuelle Stand, `titles` zeichnet den Reiter „Titel“
    /// und gibt Dateien zurück, die in die gewählte Playlist sollen.
    pub fn window(
        &mut self,
        ctx: &egui::Context,
        show: &RadioShow,
        on_air: bool,
        titles: impl FnOnce(&mut egui::Ui) -> Vec<PathBuf>,
    ) -> Vec<Action> {
        let mut out = Vec::new();
        if !self.open {
            return out;
        }
        let n = show.playlists.len();
        if self.selected >= n {
            self.selected = show.active.min(n.saturating_sub(1));
        }
        let mut open = true;
        egui::Window::new("Verwalten")
            .open(&mut open)
            .default_size([760.0, 460.0])
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.tab, Tab::Playlists, "Playlisten");
                    ui.selectable_value(&mut self.tab, Tab::Titles, "Titel");
                });
                ui.separator();
                match self.tab {
                    Tab::Playlists => self.playlists_tab(ui, show, on_air, &mut out),
                    Tab::Titles => {
                        let files = titles(ui);
                        if !files.is_empty() {
                            out.push(Action::Add(self.selected, files));
                        }
                    }
                }
            });
        self.open = open;
        out
    }

    fn playlists_tab(
        &mut self,
        ui: &mut egui::Ui,
        show: &RadioShow,
        on_air: bool,
        out: &mut Vec<Action>,
    ) {
        let sel = self.selected;
        ui.columns(2, |cols| {
            // ---- links: die Playlisten ----
            let ui = &mut cols[0];
            ui.horizontal(|ui| {
                if ui.button("+ Neu").clicked() {
                    out.push(Action::New);
                }
                if ui.button("M3U importieren …").clicked()
                    && let Some(p) = rfd::FileDialog::new()
                        .add_filter("M3U", &["m3u", "m3u8"])
                        .pick_file()
                {
                    out.push(Action::Import(p));
                }
            });
            ui.add_space(4.0);
            egui::ScrollArea::vertical()
                .id_salt("pl_list")
                .show(ui, |ui| {
                    for (i, p) in show.playlists.iter().enumerate() {
                        let count = show.playlist_entries(i).map_or(0, <[_]>::len);
                        if let Some((k, text, focused)) = &mut self.renaming
                            && *k == i
                        {
                            let r = ui.text_edit_singleline(text);
                            // Fokus nur beim Öffnen, sonst kommt man nicht mehr heraus.
                            if !*focused {
                                r.request_focus();
                                *focused = true;
                            }
                            if r.lost_focus() || r.clicked_elsewhere() {
                                if ui.input(|i| !i.key_pressed(egui::Key::Escape)) {
                                    out.push(Action::Rename(i, text.clone()));
                                }
                                self.renaming = None;
                            }
                            continue;
                        }
                        let live = i == show.active;
                        let label = format!(
                            "{}{}  ({count})",
                            p.name,
                            if live { "  · aktiv" } else { "" }
                        );
                        let r = ui.selectable_label(i == sel, label);
                        if r.clicked() {
                            self.selected = i;
                            self.marked = None;
                        }
                        if r.double_clicked() {
                            self.renaming = Some((i, p.name.clone(), false));
                        }
                        r.context_menu(|ui| self.playlist_menu(ui, i, show, on_air, out));
                    }
                });

            // ---- rechts: die Titel der gewählten ----
            let ui = &mut cols[1];
            let name = show.playlists.get(sel).map_or("", |p| p.name.as_str());
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(name).strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.menu_button(theme::ic::DOTS_THREE, |ui| {
                        self.playlist_menu(ui, sel, show, on_air, out)
                    });
                    if ui.button("+ Titel …").clicked()
                        && let Some(files) = rfd::FileDialog::new()
                            .add_filter(
                                woa_engine::decode::FILTER_NAME,
                                &woa_engine::decode::dialog_extensions(),
                            )
                            .pick_files()
                    {
                        out.push(Action::Add(sel, files));
                    }
                });
            });
            ui.add_space(4.0);
            let entries = show.playlist_entries(sel).unwrap_or(&[]);
            if entries.is_empty() {
                ui.label(
                    egui::RichText::new(
                        "Leer. „+ Titel …“ oder im Reiter „Titel“ aus der Bibliothek hinzufügen.",
                    )
                    .color(theme::text_dim()),
                );
            }
            egui::ScrollArea::vertical()
                .id_salt("pl_titles")
                .show(ui, |ui| {
                    let last = entries.len().saturating_sub(1);
                    for (k, e) in entries.iter().enumerate() {
                        ui.horizontal(|ui| {
                            let r = ui.selectable_label(
                                self.marked == Some(k),
                                format!("{:>2}. {}", k + 1, e.title()),
                            );
                            if r.clicked() {
                                self.marked = Some(k);
                            }
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui
                                        .small_button(theme::ic::X)
                                        .on_hover_text("Aus der Playlist nehmen (Entf)")
                                        .clicked()
                                    {
                                        out.push(Action::Remove(sel, k));
                                    }
                                    if ui
                                        .add_enabled(k < last, egui::Button::new("↓").small())
                                        .clicked()
                                    {
                                        out.push(Action::Move(sel, k, k + 1));
                                    }
                                    if ui
                                        .add_enabled(k > 0, egui::Button::new("↑").small())
                                        .clicked()
                                    {
                                        out.push(Action::Move(sel, k, k - 1));
                                    }
                                },
                            );
                        });
                    }
                });
            // Entf nimmt nur den markierten Titel heraus, nie eine ganze Playlist.
            if let Some(k) = self.marked
                && k < entries.len()
                && !ui.ctx().egui_wants_keyboard_input()
                && ui.input(|i| i.key_pressed(egui::Key::Delete))
            {
                out.push(Action::Remove(sel, k));
                self.marked = None;
            }
        });
    }

    /// Aktionen einer Playlist (Kontextmenü und „⋯“). Löschen zuletzt und in Rot.
    fn playlist_menu(
        &mut self,
        ui: &mut egui::Ui,
        i: usize,
        show: &RadioShow,
        on_air: bool,
        out: &mut Vec<Action>,
    ) {
        let live = i == show.active;
        if ui
            .add_enabled(!live, egui::Button::new("Aktiv setzen"))
            .clicked()
        {
            self.request_select(i, on_air, out);
            ui.close();
        }
        if ui.button("Umbenennen").clicked() {
            let name = show
                .playlists
                .get(i)
                .map(|p| p.name.clone())
                .unwrap_or_default();
            self.renaming = Some((i, name, false));
            ui.close();
        }
        if ui.button("Duplizieren").clicked() {
            out.push(Action::Duplicate(i));
            ui.close();
        }
        if ui.button("M3U exportieren …").clicked() {
            let name = show
                .playlists
                .get(i)
                .map(|p| p.name.clone())
                .unwrap_or_default();
            if let Some(p) = rfd::FileDialog::new()
                .add_filter("M3U8", &["m3u8"])
                .set_file_name(format!("{name}.m3u8"))
                .save_file()
            {
                out.push(Action::Export(i, p));
            }
            ui.close();
        }
        if ui.button("Leeren").clicked() {
            out.push(Action::Clear(i));
            ui.close();
        }
        ui.separator();
        let blocked = live && on_air;
        if ui
            .add_enabled(
                !blocked,
                egui::Button::new(egui::RichText::new("Löschen").color(theme::on_air())),
            )
            .on_disabled_hover_text("Läuft gerade auf Sendung. Erst eine andere Playlist wählen.")
            .clicked()
        {
            out.push(Action::Delete(i));
            ui.close();
        }
    }

    /// Hinweis nach dem Löschen: 8 s, „Rückgängig“, Schließen mit × oder Esc.
    pub fn undo_toast(&mut self, ctx: &egui::Context) -> Option<Action> {
        let now = ctx.input(|i| i.time);
        let mut slot = self.undo.lock().ok()?;
        let s = slot.as_mut()?;
        let until = *s.until.get_or_insert(now + UNDO_SECS);
        let left = until - now;
        if left <= 0.0 || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            *slot = None;
            return None;
        }
        let text = format!("Playlist „{}“ gelöscht", s.deleted.playlist.name);
        let (mut undo, mut close) = (false, false);
        egui::Area::new(egui::Id::new("radio_playlist_undo"))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -40.0))
            .show(ctx, |ui| {
                let r = egui::Frame::new()
                    .fill(theme::lcd_bg())
                    .stroke(egui::Stroke::new(1.0, theme::border_strong()))
                    .corner_radius(6)
                    .inner_margin(egui::Margin::symmetric(12, 7))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new(&text).color(theme::lcd_text()));
                            ui.label(egui::RichText::new("·").color(theme::text_dim()));
                            undo = ui
                                .link(egui::RichText::new("Rückgängig").color(theme::accent()))
                                .clicked();
                            close = ui
                                .add(
                                    egui::Button::new(
                                        egui::RichText::new(theme::ic::X).color(theme::text_dim()),
                                    )
                                    .frame_when_inactive(false),
                                )
                                .on_hover_text("Schließen (Esc)")
                                .clicked();
                        });
                    })
                    .response
                    .rect;
                let frac = (left / UNDO_SECS).clamp(0.0, 1.0) as f32;
                let bar = egui::Rect::from_min_size(
                    egui::pos2(r.left() + 6.0, r.bottom() - 3.0),
                    egui::vec2((r.width() - 12.0) * frac, 2.0),
                );
                ui.painter().rect_filled(bar, 1.0, theme::accent());
            });
        if close {
            *slot = None;
            return None;
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
        undo.then_some(Action::Undo)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn switch_on_air_asks_first() {
        let mut w = ManageWindow::default();
        let mut out = Vec::new();
        w.request_select(1, true, &mut out);
        assert!(out.is_empty());
        assert_eq!(w.confirm_switch, Some(1));
        w.request_select(2, false, &mut out);
        assert_eq!(out, vec![Action::Select(2)]);
    }

    /// Ein Frame mit optionalem Klick an `click`.
    fn frame(
        ctx: &egui::Context,
        w: &mut ManageWindow,
        show: &RadioShow,
        click: Option<egui::Pos2>,
    ) -> Vec<Action> {
        let mut input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1200.0, 800.0),
            )),
            ..Default::default()
        };
        if let Some(pos) = click {
            for pressed in [true, false] {
                input.events.push(egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                });
            }
        }
        let mut out = Vec::new();
        let _ = ctx.run_ui(input, |ui| {
            out = w.window(ui.ctx(), show, false, |_| Vec::new());
        });
        out
    }

    /// André 31.10.: Das Namensfeld hielt den Fokus fest, man kam nicht mehr heraus.
    #[test]
    fn rename_field_lets_go_on_click_elsewhere() {
        let ctx = egui::Context::default();
        let show = RadioShow::default();
        let mut w = ManageWindow {
            open: true,
            renaming: Some((0, "Neu".into(), false)),
            ..Default::default()
        };
        for _ in 0..3 {
            assert!(frame(&ctx, &mut w, &show, None).is_empty());
        }
        assert!(w.renaming.is_some(), "field stays open without input");
        let out = frame(&ctx, &mut w, &show, Some(egui::pos2(1190.0, 790.0)));
        assert!(w.renaming.is_none(), "click elsewhere closes the field");
        assert_eq!(out, vec![Action::Rename(0, "Neu".into())]);
    }
}
