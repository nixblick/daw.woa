//! Eine Esc-Regel für alle Fenster und Fragen (#441).
//!
//! Esc schließt das oberste offene Fenster, eine offene Frage („Projekt speichern?“,
//! „Überschreiben?“ …) geht vor und heißt dabei Abbrechen. Nicht, solange im letzten Bild ein
//! Textfeld den Tastaturfokus hatte (dann nimmt Esc nur den Fokus weg) oder ein Menü bzw.
//! eine Auswahlliste offen ist (dann schließt Esc nur diese).
//!
//! Ablauf je Bild: [`beginn`] vor allem anderen, das Esc gehört dann dem Ziel; jedes Fenster
//! meldet sich mit [`fenster`] bzw. [`frage`] an (die Antwort sagt, ob es jetzt schließen soll);
//! [`ende`] merkt sich den Textfokus. Ein Fenster, das nicht angemeldet ist, sieht kein Esc,
//! solange ein angemeldetes offen ist.

use eframe::egui::{self, Context, Id, LayerId};

#[derive(Clone, Default)]
struct Stand {
    /// Angemeldet im letzten Bild: Ebene, Schlüssel, Frage.
    zuletzt: Vec<(LayerId, Id, bool)>,
    /// Angemeldet in diesem Bild.
    jetzt: Vec<(LayerId, Id, bool)>,
    /// Wer in diesem Bild schließt.
    ziel: Option<Id>,
    /// Hatte am Ende des letzten Bildes ein Textfeld den Fokus?
    text_fokus: bool,
    /// [`ende`] hängt am Ende jedes Bildes.
    eingehakt: bool,
}

fn sid() -> Id {
    Id::new("woa_esc_regel")
}

fn laden(ctx: &Context) -> Stand {
    ctx.data_mut(|d| d.get_temp::<Stand>(sid()))
        .unwrap_or_default()
}

fn sichern(ctx: &Context, s: Stand) {
    ctx.data_mut(|d| d.insert_temp(sid(), s));
}

/// Am Anfang des Bildes, bevor Menü, Ansicht oder Fenster Tasten lesen.
pub fn beginn(ctx: &Context) {
    let mut s = laden(ctx);
    if !s.eingehakt {
        s.eingehakt = true;
        ctx.on_end_pass("woa_esc", std::sync::Arc::new(|ui| ende(ui.ctx())));
    }
    s.zuletzt = std::mem::take(&mut s.jetzt);
    // Mit dem Kreuz geschlossene Fenster sind noch angemeldet, aber nicht mehr zu sehen.
    s.zuletzt
        .retain(|(layer, _, _)| ctx.memory(|m| m.areas().visible_last_frame(layer)));
    s.ziel = None;
    if !s.text_fokus && !ctx.any_popup_open() && !egui::Popup::is_any_open(ctx) {
        let ziel = s
            .zuletzt
            .iter()
            .rev()
            .find(|(_, _, frage)| *frage)
            .map(|(_, id, _)| *id)
            .or_else(|| oberstes(ctx, &s.zuletzt));
        if let Some(id) = ziel
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
        {
            s.ziel = Some(id);
        }
    }
    sichern(ctx, s);
}

/// Das Fenster mit der obersten Ebene (egui ordnet von hinten nach vorn).
fn oberstes(ctx: &Context, offen: &[(LayerId, Id, bool)]) -> Option<Id> {
    let order: Vec<LayerId> = ctx.memory(|m| m.layer_ids().collect());
    offen
        .iter()
        .max_by_key(|(layer, _, _)| order.iter().position(|l| l == layer))
        .map(|(_, id, _)| *id)
}

/// Am Ende des Bildes: Textfokus für das nächste Esc merken (egui nimmt den Fokus bei Esc
/// schon vor dem Bild weg).
fn ende(ctx: &Context) {
    let mut s = laden(ctx);
    s.text_fokus = ctx.egui_wants_keyboard_input();
    sichern(ctx, s);
}

fn anmelden(ctx: &Context, layer: LayerId, key: Id, frage: bool) -> bool {
    let mut s = laden(ctx);
    let zu = s.ziel == Some(key);
    if !zu && !s.jetzt.iter().any(|(_, k, _)| *k == key) {
        s.jetzt.push((layer, key, frage));
    }
    sichern(ctx, s);
    zu
}

/// Offenes Fenster anmelden; `true`: Esc schließt es jetzt.
pub fn fenster(ctx: &Context, layer: LayerId, key: impl std::hash::Hash + std::fmt::Debug) -> bool {
    anmelden(ctx, layer, Id::new(("esc", key)), false)
}

/// Fenster nach `Window::show` anmelden (Antwort von `show`); `true`: Esc schließt es jetzt.
pub fn nach<R>(
    ctx: &Context,
    resp: &Option<egui::InnerResponse<R>>,
    key: impl std::hash::Hash + std::fmt::Debug,
) -> bool {
    resp.as_ref()
        .is_some_and(|r| fenster(ctx, r.response.layer_id, key))
}

/// Offene Frage anmelden; `true`: Esc bricht sie jetzt ab. Fragen gehen Fenstern vor.
pub fn frage(ctx: &Context, layer: LayerId, key: impl std::hash::Hash + std::fmt::Debug) -> bool {
    anmelden(ctx, layer, Id::new(("esc", key)), true)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ein Fenster mit Suchfeld; `fokus` setzt den Tastaturfokus ins Feld.
    fn bild(ctx: &Context, open: &mut bool, text: &mut String, fokus: bool, esc: bool) {
        let mut input = egui::RawInput::default();
        if esc {
            input.events.push(egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            });
        }
        let _ = ctx.run_ui(input, |ui| {
            beginn(ui.ctx());
            if !*open {
                return;
            }
            egui::Window::new("Probe").show(ui.ctx(), |ui| {
                let r = ui.add(egui::TextEdit::singleline(text).id(Id::new("feld")));
                if fokus {
                    r.request_focus();
                }
                if fenster(ui.ctx(), ui.layer_id(), "probe") {
                    *open = false;
                }
            });
        });
    }

    /// Esc im Textfeld nimmt nur den Fokus; erst das nächste Esc schließt das Fenster.
    #[test]
    fn escape_in_text_field_only_drops_focus() {
        let ctx = Context::default();
        let (mut open, mut text) = (true, String::new());
        bild(&ctx, &mut open, &mut text, true, false);
        bild(&ctx, &mut open, &mut text, false, false);
        assert!(ctx.egui_wants_keyboard_input(), "Feld hat keinen Fokus");
        bild(&ctx, &mut open, &mut text, false, true);
        assert!(open, "Esc im Textfeld schließt das Fenster");
        assert!(!ctx.egui_wants_keyboard_input(), "Fokus bleibt");
        bild(&ctx, &mut open, &mut text, false, true);
        assert!(!open, "zweites Esc schließt nicht");
    }

    /// Ohne angemeldetes Fenster bleibt Esc für die Ansicht (Party, Piano-Roll im Dock).
    #[test]
    fn escape_untouched_without_window() {
        let ctx = Context::default();
        let mut seen = false;
        for esc in [false, true] {
            let mut input = egui::RawInput::default();
            if esc {
                input.events.push(egui::Event::Key {
                    key: egui::Key::Escape,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                });
            }
            let _ = ctx.run_ui(input, |ui| {
                beginn(ui.ctx());
                seen |= ui.input(|i| i.key_pressed(egui::Key::Escape));
            });
        }
        assert!(seen, "Esc verschluckt, obwohl kein Fenster offen ist");
    }
}
