//! Reiter „Titel“ im Fenster „Verwalten“ ([`crate::manage`]): links die Titel der Bibliothek
//! mit Suche und Mehrfachauswahl (Klick wählt, Strg-Klick nimmt hinzu oder heraus, Umschalt-
//! Klick wählt einen Bereich), rechts die Titelinfos der Auswahl ([`crate::tag_editor`]).
//! „+“ legt einen Titel in die gewählte Playlist; die Dateien gibt [`titel_tab`] zurück.

use std::path::PathBuf;

use eframe::egui;

use crate::library::LibraryWindow;
use crate::theme;

/// Zeichnet den Reiter. `target` ist der Name der Playlist, in die „+“ legt.
pub fn titel_tab(ui: &mut egui::Ui, library: &mut LibraryWindow, target: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    ui.horizontal(|ui| {
        let mut q = library.query().to_string();
        if ui
            .add(
                egui::TextEdit::singleline(&mut q)
                    .hint_text("Suchen: Titel, Interpret, Tag, Ordner")
                    .desired_width(260.0),
            )
            .changed()
        {
            library.set_query(&q);
        }
        ui.label(
            egui::RichText::new(format!(
                "{} Titel · {} gewählt · „+“ legt in „{target}“",
                library.total(),
                library.marked().len()
            ))
            .small()
            .color(theme::text_dim()),
        );
    });
    ui.add_space(4.0);
    let mut click: Option<(usize, egui::Modifiers)> = None;
    let mut open_window = None;
    ui.columns(2, |cols| {
        egui::ScrollArea::vertical()
            .id_salt("titel_tab")
            .auto_shrink([false, false])
            .show(&mut cols[0], |ui| {
                for (i, t) in library.results().iter().enumerate() {
                    ui.horizontal(|ui| {
                        if ui
                            .small_button(theme::ic::PLUS)
                            .on_hover_text(format!("In „{target}“"))
                            .clicked()
                        {
                            out.push(PathBuf::from(&t.path));
                        }
                        let sel = library.marked().contains(&t.id);
                        let r = ui
                            .selectable_label(sel, &t.title)
                            .on_hover_text(format!("{}\nDoppelklick: Fenster Titelinfos", t.path));
                        if r.clicked() {
                            click = Some((i, ui.input(|inp| inp.modifiers)));
                        } else if r.secondary_clicked() && !sel {
                            // Das Kontextmenü gilt dem Titel darunter: ihn wählen, sonst
                            // zeigte der Editor rechts (derselbe wie im Fenster) die alte Wahl.
                            click = Some((i, egui::Modifiers::NONE));
                        }
                        // Wie in der Bibliothek: Doppelklick öffnet das Fenster „Titelinfos“.
                        if theme::double_clicked(&r) {
                            open_window = Some(t.id);
                        }
                        r.context_menu(|ui| {
                            if ui.button("Titelinfos bearbeiten …").clicked() {
                                open_window = Some(t.id);
                                ui.close();
                            }
                        });
                    });
                }
            });
        let right = &mut cols[1];
        if library.marked().is_empty() {
            right.label(
                egui::RichText::new(
                    "Links einen oder mehrere Titel wählen, um Titelinfos zu bearbeiten.",
                )
                .color(theme::text_dim()),
            );
        } else if let Some(ed) = library.tab_editor() {
            ed.ui(right);
        }
    });
    if let Some((i, m)) = click {
        library.mark_click(i, m.command || m.ctrl, m.shift);
    }
    if let Some(id) = open_window {
        library.edit_tags(&[id]);
    }
    out
}
