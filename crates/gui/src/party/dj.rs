//! DJ-Ansicht der Party (DJ-Paket, Entwurf ScreenPartyDJ): parallele Wellen mit Phase,
//! Deck A · Mixer · Deck B (B gespiegelt), unten Playlist mit BPM/Tonart und „in A/B laden“,
//! Wünsche und die Fernbedienung. Zustand und Befehle stehen in `woa_radio::control::dj`
//! (ohne Fenster testbar); hier wird nur gezeichnet und `DjCommand` geschickt.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke};
use woa_radio::control::{Band, DjCommand, DjDeckView, DjView};

use super::{GAP, PartyView, seg_lcd, wave_columns};
use crate::radio::RadioDesk;
use crate::theme;

/// Höhe der parallelen Wellen (Entwurf DjWaves: 150 px).
pub(super) const WAVES_H: f32 = 150.0;
/// Breite des Mixers zwischen den Decks.
const MIXER_W: f32 = 390.0;
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
        let v = desk.view().clone();
        self.ask_meta(&v, library.db_path());
        let full_h = ui.available_height();
        let win_h = ui.ctx().content_rect().height();
        let waves_h = if win_h < COMPACT_BELOW {
            110.0
        } else {
            WAVES_H
        };
        self.dj_waves(ui, dj, waves_h);
        let rest = ui.available_height();
        let bottom_min = if win_h < COMPACT_BELOW { 175.0 } else { 220.0 };
        let deck_h = (rest - bottom_min - GAP).clamp(300.0, 560.0);
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = GAP;
            // Rahmen zeichnen Rand und Innenabstand außerhalb der Größe: abziehen
            // (Deck 2 × 12 + 2, Mixer 2 × 10 + 2), sonst ragt Deck B aus dem Fenster.
            let w = ((ui.available_width() - MIXER_W - 2.0 * GAP) / 2.0).max(300.0);
            for part in 0..3 {
                let (outer, frame) = if part == 1 {
                    (MIXER_W, 22.0)
                } else {
                    (w, 26.0)
                };
                let size = egui::vec2(outer - frame, deck_h - frame);
                ui.allocate_ui_with_layout(size, egui::Layout::top_down(egui::Align::Min), |ui| {
                    ui.set_min_size(size);
                    match part {
                        0 => self.dj_deck(ui, desk, dj, &v, 0),
                        1 => dj_mixer(ui, desk, dj),
                        _ => self.dj_deck(ui, desk, dj, &v, 1),
                    }
                });
            }
        });
        let _ = full_h;
        let h = ui.available_height();
        let (left_w, right_w) = super::columns_split(ui.available_width());
        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(left_w, h),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.set_width(left_w);
                    self.dj_playlist(ui, desk, dj, &v, h);
                },
            );
            ui.allocate_ui_with_layout(
                egui::vec2(right_w, h),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.set_width(right_w);
                    let side_h = (h - self.remote_h - GAP).max(60.0);
                    egui::ScrollArea::vertical()
                        .id_salt("party-dj-side")
                        .max_height(side_h)
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            ui.spacing_mut().item_spacing = egui::vec2(GAP, GAP);
                            self.wish_card(ui, library, cmds);
                        });
                    let r = ui.scope(|ui| self.remote.ui_small(ui)).response;
                    self.remote_h = r.rect.height();
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
                ui.set_min_height(h);
                // Kante oben in Deckfarbe.
                let top = ui.max_rect().expand(12.0);
                ui.painter().hline(
                    top.x_range().shrink(6.0),
                    top.top() + 1.0,
                    Stroke::new(3.0, col),
                );
                // Kopf.
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 10.0;
                    ui.label(
                        theme::panel_label(if side == 0 { "DECK A" } else { "DECK B" }).color(col),
                    );
                    let (badge, bc) = if d.playing {
                        ("LÄUFT", theme::accent())
                    } else if d.index.is_some() {
                        ("BEREIT · CUE", theme::cue())
                    } else {
                        ("LEER", theme::text_dim())
                    };
                    theme::status_pill(ui, badge, bc, d.index.is_some());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let rest = if d.end > 0.0 {
                            d.end - d.position
                        } else {
                            entry.and_then(|e| e.seconds).unwrap_or(0.0)
                        };
                        let urgent = d.playing && rest < 30.0;
                        seg_lcd(ui, &mmss(rest), 26.0, urgent, None).on_hover_text("Restzeit");
                        let bpm = d
                            .bpm_now
                            .map_or_else(|| "---.-".to_string(), |b| format!("{b:5.1}"));
                        seg_lcd(ui, &bpm, 22.0, false, Some("BPM")).on_hover_text(
                            "Tempo jetzt (aus der Bibliotheksanalyse mal Tempo-Fader)",
                        );
                        if let Some(c) = &d.camelot {
                            let ok = d.key_match;
                            theme::status_pill(
                                ui,
                                c,
                                if ok { theme::ok() } else { theme::text_dim() },
                                ok,
                            );
                        }
                    });
                });
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
                // Plattenteller: so groß wie Höhe und Breite (Tasten brauchen 200 px) erlauben.
                let jog_d = (body_h - 8.0)
                    .min(ui.available_width() - 56.0 - 200.0 - 24.0)
                    .clamp(90.0, 190.0);
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = 12.0;
                    let tempo_w = 56.0;
                    let center_w = (ui.available_width() - jog_d - tempo_w - 24.0).max(200.0);
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
                    "Hot Cue {}: leer = hier setzen, belegt = anspringen, Umschalt+Klick löscht{}",
                    slot + 1,
                    key_hint(side, KeyAction::HotCue(slot))
                ));
                if r.clicked() {
                    let shift = ui.input(|i| i.modifiers.shift);
                    desk.dj(DjCommand::HotCue {
                        side,
                        slot,
                        clear: shift,
                    });
                }
            }
        });
        // Auto-Loop.
        ui.horizontal(|ui| {
            let n = woa_radio::control::dj::LOOP_BEATS.len() as f32;
            let lw = 90.0;
            let sw = ((w - lw - 8.0 - (n - 1.0) * 2.0) / n).max(24.0);
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
            ui.add_space(6.0);
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
            let kw = (w - 3.0 * 8.0 - 40.0) / 3.0;
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
            ui.set_min_height(h - 20.0);
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
            let cols = [w - 300.0, 60.0, 50.0, 70.0, 90.0];
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
    ui.allocate_ui_with_layout(egui::vec2(56.0, h), egui::Layout::top_down(egui::Align::Center), |ui| {
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
            ui.spacing_mut().item_spacing.x = 2.0;
            let mut any = false;
            for (icon, dir, tip) in [
                (theme::ic::CARET_LEFT, -1.0, "Halten: langsamer anschieben"),
                (theme::ic::CARET_RIGHT, 1.0, "Halten: schneller anschieben"),
            ] {
                let r = ui.add(egui::Button::new(icon).min_size(egui::vec2(24.0, 22.0))).on_hover_text(tip);
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

/// Drehregler des DJ-Mixers. `range` ist der Wertebereich, `default` das Ziel des
/// Doppelklicks; bipolare Regler zeichnen den Bogen ab der Mitte.
#[allow(clippy::too_many_arguments)]
fn knob(
    ui: &mut egui::Ui,
    id: egui::Id,
    value: f32,
    range: (f32, f32),
    default: f32,
    size: f32,
    color: Color32,
    label: &str,
) -> (egui::Response, Option<f32>) {
    let (rect, resp) = ui.allocate_exact_size(
        egui::vec2(size + 16.0, size + 14.0),
        Sense::click_and_drag(),
    );
    let (lo, hi) = range;
    let held: Option<f32> = ui.data(|m| m.get_temp(id));
    let mut v = held.unwrap_or(value);
    let mut out = None;
    if resp.dragged() {
        let dy = resp.drag_delta().y;
        let fine = if ui.input(|i| i.modifiers.shift) {
            0.2
        } else {
            1.0
        };
        v = (v - dy / 150.0 * (hi - lo) * fine).clamp(lo, hi);
        ui.data_mut(|m| m.insert_temp(id, v));
        out = Some(v);
    } else if held.is_some() {
        ui.data_mut(|m| m.remove::<f32>(id));
    }
    if resp.double_clicked() {
        v = default;
        out = Some(v);
    }
    let c = egui::pos2(rect.center().x, rect.top() + size / 2.0);
    let r = size / 2.0;
    let p = ui.painter();
    p.circle_filled(c, r, theme::surface());
    p.circle_stroke(c, r, Stroke::new(1.0, theme::border_strong()));
    let frac = |x: f32| ((x - lo) / (hi - lo)).clamp(0.0, 1.0);
    let ang = |f: f32| std::f32::consts::PI * (0.75 + 1.5 * f);
    let pt = |a: f32, rr: f32| egui::pos2(c.x + a.cos() * rr, c.y + a.sin() * rr);
    let (a0, a1) = (ang(frac(default)), ang(frac(v)));
    let n = 24;
    let pts: Vec<Pos2> = (0..=n)
        .map(|i| pt(a0 + (a1 - a0) * i as f32 / n as f32, r + 3.0))
        .collect();
    if (a1 - a0).abs() > 0.01 {
        p.add(egui::Shape::line(pts, Stroke::new(2.5, color)));
    }
    p.line_segment(
        [pt(a1, r * 0.25), pt(a1, r * 0.85)],
        Stroke::new(2.5, color),
    );
    p.text(
        egui::pos2(rect.center().x, rect.bottom() - 5.0),
        egui::Align2::CENTER_CENTER,
        label,
        theme::panel_label_font(),
        theme::text_dim(),
    );
    (resp, out)
}

/// DJ-Mixer: Kanal A, Mitte (Master, Kopfhörer), Kanal B, unten Crossfader mit Kurve.
fn dj_mixer(ui: &mut egui::Ui, desk: &RadioDesk, dj: &DjView) {
    let h = ui.available_height();
    egui::Frame::new()
        .fill(theme::panel())
        .stroke(Stroke::new(1.0, theme::border()))
        .corner_radius(theme::RADIUS_LG)
        .inner_margin(egui::Margin::same(10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.set_min_height(h);
            let strip_h = (h - 20.0 - 50.0).max(200.0);
            let w = ui.available_width();
            let cw = (w - 2.0 * 8.0) / 3.0;
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                channel(ui, desk, dj, 0, egui::vec2(cw, strip_h));
                master(ui, desk, dj, egui::vec2(cw, strip_h));
                channel(ui, desk, dj, 1, egui::vec2(cw, strip_h));
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
                    egui::vec2(w - 150.0, 26.0),
                    crate::widgets::Orient::Horizontal,
                )
                .on_hover_text("Crossfader: links Deck A, rechts Deck B");
                if r.changed() {
                    desk.dj(DjCommand::Crossfader { value: x });
                }
                ui.label(theme::panel_label("B").color(deck_color(1)));
                for (word, hard) in [("weich", false), ("hart", true)] {
                    let on = dj.curve_hard == hard;
                    let r = crate::widgets::Key::new(word, egui::vec2(52.0, 24.0))
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
                    if r.clicked() && !on {
                        desk.dj(DjCommand::Curve { hard });
                    }
                }
            });
        });
}

fn channel(ui: &mut egui::Ui, desk: &RadioDesk, dj: &DjView, side: usize, size: egui::Vec2) {
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
    let fader_min = 90.0;
    let k = ((size.y - fader_min - 70.0) / 5.0 - 16.0).clamp(18.0, 34.0);
    let id = |s: &str| egui::Id::new(("dj-knob", side, s.to_string()));
    let tm = woa_radio::control::dj::TRIM_DB;
    if let (r, Some(v)) = knob(ui, id("trim"), d.trim_db, (-tm, tm), 0.0, k, col, "TRIM") {
        desk.dj(DjCommand::Trim { side, db: v });
        r.on_hover_text(format!("Trim {v:+.1} dB"));
    }
    for (band, val, word) in [
        (Band::Hi, d.eq_hi, "HI"),
        (Band::Mid, d.eq_mid, "MID"),
        (Band::Lo, d.eq_lo, "LO"),
    ] {
        let (r, nv) = knob(ui, id(word), val, (-1.0, 1.0), 0.0, k, col, word);
        if let Some(v) = nv {
            desk.dj(DjCommand::Eq {
                side,
                band,
                value: v,
            });
        }
        r.on_hover_text(format!(
            "EQ {word}: Mitte 0 dB, ganz links Kill, rechts bis +6 dB. Doppelklick = Mitte"
        ));
    }
    let (r, nv) = knob(
        ui,
        id("filter"),
        d.filter,
        (-1.0, 1.0),
        0.0,
        k,
        col,
        "FILTER",
    );
    if let Some(v) = nv {
        desk.dj(DjCommand::Filter { side, value: v });
    }
    r.on_hover_text("Filter: links Tiefpass, rechts Hochpass, Mitte aus");
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
            "Kein Vorhör-Ausgang: Einstellungen → Ausgänge → Vorhören (Kanal 3/4 oder eigenes Gerät)"
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

fn master(ui: &mut egui::Ui, desk: &RadioDesk, dj: &DjView, size: egui::Vec2) {
    let (outer, _) = ui.allocate_exact_size(size, Sense::hover());
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(outer)
            .layout(egui::Layout::top_down(egui::Align::Center)),
    );
    let ui = &mut child;
    ui.spacing_mut().item_spacing.y = 4.0;
    ui.label(theme::panel_label("MASTER"));
    let k = 30.0;
    let mh = (size.y - 3.0 * (k + 14.0) - 60.0).max(60.0);
    let level = dj.decks[0].level.max(dj.decks[1].level);
    let (m, _) = ui.allocate_exact_size(egui::vec2(14.0, mh), Sense::hover());
    theme::led_meter_vertical(ui.painter(), m, level);
    let acc = theme::accent();
    let (r, nv) = knob(
        ui,
        egui::Id::new("dj-master"),
        dj.master,
        (0.0, 1.0),
        0.8,
        k,
        acc,
        "MASTER",
    );
    if let Some(v) = nv {
        desk.dj(DjCommand::Master { value: v });
    }
    r.on_hover_text("Master-Pegel der Party");
    ui.label(theme::panel_label("KOPFHÖRER"));
    let (r, nv) = knob(
        ui,
        egui::Id::new("dj-phones-mix"),
        dj.phones_mix,
        (0.0, 1.0),
        0.5,
        k,
        theme::cue(),
        "MIX",
    );
    if let Some(v) = nv {
        desk.dj(DjCommand::Phones {
            mix: v,
            level: dj.phones_level,
        });
    }
    r.on_hover_text("Kopfhörer-Mix: links nur Cue, rechts nur Master");
    let (r, nv) = knob(
        ui,
        egui::Id::new("dj-phones-level"),
        dj.phones_level,
        (0.0, 1.0),
        0.8,
        k,
        theme::cue(),
        "PEGEL",
    );
    if let Some(v) = nv {
        desk.dj(DjCommand::Phones {
            mix: dj.phones_mix,
            level: v,
        });
    }
    r.on_hover_text("Kopfhörer-Pegel");
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn deck_colors_follow_design() {
        assert_eq!(deck_color(0), theme::accent());
        assert_eq!(deck_color(1), theme::track_color(0));
        assert_eq!(mmss(125.4), "02:05");
    }
}
