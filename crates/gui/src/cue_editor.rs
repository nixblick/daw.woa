//! Cue-Editor (Paket 15): Wellenform eines Playlist-Titels mit Cue-In, Cue-Out, Ramp,
//! Hook und Fade-Out-Marker. Klick in die Wellenform setzt den gewählten Marker,
//! „Übernehmen“ schickt die Punkte als ein Undo-Schritt an den Controller. Vorhören geht
//! nur auf den Kopfhörer-Ausgang (PFL), nie auf den Sendebus.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use eframe::egui;
use woa_radio::Cues;

use crate::theme;

/// Rate, in der die Wellenform gelesen wird (reicht für die Anzeige).
const PEAK_RATE: u32 = 8_000;
/// Balken der Wellenform.
const BINS: usize = 600;

/// Welcher Marker per Klick gesetzt wird.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Marker {
    CueIn,
    CueOut,
    Ramp,
    Hook,
    FadeOut,
}

impl Marker {
    pub const ALL: [Marker; 5] = [
        Marker::CueIn,
        Marker::Ramp,
        Marker::Hook,
        Marker::FadeOut,
        Marker::CueOut,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Marker::CueIn => "Cue-In",
            Marker::CueOut => "Cue-Out",
            Marker::Ramp => "Ramp",
            Marker::Hook => "Hook",
            Marker::FadeOut => "Fade-Out",
        }
    }

    fn color(self) -> egui::Color32 {
        match self {
            Marker::CueIn | Marker::CueOut => theme::OK_GREEN,
            Marker::Ramp => theme::ACCENT,
            Marker::Hook => theme::CUE_CYAN,
            Marker::FadeOut => theme::ON_AIR,
        }
    }

    pub fn get(self, c: &Cues) -> Option<f64> {
        match self {
            Marker::CueIn => c.cue_in,
            Marker::CueOut => c.cue_out,
            Marker::Ramp => c.ramp,
            Marker::Hook => c.hook,
            Marker::FadeOut => c.fade_out,
        }
    }

    pub fn set(self, c: &mut Cues, v: Option<f64>) {
        let slot = match self {
            Marker::CueIn => &mut c.cue_in,
            Marker::CueOut => &mut c.cue_out,
            Marker::Ramp => &mut c.ramp,
            Marker::Hook => &mut c.hook,
            Marker::FadeOut => &mut c.fade_out,
        };
        *slot = v;
    }
}

/// Marker, die die Analyse vorschlägt.
const AUTO_MARKERS: [Marker; 3] = [Marker::CueIn, Marker::FadeOut, Marker::CueOut];

/// Was der Editor nach einem Bild vom Radio-Layout will.
#[derive(Debug, Clone, PartialEq)]
pub enum CueAction {
    Apply(usize, Cues),
    Preview(usize, f64),
    StopPreview,
    Undo,
    Redo,
}

/// Höchstwerte je Balken (0..1).
pub fn peaks(left: &[f32], right: &[f32], bins: usize) -> Vec<f32> {
    let n = left.len().min(right.len());
    if n == 0 || bins == 0 {
        return Vec::new();
    }
    (0..bins)
        .map(|b| {
            let (a, z) = (
                b * n / bins,
                ((b + 1) * n / bins).max(b * n / bins + 1).min(n),
            );
            left[a..z]
                .iter()
                .chain(&right[a..z])
                .fold(0.0f32, |m, v| m.max(v.abs()))
                .min(1.0)
        })
        .collect()
}

/// Wellenform und Länge in Sekunden, oder Fehler.
type PeakResult = Result<(Vec<f32>, f64, Cues), String>;

/// Ein offener Editor.
pub struct CueEditor {
    pub index: usize,
    pub path: PathBuf,
    /// Cart-Wall statt Playlist: `index` ist das Cart, nur Cue-In und Cue-Out zählen.
    pub cart: bool,
    /// Deck statt Cart: `index` ist das Deck (mit `cart` gesetzt).
    pub deck: bool,
    title: String,
    seconds: Option<f64>,
    pub draft: Cues,
    marker: Marker,
    peaks: Option<Vec<f32>>,
    loading: Option<Receiver<PeakResult>>,
    error: Option<String>,
    /// Vorschlag aus der Analyse (Stille vorn und hinten, Ausklang).
    auto: Cues,
}

impl CueEditor {
    /// Öffnet den Editor für Titel `index`; die Wellenform lädt im Hintergrund.
    pub fn open(
        index: usize,
        path: PathBuf,
        title: String,
        seconds: Option<f64>,
        cues: Cues,
    ) -> Self {
        let (tx, rx) = channel();
        let p = path.clone();
        let spawned = std::thread::Builder::new()
            .name("woa-cue-peaks".into())
            .spawn(move || {
                let r = woa_radio::audio::load_stereo(&p, PEAK_RATE).map(|s| {
                    (
                        peaks(&s.left, &s.right, BINS),
                        s.frames() as f64 / f64::from(PEAK_RATE),
                        woa_radio::audio::suggest_cues(&s.left, &s.right, PEAK_RATE),
                    )
                });
                let _ = tx.send(r);
            });
        let error = spawned.err().map(|e| format!("Wellenform: {e}"));
        Self {
            index,
            path,
            cart: false,
            deck: false,
            title,
            seconds,
            draft: cues,
            marker: Marker::CueIn,
            peaks: None,
            loading: Some(rx),
            error,
            auto: Cues::default(),
        }
    }

    /// Die Marker, die dieser Editor anbietet: bei einem Cart nur Anfang und Ende.
    fn markers(&self) -> &'static [Marker] {
        if self.cart {
            &[Marker::CueIn, Marker::CueOut]
        } else {
            &Marker::ALL
        }
    }

    /// Gibt es einen Vorschlag für einen Marker, der noch nicht gesetzt ist?
    pub fn can_fill_auto(&self) -> bool {
        AUTO_MARKERS
            .iter()
            .filter(|m| self.markers().contains(m))
            .any(|m| m.get(&self.draft).is_none() && m.get(&self.auto).is_some())
    }

    /// Setzt die vorgeschlagenen Marker, die noch fehlen. Gesetzte bleiben (Handarbeit hat
    /// Vorrang). Passt das Ergebnis nicht zusammen, bleibt der Entwurf unverändert.
    pub fn fill_auto(&mut self) -> bool {
        let mut next = self.draft;
        for m in AUTO_MARKERS
            .into_iter()
            .filter(|m| self.markers().contains(m))
        {
            if m.get(&next).is_none() {
                m.set(&mut next, m.get(&self.auto));
            }
        }
        if next == self.draft || next.validate(self.seconds).is_err() {
            return false;
        }
        self.draft = next;
        true
    }

    fn auto_text(&self) -> String {
        let parts: Vec<String> = AUTO_MARKERS
            .iter()
            .filter(|m| self.markers().contains(m))
            .filter_map(|m| m.get(&self.auto).map(|v| format!("{} {v:.2} s", m.label())))
            .collect();
        if self.loading.is_some() {
            String::new()
        } else if parts.is_empty() {
            "Kein Vorschlag: keine Stille vorn oder hinten".into()
        } else {
            format!("Vorschlag: {}", parts.join(" · "))
        }
    }

    fn poll(&mut self) {
        let Some(rx) = &self.loading else { return };
        match rx.try_recv() {
            Ok(Ok((p, secs, auto))) => {
                self.auto = auto;
                self.peaks = Some(p);
                self.seconds.get_or_insert(secs);
                self.loading = None;
            }
            Ok(Err(e)) => {
                self.error = Some(e);
                self.loading = None;
            }
            Err(TryRecvError::Disconnected) => self.loading = None,
            Err(TryRecvError::Empty) => {}
        }
    }

    /// Zeichnet das Fenster. `open` wird `false`, wenn es geschlossen wird.
    pub fn ui(
        &mut self,
        ctx: &egui::Context,
        open: &mut bool,
        pfl: (bool, bool),
        undo: (bool, bool),
    ) -> Vec<CueAction> {
        self.poll();
        if self.loading.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        let mut out = Vec::new();
        let title = if self.deck {
            format!("Deck schneiden: {}", self.title)
        } else if self.cart {
            format!("Cart schneiden: {}", self.title)
        } else {
            format!("Cue-Punkte: {}", self.title)
        };
        egui::Window::new(title)
            .id(egui::Id::new("woa-cue-editor"))
            .open(open)
            .default_width(640.0)
            .show(ctx, |ui| self.body(ui, pfl, undo, &mut out));
        out
    }

    fn body(
        &mut self,
        ui: &mut egui::Ui,
        (pfl_available, pfl_playing): (bool, bool),
        (can_undo, can_redo): (bool, bool),
        out: &mut Vec<CueAction>,
    ) {
        let len = self.seconds.unwrap_or(0.0);
        ui.horizontal(|ui| {
            ui.label("Klick setzt:");
            for &m in self.markers() {
                ui.selectable_value(
                    &mut self.marker,
                    m,
                    egui::RichText::new(m.label()).color(m.color()),
                );
            }
        });
        let (rect, resp) = ui.allocate_exact_size(
            egui::vec2(ui.available_width().max(200.0), 110.0),
            egui::Sense::click_and_drag(),
        );
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 4.0, theme::WELL);
        let x_of = |s: f64| rect.left() + (s / len.max(1e-9)) as f32 * rect.width();
        if let Some(p) = &self.peaks {
            let w = rect.width() / p.len().max(1) as f32;
            let start = self.draft.start();
            let end = self.draft.end(len);
            for (i, v) in p.iter().enumerate() {
                let x = rect.left() + i as f32 * w;
                let t = (i as f64 + 0.5) / p.len() as f64 * len;
                let col = if t < start || t > end {
                    theme::BORDER
                } else {
                    theme::DIM
                };
                let h = v * rect.height() * 0.48;
                painter.line_segment(
                    [
                        egui::pos2(x, rect.center().y - h),
                        egui::pos2(x, rect.center().y + h),
                    ],
                    egui::Stroke::new(w.max(1.0), col),
                );
            }
        } else if self.loading.is_some() {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "Wellenform wird gelesen …",
                egui::FontId::proportional(13.0),
                theme::DIM,
            );
        }
        if len > 0.0 {
            for m in Marker::ALL {
                if let Some(s) = m.get(&self.draft) {
                    let x = x_of(s);
                    painter.line_segment(
                        [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
                        egui::Stroke::new(2.0, m.color()),
                    );
                    painter.text(
                        egui::pos2(x + 3.0, rect.top() + 2.0),
                        egui::Align2::LEFT_TOP,
                        m.label(),
                        egui::FontId::proportional(11.0),
                        m.color(),
                    );
                }
            }
            // Vorschläge der Analyse als blasse Linien, solange der Marker nicht gesetzt ist.
            for m in AUTO_MARKERS {
                if m.get(&self.draft).is_none()
                    && let Some(s) = m.get(&self.auto)
                {
                    let x = x_of(s);
                    painter.line_segment(
                        [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
                        egui::Stroke::new(1.0, theme::mix(m.color(), theme::WELL, 0.45)),
                    );
                    painter.text(
                        egui::pos2(x + 3.0, rect.bottom() - 2.0),
                        egui::Align2::LEFT_BOTTOM,
                        format!("{} (Vorschlag)", m.label()),
                        egui::FontId::proportional(10.0),
                        theme::mix(m.color(), theme::WELL, 0.35),
                    );
                }
            }
            if let Some(pos) = resp.interact_pointer_pos()
                && (resp.clicked() || resp.dragged())
            {
                let s = (f64::from((pos.x - rect.left()) / rect.width()) * len).clamp(0.0, len);
                self.marker
                    .set(&mut self.draft, Some((s * 100.0).round() / 100.0));
            }
        }
        ui.add_space(4.0);
        egui::Grid::new("woa-cue-values")
            .num_columns(3)
            .show(ui, |ui| {
                for m in Marker::ALL {
                    ui.colored_label(m.color(), m.label());
                    match m.get(&self.draft) {
                        Some(mut v) => {
                            if ui
                                .add(
                                    egui::DragValue::new(&mut v)
                                        .speed(0.01)
                                        .range(0.0..=len.max(0.0))
                                        .suffix(" s")
                                        .max_decimals(2),
                                )
                                .changed()
                            {
                                m.set(&mut self.draft, Some(v));
                            }
                            if ui
                                .small_button("x")
                                .on_hover_text("Marker entfernen")
                                .clicked()
                            {
                                m.set(&mut self.draft, None);
                            }
                        }
                        None => {
                            ui.label(egui::RichText::new("nicht gesetzt").color(theme::DIM));
                            ui.label("");
                        }
                    }
                    ui.end_row();
                }
            });
        ui.horizontal(|ui| {
            let can_fill = self.can_fill_auto();
            if ui
                .add_enabled(can_fill, egui::Button::new("Vorschlag einsetzen"))
                .on_hover_text(
                    "Setzt Cue-In, Fade-Out und Cue-Out aus der Analyse (Stille vorn und hinten, \
                     Beginn des Ausklangs). Schon gesetzte Marker bleiben unverändert.",
                )
                .on_disabled_hover_text(if self.auto.is_empty() {
                    "Die Analyse schlägt hier nichts vor (keine Stille vorn oder hinten)"
                } else {
                    "Alle vorgeschlagenen Marker sind schon gesetzt"
                })
                .clicked()
            {
                self.fill_auto();
            }
            ui.label(
                egui::RichText::new(self.auto_text())
                    .small()
                    .color(theme::DIM),
            );
        });
        let check = self.draft.validate(self.seconds);
        if let Err(e) = &check {
            ui.colored_label(theme::ON_AIR, e);
        }
        if let Some(e) = &self.error {
            ui.colored_label(theme::ON_AIR, e);
        }
        ui.separator();
        ui.horizontal(|ui| {
            if ui
                .add_enabled(check.is_ok(), egui::Button::new("Übernehmen"))
                .on_hover_text("Als ein Rückgängig-Schritt speichern")
                .clicked()
            {
                out.push(CueAction::Apply(self.index, self.draft));
            }
            if ui
                .add_enabled(can_undo, egui::Button::new("Rückgängig"))
                .on_hover_text("Cue-Bearbeitung zurücknehmen")
                .clicked()
            {
                out.push(CueAction::Undo);
            }
            if ui
                .add_enabled(can_redo, egui::Button::new("Wiederholen"))
                .on_hover_text("Cue-Bearbeitung wiederholen")
                .clicked()
            {
                out.push(CueAction::Redo);
            }
            ui.separator();
            let from = self.marker.get(&self.draft).unwrap_or(self.draft.start());
            let hint = if pfl_available {
                "Im Kopfhörer (PFL) ab dem gewählten Marker, nie auf Sendung"
            } else {
                "Kein Kopfhörer-Ausgang: Einstellungen → Vorhören (PFL)"
            };
            if ui
                .add_enabled(
                    pfl_available,
                    egui::Button::new(format!("Vorhören ab {}", self.marker.label())),
                )
                .on_hover_text(hint)
                .on_disabled_hover_text(hint)
                .clicked()
            {
                out.push(CueAction::Preview(self.index, from));
            }
            if ui
                .add_enabled(pfl_playing, egui::Button::new("Vorhören stopp"))
                .clicked()
            {
                out.push(CueAction::StopPreview);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peaks_cover_the_whole_signal() {
        let l: Vec<f32> = (0..1000)
            .map(|i| if i == 999 { 0.9 } else { 0.1 })
            .collect();
        let p = peaks(&l, &l, 10);
        assert_eq!(p.len(), 10);
        assert!((p[9] - 0.9).abs() < 1e-6 && (p[0] - 0.1).abs() < 1e-6);
        assert!(peaks(&[], &[], 10).is_empty());
        // Mehr Balken als Werte: kein Panik, jeder Balken hat mindestens einen Wert.
        assert_eq!(peaks(&[0.5; 3], &[0.5; 3], 8).len(), 8);
    }

    /// 1 s Stille, 5 s Ton, 2 s Stille als WAV.
    fn write_gapped(path: &std::path::Path) {
        let rate = 22_050u32;
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut w = hound::WavWriter::create(path, spec).unwrap();
        for i in 0..8 * rate as usize {
            let loud = (rate as usize..6 * rate as usize).contains(&i);
            let v = if loud {
                (i as f32 * 440.0 * std::f32::consts::TAU / rate as f32).sin() * 0.5
            } else {
                0.0
            };
            w.write_sample((v * 32767.0) as i16).unwrap();
        }
        w.finalize().unwrap();
    }

    #[test]
    fn auto_cues_fill_only_what_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("luecke.wav");
        write_gapped(&p);
        let manual = Cues {
            cue_out: Some(7.5),
            ..Cues::default()
        };
        let mut ed = CueEditor::open(0, p, "Lücke".into(), None, manual);
        let t0 = std::time::Instant::now();
        while ed.loading.is_some() {
            assert!(t0.elapsed().as_secs() < 30, "loading hangs");
            std::thread::sleep(std::time::Duration::from_millis(10));
            ed.poll();
        }
        assert!(ed.error.is_none(), "{:?}", ed.error);
        assert!(
            (ed.auto.cue_in.unwrap() - 0.98).abs() < 0.05,
            "{:?}",
            ed.auto
        );
        assert!(
            (ed.auto.cue_out.unwrap() - 6.05).abs() < 0.1,
            "{:?}",
            ed.auto
        );
        assert!(ed.can_fill_auto());
        assert!(ed.fill_auto());
        assert!((ed.draft.cue_in.unwrap() - 0.98).abs() < 0.05);
        assert_eq!(ed.draft.cue_out, Some(7.5), "Handarbeit bleibt");
        assert!(ed.draft.validate(ed.seconds).is_ok());
        assert!(!ed.can_fill_auto());
        assert!(!ed.fill_auto(), "zweites Mal nichts zu tun");
    }

    #[test]
    fn marker_set_get() {
        let mut c = Cues::default();
        for (i, m) in Marker::ALL.into_iter().enumerate() {
            m.set(&mut c, Some(i as f64));
            assert_eq!(m.get(&c), Some(i as f64));
        }
    }
}
