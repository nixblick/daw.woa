//! Analyse-Fenster (Paket 19): Lautheit (momentan, kurzzeitig, integriert), True Peak, Spektrum,
//! Spektrogramm, Stereo-Scope und Korrelation für den Master oder eine einzelne Spur.
//!
//! Die Samples kommen aus einem Abgriff im Graphen über eine lock-freie Queue
//! (`woa_engine::tap`). Gerechnet wird hier im Oberflächen-Thread, nie im Audio-Thread.

use std::collections::VecDeque;

use eframe::egui;
use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, vec2};
use woa_dsp::analysis::loudness::{LoudnessMeter, TruePeak};
use woa_dsp::analysis::spectrogram::{MIN_DB, colour};
use woa_dsp::analysis::spectrum::{
    DISPLAY_MAX_HZ, DISPLAY_MIN_HZ, FLOOR_DB, SpectrumAnalyzer, log_bands,
};
use woa_dsp::analysis::stereo::{StereoMeter, scope_point};
use woa_model::TrackId;

use crate::theme;

/// Fenstergröße der FFT.
const FFT_SIZE: usize = 4096;
/// Spektren je Sekunde.
const HOPS_PER_SECOND: usize = 30;
/// Anzeigebänder des Spektrums.
const BARS: usize = 160;
/// Zeilen (Frequenzauflösung) und Spalten (Zeit) des Spektrogramms.
const SPEC_ROWS: usize = 128;
const SPEC_COLS: usize = 480;
/// Punkte im Scope.
const SCOPE_POINTS: usize = 1024;
/// Abfall der Balken je Spektrum in dB und der Spitzenmarken.
const BAR_FALL_DB: f32 = 1.2;
const HOLD_FALL_DB: f32 = 0.35;
/// Gedächtnis der laufenden Korrelation in Sekunden.
const STEREO_SECONDS: f64 = 0.3;

/// Zustand und Rechnung des Analyse-Fensters (ohne Zeichnen, daher testbar).
pub struct AnalysisView {
    source: Option<TrackId>,
    rate: u32,
    meter: LoudnessMeter,
    true_peak: [TruePeak; 2],
    stereo: StereoMeter,
    analyzer: SpectrumAnalyzer,
    mono: Vec<f32>,
    since_hop: usize,
    bars: Vec<f32>,
    hold: Vec<f32>,
    scope: VecDeque<(f32, f32)>,
    columns: VecDeque<Vec<f32>>,
    lost: u64,
    texture: Option<egui::TextureHandle>,
    texture_dirty: bool,
}

impl AnalysisView {
    /// Neu für den Master bei `rate` Hz.
    pub fn new(rate: u32) -> Self {
        Self {
            source: None,
            rate,
            meter: LoudnessMeter::new(rate),
            true_peak: [TruePeak::new(), TruePeak::new()],
            stereo: StereoMeter::new(STEREO_SECONDS * f64::from(rate)),
            analyzer: SpectrumAnalyzer::new(FFT_SIZE),
            mono: Vec::with_capacity(FFT_SIZE * 2),
            since_hop: 0,
            bars: vec![FLOOR_DB; BARS],
            hold: vec![FLOOR_DB; BARS],
            scope: VecDeque::with_capacity(SCOPE_POINTS + 1),
            columns: VecDeque::with_capacity(SPEC_COLS + 1),
            lost: 0,
            texture: None,
            texture_dirty: true,
        }
    }

    /// Master (`None`) oder Spur, die gemessen wird.
    pub fn source(&self) -> Option<TrackId> {
        self.source
    }

    /// Wechselt die Quelle und beginnt die Messung von vorn.
    pub fn set_source(&mut self, source: Option<TrackId>) {
        if self.source != source {
            self.source = source;
            self.reset();
        }
    }

    /// Passt die Abtastrate an (Gerätewechsel); bei Änderung von vorn.
    pub fn set_rate(&mut self, rate: u32) {
        if rate != self.rate && rate > 0 {
            self.rate = rate;
            self.reset();
        }
    }

    /// Fängt neu an: Lautheit, integrierter Wert, Spitzen, Halte-Marken, Spektrogramm.
    pub fn reset(&mut self) {
        let source = self.source;
        let texture = self.texture.take();
        *self = Self::new(self.rate);
        self.source = source;
        self.texture = texture;
    }

    /// Verloren gegangene Frames (Ring voll) merken.
    pub fn add_lost(&mut self, frames: u64) {
        self.lost += frames;
    }

    /// Nimmt Stereo-Samples auf und rechnet alle Anzeigen weiter.
    pub fn feed(&mut self, left: &[f32], right: &[f32]) {
        let n = left.len().min(right.len());
        if n == 0 {
            return;
        }
        let (left, right) = (&left[..n], &right[..n]);
        self.meter.push(left, right);
        self.true_peak[0].push(left);
        self.true_peak[1].push(right);
        self.stereo.push(left, right);
        let from = n.saturating_sub(SCOPE_POINTS);
        for (&l, &r) in left[from..].iter().zip(&right[from..]) {
            if self.scope.len() == SCOPE_POINTS {
                self.scope.pop_front();
            }
            self.scope.push_back(scope_point(l, r));
        }
        self.mono
            .extend(left.iter().zip(right).map(|(l, r)| (l + r) * 0.5));
        if self.mono.len() > FFT_SIZE * 2 {
            let cut = self.mono.len() - FFT_SIZE;
            self.mono.drain(..cut);
        }
        self.since_hop += n;
        let hop = (self.rate as usize / HOPS_PER_SECOND).max(1);
        if self.since_hop >= hop {
            // Bei einem großen Stück (Fenster lag verdeckt) reicht ein Spektrum.
            self.since_hop %= hop;
            self.update_spectrum();
        }
    }

    fn update_spectrum(&mut self) {
        let db = self.analyzer.analyze(&self.mono);
        let bars = log_bands(db, FFT_SIZE, self.rate, BARS);
        for ((b, h), new) in self.bars.iter_mut().zip(&mut self.hold).zip(&bars) {
            *b = new.max(*b - BAR_FALL_DB);
            *h = b.max(*h - HOLD_FALL_DB);
        }
        let col = log_bands(db, FFT_SIZE, self.rate, SPEC_ROWS);
        if self.columns.len() == SPEC_COLS {
            self.columns.pop_front();
        }
        self.columns.push_back(col);
        self.texture_dirty = true;
    }

    /// Kommt nichts mehr an (Wiedergabe steht): Anzeigen laufen aus.
    pub fn idle(&mut self) {
        for (b, h) in self.bars.iter_mut().zip(&mut self.hold) {
            *b = (*b - 3.0).max(FLOOR_DB);
            *h = h.max(*b);
        }
        for _ in 0..48 {
            self.scope.pop_front();
        }
    }

    /// Momentane Lautheit.
    pub fn momentary(&self) -> Option<f64> {
        self.meter.momentary()
    }

    /// Kurzzeit-Lautheit.
    pub fn short_term(&self) -> Option<f64> {
        self.meter.short_term()
    }

    /// Integrierte Lautheit.
    pub fn integrated(&self) -> Option<f64> {
        self.meter.integrated()
    }

    /// True Peak in dBTP seit dem Zurücksetzen.
    pub fn true_peak_dbtp(&self) -> Option<f64> {
        let p = self.true_peak[0].peak().max(self.true_peak[1].peak());
        woa_dsp::analysis::amp_to_db(p)
    }

    /// Korrelation der letzten Sekunden.
    pub fn correlation(&self) -> Option<f64> {
        self.stereo.correlation()
    }

    /// Geglättete Spektrum-Balken in dBFS.
    pub fn bars(&self) -> &[f32] {
        &self.bars
    }

    /// Scope-Punkte, älteste zuerst.
    pub fn scope(&self) -> impl Iterator<Item = (f32, f32)> + '_ {
        self.scope.iter().copied()
    }

    fn spectrogram_image(&self) -> egui::ColorImage {
        let mut rgb = vec![0u8; SPEC_COLS * SPEC_ROWS * 3];
        let offset = SPEC_COLS - self.columns.len();
        for (x, col) in self.columns.iter().enumerate() {
            for (y, &db) in col.iter().enumerate() {
                // Zeile 0 oben = höchste Frequenz.
                let row = SPEC_ROWS - 1 - y;
                let o = (row * SPEC_COLS + offset + x) * 3;
                rgb[o..o + 3].copy_from_slice(&colour(db));
            }
        }
        egui::ColorImage::from_rgb([SPEC_COLS, SPEC_ROWS], &rgb)
    }
}

/// Zahl für die Anzeige: eine Nachkommastelle, `--.-` ohne Messwert.
pub fn fmt_level(v: Option<f64>) -> String {
    match v {
        Some(v) if v.is_finite() => format!("{v:.1}"),
        _ => "--.-".to_string(),
    }
}

fn readout(ui: &mut egui::Ui, label: &str, value: String, unit: &str, color: Color32) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        ui.label(egui::RichText::new(label).size(10.0).color(theme::DIM));
        ui.label(
            egui::RichText::new(value)
                .size(20.0)
                .monospace()
                .strong()
                .color(color),
        );
        ui.label(egui::RichText::new(unit).size(10.0).color(theme::DIM));
    });
}

fn x_of(freq: f32, width: f32, rate: u32) -> f32 {
    let top = DISPLAY_MAX_HZ.min(rate as f32 / 2.0);
    ((freq / DISPLAY_MIN_HZ).ln() / (top / DISPLAY_MIN_HZ).ln()).clamp(0.0, 1.0) * width
}

impl AnalysisView {
    /// Zeichnet das Fenster-Innere. `tracks` füllt die Quellenwahl; `playing` zeigt, ob
    /// gerade etwas läuft.
    pub fn ui(&mut self, ui: &mut egui::Ui, tracks: &[(TrackId, String)], playing: bool) {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("QUELLE").size(10.0).color(theme::DIM));
            let name = self
                .source
                .and_then(|id| tracks.iter().find(|t| t.0 == id))
                .map_or_else(|| "Master".to_string(), |t| t.1.clone());
            let mut pick = self.source;
            egui::ComboBox::from_id_salt("analysis_source")
                .selected_text(name)
                .width(150.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut pick, None, "Master");
                    for (id, n) in tracks {
                        ui.selectable_value(&mut pick, Some(*id), n);
                    }
                });
            self.set_source(pick);
            if ui
                .add(egui::Button::new("Zurücksetzen").fill(theme::SURFACE))
                .on_hover_text("Integrierte Lautheit, Spitzen und Halte-Marken neu beginnen")
                .clicked()
            {
                self.reset();
            }
            if !playing {
                ui.label(
                    egui::RichText::new(
                        "Wiedergabe starten (Leertaste), dann laufen die Anzeigen.",
                    )
                    .small()
                    .color(theme::DIM),
                );
            }
            if self.lost > 0 {
                ui.label(
                    egui::RichText::new(format!("{} Frames verpasst", self.lost))
                        .small()
                        .color(theme::ON_AIR),
                );
            }
        });
        ui.add_space(4.0);
        self.readouts(ui);
        ui.add_space(6.0);
        let avail = ui.available_size();
        let side = 230.0f32.min(avail.x * 0.4);
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                let w = (avail.x - side - 14.0).max(120.0);
                let h = ((avail.y - 8.0) * 0.5).max(70.0);
                self.draw_spectrum(ui, vec2(w, h));
                ui.add_space(6.0);
                self.draw_spectrogram(ui, vec2(w, h));
            });
            ui.vertical(|ui| {
                self.draw_scope(ui, side);
                ui.add_space(8.0);
                self.draw_correlation(ui, side);
            });
        });
    }

    fn readouts(&self, ui: &mut egui::Ui) {
        theme::lcd_frame().show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 22.0;
                readout(
                    ui,
                    "MOMENTAN",
                    fmt_level(self.momentary()),
                    "LUFS",
                    theme::ACCENT,
                );
                readout(
                    ui,
                    "KURZZEIT",
                    fmt_level(self.short_term()),
                    "LUFS",
                    theme::ACCENT,
                );
                readout(
                    ui,
                    "INTEGRIERT",
                    fmt_level(self.integrated()),
                    "LUFS",
                    theme::ACCENT,
                );
                let tp = self.true_peak_dbtp();
                let hot = tp.is_some_and(|v| v > -1.0);
                readout(
                    ui,
                    "TRUE PEAK",
                    fmt_level(tp),
                    "dBTP",
                    if hot { theme::ON_AIR } else { theme::OK_GREEN },
                );
                let c = self.correlation();
                readout(
                    ui,
                    "KORRELATION",
                    c.map_or_else(|| "--.--".to_string(), |v| format!("{v:+.2}")),
                    "−1 … +1",
                    corr_colour(c),
                );
            });
        });
    }

    fn draw_spectrum(&self, ui: &mut egui::Ui, size: egui::Vec2) {
        let (resp, painter) = ui.allocate_painter(size, Sense::hover());
        let r = resp.rect;
        painter.rect_filled(r, 4.0, theme::WELL);
        let plot = r.shrink2(vec2(30.0, 8.0));
        let db_y = |db: f32| plot.bottom() - (db - MIN_DB) / -MIN_DB * plot.height();
        for db in [-80.0f32, -60.0, -40.0, -20.0, 0.0] {
            let y = db_y(db);
            painter.line_segment(
                [Pos2::new(plot.left(), y), Pos2::new(plot.right(), y)],
                Stroke::new(1.0, theme::GRID),
            );
            painter.text(
                Pos2::new(r.left() + 4.0, y),
                Align2::LEFT_CENTER,
                format!("{db:.0}"),
                FontId::monospace(9.0),
                theme::DIM,
            );
        }
        for (f, label) in [
            (50.0, "50"),
            (100.0, "100"),
            (500.0, "500"),
            (1000.0, "1k"),
            (5000.0, "5k"),
            (10_000.0, "10k"),
        ] {
            let x = plot.left() + x_of(f, plot.width(), self.rate);
            painter.line_segment(
                [Pos2::new(x, plot.top()), Pos2::new(x, plot.bottom())],
                Stroke::new(1.0, theme::GRID),
            );
            painter.text(
                Pos2::new(x + 2.0, plot.bottom() - 2.0),
                Align2::LEFT_BOTTOM,
                label,
                FontId::monospace(9.0),
                theme::DIM,
            );
        }
        let bw = plot.width() / BARS as f32;
        for (i, (&b, &h)) in self.bars.iter().zip(&self.hold).enumerate() {
            let x = plot.left() + i as f32 * bw;
            let frac = ((b - MIN_DB) / -MIN_DB).clamp(0.0, 1.0);
            if frac > 0.0 {
                let bar = Rect::from_min_max(
                    Pos2::new(x, db_y(b.max(MIN_DB))),
                    Pos2::new(x + bw.max(1.0) - 0.5, plot.bottom()),
                );
                painter.rect_filled(bar, 0.0, theme::mix(theme::CUE_CYAN, theme::ACCENT, frac));
            }
            if h > MIN_DB {
                let y = db_y(h);
                painter.line_segment(
                    [Pos2::new(x, y), Pos2::new(x + bw.max(1.0), y)],
                    Stroke::new(1.0, theme::TEXT_BRIGHT),
                );
            }
        }
    }

    fn draw_spectrogram(&mut self, ui: &mut egui::Ui, size: egui::Vec2) {
        let (resp, painter) = ui.allocate_painter(size, Sense::hover());
        let r = resp.rect;
        painter.rect_filled(r, 4.0, theme::WELL);
        if self.texture_dirty || self.texture.is_none() {
            let img = self.spectrogram_image();
            match &mut self.texture {
                Some(t) => t.set(img, egui::TextureOptions::LINEAR),
                None => {
                    self.texture = Some(ui.ctx().load_texture(
                        "analysis-spectrogram",
                        img,
                        egui::TextureOptions::LINEAR,
                    ));
                }
            }
            self.texture_dirty = false;
        }
        if let Some(t) = &self.texture {
            let inner = r.shrink(1.0);
            painter.image(
                t.id(),
                inner,
                Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                Color32::WHITE,
            );
            let top = DISPLAY_MAX_HZ.min(self.rate as f32 / 2.0);
            for (f, label) in [(100.0, "100"), (1000.0, "1k"), (10_000.0, "10k")] {
                let t = (f / DISPLAY_MIN_HZ).ln() / (top / DISPLAY_MIN_HZ).ln();
                let y = inner.bottom() - t * inner.height();
                painter.text(
                    Pos2::new(inner.left() + 3.0, y),
                    Align2::LEFT_CENTER,
                    label,
                    FontId::monospace(9.0),
                    theme::TEXT_BRIGHT,
                );
            }
        }
    }

    fn draw_scope(&self, ui: &mut egui::Ui, side: f32) {
        let (resp, painter) = ui.allocate_painter(vec2(side, side), Sense::hover());
        let r = resp.rect;
        painter.rect_filled(r, 4.0, theme::WELL);
        let c = r.center();
        let k = side * 0.46;
        let axis = Stroke::new(1.0, theme::GRID);
        painter.line_segment(
            [
                Pos2::new(c.x, r.top() + 4.0),
                Pos2::new(c.x, r.bottom() - 4.0),
            ],
            axis,
        );
        painter.line_segment(
            [
                Pos2::new(r.left() + 4.0, c.y),
                Pos2::new(r.right() - 4.0, c.y),
            ],
            axis,
        );
        painter.line_segment(
            [Pos2::new(c.x - k, c.y - k), Pos2::new(c.x + k, c.y + k)],
            axis,
        );
        painter.line_segment(
            [Pos2::new(c.x - k, c.y + k), Pos2::new(c.x + k, c.y - k)],
            axis,
        );
        let font = FontId::monospace(10.0);
        painter.text(
            Pos2::new(r.left() + 6.0, r.top() + 6.0),
            Align2::LEFT_TOP,
            "L",
            font.clone(),
            theme::DIM,
        );
        painter.text(
            Pos2::new(r.right() - 6.0, r.top() + 6.0),
            Align2::RIGHT_TOP,
            "R",
            font.clone(),
            theme::DIM,
        );
        painter.text(
            Pos2::new(c.x + 4.0, r.top() + 6.0),
            Align2::LEFT_TOP,
            "M",
            font,
            theme::DIM,
        );
        let n = self.scope.len().max(1);
        for (i, (x, y)) in self.scope().enumerate() {
            let age = (i + 1) as f32 / n as f32;
            let p = Pos2::new(
                (c.x + x * k).clamp(r.left() + 2.0, r.right() - 2.0),
                (c.y - y * k).clamp(r.top() + 2.0, r.bottom() - 2.0),
            );
            let col = Color32::from_rgba_unmultiplied(68, 186, 255, (30.0 + 200.0 * age) as u8);
            painter.circle_filled(p, 1.2, col);
        }
    }

    fn draw_correlation(&self, ui: &mut egui::Ui, side: f32) {
        let (resp, painter) = ui.allocate_painter(vec2(side, 34.0), Sense::hover());
        let r = resp.rect;
        painter.rect_filled(r, 4.0, theme::WELL);
        let bar = Rect::from_min_max(
            Pos2::new(r.left() + 8.0, r.top() + 6.0),
            Pos2::new(r.right() - 8.0, r.top() + 18.0),
        );
        painter.rect_filled(bar, 2.0, theme::PANEL_HEADER);
        painter.line_segment(
            [
                Pos2::new(bar.center().x, bar.top()),
                Pos2::new(bar.center().x, bar.bottom()),
            ],
            Stroke::new(1.0, theme::BORDER),
        );
        let c = self.correlation();
        if let Some(v) = c {
            let x = bar.left() + ((v as f32 + 1.0) / 2.0) * bar.width();
            painter.rect_filled(
                Rect::from_center_size(Pos2::new(x, bar.center().y), vec2(5.0, bar.height() + 2.0)),
                1.0,
                corr_colour(c),
            );
        }
        let font = FontId::monospace(9.0);
        painter.text(
            Pos2::new(bar.left(), r.bottom() - 3.0),
            Align2::LEFT_BOTTOM,
            "−1",
            font.clone(),
            theme::DIM,
        );
        painter.text(
            Pos2::new(bar.center().x, r.bottom() - 3.0),
            Align2::CENTER_BOTTOM,
            "0",
            font.clone(),
            theme::DIM,
        );
        painter.text(
            Pos2::new(bar.right(), r.bottom() - 3.0),
            Align2::RIGHT_BOTTOM,
            "+1",
            font,
            theme::DIM,
        );
    }
}

fn corr_colour(c: Option<f64>) -> Color32 {
    match c {
        None => theme::DIM,
        Some(v) if v < 0.0 => theme::ON_AIR,
        Some(v) if v < 0.3 => theme::ACCENT,
        Some(_) => theme::OK_GREEN,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(freq: f32, amp: f32, n: usize, rate: u32) -> Vec<f32> {
        (0..n)
            .map(|i| amp * (std::f32::consts::TAU * freq * i as f32 / rate as f32).sin())
            .collect()
    }

    #[test]
    fn feeding_a_tone_fills_every_display() {
        let rate = 48_000;
        let mut v = AnalysisView::new(rate);
        let t = tone(1000.0, 0.07, rate as usize * 4, rate);
        // In Stücken wie aus dem Ring.
        for chunk in t.chunks(1500) {
            v.feed(chunk, chunk);
        }
        // 0,07 ≈ −23 dBFS: etwa −23 LUFS.
        assert!(
            (v.integrated().unwrap() + 23.1).abs() < 0.3,
            "{:?}",
            v.integrated()
        );
        assert!((v.momentary().unwrap() - v.integrated().unwrap()).abs() < 0.2);
        assert!(v.short_term().is_some());
        assert!((v.correlation().unwrap() - 1.0).abs() < 1e-6);
        let tp = v.true_peak_dbtp().unwrap();
        assert!((tp + 23.1).abs() < 0.3, "{tp}");
        // Spektrum: das Maximum liegt bei 1 kHz.
        let best = v
            .bars()
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .unwrap()
            .0;
        let pos = (1000.0f32 / 20.0).ln() / (20_000.0f32 / 20.0).ln();
        assert!((best as f32 / BARS as f32 - pos).abs() < 0.015, "{best}");
        // Spektrogramm hat Spalten, Scope hat Punkte auf der Mono-Achse.
        assert!(v.columns.len() > 40 && v.columns.len() <= SPEC_COLS);
        assert_eq!(v.scope().count(), SCOPE_POINTS);
        assert!(v.scope().all(|(x, y)| x.abs() < 1e-6 && y.abs() <= 0.11));
        let img = v.spectrogram_image();
        assert_eq!(img.size, [SPEC_COLS, SPEC_ROWS]);
    }

    #[test]
    fn wide_pan_shows_in_scope_and_correlation() {
        let rate = 48_000;
        let mut v = AnalysisView::new(rate);
        let l = tone(440.0, 0.5, rate as usize, rate);
        let r = tone(1000.0, 0.5, rate as usize, rate);
        v.feed(&l, &r);
        assert!(v.correlation().unwrap().abs() < 0.2);
        let wide = v.scope().filter(|(x, _)| x.abs() > 0.1).count();
        assert!(wide > SCOPE_POINTS / 2, "{wide}");
        // Gegenphase: Korrelation −1, Scope liegt auf der Seitenachse.
        v.reset();
        let inv: Vec<f32> = l.iter().map(|s| -s).collect();
        v.feed(&l, &inv);
        assert!(v.correlation().unwrap() < -0.99);
        assert!(v.scope().all(|(_, y)| y.abs() < 1e-6));
    }

    #[test]
    fn reset_source_and_idle() {
        let mut v = AnalysisView::new(48_000);
        let t = tone(1000.0, 0.5, 48_000, 48_000);
        v.feed(&t, &t);
        assert!(v.integrated().is_none() || v.integrated().is_some());
        let peak_before = v.bars().iter().copied().fold(FLOOR_DB, f32::max);
        v.idle();
        let peak_after = v.bars().iter().copied().fold(FLOOR_DB, f32::max);
        assert!(peak_after < peak_before);
        assert_eq!(v.scope().count(), SCOPE_POINTS - 48);
        v.set_source(Some(TrackId(3)));
        assert_eq!(v.source(), Some(TrackId(3)));
        assert!(v.true_peak_dbtp().is_none() && v.columns.is_empty());
        // Gleiche Quelle noch einmal: nichts wird verworfen.
        v.feed(&t, &t);
        v.set_source(Some(TrackId(3)));
        assert!(v.true_peak_dbtp().is_some());
        v.set_rate(44_100);
        assert!(v.true_peak_dbtp().is_none());
        // Leere und ungleich lange Eingaben sind harmlos.
        v.feed(&[], &[]);
        v.feed(&t[..100], &t[..10]);
    }

    #[test]
    fn level_text() {
        assert_eq!(fmt_level(None), "--.-");
        assert_eq!(fmt_level(Some(-23.04)), "-23.0");
        assert_eq!(fmt_level(Some(f64::NEG_INFINITY)), "--.-");
    }
}
