//! Plugin-Fenster, Analyse, Studio-Dock, Mischpult und Effekt-Racks.

use super::*;

impl WoaApp {
    /// Plugin-Fenster (Paket 32), jedes Bild: Reglerbewegungen aus den Fenstern werden ein
    /// Rückgängig-Schritt (ein Regler: sein neuer Wert; ein Preset-Wechsel: der ganze Zustand),
    /// die Plugins bekommen ihre Zeitgeber, und nach „Rückgängig“ gleicht das Rack die laufenden
    /// Plugins mit dem Projekt ab.
    pub(super) fn plugin_frame(&mut self, ctx: &egui::Context) {
        let Some(player) = self.player.as_mut() else {
            return;
        };
        let edits = player.pump_plugins();
        for (insert, edit) in edits {
            let Some(track) = self
                .state
                .project()
                .tracks
                .iter()
                .find(|t| t.plugin(insert).is_some())
                .map(|t| t.id)
            else {
                continue;
            };
            match edit {
                woa_plugins::ParamEdit::Param { id, new, .. } => {
                    self.state.set_plugin_param(track, insert, id, new);
                }
                woa_plugins::ParamEdit::WholeState => {
                    if let Some(cmd) = player.capture_plugin_slot(self.state.project(), insert) {
                        self.state.apply_plugin_state(vec![cmd]);
                    }
                }
            }
        }
        player.reconcile_plugins(self.state.project());
        for insert in player.new_plugin_failures() {
            let name = self
                .state
                .project()
                .tracks
                .iter()
                .find_map(|t| t.plugin(insert))
                .map_or_else(|| "Plugin".to_string(), |s| s.name.clone());
            self.state.status = Status::Error(format!(
                "{name}: Plugin abgestürzt – neu laden. Der Rest läuft weiter."
            ));
        }
        if player.has_isolated_plugins() {
            // Die Aufsicht über die Plugin-Prozesse läuft in diesem Takt.
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
        }
        if player.any_plugin_gui_open() {
            // Offene Plugin-Fenster brauchen den Takt für Zeitgeber und Reglermeldungen.
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
        }
    }

    /// Holt je Bild die Samples des Abgriffs fürs Analyse-Fenster. Ist es zu, sind alle
    /// Abgriffe aus.
    pub(super) fn analysis_frame(&mut self) {
        let Some(player) = self.player.as_mut() else {
            return;
        };
        if !self.analysis_open {
            player.set_tap_source(None);
            return;
        }
        let rate = player.sample_rate();
        let view = self
            .analysis
            .get_or_insert_with(|| crate::analysis::AnalysisView::new(rate));
        view.set_rate(rate);
        player.set_tap_source(Some(view.source()));
        let (l, r) = &mut self.analysis_buf;
        l.clear();
        r.clear();
        view.add_lost(player.drain_tap(l, r));
        if l.is_empty() {
            view.idle();
        } else {
            view.feed(l, r);
        }
    }

    /// Pegel der Summe für Transport und Pegelansicht: höchster Spur- oder Aufnahmepegel.
    pub(super) fn sum_level(&self) -> f32 {
        self.track_levels
            .values()
            .copied()
            .fold(self.level, f32::max)
    }

    /// Das schwebende Analyse-Fenster.
    pub(super) fn analysis_window(&mut self, ctx: &egui::Context) {
        if !self.analysis_open {
            return;
        }
        let tracks: Vec<(TrackId, String)> = self
            .state
            .project()
            .tracks
            .iter()
            .map(|t| (t.id, t.name.clone()))
            .collect();
        let playing = self.player.as_ref().is_some_and(Player::is_playing);
        let sum = self.sum_level();
        let mut open = true;
        let mut vu_view = self.config.vu_view;
        if let Some(view) = self.analysis.as_mut() {
            let shown = egui::Window::new("Analyse")
                .open(&mut open)
                .default_size([760.0, 540.0])
                .min_size([420.0, 360.0])
                .resizable(true)
                .frame(theme::card_frame())
                .show(ctx, |ui| {
                    level_view(ui, &mut vu_view, sum);
                    ui.add_space(4.0);
                    view.ui(ui, &tracks, playing);
                });
            // #441: Esc schließt die Analyse wie die übrigen Fenster.
            if let Some(r) = &shown
                && crate::esc::fenster(ctx, r.response.layer_id, "analyse")
            {
                open = false;
            }
        }
        self.analysis_open = open;
        if vu_view != self.config.vu_view {
            self.config.vu_view = vu_view;
            self.save_config();
        }
    }

    /// Unteres Ableton-Style Studio-Dock: Umschaltbar zwischen Studio-Mixer-Konsole und
    /// angedockter Piano-Roll (oder eingeklappt für maximale Zeitleisten-Höhe).
    pub(super) fn studio_dock_panel(&mut self, ui: &mut egui::Ui) {
        // Vorgabehöhe: Rack samt „+ Effekt“ und Ausgang ohne Rollen. Bis #396 wuchs das Dock
        // beim ersten Bild von selbst auf diese Höhe (und nach jedem Kleinerziehen wieder).
        const DOCK_DEFAULT_H: f32 = 400.0;
        let is_open = self.studio_dock != StudioDockTab::Closed;
        let panel = egui::Panel::bottom("studio_dock")
            .resizable(is_open)
            .default_size(if is_open { DOCK_DEFAULT_H } else { 32.0 })
            .size_range(if is_open { 175.0..=440.0 } else { 32.0..=32.0 })
            .frame(
                egui::Frame::new()
                    .fill(theme::panel())
                    .stroke(egui::Stroke::new(1.0, theme::border()))
                    .inner_margin(egui::Margin::symmetric(12, 6)),
            );
        panel.show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                theme::status_pill(ui, "STUDIO-DOCK", theme::accent(), is_open);

                // Reiter als Segmentschalter mit Symbol (Design-Runde 3); ein Klick auf den
                // offenen Reiter klappt das Dock zu.
                ui.scope(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    for (tab, icon, label, tip) in [
                        (
                            StudioDockTab::Mixer,
                            theme::ic::SLIDERS_HORIZONTAL,
                            "Studio-Mixer",
                            "Vertikale Mischpult-Kanalzüge für alle Spuren",
                        ),
                        (
                            StudioDockTab::PianoRoll,
                            theme::ic::PIANO_KEYS,
                            "Piano-Roll",
                            "Angedockter MIDI-Noten-Editor wie in Ableton Live",
                        ),
                        (
                            StudioDockTab::Automation,
                            theme::ic::WAVE_SINE,
                            "Automation",
                            "Kurven für Lautstärke, Pan, Sends und Effekt-Regler der gewählten Spur",
                        ),
                        (
                            StudioDockTab::Chat,
                            theme::ic::CHAT_CIRCLE,
                            "KI-Chat",
                            "Mit der gewählten KI sprechen; sie bedient das offene Projekt",
                        ),
                    ] {
                        let active = self.studio_dock == tab;
                        if crate::widgets::Segment::new(icon, label)
                            .on(active)
                            .ui(ui)
                            .on_hover_text(tip)
                            .clicked()
                        {
                            self.studio_dock = if active { StudioDockTab::Closed } else { tab };
                        }
                    }
                });

                // Analyse als fünftes Segment; sie öffnet weiter ein eigenes Fenster.
                if crate::widgets::Segment::new(theme::ic::CHART_BAR, "Analyse")
                    .on(self.analysis_open)
                    .ui(ui)
                    .on_hover_text("Lautheit, True Peak, Spektrum, Spektrogramm und Stereo-Scope")
                .clicked()
                {
                    self.analysis_open = !self.analysis_open;
                }

                if self.studio_dock == StudioDockTab::PianoRoll {
                    // Nur Symbol mit Tooltip (Design `IconButton`); „an“ = schwebt als Fenster.
                    let (icon, tip) = if self.piano_roll_floating {
                        (theme::ic::ARROWS_IN, "Piano-Roll wieder im Dock einbetten")
                    } else {
                        (theme::ic::ARROWS_OUT, "Piano-Roll als eigenes Fenster lösen")
                    };
                    if crate::widgets::IconButton::new(icon, tip)
                        .on(self.piano_roll_floating)
                        .ui(ui)
                        .clicked()
                    {
                        self.piano_roll_floating = !self.piano_roll_floating;
                        // Die Wahl wird gemerkt (André 02.11.2026); Vorgabe ist das Dock.
                        self.config.piano_roll_window = self.piano_roll_floating;
                        self.save_config();
                    }
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let (icon, tip) = if is_open {
                        (theme::ic::CARET_DOWN, "Dock einklappen")
                    } else {
                        (theme::ic::CARET_UP, "Dock ausklappen (Studio-Mixer)")
                    };
                    if crate::widgets::IconButton::new(icon, tip).ui(ui).clicked() {
                        self.studio_dock = if is_open {
                            StudioDockTab::Closed
                        } else {
                            StudioDockTab::Mixer
                        };
                    }
                });
            });

            if !is_open {
                return;
            }
            ui.add_space(4.0);
            // #396: Der Inhalt bekommt genau die Fläche, die das Dock hat, und wird darüber
            // hinaus beschnitten. Sonst meldet er seine Mindesthöhe an das Panel zurück, das
            // daraufhin wächst: Das Dock wanderte nach dem Kleinerziehen wieder nach oben.
            let body = ui.available_rect_before_wrap();
            let mut inner = ui.new_child(
                egui::UiBuilder::new()
                    .id_salt("studio_dock_body")
                    .max_rect(body)
                    .layout(egui::Layout::top_down(egui::Align::Min)),
            );
            // Nur unten beschneiden; seitlich gilt weiter der Rahmen des Panels.
            let mut clip = ui.clip_rect();
            clip.max.y = clip.max.y.min(body.max.y);
            inner.set_clip_rect(clip);
            self.studio_dock_body(&mut inner);
            ui.advance_cursor_after_rect(body);
        });
    }

    /// Inhalt des offenen Studio-Docks (Reiter); Fläche fest, siehe `studio_dock_panel`.
    pub(super) fn studio_dock_body(&mut self, ui: &mut egui::Ui) {
        {
            match self.studio_dock {
                StudioDockTab::Closed => {}
                StudioDockTab::Mixer => self.mixer_console(ui),
                StudioDockTab::PianoRoll => self.docked_piano_roll(ui),
                StudioDockTab::Automation => self.docked_automation(ui),
                StudioDockTab::Chat => {
                    let project = self
                        .state
                        .session()
                        .path()
                        .map(std::path::Path::to_path_buf);
                    if self
                        .chat_ui
                        .dock(ui, &self.config.ki, &self.data_dir, project.as_deref())
                    {
                        self.settings
                            .goto(crate::settings::SettingId::Provider(0), self.now);
                    }
                }
            }
        }
    }

    /// Vertikale Mischpult-Konsole im unteren Studio-Dock: schlanke Kanalzüge (nur Name,
    /// Tasten, LED-Kette und Fader), rechts daneben das Insert-Rack des gewählten Kanals.
    pub(super) fn mixer_console(&mut self, ui: &mut egui::Ui) {
        let tracks: Vec<_> = self
            .state
            .project()
            .tracks
            .iter()
            .map(|t| (t.id, t.name.clone(), t.gain_db, t.mute, t.solo))
            .collect();
        const RACK_W: f32 = 340.0;
        // #443: Trennstrich (6 px) und die Abstände links und rechts davon gehören mit in die
        // Rechnung; mit pauschal 12 px stand das Rack 10 px über dem rechten Rand des Docks.
        let sep_w = 6.0 + 2.0 * ui.spacing().item_spacing.x;
        let strips_w = (ui.available_width() - RACK_W - sep_w).max(160.0);
        ui.horizontal_top(|ui| {
            egui::ScrollArea::horizontal()
                .id_salt("studio_mixer_scroll")
                .max_width(strips_w)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 8.0;
                        let empty = tracks.is_empty();
                        for (i, (id, name, gain, mute, solo)) in tracks.into_iter().enumerate() {
                            self.channel_strip(ui, i, id, &name, gain, mute, solo);
                        }
                        if !empty {
                            self.add_bus_box(ui);
                        }
                    });
                });
            ui.add(egui::Separator::default().spacing(6.0));
            ui.vertical(|ui| {
                ui.set_width(RACK_W);
                // „Ausgang / Pan“ steht fest unten im Rack und ist immer ganz sichtbar; wird der
                // Rest zu lang (viele Effekte, kleines Dock), rollt nur er (André 02.11.2026).
                // Höhe des Fußes aus dem letzten Bild (erstes Bild: Schätzung).
                let foot_id = egui::Id::new("studio_rack_footer_h");
                let footer_h = ui
                    .ctx()
                    .data(|d| d.get_temp::<f32>(foot_id))
                    .unwrap_or(120.0);
                let has_track = self.rack_track().is_some();
                let body_h = if has_track {
                    (ui.available_height() - footer_h - 4.0 - ui.spacing().item_spacing.y).max(48.0)
                } else {
                    ui.available_height()
                };
                egui::ScrollArea::vertical()
                    .id_salt("studio_insert_rack_scroll")
                    .max_height(body_h)
                    .auto_shrink([false, false])
                    .show(ui, |ui| self.insert_rack(ui));
                let top = ui.cursor().top();
                self.rack_footer(ui);
                let h = ui.min_rect().bottom() - top;
                ui.ctx().data_mut(|d| d.insert_temp(foot_id, h.max(0.0)));
            });
        });
    }

    /// Ein Kanalzug ohne Drehregler: Scribble-Strip in Spurfarbe, Tasten M, S und Aufnahme,
    /// „fx n“, LED-Kette und Fader. Gain, Pan, Routing und Effekte liegen im Insert-Rack.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn channel_strip(
        &mut self,
        ui: &mut egui::Ui,
        index: usize,
        id: TrackId,
        name: &str,
        gain: f64,
        mute: bool,
        solo: bool,
    ) {
        const W: f32 = 96.0;
        let color = theme::track_color(index);
        // Aufnahmeziel ohne Scharfschaltung bleibt die gewählte Spur; hervorgehoben ist die Spur,
        // die das Insert-Rack zeigt (gewählt, sonst die erste).
        let armed = if self.armed.is_empty() {
            self.selected_track == Some(id)
        } else {
            self.armed.contains(&id)
        };
        let is_sel = self.rack_track() == Some(id);
        let level = self.track_levels.get(&id).copied().unwrap_or(0.0);
        let (fx, fx_error) = self.state.project().track(id).map_or((0, false), |t| {
            let error = t.plugins.iter().any(|s| {
                matches!(
                    self.player.as_ref().and_then(|p| p.plugin_status(s.id)),
                    Some(woa_plugins::SlotStatus::Missing | woa_plugins::SlotStatus::Failed(_))
                )
            });
            (t.inserts.len() + t.plugins.len(), error)
        });

        let strip = egui::Frame::new()
            .fill(if is_sel {
                theme::selection_fill(color)
            } else {
                theme::well()
            })
            .stroke(if is_sel {
                theme::selection_stroke(color)
            } else {
                egui::Stroke::new(1.0, theme::border())
            })
            .corner_radius(5)
            .inner_margin(egui::Margin::symmetric(8, 6))
            .show(ui, |ui| {
                ui.set_width(W);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 4.0;
                    // Scribble-Strip: Name in Spurfarbe.
                    let (bar_rect, bar_resp) =
                        ui.allocate_exact_size(egui::vec2(W, 18.0), egui::Sense::click());
                    ui.painter().rect_filled(bar_rect, 3.0, color);
                    ui.painter().with_clip_rect(bar_rect).text(
                        bar_rect.center(),
                        egui::Align2::CENTER_CENTER,
                        name,
                        egui::FontId::proportional(11.5),
                        theme::well(),
                    );
                    let bar_resp = bar_resp.on_hover_text("Rechtsklick: Spurmenü");
                    bar_resp.widget_info(|| {
                        egui::WidgetInfo::labeled(egui::WidgetType::Label, true, name)
                    });
                    self.track_menu(&bar_resp, id, name);

                    // M / S / Aufnahme
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        let key_size = egui::vec2(28.0, 22.0);
                        let key_font = theme::label_font(13.0);
                        if crate::widgets::Key::new("M", key_size)
                            .font(key_font.clone())
                            .color(theme::accent())
                            .on(mute)
                            .ui(ui)
                            .on_hover_text("Stumm")
                            .clicked()
                        {
                            self.state.toggle_mute(id);
                        }
                        if crate::widgets::Key::new("S", key_size)
                            .font(key_font)
                            .color(theme::cue())
                            .on(solo)
                            .ui(ui)
                            .on_hover_text("Solo")
                            .clicked()
                        {
                            self.state.toggle_solo(id);
                        }
                        if crate::widgets::Key::new(theme::icf::RECORD, key_size)
                            .font(theme::fill_icon_font(12.0))
                            .color(theme::on_air_fill())
                            .on(armed)
                            .ui(ui)
                            .on_hover_text("Aufnahme scharf schalten")
                            .clicked()
                            && !self.armed.remove(&id)
                        {
                            self.armed.insert(id);
                        }
                    });

                    // „fx n“: Zahl der Effekte und Plugins; Klick wählt die Spur fürs Rack.
                    let (fx_text, fx_color) = if fx_error {
                        (format!("{} fx {fx}", theme::ic::WARNING), theme::on_air())
                    } else if fx == 0 {
                        (format!("fx {fx}"), theme::text_dim())
                    } else {
                        (format!("fx {fx}"), theme::text())
                    };
                    let fr = ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new(fx_text)
                                    .monospace()
                                    .size(10.0)
                                    .color(fx_color),
                            )
                            .frame_when_inactive(false)
                            .min_size(egui::vec2(W, 16.0)),
                        )
                        .on_hover_text(
                            "Effekte und Plugins dieser Spur: Klick zeigt das Insert-Rack",
                        );
                    if fr.clicked() {
                        self.select_track(id);
                    }

                    // LED-Kette und Kanalfader (derselbe Gain wie das Zahlenfeld im Rack).
                    ui.horizontal(|ui| {
                        let (m_rect, _) =
                            ui.allocate_exact_size(egui::vec2(14.0, 110.0), egui::Sense::hover());
                        theme::led_meter_vertical(ui.painter(), m_rect, level);
                        let mut fg = match self.gain_drag {
                            Some((d, v)) if d == id => v,
                            _ => self.shown_gain(id, gain),
                        } as f32;
                        let fr = crate::widgets::fader(
                            ui,
                            &mut fg,
                            MIN_GAIN_DB as f32..=MAX_GAIN_DB as f32,
                            0.0,
                            egui::vec2(48.0, 110.0),
                            crate::widgets::Orient::Vertical,
                        )
                        .on_hover_text("Kanal-Gain (Doppelklick: 0 dB)");
                        if fr.changed() {
                            if fr.double_clicked() {
                                self.gain_drag = None;
                                self.state.set_gain(id, 0.0);
                            } else if fr.dragged() {
                                self.gain_drag = Some((id, f64::from(fg)));
                            } else {
                                self.gain_drag = None;
                                self.state.set_gain(id, f64::from(fg));
                            }
                        } else if fr.drag_stopped()
                            && let Some((d, v)) = self.gain_drag.take()
                            && d == id
                        {
                            self.state.set_gain(id, v);
                        }
                    });
                });
            });
        // Ein Klick irgendwo in den Kanalzug wählt die Spur (Rechtsklick auf den Namen: Spurmenü).
        if strip.response.contains_pointer() && ui.input(|i| i.pointer.primary_clicked()) {
            self.select_track(id);
        }
    }

    /// Insert-Rack der gewählten Spur (Design `InsertRack`), von oben nach unten im Signalfluss:
    /// Kopf, Eingang (Quelle, Trim), Inserts, Sends, Ausgang (Ziel, Pan).
    pub(super) fn insert_rack(&mut self, ui: &mut egui::Ui) {
        // Das Rack zeigt immer die gewählte Spur (ohne Auswahl die erste), wie die Hervorhebung.
        let sel = self
            .rack_track()
            .and_then(|id| self.state.project().tracks.iter().position(|t| t.id == id));
        let Some(index) = sel else {
            // Leer-Zustand: keine Spur im Projekt.
            ui.add_space(16.0);
            ui.vertical_centered(|ui| {
                ui.label(
                    egui::RichText::new(theme::ic::FADERS)
                        .size(32.0)
                        .color(theme::text_dim()),
                );
                ui.label(egui::RichText::new("Keine Spur").strong());
                ui.label(
                    egui::RichText::new("Lege eine Spur an, dann stehen hier ihre Effekte.")
                        .small()
                        .color(theme::text_dim()),
                );
                // Untereinander statt nebeneinander: eine zentrierte Zeile braucht in egui
                // einen zweiten Durchlauf, und der erste Klick ginge ins Leere.
                if ui.button("+ Audio-Spur").clicked() {
                    self.state.add_track();
                }
                if ui.button("+ MIDI-Spur").clicked() {
                    self.state.add_midi_track();
                }
            });
            return;
        };
        let (id, name, kind) = {
            let t = &self.state.project().tracks[index];
            (t.id, t.name.clone(), t.kind)
        };
        let color = theme::track_color(index);
        ui.spacing_mut().item_spacing.y = 4.0;
        // Kopf: wie die Auswahl (14 %, 1-px-Rahmen), dazu eine 3-px-Oberkante in Spurfarbe.
        let head = egui::Frame::new()
            .fill(theme::selection_fill(color))
            .stroke(theme::selection_stroke(color))
            .corner_radius(5)
            .inner_margin(egui::Margin {
                left: 8,
                right: 4,
                top: 6,
                bottom: 4,
            })
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let (r, _) =
                        ui.allocate_exact_size(egui::vec2(12.0, 16.0), egui::Sense::hover());
                    ui.painter().circle_filled(r.center(), 5.0, color);
                    ui.label(egui::RichText::new(&name).strong().color(theme::text()));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.menu_button(theme::ic::DOTS_THREE, |ui| {
                            self.track_menu_items(ui, id, &name);
                        })
                        .response
                        .on_hover_text("Spurmenü");
                        let (icon, art) = match kind {
                            woa_model::TrackKind::Midi => (theme::ic::PIANO_KEYS, "MIDI"),
                            woa_model::TrackKind::Bus => (theme::ic::ARROWS_MERGE, "Bus"),
                            _ => (theme::ic::WAVEFORM, "Audio"),
                        };
                        ui.label(
                            egui::RichText::new(format!("{icon} {art}"))
                                .small()
                                .color(theme::text_dim()),
                        );
                    });
                });
            });
        let hr = head.response.rect;
        ui.painter().rect_filled(
            egui::Rect::from_min_size(hr.min, egui::vec2(hr.width(), 3.0)),
            egui::CornerRadius {
                nw: 5,
                ne: 5,
                sw: 0,
                se: 0,
            },
            color,
        );
        self.track_menu(&head.response, id, &name);

        // Eingang: Quelle und Trim. Der Trim ist ein eigener Wert vor den Inserts (Design-Runde 3),
        // der Spur-Gain bleibt am Fader und im Spurkopf. Ein Bus hat keinen Eingang.
        if kind != woa_model::TrackKind::Bus {
            ui.label(rack_title("EINGANG"));
            ui.horizontal(|ui| {
                self.source_combo(ui, id, kind == woa_model::TrackKind::Midi, 130.0);
                self.trim_field(ui, id);
            });
        }

        if kind == woa_model::TrackKind::Midi {
            self.instrument_section(ui, id, color);
        }
        self.strip_effects(ui, id, kind);
    }

    /// Fuß des Racks: Ausgang und Pan der gewählten Spur, außerhalb der Rollfläche.
    pub(super) fn rack_footer(&mut self, ui: &mut egui::Ui) {
        let Some(id) = self.rack_track() else {
            return;
        };
        let Some(pan) = self
            .state
            .project()
            .tracks
            .iter()
            .find(|t| t.id == id)
            .map(|t| t.pan)
        else {
            return;
        };
        ui.spacing_mut().item_spacing.y = 4.0;
        self.strip_routing(ui, id);
        self.pan_field(ui, id, pan, "Pan ");
    }

    /// „+ Bus“ (Design `MixerBus`): gestrichelter Platz in voller Kanalzug-Höhe; ein Klick legt
    /// den Bus an („Bus n“, umbenennen über das Spurmenü).
    pub(super) fn add_bus_box(&mut self, ui: &mut egui::Ui) {
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(64.0, 196.0), egui::Sense::click());
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "+ Bus"));
        let hot = resp.hovered();
        let stroke = egui::Stroke::new(
            1.0,
            if hot {
                theme::text_dim()
            } else {
                theme::border_strong()
            },
        );
        let r = rect.shrink(0.5);
        for (a, b) in [
            (r.left_top(), r.right_top()),
            (r.right_top(), r.right_bottom()),
            (r.right_bottom(), r.left_bottom()),
            (r.left_bottom(), r.left_top()),
        ] {
            ui.painter()
                .extend(egui::Shape::dashed_line(&[a, b], stroke, 4.0, 3.0));
        }
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "+ Bus",
            egui::FontId::proportional(12.0),
            if hot {
                theme::text()
            } else {
                theme::text_dim()
            },
        );
        if resp
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text(
                "Bus anlegen: Gruppenspur für Sends und Gruppen (umbenennen im Spurmenü)",
            )
            .clicked()
        {
            self.state.add_bus("");
        }
    }

    /// Ausgang, Sends und Effektkette eines Mischpult-Zugs.
    pub(super) fn strip_routing(&mut self, ui: &mut egui::Ui, id: TrackId) {
        let Some(track) = self.state.project().track(id) else {
            return;
        };
        let output = track.output;
        let sends = track.sends.clone();
        let buses: Vec<(TrackId, String)> = self
            .state
            .project()
            .tracks
            .iter()
            .filter(|b| b.kind == woa_model::TrackKind::Bus && b.id != id)
            .map(|b| (b.id, b.name.clone()))
            .collect();
        let bus_name = |t: TrackId| {
            buses
                .iter()
                .find(|(b, _)| *b == t)
                .map_or_else(|| "?".to_string(), |(_, n)| n.clone())
        };
        let small = |s: &str| {
            egui::RichText::new(s.to_owned())
                .size(theme::TYPE_SMALL)
                .monospace()
                .color(theme::text_dim())
        };

        // Sends
        if !sends.is_empty() {
            ui.label(small("SENDS"));
        }
        for s in &sends {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 3.0;
                let mut db = match self.send_drag {
                    Some((t, d, v)) if t == id && d == s.target => v,
                    _ => s.level_db,
                };
                let r = ui
                    .add(
                        egui::DragValue::new(&mut db)
                            .range(woa_model::MIN_GAIN_DB..=woa_model::MAX_SEND_DB)
                            .speed(0.2)
                            .fixed_decimals(1)
                            .prefix(format!("{} ", bus_name(s.target)))
                            .suffix(" dB"),
                    )
                    .on_hover_text("Send-Pegel (Doppelklick: 0 dB)");
                if r.double_clicked() {
                    self.send_drag = None;
                    self.state.set_send(id, s.target, 0.0, s.pre_fader);
                } else {
                    if r.changed() {
                        self.send_drag = Some((id, s.target, db));
                    }
                    if r.drag_stopped() || (r.changed() && !r.dragged()) {
                        self.send_drag = None;
                        self.state.set_send(id, s.target, db, s.pre_fader);
                    }
                }
                let tap = if s.pre_fader { "PRE" } else { "POST" };
                if ui
                    .small_button(tap)
                    .on_hover_text("Vor (PRE) oder nach (POST) dem Fader abgreifen")
                    .clicked()
                {
                    self.state.set_send(id, s.target, s.level_db, !s.pre_fader);
                }
                if ui
                    .small_button(theme::ic::X)
                    .on_hover_text("Send entfernen")
                    .clicked()
                {
                    self.state.remove_send(id, s.target);
                }
            });
        }
        let free: Vec<_> = buses
            .iter()
            .filter(|(b, _)| !sends.iter().any(|s| s.target == *b))
            .collect();
        if !free.is_empty() {
            ui.menu_button("+ Send", |ui| {
                for (b, n) in free {
                    if ui.button(n).clicked() {
                        self.state.set_send(id, *b, -12.0, false);
                        ui.close();
                    }
                }
            });
        }
        // Ausgang
        ui.label(rack_title("AUSGANG"));
        let current = output.map_or_else(|| "Master".to_string(), bus_name);
        egui::ComboBox::from_id_salt(("out", id.0))
            .selected_text(format!("→ {current}"))
            .width(100.0)
            .show_ui(ui, |ui| {
                if ui.selectable_label(output.is_none(), "Master").clicked() {
                    self.state.set_output(id, None);
                }
                for (b, n) in &buses {
                    if ui.selectable_label(output == Some(*b), n).clicked() {
                        self.state.set_output(id, Some(*b));
                    }
                }
            });
    }

    /// Effektkette und Plugins eines Kanals: hinzufügen, entfernen, umsortieren, Bypass,
    /// Plugin-Fenster. Steht im Insert-Rack oben, damit sie ohne Scrollen sichtbar sind.
    pub(super) fn strip_effects(
        &mut self,
        ui: &mut egui::Ui,
        id: TrackId,
        kind: woa_model::TrackKind,
    ) {
        let Some(track) = self.state.project().track(id) else {
            return;
        };
        let inserts = track.inserts.clone();
        let small = |s: &str| {
            egui::RichText::new(s.to_owned())
                .size(theme::TYPE_SMALL)
                .monospace()
                .color(theme::text_dim())
        };
        // Inserts in Signalreihenfolge (Design `EffectSlot`): Bypass, Name (Klick klappt die
        // Regler auf), Entfernen.
        let n = inserts.len() + track.plugins.len();
        ui.label(small(&format!("INSERTS · {n}")));
        for ins in &inserts {
            let open = self.fx_open.contains(&ins.id);
            egui::Frame::new()
                .stroke(egui::Stroke::new(
                    1.0,
                    if ins.bypass {
                        theme::border()
                    } else {
                        theme::border_strong()
                    },
                ))
                .corner_radius(4)
                .inner_margin(egui::Margin::symmetric(4, 2))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 3.0;
                        if bypass_button(ui, !ins.bypass).clicked() {
                            self.state.toggle_insert_bypass(id, ins.id);
                        }
                        let caret = if open {
                            theme::ic::CARET_DOWN
                        } else {
                            theme::ic::CARET_RIGHT
                        };
                        if ui
                            .add(
                                egui::Button::new(
                                    egui::RichText::new(format!("{caret} {}", ins.kind.label()))
                                        .size(11.0)
                                        .color(if ins.bypass {
                                            theme::text_dim()
                                        } else {
                                            theme::text()
                                        }),
                                )
                                .frame_when_inactive(false),
                            )
                            .on_hover_text("Regler auf- oder zuklappen")
                            .clicked()
                            && !self.fx_open.remove(&ins.id)
                        {
                            self.fx_open.insert(ins.id);
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .small_button(theme::ic::X)
                                .on_hover_text("Effekt entfernen (rückgängig mit Strg+Z)")
                                .clicked()
                            {
                                self.state.remove_insert(id, ins.id);
                            }
                        });
                    });
                });
            if ins.kind.supports_sidechain() {
                let sources: Vec<(TrackId, String)> = self
                    .state
                    .project()
                    .tracks
                    .iter()
                    .filter(|o| o.id != id)
                    .map(|o| (o.id, o.name.clone()))
                    .collect();
                let cur = ins
                    .sidechain
                    .and_then(|s| sources.iter().find(|(o, _)| *o == s))
                    .map_or_else(|| "eigenes Signal".to_string(), |(_, n)| n.clone());
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 3.0;
                    ui.label(small("Sidechain"));
                    if kind == woa_model::TrackKind::Bus {
                        ui.label(small("(nur Spuren)"));
                        return;
                    }
                    egui::ComboBox::from_id_salt(("sc", ins.id.0))
                        .selected_text(cur)
                        .width(110.0)
                        .show_ui(ui, |ui| {
                            if ui
                                .selectable_label(ins.sidechain.is_none(), "eigenes Signal")
                                .clicked()
                            {
                                self.state.set_sidechain(id, ins.id, None);
                            }
                            for (o, n) in &sources {
                                if ui.selectable_label(ins.sidechain == Some(*o), n).clicked() {
                                    self.state.set_sidechain(id, ins.id, Some(*o));
                                }
                            }
                        })
                        .response
                        .on_hover_text(
                            "Pegel einer anderen Spur (nach deren Fader) steuert diesen Effekt, \
                             zum Beispiel Musik unter Sprache absenken",
                        );
                });
            }
            if matches!(
                ins.kind,
                woa_model::EffectKind::Compressor
                    | woa_model::EffectKind::Gate
                    | woa_model::EffectKind::Limiter
                    | woa_model::EffectKind::DeEsser
            ) {
                let now = self
                    .player
                    .as_ref()
                    .map_or(0.0, |p| p.take_reduction(ins.id));
                let shown = self.gr_shown.entry(ins.id).or_insert(0.0);
                *shown = now.max(*shown * 0.9);
                let db = *shown;
                ui.horizontal(|ui| {
                    ui.label(small("Absenkung"));
                    crate::automation::reduction_bar(ui, db);
                });
            }
            if open {
                ui.indent(("fx", ins.id.0), |ui| {
                    ui.menu_button(small(&format!("Preset {}", theme::ic::CARET_DOWN)), |ui| {
                        for pr in ins.kind.presets() {
                            if ui.button(pr.name).clicked() {
                                self.fx_drag = None;
                                self.state.apply_insert_preset(id, ins.id, pr.name);
                                ui.close();
                            }
                        }
                    });
                    for (i, spec) in ins.kind.params().iter().enumerate() {
                        if !spec.choices.is_empty() {
                            let cur = ins.params.get(i).copied().unwrap_or(spec.default);
                            let idx = (cur.round().max(0.0) as usize).min(spec.choices.len() - 1);
                            egui::ComboBox::from_id_salt(("fxc", ins.id.0, i))
                                .selected_text(format!("{} {}", spec.label, spec.choices[idx]))
                                .show_ui(ui, |ui| {
                                    for (n, name) in spec.choices.iter().enumerate() {
                                        if ui.selectable_label(n == idx, *name).clicked() {
                                            self.state.set_insert_param(id, ins.id, i, n as f64);
                                        }
                                    }
                                });
                            continue;
                        }
                        let mut v = match self.fx_drag {
                            Some((f, p, v)) if f == ins.id && p == i => v,
                            _ => ins.params.get(i).copied().unwrap_or(spec.default),
                        };
                        let r = ui
                            .add(
                                egui::DragValue::new(&mut v)
                                    .range(spec.min..=spec.max)
                                    .speed((spec.max - spec.min) / 400.0)
                                    .max_decimals(2)
                                    .prefix(format!("{} ", spec.label))
                                    .suffix(format!(" {}", spec.unit)),
                            )
                            .on_hover_text("Doppelklick: Vorgabe");
                        if r.double_clicked() {
                            self.fx_drag = None;
                            self.state.set_insert_param(id, ins.id, i, spec.default);
                        } else {
                            if r.changed() {
                                self.fx_drag = Some((ins.id, i, v));
                            }
                            if r.drag_stopped() || (r.changed() && !r.dragged()) {
                                self.fx_drag = None;
                                self.state.set_insert_param(id, ins.id, i, v);
                            }
                        }
                    }
                });
            }
        }
        self.strip_plugins(ui, id, kind);
    }

    /// „+ Effekt“ (Design `InsertRack`): Liste mit Suche, eingebaute Effekte und die Sprachkette.
    pub(super) fn add_effect_menu(&mut self, ui: &mut egui::Ui, id: TrackId) {
        ui.menu_button("+ Effekt", |ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.fx_search)
                    .hint_text(format!("{} Suchen", theme::ic::MAGNIFYING_GLASS))
                    .desired_width(160.0),
            );
            let q = self.fx_search.to_lowercase();
            ui.label(rack_title("EINGEBAUT"));
            for k in woa_model::EffectKind::ALL {
                if !q.is_empty() && !k.label().to_lowercase().contains(&q) {
                    continue;
                }
                if ui.button(k.label()).clicked() {
                    self.state.add_insert(id, k);
                    self.fx_search.clear();
                    ui.close();
                }
            }
            ui.separator();
            if ui
                .button("Vorlage Sprachkette")
                .on_hover_text("Hochpass, Gate, EQ, Kompressor, De-Esser in einem Schritt")
                .clicked()
            {
                self.state.add_voice_chain(id);
                self.fx_search.clear();
                ui.close();
            }
            if ui
                .button("Stimme über Bett")
                .on_hover_text(
                    "Sprachkette auf diese Spur, alle anderen Spuren werden leiser, solange die Stimme spricht (Ducking)",
                )
                .clicked()
            {
                self.state.voice_over(id);
                self.fx_search.clear();
                ui.close();
            }
        });
    }

    /// Abschnitt „Instrument“ im Rack (Design `SoundPicker`, Paket J1/J7): Kachel mit Klang,
    /// aufgeklappt Rollen Lead · Bass · Pad · Kit, je Rolle vier Klänge, darunter Klangbank und
    /// eingebauter Synth. Ein CLAP-Instrument ersetzt den Klang; dann steht hier nur sein Name.
    pub(super) fn instrument_section(
        &mut self,
        ui: &mut egui::Ui,
        id: TrackId,
        color: egui::Color32,
    ) {
        use woa_model::{PluginRole, SoundCategory, TrackSound};
        let Some(track) = self.state.project().track(id) else {
            return;
        };
        let current = track.sound;
        let plugin = track
            .plugins
            .iter()
            .find(|s| s.role == PluginRole::Instrument)
            .map(|s| s.name.clone());
        ui.label(rack_title("INSTRUMENT"));
        if let Some(name) = plugin {
            ui.label(
                egui::RichText::new(format!("{} {name}", theme::ic::PLUGS)).color(theme::text()),
            )
            .on_hover_text("Plugin-Instrument; Klang und Bank gelten erst ohne es wieder");
            return;
        }
        let bank = current.and_then(|s| s.bank);
        let role = current.and_then(woa_model::sound_category);
        let open = self.sound_pick.filter(|(t, _)| *t == id).map(|(_, r)| r);
        let (name, sub, icon) = match current {
            None => (
                woa_model::SYNTH_NAME.to_owned(),
                "Eingebaut · ohne Klangbank".to_owned(),
                theme::ic::WAVE_SINE,
            ),
            Some(s) => {
                let name = woa_model::sound_name(Some(s));
                let (r, ic) = role.map_or(("GM", theme::ic::MUSIC_NOTE), category_label);
                let b = bank.map_or("GeneralUser GS", |b| b.name());
                (name, format!("{r} · {b}"), ic)
            }
        };
        // Zu: eine Kachel; ein Klick klappt die Auswahl auf oder zu.
        let tile = egui::Frame::new()
            .fill(theme::surface())
            .stroke(egui::Stroke::new(1.0, theme::border_strong()))
            .corner_radius(theme::RADIUS_MD)
            .inner_margin(egui::Margin::symmetric(6, 5))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    let (sq, _) =
                        ui.allocate_exact_size(egui::vec2(28.0, 28.0), egui::Sense::hover());
                    ui.painter()
                        .rect_filled(sq, 4.0, theme::mix(theme::panel(), color, 0.22));
                    ui.painter().text(
                        sq.center(),
                        egui::Align2::CENTER_CENTER,
                        icon,
                        egui::FontId::proportional(16.0),
                        color,
                    );
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        ui.label(egui::RichText::new(&name).strong().color(theme::text()));
                        ui.label(
                            egui::RichText::new(&sub)
                                .size(theme::TYPE_SMALL)
                                .color(theme::text_dim()),
                        );
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            egui::RichText::new(if open.is_some() {
                                theme::ic::CARET_UP
                            } else {
                                theme::ic::CARET_DOWN
                            })
                            .color(theme::text_dim()),
                        );
                    });
                });
            });
        let tip = format!("Klang wählen: {name}");
        let click = ui
            .interact(
                tile.response.rect,
                ui.id().with(("sound_tile", id.0)),
                egui::Sense::click(),
            )
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text("Klang wählen");
        click.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &tip));
        if click.clicked() {
            self.sound_pick = match open {
                Some(_) => None,
                None => Some((id, role.unwrap_or(SoundCategory::Klavier))),
            };
        }
        let Some(shown) = open else {
            return;
        };
        let mut chosen: Option<Option<TrackSound>> = None;
        // Kategorien als Segmentschalter, vier je Zeile (02.11.2026: statt Lead · Bass · Pad · Kit).
        let gap = ui.spacing().item_spacing.x;
        for row in SoundCategory::ALL.chunks(4) {
            ui.horizontal(|ui| {
                let w = ((ui.available_width() - 3.0 * gap) / 4.0).floor();
                for &r in row {
                    let (label, ic) = category_label(r);
                    // #443: Passt „Symbol Name“ nicht in die Taste (Rack 340 px), steht nur der
                    // Name, statt „Strei…“ abzuschneiden.
                    let full = format!("{ic} {label}");
                    let font = egui::TextStyle::Button.resolve(ui.style());
                    let need = ui
                        .painter()
                        .layout_no_wrap(full.clone(), font, egui::Color32::WHITE)
                        .size()
                        .x
                        + 2.0 * ui.spacing().button_padding.x;
                    let text = if need <= w { full } else { label.to_string() };
                    if ui
                        .add_sized(
                            [w, 26.0],
                            egui::Button::selectable(r == shown, text).truncate(),
                        )
                        .on_hover_text(label)
                        .clicked()
                    {
                        self.sound_pick = Some((id, r));
                    }
                }
            });
        }
        // Suche (Fassung 7): findet Klänge über alle Kategorien.
        ui.add(
            egui::TextEdit::singleline(&mut self.sound_search)
                .hint_text(format!("{} Klang suchen", theme::ic::MAGNIFYING_GLASS))
                .desired_width(f32::INFINITY),
        );
        let presets = sound_matches(&self.sound_search, shown);
        let mut preview = None;
        for pair in presets.chunks(2) {
            ui.horizontal(|ui| {
                // Zwei Pixel Luft: der Vorhören-Knopf darf nie am Rand kleben (#443).
                let w = ((ui.available_width() - gap - 2.0) / 2.0).floor();
                // #443: Mit der Grund-Polsterung war der 26-px-Vorhören-Knopf 40 px breit; die
                // Zeile ragte 28 px über das Rack und schob alles darunter mit hinaus.
                ui.spacing_mut().button_padding.x = 2.0;
                for p in pair {
                    let same = current
                        .is_some_and(|c| c.program == p.sound.program && c.drums == p.sound.drums);
                    let r = ui.add_sized(
                        [w - 26.0 - gap, 30.0],
                        egui::Button::selectable(same, p.name).truncate(),
                    );
                    if ui
                        .add_sized([26.0, 30.0], egui::Button::new(theme::ic::PLAY))
                        .on_hover_text("Vorhören, ohne zu wählen")
                        .clicked()
                    {
                        preview = Some(p.sound);
                    }
                    if r.clicked() {
                        // Die Bank bleibt, solange sie zum Klang passt (Paket J7).
                        let keep = bank.filter(|b| p.sound.drums || !b.drums_only());
                        chosen = Some(Some(TrackSound {
                            bank: keep,
                            ..p.sound
                        }));
                    }
                }
            });
        }
        // Klangbank (Paket J7): GeneralUser GS ist eingebaut, die übrigen kommen beim ersten
        // Gebrauch aus dem Netz; bis dahin spielt GeneralUser GS.
        if let Some(cur) = current {
            let text = bank.map_or_else(
                || "GeneralUser GS (eingebaut)".to_owned(),
                |b| format!("{}{}", b.name(), bank_hint(&woa_engine::banks::state(b))),
            );
            egui::ComboBox::from_id_salt(("track_bank", id.0))
                .selected_text(format!("{} {text}", theme::ic::FOLDER))
                .width(ui.available_width() - 8.0)
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(bank.is_none(), "GeneralUser GS (eingebaut)")
                        .clicked()
                    {
                        chosen = Some(Some(TrackSound { bank: None, ..cur }));
                    }
                    for b in woa_model::SoundBank::ALL {
                        if b.drums_only() && !cur.drums {
                            continue;
                        }
                        let label =
                            format!("{}{}", b.name(), bank_hint(&woa_engine::banks::state(b)));
                        if ui.selectable_label(bank == Some(b), label).clicked() {
                            chosen = Some(Some(TrackSound {
                                bank: Some(b),
                                ..cur
                            }));
                        }
                    }
                })
                .response
                .on_hover_text(match bank {
                    Some(b) => format!(
                        "Bank {}: {}",
                        b.name(),
                        match woa_engine::banks::state(b) {
                            woa_engine::banks::BankState::Ready => "geladen".to_owned(),
                            s => format!(
                                "{}, bis dahin spielt GeneralUser GS",
                                bank_hint(&s).trim_start_matches(" · ")
                            ),
                        }
                    ),
                    None => "Klangbank; GeneralUser GS ist eingebaut".to_owned(),
                });
        }
        if ui
            .add_sized(
                [ui.available_width(), 26.0],
                egui::Button::selectable(
                    current.is_none(),
                    format!("{} {}", theme::ic::WAVE_SINE, woa_model::SYNTH_NAME),
                ),
            )
            .on_hover_text("Eingebauter einfacher Synth ohne Klangbank, lädt nichts")
            .clicked()
        {
            chosen = Some(None);
        }
        if presets.is_empty() {
            ui.weak("Kein Klang gefunden");
        }
        if let Some(p) = preview {
            // Vorhören: der Spurklang bleibt, nur ein kurzer Probeton.
            self.state.probeton.tap(
                id,
                &[crate::probeton::pitch_for(Some(p))],
                100,
                std::time::Instant::now(),
                std::time::Duration::ZERO,
            );
        }
        if let Some(sound) = chosen {
            if let Some(b) = sound.and_then(|s| s.bank) {
                woa_engine::banks::ensure(b);
            }
            self.state.set_track_sound(id, sound);
            // Probeton nach kurzer Pause, damit der neue Klang schon geladen ist.
            self.state.probeton.tap(
                id,
                &[crate::probeton::pitch_for(sound)],
                100,
                std::time::Instant::now(),
                std::time::Duration::from_millis(150),
            );
        }
    }

    /// CLAP-Plugins einer Spur (Paket 31): Instrument und Effekte, Regler, Browser.
    pub(super) fn strip_plugins(
        &mut self,
        ui: &mut egui::Ui,
        id: TrackId,
        kind: woa_model::TrackKind,
    ) {
        use woa_model::PluginRole;
        let Some(track) = self.state.project().track(id) else {
            return;
        };
        let slots = track.plugins.clone();
        let small = |s: &str| {
            egui::RichText::new(s.to_owned())
                .size(theme::TYPE_SMALL)
                .color(theme::text_dim())
        };
        let has_instrument = slots.iter().any(|s| s.role == PluginRole::Instrument);
        if !slots.is_empty() {
            ui.label(small("PLUGINS"));
        }
        for slot in &slots {
            let status = self.player.as_ref().and_then(|p| p.plugin_status(slot.id));
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 3.0;
                if slot.role == PluginRole::Effect {
                    if bypass_button(ui, !slot.bypass).clicked() {
                        self.state.toggle_plugin_bypass(id, slot.id);
                    }
                    ui.label(egui::RichText::new(&slot.name).size(11.0).color(
                        if slot.bypass {
                            theme::text_dim()
                        } else {
                            theme::text()
                        },
                    ));
                } else {
                    ui.label(
                        egui::RichText::new(format!("{} {}", theme::ic::MUSIC_NOTE, slot.name))
                            .size(10.5)
                            .color(theme::text()),
                    );
                }
                ui.label(
                    egui::RichText::new("CLAP")
                        .size(9.0)
                        .monospace()
                        .color(theme::text_dim()),
                )
                .on_hover_text("Plugin-Format");
                if matches!(status, Some(woa_plugins::SlotStatus::Loaded)) {
                    let open = self
                        .player
                        .as_ref()
                        .is_some_and(|p| p.plugin_gui_open(slot.id));
                    if ui
                        .add(egui::Button::selectable(
                            open,
                            egui::RichText::new(theme::ic::ARROW_SQUARE_OUT).size(14.0),
                        ))
                        .on_hover_text("Plugin-Fenster öffnen oder schließen")
                        .clicked()
                        && let Some(p) = self.player.as_mut()
                    {
                        if open {
                            p.close_plugin_gui(slot.id);
                        } else if let Err(e) = p.open_plugin_gui(slot.id, &slot.name) {
                            tracing::warn!(plugin = %slot.clap_id, error = %e, "plugin window not opened");
                            self.state.status = Status::Error(e);
                        }
                    }
                }
                if matches!(status, Some(woa_plugins::SlotStatus::Loaded))
                    && std::env::var_os("WOA_TESTMENU").is_some_and(|v| v == "1")
                    && self
                        .player
                        .as_ref()
                        .is_some_and(|p| p.plugin_isolated(slot.id))
                    && ui
                        .small_button("Absturz simulieren")
                        .on_hover_text("Testmenü: beendet den Plugin-Prozess hart")
                        .clicked()
                    && let Some(p) = self.player.as_mut()
                {
                    p.simulate_plugin_crash(slot.id);
                }
                if ui
                    .small_button(theme::ic::X)
                    .on_hover_text("Plugin entfernen")
                    .clicked()
                {
                    self.state.remove_plugin(id, slot.id);
                }
            });
            match &status {
                Some(woa_plugins::SlotStatus::Missing) => {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(format!(
                                "{} nicht gefunden ({}); die Spur läuft ohne",
                                theme::ic::WARNING,
                                slot.clap_id
                            ))
                            .size(theme::TYPE_SMALL)
                            .color(theme::danger_text()),
                        );
                        if ui
                            .small_button("Plugin suchen")
                            .on_hover_text("Liest die Plugin-Ordner noch einmal")
                            .clicked()
                            && let Some(p) = self.player.as_mut()
                        {
                            p.rescan_plugins();
                        }
                    });
                    continue;
                }
                Some(woa_plugins::SlotStatus::Failed(why)) => {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(format!("{} {why}", theme::ic::WARNING))
                                .size(theme::TYPE_SMALL)
                                .color(theme::danger_text()),
                        );
                        let isolated = self
                            .player
                            .as_ref()
                            .is_some_and(|p| p.plugin_isolated(slot.id));
                        if isolated
                            && ui
                                .small_button("Neu laden")
                                .on_hover_text("Startet das Plugin neu, mit seinem letzten Zustand")
                                .clicked()
                            && let Some(p) = self.player.as_mut()
                            && let Err(e) = p.reload_plugin(self.state.project(), slot.id)
                        {
                            tracing::warn!(plugin = %slot.clap_id, error = %e, "plugin reload failed");
                            self.state.status = Status::Error(format!("{}: {e}", slot.name));
                        }
                        if ui
                            .small_button("Sperren")
                            .on_hover_text(
                                "Die Plugin-Datei wird beim nächsten Suchen nicht mehr geladen \
                                 (unter „+ Plugin“ wieder freigeben)",
                            )
                            .clicked()
                            && let Some(p) = self.player.as_mut()
                            && let Some(file) = p.block_plugin(&slot.clap_id)
                        {
                            self.state.status = Status::Info(format!(
                                "{file} gesperrt; gilt ab dem nächsten Suchen"
                            ));
                        }
                    });
                    self.plugin_log_ui(ui, &slot.clap_id, slot.id);
                    continue;
                }
                _ => self.plugin_log_ui(ui, &slot.clap_id, slot.id),
            }
            let params: Vec<woa_plugins::ParamDesc> = self
                .player
                .as_ref()
                .map_or_else(Vec::new, |p| p.plugin_params(slot.id))
                .into_iter()
                .filter(|p| !p.hidden && !p.read_only)
                .collect();
            if params.is_empty() {
                continue;
            }
            egui::CollapsingHeader::new(small("Regler"))
                .id_salt(("plug", slot.id.0))
                .show(ui, |ui| {
                    const SHOWN: usize = 64;
                    for p in params.iter().take(SHOWN) {
                        let stored = slot.params.iter().find(|q| q.id == p.id).map(|q| q.value);
                        let mut v = match self.plugin_drag {
                            Some((f, q, v)) if f == slot.id && q == p.id => v,
                            _ => stored
                                .or_else(|| {
                                    self.player
                                        .as_mut()
                                        .and_then(|pl| pl.plugin_value(slot.id, p.id))
                                })
                                .unwrap_or(p.default),
                        };
                        let label = if p.module.is_empty() {
                            format!("{} ", p.name)
                        } else {
                            format!("{} / {} ", p.module, p.name)
                        };
                        let mut w = egui::DragValue::new(&mut v)
                            .range(p.min..=p.max)
                            .speed(((p.max - p.min) / 400.0).max(1e-6))
                            .max_decimals(3)
                            .prefix(label);
                        if p.stepped {
                            w = w.max_decimals(0).speed(0.05);
                        }
                        let r = ui.add(w).on_hover_text("Doppelklick: Vorgabe");
                        if r.double_clicked() {
                            self.plugin_drag = None;
                            self.state.set_plugin_param(id, slot.id, p.id, p.default);
                        } else {
                            if r.changed() {
                                self.plugin_drag = Some((slot.id, p.id, v));
                            }
                            if r.drag_stopped() || (r.changed() && !r.dragged()) {
                                self.plugin_drag = None;
                                self.state.set_plugin_param(id, slot.id, p.id, v);
                            }
                        }
                    }
                    if params.len() > SHOWN {
                        ui.label(small(&format!(
                            "… und {} weitere (alle im Plugin-Fenster)",
                            params.len() - SHOWN
                        )));
                    }
                });
        }
        ui.horizontal(|ui| {
            self.add_effect_menu(ui, id);
            ui.menu_button("+ Plugin", |ui| {
                let list = self
                    .player
                    .as_mut()
                    .map_or_else(Vec::new, |p| p.plugin_list());
                if list.is_empty() {
                    ui.label(small(
                    "Keine CLAP-Plugins gefunden (Ordner ~/.clap, /usr/lib/clap oder CLAP_PATH).",
                ));
                }
                for info in &list {
                    let as_instrument = info.is_instrument();
                    let as_effect = info.is_effect() || !as_instrument;
                    if as_instrument && kind == woa_model::TrackKind::Midi && !has_instrument {
                        let label =
                            format!("{} {}  {}", theme::ic::MUSIC_NOTE, info.name, info.vendor);
                        if ui.button(label).on_hover_text("Als Instrument").clicked() {
                            self.state.add_plugin(
                                id,
                                &info.clap_id,
                                &info.name,
                                PluginRole::Instrument,
                            );
                            ui.close();
                        }
                    }
                    if as_effect && kind != woa_model::TrackKind::Bus {
                        let label = format!("fx {}  {}", info.name, info.vendor);
                        if ui.button(label).on_hover_text("Als Effekt").clicked() {
                            self.state.add_plugin(
                                id,
                                &info.clap_id,
                                &info.name,
                                PluginRole::Effect,
                            );
                            ui.close();
                        }
                    }
                }
                ui.separator();
                if ui
                    .button("Neu suchen")
                    .on_hover_text("Liest die Plugin-Ordner noch einmal")
                    .clicked()
                {
                    if let Some(p) = self.player.as_mut() {
                        p.rescan_plugins();
                    }
                    ui.close();
                }
                self.plugin_safety_ui(ui);
            });
        });
    }

    /// Sperrliste und Suchprobleme im Plugin-Menü (Paket 34).
    pub(super) fn plugin_safety_ui(&mut self, ui: &mut egui::Ui) {
        let small = |s: &str| {
            egui::RichText::new(s.to_owned())
                .size(theme::TYPE_SMALL)
                .color(theme::text_dim())
        };
        let Some(p) = self.player.as_mut() else {
            return;
        };
        let blocked = p.blocked_plugins();
        let problems: Vec<_> = p
            .plugin_scan_problems()
            .into_iter()
            .filter(|q| !blocked.iter().any(|b| b.file == q.path))
            .collect();
        let name = |f: &std::path::Path| {
            f.file_name().map_or_else(
                || f.display().to_string(),
                |n| n.to_string_lossy().into_owned(),
            )
        };
        if !problems.is_empty() {
            ui.label(small(&format!(
                "{} Datei(en) beim Suchen übersprungen:",
                problems.len()
            )));
            for q in problems.iter().take(8) {
                ui.label(
                    egui::RichText::new(format!("{}: {}", name(&q.path), q.reason))
                        .size(theme::TYPE_SMALL)
                        .color(theme::text_dim()),
                );
            }
        }
        if blocked.is_empty() {
            return;
        }
        ui.separator();
        ui.label(small(&format!(
            "Gesperrt ({}): werden nicht geladen",
            blocked.len()
        )));
        let mut free: Option<std::path::PathBuf> = None;
        for b in blocked.iter().take(50) {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(format!("{}: {}", name(&b.file), b.reason))
                        .size(theme::TYPE_SMALL)
                        .color(theme::danger_text()),
                )
                .on_hover_text(b.file.display().to_string());
                if ui.small_button("Freigeben").clicked() {
                    free = Some(b.file.clone());
                }
            });
        }
        if let Some(f) = free {
            p.unblock_plugin(&f);
            self.state.status =
                Status::Info("Freigegeben; „Neu suchen“ lädt die Datei wieder".into());
        } else if blocked.len() > 1 && ui.small_button("Alle freigeben").clicked() {
            p.unblock_all_plugins();
            self.state.status =
                Status::Info("Alle Sperren aufgehoben; „Neu suchen“ lädt neu".into());
        }
    }

    /// Das Log eines Plugins im eigenen Prozess (Paket 34): die letzten Zeilen, eingeklappt.
    pub(super) fn plugin_log_ui(
        &mut self,
        ui: &mut egui::Ui,
        clap_id: &str,
        id: woa_model::InsertId,
    ) {
        let small = |s: &str| {
            egui::RichText::new(s.to_owned())
                .size(theme::TYPE_SMALL)
                .color(theme::text_dim())
        };
        let Some(p) = self.player.as_ref().filter(|p| p.plugin_isolated(id)) else {
            return;
        };
        egui::CollapsingHeader::new(small("Log"))
            .id_salt(("plog", id.0))
            .show(ui, |ui| {
                let lines = p.plugin_log_tail(clap_id, 8);
                if lines.is_empty() {
                    ui.label(small("Noch nichts eingetragen."));
                }
                for l in &lines {
                    ui.label(
                        egui::RichText::new(l)
                            .monospace()
                            .size(theme::TYPE_SMALL)
                            .color(theme::text_dim()),
                    );
                }
                ui.label(small(&format!("Ordner: {}", p.plugin_log_dir().display())));
            });
    }

    /// Kurven-Editor im unteren Studio-Dock für die gewählte Spur.
    pub(super) fn docked_automation(&mut self, ui: &mut egui::Ui) {
        let Some(track) = self
            .selected_track
            .filter(|t| self.state.project().track(*t).is_some())
            .or_else(|| self.state.project().tracks.first().map(|t| t.id))
        else {
            ui.label(egui::RichText::new("Noch keine Spur.").color(theme::text_dim()));
            return;
        };
        if let Some(t) = self.state.project().track(track) {
            ui.label(
                egui::RichText::new(format!("Spur: {}", t.name))
                    .small()
                    .color(theme::text_dim()),
            );
        }
        let tempo = self.state.project().tempo_bpm;
        let playhead = self
            .player
            .as_ref()
            .map_or(0.0, |p| p.position_beats(tempo));
        crate::automation::show(ui, &mut self.state, &mut self.auto_ed, track, playhead);
    }

    /// Angedockte Piano-Roll im unteren Studio-Dock.
    pub(super) fn docked_piano_roll(&mut self, ui: &mut egui::Ui) {
        if self.piano_roll_floating {
            ui.label(
                egui::RichText::new(
                    "Die Piano-Roll ist aktuell als schwebendes Fenster geöffnet. Das Symbol oben in der Dock-Leiste (Tooltip „wieder im Dock einbetten“) holt sie hier unten zurück.",
                )
                .color(theme::text_dim()),
            );
            return;
        }
        // Wenn ein MIDI-Clip ausgewählt wurde, aber noch nicht in `piano_roll` liegt, automatisch laden:
        if let Some(sel) = self.selected_clip
            && self
                .state
                .project()
                .clip(sel)
                .is_some_and(|(_, c)| c.midi.is_some())
            && self.piano_roll.as_ref().is_none_or(|pr| pr.clip != sel)
        {
            self.piano_roll = Some(crate::widgets::piano_roll::PianoRoll::new(sel));
        }

        if let Some(mut pr) = self.piano_roll.take() {
            pr.playhead = self.playhead_beats();
            if pr.show_inline(ui, &mut self.state) {
                self.piano_roll = Some(pr);
                return;
            }
        }

        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(
                    "Kein MIDI-Clip ausgewählt. Doppelklicke auf einen MIDI-Clip in der Zeitleiste oder lege direkt einen neuen an:",
                )
                .color(theme::text_dim()),
            );
            if ui
                .add(
                    egui::Button::new(
                        egui::RichText::new(format!("{} + Leeren MIDI-Clip (4 Takte) anlegen", theme::ic::PIANO_KEYS))
                            .strong()
                            .color(theme::cue()),
                    )
                    .fill(theme::surface()),
                )
                .clicked()
            {
                self.create_midi_clip_at_playhead(self.selected_track);
            }
        });
    }
}
