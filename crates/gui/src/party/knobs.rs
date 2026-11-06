//! Regler des DJ-Mixers in vier wählbaren Stilen (Design-Fassung 8.1, `DjMixerStyles`):
//! Mini-Fader (Standard), Wertbalken, Zahlenfelder, Drehknöpfe. Ein Regler, eine Logik, vier
//! Zeichnungen. Für alle gleich: Doppelklick = Standardwert, Umschalt = fein, Mausrad ändert,
//! der Tooltip zeigt den Wert. Der Stilwechsel ändert keinen Wert.

use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, StrokeKind};

use crate::theme;

/// Regler-Stil, pro Nutzer gespeichert (nicht im Projekt).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnobStyle {
    /// Senkrechte Mini-Fader, Klick springt (Standard).
    #[default]
    MiniFader,
    /// Waagerechte Wertbalken, Klick springt.
    Bar,
    /// Zahlenfelder: ziehen, Mausrad, Klick zum Tippen.
    Number,
    /// Drehknöpfe, senkrecht ziehen.
    Knob,
}

impl KnobStyle {
    pub const ALL: [KnobStyle; 4] = [Self::MiniFader, Self::Bar, Self::Number, Self::Knob];

    pub fn label(self) -> &'static str {
        match self {
            Self::MiniFader => "Mini-Fader",
            Self::Bar => "Wertbalken",
            Self::Number => "Zahlenfelder",
            Self::Knob => "Drehknöpfe",
        }
    }

    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// Wie ein Wert angezeigt und getippt wird.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fmt {
    /// Wert ist schon dB (Trim).
    Db,
    /// EQ-Stellung −1..1, angezeigt in dB, ganz links „Kill“.
    Eq,
    /// Filter −1..1: links Tiefpass, rechts Hochpass.
    Filter,
    /// 0..1 als Prozent.
    Pct,
}

/// Anzeige eines Werts.
pub fn show(fmt: Fmt, v: f32) -> String {
    match fmt {
        Fmt::Db => format!("{v:+.1} dB"),
        Fmt::Eq => {
            let g = woa_dsp::dj::eq_gain(v);
            if g <= 0.0 {
                "Kill".into()
            } else {
                format!("{:+.1} dB", 20.0 * g.log10())
            }
        }
        Fmt::Filter => {
            if v.abs() < 0.02 {
                "aus".into()
            } else if v < 0.0 {
                format!("TP {:.0} %", -v * 100.0)
            } else {
                format!("HP {:.0} %", v * 100.0)
            }
        }
        Fmt::Pct => format!("{:.0} %", v * 100.0),
    }
}

/// Getippter Text → Wert (Komma oder Punkt, Einheit darf fehlen).
pub fn parse(fmt: Fmt, s: &str) -> Option<f32> {
    let t = s.trim().to_lowercase();
    if fmt == Fmt::Eq && t.starts_with("kill") {
        return Some(-1.0);
    }
    let num: String = t
        .chars()
        .filter(|c| c.is_ascii_digit() || matches!(c, '-' | '+' | '.' | ','))
        .collect();
    let x: f32 = num.replace(',', ".").parse().ok()?;
    if !x.is_finite() {
        return None;
    }
    Some(match fmt {
        Fmt::Db => x,
        Fmt::Eq => {
            if x >= 0.0 {
                x / 6.0
            } else {
                x / 40.0
            }
        }
        Fmt::Filter => {
            let x = x / 100.0;
            if t.contains("tp") { -x.abs() } else { x }
        }
        Fmt::Pct => x / 100.0,
    })
}

/// Ein Regler: Wert, Bereich, Standard, Farbe, Beschriftung.
pub struct Ctl<'a> {
    pub id: egui::Id,
    pub value: f32,
    pub range: (f32, f32),
    pub default: f32,
    pub color: Color32,
    pub label: &'a str,
    pub fmt: Fmt,
    pub tip: &'a str,
}

/// Breite eines Mini-Faders samt Beschriftung.
pub const MINI_W: f32 = 34.0;
/// Höhe einer Wertbalken- oder Zahlenfeld-Zeile (Beschriftung und 18 px Feld).
pub const ROW_H: f32 = 32.0;
/// KILL-Taste.
pub const KILL_W: f32 = 34.0;
pub const KILL_H: f32 = 22.0;

/// Größe des Reglers im Stil: `extent` ist beim Mini-Fader die Höhe, bei Balken und Zahl die
/// Breite, beim Drehknopf der Durchmesser.
/// Der Mini-Fader nimmt höchstens `MINI_W`, in enger Spalte weniger.
pub fn size(style: KnobStyle, extent: f32, avail_w: f32) -> egui::Vec2 {
    match style {
        KnobStyle::MiniFader => egui::vec2(avail_w.clamp(18.0, MINI_W), extent),
        KnobStyle::Bar | KnobStyle::Number => egui::vec2(extent, ROW_H),
        KnobStyle::Knob => egui::vec2(extent + 16.0, extent + 14.0),
    }
}

/// Zeichnet den Regler; liefert die Antwort und einen neuen Wert, falls geändert.
pub fn control(
    ui: &mut egui::Ui,
    style: KnobStyle,
    c: &Ctl,
    extent: f32,
) -> (egui::Response, Option<f32>) {
    let edit_id = c.id.with("edit");
    let editing: Option<String> = ui.data(|m| m.get_temp(edit_id));
    let (rect, resp) = ui.allocate_exact_size(
        size(style, extent, ui.available_width()),
        Sense::click_and_drag(),
    );
    let (lo, hi) = c.range;
    let span = hi - lo;
    let fine = if ui.input(|i| i.modifiers.shift) {
        0.2
    } else {
        1.0
    };
    let held: Option<f32> = ui.data(|m| m.get_temp(c.id));
    let mut v = held.unwrap_or(c.value).clamp(lo, hi);
    let mut out = None;
    let frac = |x: f32| ((x - lo) / span).clamp(0.0, 1.0);
    let track = track_rect(style, rect);
    let pointer = resp.interact_pointer_pos();
    match style {
        KnobStyle::MiniFader | KnobStyle::Bar => {
            let vertical = style == KnobStyle::MiniFader;
            if resp.dragged() && fine < 1.0 {
                let d = if vertical {
                    -resp.drag_delta().y / track.height().max(1.0)
                } else {
                    resp.drag_delta().x / track.width().max(1.0)
                };
                v = (v + d * span * fine).clamp(lo, hi);
                out = Some(v);
            } else if (resp.dragged() || resp.clicked())
                && let Some(p) = pointer
            {
                let f = if vertical {
                    1.0 - (p.y - track.top()) / track.height().max(1.0)
                } else {
                    (p.x - track.left()) / track.width().max(1.0)
                };
                v = lo + f.clamp(0.0, 1.0) * span;
                out = Some(v);
            }
        }
        KnobStyle::Number | KnobStyle::Knob => {
            if resp.dragged() {
                v = (v - resp.drag_delta().y / 150.0 * span * fine).clamp(lo, hi);
                out = Some(v);
            }
        }
    }
    if resp.hovered() {
        // Eine Rasterstufe (etwa 50 px) = 2 % des Bereichs.
        let d = ui.input(|i| i.smooth_scroll_delta.y);
        if d != 0.0 {
            v = (v + d / 50.0 * span / 50.0 * fine).clamp(lo, hi);
            out = Some(v);
        }
    }
    if theme::double_clicked(&resp) {
        v = c.default;
        out = Some(v);
        ui.data_mut(|m| m.remove::<String>(edit_id));
    } else if style == KnobStyle::Number && resp.clicked() && editing.is_none() {
        ui.data_mut(|m| m.insert_temp(edit_id, show(c.fmt, v)));
    }
    if resp.dragged() {
        ui.data_mut(|m| m.insert_temp(c.id, v));
    } else if held.is_some() {
        ui.data_mut(|m| m.remove::<f32>(c.id));
    }

    let p = ui.painter().clone();
    let label_pos = |y: f32| egui::pos2(rect.center().x, y);
    match style {
        KnobStyle::MiniFader => {
            let well = Rect::from_center_size(track.center(), egui::vec2(4.0, track.height()));
            p.rect_filled(well, 2.0, theme::well());
            let y = |f: f32| track.bottom() - f * track.height();
            let (a, b) = (y(frac(c.default)), y(frac(v)));
            p.rect_filled(
                Rect::from_x_y_ranges(well.x_range(), a.min(b)..=a.max(b)),
                2.0,
                c.color,
            );
            let cap =
                Rect::from_center_size(egui::pos2(track.center().x, b), egui::vec2(18.0, 8.0));
            p.rect_filled(cap, 2.0, theme::surface());
            p.rect_stroke(
                cap,
                2.0,
                Stroke::new(1.0, theme::border_strong()),
                StrokeKind::Inside,
            );
            p.hline(cap.x_range().shrink(3.0), b, Stroke::new(1.5, c.color));
            p.text(
                label_pos(rect.bottom() - 6.0),
                egui::Align2::CENTER_CENTER,
                c.label,
                theme::label_font(10.0),
                theme::text_dim(),
            );
        }
        KnobStyle::Bar => {
            head(&p, rect, c.label, &show(c.fmt, v));
            p.rect_filled(track, 3.0, theme::well());
            p.rect_stroke(
                track,
                3.0,
                Stroke::new(1.0, theme::border_strong()),
                StrokeKind::Inside,
            );
            let x = |f: f32| track.left() + f * track.width();
            let (a, b) = (x(frac(c.default)), x(frac(v)));
            p.rect_filled(
                Rect::from_x_y_ranges(
                    a.min(b)..=a.max(b).max(a.min(b) + 1.0),
                    track.y_range().shrink(3.0),
                ),
                2.0,
                c.color,
            );
        }
        KnobStyle::Number => {
            p.text(
                egui::pos2(rect.left(), rect.top() + 6.0),
                egui::Align2::LEFT_CENTER,
                c.label,
                theme::label_font(10.0),
                theme::text_dim(),
            );
            p.rect_filled(track, 3.0, theme::well());
            p.rect_stroke(
                track,
                3.0,
                Stroke::new(1.0, theme::border_strong()),
                StrokeKind::Inside,
            );
            if let Some(mut s) = editing {
                let te = ui.put(
                    track,
                    egui::TextEdit::singleline(&mut s)
                        .font(theme::label_font(12.0))
                        .horizontal_align(egui::Align::RIGHT),
                );
                te.request_focus();
                if te.lost_focus() {
                    ui.data_mut(|m| m.remove::<String>(edit_id));
                    if !ui.input(|i| i.key_pressed(egui::Key::Escape))
                        && let Some(x) = parse(c.fmt, &s)
                    {
                        v = x.clamp(lo, hi);
                        out = Some(v);
                    }
                } else {
                    ui.data_mut(|m| m.insert_temp(edit_id, s));
                }
            } else {
                p.text(
                    egui::pos2(track.right() - 6.0, track.center().y),
                    egui::Align2::RIGHT_CENTER,
                    show(c.fmt, v),
                    theme::label_font(12.0),
                    if (v - c.default).abs() > 1e-4 {
                        c.color
                    } else {
                        theme::text()
                    },
                );
            }
        }
        KnobStyle::Knob => {
            let size = extent;
            let center = egui::pos2(rect.center().x, rect.top() + size / 2.0);
            let r = size / 2.0;
            p.circle_filled(center, r, theme::surface());
            p.circle_stroke(center, r, Stroke::new(1.0, theme::border_strong()));
            let ang = |f: f32| std::f32::consts::PI * (0.75 + 1.5 * f);
            let pt = |a: f32, rr: f32| egui::pos2(center.x + a.cos() * rr, center.y + a.sin() * rr);
            let (a0, a1) = (ang(frac(c.default)), ang(frac(v)));
            if (a1 - a0).abs() > 0.01 {
                let n = 24;
                let pts: Vec<Pos2> = (0..=n)
                    .map(|i| pt(a0 + (a1 - a0) * i as f32 / n as f32, r + 3.0))
                    .collect();
                p.add(egui::Shape::line(pts, Stroke::new(2.5, c.color)));
            }
            p.line_segment(
                [pt(a1, r * 0.25), pt(a1, r * 0.85)],
                Stroke::new(2.5, c.color),
            );
            p.text(
                label_pos(rect.bottom() - 5.0),
                egui::Align2::CENTER_CENTER,
                c.label,
                theme::panel_label_font(),
                theme::text_dim(),
            );
        }
    }
    let label = c.label.to_string();
    let resp = resp
        .on_hover_text(format!("{}: {}\n{}", c.label, show(c.fmt, v), c.tip))
        .on_hover_text("Doppelklick = Standard · Umschalt = fein · Mausrad ändert");
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Slider, true, &label));
    (resp, out)
}

/// Kopfzeile des Wertbalkens: Name links, Wert rechts.
fn head(p: &egui::Painter, rect: Rect, label: &str, value: &str) {
    let y = rect.top() + 6.0;
    p.text(
        egui::pos2(rect.left(), y),
        egui::Align2::LEFT_CENTER,
        label,
        theme::label_font(10.0),
        theme::text_dim(),
    );
    p.text(
        egui::pos2(rect.right(), y),
        egui::Align2::RIGHT_CENTER,
        value,
        theme::label_font(10.0),
        theme::text(),
    );
}

/// Bedienfläche im Regler (Schacht, Balken oder Feld).
fn track_rect(style: KnobStyle, rect: Rect) -> Rect {
    match style {
        KnobStyle::MiniFader => Rect::from_min_max(
            egui::pos2(rect.left(), rect.top() + 6.0),
            egui::pos2(rect.right(), rect.bottom() - 18.0),
        ),
        KnobStyle::Bar | KnobStyle::Number => Rect::from_min_max(
            egui::pos2(rect.left(), rect.top() + 13.0),
            rect.right_bottom(),
        ),
        KnobStyle::Knob => rect,
    }
}

/// KILL-Taste eines EQ-Bands: setzt auf −1, ein zweiter Klick stellt den Wert davor wieder her.
pub fn kill(ui: &mut egui::Ui, id: egui::Id, value: f32) -> Option<f32> {
    let on = value <= -0.98;
    let w = ui.available_width().clamp(18.0, KILL_W);
    let r = crate::widgets::Key::new("KILL", egui::vec2(w, KILL_H))
        .font(theme::label_font(10.0))
        .radius(theme::RADIUS_SM)
        .color(theme::warn())
        .on(on)
        .ui(ui)
        .on_hover_text(if on {
            "Kill an: zurück auf den Wert davor"
        } else {
            "Kill: dieses Band ganz stumm"
        });
    if !r.clicked() {
        return None;
    }
    let mem = id.with("before-kill");
    if on {
        let back: f32 = ui.data(|m| m.get_temp(mem)).unwrap_or(0.0);
        Some(back)
    } else {
        ui.data_mut(|m| m.insert_temp(mem, value));
        Some(-1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_show_and_parse_back() {
        assert_eq!(show(Fmt::Eq, -1.0), "Kill");
        assert_eq!(show(Fmt::Eq, 0.0), "+0.0 dB");
        assert_eq!(show(Fmt::Eq, 1.0), "+6.0 dB");
        assert_eq!(show(Fmt::Filter, 0.0), "aus");
        assert_eq!(parse(Fmt::Eq, "kill"), Some(-1.0));
        assert_eq!(parse(Fmt::Eq, "+3 dB"), Some(0.5));
        assert_eq!(parse(Fmt::Eq, "-20"), Some(-0.5));
        assert_eq!(parse(Fmt::Db, "-4,5"), Some(-4.5));
        assert_eq!(parse(Fmt::Pct, "80 %"), Some(0.8));
        assert_eq!(parse(Fmt::Filter, "TP 50 %"), Some(-0.5));
        assert_eq!(parse(Fmt::Pct, "abc"), None);
    }

    /// KILL setzt auf −1 und stellt beim zweiten Klick den Wert davor her; Doppelklick auf den
    /// Regler setzt den Standard, in jedem Stil.
    #[test]
    fn kill_toggles_back_and_double_click_resets() {
        use egui_kittest::kittest::Queryable;
        for style in KnobStyle::ALL {
            let mut h = egui_kittest::Harness::builder()
                .with_size(egui::vec2(200.0, 200.0))
                .build_ui_state(
                    move |ui, v: &mut f32| {
                        let ctx = ui.ctx().clone();
                        let fonts = egui::Id::new("test_fonts");
                        if ctx.data_mut(|m| m.get_temp::<bool>(fonts)).is_none() {
                            ctx.data_mut(|m| m.insert_temp(fonts, true));
                            theme::install_fonts(&ctx);
                            // Die Testuhr springt je Schritt weiter als echte Zeit; großzügig.
                            ctx.options_mut(|o| o.input_options.max_double_click_delay = 100.0);
                            return;
                        }
                        let id = egui::Id::new("t");
                        let c = Ctl {
                            id,
                            value: *v,
                            range: (-1.0, 1.0),
                            default: 0.0,
                            color: theme::accent(),
                            label: "HI",
                            fmt: Fmt::Eq,
                            tip: "",
                        };
                        if let (_, Some(x)) = control(ui, style, &c, 80.0) {
                            *v = x;
                        }
                        if let Some(x) = kill(ui, id, *v) {
                            *v = x;
                        }
                    },
                    0.4_f32,
                );
            h.run();
            h.get_by_label("KILL").click();
            h.run();
            assert_eq!(*h.state(), -1.0, "{style:?}: Kill");
            h.get_by_label("KILL").click();
            h.run();
            assert!((*h.state() - 0.4).abs() < 1e-6, "{style:?}: zurück");
            let at = h.get_by_label("HI").rect().center();
            h.event(egui::Event::PointerMoved(at));
            h.run();
            for _ in 0..2 {
                for pressed in [true, false] {
                    h.event(egui::Event::PointerButton {
                        pos: at,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    });
                    h.step();
                }
            }
            h.run();
            assert_eq!(*h.state(), 0.0, "{style:?}: Doppelklick");
        }
    }

    #[test]
    fn old_config_gets_mini_fader() {
        assert_eq!(KnobStyle::default(), KnobStyle::MiniFader);
        let s: KnobStyle = serde_json::from_str("\"knob\"").expect("parse");
        assert_eq!(s, KnobStyle::Knob);
    }
}
