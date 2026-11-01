//! Party-Layout (Paket 35): große Now-Playing-Anzeige, nächster Titel, wenige große Tasten,
//! Wunschliste, Vollbild. Zustand und Befehle stehen in `woa_radio::party` (ohne Fenster
//! testbar); hier wird nur gezeichnet, und die Effekte gehen an die laufende Sendung.

use std::path::{Path, PathBuf};

use eframe::egui;
use woa_io::controller::Bus;
use woa_radio::party::{Effect, PartyCommand, PartyLayout, PartyState, Side};

use crate::radio::RadioDesk;
use crate::theme;

/// Der Party-Modus.
#[derive(Default)]
pub struct PartyView {
    pub state: PartyState,
    /// Fernbedienung im WLAN (Paket 36).
    pub remote: crate::remote::RemoteView,
    /// Crossfader wird gerade von Hand gezogen: die zuletzt geschickte Stellung.
    fader_drag: Option<f32>,
    /// Eingabe „+ Wunsch“.
    wish_input: String,
    /// Suchtext der Bibliothek.
    search: String,
    /// Wunsch, für den gerade gesucht wird (Treffer „→ Playlist“ erledigt ihn beim Spielen).
    wish_for: Option<PathBuf>,
    /// Wunschliste geladen bzw. zuletzt gespeicherter Stand.
    wishes_file: Option<PathBuf>,
    wishes_saved: Option<woa_radio::party::Wishlist>,
}

/// Was in der Party gezogen wird.
#[derive(Debug, Clone)]
enum PartyDrag {
    /// Zeile der Playlist.
    Row(usize),
    /// Datei aus der Bibliothekssuche.
    File(PathBuf),
}

/// Symbol je Befehl (Beschriftung der Taste ist das Wort aus `woa_radio::party::buttons`).
pub fn command_icon(
    c: &PartyCommand,
    running: bool,
    repeat: woa_radio::control::Repeat,
) -> &'static str {
    use theme::ic;
    match c {
        PartyCommand::Previous => ic::SKIP_BACK,
        PartyCommand::PlayPause if running => ic::PAUSE,
        PartyCommand::PlayPause | PartyCommand::Play => ic::PLAY,
        PartyCommand::Stop => ic::STOP,
        PartyCommand::Next => ic::SKIP_FORWARD,
        PartyCommand::Restart => ic::ARROW_COUNTER_CLOCKWISE,
        PartyCommand::SeekBack => ic::REWIND,
        PartyCommand::SeekForward => ic::FAST_FORWARD,
        PartyCommand::Mute => ic::SPEAKER_X,
        PartyCommand::VolumeDown => ic::SPEAKER_LOW,
        PartyCommand::VolumeUp => ic::SPEAKER_HIGH,
        PartyCommand::ToggleShuffle => ic::SHUFFLE,
        PartyCommand::CycleRepeat if repeat == woa_radio::control::Repeat::One => ic::REPEAT_ONCE,
        PartyCommand::CycleRepeat => ic::REPEAT,
        PartyCommand::ToggleFullscreen => ic::CORNERS_OUT,
        _ => ic::PLAY,
    }
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
    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        desk: &mut RadioDesk,
        library: &mut crate::library::LibraryWindow,
    ) -> Vec<PartyCommand> {
        self.state.level_db = desk.level_db(Bus::Music);
        self.keep_wishes();
        let view = desk.view().clone();
        let layout = woa_radio::party::layout(&view, &self.state);
        self.take_folder_wishes(desk);
        let cur_path = view
            .current
            .and_then(|i| view.playlist.get(i))
            .map(|e| e.path.clone());
        if let Some(p) = &cur_path {
            self.state.wishes.mark_played(p);
        }
        let xf = view.crossfade_seconds;
        let duo = view.party.clone().unwrap_or_default();
        if duo.sides.iter().any(|s| s.playing) {
            // Fader und Plattenteller folgen dem Mixer.
            ui.ctx().request_repaint();
        }
        let mut cmds = Vec::new();
        let mut folder = false;
        // Ein Raster (Entwurf ScreenParty): 10 px zwischen Karten.
        ui.spacing_mut().item_spacing = egui::vec2(GAP, GAP);
        // Oben: zwei Decks.
        ui.columns(2, |cols| {
            for (k, side) in [Side::Left, Side::Right].into_iter().enumerate() {
                self.deck_card(&mut cols[k], desk, side, &layout, &view, &duo);
            }
        });
        // Darunter links Schalter, Crossfader, Party-Pult und Playlist; rechts Bibliothek,
        // Wunschliste und Fernbedienung.
        let total = ui.available_width();
        let (left_w, right_w) = columns_split(total);
        let h = ui.available_height();
        let key_h = pult_key_height(ui.ctx().content_rect().height());
        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(left_w, h),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.set_width(left_w);
                    self.switches(ui, desk, &duo, xf, &layout, &mut cmds);
                    pult(ui, &layout, view.running, key_h, &mut cmds);
                    folder |= playlist_card(ui, desk, &view, &layout, &mut cmds);
                },
            );
            ui.allocate_ui_with_layout(
                egui::vec2(right_w, h),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.set_width(right_w);
                    egui::ScrollArea::vertical()
                        .id_salt("party-side")
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.spacing_mut().item_spacing = egui::vec2(GAP, GAP);
                            self.search_card(ui, desk, library, &mut cmds);
                            self.wish_card(ui, library, &mut cmds);
                            self.remote.ui(ui);
                        });
                },
            );
        });
        if folder {
            Self::load_folder(desk);
        }
        cmds
    }

    /// Lädt die Wunschliste beim ersten Bild und speichert sie nach jeder Änderung.
    fn keep_wishes(&mut self) {
        let file = self
            .wishes_file
            .get_or_insert_with(|| woa_radio::data_dir().join("party_wishes.json"))
            .clone();
        match &self.wishes_saved {
            None => {
                let l = woa_radio::party::Wishlist::load(&file);
                self.state.wishes = l.clone();
                self.wishes_saved = Some(l);
            }
            Some(saved) if *saved != self.state.wishes => {
                if let Err(e) = self.state.wishes.save(&file) {
                    tracing::warn!(error = %e, "party: wishes not saved");
                }
                self.wishes_saved = Some(self.state.wishes.clone());
            }
            Some(_) => {}
        }
    }

    /// Neue Dateien im Wunschtitel-Ordner (#134) werden Wünsche: einmal je Datei, ohne
    /// Stimme, und ein entfernter Wunsch kommt nicht wieder. Die Datei bleibt immer stehen.
    fn take_folder_wishes(&mut self, desk: &RadioDesk) {
        let Some(list) = desk.wish_folder_list() else {
            return;
        };
        folder_wishes(list, &mut self.state.wishes);
    }

    fn deck_card(
        &self,
        ui: &mut egui::Ui,
        desk: &RadioDesk,
        side: Side,
        l: &PartyLayout,
        v: &woa_radio::station::View,
        duo: &woa_radio::control::PartyDuoView,
    ) {
        let k = side_index(side);
        let s = &duo.sides[k];
        let live = k == duo.live;
        let spin = s.playing;
        let entry = s.index.and_then(|i| v.playlist.get(i));
        // Titel, Interpret, Restzeit und Fortschritt des Decks.
        let (title, artist, time, urgent, progress) = match (live, &l.now) {
            (true, Some(n)) => (
                n.title.clone(),
                n.artist.clone(),
                n.remaining.clone(),
                n.playing && n.remaining.len() <= 4 && n.remaining.starts_with("0:0"),
                n.progress,
            ),
            (true, None) => (
                "Noch kein Titel – Ordner laden".to_string(),
                String::new(),
                "0:00".to_string(),
                false,
                0.0,
            ),
            (false, _) => {
                let rest = if s.playing {
                    (s.end - s.position).max(0.0)
                } else {
                    entry.and_then(|e| e.seconds).unwrap_or(0.0)
                };
                let p = if s.end > 0.0 {
                    (s.position / s.end).clamp(0.0, 1.0) as f32
                } else {
                    0.0
                };
                (
                    entry.map_or_else(|| "– leer –".to_string(), |e| e.title.clone()),
                    String::new(),
                    woa_radio::party::clock(rest),
                    false,
                    p,
                )
            }
        };
        theme::card_frame()
            .inner_margin(egui::Margin::same(16))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                let d = if ui.available_width() > 640.0 {
                    150.0
                } else {
                    110.0
                };
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 18.0;
                    let id = egui::Id::new(if side == Side::Left {
                        "party-jog-l"
                    } else {
                        "party-jog-r"
                    });
                    crate::widgets::jog(ui, id, d, spin)
                        .on_hover_text("Der Plattenteller dreht sich, solange dieses Deck spielt");
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
                        let (badge, col) = deck_badge(live, s.playing, l);
                        ui.horizontal(|ui| {
                            ui.label(
                                theme::panel_label(if side == Side::Left {
                                    "DECK A"
                                } else {
                                    "DECK B"
                                })
                                .color(theme::accent()),
                            )
                            .on_hover_text(if side == Side::Left {
                                "Linkes Deck"
                            } else {
                                "Rechtes Deck"
                            });
                            theme::status_pill(ui, badge, col, false);
                        });
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(theme::display_text(&title).to_uppercase())
                                    .font(theme::label_font(30.0))
                                    .strong()
                                    .color(theme::text()),
                            )
                            .truncate(),
                        );
                        if !artist.is_empty() {
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(theme::display_text(&artist))
                                        .size(16.0)
                                        .color(theme::text_dim()),
                                )
                                .truncate(),
                            );
                        }
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 14.0;
                            theme::lcd_show(ui, |ui| {
                                ui.label(
                                    egui::RichText::new(&time)
                                        .font(theme::seg_font(34.0))
                                        .color(if urgent {
                                            theme::on_air()
                                        } else {
                                            theme::lcd_ink()
                                        }),
                                )
                                .on_hover_text("Restzeit des Titels");
                            });
                            let (icon, word) = if s.playing {
                                (theme::ic::PAUSE, "Pause")
                            } else {
                                (theme::ic::PLAY, "Play")
                            };
                            let fill = if s.playing { Fill::Accent } else { Fill::None };
                            let r = ui.add_enabled_ui(s.index.is_some(), |ui| {
                                big_key(ui, icon, word, fill, egui::vec2(150.0, 56.0), s.playing)
                            });
                            if r.inner
                                .on_hover_text(
                                    "Startet bzw. hält nur dieses Deck; geblendet wird mit dem Crossfader",
                                )
                                .clicked()
                            {
                                desk.party_side(k, !s.playing);
                            }
                        });
                    });
                });
                ui.add(
                    egui::ProgressBar::new(progress)
                        .desired_height(10.0)
                        .fill(theme::accent()),
                );
            });
    }

    /// Schalter Auto-Übergang und Zufall links neben dem Crossfader (Entwurf PartyControls):
    /// eingeschaltet = in `accent-2` gefüllt, sonst neutral.
    fn switches(
        &mut self,
        ui: &mut egui::Ui,
        desk: &RadioDesk,
        duo: &woa_radio::control::PartyDuoView,
        xf: f64,
        l: &PartyLayout,
        cmds: &mut Vec<PartyCommand>,
    ) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 12.0;
            let size = egui::vec2(0.0, SWITCH_H);
            if big_key(
                ui,
                AUTO_ICON,
                "Auto-Übergang",
                switch_fill(duo.auto),
                size,
                false,
            )
            .on_hover_text(
                "An: kurz vor dem Titelende startet das andere Deck und der Fader fährt hinüber. \
                     Aus: jedes Deck selbst starten und von Hand blenden.",
            )
            .clicked()
            {
                desk.set_party_auto(!duo.auto);
            }
            if big_key(
                ui,
                theme::ic::SHUFFLE,
                "Zufall",
                switch_fill(l.shuffle),
                size,
                false,
            )
            .on_hover_text("Zufällige Reihenfolge (Taste S)")
            .clicked()
            {
                cmds.push(PartyCommand::ToggleShuffle);
            }
            self.crossfader(ui, desk, duo, xf);
        });
    }

    /// Crossfader: freie, stufenlose Blende von Hand (Constant-Power im Mixer). Mit
    /// „Auto-Übergang“ fährt er am Titelende von selbst; die Hand gewinnt immer.
    fn crossfader(
        &mut self,
        ui: &mut egui::Ui,
        desk: &RadioDesk,
        duo: &woa_radio::control::PartyDuoView,
        xf: f64,
    ) {
        {
            let w = ui.available_width().max(100.0);
            let (outer, resp) =
                ui.allocate_exact_size(egui::vec2(w, SWITCH_H), egui::Sense::click_and_drag());
            // Oben die Bahn, darunter die Skala A | B.
            let rect = egui::Rect::from_min_max(
                outer.min + egui::vec2(10.0, 0.0),
                egui::pos2(outer.right() - 10.0, outer.top() + 30.0),
            );
            if (resp.dragged() || resp.clicked())
                && let Some(p) = resp.interact_pointer_pos()
            {
                let pos = fader_pos_at(p.x, rect.left(), rect.width());
                if self.fader_drag != Some(pos) {
                    self.fader_drag = Some(pos);
                    desk.party_fader(pos);
                }
            }
            let held = resp.dragged();
            if !held {
                self.fader_drag = None;
            }
            let pos = self.fader_drag.unwrap_or(duo.fader);
            let p = ui.painter();
            let track = egui::Rect::from_center_size(rect.center(), egui::vec2(rect.width(), 4.0));
            p.rect_filled(track, 2, theme::well());
            p.rect_stroke(
                track,
                2,
                egui::Stroke::new(1.0, theme::border()),
                egui::StrokeKind::Outside,
            );
            let x = rect.left() + pos * rect.width();
            let knob = egui::Rect::from_center_size(
                egui::pos2(x, rect.center().y),
                egui::vec2(28.0, 28.0),
            );
            p.rect_filled(knob, 4, theme::surface());
            p.rect_stroke(
                knob,
                4,
                egui::Stroke::new(1.0, theme::border_strong()),
                egui::StrokeKind::Inside,
            );
            p.vline(
                knob.center().x,
                knob.shrink(6.0).y_range(),
                egui::Stroke::new(2.0, theme::accent()),
            );
            // Skala A | B unter der Bahn.
            let y = outer.bottom() - 6.0;
            let scale = theme::panel_label_font();
            for (sx, t) in [
                (rect.left(), "A"),
                (rect.center().x, "|"),
                (rect.right(), "B"),
            ] {
                p.text(
                    egui::pos2(sx, y),
                    egui::Align2::CENTER_CENTER,
                    t,
                    scale.clone(),
                    theme::text_dim(),
                );
            }
            resp.on_hover_text(format!(
                "Crossfader: freie Blende zwischen Deck A und B, in der Mitte beide gleich laut. \
                 Mit Auto-Übergang fährt er am Titelende in {xf:.0} s von selbst; anfassen hält ihn."
            ));
        }
    }

    /// Bibliothekssuche (dieselbe Suche wie im Fenster „Bibliothek“).
    fn search_card(
        &mut self,
        ui: &mut egui::Ui,
        desk: &mut RadioDesk,
        library: &mut crate::library::LibraryWindow,
        cmds: &mut Vec<PartyCommand>,
    ) {
        side_card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(theme::panel_label("BIBLIOTHEK").color(theme::accent_2()));
                if let Some(w) = &self.wish_for {
                    ui.label(
                        egui::RichText::new(format!(
                            "für Wunsch: {}",
                            woa_radio::show::title_of(w).trim_start_matches("wunsch:")
                        ))
                        .small()
                        .color(theme::cue()),
                    );
                }
            });
            let r = ui.add(
                egui::TextEdit::singleline(&mut self.search)
                    .hint_text("Titel oder Interpret suchen …")
                    .desired_width(f32::INFINITY),
            );
            if r.changed() {
                library.set_query(&self.search);
                library.refresh();
            }
            if self.search.trim().is_empty() {
                return;
            }
            let hits: Vec<_> = library.results().iter().take(8).cloned().collect();
            if hits.is_empty() {
                ui.label(egui::RichText::new("Kein Treffer.").color(theme::text_dim()));
            }
            for t in hits {
                let path = PathBuf::from(&t.path);
                ui.horizontal(|ui| {
                    ui.dnd_drag_source(
                        egui::Id::new(("party-hit", t.id)),
                        PartyDrag::File(path.clone()),
                        |ui| {
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(&t.title)
                                        .size(16.0)
                                        .color(theme::text()),
                                )
                                .truncate()
                                .selectable(false),
                            );
                        },
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if crate::widgets::ctl_key(
                            ui,
                            theme::ic::QUEUE,
                            "→ Playlist",
                            crate::widgets::Tone::Normal,
                            false,
                            0.0,
                        )
                        .on_hover_text("Hinter dem laufenden Titel einreihen (auch ziehen)")
                        .clicked()
                        {
                            match self.wish_for.take() {
                                Some(wish) => cmds.push(PartyCommand::QueueWish {
                                    wish,
                                    file: path.clone(),
                                }),
                                None => desk.party_effect(Effect::InsertNext(path.clone())),
                            }
                        }
                    });
                });
            }
        });
    }

    fn wish_card(
        &mut self,
        ui: &mut egui::Ui,
        library: &mut crate::library::LibraryWindow,
        cmds: &mut Vec<PartyCommand>,
    ) {
        side_card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            let open = self.state.wishes.items().iter().filter(|w| !w.done).count();
            ui.horizontal(|ui| {
                ui.label(theme::panel_label("WUNSCHLISTE").color(theme::accent_2()));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    theme::status_pill(
                        ui,
                        &format!("{open} {}", if open == 1 { "Wunsch" } else { "Wünsche" }),
                        theme::text_dim(),
                        false,
                    );
                });
            });
            ui.horizontal(|ui| {
                let r = ui.add(
                    egui::TextEdit::singleline(&mut self.wish_input)
                        .hint_text("Wer: Titel / Interpret")
                        .desired_width((ui.available_width() - 150.0).max(80.0)),
                );
                let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                let add = big_key(
                    ui,
                    theme::ic::PLUS,
                    "Wunsch",
                    Fill::None,
                    egui::vec2(0.0, 44.0),
                    false,
                )
                .on_hover_text("Wunsch eintragen (auch Eingabetaste)")
                .clicked();
                let text = self.wish_input.trim().to_string();
                if (enter || add) && !text.is_empty() {
                    cmds.push(PartyCommand::Wish {
                        path: woa_radio::party::free_wish_path(&text),
                        title: text,
                    });
                    self.wish_input.clear();
                }
            });
            ui.add_space(6.0);
            let items: Vec<_> = self.state.wishes.items().to_vec();
            if items.is_empty() {
                ui.vertical_centered(|ui| {
                    ui.add_space(12.0);
                    ui.label(
                        egui::RichText::new(theme::ic::HEART)
                            .size(20.0)
                            .color(theme::text_dim()),
                    );
                    ui.label(
                        egui::RichText::new("Noch keine Wünsche.")
                            .size(14.0)
                            .color(theme::text_dim()),
                    );
                    ui.label(
                        egui::RichText::new(
                            "Handys im WLAN können wünschen, sobald die Fernbedienung an ist.",
                        )
                        .size(12.0)
                        .color(theme::text_dim()),
                    );
                    ui.add_space(12.0);
                });
            }
            for (i, w) in items.iter().enumerate() {
                // Eine Zeile je Wunsch: rechts die Nebenaktionen als Symbol-Tasten mit
                // Tooltip, links der Titel (Klick sucht ihn in der Bibliothek).
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    let free = woa_radio::party::is_free_wish(&w.path);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if icon_key(ui, theme::ic::X, Fill::None)
                            .on_hover_text("Entfernen")
                            .clicked()
                        {
                            cmds.push(PartyCommand::DropWish(w.path.clone()));
                        }
                        let (icon, tip) = if w.done {
                            (theme::ic::ARROW_COUNTER_CLOCKWISE, "Wieder offen")
                        } else {
                            (theme::ic::CHECK, "Erledigt")
                        };
                        if icon_key(ui, icon, Fill::None).on_hover_text(tip).clicked() {
                            cmds.push(PartyCommand::MarkWish {
                                path: w.path.clone(),
                                done: !w.done,
                            });
                        }
                        if icon_key(ui, theme::ic::QUEUE, switch_fill(w.queued.is_some()))
                            .on_hover_text(if free {
                                "→ Playlist: freier Wunsch, erst in der Bibliothek einen Titel wählen"
                            } else {
                                "→ Playlist: als nächsten Titel einreihen"
                            })
                            .clicked()
                        {
                            if free {
                                self.search_for(w, library);
                            } else {
                                cmds.push(PartyCommand::PlayWish(w.path.clone()));
                            }
                        }
                        if icon_key(ui, theme::ic::MAGNIFYING_GLASS, Fill::None)
                            .on_hover_text("In der Bibliothek suchen")
                            .clicked()
                        {
                            self.search_for(w, library);
                        }
                        if w.votes > 1 {
                            theme::status_pill(ui, &format!("×{}", w.votes), theme::accent_2(), false);
                        }
                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            let mut t = egui::RichText::new(format!("{}. {}", i + 1, w.title))
                                .size(16.0);
                            t = if w.done {
                                t.strikethrough().color(theme::text_dim())
                            } else {
                                t.color(theme::text())
                            };
                            if ui
                                .add(egui::Label::new(t).truncate().sense(egui::Sense::click()))
                                .on_hover_text("Klick: in der Bibliothek suchen")
                                .clicked()
                            {
                                self.search_for(w, library);
                            }
                        });
                    });
                });
            }
        });
    }

    /// Füllt die Bibliothekssuche mit dem Wunsch vor (bei „Wer: Titel“ nur der Titel).
    fn search_for(
        &mut self,
        w: &woa_radio::party::Wish,
        library: &mut crate::library::LibraryWindow,
    ) {
        let q = w
            .title
            .split_once(':')
            .map_or(w.title.as_str(), |(_, t)| t)
            .trim();
        self.search = q.to_string();
        self.wish_for = Some(w.path.clone());
        library.set_query(&self.search);
        library.refresh();
    }

    /// Musikordner laden (Dialog); hängt alle Titel hinten an die Playlist.
    pub fn load_folder(desk: &RadioDesk) {
        if let Some(dir) = crate::ordner::dialog_musik().pick_folder() {
            let files = audio_files_in(&dir, 3);
            if files.is_empty() {
                tracing::info!(dir = %dir.display(), "party: no audio files in folder");
            } else {
                desk.append_files(files);
            }
        }
    }
}

impl RadioDesk {
    /// Aktive Playlist wählen (Party-Ansicht).
    fn party_select_playlist(&self, i: usize) {
        self.with(move |c| {
            c.select_playlist(i);
        });
    }

    /// Eintrag verschieben (Party-Ansicht).
    fn party_move(&self, from: usize, to: usize) {
        self.with(move |c| {
            c.move_entry(from, to);
        });
    }

    /// Datei an Stelle `at` einfügen (`None`: ans Ende).
    fn party_insert(&self, path: PathBuf, at: Option<usize>) {
        self.with(move |c| {
            c.add(&[path], at);
        });
    }
}

/// Die aktive Playlist: Auswahl, laufender und nächster Titel, Ziehen zum Umordnen und
/// Treffer der Bibliothek hineinziehen. Im Kopf „Ordner laden“ und Vollbild (Entwurf
/// ScreenParty); verwaltet wird im Fenster „Verwalten“. Gibt `true` bei „Ordner laden“.
fn playlist_card(
    ui: &mut egui::Ui,
    desk: &RadioDesk,
    v: &woa_radio::station::View,
    l: &PartyLayout,
    cmds: &mut Vec<PartyCommand>,
) -> bool {
    let mut folder = false;
    theme::card_frame().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.set_min_height(ui.available_height());
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            ui.label(theme::panel_label("PLAYLIST"));
            let name = v
                .playlists
                .get(v.active_playlist)
                .cloned()
                .unwrap_or_default();
            egui::ComboBox::from_id_salt("party-playlist")
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
                })
                .response
                .on_hover_text("Playlist wählen");
            ui.label(
                egui::RichText::new(format!("{} Titel", v.playlist.len()))
                    .monospace()
                    .size(12.0)
                    .color(theme::text_dim()),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let (icon, tip) = if l.fullscreen {
                    (theme::ic::CORNERS_IN, "Vollbild aus (Taste F oder F11)")
                } else {
                    (theme::ic::CORNERS_OUT, "Vollbild (Taste F oder F11)")
                };
                if icon_key(ui, icon, switch_fill(l.fullscreen))
                    .on_hover_text(tip)
                    .clicked()
                {
                    cmds.push(PartyCommand::ToggleFullscreen);
                }
                if small_key(ui, theme::ic::FOLDER_OPEN, "Ordner laden")
                    .on_hover_text("Ordner als Playlist laden (hängt alle Titel hinten an)")
                    .clicked()
                {
                    folder = true;
                }
            });
        });
        let next = v
            .current
            .or((!v.running).then_some(v.cue))
            .and_then(|i| (i + 1..v.playlist.len()).find(|&k| v.playlist[k].voice.is_none()));
        let released = ui.input(|i| i.pointer.any_released());
        let pointer = ui.ctx().pointer_latest_pos();
        let mut dropped: Option<(PartyDrag, Option<usize>)> = None;
        let mut rows = Vec::new();
        let scroll = egui::ScrollArea::vertical()
            .id_salt("party-playlist-rows")
            .auto_shrink([false, false])
            .max_height(ui.available_height().max(ROW_H * 3.0))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.spacing_mut().item_spacing.y = 0.0;
                if v.playlist.is_empty() {
                    ui.label(
                        egui::RichText::new(
                            "Leer – Ordner laden oder Titel aus der Suche hierher ziehen.",
                        )
                        .color(theme::text_dim()),
                    );
                }
                for (i, e) in v.playlist.iter().enumerate() {
                    let on_air = v.current == Some(i);
                    let tag = if on_air {
                        Some(("läuft", theme::accent()))
                    } else if next == Some(i) {
                        Some(("als Nächstes", theme::accent_2()))
                    } else {
                        None
                    };
                    let r = ui.dnd_drag_source(
                        egui::Id::new(("party-row", i)),
                        PartyDrag::Row(i),
                        |ui| playlist_row(ui, i, e, on_air, tag),
                    );
                    rows.push(r.response.rect);
                }
            });
        let area = scroll.inner_rect;
        if released
            && let Some(p) = pointer
            && area.contains(p)
            && let Some(payload) = egui::DragAndDrop::take_payload::<PartyDrag>(ui.ctx())
        {
            let at = rows.iter().position(|r| r.contains(p));
            dropped = Some(((*payload).clone(), at));
        }
        match dropped {
            Some((PartyDrag::Row(from), Some(to))) if from != to => desk.party_move(from, to),
            Some((PartyDrag::File(path), at)) => desk.party_insert(path, at),
            _ => {}
        }
    });
    folder
}

/// Eine Zeile der Playlist: Nummer, Titel, Marke („läuft“, „als Nächstes“), Dauer. Neutral;
/// nur der laufende Titel ist leicht in der Modusfarbe getönt und hat eine Kante.
fn playlist_row(
    ui: &mut egui::Ui,
    i: usize,
    e: &woa_radio::station::ViewEntry,
    on_air: bool,
    tag: Option<(&str, egui::Color32)>,
) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), ROW_H),
        egui::Sense::hover(),
    );
    let p = ui.painter();
    if on_air {
        p.rect_filled(rect, 0, theme::mix(theme::panel(), theme::accent(), 0.12));
        p.rect_filled(
            egui::Rect::from_min_size(rect.min, egui::vec2(3.0, rect.height())),
            0,
            theme::accent(),
        );
    }
    p.hline(
        rect.x_range(),
        rect.bottom() - 0.5,
        egui::Stroke::new(1.0, theme::border()),
    );
    let inner = rect.shrink2(egui::vec2(10.0, 0.0));
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::right_to_left(egui::Align::Center)),
        |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let dur = e.seconds.map(woa_radio::party::clock).unwrap_or_default();
            ui.label(
                egui::RichText::new(dur)
                    .monospace()
                    .size(13.0)
                    .color(theme::text()),
            );
            if let Some((t, c)) = tag {
                ui.label(
                    egui::RichText::new(t.to_uppercase())
                        .font(theme::panel_label_font())
                        .color(c),
                );
            }
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.add_sized(
                    egui::vec2(28.0, ROW_H),
                    egui::Label::new(
                        egui::RichText::new(format!("{}", i + 1))
                            .monospace()
                            .size(12.0)
                            .color(theme::text_dim()),
                    )
                    .selectable(false),
                );
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(theme::display_text(&e.title))
                            .size(14.0)
                            .color(theme::text()),
                    )
                    .truncate()
                    .selectable(false),
                );
            });
        },
    );
}

/// Titel, Künstler, Restzeit im Deck mit dem laufenden Titel.
/// Übernimmt die fertig kopierten Dateien des Wunschordners in die Wunschliste; gibt die Zahl
/// der neuen Wünsche zurück.
pub fn folder_wishes(
    list: crate::wishes::WishList,
    wishes: &mut woa_radio::party::Wishlist,
) -> usize {
    list.files
        .into_iter()
        .filter(|f| !f.copying)
        .filter(|f| wishes.add_folder_file(f.path.clone(), &wish_title(&f.path, &f.name)))
        .count()
}

/// Anzeigename eines Wunschordner-Titels: „Interpret – Titel“ aus den Tags (nur gelesen, nie
/// dekodiert); fehlt eines davon oder ist die Datei nicht lesbar, der Dateiname ohne Endung.
fn wish_title(path: &std::path::Path, name: &str) -> String {
    match woa_library::tags::read_fields(path) {
        Ok((f, _)) if !f.artist.trim().is_empty() && !f.title.trim().is_empty() => {
            format!("{} – {}", f.artist.trim(), f.title.trim())
        }
        _ => name.to_string(),
    }
}

/// Index der Seite im Mixer (0 links, 1 rechts).
fn side_index(side: Side) -> usize {
    match side {
        Side::Left => 0,
        Side::Right => 1,
    }
}

/// Stellung des Crossfaders unter dem Zeiger (0 links, 1 rechts), auf Hundertstel gerundet,
/// damit nicht jedes Pixelzittern einen Befehl schickt.
fn fader_pos_at(x: f32, left: f32, width: f32) -> f32 {
    let p = ((x - left) / width.max(1.0)).clamp(0.0, 1.0);
    (p * 100.0).round() / 100.0
}

/// Abstand zwischen Karten und Tasten (ein Raster, Entwurf „Arbeitsfläche“).
const GAP: f32 = 10.0;
/// Höhe der Schalter neben dem Crossfader.
const SWITCH_H: f32 = 48.0;
/// Höhe einer Playlist-Zeile.
const ROW_H: f32 = 32.0;
/// Breite der rechten Spalte (Bibliothek, Wunschliste, Fernbedienung) im Entwurf.
const SIDE_W: f32 = 520.0;
/// Symbol des Schalters „Auto-Übergang“.
const AUTO_ICON: &str = theme::ic::ARROWS_LEFT_RIGHT;

/// Füllung einer Party-Taste. Der Zustand steht nur in der Füllung (Entwurf PartyControls):
/// läuft = `accent`, eingeschaltet = `accent-2`, sonst neutral. Nie Grün.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fill {
    None,
    Accent,
    Alt,
}

impl Fill {
    fn color(self) -> Option<egui::Color32> {
        match self {
            Fill::None => None,
            Fill::Accent => Some(theme::accent()),
            Fill::Alt => Some(theme::accent_2()),
        }
    }
}

/// Füllung eines Schalters: an = `accent-2`.
fn switch_fill(on: bool) -> Fill {
    if on { Fill::Alt } else { Fill::None }
}

/// Füllung einer Pult-Taste: Play/Pause bei laufender Sendung in `accent`, eingeschaltete
/// Funktionen (Stumm, Wiederholen, Zufall) in `accent-2`.
fn key_fill(c: &PartyCommand, active: bool, running: bool) -> Fill {
    match c {
        PartyCommand::PlayPause | PartyCommand::Play => {
            if running {
                Fill::Accent
            } else {
                Fill::None
            }
        }
        _ => switch_fill(active),
    }
}

/// Party-Großtaste (BigKey): Symbol und Wort in Versalien, große Radien. Gefüllte Tasten
/// leuchten; mit `pulse` pulsiert die gefüllte Taste ruhig (nicht bei reduzierter Bewegung).
/// `size.x` ist die Wunschbreite, nie schmaler als die Beschriftung.
fn big_key(
    ui: &mut egui::Ui,
    icon: &str,
    word: &str,
    fill: Fill,
    size: egui::Vec2,
    pulse: bool,
) -> egui::Response {
    let label = format!("{icon}  {}", word.to_uppercase());
    let font = theme::label_font((size.y * 0.3).clamp(15.0, 19.0));
    let text_w = ui
        .painter()
        .layout_no_wrap(label.clone(), font.clone(), egui::Color32::WHITE)
        .size()
        .x;
    let w = size.x.max(text_w + 32.0);
    let mut col = fill.color().unwrap_or_else(theme::accent);
    if pulse && fill != Fill::None && crate::widgets::motion(ui.ctx()) {
        let t = ui.input(|i| i.time);
        let k = (0.5 + 0.5 * (t * std::f64::consts::TAU * 2.0).sin()) as f32;
        col = theme::mix(col, theme::accent_strong(), 0.4 * k);
        ui.ctx().request_repaint();
    }
    crate::widgets::Key::new(&label, egui::vec2(w, size.y))
        .font(font)
        .color(col)
        .radius(theme::RADIUS_PARTY)
        .on(fill != Fill::None)
        .ui(ui)
}

/// Symbol-Taste 32 × 32 für Nebenaktionen (immer mit Tooltip).
fn icon_key(ui: &mut egui::Ui, icon: &str, fill: Fill) -> egui::Response {
    crate::widgets::Key::new(icon, egui::vec2(32.0, 32.0))
        .font(egui::FontId::proportional(16.0))
        .color(fill.color().unwrap_or_else(theme::accent))
        .radius(theme::RADIUS_MD)
        .on(fill != Fill::None)
        .ui(ui)
}

/// Kleine Taste mit Symbol und Wort (32 px), etwa „Ordner laden“ im Playlist-Kopf.
fn small_key(ui: &mut egui::Ui, icon: &str, word: &str) -> egui::Response {
    let label = format!("{icon} {}", word.to_uppercase());
    let font = theme::label_font(13.0);
    let w = ui
        .painter()
        .layout_no_wrap(label.clone(), font.clone(), egui::Color32::WHITE)
        .size()
        .x
        + 20.0;
    crate::widgets::Key::new(&label, egui::vec2(w, 32.0))
        .font(font)
        .radius(theme::RADIUS_MD)
        .ui(ui)
}

/// Karte der rechten Spalte (Entwurf: 14 px Innenabstand).
fn side_card() -> egui::Frame {
    theme::card_frame().inner_margin(egui::Margin::same(14))
}

/// Breiten der linken und rechten Spalte unter den Decks. Rechts 520 px wie im Entwurf, in
/// schmalen Fenstern ein Drittel, aber nie unter 320 px.
fn columns_split(total: f32) -> (f32, f32) {
    let right = (total * 0.3).clamp(320.0, SIDE_W).min(total * 0.5);
    ((total - right - GAP).max(0.0), right)
}

/// Höhe der Pult-Tasten: 64 px wie im Entwurf, in niedrigen Fenstern (unter 900 px) 48 px,
/// damit die Playlist sichtbar bleibt (Party 48–64 px).
fn pult_key_height(window_h: f32) -> f32 {
    if window_h >= 900.0 { 64.0 } else { 48.0 }
}

/// Die drei Gruppen des Party-Pults (Entwurf PartyControls); `None` ist die Lautstärke-Anzeige.
fn pult_groups() -> [(&'static str, [Option<PartyCommand>; 4]); 3] {
    use PartyCommand as C;
    [
        (
            "ABSPIELEN",
            [
                Some(C::Previous),
                Some(C::PlayPause),
                Some(C::Stop),
                Some(C::Next),
            ],
        ),
        (
            "SPRINGEN",
            [
                Some(C::Restart),
                Some(C::SeekBack),
                Some(C::SeekForward),
                Some(C::CycleRepeat),
            ],
        ),
        (
            "LAUTSTÄRKE",
            [Some(C::VolumeDown), None, Some(C::VolumeUp), Some(C::Mute)],
        ),
    ]
}

/// Party-Pult: alle Befehle als Großtasten in drei Gruppen, vier Spalten, eine Tastengröße.
/// Wort und Zustand jeder Taste kommen aus `layout.buttons` (`woa_radio::party::buttons`).
fn pult(
    ui: &mut egui::Ui,
    l: &PartyLayout,
    running: bool,
    key_h: f32,
    cmds: &mut Vec<PartyCommand>,
) {
    let w = ((ui.available_width() - 3.0 * GAP) / 4.0).max(60.0);
    let all: Vec<&woa_radio::party::PartyButton> = l.buttons.iter().flatten().collect();
    for (title, slots) in pult_groups() {
        ui.spacing_mut().item_spacing.y = 6.0;
        group_title(ui, title);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = GAP;
            for slot in slots {
                match slot {
                    None => level_display(ui, l.level_db, egui::vec2(w, key_h)),
                    Some(c) => {
                        let Some(b) = all.iter().find(|b| b.command == c) else {
                            ui.add_space(w);
                            continue;
                        };
                        let fill = key_fill(&b.command, b.active, running);
                        let icon = command_icon(&b.command, running, l.repeat);
                        let pulse = matches!(b.command, PartyCommand::PlayPause) && running;
                        if big_key(ui, icon, b.label, fill, egui::vec2(w, key_h), pulse)
                            .on_hover_text(command_tip(&b.command))
                            .clicked()
                        {
                            cmds.push(b.command.clone());
                        }
                    }
                }
            }
        });
        ui.spacing_mut().item_spacing.y = GAP;
    }
}

/// Tooltip je Pult-Taste (mit Tastenkürzel, wo es eins gibt).
fn command_tip(c: &PartyCommand) -> &'static str {
    match c {
        PartyCommand::Previous => "Zurück (← oder P)",
        PartyCommand::PlayPause => "Play/Pause (Leertaste)",
        PartyCommand::Stop => "Stopp",
        PartyCommand::Next => "Weiter (→ oder N)",
        PartyCommand::Restart => "Titel von vorn",
        PartyCommand::SeekBack => "10 Sekunden zurück",
        PartyCommand::SeekForward => "10 Sekunden vor",
        PartyCommand::CycleRepeat => "Wiederholen: aus → Playlist → Titel (Taste R)",
        PartyCommand::VolumeDown => "Leiser (↓)",
        PartyCommand::VolumeUp => "Lauter (↑)",
        PartyCommand::Mute => "Stumm (Taste M)",
        _ => "",
    }
}

/// Gruppentitel im Pult: `panel-label` mit Linie bis zum Rand.
fn group_title(ui: &mut egui::Ui, title: &str) {
    ui.horizontal(|ui| {
        let r = ui.label(theme::panel_label(title));
        let y = r.rect.center().y;
        let x0 = r.rect.right() + 8.0;
        let x1 = ui.max_rect().right();
        if x1 > x0 {
            ui.painter()
                .hline(x0..=x1, y, egui::Stroke::new(1.0, theme::border()));
        }
    });
}

/// Lautstärke-Anzeige im Pult: LCD mit Balken und dB (ersetzt „Lautstärke +0 dB“ als Text).
fn level_display(ui: &mut egui::Ui, db: f64, size: egui::Vec2) {
    use woa_radio::mixer::{LEVEL_MAX_DB, LEVEL_MIN_DB};
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::hover());
    let p = ui.painter();
    p.rect(
        rect,
        5,
        theme::lcd_bg(),
        egui::Stroke::new(1.0, theme::border()),
        egui::StrokeKind::Inside,
    );
    let inner = rect.shrink2(egui::vec2(12.0, 10.0));
    let txt = format!("{db:+.0} dB");
    let g = p.layout_no_wrap(txt.clone(), theme::label_font(22.0), theme::lcd_ink());
    let tw = g.size().x;
    p.galley(
        egui::pos2(inner.right() - tw, inner.center().y - g.size().y / 2.0),
        g,
        theme::lcd_ink(),
    );
    p.text(
        inner.left_top(),
        egui::Align2::LEFT_TOP,
        "PARTY",
        theme::panel_label_font(),
        theme::lcd_text(),
    );
    let bar_w = (inner.width() - tw - 16.0).max(10.0);
    let bar = egui::Rect::from_min_size(
        egui::pos2(inner.left(), inner.bottom() - 8.0),
        egui::vec2(bar_w, 8.0),
    );
    p.rect_filled(bar, 2, theme::led_off());
    let f = ((db - LEVEL_MIN_DB) / (LEVEL_MAX_DB - LEVEL_MIN_DB)).clamp(0.0, 1.0) as f32;
    p.rect_filled(
        egui::Rect::from_min_size(bar.min, egui::vec2(bar.width() * f, bar.height())),
        2,
        theme::accent(),
    );
    resp.on_hover_text(format!("Lautstärke der Party: {txt}"));
}

/// Anzeige oben am Deck: hörbar (JETZT, PAUSE, BEREIT) oder nicht (LÄUFT MIT, VORGELADEN).
/// Farben ohne Grün: läuft = `accent`, wartet = `accent-2`, Pause und Mitlaufen = `warn`.
fn deck_badge(live: bool, playing: bool, l: &PartyLayout) -> (&'static str, egui::Color32) {
    if live {
        match &l.now {
            Some(n) if n.playing => ("JETZT", theme::accent()),
            Some(n) if n.paused => ("PAUSE", theme::warn()),
            _ => ("BEREIT", theme::accent_2()),
        }
    } else if playing {
        ("LÄUFT MIT", theme::warn())
    } else {
        ("VORGELADEN", theme::accent_2())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Party-Pult nach Entwurf PartyControls: drei Gruppen (Abspielen, Springen, Lautstärke),
    /// vier Spalten, eine Tastenhöhe, jede Taste mit Symbol und Wort. Zufall steht nicht im
    /// Pult, sondern als Schalter neben dem Crossfader (ohne Fenster).
    #[test]
    fn party_pult_has_three_groups_of_one_key_size() {
        let ctx = theme::test_context();
        ctx.enable_accesskit();
        let rows = woa_radio::party::buttons(true, woa_radio::control::Repeat::All, true, true);
        let mut cmds = Vec::new();
        let out = ctx.run_ui(Default::default(), |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                let l = PartyLayout {
                    now: None,
                    next: None,
                    buttons: rows.clone(),
                    repeat: woa_radio::control::Repeat::All,
                    shuffle: true,
                    muted: true,
                    wishes: Vec::new(),
                    level_db: 0.0,
                    fullscreen: false,
                };
                pult(ui, &l, true, 64.0, &mut cmds);
            });
        });
        let update = out.platform_output.accesskit_update.expect("accesskit");
        let nodes: Vec<_> = update.nodes.iter().map(|(_, n)| n).collect();
        let mut buttons: Vec<_> = nodes
            .iter()
            .filter(|n| n.role() == egui::accesskit::Role::Button)
            .map(|n| {
                (
                    n.label().unwrap_or_default().to_string(),
                    n.bounds().map(|b| (b.width(), b.height())),
                    n.bounds()
                        .map(|b| ((b.y0 * 10.0) as i64, (b.x0 * 10.0) as i64)),
                )
            })
            .collect();
        buttons.sort_by_key(|b| b.2);
        let words: Vec<String> = buttons
            .iter()
            .map(|(l, _, _)| l.split_once(' ').expect(l).1.trim().to_string())
            .collect();
        assert_eq!(
            words,
            [
                "ZURÜCK", "PAUSE", "STOPP", "WEITER", "VON VORN", "−10 S", "+10 S", "PLAYLIST",
                "LEISER", "LAUTER", "STUMM"
            ],
            "{buttons:?}"
        );
        let size = buttons[0].1.expect("bounds");
        for (label, b, _) in &buttons {
            let (icon, _) = label.split_once(' ').expect(label);
            assert!(
                icon.chars().all(|c| ('\u{e000}'..='\u{f8ff}').contains(&c)),
                "Symbol: {label}"
            );
            let b = b.expect("bounds");
            assert!((b.1 - 64.0).abs() < 0.5, "{label}: {b:?}");
            assert!(
                (b.0 - size.0).abs() < 0.5,
                "eine Tastengröße: {label}: {b:?}"
            );
        }
        let labels: Vec<String> = nodes
            .iter()
            .filter_map(|n| n.value().or(n.label()).map(str::to_string))
            .collect();
        for g in ["ABSPIELEN", "SPRINGEN", "LAUTSTÄRKE"] {
            assert!(labels.iter().any(|l| l == g), "Gruppe {g}: {labels:?}");
        }
    }

    /// Zustand nur über Füllung: Play läuft = `accent`, eingeschaltet = `accent-2`, sonst
    /// neutral; keine Taste ist grün (Entwurf ScreenParty „Ohne Grün“).
    #[test]
    fn key_state_is_fill_only_and_never_green() {
        use PartyCommand as C;
        assert_eq!(key_fill(&C::PlayPause, false, true), Fill::Accent);
        assert_eq!(key_fill(&C::PlayPause, false, false), Fill::None);
        assert_eq!(key_fill(&C::Mute, true, true), Fill::Alt);
        assert_eq!(key_fill(&C::CycleRepeat, true, false), Fill::Alt);
        assert_eq!(key_fill(&C::Stop, false, true), Fill::None);
        assert_eq!(switch_fill(true), Fill::Alt);
        assert_eq!(switch_fill(false), Fill::None);
        let _ctx = theme::test_context();
        for mode in [theme::Mode::Studio, theme::Mode::Radio, theme::Mode::Party] {
            for t in [theme::Theme::Dunkel, theme::Theme::Hell] {
                let p = theme::palette_for(t, mode);
                for c in [p.accent, p.accent_2] {
                    assert_ne!(c, p.ok, "{mode:?} {t:?}");
                    assert_ne!(c, p.led_green, "{mode:?} {t:?}");
                }
            }
        }
        assert_eq!(Fill::None.color(), None);
    }

    /// Unter den Decks: rechts 520 px wie im Entwurf, bei 1366 px ein Drittel, nie unter 320.
    #[test]
    fn side_column_and_key_height_follow_window() {
        let (l, r) = columns_split(1892.0);
        assert_eq!(r, SIDE_W);
        assert!((l + r + GAP - 1892.0).abs() < 0.01);
        let (l, r) = columns_split(1338.0);
        assert!((320.0..SIDE_W).contains(&r) && l > 800.0, "{l} {r}");
        assert_eq!(pult_key_height(1030.0), 64.0);
        assert_eq!(pult_key_height(768.0), 48.0);
    }

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

    #[test]
    fn wish_folder_files_become_wishes_without_duplicates_and_stay_on_disk() {
        let tmp = tempfile::tempdir().unwrap();
        let d = tmp.path();
        std::fs::write(d.join("Gast - Lied.mp3"), b"x").unwrap();
        let later = std::time::SystemTime::now() + std::time::Duration::from_secs(10);
        let list = |now| crate::wishes::WishList {
            files: crate::wishes::scan(d, now).unwrap(),
            ..Default::default()
        };
        // Wird noch kopiert: noch kein Wunsch.
        let mut w = woa_radio::party::Wishlist::default();
        assert_eq!(folder_wishes(list(std::time::SystemTime::now()), &mut w), 0);
        assert_eq!(folder_wishes(list(later), &mut w), 1);
        assert_eq!(w.items()[0].title, "Gast - Lied");
        assert!(
            !woa_radio::party::is_free_wish(&w.items()[0].path),
            "direkt einreihbar"
        );
        assert_eq!(folder_wishes(list(later), &mut w), 0, "keine Doppelten");
        let p = w.items()[0].path.clone();
        assert!(w.take(&p).is_some());
        assert!(p.exists(), "Entfernen löscht nie die Datei");
        assert_eq!(
            folder_wishes(list(later), &mut w),
            0,
            "entfernt bleibt entfernt"
        );
        std::fs::write(d.join("Neu.wav"), b"x").unwrap();
        assert_eq!(
            folder_wishes(list(later), &mut w),
            1,
            "eine neue Datei kommt dazu"
        );
    }

    #[test]
    fn wish_title_from_tags_else_file_name() {
        let tmp = tempfile::tempdir().unwrap();
        let wav = |p: &std::path::Path| {
            let spec = hound::WavSpec {
                channels: 1,
                sample_rate: 8000,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            };
            let mut w = hound::WavWriter::create(p, spec).unwrap();
            for _ in 0..800 {
                w.write_sample(0i16).unwrap();
            }
            w.finalize().unwrap();
        };
        let tagged = tmp.path().join("datei.wav");
        wav(&tagged);
        let f = woa_library::tags::Fields {
            title: "Titel".into(),
            artist: "Interpret".into(),
            ..Default::default()
        };
        woa_library::tags::write_fields(&tagged, &woa_library::tags::Edit::exact(&f, None))
            .unwrap();
        assert_eq!(wish_title(&tagged, "datei"), "Interpret – Titel");

        let plain = tmp.path().join("ohne.wav");
        wav(&plain);
        assert_eq!(wish_title(&plain, "ohne"), "ohne");

        let only_title = tmp.path().join("halb.wav");
        wav(&only_title);
        let f = woa_library::tags::Fields {
            title: "Nur Titel".into(),
            ..Default::default()
        };
        woa_library::tags::write_fields(&only_title, &woa_library::tags::Edit::exact(&f, None))
            .unwrap();
        assert_eq!(wish_title(&only_title, "halb"), "halb");

        let broken = tmp.path().join("kaputt.mp3");
        std::fs::write(&broken, b"kein audio").unwrap();
        assert_eq!(wish_title(&broken, "kaputt"), "kaputt");
    }

    #[test]
    fn crossfader_position() {
        assert_eq!(fader_pos_at(0.0, 10.0, 100.0), 0.0);
        assert_eq!(fader_pos_at(60.0, 10.0, 100.0), 0.5);
        assert_eq!(fader_pos_at(500.0, 10.0, 100.0), 1.0);
        assert_eq!(fader_pos_at(33.333, 0.0, 100.0), 0.33);
    }
}
