//! Reiter „Titel“ im Fenster „Verwalten“ ([`crate::manage`]).
//!
//! Schnittstelle für den Tag-Editor der Bibliothek: [`titel_tab`] zeichnet den Reiter und gibt
//! Dateien zurück, die in die gewählte Playlist sollen. Bis der Tag-Editor eingehängt ist,
//! zeigt der Reiter die Trefferliste der Bibliothek (Suche wie im Bibliotheksfenster) mit
//! „+“ je Titel.

use std::path::PathBuf;

use eframe::egui;

use crate::library::LibraryWindow;
use crate::theme;

/// Zeichnet den Reiter. `target` ist der Name der Playlist, in die „+“ legt.
pub fn titel_tab(ui: &mut egui::Ui, library: &mut LibraryWindow, target: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    ui.label(
        egui::RichText::new(format!(
            "{} Titel in der Bibliothek · „+“ legt in „{target}“",
            library.total()
        ))
        .small()
        .color(theme::text_dim()),
    );
    ui.add_space(4.0);
    egui::ScrollArea::vertical()
        .id_salt("titel_tab")
        .show(ui, |ui| {
            for t in library.results() {
                ui.horizontal(|ui| {
                    if ui
                        .small_button(theme::ic::PLUS)
                        .on_hover_text(format!("In „{target}“"))
                        .clicked()
                    {
                        out.push(PathBuf::from(&t.path));
                    }
                    ui.label(&t.title);
                    if !t.tags.is_empty() {
                        ui.label(
                            egui::RichText::new(t.tags.join(", "))
                                .small()
                                .color(theme::text_dim()),
                        );
                    }
                });
            }
        });
    out
}
