//! Radio-Layout (Sendeablauf): Playlist, große Uhr, Start/Weiter, Stopp, Cart-Wall,
//! Mikrofon mit Ducking, Mitschnitt. Alle Logik steckt in `woa_radio`; hier wird nur
//! gezeichnet und weitergereicht.

use crate::ordner::Modal;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime};

use eframe::egui;
use woa_io::controller::{Bus, DeckAction, Target};
use woa_radio::control::PflSource;
use woa_radio::mixer::LevelBus;
use woa_radio::station::{Station, View, ViewEntry};
use woa_radio::{CARTS, Controller, RadioShow};

use crate::widgets::fade::{FadeKey, FadeShape, fade_button};
use woa_stream::StreamStatus;
use woa_stream::link::{Feeder, is_alive, link_path};

use crate::cue_editor::{CueAction, CueEditor};
use crate::feedback::PflOutput;
use crate::theme;
use crate::voice_editor::{VoiceAction, VoiceEditor, VoiceInput};
use crate::xrun::XrunLog;
use woa_engine::localtime::Zone;

const NO_PFL: &str = "Kein Kopfhörer-Ausgang: Einstellungen › Audio-Geräte › Vorhören (PFL)";
/// Tooltip, wenn das Vorhören in den Einstellungen aus ist.
pub const PFL_OFF: &str = "Vorhören ist aus: Einstellungen › Audio-Geräte › Vorhören (PFL)";

/// Knopf „Vorhören“: läuft gerade diese Quelle, wird er zu „⏹ Vorhören stoppen“ (hervorgehoben),
/// ein Klick darauf stoppt (#205).
fn pfl_button(active: bool) -> egui::Button<'static> {
    if active {
        egui::Button::new(theme::fill_label(
            theme::icf::STOP,
            "Vorhören stoppen",
            14.0,
            theme::text(),
        ))
        .selected(true)
    } else {
        egui::Button::new("Vorhören")
    }
}

type Stream = Box<dyn std::any::Any>;

/// Tasten, die schon etwas anderes tun und keinem Cart gehören können.
/// Breite der Restzeit-Spalte eines Decks (Uhr und Fortschrittsbalken).
const DECK_CLOCK_WIDTH: f32 = 132.0;
/// Gleiche Höhe aller Deck-Zeilen, ob leer oder geladen (RadioDecks).
const DECK_ROW_H: f32 = 44.0;

/// Wohin eine gezogene Playlist-Zeile fällt (#166).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RowDrop {
    /// Auf Deck `k` laden.
    Deck(usize),
    /// Dem Cart-Pad `slot` zuweisen (#323).
    Cart(usize),
    /// In der Playlist an diese Stelle verschieben.
    Row(usize),
    /// Daneben oder auf sich selbst: nichts.
    Nothing,
}

/// Ziel einer von Zeile `from` gezogenen Playlist-Zeile, losgelassen bei `pos`.
fn row_drop_target(
    pos: egui::Pos2,
    from: usize,
    decks: &[egui::Rect],
    carts: &[egui::Rect],
    playlist: egui::Rect,
    rows: &[egui::Rect],
) -> RowDrop {
    if let Some(k) = decks.iter().position(|r| r.contains(pos)) {
        return RowDrop::Deck(k);
    }
    if let Some(slot) = carts.iter().position(|r| r.contains(pos)) {
        return RowDrop::Cart(slot);
    }
    if !playlist.contains(pos) {
        return RowDrop::Nothing;
    }
    match rows.iter().position(|r| r.contains(pos)) {
        Some(to) if to != from => RowDrop::Row(to),
        _ => RowDrop::Nothing,
    }
}

/// Anzahl gespielter Titel am Anfang der Playlist (vor dem laufenden bzw. nächsten).
/// Eingetippte Playlist-Nummer (#92, 1-basiert) als Zielindex, auf die Liste begrenzt.
/// Eine Zeile als eine Klickfläche (#302): Auswahl, Doppelklick, Rechtsklick-Menü und Ziehen
/// greifen an jeder Stelle der Zeile. Die Fläche liegt unter dem Inhalt; nur Knöpfe und Felder,
/// die selbst klicken (Nummer eintippen, x, ^, v, Start …), behalten den Klick für sich.
/// Beschriftungen in der Zeile sind nicht markierbar, sonst fingen sie den Klick ab.
pub(crate) fn row_area<R>(
    ui: &mut egui::Ui,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    sense: egui::Sense,
    frame: egui::Frame,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::InnerResponse<R> {
    ui.scope_builder(egui::UiBuilder::new().id_salt(id_salt).sense(sense), |ui| {
        ui.style_mut().interaction.selectable_labels = false;
        frame.show(ui, add).inner
    })
}

fn typed_position(text: &str, len: usize) -> Option<usize> {
    let n: usize = text.trim().parse().ok()?;
    (len > 0).then(|| n.clamp(1, len) - 1)
}

/// Breite der Hover-Knöpfe (hoch, runter, entfernen) am rechten Rand einer Playlist-Zeile.
const ROW_TOOLS_W: f32 = 90.0;
/// Spalte Dauer und Spalte Lautheit der Playlist.
const DUR_W: f32 = 48.0;
const LUFS_W: f32 = 52.0;
/// Platz für das Badge ON AIR / CUE vor der Lautheit.
const ROW_BADGE_W: f32 = 90.0;

/// Höhe der Haupttasten (Start, Stopp, Blende, Mikro, Telefon) und der Symboltasten.
const KEY_H: f32 = 44.0;
const SYMBOL_KEY: f32 = 32.0;

/// Haupttaste der Konsole (`woa-key`): neutral auf `surface`; an, dann Rand und Symbol in der
/// eigenen Farbe auf deren getönter Fläche. Das Wort steht in Versalien neben dem Symbol.
fn console_key(
    ui: &mut egui::Ui,
    icon: &str,
    word: &str,
    tone: egui::Color32,
    on: bool,
    h: f32,
) -> egui::Response {
    let ink = theme::text();
    let mut job = egui::text::LayoutJob::default();
    job.append(
        icon,
        0.0,
        egui::TextFormat {
            font_id: egui::FontId::new(
                16.0,
                egui::FontFamily::Name(theme::FAMILY_ICON_FILL.into()),
            ),
            color: if on { tone } else { theme::text_dim() },
            valign: egui::Align::Center,
            ..Default::default()
        },
    );
    job.append(
        &word.to_uppercase(),
        8.0,
        egui::TextFormat {
            font_id: theme::label_font(13.0),
            color: ink,
            extra_letter_spacing: 0.8,
            valign: egui::Align::Center,
            ..Default::default()
        },
    );
    let b = egui::Button::new(job)
        .fill(if on {
            theme::mix(tone, theme::surface(), 0.80)
        } else {
            theme::surface()
        })
        .stroke(egui::Stroke::new(
            1.0,
            if on { tone } else { theme::border_strong() },
        ))
        .corner_radius(theme::RADIUS_MD)
        .min_size(egui::vec2(96.0, h));
    let r = ui.add(b);
    let label = word.to_string();
    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label.clone()));
    r
}

/// Symboltaste 32 × 32 (`woa-key--icon`); `on` hebt sie in `accent` hervor. Text ohne
/// Symbolzeichen (−10, +10) steht in Mono.
fn symbol_key(ui: &mut egui::Ui, icon: &str, tip: &str, on: bool) -> egui::Response {
    icon_key(ui, icon, tip, theme::accent_2(), on, true, SYMBOL_KEY).on_hover_text(tip)
}

pub(crate) fn played_count(v: &View) -> usize {
    v.first_open.min(v.playlist.len())
}

/// Geister-Segmente zu `text` (`woa-seg`, `data-ghost`): jede Stelle wird 8, Trenner bleiben.
fn seg_ghost(text: &str) -> String {
    text.chars()
        .map(|c| if matches!(c, ':' | '.') { c } else { '8' })
        .collect()
}

/// 7-Segment-Zahl mit Geister-Segmenten dahinter (`woa-seg`): erst „88:88“ in `led_off`,
/// darüber die Ziffern in `color`.
fn seg_text(ui: &mut egui::Ui, text: &str, size: f32, color: egui::Color32) -> egui::Response {
    let font = theme::seg_font(size);
    let ghost = ui
        .painter()
        .layout_no_wrap(seg_ghost(text), font.clone(), theme::led_off());
    let (rect, r) = ui.allocate_exact_size(ghost.size(), egui::Sense::hover());
    let p = ui.painter();
    p.galley(rect.min, ghost, theme::led_off());
    p.text(rect.min, egui::Align2::LEFT_TOP, text, font, color);
    let label = text.to_string();
    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, label.clone()));
    r
}

/// Quadratische Symbol-Taste (`woa-key--sq`) mit Namen für Tooltip und Barrierefreiheit.
/// `on` tönt sie in `tone` (eingeschaltet), gesperrt bleibt sie gedimmt.
fn icon_key(
    ui: &mut egui::Ui,
    icon: &str,
    name: &str,
    tone: egui::Color32,
    on: bool,
    enabled: bool,
    side: f32,
) -> egui::Response {
    let color = if !enabled {
        theme::text_dim()
    } else if on {
        tone
    } else {
        theme::text()
    };
    let b = egui::Button::new(
        egui::RichText::new(icon)
            .family(egui::FontFamily::Name(theme::FAMILY_ICON_FILL.into()))
            .size(15.0)
            .color(color),
    )
    .fill(if on {
        theme::mix(tone, theme::surface(), 0.80)
    } else {
        theme::surface()
    })
    .stroke(egui::Stroke::new(
        1.0,
        if on { tone } else { theme::border_strong() },
    ))
    .corner_radius(theme::RADIUS_MD)
    .min_size(egui::vec2(side, side));
    let r = ui.add_enabled(enabled, b);
    let label = name.to_string();
    r.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, enabled, on, label.clone())
    });
    r
}

/// Schalter `woa-tog` (36 × 20): an = `accent-2`-Fläche mit Knopf rechts. Ersetzt Häkchen.
fn toggle(ui: &mut egui::Ui, on: &mut bool, name: &str) -> egui::Response {
    let (rect, mut r) = ui.allocate_exact_size(egui::vec2(36.0, 20.0), egui::Sense::click());
    if r.clicked() {
        *on = !*on;
        r.mark_changed();
    }
    let tone = theme::accent_2();
    let p = ui.painter();
    p.rect_filled(
        rect,
        10.0,
        if *on {
            theme::mix(tone, theme::well(), 0.78)
        } else {
            theme::well()
        },
    );
    p.rect_stroke(
        rect,
        10.0,
        egui::Stroke::new(1.0, if *on { tone } else { theme::border_strong() }),
        egui::StrokeKind::Inside,
    );
    let x = if *on {
        rect.right() - 10.0
    } else {
        rect.left() + 10.0
    };
    p.circle_filled(
        egui::pos2(x, rect.center().y),
        7.0,
        if *on { tone } else { theme::text_dim() },
    );
    let (label, state) = (name.to_string(), *on);
    r.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, state, &label));
    r
}

/// Zahlenfeld `woa-num` mit Einheit: Ziehen ändert, Doppelklick setzt `default`, Klick tippt ein.
#[allow(clippy::too_many_arguments)]
fn number_field(
    ui: &mut egui::Ui,
    value: &mut f64,
    range: std::ops::RangeInclusive<f64>,
    unit: &str,
    default: f64,
    name: &str,
    enabled: bool,
) -> egui::Response {
    let r = ui.add_enabled(
        enabled,
        egui::DragValue::new(value)
            .range(range)
            .speed(0.1)
            .fixed_decimals(1)
            .suffix(format!(" {unit}")),
    );
    let mut r = r;
    if r.double_clicked() {
        *value = default;
        r.mark_changed();
    }
    let (label, v) = (name.to_string(), *value);
    r.widget_info(|| {
        let mut i = egui::WidgetInfo::labeled(egui::WidgetType::DragValue, enabled, &label);
        i.value = Some(v);
        i
    });
    r
}

/// Taste mit LED-Punkt (`woa-key--sm` mit `woa-key__led`): neutral, an leuchtet nur der Punkt
/// und der Rand in `accent-2` (eingeschaltet, nie Rot).
fn led_key(ui: &mut egui::Ui, word: &str, on: bool) -> egui::Response {
    let tone = theme::accent_2();
    let mut job = egui::text::LayoutJob::default();
    job.append(
        "\u{25CF}",
        0.0,
        egui::TextFormat {
            font_id: egui::FontId::proportional(9.0),
            color: if on { tone } else { theme::led_off() },
            valign: egui::Align::Center,
            ..Default::default()
        },
    );
    job.append(
        &word.to_uppercase(),
        6.0,
        egui::TextFormat {
            font_id: theme::label_font(12.0),
            color: theme::text(),
            extra_letter_spacing: 0.5,
            valign: egui::Align::Center,
            ..Default::default()
        },
    );
    let r = ui.add(
        egui::Button::new(job)
            .fill(if on {
                theme::mix(tone, theme::surface(), 0.86)
            } else {
                theme::surface()
            })
            .stroke(egui::Stroke::new(
                1.0,
                if on { tone } else { theme::border_strong() },
            ))
            .corner_radius(theme::RADIUS_MD)
            .min_size(egui::vec2(0.0, 24.0)),
    );
    let label = word.to_string();
    r.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, on, &label));
    r
}

/// Neutrale kleine Taste mit Symbol und Wort (`woa-key--sm`), etwa „+ Titel“.
fn small_key(ui: &mut egui::Ui, icon: &str, word: &str) -> egui::Response {
    let r = ui.add(
        egui::Button::new(theme::fill_label(
            icon,
            &word.to_uppercase(),
            12.0,
            theme::text(),
        ))
        .fill(theme::surface())
        .stroke(egui::Stroke::new(1.0, theme::border_strong()))
        .corner_radius(theme::RADIUS_MD)
        .min_size(egui::vec2(0.0, 24.0)),
    );
    let label = word.to_string();
    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
    r
}

/// Segmentschalter `woa-segs`: Mulde mit Knöpfen, gewählt = erhabene `surface`-Fläche.
/// Liefert je Knopf die Antwort.
fn segments(ui: &mut egui::Ui, items: &[String], sel: usize) -> Vec<egui::Response> {
    let mut out = Vec::with_capacity(items.len());
    egui::Frame::new()
        .fill(theme::well())
        .stroke(egui::Stroke::new(1.0, theme::border()))
        .corner_radius(theme::RADIUS_MD)
        .inner_margin(egui::Margin::same(2))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                for (i, t) in items.iter().enumerate() {
                    let on = i == sel;
                    let r = ui.add(
                        egui::Button::new(
                            egui::RichText::new(t)
                                .monospace()
                                .size(12.0)
                                .strong()
                                .color(if on { theme::text() } else { theme::text_dim() }),
                        )
                        .fill(if on {
                            theme::surface()
                        } else {
                            egui::Color32::TRANSPARENT
                        })
                        .stroke(egui::Stroke::NONE)
                        .corner_radius(theme::RADIUS_SM)
                        .min_size(egui::vec2(30.0, 22.0)),
                    );
                    out.push(r);
                }
            });
        });
    out
}

/// Menüpunkte „Playlist ablösen“ eines Decks: Das Deck startet, die Playlist blendet aus und
/// pausiert. Liefert `Some(true)` mit Blende, `Some(false)` sofort (harter Schnitt).
pub(crate) fn deck_take_menu(ui: &mut egui::Ui, deck: usize, crossfade: f64) -> Option<bool> {
    let name = woa_radio::show::deck_name(deck);
    let mut hit = None;
    if ui
        .button(format!("Playlist ablösen, mit Blende ({crossfade:.1} s)"))
        .on_hover_text(format!(
            "Deck {name} starten: Die Playlist blendet aus und pausiert, das Deck blendet ein."
        ))
        .clicked()
    {
        hit = Some(true);
    }
    if ui
        .button("Playlist ablösen, sofort")
        .on_hover_text("Harter Schnitt ohne Blende")
        .clicked()
    {
        hit = Some(false);
    }
    hit
}

/// Tasten, die das Pult selbst belegt: M Mikro, Leertaste Start/Weiter, F11 Vollbild, A–C die Decks.
pub(crate) const RESERVED_KEYS: [egui::Key; 6] = [
    egui::Key::M,
    egui::Key::Space,
    egui::Key::F11,
    egui::Key::A,
    egui::Key::B,
    egui::Key::C,
];

/// Taste, die Deck `k` startet und stoppt (A, B, C wie der Deck-Name).
const DECK_KEYS: [egui::Key; woa_radio::DECKS] = [egui::Key::A, egui::Key::B, egui::Key::C];

/// Öffnet den Pult-Eingang (#301). Fehler des Eingangs zählt `faults`; ins Log kommt höchstens
/// eine Sammelzeile je Minute (#401: 805 Einzelzeilen an einem Nachmittag).
fn open_pult_input(
    p: &woa_radio::routing::PultIn,
    rate: u32,
    faults: Arc<AtomicU64>,
) -> Result<(Stream, woa_engine::pairs::LinkRx), String> {
    let cfg = woa_engine::record::InputConfig {
        device_id: Some(p.open_id.clone()),
        pair: p.pair,
    };
    let mut burst = ErrorBurst::default();
    match woa_engine::record::start_pair_input(&cfg, rate, move |e| {
        faults.fetch_add(1, Ordering::Relaxed);
        if let Some(n) = burst.note(Instant::now()) {
            tracing::warn!(error = %e, count = n, "pult input errors (last minute)");
        }
    }) {
        Ok((stream, rx, _stats, in_rate)) => {
            tracing::info!(device = %p.name, rate = in_rate, "pult input open, pult mode on");
            Ok((Box::new(stream), rx))
        }
        Err(e) => {
            tracing::warn!(error = %e, device = %p.name, "pult input failed");
            Err(format!(
                "Pult-Modus: Eingang {} lässt sich nicht öffnen ({e}). Die Sendung kommt weiter \
                 aus WOA.",
                p.name
            ))
        }
    }
}

/// Drosselt wiederkehrende Fehlermeldungen: die erste sofort, danach höchstens eine
/// Sammelzeile je [`ErrorBurst::EVERY`] mit der Anzahl seit der letzten Zeile.
#[derive(Debug, Default)]
struct ErrorBurst {
    last: Option<Instant>,
    count: u64,
}

impl ErrorBurst {
    const EVERY: Duration = Duration::from_secs(60);

    /// Ein Fehler. `Some(n)`: jetzt eine Zeile für `n` Fehler schreiben.
    fn note(&mut self, now: Instant) -> Option<u64> {
        self.count += 1;
        if self
            .last
            .is_some_and(|t| now.duration_since(t) < Self::EVERY)
        {
            return None;
        }
        self.last = Some(now);
        Some(std::mem::take(&mut self.count))
    }
}

/// Plausibilität des Sendewegs (#400): Ist auf Sendung ein Ausgang deutlich hörbar
/// (> −40 dBFS), der Stream aber still (< −60 dBFS), schlägt nach [`SilentStream::AFTER`]
/// der Alarm an.
#[derive(Debug, Default)]
struct SilentStream {
    since: Option<Instant>,
    alarm: bool,
}

impl SilentStream {
    const AFTER: Duration = Duration::from_secs(5);
    /// −40 dBFS linear.
    const AUDIBLE: f32 = 0.01;
    /// −60 dBFS linear.
    const SILENT: f32 = 0.001;

    fn step(&mut self, on_air: bool, audible: f32, stream: f32, now: Instant) -> bool {
        if on_air && audible > Self::AUDIBLE && stream < Self::SILENT {
            let since = *self.since.get_or_insert(now);
            self.alarm = now.duration_since(since) >= Self::AFTER;
        } else {
            self.since = None;
            self.alarm = false;
        }
        self.alarm
    }

    fn alarm(&self) -> bool {
        self.alarm
    }
}

#[cfg(test)]
mod pult_guard_tests {
    use super::{ErrorBurst, SilentStream};
    use std::time::{Duration, Instant};

    #[test]
    fn error_burst_logs_once_per_minute_with_count() {
        let t0 = Instant::now();
        let mut b = ErrorBurst::default();
        assert_eq!(b.note(t0), Some(1));
        for i in 1..600 {
            assert_eq!(b.note(t0 + Duration::from_millis(100 * i)), None);
        }
        assert_eq!(b.note(t0 + Duration::from_secs(61)), Some(600));
    }

    #[test]
    fn silent_stream_alarms_after_five_seconds_only_on_air() {
        let t0 = Instant::now();
        let mut s = SilentStream::default();
        assert!(!s.step(true, 0.2, 0.0001, t0));
        assert!(!s.step(true, 0.2, 0.0001, t0 + Duration::from_secs(4)));
        assert!(s.step(true, 0.2, 0.0001, t0 + Duration::from_secs(5)));
        // Stream kommt zurück: Alarm weg.
        assert!(!s.step(true, 0.2, 0.3, t0 + Duration::from_secs(6)));
        // Nicht auf Sendung oder alles still: nie Alarm.
        let mut s = SilentStream::default();
        assert!(!s.step(false, 0.2, 0.0, t0));
        assert!(!s.step(false, 0.2, 0.0, t0 + Duration::from_secs(10)));
        assert!(!s.step(true, 0.0, 0.0, t0 + Duration::from_secs(20)));
        assert!(!s.step(true, 0.0, 0.0, t0 + Duration::from_secs(30)));
    }
}

/// Was `RadioDesk::open` geöffnet hat, bevor das Pult zusammengesetzt wird.
struct Parts {
    radio_path: PathBuf,
    data_dir: PathBuf,
    station: Option<Station>,
    opened: crate::outputs::Opened,
    output_warnings: Vec<String>,
    error: Option<String>,
    rate: u32,
    buffer_frames: Option<u32>,
    device: String,
    underruns: Arc<AtomicU64>,
    pfl_output: Option<woa_engine::device::ManagedOutput>,
    pfl_note: Option<String>,
}

/// Die laufende Sendung samt Gerät.
pub struct RadioDesk {
    /// Laufende Fade-Blenden (FadeButton, eingestellte Dauer) für die Anzeige.
    fades: crate::widgets::fade::Fades,
    /// Beginn des laufenden Mitschnitts (Dauer im REC-Tooltip).
    rec_since: Option<std::time::Instant>,
    radio_path: PathBuf,
    data_dir: PathBuf,
    station: Option<Station>,
    /// Geöffnete Ausgabeströme (Hauptgerät und weitere Geräte).
    outputs: crate::outputs::Opened,
    /// Meldungen aus dem Öffnen, die in den Einstellungen stehen bleiben.
    output_warnings: Vec<String>,
    mic: Option<Stream>,
    /// Gewähltes Mikrofon (Fenster Audio-Geräte), Vorgabe Systemstandard.
    mic_cfg: crate::outputs_ui::MicInput,
    /// Telefon-Eingang (Anrufer): Strom, gewählte Einstellung, Meldung, Zähler, Pegel.
    phone: Option<Stream>,
    phone_cfg: crate::phone::PhoneInput,
    phone_error: Option<String>,
    phone_stats: Option<Arc<woa_engine::pairs::LinkStats>>,
    phone_lost: Arc<std::sync::atomic::AtomicBool>,
    phone_level: f32,
    /// Pult-Modus (#301): Strom des Pult-Eingangs (lebt, solange das Feld lebt) und Pegel.
    /// `None`: Die Sendung kommt aus WOAs interner Mischung.
    pult: Option<Stream>,
    pult_level: f32,
    /// Fehler des Pult-Eingangs seit dem Öffnen (Unter-/Überläufe, #401), für die Anzeige.
    pult_faults: Arc<AtomicU64>,
    /// Stiller Sendeweg (#400): hörbare Ausgänge, aber stummer Stream.
    silent_stream: SilentStream,
    /// Pegel des Vorhör-Ausgangs (PFL), fällt langsam ab; für das MCP-Werkzeug `output_levels`.
    pfl_level: f32,
    /// LED-Meter neben den Fadern und Master (#216).
    meters: [MeterHold; 5],
    /// LED-Ketten der Ausgänge Notebook und Stream.
    out_meters: [MeterHold; 2],
    /// Alte Cart-Tasten auf A–C schon geprüft (Hinweis nur einmal).
    deck_keys_checked: bool,
    /// Gespielte ausblenden: lokal sofort, bis der Schnappschuss nachzieht.
    hide_played: Option<bool>,
    /// Wunschordner: Beobachter. Ohne Wahl liegt er im Datenordner der Sendung.
    wishes: Option<crate::wishes::WishWatcher>,
    wish_chosen: Option<PathBuf>,
    /// Party-Decks sind an (Party-Modus).
    party_on: bool,
    rate: u32,
    error: Option<String>,
    view: View,
    /// QR-Code der zuletzt gezeigten Einladung (Paket 38).
    invite_qr: Option<(String, woa_remote::qr::QrMatrix)>,
    /// „Trennen“ einmal geklickt: Kanal und Zeitpunkt; der zweite Klick binnen 3 s trennt.
    guest_hangup_armed: Option<(usize, Instant)>,
    saved_revision: u64,
    /// Was dieses Pult zuletzt nach radio.json geschrieben hat (#380): Ändert sich nur die
    /// Uhrzeit der Datei (Windows, Virenscanner), ist das keine fremde Änderung.
    last_written: Option<RadioShow>,
    /// Letzter Fehler beim Schreiben von radio.json; bleibt stehen, bis es wieder klappt
    /// (#380: die Meldung wurde vorher von anderen Meldungen im selben Feld verdrängt).
    save_error: Option<String>,
    saved_at: Instant,
    known_mtime: Option<SystemTime>,
    checked_at: Instant,
    mic_dropped: Arc<AtomicU64>,
    covers: Option<crate::covers::Covers>,
    /// Sendung: Feeder zum woa-streamd (nur wenn „Auf Sendung“).
    feeder: Option<Feeder>,
    /// Letztes „Auf Sendung“/„Sendung beenden“ (Entprellen, #367).
    air_toggled_at: Option<Instant>,
    stream_status: Option<StreamStatus>,
    stream_checked_at: Instant,
    sent_title: String,
    /// Aussetzer der Ausgabe (vom Fehlerpfad des Stroms gezählt).
    underruns: Arc<AtomicU64>,
    xruns: XrunLog,
    buffer_frames: Option<u32>,
    device: String,
    /// Pegel Musik, Carts, Mikro in dB (Paket 14); nicht gespeichert, Start bei 0 dB.
    levels_db: [f64; 4],
    /// „MIDI lernen“ per Rechtsklick gewählt; die App holt es ab.
    pub learn: Option<Target>,
    /// Fläche der Playlist und der Cart-Pads im letzten Bild (Ziel für Ziehen aus der Bibliothek).
    playlist_rect: egui::Rect,
    cart_rects: Vec<egui::Rect>,
    /// Flächen der Decks A bis C (Ziel für gezogene Dateien).
    deck_rects: [egui::Rect; woa_radio::DECKS],
    /// Fehlerbanner und Wunschtitel-Zeile im letzten Bild (Tests: eine Zeile, im Fenster).
    error_rect: egui::Rect,
    wish_rect: egui::Rect,
    /// Fehlerbanner aufgeklappt („Details“).
    error_details: bool,
    /// Zeilen der Playlist im letzten Bild (Ziel beim Umsortieren per Ziehen, #166).
    row_rects: Vec<egui::Rect>,
    /// Gezogene Playlist-Zeile (Index), solange die Maus gedrückt ist (#166).
    row_drag: Option<usize>,
    /// Angeklickte Playlist-Zeile (#302), nur Hervorhebung.
    row_selected: Option<usize>,
    /// Playlist-Zeile unter der Maus im letzten Bild (zeigt Verschieben und Entfernen).
    row_hover: Option<usize>,
    /// Nummer einer Playlist-Zeile wird gerade eingetippt (#92): Zeile, Text und ob der Fokus
    /// schon gesetzt wurde (nur einmal, #322).
    pos_edit: Option<(usize, String, bool)>,
    /// Cart, das die nächste gedrückte Taste als neue Taste bekommt.
    key_capture: Option<usize>,
    /// Offenes Umbenennen (#292): was, Text, Fokus schon gesetzt.
    rename: Option<(Rename, String, bool)>,
    /// Ortszeit für die Uhrzeit-Anzeige (von der Stundenuhr).
    zone: Zone,
    /// Offener Cue-Editor (Paket 15).
    cue_editor: Option<CueEditor>,
    /// Titelinfos dieses Titels wünscht das Kontextmenü (öffnet die Bibliothek, App holt ab).
    tag_wish: Option<PathBuf>,
    /// Offener Voice-Track-Editor (Paket 30).
    voice_editor: Option<VoiceEditor>,
    /// Zuletzt in den Editor übernommene Cue-Punkte je Titel (erkennt Undo/Redo).
    cue_seen: std::collections::HashMap<usize, woa_radio::Cues>,
    /// Fenster „Verwalten“ und Auswahl der aktiven Playlist.
    manage: crate::manage::ManageWindow,
    /// Eigener Strom für das Vorhören auf einem zweiten Gerät.
    /// Vorhören auf einem zweiten Gerät, im eigenen Thread überwacht (#291).
    _pfl_output: crate::outputs::OutputWatch,
    /// Warum das Vorhören keinen Ausgang hat (aus oder Fehler beim Öffnen); `None` = offen.
    pfl_note: Option<String>,
}

fn level_bus(b: Bus) -> LevelBus {
    match b {
        Bus::Music => LevelBus::Music,
        Bus::Carts => LevelBus::Carts,
        Bus::Mic => LevelBus::Mic,
        Bus::Decks => LevelBus::Decks,
    }
}

fn bus_index(b: Bus) -> usize {
    match b {
        Bus::Music => 0,
        Bus::Carts => 1,
        Bus::Mic => 2,
        Bus::Decks => 3,
    }
}

/// Mindestabstand zwischen zwei Klicks auf die Sendetaste.
const AIR_DEBOUNCE: Duration = Duration::from_millis(1500);

/// Pfad zu `woa-streamd` neben der laufenden App.
fn streamd_exe() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let name = if cfg!(windows) {
        "woa-streamd.exe"
    } else {
        "woa-streamd"
    };
    let p = exe
        .parent()
        .map(|d| d.join(name))
        .ok_or("Programmordner unbekannt")?;
    if p.is_file() {
        Ok(p)
    } else {
        Err(format!("{} fehlt", p.display()))
    }
}

/// Startet woa-streamd als eigenständigen Prozess: eigene Prozessgruppe, keine
/// Standard-Ein-/Ausgabe. So beendet weder Strg+C im Terminal noch ein Absturz der
/// Oberfläche die Sendung.
pub fn spawn_streamd(data_dir: &Path) -> Result<(), String> {
    use std::process::{Command, Stdio};
    let exe = streamd_exe()?;
    let mut cmd = Command::new(exe);
    cmd.arg("--data-dir")
        .arg(data_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }
    cmd.spawn()
        .map(|_child| ())
        .map_err(|e| format!("woa-streamd startet nicht: {e}"))
}

fn mtime(p: &Path) -> Option<SystemTime> {
    std::fs::metadata(p).and_then(|m| m.modified()).ok()
}

/// Reihenfolge und Namen der senkrechten Pegelregler neben den Decks.
pub(crate) const FADER_BUSES: [(Bus, &str); 4] = [
    (Bus::Music, "Playlist"),
    (Bus::Carts, "Carts"),
    (Bus::Decks, "Decks"),
    (Bus::Mic, "Mikro"),
];

/// Anzeige eines LED-Meters: fällt weich, die Spitze steht [`HOLD_S`] Sekunden.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct MeterHold {
    pub level: f32,
    pub hold: f32,
    hold_at: f64,
}

/// So lange bleibt die Spitze stehen (Sekunden).
const HOLD_S: f64 = 1.5;

impl MeterHold {
    /// Neue Spitze `peak` (linear) zur Zeit `now` (Sekunden).
    pub fn step(&mut self, peak: f32, now: f64) {
        self.level = peak.max(self.level * 0.85);
        if peak >= self.hold || now - self.hold_at > HOLD_S {
            self.hold = peak.max(self.level);
            self.hold_at = now;
        }
    }
}

fn now_s() -> f64 {
    static T0: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    T0.get_or_init(Instant::now).elapsed().as_secs_f64()
}

/// Ein schmales LED-Meter mit Namen für die Barrierefreiheit (und die Tests).
fn meter_strip(ui: &mut egui::Ui, m: MeterHold, name: &str, size: egui::Vec2) {
    let (rect, r) = ui.allocate_exact_size(size, egui::Sense::hover());
    theme::bus_meter(ui.painter(), rect, m.level, m.hold);
    let label = format!("Pegelanzeige {name}");
    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::ProgressIndicator, true, &label));
    r.on_hover_text(format!(
        "{name}: Spitze {} dBFS. Grün bis -18, Gelb bis -6, Rot darüber, Blau = zu leise.",
        crate::widgets::vu::fmt_db(20.0 * m.hold.max(1e-6).log10())
    ));
}

/// Senkrechte Pegelregler (wie der Kanal-Gain der Spuren): Name oben, Fader mit dB-Skala und
/// LED-Meter daneben, Wert darunter; als fünfte Spalte das Meter „Master“ (Sendemix, ohne
/// Fader). `dbs` in der Reihenfolge von [`FADER_BUSES`], `meters` dazu plus Master. Liefert je
/// Regler die Antwort (`changed()`, wenn der Wert sich geändert hat; Doppelklick = 0 dB).
/// Fader-Weg der Kanalzüge (RadioLevels: 130 px). Die Pegel stehen seit Design-Runde 2 in einer
/// eigenen Karte unter der Cart-Wall, die Cart-Wall bleibt dadurch ganz im Fenster (#303).
const FADER_HEIGHT: f32 = 130.0;
/// Breite eines Kanalzugs und seiner LED-Kette (RadioLevels: 56 px, LED-Kette sichtbar).
const STRIP_W: f32 = 56.0;
/// LED-Kette neben dem Fader, seit #287 kräftiger (vorher 10 px).
const METER_W: f32 = 16.0;
/// Breite, ab der Pegel und Processing nebeneinander stehen statt untereinander.
const LEVELS_SIDE_BY_SIDE: f32 = 720.0;

/// Farbiges Namensschild über einem Kanalzug (Scribble-Strip, RadioLevels).
fn scribble(ui: &mut egui::Ui, name: &str, color: egui::Color32, w: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, 18.0), egui::Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, theme::RADIUS_SM, theme::lcd_bg());
    p.rect_stroke(
        rect,
        theme::RADIUS_SM,
        egui::Stroke::new(1.0, theme::border()),
        egui::StrokeKind::Inside,
    );
    p.rect_filled(
        egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), 3.0)),
        theme::RADIUS_XS,
        color,
    );
    p.text(
        rect.center() + egui::vec2(0.0, 1.5),
        egui::Align2::CENTER_CENTER,
        name.to_uppercase(),
        theme::label_font(11.0),
        theme::lcd_text(),
    );
}

/// 1-px-Trennlinie zwischen den Gruppen der Pegelkarte.
fn group_sep(ui: &mut egui::Ui, h: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(9.0, h), egui::Sense::hover());
    let line = egui::Rect::from_center_size(rect.center(), egui::vec2(1.0, h));
    ui.painter().rect_filled(line, 0.0, theme::border());
}
/// Höhe eines Cart-Pads.
const CART_PAD_HEIGHT: f32 = 52.0;

pub(crate) fn level_faders(
    ui: &mut egui::Ui,
    dbs: &mut [f32; 4],
    meters: &[MeterHold; 5],
) -> Vec<egui::Response> {
    let mut out = Vec::with_capacity(4);
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        for (k, (((_, name), db), m)) in FADER_BUSES
            .iter()
            .zip(dbs.iter_mut())
            .zip(meters)
            .enumerate()
        {
            ui.vertical(|ui| {
                ui.set_width(STRIP_W);
                ui.spacing_mut().item_spacing.y = 4.0;
                let r = ui
                    .horizontal_top(|ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        let r = crate::widgets::fader(
                            ui,
                            db,
                            woa_radio::mixer::LEVEL_MIN_DB as f32
                                ..=woa_radio::mixer::LEVEL_MAX_DB as f32,
                            0.0,
                            egui::vec2(36.0, FADER_HEIGHT),
                            crate::widgets::Orient::Vertical,
                        );
                        meter_strip(ui, *m, name, egui::vec2(METER_W, FADER_HEIGHT));
                        r
                    })
                    .inner;
                let label = format!("Pegel {name}");
                let value = f64::from(*db);
                r.widget_info(|| egui::WidgetInfo::slider(true, value, &label));
                scribble(ui, name, theme::track_color(k), STRIP_W);
                out.push(r.on_hover_text(format!(
                    "Pegel {name}: {}. Doppelklick: 0 dB. Rechtsklick: MIDI lernen. Ganz unten = aus.",
                    crate::widgets::vu::fmt_db(*db)
                )));
            });
        }
        group_sep(ui, FADER_HEIGHT + 44.0);
        ui.vertical(|ui| {
            ui.set_width(STRIP_W);
            ui.spacing_mut().item_spacing.y = 4.0;
            ui.horizontal(|ui| {
                ui.add_space((STRIP_W - 1.5 * METER_W) / 2.0);
                meter_strip(
                    ui,
                    meters[4],
                    "Master",
                    egui::vec2(1.5 * METER_W, FADER_HEIGHT),
                );
            });
            ui.add_space(20.0);
            scribble(ui, "Master", theme::track_color(5), STRIP_W);
        });
    });
    out
}

/// Die zwei Ausgänge mit eigenem Regler (#96), Reihenfolge wie in [`output_faders`].
const OUTPUTS: [(woa_radio::show::Output, &str, &str); 2] = [
    (
        woa_radio::show::Output::Monitor,
        "Notebook",
        "Wiedergabe Notebook",
    ),
    (woa_radio::show::Output::Stream, "Stream", "Stream senden"),
];

/// Regler für die Ausgänge „Wiedergabe Notebook“ (Abhören) und „Stream senden“ (#96), im
/// Stil der Pegelregler, darunter je ein roter Stumm-Knopf. Liefert je Ausgang, ob sich Pegel
/// oder Stumm geändert hat.
pub(crate) fn output_faders(
    ui: &mut egui::Ui,
    outs: &mut [woa_radio::show::OutLevel; 2],
    meters: &[MeterHold; 2],
) -> [bool; 2] {
    let mut changed = [false; 2];
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        for ((((_, short, name), o), ch), m) in OUTPUTS
            .iter()
            .zip(outs.iter_mut())
            .zip(&mut changed)
            .zip(meters)
        {
            ui.vertical(|ui| {
                ui.set_width(STRIP_W);
                ui.spacing_mut().item_spacing.y = 4.0;
                let mut db = o.db as f32;
                // Fader und LED-Kette des Ausgangs nebeneinander, wie bei den Quellen.
                let r = ui
                    .horizontal_top(|ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        let r = crate::widgets::fader(
                            ui,
                            &mut db,
                            woa_radio::mixer::LEVEL_MIN_DB as f32
                                ..=woa_radio::mixer::LEVEL_MAX_DB as f32,
                            0.0,
                            egui::vec2(36.0, FADER_HEIGHT),
                            crate::widgets::Orient::Vertical,
                        );
                        meter_strip(ui, *m, short, egui::vec2(METER_W, FADER_HEIGHT));
                        r
                    })
                    .inner;
                let label = format!("Pegel {name}");
                let value = f64::from(db);
                r.widget_info(|| egui::WidgetInfo::slider(true, value, &label));
                let r = r.on_hover_text(format!(
                    "Pegel {name}: {}. Doppelklick: 0 dB. Ganz unten = aus.",
                    crate::widgets::vu::fmt_db(db)
                ));
                if r.changed() {
                    o.db = f64::from(db);
                    *ch = true;
                }
                let red = theme::on_air();
                let m = egui::Button::new(egui::RichText::new("M").strong().color(if o.mute {
                    theme::well()
                } else {
                    theme::text_dim()
                }))
                .fill(if o.mute { red } else { theme::surface() })
                .stroke(egui::Stroke::new(
                    1.0,
                    if o.mute { red } else { theme::border_strong() },
                ))
                .corner_radius(theme::RADIUS_MD)
                .min_size(egui::vec2(STRIP_W, 24.0));
                let label = format!("Stumm {name}");
                let mute = o.mute;
                let b = ui.add(m);
                b.widget_info(|| {
                    egui::WidgetInfo::selected(egui::WidgetType::Checkbox, true, mute, &label)
                });
                if b.on_hover_text(format!("{name} stumm schalten")).clicked() {
                    o.mute = !o.mute;
                    *ch = true;
                }
                scribble(ui, short, theme::track_color(4), STRIP_W);
            });
        }
    });
    changed
}

/// Umschalter „Zurück zur Playlist“ an einem Deck (Symbol-Taste, RadioDecks). Liefert die
/// Antwort (`clicked()`).
pub(crate) fn deck_return_toggle(ui: &mut egui::Ui, deck: usize, on: bool) -> egui::Response {
    let name = woa_radio::show::deck_name(deck);
    icon_key(
        ui,
        theme::icf::QUEUE,
        &format!("Zurück zur Playlist Deck {name}"),
        theme::accent_2(),
        on,
        true,
        SYMBOL_KEY,
    )
    .on_hover_text(format!(
        "Danach zurück zur Playlist ({}): Ist das an, läuft nach Deck {name} die Playlist mit dem \
         eingestellten Crossfade weiter (pausiert: ab der Pausenstelle). Aus: Nach dem Deck \
         bleibt es still, etwa zum Reden oder Telefonieren.",
        if on { "an" } else { "aus" }
    ))
}

pub fn clock(seconds: f64) -> String {
    let s = seconds.max(0.0).round() as u64;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{:02}:{:02}", s / 60, s % 60)
    }
}

/// Uhrzeit `hh:mm:ss` zur Unix-Zeit `unix` in `zone` (zum Ansagen).
pub fn time_of_day(unix: i64, zone: Zone) -> String {
    let (h, m, s) = time_parts(unix, zone);
    crate::widgets::clock::fmt_hms(h, m, s)
}

/// Stunde, Minute und Sekunde zur Unix-Zeit `unix` in `zone` (für die Studio-Uhr).
pub fn time_parts(unix: i64, zone: Zone) -> (u32, u32, u32) {
    let l = zone.local(unix);
    (l.hour as u32, l.minute as u32, l.second as u32)
}

/// Was „Umbenennen …“ umbenennt (#292); ein Dialog für alles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rename {
    CartSet(usize),
    Cart(usize),
    Title(usize),
}

impl Rename {
    /// Überschrift des Dialogs.
    pub fn heading(self) -> String {
        match self {
            Self::CartSet(i) => format!("Cart-Wall-Satz {} umbenennen", i + 1),
            Self::Cart(s) => format!("Cart {} umbenennen", s + 1),
            Self::Title(i) => format!("Titel {} umbenennen", i + 1),
        }
    }

    /// Führt das Umbenennen am Controller aus (dieselben Funktionen wie der MCP).
    pub fn apply(self, c: &mut Controller, name: &str) -> Result<(), String> {
        match self {
            Self::CartSet(i) => c.rename_cart_set(i, name),
            Self::Cart(s) => c.rename_cart(s, name),
            Self::Title(i) => c.rename_title(i, name),
        }
    }
}

/// Name der Zifferntaste, die zur Nummer von Cart `slot` passt (1–9, 0 für 10).
fn cart_number_key(slot: usize) -> Option<String> {
    woa_radio::show::default_cart_key(slot).map(str::to_string)
}

/// Carts, deren gespeicherte Taste jetzt einem Deck gehört (A–C): `(slot, Taste)`.
fn deck_key_conflicts(keys: &[Option<String>]) -> Vec<(usize, String)> {
    keys.iter()
        .enumerate()
        .filter_map(|(slot, k)| {
            let k = k.as_deref()?;
            let key = egui::Key::from_name(k)?;
            DECK_KEYS.contains(&key).then(|| (slot, k.to_string()))
        })
        .collect()
}

/// Hinweis zu [`deck_key_conflicts`]; die Belegung bleibt in der Datei, bis sie neu gesetzt wird.
fn deck_key_note(conflicts: &[(usize, String)]) -> Option<String> {
    if conflicts.is_empty() {
        return None;
    }
    let list: Vec<String> = conflicts
        .iter()
        .map(|(slot, k)| format!("Cart {} (Taste {k})", slot + 1))
        .collect();
    Some(format!(
        "Die Tasten A–C starten jetzt die Decks. {} reagiert nicht mehr auf die Taste; \
         bitte per Rechtsklick eine andere Taste wählen.",
        list.join(", ")
    ))
}

/// Egui-Taste zu einem gespeicherten Namen (`None`, wenn es sie nicht gibt oder sie reserviert ist).
fn cart_key_of(name: &str) -> Option<egui::Key> {
    egui::Key::from_name(name).filter(|k| !RESERVED_KEYS.contains(k))
}

/// Beim Verwerfen des Pults (Schließen, Neu-Öffnen für Vorhören oder Ausgänge) erst sichern (#380).
impl Drop for RadioDesk {
    fn drop(&mut self) {
        self.flush();
    }
}

impl RadioDesk {
    /// Zone der Uhrzeit-Anzeige: Versatz in Minuten im Winter, EU-Sommerzeit ja/nein.
    pub fn set_zone(&mut self, base_min: i32, eu_dst: bool) {
        self.zone = Zone { base_min, eu_dst };
    }

    /// Lädt `radio.json` und öffnet die Ausgabe. Geht das Gerät nicht, bleibt die
    /// Playlist bearbeitbar und die Meldung steht oben.
    /// `buffer_frames`: Puffergröße der Ausgabe (`None` = Treiber).
    pub fn open(
        radio_path: PathBuf,
        data_dir: PathBuf,
        buffer_frames: Option<u32>,
        pfl: &PflOutput,
        output_map: &woa_radio::routing::OutputMap,
        pult_cfg: &woa_radio::routing::Pult,
    ) -> Self {
        let (show, mut error) = match RadioShow::load(&radio_path) {
            Ok(s) => (s, None),
            Err(e) => (RadioShow::default(), Some(e)),
        };
        // Ausgänge je Gerät: Das Gerät der Playlist taktet den Mixer. Vorhören auf Kanal 3/4
        // belegt Paar 1 des Hauptgeräts.
        let reserved: &[usize] = if matches!(pfl, PflOutput::Kanal34) {
            &[1]
        } else {
            &[]
        };
        let mut plan = crate::outputs::plan(output_map, pult_cfg, reserved);
        // Pult-Modus: Vorhören auf Kanal 3/4 des Pult-Geräts käme über das Pult auf Sendung.
        let pfl_refused = plan.pult.is_some() && matches!(pfl, PflOutput::Kanal34);
        let pfl = if pfl_refused { &PflOutput::Aus } else { pfl };
        let config = woa_engine::device::OutputConfig {
            device_id: plan.devices[0].open_id.clone(),
            buffer_frames,
        };
        let rate = plan.devices[0].info.rate;
        let (mut controller, mixer) = Controller::new(show, rate);
        let device = plan.devices[0].info.name.clone();
        let underruns = Arc::new(AtomicU64::new(0));
        let counter = Arc::clone(&underruns);
        // Vorhören (Paket 15): Kanal 3/4 im selben Callback oder ein eigener Strom.
        // Der PFL-Spieler hat keinen Weg in den Mixer, den Mitschnitt oder den Sende-Ring.
        let mut pfl_note: Option<String> = None;
        let (aux, pfl_output) = match pfl {
            PflOutput::Aus if pfl_refused => {
                pfl_note = Some(
                    "Vorhören aus: Im Pult-Modus käme Kanal 3/4 des Pult-Geräts auf Sendung. \
                     Bitte ein eigenes Kopfhörer-Gerät wählen."
                        .into(),
                );
                error.clone_from(&pfl_note);
                (None, None)
            }
            PflOutput::Aus => {
                pfl_note = Some(PFL_OFF.into());
                (None, None)
            }
            PflOutput::Kanal34 => match woa_engine::device::output_channels(&config) {
                Ok(n) if n >= 4 => {
                    let (h, mut player) = woa_radio::pfl::channel_with_bus(rate, rate);
                    controller.set_pfl(Some(h));
                    tracing::info!(channels = n, "pfl on channels 3/4");
                    let f: woa_engine::device::AuxRender =
                        Box::new(move |l, r| player.process(l, r));
                    (Some(f), None)
                }
                Ok(n) => {
                    tracing::warn!(channels = n, "pfl: device has too few channels");
                    pfl_note = Some(format!(
                        "Vorhören aus: Das Gerät hat nur {n} Kanäle, Kanal 3/4 gibt es nicht"
                    ));
                    error.clone_from(&pfl_note);
                    (None, None)
                }
                Err(e) => {
                    tracing::warn!(error = %e, "pfl: channel count");
                    pfl_note = Some(format!("Vorhören aus: Kanalzahl nicht lesbar ({e})"));
                    error.clone_from(&pfl_note);
                    (None, None)
                }
            },
            PflOutput::Geraet(id) => match open_pfl_device(id, rate) {
                Ok((h, st)) => {
                    controller.set_pfl(Some(h));
                    (None, Some(st))
                }
                Err(e) => {
                    pfl_note = Some(e);
                    error.clone_from(&pfl_note);
                    (None, None)
                }
            },
        };
        // Pult-Eingang öffnen; geht er nicht, kommt die Sendung weiter aus WOA (mit Meldung).
        let mut pult_stream: Option<Stream> = None;
        let pult_faults = Arc::new(AtomicU64::new(0));
        if let Some(p) = plan.pult.take() {
            match open_pult_input(&p, rate, Arc::clone(&pult_faults)) {
                Ok((stream, rx)) => {
                    controller.set_pult_input(Some(rx));
                    controller.set_pult_mode(true);
                    pult_stream = Some(stream);
                }
                Err(e) => plan.warnings.push(e),
            }
        }
        let opened = crate::outputs::open(
            &mut controller,
            mixer,
            &plan,
            buffer_frames,
            aux,
            Arc::clone(&counter),
        );
        let output_warnings = opened.warnings.clone();
        if !output_warnings.is_empty() {
            let all = output_warnings.join("\n");
            error = Some(match error {
                Some(e) => format!("{e}\n{all}"),
                None => all,
            });
        }
        let station = match Station::spawn(controller) {
            Ok(s) => {
                s.measure_missing(&s.show());
                Some(s)
            }
            Err(e) => {
                error = Some(format!("Sendesteuerung startet nicht: {e}"));
                None
            }
        };
        Self::assemble(Parts {
            radio_path,
            data_dir,
            station,
            opened,
            output_warnings,
            error,
            rate,
            buffer_frames,
            device,
            underruns,
            pfl_output,
            pfl_note,
        })
        .with_pult(pult_stream, pult_faults)
        .reattach()
    }

    fn with_pult(mut self, pult: Option<Stream>, faults: Arc<AtomicU64>) -> Self {
        self.pult = pult;
        self.pult_faults = faults;
        self
    }

    /// Pult-Modus während des Betriebs umschalten (#399), ohne die Sendeausgänge neu zu
    /// öffnen. Aus: erst den Mixer auf die interne Mischung, dann den Eingang lösen. An: erst
    /// den Eingang öffnen, dann umschalten; geht er nicht auf, bleibt die interne Mischung.
    /// Gibt `true` zurück, wenn der Mixer danach im gewünschten Zustand ist.
    pub fn set_pult_live(
        &mut self,
        output_map: &woa_radio::routing::OutputMap,
        pult_cfg: &woa_radio::routing::Pult,
    ) -> bool {
        if !pult_cfg.on {
            self.with(|c| {
                c.set_pult_mode(false);
                c.set_pult_input(None);
            });
            self.pult = None;
            tracing::info!("pult mode off (live)");
            return true;
        }
        let plan = crate::outputs::plan(output_map, pult_cfg, &[]);
        let Some(p) = plan.pult else {
            self.error = Some(
                "Pult-Modus: kein Pult-Eingang gewählt oder gefunden. Die Sendung kommt weiter \
                 aus WOA."
                    .into(),
            );
            return false;
        };
        // Alten Eingang erst lösen (dasselbe Gerät ginge sonst nicht ein zweites Mal auf).
        self.with(|c| {
            c.set_pult_mode(false);
            c.set_pult_input(None);
        });
        self.pult = None;
        let faults = Arc::new(AtomicU64::new(0));
        match open_pult_input(&p, self.rate, Arc::clone(&faults)) {
            Ok((stream, rx)) => {
                self.with(move |c| {
                    c.set_pult_input(Some(rx));
                    c.set_pult_mode(true);
                });
                self.pult = Some(stream);
                self.pult_faults = faults;
                tracing::info!("pult mode on (live)");
                true
            }
            Err(e) => {
                self.error = Some(e);
                false
            }
        }
    }

    /// Fehler des Pult-Eingangs seit dem Öffnen (#401).
    pub fn pult_faults(&self) -> u64 {
        self.pult_faults.load(Ordering::Relaxed)
    }

    /// Pult ohne Audiogerät für Oberflächen-Tests: Steuerung und Mixer laufen, nichts klingt
    /// (der Mixer rechnet zehn Sekunden lang in einen Puffer, der nirgends hingeht).
    #[cfg(test)]
    pub(crate) fn headless(dir: &std::path::Path) -> Self {
        let (controller, mut mixer) = Controller::new(RadioShow::default(), 48_000);
        std::thread::spawn(move || {
            let (mut l, mut r) = (vec![0.0f32; 256], vec![0.0f32; 256]);
            let end = Instant::now() + Duration::from_secs(10);
            while Instant::now() < end {
                mixer.process(&mut l, &mut r);
                std::thread::sleep(Duration::from_millis(2));
            }
        });
        Self::assemble(Parts {
            radio_path: dir.join("radio.json"),
            data_dir: dir.to_path_buf(),
            station: Station::spawn(controller).ok(),
            opened: crate::outputs::Opened {
                watch: crate::outputs::OutputWatch::default(),
                warnings: Vec::new(),
                faults: Vec::new(),
                links: Vec::new(),
            },
            output_warnings: Vec::new(),
            error: None,
            rate: 48_000,
            buffer_frames: None,
            device: String::new(),
            underruns: Arc::new(AtomicU64::new(0)),
            pfl_output: None,
            pfl_note: None,
        })
    }

    /// Setzt das Pult aus schon geöffneten Teilen zusammen (von `open` und den Tests).
    fn assemble(p: Parts) -> Self {
        let Parts {
            radio_path,
            data_dir,
            station,
            opened,
            output_warnings,
            error,
            rate,
            buffer_frames,
            device,
            underruns,
            pfl_output,
            pfl_note,
        } = p;
        let known_mtime = mtime(&radio_path);
        Self {
            rec_since: None,
            radio_path,
            data_dir,
            station,
            outputs: opened,
            output_warnings,
            mic: None,
            mic_cfg: crate::outputs_ui::MicInput::default(),
            phone: None,
            phone_cfg: crate::phone::PhoneInput::default(),
            phone_error: None,
            phone_stats: None,
            phone_lost: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            phone_level: 0.0,
            pult: None,
            pult_level: 0.0,
            pult_faults: Arc::new(AtomicU64::new(0)),
            silent_stream: SilentStream::default(),
            pfl_level: 0.0,
            meters: Default::default(),
            out_meters: Default::default(),
            deck_keys_checked: false,
            hide_played: None,
            wishes: None,
            party_on: false,
            wish_chosen: None,
            rate,
            error,
            view: View::default(),
            invite_qr: None,
            guest_hangup_armed: None,
            saved_revision: 0,
            last_written: None,
            save_error: None,
            saved_at: Instant::now(),
            known_mtime,
            checked_at: Instant::now(),
            mic_dropped: Arc::new(AtomicU64::new(0)),
            covers: None,
            feeder: None,
            air_toggled_at: None,
            stream_status: None,
            stream_checked_at: Instant::now(),
            sent_title: String::new(),
            underruns,
            xruns: XrunLog::new(Instant::now()),
            buffer_frames,
            device,
            levels_db: [0.0; 4],
            learn: None,
            fades: Default::default(),
            playlist_rect: egui::Rect::NOTHING,
            cart_rects: Vec::new(),
            deck_rects: [egui::Rect::NOTHING; woa_radio::DECKS],
            error_rect: egui::Rect::NOTHING,
            wish_rect: egui::Rect::NOTHING,
            error_details: false,
            row_rects: Vec::new(),
            row_drag: None,
            row_hover: None,
            row_selected: None,
            pos_edit: None,
            key_capture: None,
            rename: None,
            zone: Zone::CENTRAL_EUROPE,
            cue_editor: None,
            tag_wish: None,
            voice_editor: None,
            cue_seen: std::collections::HashMap::new(),
            manage: crate::manage::ManageWindow::default(),
            _pfl_output: crate::outputs::OutputWatch::spawn(pfl_output.into_iter().collect()),
            pfl_note,
        }
    }

    /// Läuft woa-streamd schon (Oberfläche nach Absturz neu gestartet), hängt sich die
    /// Oberfläche wieder an; woa-streamd blendet von der Notfall-Playlist zurück.
    fn reattach(mut self) -> Self {
        if self.station.is_some() && is_alive(&link_path(&self.data_dir)) {
            tracing::info!("streamd running, reattaching");
            self.attach_feeder();
        }
        self
    }

    fn attach_feeder(&mut self) {
        if self.feeder.is_some() || self.station.is_none() {
            return;
        }
        // Eine Sekunde Puffer; der Feeder leert ihn alle 20 ms.
        let (producer, consumer) = rtrb::RingBuffer::new(self.rate as usize * 2);
        match Feeder::spawn(consumer, self.rate, link_path(&self.data_dir)) {
            Ok(f) => {
                self.with(move |c| c.set_stream_output(Some(producer)));
                self.sent_title.clear();
                self.feeder = Some(f);
            }
            Err(e) => self.error = Some(format!("Sendung: {e}")),
        }
    }

    /// Zweiter Klick auf die Sendetaste binnen [`AIR_DEBOUNCE`] (hängendes Fenster, Doppelklick)
    /// zählt nicht (#367: „sendung ende ×2“, „sendung start ×3“ in drei Sekunden).
    fn air_bounce(&mut self) -> bool {
        let now = Instant::now();
        if self
            .air_toggled_at
            .is_some_and(|t| now.duration_since(t) < AIR_DEBOUNCE)
        {
            tracing::debug!("on-air toggle ignored (debounce)");
            return true;
        }
        self.air_toggled_at = Some(now);
        false
    }

    /// „Auf Sendung“: woa-streamd starten (falls nötig) und den Sendemix hinschicken.
    pub fn go_on_air(&mut self) {
        if self.feeder.is_some() || self.air_bounce() {
            return;
        }
        if self.station.is_none() {
            self.error = Some("Keine Sendesteuerung".into());
            return;
        }
        if !is_alive(&link_path(&self.data_dir))
            && let Err(e) = spawn_streamd(&self.data_dir)
        {
            tracing::error!(error = %e, "streamd spawn failed");
            self.error = Some(e);
            return;
        }
        tracing::info!("sendung start");
        self.attach_feeder();
    }

    /// „Sendung beenden“: woa-streamd meldet sich beim Server ab und endet.
    /// Wartet, bis woa-streamd sich abgemeldet hat; erst danach darf eine neue Sendung
    /// starten, sonst liefen zwei (#367).
    pub fn go_off_air(&mut self) {
        if self.air_bounce() {
            return;
        }
        tracing::info!("sendung ende");
        self.with(|c| c.set_stream_output(None));
        match self.feeder.take() {
            Some(f) => f.stop_broadcast(),
            None => {
                woa_stream::link::stop_daemon(&link_path(&self.data_dir), self.rate);
            }
        }
    }

    /// Läuft woa-streamd gerade (frische Status-Datei)?
    pub fn stream_running(&self) -> bool {
        self.stream_status
            .as_ref()
            .is_some_and(|s| s.is_running(woa_stream::unix_now()))
    }

    /// Zustand der Sendung (Status-Datei von woa-streamd), wenn er läuft.
    pub fn stream_status(&self) -> Option<&StreamStatus> {
        self.stream_status.as_ref()
    }

    fn tick_stream(&mut self) {
        let now = Instant::now();
        if now.duration_since(self.stream_checked_at).as_millis() >= 500 {
            self.stream_checked_at = now;
            let path = woa_stream::status::status_path(&self.data_dir);
            self.stream_status = woa_stream::status::read(&path);
        }
        if let Some(f) = &self.feeder {
            let title = self
                .view
                .current
                .and_then(|i| self.view.playlist.get(i))
                .map(|e| e.title.clone())
                .unwrap_or_default();
            if title != self.sent_title {
                f.set_title(&title);
                self.sent_title = title;
            }
        }
    }

    /// Aussetzer der Radio-Ausgabe seit dem Öffnen.
    pub fn underruns(&self) -> u64 {
        self.underruns.load(Ordering::Relaxed)
    }

    pub(crate) fn with(&self, f: impl FnOnce(&mut Controller) + Send + 'static) {
        if let Some(s) = &self.station {
            s.with(f);
        }
    }

    /// Tauscht das Kopfhörer-Gerät fürs Vorhören, ohne die Sendeausgänge anzufassen (#373/#374):
    /// Ein eigenes Gerät ist ein eigener Strom und darf auch bei laufender Playlist wechseln.
    /// Nur für `Aus` und `Geraet`; Kanal 3/4 hängt am Sendegerät (siehe `WoaApp`).
    pub fn swap_pfl_device(&mut self, pfl: &PflOutput) {
        tracing::info!(choice = ?pfl, "pfl: switching headphone device live");
        self.with(|c| c.set_pfl(None));
        // Alter Strom zu, bevor das neue Gerät aufgeht (dasselbe Gerät ginge sonst nicht auf).
        self._pfl_output = crate::outputs::OutputWatch::default();
        match pfl {
            PflOutput::Geraet(id) => match open_pfl_device(id, self.rate) {
                Ok((h, st)) => {
                    self.with(move |c| c.set_pfl(Some(h)));
                    self._pfl_output = crate::outputs::OutputWatch::spawn(vec![st]);
                    self.pfl_note = None;
                }
                Err(e) => {
                    self.error = Some(e.clone());
                    self.pfl_note = Some(e);
                }
            },
            _ => self.pfl_note = Some(PFL_OFF.into()),
        }
    }

    /// Rettungsknopf „Ausgabe neu verbinden“: öffnet Sendeausgänge und Vorhören sofort neu.
    /// Das Ergebnis steht oben im Sendepult.
    pub fn reconnect_outputs(&mut self) {
        // Im Wächter-Thread (#291); das Ergebnis meldet `tick`.
        let ok = self.outputs.reconnect_now();
        self._pfl_output.reconnect_now();
        self.error = if ok {
            None
        } else {
            Some("Neu verbinden hat nicht geklappt, WOA versucht es weiter".into())
        };
    }

    /// Ein Takt: Schnappschuss holen, speichern, fremde Änderungen an radio.json laden.
    /// `player_active`: Der Player-Strom läuft gleichzeitig (für das Log).
    pub fn tick(&mut self, player_active: bool) {
        // Die Ströme prüft und öffnet der Wächter-Thread neu (#291); hier nur sein Ergebnis.
        let main = self.outputs.watch.take_reconnected();
        let pfl = self._pfl_output.take_reconnected();
        if main == Some(false) || pfl == Some(false) {
            self.error = Some("Neu verbinden hat nicht geklappt, WOA versucht es weiter".into());
        }
        let Some(st) = &self.station else { return };
        self.view = st.view();
        self.phone_level = self.view.phone_peak.max(self.phone_level * 0.9);
        self.pult_level = self.view.pult_peak.max(self.pult_level * 0.9);
        self.pfl_level = self.view.pfl_peak.max(self.pfl_level * 0.85);
        for (m, v) in self.meters.iter_mut().zip(self.view.meters) {
            m.step(v, now_s());
        }
        for (m, v) in self.out_meters.iter_mut().zip(self.view.out_meters) {
            m.step(v, now_s());
        }
        let alarm_before = self.silent_stream.alarm();
        let alarm = self.silent_stream.step(
            self.feeder.is_some(),
            self.out_meters[0].level.max(self.pfl_level),
            self.out_meters[1].level,
            Instant::now(),
        );
        if alarm && !alarm_before {
            tracing::warn!(
                pult = self.pult.is_some(),
                "stream silent while outputs are audible"
            );
        }
        // Einmal je Sitzung: Carts, deren alte Taste jetzt ein Deck startet.
        if !self.deck_keys_checked && !self.view.cart_keys.is_empty() {
            self.deck_keys_checked = true;
            if let Some(note) = deck_key_note(&deck_key_conflicts(&self.view.cart_keys)) {
                tracing::warn!("cart keys on deck letters");
                self.error = Some(note);
            }
        }
        if self.hide_played == Some(self.view.hide_played) {
            self.hide_played = None;
        }
        if self.phone_lost.swap(false, Ordering::Relaxed) {
            self.phone = None;
            self.phone_stats = None;
            self.with(|c| c.set_phone_input(None));
            self.phone_error = Some(format!(
                "Der Telefon-Eingang {} ist ausgefallen (abgezogen?). Bitte anstecken und unter \
                 Einstellungen › Audio-Geräte „Übernehmen“ drücken.",
                self.phone_cfg.label()
            ));
        }
        self.tick_stream();
        for m in self.outputs.take_new_faults() {
            tracing::warn!(message = %m, "output lost");
            self.error = Some(match self.error.take() {
                Some(e) => format!("{e}\n{m}"),
                None => m,
            });
        }
        let total = self.underruns();
        if let Some(new) = self.xruns.due(total, Instant::now()) {
            let v = &self.view;
            let title = v.current.and_then(|i| v.playlist.get(i));
            let format = title
                .and_then(|e| e.path.extension())
                .map(|x| x.to_string_lossy().to_lowercase());
            let line = (
                v.last_load.as_ref().map(|(_, ms)| *ms),
                v.last_load
                    .as_ref()
                    .and_then(|(p, _)| p.extension())
                    .map(|x| x.to_string_lossy().to_lowercase()),
            );
            let fields = (title.map(|e| e.title.clone()), format);
            if new > 0 {
                tracing::warn!(
                    output = "radio",
                    underruns = new,
                    total,
                    buffer_frames = ?self.buffer_frames,
                    rate = self.rate,
                    device = %woa_telemetry::short_device(&self.device),
                    recording = v.recording.is_some(),
                    player_active,
                    playing = v.running,
                    title = ?fields.0,
                    format = ?fields.1,
                    last_load_ms = ?line.0,
                    last_load_format = ?line.1,
                    "output underruns (last minute)"
                );
            }
        }
        if self.station.is_none() {
            return;
        }
        let now = Instant::now();
        let mut check_now = false;
        if self.view.revision != self.saved_revision
            && now.duration_since(self.saved_at).as_millis() >= 700
        {
            if self.foreign_change() {
                // Erst die fremde Änderung (build_playlist) übernehmen, nicht überschreiben.
                self.saved_revision = self.view.revision;
                self.error = Some(
                    "radio.json wurde außerhalb geändert: neuer Stand übernommen, \
                     die letzte Änderung hier nicht gespeichert"
                        .into(),
                );
                check_now = true;
            } else {
                let show = self
                    .station
                    .as_ref()
                    .map(|st| st.show())
                    .unwrap_or_default();
                self.write_show(&show);
                if self.save_error.is_none() {
                    self.saved_revision = self.view.revision;
                }
            }
            self.saved_at = now;
        }
        if check_now || now.duration_since(self.checked_at).as_secs() >= 2 {
            self.checked_at = now;
            let m = mtime(&self.radio_path);
            if m.is_some() && m != self.known_mtime {
                // Changed outside (agent tool build_playlist): take it over.
                self.known_mtime = m;
                match RadioShow::load(&self.radio_path) {
                    Ok(show) => {
                        if let Some(st) = &self.station {
                            st.measure_missing(&show);
                        }
                        self.with(move |c| c.replace_show(show));
                    }
                    Err(e) => self.error = Some(e),
                }
            }
        }
    }

    /// Hat jemand anderes radio.json geändert, seit dieses Pult es gelesen oder geschrieben
    /// hat? Nur die Uhrzeit zählt nicht: Steht auf der Platte genau das, was wir zuletzt
    /// geschrieben haben, ist es unsere Datei (#380).
    fn foreign_change(&mut self) -> bool {
        let m = mtime(&self.radio_path);
        if m.is_none() || m == self.known_mtime {
            return false;
        }
        match (&self.last_written, RadioShow::load(&self.radio_path)) {
            (Some(ours), Ok(disk)) if *ours == disk => {
                self.known_mtime = m;
                false
            }
            _ => true,
        }
    }

    /// Schreibt `show` nach radio.json und merkt sich Stand und Uhrzeit.
    fn write_show(&mut self, show: &RadioShow) {
        match show.save(&self.radio_path) {
            Ok(()) => {
                self.known_mtime = mtime(&self.radio_path);
                self.last_written = RadioShow::load(&self.radio_path).ok();
                self.save_error = None;
            }
            Err(e) => {
                tracing::warn!(error = %e, "radio.json not saved");
                let msg = format!("radio.json nicht gespeichert: {e}");
                self.error = Some(msg.clone());
                self.save_error = Some(msg);
            }
        }
    }

    /// Sichert sofort, was noch nicht in radio.json steht (#380): beim Schließen von WOA und
    /// bevor das Pult neu geöffnet wird (Vorhören, Ausgänge). Vorher ging eine Änderung der
    /// letzten 0,7 s verloren, und ein neu geöffnetes Pult las den alten Stand.
    pub fn flush(&mut self) {
        let Some(st) = &self.station else { return };
        let show = st.show_synced(Duration::from_secs(1));
        let on_disk = self.last_written.as_ref() == Some(&show)
            || (self.last_written.is_none()
                && RadioShow::load(&self.radio_path).ok() == Some(show.clone()));
        if on_disk || self.foreign_change() {
            return;
        }
        self.write_show(&show);
        if self.save_error.is_none() {
            tracing::info!("radio.json flushed");
        }
    }

    /// Fehler beim Speichern von radio.json, falls der letzte Versuch scheiterte.
    pub fn save_error(&self) -> Option<&str> {
        self.save_error.as_deref()
    }

    /// Letzter Schnappschuss der Sendung (für den AutoDJ).
    pub fn view(&self) -> &View {
        &self.view
    }

    /// Reicht die Tabelle der Zeit-Events an den Sende-Thread (gleiche Tabelle: keine Wirkung).
    pub fn push_events(&self, table: woa_radio::events::EventTable) {
        if let Some(s) = &self.station {
            s.with(move |c| c.set_events(table));
        }
    }

    /// Hängt Dateien hinten an die Playlist (AutoDJ); vorhandene Einträge bleiben stehen.
    pub fn append_files(&self, paths: Vec<PathBuf>) {
        if let Some(s) = &self.station {
            s.add_files(paths);
        }
    }

    /// Meldungen aus dem Öffnen der Ausgänge (fehlende Geräte, Ersatz).
    pub fn output_warnings(&self) -> &[String] {
        &self.output_warnings
    }

    /// Zustand der Ringe zu den weiteren Geräten, eine Zeile je Ring.
    pub fn output_links(&self) -> Vec<String> {
        self.outputs.link_lines()
    }

    pub fn is_on_air(&self) -> bool {
        self.view.running
    }

    /// Auf Sendung (woa-streamd wird gespeist), ohne das Mikro.
    pub fn is_broadcasting(&self) -> bool {
        self.feeder.is_some()
    }

    /// Live für die ON-AIR-Lampe: nur „Auf Sendung“ (Feeder läuft), nicht das Mikro (#225).
    pub fn is_live(&self) -> bool {
        onair_lamp_lit(self.feeder.is_some(), self.view.mic_open)
    }

    fn add_dialog(&self) {
        if let Some(files) = crate::ordner::dialog_musik()
            .add_filter(
                woa_engine::decode::FILTER_NAME,
                &woa_engine::decode::dialog_extensions(),
            )
            .pick_files_modal()
            && let Some(s) = &self.station
        {
            s.add_files(files);
        }
    }

    /// Pegel eines Zweigs (dB).
    pub fn level_db(&self, b: Bus) -> f64 {
        self.levels_db[bus_index(b)]
    }

    /// Pegel eines Zweigs setzen (Fader am Controller oder Regler im Fenster).
    pub fn set_level(&mut self, b: Bus, db: f64) {
        let db = db.clamp(
            woa_radio::mixer::LEVEL_MIN_DB,
            woa_radio::mixer::LEVEL_MAX_DB,
        );
        if self.levels_db[bus_index(b)] == db {
            return;
        }
        self.levels_db[bus_index(b)] = db;
        let bus = level_bus(b);
        self.with(move |c| c.set_level(bus, db));
    }

    /// Pegel je Ausgang für Agenten (MCP `output_levels`): Notebook, Stream, Vorhören;
    /// linear, wie die LED-Meter (Spitze, fällt langsam ab).
    pub fn output_levels(&self) -> [f32; 3] {
        [
            self.out_meters[0].level,
            self.out_meters[1].level,
            self.pfl_level,
        ]
    }

    /// Taster/Pad vom Controller.
    pub fn press(&mut self, t: Target) {
        match t {
            Target::Mic => self.toggle_mic(),
            Target::StartNext => self.with(|c| c.start_or_next()),
            Target::Stop => self.with(|c| c.stop_or_pause()),
            Target::Cart(slot) if slot < CARTS => self.with(move |c| c.trigger_cart(slot)),
            // Decks A–C (André 02.11.2026): wie die Tasten am Deck.
            Target::Deck(k, DeckAction::Start) => {
                let Some(d) = self.view.decks.get(k) else {
                    return;
                };
                if d.playing {
                    self.with(move |c| c.deck_stop(k));
                } else {
                    self.with(move |c| c.deck_play(k));
                }
            }
            Target::Deck(k, DeckAction::Cue) if k < self.view.decks.len() => {
                self.with(move |c| c.deck_cue(k))
            }
            Target::Deck(k, DeckAction::Fade) if k < self.view.decks.len() => {
                self.with(move |c| c.fade_deck(k))
            }
            Target::Deck(k, DeckAction::Pfl) if k < self.view.decks.len() => self.with(move |c| {
                if let Err(e) = c.pfl_toggle(PflSource::Deck(k)) {
                    c.report(e);
                }
            }),
            _ => {}
        }
    }

    /// Wirkung eines Party-Befehls auf die Sendung (Paket 35). Das Vollbild macht die App.
    /// Party-Decks an (Party-Modus) oder aus. Jeden Frame aufrufen; geschickt wird nur beim
    /// Wechsel. Beim Betreten übernimmt die linke Seite einen laufenden Titel, beim Verlassen
    /// die Playlist die hörbare Seite.
    pub fn set_party_mode(&mut self, on: bool) {
        if on != self.party_on {
            self.party_on = on;
            if on {
                self.with(|c| c.party_enter());
            } else {
                self.with(|c| c.party_leave());
            }
        }
    }

    /// Befehle der Party-Decks (Tasten, Handy) im Party-Modus.
    fn party_duo_effect(&mut self, e: woa_radio::party::Effect) {
        use woa_radio::party::Effect;
        match e {
            Effect::StartOrNext => self.with(|c| c.party_next()),
            Effect::Stop | Effect::Pause => self.with(|c| c.party_pause_all()),
            Effect::Play => self.with(|c| {
                if !c.party_playing() {
                    c.party_toggle();
                }
            }),
            Effect::Previous => self.with(|c| c.party_previous()),
            Effect::Restart => self.with(|c| c.party_restart()),
            Effect::Seek(s) => self.with(move |c| c.party_seek(s)),
            Effect::CycleRepeat => self.with(|c| c.cycle_repeat()),
            Effect::ToggleShuffle => self.with(|c| c.toggle_shuffle()),
            Effect::MusicLevel(db) => self.set_level(Bus::Music, db),
            Effect::InsertNext(path) => self.with(move |c| {
                // Hinter den hörbaren Titel: die freie Seite lädt ihn dann vor.
                let at = c.party_live_index().map_or(0, |i| i + 1);
                let at = at.min(c.show().playlist.len());
                c.add(&[path], Some(at));
            }),
            Effect::Fullscreen(_) => {}
        }
    }

    /// Crossfader der Party von Hand (0 links, 1 rechts).
    pub fn party_fader(&self, pos: f32) {
        self.with(move |c| c.party_fader(pos));
    }

    /// ▶ Play bzw. Pause an einer Party-Seite.
    /// Ein Kommando der DJ-Ansicht (DJ-Paket); Fehler erscheinen als Meldung.
    pub fn dj(&self, cmd: woa_radio::control::DjCommand) {
        self.with(move |c| {
            if let Err(e) = c.dj(cmd) {
                c.report(e);
            }
        });
    }

    /// Führt DJ-Kommandos des Agenten aus und liefert den DJ-Zustand (wartet höchstens 2 s
    /// auf die Sendung).
    pub fn dj_agent(
        &self,
        cmds: Vec<woa_radio::control::DjCommand>,
    ) -> Result<serde_json::Value, String> {
        if self.station.is_none() {
            return Err("Keine Sendung offen: Party einschalten".into());
        }
        let (tx, rx) = std::sync::mpsc::channel();
        self.with(move |c| {
            let mut r = Ok(());
            for cmd in cmds {
                if let Err(e) = c.dj(cmd) {
                    r = Err(e);
                    break;
                }
            }
            let out = r.and_then(|()| {
                let v = c
                    .dj_view()
                    .ok_or_else(|| "Party ist nicht an".to_string())?;
                serde_json::to_value(v).map_err(|e| e.to_string())
            });
            let _ = tx.send(out);
        });
        rx.recv_timeout(std::time::Duration::from_secs(2))
            .unwrap_or_else(|_| Err("Die Sendung antwortet nicht".into()))
    }

    pub fn party_side(&self, side: usize, play: bool) {
        self.with(move |c| {
            if play {
                c.party_play(side);
            } else {
                c.party_pause(side);
            }
        });
    }

    /// Schalter „Auto-Übergang“ (wird mit der Sendung gespeichert).
    pub fn set_party_auto(&self, on: bool) {
        self.with(move |c| c.set_party_auto(on));
    }

    /// Stand des Wunschtitel-Ordners (#134), falls einer beobachtet wird.
    pub fn wish_folder_list(&self) -> Option<crate::wishes::WishList> {
        self.wishes.as_ref().map(|w| w.list())
    }

    pub fn party_effect(&mut self, e: woa_radio::party::Effect) {
        use woa_radio::party::Effect;
        match e {
            _ if self.party_on => self.party_duo_effect(e),
            Effect::StartOrNext => self.with(|c| c.start_or_next()),
            Effect::Stop => self.with(|c| c.stop()),
            Effect::Pause => self.with(|c| c.pause()),
            Effect::Play => self.with(|c| c.play()),
            Effect::Previous => self.with(|c| c.previous()),
            Effect::Restart => self.with(|c| c.restart()),
            Effect::Seek(s) => self.with(move |c| c.seek(s)),
            Effect::CycleRepeat => self.with(|c| c.cycle_repeat()),
            Effect::ToggleShuffle => self.with(|c| c.toggle_shuffle()),
            Effect::MusicLevel(db) => self.set_level(Bus::Music, db),
            Effect::InsertNext(path) => self.with(move |c| {
                // Hinter den laufenden Titel, sonst hinter den Titel, der als Nächstes startet.
                let at = match c.current_index() {
                    Some(i) => i + 1,
                    None if c.show().playlist.is_empty() => 0,
                    None => c.cue_index() + 1,
                };
                c.add(&[path], Some(at));
            }),
            Effect::Fullscreen(_) => {}
        }
    }

    /// LED-Zustand eines Ziels.
    pub fn lit(&self, t: &Target) -> bool {
        match t {
            Target::Mic => self.view.mic_open,
            Target::StartNext => self.view.running,
            Target::Cart(slot) => *slot < 32 && self.view.carts_playing & (1 << slot) != 0,
            Target::Deck(k, DeckAction::Start) => {
                self.view.decks.get(*k).is_some_and(|d| d.playing)
            }
            _ => false,
        }
    }

    fn learn_menu(&mut self, r: &egui::Response, t: Target) {
        r.context_menu(|ui| {
            if let Some(t) = crate::controls::learn_menu(ui, t, None) {
                self.learn = Some(t);
            }
        });
    }

    /// Pult-Modus an: Die Sendung kommt vom Eingang des externen Mischpults (#301).
    pub fn pult_mode(&self) -> bool {
        self.pult.is_some()
    }

    fn toggle_mic(&mut self) {
        if self.pult.is_some() {
            // Das Mikro hängt am Pult; WOAs Mikro ginge sonst zurück ins Pult (Rückkopplung).
            self.error = Some(
                "Mikrofon im Pult-Modus aus: Das Mikro hängt am Pult und kommt mit dem \
                 Pult-Eingang auf Sendung."
                    .into(),
            );
            return;
        }
        if self.mic.is_none() && !self.open_mic() {
            return;
        }
        let open = !self.view.mic_open;
        self.view.mic_open = open;
        self.with(move |c| c.set_mic_open(open));
    }

    /// Öffnet das gewählte Mikrofon (Fenster Audio-Geräte; ohne Wahl der Systemstandard).
    /// `false` mit Meldung, wenn es nicht geht; nie ein anderes Gerät stillschweigend.
    fn open_mic(&mut self) -> bool {
        let (producer, consumer) = rtrb::RingBuffer::new(self.rate as usize);
        match woa_engine::record::start_live_input(
            &self.mic_cfg.input_config(),
            producer,
            Arc::clone(&self.mic_dropped),
            |e| tracing::warn!(error = %e, "mic input"),
        ) {
            Ok((stream, rate)) if rate == self.rate => {
                tracing::info!(device = %self.mic_cfg.label(), "mic input open");
                self.mic = Some(Box::new(stream));
                self.with(move |c| c.set_mic_input(Some(consumer)));
                true
            }
            Ok((_, rate)) => {
                self.error = Some(format!(
                    "Mikrofon {} läuft mit {rate} Hz, die Ausgabe mit {} Hz. Bitte im System \
                     gleich einstellen.",
                    self.mic_cfg.label(),
                    self.rate
                ));
                false
            }
            Err(e) => {
                self.error = Some(format!(
                    "Mikrofon {} lässt sich nicht öffnen oder fehlt ({e}). Einstellungen › \
                     Audio-Geräte, dort unter Eingänge das Mikrofon wählen.",
                    self.mic_cfg.label()
                ));
                false
            }
        }
    }

    /// Neue Wahl des Mikrofons, sofort wirksam: Ein offenes Mikro wechselt auf das neue Gerät;
    /// geht es nicht, ist das Mikro zu (mit Meldung).
    pub fn set_mic_config(&mut self, cfg: crate::outputs_ui::MicInput) {
        if cfg == self.mic_cfg {
            return;
        }
        self.mic_cfg = cfg;
        if self.mic.is_none() {
            return;
        }
        self.mic = None;
        self.with(|c| c.set_mic_input(None));
        if self.view.mic_open && !self.open_mic() {
            self.view.mic_open = false;
            self.with(|c| c.set_mic_open(false));
        }
    }

    /// Neue Einstellung für den Telefon-Eingang: schließt den alten und öffnet den neuen.
    /// Ein fehlendes Gerät gibt eine klare Meldung, die Sendung läuft weiter.
    pub fn set_phone_config(&mut self, cfg: crate::phone::PhoneInput) {
        self.phone = None;
        self.phone_stats = None;
        self.phone_error = None;
        self.phone_cfg = cfg;
        self.with(|c| c.set_phone_input(None));
        self.open_phone();
    }

    fn open_phone(&mut self) {
        let Some(config) = self.phone_cfg.input_config() else {
            return;
        };
        let lost = Arc::clone(&self.phone_lost);
        match woa_engine::record::start_pair_input(&config, self.rate, move |e| {
            tracing::warn!(error = %e, "phone input");
            lost.store(true, Ordering::Relaxed);
        }) {
            Ok((stream, rx, stats, rate)) => {
                tracing::info!(rate, device = %self.phone_cfg.label(), "phone input open");
                self.phone = Some(Box::new(stream));
                self.phone_stats = Some(stats);
                self.with(move |c| c.set_phone_input(Some(rx)));
            }
            Err(e) => {
                self.phone_error = Some(format!(
                    "Telefon-Eingang {} lässt sich nicht öffnen oder fehlt ({e}). Die Sendung \
                     läuft weiter. Bitte anstecken und unter Einstellungen › Audio-Geräte „Übernehmen“ drücken.",
                    self.phone_cfg.label()
                ));
            }
        }
    }

    /// „Telefon auf Sendung“: der Anrufer liegt auf der Sendung, die Musik geht runter.
    fn toggle_phone(&mut self) {
        let on = !self.view.phone_on;
        if on && self.phone.is_none() {
            if self.phone_cfg.device_id.is_none() {
                self.phone_error = Some(
                    "Kein Telefon-Eingang gewählt. Einstellungen › Audio-Geräte, dort unter \
                     Eingänge Gerät und Kanalpaar wählen."
                        .into(),
                );
                return;
            }
            self.phone_error = None;
            self.open_phone();
            if self.phone.is_none() {
                return;
            }
        }
        self.view.phone_on = on;
        self.with(move |c| c.set_phone_on(on));
    }

    /// Zeilen fürs Telefon-Fenster: Meldung, Zähler der Taktverbindung.
    pub fn phone_lines(&self) -> Vec<String> {
        let mut v = Vec::new();
        if let Some(e) = &self.phone_error {
            v.push(e.clone());
        }
        if let Some(s) = &self.phone_stats {
            v.push(format!(
                "Eingang läuft. Aussetzer: {} (Ring leer {}, übersprungen {}, verworfen {})",
                s.faults(),
                s.underruns.load(Ordering::Relaxed),
                s.skipped.load(Ordering::Relaxed),
                s.dropped.load(Ordering::Relaxed),
            ));
        }
        v
    }

    /// Wunschordner wählen (`None` = der Ordner `Wunschtitel` im Datenordner der Sendung).
    pub fn set_wish_folder(&mut self, folder: Option<PathBuf>) {
        let dir = folder.unwrap_or_else(|| self.data_dir.join("Wunschtitel"));
        if self.wishes.as_ref().is_some_and(|w| w.dir() == dir) {
            return;
        }
        self.wishes = Some(crate::wishes::WishWatcher::start(
            dir,
            Duration::from_secs(1),
        ));
    }

    /// Der Wunschordner, den die App speichern soll, wenn er gewählt wurde.
    pub fn wish_folder(&self) -> Option<&Path> {
        self.wishes.as_ref().map(|w| w.dir())
    }

    /// Die Wunschtitel: Liste mit einem Klick in die Playlist oder auf ein Deck.
    fn wish_card(&mut self, ui: &mut egui::Ui) {
        // Eine Zeile (ScreenRadio): Kopf, Ordner, Zahl der Wünsche, Ordnerwahl; die Wünsche
        // selbst stehen nur darunter, wenn es welche gibt.
        let list = self
            .wishes
            .as_ref()
            .map(|w| (w.dir().display().to_string(), w.list()));
        ui.horizontal(|ui| {
            ui.label(theme::panel_label("WUNSCHTITEL"));
            match &list {
                None => {
                    ui.label(
                        egui::RichText::new("Kein Wunschordner.")
                            .small()
                            .color(theme::text_dim()),
                    );
                }
                Some((dir, l)) => {
                    let n = l.files.len();
                    let fresh = l.fresh.len();
                    let t = match (n, fresh) {
                        (0, _) => "Noch kein Wunsch im Ordner.".to_string(),
                        (n, 0) => format!("{n} im Ordner"),
                        (n, f) => format!("{n} im Ordner, {f} neu"),
                    };
                    ui.label(egui::RichText::new(t).small().color(if fresh > 0 {
                        theme::accent()
                    } else {
                        theme::text_dim()
                    }));
                    ui.add(
                        egui::Label::new(egui::RichText::new(dir).small().color(theme::text_dim()))
                            .truncate(),
                    );
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if small_key(ui, theme::icf::FOLDER, "Ordner")
                    .on_hover_text(
                        "Wunschordner wählen: Alles, was hier hineinkommt, erscheint sofort in \
                         der Liste",
                    )
                    .clicked()
                    && let Some(dir) = crate::ordner::dialog_musik().pick_folder_modal()
                {
                    self.set_wish_folder(Some(dir.clone()));
                    self.wish_chosen = Some(dir);
                }
            });
        });
        let Some((_, list)) = list else {
            return;
        };
        if let Some(e) = &list.error {
            ui.colored_label(theme::on_air(), e);
        }
        // Die Liste ändert sich ohne Eingabe: ab und zu neu zeichnen.
        ui.ctx().request_repaint_after(Duration::from_secs(1));
        let mut act: Option<(PathBuf, Option<usize>)> = None;
        egui::ScrollArea::vertical()
            .id_salt("wishes")
            .max_height(150.0)
            .show(ui, |ui| {
                for f in &list.files {
                    let fresh = list.fresh.contains(&f.path);
                    ui.horizontal(|ui| {
                        let mut name = egui::RichText::new(&f.name).color(if f.copying {
                            theme::text_dim()
                        } else {
                            theme::text()
                        });
                        if fresh {
                            name = name.strong();
                        }
                        ui.add(egui::Label::new(name).truncate())
                            .on_hover_text(f.path.display().to_string());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if f.copying {
                                ui.label(
                                    egui::RichText::new("wird kopiert …")
                                        .small()
                                        .color(theme::text_dim()),
                                );
                                return;
                            }
                            for k in (0..woa_radio::DECKS).rev() {
                                if ui
                                    .small_button(woa_radio::show::deck_name(k).to_string())
                                    .on_hover_text("Auf dieses Deck laden")
                                    .clicked()
                                {
                                    act = Some((f.path.clone(), Some(k)));
                                }
                            }
                            if ui
                                .small_button("In die Playlist")
                                .on_hover_text("Direkt nach dem laufenden Titel einreihen")
                                .clicked()
                            {
                                act = Some((f.path.clone(), None));
                            }
                            if fresh {
                                ui.label(egui::RichText::new("neu").small().color(theme::accent()));
                            }
                        });
                    });
                }
            });
        match act {
            Some((path, Some(k))) => self.deck_load_file(k, path),
            Some((path, None)) => self.party_effect(woa_radio::party::Effect::InsertNext(path)),
            None => {}
        }
    }

    /// Der Ordner, den der Nutzer eben gewählt hat (die App speichert ihn einmal).
    /// Titel, dessen Titelinfos im Kontextmenü der Playlist gewählt wurden (einmal).
    pub fn take_tag_wish(&mut self) -> Option<PathBuf> {
        self.tag_wish.take()
    }

    pub fn take_wish_choice(&mut self) -> Option<PathBuf> {
        self.wish_chosen.take()
    }

    /// Laufender Mitschnitt: Ziel-Datei und Beginn (für die REC-Pille der Kopfleiste).
    pub fn recording(&self) -> Option<(Option<&std::path::Path>, Option<std::time::Instant>)> {
        (self.view.recording_on || self.view.recording.is_some())
            .then_some((self.view.recording.as_deref(), self.rec_since))
    }

    /// Ordner der Mitschnitte.
    pub fn recordings_dir(&self) -> PathBuf {
        crate::ordner::dir(crate::ordner::Art::Mitschnitte)
    }

    /// Startet oder beendet den Mitschnitt der Sendesumme (REC in der Kopfleiste).
    pub fn toggle_recording(&mut self) {
        let on = self.view.recording_on || self.view.recording.is_some();
        self.rec_since = if on {
            None
        } else {
            Some(std::time::Instant::now())
        };
        if on {
            self.with(|c| {
                c.stop_recording();
            });
        } else {
            let dir = self.recordings_dir();
            self.with(move |c| c.start_recording_in(&dir));
        }
    }

    /// Tasten im Radio-Modus (nur, wenn kein Textfeld den Fokus hat).
    pub fn shortcuts(&mut self, ctx: &egui::Context) {
        // Abgelegte Dateien zuerst: ein Textfeld mit Fokus darf sie nicht schlucken (#166).
        self.drop_os_files(ctx);
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        let n = egui::Modifiers::NONE;
        // Neue Taste für ein Cart: die nächste Taste gilt (Esc bricht ab, Entf nimmt die Taste weg).
        if let Some(slot) = self.key_capture {
            let pressed = ctx.input(|i| {
                i.events.iter().find_map(|e| match e {
                    egui::Event::Key {
                        key,
                        pressed: true,
                        repeat: false,
                        ..
                    } => Some(*key),
                    _ => None,
                })
            });
            if let Some(key) = pressed {
                self.key_capture = None;
                match key {
                    egui::Key::Escape => {}
                    egui::Key::Delete | egui::Key::Backspace => {
                        self.with(move |c| {
                            let _ = c.set_cart_key(slot, None);
                        });
                    }
                    k if RESERVED_KEYS.contains(&k) => {
                        self.error = Some(format!(
                            "Die Taste {} ist belegt (M: Mikro, Leertaste: Start/Weiter, F11: Vollbild, A–C: Decks)",
                            k.name()
                        ));
                    }
                    k => {
                        let name = k.name();
                        self.with(move |c| {
                            if let Err(e) = c.set_cart_key(slot, Some(name)) {
                                c.report(e);
                            }
                        });
                    }
                }
            }
            ctx.input_mut(|i| {
                i.events
                    .retain(|e| !matches!(e, egui::Event::Key { .. } | egui::Event::Text(_)));
            });
            return;
        }
        let keys = self.view.cart_keys.clone();
        let (mic, go, carts, decks) = ctx.input_mut(|i| {
            let carts: Vec<usize> = keys
                .iter()
                .enumerate()
                .filter_map(|(slot, k)| Some((slot, cart_key_of(k.as_deref()?)?)))
                .filter(|(_, k)| i.consume_key(n, *k))
                .map(|(slot, _)| slot)
                .collect();
            let decks: Vec<usize> = (0..woa_radio::DECKS)
                .filter(|&k| i.consume_key(n, DECK_KEYS[k]))
                .collect();
            (
                i.consume_key(n, egui::Key::M),
                i.consume_key(n, egui::Key::Space),
                carts,
                decks,
            )
        });
        // A, B, C: Deck starten, läuft es, stoppen (wie die Start-Taste am Deck).
        for k in decks {
            let playing = self.view.decks.get(k).is_some_and(|d| d.playing);
            if playing {
                self.with(move |c| c.deck_stop(k));
            } else {
                self.with(move |c| c.deck_play(k));
            }
        }
        for slot in carts {
            self.with(move |c| c.trigger_cart(slot));
        }
        if mic {
            self.toggle_mic();
        }
        if go {
            // Leertaste: ein Knopf, läuft es, dann Pause, sonst Play.
            let running = self.view.running;
            for e in woa_radio::party::PartyState::default()
                .apply(woa_radio::party::PartyCommand::PlayPause, running)
            {
                self.party_effect(e);
            }
        }
    }

    /// Aufs Fenster gezogene Dateien: auf ein Deck oder Cart-Pad die erste, sonst alle in die
    /// Playlist.
    fn drop_os_files(&mut self, ctx: &egui::Context) {
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .filter_map(|f| f.path.clone())
                .collect()
        });
        if !dropped.is_empty() {
            let pos = ctx.input(|i| i.pointer.latest_pos());
            let deck = pos.and_then(|p| self.deck_rects.iter().position(|r| r.contains(p)));
            let cart = pos.and_then(|p| self.cart_rects.iter().position(|r| r.contains(p)));
            if let Some(k) = deck {
                if let Some(path) = dropped.into_iter().next() {
                    self.deck_load_file(k, path);
                }
            } else if let Some(slot) = cart {
                if let Some(path) = dropped.into_iter().next() {
                    self.assign_cart_file(slot, path);
                }
            } else if let Some(s) = &self.station {
                s.add_files(dropped);
            }
        }
    }

    /// Zeichnet das ganze Layout in `ui`.
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        let v = self.view.clone();
        self.fades.set_seconds(v.fade_seconds);
        // Fade-Anzeige: abgelaufene Blenden abräumen, laufende neu zeichnen; steht ein Deck
        // schon (von Hand gestoppt), endet seine Anzeige.
        if std::env::var_os("WOA_FADE").is_some() {
            // Fotomodus „blendet aus“: nur Anzeige, kein Befehl an den Mixer, kein Ton.
            let now = ui.input(|i| i.time);
            if self.fades.progress(FadeKey::All, now).is_none() {
                self.fades.press(FadeKey::All, now - 1.9);
            }
        }
        if self.fades.tick(ui.input(|i| i.time)) {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(50));
        }
        for (k, d) in v.decks.iter().enumerate() {
            if !d.playing {
                self.fades.clear(FadeKey::Deck(k));
            }
        }
        if let Some(e) = self.error.clone() {
            // Eine Zeile mit Symbol (Design-Runde 3); der ganze Text im Tooltip und hinter
            // „Details“.
            let first = e.lines().next().unwrap_or_default().to_string();
            let more = e.lines().count() > 1 || first.chars().count() > 120;
            let r = egui::Frame::new()
                .fill(theme::mix(theme::on_air(), theme::well(), 0.82))
                .stroke(egui::Stroke::new(1.0, theme::on_air()))
                .corner_radius(theme::RADIUS_MD)
                .inner_margin(egui::Margin::symmetric(10, 4))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(theme::icf::WARNING)
                                .family(egui::FontFamily::Name(theme::FAMILY_ICON_FILL.into()))
                                .size(15.0)
                                .color(theme::on_air()),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .small_button(theme::ic::X)
                                .on_hover_text("Meldung schließen")
                                .clicked()
                            {
                                self.error = None;
                                self.error_details = false;
                            }
                            if more {
                                let d = ui.selectable_label(self.error_details, "Details");
                                d.widget_info(|| {
                                    egui::WidgetInfo::labeled(
                                        egui::WidgetType::Button,
                                        true,
                                        "Details",
                                    )
                                });
                                if d.clicked() {
                                    self.error_details = !self.error_details;
                                }
                            }
                            ui.with_layout(
                                egui::Layout::left_to_right(egui::Align::Center),
                                |ui| {
                                    ui.add(
                                        egui::Label::new(
                                            egui::RichText::new(&first).color(theme::text()),
                                        )
                                        .truncate(),
                                    )
                                    .on_hover_text(&e);
                                },
                            );
                        });
                    });
                    if self.error_details {
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(&e).small().color(theme::text_dim()),
                            )
                            .wrap(),
                        );
                    }
                });
            self.error_rect = r.response.rect;
            ui.add_space(6.0);
        } else {
            self.error_rect = egui::Rect::NOTHING;
        }
        for m in &v.messages {
            ui.label(egui::RichText::new(m).small().color(theme::text_dim()));
        }
        ui.columns(2, |cols| {
            cols[0].spacing_mut().item_spacing.y = 10.0;
            cols[1].spacing_mut().item_spacing.y = 10.0;
            theme::card_frame().show(&mut cols[0], |ui| {
                self.clock_and_controls(ui, &v);
            });
            self.playlist_rect = theme::card_frame()
                .show(&mut cols[0], |ui| {
                    self.playlist(ui, &v);
                })
                .response
                .rect;
            // Die rechte Spalte rollt: sonst liegen Rack (Crossfade, Ducking) und Gäste
            // unter dem Fensterrand und sind nicht erreichbar (#168).
            egui::ScrollArea::vertical()
                .id_salt("radio_right_column")
                .auto_shrink([false, false])
                .show(&mut cols[1], |ui| {
                    ui.spacing_mut().item_spacing.y = 10.0;
                    theme::card_frame().show(ui, |ui| {
                        self.deck_card(ui, &v);
                    });
                    // Zweites oben: die Cart-Wall, ganz sichtbar auch bei 1366×768 (#303).
                    theme::card_frame().show(ui, |ui| {
                        self.cart_wall(ui, &v);
                    });
                    // Pegel und Processing nebeneinander (ScreenRadio), schmal untereinander.
                    if ui.available_width() >= LEVELS_SIDE_BY_SIDE {
                        ui.horizontal_top(|ui| {
                            let w = ui.available_width();
                            let left = theme::card_frame()
                                .show(ui, |ui| ui.vertical(|ui| self.levels_card(ui)))
                                .response
                                .rect
                                .width();
                            let rest = w - left - ui.spacing().item_spacing.x;
                            ui.allocate_ui_with_layout(
                                egui::vec2(rest, 0.0),
                                egui::Layout::top_down(egui::Align::Min),
                                |ui| {
                                    theme::card_frame().show(ui, |ui| {
                                        ui.set_width(ui.available_width());
                                        self.processing_rack(ui);
                                    });
                                },
                            );
                        });
                    } else {
                        theme::card_frame().show(ui, |ui| self.levels_card(ui));
                        theme::card_frame().show(ui, |ui| self.processing_rack(ui));
                    }
                    self.wish_rect = theme::card_frame()
                        .show(ui, |ui| {
                            self.wish_card(ui);
                        })
                        .response
                        .rect;
                    theme::card_frame().show(ui, |ui| {
                        self.guests_card(ui);
                    });
                });
        });
    }

    fn clock_and_controls(&mut self, ui: &mut egui::Ui, v: &View) {
        let idx = v.current.or((!v.running).then_some(v.cue));
        let entry = idx.and_then(|i| v.playlist.get(i));
        let title = v
            .current
            .and_then(|i| v.playlist.get(i))
            .map(|e| e.title.clone())
            .unwrap_or_else(|| {
                entry
                    .map(|e| format!("Bereit: {}", e.title))
                    .unwrap_or_else(|| "– Kein Titel geladen –".to_string())
            });
        let total_title_secs = entry.and_then(|e| e.seconds).unwrap_or(0.0);
        // Das große Cover entfällt (ScreenRadio: die Studio-Uhr steht vorn); die kleinen Cover
        // der Playlist laden weiter.
        let ctx = ui.ctx().clone();
        let covers = self
            .covers
            .get_or_insert_with(|| crate::covers::Covers::new(&ctx));
        covers.poll(&ctx);

        // Modul-Kopfzeile: Status-Badges links, Sendestatus rechts.
        ui.horizontal(|ui| {
            ui.label(
                theme::panel_label("SENDE-DECK & MASTER-UHR"),
            );
            if v.running {
                theme::status_pill(ui, "DECK LÄUFT", theme::ok(), true);
            } else {
                theme::status_pill(ui, "BEREIT / CUE", theme::accent(), false);
            }
            if v.mic_open {
                theme::status_pill(ui, "MIKRO OFFEN", theme::on_air(), true);
            }
            if v.recording.is_some() {
                theme::status_pill(ui, "REC LÄUFT", theme::on_air(), true);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // Die ON-AIR-Lampe sitzt in der Kopfleiste neben MIDI (André 31.10.).
                let n = self.underruns();
                if n > 0 {
                    ui.label(
                        egui::RichText::new(format!("Aussetzer: {n}"))
                            .small()
                            .color(theme::danger_text()),
                    )
                    .on_hover_text(
                        "Puffer-Unterläufe der Ausgabe seit dem Öffnen. Steigt die Zahl, \
                         in config.json radio_buffer_frames setzen, etwa 2048 oder 4096 (Vorgabe: 1024).",
                    );
                }
            });
        });
        ui.add_space(4.0);

        // Haupt-Display: Studio-Uhr links, LCD-Cockpit rechts.
        ui.horizontal(|ui| {
            // Studio-Uhr: 60 Sekunden-LEDs und Digitalzeit (Ortszeit der Stundenuhr).
            let (h, m, s) = time_parts(crate::clock_editor::now_unix(), self.zone);
            crate::widgets::studio_clock(ui, 152.0, h, m, s);

            theme::lcd_show(ui, |ui| {
                ui.set_min_height(142.0);
                ui.set_width(ui.available_width());
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        let badge_text = if v.running {
                            "AKTUELL ON AIR"
                        } else {
                            "NÄCHSTER START (CUE)"
                        };
                        let badge_col = if v.running {
                            theme::accent()
                        } else {
                            theme::cue()
                        };
                        ui.label(
                            egui::RichText::new(badge_text)
                                .size(10.5)
                                .strong()
                                .monospace()
                                .color(badge_col),
                        );
                        if let Some(lufs) = entry.and_then(|e| e.lufs) {
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.label(
                                        egui::RichText::new(format!("{lufs:.1} LUFS"))
                                            .size(11.0)
                                            .monospace()
                                            .color(theme::text_dim()),
                                    );
                                },
                            );
                        }
                    });
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(title)
                                .size(17.5)
                                .strong()
                                .color(theme::text()),
                        )
                        .truncate(),
                    );
                    ui.add_space(2.0);

                    // Die Uhr zeigt die Studio-Uhr links; hier nur der Takt für ihre Sekunden.
                    ui.ctx()
                        .request_repaint_after(std::time::Duration::from_millis(250));

                    let urgent = v.running && v.remaining_title < 10.0;
                    let clock_col = if urgent {
                        theme::on_air()
                    } else {
                        theme::accent()
                    };
                    ui.horizontal(|ui| {
                        seg_text(ui, &clock(v.remaining_title), 54.0, clock_col)
                            .on_hover_text("Restzeit des laufenden Titels (bis Cue-Out)");
                        if let Some(r) = v.ramp_remaining {
                            ui.add_space(8.0);
                            ui.label(
                                egui::RichText::new(format!("RAMP {:.0}", r.ceil()))
                                    .size(28.0)
                                    .strong()
                                    .monospace()
                                    .color(theme::accent()),
                            )
                            .on_hover_text(
                                "Sekunden bis zum Gesang (Ramp-Marke oder geschätzter \
                                 Gesangseinsatz): bis dahin sprechen",
                            );
                        }

                        ui.add_space(10.0);
                        ui.vertical(|ui| {
                            ui.add_space(10.0);
                            ui.label(
                                egui::RichText::new("bis Playlist-Ende")
                                    .size(11.0)
                                    .color(theme::text_dim()),
                            );
                            ui.label(
                                egui::RichText::new(clock(v.remaining_total))
                                    .size(24.0)
                                    .strong()
                                    .monospace()
                                    .color(theme::text()),
                            );
                        });
                    });

                    if let Some((name, secs, hard)) = &v.next_event {
                        let near = *hard && *secs < 30.0;
                        ui.label(
                            egui::RichText::new(format!(
                                "NÄCHSTES EVENT  {name}  in {}  ({})",
                                clock(*secs),
                                if *hard { "hart" } else { "weich" }
                            ))
                            .size(12.0)
                            .monospace()
                            .color(if near {
                                theme::on_air()
                            } else {
                                theme::text_dim()
                            }),
                        )
                        .on_hover_text("Zeit-Events: Fenster „Zeit-Events“");
                    }
                    if let Some((name, dev)) = &v.last_event {
                        ui.label(
                            egui::RichText::new(format!(
                                "ZULETZT  {name}  {dev:+.1} s gegen die Zeit"
                            ))
                            .size(11.0)
                            .monospace()
                            .color(theme::text_dim()),
                        );
                    }
                    // Visueller Restzeit-Balken im LCD
                    let progress = if total_title_secs > 0.1 {
                        ((total_title_secs - v.remaining_title) / total_title_secs).clamp(0.0, 1.0)
                            as f32
                    } else if v.running {
                        0.5
                    } else {
                        0.0
                    };
                    let (bar_rect, _) = ui.allocate_exact_size(
                        egui::vec2(ui.available_width(), 6.0),
                        egui::Sense::hover(),
                    );
                    ui.painter().rect_filled(bar_rect, 3.0, theme::panel());
                    if progress > 0.0 {
                        let mut fill_r = bar_rect;
                        fill_r.set_width(bar_rect.width() * progress);
                        ui.painter().rect_filled(fill_r, 3.0, clock_col);
                    }
                });
            });
        });

        ui.add_space(6.0);

        // Tastenreihe (Design-Runde 2, ScreenRadio): Start · Stopp · Blende · Mikro · Telefon als
        // 44-px-Tasten, darunter Sprung- und Spieltasten als 32-px-Symboltasten und die Decks.
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
            let (go_icon, go_text) = if v.running {
                (theme::icf::PAUSE, "Pause")
            } else {
                (theme::icf::PLAY, "Start")
            };
            let r = console_key(ui, go_icon, go_text, theme::ok(), v.running, KEY_H)
                .on_hover_text("Start / Pause (Leertaste)");
            r.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Play / Pause")
            });
            if r.clicked() {
                for e in woa_radio::party::PartyState::default()
                    .apply(woa_radio::party::PartyCommand::PlayPause, v.running)
                {
                    self.party_effect(e);
                }
            }
            self.learn_menu(&r, Target::StartNext);

            let r = console_key(ui, theme::icf::STOP, "Stopp", theme::on_air(), false, KEY_H)
                .on_hover_text(
                    "Stopp: 1× hält an wie Pause, 2× stoppt; ein von Hand gestartetes Deck hält an",
                );
            if r.clicked() {
                self.with(|c| c.stop_or_pause());
            }
            self.learn_menu(&r, Target::Stop);

            // Fade (Design Fassung 7, #320): blendet alles Laufende fest in 3 s aus.
            let now = ui.input(|i| i.time);
            let any = v.running || v.carts_playing != 0 || v.decks.iter().any(|d| d.playing);
            let pr = self.fades.progress(FadeKey::All, now);
            let r = fade_button(
                ui,
                FadeShape::Big(KEY_H),
                "Fade alles",
                any,
                pr,
                self.fades.seconds(),
            );
            if r.clicked() {
                self.fades.press(FadeKey::All, now);
                self.with(|c| c.fade_all());
            }

            let r = console_key(
                ui,
                theme::icf::SHUFFLE_ANGULAR,
                "Blende",
                theme::accent(),
                false,
                KEY_H,
            )
            .on_hover_text(format!(
                "Blende: läuft nichts, startet die Playlist; sonst blendet sie in {:.1} s auf den \
                 nächsten Titel",
                v.crossfade_seconds
            ));
            if r.clicked() {
                self.with(|c| c.start_or_next());
            }

            let r = console_key(
                ui,
                theme::icf::MICROPHONE,
                "Mikro",
                theme::on_air(),
                v.mic_open,
                KEY_H,
            )
            .on_hover_text("Mikro (M)");
            r.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Mikro (M)")
            });
            if r.clicked() {
                self.toggle_mic();
            }
            self.learn_menu(&r, Target::Mic);

            let ph = v.phone_on;
            let r = console_key(ui, theme::icf::PHONE, "Telefon", theme::on_air(), ph, KEY_H)
                .on_hover_text(if ph {
                    "Telefon auf Sendung. Klick: Anrufer von der Sendung nehmen"
                } else {
                    "Anrufer auf die Sendung legen. Der Pegel rechts zeigt ihn auch vorher; \
                     den Rückweg zum Anrufer wählst du unter Ausgänge."
                });
            if r.clicked() {
                self.toggle_phone();
            }
            let (rect, _) = ui.allocate_exact_size(egui::vec2(70.0, 12.0), egui::Sense::hover());
            theme::led_meter(ui.painter(), rect, self.phone_level);
            if self.pult.is_some() {
                ui.label(egui::RichText::new("PULT").small().strong())
                    .on_hover_text(
                        "Pult-Modus: Stream und Mitschnitt kommen vom Pult-Eingang. Mikro aus; \
                         Telefon und Gäste gehen mit der Musik ins Pult.",
                    );
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(70.0, 12.0), egui::Sense::hover());
                theme::led_meter(ui.painter(), rect, self.pult_level);
                let faults = self.pult_faults();
                if faults > 0 {
                    ui.label(
                        egui::RichText::new(format!("{faults} Aussetzer"))
                            .small()
                            .color(theme::text_dim()),
                    )
                    .on_hover_text("Unter- oder Überläufe des Pult-Eingangs seit dem Öffnen");
                }
            }
            if self.silent_stream.alarm() {
                let source = if self.pult.is_some() {
                    "Pult-Eingang"
                } else {
                    "Sendemix"
                };
                ui.colored_label(
                    theme::on_air(),
                    format!("Stream bekommt keinen Ton (Quelle: {source})"),
                );
            }
            if let Some(e) = &self.phone_error {
                ui.colored_label(theme::on_air(), e);
            }
            // Mitschnitt: seit 0.34.56 als REC-Pille in der Kopfleiste, in jedem Modus.
        });
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
            // Sprungtasten: derselbe Weg wie im Party-Modus und am Handy
            // (`PartyState::apply` und die Effekte in `party_effect`).
            let mut state = woa_radio::party::PartyState::default();
            for (icon, cmd, tip) in [
                (
                    theme::icf::SKIP_BACK,
                    woa_radio::party::PartyCommand::Previous,
                    "Vorheriger Titel (P)",
                ),
                (
                    theme::icf::SKIP_FORWARD,
                    woa_radio::party::PartyCommand::Next,
                    "Nächster Titel (N)",
                ),
                (
                    theme::icf::ARROW_COUNTER_CLOCKWISE,
                    woa_radio::party::PartyCommand::Restart,
                    "Titel von vorn",
                ),
                (
                    theme::icf::REWIND,
                    woa_radio::party::PartyCommand::SeekBack,
                    "10 s zurück",
                ),
                (
                    theme::icf::FAST_FORWARD,
                    woa_radio::party::PartyCommand::SeekForward,
                    "10 s vor",
                ),
            ] {
                if symbol_key(ui, icon, tip, false).clicked() {
                    for e in state.apply(cmd, v.running) {
                        self.party_effect(e);
                    }
                }
            }
            let (rep_icon, rep_tip) = match v.repeat {
                woa_radio::control::Repeat::Off => (theme::icf::REPEAT, "Wiederholen: aus"),
                woa_radio::control::Repeat::All => {
                    (theme::icf::REPEAT, "Wiederholen: ganze Playlist")
                }
                woa_radio::control::Repeat::One => {
                    (theme::icf::REPEAT_ONCE, "Wiederholen: ein Titel")
                }
            };
            let on = v.repeat != woa_radio::control::Repeat::Off;
            if symbol_key(ui, rep_icon, rep_tip, on).clicked() {
                self.party_effect(woa_radio::party::Effect::CycleRepeat);
            }
            let tip = if v.shuffle {
                "Zufall: an"
            } else {
                "Zufall: aus"
            };
            if symbol_key(ui, theme::icf::SHUFFLE, tip, v.shuffle).clicked() {
                self.party_effect(woa_radio::party::Effect::ToggleShuffle);
            }
            // Die Deck-Tasten „Deck A/B/C“ stehen seit Design-Runde 3 nicht mehr hier: Ablösen
            // der Playlist liegt im Rechtsklick-Menü jedes Decks.
        });

        ui.add_space(4.0);
        self.on_air_row(ui);
    }

    /// 19-Zoll-Broadcast-Processing- & Übergangs-Rack (rechts unter der Cart-Wall).
    fn processing_rack(&mut self, ui: &mut egui::Ui) {
        let v = self.view.clone();
        ui.horizontal(|ui| {
            ui.label(theme::panel_label("PROCESSING & ÜBERGÄNGE"));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                theme::status_pill(
                    ui,
                    &format!("ZIEL {:.0} LUFS", v.target_lufs),
                    theme::ok(),
                    v.bus_processing,
                );
            });
        });
        ui.add_space(4.0);

        // Zeilen nach ScreenRadio (`woa-set`): links die Bezeichnung, rechts Zahlenfeld oder
        // Schalter. Die Werte und Befehle sind dieselben wie vorher.
        let mut xf = v.crossfade_seconds;
        let mut duck = v.duck_db;
        let mut cart_duck = v.cart_duck;
        let (mut stop_on, mut stop_secs) = (v.stop_fade, v.stop_fade_seconds);
        let mut fade_secs = self.fades.seconds();
        let mut fade_ch = false;
        let (mut bus, mut mic) = (v.bus_processing, v.mic_processing);
        let (mut xf_ch, mut duck_ch, mut cart_ch, mut stop_ch, mut proc_ch) =
            (false, false, false, false, false);
        let label_w = (ui.available_width() - 170.0).max(100.0);
        let row = |ui: &mut egui::Ui, name: &str, sub: Option<&str>| {
            ui.vertical(|ui| {
                ui.set_min_width(label_w);
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.label(egui::RichText::new(name).size(13.0).color(theme::text()));
                if let Some(sub) = sub {
                    ui.label(egui::RichText::new(sub).size(11.0).color(theme::text_dim()));
                }
            });
        };
        egui::Grid::new("processing_rows")
            .num_columns(2)
            .spacing(egui::vec2(12.0, 3.0))
            .min_col_width(0.0)
            .show(ui, |ui| {
                let right = egui::Layout::right_to_left(egui::Align::Center);
                row(ui, "Crossfade-Blende", None);
                ui.with_layout(right, |ui| {
                    xf_ch = number_field(
                        ui,
                        &mut xf,
                        0.0..=10.0,
                        "s",
                        1.5,
                        "Crossfade-Blende",
                        true,
                    )
                    .on_hover_text(
                        "Länge der Überblendung · Ziehen · Doppelklick: 1,5 s · Klick: eintippen",
                    )
                    .changed();
                });
                ui.end_row();
                row(ui, "Musik-Ducking", Some("wenn das Mikro offen ist"));
                ui.with_layout(right, |ui| {
                    duck_ch = number_field(
                        ui,
                        &mut duck,
                        -30.0..=0.0,
                        "dB",
                        -12.0,
                        "Musik-Ducking",
                        true,
                    )
                    .on_hover_text("Absenkung der Musik bei offenem Mikro · Doppelklick: −12 dB")
                    .changed();
                });
                ui.end_row();
                row(ui, "Ducking auch bei Carts", None);
                ui.with_layout(right, |ui| {
                    cart_ch = toggle(ui, &mut cart_duck, "Ducking auch bei Carts")
                        .on_hover_text("Musik leiser, solange ein Cart spielt")
                        .changed();
                });
                ui.end_row();
                // Stopp mit Ausblenden (#227): kein eigener Knopf, die Stopp-Knöpfe bleiben.
                row(ui, "Stopp blendet aus", None);
                ui.with_layout(right, |ui| {
                    stop_ch |= toggle(ui, &mut stop_on, "Stopp blendet aus")
                        .on_hover_text(
                            "An: Stopp am Sende-Player und an den Decks A/B/C blendet über die \
                             eingestellte Dauer aus (etwa um mitten im Titel auszusteigen). \
                             Aus: Stopp bricht sofort ab.",
                        )
                        .changed();
                    let mut secs = stop_secs;
                    if number_field(
                        ui,
                        &mut secs,
                        0.5..=10.0,
                        "s",
                        3.0,
                        "Ausblenddauer",
                        stop_on,
                    )
                    .on_hover_text("Ausblenddauer beim Stopp")
                    .changed()
                    {
                        stop_secs = secs;
                        stop_ch = true;
                    }
                });
                ui.end_row();
                // Dauer der Fade-Tasten (Moderator 05.11.2026): 1–10 s, Vorgabe 3 s.
                row(ui, "Fade-Taste", Some("Sende-Deck, Decks, Carts"));
                ui.with_layout(right, |ui| {
                    fade_ch = number_field(
                        ui,
                        &mut fade_secs,
                        woa_radio::FADE_MIN_SECONDS..=woa_radio::FADE_MAX_SECONDS,
                        "s",
                        woa_radio::FADE_SECONDS,
                        "Fade-Taste",
                        true,
                    )
                    .on_hover_text("Wie lange die Fade-Tasten ausblenden · Doppelklick: 3 s")
                    .changed();
                });
                ui.end_row();
                row(ui, "Sendebus-Processing", None);
                ui.with_layout(right, |ui| {
                    proc_ch |= toggle(ui, &mut bus, "Sendebus-Processing")
                        .on_hover_text(format!(
                            "Lautheitsregelung auf {:.0} LUFS (±12 dB, langsam), Kompressor und \
                             Limiter bei −1 dBFS. Wirkt auf Sendung, Mitschnitt und Abhören.",
                            v.target_lufs
                        ))
                        .changed();
                });
                ui.end_row();
                row(ui, "Mikro-Kette", None);
                ui.with_layout(right, |ui| {
                    proc_ch |= toggle(ui, &mut mic, "Mikro-Kette")
                        .on_hover_text(
                            "Hochpass 80 Hz, Präsenz +3 dB bei 3 kHz, Gate ab −50 dBFS, \
                             Kompressor 4:1 ab −24 dBFS mit +6 dB.",
                        )
                        .changed();
                    if v.mic_processing && v.mic_open {
                        let (t, c, active) = if v.mic_gate_open {
                            ("Gate offen", theme::ok(), true)
                        } else {
                            ("Gate zu", theme::text_dim(), false)
                        };
                        theme::status_pill(ui, t, c, active);
                    }
                });
                ui.end_row();
            });
        if xf_ch {
            self.view.crossfade_seconds = xf;
            self.with(move |c| c.set_crossfade(xf));
        }
        if duck_ch {
            self.view.duck_db = duck;
            self.with(move |c| c.set_duck(duck));
        }
        if cart_ch {
            self.view.cart_duck = cart_duck;
            self.with(move |c| c.set_cart_duck(cart_duck));
        }
        if stop_ch {
            self.view.stop_fade = stop_on;
            self.view.stop_fade_seconds = stop_secs;
            self.with(move |c| c.set_stop_fade(stop_on, stop_secs));
        }
        if fade_ch {
            let secs = fade_secs.clamp(woa_radio::FADE_MIN_SECONDS, woa_radio::FADE_MAX_SECONDS);
            self.view.fade_seconds = secs;
            self.fades.set_seconds(secs);
            self.with(move |c| c.set_fade_seconds(secs));
        }
        if proc_ch {
            self.view.bus_processing = bus;
            self.view.mic_processing = mic;
            self.with(move |c| c.set_processing(bus, mic));
        }
        ui.add_space(6.0);
        // Messzeile als LCD in lcd-ink: Lautheit · Regelung · Begrenzer am Sendeausgang.
        let on = self.view.bus_processing;
        let (lufs, agc, red) = (
            self.view.bus_lufs,
            self.view.bus_agc_db,
            self.view.bus_reduction_db,
        );
        theme::lcd_show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let cell = |ui: &mut egui::Ui, cap: &str, val: String| {
                    ui.label(
                        egui::RichText::new(cap.to_uppercase())
                            .font(theme::label_font(11.0))
                            .color(theme::lcd_text()),
                    );
                    ui.label(
                        egui::RichText::new(val)
                            .monospace()
                            .size(13.0)
                            .color(if on {
                                theme::lcd_ink()
                            } else {
                                theme::led_off()
                            }),
                    );
                };
                let w = ui.available_width() / 3.0;
                for (cap, val) in [
                    (
                        "Lautheit",
                        if on && lufs > -60.0 {
                            format!("{lufs:.1}")
                        } else {
                            "–".into()
                        },
                    ),
                    (
                        "Regelung",
                        if on {
                            format!("{agc:+.1} dB")
                        } else {
                            "–".into()
                        },
                    ),
                    (
                        "Begrenzer",
                        if on {
                            format!("−{:.1} dB", red.max(0.0))
                        } else {
                            "–".into()
                        },
                    ),
                ] {
                    ui.allocate_ui_with_layout(
                        egui::vec2(w - 6.0, 18.0),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| cell(ui, cap, val),
                    );
                }
            })
            .response
            .on_hover_text(
                "Kurzzeit-Lautheit am Sendeausgang (LUFS), Verstärkung der Regelung, Absenkung \
                 durch Kompressor und Limiter",
            );
        });
    }

    /// Gast-Kanäle (Paket 37): Testgäste, Fader, Pegel, Rückweg (Mix-Minus) im Kopfhörer.
    fn guests_card(&mut self, ui: &mut egui::Ui) {
        let guests = self.view.guests.clone();
        ui.horizontal(|ui| {
            ui.label(theme::panel_label("GÄSTE & MIX-MINUS"));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let full = guests.len() >= woa_guests::MAX_GUESTS;
                let file = ui
                    .add_enabled(
                        !full,
                        egui::Button::new(format!("{} Datei als Gast", theme::ic::PLUS)),
                    )
                    .on_hover_text(
                        "Eine Audiodatei in Schleife als Testgast (kein Netz, kein Mikrofon)",
                    );
                if file.clicked()
                    && let Some(p) = crate::ordner::dialog(crate::ordner::Art::Aufnahmen)
                        .add_filter(
                            woa_engine::decode::FILTER_NAME,
                            &woa_engine::decode::dialog_extensions(),
                        )
                        .pick_file_modal()
                {
                    self.add_guest(woa_guests::TestSignal::File(p));
                }
                let sine = ui
                    .add_enabled(
                        !full,
                        egui::Button::new(format!("{} Testgast", theme::ic::PLUS)),
                    )
                    .on_hover_text("Ein Sinuston als Gast, jeder Gast mit eigener Tonhöhe");
                if sine.clicked() {
                    let freq = 330.0 + 110.0 * guests.len() as f32;
                    self.add_guest(woa_guests::TestSignal::Sine { freq });
                }
            });
        });
        ui.add_space(4.0);
        theme::lcd_show(ui, |ui| {
            ui.set_width(ui.available_width());
            if guests.is_empty() {
                ui.label(
                    egui::RichText::new(
                        "Kein Gast. Jeder Gast bekommt einen Kanal und einen Rückweg ohne die eigene Stimme.",
                    )
                    .small()
                    .color(theme::text_dim()),
                );
            }
            for g in &guests {
                self.guest_row(ui, g);
                ui.add_space(2.0);
            }
        });
        ui.add_space(6.0);
        self.web_guests(ui);
    }

    /// Browser-Gäste (Paket 38): Zugang an/aus, Einladungen mit QR-Code, Warteraum.
    fn web_guests(&mut self, ui: &mut egui::Ui) {
        let web = self.view.web.clone();
        ui.horizontal(|ui| {
            ui.label(
                theme::panel_label("BROWSER-GÄSTE"),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if web.running {
                    if ui
                        .button("Ausschalten")
                        .on_hover_text("Trennt alle Browser-Gäste und schließt die Ports")
                        .clicked()
                    {
                        self.with(|c| c.guests_web_set(false, None));
                    }
                    let full = self.view.guests.len() >= woa_guests::MAX_GUESTS;
                    if ui
                        .add_enabled(!full, egui::Button::new(format!("{} Einladung", theme::ic::PLUS)))
                        .on_hover_text("Ein Link für ein Handy im selben WLAN, einmal benutzbar")
                        .clicked()
                    {
                        self.with(|c| {
                            if c.guests().web_invite().is_none() {
                                tracing::warn!("invite not created");
                            }
                        });
                    }
                } else if ui
                    .button("Browser-Zugang einschalten")
                    .on_hover_text(
                        "Öffnet zwei Ports im WLAN (HTTPS und UDP). Gäste brauchen nur einen Browser.",
                    )
                    .clicked()
                {
                    let ip = woa_remote::lan_ip();
                    self.with(move |c| c.guests_web_set(true, ip));
                }
            });
        });
        ui.add_space(4.0);
        theme::lcd_show(ui, |ui| {
            ui.set_width(ui.available_width());
            if !web.running {
                ui.label(
                    egui::RichText::new(
                        "Aus. Einschalten, Einladung erzeugen, Link ans Handy schicken: Der Gast \
                         spricht über den Browser, ohne App.",
                    )
                    .small()
                    .color(theme::text_dim()),
                );
                return;
            }
            for w in &web.waiting {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(format!("{} wartet seit {} s", w.name, w.waited_s))
                            .strong()
                            .color(theme::accent()),
                    )
                    .on_hover_text(&w.peer);
                    let ticket = w.ticket.clone();
                    if ui
                        .button(egui::RichText::new("Annehmen").color(theme::ok()))
                        .clicked()
                    {
                        self.with(move |c| {
                            if let Err(e) = c.guests().web_accept(&ticket) {
                                tracing::warn!(error = %e, "guest not accepted");
                            }
                        });
                    }
                    let ticket = w.ticket.clone();
                    if ui.button("Ablehnen").clicked() {
                        self.with(move |c| c.guests().web_reject(&ticket));
                    }
                });
            }
            for (link, inv) in &web.invites {
                if self.invite_qr.as_ref().map(|(l, _)| l) != Some(link) {
                    self.invite_qr = woa_remote::qr::qr(link).map(|m| (link.clone(), m));
                }
                ui.horizontal(|ui| {
                    if let Some((l, m)) = &self.invite_qr
                        && l == link
                    {
                        crate::remote::draw_qr(ui, m, 150.0);
                    }
                    ui.vertical(|ui| {
                        ui.label(
                            egui::RichText::new(format!(
                                "Einladung, noch {} min gültig",
                                inv.left_s.div_ceil(60)
                            ))
                            .strong(),
                        );
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(link)
                                    .small()
                                    .monospace()
                                    .color(theme::cue()),
                            )
                            .wrap()
                            .selectable(true),
                        );
                        ui.horizontal(|ui| {
                            if ui.small_button("Link kopieren").clicked() {
                                ui.ctx().copy_text(link.clone());
                            }
                            if ui.small_button("Zurückziehen").clicked() {
                                let token = inv.token.clone();
                                self.with(move |c| c.guests().web_revoke(&token));
                            }
                        });
                        ui.label(
                            egui::RichText::new(
                                "Das Handy zeigt eine Zertifikatswarnung (selbst ausgestellt): \
                                 „Erweitert“, dann „Weiter“.",
                            )
                            .small()
                            .color(theme::text_dim()),
                        );
                    });
                });
                ui.add_space(4.0);
            }
            if web.invites.is_empty() && web.waiting.is_empty() {
                ui.label(
                    egui::RichText::new("Bereit. „Einladung“ erzeugt einen Link mit QR-Code.")
                        .small()
                        .color(theme::text_dim()),
                );
            }
        });
    }

    fn add_guest(&mut self, signal: woa_guests::TestSignal) {
        self.with(move |c| {
            if let Err(e) = c.guests().add_test_guest(signal) {
                tracing::warn!(error = %e, "guest not added");
            }
        });
    }

    fn guest_row(&mut self, ui: &mut egui::Ui, g: &woa_guests::GuestView) {
        let slot = g.slot;
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(&g.name).strong().color(theme::text()))
                .on_hover_text(&g.source);
            // Pegel vor dem Fader: zeigt auch im Warteraum und stumm, ob der Gast spricht.
            let (rect, _) = ui.allocate_exact_size(egui::vec2(90.0, 12.0), egui::Sense::hover());
            theme::led_meter(ui.painter(), rect, g.input_peak);
            // Auf Sendung / Warteraum.
            let (air_text, air_col, air_tip) = if g.on_air {
                (
                    format!("{} Auf Sendung", theme::ic::BROADCAST),
                    theme::on_air(),
                    "Der Gast ist zu hören. Klick: zurück in den Warteraum (hört weiter die Sendung).",
                )
            } else {
                (
                    format!("{} Warteraum", theme::ic::PAUSE),
                    theme::text_dim(),
                    "Der Gast hört die Sendung, ist aber nicht zu hören. Klick: auf Sendung holen.",
                )
            };
            if ui
                .add(egui::Button::new(egui::RichText::new(air_text).strong().color(air_col)))
                .on_hover_text(air_tip)
                .clicked()
            {
                let on = !g.on_air;
                if let Some(x) = self.view.guests.iter_mut().find(|x| x.slot == slot) {
                    x.on_air = on;
                }
                self.with(move |c| c.guests().set_on_air(slot, on));
            }
            // Stumm (Moderator): nicht auf Sendung, hört die Sendung weiter.
            let mute = egui::Button::new(
                egui::RichText::new(format!("{} Stumm", theme::ic::MICROPHONE_SLASH)).color(
                    if g.muted {
                        theme::on_air()
                    } else {
                        theme::text_dim()
                    },
                ),
            )
            .selected(g.muted);
            if ui
                .add(mute)
                .on_hover_text("Gast stumm: nicht auf Sendung, hört die Sendung aber weiter")
                .clicked()
            {
                let on = !g.muted;
                if let Some(x) = self.view.guests.iter_mut().find(|x| x.slot == slot) {
                    x.muted = on;
                }
                self.with(move |c| c.guests().set_mute(slot, on));
            }
            // Lautstärke wie der Kanal-Gain: ziehen, Doppelklick = 0 dB.
            let mut db = g.gain_db;
            let r = ui
                .add(
                    egui::DragValue::new(&mut db)
                        .range(-60.0..=12.0)
                        .speed(0.2)
                        .fixed_decimals(1)
                        .suffix(" dB"),
                )
                .on_hover_text("Lautstärke des Gastes. Ziehen, Doppelklick = 0 dB");
            let set = if r.double_clicked() {
                Some(0.0)
            } else if r.changed() {
                Some(db)
            } else {
                None
            };
            if let Some(db) = set {
                if let Some(x) = self.view.guests.iter_mut().find(|x| x.slot == slot) {
                    x.gain_db = db;
                }
                self.with(move |c| c.guests().set_gain_db(slot, db));
            }
            let ret = egui::Button::new(egui::RichText::new(theme::ic::HEADPHONES).color(
                if g.monitored {
                    theme::cue()
                } else {
                    theme::text_dim()
                },
            ));
            if ui
                .add(ret)
                .on_hover_text(
                    "Rückweg dieses Gastes abhören: Musik, Moderator und alle anderen Gäste, \
                     nur nicht er selbst. Der Abhörausgang trägt dann diesen Rückweg.",
                )
                .clicked()
            {
                let to = (!g.monitored).then_some(slot);
                self.with(move |c| c.guests().set_monitor(to));
            }
            // Trennen mit kurzer Bestätigung: zweiter Klick binnen 3 s.
            let armed = self
                .guest_hangup_armed
                .is_some_and(|(s, t)| s == slot && t.elapsed() < Duration::from_secs(3));
            let label = if armed {
                egui::RichText::new(format!("{} Wirklich trennen?", theme::ic::X))
                    .color(theme::danger_text())
            } else {
                egui::RichText::new(theme::ic::X)
            };
            if ui
                .small_button(label)
                .on_hover_text("Gast trennen (zweimal klicken)")
                .clicked()
            {
                if armed {
                    self.guest_hangup_armed = None;
                    self.with(move |c| c.guests().remove(slot));
                } else {
                    self.guest_hangup_armed = Some((slot, Instant::now()));
                }
            }
            if armed {
                ui.ctx().request_repaint_after(Duration::from_millis(250));
            }
        });
        let mut line = format!("{} · Puffer {:.0} ms", g.source, g.buffered_ms);
        if g.prebuffering {
            line.push_str(" · füllt");
        }
        if let Some(n) = &g.net {
            if n.connected {
                let q = if n.loss_permille > 50 || n.rtt_ms > 300 {
                    "schlecht"
                } else if n.loss_permille > 10 || n.rtt_ms > 150 {
                    "mäßig"
                } else {
                    "gut"
                };
                line.push_str(&format!(
                    " · Leitung {q} · RTT {} ms · Verlust {:.1} %",
                    n.rtt_ms,
                    n.loss_permille as f32 / 10.0
                ));
            } else {
                line.push_str(" · verbindet …");
            }
        }
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(line)
                    .small()
                    .monospace()
                    .color(theme::text_dim()),
            );
            if g.self_muted {
                ui.label(
                    egui::RichText::new(format!(
                        "{} hat sich selbst stumm geschaltet",
                        theme::ic::MICROPHONE_SLASH
                    ))
                    .small()
                    .color(theme::accent()),
                );
            }
            if g.underruns > 0 || g.overflows > 0 {
                ui.label(
                    egui::RichText::new(format!(
                        "Aussetzer {} · Überlauf {}",
                        g.underruns, g.overflows
                    ))
                    .small()
                    .color(theme::danger_text()),
                );
            }
        });
    }

    fn on_air_row(&mut self, ui: &mut egui::Ui) {
        let now = woa_stream::unix_now();
        let st = self.stream_status.clone();
        let running = st.as_ref().is_some_and(|s| s.is_running(now));
        let on = self.feeder.is_some();
        theme::lcd_show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                // Kleine ON-AIR-Lampe vorn (OnAirLamp `--sm`): leuchtet nur, wenn die Sendung
                // läuft (wie die Lampe der Kopfleiste).
                crate::widgets::onair_lamp(
                    ui,
                    on_air_lamp_size(),
                    onair_lamp_lit(running && on, false),
                );
                // Rot nur für live: in Ruhe ist die Taste neutral, auf Sendung rot gefüllt.
                let (label, fill, border) = if on {
                    (
                        "Sendung beenden",
                        theme::on_air_fill(),
                        theme::on_air_fill(),
                    )
                } else {
                    ("Auf Sendung", theme::surface(), theme::border_strong())
                };
                let text = theme::fill_label(
                    theme::icf::BROADCAST,
                    &label.to_uppercase(),
                    13.0,
                    if on {
                        egui::Color32::WHITE
                    } else {
                        theme::text()
                    },
                );
                let b = ui
                    .add(
                        egui::Button::new(text)
                            .fill(fill)
                            .stroke(egui::Stroke::new(1.0, border))
                            .corner_radius(theme::RADIUS_MD)
                            .min_size(egui::vec2(148.0, ON_AIR_ROW_H)),
                    )
                    .on_hover_text("Sendung an den eigenen Shoutcast (woa-streamd)");
                b.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
                if b.clicked() {
                    if on {
                        self.go_off_air();
                    } else {
                        self.go_on_air();
                    }
                }
                match &st {
                    Some(s) if running => {
                        let color = if s.emergency || !s.connected {
                            theme::on_air()
                        } else {
                            theme::ok()
                        };
                        let summary = s.summary();
                        let mut pill = crate::widgets::chip::StateChip::new(
                            &summary,
                            crate::widgets::chip::ChipTone::Color(color),
                        );
                        pill.height = ON_AIR_ROW_H;
                        pill.show(ui);
                        if let Some(line) = s.emergency_line() {
                            ui.label(
                                egui::RichText::new(line)
                                    .strong()
                                    .color(theme::danger_text()),
                            );
                        }
                    }
                    Some(s) if s.last_error.is_some() && on => {
                        ui.colored_label(
                            ui.visuals().error_fg_color,
                            s.last_error.clone().unwrap_or_default(),
                        );
                    }
                    _ if on => {
                        ui.label(
                            egui::RichText::new("starte woa-streamd …").color(theme::text_dim()),
                        );
                    }
                    _ => {
                        ui.label(
                            egui::RichText::new("Stream-Encoder bereit (Shoutcast / Icecast)")
                                .small()
                                .color(theme::text_dim()),
                        );
                    }
                }
            });
            if let Some(p) = st
                .as_ref()
                .filter(|_| running)
                .and_then(StreamStatus::problem)
            {
                ui.label(egui::RichText::new(p).small().color(theme::danger_text()));
            }
            if on
                && let Some(h) = st
                    .as_ref()
                    .filter(|_| running)
                    .and_then(StreamStatus::empty_emergency_hint)
            {
                ui.label(egui::RichText::new(h).small().color(theme::danger_text()));
            }
            if let Some(line) = link_line(
                self.feeder
                    .as_ref()
                    .map(|f| (f.is_connected(), f.reconnects())),
                st.as_ref().filter(|_| running),
            ) {
                ui.label(
                    egui::RichText::new(line)
                        .small()
                        .color(theme::danger_text()),
                );
            }
        });
    }

    fn playlist(&mut self, ui: &mut egui::Ui, v: &View) {
        ui.horizontal(|ui| {
            ui.label(theme::panel_label("SENDE-PLAYLIST"));
            let acts = self
                .manage
                .header(ui, &v.playlists, v.active_playlist, v.running);
            self.run_actions(acts);
            let hide = self.hide_played.unwrap_or(v.hide_played);
            let played = played_count(v);
            let count = if hide && played > 0 {
                format!("({} offen, {played} gespielt)", v.playlist.len() - played)
            } else {
                format!("({} Titel)", v.playlist.len())
            };
            ui.label(egui::RichText::new(count).small().color(theme::text_dim()));
            let r = ui
                .selectable_label(hide, egui::RichText::new(theme::ic::EYE_SLASH).small())
                .on_hover_text(
                    "Gespielte ausblenden: Titel vor dem laufenden verschwinden aus der Liste \
                     (nicht gelöscht). „bis Playlist-Ende“ zählt nur die offenen.",
                );
            if r.clicked() {
                let on = !hide;
                self.hide_played = Some(on);
                self.with(move |c| c.set_hide_played(on));
            }
            // Auto-Weiter (#148): aus, dann hält die Playlist nach jedem Titel an.
            let r = led_key(ui, "Auto-Weiter", v.auto_next).on_hover_text(
                "An: Die Titel laufen nacheinander. Aus: Nach jedem Titel hält die \
                     Playlist an, der nächste steht bereit und startet mit „Play“.",
            );
            if r.clicked() {
                let on = !v.auto_next;
                self.view.auto_next = on;
                self.with(move |c| c.set_auto_next(on));
            }
            if v.measuring > 0 {
                ui.spinner();
                ui.label(
                    egui::RichText::new("misst Lautheit …")
                        .small()
                        .color(theme::accent()),
                );
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if small_key(ui, theme::icf::PLUS, "Titel")
                    .on_hover_text("Titel hinzufügen …")
                    .clicked()
                {
                    self.add_dialog();
                }
            });
        });
        ui.add_space(4.0);

        if v.playlist.is_empty() {
            theme::lcd_show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.vertical_centered(|ui| {
                    ui.add_space(12.0);
                    ui.label(
                        egui::RichText::new(
                            "Audiodateien (MP3, FLAC, WAV, OGG, M4A, AIFF …) hierher ziehen oder „+ Titel …“.",
                        )
                        .color(theme::text_dim()),
                    );
                    ui.add_space(12.0);
                });
            });
        } else {
            // Spaltenkopf wie in mAirList
            egui::Frame::new()
                .fill(theme::panel_header())
                .corner_radius(egui::CornerRadius {
                    nw: 4,
                    ne: 4,
                    sw: 0,
                    se: 0,
                })
                .inner_margin(egui::Margin::symmetric(8, 4))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new("#")
                                .small()
                                .strong()
                                .color(theme::text_dim()),
                        );
                        ui.add_space(30.0);
                        let head = |t: &str| theme::panel_label(t);
                        ui.label(head("TITEL"));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            // Platz der Hover-Knöpfe, damit die Köpfe über ihren Spalten stehen.
                            ui.add_space(ROW_TOOLS_W);
                            ui.add_sized([DUR_W, 14.0], egui::Label::new(head("DAUER")));
                            ui.add_sized([LUFS_W, 14.0], egui::Label::new(head("LAUTHEIT")));
                        });
                    });
                });
        }

        let mut released: Option<(usize, egui::Pos2)> = None;
        let mut hover_now = None;
        self.row_rects.clear();
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 3.0;
            let hide_before = if self.hide_played.unwrap_or(v.hide_played) {
                played_count(v)
            } else {
                0
            };
            for (i, e) in v.playlist.iter().enumerate() {
                if i < hide_before {
                    // Ausgeblendet: Platzhalter, damit die Zeilennummern fürs Ziehen stimmen.
                    self.row_rects.push(egui::Rect::NOTHING);
                    continue;
                }
                let on_air = v.current == Some(i);
                let is_cue = i == v.cue && !v.running;
                let tools = self.row_hover == Some(i) || self.row_selected == Some(i);
                let row_fill = if on_air {
                    theme::mix(theme::on_air(), theme::panel(), 0.86)
                } else if is_cue {
                    theme::mix(theme::cue(), theme::panel(), 0.90)
                } else if self.row_hover == Some(i) {
                    theme::surface()
                } else {
                    theme::well()
                };
                let stroke = if on_air {
                    egui::Stroke::new(1.0, theme::on_air())
                } else if is_cue {
                    egui::Stroke::new(1.0, theme::cue())
                } else if self.row_selected == Some(i) {
                    egui::Stroke::new(1.5, theme::accent())
                } else {
                    egui::Stroke::new(1.0, theme::mix(theme::border(), theme::well(), 0.5))
                };
                let frame = egui::Frame::new()
                    .fill(row_fill)
                    .stroke(stroke)
                    .corner_radius(4)
                    .inner_margin(egui::Margin::symmetric(8, 5));
                let resp = row_area(
                    ui,
                    ("playlist_row", i),
                    egui::Sense::click_and_drag(),
                    frame,
                    |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            // Griff zum Ziehen (#92): die ganze Zeile lässt sich ziehen (#166).
                            ui.label(
                                egui::RichText::new(theme::ic::DOTS_SIX_VERTICAL)
                                    .color(theme::text_dim()),
                            )
                            .on_hover_text("Ziehen zum Verschieben");
                            // Nummer eintippen (#92): Klick, Zahl, Enter verschiebt den Titel.
                            let editing = self.pos_edit.as_ref().is_some_and(|(k, _, _)| *k == i);
                            if editing {
                                let len = v.playlist.len();
                                let mut done = None;
                                if let Some((_, text, focused)) = self.pos_edit.as_mut() {
                                    let r = ui.add(
                                        egui::TextEdit::singleline(text)
                                            .desired_width(26.0)
                                            .font(egui::TextStyle::Monospace),
                                    );
                                    // Fokus nur einmal setzen (#322): sonst holt jedes Bild ihn
                                    // zurück, und Tab, Esc oder ein Klick daneben enden nie.
                                    if !*focused {
                                        r.request_focus();
                                        *focused = true;
                                    }
                                    if r.lost_focus() || (*focused && !r.has_focus()) {
                                        let enter =
                                            ui.input(|inp| inp.key_pressed(egui::Key::Enter));
                                        done = Some(
                                            enter.then(|| typed_position(text, len)).flatten(),
                                        );
                                    }
                                }
                                if let Some(target) = done {
                                    self.pos_edit = None;
                                    if let Some(to) = target.filter(|&to| to != i) {
                                        self.with(move |c| {
                                            c.move_entry(i, to);
                                        });
                                    }
                                }
                            } else {
                                let r = ui
                                    .add(
                                        egui::Label::new(
                                            egui::RichText::new(format!("{:>2}", i + 1))
                                                .monospace()
                                                .strong()
                                                .color(if on_air {
                                                    theme::on_air()
                                                } else if is_cue {
                                                    theme::cue()
                                                } else {
                                                    theme::text_dim()
                                                }),
                                        )
                                        .sense(egui::Sense::click()),
                                    )
                                    .on_hover_text("Klicken und neue Nummer eintippen (Enter)");
                                if r.clicked() {
                                    self.pos_edit = Some((i, (i + 1).to_string(), false));
                                }
                            }
                            let thumb = self.covers.as_mut().and_then(|c| c.thumb(&e.path));
                            crate::covers::paint_square(ui, 34.0, thumb.as_ref(), None, 1.0);
                            crate::protokoll::damage_badge(ui, &e.path);
                            if let Some(vb) = e.voice {
                                ui.label(
                                    egui::RichText::new("VOICE")
                                        .small()
                                        .strong()
                                        .color(theme::accent()),
                                );
                                if vb.check {
                                    ui.label(
                                        egui::RichText::new("prüfen")
                                            .small()
                                            .strong()
                                            .color(theme::danger_text()),
                                    )
                                    .on_hover_text(
                                        "Ein Nachbar hat sich geändert: der Voice-Track geht erst \
                                     nach der Bestätigung im Editor auf Sendung",
                                    );
                                }
                            }

                            // Rechte Seite zuerst reservieren (Steuerung + Dauer + LUFS-Badge),
                            // damit der Titel links sauber abschneidet.
                            let lufs_badge = e
                                .lufs
                                .map(|l| format!("{l:.1} LUFS ({:+.1} dB)", e.gain_db));
                            // Der Titel klickt nicht selbst: Klick, Doppelklick und Menü gehören
                            // der ganzen Zeile (#302).
                            // Rechts bleiben Badge, Lautheit, Dauer und Knöpfe frei.
                            let right = ROW_TOOLS_W
                                + DUR_W
                                + LUFS_W
                                + ROW_BADGE_W
                                + crate::vocal_ui::VOCAL_W;
                            let w = (ui.available_width() - right).max(40.0);
                            ui.allocate_ui(egui::vec2(w, 20.0), |ui| {
                                ui.add(
                                    egui::Label::new(
                                        egui::RichText::new(&e.title).strong().color(theme::text()),
                                    )
                                    .truncate(),
                                );
                            });

                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    // Verschieben und Entfernen beim Hover (und an der gewählten
                                    // Zeile); der Platz bleibt immer frei, damit nichts springt.
                                    ui.allocate_ui_with_layout(
                                        egui::vec2(ROW_TOOLS_W, 22.0),
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            ui.set_min_width(ROW_TOOLS_W);
                                            let here = v.pfl_source == Some(PflSource::Title(i));
                                            ui.spacing_mut().item_spacing.x = 2.0;
                                            // Vorhören (#273) wie am Deck, an jeder Zeile
                                            // (Moderator 05.11.2026), nicht erst beim Hover.
                                            let r = icon_key(
                                                ui,
                                                theme::ic::HEADPHONES,
                                                &format!("Vorhören Titel {}", i + 1),
                                                theme::cue(),
                                                here,
                                                v.pfl_available,
                                                22.0,
                                            );
                                            let r = if v.pfl_available {
                                                r.on_hover_text(if here {
                                                    "Vorhören stoppen"
                                                } else {
                                                    "Im Kopfhörer vorhören (nie auf Sendung)"
                                                })
                                            } else {
                                                r.on_disabled_hover_text(self.pfl_hint())
                                            };
                                            if r.clicked() {
                                                self.with(move |c| {
                                                    if let Err(e) =
                                                        c.pfl_toggle(PflSource::Title(i))
                                                    {
                                                        c.report(e);
                                                    }
                                                });
                                            }
                                            if !tools {
                                                return;
                                            }
                                            let icon = |ui: &mut egui::Ui, t: &str, tip: &str| {
                                                ui.add(egui::Button::new(t).small().frame(false))
                                                    .on_hover_text(tip)
                                                    .clicked()
                                            };
                                            if icon(ui, theme::ic::X, "Entfernen") {
                                                self.with(move |c| {
                                                    c.remove(i);
                                                });
                                            }
                                            if icon(ui, theme::ic::CARET_DOWN, "Nach unten") {
                                                self.with(move |c| {
                                                    c.move_entry(i, i + 1);
                                                });
                                            }
                                            if icon(ui, theme::ic::CARET_UP, "Nach oben") && i > 0 {
                                                self.with(move |c| {
                                                    c.move_entry(i, i - 1);
                                                });
                                            }
                                        },
                                    );
                                    // Dauer neutral (Design-Runde 2), rechtsbündig in Mono.
                                    ui.add_sized(
                                        [DUR_W, 18.0],
                                        egui::Label::new(
                                            egui::RichText::new(
                                                e.seconds
                                                    .map(|s| clock(e.cues.play_seconds(s)))
                                                    .unwrap_or_else(|| "--:--".into()),
                                            )
                                            .monospace()
                                            .color(theme::text()),
                                        ),
                                    );
                                    let lufs = ui.add_sized(
                                        [LUFS_W, 18.0],
                                        egui::Label::new(
                                            egui::RichText::new(
                                                e.lufs
                                                    .map(|l| format!("{l:.1}"))
                                                    .unwrap_or_else(|| "–".into()),
                                            )
                                            .size(11.0)
                                            .monospace()
                                            .color(theme::text_dim()),
                                        ),
                                    );
                                    if let Some(lb) = lufs_badge {
                                        lufs.on_hover_text(lb);
                                    }
                                    // Gesangseinsatz (#299) links neben der Lautheit.
                                    crate::vocal_ui::vocal_tag(
                                        ui,
                                        e.vocal,
                                        e.cues.start(),
                                        crate::vocal_ui::VOCAL_W,
                                    );
                                    if on_air {
                                        theme::status_pill(ui, "ON AIR", theme::on_air(), true);
                                        // Fade in der laufenden Zeile (FadeButton, #274).
                                        let now = ui.input(|i| i.time);
                                        let pr = self.fades.progress(FadeKey::Playlist, now);
                                        if fade_button(
                                            ui,
                                            FadeShape::Icon(22.0),
                                            "Fade Playlist",
                                            v.running,
                                            pr,
                                            self.fades.seconds(),
                                        )
                                        .clicked()
                                        {
                                            self.fades.press(FadeKey::Playlist, now);
                                            self.with(|c| c.fade_music());
                                        }
                                    } else if is_cue {
                                        theme::status_pill(ui, "CUE", theme::cue(), true);
                                    }
                                },
                            );
                        });
                    },
                );
                // Die ganze Zeile (#302): Klick wählt aus, Doppelklick setzt „als Nächstes“,
                // Rechtsklick öffnet das Titelmenü, Ziehen (#166) legt auf ein Deck oder an eine
                // andere Stelle. Knöpfe und die Nummer behalten ihren eigenen Klick.
                let row = resp.response.rect;
                self.row_rects.push(row);
                let drag = resp
                    .response
                    .clone()
                    .on_hover_cursor(egui::CursorIcon::Grab)
                    .on_hover_ui(|ui| {
                        ui.label(format!(
                            "{}\n{}\nDoppelklick: als Nächstes beim Start · Rechtsklick: Menü",
                            e.path.display(),
                            e.lufs
                                .map(|l| format!("{l:.1} LUFS, Auto-Gain {:+.1} dB", e.gain_db))
                                .unwrap_or_else(|| "noch nicht gemessen".into())
                        ));
                    });
                if drag.clicked() {
                    self.row_selected = Some(i);
                }
                if drag.contains_pointer() {
                    hover_now = Some(i);
                }
                if drag.double_clicked() {
                    self.with(move |c| c.set_cue(i));
                }
                drag.context_menu(|ui| self.title_menu(ui, i, e, v));
                if drag.drag_started() {
                    self.row_drag = Some(i);
                }
                if drag.dragged() && self.row_drag == Some(i) {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                    if let Some(p) = ui.ctx().pointer_latest_pos() {
                        egui::Area::new(egui::Id::new("playlist_drag_ghost"))
                            .order(egui::Order::Tooltip)
                            .fixed_pos(p + egui::vec2(12.0, 8.0))
                            .interactable(false)
                            .show(ui.ctx(), |ui| {
                                theme::card_frame().show(ui, |ui| {
                                    ui.label(
                                        egui::RichText::new(&e.title).strong().color(theme::text()),
                                    );
                                });
                            });
                    }
                }
                if drag.drag_stopped() && self.row_drag == Some(i) {
                    self.row_drag = None;
                    if let Some(p) = drag
                        .interact_pointer_pos()
                        .or(ui.ctx().pointer_latest_pos())
                    {
                        released = Some((i, p));
                    }
                }
                // Farbiger Indikator-Streifen am linken Zeilenrand bei aktivem oder gecuetem Track
                if on_air || is_cue {
                    let r = resp.response.rect;
                    let stripe = egui::Rect::from_min_size(r.min, egui::vec2(4.0, r.height()));
                    ui.painter().rect_filled(
                        stripe,
                        egui::CornerRadius {
                            nw: 4,
                            sw: 4,
                            ne: 0,
                            se: 0,
                        },
                        if on_air {
                            theme::on_air()
                        } else {
                            theme::cue()
                        },
                    );
                }
            }
        });
        if self.row_hover != hover_now {
            self.row_hover = hover_now;
            ui.ctx().request_repaint();
        }
        if let Some((from, pos)) = released {
            match row_drop_target(
                pos,
                from,
                &self.deck_rects,
                &self.cart_rects,
                self.playlist_rect,
                &self.row_rects,
            ) {
                RowDrop::Deck(k) => self.with(move |c| {
                    if let Err(e) = c.deck_load_title(k, from) {
                        c.report(e);
                    }
                }),
                RowDrop::Cart(slot) => {
                    if let Some(path) = self.view.playlist.get(from).map(|e| e.path.clone()) {
                        self.assign_cart_file(slot, path);
                    }
                }
                RowDrop::Row(to) => self.with(move |c| {
                    c.move_entry(from, to);
                }),
                RowDrop::Nothing => {}
            }
        }
    }

    fn cart_wall(&mut self, ui: &mut egui::Ui, v: &View) {
        // Umbrechend: Label, fünf Satz-Reiter und Hinweis dürfen die Spalte nicht verbreitern (#303).
        ui.horizontal_wrapped(|ui| {
            ui.label(theme::panel_label("CART-WALL")).on_hover_text(
                "Taste startet und stoppt · Rechtsklick: Umbenennen, Belegen, Taste, Schnitt",
            );
            // Sätze 1–5 als Segmentschalter (Design-Runde 3, #292): Klick wechselt, Rechtsklick
            // benennt um; der Name des gewählten Satzes steht daneben.
            let nums: Vec<String> = (1..=v.cart_sets.len()).map(|i| i.to_string()).collect();
            let rs = segments(ui, &nums, v.cart_set);
            for (i, (r, name)) in rs.into_iter().zip(v.cart_sets.iter()).enumerate() {
                let label = format!("{} {name}", i + 1);
                let sel = i == v.cart_set;
                r.widget_info(|| {
                    egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, sel, &label)
                });
                let r = r.on_hover_text(format!(
                    "{name}: Cart-Wall-Satz wechseln · Rechtsklick: Umbenennen"
                ));
                if r.clicked() && i != v.cart_set {
                    self.with(move |c| {
                        if let Err(e) = c.select_cart_set(i) {
                            c.report(e);
                        }
                    });
                }
                r.context_menu(|ui| {
                    if ui.button("Umbenennen …").clicked() {
                        self.rename = Some((Rename::CartSet(i), name.clone(), false));
                        ui.close();
                    }
                });
            }
            if let Some(name) = v.cart_sets.get(v.cart_set) {
                ui.label(
                    egui::RichText::new(name)
                        .size(12.0)
                        .color(theme::text_dim()),
                );
            }
            // Bedienhinweis am Titel; nur die laufende Tastenwahl steht sichtbar daneben.
            if let Some(n) = self.key_capture {
                ui.label(
                    egui::RichText::new(format!(
                        "Taste für Cart {} drücken · Esc: abbrechen · Entf: ohne Taste",
                        n + 1
                    ))
                    .small()
                    .color(theme::text_dim()),
                );
            }
        });
        ui.add_space(2.0);

        let w = ((ui.available_width() - 3.0 * 6.0) / 4.0).max(60.0);
        let mut cut: Option<usize> = None;
        egui::Grid::new("carts")
            .spacing(egui::vec2(6.0, 6.0))
            .show(ui, |ui| {
                for slot in 0..CARTS {
                    let label = v.carts.get(slot).cloned().flatten();
                    let playing = v.carts_playing & (1 << slot) != 0;
                    // Die Nummer ist die große Zahl; die Taste steht klein in der Ecke,
                    // sobald sie nicht die Nummer selbst ist.
                    let key = v.cart_keys.get(slot).cloned().flatten();
                    let key_text = match &key {
                        Some(k) if cart_number_key(slot).as_deref() == Some(k.as_str()) => None,
                        // Alte Belegung auf einem Deck-Buchstaben: wirkt nicht mehr.
                        Some(k) if cart_key_of(k).is_none() => Some("–".to_string()),
                        Some(k) => Some(k.clone()),
                        None => Some("–".to_string()),
                    };
                    // Das Pad muss vor dem Zeichnen wissen, ob oben rechts Symbole liegen (#388).
                    let pfl_here = v.pfl_source == Some(PflSource::Cart(slot));
                    let show_pfl = pfl_key_shown(label.is_some());
                    let r = crate::widgets::CartPad {
                        number: slot + 1,
                        title: label.as_deref(),
                        key: key_text.as_deref(),
                        capturing: self.key_capture == Some(slot),
                        playing,
                        color: theme::track_color(slot),
                        remaining: v.cart_remaining.get(slot).copied().filter(|r| *r > 0.0),
                        seconds: v.cart_seconds.get(slot).copied().flatten(),
                        vocal_in: v.cart_vocal_in.get(slot).copied().flatten(),
                        actions: usize::from(playing) + usize::from(show_pfl),
                    }
                    .ui(ui, egui::vec2(w, CART_PAD_HEIGHT));
                    if self.cart_rects.len() <= slot {
                        self.cart_rects.resize(slot + 1, egui::Rect::NOTHING);
                    }
                    self.cart_rects[slot] = r.rect;
                    // Fade-Symbol oben rechts (FadeButton, #274): beim Hover oder laufend.
                    let now = ui.input(|i| i.time);
                    let pr = self
                        .fades
                        .progress(FadeKey::Cart(slot), now)
                        .filter(|_| playing);
                    if playing {
                        let at = egui::Rect::from_min_size(
                            r.rect.right_top() + egui::vec2(-28.0, 4.0),
                            egui::vec2(24.0, 24.0),
                        );
                        // Eigener Kind-Bereich ohne Platz im Raster (#369/#370): `scope_builder`
                        // belegte eine Grid-Zelle und schob den Nachbarn aus dem Bild.
                        let fr = fade_button(
                            &mut ui.new_child(egui::UiBuilder::new().max_rect(at)),
                            FadeShape::Icon(24.0),
                            &format!("Fade Cart {}", slot + 1),
                            true,
                            pr,
                            self.fades.seconds(),
                        );
                        if fr.clicked() {
                            self.fades.press(FadeKey::Cart(slot), now);
                            self.with(move |c| c.fade_cart(slot));
                        }
                    }
                    // Vorhören (#273) oben rechts, links neben dem Fade-Symbol: an jedem belegten
                    // Pad (Moderator 05.11.2026).
                    if show_pfl {
                        let x = if playing { -54.0 } else { -28.0 };
                        let at = egui::Rect::from_min_size(
                            r.rect.right_top() + egui::vec2(x, 4.0),
                            egui::vec2(24.0, 24.0),
                        );
                        let hint = self.pfl_hint();
                        let pr = {
                            let ui = &mut ui.new_child(egui::UiBuilder::new().max_rect(at));
                            {
                                let k = icon_key(
                                    ui,
                                    theme::ic::HEADPHONES,
                                    &format!("Vorhören Cart {}", slot + 1),
                                    theme::cue(),
                                    pfl_here,
                                    v.pfl_available,
                                    24.0,
                                );
                                if v.pfl_available {
                                    k.on_hover_text(if pfl_here {
                                        "Vorhören stoppen"
                                    } else {
                                        "Im Kopfhörer vorhören (nie auf Sendung)"
                                    })
                                } else {
                                    k.on_disabled_hover_text(hint)
                                }
                            }
                        };
                        if pr.clicked() {
                            self.with(move |c| {
                                if let Err(e) = c.pfl_toggle(PflSource::Cart(slot)) {
                                    c.report(e);
                                }
                            });
                        }
                    }

                    if r.clicked() {
                        if label.is_some() {
                            self.with(move |c| c.trigger_cart(slot));
                        } else {
                            self.assign_cart(slot);
                        }
                    }
                    r.context_menu(|ui| {
                        if ui.button("Belegen …").clicked() {
                            self.assign_cart(slot);
                            ui.close();
                        }
                        if let Some(name) = &label
                            && ui.button("Umbenennen …").clicked()
                        {
                            self.rename = Some((Rename::Cart(slot), name.clone(), false));
                            ui.close();
                        }
                        if let Some(t) = crate::controls::learn_menu(ui, Target::Cart(slot), None) {
                            self.learn = Some(t);
                        }
                        let fade_word = format!(
                            "Ausblenden ({})",
                            crate::widgets::fade::secs_text(self.fades.seconds())
                        );
                        if playing && ui.button(fade_word).clicked() {
                            let now = ui.input(|i| i.time);
                            self.fades.press(FadeKey::Cart(slot), now);
                            self.with(move |c| c.fade_cart(slot));
                            ui.close();
                        }
                        if label.is_some()
                            && ui
                                .add_enabled(
                                    v.pfl_available,
                                    pfl_button(v.pfl_source == Some(PflSource::Cart(slot))),
                                )
                                .on_disabled_hover_text(self.pfl_hint())
                                .clicked()
                        {
                            self.with(move |c| {
                                if let Err(e) = c.pfl_toggle(PflSource::Cart(slot)) {
                                    c.report(e);
                                }
                            });
                            ui.close();
                        }
                        if ui.button("Taste ändern …").clicked() {
                            self.key_capture = Some(slot);
                            ui.close();
                        }
                        if label.is_some() && ui.button("Start und Ende schneiden …").clicked() {
                            cut = Some(slot);
                            ui.close();
                        }
                        if label.is_some() && ui.button("Leeren").clicked() {
                            self.with(move |c| {
                                let _ = c.assign_cart(slot, None);
                            });
                            ui.close();
                        }
                    });
                    if slot % 4 == 3 {
                        ui.end_row();
                    }
                }
            });
        if let Some(slot) = cut {
            self.open_cart_cue_editor(slot);
        }
    }

    /// Rechtsklick auf einen Titel: Cue-Punkte, Vorhören (Paket 15).
    fn title_menu(&mut self, ui: &mut egui::Ui, i: usize, e: &ViewEntry, v: &View) {
        if e.voice.is_some() {
            if ui.button("Voice-Track bearbeiten …").clicked() {
                self.open_voice_editor(i);
                ui.close();
            }
        } else if ui.button("Voice-Track danach einfügen …").clicked() {
            self.add_voice_dialog(i + 1);
            ui.close();
        }
        if ui.button("Umbenennen …").clicked() {
            self.rename = Some((Rename::Title(i), e.title.clone(), false));
            ui.close();
        }
        // Doppelklick setzt hier schon „als Nächstes“, darum Titelinfos nur im Kontextmenü.
        if ui.button("Titelinfos bearbeiten …").clicked() {
            self.tag_wish = Some(e.path.clone());
            ui.close();
        }
        if ui.button("Cue-Punkte …").clicked() {
            self.cue_editor = Some(CueEditor::open(
                i,
                e.path.clone(),
                e.title.clone(),
                e.seconds,
                e.cues,
            ));
            ui.close();
        }
        let here = v.pfl_source == Some(PflSource::Title(i));
        for hook in [false, true] {
            if hook && e.cues.hook.is_none() {
                continue;
            }
            let button = if hook {
                egui::Button::new("Vorhören ab Hook")
            } else {
                pfl_button(here)
            };
            if ui
                .add_enabled(v.pfl_available, button)
                .on_disabled_hover_text(self.pfl_hint())
                .clicked()
            {
                self.with(move |c| {
                    let r = if hook {
                        c.pfl_title(i, true)
                    } else {
                        c.pfl_toggle(PflSource::Title(i))
                    };
                    if let Err(e) = r {
                        c.report(e);
                    }
                });
                ui.close();
            }
        }
        if e.voice.is_none() {
            ui.menu_button("Auf Deck laden", |ui| {
                for k in 0..woa_radio::DECKS {
                    let label = format!("Deck {}", woa_radio::show::deck_name(k));
                    if ui.button(label).clicked() {
                        self.with(move |c| {
                            if let Err(e) = c.deck_load_title(k, i) {
                                c.report(e);
                            }
                        });
                        ui.close();
                    }
                }
            });
        }
        if v.pfl_playing && ui.button("Vorhören anhalten").clicked() {
            self.with(Controller::pfl_stop);
            ui.close();
        }
        // Verschieben und Entfernen: auch ohne Hover-Knöpfe erreichbar.
        ui.separator();
        if i > 0 && ui.button("Nach oben").clicked() {
            self.with(move |c| {
                c.move_entry(i, i - 1);
            });
            ui.close();
        }
        if i + 1 < v.playlist.len() && ui.button("Nach unten").clicked() {
            self.with(move |c| {
                c.move_entry(i, i + 1);
            });
            ui.close();
        }
        if ui.button("Entfernen").clicked() {
            self.with(move |c| {
                c.remove(i);
            });
            ui.close();
        }
    }

    /// Fenster „Verwalten“, Rückfrage beim Wechsel und Undo-Hinweis (aus `app.rs` je Bild).
    pub fn manage_window(
        &mut self,
        ctx: &egui::Context,
        library: &mut crate::library::LibraryWindow,
    ) {
        let mut acts = Vec::new();
        // Neue aktive Playlist: ungemessene Titel im Hintergrund messen.
        if self.manage.seen_active != Some(self.view.active_playlist)
            && let Some(st) = &self.station
        {
            if self.manage.seen_active.is_some() {
                st.measure_missing(&st.show());
            }
            self.manage.seen_active = Some(self.view.active_playlist);
        }
        let names = self.view.playlists.clone();
        if let Some(a) = self.manage.confirm_ui(ctx, &names) {
            acts.push(a);
        }
        if self.manage.open
            && let Some(st) = &self.station
        {
            let show = st.show();
            let target = show
                .playlists
                .get(self.manage.selected)
                .map(|p| p.name.clone())
                .unwrap_or_default();
            acts.extend(self.manage.window(ctx, &show, self.view.running, |ui| {
                crate::titel_tab::titel_tab(ui, library, &target)
            }));
        }
        if let Some(a) = self.manage.undo_toast(ctx) {
            acts.push(a);
        }
        self.run_actions(acts);
    }

    /// Schickt Aufträge aus Kopf und Fenster an den Controller.
    fn run_actions(&mut self, acts: Vec<crate::manage::Action>) {
        use crate::manage::{Action, UndoSlot};
        for a in acts {
            match a {
                Action::Select(i) => self.with(move |c| {
                    c.select_playlist(i);
                }),
                Action::New => {
                    let open = self.manage.open;
                    self.with(move |c| {
                        let i = c.new_playlist("");
                        if !open && !c.on_air() {
                            c.select_playlist(i);
                        }
                    });
                    // Im Fenster gleich auswählen (neue steht hinten).
                    self.manage.selected = self.view.playlists.len();
                    self.manage.marked = None;
                }
                Action::Rename(i, name) => self.with(move |c| {
                    if let Err(e) = c.rename_playlist(i, &name) {
                        c.report(e);
                    }
                }),
                Action::Duplicate(i) => {
                    self.manage.selected = i + 1;
                    self.with(move |c| {
                        c.duplicate_playlist(i);
                    });
                }
                Action::Clear(i) => self.with(move |c| {
                    c.clear_playlist(i);
                }),
                Action::Delete(i) => {
                    let slot = self.manage.undo.clone();
                    self.with(move |c| match c.delete_playlist(i) {
                        Ok(d) => {
                            if let Ok(mut s) = slot.lock() {
                                *s = Some(UndoSlot {
                                    deleted: d,
                                    until: None,
                                });
                            }
                        }
                        Err(e) => c.report(e),
                    });
                    self.manage.marked = None;
                }
                Action::Undo => {
                    let taken = self.manage.undo.lock().ok().and_then(|mut s| s.take());
                    if let Some(u) = taken {
                        self.manage.selected = u.deleted.index;
                        self.with(move |c| {
                            c.restore_playlist(u.deleted);
                        });
                    }
                }
                Action::Import(p) => self.with(move |c| {
                    if let Err(e) = c.import_m3u(&p) {
                        c.report(e);
                    }
                }),
                Action::Export(i, p) => self.with(move |c| {
                    if let Err(e) = c.export_m3u(i, &p) {
                        c.report(e);
                    }
                }),
                Action::Add(i, files) => {
                    if i == self.view.active_playlist
                        && let Some(s) = &self.station
                    {
                        // Wie „+ Titel …“: messen im Hintergrund.
                        s.add_files(files);
                    } else {
                        self.with(move |c| {
                            c.playlist_add(i, &files);
                        });
                    }
                }
                Action::Remove(i, k) => self.with(move |c| {
                    c.playlist_remove(i, k);
                }),
                Action::Move(i, from, to) => self.with(move |c| {
                    c.playlist_move(i, from, to);
                }),
            }
        }
    }

    /// Wählt eine Sprachdatei und legt sie als Voice-Track bei `at` in die Playlist.
    fn add_voice_dialog(&self, at: usize) {
        if let Some(path) = crate::ordner::dialog(crate::ordner::Art::Aufnahmen)
            .add_filter(
                woa_engine::decode::FILTER_NAME,
                &woa_engine::decode::dialog_extensions(),
            )
            .pick_file_modal()
        {
            self.add_voice_file(path, Some(at));
        }
    }

    /// Legt eine Sprachdatei als Voice-Track bei `at` (sonst hinten) in die Playlist.
    pub fn add_voice_file(&self, path: PathBuf, at: Option<usize>) {
        self.with(move |c| {
            if let Err(e) = c.add_voice(&path, at) {
                c.report(e);
            }
        });
    }

    /// Öffnet den Voice-Track-Editor für Eintrag `i` (falls es ein Voice-Track ist).
    pub fn open_voice_editor(&mut self, i: usize) {
        let v = &self.view;
        let Some(e) = v.playlist.get(i) else { return };
        let Some(vb) = e.voice else { return };
        let music = |k: Option<usize>| k.and_then(|k| v.playlist.get(k));
        let a = music((0..i).rev().find(|&k| v.playlist[k].voice.is_none()));
        let b = music((i + 1..v.playlist.len()).find(|&k| v.playlist[k].voice.is_none()));
        let input = VoiceInput {
            index: i,
            title: e.title.clone(),
            voice_path: e.path.clone(),
            voice_seconds: e.seconds,
            a_path: a.map(|x| x.path.clone()),
            a_title: a.map(|x| x.title.clone()).unwrap_or_else(|| "–".into()),
            a_seconds: a.and_then(|x| x.seconds),
            a_cues: a.map(|x| x.cues).unwrap_or_default(),
            b_path: b.map(|x| x.path.clone()),
            b_title: b.map(|x| x.title.clone()).unwrap_or_else(|| "–".into()),
            b_seconds: b.and_then(|x| x.seconds),
            b_cues: b.map(|x| x.cues).unwrap_or_default(),
            crossfade: v.crossfade_seconds,
            params: vb.params,
            check: vb.check,
        };
        self.voice_editor = Some(VoiceEditor::open(input));
    }

    /// Zeichnet den Voice-Track-Editor, falls offen, und reicht seine Aufträge weiter.
    pub fn voice_window(&mut self, ctx: &egui::Context) {
        let Some(ed) = self.voice_editor.as_mut() else {
            return;
        };
        let v = &self.view;
        // Eintrag verschoben oder entfernt: Editor schließen statt falschen zu ändern.
        let Some(cur) = v
            .playlist
            .get(ed.input.index)
            .filter(|e| e.path == ed.input.voice_path && e.voice.is_some())
        else {
            self.voice_editor = None;
            return;
        };
        if let Some(vb) = cur.voice {
            ed.sync(vb.params, vb.check);
        }
        let mut open = true;
        let actions = ed.ui(ctx, &mut open, (v.pfl_available, v.pfl_playing));
        if !open {
            self.voice_editor = None;
        }
        for a in actions {
            match a {
                VoiceAction::Apply(i, p) => self.with(move |c| {
                    if let Err(e) = c.set_voice(i, p) {
                        c.report(e);
                    }
                }),
                VoiceAction::Confirm(i) => self.with(move |c| {
                    if let Err(e) = c.confirm_voice(i) {
                        c.report(e);
                    }
                }),
                VoiceAction::Preview(i) => self.with(move |c| {
                    if let Err(e) = c.pfl_voice(i) {
                        c.report(e);
                    }
                }),
                VoiceAction::StopPreview => self.with(Controller::pfl_stop),
            }
        }
    }

    /// Öffnet den Cue-Editor für Titel `i` (falls es ihn gibt).
    pub fn open_cue_editor(&mut self, i: usize) {
        if let Some(e) = self.view.playlist.get(i) {
            self.cue_editor = Some(CueEditor::open(
                i,
                e.path.clone(),
                e.title.clone(),
                e.seconds,
                e.cues,
            ));
        }
    }

    /// Öffnet den Schnitt (Start und Ende) für Cart `slot`.
    pub fn open_cart_cue_editor(&mut self, slot: usize) {
        let path = self.view.cart_paths.get(slot).cloned().flatten();
        if let (Some(path), Some(Some(title))) = (path, self.view.carts.get(slot)) {
            let cues = self.view.cart_cues.get(slot).copied().unwrap_or_default();
            let mut ed = CueEditor::open(slot, path, title.clone(), None, cues);
            ed.cart = true;
            self.cue_editor = Some(ed);
        }
    }

    /// Zeichnet den Cue-Editor, falls offen, und reicht seine Aufträge weiter.
    /// Dialog „Umbenennen“ für Satz, Cart und Titel (#292). Enter übernimmt, Esc bricht ab;
    /// ein leerer Name setzt den Dateinamen (bzw. „Satz n“) zurück.
    fn rename_window(&mut self, ctx: &egui::Context) {
        let Some((what, buf, focused)) = self.rename.as_mut() else {
            return;
        };
        let what = *what;
        let mut done = None;
        let mut open = true;
        egui::Window::new(what.heading())
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                let r = ui.add(egui::TextEdit::singleline(buf).desired_width(260.0));
                if !*focused {
                    r.request_focus();
                    *focused = true;
                }
                let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                ui.label(
                    egui::RichText::new("Leer: zurück zum Dateinamen")
                        .small()
                        .color(theme::text_dim()),
                );
                ui.horizontal(|ui| {
                    if ui.button("Übernehmen").clicked() || enter {
                        done = Some(true);
                    }
                    // #441: Esc heißt Abbrechen, über die gemeinsame Regel (Fragen zuerst).
                    let esc = crate::esc::frage(ui.ctx(), ui.layer_id(), "umbenennen");
                    if ui.button("Abbrechen").clicked()
                        || esc
                        || ui.input(|i| i.key_pressed(egui::Key::Escape))
                    {
                        done = Some(false);
                    }
                });
            });
        if !open {
            done = Some(false);
        }
        if let Some(apply) = done
            && let Some((what, name, _)) = self.rename.take()
            && apply
        {
            self.with(move |c| {
                if let Err(e) = what.apply(c, &name) {
                    c.report(e);
                }
            });
        }
    }

    pub fn cue_window(&mut self, ctx: &egui::Context) {
        self.rename_window(ctx);
        let Some(ed) = self.cue_editor.as_mut() else {
            return;
        };
        let v = &self.view;
        // Titel verschoben oder entfernt: Editor schließen statt falschen Titel zu ändern.
        let gone = if ed.deck {
            v.decks.get(ed.index).is_none_or(|d| d.title.is_none())
        } else if ed.cart {
            v.carts.get(ed.index).is_none_or(Option::is_none)
        } else {
            v.playlist.get(ed.index).map(|e| &e.path) != Some(&ed.path)
        };
        if gone {
            self.cue_editor = None;
            return;
        }
        let mut open = true;
        let actions = ed.ui(
            ctx,
            &mut open,
            (v.pfl_available, v.pfl_playing),
            (v.cues_undo && !ed.cart, v.cues_redo && !ed.cart),
        );
        let cart = ed.cart;
        let deck = ed.deck;
        if !open {
            self.cue_editor = None;
        }
        for a in actions {
            match a {
                CueAction::Apply(i, cues) => self.with(move |c| {
                    let r = if deck {
                        c.set_deck_cues(i, cues)
                    } else if cart {
                        c.set_cart_cues(i, cues)
                    } else {
                        c.set_cues(i, cues)
                    };
                    if let Err(e) = r {
                        c.report(e);
                    }
                }),
                CueAction::Preview(i, from) => self.with(move |c| {
                    let r = if deck {
                        c.pfl_deck_at(i, from)
                    } else if cart {
                        c.pfl_cart_at(i, from)
                    } else {
                        c.pfl_title_at(i, from)
                    };
                    if let Err(e) = r {
                        c.report(e);
                    }
                }),
                CueAction::StopPreview => self.with(Controller::pfl_stop),
                CueAction::Undo => self.with(|c| {
                    c.undo_cues();
                }),
                CueAction::Redo => self.with(|c| {
                    c.redo_cues();
                }),
            }
        }
        // Nach Undo/Redo zeigt der Editor die Punkte aus der Playlist.
        if let Some(ed) = self.cue_editor.as_mut()
            && let Some(e) = self.view.playlist.get(ed.index)
            && self.cue_seen.get(&ed.index) != Some(&e.cues)
        {
            ed.draft = e.cues;
            self.cue_seen.insert(ed.index, e.cues);
        }
    }

    /// Warum das Vorhören aus ist: Einstellung „Aus“ oder der Fehler beim Öffnen.
    pub fn pfl_note(&self) -> Option<&str> {
        self.pfl_note.as_deref()
    }

    /// Tooltip für deaktivierte Vorhören-Knöpfe.
    pub fn pfl_hint(&self) -> String {
        pfl_hint_for(self.pfl_note.as_deref())
    }

    /// Ob das Vorhören (PFL) einen Ausgang hat.
    pub fn pfl_available(&self) -> bool {
        self.view.pfl_available
    }

    /// Hört eine Datei der Bibliothek im Kopfhörer vor (nie auf Sendung, derselbe PFL-Weg).
    pub fn preview_file(&self, path: PathBuf) {
        self.with(move |c| {
            if let Err(e) = c.pfl_file(&path) {
                c.report(e);
            }
        });
    }

    pub fn preview_stop(&self) {
        self.with(Controller::pfl_stop);
    }

    /// Eine aus der Bibliothek gezogene Datei bei `pos` ablegen: auf einem Cart-Pad wird sie
    /// dem Pad zugewiesen, auf der Playlist hinten angehängt. Gibt zurück, ob etwas geschah.
    pub fn drop_library_path(&mut self, path: PathBuf, pos: egui::Pos2) -> bool {
        if let Some(k) = self.deck_rects.iter().position(|r| r.contains(pos)) {
            self.deck_load_file(k, path);
            return true;
        }
        if let Some(slot) = self.cart_rects.iter().position(|r| r.contains(pos)) {
            self.assign_cart_file(slot, path);
            return true;
        }
        if self.playlist_rect.contains(pos)
            && let Some(s) = &self.station
        {
            s.add_files(vec![path]);
            return true;
        }
        false
    }

    /// Belegt Cart-Pad `slot` mit einer Datei (Ziehen aus Bibliothek, Playlist, Explorer).
    fn assign_cart_file(&self, slot: usize, path: PathBuf) {
        self.with(move |c| {
            if let Err(e) = c.assign_cart(slot, Some(path)) {
                c.report(e);
            }
        });
    }

    /// Legt eine Datei auf Deck `k` (steht danach am Cue-In).
    fn deck_load_file(&self, k: usize, path: PathBuf) {
        self.with(move |c| {
            if let Err(e) = c.deck_load(k, path, woa_radio::Cues::default()) {
                c.report(e);
            }
        });
    }

    fn pick_deck_file(&self, k: usize) {
        if let Some(path) = crate::ordner::dialog_musik()
            .add_filter(
                woa_engine::decode::FILTER_NAME,
                &woa_engine::decode::dialog_extensions(),
            )
            .pick_file_modal()
        {
            self.deck_load_file(k, path);
        }
    }

    /// Öffnet den Schnitt (Start und Ende) für Deck `k`.
    fn open_deck_cue_editor(&mut self, k: usize) {
        if let Some(d) = self.view.decks.get(k)
            && let (Some(path), Some(title)) = (d.path.clone(), d.title.clone())
        {
            let mut ed = CueEditor::open(k, path, title, None, d.cues);
            ed.cart = true;
            ed.deck = true;
            self.cue_editor = Some(ed);
        }
    }

    /// Die drei Decks: große Restzeit, Start und Stopp, Laden per Ziehen oder Menü.
    fn deck_card(&mut self, ui: &mut egui::Ui, v: &View) {
        ui.horizontal(|ui| {
            ui.label(theme::panel_label("DECKS A · B · C"));
            ui.label(
                egui::RichText::new(
                    "Tasten A–C starten · Titel aus der Playlist hierher ziehen · Rechtsklick: Playlist ablösen",
                )
                .size(12.0)
                .color(theme::text_dim()),
            );
        });
        ui.add_space(4.0);
        self.deck_rows(ui, v);
    }

    /// Karte PEGEL (RadioLevels): vier Kanalzüge, Master-Meter, Notebook und Stream.
    fn levels_card(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(theme::panel_label("PEGEL"));
            ui.label(
                egui::RichText::new("Quellen · Master · Ausgänge")
                    .size(12.0)
                    .color(theme::text_dim()),
            );
        });
        ui.add_space(4.0);
        ui.horizontal_top(|ui| self.level_fader_group(ui));
    }

    /// Pegel Playlist, Carts, Decks, Mikro als senkrechte Fader (Werte wie bisher).
    fn level_fader_group(&mut self, ui: &mut egui::Ui) {
        let mut dbs = FADER_BUSES.map(|(b, _)| self.level_db(b) as f32);
        let rs = level_faders(ui, &mut dbs, &self.meters);
        for ((r, (b, _)), db) in rs.iter().zip(FADER_BUSES).zip(dbs) {
            if r.changed() {
                self.set_level(b, f64::from(db));
            }
            self.learn_menu(r, Target::Level(b));
        }
        // Kein `separator()`: senkrecht in der rollenden Spalte nimmt er die ganze Höhe und
        // schob die Karten darunter (Cart-Wall) aus dem Bild (#303).
        group_sep(ui, FADER_HEIGHT + 40.0);
        let mut outs = [self.view.out_monitor, self.view.out_stream];
        let ch = output_faders(ui, &mut outs, &self.out_meters);
        for ((out, _, _), (o, c)) in OUTPUTS.iter().zip(outs.into_iter().zip(ch)) {
            if c {
                match out {
                    woa_radio::show::Output::Monitor => self.view.out_monitor = o,
                    woa_radio::show::Output::Stream => self.view.out_stream = o,
                }
                let out = *out;
                self.with(move |c| c.set_output(out, o));
            }
        }
    }

    fn deck_rows(&mut self, ui: &mut egui::Ui, v: &View) {
        for k in 0..woa_radio::DECKS {
            let mut d = v.decks.get(k).cloned().unwrap_or_default();
            let name = woa_radio::show::deck_name(k);
            let loaded = d.title.is_some();
            // #375: Wird das Deck vorgehört, läuft es sichtbar (egal auf welchem Ausgang).
            let pfl_pos = (v.pfl_source == Some(PflSource::Deck(k))).then_some(v.pfl_position);
            let previewing = pfl_pos.is_some() && !d.playing;
            // `runs` färbt Zeile, Buchstabe und Uhr; die Tasten folgen weiter `d.playing`.
            let runs;
            (runs, d.remaining) = deck_shown(&d, pfl_pos);
            if previewing {
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_millis(100));
            }
            let urgent = runs && d.remaining < 10.0;
            // Unter 10 s blinkt die Restzeit rot (RadioDecks).
            let urgent_on = urgent && crate::widgets::blink_on(ui.input(|i| i.time), 2.0);
            if urgent {
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_millis(120));
            }
            // Zieht jemand gerade eine Playlist-Zeile über dieses Deck: „Hier ablegen“ (#298).
            let drop_here = self.row_drag.is_some()
                && ui
                    .ctx()
                    .pointer_latest_pos()
                    .is_some_and(|p| self.deck_rects[k].contains(p));
            let fill = if drop_here {
                theme::mix(theme::accent_2(), theme::surface(), 0.88)
            } else if runs {
                theme::mix(theme::accent(), theme::surface(), 0.90)
            } else if loaded {
                theme::surface()
            } else {
                egui::Color32::TRANSPARENT
            };
            let edge = if drop_here {
                theme::accent_2()
            } else if runs {
                theme::accent()
            } else if loaded {
                theme::border_strong()
            } else {
                egui::Color32::TRANSPARENT
            };
            let frame = egui::Frame::new()
                .fill(fill)
                .stroke(egui::Stroke::new(1.0, edge))
                .corner_radius(theme::RADIUS_MD)
                .inner_margin(egui::Margin::symmetric(8, 5));
            // Die ganze Deck-Zeile ist eine Klickfläche (#302), die Knöpfe klicken selbst.
            let out = row_area(ui, ("deck_row", k), egui::Sense::click(), frame, |ui| {
                ui.set_width(ui.available_width());
                ui.set_min_height(DECK_ROW_H);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 10.0;
                    // Buchstabe im LCD-Feld, leuchtet in lcd-ink, solange das Deck spielt.
                    let (lr, _) =
                        ui.allocate_exact_size(egui::vec2(44.0, DECK_ROW_H), egui::Sense::hover());
                    ui.painter()
                        .rect_filled(lr, theme::RADIUS_MD, theme::lcd_bg());
                    ui.painter().text(
                        lr.center(),
                        egui::Align2::CENTER_CENTER,
                        name.to_string(),
                        theme::label_font(30.0),
                        if runs {
                            theme::lcd_ink()
                        } else {
                            theme::mix(theme::lcd_text(), theme::lcd_bg(), 0.55)
                        },
                    );
                    // Restzeit-LCD 132 px: 7-Segment mit Geistern, Fortschritt darunter.
                    let (cr, _) = ui.allocate_exact_size(
                        egui::vec2(DECK_CLOCK_WIDTH, DECK_ROW_H),
                        egui::Sense::hover(),
                    );
                    ui.painter()
                        .rect_filled(cr, theme::RADIUS_MD, theme::lcd_bg());
                    let ink = if urgent_on {
                        theme::on_air()
                    } else if urgent {
                        theme::mix(theme::on_air(), theme::lcd_bg(), 0.5)
                    } else if loaded {
                        theme::lcd_ink()
                    } else {
                        theme::led_off()
                    };
                    let text = if loaded {
                        clock(d.remaining)
                    } else {
                        "--:--".to_string()
                    };
                    let inner = cr.shrink2(egui::vec2(10.0, 6.0));
                    ui.scope_builder(
                        egui::UiBuilder::new()
                            .max_rect(inner)
                            .layout(egui::Layout::top_down(egui::Align::Min)),
                        |ui| {
                            ui.spacing_mut().item_spacing.y = 4.0;
                            let r = seg_text(ui, &text, 22.0, ink);
                            let rest = format!("Restzeit Deck {name}");
                            r.widget_info(|| {
                                egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &rest)
                            });
                            let frac = if d.length > 0.0 {
                                (1.0 - d.remaining / d.length).clamp(0.0, 1.0) as f32
                            } else {
                                0.0
                            };
                            let (bar, _) = ui.allocate_exact_size(
                                egui::vec2(inner.width(), 4.0),
                                egui::Sense::hover(),
                            );
                            ui.painter().rect_filled(bar, 2.0, theme::led_off());
                            if frac > 0.0 {
                                let mut f = bar;
                                f.set_width(bar.width() * frac);
                                ui.painter().rect_filled(f, 2.0, ink);
                            }
                            // Gesangseinsatz (#299) als Strich auf dem Balken.
                            if loaded {
                                crate::vocal_ui::vocal_tick(
                                    ui.painter(),
                                    bar,
                                    d.vocal,
                                    d.cues.start(),
                                    d.length,
                                );
                            }
                        },
                    );
                    // Tasten rechts: Start/Stopp mit Text, Cue, Vorhören und Rückweg als Symbole;
                    // der Titel füllt den Rest.
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        if deck_return_toggle(ui, k, d.back_to_playlist).clicked() {
                            let on = !d.back_to_playlist;
                            self.with(move |c| c.set_deck_return(k, on));
                        }
                        let pfl_on = v.pfl_source == Some(PflSource::Deck(k));
                        let r = icon_key(
                            ui,
                            theme::icf::HEADPHONES,
                            &format!("Vorhören Deck {name}"),
                            theme::cue(),
                            pfl_on,
                            loaded && v.pfl_available,
                            SYMBOL_KEY,
                        )
                        .on_hover_text(if pfl_on {
                            "Vorhören stoppen"
                        } else {
                            "Vorhören (PFL) im Kopfhörer"
                        })
                        .on_disabled_hover_text(self.pfl_hint());
                        self.learn_menu(&r, Target::Deck(k, DeckAction::Pfl));
                        if r.clicked() {
                            self.with(move |c| {
                                if let Err(e) = c.pfl_toggle(PflSource::Deck(k)) {
                                    c.report(e);
                                }
                            });
                        }
                        let r = icon_key(
                            ui,
                            theme::icf::SKIP_BACK,
                            &format!("Cue Deck {name}"),
                            theme::accent_2(),
                            false,
                            loaded,
                            SYMBOL_KEY,
                        )
                        .on_hover_text("Cue: zurück an den Anfang (Rechtsklick: MIDI lernen)");
                        self.learn_menu(&r, Target::Deck(k, DeckAction::Cue));
                        if r.clicked() {
                            self.with(move |c| c.deck_cue(k));
                        }
                        // Fade (FadeButton, #321): quadratisch neben Start/Stopp.
                        let now = ui.input(|i| i.time);
                        let pr = self
                            .fades
                            .progress(FadeKey::Deck(k), now)
                            .filter(|_| d.playing);
                        let r = fade_button(
                            ui,
                            FadeShape::Square(SYMBOL_KEY),
                            &format!("Fade Deck {name}"),
                            d.playing,
                            pr,
                            self.fades.seconds(),
                        );
                        self.learn_menu(&r, Target::Deck(k, DeckAction::Fade));
                        if r.clicked() {
                            self.fades.press(FadeKey::Deck(k), now);
                            self.with(move |c| c.fade_deck(k));
                        }
                        let (icon, word) = if d.playing {
                            (theme::icf::STOP, "Stopp")
                        } else {
                            (theme::icf::PLAY, "Start")
                        };
                        let r = ui.add_enabled(
                            loaded,
                            egui::Button::new(theme::fill_label(
                                icon,
                                &word.to_uppercase(),
                                13.0,
                                if d.playing {
                                    theme::lcd_bg()
                                } else {
                                    theme::text()
                                },
                            ))
                            .fill(if d.playing {
                                theme::ok()
                            } else {
                                theme::surface()
                            })
                            .stroke(egui::Stroke::new(
                                1.0,
                                if loaded {
                                    theme::ok()
                                } else {
                                    theme::border_strong()
                                },
                            ))
                            .corner_radius(theme::RADIUS_MD)
                            .min_size(egui::vec2(86.0, SYMBOL_KEY)),
                        );
                        let label = format!("{word} Deck {name}");
                        r.widget_info(|| {
                            egui::WidgetInfo::labeled(egui::WidgetType::Button, loaded, &label)
                        });
                        let r = r.on_hover_text(format!(
                            "Deck {name}: {word} (Taste {name}, Rechtsklick: MIDI lernen)"
                        ));
                        self.learn_menu(&r, Target::Deck(k, DeckAction::Start));
                        if r.clicked() {
                            if d.playing {
                                self.with(move |c| c.deck_stop(k));
                            } else {
                                self.with(move |c| c.deck_play(k));
                            }
                        }
                        ui.add_space(4.0);
                        ui.allocate_ui_with_layout(
                            ui.available_size() - egui::vec2(10.0, 0.0),
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| {
                                ui.spacing_mut().item_spacing.y = 2.0;
                                let (t, sub) = if drop_here {
                                    ("Hier ablegen".to_string(), String::new())
                                } else if let Some(t) = d.title.clone() {
                                    let state = if previewing {
                                        "Vorhören im Kopfhörer"
                                    } else if d.playing {
                                        "läuft"
                                    } else {
                                        "Deck geladen"
                                    };
                                    // Rampzeit (#299): wie lange das Intro bis zum Gesang läuft;
                                    // läuft das Deck, zählt sie herunter (Moderator 05.11.2026).
                                    let pos = d.cues.start() + (d.length - d.remaining);
                                    let left = d
                                        .playing
                                        .then(|| {
                                            crate::vocal_ui::seconds_to_vocal(d.vocal.as_ref(), pos)
                                        })
                                        .flatten();
                                    let sub = match (&d.vocal, left) {
                                        (_, Some(l)) => format!(
                                            "{state} · {}",
                                            crate::vocal_ui::vocal_in_text(l)
                                        ),
                                        (Some(m), None) if !d.playing => format!(
                                            "{state} · Gesang ab {}",
                                            crate::vocal_ui::vocal_text(m, d.cues.start())
                                        ),
                                        _ => state.to_string(),
                                    };
                                    (t, sub)
                                } else {
                                    (
                                        "Leer".to_string(),
                                        "Titel hierher ziehen · Rechtsklick: Datei laden"
                                            .to_string(),
                                    )
                                };
                                ui.add(
                                    egui::Label::new(
                                        egui::RichText::new(t).size(14.0).strong().color(
                                            if drop_here {
                                                theme::accent_2()
                                            } else if loaded {
                                                theme::text()
                                            } else {
                                                theme::text_dim()
                                            },
                                        ),
                                    )
                                    .truncate(),
                                );
                                if !sub.is_empty() {
                                    ui.add(
                                        egui::Label::new(
                                            egui::RichText::new(sub)
                                                .size(12.0)
                                                .color(theme::text_dim()),
                                        )
                                        .truncate(),
                                    );
                                }
                            },
                        );
                    });
                });
            });
            self.deck_rects[k] = out.response.rect;
            if !loaded && !drop_here {
                // Leeres Deck: gestrichelter Rand.
                let q = out.response.rect.shrink(0.5);
                let pts = [
                    q.left_top(),
                    q.right_top(),
                    q.right_bottom(),
                    q.left_bottom(),
                    q.left_top(),
                ];
                ui.painter().extend(egui::Shape::dashed_line(
                    &pts,
                    egui::Stroke::new(1.0, theme::border_strong()),
                    4.0,
                    3.0,
                ));
            }
            out.response.context_menu(|ui| {
                if loaded {
                    if let Some(fade) = deck_take_menu(ui, k, v.crossfade_seconds) {
                        self.with(move |c| {
                            if let Err(e) = c.deck_take_over(k, fade) {
                                c.report(e);
                            }
                        });
                        ui.close();
                    }
                    ui.separator();
                }
                if ui.button("Datei laden …").clicked() {
                    self.pick_deck_file(k);
                    ui.close();
                }
                if loaded && ui.button("Start und Ende schneiden …").clicked() {
                    self.open_deck_cue_editor(k);
                    ui.close();
                }
                if loaded
                    && ui
                        .button(egui::RichText::new("Leeren").color(theme::danger_text()))
                        .clicked()
                {
                    self.with(move |c| c.deck_eject(k));
                    ui.close();
                }
            });
            ui.add_space(2.0);
        }
    }

    fn assign_cart(&self, slot: usize) {
        if let Some(path) = crate::ordner::dialog(crate::ordner::Art::Jingles)
            .add_filter(
                woa_engine::decode::FILTER_NAME,
                &woa_engine::decode::dialog_extensions(),
            )
            .pick_file_modal()
        {
            self.with(move |c| {
                if let Err(e) = c.assign_cart(slot, Some(path)) {
                    tracing::warn!(error = %e, "cart not assigned");
                }
            });
        }
    }
}

/// ON-AIR-Lampe: leuchtet nur, wenn die Sendung läuft. Mikro an ist eine eigene Anzeige (#225).
/// Gemeinsame Höhe von Lampe, Taste und Status-Pille in der Sendezeile (#450).
const ON_AIR_ROW_H: f32 = 32.0;

/// Größe der kleinen ON-AIR-Lampe vorn: gleich hoch wie Taste und Pille.
fn on_air_lamp_size() -> egui::Vec2 {
    egui::vec2(64.0, ON_AIR_ROW_H)
}

fn onair_lamp_lit(on_air: bool, _mic_open: bool) -> bool {
    on_air
}

#[cfg(test)]
mod tests {
    use eframe::egui;

    /// Alle 16 Cart-Pads liegen im Fenster, bei 1366×768 und 1920×1080 (#303). Die Fläche ist
    /// die des Radio-Modus: Fenster minus Menüleiste (rund 36 px, hier 40), mit dem Rand des
    /// CentralPanel aus `app.rs`.
    #[test]
    fn all_cart_pads_fit_the_window() {
        const MENU_BAR: f32 = 40.0;
        for (w, h) in [(1366.0, 768.0), (1920.0, 1080.0)] {
            let dir = tempfile::tempdir().unwrap();
            let mut harness = egui_kittest::Harness::builder()
                .with_size(egui::vec2(w, h - MENU_BAR))
                .build_ui_state(
                    |ui, d: &mut super::RadioDesk| {
                        let ctx = ui.ctx().clone();
                        // Schriften und Farben wie in der App; sie greifen ab dem nächsten Bild.
                        let fresh = ctx.data_mut(|m| {
                            let id = egui::Id::new("test_fonts");
                            let fresh = m.get_temp::<bool>(id).is_none();
                            m.insert_temp(id, true);
                            fresh
                        });
                        if fresh {
                            crate::theme::install_fonts(&ctx);
                            crate::theme::apply(
                                &ctx,
                                crate::theme::Theme::Dunkel,
                                crate::theme::Mode::Radio,
                            );
                            return;
                        }
                        egui::CentralPanel::default()
                            .frame(egui::Frame::new().inner_margin(egui::Margin::symmetric(14, 12)))
                            .show(ui, |ui| d.ui(ui));
                    },
                    {
                        let mut d = super::RadioDesk::headless(dir.path());
                        d.view.cart_sets = (1..=5).map(|i| format!("Satz {i}")).collect();
                        d
                    },
                );
            harness.run_steps(3);
            let window = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(w, h - MENU_BAR));
            let rects = &harness.state().cart_rects;
            assert_eq!(rects.len(), 16, "{w}x{h}");
            for (i, r) in rects.iter().enumerate() {
                assert!(
                    window.contains_rect(*r),
                    "{w}x{h}: Cart {} bei {r:?} liegt außerhalb von {window:?}",
                    i + 1
                );
            }
        }
    }

    /// #369/#370: Ein laufendes Cart (Fade-Symbol) und das Vorhören-Symbol beim Hover
    /// dürfen im Raster keine eigene Zelle belegen; sonst rutscht der Nachbar aus dem Bild.
    #[test]
    fn playing_or_hovered_cart_keeps_the_grid() {
        for case in ["playing", "hover"] {
            let dir = tempfile::tempdir().unwrap();
            let (w, h) = (1920.0, 1040.0);
            let mut harness = egui_kittest::Harness::builder()
                .with_size(egui::vec2(w, h))
                .build_ui_state(
                    |ui, d: &mut super::RadioDesk| {
                        let ctx = ui.ctx().clone();
                        let fresh = ctx.data_mut(|m| {
                            let id = egui::Id::new("test_fonts");
                            let fresh = m.get_temp::<bool>(id).is_none();
                            m.insert_temp(id, true);
                            fresh
                        });
                        if fresh {
                            crate::theme::install_fonts(&ctx);
                            crate::theme::apply(
                                &ctx,
                                crate::theme::Theme::Dunkel,
                                crate::theme::Mode::Radio,
                            );
                            return;
                        }
                        egui::CentralPanel::default()
                            .frame(egui::Frame::new().inner_margin(egui::Margin::symmetric(14, 12)))
                            .show(ui, |ui| d.ui(ui));
                    },
                    {
                        let mut d = super::RadioDesk::headless(dir.path());
                        d.view.cart_sets = (1..=5).map(|i| format!("Satz {i}")).collect();
                        d.view.carts = (1..=16).map(|i| Some(format!("Jingle {i}"))).collect();
                        d.view.cart_remaining = vec![30.0; 16];
                        if case == "playing" {
                            d.view.carts_playing = 1 << 2;
                        }
                        d
                    },
                );
            harness.run_steps(3);
            if case == "hover" {
                let at = harness.state().cart_rects[2].center();
                harness
                    .input_mut()
                    .events
                    .push(egui::Event::PointerMoved(at));
            }
            harness.run_steps(3);
            let window = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(w, h));
            let rects = &harness.state().cart_rects;
            assert_eq!(rects.len(), 16, "{case}");
            for row in 0..4 {
                let y = rects[row * 4].min.y;
                for col in 1..4 {
                    let r = rects[row * 4 + col];
                    assert!(
                        (r.min.y - y).abs() < 0.5 && r.min.x > rects[row * 4 + col - 1].max.x,
                        "{case}: Cart {} bei {r:?} nicht in Reihe {}",
                        row * 4 + col + 1,
                        row + 1
                    );
                }
            }
            for (i, r) in rects.iter().enumerate() {
                assert!(window.contains_rect(*r), "{case}: Cart {} außerhalb", i + 1);
            }
        }
    }

    /// #375: Hört man ein Deck vor, läuft es sichtbar (Restzeit zählt ab der Vorhör-Stelle).
    #[test]
    fn previewed_deck_runs_visibly() {
        let d = woa_radio::station::DeckView {
            title: Some("Intro".into()),
            playing: false,
            remaining: 13.0,
            length: 13.0,
            ..Default::default()
        };
        assert_eq!(super::deck_shown(&d, None), (false, 13.0));
        assert_eq!(super::deck_shown(&d, Some(3.0)), (true, 10.0));
        assert_eq!(super::deck_shown(&d, Some(99.0)), (true, 0.0));
        let live = woa_radio::station::DeckView {
            playing: true,
            remaining: 5.0,
            ..d
        };
        assert_eq!(super::deck_shown(&live, Some(1.0)), (true, 5.0));
    }

    #[test]
    fn typed_position_moves_within_the_playlist() {
        // Eingetippte Nummer (#92): 1-basiert, auf die Liste begrenzt, Unsinn tut nichts.
        assert_eq!(super::typed_position(" 3 ", 5), Some(2));
        assert_eq!(super::typed_position("1", 5), Some(0));
        assert_eq!(super::typed_position("99", 5), Some(4));
        assert_eq!(super::typed_position("0", 5), Some(0));
        assert_eq!(super::typed_position("abc", 5), None);
        assert_eq!(super::typed_position("", 5), None);
        assert_eq!(super::typed_position("2", 0), None);
    }

    #[test]
    fn on_air_row_widgets_share_one_height() {
        // #450: Lampe, Taste und Status-Pille waren 24, 32 und 26 px hoch.
        assert_eq!(super::on_air_lamp_size().y, super::ON_AIR_ROW_H);
    }

    #[test]
    fn onair_lamp_ignores_the_mic() {
        use super::onair_lamp_lit;
        assert!(
            !onair_lamp_lit(false, true),
            "mic open is not on air (#225)"
        );
        assert!(!onair_lamp_lit(false, false));
        assert!(onair_lamp_lit(true, false));
        assert!(onair_lamp_lit(true, true));
    }

    #[test]
    fn time_of_day_follows_the_zone() {
        use woa_engine::localtime::Zone;
        // 2026-12-31 22:59:58 UTC ist in Mitteleuropa (Winter) 23:59:58.
        let unix = 1_798_757_998;
        assert_eq!(super::time_of_day(unix, Zone::CENTRAL_EUROPE), "23:59:58");
        assert_eq!(
            super::time_of_day(unix + 2, Zone::CENTRAL_EUROPE),
            "00:00:00"
        );
        assert_eq!(super::time_of_day(unix, Zone::fixed(0)), "22:59:58");
    }

    #[test]
    fn cart_keys_map_to_egui_keys_and_skip_reserved_ones() {
        use eframe::egui::Key;
        assert_eq!(super::cart_key_of("1"), Some(Key::Num1));
        assert_eq!(super::cart_key_of("0"), Some(Key::Num0));
        assert_eq!(super::cart_key_of("F5"), Some(Key::F5));
        assert_eq!(super::cart_key_of("M"), None);
        assert_eq!(super::cart_key_of("Space"), None);
        assert_eq!(super::cart_key_of("gibtsnicht"), None);
        // Jeder Name, den egui liefert, kommt als Taste zurück.
        for k in [Key::Num7, Key::A, Key::F12, Key::Comma] {
            assert_eq!(Key::from_name(k.name()), Some(k));
        }
        assert_eq!(super::cart_number_key(9).as_deref(), Some("0"));
        assert_eq!(super::cart_number_key(10), None);
    }

    #[test]
    fn dragged_row_lands_on_deck_row_or_nowhere() {
        use super::{RowDrop, row_drop_target};
        use eframe::egui::{Rect, pos2, vec2};
        let decks = [
            Rect::from_min_size(pos2(1000.0, 100.0), vec2(400.0, 80.0)),
            Rect::from_min_size(pos2(1000.0, 190.0), vec2(400.0, 80.0)),
            Rect::from_min_size(pos2(1000.0, 280.0), vec2(400.0, 80.0)),
        ];
        let playlist = Rect::from_min_size(pos2(0.0, 400.0), vec2(900.0, 500.0));
        let rows: Vec<Rect> = (0..5)
            .map(|k| Rect::from_min_size(pos2(10.0, 450.0 + 50.0 * k as f32), vec2(880.0, 46.0)))
            .collect();
        let carts = [Rect::from_min_size(pos2(1000.0, 400.0), vec2(90.0, 60.0))];
        let t = |p| row_drop_target(p, 1, &decks, &carts, playlist, &rows);
        assert_eq!(t(pos2(1200.0, 220.0)), RowDrop::Deck(1));
        assert_eq!(t(pos2(1040.0, 430.0)), RowDrop::Cart(0), "#323");
        assert_eq!(t(pos2(500.0, 620.0)), RowDrop::Row(3));
        // Auf sich selbst, in die Lücke oder außerhalb der Playlist: nichts.
        assert_eq!(t(pos2(500.0, 520.0)), RowDrop::Nothing);
        assert_eq!(t(pos2(500.0, 548.0)), RowDrop::Nothing);
        assert_eq!(t(pos2(950.0, 50.0)), RowDrop::Nothing);
        // Zeilen, die aus der Playlist gerollt sind, zählen nicht.
        assert_eq!(
            row_drop_target(pos2(500.0, 620.0), 1, &decks, &carts, Rect::NOTHING, &rows),
            RowDrop::Nothing
        );
    }

    /// Ein Bild mit `f`, Rückgabe: (Beschriftung, Rechteck, deaktiviert) je Bedienelement.
    pub(super) fn frame(
        ctx: &eframe::egui::Context,
        n: u32,
        events: Vec<eframe::egui::Event>,
        mut f: impl FnMut(&mut eframe::egui::Ui),
    ) -> Vec<(String, eframe::egui::Rect, bool)> {
        use eframe::egui;
        let mut input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1200.0, 800.0),
            )),
            time: Some(f64::from(n) * 0.05),
            events,
            ..Default::default()
        };
        if let Some(egui::Event::PointerButton { modifiers, .. }) = input.events.get(1) {
            input.modifiers = *modifiers;
        }
        if n == 0 {
            crate::theme::install_fonts(ctx);
        }
        input.viewports.entry(egui::ViewportId::ROOT).or_default();
        let out = ctx.run_ui(input, |ui| {
            egui::CentralPanel::default().show(ui, |ui| f(ui));
        });
        let mut nodes = Vec::new();
        if let Some(u) = out.platform_output.accesskit_update {
            for (_, node) in u.nodes {
                let label = node.label().map(str::to_string).unwrap_or_default();
                if let Some(b) = node.bounds() {
                    let r = egui::Rect::from_min_max(
                        egui::pos2(b.x0 as f32, b.y0 as f32),
                        egui::pos2(b.x1 as f32, b.y1 as f32),
                    );
                    nodes.push((label, r, node.is_disabled()));
                }
            }
        }
        nodes
    }

    pub(super) fn click_at(p: eframe::egui::Pos2, shift: bool) -> Vec<eframe::egui::Event> {
        use eframe::egui::{Event, Modifiers, PointerButton};
        let modifiers = if shift {
            Modifiers::SHIFT
        } else {
            Modifiers::NONE
        };
        vec![
            Event::PointerMoved(p),
            Event::PointerButton {
                pos: p,
                button: PointerButton::Primary,
                pressed: true,
                modifiers,
            },
            Event::PointerButton {
                pos: p,
                button: PointerButton::Primary,
                pressed: false,
                modifiers,
            },
        ]
    }

    fn find<'a>(
        nodes: &'a [(String, eframe::egui::Rect, bool)],
        text: &str,
    ) -> &'a (String, eframe::egui::Rect, bool) {
        nodes
            .iter()
            .find(|(l, _, _)| l.contains(text))
            .unwrap_or_else(|| panic!("„{text}“ fehlt: {nodes:?}"))
    }

    /// Zeile wie in der Playlist: Griff, Nummer (eigener Klick), Titel, Knopf rechts.
    fn demo_row(ui: &mut eframe::egui::Ui, number_hit: &mut bool) -> eframe::egui::Response {
        use eframe::egui;
        super::row_area(
            ui,
            "demo_row",
            egui::Sense::click_and_drag(),
            egui::Frame::new().inner_margin(5),
            |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label("::");
                    if ui
                        .add(egui::Label::new(" 1").sense(egui::Sense::click()))
                        .clicked()
                    {
                        *number_hit = true;
                    }
                    ui.label("Ein Titel");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label("03:21");
                    });
                });
            },
        )
        .response
    }

    /// Trägheit (#302): 300 Zeilen je Bild mit Klickfläche gegen dieselben Zeilen ohne. Die
    /// absolute Zeit hängt am Rechner (die CI ist langsamer), daher der Vergleich: Die Klickfläche
    /// darf die Zeilen nicht spürbar langsamer machen. Die Zeiten stehen in der Testausgabe.
    #[test]
    fn many_rows_stay_fast() {
        use eframe::egui;
        fn ms(with_area: bool) -> f64 {
            let ctx = egui::Context::default();
            let mut hit = false;
            let mut ui = |ui: &mut egui::Ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for _ in 0..300 {
                        if with_area {
                            demo_row(ui, &mut hit);
                        } else {
                            egui::Frame::new().inner_margin(5).show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.horizontal(|ui| {
                                    ui.label("::");
                                    ui.label(" 1");
                                    ui.label("Ein Titel");
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| ui.label("03:21"),
                                    );
                                });
                            });
                        }
                    }
                });
            };
            frame(&ctx, 0, vec![], &mut ui);
            (1..=3)
                .map(|k| {
                    let t = std::time::Instant::now();
                    for n in 1..=10 {
                        frame(&ctx, k * 10 + n, vec![], &mut ui);
                    }
                    t.elapsed().as_secs_f64() * 100.0
                })
                .fold(f64::MAX, f64::min)
        }
        let plain = ms(false);
        let area = ms(true);
        eprintln!("300 Zeilen: {area:.2} ms je Bild mit Klickfläche, {plain:.2} ms ohne");
        // Auf geteilten CI-Runnern schwanken Wandzeiten stark (Windows 01.11.: rot ohne
        // Codeänderung). Dort fängt die Schranke nur einen groben Rückfall; die enge gilt lokal.
        let (factor, slack) = if std::env::var_os("CI").is_some() {
            (5.0, 20.0)
        } else {
            (2.0, 2.0)
        };
        assert!(
            area < plain * factor + slack,
            "{area:.1} ms gegen {plain:.1} ms"
        );
    }

    #[test]
    fn whole_row_selects_and_opens_the_menu_anywhere() {
        use eframe::egui::{self, Event, Modifiers, PointerButton};
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let rect = std::cell::Cell::new(egui::Rect::NOTHING);
        let mut clicked = false;
        let mut secondary = false;
        let mut number = false;
        let mut n = 0;
        let mut step = |events: Vec<Event>, number: &mut bool| {
            let mut c = false;
            let mut s = false;
            frame(&ctx, n, events, |ui| {
                let r = demo_row(ui, number);
                rect.set(r.rect);
                c |= r.clicked();
                s |= r.secondary_clicked();
            });
            n += 1;
            (c, s)
        };
        step(vec![], &mut number);
        step(vec![], &mut number);
        let rect = rect.get();
        // Rechter Rand der Zeile, neben der Dauer: kein Text, trotzdem Auswahl.
        let edge = egui::pos2(rect.right() - 2.0, rect.center().y);
        let (c, _) = step(click_at(edge, false), &mut number);
        clicked |= c;
        assert!(clicked, "Klick auf den rechten Rand wählt aus: {rect:?}");
        step(vec![], &mut number);
        // Rechtsklick mitten auf den Titel öffnet das Menü der Zeile.
        let mid = egui::pos2(rect.center().x, rect.center().y);
        let right = vec![
            Event::PointerMoved(mid),
            Event::PointerButton {
                pos: mid,
                button: PointerButton::Secondary,
                pressed: true,
                modifiers: Modifiers::NONE,
            },
            Event::PointerButton {
                pos: mid,
                button: PointerButton::Secondary,
                pressed: false,
                modifiers: Modifiers::NONE,
            },
        ];
        let (_, s) = step(right, &mut number);
        secondary |= s;
        assert!(secondary, "Rechtsklick irgendwo öffnet das Menü");
        step(vec![], &mut number);
        // Die Nummer behält ihren eigenen Klick (eintippen), die Zeile wählt dann nicht aus.
        let num = egui::pos2(rect.left() + 22.0, rect.center().y);
        let (c, _) = step(click_at(num, false), &mut number);
        assert!(number, "Nummer bekommt den Klick");
        assert!(!c, "Klick auf die Nummer wählt die Zeile nicht aus");
    }

    #[test]
    fn back_to_playlist_toggle_is_there_and_clicks() {
        let ctx = eframe::egui::Context::default();
        ctx.enable_accesskit();
        let mut clicked = false;
        let mut ui = |ui: &mut eframe::egui::Ui| {
            clicked |= super::deck_return_toggle(ui, 0, false).clicked();
        };
        frame(&ctx, 0, vec![], &mut ui);
        let nodes = frame(&ctx, 1, vec![], &mut ui);
        let p = find(&nodes, "Zurück zur Playlist Deck A").1.center();
        let _ = frame(&ctx, 2, click_at(p, false), &mut ui);
        assert!(clicked);
    }

    #[test]
    fn vertical_faders_are_there_and_set_the_level() {
        let ctx = eframe::egui::Context::default();
        ctx.enable_accesskit();
        let dbs = std::cell::RefCell::new([0.0f32; 4]);
        let changed = std::cell::RefCell::new(Vec::new());
        let mut ui = |ui: &mut eframe::egui::Ui| {
            for (k, r) in super::level_faders(ui, &mut dbs.borrow_mut(), &Default::default())
                .iter()
                .enumerate()
            {
                if r.changed() {
                    changed.borrow_mut().push(k);
                }
            }
        };
        frame(&ctx, 0, vec![], &mut ui);
        let nodes = frame(&ctx, 1, vec![], &mut ui);
        for name in [
            "Pegel Playlist",
            "Pegel Carts",
            "Pegel Decks",
            "Pegel Mikro",
        ] {
            let r = find(&nodes, name).1;
            assert!(
                r.height() > 1.5 * r.width(),
                "{name} steht senkrecht: {r:?}"
            );
        }
        // Klick unten auf den Deck-Fader: Pegel sinkt.
        let r = find(&nodes, "Pegel Decks").1;
        let p = eframe::egui::pos2(r.center().x, r.bottom() - 15.0);
        let _ = frame(&ctx, 2, click_at(p, false), &mut ui);
        assert_eq!(*changed.borrow(), vec![2]);
        assert!(dbs.borrow()[2] < -30.0, "Decks leise: {}", dbs.borrow()[2]);
        assert_eq!(dbs.borrow()[0], 0.0);
        // Doppelklick: zurück auf 0 dB.
        let _ = frame(&ctx, 3, vec![], &mut ui);
        let mut ev = click_at(p, false);
        ev.extend(click_at(p, false));
        let _ = frame(&ctx, 4, ev, &mut ui);
        assert_eq!(dbs.borrow()[2], 0.0, "Doppelklick: 0 dB");
    }

    #[test]
    fn output_faders_have_level_and_mute() {
        let ctx = eframe::egui::Context::default();
        ctx.enable_accesskit();
        let outs = std::cell::RefCell::new([woa_radio::show::OutLevel::default(); 2]);
        let changed = std::cell::RefCell::new(Vec::new());
        let mut ui = |ui: &mut eframe::egui::Ui| {
            let ch = super::output_faders(ui, &mut outs.borrow_mut(), &Default::default());
            changed.borrow_mut().push(ch);
        };
        frame(&ctx, 0, vec![], &mut ui);
        let nodes = frame(&ctx, 1, vec![], &mut ui);
        for name in ["Pegel Wiedergabe Notebook", "Pegel Stream senden"] {
            let r = find(&nodes, name).1;
            assert!(r.height() > 1.5 * r.width(), "{name} senkrecht: {r:?}");
        }
        // Je Ausgang eine LED-Kette neben dem Fader, wie bei den Quellen.
        for (fader, led) in [
            ("Pegel Wiedergabe Notebook", "Pegelanzeige Notebook"),
            ("Pegel Stream senden", "Pegelanzeige Stream"),
        ] {
            let (f, m) = (find(&nodes, fader).1, find(&nodes, led).1);
            assert!(
                m.height() > 5.0 * m.width(),
                "{led} senkrecht und schmal: {m:?}"
            );
            assert!(m.left() >= f.right() - 0.5, "{led} rechts neben dem Fader");
        }
        let p = find(&nodes, "Stumm Stream senden").1.center();
        let _ = frame(&ctx, 2, click_at(p, false), &mut ui);
        assert_eq!(changed.borrow().last(), Some(&[false, true]));
        assert!(outs.borrow()[1].mute, "Stream stumm");
        assert!(!outs.borrow()[0].mute);
        assert_eq!(outs.borrow()[1].db, 0.0, "Pegel bleibt beim Stummschalten");
    }

    #[test]
    fn five_led_meters_stand_next_to_the_faders() {
        let ctx = eframe::egui::Context::default();
        ctx.enable_accesskit();
        let mut dbs = [0.0f32; 4];
        let meters: [super::MeterHold; 5] = Default::default();
        let mut ui = |ui: &mut eframe::egui::Ui| {
            let _ = super::level_faders(ui, &mut dbs, &meters);
        };
        frame(&ctx, 0, vec![], &mut ui);
        let nodes = frame(&ctx, 1, vec![], &mut ui);
        for name in ["Playlist", "Carts", "Decks", "Mikro", "Master"] {
            let r = find(&nodes, &format!("Pegelanzeige {name}")).1;
            assert!(
                r.height() > 5.0 * r.width(),
                "{name} senkrecht und schmal: {r:?}"
            );
        }
    }

    #[test]
    fn level_leds_are_fat() {
        // #287: kräftige LEDs, Fader und Namensschilder bleiben.
        const { assert!(super::METER_W >= 16.0) };
        let (n, gap) = crate::theme::bus_segments(super::FADER_HEIGHT - 2.0);
        let seg_h = (super::FADER_HEIGHT - 2.0 - gap * (n as f32 - 1.0)) / n as f32;
        assert!(seg_h >= 4.5, "LED {seg_h} px hoch");
        const { assert!(36.0 + 2.0 + super::METER_W <= super::STRIP_W) };
    }

    #[test]
    fn led_meter_colour_steps() {
        use crate::theme::{BusZone, bus_zone};
        assert_eq!(bus_zone(-1.0), BusZone::Red);
        assert_eq!(bus_zone(-5.9), BusZone::Red);
        assert_eq!(bus_zone(-6.0), BusZone::Yellow);
        assert_eq!(bus_zone(-12.0), BusZone::Yellow);
        assert_eq!(bus_zone(-18.0), BusZone::Green);
        assert_eq!(bus_zone(-30.0), BusZone::Green);
        assert_eq!(bus_zone(-50.0), BusZone::Blue, "zu leise");
    }

    #[test]
    fn meter_peak_holds_then_falls() {
        let mut m = super::MeterHold::default();
        m.step(0.8, 0.0);
        assert_eq!((m.level, m.hold), (0.8, 0.8));
        m.step(0.0, 1.0);
        assert!(m.level < 0.8, "fällt");
        assert_eq!(m.hold, 0.8, "Spitze hält");
        m.step(0.0, 2.0);
        assert!(m.hold < 0.8, "nach der Haltezeit fällt die Spitze");
    }

    #[test]
    fn running_cart_shows_its_countdown() {
        assert_eq!(crate::widgets::pad::countdown(12.2), "-13 s");
        assert_eq!(crate::widgets::pad::countdown(9.43), "-9,4 s");
        let ctx = eframe::egui::Context::default();
        ctx.enable_accesskit();
        let mut ui = |ui: &mut eframe::egui::Ui| {
            crate::widgets::CartPad {
                number: 3,
                title: Some("Station-ID"),
                key: None,
                capturing: false,
                playing: true,
                color: eframe::egui::Color32::RED,
                remaining: Some(12.0),
                seconds: Some(15.0),
                vocal_in: None,
                actions: 0,
            }
            .ui(ui, eframe::egui::vec2(200.0, 68.0));
        };
        frame(&ctx, 0, vec![], &mut ui);
        let nodes = frame(&ctx, 1, vec![], &mut ui);
        find(&nodes, "Cart 3: Station-ID -12 s");
    }

    #[test]
    fn idle_cart_shows_its_length() {
        let ctx = eframe::egui::Context::default();
        ctx.enable_accesskit();
        let mut ui = |ui: &mut eframe::egui::Ui| {
            crate::widgets::CartPad {
                number: 2,
                title: Some("Wetter"),
                key: None,
                capturing: false,
                playing: false,
                color: eframe::egui::Color32::RED,
                remaining: None,
                seconds: Some(90.0),
                vocal_in: None,
                actions: 0,
            }
            .ui(ui, eframe::egui::vec2(200.0, 68.0));
        };
        frame(&ctx, 0, vec![], &mut ui);
        let nodes = frame(&ctx, 1, vec![], &mut ui);
        find(&nodes, "Cart 2: Wetter 1:30");
    }

    #[test]
    fn clock_format() {
        assert_eq!(super::clock(0.0), "00:00");
        assert_eq!(super::clock(83.4), "01:23");
        assert_eq!(super::clock(3725.0), "1:02:05");
        assert_eq!(super::clock(-3.0), "00:00");
    }
}

/// Zeile unter „Auf Sendung“ zur lokalen Verbindung Oberfläche → woa-streamd und zu
/// Lücken im Sendemix. `feeder`: (verbunden, Neuverbindungen), `None` = nicht auf Sendung.
pub fn link_line(feeder: Option<(bool, u32)>, status: Option<&StreamStatus>) -> Option<String> {
    let (connected, reconnects) = feeder?;
    if !connected {
        return Some("Verbindung zu woa-streamd unterbrochen, verbinde neu …".into());
    }
    let gaps = status.map_or(0, |s| s.live_underruns + s.live_trims);
    match (reconnects, gaps) {
        (0, 0) => None,
        (r, 0) => Some(format!("Verbindung zu woa-streamd {r}× neu aufgebaut")),
        (0, g) => Some(format!("Lücken im Sendestream: {g}")),
        (r, g) => Some(format!(
            "Verbindung zu woa-streamd {r}× neu aufgebaut · Lücken im Sendestream: {g}"
        )),
    }
}

#[cfg(test)]
mod link_line_tests {
    use super::*;

    #[test]
    fn link_line_shows_drop_reconnects_and_gaps() {
        assert_eq!(link_line(None, None), None);
        assert_eq!(link_line(Some((true, 0)), None), None);
        assert!(
            link_line(Some((false, 0)), None)
                .unwrap()
                .contains("verbinde neu")
        );
        assert_eq!(
            link_line(Some((true, 2)), None).as_deref(),
            Some("Verbindung zu woa-streamd 2× neu aufgebaut")
        );
        let s = StreamStatus {
            live_underruns: 3,
            live_trims: 1,
            ..StreamStatus::default()
        };
        assert_eq!(
            link_line(Some((true, 0)), Some(&s)).as_deref(),
            Some("Lücken im Sendestream: 4")
        );
    }
}

/// Tooltip-Text: der Grund, falls bekannt, sonst der allgemeine Hinweis.
/// Vorhör-Knopf an Playlist-Zeile und Cart (#273): seit dem Wunsch des Moderators (05.11.2026)
/// an jeder belegten Zeile und Cart, nicht erst beim Hover. Rechtsklick › Vorhören bleibt
/// zusätzlich. Playlist-Zeilen sind immer belegt.
fn pfl_key_shown(occupied: bool) -> bool {
    occupied
}

/// Öffnet das Kopfhörer-Gerät `id` als eigenen Strom fürs Vorhören (Mixer-Takt `rate`).
fn open_pfl_device(
    id: &str,
    rate: u32,
) -> Result<(woa_radio::pfl::PflHandle, woa_engine::device::ManagedOutput), String> {
    let cfg = woa_engine::device::OutputConfig {
        device_id: Some(id.to_string()),
        buffer_frames: None,
    };
    let prate = woa_engine::device::output_sample_rate(&cfg).unwrap_or(48_000);
    let (h, mut player) = woa_radio::pfl::channel_with_bus(prate, rate);
    let render: woa_engine::device::PairsRender = Box::new(move |bufs, frames| {
        if let Some(b) = bufs.first_mut() {
            player.process(&mut b.l[..frames], &mut b.r[..frames]);
        }
    });
    match woa_engine::device::ManagedOutput::open("pfl", &cfg, 1, false, render, |_| {}) {
        Ok(st) => {
            tracing::info!(device = %id, rate = prate, "pfl on second device");
            Ok((h, st))
        }
        Err(e) => {
            tracing::warn!(error = %e, device = %id, "pfl output failed");
            Err(format!("Vorhören aus: Kopfhörer-Gerät geht nicht ({e})"))
        }
    }
}

/// Läuft ein Deck sichtbar, und wie viel Restzeit zeigt es? Spielt es nicht, wird aber
/// vorgehört (`pfl_pos`: Stelle in der Datei), zählt die Restzeit ab dort (#375).
pub(crate) fn deck_shown(d: &woa_radio::station::DeckView, pfl_pos: Option<f64>) -> (bool, f64) {
    match pfl_pos {
        Some(pos) if !d.playing => (true, (d.cues.start() + d.length - pos).clamp(0.0, d.length)),
        _ => (d.playing, d.remaining),
    }
}

pub fn pfl_hint_for(note: Option<&str>) -> String {
    note.map_or_else(|| NO_PFL.to_string(), str::to_string)
}

#[cfg(test)]
mod pfl_hint_tests {
    use super::*;

    #[test]
    fn preview_key_shows_on_every_occupied_row_and_cart() {
        assert!(pfl_key_shown(true), "ohne Hover sichtbar");
        assert!(!pfl_key_shown(false), "freie Cart ohne Knopf");
    }

    #[test]
    fn hint_shows_open_error() {
        let e = "Vorhören aus: Kopfhörer-Gerät geht nicht (busy)";
        assert_eq!(pfl_hint_for(Some(e)), e);
        assert_eq!(pfl_hint_for(Some(PFL_OFF)), PFL_OFF);
        assert_eq!(pfl_hint_for(None), NO_PFL);
    }
}

#[cfg(test)]
mod deck_drop_tests {
    //! #298: Titel per Ziehen auf ein Deck, im echten Pult-Layout (ohne Audiogerät).
    use super::*;
    use eframe::egui::{self, Event, PointerButton, pos2};

    fn wav(dir: &std::path::Path, name: &str) -> PathBuf {
        let p = dir.join(name);
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: 48_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut w = hound::WavWriter::create(&p, spec).unwrap();
        for _ in 0..48_000 {
            w.write_sample(0i16).unwrap();
            w.write_sample(0i16).unwrap();
        }
        w.finalize().unwrap();
        p
    }

    fn run(ctx: &egui::Context, desk: &mut RadioDesk, n: u32, input: egui::RawInput) {
        let mut input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1600.0, 1000.0),
            )),
            time: Some(f64::from(n) * 0.05),
            ..input
        };
        if n == 0 {
            crate::theme::install_fonts(ctx);
        }
        input.viewports.entry(egui::ViewportId::ROOT).or_default();
        desk.tick(false);
        desk.shortcuts(ctx);
        let _ = ctx.run_ui(input, |ui| {
            egui::CentralPanel::default().show(ui, |ui| desk.ui(ui));
        });
    }

    fn events(ev: Vec<Event>) -> egui::RawInput {
        egui::RawInput {
            events: ev,
            ..Default::default()
        }
    }

    /// Bilder zeichnen, bis `ok` gilt (Steuerung läuft im eigenen Thread).
    fn settle(
        ctx: &egui::Context,
        desk: &mut RadioDesk,
        n: &mut u32,
        ok: impl Fn(&View) -> bool,
    ) -> bool {
        for _ in 0..100 {
            *n += 1;
            run(ctx, desk, *n, egui::RawInput::default());
            if ok(desk.view()) {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        false
    }

    fn deck_title(v: &View, k: usize) -> Option<String> {
        v.decks.get(k).and_then(|d| d.title.clone())
    }

    fn key(k: egui::Key) -> egui::RawInput {
        events(vec![Event::Key {
            key: k,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }])
    }

    /// Tasten A, B, C starten und stoppen Deck A, B, C (Soll-Bild Radio).
    #[test]
    fn letter_keys_start_and_stop_their_deck() {
        let dir = tempfile::tempdir().unwrap();
        let song = wav(dir.path(), "deck.wav");
        let ctx = egui::Context::default();
        let mut desk = RadioDesk::headless(dir.path());
        let mut n = 0;
        run(&ctx, &mut desk, n, egui::RawInput::default());
        desk.deck_load_file(1, song);
        assert!(settle(&ctx, &mut desk, &mut n, |v| deck_title(v, 1).is_some()));
        n += 1;
        run(&ctx, &mut desk, n, key(egui::Key::B));
        assert!(
            settle(&ctx, &mut desk, &mut n, |v| v.decks[1].playing),
            "Taste B startet Deck B"
        );
        assert!(!desk.view().decks[0].playing, "Deck A bleibt still");
        n += 1;
        run(&ctx, &mut desk, n, key(egui::Key::B));
        assert!(
            settle(&ctx, &mut desk, &mut n, |v| !v.decks[1].playing),
            "Taste B stoppt Deck B"
        );
    }

    /// MIDI-Learn an den Decks (André 02.11.2026): Start/Stopp vom Controller wie die Taste,
    /// die LED folgt dem Deck; der Rechtsklick „MIDI lernen“ steht an allen drei Tasten.
    #[test]
    fn controller_starts_and_stops_a_deck() {
        let dir = tempfile::tempdir().unwrap();
        let song = wav(dir.path(), "deck.wav");
        let ctx = egui::Context::default();
        let mut desk = RadioDesk::headless(dir.path());
        let mut n = 0;
        run(&ctx, &mut desk, n, egui::RawInput::default());
        desk.deck_load_file(2, song);
        assert!(settle(&ctx, &mut desk, &mut n, |v| deck_title(v, 2).is_some()));
        desk.press(Target::Deck(2, DeckAction::Start));
        assert!(settle(&ctx, &mut desk, &mut n, |v| v.decks[2].playing));
        assert!(desk.lit(&Target::Deck(2, DeckAction::Start)));
        desk.press(Target::Deck(2, DeckAction::Start));
        assert!(settle(&ctx, &mut desk, &mut n, |v| !v.decks[2].playing));
        desk.press(Target::Deck(9, DeckAction::Start));
        let src = include_str!("radio.rs");
        for t in [
            "Target::Deck(k, DeckAction::Start)",
            "Target::Deck(k, DeckAction::Cue)",
            "Target::Deck(k, DeckAction::Pfl)",
            "Target::Deck(k, DeckAction::Fade)",
        ] {
            assert!(
                src.contains(&format!("learn_menu(&r, {t})")),
                "{t} ohne MIDI lernen"
            );
        }
    }

    /// Alte Belegung eines Carts auf A, B oder C: bleibt in der Datei, wird gemeldet.
    #[test]
    fn old_cart_keys_on_deck_letters_are_reported() {
        let keys = vec![
            Some("1".to_string()),
            Some("B".to_string()),
            None,
            Some("F5".into()),
        ];
        assert_eq!(deck_key_conflicts(&keys), vec![(1, "B".to_string())]);
        let msg = deck_key_note(&deck_key_conflicts(&keys)).unwrap();
        assert!(msg.contains("Cart 2") && msg.contains("Taste B"), "{msg}");
        assert_eq!(deck_key_note(&[]), None);
    }

    /// Hat ein Textfeld den Fokus, ist B ein Buchstabe und startet kein Deck.
    #[test]
    fn letter_key_in_a_text_field_starts_no_deck() {
        let dir = tempfile::tempdir().unwrap();
        let song = wav(dir.path(), "deck.wav");
        let ctx = egui::Context::default();
        let mut desk = RadioDesk::headless(dir.path());
        let mut n = 0;
        run(&ctx, &mut desk, n, egui::RawInput::default());
        desk.deck_load_file(1, song);
        assert!(settle(&ctx, &mut desk, &mut n, |v| deck_title(v, 1).is_some()));
        let id = egui::Id::new("probe_text");
        let mut text = String::new();
        for (k, input) in [
            egui::RawInput::default(),
            egui::RawInput::default(),
            key(egui::Key::B),
            egui::RawInput::default(),
        ]
        .into_iter()
        .enumerate()
        {
            n += 1;
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1600.0, 1000.0),
                )),
                time: Some(f64::from(n) * 0.05),
                ..input
            };
            desk.tick(false);
            desk.shortcuts(&ctx);
            let _ = ctx.run_ui(input, |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    let r = ui.add(egui::TextEdit::singleline(&mut text).id(id));
                    if k == 0 {
                        r.request_focus();
                    }
                    desk.ui(ui);
                });
            });
        }
        assert!(
            !settle(&ctx, &mut desk, &mut n, |v| v.decks[1].playing),
            "Deck B darf nicht starten"
        );
    }

    /// A, B, C sind für die Decks reserviert und taugen nicht als Cart-Taste.
    #[test]
    fn deck_letters_are_no_cart_keys() {
        for k in ["A", "B", "C"] {
            assert_eq!(cart_key_of(k), None, "{k}");
        }
        assert_eq!(cart_key_of("D"), Some(egui::Key::D));
    }

    #[test]
    fn library_title_dropped_on_a_deck_lands_there() {
        let dir = tempfile::tempdir().unwrap();
        let song = wav(dir.path(), "bibliothek.wav");
        let ctx = egui::Context::default();
        let mut desk = RadioDesk::headless(dir.path());
        let mut n = 0;
        run(&ctx, &mut desk, n, egui::RawInput::default());
        settle(&ctx, &mut desk, &mut n, |_| true);
        let deck = desk.deck_rects[1];
        assert!(
            deck.width() > 0.0 && deck.height() > 0.0,
            "Deck B gezeichnet: {deck:?}"
        );
        assert!(
            desk.drop_library_path(song, deck.center()),
            "Ablegen angenommen"
        );
        assert!(
            settle(&ctx, &mut desk, &mut n, |v| deck_title(v, 1).is_some()),
            "Titel liegt auf Deck B: {:?} {:?} {:?}",
            desk.view().messages,
            desk.error,
            desk.view().decks
        );
    }

    #[test]
    fn explorer_file_dropped_on_a_deck_lands_there() {
        let dir = tempfile::tempdir().unwrap();
        let song = wav(dir.path(), "explorer.wav");
        let ctx = egui::Context::default();
        let mut desk = RadioDesk::headless(dir.path());
        let mut n = 0;
        run(&ctx, &mut desk, n, egui::RawInput::default());
        settle(&ctx, &mut desk, &mut n, |_| true);
        let p = desk.deck_rects[0].center();
        n += 1;
        run(&ctx, &mut desk, n, events(vec![Event::PointerMoved(p)]));
        n += 1;
        let input = egui::RawInput {
            events: vec![Event::PointerMoved(p)],
            dropped_files: vec![egui::DroppedFile {
                path: Some(song),
                ..Default::default()
            }],
            ..Default::default()
        };
        run(&ctx, &mut desk, n, input);
        assert!(
            settle(&ctx, &mut desk, &mut n, |v| deck_title(v, 0).is_some()),
            "Titel liegt auf Deck A"
        );
    }

    /// #322: Nach dem Klick auf die Nummer kommt man mit Esc, Tab oder einem Klick daneben
    /// wieder aus dem Feld; Enter verschiebt.
    #[test]
    fn playlist_number_field_can_be_left() {
        let dir = tempfile::tempdir().unwrap();
        let a = wav(dir.path(), "a.wav");
        let b = wav(dir.path(), "b.wav");
        let ctx = egui::Context::default();
        let mut desk = RadioDesk::headless(dir.path());
        let mut n = 0;
        run(&ctx, &mut desk, n, egui::RawInput::default());
        if let Some(s) = &desk.station {
            s.add_files(vec![a, b.clone()]);
        }
        assert!(settle(&ctx, &mut desk, &mut n, |v| v.playlist.len() == 2));
        let key = |k| Event::Key {
            key: k,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        for leave in [
            vec![key(egui::Key::Escape)],
            vec![key(egui::Key::Tab)],
            vec![
                Event::PointerMoved(pos2(5.0, 5.0)),
                Event::PointerButton {
                    pos: pos2(5.0, 5.0),
                    button: PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
                Event::PointerButton {
                    pos: pos2(5.0, 5.0),
                    button: PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        ] {
            desk.pos_edit = Some((0, "1".into(), false));
            for _ in 0..3 {
                n += 1;
                run(&ctx, &mut desk, n, egui::RawInput::default());
            }
            assert!(desk.pos_edit.is_some(), "Feld offen");
            n += 1;
            run(&ctx, &mut desk, n, events(leave.clone()));
            for _ in 0..2 {
                n += 1;
                run(&ctx, &mut desk, n, egui::RawInput::default());
            }
            assert!(desk.pos_edit.is_none(), "Feld bleibt nach {leave:?}");
        }
        // Enter verschiebt: Titel 1 an Stelle 2.
        desk.pos_edit = Some((0, "2".into(), false));
        for _ in 0..3 {
            n += 1;
            run(&ctx, &mut desk, n, egui::RawInput::default());
        }
        n += 1;
        run(&ctx, &mut desk, n, events(vec![key(egui::Key::Enter)]));
        assert!(desk.pos_edit.is_none());
        assert!(settle(&ctx, &mut desk, &mut n, |v| v.playlist[0].path == b));
    }

    /// #360: „→ Playlist“ aus der Bibliothek (Party) hängt hinten an, auch wenn nichts läuft
    /// (vorher landete der Titel als Nächster ganz oben).
    #[test]
    fn library_title_is_appended_at_the_end() {
        let dir = tempfile::tempdir().unwrap();
        let a = wav(dir.path(), "a.wav");
        let b = wav(dir.path(), "b.wav");
        let c = wav(dir.path(), "c.wav");
        let ctx = egui::Context::default();
        let mut desk = RadioDesk::headless(dir.path());
        let mut n = 0;
        run(&ctx, &mut desk, n, egui::RawInput::default());
        if let Some(s) = &desk.station {
            s.add_files(vec![a, b]);
        }
        assert!(settle(&ctx, &mut desk, &mut n, |v| v.playlist.len() == 2));
        desk.party_insert(c.clone(), crate::party::library_add_at());
        assert!(settle(&ctx, &mut desk, &mut n, |v| v.playlist.len() == 3));
        assert_eq!(desk.view().playlist[2].path, c, "hinten angehängt");
    }

    /// #323: Datei aus dem Explorer auf ein Cart-Pad belegt das Pad (ging in die Playlist).
    #[test]
    fn explorer_file_dropped_on_a_cart_lands_there() {
        let dir = tempfile::tempdir().unwrap();
        let song = wav(dir.path(), "cart.wav");
        let ctx = egui::Context::default();
        let mut desk = RadioDesk::headless(dir.path());
        let mut n = 0;
        run(&ctx, &mut desk, n, egui::RawInput::default());
        settle(&ctx, &mut desk, &mut n, |_| true);
        let pad = *desk.cart_rects.first().expect("Cart-Pads gezeichnet");
        assert!(pad.width() > 0.0, "Pad 1 sichtbar: {pad:?}");
        let p = pad.center();
        n += 1;
        run(&ctx, &mut desk, n, events(vec![Event::PointerMoved(p)]));
        n += 1;
        let input = egui::RawInput {
            events: vec![Event::PointerMoved(p)],
            dropped_files: vec![egui::DroppedFile {
                path: Some(song),
                ..Default::default()
            }],
            ..Default::default()
        };
        run(&ctx, &mut desk, n, input);
        assert!(
            settle(&ctx, &mut desk, &mut n, |v| v
                .carts
                .first()
                .is_some_and(Option::is_some)),
            "Pad 1 belegt: {:?}",
            desk.view().carts.first()
        );
        assert!(desk.view().playlist.is_empty(), "nicht in die Playlist");
    }

    #[test]
    fn playlist_row_dragged_by_its_grip_lands_on_a_deck() {
        drag_row_to_deck_c(8.0);
    }

    #[test]
    fn playlist_row_dragged_by_its_title_lands_on_a_deck() {
        // Hinter Griff, Nummer und Cover beginnt der Titeltext.
        drag_row_to_deck_c(110.0);
    }

    fn drag_row_to_deck_c(x: f32) {
        let dir = tempfile::tempdir().unwrap();
        let song = wav(dir.path(), "playlist.wav");
        let ctx = egui::Context::default();
        let mut desk = RadioDesk::headless(dir.path());
        let mut n = 0;
        run(&ctx, &mut desk, n, egui::RawInput::default());
        if let Some(s) = &desk.station {
            s.add_files(vec![song]);
        }
        assert!(settle(&ctx, &mut desk, &mut n, |v| v.playlist.len() == 1));
        settle(&ctx, &mut desk, &mut n, |_| true);
        let row = desk.row_rects[0];
        let deck = desk.deck_rects[2].center();
        // Bei `x` packen, dann schrittweise aufs Deck C.
        let from = pos2(row.left() + x, row.center().y);
        n += 1;
        run(&ctx, &mut desk, n, events(vec![Event::PointerMoved(from)]));
        n += 1;
        run(
            &ctx,
            &mut desk,
            n,
            events(vec![Event::PointerButton {
                pos: from,
                button: PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            }]),
        );
        for k in 1..=10 {
            let t = k as f32 / 10.0;
            n += 1;
            run(
                &ctx,
                &mut desk,
                n,
                events(vec![Event::PointerMoved(from + (deck - from) * t)]),
            );
        }
        assert_eq!(desk.row_drag, Some(0), "Ziehen hat begonnen, Zeile {row:?}");
        n += 1;
        run(
            &ctx,
            &mut desk,
            n,
            events(vec![Event::PointerButton {
                pos: deck,
                button: PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }]),
        );
        assert!(
            settle(&ctx, &mut desk, &mut n, |v| deck_title(v, 2).is_some()),
            "Titel liegt auf Deck C"
        );
    }
}

#[cfg(test)]
mod design3_tests {
    //! Design-Runde 3, Radio: Fehlerbanner, Tastenreihe, Decks, Pegel, Processing, Höhe.
    use super::*;
    use eframe::egui;
    use egui_kittest::kittest::NodeT as _;

    fn harness(size: egui::Vec2, desk: RadioDesk) -> egui_kittest::Harness<'static, RadioDesk> {
        egui_kittest::Harness::builder()
            .with_size(size)
            .build_ui_state(
                |ui, d: &mut RadioDesk| {
                    let ctx = ui.ctx().clone();
                    let fresh = ctx.data_mut(|m| {
                        let id = egui::Id::new("test_fonts");
                        let fresh = m.get_temp::<bool>(id).is_none();
                        m.insert_temp(id, true);
                        fresh
                    });
                    if fresh {
                        crate::theme::install_fonts(&ctx);
                        crate::theme::apply(
                            &ctx,
                            crate::theme::Theme::Dunkel,
                            crate::theme::Mode::Radio,
                        );
                        return;
                    }
                    egui::CentralPanel::default()
                        .frame(egui::Frame::new().inner_margin(egui::Margin::symmetric(14, 12)))
                        .show(ui, |ui| d.ui(ui));
                },
                desk,
            )
    }

    fn labels(h: &egui_kittest::Harness<'_, RadioDesk>) -> Vec<String> {
        use egui_kittest::kittest::Queryable;
        h.query_all(egui_kittest::kittest::by())
            .filter_map(|n| n.accesskit_node().label())
            .collect()
    }

    #[test]
    fn seg_ghost_has_an_eight_per_digit() {
        assert_eq!(seg_ghost("03:42"), "88:88");
        assert_eq!(seg_ghost("123:05"), "888:88");
        assert_eq!(seg_ghost("--:--"), "88:88");
    }

    /// Ein langer, mehrzeiliger Fehler bleibt eine Zeile; der Rest steht hinter „Details“.
    #[test]
    fn error_banner_is_one_line() {
        let dir = tempfile::tempdir().unwrap();
        let mut d = RadioDesk::headless(dir.path());
        d.error = Some(format!(
            "Ausgabe nicht offen.\n{}",
            "sehr langer Grund ".repeat(40)
        ));
        let mut h = harness(egui::vec2(1366.0, 728.0), d);
        h.run_steps(3);
        let r = h.state().error_rect;
        assert!(r.height() <= 36.0, "Banner {r:?} ist mehr als eine Zeile");
        assert!(labels(&h).iter().any(|l| l == "Details"), "Details fehlt");
    }

    /// Die Sende-Tastenreihe hat keine Deck-Tasten mehr; −10/+10 sind Symbole mit Tooltip.
    #[test]
    fn send_row_without_deck_keys_and_seek_symbols() {
        let dir = tempfile::tempdir().unwrap();
        let mut h = harness(egui::vec2(1920.0, 990.0), RadioDesk::headless(dir.path()));
        h.run_steps(3);
        let l = labels(&h);
        assert!(
            !l.iter()
                .any(|l| *l == format!("{} Deck A", theme::icf::PLAY) || l == "Deck A"),
            "Deck-Taste noch da: {l:?}"
        );
        assert!(
            !l.iter().any(|l| l == "−10" || l == "+10"),
            "−10/+10 als Text: {l:?}"
        );
        for want in ["10 s zurück", "10 s vor"] {
            assert!(l.iter().any(|x| x == want), "{want} fehlt: {l:?}");
        }
    }

    /// Ablösen der Playlist durch ein Deck bleibt erreichbar: im Deck-Menü, mit und ohne Blende.
    #[test]
    fn deck_take_over_menu_offers_fade_and_hard() {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let hit = std::cell::Cell::new(None);
        let mut ui = |ui: &mut egui::Ui| {
            if let Some(f) = deck_take_menu(ui, 0, 3.0) {
                hit.set(Some(f));
            }
        };
        let _ = super::tests::frame(&ctx, 0, vec![], &mut ui);
        let nodes = super::tests::frame(&ctx, 1, vec![], &mut ui);
        let p = |t: &str| {
            nodes
                .iter()
                .find(|(l, _, _)| l.contains(t))
                .unwrap_or_else(|| panic!("{t} fehlt: {nodes:?}"))
                .1
                .center()
        };
        let (a, b) = (p("mit Blende"), p("sofort"));
        let _ = super::tests::frame(&ctx, 2, super::tests::click_at(a, false), &mut ui);
        assert_eq!(hit.take(), Some(true));
        let _ = super::tests::frame(&ctx, 3, vec![], &mut ui);
        let _ = super::tests::frame(&ctx, 4, super::tests::click_at(b, false), &mut ui);
        assert_eq!(hit.take(), Some(false));
    }

    /// Deck-Tasten: Start mit Text, Cue, Vorhören und Zurück zur Playlist als Symbol-Tasten.
    #[test]
    fn deck_keys_are_start_text_and_symbols() {
        let dir = tempfile::tempdir().unwrap();
        let mut h = harness(egui::vec2(1920.0, 990.0), RadioDesk::headless(dir.path()));
        h.run_steps(3);
        let l = labels(&h);
        for k in ["A", "B", "C"] {
            for want in [
                format!("Start Deck {k}"),
                format!("Cue Deck {k}"),
                format!("Vorhören Deck {k}"),
                format!("Zurück zur Playlist Deck {k}"),
            ] {
                assert!(l.contains(&want), "{want} fehlt: {l:?}");
            }
        }
        assert!(
            !l.iter().any(|x| x == "↩ Playlist"),
            "Text-Schalter noch da"
        );
    }

    /// Processing als Zeilen mit Zahlenfeld oder Schalter, keine Slider und Häkchen.
    #[test]
    fn processing_rows_use_fields_and_toggles() {
        let dir = tempfile::tempdir().unwrap();
        let mut h = harness(egui::vec2(1920.0, 990.0), RadioDesk::headless(dir.path()));
        h.run_steps(3);
        use egui_kittest::kittest::Queryable;
        let kinds: Vec<(String, egui::accesskit::Role)> = h
            .query_all(egui_kittest::kittest::by())
            .filter_map(|n| Some((n.accesskit_node().label()?, n.accesskit_node().role())))
            .collect();
        for name in [
            "Crossfade-Blende",
            "Musik-Ducking",
            "Ducking auch bei Carts",
            "Stopp blendet aus",
            "Ausblenddauer",
            "Fade-Taste",
            "Sendebus-Processing",
            "Mikro-Kette",
        ] {
            let roles: Vec<_> = kinds
                .iter()
                .filter(|(l, _)| l == name)
                .map(|k| k.1)
                .collect();
            assert!(!roles.is_empty(), "{name} fehlt: {kinds:?}");
            assert!(
                !roles.iter().any(|r| matches!(
                    r,
                    egui::accesskit::Role::Slider | egui::accesskit::Role::CheckBox
                )),
                "{name} ist noch {roles:?}"
            );
        }
    }

    /// Bei 1920 × 1030 liegt alles im Fenster, auch die Wunschtitel-Zeile, ohne Rollen.
    #[test]
    fn everything_fits_1920x1030() {
        let dir = tempfile::tempdir().unwrap();
        let mut d = RadioDesk::headless(dir.path());
        d.view.cart_sets = (1..=5).map(|i| format!("Satz {i}")).collect();
        let mut h = harness(egui::vec2(1920.0, 1030.0 - 40.0), d);
        h.run_steps(3);
        let w = h.state().wish_rect;
        assert!(w.is_positive(), "Wunschtitel nicht gezeichnet");
        assert!(
            w.bottom() <= 990.0,
            "Wunschtitel bei {w:?} unter dem Fensterrand"
        );
    }
}

/// #380: Einstellungen des Sendepults überleben Schließen und Neu-Öffnen.
#[cfg(test)]
mod save_380 {
    use super::*;

    /// Schalter umlegen und das Pult sofort verwerfen (Schließen, Vorhören neu öffnen):
    /// der Stand steht trotzdem in radio.json. Vorher gingen die letzten 0,7 s verloren.
    #[test]
    fn change_survives_immediate_drop() {
        let dir = tempfile::tempdir().unwrap();
        let desk = RadioDesk::headless(dir.path());
        desk.with(|c| c.set_stop_fade(true, 0.5));
        desk.with(|c| c.set_cart_duck(false));
        drop(desk);
        let s = RadioShow::load(&dir.path().join("radio.json")).unwrap();
        assert!(s.stop_fade, "Stopp blendet aus nicht gespeichert");
        assert!((s.stop_fade_seconds - 0.5).abs() < 1e-9);
        assert!(!s.cart_duck, "Ducking bei Carts nicht gespeichert");
    }

    /// Nur die Uhrzeit der Datei ändert sich (Windows, Virenscanner), der Inhalt ist unserer:
    /// das ist keine fremde Änderung, die nächste Einstellung wird gespeichert, nicht verworfen.
    #[test]
    fn touched_file_with_our_content_is_not_foreign() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("radio.json");
        let mut desk = RadioDesk::headless(dir.path());
        desk.with(|c| c.set_crossfade(4.0));
        desk.flush();
        let text = std::fs::read_to_string(&path).unwrap();
        std::thread::sleep(Duration::from_millis(30));
        std::fs::write(&path, text).unwrap();
        desk.known_mtime = Some(SystemTime::UNIX_EPOCH);
        assert!(!desk.foreign_change());
        // Eine echte fremde Änderung bleibt fremd.
        let mut other = RadioShow::load(&path).unwrap();
        other.crossfade_seconds = 9.0;
        other.save(&path).unwrap();
        desk.known_mtime = Some(SystemTime::UNIX_EPOCH);
        assert!(desk.foreign_change());
    }

    /// Fehler beim Schreiben bleiben sichtbar und werden nicht als gespeichert gezählt.
    #[test]
    fn save_error_is_reported() {
        let dir = tempfile::tempdir().unwrap();
        let mut desk = RadioDesk::headless(dir.path());
        desk.radio_path = dir.path().join("radio.txt");
        desk.with(|c| c.set_duck(-20.0));
        desk.flush();
        assert!(
            desk.save_error()
                .is_some_and(|e| e.contains("nicht gespeichert"))
        );
        desk.radio_path = dir.path().join("radio.json");
    }
}
