//! Aufnahme, Wiedergabe, Transportleiste.

use super::*;

impl WoaApp {
    /// Startet oder beendet die Aufnahme. Jede scharf geschaltete Spur (sonst die
    /// ausgewählte) nimmt ihre eigene Quelle in `aufnahmen/` auf, alle ab demselben Punkt.
    pub(super) fn toggle_record(&mut self) {
        if self.recording() {
            self.stop_record();
            return;
        }
        let dir = self.state.takes_dir();
        let tempo = self.state.project().tempo_bpm;
        let mut start = self
            .player
            .as_ref()
            .map_or(0.0, |p| p.position_beats(tempo));
        // Paket 22: Mit Loop beginnt die Aufnahme am Loop-Anfang, jede Runde wird ein Take.
        let loop_range = self.loop_beats();
        let punch_range = self.punch_beats();
        let targets: Vec<Option<TrackId>> = if self.armed.is_empty() {
            vec![self.selected_track]
        } else {
            self.armed.iter().copied().map(Some).collect()
        };
        let mut errors = Vec::new();
        let project = self.state.project();
        let (midi_targets, targets): (Vec<_>, Vec<_>) = targets.into_iter().partition(|t| {
            t.and_then(|t| project.track(t))
                .is_some_and(|t| t.kind == woa_model::TrackKind::Midi)
        });
        if !midi_targets.is_empty() {
            match self.player.as_ref() {
                Some(p) => {
                    let tap = self.midi.record_tap();
                    let at = p.position_samples();
                    for t in midi_targets.into_iter().flatten() {
                        let take = woa_io::record::MidiTake::start(&tap, p.clock(), at);
                        tracing::info!(track = t.0, start_sample = at, "midi take started");
                        self.midi_takes.push((t, take));
                    }
                }
                None => errors.push("MIDI: keine Wiedergabe, keine Uhr".into()),
            }
        }
        for track in targets {
            let key = track
                .and_then(|t| self.track_source.get(&t).cloned())
                .unwrap_or_default();
            let monitor = track.is_some_and(|t| self.track_monitor.contains(&t));
            let started = next_take_path(&dir)
                .map_err(|e| e.to_string())
                .and_then(|path| Take::start(&key, &path, monitor));
            match started {
                Ok(take) => {
                    let trim = track
                        .and_then(|t| self.state.project().track(t))
                        .map_or(0.0, |t| t.trim_db);
                    take.set_trim_db(trim);
                    tracing::info!(path = %take.path().display(), source = %key, "take started");
                    self.takes.push((track, take));
                }
                Err(e) => {
                    tracing::error!(error = %e, source = %key, "take start failed");
                    errors.push(format!("{}: {e}", sources::short_label(&key)));
                }
            }
        }
        if !self.recording() {
            self.state.status =
                Status::Error(format!("Aufnahme nicht gestartet: {}", errors.join("; ")));
            return;
        }
        if let Some((ls, _)) = loop_range
            && !self.takes.is_empty()
        {
            start = ls;
            if let Some(p) = self.player.as_mut() {
                p.seek_beats(ls, tempo);
            }
        }
        self.rec_loop = loop_range.filter(|_| !self.takes.is_empty());
        self.rec_punch = punch_range;
        tracing::info!(start_beats = start, loop = ?self.rec_loop, punch = ?self.rec_punch, "recording starts");
        self.rec_start = start;
        self.live_peaks.clear();
        self.rec_clipped = false;
        self.state.status = if errors.is_empty() {
            Status::Info(format!(
                "Nimmt auf: {} Spur(en)",
                self.takes.len() + self.midi_takes.len()
            ))
        } else {
            Status::Error(format!("Nimmt auf, aber: {}", errors.join("; ")))
        };
        // Overdub: die anderen Spuren spielen ab dem Kopf mit.
        if let Some(p) = self.player.as_mut() {
            p.play();
        }
    }

    /// Loop-Bereich in Beats, wenn die Schleife an ist.
    pub(super) fn loop_beats(&self) -> Option<(f64, f64)> {
        (self.loop_enabled && self.loop_end_bar > self.loop_start_bar).then(|| {
            (
                f64::from(self.loop_start_bar.saturating_sub(1) * 4),
                f64::from(self.loop_end_bar.saturating_sub(1) * 4),
            )
        })
    }

    /// Punch-Bereich in Beats, wenn Punch an ist.
    pub(super) fn punch_beats(&self) -> Option<(f64, f64)> {
        (self.punch_enabled && self.punch_out_bar > self.punch_in_bar).then(|| {
            (
                f64::from(self.punch_in_bar.saturating_sub(1) * 4),
                f64::from(self.punch_out_bar.saturating_sub(1) * 4),
            )
        })
    }

    pub(super) fn stop_record(&mut self) {
        self.level = 0.0;
        self.rec_levels.clear();
        self.live_peaks.clear();
        if let Some(p) = self.player.as_mut() {
            p.stop();
        }
        let start = self.rec_start;
        let mut saved = Vec::new();
        let mut errors = Vec::new();
        let tempo = self.state.project().tempo_bpm;
        for (track, take) in std::mem::take(&mut self.midi_takes) {
            let r = take.finish(tempo);
            tracing::info!(
                events = r.events,
                notes = r.midi.notes.len(),
                dropped = r.dropped,
                "midi take finished"
            );
            if r.midi.notes.is_empty() && r.midi.cc.is_empty() {
                errors.push("MIDI: keine Noten gespielt".to_string());
                continue;
            }
            let n = r.midi.notes.len();
            if let Some(clip) =
                self.state
                    .add_midi_clip(Some(track), "MIDI-Take", start, r.length_beats, r.midi)
            {
                self.selected_clip = Some(clip);
                self.track_focus = false;
                self.selected_track = Some(track);
            }
            let lost = if r.dropped > 0 {
                format!(", {} Events verloren", r.dropped)
            } else {
                String::new()
            };
            saved.push(format!("{n} Noten{lost}"));
        }
        for (track, take) in std::mem::take(&mut self.takes) {
            match take.stop() {
                Ok((s, measured)) => {
                    tracing::info!(seconds = s.seconds(), overruns = s.overruns, "take saved");
                    if let Some(p) = self.player.as_mut() {
                        p.forget(&s.path);
                    }
                    let measured = if self.latency_auto {
                        measured.as_secs_f64()
                    } else {
                        0.0
                    };
                    let latency = measured + self.latency_extra_ms / 1000.0;
                    let rate = f64::from(s.sample_rate.max(1));
                    let latency_frames = (latency.max(0.0) * rate).round() as u64;
                    tracing::info!(
                        latency_ms = latency * 1000.0,
                        latency_frames,
                        frames = s.frames,
                        "latency compensation"
                    );
                    let plan = woa_model::plan_recording(&woa_model::RecordInput {
                        start_beats: start,
                        tempo_bpm: tempo,
                        rate,
                        frames: s.frames,
                        latency_frames,
                        loop_beats: self.rec_loop,
                        punch_beats: self.rec_punch,
                    });
                    let Some(plan) = plan else {
                        errors.push("Aufnahme zu kurz oder außerhalb des Punch-Bereichs".into());
                        continue;
                    };
                    if let woa_model::RecordPlan::Folder { offsets, .. } = &plan {
                        tracing::info!(takes = offsets.len(), "loop takes");
                    }
                    if let Some(clip) = self.state.add_recording(track, &s.path, &plan) {
                        self.selected_clip = Some(clip);
                        self.track_focus = false;
                        self.selected_track = self.state.project().clip(clip).map(|(t, _)| t.id);
                    }
                    if let woa_model::RecordPlan::Folder { offsets, .. } = &plan {
                        let n = offsets.len();
                        saved.push(format!("{n} Takes"));
                        continue;
                    }
                    let lost = if s.overruns > 0 {
                        format!(", {} Samples verloren", s.overruns)
                    } else {
                        String::new()
                    };
                    saved.push(format!("{:.1} s{lost}", s.seconds()));
                }
                Err(e) => {
                    tracing::error!(error = %e, "take failed");
                    errors.push(e);
                }
            }
        }
        self.state.status = if errors.is_empty() {
            Status::Info(format!(
                "Aufnahme gespeichert: {} Spur(en) ({})",
                saved.len(),
                saved.join(", ")
            ))
        } else {
            Status::Error(format!("Aufnahme fehlgeschlagen: {}", errors.join("; ")))
        };
    }

    /// Quellenliste, höchstens jede Sekunde neu abgefragt.
    pub(super) fn sources(&mut self) -> &[SourceEntry] {
        let stale = self
            .source_list
            .1
            .is_none_or(|t| t.elapsed() > std::time::Duration::from_secs(1));
        if stale {
            self.source_list = (sources::list(), Some(Instant::now()));
        }
        &self.source_list.0
    }

    pub(super) fn rewind(&mut self) {
        let tempo = self.state.project().tempo_bpm;
        if let Some(p) = self.player.as_mut() {
            p.seek_beats(0.0, tempo);
        }
    }

    pub(super) fn toggle_play(&mut self) {
        if self.recording() {
            self.toggle_record();
        } else {
            let end = self.audio_end();
            let tempo = self.state.project().tempo_bpm;
            let at_end = self.stop_at_end;
            if let Some(p) = self.player.as_mut() {
                // Am Ende angekommen: Abspielen beginnt wieder vorn.
                if at_end && !p.is_playing() && end > 0.0 && p.position_beats(tempo) >= end {
                    p.seek_beats(0.0, tempo);
                }
                p.toggle();
            }
        }
    }

    /// Transportleiste: Wiedergabe, große Zeitanzeige, Aufnahme mit Pegel, Latenz;
    /// rechts Zoom und Export-Lautheit.
    pub(super) fn transport_bar(&mut self, ui: &mut egui::Ui) {
        // #443: Umbrechend statt überlappend. Vor jeder Gruppe prüft `fit`, ob sie noch in die
        // Zeile passt (Breiten als Schätzung, lieber großzügig), sonst beginnt eine neue Zeile.
        ui.horizontal_wrapped(|ui| {
            let playing = self.player.as_ref().is_some_and(Player::is_playing);

            // 1. Hardware-Transport-Gruppe
            egui::Frame::new()
                .fill(theme::well())
                .stroke(egui::Stroke::new(1.0, theme::border()))
                .corner_radius(5)
                .inner_margin(egui::Margin::symmetric(4, 3))
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    if crate::widgets::Key::new("", egui::vec2(38.0, 32.0))
                        .icon(theme::icf::SKIP_BACK)
                        .font(theme::label_font(16.0))
                        .ui(ui)
                        .on_hover_text("Zum Anfang (Pos1)")
                        .clicked()
                    {
                        self.rewind();
                    }
                    // Play: grün eingerastet, solange es läuft.
                    let play = crate::widgets::Key::new("", egui::vec2(48.0, 32.0))
                        .icon(if playing {
                            theme::icf::PAUSE
                        } else {
                            theme::icf::PLAY
                        })
                        .font(theme::label_font(16.0))
                        .color(theme::ok())
                        .on(playing)
                        .enabled(self.player.is_some())
                        .ui(ui);
                    if play
                        .on_hover_text("Abspielen / Pause (Leertaste)")
                        .on_disabled_hover_text(
                            "Keine Audioausgabe geöffnet. Den Grund nennt die Statuszeile beim Start.",
                        )
                        .clicked()
                    {
                        self.toggle_play();
                    }

                    let recording = self.recording();
                    let (rec_icon, label) = match self.takes.first() {
                        Some((_, rec)) => {
                            let t = rec.elapsed().as_secs();
                            (theme::icf::STOP, format!("{}:{:02}", t / 60, t % 60))
                        }
                        None if recording => (theme::icf::STOP, "MIDI".to_string()),
                        None => (theme::icf::RECORD, "Aufnahme".to_string()),
                    };
                    // Aufnahme: rot (live) eingerastet, solange aufgenommen wird.
                    let rec_resp = crate::widgets::Key::new(&label, egui::vec2(114.0, 32.0))
                        .icon(rec_icon)
                        .color(theme::on_air())
                        .ink(egui::Color32::WHITE)
                        .on(recording)
                        .ui(ui)
                        .on_hover_text(
                        "Nimmt auf jeder scharfen Spur (roter Punkt) ihre Quelle auf, \
                             sonst auf der ausgewählten Spur (Strg+R)",
                    );
                    if rec_resp.clicked() {
                        self.toggle_record();
                    }


                });

            // 1b. Loop · Punch · Stopp am Ende (eine Gruppe, eingeschaltet = accent-soft)
            transport_fit(
                ui,
                TRANSPORT_LOOP_W
                    + if self.loop_enabled { TRANSPORT_BARS_W } else { 0.0 }
                    + if self.punch_enabled { TRANSPORT_BARS_W } else { 0.0 },
            );
            egui::Frame::new()
                .fill(theme::well())
                .stroke(egui::Stroke::new(1.0, theme::border()))
                .corner_radius(5)
                .inner_margin(egui::Margin::symmetric(4, 3))
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    let font = theme::label_font(12.0);
                    let w = |ui: &egui::Ui, t: &str| crate::widgets::key_width(ui, t, &font, 0.0) + 10.0;
                    if crate::widgets::Key::new("Loop", egui::vec2(w(ui, "Loop"), 32.0))
                        .icon(theme::icf::REPEAT)
                        .font(font.clone())
                        .soft(self.loop_enabled)
                        .ui(ui)
                        .on_hover_text("Wiedergabe im gewählten Taktbereich im Kreis abspielen")
                        .clicked()
                    {
                        self.loop_enabled = !self.loop_enabled;
                    }
                    if self.loop_enabled {
                        ui.add(
                            egui::DragValue::new(&mut self.loop_start_bar)
                                .range(1..=self.loop_end_bar.saturating_sub(1).max(1))
                                .speed(0.2)
                                .prefix("Takt "),
                        )
                        .on_hover_text("Loop-Start (Takt)");
                        ui.add(
                            egui::DragValue::new(&mut self.loop_end_bar)
                                .range((self.loop_start_bar + 1)..=512)
                                .speed(0.2)
                                .prefix("– "),
                        )
                        .on_hover_text("Loop-Ende (Takt)");
                    }

                    // Paket 22: Punch-Bereich. Von der Aufnahme bleibt nur dieser Teil.
                    if crate::widgets::Key::new("Punch", egui::vec2(w(ui, "Punch"), 32.0))
                        .icon(theme::icf::RECORD)
                        .font(font.clone())
                        .soft(self.punch_enabled)
                        .ui(ui)
                        .on_hover_text(
                            "Punch-Bereich: von der Aufnahme bleibt nur der Teil zwischen \
                             Punch-In und Punch-Out. Mit Loop landet nur dieser Teil im Take-Ordner.",
                        )
                        .clicked()
                    {
                        self.punch_enabled = !self.punch_enabled;
                    }
                    if self.punch_enabled {
                        ui.add(
                            egui::DragValue::new(&mut self.punch_in_bar)
                                .range(1..=self.punch_out_bar.saturating_sub(1).max(1))
                                .speed(0.2)
                                .prefix("Takt "),
                        )
                        .on_hover_text("Punch-In (Takt)");
                        ui.add(
                            egui::DragValue::new(&mut self.punch_out_bar)
                                .range((self.punch_in_bar + 1)..=512)
                                .speed(0.2)
                                .prefix("– "),
                        )
                        .on_hover_text("Punch-Out (Takt)");
                    }

                    if crate::widgets::Key::new("Stopp am Ende", egui::vec2(w(ui, "Stopp am Ende"), 32.0))
                        .icon(theme::icf::ARROW_LINE_RIGHT)
                        .font(font.clone())
                        .soft(self.stop_at_end)
                        .ui(ui)
                        .on_hover_text(
                            "An: die Wiedergabe hält am Ende des letzten Clips an. \
                             Aus: sie läuft weiter, z. B. zum Aufnehmen über das Ende hinaus.",
                        )
                        .clicked()
                    {
                        self.stop_at_end = !self.stop_at_end;
                    }
                });

            // 2. Zeit-LCD (ScreenStudio): Position Takt.Schlag.16tel, Zeit und das Tempo-Feld.
            let secs = self.player.as_ref().map_or(0.0, Player::position_seconds);
            let tempo = self.state.project().tempo_bpm;
            let beats = self
                .player
                .as_ref()
                .map_or(0.0, |p| p.position_beats(tempo));
            transport_fit(ui, TRANSPORT_LCD_W);
            ui.add_space(4.0);
            theme::lcd_show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 10.0;
                    ui.label(
                        egui::RichText::new(bar_position(beats))
                            .monospace()
                            .strong()
                            .size(22.0)
                            .color(theme::lcd_ink()),
                    )
                    .on_hover_text("Position: Takt.Schlag.Sechzehntel");
                    ui.label(
                        egui::RichText::new(format!(
                            "{:02}:{:04.1}",
                            (secs / 60.0) as u64,
                            secs % 60.0
                        ))
                        .monospace()
                        .strong()
                        .size(22.0)
                        .color(theme::lcd_text()),
                    )
                    .on_hover_text("Zeit seit Projektanfang (Minuten:Sekunden)");
                    let mut bpm = self.tempo_drag.unwrap_or(tempo);
                    let r = ui
                        .add(
                            egui::DragValue::new(&mut bpm)
                                .range(MIN_TEMPO_BPM..=MAX_TEMPO_BPM)
                                .speed(0.5)
                                .fixed_decimals(0)
                                .suffix(" BPM"),
                        )
                        .on_hover_text(
                            "Tempo in Schlägen pro Minute. Ziehen · Doppelklick = 120 BPM · \
                             Klick = eintippen",
                        );
                    if r.double_clicked() {
                        self.tempo_drag = None;
                        self.state.set_tempo(120.0);
                    } else {
                        if r.changed() {
                            self.tempo_drag = Some(bpm);
                        }
                        if r.drag_stopped() || (r.changed() && !r.dragged()) {
                            self.tempo_drag = None;
                            self.state.set_tempo(bpm);
                        }
                    }
                });
            });

            // 3. Live-Aufnahme-Pegelanzeige (wenn Aufnahme aktiv)
            if self.recording() {
                let mut fresh = 0.0f32;
                for (track, take) in &self.takes {
                    // Trim der Spur, auch wenn er sich während der Aufnahme ändert.
                    let trim = track
                        .and_then(|t| self.state.project().track(t))
                        .map_or(0.0, |t| t.trim_db);
                    take.set_trim_db(trim);
                    let l = take.take_level();
                    fresh = fresh.max(l);
                    if let Some(t) = track {
                        let shown = self.rec_levels.entry(*t).or_insert(0.0);
                        *shown = l.max(*shown);
                    }
                }
                self.level = fresh.max(self.level * 0.85);
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(140.0, 14.0), egui::Sense::hover());
                level_bar(ui.painter(), rect, self.level);
                if self.rec_clipped {
                    theme::status_pill(ui, "ÜBERSTEUERT", theme::warn(), true);
                }
            }

            // 3b. Pegel-LCD der Summe (Vorlage ScreenStudio, Gruppe „Pegel“): Beschriftung,
            // horizontale LED-Kette, Spitzenwert in dB. Das analoge VU steht nur in der
            // Pegelansicht des Analyse-Fensters (André 02.11.2026).
            // Fehlt in der Zeile der Platz, rutscht die Pegel-LCD in die nächste (#443),
            // sie verschwindet nie.
            {
                transport_fit(ui, TRANSPORT_LEVEL_W);
                let sum = self.sum_level();
                theme::lcd_show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 10.0;
                        ui.label(theme::panel_label("PEGEL").color(theme::lcd_text()));
                        let (rect, _) =
                            ui.allocate_exact_size(egui::vec2(150.0, 12.0), egui::Sense::hover());
                        theme::led_meter(ui.painter(), rect, sum);
                        ui.label(
                            egui::RichText::new(level_db_text(sum))
                                .monospace()
                                .size(12.0)
                                .color(theme::lcd_text()),
                        );
                    });
                })
                .response
                .on_hover_text("Pegel der Summe (analoges VU: Analyse-Fenster › Pegelansicht)");
            }

            // 3c. Abhörlautstärke (#358): kleiner Regler, nur der Lautsprecher.
            transport_fit(ui, TRANSPORT_VOLUME_W);
            self.monitor_volume_ui(ui);

            // 4. Latenz: Symbol-Schalter und Zusatz-Feld in ms.
            transport_fit(ui, TRANSPORT_LATENCY_W);
            egui::Frame::new()
                .fill(theme::well())
                .stroke(egui::Stroke::new(1.0, theme::border()))
                .corner_radius(5)
                .inner_margin(egui::Margin::symmetric(4, 3))
                .show(ui, |ui| {
                    // #443: eine Zeile; in der umbrechenden Reihe rutschte das ms-Feld sonst
                    // unter den Schalter und über die Zeile darunter.
                    ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    let out_ms = woa_engine::device::output_latency().as_secs_f64() * 1000.0;
                    let on = self.latency_auto;
                    // Hardware-Schalter (`Key`, leuchtet bei an), kein egui-Knopf.
                    if crate::widgets::Key::new(theme::ic::TIMER, egui::vec2(32.0, 32.0))
                        .font(egui::FontId::proportional(16.0))
                        .lit(on)
                        .ui(ui)
                        .on_hover_text(format!(
                            "Latenz ausgleichen: {}. Verschiebt jede Aufnahme um die vom Treiber \
                             gemeldete Latenz nach vorn (Ausgang zuletzt {out_ms:.1} ms, Eingang \
                             wird bei der Aufnahme gemessen)",
                            if on { "an" } else { "aus" }
                        ))
                        .clicked()
                    {
                        self.latency_auto = !on;
                    }
                    ui.add_sized(
                        [76.0, 32.0],
                        egui::DragValue::new(&mut self.latency_extra_ms)
                            .range(-500.0..=500.0)
                            .speed(0.5)
                            .fixed_decimals(1)
                            .suffix(" ms"),
                    )
                    .on_hover_text(
                        "Zusätzlicher Ausgleich von Hand in ms, falls Aufnahmen noch zu spät \
                         liegen. Ziehen · Klick = eintippen",
                    );
                    });
                });

            // 5. Rechtsbündig: Export · Zoom · Raster · Takte/Zeit (Reihenfolge von rechts).
            // Reicht der Rest der Zeile nicht, steht die Gruppe rechtsbündig in einer eigenen Zeile.
            transport_fit(ui, TRANSPORT_RIGHT_W);
            // #443: als eigenes Element über den Rest der Zeile, nicht per `with_layout`: das
            // legte die Gruppe in der umbrechenden Reihe über die Pegel-LCD.
            let rest = egui::vec2(row_rest(ui) - 1.0, TRANSPORT_ROW_H);
            ui.allocate_ui_with_layout(rest, egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let font = theme::panel_label_font();
                let w = crate::widgets::key_width(ui, "EXPORT", &font, 72.0);
                if crate::widgets::Key::new("EXPORT", egui::vec2(w, 32.0))
                    .font(font)
                    .ui(ui)
                    .on_hover_text("Projekt exportieren (WAV, FLAC, MP3, Stems …)")
                    .clicked()
                {
                    self.export();
                }
                let name = |v: Option<f64>| match v {
                    None => "unverändert".to_string(),
                    Some(l) => format!("{l:.0} LUFS"),
                };
                egui::ComboBox::from_id_salt("export_lufs")
                    .selected_text(name(self.export_lufs))
                    .width(106.0)
                    .show_ui(ui, |ui| {
                        for (v, hint) in [
                            (Some(-16.0), " · Web, Podcast"),
                            (Some(-23.0), " · Rundfunk, EBU R128"),
                            (None, ""),
                        ] {
                            ui.selectable_value(
                                &mut self.export_lufs,
                                v,
                                format!("{}{hint}", name(v)),
                            );
                        }
                    })
                    .response
                    .on_hover_text("Lautheit beim Export");
                ui.separator();
                if icon_button(ui, theme::ic::MAGNIFYING_GLASS_PLUS, "Hineinzoomen (Strg+Mausrad)",
                )
                .clicked()
                {
                    self.zoom = (self.zoom * 1.5).clamp(*ZOOM_RANGE.start(), *ZOOM_RANGE.end());
                }
                if icon_button(ui, theme::ic::MAGNIFYING_GLASS_MINUS, "Herauszoomen (Strg+Mausrad)",
                )
                .clicked()
                {
                    self.zoom = (self.zoom / 1.5).clamp(*ZOOM_RANGE.start(), *ZOOM_RANGE.end());
                }
                egui::ComboBox::from_id_salt("snap")
                    .selected_text(self.snap.label())
                    .width(64.0)
                    .show_ui(ui, |ui| {
                        for s in Snap::ALL {
                            ui.selectable_value(&mut self.snap, s, s.label());
                        }
                    })
                    .response
                    .on_hover_text("Raster beim Verschieben und Kürzen von Clips");
                // Lineal: Takte oder Zeit als Segmentschalter (rechts nach links gebaut).
                let mut unit = self.time_unit;
                ui.scope(|ui| {
                    // Segmentschalter aus zwei Hardware-Tasten: die gewählte rastet ein.
                    ui.spacing_mut().item_spacing.x = 2.0;
                    let font = theme::label_font(13.0);
                    for (u, label, tip) in [
                        (TimeUnit::Seconds, "Zeit", "Lineal in Minuten und Sekunden"),
                        (TimeUnit::Bars, "Takte", "Lineal in Takten"),
                    ] {
                        let w = crate::widgets::key_width(ui, label, &font, 52.0);
                        if crate::widgets::Key::new(label, egui::vec2(w, 32.0))
                            .font(font.clone())
                            .lit(unit == u)
                            .ui(ui)
                            .on_hover_text(tip)
                            .clicked()
                        {
                            unit = u;
                        }
                    }
                });
                self.time_unit = unit;
            });
        });
    }
}
