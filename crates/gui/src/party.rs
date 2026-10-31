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
        // Oben: zwei Decks.
        ui.columns(2, |cols| {
            for (k, side) in [Side::Left, Side::Right].into_iter().enumerate() {
                self.deck_card(&mut cols[k], desk, side, &layout, &view, &duo);
            }
        });
        ui.add_space(8.0);
        self.crossfader(ui, desk, &duo, xf);
        ui.add_space(8.0);
        folder |= buttons_row(ui, &layout, view.running, view.repeat, &mut cmds);
        ui.add_space(10.0);
        // Unten: links Playlist, rechts Bibliothekssuche und Wünsche.
        ui.columns(2, |cols| {
            playlist_card(&mut cols[0], desk, &view);
            let ui = &mut cols[1];
            self.search_card(ui, desk, library, &mut cmds);
            ui.add_space(8.0);
            self.wish_card(ui, library, &mut cmds);
            ui.add_space(10.0);
            self.remote.ui(ui);
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
        use crate::widgets::{Tone, ctl_key};
        let k = side_index(side);
        let s = &duo.sides[k];
        let live = k == duo.live;
        let spin = s.playing;
        theme::lcd_show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.set_min_height(200.0);
            let d = 110.0;
            let area = ui.max_rect();
            let text_w = (ui.available_width() - d - 16.0).max(120.0);
            ui.scope(|ui| {
                ui.set_max_width(text_w);
                let name = if side == Side::Left {
                    "DECK A · LINKS"
                } else {
                    "DECK B · RECHTS"
                };
                let (badge, col) = deck_badge(live, s.playing, l);
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(name)
                            .size(12.0)
                            .strong()
                            .monospace()
                            .color(theme::text_dim()),
                    );
                    ui.label(
                        egui::RichText::new(badge)
                            .size(12.0)
                            .strong()
                            .monospace()
                            .color(col),
                    );
                });
                if live {
                    now_texts(ui, l);
                } else {
                    let title = s
                        .index
                        .and_then(|i| v.playlist.get(i))
                        .map(|e| e.title.clone());
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(title.unwrap_or_else(|| "– leer –".into()))
                                .size(28.0)
                                .strong()
                                .color(theme::text()),
                        )
                        .truncate(),
                    );
                    if s.playing {
                        ui.label(
                            egui::RichText::new(format!(
                                "noch {}",
                                woa_radio::party::clock((s.end - s.position).max(0.0))
                            ))
                            .size(16.0)
                            .color(theme::text_dim()),
                        );
                    }
                }
                ui.add_space(6.0);
                let (icon, word, tone) = if s.playing {
                    (theme::ic::PAUSE, "Pause", Tone::Pause)
                } else {
                    (theme::ic::PLAY, "Play", Tone::Play)
                };
                let r = ui.add_enabled_ui(s.index.is_some(), |ui| {
                    ctl_key(ui, icon, word, tone, s.playing, 0.0)
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
            let jog_rect = egui::Rect::from_min_size(
                egui::pos2(area.right() - d, area.top() + 4.0),
                egui::vec2(d, d),
            );
            let id = egui::Id::new(if side == Side::Left {
                "party-jog-l"
            } else {
                "party-jog-r"
            });
            ui.scope_builder(egui::UiBuilder::new().max_rect(jog_rect), |ui| {
                crate::widgets::jog(ui, id, d, spin)
                    .on_hover_text("Der Plattenteller dreht sich, solange dieses Deck spielt");
            });
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
        use crate::widgets::{Tone, ctl_key};
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("A").size(18.0).strong().color(theme::text_dim()));
            let w = (ui.available_width() - 30.0 - crate::widgets::CTL_MIN_WIDTH - 60.0).max(100.0);
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, 30.0), egui::Sense::click_and_drag());
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
            let track = egui::Rect::from_center_size(rect.center(), egui::vec2(rect.width(), 6.0));
            p.rect_filled(track, 3, theme::well());
            p.vline(rect.center().x, rect.y_range(), egui::Stroke::new(1.0, theme::border_strong()));
            let x = rect.left() + pos * rect.width();
            let knob = egui::Rect::from_center_size(egui::pos2(x, rect.center().y), egui::vec2(18.0, 28.0));
            p.rect_filled(knob, 4, theme::accent());
            p.rect_stroke(knob, 4, egui::Stroke::new(1.0, theme::border_strong()), egui::StrokeKind::Inside);
            resp.on_hover_text(format!(
                "Crossfader: freie Blende zwischen Deck A und B, in der Mitte beide gleich laut. \
                 Mit Auto-Übergang fährt er am Titelende in {xf:.0} s von selbst; anfassen hält ihn."
            ));
            ui.label(egui::RichText::new("B").size(18.0).strong().color(theme::text_dim()));
            ui.add_space(12.0);
            let (icon, word) = auto_label(duo.auto);
            if ctl_key(ui, icon, word, Tone::Normal, duo.auto, 0.0)
                .on_hover_text(
                    "An: kurz vor dem Titelende startet das andere Deck und der Fader fährt hinüber. \
                     Aus: jedes Deck selbst starten und von Hand blenden.",
                )
                .clicked()
            {
                desk.set_party_auto(!duo.auto);
            }
        });
    }

    /// Bibliothekssuche (dieselbe Suche wie im Fenster „Bibliothek“).
    fn search_card(
        &mut self,
        ui: &mut egui::Ui,
        desk: &mut RadioDesk,
        library: &mut crate::library::LibraryWindow,
        cmds: &mut Vec<PartyCommand>,
    ) {
        theme::card_frame().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(format!("{} BIBLIOTHEK", theme::ic::MAGNIFYING_GLASS))
                        .size(11.5)
                        .strong()
                        .color(theme::text_dim()),
                );
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
        use crate::widgets::{Tone, ctl_key};
        theme::card_frame().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(
                egui::RichText::new("WUNSCHLISTE")
                    .size(11.5)
                    .strong()
                    .color(theme::text_dim()),
            );
            ui.horizontal(|ui| {
                let r = ui.add(
                    egui::TextEdit::singleline(&mut self.wish_input)
                        .hint_text("Wer: Titel / Interpret")
                        .desired_width((ui.available_width() - 130.0).max(80.0)),
                );
                let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                let add =
                    ctl_key(ui, theme::ic::PLUS, "Wunsch", Tone::Normal, false, 0.0).clicked();
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
                ui.label(
                    egui::RichText::new("Noch keine Wünsche.")
                        .size(18.0)
                        .color(theme::text_dim()),
                );
            }
            for (i, w) in items.iter().enumerate() {
                ui.horizontal_wrapped(|ui| {
                    let mut t = egui::RichText::new(format!("{}. {}", i + 1, w.title)).size(18.0);
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
                    if w.votes > 1 {
                        theme::status_pill(ui, &format!("×{}", w.votes), theme::cue(), true);
                    }
                });
                ui.horizontal(|ui| {
                    if ctl_key(
                        ui,
                        theme::ic::MAGNIFYING_GLASS,
                        "Suchen",
                        Tone::Normal,
                        false,
                        0.0,
                    )
                    .clicked()
                    {
                        self.search_for(w, library);
                    }
                    let free = woa_radio::party::is_free_wish(&w.path);
                    if ctl_key(
                        ui,
                        theme::ic::QUEUE,
                        "→ Playlist",
                        Tone::Normal,
                        w.queued.is_some(),
                        0.0,
                    )
                    .on_hover_text(if free {
                        "Freier Wunsch: erst in der Bibliothek einen Titel wählen"
                    } else {
                        "Als nächsten Titel einreihen"
                    })
                    .clicked()
                    {
                        if free {
                            self.search_for(w, library);
                        } else {
                            cmds.push(PartyCommand::PlayWish(w.path.clone()));
                        }
                    }
                    let (icon, word) = if w.done {
                        (theme::ic::ARROW_COUNTER_CLOCKWISE, "Offen")
                    } else {
                        (theme::ic::CHECK, "Erledigt")
                    };
                    if ctl_key(ui, icon, word, Tone::Normal, false, 0.0).clicked() {
                        cmds.push(PartyCommand::MarkWish {
                            path: w.path.clone(),
                            done: !w.done,
                        });
                    }
                    if ctl_key(ui, theme::ic::X, "Entfernen", Tone::Normal, false, 0.0).clicked() {
                        cmds.push(PartyCommand::DropWish(w.path.clone()));
                    }
                });
                ui.add_space(4.0);
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
/// Treffer der Bibliothek hineinziehen. Verwaltet wird im Fenster „Verwalten“.
fn playlist_card(ui: &mut egui::Ui, desk: &RadioDesk, v: &woa_radio::station::View) {
    theme::card_frame().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(format!("{} PLAYLIST", theme::ic::PLAYLIST))
                    .size(11.5)
                    .strong()
                    .color(theme::text_dim()),
            );
            let name = v
                .playlists
                .get(v.active_playlist)
                .cloned()
                .unwrap_or_default();
            egui::ComboBox::from_id_salt("party-playlist")
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
            ui.label(
                egui::RichText::new(format!("({} Titel)", v.playlist.len()))
                    .small()
                    .color(theme::text_dim()),
            );
        });
        ui.add_space(4.0);
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
            .max_height(320.0)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
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
                    let (mark, col) = if on_air {
                        (format!("{} läuft", theme::ic::PLAY), theme::ok())
                    } else if next == Some(i) {
                        (
                            format!("{} nächster", theme::ic::SKIP_FORWARD),
                            theme::cue(),
                        )
                    } else {
                        (String::new(), theme::text_dim())
                    };
                    let r =
                        ui.dnd_drag_source(
                            egui::Id::new(("party-row", i)),
                            PartyDrag::Row(i),
                            |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        egui::RichText::new(format!("{:>3}", i + 1))
                                            .monospace()
                                            .color(theme::text_dim()),
                                    );
                                    ui.add(
                                        egui::Label::new(
                                            egui::RichText::new(&e.title).size(16.0).color(
                                                if on_air { theme::ok() } else { theme::text() },
                                            ),
                                        )
                                        .truncate()
                                        .selectable(false),
                                    );
                                    if !mark.is_empty() {
                                        ui.label(
                                            egui::RichText::new(&mark).small().strong().color(col),
                                        );
                                    }
                                });
                            },
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
        .filter(|f| wishes.add_folder_file(f.path.clone(), &f.name))
        .count()
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

/// Beschriftung des Schalters „Auto-Übergang“ (Symbol und Wort).
fn auto_label(on: bool) -> (&'static str, &'static str) {
    if on {
        (theme::ic::ARROWS_LEFT_RIGHT, "Auto-Übergang an")
    } else {
        (theme::ic::HAND, "Auto-Übergang aus")
    }
}

/// Anzeige oben am Deck: hörbar (JETZT, PAUSE, BEREIT) oder nicht (LÄUFT MIT, VORGELADEN).
fn deck_badge(live: bool, playing: bool, l: &PartyLayout) -> (&'static str, egui::Color32) {
    if live {
        match &l.now {
            Some(n) if n.playing => ("JETZT", theme::ok()),
            Some(n) if n.paused => ("PAUSE", theme::warn()),
            _ => ("BEREIT", theme::cue()),
        }
    } else if playing {
        ("LÄUFT MIT", theme::warn())
    } else {
        ("VORGELADEN", theme::cue())
    }
}

fn now_texts(ui: &mut egui::Ui, l: &PartyLayout) {
    match &l.now {
        Some(n) => {
            ui.add(
                egui::Label::new(
                    egui::RichText::new(&n.title)
                        .size(32.0)
                        .strong()
                        .color(theme::text()),
                )
                .truncate(),
            );
            if !n.artist.is_empty() {
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(&n.artist)
                            .size(22.0)
                            .color(theme::text_dim()),
                    )
                    .truncate(),
                );
            }
            ui.add_space(4.0);
            let urgent = n.playing && n.remaining.len() <= 4 && n.remaining.starts_with("0:0");
            ui.label(
                egui::RichText::new(&n.remaining)
                    .size(56.0)
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
                    .desired_height(10.0)
                    .fill(theme::accent()),
            );
        }
        None => {
            ui.label(
                egui::RichText::new("Noch kein Titel – Ordner laden")
                    .size(24.0)
                    .color(theme::text_dim()),
            );
        }
    }
}

/// Zeichnet die Tasten (alle als einheitlicher Steuerknopf); `true` bei „Ordner laden“.
fn buttons_row(
    ui: &mut egui::Ui,
    l: &PartyLayout,
    running: bool,
    repeat: woa_radio::control::Repeat,
    cmds: &mut Vec<PartyCommand>,
) -> bool {
    use crate::widgets::{Tone, ctl_key};
    let mut folder = false;
    let gap = 8.0;
    let w = (ui.available_width() - 3.0 * gap) / 4.0;
    for row in &l.buttons {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for b in row {
                let tone = match b.command {
                    PartyCommand::PlayPause if running => Tone::Pause,
                    PartyCommand::PlayPause => Tone::Play,
                    _ => Tone::Normal,
                };
                let icon = command_icon(&b.command, running, repeat);
                if ctl_key(ui, icon, b.label, tone, b.active, w).clicked() {
                    cmds.push(b.command.clone());
                }
            }
        });
        ui.add_space(4.0);
    }
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        ui.label(
            egui::RichText::new(format!("Lautstärke {:+.0} dB", l.level_db))
                .size(16.0)
                .color(theme::text_dim()),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let (icon, word) = if l.fullscreen {
                (theme::ic::CORNERS_IN, "Vollbild aus")
            } else {
                (theme::ic::CORNERS_OUT, "Vollbild")
            };
            if ctl_key(ui, icon, word, Tone::Normal, l.fullscreen, 0.0)
                .on_hover_text("Taste F oder F11")
                .clicked()
            {
                cmds.push(PartyCommand::ToggleFullscreen);
            }
            if ctl_key(
                ui,
                theme::ic::FOLDER_OPEN,
                "Ordner laden",
                Tone::Normal,
                false,
                0.0,
            )
            .clicked()
            {
                folder = true;
            }
        });
    });
    folder
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Alle Steuerknöpfe der Party haben Symbol und Wort und dieselbe Höhe (ohne Fenster).
    #[test]
    fn party_buttons_have_symbol_word_and_one_height() {
        let ctx = theme::test_context();
        ctx.enable_accesskit();
        let rows = woa_radio::party::buttons(true, woa_radio::control::Repeat::All, true, false);
        let out = ctx.run_ui(Default::default(), |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                let l = PartyLayout {
                    now: None,
                    next: None,
                    buttons: rows.clone(),
                    repeat: woa_radio::control::Repeat::All,
                    shuffle: true,
                    muted: false,
                    wishes: Vec::new(),
                    level_db: 0.0,
                    fullscreen: false,
                };
                let mut cmds = Vec::new();
                buttons_row(ui, &l, true, woa_radio::control::Repeat::All, &mut cmds);
            });
        });
        let update = out.platform_output.accesskit_update.expect("accesskit");
        let buttons: Vec<_> = update
            .nodes
            .iter()
            .filter(|(_, n)| n.role() == egui::accesskit::Role::Button)
            .map(|(_, n)| {
                (
                    n.label().unwrap_or_default().to_string(),
                    n.bounds().map(|b| b.height()),
                )
            })
            .collect();
        assert_eq!(buttons.len(), 12 + 2, "{buttons:?}");
        for (label, h) in &buttons {
            let (icon, word) = label.split_once(' ').expect(label);
            assert!(!icon.is_empty() && !word.trim().is_empty(), "{label}");
            assert!(
                icon.chars().all(|c| ('\u{e000}'..='\u{f8ff}').contains(&c)),
                "Symbol: {label}"
            );
            let h = h.expect("bounds");
            assert!(
                (h - f64::from(crate::widgets::CTL_HEIGHT)).abs() < 0.5,
                "{label}: {h}"
            );
        }
        assert!(buttons.iter().any(|(l, _)| l.ends_with(" Pause")));
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
    fn crossfader_and_auto_switch() {
        assert_eq!(fader_pos_at(0.0, 10.0, 100.0), 0.0);
        assert_eq!(fader_pos_at(60.0, 10.0, 100.0), 0.5);
        assert_eq!(fader_pos_at(500.0, 10.0, 100.0), 1.0);
        assert_eq!(fader_pos_at(33.333, 0.0, 100.0), 0.33);
        for on in [true, false] {
            let (icon, word) = auto_label(on);
            assert!(!icon.is_empty() && word.starts_with("Auto-Übergang"));
        }
        assert_ne!(auto_label(true), auto_label(false));
    }
}
