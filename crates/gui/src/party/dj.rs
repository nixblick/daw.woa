//! DJ-Ansicht der Party (DJ-Paket, Entwurf ScreenPartyDJ): parallele Wellen mit Phase,
//! Deck A · Mixer · Deck B (B gespiegelt), unten Playlist mit BPM/Tonart und „in A/B laden“,
//! Wünsche und die Fernbedienung. Zustand und Befehle stehen in `woa_radio::control::dj`
//! (ohne Fenster testbar); hier wird nur gezeichnet und `DjCommand` geschickt.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use eframe::egui::{self, Color32, Rect, Sense, Stroke};
use woa_radio::control::{Band, DjCommand, DjDeckView, DjView};

use super::knobs::{self, Ctl, Fmt, KnobStyle};
use super::{GAP, PartyView, seg_lcd, wave_columns};
use crate::radio::RadioDesk;
use crate::theme;

/// Höhe der parallelen Wellen (Entwurf DjWaves: 150 px).
pub(super) const WAVES_H: f32 = 150.0;
/// Breite des Mixers zwischen den Decks.
const MIXER_W: f32 = 390.0;
const MIXER_MIN_W: f32 = 270.0;
/// Mindestbreite der Tastenspalte im Deck (Auto-Loop-Zeile mit LOOP-Taste); darunter wird die
/// Jog-Scheibe kleiner (#443).
#[cfg(test)]
const KEYS_MIN_W: f32 = 168.0;
/// Kleinste Jog-Scheibe (#443).
const JOG_MIN: f32 = 64.0;
/// Wunschbreite der Tastenspalte (Cue, Play, Sync lesbar nebeneinander).
const KEYS_WANT_W: f32 = 210.0;
/// Lücke zwischen den Auto-Loop-Zahlen und der LOOP-Taste.
const LOOP_GAP: f32 = 6.0;
/// Tempo-Spalte: Breite und die zwei Anschiebe-Tasten darunter (#443: 2 × 27 + 2 = 56).
const TEMPO_W: f32 = 56.0;
const NUDGE_GAP: f32 = 2.0;
const NUDGE_W: f32 = (TEMPO_W - NUDGE_GAP) / 2.0;
/// Unter dieser Deck-Innenbreite rutschen Restzeit, BPM und Tonart in eine zweite Kopfzeile (#443).
const HEAD_WIDE_MIN: f32 = 560.0;
/// Sichtbares Fenster der Wellen in Sekunden (Mausrad zoomt).
const ZOOM_DEFAULT: f64 = 12.0;

/// Zustand der DJ-Ansicht, der nur in der Oberfläche lebt.
#[derive(Default)]
pub(super) struct DjUi {
    /// Cue-Taste je Deck gerade gehalten (Maus oder Taste).
    cue_held: [bool; 2],
    /// Anschieben je Deck läuft (Taste oder Jog-Rand).
    nudging: [bool; 2],
    /// Jogwheel: Startmodus (true = Scratch oben auf der Platte) und letzter Winkel.
    jog: [Option<(bool, f32)>; 2],
    /// Sichtfenster der Wellen in Sekunden (0 = Vorgabe).
    zoom: f64,
    /// Analyse-Angaben schon an das Deck gegeben: (Playlist-Index, Hash).
    sent: [Option<(usize, String)>; 2],
    /// Wann zuletzt nach gespeicherten Marken gesehen wurde.
    marks_checked: Option<std::time::Instant>,
    /// BPM und Tonart je Datei für die Playlist (im Hintergrund aus der Bibliothek).
    meta: Arc<Mutex<HashMap<PathBuf, TrackMeta>>>,
    meta_asked: HashSet<PathBuf>,
    /// Fotomodus `WOA_PARTY_VIEW=dj` hat die Ansicht schon gesetzt.
    photo_view: bool,
}

/// BPM und Camelot-Tonart eines Playlist-Titels.
type TrackMeta = (Option<f64>, Option<String>);

/// Deckfarben: A = `accent`, B = `track-1`.
pub(super) fn deck_color(side: usize) -> Color32 {
    if side == 0 {
        theme::accent()
    } else {
        theme::track_color(0)
    }
}

/// Farbe eines Hot Cues (1–4).
/// Deck A · Mixer · Deck B über die ganze Breite (#332): die Decks teilen sich, was neben dem
/// Mixer bleibt, der Mixer steht genau in der Mitte. Jeder Teil bekommt seine äußere Größe;
/// sein Rahmen liegt innerhalb (früher wurde der Rahmen doppelt abgezogen, rechts blieb ein
/// Rand und der Mixer stand links der Mitte). Liefert die belegten Flächen.
fn three_columns(
    ui: &mut egui::Ui,
    h: f32,
    mut part: impl FnMut(&mut egui::Ui, usize),
) -> [Rect; 3] {
    // #443: Bei schmalem Fenster (1280 px mit 125 % sind 1024 Punkte) schrumpft auch das
    // Mischpult; die Decks bekommen nie mehr als den Rest (früher mindestens 300 px, dann
    // ragte Deck B aus dem Fenster).
    let avail = ui.available_width();
    let mixer_w = mixer_width(avail);
    let w = ((avail - mixer_w - 2.0 * GAP) / 2.0).max(0.0);
    let mut rects = [Rect::NOTHING; 3];
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = GAP;
        for (k, r) in rects.iter_mut().enumerate() {
            let size = egui::vec2(if k == 1 { mixer_w } else { w }, h);
            *r = ui
                .allocate_ui_with_layout(size, egui::Layout::top_down(egui::Align::Min), |ui| {
                    ui.set_min_size(size);
                    ui.set_max_size(size);
                    part(ui, k);
                })
                .response
                .rect;
        }
    });
    rects
}

/// Breite des Mischpults: 390 px, in schmalen Fenstern 30 % der Breite, mindestens 270 px (#443).
fn mixer_width(avail: f32) -> f32 {
    MIXER_W.min((avail * 0.3).max(MIXER_MIN_W))
}

/// Was ein Klick auf eine Hot-Cue-Taste tut (#331): `Some(clear)` schickt das Kommando.
/// Links setzt bzw. springt an (mit Umschalt: löschen), rechts löscht eine belegte Taste.
fn hot_cue_click(primary: bool, secondary: bool, shift: bool, set: bool) -> Option<bool> {
    if secondary {
        return set.then_some(true);
    }
    primary.then_some(shift)
}

/// Jog-Scheibe und Tastenspalte im Deck-Körper (#443): Jog, zwei Abstände zu 12 px, Tasten
/// und Tempo-Spalte füllen höchstens `w`. Die Tasten möchten 210 px; wird es enger, wird
/// zuerst die Scheibe kleiner (bis 64 px), danach die Tasten (bis 168 px).
fn deck_body_widths(w: f32, body_h: f32) -> (f32, f32) {
    let jog = (body_h - 8.0)
        .min(w - TEMPO_W - KEYS_WANT_W - 24.0)
        .clamp(JOG_MIN, 190.0);
    let center = (w - jog - TEMPO_W - 24.0).max(60.0);
    (jog, center)
}

/// Breite einer Auto-Loop-Zahl: n Tasten, Lücke, LOOP-Taste (`lw`) und n + 1 Abstände zu 2 px
/// füllen `w` genau (#443; vorher ragte die Zeile 2 px, bei schmalem Deck 50 px hinaus).
fn loop_key_width(w: f32, lw: f32, n: f32) -> f32 {
    ((w - lw - LOOP_GAP - (n + 1.0) * 2.0) / n).max(12.0)
}

/// Grundbreite der Transporttasten: drei Tasten (Faktoren 0,85 + 1,3 + 0,85) plus Keylock (36 px)
/// und drei Abstände zu 8 px füllen `w` genau (#443).
fn transport_key_width(w: f32) -> f32 {
    (w - 3.0 * 8.0 - 36.0) / 3.0
}

fn hot_color(slot: usize) -> Color32 {
    match slot {
        0 => theme::ok(),
        1 => theme::warn(),
        2 => theme::track_color(4),
        _ => theme::cue(),
    }
}

/// Tastenkürzel der DJ-Ansicht: (Taste, Deck, Aktion). Keine davon in `RESERVED_KEYS` oder in
/// den Party-Tasten (Leertaste, N, P, M, R, S, F, F11, Pfeile, Q).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum KeyAction {
    Cue,
    PlayPause,
    Sync,
    HotCue(usize),
}

pub(super) const DJ_KEYS: [(egui::Key, usize, KeyAction); 14] = [
    (egui::Key::Z, 0, KeyAction::Cue),
    (egui::Key::X, 0, KeyAction::PlayPause),
    (egui::Key::V, 0, KeyAction::Sync),
    (egui::Key::Num1, 0, KeyAction::HotCue(0)),
    (egui::Key::Num2, 0, KeyAction::HotCue(1)),
    (egui::Key::Num3, 0, KeyAction::HotCue(2)),
    (egui::Key::Num4, 0, KeyAction::HotCue(3)),
    (egui::Key::Comma, 1, KeyAction::Cue),
    (egui::Key::Period, 1, KeyAction::PlayPause),
    (egui::Key::Slash, 1, KeyAction::Sync),
    (egui::Key::Num7, 1, KeyAction::HotCue(0)),
    (egui::Key::Num8, 1, KeyAction::HotCue(1)),
    (egui::Key::Num9, 1, KeyAction::HotCue(2)),
    (egui::Key::Num0, 1, KeyAction::HotCue(3)),
];

/// Tastenname für Tooltips.
fn key_hint(side: usize, a: KeyAction) -> String {
    DJ_KEYS
        .iter()
        .find(|(_, s, k)| *s == side && *k == a)
        .map(|(k, _, _)| format!(" (Taste {})", k.symbol_or_name()))
        .unwrap_or_default()
}

/// Restzeit als MM:SS.
fn mmss(secs: f64) -> String {
    let s = secs.max(0.0).round() as u64;
    format!("{:02}:{:02}", s / 60, s % 60)
}

/// Abweichung der Schläge zwischen den Decks in Millisekunden (A gegen B), wenn beide
/// ein Tempo haben. Positiv: A liegt vorn.
pub(super) fn phase_ms(a: &DjDeckView, b: &DjDeckView) -> Option<f64> {
    let (pa, pb, bpm) = (a.phase?, b.phase?, b.bpm_now.or(a.bpm_now)?);
    if bpm <= 0.0 {
        return None;
    }
    let mut d = pa - pb;
    d -= d.round();
    Some(d * 60_000.0 / bpm)
}

/// Kompakte Fernbedienung unter dieser Fensterhöhe (RemoteCard).
pub(super) const COMPACT_BELOW: f32 = 900.0;
/// Kleinste Höhe der DJ-Ansicht unter der Umschaltleiste; darunter rollt sie senkrecht (#443).
const DJ_MIN_H: f32 = 600.0;

impl PartyView {
    /// Leiste oben: Ansicht Einfach|DJ, Auto-Übergang (in DJ aus), Kopfhörer-Ausgang.
    pub(super) fn view_bar(
        &mut self,
        ui: &mut egui::Ui,
        desk: &RadioDesk,
        dj: Option<&DjView>,
        auto: bool,
    ) -> bool {
        let on = dj.is_some_and(|d| d.dj);
        let mut folder = false;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            ui.label(theme::panel_label("ANSICHT"));
            for (word, want, tip) in [
                ("Einfach", false, "Große Tasten, Auto-Übergang"),
                ("DJ", true, "Zwei Decks und Mixer zum Mixen von Hand"),
            ] {
                let font = theme::label_font(13.0);
                let w = crate::widgets::key_width(ui, word, &font, 56.0);
                let r = crate::widgets::Key::new(word, egui::vec2(w, 28.0))
                    .font(font)
                    .radius(theme::RADIUS_MD)
                    .color(theme::accent())
                    .on(on == want)
                    .enabled(dj.is_some())
                    .ui(ui)
                    .on_hover_text(tip);
                if r.clicked() && on != want {
                    desk.dj(DjCommand::View { dj: want });
                }
            }
            // Wunschliste und Fernbedienung ein-/ausblenden (#361).
            ui.add_space(12.0);
            self.panel_keys(ui);
            if on {
                ui.add_space(8.0);
                ui.add_enabled_ui(false, |ui| {
                    crate::widgets::Key::new(
                        &format!("{}  AUTO-ÜBERGANG", super::AUTO_ICON),
                        egui::vec2(170.0, 28.0),
                    )
                    .font(theme::label_font(13.0))
                    .radius(theme::RADIUS_MD)
                    .on(auto)
                    .ui(ui)
                });
                ui.label(
                    egui::RichText::new("im DJ-Modus aus – du blendest selbst")
                        .size(12.0)
                        .color(theme::text_dim()),
                );
                if let Some(m) = desk.view().messages.last() {
                    ui.label(
                        egui::RichText::new(m)
                            .size(12.0)
                            .color(theme::danger_text()),
                    );
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    folder = ui
                        .button(format!("{} ORDNER LADEN", theme::ic::FOLDER_OPEN))
                        .on_hover_text("Alle Titel eines Ordners in die Playlist")
                        .clicked();
                    let cue = dj.is_some_and(|d| d.cue_available);
                    theme::status_pill(
                        ui,
                        if cue {
                            "KOPFHÖRER · AUSGANG 3/4"
                        } else {
                            "KOPFHÖRER · KEIN AUSGANG"
                        },
                        theme::cue(),
                        cue,
                    );
                });
            }
        });
        folder
    }

    /// Die ganze DJ-Ansicht. Liefert, ob „Ordner laden“ gedrückt wurde.
    pub(super) fn dj_ui(
        &mut self,
        ui: &mut egui::Ui,
        desk: &mut RadioDesk,
        library: &mut crate::library::LibraryWindow,
        dj: &DjView,
        cmds: &mut Vec<woa_radio::party::PartyCommand>,
    ) {
        self.dj_keys(ui.ctx(), desk, dj);
        // MIDI-Learn aus den Kontextmenüen der DJ-Tasten (die bekommen nur `&RadioDesk`).
        if let Some(t) = ui
            .ctx()
            .data_mut(|d| d.remove_temp::<Option<woa_io::controller::Target>>(learn_id()))
            .flatten()
        {
            desk.learn = Some(t);
        }
        let v = desk.view().clone();
        self.ask_meta(&v, library.db_path());
        let full_h = ui.available_height();
        // #443: In sehr niedrigen Fenstern (1280 × 800 oder × 720 mit 125 % Skalierung) passt
        // nicht alles untereinander: dann rollt die Ansicht senkrecht, statt unten Fernbedienung
        // und Wunschliste abzuschneiden. Waagerecht bleibt alles im Fenster.
        if full_h < DJ_MIN_H {
            egui::ScrollArea::vertical()
                .id_salt("party-dj-klein")
                .auto_shrink([false, false])
                .max_height(full_h)
                .show(ui, |ui| {
                    self.dj_body(ui, desk, library, dj, &v, cmds, DJ_MIN_H);
                });
        } else {
            self.dj_body(ui, desk, library, dj, &v, cmds, full_h);
        }
    }

    /// Wellen, Decks mit Mischpult, unten Playlist und rechte Spalte in der Höhe `full_h`.
    #[allow(clippy::too_many_arguments)]
    fn dj_body(
        &mut self,
        ui: &mut egui::Ui,
        desk: &mut RadioDesk,
        library: &mut crate::library::LibraryWindow,
        dj: &DjView,
        v: &woa_radio::station::View,
        cmds: &mut Vec<woa_radio::party::PartyCommand>,
        full_h: f32,
    ) {
        let top = ui.cursor().top();
        let used = |ui: &egui::Ui| ui.cursor().top() - top;
        let win_h = ui.ctx().content_rect().height();
        // #443: 1280 × 720 mit 125 % sind nur 576 Punkte Höhe: Wellen, Decks und unten
        // Playlist/Bibliothek teilen sich, was da ist, statt unten über den Rand zu laufen.
        let waves_h = if win_h < 620.0 {
            64.0
        } else if win_h < 700.0 {
            80.0
        } else if win_h < COMPACT_BELOW {
            110.0
        } else {
            WAVES_H
        };
        self.dj_waves(ui, dj, waves_h);
        let rest = full_h - used(ui);
        let bottom_min = if win_h < 700.0 {
            // Fernbedienung (eine Zeile) und darüber zwei Zeilen Wunschliste.
            150.0
        } else if win_h < COMPACT_BELOW {
            175.0
        } else {
            220.0
        };
        let deck_min = if win_h < 700.0 { 200.0 } else { 300.0 };
        let deck_h = (rest - bottom_min - GAP).clamp(deck_min, 560.0);
        three_columns(ui, deck_h, |ui, part| match part {
            0 => self.dj_deck(ui, desk, dj, v, 0),
            1 => dj_mixer(ui, desk, dj, self.knob_style),
            _ => self.dj_deck(ui, desk, dj, v, 1),
        });
        let h = (full_h - used(ui)).max(0.0);
        let (left_w, right_w) = super::columns_split(ui.available_width());
        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(left_w, h),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.set_width(left_w);
                    self.dj_playlist(ui, desk, dj, v, h);
                },
            );
            ui.allocate_ui_with_layout(
                egui::vec2(right_w, h),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.set_width(right_w);
                    // Ausgeblendete Karten (#361): ohne Wunschliste steht hier die Bibliothek
                    // und wächst in den freien Platz.
                    let panels = self.panels;
                    if !panels.remote {
                        self.remote_h = 0.0;
                    }
                    let side_h = if panels.remote {
                        (h - self.remote_h - GAP).max(60.0)
                    } else {
                        h - self.hidden_remote_h(panels)
                    };
                    egui::ScrollArea::vertical()
                        .id_salt("party-dj-side")
                        .max_height(side_h)
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            ui.spacing_mut().item_spacing = egui::vec2(GAP, GAP);
                            if panels.wishes {
                                self.wish_card(ui, library, cmds);
                            } else {
                                let rows = super::library_rows(side_h);
                                self.search_card(ui, desk, library, cmds, rows);
                            }
                        });
                    if panels.remote {
                        let r = ui.scope(|ui| self.remote.ui_small(ui)).response;
                        self.remote_h = r.rect.height();
                    } else {
                        self.hidden_remote_chip(ui);
                    }
                },
            );
        });
    }

    /// Fotomodus: `WOA_PARTY_VIEW=dj` schaltet beim ersten Bild die DJ-Ansicht ein.
    pub(super) fn photo_dj_view(&mut self, desk: &RadioDesk, dj: Option<&DjView>) {
        let Some(dj) = dj else { return };
        if self.dj.photo_view || dj.dj {
            self.dj.photo_view = true;
            return;
        }
        if std::env::var("WOA_PARTY_VIEW").as_deref() == Ok("dj") {
            // Bis die Sendung die Ansicht zeigt (sie antwortet asynchron).
            desk.dj(DjCommand::View { dj: true });
        } else {
            self.dj.photo_view = true;
        }
    }

    /// Gibt die Analyse (Tempo, Beatgrid, Tonart, Marken) an die Decks und speichert neue
    /// Hot Cues und Schleifen in der Bibliothek.
    pub(super) fn dj_feed(&mut self, desk: &RadioDesk, dj: Option<&DjView>, db: &std::path::Path) {
        let Some(dj) = dj else { return };
        for k in 0..2 {
            let Some(idx) = dj.decks[k].index else {
                self.dj.sent[k] = None;
                continue;
            };
            let Some(info) = self.waves[k].get() else {
                continue;
            };
            // Die Wellen folgen demselben Titel; erst wenn beide übereinstimmen, senden.
            let path_ok = desk
                .view()
                .playlist
                .get(idx)
                .is_some_and(|e| self.waves[k].path.as_deref() == Some(e.path.as_path()));
            let key = (idx, info.hash.clone());
            if !path_ok || self.dj.sent[k].as_ref() == Some(&key) {
                continue;
            }
            self.dj.sent[k] = Some(key);
            // Das Deck misst Titel ohne Bibliotheksanalyse selbst (#347); die Playlist zeigt das mit.
            if let (Some(path), Ok(mut g)) = (self.waves[k].path.clone(), self.dj.meta.lock()) {
                let camelot = info
                    .key
                    .as_deref()
                    .and_then(woa_dsp::analysis::track::camelot_of_name);
                g.insert(path, (info.bpm, camelot));
            }
            let t = woa_radio::control::DjTrackInfo {
                index: idx,
                bpm: info.bpm,
                beat_offset: info.beat_offset,
                key: info.key.clone(),
                hash: Some(info.hash.clone()),
                hot: info.marks.hot,
                looped: info.marks.looped,
            };
            desk.with(move |c| c.dj_track_info(k, t));
        }
        let due = self
            .dj
            .marks_checked
            .is_none_or(|t| t.elapsed() >= std::time::Duration::from_millis(500));
        if due {
            self.dj.marks_checked = Some(std::time::Instant::now());
            let db = db.to_path_buf();
            desk.with(move |c| {
                let changes = c.dj_take_marks();
                if changes.is_empty() {
                    return;
                }
                match woa_library::Library::open(&db) {
                    Ok(lib) => {
                        for ch in changes {
                            let m = woa_library::dj::DjMarks {
                                hot: ch.hot,
                                looped: ch.looped,
                            };
                            if let Err(e) = lib.set_dj_marks(&ch.hash, &m) {
                                tracing::warn!(error = %e, "dj: marks not saved");
                            }
                        }
                    }
                    Err(e) => tracing::warn!(error = %e, "dj: library not open"),
                }
            });
        }
    }

    /// BPM und Tonart der Playlist-Titel im Hintergrund holen (einmal je Datei).
    fn ask_meta(&mut self, v: &woa_radio::station::View, db: &std::path::Path) {
        let new: Vec<PathBuf> = v
            .playlist
            .iter()
            .filter(|e| !self.dj.meta_asked.contains(&e.path))
            .map(|e| e.path.clone())
            .collect();
        if new.is_empty() {
            return;
        }
        self.dj.meta_asked.extend(new.iter().cloned());
        let out = self.dj.meta.clone();
        let db = db.to_path_buf();
        let spawned = std::thread::Builder::new()
            .name("party-dj-meta".into())
            .spawn(move || {
                let lib = match woa_library::Library::open(&db) {
                    Ok(l) => l,
                    Err(e) => {
                        tracing::warn!(error = %e, "dj: library not open");
                        return;
                    }
                };
                for p in new {
                    let a = lib
                        .by_path(&p.to_string_lossy())
                        .ok()
                        .flatten()
                        .and_then(|t| t.analysis);
                    let m = (
                        a.as_ref().and_then(|a| a.bpm),
                        a.and_then(|a| a.key)
                            .and_then(|k| woa_dsp::analysis::track::camelot_of_name(&k)),
                    );
                    if let Ok(mut g) = out.lock() {
                        g.insert(p, m);
                    }
                }
            });
        if let Err(e) = spawned {
            tracing::warn!(error = %e, "dj: meta thread not started");
        }
    }

    /// Tastenkürzel der DJ-Ansicht (nur ohne Textfokus).
    fn dj_keys(&mut self, ctx: &egui::Context, desk: &RadioDesk, dj: &DjView) {
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        let events: Vec<(egui::Key, bool, bool)> = ctx.input(|i| {
            i.events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::Key {
                        key,
                        pressed,
                        repeat: false,
                        modifiers,
                        ..
                    } if !modifiers.ctrl && !modifiers.alt && !modifiers.command => {
                        Some((*key, *pressed, modifiers.shift))
                    }
                    _ => None,
                })
                .collect()
        });
        for (key, pressed, shift) in events {
            let Some((_, side, a)) = DJ_KEYS.iter().find(|(k, _, _)| *k == key).copied() else {
                continue;
            };
            let d = &dj.decks[side];
            match (a, pressed) {
                (KeyAction::Cue, true) => {
                    self.dj.cue_held[side] = true;
                    desk.dj(DjCommand::CueDown { side });
                }
                (KeyAction::Cue, false) => {
                    self.dj.cue_held[side] = false;
                    desk.dj(DjCommand::CueUp { side });
                }
                (KeyAction::PlayPause, true) => desk.dj(if d.playing {
                    DjCommand::Pause { side }
                } else {
                    DjCommand::Play { side }
                }),
                (KeyAction::Sync, true) => desk.dj(DjCommand::Sync { side, on: !d.sync }),
                (KeyAction::HotCue(slot), true) => desk.dj(DjCommand::HotCue {
                    side,
                    slot,
                    clear: shift,
                }),
                _ => {}
            }
        }
    }

    /// Parallele Wellen (DjWaves): Mitte = jetzt, Beatgrid oben, dazwischen die Phase.
    fn dj_waves(&mut self, ui: &mut egui::Ui, dj: &DjView, h: f32) {
        let (rect, resp) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), h), Sense::hover());
        if resp.hovered() {
            let s = ui.input(|i| i.smooth_scroll_delta.y);
            if s != 0.0 {
                let z = if self.dj.zoom > 0.0 {
                    self.dj.zoom
                } else {
                    ZOOM_DEFAULT
                };
                self.dj.zoom = (z * (1.0 - f64::from(s) * 0.002)).clamp(3.0, 60.0);
            }
        }
        let window = if self.dj.zoom > 0.0 {
            self.dj.zoom
        } else {
            ZOOM_DEFAULT
        };
        let p = ui.painter_at(rect);
        p.rect(
            rect,
            theme::RADIUS_MD,
            theme::lcd_bg(),
            Stroke::new(1.0, theme::border()),
            egui::StrokeKind::Inside,
        );
        let phase_h = 18.0;
        let lane_h = (rect.height() - phase_h) / 2.0;
        for k in 0..2 {
            let top = rect.top() + if k == 0 { 0.0 } else { lane_h + phase_h };
            let lane = Rect::from_min_size(
                egui::pos2(rect.left(), top),
                egui::vec2(rect.width(), lane_h),
            );
            self.wave_lane(&p, lane, &dj.decks[k], k, window);
        }
        // Phase.
        let mid_y = rect.top() + lane_h + phase_h / 2.0;
        let font = theme::panel_label_font();
        p.text(
            egui::pos2(rect.left() + 10.0, mid_y),
            egui::Align2::LEFT_CENTER,
            "PHASE",
            font.clone(),
            theme::lcd_text(),
        );
        let bar = Rect::from_center_size(
            egui::pos2(rect.center().x, mid_y),
            egui::vec2(rect.width() - 160.0, 3.0),
        );
        p.rect_filled(bar, 1, theme::well());
        if let Some(ms) = phase_ms(&dj.decks[0], &dj.decks[1]) {
            let x = rect.center().x + (ms as f32 / 250.0).clamp(-1.0, 1.0) * bar.width() / 2.0;
            let col = if ms.abs() < 10.0 {
                theme::ok()
            } else {
                theme::warn()
            };
            p.circle_filled(egui::pos2(x, mid_y), 4.0, col);
            p.text(
                egui::pos2(rect.right() - 10.0, mid_y),
                egui::Align2::RIGHT_CENTER,
                format!("{ms:+.0} ms"),
                font,
                col,
            );
        }
        // Jetzt.
        p.vline(
            rect.center().x,
            rect.y_range(),
            Stroke::new(2.0, theme::lcd_ink()),
        );
        resp.on_hover_text("Wellen beider Decks, die Mitte ist jetzt. Mausrad zoomt.");
    }

    fn wave_lane(&self, p: &egui::Painter, lane: Rect, d: &DjDeckView, k: usize, window: f64) {
        let info = self.waves[k].get();
        let col = deck_color(k);
        let dim = theme::mix(col, theme::lcd_bg(), 0.45);
        let mid = lane.center().y;
        let half = lane.height() / 2.0 - 6.0;
        let end = d.end.max(1e-6);
        let step = 2.0;
        let cols = (lane.width() / step) as usize;
        let t0 = d.position - window / 2.0;
        if let Some(info) = &info
            && !info.peaks.is_empty()
            && d.end > 0.0
        {
            let n = info.peaks.len();
            for x in 0..cols {
                let t = t0 + window * (x as f64 + 0.5) / cols as f64;
                if !(0.0..end).contains(&t) {
                    continue;
                }
                let i = ((t / end) * n as f64) as usize;
                let a = f32::from(info.peaks[i.min(n - 1)]) / 255.0;
                let hh = (a * half).max(0.5);
                let x0 = lane.left() + x as f32 * step;
                p.rect_filled(
                    Rect::from_min_max(
                        egui::pos2(x0, mid - hh),
                        egui::pos2(x0 + step - 0.5, mid + hh),
                    ),
                    0,
                    if t < d.position { dim } else { col },
                );
            }
        }
        // Beatgrid oben, Hot Cues als Striche.
        if let Some(bpm) = d.bpm.filter(|b| *b > 0.0) {
            let beat = 60.0 / bpm;
            let off = info.as_ref().and_then(|i| i.beat_offset).unwrap_or(0.0);
            let first = ((t0 - off) / beat).ceil() as i64;
            let mut n = first;
            loop {
                let t = off + n as f64 * beat;
                if t > t0 + window {
                    break;
                }
                let x = lane.left() + ((t - t0) / window) as f32 * lane.width();
                let bar_start = n.rem_euclid(4) == 0;
                p.vline(
                    x,
                    lane.top()..=lane.top() + if bar_start { 8.0 } else { 4.0 },
                    Stroke::new(1.0, crate::widgets::with_alpha(theme::accent_2(), 160)),
                );
                n += 1;
                if n - first > 2000 {
                    break;
                }
            }
        }
        for (slot, h) in d.hot.iter().enumerate() {
            if let Some(t) = h
                && (t0..t0 + window).contains(t)
            {
                let x = lane.left() + ((t - t0) / window) as f32 * lane.width();
                p.vline(x, lane.y_range(), Stroke::new(2.0, hot_color(slot)));
            }
        }
        if let Some((a, b)) = d.looped {
            let xa = lane.left() + ((a - t0) / window) as f32 * lane.width();
            let xb = lane.left() + ((b - t0) / window) as f32 * lane.width();
            let r =
                Rect::from_x_y_ranges(xa.max(lane.left())..=xb.min(lane.right()), lane.y_range());
            if r.width() > 0.0 {
                p.rect_filled(r, 0, crate::widgets::with_alpha(theme::accent_2(), 40));
            }
        }
        let label = format!(
            "{} · {}{}",
            if k == 0 { "A" } else { "B" },
            d.bpm_now
                .map_or_else(|| "–".to_string(), |b| format!("{b:.1}")),
            if d.sync { " (Sync)" } else { "" }
        );
        p.text(
            egui::pos2(
                lane.left() + 10.0,
                if k == 0 {
                    lane.top() + 12.0
                } else {
                    lane.bottom() - 12.0
                },
            ),
            egui::Align2::LEFT_CENTER,
            label,
            theme::panel_label_font(),
            col,
        );
    }

    /// Ein Deck (DjDeck). B ist gespiegelt: Tempo links, Plattenteller rechts.
    fn dj_deck(
        &mut self,
        ui: &mut egui::Ui,
        desk: &RadioDesk,
        dj: &DjView,
        v: &woa_radio::station::View,
        side: usize,
    ) {
        let d = &dj.decks[side];
        let other = &dj.decks[1 - side];
        let col = deck_color(side);
        let entry = d.index.and_then(|i| v.playlist.get(i));
        let h = ui.available_height();
        egui::Frame::new()
            .fill(theme::panel())
            .stroke(Stroke::new(1.0, theme::border()))
            .corner_radius(theme::RADIUS_LG)
            .inner_margin(egui::Margin::same(12))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.set_min_height(h - 26.0);
                // Kante oben in Deckfarbe.
                let top = ui.max_rect().expand(12.0);
                ui.painter().hline(
                    top.x_range().shrink(6.0),
                    top.top() + 1.0,
                    Stroke::new(3.0, col),
                );
                // Kopf. Restzeit, BPM und Tonart rechts, bei schmalem Deck in einer zweiten Zeile.
                let narrow_head = ui.available_width() < HEAD_WIDE_MIN;
                let head_readouts = |ui: &mut egui::Ui| {
                    let rest = if d.end > 0.0 {
                        d.end - d.position
                    } else {
                        entry.and_then(|e| e.seconds).unwrap_or(0.0)
                    };
                    let urgent = d.playing && rest < 30.0;
                    deck_readouts(
                        ui,
                        rest,
                        urgent,
                        d.bpm_now,
                        d.camelot.as_deref(),
                        d.key_match,
                    );
                };
                // #443: Im schmalen Deck brechen die Chips um („Analyse fehlgeschlagen“ samt
                // Knopf ist breiter als ein Deck bei 1280 px und 125 %).
                let chip_row = |ui: &mut egui::Ui, add: &mut dyn FnMut(&mut egui::Ui)| {
                    if narrow_head {
                        ui.horizontal_wrapped(|ui| add(ui));
                    } else {
                        ui.horizontal(|ui| add(ui));
                    }
                };
                chip_row(ui, &mut |ui: &mut egui::Ui| {
                    ui.spacing_mut().item_spacing.x = 10.0;
                    use crate::widgets::{ChipTone, StateChip};
                    // Fassung 7: alle Chips 28 px auf einer Mittellinie. Im DJ-Modus gefüllt, damit
                    // man sie im dunklen Club sieht (Moderator 05.11.2026); sonst nie gefüllt.
                    StateChip::new(
                        if side == 0 { "DECK A" } else { "DECK B" },
                        ChipTone::Color(col),
                    )
                    .dj()
                    .filled()
                    .show(ui);
                    let badge = deck_state_badge(d.playing, d.index.is_some(), col);
                    badge.dj().filled().show(ui);
                    let result = self.waves[side].get().map(|i| i.bpm);
                    match analysis_state(d.index.is_some(), result) {
                        Analysis::Running => {
                            StateChip::new("Analyse", ChipTone::Busy)
                                .icon(theme::ic::CIRCLE_NOTCH)
                                .dj()
                                .show(ui)
                                .on_hover_text("Tempo und Tonart werden gemessen");
                        }
                        Analysis::Failed => {
                            let text = if narrow_head {
                                "Kein Tempo"
                            } else {
                                "Analyse fehlgeschlagen"
                            };
                            StateChip::new(text, ChipTone::Warn)
                                .icon(theme::ic::WARNING)
                                .dj()
                                .show(ui)
                                .on_hover_text("Kein Tempo gefunden: kein klarer Taktschlag erkennbar. Tonart und Wellenform sind davon unberührt. Einzelheiten im Protokoll (\"deck analysis\").");
                            if crate::widgets::icon_button::IconButton::new(
                                theme::ic::ARROW_CLOCKWISE,
                                "Neu analysieren",
                            )
                            .ui(ui)
                            .clicked()
                            {
                                // Nächstes Bild lädt neu (WaveSlot folgt dem Pfad).
                                self.waves[side] = Default::default();
                                self.dj.sent[side] = None;
                            }
                        }
                        Analysis::Empty | Analysis::Done => {}
                    }
                    if !narrow_head {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            head_readouts(ui);
                        });
                    }
                });
                if narrow_head {
                    // #443: Bei schmalem Deck (Fenster ~1280 px) eine zweite Zeile statt Überlappung.
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 10.0;
                        head_readouts(ui);
                    });
                }
                {
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(entry.map_or_else(
                                || "– leer –".to_string(),
                                |e| theme::display_text(&e.title).to_uppercase(),
                            ))
                            .font(theme::label_font(18.0))
                            .strong()
                            .color(theme::text()),
                        )
                        .truncate(),
                    );
                    let sub = match (&d.key, entry.and_then(|e| e.lufs)) {
                        (Some(k), Some(l)) => format!("{k} · {l:.1} LUFS"),
                        (Some(k), None) => k.clone(),
                        (None, Some(l)) => format!("{l:.1} LUFS"),
                        _ => String::new(),
                    };
                    ui.label(egui::RichText::new(sub).size(12.0).color(theme::text_dim()));
                }
                if other.camelot.is_some() && d.camelot.is_some() && !d.key_match {
                    // nichts: der Chip bleibt grau.
                }
                self.overview(ui, desk, d, side);
                ui.add_space(6.0);
                let body_h = ui.available_height().max(120.0);
                // Plattenteller: so groß wie Höhe und Breite erlauben; die Tasten brauchen
                // 168 px (Auto-Loop-Zeile), bei engem Deck wird die Jog-Scheibe kleiner (#443).
                let (jog_d, center_w) = deck_body_widths(ui.available_width(), body_h);
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = 12.0;
                    let order: [u8; 3] = if side == 0 { [0, 1, 2] } else { [2, 1, 0] };
                    for part in order {
                        match part {
                            0 => self.jog(ui, desk, d, side, jog_d),
                            1 => {
                                ui.allocate_ui_with_layout(
                                    egui::vec2(center_w, body_h),
                                    egui::Layout::top_down(egui::Align::Min),
                                    |ui| self.deck_keys(ui, desk, dj, side, center_w),
                                );
                            }
                            _ => tempo_column(ui, desk, d, side, body_h, &mut self.dj.nudging),
                        }
                    }
                });
            });
    }

    /// Übersicht des ganzen Titels mit Hot-Cue-Marken und Loop; Klick springt.
    fn overview(&self, ui: &mut egui::Ui, desk: &RadioDesk, d: &DjDeckView, side: usize) {
        let (rect, resp) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), 34.0), Sense::click());
        let p = ui.painter_at(rect);
        p.rect(
            rect,
            theme::RADIUS_MD,
            theme::lcd_bg(),
            Stroke::new(1.0, theme::border()),
            egui::StrokeKind::Inside,
        );
        let inner = rect.shrink(2.0);
        let info = self.waves[side].get();
        let col = deck_color(side);
        let played = if d.end > 0.0 {
            (d.position / d.end) as f32
        } else {
            0.0
        };
        let step = 2.0;
        let cols = (inner.width() / step) as usize;
        let peaks = info.as_ref().map_or(&[][..], |i| i.peaks.as_slice());
        let dim = theme::mix(theme::lcd_bg(), theme::lcd_text(), 0.38);
        for (x, a) in wave_columns(peaks, cols).into_iter().enumerate() {
            let x0 = inner.left() + x as f32 * step;
            let hh = (a * (inner.height() / 2.0 - 1.0)).max(0.5);
            p.rect_filled(
                Rect::from_min_max(
                    egui::pos2(x0, inner.center().y - hh),
                    egui::pos2(x0 + step - 0.5, inner.center().y + hh),
                ),
                0,
                if (x as f32 + 0.5) / cols.max(1) as f32 <= played {
                    col
                } else {
                    dim
                },
            );
        }
        let at = |t: f64| inner.left() + (t / d.end.max(1e-6)) as f32 * inner.width();
        if d.end > 0.0 {
            if let Some((a, b)) = d.looped {
                p.rect_filled(
                    Rect::from_x_y_ranges(at(a)..=at(b).max(at(a) + 2.0), inner.y_range()),
                    0,
                    crate::widgets::with_alpha(theme::accent_2(), 90),
                );
            }
            for (slot, h) in d.hot.iter().enumerate() {
                if let Some(t) = h {
                    let x = at(*t);
                    p.vline(x, inner.y_range(), Stroke::new(2.0, hot_color(slot)));
                    p.text(
                        egui::pos2(x + 2.0, inner.top() + 1.0),
                        egui::Align2::LEFT_TOP,
                        format!("{}", slot + 1),
                        theme::panel_label_font(),
                        hot_color(slot),
                    );
                }
            }
            if let Some(c) = d.cue_point {
                let x = at(c);
                p.vline(x, inner.y_range(), Stroke::new(1.0, theme::cue()));
            }
            p.vline(
                at(d.position),
                inner.y_range(),
                Stroke::new(2.0, theme::lcd_text()),
            );
        }
        if resp.clicked()
            && d.end > 0.0
            && let Some(pos) = resp.interact_pointer_pos()
        {
            let f = ((pos.x - inner.left()) / inner.width()).clamp(0.0, 1.0);
            desk.dj(DjCommand::Seek {
                side,
                seconds: f64::from(f) * d.end,
            });
        }
        resp.on_hover_text("Ganzer Titel mit Hot Cues und Schleife; Klick springt");
    }

    /// Jogwheel: Rand schiebt an, oben auf der Platte wird gescratcht.
    fn jog(&mut self, ui: &mut egui::Ui, desk: &RadioDesk, d: &DjDeckView, side: usize, size: f32) {
        let id = egui::Id::new(("party-dj-jog", side));
        let r = crate::widgets::jog(ui, id, size, d.playing && self.dj.jog[side].is_none());
        let resp = ui.interact(r.rect, id.with("drag"), Sense::drag());
        let c = r.rect.center();
        let radius = size / 2.0;
        if let Some(pos) = resp.interact_pointer_pos()
            && resp.dragged()
        {
            let angle = (pos.y - c.y).atan2(pos.x - c.x);
            let dt = ui.input(|i| i.stable_dt).max(1e-3);
            let (scratch, last) = *self.dj.jog[side]
                .get_or_insert_with(|| ((pos - c).length() < radius * 0.72, angle));
            let mut da = angle - last;
            if da > std::f32::consts::PI {
                da -= std::f32::consts::TAU;
            } else if da < -std::f32::consts::PI {
                da += std::f32::consts::TAU;
            }
            self.dj.jog[side] = Some((scratch, angle));
            let rate = f64::from(da / dt)
                / f64::from(crate::widgets::jog::REV_PER_S * std::f32::consts::TAU);
            if scratch {
                desk.dj(DjCommand::Scratch {
                    side,
                    speed: Some(rate.clamp(-4.0, 4.0)),
                });
            } else {
                desk.dj(DjCommand::Nudge {
                    side,
                    value: (rate * 0.5).clamp(-1.0, 1.0),
                });
            }
        } else if let Some((scratch, _)) = self.dj.jog[side].take() {
            desk.dj(if scratch {
                DjCommand::Scratch { side, speed: None }
            } else {
                DjCommand::Nudge { side, value: 0.0 }
            });
        }
        resp.on_hover_text(
            "Jogwheel: am Rand drehen schiebt das Tempo an, oben auf der Platte wird gescratcht",
        );
    }

    /// Hot Cues, Auto-Loop, Cue/Play/Sync und Keylock.
    fn deck_keys(&mut self, ui: &mut egui::Ui, desk: &RadioDesk, dj: &DjView, side: usize, w: f32) {
        let d = &dj.decks[side];
        let loaded = d.index.is_some();
        ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
        let rows_h = ui.available_height();
        let key_h = ((rows_h - 24.0) / 3.6).clamp(28.0, 52.0);
        // Hot Cues.
        ui.horizontal(|ui| {
            let pw = (w - 3.0 * 8.0) / 4.0;
            for slot in 0..4 {
                let set = d.hot[slot].is_some();
                let label = format!("{}", slot + 1);
                let r = crate::widgets::Key::new(&label, egui::vec2(pw, key_h * 0.8))
                    .font(theme::label_font(15.0))
                    .radius(theme::RADIUS_MD)
                    .color(hot_color(slot))
                    .lit(set)
                    .enabled(loaded)
                    .ui(ui);
                if set {
                    let b = r.rect;
                    ui.painter().hline(
                        b.x_range().shrink(6.0),
                        b.bottom() - 4.0,
                        Stroke::new(3.0, hot_color(slot)),
                    );
                }
                let r = r.on_hover_text(format!(
                    "Hot Cue {}: leer = hier setzen, belegt = anspringen, Rechtsklick oder \
                     Umschalt+Klick löscht{}",
                    slot + 1,
                    key_hint(side, KeyAction::HotCue(slot))
                ));
                let shift = ui.input(|i| i.modifiers.shift);
                if let Some(clear) = hot_cue_click(r.clicked(), r.secondary_clicked(), shift, set) {
                    desk.dj(DjCommand::HotCue { side, slot, clear });
                }
            }
        });
        // Auto-Loop.
        ui.horizontal(|ui| {
            let n = woa_radio::control::dj::LOOP_BEATS.len() as f32;
            let lw = 90.0;
            // #443: n Tasten + 6 px Lücke + LOOP-Taste, n + 1 Abstände zu 2 px: füllt w genau.
            let sw = loop_key_width(w, lw, n);
            ui.spacing_mut().item_spacing.x = 2.0;
            for b in woa_radio::control::dj::LOOP_BEATS {
                let on = d.loop_beats == Some(b);
                let r = crate::widgets::Key::new(&b.to_string(), egui::vec2(sw, key_h * 0.6))
                    .font(theme::label_font(13.0))
                    .radius(theme::RADIUS_SM)
                    .color(theme::accent_2())
                    .ink(theme::on_accent_2())
                    .on(on)
                    .enabled(loaded && d.bpm.is_some())
                    .ui(ui)
                    .on_hover_text(format!("Schleife über {b} Schläge ab dem letzten Schlag"));
                if r.clicked() {
                    desk.dj(DjCommand::Loop {
                        side,
                        beats: if on { None } else { Some(b) },
                    });
                }
            }
            ui.add_space(LOOP_GAP);
            let on = d.looped.is_some();
            let r = crate::widgets::Key::new(
                &format!("{} LOOP", theme::ic::REPEAT),
                egui::vec2(lw, key_h * 0.6),
            )
            .font(theme::label_font(13.0))
            .radius(theme::RADIUS_SM)
            .color(theme::accent_2())
            .ink(theme::on_accent_2())
            .on(on)
            .enabled(loaded && d.bpm.is_some())
            .ui(ui)
            .on_hover_text(if d.bpm.is_some() {
                "Schleife an/aus (Vorgabe 4 Schläge)"
            } else {
                "Schleifen brauchen ein Tempo aus der Bibliotheksanalyse"
            });
            if r.clicked() {
                desk.dj(DjCommand::Loop {
                    side,
                    beats: if on {
                        None
                    } else {
                        Some(d.loop_beats.unwrap_or(4))
                    },
                });
            }
        });
        // Transport.
        ui.horizontal(|ui| {
            // #443: drei Tasten + Keylock (36 px) + 3 Abstände zu 8 px füllen w genau.
            let kw = transport_key_width(w);
            // Cue: halten spielt ab Cue, loslassen springt zurück.
            let at_cue = !d.playing && d.cue_point.is_some_and(|c| (c - d.position).abs() < 0.05);
            let r = crate::widgets::Key::new("CUE", egui::vec2(kw * 0.85, key_h))
                .font(theme::label_font(17.0))
                .radius(theme::RADIUS_PARTY)
                .color(theme::cue())
                .lit(at_cue || self.dj.cue_held[side])
                .enabled(loaded)
                .ui(ui)
                .on_hover_text(format!(
                    "Steht das Deck: Cue-Punkt hierher. Halten spielt ab Cue, Loslassen springt zurück{}",
                    key_hint(side, KeyAction::Cue)
                ));
            dj_learn_menu(&r, woa_io::controller::Target::DjCue(side));
            let down = r.is_pointer_button_down_on();
            if down && !self.dj.cue_held[side] {
                self.dj.cue_held[side] = true;
                desk.dj(DjCommand::CueDown { side });
            } else if !down && self.dj.cue_held[side] && !ui.input(|i| DJ_KEYS.iter().any(|(k, s, a)| *s == side && *a == KeyAction::Cue && i.key_down(*k))) {
                self.dj.cue_held[side] = false;
                desk.dj(DjCommand::CueUp { side });
            }
            let (icon, word) = if d.playing { (theme::ic::PAUSE, "PAUSE") } else { (theme::ic::PLAY, "PLAY") };
            let r = crate::widgets::Key::new(&format!("{icon}  {word}"), egui::vec2(kw * 1.3, key_h))
                .font(theme::label_font(17.0))
                .radius(theme::RADIUS_PARTY)
                .color(deck_color(side))
                .on(d.playing)
                .enabled(loaded)
                .ui(ui)
                .on_hover_text(format!("Play/Pause dieses Decks{}", key_hint(side, KeyAction::PlayPause)));
            dj_learn_menu(&r, woa_io::controller::Target::DjPlay(side));
            if r.clicked() {
                desk.dj(if d.playing { DjCommand::Pause { side } } else { DjCommand::Play { side } });
            }
            let r = crate::widgets::Key::new("SYNC", egui::vec2(kw * 0.85, key_h))
                .font(theme::label_font(17.0))
                .radius(theme::RADIUS_PARTY)
                .color(theme::accent_2())
                .ink(theme::on_accent_2())
                .on(d.sync)
                .enabled(loaded && d.bpm.is_some())
                .ui(ui)
                .on_hover_text(format!(
                    "Tempo und Takt ans andere Deck (auch Doppel-/Halbtempo, höchstens ±8 %){}",
                    key_hint(side, KeyAction::Sync)
                ));
            if r.clicked() {
                desk.dj(DjCommand::Sync { side, on: !d.sync });
            }
            let r = crate::widgets::Key::new(theme::ic::MUSIC_NOTES, egui::vec2(36.0, 36.0))
                .font(theme::label_font(16.0))
                .radius(theme::RADIUS_MD)
                .color(theme::accent())
                .on(d.keylock)
                .enabled(loaded)
                .ui(ui)
                .on_hover_text("Keylock: Tonhöhe bleibt beim Tempoändern");
            if r.clicked() {
                desk.dj(DjCommand::Keylock { side, on: !d.keylock });
            }
        });
    }

    /// Playlist der DJ-Ansicht: BPM, Tonart, Dauer, „in A/B laden“.
    fn dj_playlist(
        &mut self,
        ui: &mut egui::Ui,
        desk: &RadioDesk,
        dj: &DjView,
        v: &woa_radio::station::View,
        h: f32,
    ) {
        let meta = self.dj.meta.lock().map(|g| g.clone()).unwrap_or_default();
        theme::card_frame().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.set_min_height((h - 20.0).max(0.0));
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                ui.label(theme::panel_label("PLAYLIST").color(theme::accent_2()));
                let name = v.playlists.get(v.active_playlist).cloned().unwrap_or_default();
                egui::ComboBox::from_id_salt("party-dj-playlist")
                    .width(160.0)
                    .selected_text(name)
                    .show_ui(ui, |ui| {
                        for (i, n) in v.playlists.iter().enumerate() {
                            if ui.selectable_label(i == v.active_playlist, n).clicked()
                                && i != v.active_playlist
                            {
                                desk.party_select_playlist(i);
                            }
                        }
                    });
                let key_a = dj.decks[0].camelot.clone();
                ui.label(
                    egui::RichText::new(match &key_a {
                        Some(k) => format!("{} Titel · grün: passt zu A ({k})", v.playlist.len()),
                        None => format!("{} Titel", v.playlist.len()),
                    })
                    .monospace()
                    .size(12.0)
                    .color(theme::text_dim()),
                );
            });
            let row_h = 24.0;
            let w = ui.available_width();
            let cols = [(w - 300.0).max(60.0), 60.0, 50.0, 70.0, 90.0];
            let head = |ui: &mut egui::Ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    for (t, cw) in ["#  TITEL", "BPM", "KEY", "DAUER", "LADEN"].iter().zip(cols) {
                        ui.add_sized(
                            egui::vec2(cw, 18.0),
                            egui::Label::new(theme::panel_label(*t)),
                        );
                    }
                });
            };
            head(ui);
            let a_key = dj.decks[0].camelot.clone();
            egui::ScrollArea::vertical()
                .id_salt("party-dj-rows")
                .auto_shrink([false, false])
                .show_rows(ui, row_h, v.playlist.len(), |ui, range| {
                    for i in range {
                        let e = &v.playlist[i];
                        let (bpm, key) = meta.get(&e.path).cloned().unwrap_or((None, None));
                        let on_a = dj.decks[0].index == Some(i);
                        let on_b = dj.decks[1].index == Some(i);
                        let (rect, _) = ui.allocate_exact_size(egui::vec2(w, row_h), Sense::hover());
                        let p = ui.painter();
                        if on_a || on_b {
                            p.rect_filled(rect, 0, crate::widgets::with_alpha(deck_color(if on_a { 0 } else { 1 }), 30));
                            p.rect_filled(
                                Rect::from_min_size(rect.min, egui::vec2(3.0, rect.height())),
                                0,
                                deck_color(if on_a { 0 } else { 1 }),
                            );
                        }
                        let y = rect.center().y;
                        let mut x = rect.left() + 8.0;
                        let font = egui::FontId::proportional(13.0);
                        let mono = egui::FontId::monospace(12.0);
                        p.text(egui::pos2(x + 16.0, y), egui::Align2::RIGHT_CENTER, format!("{}", i + 1), mono.clone(), theme::text_dim());
                        let title = p.layout(theme::display_text(&e.title).into_owned(), font, theme::text(), f32::INFINITY);
                        let clip = Rect::from_min_max(egui::pos2(x + 26.0, rect.top()), egui::pos2(rect.left() + cols[0] - 8.0, rect.bottom()));
                        ui.painter_at(clip).galley(egui::pos2(x + 26.0, y - title.size().y / 2.0), title, theme::text());
                        x = rect.left() + cols[0];
                        let txt = |p: &egui::Painter, x: f32, cw: f32, s: String, c: Color32| {
                            p.text(egui::pos2(x + cw - 10.0, y), egui::Align2::RIGHT_CENTER, s, mono.clone(), c);
                        };
                        txt(p, x, cols[1], bpm.map_or_else(|| "–".into(), |b| format!("{b:.1}")), theme::text_dim());
                        x += cols[1];
                        let fits = matches!((&key, &a_key), (Some(k), Some(a)) if woa_dsp::analysis::track::camelot_compatible(k, a));
                        txt(p, x, cols[2], key.clone().unwrap_or_else(|| "–".into()), if fits { theme::ok() } else { theme::text_dim() });
                        x += cols[2];
                        txt(p, x, cols[3], e.seconds.map_or_else(|| "–".into(), mmss), theme::text());
                        x += cols[3];
                        for side in 0..2 {
                            let r = Rect::from_min_size(egui::pos2(x + 8.0 + side as f32 * 40.0, rect.top() + 2.0), egui::vec2(34.0, row_h - 4.0));
                            let busy = dj.decks[side].playing;
                            let resp = ui.interact(r, egui::Id::new(("dj-load", i, side)), if busy { Sense::hover() } else { Sense::click() });
                            let c = if busy { theme::text_dim() } else { deck_color(side) };
                            let p = ui.painter();
                            p.rect_stroke(r, 4, Stroke::new(1.0, if resp.hovered() && !busy { c } else { theme::border() }), egui::StrokeKind::Inside);
                            p.text(r.center(), egui::Align2::CENTER_CENTER, if side == 0 { "A" } else { "B" }, theme::label_font(12.0), c);
                            let resp = resp.on_hover_text(if busy {
                                "Das Deck spielt gerade; erst anhalten"
                            } else if side == 0 {
                                "In Deck A laden"
                            } else {
                                "In Deck B laden"
                            });
                            if resp.clicked() {
                                desk.dj(DjCommand::Load { side, index: i });
                            }
                        }
                        ui.painter().hline(rect.x_range(), rect.bottom(), Stroke::new(1.0, theme::border()));
                    }
                });
        });
    }
}

/// Tempo-Fader ±8 % mit Anschieben (Symbol-Tasten halten). Doppelklick = 0 %.
fn tempo_column(
    ui: &mut egui::Ui,
    desk: &RadioDesk,
    d: &DjDeckView,
    side: usize,
    h: f32,
    nudging: &mut [bool; 2],
) {
    ui.allocate_ui_with_layout(egui::vec2(TEMPO_W, h), egui::Layout::top_down(egui::Align::Center), |ui| {
        ui.spacing_mut().item_spacing.y = 4.0;
        ui.label(theme::panel_label("TEMPO"));
        let fh = (h - 80.0).max(60.0);
        let mut v = -(d.tempo as f32);
        let id = egui::Id::new(("dj-tempo", side));
        let held: Option<f32> = ui.data(|m| m.get_temp(id));
        if let Some(h) = held {
            v = h;
        }
        let r = crate::widgets::fader(ui, &mut v, -1.0..=1.0, 0.0, egui::vec2(30.0, fh), crate::widgets::Orient::Vertical);
        if r.changed() {
            desk.dj(DjCommand::Tempo { side, value: -f64::from(v) });
        }
        if r.dragged() {
            ui.data_mut(|m| m.insert_temp(id, v));
        } else {
            ui.data_mut(|m| m.remove::<f32>(id));
        }
        r.on_hover_text("Tempo ±8 % (oben langsamer, unten schneller wie am Plattenspieler); Doppelklick = 0 %");
        ui.label(egui::RichText::new(format!("{:+.1} %", d.tempo_percent)).monospace().size(12.0));
        ui.horizontal(|ui| {
            // #443: beide Tasten zusammen genau so breit wie die Spalte (56 px); mit der
            // Grund-Polsterung waren sie 70 px breit und schoben Deck B 14 px aus dem Fenster.
            ui.spacing_mut().item_spacing.x = NUDGE_GAP;
            ui.spacing_mut().button_padding = egui::vec2(2.0, 1.0);
            let mut any = false;
            for (icon, dir, tip) in [
                (theme::ic::CARET_LEFT, -1.0, "Halten: langsamer anschieben"),
                (theme::ic::CARET_RIGHT, 1.0, "Halten: schneller anschieben"),
            ] {
                let r = ui
                    .add_sized(egui::vec2(NUDGE_W, 22.0), egui::Button::new(icon))
                    .on_hover_text(tip);
                if r.is_pointer_button_down_on() {
                    any = true;
                    if !nudging[side] {
                        desk.dj(DjCommand::Nudge { side, value: dir * 0.5 });
                    }
                }
            }
            if any {
                nudging[side] = true;
            } else if nudging[side] {
                nudging[side] = false;
                desk.dj(DjCommand::Nudge { side, value: 0.0 });
            }
        });
    });
}

/// DJ-Mixer: Kanal A, Mitte (Master, Kopfhörer), Kanal B, unten Crossfader mit Kurve.
fn dj_mixer(ui: &mut egui::Ui, desk: &RadioDesk, dj: &DjView, style: KnobStyle) {
    let h = ui.available_height();
    egui::Frame::new()
        .fill(theme::panel())
        .stroke(Stroke::new(1.0, theme::border()))
        .corner_radius(theme::RADIUS_LG)
        .inner_margin(egui::Margin::same(10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.set_min_height(h - 22.0);
            // Unten: Crossfader-Zeile und darunter „Weiter“ (#331).
            let strip_h = (h - 22.0 - 20.0 - 50.0 - 44.0).max(200.0);
            let w = ui.available_width();
            let cw = (w - 2.0 * 8.0) / 3.0;
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                channel(ui, desk, dj, 0, egui::vec2(cw, strip_h), style);
                master(ui, desk, dj, egui::vec2(cw, strip_h), style);
                channel(ui, desk, dj, 1, egui::vec2(cw, strip_h), style);
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(theme::panel_label("A").color(deck_color(0)));
                let mut x = dj.crossfader;
                let r = crate::widgets::fader(
                    ui,
                    &mut x,
                    0.0..=1.0,
                    0.5,
                    egui::vec2(w - 40.0, 26.0),
                    crate::widgets::Orient::Horizontal,
                )
                .on_hover_text("Crossfader: links Deck A, rechts Deck B");
                dj_learn_menu(&r, woa_io::controller::Target::DjCrossfader);
                if r.changed() {
                    desk.dj(DjCommand::Crossfader { value: x });
                }
                ui.label(theme::panel_label("B").color(deck_color(1)));
            });
            ui.add_space(6.0);
            // Fassung 7: unter dem Crossfader eine Zeile `weich | hart` und „Weiter · Übergang“.
            let mut r = None;
            ui.horizontal(|ui| {
                for (word, hard) in [("weich", false), ("hart", true)] {
                    let on = dj.curve_hard == hard;
                    let k = crate::widgets::Key::new(word, egui::vec2(52.0, 36.0))
                        .font(theme::label_font(12.0))
                        .radius(theme::RADIUS_SM)
                        .color(theme::accent())
                        .on(on)
                        .ui(ui)
                        .on_hover_text(if hard {
                            "Kurve hart: schon kurz nach dem Rand voll da (Scratchen, Cuts)"
                        } else {
                            "Kurve weich: gleichmäßige Blende mit gleicher Lautheit in der Mitte"
                        });
                    if k.clicked() && !on {
                        desk.dj(DjCommand::Curve { hard });
                    }
                }
                let any = dj.decks.iter().any(|d| d.index.is_some());
                r = Some(
                    crate::widgets::Key::new(
                        &format!("{}  WEITER · ÜBERGANG", super::AUTO_ICON),
                        egui::vec2(ui.available_width(), 36.0),
                    )
                    .font(theme::label_font(14.0))
                    .radius(theme::RADIUS_MD)
                    .color(theme::accent())
                    .enabled(any)
                    .ui(ui)
                    .on_hover_text(
                        "Zum anderen Deck blenden (Crossfade-Länge der Sendung); danach hält das \
                         alte Deck an und lädt den nächsten Titel (Taste N oder →)",
                    ),
                );
            });
            if r.is_some_and(|r| r.clicked()) {
                desk.dj(DjCommand::Transition);
            }
        });
}

fn channel(
    ui: &mut egui::Ui,
    desk: &RadioDesk,
    dj: &DjView,
    side: usize,
    size: egui::Vec2,
    style: KnobStyle,
) {
    let d = &dj.decks[side];
    let col = deck_color(side);
    let (outer, _) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter()
        .rect_filled(outer, theme::RADIUS_MD, crate::widgets::with_alpha(col, 18));
    ui.painter().hline(
        outer.x_range().shrink(4.0),
        outer.top() + 1.0,
        Stroke::new(2.0, col),
    );
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(outer.shrink(4.0))
            .layout(egui::Layout::top_down(egui::Align::Center)),
    );
    let ui = &mut child;
    ui.spacing_mut().item_spacing.y = 2.0;
    ui.label(theme::panel_label(if side == 0 { "A" } else { "B" }).color(col));
    let id = |s: &str| egui::Id::new(("dj-knob", side, s.to_string()));
    let tm = woa_radio::control::dj::TRIM_DB;
    let trim = Ctl {
        id: id("trim"),
        value: d.trim_db,
        range: (-tm, tm),
        default: 0.0,
        color: col,
        label: "TRIM",
        fmt: Fmt::Db,
        tip: "Eingangspegel des Decks",
    };
    let filter = Ctl {
        id: id("filter"),
        value: d.filter,
        range: (-1.0, 1.0),
        default: 0.0,
        color: col,
        label: "FILTER",
        fmt: Fmt::Filter,
        tip: "Filter: links Tiefpass, rechts Hochpass, Mitte aus",
    };
    let bands = [
        (Band::Hi, d.eq_hi, "HI"),
        (Band::Mid, d.eq_mid, "MID"),
        (Band::Lo, d.eq_lo, "LO"),
    ];
    let eq = |word: &'static str, val: f32| Ctl {
        id: id(word),
        value: val,
        range: (-1.0, 1.0),
        default: 0.0,
        color: col,
        label: word,
        fmt: Fmt::Eq,
        tip: "EQ: Mitte 0 dB, ganz links Kill, rechts bis +6 dB",
    };
    let send_trim = |v: f32| desk.dj(DjCommand::Trim { side, db: v });
    let send_filter = |v: f32| desk.dj(DjCommand::Filter { side, value: v });
    let send_eq = |band: Band, v: f32| {
        desk.dj(DjCommand::Eq {
            side,
            band,
            value: v,
        })
    };
    let fader_min = 90.0;
    let w = ui.available_width();
    match style {
        KnobStyle::MiniFader => {
            let fh = ((size.y - fader_min - 70.0 - knobs::KILL_H) / 2.0).clamp(46.0, 110.0);
            ui.horizontal(|ui| {
                ui.add_space(((w - 2.0 * knobs::MINI_W - 8.0) / 2.0).max(0.0));
                ui.spacing_mut().item_spacing.x = 8.0;
                if let (_, Some(v)) = knobs::control(ui, style, &trim, fh) {
                    send_trim(v);
                }
                if let (_, Some(v)) = knobs::control(ui, style, &filter, fh) {
                    send_filter(v);
                }
            });
            ui.horizontal(|ui| {
                let col = ((w - 8.0) / 3.0).clamp(18.0, knobs::KILL_W);
                ui.add_space(((w - 3.0 * col - 8.0) / 2.0).max(0.0));
                ui.spacing_mut().item_spacing.x = 4.0;
                for (band, val, word) in bands {
                    ui.vertical(|ui| {
                        ui.set_width(col);
                        ui.spacing_mut().item_spacing.y = 2.0;
                        if let (_, Some(v)) = knobs::control(ui, style, &eq(word, val), fh) {
                            send_eq(band, v);
                        }
                        if let Some(v) = knobs::kill(ui, id(word), val) {
                            send_eq(band, v);
                        }
                    });
                }
            });
        }
        KnobStyle::Bar | KnobStyle::Number => {
            if let (_, Some(v)) = knobs::control(ui, style, &trim, w) {
                send_trim(v);
            }
            for (band, val, word) in bands {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    let bw = (w - knobs::KILL_W - 4.0).max(40.0);
                    if let (_, Some(v)) = knobs::control(ui, style, &eq(word, val), bw) {
                        send_eq(band, v);
                    }
                    ui.vertical(|ui| {
                        ui.add_space(knobs::ROW_H - knobs::KILL_H);
                        if let Some(v) = knobs::kill(ui, id(word), val) {
                            send_eq(band, v);
                        }
                    });
                });
            }
            if let (_, Some(v)) = knobs::control(ui, style, &filter, w) {
                send_filter(v);
            }
        }
        KnobStyle::Knob => {
            let k =
                ((size.y - fader_min - 70.0 - 3.0 * knobs::KILL_H) / 5.0 - 16.0).clamp(18.0, 34.0);
            if let (_, Some(v)) = knobs::control(ui, style, &trim, k) {
                send_trim(v);
            }
            for (band, val, word) in bands {
                if let (_, Some(v)) = knobs::control(ui, style, &eq(word, val), k) {
                    send_eq(band, v);
                }
                if let Some(v) = knobs::kill(ui, id(word), val) {
                    send_eq(band, v);
                }
            }
            if let (_, Some(v)) = knobs::control(ui, style, &filter, k) {
                send_filter(v);
            }
        }
    }
    let avail = dj.cue_available;
    let r = crate::widgets::Key::new(theme::ic::HEADPHONES, egui::vec2(32.0, 28.0))
        .font(theme::label_font(15.0))
        .radius(theme::RADIUS_MD)
        .color(theme::cue())
        .on(d.cue)
        .enabled(avail)
        .ui(ui)
        .on_hover_text(if avail {
            "Kopfhörer-Cue: dieses Deck auf den Vorhör-Ausgang (geht nie auf Sendung)"
        } else {
            "Kein Vorhör-Ausgang: Einstellungen › Audio-Geräte › Vorhören (Kanal 3/4 oder eigenes Gerät)"
        });
    if r.clicked() {
        desk.dj(DjCommand::Cue { side, on: !d.cue });
    }
    let fh = ui.available_height().max(60.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        ui.add_space((ui.available_width() - 30.0 - 4.0 - 12.0).max(0.0) / 2.0);
        let mut f = d.fader;
        let r = crate::widgets::fader(
            ui,
            &mut f,
            0.0..=1.0,
            1.0,
            egui::vec2(30.0, fh),
            crate::widgets::Orient::Vertical,
        )
        .on_hover_text("Kanalfader");
        if r.changed() {
            desk.dj(DjCommand::Fader { side, value: f });
        }
        let (m, _) = ui.allocate_exact_size(egui::vec2(12.0, fh), Sense::hover());
        theme::led_meter_vertical(ui.painter(), m, d.level);
    });
}

fn master(ui: &mut egui::Ui, desk: &RadioDesk, dj: &DjView, size: egui::Vec2, style: KnobStyle) {
    let (outer, _) = ui.allocate_exact_size(size, Sense::hover());
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(outer)
            .layout(egui::Layout::top_down(egui::Align::Center)),
    );
    let ui = &mut child;
    ui.spacing_mut().item_spacing.y = 4.0;
    ui.label(theme::panel_label("MASTER"));
    let w = ui.available_width();
    // Höhe der drei Regler je Stil, der Rest gehört der Master-LED-Kette.
    let k = 30.0;
    let ctl_h = match style {
        KnobStyle::MiniFader => ((size.y - 80.0) / 4.0).clamp(46.0, 90.0),
        KnobStyle::Bar | KnobStyle::Number => knobs::ROW_H,
        KnobStyle::Knob => k + 14.0,
    };
    let rows = match style {
        KnobStyle::MiniFader => 2.0,
        _ => 3.0,
    };
    let mh = (size.y - rows * (ctl_h + 4.0) - 60.0).max(60.0);
    let level = dj.decks[0].level.max(dj.decks[1].level);
    let (m, _) = ui.allocate_exact_size(egui::vec2(14.0, mh), Sense::hover());
    theme::led_meter_vertical(ui.painter(), m, level);
    let master = Ctl {
        id: egui::Id::new("dj-master"),
        value: dj.master,
        range: (0.0, 1.0),
        default: 0.8,
        color: theme::accent(),
        label: "MASTER",
        fmt: Fmt::Pct,
        tip: "Master-Pegel der Party",
    };
    let mix = Ctl {
        id: egui::Id::new("dj-phones-mix"),
        value: dj.phones_mix,
        range: (0.0, 1.0),
        default: 0.5,
        color: theme::cue(),
        label: "MIX",
        fmt: Fmt::Pct,
        tip: "Kopfhörer-Mix: links nur Cue, rechts nur Master",
    };
    let lvl = Ctl {
        id: egui::Id::new("dj-phones-level"),
        value: dj.phones_level,
        range: (0.0, 1.0),
        default: 0.8,
        color: theme::cue(),
        label: "PEGEL",
        fmt: Fmt::Pct,
        tip: "Kopfhörer-Pegel",
    };
    let extent = match style {
        KnobStyle::MiniFader => ctl_h,
        KnobStyle::Bar | KnobStyle::Number => w.min(110.0),
        KnobStyle::Knob => k,
    };
    if let (_, Some(v)) = knobs::control(ui, style, &master, extent) {
        desk.dj(DjCommand::Master { value: v });
    }
    ui.label(theme::panel_label("KOPFHÖRER"));
    let phones = |ui: &mut egui::Ui| {
        if let (_, Some(v)) = knobs::control(ui, style, &mix, extent) {
            desk.dj(DjCommand::Phones {
                mix: v,
                level: dj.phones_level,
            });
        }
        if let (_, Some(v)) = knobs::control(ui, style, &lvl, extent) {
            desk.dj(DjCommand::Phones {
                mix: dj.phones_mix,
                level: v,
            });
        }
    };
    if style == KnobStyle::MiniFader {
        ui.horizontal(|ui| {
            ui.add_space(((w - 2.0 * knobs::MINI_W - 8.0) / 2.0).max(0.0));
            ui.spacing_mut().item_spacing.x = 8.0;
            phones(ui);
        });
    } else {
        phones(ui);
    }
}

/// Stand der Tempo-/Tonart-Analyse eines Decks (DjHead Fassung 7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Analysis {
    /// Kein Titel.
    Empty,
    /// Läuft noch (Chip `cue` mit Kreisel, BPM „---.-“, kein Tonart-Chip).
    Running,
    /// Fertig, aber ohne Tempo: fehlgeschlagen (Chip `warn` + „Neu analysieren“).
    Failed,
    /// Fertig.
    Done,
}

/// Stand aus „Titel liegt“, „Ergebnis da“ und dessen Tempo.
pub(crate) fn analysis_state(loaded: bool, result: Option<Option<f64>>) -> Analysis {
    match (loaded, result) {
        (false, _) => Analysis::Empty,
        (true, None) => Analysis::Running,
        (true, Some(b)) if b.is_some_and(|b| b > 0.0) => Analysis::Done,
        (true, Some(_)) => Analysis::Failed,
    }
}

/// Restzeit, BPM und Camelot-Chip im Deck-Kopf (von rechts nach links), alle gleich hoch
/// (#347). Gibt die Rahmen in dieser Reihenfolge zurück (Chip nur mit Tonart).
fn deck_readouts(
    ui: &mut egui::Ui,
    rest: f64,
    urgent: bool,
    bpm_now: Option<f64>,
    camelot: Option<&str>,
    key_match: bool,
) -> Vec<egui::Rect> {
    let mut out = Vec::with_capacity(3);
    let r =
        seg_lcd(ui, &mmss(rest), super::DECK_LCD_DIGITS, urgent, None).on_hover_text("Restzeit");
    out.push(r.rect);
    let bpm = bpm_now.map_or_else(|| "---.-".to_string(), |b| format!("{b:5.1}"));
    let b = seg_lcd(ui, &bpm, super::DECK_LCD_DIGITS, false, Some("BPM")).on_hover_text(
        "Tempo jetzt (gemessen beim Laden oder aus der Bibliothek, mal Tempo-Fader)",
    );
    out.push(b.rect);
    if let Some(c) = camelot {
        let h = r.rect.height().max(b.rect.height());
        out.push(super::camelot_chip(ui, c, key_match, h).rect);
    }
    out
}

/// Ablage für ein gewähltes Lernziel bis zum nächsten Bild.
fn learn_id() -> egui::Id {
    egui::Id::new("dj-midi-learn")
}

/// Kontextmenü „MIDI lernen“ an einer DJ-Taste (Moderator 05.11.2026).
fn dj_learn_menu(r: &egui::Response, t: woa_io::controller::Target) {
    r.context_menu(|ui| {
        if let Some(t) = crate::controls::learn_menu(ui, t, None) {
            ui.ctx().data_mut(|d| d.insert_temp(learn_id(), Some(t)));
        }
    });
}

/// Zustands-Badge im DJ-Deckkopf. Im DJ-Modus ist jedes Badge gefüllt; ein neutrales bliebe
/// trotz `filled()` dunkel mit Rahmen (#538), darum bekommt „LEER“ die Deckfarbe.
fn deck_state_badge(
    playing: bool,
    loaded: bool,
    deck_color: Color32,
) -> crate::widgets::StateChip<'static> {
    use crate::widgets::{ChipTone, StateChip};
    if playing {
        StateChip::new("LÄUFT", ChipTone::Color(theme::accent())).icon(theme::ic::PLAY)
    } else if loaded {
        StateChip::new("BEREIT · CUE", ChipTone::Cue)
    } else {
        StateChip::new("LEER", ChipTone::Color(deck_color))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #538: Der leere Deck-Zustand ist wie die anderen Badges gefüllt (Deckfarbe, dunkle Schrift).
    #[test]
    fn empty_deck_badge_is_filled_in_deck_color() {
        let _ctx = theme::test_context();
        let col = Color32::from_rgb(200, 40, 120);
        for (playing, loaded) in [(false, false), (false, true), (true, true)] {
            let (fill, _, _, text) = deck_state_badge(playing, loaded, col)
                .filled()
                .colors(false);
            assert_ne!(fill, theme::well(), "playing={playing} loaded={loaded}");
            assert_eq!(text, theme::lcd_bg());
        }
        let (fill, _, _, _) = deck_state_badge(false, false, col).filled().colors(false);
        assert_eq!(fill, col);
    }

    /// Design 8.1: In jedem Regler-Stil stehen alle Regler des Mixers (je Deck Trim, Hi, Mid,
    /// Lo, Filter; Mitte Master, Mix, Pegel) und die drei KILL-Tasten je Deck im Mixer, ohne
    /// sich zu überdecken (Mixerhöhe wie bei 1366 × 768).
    #[test]
    fn every_knob_style_fits_the_mixer() {
        use egui_kittest::kittest::{NodeT, Queryable};
        let dir = tempfile::tempdir().expect("tempdir");
        for style in KnobStyle::ALL {
            let desk = RadioDesk::headless(dir.path());
            let dj = DjView::default();
            let mut h = egui_kittest::Harness::builder()
                .with_size(egui::vec2(MIXER_MIN_W, 400.0))
                .build_ui(move |ui| {
                    let ctx = ui.ctx().clone();
                    let id = egui::Id::new("test_fonts");
                    if ctx.data_mut(|m| m.get_temp::<bool>(id)).is_none() {
                        ctx.data_mut(|m| m.insert_temp(id, true));
                        theme::install_fonts(&ctx);
                        return;
                    }
                    dj_mixer(ui, &desk, &dj, style);
                });
            h.run_steps(3);
            let area = Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(MIXER_MIN_W, 400.0));
            let mut rects = Vec::new();
            for (label, n) in [
                ("TRIM", 2),
                ("HI", 2),
                ("MID", 2),
                ("LO", 2),
                ("FILTER", 2),
                ("MASTER", 1),
                ("MIX", 1),
                ("PEGEL", 1),
                ("KILL", 6),
            ] {
                let found: Vec<Rect> = h
                    .get_all_by_label(label)
                    .filter(|n| n.accesskit_node().role() != egui::accesskit::Role::Label)
                    .map(|n| n.rect())
                    .collect();
                assert_eq!(found.len(), n, "{style:?}: {label}");
                for r in &found {
                    assert!(area.contains_rect(*r), "{style:?}: {label} {r:?} außerhalb");
                }
                rects.extend(found.into_iter().map(|r| (label, r)));
            }
            for (i, (la, a)) in rects.iter().enumerate() {
                for (lb, b) in &rects[i + 1..] {
                    let o = a.intersect(*b);
                    assert!(
                        !o.is_positive() || o.area() < 1.0,
                        "{style:?}: {la} {a:?} überdeckt {lb} {b:?}"
                    );
                }
            }
        }
    }

    /// #347: Restzeit-LCD, BPM-LCD und Camelot-Chip im Deck-Kopf sind gleich hoch.
    #[test]
    fn deck_head_readouts_share_one_height() {
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx);
        let mut rects = Vec::new();
        for _ in 0..2 {
            let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
                ui.horizontal(|ui| {
                    rects = deck_readouts(ui, 125.0, false, Some(124.0), Some("8A"), true);
                });
            });
        }
        assert_eq!(rects.len(), 3);
        let h = rects[0].height();
        for r in &rects {
            assert!((r.height() - h).abs() < 0.5, "{rects:?}");
        }
    }

    #[test]
    fn analysis_runs_fails_without_tempo_and_is_done_with_one() {
        assert_eq!(analysis_state(false, None), Analysis::Empty);
        assert_eq!(analysis_state(true, None), Analysis::Running);
        assert_eq!(analysis_state(true, Some(None)), Analysis::Failed);
        assert_eq!(analysis_state(true, Some(Some(0.0))), Analysis::Failed);
        assert_eq!(analysis_state(true, Some(Some(124.0))), Analysis::Done);
    }

    #[test]
    fn dj_keys_avoid_reserved_and_party_keys() {
        let party = [
            egui::Key::Space,
            egui::Key::N,
            egui::Key::P,
            egui::Key::M,
            egui::Key::R,
            egui::Key::S,
            egui::Key::F,
            egui::Key::F11,
            egui::Key::ArrowUp,
            egui::Key::ArrowDown,
            egui::Key::ArrowLeft,
            egui::Key::ArrowRight,
            egui::Key::Q,
            egui::Key::Escape,
        ];
        let mut seen = HashSet::new();
        for (k, side, _) in DJ_KEYS {
            assert!(
                !crate::radio::RESERVED_KEYS.contains(&k),
                "{k:?} ist reserviert"
            );
            assert!(!party.contains(&k), "{k:?} ist eine Party-Taste");
            assert!(seen.insert(k), "{k:?} doppelt");
            assert!(side < 2);
        }
    }

    #[test]
    fn phase_in_ms_wraps_to_half_a_beat() {
        let d = |phase: f64| DjDeckView {
            phase: Some(phase),
            bpm_now: Some(120.0),
            ..Default::default()
        };
        // 120 BPM: ein Schlag 500 ms.
        assert!((phase_ms(&d(0.1), &d(0.0)).unwrap() - 50.0).abs() < 1e-6);
        assert!((phase_ms(&d(0.95), &d(0.05)).unwrap() + 50.0).abs() < 1e-6);
        assert!(phase_ms(&d(0.1), &DjDeckView::default()).is_none());
    }

    /// #332: Die drei Teile füllen die Breite genau, der Mixer steht mittig, auch im Vollbild.
    #[test]
    fn decks_and_mixer_fill_the_width_with_the_mixer_centered() {
        for width in [1280.0_f32, 1366.0, 1600.0, 1920.0, 2560.0] {
            let ctx = egui::Context::default();
            let mut got = [Rect::NOTHING; 3];
            let mut avail = Rect::NOTHING;
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(width, 1000.0),
                )),
                ..Default::default()
            };
            let _ = ctx.run_ui(input, |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    avail = ui.available_rect_before_wrap();
                    got = three_columns(ui, 500.0, |ui, k| {
                        // Wie Deck und Mixer: Rahmen mit Rand und Innenabstand, füllt den Platz.
                        let (m, frame) = if k == 1 { (10, 22.0) } else { (12, 26.0) };
                        let h = ui.available_height();
                        egui::Frame::new()
                            .stroke(Stroke::new(1.0, Color32::WHITE))
                            .inner_margin(egui::Margin::same(m))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.set_min_height(h - frame);
                            });
                    });
                });
            });
            let [a, mix, b] = got;
            assert!(
                (a.left() - avail.left()).abs() < 1.0,
                "{width}: links {a:?}"
            );
            assert!(
                (b.right() - avail.right()).abs() < 1.0,
                "{width}: rechts Rand {} px",
                avail.right() - b.right()
            );
            assert!(
                (mix.center().x - avail.center().x).abs() < 1.0,
                "{width}: Mixer {} px neben der Mitte",
                mix.center().x - avail.center().x
            );
            assert!(
                (a.width() - b.width()).abs() < 1.0,
                "{width}: Decks gleich breit"
            );
            assert!(
                a.height() <= 500.5 && mix.height() <= 500.5,
                "{width}: nicht höher"
            );
        }
    }

    /// #443: Die Tastenzeilen im Deck sind nie breiter als die Mittelspalte; sonst schöbe sie das
    /// Jogwheel von Deck B aus dem Fenster.
    #[test]
    fn deck_key_rows_never_exceed_their_width() {
        let n = woa_radio::control::dj::LOOP_BEATS.len() as f32;
        for w in [200.0_f32, 240.0, 300.0, 400.0, 600.0] {
            let sw = loop_key_width(w, 90.0, n);
            let loop_row = n * sw + LOOP_GAP + 90.0 + (n + 1.0) * 2.0;
            assert!(
                loop_row <= w + 0.01 || sw <= 12.0,
                "{w}: Loop-Zeile {loop_row}"
            );
            let kw = transport_key_width(w);
            let row = kw * (0.85 + 1.3 + 0.85) + 36.0 + 3.0 * 8.0;
            assert!(row <= w + 0.01, "{w}: Transportzeile {row}");
        }
    }

    /// #443: Jog-Scheibe, Tasten und Tempo-Spalte passen in jedes Deck ab 300 px; die Tasten
    /// behalten mindestens die Breite der Auto-Loop-Zeile, die Scheibe wird kleiner.
    #[test]
    fn deck_body_fits_and_jog_shrinks_first() {
        let mut last_jog = f32::MAX;
        for w in [520.0_f32, 450.0, 400.0, 350.0, 320.0, 313.0] {
            let (jog, keys) = deck_body_widths(w, 260.0);
            assert!(
                jog + keys + TEMPO_W + 24.0 <= w + 0.01,
                "{w}: {jog} + {keys} zu breit"
            );
            assert!(keys >= KEYS_MIN_W - 0.01, "{w}: Tasten {keys}");
            assert!(jog >= JOG_MIN && jog <= last_jog, "{w}: Jog {jog}");
            last_jog = jog;
        }
        let n = woa_radio::control::dj::LOOP_BEATS.len() as f32;
        assert!(
            n * 12.0 + LOOP_GAP + 90.0 + (n + 1.0) * 2.0 <= KEYS_MIN_W,
            "Auto-Loop-Zeile breiter als die kleinste Tastenspalte"
        );
    }

    #[test]
    fn right_click_clears_a_set_hot_cue() {
        // Rechtsklick löscht eine belegte Taste, eine leere bleibt leer (kein Setzen).
        assert_eq!(hot_cue_click(false, true, false, true), Some(true));
        assert_eq!(hot_cue_click(false, true, false, false), None);
        // Links: setzen/anspringen, mit Umschalt löschen.
        assert_eq!(hot_cue_click(true, false, false, true), Some(false));
        assert_eq!(hot_cue_click(true, false, true, true), Some(true));
        assert_eq!(hot_cue_click(false, false, false, true), None);
    }

    #[test]
    fn deck_colors_follow_design() {
        assert_eq!(deck_color(0), theme::accent());
        assert_eq!(deck_color(1), theme::track_color(0));
        assert_eq!(mmss(125.4), "02:05");
    }
}
