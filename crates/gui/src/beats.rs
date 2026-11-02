//! Beats-Reiter im Browser (Design-Runde 3): die Drum-Presets mit Mini-Raster (6 Reihen ×
//! 16 Schritte) und darunter die Kits. Ein Klick legt eine Drum-Spur mit dem gewählten Kit und
//! einem Clip aus dem Preset am Abspielkopf an, als ein Undo-Schritt.

use eframe::egui;
use woa_model::DRUM_PRESETS;
use woa_model::drums::DRUM_ROWS;
use woa_model::sound::{SOUND_PRESETS, SoundPreset, SoundRole};

use crate::state::AppState;
use crate::theme;

/// Länge des eingefügten Beats in Takten.
pub const BEAT_BARS: u32 = 4;

/// Die Kits aus der Klangliste, in Anzeige-Reihenfolge.
pub fn kits() -> impl Iterator<Item = &'static SoundPreset> {
    SOUND_PRESETS.iter().filter(|p| p.role == SoundRole::Kit)
}

/// Farbe je Reihe (Becken … Kick), oben hell, unten kräftig.
fn row_color(r: usize) -> egui::Color32 {
    match r {
        0 | 1 => theme::warn(),
        2 => theme::cue(),
        3 => theme::ok(),
        4 => theme::accent_2(),
        _ => theme::accent(),
    }
}

/// Mini-Raster eines Presets: 6 × 16 Kästchen, gesetzte Schritte in der Farbe der Reihe.
fn mini_raster(ui: &mut egui::Ui, rows: &[&str; 6]) {
    let cell = egui::vec2(5.0, 3.0);
    let gap = 1.0;
    let size = egui::vec2(16.0 * (cell.x + gap), 6.0 * (cell.y + gap));
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect.expand(2.0), 2.0, theme::lcd_bg());
    for (r, pattern) in rows.iter().enumerate() {
        for (i, c) in pattern.chars().take(16).enumerate() {
            let min = rect.min + egui::vec2(i as f32 * (cell.x + gap), r as f32 * (cell.y + gap));
            let on = matches!(c, 'x' | 'X' | 'o' | 'O');
            let col = if on {
                row_color(r)
            } else {
                theme::mix(theme::lcd_bg(), theme::border(), 0.5)
            };
            p.rect_filled(egui::Rect::from_min_size(min, cell), 0.5, col);
        }
    }
}

/// Zeichnet den Reiter. Gibt `true` zurück, wenn ein Beat eingefügt wurde.
pub fn body(ui: &mut egui::Ui, state: &mut AppState, at_beats: f64) -> bool {
    let kit_key = egui::Id::new("beats_kit");
    let kit_names: Vec<&str> = kits().map(|k| k.name).collect();
    let mut kit: usize = ui.data(|d| d.get_temp(kit_key)).unwrap_or(0);
    let mut inserted = false;
    ui.label(theme::panel_label("PRESETS"));
    for preset in DRUM_PRESETS {
        let r = ui
            .horizontal(|ui| {
                mini_raster(ui, &preset.rows);
                ui.add_sized(
                    [ui.available_width(), 26.0],
                    egui::Button::new(egui::RichText::new(preset.name).color(theme::text()))
                        .fill(theme::surface()),
                )
            })
            .inner
            .on_hover_text(format!(
                "{}: Drum-Spur mit {} und {BEAT_BARS} Takten am Abspielkopf anlegen \
                 (Reihen von oben: {})",
                preset.name,
                kit_names.get(kit).copied().unwrap_or("Kit"),
                DRUM_ROWS.map(|r| r.name).join(", ")
            ));
        if r.clicked()
            && let Some(k) = kits().nth(kit)
        {
            inserted |= state
                .insert_beat(preset, k.sound, at_beats, BEAT_BARS)
                .is_some();
        }
    }
    ui.add_space(8.0);
    ui.label(theme::panel_label("KITS"));
    ui.horizontal_wrapped(|ui| {
        for (i, name) in kit_names.iter().enumerate() {
            let on = i == kit;
            let b = egui::Button::new(egui::RichText::new(*name).small().color(if on {
                theme::text()
            } else {
                theme::text_dim()
            }))
            .fill(if on {
                theme::accent_soft()
            } else {
                theme::well()
            })
            .stroke(egui::Stroke::new(
                1.0,
                if on { theme::accent() } else { theme::border() },
            ));
            if ui
                .add(b)
                .on_hover_text("Kit für den nächsten Beat")
                .clicked()
            {
                kit = i;
            }
        }
    });
    ui.data_mut(|d| d.insert_temp(kit_key, kit));
    inserted
}
