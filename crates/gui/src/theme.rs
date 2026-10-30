//! Aussehen: dunkles Broadcast- & Studio-Thema (Ableton / mAirList-inspiriert).

use eframe::egui;
use egui::{
    Color32, CornerRadius, FontId, Frame, Margin, Rect, Stroke, StrokeKind, TextStyle, vec2,
};

/// Akzent (Wiedergabe, Kopf, Auswahl, LCD-Zahlen).
pub const ACCENT: Color32 = Color32::from_rgb(255, 152, 56);
/// Aufnahme, „On Air“, Übersteuerung.
pub const ON_AIR: Color32 = Color32::from_rgb(238, 56, 68);
/// positives Signal, Start, Gate offen, Stream verbunden.
pub const OK_GREEN: Color32 = Color32::from_rgb(56, 208, 112);
/// Cue-Punkt, MIDI-Akzent, Info-Badge.
pub const CUE_CYAN: Color32 = Color32::from_rgb(68, 186, 255);
/// Tiefste Vertiefung (LCD-Displays, Meter-Schächte, leere Zeitleiste).
pub const WELL: Color32 = Color32::from_rgb(12, 14, 18);
/// Tiefster Hintergrund (Zeitleiste, Hauptfläche).
pub const BG: Color32 = Color32::from_rgb(18, 20, 26);
/// Leisten, Karten und Spurköpfe.
pub const PANEL: Color32 = Color32::from_rgb(27, 30, 38);
/// Kopfzeile von Studio-Modulen.
pub const PANEL_HEADER: Color32 = Color32::from_rgb(34, 38, 48);
/// Knöpfe, Pads und bedienbare Felder.
pub const SURFACE: Color32 = Color32::from_rgb(41, 46, 58);
/// Rahmen- und Bevel-Linien für Module und Taster.
pub const BORDER: Color32 = Color32::from_rgb(56, 63, 78);
/// Rasterlinien in der Zeitleiste.
pub const GRID: Color32 = Color32::from_rgb(44, 49, 61);
/// Helle Hauptschrift.
pub const TEXT_BRIGHT: Color32 = Color32::from_rgb(236, 240, 248);
/// Gedämpfte Schrift.
pub const DIM: Color32 = Color32::from_rgb(138, 148, 166);

const TRACK_COLORS: [Color32; 6] = [
    Color32::from_rgb(74, 164, 255),
    Color32::from_rgb(76, 212, 138),
    Color32::from_rgb(192, 122, 255),
    Color32::from_rgb(255, 192, 68),
    Color32::from_rgb(64, 214, 232),
    Color32::from_rgb(255, 112, 158),
];

/// Farbe der Spur Nummer `i` (wiederholt sich nach sechs).
pub fn track_color(i: usize) -> Color32 {
    TRACK_COLORS[i % TRACK_COLORS.len()]
}

/// Mischt `c` mit `with` (0 = c, 1 = with).
pub fn mix(c: Color32, with: Color32, t: f32) -> Color32 {
    let m = |a: u8, b: u8| (f32::from(a) + (f32::from(b) - f32::from(a)) * t).round() as u8;
    Color32::from_rgb(m(c.r(), with.r()), m(c.g(), with.g()), m(c.b(), with.b()))
}

/// Eingefasster Studio-Modul-Rahmen (Karte mit 1px-Bevel).
pub fn card_frame() -> Frame {
    Frame::new()
        .fill(PANEL)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(CornerRadius::same(6))
        .inner_margin(Margin::symmetric(12, 10))
}

/// Tiefschwarzes LCD-/Display-Panel für Uhren, Zähler und Messwerte.
pub fn lcd_frame() -> Frame {
    Frame::new()
        .fill(WELL)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(CornerRadius::same(5))
        .inner_margin(Margin::symmetric(12, 8))
}

/// Zeichnet ein kompaktes, beleuchtetes Status-Badge (LED-Pille).
pub fn status_pill(ui: &mut egui::Ui, label: &str, color: Color32, active: bool) {
    let fill = if active { mix(color, WELL, 0.72) } else { WELL };
    let stroke_col = if active {
        mix(color, Color32::WHITE, 0.15)
    } else {
        BORDER
    };
    let text_col = if active {
        mix(color, Color32::WHITE, 0.35)
    } else {
        DIM
    };
    Frame::new()
        .fill(fill)
        .stroke(Stroke::new(1.0, stroke_col))
        .corner_radius(CornerRadius::same(4))
        .inner_margin(Margin::symmetric(7, 2))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 5.0;
            ui.horizontal(|ui| {
                let (dot, _) = ui.allocate_exact_size(vec2(7.0, 7.0), egui::Sense::hover());
                ui.painter().circle_filled(
                    dot.center(),
                    3.2,
                    if active { color } else { mix(DIM, WELL, 0.5) },
                );
                ui.label(
                    egui::RichText::new(label)
                        .size(11.0)
                        .strong()
                        .monospace()
                        .color(text_col),
                );
            });
        });
}

/// Zeichnet eine mehrgliedrige Studio-LED-Pegelanzeige (-60..0 dB, Grün → Gelb → Rot).
pub fn led_meter(painter: &egui::Painter, rect: Rect, level: f32) {
    painter.rect_filled(rect, 3.0, WELL);
    painter.rect_stroke(
        rect,
        3.0,
        Stroke::new(1.0, mix(BORDER, WELL, 0.35)),
        StrokeKind::Inside,
    );
    let inner = rect.shrink2(vec2(2.0, 2.0));
    if inner.width() <= 4.0 || inner.height() <= 2.0 {
        return;
    }
    let db = 20.0 * level.max(1e-6).log10();
    let frac = ((db + 60.0) / 60.0).clamp(0.0, 1.0);
    let segments = ((inner.width() / 4.5).floor() as usize).clamp(10, 32);
    let gap = 1.5f32;
    let seg_w = ((inner.width() - gap * (segments as f32 - 1.0)) / segments as f32).max(1.5);
    let lit_count = (frac * segments as f32).ceil() as usize;

    for i in 0..segments {
        let seg_frac = (i + 1) as f32 / segments as f32;
        let seg_db = seg_frac * 60.0 - 60.0;
        let base_col = if seg_db > -3.0 {
            ON_AIR
        } else if seg_db > -12.0 {
            Color32::from_rgb(245, 198, 48)
        } else {
            OK_GREEN
        };
        let col = if i < lit_count && frac > 0.005 {
            base_col
        } else {
            mix(base_col, WELL, 0.86)
        };
        let x0 = inner.left() + i as f32 * (seg_w + gap);
        let seg_rect =
            Rect::from_min_size(egui::pos2(x0, inner.top()), vec2(seg_w, inner.height()));
        painter.rect_filled(seg_rect, 1.0, col);
    }
}

/// Zeichnet eine vertikale Studio-LED-Pegelanzeige (von unten nach oben, -60..0 dB) für den Mixer,
/// inklusive feiner dB-Skalenstriche (0, -6, -12, -24, -48 dB).
pub fn led_meter_vertical(painter: &egui::Painter, rect: Rect, level: f32) {
    painter.rect_filled(rect, 3.0, WELL);
    painter.rect_stroke(
        rect,
        3.0,
        Stroke::new(1.0, mix(BORDER, WELL, 0.35)),
        StrokeKind::Inside,
    );
    let inner = rect.shrink2(vec2(2.0, 2.0));
    if inner.width() <= 2.0 || inner.height() <= 8.0 {
        return;
    }
    let db = 20.0 * level.max(1e-6).log10();
    let frac = ((db + 60.0) / 60.0).clamp(0.0, 1.0);
    let segments = ((inner.height() / 4.2).floor() as usize).clamp(10, 32);
    let gap = 1.4f32;
    let seg_h = ((inner.height() - gap * (segments as f32 - 1.0)) / segments as f32).max(1.5);
    let lit_count = (frac * segments as f32).ceil() as usize;
    let bar_w = (inner.width() - 3.0).max(2.0);

    for i in 0..segments {
        let seg_frac = (i + 1) as f32 / segments as f32;
        let seg_db = seg_frac * 60.0 - 60.0;
        let base_col = if seg_db > -3.0 {
            ON_AIR
        } else if seg_db > -12.0 {
            Color32::from_rgb(245, 198, 48)
        } else {
            OK_GREEN
        };
        let is_top_lit = lit_count > 0 && i + 1 == lit_count && frac > 0.005;
        let col = if is_top_lit {
            mix(base_col, Color32::WHITE, 0.35)
        } else if i < lit_count && frac > 0.005 {
            base_col
        } else {
            mix(base_col, WELL, 0.86)
        };
        let y0 = inner.bottom() - (i as f32 + 1.0) * seg_h - i as f32 * gap;
        let seg_rect = Rect::from_min_size(egui::pos2(inner.left(), y0), vec2(bar_w, seg_h));
        painter.rect_filled(seg_rect, 1.0, col);
    }

    // Feine dB-Skalen-Ticks am rechten Schachtrand (-6, -12, -24, -48 dB)
    for mark_db in [-6.0f32, -12.0, -24.0, -48.0] {
        let m_frac = ((mark_db + 60.0) / 60.0).clamp(0.0, 1.0);
        let y = inner.bottom() - m_frac * inner.height();
        painter.hline(
            (inner.right() - 2.2)..=inner.right(),
            y,
            Stroke::new(1.0, mix(DIM, WELL, 0.35)),
        );
    }
}

/// Digitales dB-Spitzenwert-Badge (RadioKing / Studio One-Stil) für Kanalzüge.
pub fn db_readout_badge(ui: &mut egui::Ui, level: f32) {
    let db = 20.0 * level.max(1e-6).log10();
    let (text, col) = if db <= -59.5 {
        ("-∞ dB".to_string(), DIM)
    } else if db > -0.1 {
        ("CLIP".to_string(), ON_AIR)
    } else if db > -3.0 {
        (format!("{db:+.1}"), ON_AIR)
    } else if db > -12.0 {
        (format!("{db:+.1}"), Color32::from_rgb(245, 198, 48))
    } else {
        (format!("{db:+.1}"), OK_GREEN)
    };
    let (rect, _) = ui.allocate_exact_size(vec2(42.0, 14.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, 2.5, PANEL);
    ui.painter().rect_stroke(
        rect,
        2.5,
        Stroke::new(1.0, mix(BORDER, WELL, 0.4)),
        StrokeKind::Inside,
    );
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        text,
        FontId::monospace(9.0),
        col,
    );
}

/// Ableton-Live-12-inspirierter bipolarer Arc-Drehregler für Panorama (`-1.0..=1.0`).
/// Ziehen ändert den Wert, Doppelklick stellt ihn auf Mitte (`0.0`).
pub fn pan_knob(ui: &mut egui::Ui, pan: &mut f64, color: Color32) -> egui::Response {
    let (rect, mut resp) = ui.allocate_exact_size(vec2(62.0, 22.0), egui::Sense::click_and_drag());
    if resp.double_clicked() {
        *pan = 0.0;
        resp.mark_changed();
    } else if resp.dragged() {
        let delta = (resp.drag_delta().x - resp.drag_delta().y) * 0.008;
        let next = (*pan + f64::from(delta)).clamp(-1.0, 1.0);
        if (next - *pan).abs() > 1e-6 {
            *pan = next;
            resp.mark_changed();
        }
    }

    let hot = resp.hovered() || resp.dragged();
    let painter = ui.painter();
    painter.rect_filled(rect, 4.0, if hot { mix(WELL, SURFACE, 0.35) } else { WELL });
    painter.rect_stroke(
        rect,
        4.0,
        Stroke::new(1.0, if hot { color } else { BORDER }),
        StrokeKind::Inside,
    );

    // 270°-Arc-Ring links im Widget
    let center = egui::pos2(rect.left() + 11.5, rect.center().y);
    let radius = 7.2f32;
    let start_ang = -std::f32::consts::FRAC_PI_2 - 0.75 * std::f32::consts::PI;
    let end_ang = -std::f32::consts::FRAC_PI_2 + 0.75 * std::f32::consts::PI;
    let steps = 18usize;
    let bg_pts: Vec<egui::Pos2> = (0..=steps)
        .map(|i| {
            let t = i as f32 / steps as f32;
            let a = start_ang + t * (end_ang - start_ang);
            egui::pos2(center.x + a.cos() * radius, center.y + a.sin() * radius)
        })
        .collect();
    painter.add(egui::Shape::line(
        bg_pts,
        Stroke::new(2.0, mix(BORDER, WELL, 0.25)),
    ));

    // Aktiver Bogen von 12 Uhr (-PI/2) bis zum aktuellen Winkel
    let mid_ang = -std::f32::consts::FRAC_PI_2;
    let val_ang = mid_ang + (*pan as f32).clamp(-1.0, 1.0) * 0.75 * std::f32::consts::PI;
    if (*pan).abs() > 0.01 {
        let (a0, a1) = if val_ang < mid_ang {
            (val_ang, mid_ang)
        } else {
            (mid_ang, val_ang)
        };
        let segs = 10usize;
        let arc_pts: Vec<egui::Pos2> = (0..=segs)
            .map(|i| {
                let t = i as f32 / segs as f32;
                let a = a0 + t * (a1 - a0);
                egui::pos2(center.x + a.cos() * radius, center.y + a.sin() * radius)
            })
            .collect();
        painter.add(egui::Shape::line(arc_pts, Stroke::new(2.2, color)));
    } else {
        painter.circle_filled(
            egui::pos2(center.x, center.y - radius),
            1.4,
            mix(color, Color32::WHITE, 0.3),
        );
    }

    // Zeigerlinie vom Zentrum zum Rand
    let tip = egui::pos2(
        center.x + val_ang.cos() * (radius - 0.5),
        center.y + val_ang.sin() * (radius - 0.5),
    );
    painter.line_segment([center, tip], Stroke::new(1.5, TEXT_BRIGHT));
    painter.circle_filled(center, 1.6, mix(SURFACE, TEXT_BRIGHT, 0.4));

    // Kompakte L/C/R-Anzeige rechts im Widget
    let iv = (*pan * 100.0).round() as i32;
    let txt = if iv == 0 {
        "Pan C".to_string()
    } else if iv < 0 {
        format!("L{}", -iv)
    } else {
        format!("R{iv}")
    };
    painter.text(
        egui::pos2(rect.left() + 23.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        txt,
        FontId::monospace(10.0),
        if iv == 0 { DIM } else { TEXT_BRIGHT },
    );
    resp
}

/// Kreisförmige Fortschritts-Torte (`◐`, Ableton-Live-Session-Stil) für Sende-Deck und Cart-Pads.
pub fn progress_pie(
    painter: &egui::Painter,
    center: egui::Pos2,
    radius: f32,
    progress: f32,
    color: Color32,
) {
    let p = progress.clamp(0.0, 1.0);
    painter.circle_filled(center, radius, WELL);
    painter.circle_stroke(center, radius, Stroke::new(1.0, mix(color, BORDER, 0.55)));
    if p <= 0.001 {
        return;
    }
    let steps = ((p * 24.0).ceil() as usize).clamp(3, 24);
    let start = -std::f32::consts::FRAC_PI_2;
    let sweep = p * std::f32::consts::TAU;
    let r_inner = (radius - 1.5).max(1.0);
    let fill_col = mix(color, WELL, 0.35);
    for i in 0..steps {
        let t0 = i as f32 / steps as f32;
        let t1 = (i + 1) as f32 / steps as f32;
        let a0 = start + t0 * sweep;
        let a1 = start + t1 * sweep;
        let p0 = egui::pos2(center.x + a0.cos() * r_inner, center.y + a0.sin() * r_inner);
        let p1 = egui::pos2(center.x + a1.cos() * r_inner, center.y + a1.sin() * r_inner);
        painter.add(egui::Shape::convex_polygon(
            vec![center, p0, p1],
            fill_col,
            Stroke::NONE,
        ));
    }
    let rim_pts: Vec<egui::Pos2> = (0..=steps)
        .map(|i| {
            let t = i as f32 / steps as f32;
            let a = start + t * sweep;
            egui::pos2(center.x + a.cos() * r_inner, center.y + a.sin() * r_inner)
        })
        .collect();
    painter.add(egui::Shape::line(rim_pts, Stroke::new(1.5, color)));
}

/// Automatisches Spur-Piktogramm (GarageBand-Stil) anhand von Spur-Typ, Name und Eingangsquelle.
pub fn track_icon(
    kind: woa_model::TrackKind,
    name: &str,
    has_source: bool,
) -> (&'static str, &'static str) {
    match kind {
        woa_model::TrackKind::Midi => ("🎹", "MIDI-Instrument / Synthesizer-Spur"),
        woa_model::TrackKind::Bus => ("🎚", "Gruppen- / Effekt-Bus"),
        woa_model::TrackKind::Audio => {
            let low = name.to_lowercase();
            if has_source
                || low.contains("mik")
                || low.contains("mic")
                || low.contains("mod")
                || low.contains("stimme")
                || low.contains("sprach")
                || low.contains("voice")
                || low.contains("talk")
                || low.contains("host")
                || low.contains("gast")
            {
                ("🎙", "Sprach- / Mikrofon-Spur")
            } else if low.contains("jing")
                || low.contains("sfx")
                || low.contains("cart")
                || low.contains("drop")
                || low.contains("intro")
                || low.contains("outro")
            {
                ("🔔", "Jingle- / Soundeffekt-Spur")
            } else if low.contains("drum")
                || low.contains("beat")
                || low.contains("kick")
                || low.contains("perc")
            {
                ("🥁", "Drum- / Rhythmus-Spur")
            } else {
                ("🎵", "Musik- / Audio-Spur")
            }
        }
    }
}

/// Setzt Farben, Abstände und Schriftgrößen.
pub fn apply(ctx: &egui::Context) {
    // Helles Thema nur zur Probe (`WOA_THEME=hell`): Fenster und Piano-Roll folgen ihm,
    // die Zeitleiste bleibt dunkel.
    if std::env::var("WOA_THEME").is_ok_and(|t| t == "hell" || t == "light") {
        ctx.set_theme(egui::Theme::Light);
        return;
    }
    ctx.set_theme(egui::Theme::Dark);
    ctx.style_mut_of(egui::Theme::Dark, |s| {
        let v = &mut s.visuals;
        v.override_text_color = Some(TEXT_BRIGHT);
        v.panel_fill = PANEL;
        v.window_fill = PANEL;
        v.extreme_bg_color = WELL;
        v.faint_bg_color = SURFACE;
        v.selection.bg_fill = mix(ACCENT, BG, 0.40);
        v.selection.stroke = Stroke::new(1.0, ACCENT);
        v.hyperlink_color = ACCENT;
        v.window_corner_radius = CornerRadius::same(8);
        v.window_stroke = Stroke::new(1.0, BORDER);
        v.menu_corner_radius = CornerRadius::same(6);
        let r = CornerRadius::same(4);
        for (w, fill, stroke) in [
            (
                &mut v.widgets.noninteractive,
                PANEL,
                Stroke::new(1.0, BORDER),
            ),
            (&mut v.widgets.inactive, SURFACE, Stroke::new(1.0, BORDER)),
            (
                &mut v.widgets.hovered,
                mix(SURFACE, Color32::WHITE, 0.10),
                Stroke::new(1.0, mix(BORDER, ACCENT, 0.55)),
            ),
            (
                &mut v.widgets.active,
                mix(SURFACE, ACCENT, 0.28),
                Stroke::new(1.0, ACCENT),
            ),
            (
                &mut v.widgets.open,
                mix(SURFACE, Color32::WHITE, 0.07),
                Stroke::new(1.0, mix(BORDER, ACCENT, 0.4)),
            ),
        ] {
            w.corner_radius = r;
            w.weak_bg_fill = fill;
            w.bg_fill = fill;
            w.bg_stroke = stroke;
        }
        v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT_BRIGHT);
        v.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT_BRIGHT);
        v.widgets.hovered.fg_stroke = Stroke::new(1.0, Color32::WHITE);
        v.widgets.active.fg_stroke = Stroke::new(1.0, Color32::WHITE);

        s.spacing.item_spacing = vec2(8.0, 6.0);
        s.spacing.button_padding = vec2(10.0, 5.0);
        s.spacing.interact_size.y = 26.0;
        s.spacing.slider_width = 110.0;
        s.visuals.slider_trailing_fill = true;
        s.text_styles
            .insert(TextStyle::Body, FontId::proportional(13.5));
        s.text_styles
            .insert(TextStyle::Button, FontId::proportional(13.5));
        s.text_styles
            .insert(TextStyle::Monospace, FontId::monospace(13.5));
        s.text_styles
            .insert(TextStyle::Small, FontId::proportional(11.0));
        s.text_styles
            .insert(TextStyle::Heading, FontId::proportional(16.0));
    });
}
