//! Automation im Fenster (Paket 18): der Aufnehmer für Controller-Fahrten (rein, ohne Display
//! testbar) und der Kurven-Editor im Studio-Dock.

use eframe::egui;
use woa_model::{
    AutoMode, AutoPoint, AutoRange, AutoTarget, AutomationLane, MAX_AUTOMATION_POINTS, Project,
    TrackId,
};

use crate::state::AppState;
use crate::theme;

/// Kleinste Wertänderung (Anteil am Bereich), die einen neuen Punkt wert ist.
const MIN_STEP: f64 = 0.004;

/// Nimmt die Fahrt eines Reglers auf: Schreiben ab Wiedergabestart, Latch ab der ersten
/// Berührung; beides bis zum Stopp. Der Aufnehmer kennt keine Zeit und kein Gerät, die
/// Oberfläche füttert ihn je Bild.
#[derive(Debug, Clone)]
pub struct AutoRecorder {
    mode: AutoMode,
    range: AutoRange,
    /// Letzter Wert, der gilt (Berührung).
    held: Option<f64>,
    /// Beat der ersten aufgenommenen Stelle.
    start: Option<f64>,
    /// Beat der letzten Stelle.
    end: f64,
    points: Vec<AutoPoint>,
    full: bool,
}

impl AutoRecorder {
    /// Neuer Aufnehmer für `mode` (Schreiben oder Latch).
    pub fn new(mode: AutoMode, range: AutoRange) -> Self {
        Self {
            mode,
            range,
            held: None,
            start: None,
            end: 0.0,
            points: Vec::new(),
            full: false,
        }
    }

    /// Hat er etwas aufgenommen?
    pub fn has_data(&self) -> bool {
        !self.points.is_empty()
    }

    /// Ein Bild während der Wiedergabe: `beat` ist die Kopfposition, `touch` der Wert, den der
    /// Controller in diesem Bild gesendet hat, `fader` der Stand des Reglers im Projekt.
    pub fn update(&mut self, beat: f64, touch: Option<f64>, fader: f64) {
        if let Some(v) = touch {
            self.held = Some(v);
        }
        let armed = match self.mode {
            AutoMode::Write => true,
            AutoMode::Latch => self.held.is_some(),
            AutoMode::Read => false,
        };
        if !armed || self.full {
            return;
        }
        let raw = self
            .held
            .unwrap_or(fader)
            .clamp(self.range.min, self.range.max);
        let value = if self.range.step { raw.round() } else { raw };
        if self.start.is_none() {
            self.start = Some(beat);
        }
        // Springt der Kopf zurück (Loop), gilt die neuere Fahrt.
        while self.points.last().is_some_and(|l| l.beat >= beat) {
            self.points.pop();
        }
        let prev = if self.end < beat { self.end } else { beat };
        self.end = beat;
        let span = (self.range.max - self.range.min).abs().max(f64::EPSILON);
        let moved = self
            .points
            .last()
            .is_none_or(|l| (l.value - value).abs() / span >= MIN_STEP);
        if self.points.len() + 8 >= MAX_AUTOMATION_POINTS {
            self.full = true;
        } else if moved {
            // Ein gehaltener Wert bleibt bis kurz vor der Änderung stehen (kein Anstieg).
            if let Some(l) = self.points.last().copied()
                && l.beat < prev
            {
                self.points.push(AutoPoint {
                    beat: prev,
                    value: l.value,
                });
            }
            self.points.push(AutoPoint { beat, value });
        }
    }

    /// Stopp: Die aufgenommene Strecke ersetzt `existing` zwischen ihrem Anfang und Ende,
    /// davor und danach bleibt die alte Kurve. `None`, wenn nichts aufgenommen wurde.
    pub fn finish(mut self, existing: &[AutoPoint]) -> Option<Vec<AutoPoint>> {
        let start = self.start?;
        let value = self.points.last()?.value;
        if self.points.last().is_some_and(|l| l.beat < self.end) {
            self.points.push(AutoPoint {
                beat: self.end,
                value,
            });
        }
        let end = self.end;
        let mut out: Vec<AutoPoint> = existing
            .iter()
            .copied()
            .filter(|p| p.beat < start || p.beat > end)
            .collect();
        out.extend(self.points);
        out.sort_by(|a, b| a.beat.total_cmp(&b.beat));
        out.truncate(MAX_AUTOMATION_POINTS);
        Some(out)
    }
}

/// Alle Ziele einer Spur mit Namen für die Auswahl.
pub fn targets_of(project: &Project, track: TrackId) -> Vec<(AutoTarget, String)> {
    let Some(t) = project.track(track) else {
        return Vec::new();
    };
    let mut out = vec![
        (AutoTarget::Gain, "Lautstärke".to_string()),
        (AutoTarget::Pan, "Pan".to_string()),
    ];
    for s in &t.sends {
        let name = project
            .track(s.target)
            .map_or_else(|| "?".to_string(), |b| b.name.clone());
        out.push((
            AutoTarget::Send { target: s.target },
            format!("Send {name}"),
        ));
    }
    for i in &t.inserts {
        for (n, spec) in i.kind.params().iter().enumerate() {
            out.push((
                AutoTarget::InsertParam {
                    insert: i.id,
                    param: n,
                },
                format!("{} · {}", i.kind.label(), spec.label),
            ));
        }
    }
    out
}

/// Zustand des Editors (nur Anzeige, nichts davon wird gespeichert).
#[derive(Debug, Default)]
pub struct EditorState {
    /// Gewähltes Ziel; `None` = Lautstärke.
    pub target: Option<AutoTarget>,
    /// Punkt, der gerade gezogen wird, mit seiner vorläufigen Lage.
    drag: Option<(usize, AutoPoint)>,
}

/// Sichtbare Länge in Beats: bis hinter den letzten Punkt, mindestens 16.
fn view_beats(points: &[AutoPoint], playhead: f64) -> f64 {
    let last = points.last().map_or(0.0, |p| p.beat);
    (last.max(playhead) + 4.0).max(16.0).ceil()
}

/// Kurven-Editor für die Spur `track`. Klick auf freie Fläche setzt einen Punkt, Ziehen
/// bewegt ihn (ein Undo-Schritt beim Loslassen), Rechtsklick löscht ihn.
pub fn show(
    ui: &mut egui::Ui,
    state: &mut AppState,
    ed: &mut EditorState,
    track: TrackId,
    playhead: f64,
) {
    let targets = targets_of(state.project(), track);
    if targets.is_empty() {
        ui.label(egui::RichText::new("Spur wählen").color(theme::text_dim()));
        return;
    }
    let target = ed
        .target
        .filter(|t| targets.iter().any(|(x, _)| x == t))
        .unwrap_or(AutoTarget::Gain);
    let Some(t) = state.project().track(track) else {
        return;
    };
    let Ok(range) = target.range_in(t) else {
        return;
    };
    let lane: AutomationLane = t.lane(&target).cloned().unwrap_or(AutomationLane {
        target,
        mode: AutoMode::Read,
        points: Vec::new(),
    });
    let name = targets
        .iter()
        .find(|(x, _)| *x == target)
        .map_or("", |(_, n)| n.as_str())
        .to_string();

    let mut new_mode = lane.mode;
    let mut clear = false;
    ui.horizontal(|ui| {
        ui.label(theme::panel_label("AUTOMATION"));
        egui::ComboBox::from_id_salt("auto_target")
            .selected_text(name.clone())
            .width(180.0)
            .show_ui(ui, |ui| {
                for (x, n) in &targets {
                    if ui.selectable_label(*x == target, n).clicked() {
                        ed.target = Some(*x);
                        ed.drag = None;
                    }
                }
            });
        for m in AutoMode::ALL {
            let on = lane.mode == m;
            let hover = match m {
                AutoMode::Read => "Lesen: die Kurve wird abgespielt",
                AutoMode::Write => {
                    "Schreiben: beim nächsten Abspielen nimmt die Kurve den Reglerstand \
                     und die Controller-Fahrt auf"
                }
                AutoMode::Latch => {
                    "Latch: Aufnahme beginnt bei der ersten Controller-Berührung und \
                     läuft bis zum Stopp"
                }
            };
            if ui
                .add(
                    egui::Button::new(egui::RichText::new(m.label()).small().color(if on {
                        theme::well()
                    } else {
                        theme::text()
                    }))
                    .fill(if on {
                        theme::accent()
                    } else {
                        theme::surface()
                    }),
                )
                .on_hover_text(hover)
                .clicked()
            {
                new_mode = m;
            }
        }
        if ui
            .add_enabled(!lane.is_blank(), egui::Button::new("Kurve löschen"))
            .clicked()
        {
            clear = true;
        }
        if let Some(v) = lane.value_at(playhead, range.step) {
            ui.label(
                egui::RichText::new(format!("am Kopf: {v:.2} {}", range.unit))
                    .monospace()
                    .small()
                    .color(theme::cue()),
            );
        }
    });
    if clear {
        state.set_automation(track, target, Vec::new(), AutoMode::Read);
        return;
    }
    if new_mode != lane.mode {
        state.set_automation(track, target, lane.points.clone(), new_mode);
        return;
    }
    if lane.mode != AutoMode::Read {
        ui.label(
            egui::RichText::new(
                "Aufnahme bereit: Wiedergabe starten und den Regler am Controller fahren. \
                 Beim Stopp wird die Fahrt eine Kurve (ein Rückgängig-Schritt).",
            )
            .small()
            .color(theme::accent()),
        );
    }

    let size = egui::vec2(
        ui.available_width(),
        (ui.available_height() - 4.0).max(90.0),
    );
    let (resp, painter) = ui.allocate_painter(size, egui::Sense::click_and_drag());
    let rect = resp.rect;
    painter.rect_filled(rect, 2.0, theme::well());
    let beats = view_beats(&lane.points, playhead);
    let to_x = |b: f64| rect.left() + (b / beats) as f32 * rect.width();
    let to_y = |v: f64| rect.bottom() - range.to_norm(v) as f32 * rect.height();
    let from_pos = |p: egui::Pos2| -> AutoPoint {
        let beat = ((f64::from(p.x - rect.left()) / f64::from(rect.width())) * beats).max(0.0);
        let beat = (beat * 4.0).round() / 4.0;
        let n = f64::from((rect.bottom() - p.y) / rect.height());
        AutoPoint {
            beat,
            value: range.from_norm(n),
        }
    };
    for b in (0..=(beats as i64)).step_by(4) {
        painter.line_segment(
            [
                egui::pos2(to_x(b as f64), rect.top()),
                egui::pos2(to_x(b as f64), rect.bottom()),
            ],
            egui::Stroke::new(1.0, theme::grid()),
        );
    }
    // Punkte mit der Lage des laufenden Ziehens.
    let mut pts = lane.points.clone();
    if let Some((i, p)) = ed.drag
        && i < pts.len()
    {
        pts[i] = p;
    }
    let color = if lane.mode == AutoMode::Read {
        theme::accent()
    } else {
        theme::text_dim()
    };
    if let (Some(first), Some(last)) = (pts.first(), pts.last()) {
        let mut line: Vec<egui::Pos2> = vec![egui::pos2(rect.left(), to_y(first.value))];
        for (k, p) in pts.iter().enumerate() {
            if range.step && k > 0 {
                line.push(egui::pos2(to_x(p.beat), to_y(pts[k - 1].value)));
            }
            line.push(egui::pos2(to_x(p.beat), to_y(p.value)));
        }
        line.push(egui::pos2(rect.right(), to_y(last.value)));
        painter.add(egui::Shape::line(line, egui::Stroke::new(1.5, color)));
    }
    for p in &pts {
        painter.circle_filled(egui::pos2(to_x(p.beat), to_y(p.value)), 4.0, color);
    }
    painter.line_segment(
        [
            egui::pos2(to_x(playhead), rect.top()),
            egui::pos2(to_x(playhead), rect.bottom()),
        ],
        egui::Stroke::new(1.0, theme::cue()),
    );
    painter.text(
        rect.left_top() + egui::vec2(4.0, 2.0),
        egui::Align2::LEFT_TOP,
        format!("{:.1} … {:.1} {}", range.max, range.min, range.unit),
        egui::FontId::monospace(9.5),
        theme::text_dim(),
    );

    // Bedienung
    let hit = |pos: egui::Pos2| -> Option<usize> {
        lane.points
            .iter()
            .position(|p| egui::pos2(to_x(p.beat), to_y(p.value)).distance(pos) <= 7.0)
    };
    if resp.drag_started()
        && let Some(pos) = resp.interact_pointer_pos()
        && let Some(i) = hit(pos)
    {
        ed.drag = Some((i, lane.points[i]));
    }
    if resp.dragged()
        && let Some((i, _)) = ed.drag
        && let Some(pos) = resp.interact_pointer_pos()
    {
        let mut p = from_pos(pos);
        // Zwischen den Nachbarn bleiben, damit die Reihenfolge hält.
        if i > 0 {
            p.beat = p.beat.max(lane.points[i - 1].beat + 0.25);
        }
        if let Some(next) = lane.points.get(i + 1) {
            p.beat = p.beat.min(next.beat - 0.25);
        }
        ed.drag = Some((i, p));
    }
    if resp.drag_stopped()
        && let Some((i, p)) = ed.drag.take()
    {
        let mut next = lane.points.clone();
        if i < next.len() {
            next[i] = p;
            state.set_automation(track, target, next, lane.mode);
        }
    }
    if resp.clicked()
        && ed.drag.is_none()
        && let Some(pos) = resp.interact_pointer_pos()
        && hit(pos).is_none()
    {
        let p = from_pos(pos);
        let next = lane.clone().with_point(p.beat, p.value).points;
        state.set_automation(track, target, next, lane.mode);
    }
    if resp.secondary_clicked()
        && let Some(pos) = resp.interact_pointer_pos()
        && let Some(i) = hit(pos)
    {
        let mut next = lane.points.clone();
        next.remove(i);
        state.set_automation(track, target, next, lane.mode);
    }
}

/// Gain-Reduction-Anzeige: ein schmaler Balken, 0 bis 24 dB von rechts nach links.
pub fn reduction_bar(ui: &mut egui::Ui, db: f32) {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(46.0, 8.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, 1.0, theme::well());
    let w = (db / 24.0).clamp(0.0, 1.0) * rect.width();
    let bar = egui::Rect::from_min_size(rect.right_top() - egui::vec2(w, 0.0), egui::vec2(w, 8.0));
    ui.painter().rect_filled(bar, 1.0, theme::accent());
    resp.on_hover_text(format!("Absenkung {db:.1} dB"));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn range() -> AutoRange {
        AutoRange {
            min: -60.0,
            max: 12.0,
            step: false,
            unit: "dB",
        }
    }

    fn pt(beat: f64, value: f64) -> AutoPoint {
        AutoPoint { beat, value }
    }

    #[test]
    fn write_records_from_the_start_and_keeps_the_fader_value() {
        let mut r = AutoRecorder::new(AutoMode::Write, range());
        r.update(0.0, None, -6.0);
        r.update(1.0, None, -6.0);
        r.update(2.0, Some(-20.0), -6.0);
        r.update(3.0, None, -6.0);
        let out = r.finish(&[]).unwrap();
        assert_eq!(out[0], pt(0.0, -6.0));
        assert!(out.contains(&pt(2.0, -20.0)));
        assert_eq!(out.last().unwrap(), &pt(3.0, -20.0));
    }

    #[test]
    fn a_held_value_stays_flat_until_it_changes() {
        let mut r = AutoRecorder::new(AutoMode::Write, range());
        for i in 0..=20 {
            let t = if i == 10 { Some(-40.0) } else { None };
            r.update(f64::from(i) * 0.1, t, -6.0);
        }
        let out = r.finish(&[]).unwrap();
        let at = |b: f64| woa_model::automation::value_at(&out, b, false).unwrap();
        assert!((at(0.5) + 6.0).abs() < 1e-9, "{}", at(0.5));
        assert!((at(0.85) + 6.0).abs() < 1e-9);
        assert!((at(1.5) + 40.0).abs() < 1e-9);
    }

    #[test]
    fn latch_waits_for_the_first_touch() {
        let mut r = AutoRecorder::new(AutoMode::Latch, range());
        r.update(0.0, None, -6.0);
        r.update(1.0, None, -6.0);
        assert!(!r.has_data());
        r.update(2.0, Some(-12.0), -6.0);
        r.update(4.0, None, -6.0);
        let out = r.finish(&[]).unwrap();
        assert_eq!(out.first().unwrap(), &pt(2.0, -12.0));
        assert_eq!(out.last().unwrap(), &pt(4.0, -12.0));
    }

    #[test]
    fn finishing_replaces_only_the_recorded_stretch() {
        let old = [pt(0.0, 0.0), pt(3.0, -30.0), pt(8.0, 0.0)];
        let mut r = AutoRecorder::new(AutoMode::Latch, range());
        r.update(2.0, Some(-10.0), 0.0);
        r.update(4.0, None, 0.0);
        let out = r.finish(&old).unwrap();
        assert_eq!(out.first().unwrap(), &pt(0.0, 0.0));
        assert!(!out.iter().any(|p| p.beat == 3.0));
        assert_eq!(out.last().unwrap(), &pt(8.0, 0.0));
        assert!(out.windows(2).all(|w| w[0].beat < w[1].beat));
    }

    #[test]
    fn nothing_recorded_gives_nothing() {
        let r = AutoRecorder::new(AutoMode::Latch, range());
        assert!(r.finish(&[]).is_none());
    }

    #[test]
    fn values_are_clamped_and_a_standing_fader_adds_no_points() {
        let mut r = AutoRecorder::new(AutoMode::Write, range());
        for i in 0..100 {
            r.update(f64::from(i) * 0.1, None, 40.0);
        }
        let out = r.finish(&[]).unwrap();
        assert_eq!(out[0].value, 12.0);
        assert!(out.len() <= 2, "{}", out.len());
    }

    #[test]
    fn a_long_take_stays_inside_the_point_limit() {
        let mut r = AutoRecorder::new(AutoMode::Write, range());
        for i in 0..20_000 {
            let v = if i % 2 == 0 { -10.0 } else { -40.0 };
            r.update(f64::from(i) * 0.01, Some(v), 0.0);
        }
        let out = r.finish(&[]).unwrap();
        assert!(out.len() <= MAX_AUTOMATION_POINTS);
    }

    #[test]
    fn a_loop_jump_back_overwrites_the_newer_stretch() {
        let mut r = AutoRecorder::new(AutoMode::Write, range());
        r.update(0.0, Some(-10.0), 0.0);
        r.update(6.0, Some(-30.0), 0.0);
        r.update(0.0, Some(-5.0), 0.0);
        r.update(2.0, Some(-6.0), 0.0);
        let out = r.finish(&[]).unwrap();
        assert!(out.windows(2).all(|w| w[0].beat < w[1].beat));
    }
}
