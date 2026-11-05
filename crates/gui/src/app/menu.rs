//! Tastenkürzel, Menü, Chips, Entwickler-Menü, Screenshot und Foto.

use super::*;

impl WoaApp {
    /// F11 schaltet in Produzent und Radio das Vollbild um (Party fängt F und F11 selbst).
    /// Kein Fenster meldet seinen Zustand (Tests): gilt als nicht Vollbild.
    pub(super) fn fullscreen_key(&mut self, ctx: &egui::Context) {
        if self.config.mode == Mode::Party {
            return;
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::F11)) {
            let on = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(!on));
        }
    }

    pub(super) fn shortcuts(&mut self, ctx: &egui::Context) {
        if let Some(pr) = self.piano_roll.as_mut()
            && !pr.keys(ctx, &mut self.state)
        {
            self.piano_roll = None;
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Q)) {
            self.quit(ctx);
        }
        let (undo, redo, save, open, new, record) = ctx.input_mut(|i| {
            let m = egui::Modifiers::COMMAND;
            let sm = egui::Modifiers::COMMAND | egui::Modifiers::SHIFT;
            (
                i.consume_key(m, egui::Key::Z),
                i.consume_key(m, egui::Key::Y) || i.consume_key(sm, egui::Key::Z),
                i.consume_key(m, egui::Key::S),
                i.consume_key(m, egui::Key::O),
                i.consume_key(m, egui::Key::N),
                i.consume_key(m, egui::Key::R),
            )
        });
        if record {
            self.toggle_record();
        }
        // Offene Dialoge (Über, Export, Feedback, Popups) schlucken die Bearbeitungs-Kürzel.
        if !ctx.egui_wants_keyboard_input() && !self.overlay_open(ctx) {
            let (copy, cut, paste, dup) = ctx.input_mut(|i| {
                let has =
                    |i: &egui::InputState, f: fn(&egui::Event) -> bool| i.events.iter().any(f);
                (
                    has(i, |e| matches!(e, egui::Event::Copy)),
                    has(i, |e| matches!(e, egui::Event::Cut)),
                    has(i, |e| matches!(e, egui::Event::Paste(_))),
                    i.consume_key(egui::Modifiers::COMMAND, egui::Key::D),
                )
            });
            if copy || cut {
                self.copy_selected();
            }
            if cut {
                self.delete_selected();
            }
            if paste {
                self.paste_at_playhead();
            }
            if dup {
                self.duplicate_selected();
            }
            let (space, home, del, split, export) = ctx.input_mut(|i| {
                let n = egui::Modifiers::NONE;
                (
                    i.consume_key(n, egui::Key::Space),
                    i.consume_key(n, egui::Key::Home),
                    i.consume_key(n, egui::Key::Delete),
                    i.consume_key(n, egui::Key::S),
                    i.consume_key(egui::Modifiers::COMMAND, egui::Key::E),
                )
            });
            if split {
                self.split_at_playhead();
            }
            if export {
                self.export();
            }
            if space {
                self.toggle_play();
            }
            if home {
                self.rewind();
            }
            if del {
                if self.selected_clip.is_none() && self.selected_clips.is_empty() {
                    if let Some(t) = self.marked_track() {
                        self.delete_track(t);
                    }
                } else {
                    self.delete_selected();
                }
            }
        }
        if undo {
            self.state.undo();
            self.undo_toast = None;
        }
        if redo {
            self.state.redo();
        }
        if save {
            self.save();
        }
        if open {
            self.open();
        }
        if new {
            self.new_project();
        }
    }

    /// Hinweis nach dem Löschen einer Spur (Design `DeleteTrack`): unten mittig über der
    /// Zeitleiste, 8 s mit ablaufendem Strich, „Rückgängig (Strg+Z)“, Schließen mit × oder Esc.
    pub(super) fn undo_toast_ui(&mut self, ctx: &egui::Context) {
        let Some(t) = &self.undo_toast else {
            return;
        };
        let left = t.until - self.now;
        if left <= 0.0 || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.undo_toast = None;
            return;
        }
        let text = t.text();
        let mut undo = false;
        let mut close = false;
        egui::Area::new(egui::Id::new("undo_toast"))
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
                            if ui
                                .link(
                                    egui::RichText::new("Rückgängig (Strg+Z)")
                                        .color(theme::accent()),
                                )
                                .clicked()
                            {
                                undo = true;
                            }
                            if ui
                                .add(
                                    egui::Button::new(
                                        egui::RichText::new(theme::ic::X).color(theme::text_dim()),
                                    )
                                    .frame_when_inactive(false),
                                )
                                .on_hover_text("Schließen (Esc)")
                                .clicked()
                            {
                                close = true;
                            }
                        });
                    })
                    .response
                    .rect;
                let frac = (left / UNDO_TOAST_SECS).clamp(0.0, 1.0) as f32;
                let bar = egui::Rect::from_min_size(
                    egui::pos2(r.left() + 6.0, r.bottom() - 3.0),
                    egui::vec2((r.width() - 12.0) * frac, 2.0),
                );
                ui.painter().rect_filled(bar, 1.0, theme::accent());
            });
        if undo {
            self.state.undo();
            self.undo_toast = None;
        } else if close {
            self.undo_toast = None;
        } else {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
    }

    pub(super) fn take_screenshot(&mut self, ctx: &egui::Context) {
        if self.feedback.phase != Phase::Capturing {
            return;
        }
        // The dialog vanished last frame; take the shot now, so it is not in the picture.
        if !self.feedback.capture_requested {
            self.feedback.capture_requested = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
            ctx.request_repaint();
            return;
        }
        let image = ctx.input(|i| {
            i.raw.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(image) = image {
            let [w, h] = image.size;
            let png = match (u32::try_from(w), u32::try_from(h)) {
                (Ok(w), Ok(h)) => encode_png(w, h, image.as_raw())
                    .map_err(|e| tracing::warn!(error = %e, "screenshot encode failed"))
                    .ok(),
                _ => None,
            };
            self.feedback.set_screenshot(png);
        }
    }

    /// Fotomodus für Design-Prüfungen: nach ein paar Bildern ein Bildschirmfoto des Fensters
    /// als PNG speichern und beenden.
    pub(super) fn photo(&mut self, ctx: &egui::Context) {
        let Some((_, frames)) = &mut self.photo else {
            return;
        };
        *frames += 1;
        let first = *frames == 2;
        let n = *frames;
        if first
            && std::env::var_os("WOA_CUE_EDITOR").is_some()
            && let Some(r) = self.radio.as_mut()
        {
            // Fotomodus des Cue-Editors: erster Titel der Playlist.
            r.open_cue_editor(0);
        }
        if first && let Ok(q) = std::env::var("WOA_SETTINGS") {
            // Fotomodus „Einstellungen“ (Fassung 7); der Wert ist der Filtertext.
            self.settings.open = true;
            self.settings.query = q;
        }
        if first && let Ok(a) = std::env::var("WOA_SETTINGS_AREA") {
            // Fotomodus je Bereich: Name wie links im Fenster (Entwickler nur für dieses Bild).
            if let Some(area) = crate::settings::Area::ALL
                .into_iter()
                .find(|x| x.name() == a)
            {
                if area.dev_only() {
                    self.config.entwickler = true;
                }
                self.settings.open_area(area);
            }
        }
        if n == 4 && std::env::var_os("WOA_SOUNDPICKER").is_some() {
            // Fotomodus Klangauswahl: erste MIDI-Spur (mit WOA_DEMO_SONG), nur Anzeige.
            let id = self
                .state
                .project()
                .tracks
                .iter()
                .find(|t| t.kind == woa_model::TrackKind::Midi)
                .map(|t| t.id);
            if let Some(id) = id {
                self.open_sound_picker(id);
            }
        }
        if n == 4 && std::env::var_os("WOA_DJ_LOAD").is_some() {
            // Fotomodus DJ (#347): Playlist-Titel 1 und 2 auf Deck A und B legen (nicht abspielen).
            self.ensure_radio();
            if let Some(r) = self.radio.as_ref() {
                use woa_radio::control::DjCommand;
                r.dj(DjCommand::Load { side: 0, index: 0 });
                r.dj(DjCommand::Load { side: 1, index: 1 });
            }
        }
        if first && std::env::var_os("WOA_CONTROLLER").is_some() {
            // Fotomodus der Zuordnungsliste.
            self.controls.window = true;
        }
        if first && std::env::var_os("WOA_FRAGEN").is_some() {
            // Fotomodus „Fragen und To-dos“: Entwickler-Modus nur für dieses Bild, nicht gespeichert.
            self.config.entwickler = true;
            self.fragen.open_window(ctx);
        }
        if first && std::env::var_os("WOA_ABNAHME").is_some() {
            // Fotomodus „Abnahme“: Entwickler-Modus nur für dieses Bild, nicht gespeichert.
            self.config.entwickler = true;
            self.abnahme.open_window(ctx);
        }
        if first && std::env::var("WOA_MENU").is_ok_and(|v| v == "Entwickler") {
            self.config.entwickler = true;
        }
        if first && std::env::var_os("WOA_JINGLE").is_some() {
            // Fotomodus des Jingle-Assistenten: Fenster beim ersten Bild öffnen.
            self.jingle_assistent.open = true;
            // `WOA_JINGLE=elevenlabs`: Schritt „Stimme“ mit Sprecher ElevenLabs (Beispieldaten).
            if std::env::var("WOA_JINGLE").is_ok_and(|v| v == "elevenlabs")
                && let Some(m) = woa_model::jingle::MOODS.first()
                && let Some(parts) = self.state.build_jingle(m, 10)
            {
                self.jingle_assistent.foto_elevenlabs(parts);
            }
        }
        if first && std::env::var_os("WOA_AUDIO").is_some() {
            // Fotomodus Fenster „Audio-Geräte“ (#443: passt es auf kleine Bildschirme?).
            self.open_audio();
        }
        if first && std::env::var_os("WOA_KI").is_some() {
            // Fotomodus Fenster „Dienste und Konten“ (u. a. ElevenLabs-Schlüssel).
            self.ki_ui.open(&self.config.ki, &self.data_dir);
        }
        if first && std::env::var_os("WOA_DEMO_SONG").is_some() {
            // Fotomodus des Produzenten: ein Jingle-Gerüst plus Akkorde und ein FX-Clip,
            // damit Arrangement, Browser und Dock echte Inhalte zeigen (keine Nutzerdaten).
            if let Some(m) = woa_model::jingle::MOODS.first() {
                let _ = self.state.build_jingle(m, 10);
            }
            if let Some(p) = woa_model::chords::CHORD_PROGRESSIONS.first() {
                let _ = self.state.add_chord_tracks(p, 0, 4, 0.0);
            }
        }
        if first
            && let Some(n) = std::env::var("WOA_SELECT")
                .ok()
                .and_then(|v| v.parse::<usize>().ok())
        {
            // Fotomodus des Clip-Inspektors: die ersten `n` Clips wählen (1 = ein Clip).
            let ids: Vec<ClipId> = self
                .state
                .project()
                .tracks
                .iter()
                .flat_map(|t| &t.clips)
                .map(|c| c.id)
                .take(n)
                .collect();
            self.selected_clip = ids.first().copied();
            self.selected_clips = ids;
        }
        if first && std::env::var_os("WOA_HOERER").is_some() {
            // Fotomodus der Hörer-Statistik: Beispielzahlen statt Server.
            self.hoerer.demo();
        }
        if first && std::env::var_os("WOA_PIANOROLL").is_some() {
            // Fotomodus der Piano-Roll: ersten MIDI-Clip öffnen, zwei Noten wählen;
            // `WOA_PIANOROLL=kit` nimmt eine Kit-Spur (Drum-Raster), sonst keine.
            let kit = std::env::var("WOA_PIANOROLL").is_ok_and(|v| v == "kit");
            let clip = self
                .state
                .project()
                .tracks
                .iter()
                .filter(|t| t.sound.is_some_and(|s| s.drums) == kit)
                .flat_map(|t| &t.clips)
                .find(|c| c.midi.is_some())
                .map(|c| c.id);
            if let Some(clip) = clip {
                self.open_piano_roll(clip);
                if let Some(pr) = self.piano_roll.as_mut() {
                    pr.sel = vec![1, 2];
                }
            }
            ctx.request_repaint();
            return;
        }
        let Some((path, frames)) = &mut self.photo else {
            return;
        };
        // `WOA_PHOTO_FRAMES=n` wartet länger (etwa bis Wellenformen gelesen sind).
        let at = std::env::var("WOA_PHOTO_FRAMES")
            .ok()
            .and_then(|v| v.parse::<u32>().ok())
            .unwrap_or(20);
        if *frames < at {
            ctx.request_repaint_after(std::time::Duration::from_millis(30));
        }
        if *frames == at {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
        }
        let image = ctx.input(|i| {
            i.raw.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(image) = image {
            let [w, h] = image.size;
            let result = encode_png(w as u32, h as u32, image.as_raw())
                .map_err(|e| e.to_string())
                .and_then(|png| std::fs::write(&*path, png).map_err(|e| e.to_string()));
            match result {
                Ok(()) => tracing::info!(path = %path.display(), "photo saved"),
                Err(e) => tracing::error!(error = %e, "photo failed"),
            }
            self.exit.allow(crate::exit::ExitReason::Quit);
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        ctx.request_repaint();
    }

    /// Kopfleiste (TopBar): links Marke und Menüs, in der Mitte die Update-Info (nur wenn ein
    /// Update da ist), rechts MIDI, ON AIR, Modus-Umschalter und ganz rechts Feedback.
    pub(super) fn menu(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        egui::MenuBar::new().ui(ui, |ui| {
            self.menu_items(ui);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // Rechts ein Block (TopBar-README): Modus | MIDI · ON AIR · REC · KI | Feedback,
                // links davon nur bei Bedarf die Update-Info.
                let compact = ui.available_width() < TOPBAR_COMPACT_BELOW;
                // Der Block füllt von rechts: Aufrufe in umgekehrter Lesereihenfolge.
                status_bar(ui, |ui| {
                    self.feedback_button(ui, ctx, compact);
                    status_sep(ui);
                    if let Some(r) = self.mcp_live.chip(ui, compact)
                        && r.clicked()
                    {
                        self.settings
                            .goto(crate::settings::SettingId::McpLive, self.now);
                    }
                    self.rec_chip(ui, compact);
                    self.hoerer_chip(ui, compact);
                    self.onair_lamp(ui, compact);
                    self.midi_lamp(ui, compact);
                    status_sep(ui);
                    self.mode_switch(ui, compact);
                    // Zahnrad: Einstellungen beim Bereich der aktuellen Ansicht.
                    let area = self.mode_area();
                    if topbar_chip(
                        ui,
                        theme::ic::GEAR_SIX,
                        "Einstellungen der Ansicht",
                        ChipLook::Plain,
                        true,
                        true,
                    )
                    .on_hover_text(format!("Einstellungen › {} (Strg+,)", area.name()))
                    .clicked()
                    {
                        self.settings.open_area(area);
                    }
                });
                ui.add_space(TOPBAR_GROUP_GAP);
                self.update_info(ui);
            });
        });
        if let Some(t) = self.controls.learning_text() {
            ui.horizontal(|ui| {
                ui.colored_label(theme::accent(), t);
                if ui.small_button("Abbrechen").clicked() {
                    self.controls.mapper.cancel_learn();
                }
            });
        }
    }

    /// Feedback-Taste: neutral mit Symbol, immer ganz rechts im Block (TopBar-README).
    pub(super) fn feedback_button(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        compact: bool,
    ) {
        let btn = topbar_chip(
            ui,
            theme::ic::CHAT_CIRCLE_TEXT,
            "Feedback",
            ChipLook::Plain,
            compact,
            true,
        );
        if btn
            .on_hover_text("Fehler gefunden, Idee gehabt? Schreib uns direkt aus WOA.")
            .clicked()
        {
            // #386: Ein Textfeld dahinter (Bibliothekssuche) behält sonst den Fokus und
            // nimmt die Tasten, während das Formular offen ist.
            ctx.memory_mut(|m| {
                if let Some(id) = m.focused() {
                    m.surrender_focus(id);
                }
            });
            // 0.42.1 (wie fotobrett): erst das Bild, dann der Dialog.
            self.feedback.open_with_screenshot();
            self.image_error = None;
        }
    }

    /// ON-AIR-Lampe in der Kopfleiste, direkt neben MIDI: leuchtet, solange die Sendung läuft
    /// oder das Mikro offen ist (vorher im Kopf des Radio-Decks).
    /// REC in der Kopfleiste (André 31.10.2026): der eine Mitschnitt-Knopf für alle Modi,
    /// ersetzt den Knopf im Sendepult. Schreibt die Sendesumme über denselben Weg wie bisher;
    /// ein Moduswechsel bricht ihn nicht ab, weil das Sendepult in jedem Modus weiterläuft.
    pub(super) fn rec_chip(&mut self, ui: &mut egui::Ui, compact: bool) {
        let rec = self.radio.as_ref().and_then(|r| {
            r.recording()
                .map(|(p, t)| (p.map(std::path::Path::to_path_buf), t))
        });
        let look = if rec.is_some() {
            ChipLook::On(theme::on_air())
        } else {
            ChipLook::Off(theme::on_air())
        };
        // Läuft der Mitschnitt, blinkt die LED und die Laufzeit steht hinter dem Wort.
        let since = rec.as_ref().map(|(_, t)| {
            let secs = t.map_or(0, |t| t.elapsed().as_secs());
            format!("{}:{:02}:{:02}", secs / 3600, secs / 60 % 60, secs % 60)
        });
        let resp = Chip {
            icon: theme::ic::RECORD,
            word: "REC",
            look,
            compact,
            click: true,
            suffix: since.as_deref(),
            blink: rec.is_some(),
        }
        .show(ui);
        let dir = crate::ordner::dir(crate::ordner::Art::Mitschnitte);
        let mut tip = match &rec {
            Some((path, since)) => {
                let secs = since.map_or(0, |t| t.elapsed().as_secs());
                format!(
                    "REC – Mitschnitt läuft seit {}:{:02}:{:02}\nZiel: {}",
                    secs / 3600,
                    secs / 60 % 60,
                    secs % 60,
                    path.as_deref().unwrap_or(&dir).display()
                )
            }
            None => format!("REC – Sendesumme als WAV nach {}", dir.display()),
        };
        if self.config.mode == Mode::Produzent {
            tip.push_str(
                "\nIm Produzent-Modus nimmt REC die Sendesumme auf, nicht die Zeitleiste \
                 (die über Datei → Exportieren → Mixdown).",
            );
        }
        if rec.is_some() {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(500));
        }
        if resp.on_hover_text(tip).clicked() {
            self.toggle_mitschnitt();
        }
    }

    /// Startet oder beendet den Mitschnitt, in jedem Modus (öffnet das Sendepult bei Bedarf).
    pub fn toggle_mitschnitt(&mut self) {
        self.ensure_radio();
        if let Some(r) = self.radio.as_mut() {
            r.toggle_recording();
        }
    }

    /// Läuft der Mitschnitt? (Für Tests; derselbe Zustand wie die REC-Pille.)
    pub fn mitschnitt_laeuft(&self) -> bool {
        self.radio.as_ref().is_some_and(|r| r.recording().is_some())
    }

    /// Hörer in der Kopfleiste, nur auf Sendung: „HÖRER 12 | 18“ (jetzt | Spitze der Sendung),
    /// Zahl aus `hoerer_url` (`jetzt.aktuell`, eigener Statistik-Server, höchstens alle 30 s im
    /// Hintergrund), „–“, solange keine Zahl kommt oder der Server nicht antwortet. Farbe `ok`, Rot bleibt ON AIR. Klick: Hörer-Statistik.
    pub(super) fn hoerer_chip(&mut self, ui: &mut egui::Ui, compact: bool) {
        let photo = self.photo.is_some() && std::env::var_os("WOA_ONAIR").is_some();
        let live = photo || self.radio.as_ref().is_some_and(RadioDesk::is_broadcasting);
        if live && !photo {
            self.hoerer_chip.poll(&self.config.hoerer_url);
        }
        let current = if photo {
            Some(14)
        } else if live {
            self.hoerer_chip.aktuell
        } else {
            None
        };
        let peak = self.hoerer_peak.update(live, current);
        // `spitze_server` ist die Sendungsspitze des Sammlers (eindeutige Hörer).
        let peak = match (peak, self.hoerer_chip.spitze_server.filter(|_| live)) {
            (Some(a), Some(b)) => Some(a.max(b)),
            (a, b) => a.or(b),
        };
        if !live {
            return;
        }
        let peak = if photo { Some(18) } else { peak };
        let suffix = crate::hoerer::chip_suffix(current, peak);
        let look = if current.is_some() {
            ChipLook::On(theme::ok())
        } else {
            ChipLook::Off(theme::ok())
        };
        let resp = Chip {
            icon: theme::ic::USERS_THREE,
            word: "Hörer",
            look,
            compact,
            click: true,
            suffix: Some(&suffix),
            blink: false,
        }
        .show(ui);
        let tip = match current {
            Some(c) => format!(
                "Hörer jetzt: {c} · Spitze dieser Sendung: {}\nKlick: Hörer-Statistik",
                peak.unwrap_or(c)
            ),
            None => "Hörerzahl noch nicht da (Statistik-Server antwortet nicht, \
                     config.json: hoerer_url).\nKlick: Hörer-Statistik"
                .to_string(),
        };
        let label = format!("Hörer {suffix}");
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
        if resp.on_hover_text(tip).clicked() {
            self.hoerer.open = true;
        }
    }

    pub(super) fn onair_lamp(&mut self, ui: &mut egui::Ui, compact: bool) {
        // Fotomodus `WOA_ONAIR=1` zeigt die leuchtende Lampe ohne echte Sendung (#353).
        let photo_live = self.photo.is_some() && std::env::var_os("WOA_ONAIR").is_some();
        let live = photo_live || self.radio.as_ref().is_some_and(RadioDesk::is_live);
        // #353: Auf Sendung leuchtet ON AIR wie die Lampe im Sendepult (rot, weiß, Glow).
        // Fassung 7: Lampe `woa-onair--bar` statt Pille; gedimmt im Pult-Modus (#301).
        let _ = compact;
        let state = crate::widgets::lamp::onair_state(live, self.config.pult.on);
        let word = match state {
            crate::widgets::lamp::LampState::Off => "ON AIR aus",
            crate::widgets::lamp::LampState::On => "ON AIR an",
            crate::widgets::lamp::LampState::Dim => "ON AIR an (Pult)",
        };
        let resp = crate::widgets::lamp::onair_bar(ui, state);
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, word));
        // Klick öffnet die Stream-Verbindung (wie Einstellungen → Sendung).
        if resp
            .on_hover_text(if live {
                "ON AIR – Auf Sendung: der Stream läuft. Klick: Stream-Verbindung"
            } else {
                "ON AIR – Nicht auf Sendung. Klick: Stream-Verbindung"
            })
            .clicked()
        {
            self.stream_ui.open(&self.config.stream, &self.data_dir);
        }
        // #400: Notfall-Playlist sendet → deutlich sichtbar, mit Grund.
        let now = woa_stream::unix_now();
        if let Some(line) = self
            .radio
            .as_ref()
            .and_then(RadioDesk::stream_status)
            .filter(|s| s.is_running(now))
            .and_then(woa_stream::StreamStatus::emergency_line)
        {
            ui.label(
                egui::RichText::new(line)
                    .strong()
                    .color(theme::danger_text()),
            );
        }
    }

    pub(super) fn menu_items(&mut self, ui: &mut egui::Ui) {
        // Reihenfolge je Gruppe: `MENU_GROUPS` (alphabetisch, ohne „…“).
        theme::logo_mark(ui, 20.0).on_hover_text("WOA - Wir on Air");
        ui.add_space(4.0);
        let r = ui.menu_button("Datei", |ui| {
            if ui.button("Neu (Strg+N)").clicked() {
                self.new_project();
            }
            if ui.button("Öffnen (Strg+O)").clicked() {
                self.open();
            }
            if ui.button("Speichern (Strg+S)").clicked() {
                self.save();
            }
            if ui.button("Speichern unter").clicked() {
                self.save_as();
            }
            ui.separator();
            ui.menu_button("Exportieren", |ui| {
                if ui.button("Als DAWproject").clicked() {
                    self.export_dawproject();
                }
                if ui.button("MIDI-Clip (.mid)").clicked() {
                    self.export_midi();
                }
                if ui.button("Mixdown exportieren (Strg+E)").clicked() {
                    self.export();
                }
            });
            ui.menu_button("Importieren", |ui| {
                if ui.button("Audio-Datei (WAV, FLAC, MP3, OGG)").clicked() {
                    self.import_audio();
                }
                if ui.button("DAWproject öffnen").clicked() {
                    self.open_dawproject();
                }
                if ui.button("MIDI-Datei (.mid)").clicked() {
                    self.import_midi();
                }
            });
            ui.separator();
            if ui.button("Beenden (Strg+Q)").clicked() {
                self.quit(ui.ctx());
            }
        });
        self.photo_menu(ui, "Datei", &r.response);
        let r = ui.menu_button("Bearbeiten", |ui| {
            let h = self.state.session().history();
            let undo = format!("Rückgängig {}", h.undo_label().unwrap_or(""));
            let redo = format!("Wiederholen {}", h.redo_label().unwrap_or(""));
            if ui.button(undo).clicked() {
                self.state.undo();
            }
            if ui.button(redo).clicked() {
                self.state.redo();
            }
            ui.separator();
            if ui.button("Leeren MIDI-Clip anlegen (4 Takte)").clicked() {
                self.create_midi_clip_at_playhead(self.selected_track);
            }
        });
        self.photo_menu(ui, "Bearbeiten", &r.response);
        let r = ui.menu_button("Ansicht", |ui| {
            if ui
                .checkbox(&mut self.library.open, "Bibliothek")
                .on_hover_text(
                    "Musik, Samples und Jingles suchen, taggen, vorhören und hineinziehen",
                )
                .clicked()
            {
                ui.close();
            }
            if self.config.mode == Mode::Produzent
                && ui
                    .checkbox(&mut self.browser.open, "Browser")
                    .on_hover_text("Browser links: Klänge, Beats, Akkorde, FX, Samples, Jingles")
                    .clicked()
            {
                ui.close();
            }
            // Party-Karten rechts (#361); der Zustand wird gemerkt.
            let party = self.config.mode == Mode::Party;
            if party
                && ui
                    .checkbox(&mut self.party.panels.remote, "Fernbedienung")
                    .on_hover_text(
                        "Fernbedienung in der Party zeigen; ausgeblendet läuft eine \
                         eingeschaltete weiter",
                    )
                    .clicked()
            {
                ui.close();
            }
            if ui
                .checkbox(&mut self.hoerer.open, "Hörer-Statistik")
                .on_hover_text("Hörer jetzt, Verlauf der Sendung und vergangene Sendungen (vom eigenen Server)")
                .clicked()
            {
                ui.close();
            }
            let mut clock = self.clock_editor.open;
            if ui
                .checkbox(&mut clock, "Stundenuhr")
                .on_hover_text("Stundenuhren, Wochenplan und AutoDJ für den Radio-Modus")
                .clicked()
            {
                self.clock_editor.toggle();
                ui.close();
            }
            if party
                && ui
                    .checkbox(&mut self.party.panels.wishes, "Wunschliste")
                    .on_hover_text(
                        "Wunschliste in der Party zeigen; ausgeblendet wächst die Bibliothek",
                    )
                    .clicked()
            {
                ui.close();
            }
            let mut events = self.event_editor.open;
            if ui
                .checkbox(&mut events, "Zeit-Events")
                .on_hover_text("Nachrichten, Werbung und Stundenjingle zur festen Zeit")
                .clicked()
            {
                self.event_editor.toggle();
                ui.close();
            }
            ui.separator();
            self.theme_menu(ui);
        });
        self.photo_menu(ui, "Ansicht", &r.response);
        let r = ui.menu_button("Einstellungen", |ui| {
            if ui
                .button(format!(
                    "{} Alle Einstellungen (Strg+,)",
                    theme::ic::GEAR_SIX
                ))
                .on_hover_text("Fenster mit Bereichen und Filter, der jede Einstellung findet")
                .clicked()
            {
                self.settings.open_area(self.mode_area());
                ui.close();
            }
            // Abkürzungen aus dem Register: springen an die Stelle im Fenster (2 s markiert).
            for (area, entries) in crate::settings::menu_entries(self.config.entwickler) {
                ui.separator();
                ui.label(
                    egui::RichText::new(area.name())
                        .small()
                        .color(theme::text_dim()),
                );
                for e in entries {
                    if ui.button(e.name).on_hover_text(e.desc).clicked() {
                        self.settings.goto(e.id, self.now);
                        ui.close();
                    }
                }
            }
        });
        self.photo_menu(ui, "Einstellungen", &r.response);
        if self.config.entwickler {
            self.entwickler_menu(ui);
        }
        let r = ui.menu_button("Hilfe", |ui| {
            self.update_menu(ui);
            if ui.button("Protokoll").clicked() {
                self.protokoll.open = true;
                ui.close();
            }
            ui.separator();
            if ui.button("Über WOA").clicked() {
                self.about_open = true;
                ui.close();
            }
        });
        self.photo_menu(ui, "Hilfe", &r.response);
    }

    /// Menü „Entwickler“ (nur freigeschaltet), direkt vor „Hilfe“.
    pub(super) fn entwickler_menu(&mut self, ui: &mut egui::Ui) {
        let r = ui.menu_button("Entwickler", |ui| {
            if ui
                .button("Abnahme")
                .on_hover_text("Was abzunehmen ist, je Ansicht; Daumen hoch oder runter geht sofort ans Projekt")
                .clicked()
            {
                self.abnahme.open_window(ui.ctx());
                ui.close();
            }
            if ui.button("Entwickler-Modus beenden").clicked() {
                self.entwickler_beenden();
                ui.close();
            }
            if ui
                .button("Fragen und To-dos")
                .on_hover_text("Inhaltliche Entscheidungen mit Vorschlägen, je Oberfläche")
                .clicked()
            {
                self.fragen.open_window(ui.ctx());
                ui.close();
            }
            if ui
                .add_enabled(
                    !self.update.busy(),
                    egui::Button::new("Nach Updates suchen"),
                )
                .clicked()
            {
                self.check_updates_now(ui.ctx());
                ui.close();
            }
        });
        self.photo_menu(ui, "Entwickler", &r.response);
    }

    /// Fotomodus `WOA_MENU=<Name>`: öffnet dieses Menü der Menüleiste für ein Bildschirmfoto.
    pub(super) fn photo_menu(&self, ui: &egui::Ui, name: &str, button: &egui::Response) {
        if self.photo.as_ref().is_some_and(|(_, n)| *n == 3)
            && std::env::var("WOA_MENU").is_ok_and(|v| v == name)
        {
            egui::Popup::open_id(ui.ctx(), egui::Popup::default_response_id(button));
        }
    }

    /// Schaltet den Entwickler-Modus aus: Kennwort aus dem Schlüsselbund, Menü weg.
    pub(super) fn entwickler_beenden(&mut self) {
        crate::fragen::EntwicklerTor::lock(&self.data_dir);
        self.config.entwickler = false;
        self.fragen.open = false;
        self.abnahme.open = false;
        self.save_config();
        tracing::info!("developer mode off");
    }

    /// Fenster „Entwickler-Modus“ (Einstellungen) und „Fragen und To-dos“.
    pub(super) fn entwickler_windows(&mut self, ctx: &egui::Context) {
        match self.entwickler_tor.show(ctx, self.config.entwickler) {
            crate::fragen::TorAktion::Keine => {}
            crate::fragen::TorAktion::Freischalten(k) => {
                match crate::fragen::EntwicklerTor::unlock(&self.data_dir, &k) {
                    Ok(()) => {
                        self.config.entwickler = true;
                        self.save_config();
                        self.entwickler_tor
                            .set_note("Freigeschaltet: Menü „Entwickler“ steht vor „Hilfe“.");
                        tracing::info!("developer mode on");
                    }
                    Err(e) => self.entwickler_tor.set_note(e),
                }
            }
            crate::fragen::TorAktion::Beenden => {
                self.entwickler_beenden();
                self.entwickler_tor.set_note("Entwickler-Modus ist aus.");
            }
        }
        if self.config.entwickler {
            self.fragen.show(ctx, &self.config, &self.data_dir);
            self.abnahme.show(ctx, &self.config, &self.data_dir);
        }
    }
}
