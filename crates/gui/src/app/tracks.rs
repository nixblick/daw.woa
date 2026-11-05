//! Spurliste, Spurkopf, Lineal, Akkordspur, Import und Export.

use super::*;

impl WoaApp {
    pub(super) fn tracks(&mut self, ui: &mut egui::Ui) {
        // Strg+Mausrad zoomt die Zeitleiste.
        let z = ui.ctx().input(|i| i.zoom_delta());
        if z != 1.0 {
            self.zoom = (self.zoom * z).clamp(*ZOOM_RANGE.start(), *ZOOM_RANGE.end());
        }
        self.prev_track_rects = std::mem::take(&mut self.track_rects);
        self.header_rects.clear();
        let tracks: Vec<_> = self
            .state
            .project()
            .tracks
            .iter()
            .map(|t| (t.id, t.name.clone(), t.gain_db, t.pan, t.mute, t.solo))
            .collect();
        egui::ScrollArea::both().auto_shrink(false).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 5.0;
            let ruler = ui
                .horizontal(|ui| {
                    let (corner_rect, _) = ui.allocate_exact_size(
                        egui::vec2(HEADER_W - 30.0, 24.0),
                        egui::Sense::hover(),
                    );
                    ui.painter()
                        .rect_filled(corner_rect, 4.0, theme::panel_header());
                    ui.painter().text(
                        corner_rect.left_center() + egui::vec2(10.0, 0.0),
                        egui::Align2::LEFT_CENTER,
                        format!("SPUREN ({})  ·  KANALZUG", tracks.len()),
                        egui::FontId::proportional(11.0),
                        theme::text_dim(),
                    );
                    ui.add_space(-ui.spacing().item_spacing.x);
                    self.new_track_menu(ui);
                    self.ruler(ui)
                })
                .inner;
            self.chord_lane(ui, ruler.left());
            for (i, (id, name, gain, pan, mute, solo)) in tracks.into_iter().enumerate() {
                ui.horizontal(|ui| {
                    self.track_header(ui, i, id, &name, gain, pan, mute, solo);
                    self.timeline(ui, id);
                });
                self.take_folder_rows(ui, id, i);
            }
            if self.state.project().tracks.is_empty() {
                ui.add_space(24.0);
                ui.label(
                    egui::RichText::new(
                        "Noch keine Spur. „+“ im Spurenkopf legt eine an, Strg+R nimmt sofort auf.",
                    )
                    .color(theme::text_dim()),
                );
            }
        });
    }

    /// „+“ im Spurenkopf (ScreenStudio): neue Audio- oder MIDI-Spur.
    pub(super) fn new_track_menu(&mut self, ui: &mut egui::Ui) {
        ui.scope(|ui| {
            ui.spacing_mut().button_padding = egui::vec2(6.0, 2.0);
            ui.menu_button(
                egui::RichText::new(theme::ic::PLUS).color(theme::text()),
                |ui| {
                    if ui.button("Audio-Spur").clicked() {
                        self.state.add_track();
                    }
                    if ui
                        .button("MIDI-Spur")
                        .on_hover_text("Spur für das Keyboard; scharf schalten zum Spielen")
                        .clicked()
                    {
                        self.state.add_midi_track();
                    }
                },
            )
            .response
            .on_hover_text("Neue Spur");
        });
    }

    /// Lineal über der Zeitleiste: Taktnummern, Klick setzt den Kopf.
    pub(super) fn ruler(&mut self, ui: &mut egui::Ui) -> egui::Rect {
        let px = self.zoom;
        let end = self.timeline_end();
        let (rect, resp) = ui.allocate_exact_size(
            egui::vec2((end as f32 + 16.0) * px, 24.0),
            egui::Sense::click_and_drag(),
        );
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 4.0, theme::well());
        painter.hline(
            rect.x_range(),
            rect.bottom() - 1.0,
            egui::Stroke::new(1.0, theme::border()),
        );
        let tempo = self.state.project().tempo_bpm;
        for tick in ruler_ticks(self.time_unit, tempo, px, end + 16.0) {
            let x = rect.left() + tick.beat as f32 * px;
            painter.vline(
                x,
                rect.top() + 4.0..=rect.bottom(),
                egui::Stroke::new(1.0, theme::border()),
            );
            painter.text(
                egui::pos2(x + 4.0, rect.top() + 3.0),
                egui::Align2::LEFT_TOP,
                &tick.label,
                egui::FontId::monospace(11.0),
                theme::text_dim(),
            );
        }
        if px >= 12.0 {
            // Schläge als feine Striche am unteren Rand.
            for beat in 0..=(end as usize + 16) {
                let x = rect.left() + beat as f32 * px;
                painter.vline(
                    x,
                    rect.bottom() - 4.0..=rect.bottom(),
                    egui::Stroke::new(1.0, theme::grid()),
                );
            }
        }
        if self.loop_enabled && self.loop_end_bar > self.loop_start_bar {
            let l0 = ((self.loop_start_bar - 1) * 4) as f32 * px;
            let l1 = ((self.loop_end_bar - 1) * 4) as f32 * px;
            let loop_rect = egui::Rect::from_x_y_ranges(
                (rect.left() + l0)..=(rect.left() + l1),
                rect.top()..=rect.bottom(),
            );
            painter.rect_filled(
                loop_rect,
                2.0,
                egui::Color32::from_rgba_unmultiplied(
                    theme::cue().r(),
                    theme::cue().g(),
                    theme::cue().b(),
                    48,
                ),
            );
            painter.hline(
                loop_rect.x_range(),
                rect.top() + 1.5,
                egui::Stroke::new(2.5, theme::cue()),
            );
        }
        if let Some(p) = &self.player {
            let x = rect.left() + p.position_beats(tempo) as f32 * px;
            painter.add(egui::Shape::convex_polygon(
                vec![
                    egui::pos2(x - 6.0, rect.top() + 8.0),
                    egui::pos2(x + 6.0, rect.top() + 8.0),
                    egui::pos2(x, rect.bottom()),
                ],
                theme::accent(),
                egui::Stroke::NONE,
            ));
        }
        if (resp.clicked() || resp.dragged())
            && let (Some(pos), Some(p)) = (resp.interact_pointer_pos(), self.player.as_mut())
        {
            p.seek_beats(f64::from(((pos.x - rect.left()) / px).max(0.0)), tempo);
        }
        rect
    }

    /// Akkordspur unter dem Lineal (Design `ChordTrack`): Kopf „Akkorde · C Dur“ und je Takt
    /// ein Chip mit Akkord und Stufe. Nur, wenn MIDI-Spuren mit Tönen da sind; Tonart und
    /// Akkorde folgen aus den Noten, also auch jedem Tonartwechsel.
    pub(super) fn chord_lane(&mut self, ui: &mut egui::Ui, lane_left: f32) {
        let px = self.zoom;
        let end = self.timeline_end() + 16.0;
        let h = crate::timeline::harmony(self.state.project(), 0.0, (end / 4.0).ceil() as usize);
        let Some(key) = h.key else {
            return;
        };
        ui.horizontal(|ui| {
            let gap = ui.spacing().item_spacing.x;
            let head_w = (lane_left - ui.cursor().left() - gap).max(40.0);
            let label = format!("Akkorde · {}", key.name());
            let (head, resp) =
                ui.allocate_exact_size(egui::vec2(head_w, CHORD_LANE_H), egui::Sense::hover());
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &label));
            ui.painter().rect_filled(head, 4.0, theme::panel_header());
            ui.painter().text(
                head.left_center() + egui::vec2(10.0, 0.0),
                egui::Align2::LEFT_CENTER,
                label.to_uppercase(),
                theme::panel_label_font(),
                theme::text_dim(),
            );
            let (rect, _) = ui.allocate_exact_size(
                egui::vec2(end as f32 * px, CHORD_LANE_H),
                egui::Sense::hover(),
            );
            let painter = ui.painter_at(rect);
            painter.rect_filled(rect, 4.0, theme::well());
            for (bar, chord) in h.bars.iter().enumerate() {
                let Some(chord) = chord else { continue };
                let x = rect.left() + (bar * 4) as f32 * px;
                painter.vline(x, rect.y_range(), egui::Stroke::new(1.0, theme::border()));
                let name = chord.name();
                let degree = chord.degree(key).unwrap_or("");
                let chip_w = (4.0 * px - 6.0).min(64.0);
                if chip_w < 18.0 {
                    continue;
                }
                let chip = egui::Rect::from_min_size(
                    egui::pos2(x + 3.0, rect.top() + 4.0),
                    egui::vec2(chip_w, CHORD_LANE_H - 8.0),
                );
                painter.rect_filled(chip, 3.0, theme::surface());
                painter.rect_stroke(
                    chip,
                    3.0,
                    egui::Stroke::new(1.0, theme::border_strong()),
                    egui::StrokeKind::Inside,
                );
                let g = painter.layout_no_wrap(
                    name.clone(),
                    egui::FontId::new(12.0, egui::FontFamily::Name("ui-bold".into())),
                    theme::text(),
                );
                let nx = chip.left() + 5.0;
                painter.galley(
                    egui::pos2(nx, chip.center().y - g.size().y / 2.0),
                    g.clone(),
                    theme::text(),
                );
                painter.text(
                    egui::pos2(nx + g.size().x + 4.0, chip.center().y),
                    egui::Align2::LEFT_CENTER,
                    degree,
                    egui::FontId::monospace(10.0),
                    theme::text_dim(),
                );
                let tip = format!("{name} {degree}");
                let chip_resp = ui.interact(
                    chip,
                    ui.id().with(("chord_chip", bar)),
                    egui::Sense::hover(),
                );
                chip_resp.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Label, true, tip.trim())
                });
                chip_resp.on_hover_text(format!("Takt {}: {}", bar + 1, tip.trim()));
            }
        });
    }

    /// Längste Spur in Beats, mindestens 64.
    /// Ende des letzten Clips in Beats (0 ohne Clips).
    pub(super) fn audio_end(&self) -> f64 {
        self.state
            .project()
            .tracks
            .iter()
            .flat_map(|t| t.clips.iter().map(|c| c.end_beats()))
            .fold(0.0f64, f64::max)
    }

    /// Hält die Wiedergabe am Ende des letzten Clips an, wenn „Stopp am Ende“ an ist.
    /// Beim Aufnehmen oder aktivem Loop nie.
    pub(super) fn stop_at_audio_end(&mut self) {
        if !self.stop_at_end || self.recording() || self.loop_enabled {
            return;
        }
        let end = self.audio_end();
        let tempo = self.state.project().tempo_bpm;
        if let Some(p) = self.player.as_mut()
            && p.is_playing()
            && p.position_beats(tempo) >= end
        {
            p.stop();
            p.seek_beats(end, tempo);
        }
    }

    pub(super) fn timeline_end(&self) -> f64 {
        self.state
            .project()
            .tracks
            .iter()
            .flat_map(|t| t.clips.iter().map(|c| c.end_beats()))
            .fold(64.0f64, f64::max)
            .max(self.live_end_beats())
    }

    /// Läuft eine Aufnahme (Audio oder MIDI)?
    pub(super) fn recording(&self) -> bool {
        !self.takes.is_empty() || !self.midi_takes.is_empty()
    }

    /// Holt die gestempelten MIDI-Events der laufenden Aufnahmen ab.
    pub(super) fn drain_midi_takes(&mut self) {
        for (_, take) in &mut self.midi_takes {
            take.drain();
        }
    }

    /// Importiert eine Audiodatei (WAV, FLAC, MP3, OGG, M4A …) in `aufnahmen/` und legt
    /// sie als Clip am Abspielkopf auf eine Audio-Spur.
    pub(super) fn import_audio(&mut self) {
        let exts = woa_engine::decode::dialog_extensions();
        let ext_refs: Vec<&str> = exts.iter().map(String::as_str).collect();
        let Some(path) = crate::ordner::dialog_musik()
            .set_title("Audiodatei importieren")
            .add_filter(woa_engine::decode::FILTER_NAME, &ext_refs)
            .pick_file_modal()
        else {
            return;
        };
        let tempo = self.state.project().tempo_bpm;
        let at = self
            .player
            .as_ref()
            .map_or(0.0, |p| p.position_beats(tempo));
        self.import_audio_file(&path, self.selected_track, at);
    }

    /// Kopiert eine Audiodatei ins Projekt und legt sie als Clip bei `at` auf `track`
    /// (`None`: neue Spur). Dialog und Ziehen-und-Ablegen nutzen denselben Weg.
    pub(super) fn import_audio_file(
        &mut self,
        path: &std::path::Path,
        track: Option<TrackId>,
        at: f64,
    ) {
        // Nur der Datei-Kopf: ganz dekodieren hielt die Oberfläche bei langen Titeln sekundenlang
        // an. Verrät der Kopf keine Länge (selten), wird doch dekodiert.
        let secs = match woa_library::tags::read_header(path) {
            Ok(h) if h.duration > 0.0 => h.duration,
            _ => match woa_engine::decode::decode_file(path) {
                Ok(d) => d.frames() as f64 / f64::from(d.rate.max(1)),
                Err(e) => {
                    tracing::error!(error = %e, path = %path.display(), "audio import decode failed");
                    self.state.status = Status::Error(format!("Audio-Import: {e}"));
                    return;
                }
            },
        };
        if secs <= 0.0 {
            self.state.status = Status::Error("Audio-Import: Datei ist leer".into());
            return;
        }
        let dir = self.state.takes_dir();
        if let Err(e) = std::fs::create_dir_all(&dir) {
            tracing::error!(error = %e, dir = %dir.display(), "creating takes_dir failed");
            self.state.status = Status::Error(format!("Audio-Import: {e}"));
            return;
        }
        let stem: String = path
            .file_stem()
            .map_or_else(|| "audio".into(), |s| s.to_string_lossy().into_owned())
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("wav")
            .to_ascii_lowercase();
        let mut dst = dir.join(format!("{stem}.{ext}"));
        let mut n = 2u32;
        while dst.exists() && dst != path {
            dst = dir.join(format!("{stem}-{n}.{ext}"));
            n += 1;
        }
        if dst != path
            && let Err(e) = std::fs::copy(path, &dst)
        {
            tracing::error!(error = %e, "copying imported audio failed");
            self.state.status = Status::Error(format!("Audio-Import: {e}"));
            return;
        }
        if let Some(clip) = self.state.add_take(track, &dst, at, secs, 0.0) {
            self.selected_clip = Some(clip);
            self.track_focus = false;
            self.selected_track = self.state.project().clip(clip).map(|(t, _)| t.id);
            self.state.status = Status::Info(format!("Audio importiert ({secs:.1} s)"));
        }
    }

    /// Legt einen leeren 4-Takt-MIDI-Clip am Abspielkopf an und öffnet die Piano-Roll.
    pub(super) fn create_midi_clip_at_playhead(&mut self, target: Option<TrackId>) {
        let tempo = self.state.project().tempo_bpm;
        let at = self
            .player
            .as_ref()
            .map_or(0.0, |p| ((p.position_beats(tempo) / 4.0).floor()) * 4.0);
        if let Some(clip) = self.state.create_empty_midi_clip(target, at, 16.0) {
            self.open_piano_roll(clip);
            self.state.status =
                Status::Info("Leerer MIDI-Clip (4 Takte) angelegt – Piano-Roll bereit".into());
        }
    }

    /// Importiert eine `.mid`-Datei als MIDI-Clip am Kopf (Paket 12).
    pub(super) fn import_midi(&mut self) {
        let Some(path) = crate::ordner::dialog(crate::ordner::Art::Midi)
            .set_title("MIDI importieren")
            .add_filter("MIDI", &["mid", "midi"])
            .pick_file_modal()
        else {
            return;
        };
        let read = std::fs::read(&path)
            .map_err(|e| e.to_string())
            .and_then(|b| woa_io::smf::read_smf(&b).map_err(|e| e.to_string()));
        let smf = match read {
            Ok(s) => s,
            Err(e) => {
                tracing::error!(error = %e, path = %path.display(), "midi import failed");
                self.state.status = Status::Error(format!("MIDI-Import: {e}"));
                return;
            }
        };
        let tempo = self.state.project().tempo_bpm;
        let at = self
            .player
            .as_ref()
            .map_or(0.0, |p| p.position_beats(tempo));
        let name = smf.name.clone().unwrap_or_else(|| {
            path.file_stem()
                .map_or_else(|| "MIDI".into(), |s| s.to_string_lossy().into_owned())
        });
        let n = smf.midi.notes.len();
        let target = self.selected_track;
        if let Some(clip) =
            self.state
                .add_midi_clip(target, &name, at, smf.end_beats.max(1.0), smf.midi)
        {
            self.selected_clip = Some(clip);
            self.track_focus = false;
            self.selected_track = self.state.project().clip(clip).map(|(t, _)| t.id);
            self.state.status = Status::Info(format!("MIDI importiert: {n} Noten"));
        }
    }

    /// Schreibt den ausgewählten MIDI-Clip als `.mid` (Paket 12).
    pub(super) fn export_midi(&mut self) {
        let clip = self
            .selected_clip
            .and_then(|c| self.state.project().clip(c))
            .and_then(|(_, c)| c.midi.clone().map(|m| (c.name.clone(), m)));
        let Some((name, midi)) = clip else {
            self.state.status = Status::Error("Erst einen MIDI-Clip auswählen".into());
            return;
        };
        let Some(path) = crate::ordner::dialog(crate::ordner::Art::Midi)
            .set_title("MIDI exportieren")
            .set_file_name(format!("{name}.mid"))
            .add_filter("MIDI", &["mid"])
            .save_file_modal()
        else {
            return;
        };
        let bytes = woa_io::smf::write_smf(&midi, self.state.project().tempo_bpm, &name);
        self.state.status = match std::fs::write(&path, bytes) {
            Ok(()) => Status::Info(format!("MIDI gespeichert: {}", path.display())),
            Err(e) => {
                tracing::error!(error = %e, path = %path.display(), "midi export failed");
                Status::Error(format!("MIDI-Export: {e}"))
            }
        };
    }

    /// Ende der laufenden Aufnahme in Beats (0 ohne Aufnahme).
    pub(super) fn live_end_beats(&self) -> f64 {
        match self.takes.first() {
            Some((_, take)) => {
                self.rec_start
                    + take.elapsed().as_secs_f64() * self.state.project().tempo_bpm / 60.0
            }
            None => 0.0,
        }
    }

    /// Holt die neuen Wellenform-Punkte aller laufenden Aufnahmen ab (#43).
    pub(super) fn drain_live_peaks(&mut self) {
        for (track, take) in &mut self.takes {
            let Some(t) = track else { continue };
            let rate = take.sample_rate();
            let (peaks, _) = self
                .live_peaks
                .entry(*t)
                .or_insert_with(|| (Vec::new(), rate));
            let before = peaks.len();
            take.drain_peaks(peaks);
            if peaks[before..].iter().any(|p| *p >= 1.0) {
                self.rec_clipped = true;
            }
        }
    }

    /// Spurkopf (Design `TrackHeader`, 64 px, zwei Zeilen): Zeile 1 Aufnahme scharf, Name, M, S,
    /// Papierkorb; Zeile 2 Eingang, Gain, Pan; rechts die LED-Kette, links die Spurfarbe.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn track_header(
        &mut self,
        ui: &mut egui::Ui,
        index: usize,
        id: TrackId,
        name: &str,
        gain: f64,
        pan: f64,
        mute: bool,
        solo: bool,
    ) {
        let color = theme::track_color(index);
        let is_midi = self
            .state
            .project()
            .track(id)
            .is_some_and(|t| t.kind == woa_model::TrackKind::Midi);
        let armed = if self.armed.is_empty() {
            self.selected_track == Some(id)
        } else {
            self.armed.contains(&id)
        };
        // Gewählt = Spurfarbe 14 % und 1-px-Rahmen, wie Kanalzug und Rack; scharf zeigt nur der Punkt.
        let is_selected = self.rack_track() == Some(id);
        let frame = egui::Frame::new()
            .fill(if is_selected {
                theme::selection_fill(color)
            } else {
                theme::panel()
            })
            .stroke(if is_selected {
                theme::selection_stroke(color)
            } else {
                egui::Stroke::new(1.0, theme::border())
            })
            .corner_radius(5)
            .inner_margin(egui::Margin {
                left: 12,
                right: 22,
                top: 5,
                bottom: 5,
            });
        // Fest reserviertes Rechteck, damit Lineal und Zeitleisten bündig bleiben.
        let (slot, slot_resp) =
            ui.allocate_exact_size(egui::vec2(HEADER_W, self.row_h()), egui::Sense::click());
        let mut name_resp = None;
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(slot)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        // Was trotzdem breiter wird, schneidet der Spurkopf ab, statt ins Lineal zu ragen.
        child.set_clip_rect(slot.intersect(ui.clip_rect()));
        let resp = {
            let ui = &mut child;
            frame.show(ui, |ui| {
                ui.set_width(HEADER_W - 34.0);
                ui.set_max_width(HEADER_W - 34.0);
                ui.set_height(self.row_h() - 10.0);
                ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
                ui.spacing_mut().interact_size.y = 22.0;
                ui.horizontal(|ui| {
                    let (dot, arm) =
                        ui.allocate_exact_size(egui::vec2(20.0, 22.0), egui::Sense::click());
                    ui.painter().circle(
                        dot.center(),
                        5.5,
                        if armed {
                            theme::on_air_fill()
                        } else {
                            theme::well()
                        },
                        egui::Stroke::new(
                            1.5,
                            if armed {
                                theme::on_air()
                            } else {
                                theme::text_dim()
                            },
                        ),
                    );
                    if arm
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .on_hover_text(
                            "Aufnahme scharf: nimmt bei der nächsten Aufnahme mit auf. \
                             Mehrere Spuren gleichzeitig möglich.",
                        )
                        .clicked()
                        && !self.armed.remove(&id)
                    {
                        self.armed.insert(id);
                    }
                    name_resp = self.track_name(ui, id, name);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 3.0;
                        if danger_icon_button(ui, theme::ic::TRASH, 14.0, "Spur löschen (Entf)")
                            .clicked()
                        {
                            self.delete_track(id);
                        }
                        let s_btn = egui::Button::new(
                            egui::RichText::new("S").size(11.5).strong().color(if solo {
                                theme::well()
                            } else {
                                theme::text_dim()
                            }),
                        )
                        .fill(if solo { theme::cue() } else { theme::well() })
                        .stroke(egui::Stroke::new(
                            1.0,
                            if solo { theme::cue() } else { theme::border() },
                        ))
                        .min_size(egui::vec2(22.0, 21.0));
                        if ui
                            .add(s_btn)
                            .on_hover_text("Solo: nur Solo-Spuren sind zu hören")
                            .clicked()
                        {
                            self.state.toggle_solo(id);
                        }
                        let m = egui::Button::new(
                            egui::RichText::new("M").size(11.5).strong().color(if mute {
                                theme::well()
                            } else {
                                theme::text_dim()
                            }),
                        )
                        .fill(if mute { theme::accent() } else { theme::well() })
                        .stroke(egui::Stroke::new(
                            1.0,
                            if mute {
                                theme::accent()
                            } else {
                                theme::border()
                            },
                        ))
                        .min_size(egui::vec2(22.0, 21.0));
                        let r = ui.add(m).on_hover_text("Stumm (Rechtsklick: MIDI lernen)");
                        if r.clicked() {
                            self.state.toggle_mute(id);
                        }
                        if let Some(i) = self.track_index(id) {
                            self.learn_menu(&r, Target::TrackMute(i));
                        }
                        // Kein „MIDI“-Badge (Design-Runde 3): es überlappte den Namen; die
                        // MIDI-Spur zeigt sich am Eingang mit Klaviatur-Symbol.
                    });
                });
                // Die Quellenwahl bekommt, was neben Gain und Pan bleibt, und kürzt ihren Text
                // (#212); der Rest wird am Spurkopf abgeschnitten, nichts ragt in die Zeitleiste.
                ui.horizontal(|ui| {
                    let w = (ui.available_width() - HEADER_FIELDS_W).max(24.0);
                    ui.allocate_ui_with_layout(
                        egui::vec2(w, 22.0),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            ui.set_max_width(w);
                            if is_midi {
                                self.midi_head_source(ui, id, w);
                            } else {
                                self.source_combo(ui, id, is_midi, w);
                            }
                        },
                    );
                    let r = self.gain_field(ui, id, gain, "");
                    let r = r.on_hover_text(
                        "Gain der Spur, derselbe Wert wie der Fader. Ziehen · Doppelklick = 0 dB · \
                         Klick = eintippen · Rechtsklick: MIDI lernen für Fader, Pan und Solo",
                    );
                    if let Some(i) = self.track_index(id) {
                        let bound = |t| self.controls.mapper.source_of(t).map(|s| s.label());
                        let items = [
                            (Target::TrackGain(i), bound(Target::TrackGain(i))),
                            (Target::TrackPan(i), bound(Target::TrackPan(i))),
                            (Target::TrackSolo(i), bound(Target::TrackSolo(i))),
                        ];
                        let mut chosen = None;
                        r.context_menu(|ui| {
                            for (t, b) in items {
                                if let Some(t) = crate::controls::learn_menu(ui, t, b) {
                                    chosen = Some(t);
                                }
                            }
                        });
                        if let Some(t) = chosen {
                            self.controls.mapper.learn(t);
                        }
                    }
                    // Pan doppelt (Spurkopf und Rack) ist Andrés offene Entscheidung: `HEADER_PAN`.
                    if HEADER_PAN {
                        self.pan_field(ui, id, pan, "");
                    }
                });
            })
        };
        let r = resp.response.rect;
        self.header_rects.push((id, r));
        // Ein Klick irgendwo in den Spurkopf wählt die Spur (auch auf M/S/Name).
        if resp.response.contains_pointer() && ui.input(|i| i.pointer.primary_clicked()) {
            self.select_track(id);
        }
        // Rechtsklick irgendwo auf den Spurkopf (Hintergrund oder Name) öffnet das Spurmenü.
        // Der Rahmen selbst meldet nur Hover, deshalb trägt das Slot-Rechteck den Klick.
        self.track_menu(&slot_resp, id, name);
        if let Some(nr) = name_resp {
            self.track_menu(&nr, id, name);
        }
        ui.painter().rect_filled(
            egui::Rect::from_min_size(r.min, egui::vec2(5.0, r.height())),
            egui::CornerRadius {
                nw: 5,
                sw: 5,
                ne: 0,
                se: 0,
            },
            color,
        );
        // LED-Kette rechts, über beide Zeilen.
        let level = self.track_level(id, gain, mute);
        let m = egui::Rect::from_min_max(
            egui::pos2(r.right() - 15.0, r.top() + 7.0),
            egui::pos2(r.right() - 8.0, r.bottom() - 7.0),
        );
        theme::led_meter_vertical(ui.painter(), m, level);
    }

    /// Entfernt eine Spur samt Aufnahme-Scharfschaltung und Auswahl. Alle Wege (Papierkorb,
    /// Spurmenü, Entf) laufen hierher; danach steht 8 s der Rückgängig-Hinweis.
    pub(super) fn delete_track(&mut self, id: TrackId) {
        let name = self
            .state
            .project()
            .track(id)
            .map(|t| t.name.clone())
            .unwrap_or_default();
        self.armed.remove(&id);
        if self.selected_track == Some(id) {
            self.selected_track = None;
        }
        if self.state.remove_track(id) {
            let count = match &self.undo_toast {
                Some(t) if t.until > self.now => t.count + 1,
                _ => 1,
            };
            self.undo_toast = Some(UndoToast {
                count,
                name,
                until: self.now + UNDO_TOAST_SECS,
            });
        }
    }

    /// Kontextmenü einer Spur (Rechtsklick): umbenennen, Inhalt anlegen, zuletzt löschen.
    pub(super) fn track_menu(&mut self, resp: &egui::Response, id: TrackId, name: &str) {
        resp.context_menu(|ui| self.track_menu_items(ui, id, name));
    }

    /// Einträge des Spurmenüs, auch für „⋯“ im Rack-Kopf. Löschen steht zuletzt und in Rot.
    pub(super) fn track_menu_items(&mut self, ui: &mut egui::Ui, id: TrackId, name: &str) {
        let kind = self.state.project().track(id).map(|t| t.kind);
        if ui
            .button(format!("{} Spur umbenennen", theme::ic::PENCIL_SIMPLE))
            .clicked()
        {
            self.rename = Some((id, name.to_string(), false));
            ui.close();
        }
        match kind {
            Some(woa_model::TrackKind::Midi) => {
                if ui
                    .button(format!(
                        "{} + Leerer MIDI-Clip (4 Takte)",
                        theme::ic::PIANO_KEYS
                    ))
                    .clicked()
                {
                    self.create_midi_clip_at_playhead(Some(id));
                    ui.close();
                }
            }
            Some(woa_model::TrackKind::Bus) => {}
            _ => {
                if ui
                    .button(format!(
                        "{} Audio-Datei importieren …",
                        theme::ic::FOLDER_OPEN
                    ))
                    .clicked()
                {
                    self.selected_track = Some(id);
                    self.import_audio();
                    ui.close();
                }
            }
        }
        if ui
            .button(format!("{} Spur exportieren …", theme::ic::EXPORT))
            .clicked()
        {
            self.track_export = Some(crate::track_export::TrackExportDialog::new(id, name));
            ui.close();
        }
        ui.separator();
        if ui
            .button(
                egui::RichText::new(format!("{} Spur löschen", theme::ic::TRASH))
                    .color(theme::danger_text()),
            )
            .clicked()
        {
            self.delete_track(id);
            ui.close();
        }
    }

    /// Eingang einer Spur (Spurkopf und Rack): Quelle aus der Liste, darin „Mithören“.
    pub(super) fn source_combo(
        &mut self,
        ui: &mut egui::Ui,
        id: TrackId,
        is_midi: bool,
        width: f32,
    ) {
        if is_midi {
            // MIDI-Spuren spielen vom MIDI-Eingang, nie vom Mikrofon (Design `ChordTrack`).
            self.midi_input_combo(ui, id, width);
            return;
        }
        let busy = self.recording();
        let current = self.track_source.get(&id).cloned().unwrap_or_default();
        let mut chosen = None;
        let mut opened = false;
        let can = self.track_source.contains_key(&id);
        let mut on = self.track_monitor.contains(&id);
        let was_on = on;
        let icon = if is_midi {
            theme::ic::PIANO_KEYS
        } else {
            theme::ic::MICROPHONE
        };
        ui.add_enabled_ui(!busy, |ui| {
            opened = egui::ComboBox::from_id_salt(("source", id))
                .selected_text(
                    egui::RichText::new(format!(
                        "{icon} {}{}",
                        sources::track_label(&current, &self.source_list.0),
                        if on { " · hören" } else { "" }
                    ))
                    .small()
                    .color(theme::text_dim()),
                )
                .width(width)
                .truncate()
                .show_ui(ui, |ui| {
                    let list = self.sources().to_vec();
                    for s in &list {
                        if ui.selectable_label(s.key == current, &s.label).clicked() {
                            chosen = Some(s.key.clone());
                        }
                    }
                    if cfg!(target_os = "linux") && !sources::has_apps(&list) {
                        ui.weak(sources::NO_APPS_HINT);
                    }
                    ui.separator();
                    ui.add_enabled(can, egui::Checkbox::new(&mut on, "Mithören"))
                        .on_hover_text(
                            "Quelle beim Aufnehmen über den Ausgang hören. Für Mikrofone und \
                             Eingänge; Systemklang hört man ohnehin (Rückkopplung!).",
                        )
                        .on_disabled_hover_text("Erst eine Quelle aus der Liste wählen");
                })
                .response
                .on_hover_text(
                    "Eingang der Spur: Mikrofon, Systemklang oder ein Programm; \
                     darin „Mithören“",
                )
                .clicked();
        });
        if opened {
            // Beim Öffnen frisch abfragen, nicht erst nach Ablauf der Sekunde.
            self.source_list.1 = None;
        }
        if on != was_on {
            if on {
                self.track_monitor.insert(id);
            } else {
                self.track_monitor.remove(&id);
            }
        }
        if let Some(key) = chosen {
            if key == sources::DEFAULT_KEY {
                self.track_source.remove(&id);
                self.track_monitor.remove(&id);
            } else {
                self.track_source.insert(id, key);
            }
        }
    }

    /// Spurkopf einer MIDI-Spur: links das Instrument (Klick öffnet die Klangauswahl im Rack),
    /// rechts der MIDI-Eingang (André 02.11.2026: Klangwahl sichtbar).
    pub(super) fn midi_head_source(&mut self, ui: &mut egui::Ui, id: TrackId, width: f32) {
        use crate::widgets::track_head::{
            MidiInLook, instrument_button, midi_in_button, midi_in_width,
        };
        use woa_io::midi::MidiStatus;
        let gap = ui.spacing().item_spacing.x;
        let midi_w = midi_in_width(width, gap);
        let inst_w = (width - gap - midi_w).max(24.0);
        let sound = self.state.project().track(id).and_then(|t| t.sound);
        let name = woa_model::sound_name(sound);
        let index = self
            .state
            .project()
            .tracks
            .iter()
            .position(|t| t.id == id)
            .unwrap_or(0);
        let icon = sound
            .and_then(woa_model::sound_category)
            .map_or(theme::ic::MUSIC_NOTE, |c| category_label(c).1);
        // Fassung 7: Instrument-Knopf links (Symbol der Kategorie in Spurfarbe, Name, Pfeil).
        if instrument_button(ui, icon, &name, theme::track_color(index), inst_w).clicked() {
            self.open_sound_picker(id);
        }
        // MIDI-Eingang rechts, 104 px: leuchtet bei Noten auf der spielenden Spur,
        // gestrichelt ohne Gerät.
        let status = self.midi.status();
        let lit = self
            .midi_seen
            .1
            .is_some_and(|t| t.elapsed().as_millis() < 150)
            && self.live_midi_track() == Some(id);
        let look = match &status {
            MidiStatus::Connected(_) if lit => MidiInLook::Notes,
            MidiStatus::Connected(_) => MidiInLook::Connected,
            _ => MidiInLook::NoDevice,
        };
        let r = midi_in_button(ui, midi_w, &midi_input_label(&status), look)
            .on_hover_text("MIDI-Eingang: spielt die scharf geschaltete MIDI-Spur");
        egui::Popup::menu(&r).show(|ui| self.midi_choices(ui));
    }

    /// Wählt die Spur und klappt im Rack die Klangauswahl in der Kategorie ihres Klangs auf.
    pub(super) fn open_sound_picker(&mut self, id: TrackId) {
        let sound = self.state.project().track(id).and_then(|t| t.sound);
        self.select_track(id);
        self.sound_pick = Some((
            id,
            sound
                .and_then(woa_model::sound_category)
                .unwrap_or(woa_model::SoundCategory::Klavier),
        ));
    }

    pub(super) fn midi_input_combo(&mut self, ui: &mut egui::Ui, id: TrackId, width: f32) {
        let text = midi_input_label(&self.midi.status());
        egui::ComboBox::from_id_salt(("midi_source", id))
            .selected_text(
                egui::RichText::new(format!("{} {text}", theme::ic::PIANO_KEYS))
                    .small()
                    .color(theme::text_dim()),
            )
            .width(width)
            .truncate()
            .show_ui(ui, |ui| self.midi_choices(ui))
            .response
            .on_hover_text("MIDI-Eingang: spielt die scharf geschaltete MIDI-Spur");
    }

    /// Zahlenfeld für den Spur-Gain in dB (Spurkopf und Rack): ziehen, Doppelklick = 0 dB,
    /// Klick = eintippen. Derselbe Wert wie der Fader.
    pub(super) fn gain_field(
        &mut self,
        ui: &mut egui::Ui,
        id: TrackId,
        gain: f64,
        prefix: &str,
    ) -> egui::Response {
        let mut g = match self.gain_drag {
            Some((d, v)) if d == id => v,
            _ => self.shown_gain(id, gain),
        };
        let r = ui.add(
            egui::DragValue::new(&mut g)
                .range(MIN_GAIN_DB..=MAX_GAIN_DB)
                .speed(0.2)
                .fixed_decimals(1)
                .prefix(prefix)
                .suffix(" dB"),
        );
        if r.double_clicked() {
            self.gain_drag = None;
            self.state.set_gain(id, 0.0);
        } else {
            if r.changed() {
                self.gain_drag = Some((id, g));
            }
            if r.drag_stopped() || (r.changed() && !r.dragged()) {
                self.gain_drag = None;
                self.state.set_gain(id, g);
            }
        }
        r
    }

    /// Zahlenfeld für den Eingangs-Trim in dB (Rack): vor den Inserts, getrennt vom Fader.
    /// Ziehen, Doppelklick = 0 dB, Klick = eintippen; ein Undo-Schritt beim Loslassen.
    pub(super) fn trim_field(&mut self, ui: &mut egui::Ui, id: TrackId) -> egui::Response {
        let trim = self.state.project().track(id).map_or(0.0, |t| t.trim_db);
        let key = egui::Id::new(("trim_drag", id.0));
        let mut v: f64 = ui.data(|d| d.get_temp(key)).unwrap_or(trim);
        let r = ui
            .add(
                egui::DragValue::new(&mut v)
                    .range(woa_model::TRIM_RANGE.0..=woa_model::TRIM_RANGE.1)
                    .speed(0.1)
                    .fixed_decimals(1)
                    .prefix("Trim ")
                    .suffix(" dB"),
            )
            .on_hover_text(
                "Eingangs-Trim in dB (−24 bis +24), vor den Effekten; der Fader bleibt. \
                 Ziehen · Doppelklick = 0 dB · Klick = eintippen",
            );
        if r.double_clicked() {
            ui.data_mut(|d| d.remove::<f64>(key));
            self.state.set_trim(id, 0.0);
        } else if r.dragged() || r.has_focus() {
            ui.data_mut(|d| d.insert_temp(key, v));
        } else {
            ui.data_mut(|d| d.remove::<f64>(key));
            if v != trim {
                self.state.set_trim(id, v);
            }
        }
        r
    }

    /// Zahlenfeld für das Panorama: C, L1–L100, R1–R100; Doppelklick = Mitte.
    pub(super) fn pan_field(
        &mut self,
        ui: &mut egui::Ui,
        id: TrackId,
        pan: f64,
        prefix: &'static str,
    ) {
        let mut p = match self.pan_drag {
            Some((d, v)) if d == id => v,
            _ => pan,
        };
        let mut p_pct = (p * 100.0).round();
        let pr = ui
            .add(
                egui::DragValue::new(&mut p_pct)
                    .range(-100.0..=100.0)
                    .speed(1.0)
                    .fixed_decimals(0)
                    .custom_formatter(move |v, _| {
                        let iv = v.round() as i32;
                        if iv == 0 {
                            format!("{prefix}C")
                        } else if iv < 0 {
                            format!("{prefix}L{}", -iv)
                        } else {
                            format!("{prefix}R{iv}")
                        }
                    }),
            )
            .on_hover_text(
                "Panorama links/rechts. Ziehen · Doppelklick = Mitte · Klick = eintippen",
            );
        if pr.double_clicked() {
            self.pan_drag = None;
            self.state.set_pan(id, 0.0);
        } else {
            p = (p_pct / 100.0).clamp(-1.0, 1.0);
            if pr.changed() {
                self.pan_drag = Some((id, p));
            }
            if pr.drag_stopped() || (pr.changed() && !pr.dragged()) {
                self.pan_drag = None;
                self.state.set_pan(id, p);
            }
        }
    }

    /// Pegel einer Spur für die LED-Kette: Spitzenpegel nach Gain und Mute, fällt langsam ab.
    pub(super) fn track_level(&mut self, track: TrackId, gain_db: f64, mute: bool) -> f32 {
        let raw = self.player.as_ref().map_or(0.0, |p| p.take_level(track));
        let project = self.state.project();
        let solo = project.tracks.iter().any(|t| t.solo);
        let audible = !mute && (!solo || project.track(track).is_some_and(|t| t.solo));
        let fresh = if audible {
            raw * 10f32.powf(gain_db as f32 / 20.0)
        } else {
            0.0
        };
        let fresh = fresh.max(self.rec_levels.insert(track, 0.0).unwrap_or(0.0));
        let shown = self.track_levels.entry(track).or_insert(0.0);
        *shown = fresh.max(*shown * 0.85);
        *shown
    }
}
