//! Aussehen: Themen Hell und Dunkel, Modusfarbe je Layout (Produzent, Radio, Party).
//!
//! Gestaltungsvorgabe ist `docs/design/woa-design-system/` (`tokens.json`). Die Werte stehen hier
//! als Palette je Kombination aus Thema und Modus (sechs Stück); ein Test liest `tokens.json` und
//! prüft, dass beides übereinstimmt. Alle Farben holt der Code über die Funktionen `accent()`,
//! `well()` usw.; sie liefern die Farbe der gerade aktiven Palette ([`apply`] stellt sie ein).
//! Die Oberfläche läuft in einem Thread, die Umschaltung ist ein einzelner atomarer Wert.
// Der Token-Satz ist vollständig, auch wo noch nicht jeder Token verdrahtet ist.
#![allow(dead_code)]

use std::sync::atomic::{AtomicUsize, Ordering};

use eframe::egui;
use egui::{
    Color32, CornerRadius, FontId, Frame, Margin, Rect, Stroke, StrokeKind, TextStyle, vec2,
};
use serde::{Deserialize, Serialize};

/// Farbthema, eine Einstellung des Nutzers (Vorgabe: Dunkel).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    Hell,
    #[default]
    Dunkel,
}

impl Theme {
    pub fn label(self) -> &'static str {
        match self {
            Theme::Hell => "Hell",
            Theme::Dunkel => "Dunkel",
        }
    }
}

/// Modus des aktiven Layouts; bestimmt die Modusfarbe (`accent`, `accent_2`, `lcd_ink`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Mode {
    #[default]
    Studio,
    Radio,
    Party,
}

impl From<crate::feedback::Mode> for Mode {
    fn from(m: crate::feedback::Mode) -> Self {
        match m {
            crate::feedback::Mode::Produzent => Mode::Studio,
            crate::feedback::Mode::Radio => Mode::Radio,
            crate::feedback::Mode::Party => Mode::Party,
        }
    }
}

// Abstände und Radien aus `tokens.json` (space-*, radius-*), in Punkten.
pub const SPACE_1: f32 = 2.0;
pub const SPACE_2: f32 = 4.0;
pub const SPACE_3: f32 = 8.0;
pub const SPACE_4: f32 = 12.0;
pub const SPACE_5: f32 = 16.0;
pub const SPACE_6: f32 = 24.0;
pub const SPACE_7: f32 = 40.0;
pub const RADIUS_XS: u8 = 1;
pub const RADIUS_SM: u8 = 3;
pub const RADIUS_MD: u8 = 4;
pub const RADIUS_LG: u8 = 6;
pub const RADIUS_XL: u8 = 8;
pub const RADIUS_PARTY: u8 = 14;

/// Alle Farben einer Kombination aus Thema und Modus (Namen wie in `tokens.json`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    pub bg: Color32,
    pub well: Color32,
    pub panel: Color32,
    pub panel_header: Color32,
    pub surface: Color32,
    pub surface_hover: Color32,
    pub border: Color32,
    pub border_strong: Color32,
    pub grid: Color32,
    pub lcd_bg: Color32,
    pub led_off: Color32,
    pub text: Color32,
    pub text_dim: Color32,
    pub lcd_text: Color32,
    pub lcd_ink: Color32,
    pub accent: Color32,
    pub accent_strong: Color32,
    pub accent_soft: Color32,
    pub on_accent: Color32,
    pub accent_2: Color32,
    pub on_accent_2: Color32,
    pub on_air: Color32,
    pub on_air_fill: Color32,
    pub ok: Color32,
    pub cue: Color32,
    pub warn: Color32,
    pub led_green: Color32,
    pub led_yellow: Color32,
    pub led_red: Color32,
    pub vu_face: Color32,
    pub vu_ink: Color32,
    pub focus: Color32,
    pub mode_studio: Color32,
    pub mode_radio: Color32,
    pub mode_party: Color32,
    /// Spurfarben 1 bis 6.
    pub track: [Color32; 6],
}

impl Palette {
    /// Alle Tokens mit ihrem Namen aus `tokens.json` (Spurfarben als `track-1` bis `track-6`).
    pub fn named(&self) -> Vec<(String, Color32)> {
        let mut v: Vec<(String, Color32)> = [
            ("bg", self.bg),
            ("well", self.well),
            ("panel", self.panel),
            ("panel-header", self.panel_header),
            ("surface", self.surface),
            ("surface-hover", self.surface_hover),
            ("border", self.border),
            ("border-strong", self.border_strong),
            ("grid", self.grid),
            ("lcd-bg", self.lcd_bg),
            ("led-off", self.led_off),
            ("text", self.text),
            ("text-dim", self.text_dim),
            ("lcd-text", self.lcd_text),
            ("lcd-ink", self.lcd_ink),
            ("accent", self.accent),
            ("accent-strong", self.accent_strong),
            ("accent-soft", self.accent_soft),
            ("on-accent", self.on_accent),
            ("accent-2", self.accent_2),
            ("on-accent-2", self.on_accent_2),
            ("on-air", self.on_air),
            ("on-air-fill", self.on_air_fill),
            ("ok", self.ok),
            ("cue", self.cue),
            ("warn", self.warn),
            ("led-green", self.led_green),
            ("led-yellow", self.led_yellow),
            ("led-red", self.led_red),
            ("vu-face", self.vu_face),
            ("vu-ink", self.vu_ink),
            ("focus", self.focus),
            ("mode-studio", self.mode_studio),
            ("mode-radio", self.mode_radio),
            ("mode-party", self.mode_party),
        ]
        .into_iter()
        .map(|(n, c)| (n.to_string(), c))
        .collect();
        for (i, c) in self.track.iter().enumerate() {
            v.push((format!("track-{}", i + 1), *c));
        }
        v
    }
}

const fn c(rgb: u32) -> Color32 {
    Color32::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

/// Reihenfolge wie `color.themes` in `tokens.json`: Studio, Radio, Party, dann dieselben hell.
pub static PALETTES: [Palette; 6] = [
    // studio
    Palette {
        bg: c(0x12141a),
        well: c(0x0c0e12),
        panel: c(0x1b1e26),
        panel_header: c(0x222630),
        surface: c(0x292e3a),
        surface_hover: c(0x343a48),
        border: c(0x383f4e),
        border_strong: c(0x6a7488),
        grid: c(0x2c313d),
        lcd_bg: c(0x0a0c10),
        led_off: c(0x1c2028),
        text: c(0xecf0f8),
        text_dim: c(0x8a94a6),
        lcd_text: c(0xd8e0ec),
        lcd_ink: c(0xff9838),
        accent: c(0xff9838),
        accent_strong: c(0xffb46a),
        accent_soft: c(0x4a3018),
        on_accent: c(0x1a0e04),
        accent_2: c(0x5ec8ff),
        on_accent_2: c(0x04131c),
        on_air: c(0xee3844),
        on_air_fill: c(0xc41d2a),
        ok: c(0x38d070),
        cue: c(0x44baff),
        warn: c(0xf5c630),
        led_green: c(0x38d070),
        led_yellow: c(0xf5c630),
        led_red: c(0xee3844),
        vu_face: c(0xf2e2bb),
        vu_ink: c(0x2a1d0e),
        focus: c(0x9fd8ff),
        mode_studio: c(0xff9838),
        mode_radio: c(0xff4650),
        mode_party: c(0xff3da6),
        track: [
            c(0x4aa4ff),
            c(0x4cd48a),
            c(0xc07aff),
            c(0xffc044),
            c(0x40d6e8),
            c(0xff709e),
        ],
    },
    // radio
    Palette {
        bg: c(0x14171c),
        well: c(0x0d1014),
        panel: c(0x1e232a),
        panel_header: c(0x262c34),
        surface: c(0x2f363f),
        surface_hover: c(0x3a424c),
        border: c(0x3e4652),
        border_strong: c(0x6e7886),
        grid: c(0x2f353e),
        lcd_bg: c(0x0b0d10),
        led_off: c(0x1d2228),
        text: c(0xedf1f6),
        text_dim: c(0x8f99a8),
        lcd_text: c(0xd8e0ec),
        lcd_ink: c(0xff4650),
        accent: c(0xff4650),
        accent_strong: c(0xff747c),
        accent_soft: c(0x4a1c22),
        on_accent: c(0x1a0507),
        accent_2: c(0xffc83a),
        on_accent_2: c(0x1a1300),
        on_air: c(0xee3844),
        on_air_fill: c(0xc41d2a),
        ok: c(0x38d070),
        cue: c(0x44baff),
        warn: c(0xf5c630),
        led_green: c(0x38d070),
        led_yellow: c(0xf5c630),
        led_red: c(0xee3844),
        vu_face: c(0xf2e2bb),
        vu_ink: c(0x2a1d0e),
        focus: c(0x9fd8ff),
        mode_studio: c(0xff9838),
        mode_radio: c(0xff4650),
        mode_party: c(0xff3da6),
        track: [
            c(0x4aa4ff),
            c(0x4cd48a),
            c(0xc07aff),
            c(0xffc044),
            c(0x40d6e8),
            c(0xff709e),
        ],
    },
    // party
    Palette {
        bg: c(0x0e0c14),
        well: c(0x09080d),
        panel: c(0x18151f),
        panel_header: c(0x211c2a),
        surface: c(0x2a2435),
        surface_hover: c(0x352d43),
        border: c(0x3b3348),
        border_strong: c(0x76698a),
        grid: c(0x2a2433),
        lcd_bg: c(0x08070c),
        led_off: c(0x1c1824),
        text: c(0xf3eefa),
        text_dim: c(0x9c92ae),
        lcd_text: c(0xe4dcf0),
        lcd_ink: c(0xff3da6),
        accent: c(0xff3da6),
        accent_strong: c(0xff6fbe),
        accent_soft: c(0x4a1636),
        on_accent: c(0x1a0410),
        accent_2: c(0xc6ff3a),
        on_accent_2: c(0x0e1400),
        on_air: c(0xee3844),
        on_air_fill: c(0xc41d2a),
        ok: c(0x38d070),
        cue: c(0x44baff),
        warn: c(0xf5c630),
        led_green: c(0x38d070),
        led_yellow: c(0xf5c630),
        led_red: c(0xee3844),
        vu_face: c(0xf2e2bb),
        vu_ink: c(0x2a1d0e),
        focus: c(0x9fd8ff),
        mode_studio: c(0xff9838),
        mode_radio: c(0xff4650),
        mode_party: c(0xff3da6),
        track: [
            c(0x4aa4ff),
            c(0x4cd48a),
            c(0xc07aff),
            c(0xffc044),
            c(0x40d6e8),
            c(0xff709e),
        ],
    },
    // studio-hell
    Palette {
        bg: c(0xeef0f4),
        well: c(0xe1e5eb),
        panel: c(0xffffff),
        panel_header: c(0xe6e9ef),
        surface: c(0xdde1e8),
        surface_hover: c(0xd0d5de),
        border: c(0xc4cad4),
        border_strong: c(0x7d8696),
        grid: c(0xd5dae2),
        lcd_bg: c(0x0a0c10),
        led_off: c(0x1c2028),
        text: c(0x161a22),
        text_dim: c(0x545d6c),
        lcd_text: c(0xd8e0ec),
        lcd_ink: c(0xff9838),
        accent: c(0xa84e00),
        accent_strong: c(0x8e4200),
        accent_soft: c(0xfbe3cc),
        on_accent: c(0xffffff),
        accent_2: c(0x0a679c),
        on_accent_2: c(0xffffff),
        on_air: c(0xc8202e),
        on_air_fill: c(0xc41d2a),
        ok: c(0x137a3c),
        cue: c(0x0b6aa8),
        warn: c(0x8a6400),
        led_green: c(0x38d070),
        led_yellow: c(0xf5c630),
        led_red: c(0xee3844),
        vu_face: c(0xf2e2bb),
        vu_ink: c(0x2a1d0e),
        focus: c(0x0b5d94),
        mode_studio: c(0xa84e00),
        mode_radio: c(0xc41b2d),
        mode_party: c(0xb5146d),
        track: [
            c(0x1d64b8),
            c(0x17804a),
            c(0x7a3bc4),
            c(0x8d6200),
            c(0x0d7c8a),
            c(0xb8245a),
        ],
    },
    // radio-hell
    Palette {
        bg: c(0xeef0f3),
        well: c(0xe0e4e9),
        panel: c(0xffffff),
        panel_header: c(0xe5e8ed),
        surface: c(0xdbdfe5),
        surface_hover: c(0xced3db),
        border: c(0xc3c9d2),
        border_strong: c(0x7b8594),
        grid: c(0xd4d9e0),
        lcd_bg: c(0x0b0d10),
        led_off: c(0x1d2228),
        text: c(0x161a21),
        text_dim: c(0x535c6a),
        lcd_text: c(0xd8e0ec),
        lcd_ink: c(0xff4650),
        accent: c(0xc41b2d),
        accent_strong: c(0x9c1221),
        accent_soft: c(0xf9d6da),
        on_accent: c(0xffffff),
        accent_2: c(0x7d5900),
        on_accent_2: c(0xffffff),
        on_air: c(0xc8202e),
        on_air_fill: c(0xc41d2a),
        ok: c(0x137a3c),
        cue: c(0x0b6aa8),
        warn: c(0x8a6400),
        led_green: c(0x38d070),
        led_yellow: c(0xf5c630),
        led_red: c(0xee3844),
        vu_face: c(0xf2e2bb),
        vu_ink: c(0x2a1d0e),
        focus: c(0x0b5d94),
        mode_studio: c(0xa84e00),
        mode_radio: c(0xc41b2d),
        mode_party: c(0xb5146d),
        track: [
            c(0x1d64b8),
            c(0x17804a),
            c(0x7a3bc4),
            c(0x8d6200),
            c(0x0d7c8a),
            c(0xb8245a),
        ],
    },
    // party-hell
    Palette {
        bg: c(0xf2eff5),
        well: c(0xe7e2ec),
        panel: c(0xffffff),
        panel_header: c(0xece7f1),
        surface: c(0xe4dee9),
        surface_hover: c(0xd9d1e0),
        border: c(0xcdc4d6),
        border_strong: c(0x86799a),
        grid: c(0xddd6e4),
        lcd_bg: c(0x08070c),
        led_off: c(0x1c1824),
        text: c(0x1b1624),
        text_dim: c(0x5c536b),
        lcd_text: c(0xe4dcf0),
        lcd_ink: c(0xff3da6),
        accent: c(0xb5146d),
        accent_strong: c(0x900f56),
        accent_soft: c(0xf8d3e8),
        on_accent: c(0xffffff),
        accent_2: c(0x3f6600),
        on_accent_2: c(0xffffff),
        on_air: c(0xc8202e),
        on_air_fill: c(0xc41d2a),
        ok: c(0x137a3c),
        cue: c(0x0b6aa8),
        warn: c(0x8a6400),
        led_green: c(0x38d070),
        led_yellow: c(0xf5c630),
        led_red: c(0xee3844),
        vu_face: c(0xf2e2bb),
        vu_ink: c(0x2a1d0e),
        focus: c(0x0b5d94),
        mode_studio: c(0xa84e00),
        mode_radio: c(0xc41b2d),
        mode_party: c(0xb5146d),
        track: [
            c(0x1d64b8),
            c(0x17804a),
            c(0x7a3bc4),
            c(0x8d6200),
            c(0x0d7c8a),
            c(0xb8245a),
        ],
    },
];

/// Index der aktiven Palette in [`PALETTES`].
static CURRENT: AtomicUsize = AtomicUsize::new(0);

/// Index in [`PALETTES`] für Thema und Modus.
pub fn palette_index(theme: Theme, mode: Mode) -> usize {
    let base = match mode {
        Mode::Studio => 0,
        Mode::Radio => 1,
        Mode::Party => 2,
    };
    base + if theme == Theme::Hell { 3 } else { 0 }
}

/// Palette für Thema und Modus.
pub fn palette_for(theme: Theme, mode: Mode) -> &'static Palette {
    &PALETTES[palette_index(theme, mode)]
}

/// Die gerade aktive Palette.
pub fn pal() -> &'static Palette {
    &PALETTES[CURRENT.load(Ordering::Relaxed).min(PALETTES.len() - 1)]
}

/// Läuft gerade ein dunkles Thema?
pub fn is_dark() -> bool {
    CURRENT.load(Ordering::Relaxed) < 3
}

macro_rules! getters {
    ($($name:ident),* $(,)?) => {
        $(
            #[doc = concat!("Token `", stringify!($name), "` der aktiven Palette.")]
            pub fn $name() -> Color32 {
                pal().$name
            }
        )*
    };
}

getters!(
    bg,
    well,
    panel,
    panel_header,
    surface,
    surface_hover,
    border,
    border_strong,
    grid,
    lcd_bg,
    led_off,
    text,
    text_dim,
    lcd_text,
    lcd_ink,
    accent,
    accent_strong,
    accent_soft,
    on_accent,
    accent_2,
    on_accent_2,
    on_air,
    on_air_fill,
    ok,
    cue,
    warn,
    led_green,
    led_yellow,
    led_red,
    vu_face,
    vu_ink,
    focus,
    mode_studio,
    mode_radio,
    mode_party
);

/// App-Icon (Kachel aus dem Designsystem) als Fensterbild; `None`, wenn das PNG nicht lesbar ist.
pub fn app_icon() -> Option<egui::IconData> {
    const PNG: &[u8] = include_bytes!("../../../packaging/linux/icons/de.wironair.woa.png");
    let img = image::load_from_memory_with_format(PNG, image::ImageFormat::Png)
        .ok()?
        .into_rgba8();
    let (width, height) = img.dimensions();
    Some(egui::IconData {
        rgba: img.into_raw(),
        width,
        height,
    })
}

/// Bildmarke in der Kopfleiste: fünf LED-Säulen, deren Oberkanten ein W zeichnen, darüber die
/// ON-AIR-Lampe. Die Säulen nehmen die Modusfarbe an, die Lampe bleibt Rot (Logos/README).
/// `height` ist die Höhe der Fläche; die Marke ist quadratisch (viewBox 184 x 184).
pub fn logo_mark(ui: &mut egui::Ui, height: f32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(height, height), egui::Sense::hover());
    if ui.is_rect_visible(rect) {
        let k = height / 184.0;
        let p = |x: f32, y: f32| {
            egui::Pos2::new(rect.min.x + (x - 36.0) * k, rect.min.y + (y - 28.0) * k)
        };
        let painter = ui.painter();
        // Säulen-x, Anzahl Segmente (von unten, Raster 20, unterste Oberkante y=190).
        let cols: [(f32, u32); 5] = [(44.0, 7), (80.0, 3), (116.0, 5), (152.0, 3), (188.0, 7)];
        let base = accent();
        let cap = emphasize(base, 0.35);
        for (x, n) in cols {
            for i in 0..n {
                let y = 190.0 - 20.0 * i as f32;
                let color = if i + 1 == n { cap } else { base };
                painter.rect_filled(
                    Rect::from_min_max(p(x, y), p(x + 24.0, y + 14.0)),
                    3.0 * k,
                    color,
                );
            }
        }
        let lamp = p(128.0, 54.0);
        painter.circle_filled(lamp, 22.0 * k, on_air().gamma_multiply(0.22));
        painter.circle_filled(lamp, 15.0 * k, on_air());
    }
    resp
}

/// Farbe der Spur Nummer `i` (wiederholt sich nach sechs).
pub fn track_color(i: usize) -> Color32 {
    let t = &pal().track;
    t[i % t.len()]
}

/// Mischt `c` mit `with` (0 = c, 1 = with).
pub fn mix(c: Color32, with: Color32, t: f32) -> Color32 {
    let m = |a: u8, b: u8| (f32::from(a) + (f32::from(b) - f32::from(a)) * t).round() as u8;
    Color32::from_rgb(m(c.r(), with.r()), m(c.g(), with.g()), m(c.b(), with.b()))
}

/// Anteil der Spurfarbe in der Fläche einer gewählten Spur (Design `SelectionSync`: 14 %).
pub const SELECT_TINT: f32 = 0.14;

/// Fläche einer gewählten Spur: Spurfarbe 14 % auf `panel`, gleich in Spurkopf, Kanalzug und
/// Rack-Kopf.
pub fn selection_fill(track: Color32) -> Color32 {
    mix(panel(), track, SELECT_TINT)
}

/// Rahmen einer gewählten Spur: 1 px in Spurfarbe (nie `accent`).
pub fn selection_stroke(track: Color32) -> Stroke {
    Stroke::new(1.0, track)
}

/// Tooltips erscheinen nach 500 ms (Design `Tooltip`).
pub const TOOLTIP_DELAY: f32 = 0.5;

/// Hebt `c` hervor (Glanzlicht, Spitzen, Umriss): im dunklen Thema Richtung Weiß, im hellen
/// Richtung Schwarz, damit es sich auf hellen Flächen noch abhebt.
pub fn emphasize(c: Color32, t: f32) -> Color32 {
    if is_dark() {
        mix(c, Color32::WHITE, t)
    } else {
        mix(c, Color32::BLACK, t * 0.6)
    }
}

/// Eingefasster Studio-Modul-Rahmen (Karte mit 1px-Bevel).
pub fn card_frame() -> Frame {
    Frame::new()
        .fill(panel())
        .stroke(Stroke::new(1.0, border()))
        .corner_radius(CornerRadius::same(RADIUS_LG))
        .inner_margin(Margin::symmetric(SPACE_4 as i8, 10))
}

/// LCD-/Display-Panel für Uhren, Zähler und Messwerte. Bleibt in beiden Themen dunkel
/// (echte Hardware); Ziffern darin in `lcd_ink()`, Schrift in `lcd_text()`.
pub fn lcd_frame() -> Frame {
    Frame::new()
        .fill(lcd_bg())
        .stroke(Stroke::new(1.0, border()))
        .corner_radius(CornerRadius::same(5))
        .inner_margin(Margin::symmetric(12, 8))
}

/// Zeichnet ein kompaktes, beleuchtetes Status-Badge (LED-Pille).
pub fn status_pill(ui: &mut egui::Ui, label: &str, color: Color32, active: bool) {
    let dark = is_dark();
    let fill = if active {
        mix(color, well(), if dark { 0.72 } else { 0.80 })
    } else {
        well()
    };
    let stroke_col = if active {
        if dark {
            mix(color, Color32::WHITE, 0.15)
        } else {
            color
        }
    } else {
        border()
    };
    let text_col = if active {
        if dark {
            mix(color, Color32::WHITE, 0.35)
        } else {
            mix(color, Color32::BLACK, 0.2)
        }
    } else {
        text_dim()
    };
    Frame::new()
        .fill(fill)
        .stroke(Stroke::new(1.0, stroke_col))
        .corner_radius(CornerRadius::same(RADIUS_MD))
        .inner_margin(Margin::symmetric(7, 2))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 5.0;
            ui.horizontal(|ui| {
                let (dot, _) = ui.allocate_exact_size(vec2(7.0, 7.0), egui::Sense::hover());
                ui.painter().circle_filled(
                    dot.center(),
                    3.2,
                    if active {
                        color
                    } else {
                        mix(text_dim(), well(), 0.5)
                    },
                );
                ui.label(
                    egui::RichText::new(label)
                        .font(label_font(13.0))
                        .color(text_col),
                );
            });
        });
}

/// Farbe eines LED-Segments bei `seg_db` (Grün → Gelb → Rot), auf `lcd_bg`.
fn led_color(seg_db: f32) -> Color32 {
    use crate::widgets::vu::{LedZone, zone_for_db};
    match zone_for_db(seg_db) {
        LedZone::Red => led_red(),
        LedZone::Yellow => led_yellow(),
        LedZone::Green => led_green(),
    }
}

/// Zeichnet eine mehrgliedrige Studio-LED-Pegelanzeige (-60..0 dB, Grün → Gelb → Rot).
pub fn led_meter(painter: &egui::Painter, rect: Rect, level: f32) {
    painter.rect_filled(rect, 3.0, lcd_bg());
    painter.rect_stroke(
        rect,
        3.0,
        Stroke::new(1.0, mix(border(), lcd_bg(), 0.35)),
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
        let base_col = led_color(seg_frac * 60.0 - 60.0);
        let col = if i < lit_count && frac > 0.005 {
            base_col
        } else {
            mix(base_col, lcd_bg(), 0.86)
        };
        let x0 = inner.left() + i as f32 * (seg_w + gap);
        let seg_rect =
            Rect::from_min_size(egui::pos2(x0, inner.top()), vec2(seg_w, inner.height()));
        painter.rect_filled(seg_rect, 1.0, col);
    }
}

/// Zeichnet eine vertikale Studio-LED-Pegelanzeige (von unten nach oben, -60..0 dB) für den Mixer.
pub fn led_meter_vertical(painter: &egui::Painter, rect: Rect, level: f32) {
    painter.rect_filled(rect, 3.0, lcd_bg());
    painter.rect_stroke(
        rect,
        3.0,
        Stroke::new(1.0, mix(border(), lcd_bg(), 0.35)),
        StrokeKind::Inside,
    );
    let inner = rect.shrink2(vec2(2.0, 2.0));
    if inner.width() <= 2.0 || inner.height() <= 8.0 {
        return;
    }
    let db = 20.0 * level.max(1e-6).log10();
    let frac = ((db + 60.0) / 60.0).clamp(0.0, 1.0);
    let segments = ((inner.height() / 4.5).floor() as usize).clamp(10, 32);
    let gap = 1.5f32;
    let seg_h = ((inner.height() - gap * (segments as f32 - 1.0)) / segments as f32).max(1.5);
    let lit_count = (frac * segments as f32).ceil() as usize;

    for i in 0..segments {
        let seg_frac = (i + 1) as f32 / segments as f32;
        let base_col = led_color(seg_frac * 60.0 - 60.0);
        let col = if i < lit_count && frac > 0.005 {
            base_col
        } else {
            mix(base_col, lcd_bg(), 0.86)
        };
        let y0 = inner.bottom() - (i as f32 + 1.0) * seg_h - i as f32 * gap;
        let seg_rect =
            Rect::from_min_size(egui::pos2(inner.left(), y0), vec2(inner.width(), seg_h));
        painter.rect_filled(seg_rect, 1.0, col);
    }
}

/// Farbstufe eines Sende-Meters (#216): Rot über -6 dBFS, Gelb bis -6, Grün bis -18,
/// Blau darunter („zu leise“, unter [`BUS_METER_QUIET_DB`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BusZone {
    Blue,
    Green,
    Yellow,
    Red,
}

/// Unter diesem Pegel (dBFS) zeigt das Sende-Meter Blau: Signal zu leise.
pub const BUS_METER_QUIET_DB: f32 = -40.0;

pub fn bus_zone(db: f32) -> BusZone {
    if db > -6.0 {
        BusZone::Red
    } else if db > -18.0 {
        BusZone::Yellow
    } else if db > BUS_METER_QUIET_DB {
        BusZone::Green
    } else {
        BusZone::Blue
    }
}

fn bus_color(z: BusZone) -> Color32 {
    match z {
        BusZone::Blue => Color32::from_rgb(70, 140, 235),
        BusZone::Green => led_green(),
        BusZone::Yellow => led_yellow(),
        BusZone::Red => led_red(),
    }
}

/// Schmales senkrechtes LED-Meter der Sende-Fader (-60..0 dBFS) mit gehaltener Spitze `hold`
/// (linear, ein einzelnes Segment). Billig: nur Rechtecke, keine Texte.
pub fn bus_meter(painter: &egui::Painter, rect: Rect, level: f32, hold: f32) {
    painter.rect_filled(rect, 2.0, lcd_bg());
    let inner = rect.shrink(1.0);
    if inner.height() <= 8.0 {
        return;
    }
    let frac = |v: f32| ((20.0 * v.max(1e-6).log10() + 60.0) / 60.0).clamp(0.0, 1.0);
    let (lit, held) = (frac(level), frac(hold));
    let segments = ((inner.height() / 4.0).floor() as usize).clamp(10, 40);
    let gap = 1.0f32;
    let seg_h = (inner.height() - gap * (segments as f32 - 1.0)) / segments as f32;
    let lit_count = (lit * segments as f32).ceil() as usize;
    let hold_seg = (held * segments as f32).ceil() as usize;
    for i in 0..segments {
        let base = bus_color(bus_zone((i + 1) as f32 / segments as f32 * 60.0 - 60.0));
        let on = (i < lit_count && lit > 0.005) || (hold_seg > 0 && i + 1 == hold_seg);
        let col = if on { base } else { mix(base, lcd_bg(), 0.86) };
        let y0 = inner.bottom() - (i as f32 + 1.0) * seg_h - i as f32 * gap;
        painter.rect_filled(
            Rect::from_min_size(egui::pos2(inner.left(), y0), vec2(inner.width(), seg_h)),
            0.0,
            col,
        );
    }
}

/// Probe für Fotomodus und Tests: `WOA_THEME=hell` oder `dunkel` schlägt die Einstellung.
pub fn env_override() -> Option<Theme> {
    static V: std::sync::OnceLock<Option<Theme>> = std::sync::OnceLock::new();
    *V.get_or_init(|| match std::env::var("WOA_THEME").as_deref() {
        Ok("hell" | "light") => Some(Theme::Hell),
        Ok("dunkel" | "dark") => Some(Theme::Dunkel),
        _ => None,
    })
}

/// Rahmt ein LCD-/Display-Feld: wie [`lcd_frame`], und der Inhalt läuft auch im hellen Thema mit
/// der dunklen Palette des Modus (Stil und Farbfunktionen), weil LCDs wie echte Hardware immer
/// dunkel bleiben.
pub fn lcd_show<R>(
    ui: &mut egui::Ui,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::InnerResponse<R> {
    lcd_frame().show(ui, |ui| {
        if is_dark() {
            return add_contents(ui);
        }
        let saved = CURRENT.load(Ordering::Relaxed);
        // Hell → gleicher Modus dunkel (Index − 3).
        CURRENT.store(saved.saturating_sub(3), Ordering::Relaxed);
        let style = ui.ctx().style_of(egui::Theme::Dark);
        ui.set_style(style);
        let r = add_contents(ui);
        CURRENT.store(saved, Ordering::Relaxed);
        r
    })
}

/// Stellt Thema und Modus ein und setzt Farben, Abstände und Schriftgrößen von egui.
/// Beide egui-Themen bekommen den Stil zum Modus (das dunkle auch im hellen Thema, für LCD-Felder);
/// aktiv ist das gewählte. Nur bei Änderung von Thema oder Modus nötig.
pub fn apply(ctx: &egui::Context, theme: Theme, mode: Mode) {
    let _ = install_fonts(ctx);
    ctx.options_mut(|o| o.input_options.max_double_click_delay = DOUBLE_CLICK_DELAY);
    CURRENT.store(palette_index(theme, mode), Ordering::Relaxed);
    for (t, egui_theme) in [
        (Theme::Dunkel, egui::Theme::Dark),
        (Theme::Hell, egui::Theme::Light),
    ] {
        let p = palette_for(t, mode);
        ctx.style_mut_of(egui_theme, |s| style_for(s, p, t == Theme::Dunkel));
    }
    ctx.set_theme(if theme == Theme::Dunkel {
        egui::Theme::Dark
    } else {
        egui::Theme::Light
    });
}

fn style_for(s: &mut egui::Style, p: &Palette, dark: bool) {
    let v = &mut s.visuals;
    v.override_text_color = Some(p.text);
    v.panel_fill = p.panel;
    v.window_fill = p.panel;
    v.extreme_bg_color = p.well;
    v.faint_bg_color = p.surface;
    v.code_bg_color = p.well;
    v.error_fg_color = p.on_air;
    v.warn_fg_color = p.warn;
    v.selection.bg_fill = if dark {
        mix(p.accent, p.bg, 0.40)
    } else {
        p.accent_soft
    };
    v.selection.stroke = Stroke::new(1.0, p.accent);
    v.hyperlink_color = p.accent;
    v.window_corner_radius = CornerRadius::same(RADIUS_XL);
    v.window_stroke = Stroke::new(1.0, p.border);
    v.menu_corner_radius = CornerRadius::same(RADIUS_LG);
    let r = CornerRadius::same(RADIUS_MD);
    let (open_fill, active_fill) = if dark {
        (
            mix(p.surface, Color32::WHITE, 0.07),
            mix(p.surface, p.accent, 0.28),
        )
    } else {
        (p.surface_hover, p.accent_soft)
    };
    for (w, fill, stroke) in [
        (
            &mut v.widgets.noninteractive,
            p.panel,
            Stroke::new(1.0, p.border),
        ),
        (
            &mut v.widgets.inactive,
            p.surface,
            Stroke::new(1.0, p.border_strong),
        ),
        (
            &mut v.widgets.hovered,
            p.surface_hover,
            Stroke::new(1.0, mix(p.border_strong, p.accent, 0.55)),
        ),
        (
            &mut v.widgets.active,
            active_fill,
            Stroke::new(1.0, p.accent),
        ),
        (
            &mut v.widgets.open,
            open_fill,
            Stroke::new(1.0, mix(p.border_strong, p.accent, 0.4)),
        ),
    ] {
        w.corner_radius = r;
        w.weak_bg_fill = fill;
        w.bg_fill = fill;
        w.bg_stroke = stroke;
    }
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, p.text);
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, p.text);
    let strong = if dark { Color32::WHITE } else { p.text };
    v.widgets.hovered.fg_stroke = Stroke::new(1.0, strong);
    v.widgets.active.fg_stroke = Stroke::new(1.0, strong);
    v.slider_trailing_fill = true;
    s.interaction.tooltip_delay = TOOLTIP_DELAY;

    s.spacing.item_spacing = vec2(SPACE_3, 6.0);
    s.spacing.button_padding = vec2(10.0, 5.0);
    s.spacing.interact_size.y = 26.0;
    s.spacing.slider_width = 110.0;
    s.text_styles
        .insert(TextStyle::Body, FontId::proportional(TYPE_BODY));
    s.text_styles
        .insert(TextStyle::Button, FontId::proportional(TYPE_BODY));
    s.text_styles
        .insert(TextStyle::Monospace, FontId::monospace(TYPE_BODY));
    s.text_styles
        .insert(TextStyle::Small, FontId::proportional(TYPE_SMALL));
    s.text_styles
        .insert(TextStyle::Heading, label_font(TYPE_TITLE));
}

/// Typo-Skala des Design Systems (px): `body`, `small`, `title`.
pub const TYPE_BODY: f32 = 14.0;
pub const TYPE_SMALL: f32 = 12.0;
pub const TYPE_TITLE: f32 = 22.0;

/// Name der Familie für Pult-Beschriftungen (Barlow Condensed, `font-label`).
pub const FAMILY_LABEL: &str = "label";
/// Name der Familie für Leuchtziffern (DSEG7 Classic, `font-seg`).
pub const FAMILY_SEG: &str = "seg";
/// Familie der Phosphor-Symbole im Fill-Stil (Transport). Regular und Fill teilen sich dieselben
/// Codepunkte; deshalb liegt Fill in einer eigenen Familie und wird nur über [`fill_label`] benutzt.
pub const FAMILY_ICON_FILL: &str = "icon-fill";

/// Phosphor-Symbole im Fill-Stil, nur für [`fill_label`] und [`fill_icon`].
pub use egui_phosphor::fill as icf;
/// Phosphor-Symbole im Regular-Stil (Linie). Sie stehen als Rückfall in allen Textfamilien und
/// lassen sich direkt in Beschriftungen einsetzen: `format!("{} Text", theme::ic::TRASH)`.
pub use egui_phosphor::regular as ic;

/// Ein Fill-Symbol allein als `RichText` (Familie [`FAMILY_ICON_FILL`]).
pub fn fill_icon(glyph: &str, size: f32) -> egui::RichText {
    egui::RichText::new(glyph)
        .size(size)
        .family(egui::FontFamily::Name(FAMILY_ICON_FILL.into()))
}

/// Schrift für Fill-Symbole (Familie [`FAMILY_ICON_FILL`]), etwa für Beschriftungen von Tasten.
pub fn fill_icon_font(size: f32) -> FontId {
    FontId::new(size, egui::FontFamily::Name(FAMILY_ICON_FILL.into()))
}

/// Beschriftung aus einem Fill-Symbol und optionalem Text, für Knöpfe. Das Symbol kommt aus
/// [`icf`], der Text läuft in der Oberflächenschrift; beides in `color`.
pub fn fill_label(glyph: &str, text: &str, size: f32, color: Color32) -> egui::text::LayoutJob {
    use egui::text::{LayoutJob, TextFormat};
    let mut job = LayoutJob::default();
    job.append(
        glyph,
        0.0,
        TextFormat {
            font_id: FontId::new(size, egui::FontFamily::Name(FAMILY_ICON_FILL.into())),
            color,
            valign: egui::Align::Center,
            ..Default::default()
        },
    );
    if !text.is_empty() {
        job.append(
            text,
            size * 0.4,
            TextFormat {
                font_id: FontId::new(size, egui::FontFamily::Name("ui-bold".into())),
                color,
                valign: egui::Align::Center,
                ..Default::default()
            },
        );
    }
    job
}

/// Schrift für Pult-Beschriftungen und Titel (Barlow Condensed SemiBold).
pub fn label_font(size: f32) -> FontId {
    FontId::new(size, egui::FontFamily::Name(FAMILY_LABEL.into()))
}

/// Schrift für Leuchtziffern in LCDs und Uhren (DSEG7 Classic Bold, nur Ziffern und `:`).
pub fn seg_font(size: f32) -> FontId {
    FontId::new(size, egui::FontFamily::Name(FAMILY_SEG.into()))
}

/// Die eingebetteten Schriften (alle SIL OFL 1.1, Lizenztexte in `assets/fonts/`):
/// (Name, Daten, Familien in der Reihenfolge ihrer Wahl).
const FONT_FILES: &[(&str, &[u8])] = &[
    (
        "Barlow-Regular",
        include_bytes!("../assets/fonts/Barlow-Regular.ttf"),
    ),
    (
        "Barlow-Medium",
        include_bytes!("../assets/fonts/Barlow-Medium.ttf"),
    ),
    (
        "Barlow-SemiBold",
        include_bytes!("../assets/fonts/Barlow-SemiBold.ttf"),
    ),
    (
        "Barlow-Bold",
        include_bytes!("../assets/fonts/Barlow-Bold.ttf"),
    ),
    (
        "BarlowCondensed-Medium",
        include_bytes!("../assets/fonts/BarlowCondensed-Medium.ttf"),
    ),
    (
        "BarlowCondensed-SemiBold",
        include_bytes!("../assets/fonts/BarlowCondensed-SemiBold.ttf"),
    ),
    (
        "BarlowCondensed-Bold",
        include_bytes!("../assets/fonts/BarlowCondensed-Bold.ttf"),
    ),
    (
        "BarlowCondensed-ExtraBold",
        include_bytes!("../assets/fonts/BarlowCondensed-ExtraBold.ttf"),
    ),
    (
        "JetBrainsMono-Regular",
        include_bytes!("../assets/fonts/JetBrainsMono-Regular.ttf"),
    ),
    (
        "JetBrainsMono-Medium",
        include_bytes!("../assets/fonts/JetBrainsMono-Medium.ttf"),
    ),
    (
        "JetBrainsMono-Bold",
        include_bytes!("../assets/fonts/JetBrainsMono-Bold.ttf"),
    ),
    (
        "DSEG7Classic-Bold",
        include_bytes!("../assets/fonts/DSEG7Classic-Bold.ttf"),
    ),
];

/// Die Schriftdefinitionen: Barlow als Oberflächenschrift (`Proportional`), JetBrains Mono als
/// `Monospace`, Barlow Condensed als Familie `label`, DSEG7 als Familie `seg`. Die egui-
/// Standardschriften bleiben dahinter als Rückfall für Zeichen, die unsere nicht haben
/// (Pfeile, Symbole).
pub fn font_definitions() -> egui::FontDefinitions {
    use egui::{FontData, FontFamily};
    let mut fd = egui::FontDefinitions::default();
    for (name, bytes) in FONT_FILES {
        fd.font_data.insert(
            (*name).to_owned(),
            std::sync::Arc::new(FontData::from_static(bytes)),
        );
    }
    // Phosphor: Regular als Rückfall hinter der ersten Schrift jeder Textfamilie (wird unten
    // eingefügt), Fill als eigene Familie.
    fd.font_data.insert(
        "phosphor".to_owned(),
        std::sync::Arc::new(egui_phosphor::Variant::Regular.font_data()),
    );
    fd.font_data.insert(
        "phosphor-fill".to_owned(),
        std::sync::Arc::new(egui_phosphor::Variant::Fill.font_data()),
    );
    let fallback_prop = fd
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    let fallback_mono = fd
        .families
        .get(&FontFamily::Monospace)
        .cloned()
        .unwrap_or_default();
    let with = |first: &str, fallback: &[String]| {
        let mut v = vec![first.to_owned(), "phosphor".to_owned()];
        v.extend(fallback.iter().cloned());
        v
    };
    fd.families.insert(
        FontFamily::Proportional,
        with("Barlow-Regular", &fallback_prop),
    );
    fd.families.insert(
        FontFamily::Monospace,
        with("JetBrainsMono-Regular", &fallback_mono),
    );
    for (family, first) in [
        (FAMILY_LABEL, "BarlowCondensed-SemiBold"),
        (FAMILY_SEG, "DSEG7Classic-Bold"),
        ("ui-medium", "Barlow-Medium"),
        ("ui-semibold", "Barlow-SemiBold"),
        ("ui-bold", "Barlow-Bold"),
        ("label-medium", "BarlowCondensed-Medium"),
        ("label-bold", "BarlowCondensed-Bold"),
        ("label-extrabold", "BarlowCondensed-ExtraBold"),
        ("mono-medium", "JetBrainsMono-Medium"),
        ("mono-bold", "JetBrainsMono-Bold"),
    ] {
        // Die Leuchtziffern fallen auf Barlow zurück, nicht auf die Standard-Proportionalschrift.
        let mut v = with(first, &fallback_prop);
        if family == FAMILY_SEG {
            v.insert(1, "Barlow-Regular".to_owned());
        }
        fd.families.insert(FontFamily::Name(family.into()), v);
    }
    fd.families.insert(
        FontFamily::Name(FAMILY_ICON_FILL.into()),
        vec!["phosphor-fill".to_owned()],
    );
    fd
}

/// Höchster Abstand der Klicks eines Doppelklicks in Sekunden: so viel wie die Voreinstellung
/// von Windows (egui erlaubt nur 0,3 s, André 31.10.2026: „egal wo ich doppelt klicke“).
pub const DOUBLE_CLICK_DELAY: f64 = 0.5;

/// Doppelklick auf `r`. Auch der Dreifachklick zählt: wer erst wählt und kurz darauf
/// doppelklickt, erzeugt für egui einen Dreifachklick, und `double_clicked()` bliebe falsch.
pub fn double_clicked(r: &egui::Response) -> bool {
    r.double_clicked() || r.triple_clicked()
}

/// Kontext für Tests: Schriften gesetzt und einmal durchlaufen, sodass alle Familien (auch
/// `icon-fill`) beim Zeichnen schon gelten.
#[cfg(test)]
pub fn test_context() -> egui::Context {
    let ctx = egui::Context::default();
    install_fonts(&ctx);
    let _ = ctx.run_ui(egui::RawInput::default(), |_| {});
    ctx
}

/// Setzt die Schriften einmal je egui-Kontext. egui lädt sie erst im nächsten Durchlauf; wer
/// `label_font`/`seg_font` zeichnet, darf das also erst, wenn die Funktion `false` lieferte.
/// Liefert `true`, wenn sie gerade erst gesetzt wurden.
pub fn install_fonts(ctx: &egui::Context) -> bool {
    let id = egui::Id::new("woa_fonts_installed");
    if ctx.data(|d| d.get_temp::<bool>(id)).unwrap_or(false) {
        return false;
    }
    ctx.set_fonts(font_definitions());
    ctx.data_mut(|d| d.insert_temp(id, true));
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_icon_is_decodable_square() {
        let icon = app_icon().expect("Icon lesbar");
        assert_eq!(icon.width, icon.height);
        assert_eq!(icon.rgba.len(), (icon.width * icon.height * 4) as usize);
    }

    const TOKENS: &str = include_str!("../../../docs/design/woa-design-system/tokens.json");

    fn hex(s: &str) -> Color32 {
        let h = s.trim_start_matches('#');
        let v = u32::from_str_radix(h, 16).expect("hex color");
        Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
    }

    fn px(s: &str) -> f32 {
        s.trim_end_matches("px").parse().expect("px value")
    }

    #[test]
    fn palettes_match_tokens_json() {
        let json: serde_json::Value = serde_json::from_str(TOKENS).expect("tokens.json");
        let ids: Vec<&str> = json["color"]["themes"]
            .as_array()
            .expect("themes")
            .iter()
            .map(|t| t["id"].as_str().expect("id"))
            .collect();
        assert_eq!(ids.len(), PALETTES.len());
        let tokens = json["color"]["tokens"].as_array().expect("tokens");
        for (idx, id) in ids.iter().enumerate() {
            let named = PALETTES[idx].named();
            assert_eq!(
                named.len(),
                tokens.len(),
                "Zahl der Tokens in {id}: tokens.json und theme.rs weichen ab"
            );
            for t in tokens {
                let name = t["name"].as_str().expect("name");
                let want = hex(t["value"][id].as_str().expect("value"));
                let got = named
                    .iter()
                    .find(|(n, _)| n == name)
                    .unwrap_or_else(|| panic!("Token {name} fehlt in theme.rs"))
                    .1;
                assert_eq!(got, want, "Token {name} in {id}");
            }
        }
    }

    #[test]
    fn palette_order_follows_theme_ids() {
        let json: serde_json::Value = serde_json::from_str(TOKENS).expect("tokens.json");
        let ids: Vec<&str> = json["color"]["themes"]
            .as_array()
            .expect("themes")
            .iter()
            .map(|t| t["id"].as_str().expect("id"))
            .collect();
        for (theme, suffix) in [(Theme::Dunkel, ""), (Theme::Hell, "-hell")] {
            for (mode, name) in [
                (Mode::Studio, "studio"),
                (Mode::Radio, "radio"),
                (Mode::Party, "party"),
            ] {
                assert_eq!(ids[palette_index(theme, mode)], format!("{name}{suffix}"));
            }
        }
    }

    #[test]
    fn spacing_and_radius_match_tokens_json() {
        let json: serde_json::Value = serde_json::from_str(TOKENS).expect("tokens.json");
        let val = |group: &str, name: &str| -> f32 {
            let t = json[group]["tokens"]
                .as_array()
                .expect("group")
                .iter()
                .find(|t| t["name"] == name)
                .unwrap_or_else(|| panic!("{name} fehlt"));
            px(t["value"].as_str().expect("value"))
        };
        for (name, got) in [
            ("space-1", SPACE_1),
            ("space-2", SPACE_2),
            ("space-3", SPACE_3),
            ("space-4", SPACE_4),
            ("space-5", SPACE_5),
            ("space-6", SPACE_6),
            ("space-7", SPACE_7),
        ] {
            assert_eq!(val("spacing", name), got, "{name}");
        }
        for (name, got) in [
            ("radius-xs", RADIUS_XS),
            ("radius-sm", RADIUS_SM),
            ("radius-md", RADIUS_MD),
            ("radius-lg", RADIUS_LG),
            ("radius-xl", RADIUS_XL),
            ("radius-party", RADIUS_PARTY),
        ] {
            assert_eq!(val("radius", name), f32::from(got), "{name}");
        }
    }

    #[test]
    fn phosphor_icons_are_registered_and_render() {
        let fd = font_definitions();
        for fam in [
            egui::FontFamily::Proportional,
            egui::FontFamily::Monospace,
            egui::FontFamily::Name(FAMILY_LABEL.into()),
        ] {
            assert!(fd.families[&fam].iter().any(|f| f == "phosphor"), "{fam:?}");
        }
        assert_eq!(
            fd.families[&egui::FontFamily::Name(FAMILY_ICON_FILL.into())],
            vec!["phosphor-fill".to_owned()]
        );
        let ctx = egui::Context::default();
        install_fonts(&ctx);
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            let job = fill_label(icf::PLAY, "Play", 15.0, Color32::WHITE);
            let g = ui.painter().layout_job(job);
            assert!(g.size().x > 15.0);
            let icon = ui.painter().layout_no_wrap(
                ic::TRASH.to_owned(),
                FontId::proportional(14.0),
                Color32::WHITE,
            );
            assert!(icon.size().x > 4.0);
        });
    }

    #[test]
    fn fonts_register_all_families_and_render() {
        let fd = font_definitions();
        for fam in [
            FAMILY_LABEL,
            FAMILY_SEG,
            "ui-bold",
            "label-extrabold",
            "mono-bold",
        ] {
            assert!(
                fd.families
                    .contains_key(&egui::FontFamily::Name(fam.into())),
                "Familie {fam} fehlt"
            );
        }
        assert_eq!(
            fd.families[&egui::FontFamily::Proportional][0],
            "Barlow-Regular"
        );
        assert_eq!(
            fd.families[&egui::FontFamily::Monospace][0],
            "JetBrainsMono-Regular"
        );
        // Jede Familie muss in egui tatsächlich laden und Text mit Breite liefern.
        let ctx = egui::Context::default();
        assert!(install_fonts(&ctx));
        assert!(!install_fonts(&ctx));
        let _ = ctx.run_ui(egui::RawInput::default(), |_| {});
        let mut widths = Vec::new();
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            for font in [
                FontId::proportional(TYPE_BODY),
                FontId::monospace(TYPE_BODY),
                label_font(TYPE_TITLE),
                seg_font(40.0),
            ] {
                let g = ui
                    .painter()
                    .layout_no_wrap("88:88".into(), font, Color32::WHITE);
                widths.push(g.size().x);
            }
        });
        assert_eq!(widths.len(), 4);
        assert!(widths.iter().all(|w| *w > 5.0), "{widths:?}");
    }

    #[test]
    fn theme_serializes_lowercase_and_defaults_to_dark() {
        assert_eq!(Theme::default(), Theme::Dunkel);
        assert_eq!(serde_json::to_string(&Theme::Hell).unwrap(), "\"hell\"");
        assert_eq!(
            serde_json::from_str::<Theme>("\"dunkel\"").unwrap(),
            Theme::Dunkel
        );
    }

    #[test]
    fn mode_color_follows_layout() {
        let studio = palette_for(Theme::Dunkel, Mode::Studio);
        let radio = palette_for(Theme::Dunkel, Mode::Radio);
        assert_ne!(studio.accent, radio.accent);
        assert_eq!(studio.lcd_bg, palette_for(Theme::Hell, Mode::Studio).lcd_bg);
    }
}
