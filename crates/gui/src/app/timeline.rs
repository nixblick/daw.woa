//! Timeline und Clip-Bearbeitung.

use super::*;

impl WoaApp {
    pub(super) fn timeline(&mut self, ui: &mut egui::Ui, track: TrackId) {
        let tempo = self.state.project().tempo_bpm;
        let px = self.zoom;
        let Some(t) = self.state.project().track(track) else {
            return;
        };
        let clips = t.clips.clone();
        let caption =
            (t.kind == woa_model::TrackKind::Midi).then(|| crate::timeline::sound_caption(t.sound));
        let key = crate::timeline::harmony(self.state.project(), 0.0, 0).key;
        let index = self
            .state
            .project()
            .tracks
            .iter()
            .position(|t| t.id == track)
            .unwrap_or(0);
        let color = theme::track_color(index);
        let end = self.timeline_end();
        let width = (end as f32 + 16.0) * px;
        let (rect, resp) =
            ui.allocate_exact_size(egui::vec2(width, self.row_h()), egui::Sense::click());
        self.track_rects.push((track, rect));
        let painter = ui.painter_at(rect);
        // Gewählte Spur: 6 % Spurfarbe auf `well` (Design `SelectionSync`).
        let lane = if self.rack_track() == Some(track) {
            theme::mix(theme::well(), color, 0.06)
        } else {
            theme::well()
        };
        painter.rect_filled(rect, 5.0, lane);
        painter.rect_stroke(
            rect,
            5.0,
            egui::Stroke::new(1.0, theme::border()),
            egui::StrokeKind::Inside,
        );
        // Raster: jeder zweite Takt leicht aufgehellt, Schläge als feine Linien.
        let bars = end as usize / 4 + 4;
        for bar in 0..bars {
            let x = rect.left() + (bar * 4) as f32 * px;
            if bar % 2 == 1 {
                let r = egui::Rect::from_x_y_ranges(x..=x + 4.0 * px, rect.y_range());
                painter.rect_filled(r, 0.0, theme::mix(theme::well(), theme::panel(), 0.38));
            }
            if px >= 10.0 {
                for b in 1..4 {
                    let xb = x + b as f32 * px;
                    painter.vline(
                        xb,
                        rect.y_range(),
                        egui::Stroke::new(1.0, theme::mix(theme::well(), theme::grid(), 0.45)),
                    );
                }
            }
            painter.vline(x, rect.y_range(), egui::Stroke::new(1.0, theme::grid()));
        }
        let ppb = self.player.as_ref().map(|p| p.peaks_per_beat(tempo));
        let auto_xf = crate::timeline::crossfade_overlaps(&clips);
        for c in &clips {
            let drag = match self.clip_drag {
                Some((id, d))
                    if id == c.id
                        || (self.selected_clips.contains(&id)
                            && self.selected_clips.contains(&c.id)) =>
                {
                    d
                }
                _ => 0.0,
            };
            // Gezogener Rand: Anzeige mit vorläufigen Werten.
            let (mut start, mut len, mut offset) =
                (c.start_beats + drag, c.length_beats, c.offset_beats);
            if let Some((id, left, d)) = self.trim
                && id == c.id
            {
                if left {
                    (start, len, offset) = (start + d, len - d, offset + d);
                } else {
                    len += d;
                }
            }
            // Alt-Ziehen am Rand dehnt die Zeit: Länge ändert sich, der Ton wird mitgedehnt.
            let mut view_ratio = c.time_ratio(tempo);
            if let Some((id, left, d)) = self.stretch_drag
                && id == c.id
            {
                if left {
                    (start, len) = (start + d, len - d);
                } else {
                    len += d;
                }
                view_ratio *= len / c.length_beats;
            }
            let start = start.max(0.0);
            let x0 = rect.left() + start as f32 * px;
            let x1 = x0 + (len as f32 * px).max(2.0);
            let r = egui::Rect::from_x_y_ranges(x0..=x1, rect.top() + 3.0..=rect.bottom() - 3.0);
            // Anzeige-Kopie mit gezogenen und automatischen Fades (Überblendung).
            let mut cv = c.clone();
            cv.length_beats = len;
            if let Some((id, a, b)) = self.fade_handle
                && id == c.id
            {
                (cv.fade_in_beats, cv.fade_out_beats) = (a, b);
            }
            if let Some(&(ai, ao)) = auto_xf.get(&c.id) {
                if ai > cv.fade_in_beats {
                    (cv.fade_in_beats, cv.fade_in_curve) = (ai, woa_model::FadeCurve::EqualPower);
                }
                if ao > cv.fade_out_beats {
                    (cv.fade_out_beats, cv.fade_out_curve) = (ao, woa_model::FadeCurve::EqualPower);
                }
            }
            let gain_lin = 10f32.powf(c.gain_db as f32 / 20.0);
            let cresp = ui.interact(
                r,
                ui.id().with(("clip", c.id.0)),
                egui::Sense::click_and_drag(),
            );
            let base = if c.audio.is_some() || c.midi.is_some() {
                color
            } else {
                theme::text_dim()
            };
            let selected = self.selected_clip == Some(c.id) || self.selected_clips.contains(&c.id);
            painter.rect_filled(r, 5.0, theme::mix(base, theme::well(), 0.68));
            painter.rect_stroke(
                r,
                5.0,
                egui::Stroke::new(1.0, theme::mix(base, theme::border(), 0.35)),
                egui::StrokeKind::Inside,
            );
            // Kopfzeile des Clips in der Spurfarbe, darin der Name.
            let head = egui::Rect::from_min_max(r.min, egui::pos2(r.right(), r.top() + 16.0));
            painter.rect_filled(
                head,
                egui::CornerRadius {
                    nw: 5,
                    ne: 5,
                    sw: 0,
                    se: 0,
                },
                theme::mix(base, theme::well(), if selected { 0.0 } else { 0.18 }),
            );
            // Waveform from the cached peaks.
            let file = c
                .audio
                .as_deref()
                .and_then(|rel| self.state.resolve_audio(rel));
            let peaks = match (file, self.player.as_mut()) {
                (Some(file), Some(p)) => p.peaks(&file),
                _ => None,
            };
            let audio_beats = peaks.as_ref().zip(ppb).map(|(p, ppb)| p.len() as f64 / ppb);
            if let (Some(ppb), Some(peaks)) = (ppb, peaks) {
                let mid = (r.top() + 16.0 + r.bottom()) / 2.0;
                let half = (r.bottom() - r.top() - 16.0) / 2.0 - 2.0;
                let fill_stroke = egui::Stroke::new(1.0, theme::emphasize(base, 0.25));
                let peak_col = theme::emphasize(base, 0.65);
                let mut x = r.left();
                while x < r.right() {
                    // Position im Ton in Takt-Einheiten des Projekttempos; Dehnung und
                    // Tempo-Folgen strecken die Zeitachse des Clips (Paket 23).
                    let b0 = offset * tempo / c.source_bpm(tempo)
                        + f64::from((x - r.left()) / px) / view_ratio;
                    let b1 = b0 + f64::from(1.0 / px) / view_ratio;
                    let (i0, i1) = (
                        (b0 * ppb) as usize,
                        ((b1 * ppb) as usize).max((b0 * ppb) as usize + 1),
                    );
                    let v = peaks
                        .get(i0.min(peaks.len())..i1.min(peaks.len()))
                        .map_or(0.0, |w| w.iter().fold(0.0f32, |m, s| m.max(*s)));
                    let local = f64::from((x - r.left()) / px);
                    let env = fade_gain(&cv, local) as f32 * gain_lin;
                    let h = (v * env).min(1.0) * half;
                    painter.vline(x, (mid - h)..=(mid + h), fill_stroke);
                    if h > 1.0 {
                        painter.circle_filled(egui::pos2(x, mid - h), 0.7, peak_col);
                        painter.circle_filled(egui::pos2(x, mid + h), 0.7, peak_col);
                    }
                    x += 1.0;
                }
            }
            // MIDI-Noten-Vorschau (Mini-Piano-Roll direkt im Clip wie in Ableton)
            // Akkord-Clip (Design `ChordTrack`): je Takt der Akkord, Trennlinien in Spurfarbe.
            let mut body_top = r.top() + 18.0;
            if let Some(midi) = &c.midi
                && woa_model::harmony::is_chord_clip(&midi.notes)
                && r.height() > 44.0
            {
                let row = egui::Rect::from_min_max(
                    egui::pos2(r.left(), r.top() + 16.0),
                    egui::pos2(r.right(), r.top() + 30.0),
                );
                let bars = (len / 4.0).ceil().max(1.0) as usize;
                let local = midi.notes.iter().map(|n| (n.beat - offset, n));
                let row_view = painter.with_clip_rect(row.intersect(rect));
                for (bar, chord) in woa_model::harmony::bar_chords(local, bars)
                    .into_iter()
                    .enumerate()
                {
                    let x = r.left() + (bar * 4) as f32 * px;
                    if bar > 0 {
                        row_view.vline(x, row.y_range(), egui::Stroke::new(1.0, base));
                    }
                    if let Some(ch) = chord {
                        let deg = key.and_then(|k| ch.degree(k)).unwrap_or("");
                        row_view.text(
                            egui::pos2(x + 4.0, row.center().y),
                            egui::Align2::LEFT_CENTER,
                            format!("{} {deg}", ch.name()).trim_end(),
                            egui::FontId::new(11.0, egui::FontFamily::Name("ui-bold".into())),
                            theme::text(),
                        );
                    }
                }
                row_view.hline(row.x_range(), row.bottom(), egui::Stroke::new(1.0, base));
                body_top = row.bottom() + 2.0;
            }
            if let Some(midi) = &c.midi
                && !midi.notes.is_empty()
            {
                let body = egui::Rect::from_min_max(
                    egui::pos2(r.left() + 2.0, body_top),
                    egui::pos2(r.right() - 2.0, r.bottom() - 3.0),
                );
                let min_p = midi.notes.iter().map(|n| n.pitch).min().unwrap_or(48);
                let max_p = midi.notes.iter().map(|n| n.pitch).max().unwrap_or(72);
                let span = f32::from((max_p.saturating_sub(min_p)).max(12)) + 2.0;
                let note_h = (body.height() / span).clamp(2.5, 6.0);
                let note_col = theme::emphasize(base, 0.55);
                let clip_view = painter.with_clip_rect(body.intersect(rect));
                for n in &midi.notes {
                    let ns = n.beat - offset;
                    let ne = ns + n.length_beats;
                    if ne <= 0.0 || ns >= len {
                        continue;
                    }
                    let nx0 = r.left() + (ns.max(0.0) as f32) * px;
                    let nx1 = r.left() + (ne.min(len) as f32) * px;
                    let rel = 1.0 - f32::from(n.pitch.saturating_sub(min_p)) / span;
                    let ny = body.top() + rel * (body.height() - note_h);
                    let nr = egui::Rect::from_min_max(
                        egui::pos2(nx0, ny),
                        egui::pos2(nx1.max(nx0 + 2.0), ny + note_h),
                    );
                    clip_view.rect_filled(nr, 1.5, note_col);
                }
            }
            // Fades als abgedunkelte Dreiecke + helle Hüllkurven-Diagonale.
            let shade = egui::Color32::from_black_alpha(135);
            let fade_line = egui::Stroke::new(1.5, theme::emphasize(base, 0.7));
            if c.audio.is_some() {
                let body_top = r.top() + 16.0;
                let h = r.bottom() - body_top;
                for (fade, curve, out) in [
                    (cv.fade_in_beats, cv.fade_in_curve, false),
                    (cv.fade_out_beats, cv.fade_out_curve, true),
                ] {
                    if fade <= 0.0 {
                        continue;
                    }
                    let w = (fade as f32 * px).min(r.width());
                    let x_start = if out { r.right() - w } else { r.left() };
                    let pts = crate::timeline::fade_points(curve, out, 16);
                    let at = |(x, g): (f32, f32)| egui::pos2(x_start + x * w, r.bottom() - g * h);
                    for pair in pts.windows(2) {
                        let (p, q) = (at(pair[0]), at(pair[1]));
                        let seg = egui::Rect::from_min_max(
                            egui::pos2(p.x, r.top()),
                            egui::pos2(q.x, (p.y + q.y) / 2.0),
                        );
                        painter.rect_filled(seg, 0.0, shade);
                    }
                    painter.add(egui::Shape::line(
                        pts.iter().map(|&p| at(p)).collect(),
                        fade_line,
                    ));
                }
                // Griffe an den oberen Ecken: ziehen ändert die Fade-Länge.
                let selected_audio = selected;
                for out in [false, true] {
                    let fade = if out {
                        cv.fade_out_beats
                    } else {
                        cv.fade_in_beats
                    };
                    let edge = (fade as f32 * px).clamp(10.0, (r.width() - 10.0).max(10.0));
                    let hx = if out {
                        r.right() - edge
                    } else {
                        r.left() + edge
                    };
                    let hp = egui::pos2(hx, body_top);
                    let hresp = ui
                        .interact(
                            egui::Rect::from_center_size(hp, egui::vec2(14.0, 14.0)),
                            ui.id().with(("fade", c.id.0, out)),
                            egui::Sense::drag(),
                        )
                        .on_hover_cursor(egui::CursorIcon::ResizeHorizontal)
                        .on_hover_text(if out {
                            "Fade-out ziehen"
                        } else {
                            "Fade-in ziehen"
                        });
                    let hot = hresp.hovered() || hresp.dragged();
                    if hot || selected_audio {
                        painter.circle_filled(
                            hp,
                            if hot { 5.5 } else { 4.0 },
                            if hot { theme::accent() } else { theme::text() },
                        );
                    }
                    if hresp.drag_started() {
                        self.selected_clip = Some(c.id);
                        self.track_focus = false;
                        self.selected_clips = vec![c.id];
                    }
                    if hresp.dragged() {
                        let (mut fi, mut fo) = self
                            .fade_handle
                            .filter(|(id, _, _)| *id == c.id)
                            .map_or((c.fade_in_beats, c.fade_out_beats), |(_, a, b)| (a, b));
                        let d = f64::from(hresp.drag_delta().x / px);
                        if out {
                            fo = (fo - d).clamp(0.0, (len - fi).max(0.0));
                        } else {
                            fi = (fi + d).clamp(0.0, (len - fo).max(0.0));
                        }
                        self.fade_handle = Some((c.id, fi, fo));
                    }
                    if hresp.drag_stopped()
                        && let Some((id, fi, fo)) = self.fade_handle.take()
                    {
                        let round = |v: f64| (v * 100.0).round() / 100.0;
                        self.state.set_fades(id, round(fi), round(fo));
                    }
                }
            }
            if selected {
                painter.rect_stroke(
                    r,
                    5.0,
                    egui::Stroke::new(2.0, theme::text()),
                    egui::StrokeKind::Inside,
                );
            }
            painter.with_clip_rect(head.intersect(rect)).text(
                head.left_center() + egui::vec2(6.0, 0.0),
                egui::Align2::LEFT_CENTER,
                &c.name,
                egui::FontId::proportional(11.5),
                theme::well(),
            );
            // Klang rechts im Clip-Kopf, in Versalien (Design `ChordTrack`).
            let mut cap_rect = None;
            if let Some(cap) = &caption
                && head.width() > 120.0
            {
                cap_rect = Some(painter.with_clip_rect(head.intersect(rect)).text(
                    head.right_center() - egui::vec2(6.0, 0.0),
                    egui::Align2::RIGHT_CENTER,
                    cap,
                    theme::label_font(10.5),
                    theme::well(),
                ));
            }
            // Klick auf den Klangnamen im Clip-Kopf öffnet die Klangauswahl (02.11.2026).
            if cresp.clicked()
                && let (Some(r), Some(p)) = (cap_rect, cresp.interact_pointer_pos())
                && r.expand(3.0).contains(p)
            {
                self.open_sound_picker(track);
            }
            if cresp.clicked() || cresp.drag_started() {
                let m = ui.input(|i| i.modifiers);
                if m.command || m.shift {
                    // Strg/Umschalt+Klick: zur Auswahl hinzufügen oder herausnehmen.
                    if let Some(p) = self.selected_clip
                        && !self.selected_clips.contains(&p)
                    {
                        self.selected_clips.push(p);
                    }
                    if cresp.clicked() && self.selected_clips.contains(&c.id) {
                        self.selected_clips.retain(|x| *x != c.id);
                    } else if !self.selected_clips.contains(&c.id) {
                        self.selected_clips.push(c.id);
                    }
                } else if !(cresp.drag_started() && self.selected_clips.contains(&c.id)) {
                    self.selected_clips = vec![c.id];
                }
                self.selected_clip = Some(c.id);
                self.track_focus = false;
                self.selected_track = Some(track);
            }
            if cresp.dragged() {
                let d =
                    self.clip_drag.map_or(0.0, |(_, d)| d) + f64::from(cresp.drag_delta().x / px);
                self.clip_drag = Some((c.id, d));
            }
            if cresp.drag_stopped()
                && let Some((id, d)) = self.clip_drag.take()
            {
                let snapped = self.snap.apply(c.start_beats + d);
                if self.selected_clips.len() > 1 && self.selected_clips.contains(&id) {
                    let ids = self.selected_clips.clone();
                    self.state.shift_clips(&ids, snapped - c.start_beats);
                    continue;
                }
                // Losgelassen über einer anderen Spur: dorthin verschieben.
                let target = ui
                    .ctx()
                    .pointer_latest_pos()
                    .and_then(|pos| {
                        self.prev_track_rects
                            .iter()
                            .find(|(_, r)| r.y_range().contains(pos.y))
                    })
                    .map(|(t, _)| *t);
                self.state.move_clip_to(id, target, snapped);
            }
            if cresp.double_clicked() && c.midi.is_some() {
                self.open_piano_roll(c.id);
            }
            cresp.on_hover_text(
                if c.midi.is_some() {
                    "Doppelklick: Piano-Roll · Ziehen: verschieben (auch auf andere Spur) · Ränder ziehen: kürzen · S: am Kopf schneiden · Strg+C/X/V: kopieren, ausschneiden, einfügen · Strg+D: verdoppeln · Entf: löschen"
                } else {
                    "Ziehen: verschieben (auch auf andere Spur) · Ränder ziehen: kürzen, mit Alt: Zeit dehnen · S: am Kopf schneiden · Strg+C/X/V: kopieren, ausschneiden, einfügen · Strg+D: verdoppeln · Entf: löschen"
                },
            );
            // Griffe an beiden Rändern zum Kürzen.
            for left in [true, false] {
                let hx = if left { r.left() } else { r.right() - 6.0 };
                let h = egui::Rect::from_x_y_ranges(hx..=hx + 6.0, r.y_range());
                let hresp = ui
                    .interact(h, ui.id().with(("trim", c.id.0, left)), egui::Sense::drag())
                    .on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
                if hresp.drag_started() {
                    self.selected_clip = Some(c.id);
                    self.track_focus = false;
                }
                let alt_stretch = c.audio.is_some()
                    && (self.stretch_drag.is_some_and(|(id, _, _)| id == c.id)
                        || (hresp.drag_started() && ui.input(|i| i.modifiers.alt)));
                if hresp.dragged() && alt_stretch {
                    let d = self.stretch_drag.map_or(0.0, |(_, _, d)| d)
                        + f64::from(hresp.drag_delta().x / px);
                    // Grenzen: Faktor im erlaubten Bereich, mindestens 1/4 Beat, links nicht
                    // vor den Anfang der Zeitachse.
                    let (lo, hi) = woa_model::STRETCH_RANGE;
                    let (min_len, max_len) = (
                        (c.length_beats * lo / c.stretch).max(0.25),
                        c.length_beats * hi / c.stretch,
                    );
                    let d = if left {
                        d.clamp(c.length_beats - max_len, c.length_beats - min_len)
                            .max(-c.start_beats)
                    } else {
                        d.clamp(min_len - c.length_beats, max_len - c.length_beats)
                    };
                    self.stretch_drag = Some((c.id, left, d));
                } else if hresp.dragged() {
                    let d =
                        self.trim.map_or(0.0, |(_, _, d)| d) + f64::from(hresp.drag_delta().x / px);
                    // Grenzen: mindestens 1/4 Beat lang, links nicht vor den Audio-Anfang,
                    // rechts nicht über das Audio-Ende (wenn bekannt). Die Zeitachse des
                    // Clips ist um `stretch` gedehnt, Versatz und Audio-Ende liegen in Quell-Beats.
                    let d = if left {
                        d.clamp(-c.offset_beats * c.stretch, c.length_beats - 0.25)
                    } else {
                        let max = audio_beats.map_or(f64::MAX, |a| {
                            (a * c.source_bpm(tempo) / tempo - c.offset_beats) * c.stretch
                                - c.length_beats
                        });
                        d.clamp(0.25 - c.length_beats, max.max(0.0))
                    };
                    self.trim = Some((c.id, left, d));
                }
                if hresp.drag_stopped()
                    && let Some((id, l, d)) = self.stretch_drag.take()
                {
                    let edge = if l {
                        c.start_beats + d
                    } else {
                        c.end_beats() + d
                    };
                    let edge = self.snap.apply(edge);
                    let length = if l {
                        c.end_beats() - edge
                    } else {
                        edge - c.start_beats
                    };
                    self.state.stretch_clip(id, length, l);
                } else if hresp.drag_stopped()
                    && let Some((id, l, d)) = self.trim.take()
                {
                    let d = self.snap_trim(c, l, d);
                    if l {
                        self.state.trim_clip(
                            id,
                            c.start_beats + d,
                            c.length_beats - d,
                            c.offset_beats + d / c.stretch,
                        );
                    } else {
                        self.state
                            .trim_clip(id, c.start_beats, c.length_beats + d, c.offset_beats);
                    }
                }
            }
        }
        if let Some((peaks, rate)) = self.live_peaks.get(&track) {
            draw_live_take(
                &painter,
                rect,
                px,
                self.rec_start,
                self.live_end_beats(),
                peaks,
                f64::from(*rate) * 60.0 / tempo / PEAK_BLOCK as f64,
            );
        }
        if let Some(p) = &self.player {
            let x = rect.left() + p.position_beats(tempo) as f32 * px;
            painter.vline(x, rect.y_range(), egui::Stroke::new(2.0, theme::accent()));
        }
        let is_midi_track = self
            .state
            .project()
            .track(track)
            .is_some_and(|t| t.kind == woa_model::TrackKind::Midi);
        if resp.double_clicked()
            && is_midi_track
            && let Some(pos) = resp.interact_pointer_pos()
        {
            let beat = f64::from(((pos.x - rect.left()) / px).max(0.0));
            let bar_start = (beat / 4.0).floor() * 4.0;
            if let Some(clip) = self
                .state
                .create_empty_midi_clip(Some(track), bar_start, 16.0)
            {
                self.open_piano_roll(clip);
            }
        } else if resp.clicked() {
            self.selected_track = Some(track);
            self.selected_clip = None;
            self.selected_clips.clear();
            self.track_focus = false;
            if let (Some(pos), Some(p)) = (resp.interact_pointer_pos(), self.player.as_mut()) {
                p.seek_beats(f64::from((pos.x - rect.left()) / px), tempo);
            }
        }
    }

    /// Clip-Inspektor (Design `ClipInspector`, 44 px, eine Zeile): links die Kennung (Spurfarbe,
    /// Typ, Name), dann Fade bzw. Noten, Gain, Zeit; rechts die Werkzeuge, ganz rechts Löschen.
    /// Zustände: nichts gewählt, ein Audio-Clip, ein MIDI-Clip, mehrere Clips („n Clips“).
    pub(super) fn clip_bar(&mut self, ui: &mut egui::Ui) {
        ui.set_min_height(34.0);
        let narrow = ui.available_width() < INSPECTOR_NARROW;
        let ids: Vec<ClipId> = self
            .selection_ids()
            .into_iter()
            .filter(|c| self.state.project().clip(*c).is_some())
            .collect();
        if ids.is_empty() {
            if self.selected_clip.is_some() || !self.selected_clips.is_empty() {
                self.selected_clip = None;
                self.selected_clips.clear();
            }
            // Leer-Zustand in voller Höhe, damit die Zeitleiste nicht springt.
            ui.horizontal_centered(|ui| {
                ui.label(
                    egui::RichText::new(theme::ic::CURSOR_CLICK)
                        .size(16.0)
                        .color(theme::text_dim()),
                );
                ui.label(egui::RichText::new("Kein Clip gewählt").color(theme::text_dim()))
                    .on_hover_text(
                        "Clip anklicken zum Bearbeiten\n\
                         Doppelklick in MIDI-Spur: neuer MIDI-Clip\n\
                         S: Schneiden am Kopf · Entf: Löschen\n\
                         Strg+C / V / D: Kopieren, Einfügen, Verdoppeln\n\
                         Audio-Datei hineinziehen · Strg+Mausrad: Zoom",
                    );
                ui.label(
                    egui::RichText::new("Clip anklicken, um ihn hier zu bearbeiten")
                        .small()
                        .color(theme::text_dim()),
                );
            });
            return;
        }
        let clips: Vec<(usize, woa_model::Clip)> = ids
            .iter()
            .filter_map(|c| {
                let (t, clip) = self.state.project().clip(*c)?;
                let idx = self.track_index(t.id).unwrap_or(0);
                Some((idx, clip.clone()))
            })
            .collect();
        let single = clips.len() == 1;
        let primary = self
            .selected_clip
            .and_then(|p| clips.iter().find(|(_, c)| c.id == p))
            .unwrap_or(&clips[0])
            .clone();
        let color = theme::track_color(primary.0);
        let c = primary.1;
        let is_midi = single && c.midi.is_some();
        let all_audio = clips.iter().all(|(_, c)| c.audio.is_some());
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // Werkzeuge von rechts: Löschen mit Abstand, dann Bearbeiten.
                let n = clips.len();
                let tip = if n > 1 {
                    format!("{n} Clips löschen (Entf)")
                } else {
                    "Löschen (Entf)".to_string()
                };
                if danger_icon_button(ui, theme::ic::TRASH, 16.0, &tip).clicked() {
                    self.delete_selected();
                }
                ui.add_space(24.0);
                if is_midi && icon_button(ui, theme::ic::PIANO_KEYS, "Piano-Roll öffnen").clicked()
                {
                    self.open_piano_roll(c.id);
                }
                if icon_button(ui, theme::ic::COPY, "Kopieren (Strg+C)").clicked() {
                    self.copy_selected();
                }
                if icon_button(ui, theme::ic::STACK_PLUS, "Duplizieren (Strg+D)").clicked() {
                    self.duplicate_selected();
                }
                if icon_button(ui, theme::ic::SCISSORS, "Teilen am Kopf (S)").clicked() {
                    self.split_at_playhead();
                }
                ui.separator();
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    // Kennung: Spurfarbe als Kante und 12 % Fläche, Typ-Symbol, Name.
                    let (icon, title) = if !single {
                        (theme::ic::STACK, format!("{} Clips", clips.len()))
                    } else if is_midi {
                        (theme::ic::PIANO_KEYS, c.name.clone())
                    } else {
                        (theme::ic::WAVEFORM, c.name.clone())
                    };
                    let id_frame = egui::Frame::new()
                        .fill(theme::mix(theme::panel(), color, 0.12))
                        .corner_radius(4)
                        .inner_margin(egui::Margin {
                            left: 9,
                            right: 8,
                            top: 3,
                            bottom: 3,
                        })
                        .show(ui, |ui| {
                            ui.label(egui::RichText::new(icon).color(theme::text()));
                            ui.add(
                                egui::Label::new(egui::RichText::new(&title).strong()).truncate(),
                            );
                        });
                    let ir = id_frame.response.rect;
                    ui.painter().rect_filled(
                        egui::Rect::from_min_size(ir.min, egui::vec2(4.0, ir.height())),
                        egui::CornerRadius {
                            nw: 4,
                            sw: 4,
                            ne: 0,
                            se: 0,
                        },
                        color,
                    );
                    let group = |ui: &mut egui::Ui, t: &str| {
                        ui.separator();
                        if !narrow {
                            ui.label(rack_title(t));
                        }
                    };
                    if single {
                        if is_midi {
                            group(ui, "NOTEN");
                            self.clip_notes(ui, &c);
                        }
                        group(ui, "FADE");
                        self.clip_fades(ui, &c);
                        if !is_midi {
                            group(ui, "GAIN");
                            self.clip_gain(ui, &c);
                        }
                        group(ui, "ZEIT");
                        self.clip_position(ui, &c);
                        if !is_midi {
                            self.clip_time(ui, &c);
                        }
                    } else {
                        let all: Vec<woa_model::Clip> =
                            clips.iter().map(|(_, c)| c.clone()).collect();
                        group(ui, "FADE");
                        self.multi_fades(ui, &all);
                        if all_audio {
                            group(ui, "GAIN");
                            self.multi_gain(ui, &all);
                        }
                    }
                });
            });
        });
    }

    /// Start und Länge (Takt.Schlag.Sechzehntel, 4/4), ziehen oder eintippen.
    pub(super) fn clip_position(&mut self, ui: &mut egui::Ui, c: &woa_model::Clip) {
        if let Some(v) = mixed_field(
            ui,
            ("clip_start", c.id.0),
            &[c.start_beats],
            0.0..=1.0e6,
            0.05,
            |b| bar_beat(b, true),
            "Start (Takt.Schlag.Sechzehntel). Ziehen · Klick = eintippen",
        ) {
            self.state.move_clip(c.id, v.value().max(0.0));
        }
        if let Some(v) = mixed_field(
            ui,
            ("clip_len", c.id.0),
            &[c.length_beats],
            0.0625..=1.0e6,
            0.05,
            |b| bar_beat(b, false),
            "Länge (Takte.Schläge.Sechzehntel). Ziehen · Klick = eintippen",
        ) {
            self.state
                .trim_clip(c.id, c.start_beats, v.value().max(0.0625), c.offset_beats);
        }
    }

    /// MIDI-Noten im Inspektor: Raster, Quantisieren aller Noten, Velocity aller Noten.
    pub(super) fn clip_notes(&mut self, ui: &mut egui::Ui, c: &woa_model::Clip) {
        let Some(midi) = c.midi.as_ref() else {
            return;
        };
        egui::ComboBox::from_id_salt(("q_grid", c.id.0))
            .width(52.0)
            .selected_text(format!("1/{}", self.quantize_div))
            .show_ui(ui, |ui| {
                for d in [4u32, 8, 16, 32] {
                    ui.selectable_value(&mut self.quantize_div, d, format!("1/{d}"));
                }
            })
            .response
            .on_hover_text("Raster für Quantisieren");
        // „Q“ ist kein eindeutiges Symbol, also Text-Taste (`Key`) statt `IconButton`.
        if crate::widgets::Key::new("Q", egui::vec2(24.0, 24.0))
            .ui(ui)
            .on_hover_text("Quantisieren: alle Noten auf das Raster (Q)")
            .clicked()
        {
            let grid = woa_model::grid_beats(self.quantize_div, false);
            let (q, _) = midi.quantize(&[], grid, false);
            self.state.set_notes(c.id, q);
        }
        let vel: Vec<f64> = midi.notes.iter().map(|n| f64::from(n.velocity)).collect();
        if !vel.is_empty()
            && let Some(v) = mixed_field(
                ui,
                ("clip_vel", c.id.0),
                &vel,
                1.0..=127.0,
                0.5,
                |v| format!("Vel {v:.0}"),
                "Velocity aller Noten (1–127). Ziehen · Klick = eintippen",
            )
        {
            let all: Vec<usize> = (0..midi.notes.len()).collect();
            let m = midi.set_velocity(&all, v.value().round().clamp(1.0, 127.0) as u8);
            self.state.set_notes(c.id, m);
        }
    }

    /// Fades mehrerer Clips in ms (wie beim einzelnen Clip; gespeichert bleiben Schläge): gleiche
    /// Werte stehen da, unterschiedliche als „—“; Eintippen oder Ziehen setzt alle auf den Wert
    /// (ein Undo-Schritt).
    pub(super) fn multi_fades(&mut self, ui: &mut egui::Ui, clips: &[woa_model::Clip]) {
        let ms_per_beat = 60_000.0 / self.state.project().tempo_bpm.max(1.0);
        let max = clips
            .iter()
            .map(|c| c.length_beats)
            .fold(f64::MAX, f64::min);
        for (icon, key, tip, which) in [
            (
                theme::ic::TREND_UP,
                "multi_fi",
                "Fade-in aller Clips in Millisekunden",
                0u8,
            ),
            (
                theme::ic::TREND_DOWN,
                "multi_fo",
                "Fade-out aller Clips in Millisekunden",
                1u8,
            ),
        ] {
            ui.label(egui::RichText::new(icon).color(theme::text_dim()))
                .on_hover_text(tip);
            let vals: Vec<f64> = clips
                .iter()
                .map(|c| {
                    ms_per_beat
                        * if which == 0 {
                            c.fade_in_beats
                        } else {
                            c.fade_out_beats
                        }
                })
                .collect();
            if let Some(v) = mixed_field(
                ui,
                (key, 0),
                &vals,
                0.0..=max * ms_per_beat,
                5.0,
                |v| format!("{v:.0} ms"),
                tip,
            ) {
                let v = v.value() / ms_per_beat;
                self.state.grouped("Fades", |st| {
                    for c in clips {
                        let (fi, fo) = if which == 0 {
                            (v, c.fade_out_beats)
                        } else {
                            (c.fade_in_beats, v)
                        };
                        st.set_fades(c.id, fi, fo);
                    }
                });
            }
        }
    }

    /// Gain mehrerer Clips: Eintippen setzt alle, Ziehen verschiebt alle relativ.
    pub(super) fn multi_gain(&mut self, ui: &mut egui::Ui, clips: &[woa_model::Clip]) {
        let vals: Vec<f64> = clips.iter().map(|c| c.gain_db).collect();
        if let Some(v) = mixed_field(
            ui,
            ("multi_gain", 0),
            &vals,
            woa_model::MIN_GAIN_DB..=woa_model::MAX_GAIN_DB,
            0.1,
            |v| format!("{v:.1} dB"),
            "Clip-Gain aller Clips. Ziehen verschiebt alle relativ · Doppelklick = 0 dB · \
             Klick = eintippen setzt alle",
        ) {
            self.state.grouped("Clip-Gain", |st| {
                for c in clips {
                    let g = match v {
                        Mixed::Dragged { from, to } => c.gain_db + (to - from),
                        other => other.value(),
                    };
                    st.set_clip_gain(c.id, g);
                }
            });
        }
    }

    /// Fade-in und Fade-out: Länge in Beats, bei Audio daneben die Kurve.
    pub(super) fn clip_fades(&mut self, ui: &mut egui::Ui, c: &woa_model::Clip) {
        use woa_model::FadeCurve;
        let id = c.id;
        let (mut fi, mut fo) = match self.fade_edit {
            Some((cid, a, b)) if cid == id => (a, b),
            _ => (c.fade_in_beats, c.fade_out_beats),
        };
        let (mut cin, mut cout) = (c.fade_in_curve, c.fade_out_curve);
        let short = |k: FadeCurve| match k {
            FadeCurve::Linear => "Lin",
            FadeCurve::EqualPower => "Pow",
            FadeCurve::SCurve => "S",
        };
        // Anzeige und Eingabe in ms (Design-Runde 3), gespeichert bleibt die Länge in Beats.
        let ms_per_beat = 60_000.0 / self.state.project().tempo_bpm.max(1.0);
        let mut rs = Vec::with_capacity(2);
        for (icon, v, cur, salt, tip) in [
            (theme::ic::TREND_UP, &mut fi, &mut cin, 0u8, "Fade-in"),
            (theme::ic::TREND_DOWN, &mut fo, &mut cout, 1u8, "Fade-out"),
        ] {
            ui.label(egui::RichText::new(icon).color(theme::text_dim()))
                .on_hover_text(tip);
            let r = ui
                .add(
                    egui::DragValue::new(v)
                        .range(0.0..=c.length_beats)
                        .speed(0.05)
                        .custom_formatter(move |b, _| format!("{:.0} ms", b * ms_per_beat))
                        .custom_parser(move |t| {
                            let t = t.trim().trim_end_matches("ms").trim().replace(',', ".");
                            t.parse::<f64>().ok().map(|ms| ms / ms_per_beat)
                        }),
                )
                .on_hover_text(format!(
                    "{tip} in Millisekunden. Auch am Clip-Eck ziehen. \
                     Überlappende Clips blenden automatisch über (gleiche Leistung)."
                ));
            rs.push(r);
            if c.audio.is_some() {
                egui::ComboBox::from_id_salt(("fade_curve", id.0, salt))
                    .width(44.0)
                    .selected_text(short(*cur))
                    .show_ui(ui, |ui| {
                        for k in FadeCurve::ALL {
                            ui.selectable_value(cur, k, k.label());
                        }
                    })
                    .response
                    .on_hover_text(format!("{tip}-Kurve: {}", cur.label()));
            }
        }
        if rs.iter().any(egui::Response::changed) {
            self.fade_edit = Some((id, fi, fo));
        }
        let editing = rs.iter().any(|r| r.dragged() || r.has_focus());
        if !editing && let Some((cid, a, b)) = self.fade_edit.take() {
            self.state.set_fades(cid, a, b);
        }
        if (cin, cout) != (c.fade_in_curve, c.fade_out_curve) {
            self.state.set_fade_curves(id, cin, cout);
        }
    }

    /// Clip-Gain in dB und Normalisieren.
    pub(super) fn clip_gain(&mut self, ui: &mut egui::Ui, c: &woa_model::Clip) {
        let id = c.id;
        let mut g = match self.gain_preview {
            Some((pid, pg)) if pid == id => pg,
            _ => c.gain_db,
        };
        let r = ui
            .add(
                egui::DragValue::new(&mut g)
                    .range(woa_model::MIN_GAIN_DB..=woa_model::MAX_GAIN_DB)
                    .speed(0.1)
                    .fixed_decimals(1)
                    .suffix(" dB"),
            )
            .on_hover_text("Clip-Gain. Doppelklick: 0 dB");
        if r.double_clicked() {
            g = 0.0;
        }
        if r.dragged() || r.has_focus() {
            if g != c.gain_db {
                self.gain_preview = Some((id, g));
            }
        } else {
            self.gain_preview = None;
            if g != c.gain_db {
                self.state.set_clip_gain(id, g);
            }
        }
        if icon_button(
            ui,
            theme::ic::ARROW_LINE_UP,
            "Normalisieren: lauteste Stelle auf -1 dBFS",
        )
        .clicked()
        {
            self.normalize_selected(c);
        }
    }

    /// Dehnung, Tonhöhe und Tempo folgen. Die Symbole leuchten, wenn der Clip bearbeitet ist.
    pub(super) fn clip_time(&mut self, ui: &mut egui::Ui, c: &woa_model::Clip) {
        let id = c.id;
        let lit = if c.is_processed(self.state.project().tempo_bpm) {
            theme::cue()
        } else {
            theme::text_dim()
        };
        // Dehnung in Prozent der Länge: 110 = zehn Prozent länger, Tonhöhe bleibt.
        let mut pct = match self.stretch_edit {
            Some((cid, v)) if cid == id => v,
            _ => c.stretch * 100.0,
        };
        ui.label(egui::RichText::new(theme::ic::ARROWS_HORIZONTAL).color(lit))
            .on_hover_text("Dehnung");
        let r = ui
            .add(
                egui::DragValue::new(&mut pct)
                    .range(woa_model::STRETCH_RANGE.0 * 100.0..=woa_model::STRETCH_RANGE.1 * 100.0)
                    .speed(0.5)
                    .fixed_decimals(1)
                    .suffix(" %"),
            )
            .on_hover_text(
                "Dehnung: Länge in Prozent, die Tonhöhe bleibt. Doppelklick: 100 %. \
                 Am Clip-Rand mit Alt ziehen geht auch.",
            );
        if r.double_clicked() {
            pct = 100.0;
        }
        if r.dragged() || r.has_focus() {
            if (pct - c.stretch * 100.0).abs() > 1e-9 {
                self.stretch_edit = Some((id, pct));
            }
        } else {
            self.stretch_edit = None;
            if (pct - c.stretch * 100.0).abs() > 1e-9 {
                self.state.set_clip_stretch(id, pct / 100.0);
            }
        }
        let mut st = match self.pitch_edit {
            Some((cid, v)) if cid == id => v,
            _ => c.pitch_semitones,
        };
        ui.label(egui::RichText::new(theme::ic::MUSIC_NOTE).color(lit))
            .on_hover_text("Tonhöhe");
        let r = ui
            .add(
                egui::DragValue::new(&mut st)
                    .range(woa_model::PITCH_RANGE.0..=woa_model::PITCH_RANGE.1)
                    .speed(0.05)
                    .fixed_decimals(1)
                    .suffix(" st"),
            )
            .on_hover_text("Tonhöhe in Halbtönen (st), die Länge bleibt. Doppelklick: 0.");
        if r.double_clicked() {
            st = 0.0;
        }
        if r.dragged() || r.has_focus() {
            if st != c.pitch_semitones {
                self.pitch_edit = Some((id, st));
            }
        } else {
            self.pitch_edit = None;
            if st != c.pitch_semitones {
                self.state.set_clip_pitch(id, st);
            }
        }
        let follow = c.follow_tempo_bpm;
        let tip = match follow {
            Some(b) => format!(
                "Tempo folgen: an, Ton gehört zu {b:.1} BPM. Ändert sich das Projekt-Tempo, \
                 wird der Ton gedehnt statt verschoben."
            ),
            None => "Tempo folgen: aus. Eingeschaltet wird der Ton bei Tempo-Änderung gedehnt \
                     statt verschoben: er bleibt auf dem Raster, die Tonhöhe bleibt."
                .to_string(),
        };
        if crate::widgets::IconButton::new(theme::ic::METRONOME, &tip)
            .size(crate::widgets::IconSize::Sm)
            .on(follow.is_some())
            .ui(ui)
            .clicked()
        {
            self.state.set_clip_follow_tempo(id, follow.is_none());
        }
    }

    /// Setzt den Gain des Clips so, dass seine Spitze bei -1 dBFS liegt.
    pub(super) fn normalize_selected(&mut self, c: &woa_model::Clip) {
        let tempo = self.state.project().tempo_bpm;
        let file = c
            .audio
            .as_deref()
            .and_then(|rel| self.state.resolve_audio(rel));
        let peak = match (file, self.player.as_mut()) {
            (Some(file), Some(p)) => p.peaks(&file).map(|pk| {
                crate::timeline::region_peak(
                    &pk,
                    p.peaks_per_beat(tempo),
                    c.offset_beats * tempo / c.source_bpm(tempo),
                    c.source_length_beats() * tempo / c.source_bpm(tempo),
                )
            }),
            // Ohne Wiedergabe-Engine (Tests): Datei direkt lesen, sample-genau.
            (Some(file), None) => woa_engine::clip::load_wav_mono(&file, 48_000)
                .ok()
                .map(|s| {
                    let per_beat = 48_000.0 * 60.0 / c.source_bpm(tempo);
                    let a = ((c.offset_beats * per_beat) as usize).min(s.len());
                    let b = (((c.offset_beats + c.source_length_beats()) * per_beat).ceil()
                        as usize)
                        .clamp(a, s.len());
                    s[a..b].iter().fold(0.0f32, |m, v| m.max(v.abs()))
                }),
            _ => None,
        };
        let done = peak.is_some_and(|pk| {
            self.state
                .normalize_clip(c.id, pk, crate::timeline::NORMALIZE_TARGET_DB)
        });
        if !done {
            self.state.status = Status::Info(
                "Normalisieren nicht möglich (Stille oder keine Wiedergabe-Engine)".into(),
            );
        }
    }

    /// Rastet einen gezogenen Clip-Rand ein: die neue Kante liegt auf dem Raster.
    pub(super) fn snap_trim(&self, c: &woa_model::Clip, left: bool, d: f64) -> f64 {
        let snapped = if left {
            self.snap.apply(c.start_beats + d) - c.start_beats
        } else {
            self.snap.apply(c.end_beats() + d) - c.end_beats()
        };
        if left {
            snapped.clamp(-c.offset_beats * c.stretch, c.length_beats - 0.25)
        } else {
            snapped.max(0.25 - c.length_beats)
        }
    }

    /// Ids der markierten Clips (Mehrfachauswahl oder der eine gewählte).
    pub(super) fn selection_ids(&self) -> Vec<ClipId> {
        let mut ids = self.selected_clips.clone();
        if let Some(c) = self.selected_clip
            && !ids.contains(&c)
        {
            ids.push(c);
        }
        ids
    }

    /// Strg+C: markierte Clips in die Zwischenablage.
    pub(super) fn copy_selected(&mut self) {
        let items = self.state.copy_clips(&self.selection_ids());
        if items.is_empty() {
            return;
        }
        self.state.status = Status::Info(format!("{} Clip(s) kopiert", items.len()));
        self.clipboard = Clipboard { items };
    }

    /// Strg+V: Zwischenablage am Abspielkopf einfügen, auf die gewählte Spur, wenn sie passt.
    pub(super) fn paste_at_playhead(&mut self) {
        if self.clipboard.is_empty() {
            return;
        }
        let tempo = self.state.project().tempo_bpm;
        let at = self
            .player
            .as_ref()
            .map_or(0.0, |p| p.position_beats(tempo));
        let items = self.clipboard.items.clone();
        let ids = self
            .state
            .paste_clips(&items, self.snap.apply(at), self.selected_track);
        self.select_new(ids);
    }

    /// Strg+D: markierte Clips direkt hinter sich verdoppeln.
    pub(super) fn duplicate_selected(&mut self) {
        let ids = self.state.duplicate_clips(&self.selection_ids());
        self.select_new(ids);
    }

    pub(super) fn select_new(&mut self, ids: Vec<ClipId>) {
        if let Some(&last) = ids.last() {
            self.selected_clip = Some(last);
            self.track_focus = false;
            self.selected_track = self.state.project().clip(last).map(|(t, _)| t.id);
            self.selected_clips = ids;
        }
    }

    /// Audiodateien, die aufs Fenster gezogen wurden, als Clips ablegen: die erste auf die
    /// Spur unter dem Zeiger bei der Zeigerposition, jede weitere auf eine neue Spur
    /// an derselben Stelle.
    pub(super) fn drop_files(&mut self, ctx: &egui::Context) {
        let files: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .filter_map(|f| f.path.clone())
                .collect()
        });
        if files.is_empty() {
            return;
        }
        self.place_audio(&files, ctx.pointer_latest_pos());
    }

    /// Das Bibliotheks-Fenster zeichnen und seine Wünsche ausführen: Vorhören im Kopfhörer
    /// (derselbe PFL-Weg wie im Radio, nie auf Sendung) und Ablegen gezogener Titel.
    pub(super) fn library_window(&mut self, ctx: &egui::Context) {
        let pfl = self.radio.as_ref().is_some_and(RadioDesk::pfl_available);
        let out = self.library.show(ctx, pfl);
        self.apply_pick(out);
    }

    /// Assistent „Jingle bauen“ (Paket J4): führt seine Wünsche mit App-Zustand aus
    /// (neues Projekt mit Speicher-Frage, Loop über das Jingle, Drum-Raster, Export).
    pub(super) fn jingle_assistent_window(&mut self, ctx: &egui::Context) {
        use crate::widgets::wizard::Wunsch;
        if !self.jingle_assistent.open {
            return;
        }
        self.jingle_assistent.el.data_dir.clone_from(&self.data_dir);
        match self.jingle_assistent.show(ctx, &mut self.state) {
            Some(Wunsch::Anlegen { mood, seconds }) => {
                let Some(m) = woa_model::jingle::MOODS.get(mood) else {
                    return;
                };
                if !self.state.project().tracks.is_empty() {
                    if !self.save_before_replace("Jingle bauen") {
                        return;
                    }
                    self.state.new_project();
                }
                if let Some(parts) = self.state.build_jingle(m, seconds) {
                    self.loop_start_bar = 1;
                    self.loop_end_bar = parts.bars + 1;
                    self.loop_enabled = true;
                    self.jingle_assistent.angelegt(parts);
                }
            }
            Some(Wunsch::Raster(clip)) => self.open_piano_roll(clip),
            Some(Wunsch::Export(name)) => {
                self.export_dialog
                    .set_jingle(Some(crate::export_dialog::JingleOptions {
                        name,
                        ..Default::default()
                    }));
                self.export_dialog.open_now();
            }
            None => {}
        }
    }

    /// Browser links (Design System 2): volle Höhe zwischen Transport und Statuszeile,
    /// einklappbar auf einen schmalen Streifen.
    pub(super) fn browser_panel(&mut self, ui: &mut egui::Ui) {
        use crate::browser::{WIDTH, Wish};
        let frame = egui::Frame::new()
            .fill(theme::panel())
            .stroke(egui::Stroke::new(1.0, theme::border()))
            .inner_margin(egui::Margin::symmetric(10, 10));
        if !self.browser.open {
            egui::Panel::left("browser_zu")
                .resizable(false)
                .exact_size(30.0)
                .frame(frame.inner_margin(egui::Margin::symmetric(2, 10)))
                .show(ui, |ui| {
                    if ui
                        .small_button(theme::ic::CARET_DOUBLE_RIGHT)
                        .on_hover_text(
                            "Browser zeigen (Klänge, Beats, Akkorde, FX, Samples, Jingles)",
                        )
                        .clicked()
                    {
                        self.browser.open = true;
                    }
                });
            return;
        }
        let tempo = self.state.project().tempo_bpm;
        let at = self
            .player
            .as_ref()
            .map_or(0.0, |p| ((p.position_beats(tempo) / 4.0).floor()) * 4.0);
        let track = self.selected_track;
        let mut wish = None;
        egui::Panel::left("browser")
            .resizable(false)
            .exact_size(WIDTH)
            .frame(frame)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(theme::panel_label("BROWSER"));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .small_button(theme::ic::CARET_DOUBLE_LEFT)
                            .on_hover_text("Browser einklappen")
                            .clicked()
                        {
                            self.browser.open = false;
                        }
                    });
                });
                wish = self.browser.ui(
                    ui,
                    &mut self.state,
                    &mut self.akkorde,
                    &mut self.jingle_fx,
                    track,
                    at,
                );
            });
        match wish {
            Some(Wish::ImportAudio) => self.import_audio(),
            Some(Wish::Produktionen) => self.produktionen.open_window(),
            Some(Wish::MidiClip) => self.create_midi_clip_at_playhead(self.selected_track),
            Some(Wish::OpenChat) => self.studio_dock = StudioDockTab::Chat,
            Some(Wish::JingleWizard) => self.jingle_assistent.open = true,
            None => {}
        }
    }

    /// Fenster „Produktionen“: gleicher Weg wie die Bibliothek (Vorhören, Ablegen).
    pub(super) fn produktionen_window(&mut self, ctx: &egui::Context) {
        let pfl = self.radio.as_ref().is_some_and(RadioDesk::pfl_available);
        let root = self.state.project_dir();
        let out = self.produktionen.show(ctx, &root, pfl);
        self.apply_pick(out);
    }

    /// Wünsche aus Bibliothek oder Produktionen: vorhören, stoppen, gezogene Datei ablegen.
    pub(super) fn apply_pick(&mut self, out: crate::library::Output) {
        if let Some(path) = out.preview {
            match self.radio.as_ref() {
                Some(r) => r.preview_file(path),
                None => {
                    self.state.status = Status::Error(
                        "Vorhören braucht den Radio-Modus mit Kopfhörer-Ausgang (Einstellungen › Audio-Geräte › Vorhören)".into(),
                    );
                }
            }
        }
        if out.stop_preview
            && let Some(r) = self.radio.as_ref()
        {
            r.preview_stop();
        }
        if let Some((path, pos)) = out.drop {
            if self.config.mode.uses_desk() {
                let done = self
                    .radio
                    .as_mut()
                    .is_some_and(|r| r.drop_library_path(path, pos));
                if !done {
                    self.state.status = Status::Info(
                        "Zum Ablegen auf die Playlist oder ein Cart-Pad ziehen".into(),
                    );
                }
            } else {
                self.place_audio(&[path], Some(pos));
            }
        }
    }

    /// Legt Audiodateien als Clips ab (Ziehen aufs Fenster oder aus der Bibliothek).
    pub(super) fn place_audio(&mut self, files: &[PathBuf], pos: Option<egui::Pos2>) {
        let hit = pos.and_then(|p| self.track_rects.iter().find(|(_, r)| r.contains(p)));
        let (track, at) = match (pos, hit) {
            (Some(p), Some((t, r))) => (
                Some(*t),
                self.snap.apply(f64::from((p.x - r.left()) / self.zoom)),
            ),
            _ => {
                let tempo = self.state.project().tempo_bpm;
                (
                    self.selected_track,
                    self.player
                        .as_ref()
                        .map_or(0.0, |p| p.position_beats(tempo)),
                )
            }
        };
        for (n, f) in files.iter().enumerate() {
            self.import_audio_file(f, if n == 0 { track } else { None }, at);
        }
    }

    pub(super) fn delete_selected(&mut self) {
        self.track_focus = false;
        let mut ids = std::mem::take(&mut self.selected_clips);
        if let Some(c) = self.selected_clip.take()
            && !ids.contains(&c)
        {
            ids.push(c);
        }
        match ids.as_slice() {
            [] => {}
            [one] => {
                self.state.remove_clip(*one);
            }
            many => {
                self.state.remove_clips(many);
            }
        }
    }

    pub(super) fn split_at_playhead(&mut self) {
        let Some(id) = self.selected_clip else { return };
        let tempo = self.state.project().tempo_bpm;
        let at = self
            .player
            .as_ref()
            .map_or(0.0, |p| p.position_beats(tempo));
        match self.state.split_clip(id, at) {
            Some(right) => self.selected_clip = Some(right),
            None => {
                self.state.status =
                    Status::Info("Zum Schneiden den roten Kopf in den Clip setzen".into());
            }
        }
    }

    /// Öffnet das Export-Fenster (Format, Stems); gerechnet wird nach dem Klick darin.
    pub(super) fn export(&mut self) {
        self.export_dialog.open_now();
    }

    pub(super) fn export_window(&mut self, ctx: &egui::Context) {
        if let Some(req) = self.export_dialog.show(ctx) {
            self.run_export(req);
        }
        self.track_export_window(ctx);
    }

    /// Fenster und Hintergrund-Export „Spur exportieren“ (Ticket 229).
    pub(super) fn track_export_window(&mut self, ctx: &egui::Context) {
        if let Some(job) = &self.track_export_job {
            match job.rx.try_recv() {
                Ok(result) => {
                    self.state.status = match result {
                        Ok(secs) => {
                            tracing::info!(seconds = secs, "track export done");
                            Status::Info(format!(
                                "Spur exportiert: {} ({secs:.1} s)",
                                job.out.display()
                            ))
                        }
                        Err(e) => {
                            tracing::error!(error = %e, "track export failed");
                            Status::Error(format!("Export fehlgeschlagen: {e}"))
                        }
                    };
                    self.track_export_job = None;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    ctx.request_repaint_after(std::time::Duration::from_millis(200));
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    tracing::error!("track export thread ended without result");
                    self.state.status = Status::Error("Export fehlgeschlagen".into());
                    self.track_export_job = None;
                }
            }
        }
        let Some(dialog) = self.track_export.as_mut() else {
            return;
        };
        let req = match dialog.show(ctx) {
            Err(()) => {
                self.track_export = None;
                return;
            }
            Ok(None) => return,
            Ok(Some(req)) => req,
        };
        if self.track_export_job.is_some() {
            self.state.status = Status::Error("Es läuft schon ein Spur-Export".into());
            return;
        }
        let ext = req.format.extension();
        let Some(out) = crate::ordner::dialog(crate::ordner::Art::Exporte)
            .set_title("Spur exportieren")
            .set_file_name(&req.file_name)
            .add_filter(req.format.label(), &[ext])
            .save_file_modal()
        else {
            return;
        };
        self.track_export = None;
        let out = if out.extension().is_none() {
            out.with_extension(ext)
        } else {
            out
        };
        self.capture_plugins();
        let project = self.state.project().clone();
        let state = &self.state;
        let job = match self.player.as_mut() {
            Some(p) => p.track_export_job(
                &project,
                &|rel| state.resolve_audio(rel),
                req.track,
                out.clone(),
                woa_engine::export::ExportSpec::new(req.format),
                req.tags,
            ),
            None => Err("keine Audio-Engine".into()),
        };
        let work = match job {
            Ok(w) => w,
            Err(e) => {
                self.state.status = Status::Error(format!("Export fehlgeschlagen: {e}"));
                return;
            }
        };
        let (tx, rx) = std::sync::mpsc::channel();
        let ctx2 = ctx.clone();
        let spawned = std::thread::Builder::new()
            .name("track-export".into())
            .spawn(move || {
                let _ = tx.send(work());
                ctx2.request_repaint();
            });
        match spawned {
            Ok(_) => {
                self.state.status =
                    Status::Info(format!("Spur wird exportiert: {}", out.display()));
                self.track_export_job = Some(crate::track_export::TrackExportJob { out, rx });
            }
            Err(e) => {
                tracing::error!(error = %e, "track export thread not started");
                self.state.status = Status::Error(format!("Export fehlgeschlagen: {e}"));
            }
        }
    }

    pub(super) fn run_export(&mut self, req: crate::export_dialog::ExportRequest) {
        let ext = req.spec.format.extension();
        self.capture_plugins();
        let project = self.state.project().clone();
        let name = project.meta.name.clone();
        if let Some(j) = req.jingle {
            self.state.status = self.export_jingle(&project, j);
            return;
        }
        let status = match req.stems {
            None => self.export_file(&project, &name, ext, &req.spec),
            Some(opts) => self.export_stem_folder(&project, &name, &req.spec, opts),
        };
        if let Some(s) = status {
            self.state.status = s;
        }
    }

    /// „Als Jingle“ (J6): rendern wie der Export, dann derselbe Weg wie das MCP-Werkzeug
    /// `export_jingle` (Lautheit, Limiter, Jingle-Ordner, Bibliothek, Cart).
    pub(super) fn export_jingle(
        &mut self,
        project: &woa_model::Project,
        j: crate::export_dialog::JingleOptions,
    ) -> Status {
        let state = &self.state;
        let Some(player) = self.player.as_mut() else {
            return Status::Error("Jingle fehlgeschlagen: keine Audio-Engine".into());
        };
        let rate = player.sample_rate();
        let to = woa_agent::jingle::Target {
            dir: crate::ordner::dir(crate::ordner::Art::Jingles),
            name: j.name,
            target_lufs: j.target_lufs,
            cart: j.cart,
            db: self.data_dir.join(crate::library::DB_FILE),
            radio: self.data_dir.join(woa_radio::RADIO_FILE),
        };
        let result = player
            .render_mix(project, &|rel| state.resolve_audio(rel))
            .and_then(|(mut l, mut r, _)| woa_agent::jingle::publish(&mut l, &mut r, rate, &to));
        match result {
            Ok(p) => {
                tracing::info!(cart = ?p.cart, "jingle exported");
                self.library.refresh();
                let cart = p.cart.map_or(String::new(), |c| format!(", auf Cart {c}"));
                Status::Info(format!(
                    "Jingle: {} ({:.1} LUFS, Spitze {:.1} dBFS, in der Bibliothek{cart})",
                    p.path.display(),
                    p.lufs.unwrap_or(f64::NAN),
                    p.peak_db.unwrap_or(f64::NAN),
                ))
            }
            Err(e) => {
                tracing::error!(error = %e, "jingle export failed");
                Status::Error(format!("Jingle fehlgeschlagen: {e}"))
            }
        }
    }

    /// Text zur Lautheit für die Statuszeile.
    pub(super) fn loudness_note(norm: Option<woa_engine::loudness::Normalized>) -> String {
        match norm {
            Some(n) => format!(
                ", Lautheit {:.1} → {:.1} LUFS{}",
                n.before_lufs,
                n.before_lufs + n.gain_db,
                if n.peak_limited {
                    " (Spitzen begrenzen, leiser als Ziel)"
                } else {
                    ""
                }
            ),
            None => String::new(),
        }
    }

    pub(super) fn export_file(
        &mut self,
        project: &woa_model::Project,
        name: &str,
        ext: &str,
        spec: &woa_engine::export::ExportSpec,
    ) -> Option<Status> {
        let out = crate::ordner::dialog(crate::ordner::Art::Exporte)
            .set_title("Exportieren")
            .set_file_name(format!("{name}.{ext}"))
            .add_filter(spec.format.label(), &[ext])
            .save_file_modal()?;
        let out = if out.extension().is_none() {
            out.with_extension(ext)
        } else {
            out
        };
        let state = &self.state;
        let result = match self.player.as_mut() {
            Some(p) => p.export(
                project,
                &|rel| state.resolve_audio(rel),
                &out,
                spec,
                self.export_lufs,
            ),
            None => Err("keine Audio-Engine".into()),
        };
        Some(match result {
            Ok((secs, norm)) => {
                tracing::info!(seconds = secs, ?norm, "export done");
                Status::Info(format!(
                    "Exportiert: {} ({secs:.1} s{})",
                    out.display(),
                    Self::loudness_note(norm)
                ))
            }
            Err(e) => {
                tracing::error!(error = %e, "export failed");
                Status::Error(format!("Export fehlgeschlagen: {e}"))
            }
        })
    }

    pub(super) fn export_stem_folder(
        &mut self,
        project: &woa_model::Project,
        name: &str,
        spec: &woa_engine::export::ExportSpec,
        opts: crate::export_dialog::StemOptions,
    ) -> Option<Status> {
        let parent = crate::ordner::dialog(crate::ordner::Art::Exporte)
            .set_title("Ordner für die Stems wählen")
            .pick_folder_modal()?;
        // Eigener Unterordner „<Projekt> Stems“, damit nichts Fremdes überschrieben wird.
        let dir = parent.join(format!("{} Stems", woa_engine::stems::sanitize_name(name)));
        if let Err(e) = std::fs::create_dir_all(&dir) {
            tracing::error!(error = %e, "stem folder not created");
            return Some(Status::Error(format!(
                "Ordner {} nicht anlegbar: {e}",
                dir.display()
            )));
        }
        let state = &self.state;
        let result = match self.player.as_mut() {
            Some(p) => p.export_stems(
                project,
                &|rel| state.resolve_audio(rel),
                &dir,
                spec,
                opts,
                self.export_lufs,
            ),
            None => Err("keine Audio-Engine".into()),
        };
        Some(match result {
            Ok(done) => {
                tracing::info!(
                    files = done.files,
                    seconds = done.seconds,
                    "stem export done"
                );
                let skipped = if done.skipped.is_empty() {
                    String::new()
                } else {
                    format!(", ohne stumme Spuren: {}", done.skipped.join(", "))
                };
                Status::Info(format!(
                    "{} Dateien in {} ({:.1} s{}{skipped})",
                    done.files,
                    dir.display(),
                    done.seconds,
                    Self::loudness_note(done.norm)
                ))
            }
            Err(e) => {
                tracing::error!(error = %e, "stem export failed");
                // Ein leerer Ordner, den dieser Lauf angelegt hat, bleibt nicht liegen.
                let _ = std::fs::remove_dir(&dir);
                Status::Error(format!("Export fehlgeschlagen: {e}"))
            }
        })
    }

    pub(super) fn track_name(
        &mut self,
        ui: &mut egui::Ui,
        id: TrackId,
        name: &str,
    ) -> Option<egui::Response> {
        match &mut self.rename {
            Some((rid, buf, focused)) if *rid == id => {
                let r = ui.add(egui::TextEdit::singleline(buf).desired_width(130.0));
                if !*focused {
                    r.request_focus();
                    *focused = true;
                }
                if r.lost_focus() || r.clicked_elsewhere() {
                    let new = buf.clone();
                    self.rename = None;
                    if !ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                        self.state.rename_track(id, &new);
                    }
                }
                None
            }
            _ => {
                let r = ui
                    .add_sized(
                        [130.0, 22.0],
                        egui::Button::new(egui::RichText::new(name).strong())
                            .frame(false)
                            .truncate(),
                    )
                    .on_hover_text("Doppelklick zum Umbenennen");
                if r.double_clicked() {
                    self.rename = Some((id, name.to_string(), false));
                }
                Some(r)
            }
        }
    }

    /// Merkt die gezogene Größe des Fensters Einstellungen in `config.json` (#379), erst wenn
    /// die Maus losgelassen ist (nicht bei jedem Bild des Ziehens).
    pub(super) fn remember_settings_size(&mut self, ctx: &egui::Context) {
        if self.settings.size != self.config.settings_size && !ctx.input(|i| i.pointer.any_down()) {
            self.config.settings_size = self.settings.size;
            self.save_config();
        }
    }

    /// Sets where setting changes are saved (`config.json`).
    pub fn set_config_path(&mut self, path: PathBuf) {
        self.config_path = Some(path);
    }

    /// Bereich im Fenster Einstellungen, der zur aktuellen Ansicht gehört (Zahnrad, Strg+,).
    pub(super) fn mode_area(&self) -> crate::settings::Area {
        use crate::settings::Area;
        match self.config.mode {
            Mode::Produzent => Area::Produzent,
            Mode::Radio => Area::Radio,
            Mode::Party => Area::Party,
        }
    }

    /// Bedienelement einer Zeile im Fenster Einstellungen bzw. Status und Aktion einer
    /// Dienst-Karte: dieselbe Wirkung wie bisher im Menü und in den Dialogen.
    pub(super) fn setting_row(&mut self, ui: &mut egui::Ui, id: crate::settings::SettingId) {
        use crate::ki_ui::Tab as KiTab;
        use crate::settings::SettingId as S;
        use crate::widgets::{ChipTone, StateChip};
        let open = |ui: &mut egui::Ui| ui.button("Öffnen").clicked();
        let setup = |ui: &mut egui::Ui| ui.button("Einrichten").clicked();
        // Einstellungen, die in einer Ansicht wohnen: dorthin wechseln.
        let show_in = |ui: &mut egui::Ui, mode: Mode| {
            let name = match mode {
                Mode::Produzent => "Im Produzenten zeigen",
                Mode::Radio => "Im Radio zeigen",
                Mode::Party => "In Party zeigen",
            };
            ui.button(name).clicked().then_some(mode)
        };
        let mut go = None;
        match id {
            // Menülisten als Aufklapper: sonst sprengen sie die Zeile (Foto 0.41.0).
            // Verweise auf den einen Ort für Soundkarten, keine zweiten Wähler.
            S::Pfl | S::Mic | S::Phone => {
                if ui
                    .button(format!("In {} öffnen", crate::outputs_ui::WINDOW))
                    .clicked()
                {
                    self.open_audio();
                }
            }
            S::MidiInput => {
                ui.menu_button(format!("Wählen {}", theme::ic::CARET_DOWN), |ui| {
                    self.midi_menu(ui)
                });
            }
            S::Theme => {
                for t in [theme::Theme::Hell, theme::Theme::Dunkel] {
                    if ui
                        .selectable_value(&mut self.config.theme, t, t.label())
                        .changed()
                    {
                        self.save_config();
                    }
                }
            }
            S::VuView => {
                ui.menu_button(
                    format!("{} {}", self.config.vu_view.label(), theme::ic::CARET_DOWN),
                    |ui| {
                        for v in crate::widgets::VuView::ALL {
                            if ui
                                .radio_value(&mut self.config.vu_view, v, v.label())
                                .changed()
                            {
                                self.save_config();
                            }
                        }
                    },
                );
            }
            S::AutoUpdate => {
                let mut on = self.config.auto_update;
                if ui.checkbox(&mut on, "").changed() {
                    self.config.auto_update = on;
                    self.save_config();
                }
            }
            S::CheckUpdates => {
                if ui
                    .add_enabled(!self.update.busy(), egui::Button::new("Jetzt suchen"))
                    .clicked()
                {
                    self.check_updates_now(ui.ctx());
                }
            }
            S::Telemetry => {
                let mut on = self.config.telemetry_enabled == Some(true);
                if ui
                    .checkbox(&mut on, "")
                    .on_hover_text(TELEMETRY_HINT)
                    .changed()
                {
                    self.set_telemetry(on);
                }
            }
            S::Developer => {
                if self.config.entwickler {
                    StateChip::new("an", ChipTone::Ok).show(ui);
                } else if ui.button("Freischalten").clicked() {
                    self.entwickler_tor.open = true;
                }
            }
            S::ReconnectOutput => {
                if ui.button("Neu verbinden").clicked() {
                    if let Some(p) = self.player.as_mut() {
                        p.reconnect_output();
                    }
                    if let Some(r) = self.radio.as_mut() {
                        r.reconnect_outputs();
                    }
                }
            }
            S::Pult => {
                if ui
                    .button(format!("In {} öffnen", crate::outputs_ui::WINDOW))
                    .clicked()
                {
                    self.open_audio();
                }
                let (t, tone) = if self.config.pult.on {
                    ("an", ChipTone::Ok)
                } else {
                    ("aus", ChipTone::Neutral)
                };
                StateChip::new(t, tone).show(ui);
            }
            S::Crossfade
            | S::Ducking
            | S::CartDuck
            | S::StopFade
            | S::FadeLength
            | S::DuckTimes
            | S::BusProcessing
            | S::MicChain
            | S::WishFolder => go = show_in(ui, Mode::Radio),
            S::DjCurve => go = show_in(ui, Mode::Party),
            S::PartyPanels => {
                let mut p = self.config.party_panels;
                let a = ui.checkbox(&mut p.wishes, "Wunschliste").changed();
                let b = ui.checkbox(&mut p.remote, "Fernbedienung").changed();
                if a || b {
                    self.config.party_panels = p;
                    self.party.panels = p;
                    self.save_config();
                }
            }
            S::MonitorVolume => {
                let mut v = self.config.monitor_volume * 100.0;
                if ui
                    .add(egui::Slider::new(&mut v, 0.0..=100.0).suffix(" %"))
                    .changed()
                {
                    self.config.monitor_volume = v / 100.0;
                    if let Some(p) = &self.player {
                        p.set_monitor_volume(v / 100.0);
                    }
                    self.save_config();
                }
            }
            S::SoundBanks => go = show_in(ui, Mode::Produzent),
            S::PianoRollWindow => {
                let mut on = self.config.piano_roll_window;
                if ui.checkbox(&mut on, "").changed() {
                    self.config.piano_roll_window = on;
                    self.piano_roll_floating = on;
                    self.save_config();
                }
            }
            S::Plugins => {
                if ui
                    .add_enabled(self.player.is_some(), egui::Button::new("Neu suchen"))
                    .clicked()
                    && let Some(p) = self.player.as_mut()
                {
                    p.rescan_plugins();
                }
            }
            S::Provider(i) => {
                if setup(ui) {
                    // Den angeklickten Anbieter gleich vorwählen (#376).
                    self.ki_ui.open_for(
                        &self.config.ki,
                        &self.data_dir,
                        woa_assistant::Pick::Preset(usize::from(i)),
                    );
                }
                let (t, tone) = provider_state(&self.config.ki, usize::from(i));
                StateChip::new(t, tone).show(ui);
            }
            S::Tools => {
                if open(ui) {
                    self.ki_ui
                        .open_at(&self.config.ki, &self.data_dir, KiTab::Assistant);
                }
            }
            S::ElevenLabs | S::Freesound => {
                if setup(ui) {
                    if id == S::ElevenLabs {
                        self.ki_ui.open_elevenlabs(&self.config.ki, &self.data_dir);
                    } else {
                        self.ki_ui
                            .open_at(&self.config.ki, &self.data_dir, KiTab::Setup);
                    }
                }
                let account = if id == S::ElevenLabs {
                    woa_agent::elevenlabs::KEY_ACCOUNT
                } else {
                    woa_agent::freesound::KEY_ACCOUNT
                };
                // Grün = aktiv, Orange = Fehler bei der letzten Prüfung (#377).
                use crate::elevenlabs_ui::KeyState;
                let checked = (id == S::ElevenLabs).then(|| self.ki_ui.elevenlabs_state());
                let (t, tone) = match (self.service_keys.get(&self.data_dir, account), checked) {
                    (_, Some(KeyState::Invalid)) => ("Fehler", ChipTone::Warn),
                    (None, _) => {
                        ui.ctx()
                            .request_repaint_after(std::time::Duration::from_millis(150));
                        ("prüfe", ChipTone::Busy)
                    }
                    (Some(Some(_)), Some(KeyState::Connected)) => ("verbunden", ChipTone::Ok),
                    (Some(Some(_)), _) => ("Schlüssel gespeichert", ChipTone::Ok),
                    (Some(None), _) => ("nicht eingerichtet", ChipTone::Neutral),
                };
                StateChip::new(t, tone).show(ui);
            }
            S::McpLive => {
                if setup(ui) {
                    // Claude Desktop/Code vorwählen (#376), den schon gewählten Client behalten.
                    let client = match self.config.ki.pick() {
                        Some(woa_assistant::Pick::Mcp(c)) => c,
                        _ => woa_assistant::McpClient::ALL[0],
                    };
                    self.ki_ui.open_for(
                        &self.config.ki,
                        &self.data_dir,
                        woa_assistant::Pick::Mcp(client),
                    );
                }
                use crate::mcp_live::LiveStatus;
                let (t, tone) = match self.mcp_live.status() {
                    LiveStatus::Connected => ("verbunden", ChipTone::Ok),
                    LiveStatus::Ready => ("bereit", ChipTone::Ok),
                    LiveStatus::Off => ("nicht verbunden", ChipTone::Neutral),
                };
                StateChip::new(t, tone).show(ui);
            }
            S::McpHttp => {
                if open(ui) {
                    self.ki_ui
                        .open_at(&self.config.ki, &self.data_dir, KiTab::Server);
                }
                let (t, tone) = if self.ki_ui.http_error().is_some() {
                    ("Fehler", ChipTone::Warn)
                } else if self.ki_ui.http_running() {
                    ("läuft", ChipTone::Ok)
                } else {
                    ("aus", ChipTone::Neutral)
                };
                StateChip::new(t, tone).show(ui);
            }
            S::StreamRef => {
                if ui
                    .button(format!("Zu Radio › Sendung {}", theme::ic::CARET_RIGHT))
                    .clicked()
                {
                    self.settings_goto = Some(S::Stream);
                }
            }
            S::Abnahme if open(ui) => self.abnahme.open_window(ui.ctx()),
            S::Fragen if open(ui) => self.fragen.open_window(ui.ctx()),
            S::DevEnd => {
                if ui.button("Beenden").clicked() {
                    self.entwickler_beenden();
                }
            }
            S::Outputs if open(ui) => self.open_audio(),
            S::Stream if open(ui) => self.stream_ui.open(&self.config.stream, &self.data_dir),
            S::Folders if open(ui) => self.ordner_ui.open = true,
            S::Controls if open(ui) => self.controls.window = true,
            S::Assistant if open(ui) => {
                let project = self
                    .state
                    .session()
                    .path()
                    .map(std::path::Path::to_path_buf);
                if self.config.mode == Mode::Produzent {
                    self.studio_dock = StudioDockTab::Chat;
                } else {
                    self.chat_ui
                        .open(&self.config.ki, &self.data_dir, project.as_deref());
                }
            }
            _ => {}
        }
        if let Some(mode) = go {
            self.set_mode(mode);
        }
    }

    pub(super) fn set_telemetry(&mut self, on: bool) {
        self.config.telemetry_enabled = Some(on);
        if let Some(p) = &self.config_path
            && let Err(e) = self.config.save(p)
        {
            tracing::warn!(error = %e, "config not saved");
        }
    }

    /// „Über WOA“: Marke, Widmung, Spenden (PayPal, Ko-fi), Version, Lizenz und der Dank an
    /// alle Lizenzgeber samt eingebetteten Schriften (SIL OFL) und Symbolen (MIT).
    pub(super) fn about_window(&mut self, ctx: &egui::Context) {
        if !self.about_open {
            return;
        }
        let mut open = true;
        let shown = egui::Window::new("Über WOA")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .fixed_size(egui::vec2(440.0, 0.0))
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(6.0);
                    theme::logo_mark(ui, 72.0);
                    ui.add_space(6.0);
                    ui.label(
                        egui::RichText::new("WIR ON AIR")
                            .font(theme::label_font(theme::TYPE_TITLE)),
                    );
                    ui.label(
                        egui::RichText::new(ABOUT_TAGLINE)
                            .size(15.0)
                            .italics()
                            .color(theme::text()),
                    );
                    ui.label(
                        egui::RichText::new(format!(
                            "Version {} · freie Software unter der GPL-3.0-only",
                            env!("CARGO_PKG_VERSION")
                        ))
                        .size(theme::TYPE_SMALL)
                        .color(theme::text_dim()),
                    );
                    ui.add_space(12.0);
                    ui.label("Gefällt dir WOA? Über einen Kaffee freue ich mich sehr.");
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        // Zentriert: Breite der zwei Knöpfe grob vorab abziehen.
                        ui.add_space(((ui.available_width() - 330.0) / 2.0).max(0.0));
                        let paypal = egui::Button::new(
                            egui::RichText::new(format!(
                                "{} Spendier mir einen Kaffee · PayPal",
                                theme::ic::COFFEE
                            ))
                            .strong()
                            .color(theme::on_accent()),
                        )
                        .fill(theme::accent())
                        .min_size(egui::vec2(0.0, 30.0));
                        if ui.add(paypal).on_hover_text(DONATE_PAYPAL).clicked() {
                            ui.ctx().open_url(egui::OpenUrl::new_tab(DONATE_PAYPAL));
                        }
                        let kofi = egui::Button::new(
                            egui::RichText::new(format!("{} Ko-fi", theme::ic::COFFEE)).strong(),
                        )
                        .min_size(egui::vec2(0.0, 30.0));
                        if ui.add(kofi).on_hover_text(DONATE_KOFI).clicked() {
                            ui.ctx().open_url(egui::OpenUrl::new_tab(DONATE_KOFI));
                        }
                    });
                    // #231: zum Scannen mit dem Handy, kodiert den PayPal-Link.
                    if let Some(m) = donate_qr() {
                        ui.add_space(8.0);
                        crate::remote::draw_qr(ui, m, 140.0)
                            .on_hover_text(DONATE_PAYPAL)
                            .widget_info(|| {
                                egui::WidgetInfo::labeled(
                                    egui::WidgetType::Image,
                                    true,
                                    "Spenden-QR-Code",
                                )
                            });
                    }
                });
                ui.add_space(12.0);
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Danke an alle Lizenzgeber").strong());
                    // Das Emoji ❤ fehlt in den Schriften: Phosphor-Herz in Rot.
                    ui.label(theme::fill_icon(theme::icf::HEART, 16.0).color(theme::on_air_fill()));
                });
                ui.label(
                    "Barlow und Barlow Condensed (Jeremy Tribby), JetBrains Mono (JetBrains) \
                     und DSEG7 Classic (keshikan) sind eingebettet. Alle vier stehen unter \
                     der SIL Open Font License 1.1 (OFL).",
                );
                ui.label(
                    "Die Symbole sind Phosphor Icons (MIT-Lizenz), eingebunden über egui-phosphor.",
                );
                ui.label(
                    "Klänge: GeneralUser GS (S. Christian Collins). Auf Wunsch geladen: \
                     MuseScore General (S. Christian Collins, auf Basis von FluidR3 von Frank \
                     Wen, MIT-Lizenz) und AVL Drumkits (Glen MacArthur, CC BY-SA 3.0).",
                );
                ui.label(
                    "Dazu kommen die vielen freien Rust-Crates, auf denen WOA aufbaut, \
                     unter anderem egui, cpal und symphonia.",
                );
                ui.label(
                    egui::RichText::new(
                        "Die Lizenztexte liegen im Quelltext unter crates/gui/assets/fonts/.",
                    )
                    .size(theme::TYPE_SMALL)
                    .color(theme::text_dim()),
                );
            });
        // #441: Esc schließt über die gemeinsame Esc-Regel.
        if let Some(r) = &shown
            && crate::esc::fenster(ctx, r.response.layer_id, "about")
        {
            open = false;
        }
        self.about_open = open;
    }

    /// First start: ask once whether anonymous error codes may be sent (opt-in).
    pub(super) fn telemetry_window(&mut self, ctx: &egui::Context) {
        if self.config.telemetry_enabled.is_some() || self.photo.is_some() {
            return;
        }
        let mut choice = None;
        egui::Window::new("Fehlercodes senden?")
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label(TELEMETRY_HINT);
                ui.horizontal(|ui| {
                    if ui.button("Ja, anonym senden").clicked() {
                        choice = Some(true);
                    }
                    if ui.button("Nein").clicked() {
                        choice = Some(false);
                    }
                });
            });
        if let Some(on) = choice {
            self.set_telemetry(on);
        }
    }

    /// Kleine Vorschau von Bildschirmfoto und eigenem Bild, je mit „× Entfernen“ (#211).
    pub(super) fn feedback_previews(&mut self, ui: &mut egui::Ui) {
        let previews: Vec<(&'static str, Vec<u8>)> = self
            .feedback
            .previews()
            .into_iter()
            .map(|(k, p)| (k, p.to_vec()))
            .collect();
        let keys: Vec<_> = previews.iter().map(|(k, p)| (*k, p.len())).collect();
        self.feedback_thumbs.retain(|(k, _)| keys.contains(k));
        if previews.is_empty() {
            return;
        }
        ui.horizontal_wrapped(|ui| {
            for (kind, png) in previews {
                let key = (kind, png.len());
                if !self.feedback_thumbs.iter().any(|(k, _)| *k == key)
                    && let Some(img) = crate::feedback::thumbnail(&png)
                {
                    let tex = ui.ctx().load_texture(
                        format!("feedback-{kind}"),
                        img,
                        egui::TextureOptions::LINEAR,
                    );
                    self.feedback_thumbs.push((key, tex));
                }
                let Some((_, tex)) = self.feedback_thumbs.iter().find(|(k, _)| *k == key) else {
                    continue;
                };
                let tex = tex.clone();
                ui.vertical(|ui| {
                    ui.add(egui::Image::new(&tex).fit_to_original_size(1.0));
                    if ui.small_button("× Entfernen").clicked() {
                        if kind == "screenshot" {
                            self.feedback.remove_screenshot();
                        } else {
                            self.feedback.remove_image();
                        }
                    }
                });
            }
        });
    }

    pub(super) fn feedback_window(&mut self, ctx: &egui::Context) {
        if matches!(self.feedback.phase, Phase::Closed | Phase::Capturing) {
            return;
        }
        let now = Instant::now();
        self.feedback.tick(now);
        if let Some(left) = self.feedback.thanks_left(now) {
            ctx.request_repaint_after(left.min(std::time::Duration::from_millis(100)));
        }
        let mut close = false;
        // Dim the app behind the dialog so it reads as modal.
        egui::Area::new(egui::Id::new("feedback-scrim"))
            .order(egui::Order::Middle)
            .fixed_pos(egui::Pos2::ZERO)
            .interactable(true)
            .show(ctx, |ui| {
                let screen = ctx.content_rect();
                ui.painter()
                    .rect_filled(screen, 0.0, egui::Color32::from_black_alpha(140));
                ui.allocate_rect(screen, egui::Sense::click());
            });
        egui::Window::new("Feedback")
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .fixed_size(egui::vec2(FEEDBACK_W, 0.0))
            .frame(
                theme::card_frame()
                    .inner_margin(egui::Margin::same(28))
                    .shadow(egui::epaint::Shadow {
                        offset: [0, 8],
                        blur: 28,
                        spread: 0,
                        color: egui::Color32::from_black_alpha(160),
                    }),
            )
            .show(ctx, |ui| {
                // #441: Esc schließt den Feedback-Dialog (modal, geht Fenstern vor).
                if crate::esc::frage(ui.ctx(), ui.layer_id(), "feedback") {
                    close = true;
                }
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(theme::ic::CHAT_CIRCLE_TEXT)
                            .size(34.0)
                            .color(theme::ok()),
                    );
                    ui.add_space(6.0);
                    ui.vertical(|ui| {
                        ui.label(
                            egui::RichText::new("Schön, dass du dich meldest!")
                                .size(22.0)
                                .strong()
                                .color(theme::text()),
                        );
                        ui.label(
                            egui::RichText::new(
                                "Jede Rückmeldung landet direkt bei uns und macht WOA besser.",
                            )
                            .color(theme::text_dim()),
                        );
                    });
                    // Schließen-X oben rechts (André 02.11.2026).
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                        let r = ui
                            .add(egui::Button::new(theme::ic::X).frame(false))
                            .on_hover_text("Schließen");
                        r.widget_info(|| {
                            egui::WidgetInfo::labeled(
                                egui::WidgetType::Button,
                                true,
                                "Feedback schließen",
                            )
                        });
                        if r.clicked() {
                            close = true;
                        }
                    });
                });
                ui.add_space(14.0);
                match self.feedback.phase.clone() {
                    Phase::Editing => {
                        // Strg+Enter sendet mit den gesetzten Haken, wie „Abschicken“ (#211).
                        // Vor dem Textfeld abgefangen, damit es keine neue Zeile wird.
                        if ui.ctx().input_mut(crate::feedback::send_shortcut) {
                            self.feedback.send(&self.config, &feedback_log_lines());
                        }
                        ui.label(
                            egui::RichText::new("Was ist passiert, oder was wünschst du dir?")
                                .strong()
                                .color(theme::text()),
                        );
                        ui.label(
                            egui::RichText::new(
                                "Ein Fehler, eine Idee, ein Lob: ein, zwei Sätze reichen. \
                                 Wenn du magst, schreib dazu, was du gerade gemacht hast.",
                            )
                            .size(theme::TYPE_SMALL)
                            .color(theme::text_dim()),
                        );
                        ui.add_space(8.0);
                        ui.add(
                            egui::TextEdit::multiline(&mut self.feedback.text)
                                .desired_rows(9)
                                .desired_width(f32::INFINITY)
                                .hint_text("Schreib einfach los …"),
                        );
                        ui.add_space(8.0);
                        // Bildschirmfoto erst auf Klick: Haken setzen blendet das Fenster
                        // aus und nimmt auf; Haken weg wirft das Bild weg.
                        let mut shot = self.feedback.attach;
                        if ui
                            .checkbox(
                                &mut shot,
                                "Bildschirmfoto mitsenden (zeigt uns, was du siehst)",
                            )
                            .on_hover_text("Das Fenster verschwindet kurz, dann wird aufgenommen.")
                            .changed()
                        {
                            if shot {
                                self.feedback.request_screenshot();
                            } else {
                                self.feedback.remove_screenshot();
                            }
                        }
                        ui.horizontal(|ui| {
                            match self.feedback.image.as_ref().map(|(n, _)| n.clone()) {
                                None => {
                                    if ui
                                        .button(format!("{} Eigenes Bild anhängen …", theme::ic::FOLDER_OPEN))
                                        .on_hover_text("PNG oder JPEG, z. B. ein Bildschirmfoto, das du selbst gemacht hast")
                                        .clicked()
                                        && let Some(p) = rfd::FileDialog::new()
                                            .add_filter("Bild", &["png", "jpg", "jpeg"])
                                            .pick_file_modal()
                                    {
                                        let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                                        let res = std::fs::read(&p)
                                            .map_err(|e| e.to_string())
                                            .and_then(|b| self.feedback.set_image(&name, &b));
                                        if let Err(e) = res {
                                            self.image_error = Some(e);
                                        } else {
                                            self.image_error = None;
                                        }
                                    }
                                }
                                Some(name) => {
                                    ui.label(format!("{} {name}", theme::ic::PAPERCLIP));
                                }
                            }
                        });
                        self.feedback_previews(ui);
                        if let Some(e) = &self.image_error {
                            ui.label(egui::RichText::new(e).color(theme::danger_text()));
                        }
                        ui.checkbox(
                            &mut self.feedback.send_log,
                            "Protokoll mitsenden (Meldungen dieser Sitzung, wie unter Hilfe → Protokoll)",
                        );
                        let log_lines = feedback_log_lines();
                        if self.feedback.send_log {
                            egui::CollapsingHeader::new(format!(
                                "Protokoll ansehen ({} Zeilen)",
                                log_lines.len()
                            ))
                            .id_salt("feedback-log")
                            .show(ui, |ui| {
                                let mut shown = crate::feedback::log_text(
                                    &log_lines,
                                    crate::feedback::home_dir().as_deref(),
                                    usize::MAX,
                                );
                                egui::ScrollArea::vertical().max_height(140.0).show(ui, |ui| {
                                    ui.add(
                                        egui::TextEdit::multiline(&mut shown)
                                            .interactive(false)
                                            .font(egui::TextStyle::Monospace)
                                            .desired_width(f32::INFINITY),
                                    );
                                });
                            });
                        }
                        ui.add_space(8.0);
                        ui.label(
                            egui::RichText::new(format!(
                                "Geht mit: {}",
                                self.feedback.outgoing(log_lines.len()).join(" · ")
                            ))
                            .size(theme::TYPE_SMALL)
                            .color(theme::text_dim()),
                        );
                        ui.add_space(16.0);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let send = egui::Button::new(
                                egui::RichText::new(format!(
                                    "{} Abschicken",
                                    theme::ic::PAPER_PLANE_RIGHT
                                ))
                                .strong()
                                .color(theme::well()),
                            )
                            .fill(theme::ok())
                            .min_size(egui::vec2(132.0, 34.0));
                            if ui.add(send).on_hover_text("Strg+Enter").clicked() {
                                self.feedback.send(&self.config, &feedback_log_lines());
                            }
                            let cancel = egui::Button::new("Lieber später")
                                .min_size(egui::vec2(110.0, 34.0));
                            if ui.add(cancel).clicked() {
                                close = true;
                            }
                        });
                    }
                    Phase::Sending => {
                        ui.add_space(16.0);
                        ui.vertical_centered(|ui| {
                            ui.spinner();
                            ui.label(
                                egui::RichText::new("Wird gesendet …").color(theme::text_dim()),
                            );
                        });
                        ui.add_space(16.0);
                    }
                    Phase::Done(msg) if self.feedback.is_thanks() => {
                        ui.add_space(12.0);
                        ui.vertical_centered(|ui| {
                            ui.label(
                                egui::RichText::new(theme::ic::CHECK_CIRCLE)
                                    .size(40.0)
                                    .color(theme::ok()),
                            );
                            ui.add_space(6.0);
                            ui.label(
                                egui::RichText::new(msg)
                                    .size(16.0)
                                    .strong()
                                    .color(theme::text()),
                            );
                            ui.label(
                                egui::RichText::new("Das Fenster schließt sich gleich.")
                                    .small()
                                    .color(theme::text_dim()),
                            );
                        });
                        // Thin bar that runs out with the countdown.
                        let left = self.feedback.thanks_left(now).unwrap_or_default();
                        let frac =
                            left.as_secs_f32() / crate::feedback::THANKS_VISIBLE.as_secs_f32();
                        ui.add_space(10.0);
                        let (r, _) = ui.allocate_exact_size(
                            egui::vec2(ui.available_width(), 3.0),
                            egui::Sense::hover(),
                        );
                        ui.painter().rect_filled(r, 1.5, theme::well());
                        let mut bar = r;
                        bar.set_width(r.width() * frac.clamp(0.0, 1.0));
                        ui.painter().rect_filled(bar, 1.5, theme::ok());
                    }
                    Phase::Done(msg) => {
                        ui.add_space(8.0);
                        ui.label(egui::RichText::new(msg).color(theme::danger_text()));
                        ui.add_space(12.0);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .add(
                                    egui::Button::new("Schließen").min_size(egui::vec2(96.0, 30.0)),
                                )
                                .clicked()
                            {
                                close = true;
                            }
                        });
                    }
                    Phase::Closed | Phase::Capturing => {}
                }
            });
        if close {
            self.feedback.close();
        }
    }
}
