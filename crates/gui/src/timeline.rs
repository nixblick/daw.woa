//! Zeitleisten-Hilfen ohne Oberfläche (Paket 20): Raster, Lineal-Teilung, Zwischenablage.
//! Alles reine Rechnung, damit es ohne Fenster testbar ist.

use woa_model::{Clip, TrackId};

/// Rasterweite beim Verschieben und Kürzen von Clips.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Snap {
    /// Kein Einrasten.
    Off,
    /// Ganze Takte (4 Beats).
    Bar,
    /// Ganze Schläge.
    Beat,
    /// Halbe Schläge.
    Half,
    /// Viertel Schlag (1/16-Note), die Vorgabe seit dem ersten Prototyp.
    #[default]
    Quarter,
}

impl Snap {
    /// Alle Stufen in der Reihenfolge der Auswahl.
    pub const ALL: [Snap; 5] = [Snap::Off, Snap::Bar, Snap::Beat, Snap::Half, Snap::Quarter];

    /// Schrittweite in Beats; `None` bei `Off`.
    pub fn step(self) -> Option<f64> {
        match self {
            Snap::Off => None,
            Snap::Bar => Some(4.0),
            Snap::Beat => Some(1.0),
            Snap::Half => Some(0.5),
            Snap::Quarter => Some(0.25),
        }
    }

    /// Anzeigename.
    pub fn label(self) -> &'static str {
        match self {
            Snap::Off => "Aus",
            Snap::Bar => "Takt",
            Snap::Beat => "1/4",
            Snap::Half => "1/8",
            Snap::Quarter => "1/16",
        }
    }

    /// Rastet `beats` auf das nächste Raster ein (nie unter 0).
    pub fn apply(self, beats: f64) -> f64 {
        let v = match self.step() {
            Some(s) => (beats / s).round() * s,
            None => beats,
        };
        v.max(0.0)
    }
}

/// Einheit der Lineal-Beschriftung.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TimeUnit {
    /// Taktnummern.
    #[default]
    Bars,
    /// Sekunden (Minuten:Sekunden bei langen Stücken).
    Seconds,
}

/// Eine Markierung im Lineal.
#[derive(Debug, Clone, PartialEq)]
pub struct Tick {
    /// Lage in Beats.
    pub beat: f64,
    /// Beschriftung; leer bei feinen Strichen.
    pub label: String,
    /// Kräftiger Strich.
    pub major: bool,
}

/// Teilung des Lineals für `end_beats` bei `px` Pixel je Beat. Beschriftete Striche stehen
/// mindestens ~64 Pixel auseinander.
pub fn ruler_ticks(unit: TimeUnit, tempo_bpm: f64, px: f32, end_beats: f64) -> Vec<Tick> {
    let end = end_beats.max(0.0);
    let px = f64::from(px.max(0.1));
    let mut out = Vec::new();
    match unit {
        TimeUnit::Bars => {
            let bars_per_label = [1u32, 2, 4, 8, 16, 32, 64, 128]
                .into_iter()
                .find(|b| f64::from(*b) * 4.0 * px >= 64.0)
                .unwrap_or(256);
            let mut bar = 0u32;
            while f64::from(bar) * 4.0 <= end {
                out.push(Tick {
                    beat: f64::from(bar) * 4.0,
                    label: format!("{}", bar + 1),
                    major: true,
                });
                bar += bars_per_label;
            }
        }
        TimeUnit::Seconds => {
            let secs_per_beat = 60.0 / tempo_bpm.max(1.0);
            let px_per_s = px / secs_per_beat;
            let step = [
                0.5f64, 1.0, 2.0, 5.0, 10.0, 15.0, 30.0, 60.0, 120.0, 300.0, 600.0,
            ]
            .into_iter()
            .find(|s| s * px_per_s >= 64.0)
            .unwrap_or(1200.0);
            let mut n = 0u32;
            loop {
                let s = f64::from(n) * step;
                let beat = s / secs_per_beat;
                if beat > end {
                    break;
                }
                out.push(Tick {
                    beat,
                    label: format_time(s, step < 1.0),
                    major: true,
                });
                n += 1;
            }
        }
    }
    out
}

/// `m:ss` bzw. `m:ss.d` mit Zehntelsekunden.
pub fn format_time(seconds: f64, tenths: bool) -> String {
    let s = seconds.max(0.0);
    let m = (s / 60.0).floor();
    let rest = s - m * 60.0;
    if tenths {
        format!("{}:{:04.1}", m as u64, rest)
    } else {
        format!("{}:{:02}", m as u64, rest.round() as u64 % 60)
    }
}

/// Kopierte Clips mit ihrer ursprünglichen Spur.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Clipboard {
    /// Spur und Clip-Kopie.
    pub items: Vec<(TrackId, Clip)>,
}

impl Clipboard {
    /// Früheste Startposition der kopierten Clips.
    pub fn first_start(&self) -> f64 {
        self.items
            .iter()
            .map(|(_, c)| c.start_beats)
            .fold(f64::MAX, f64::min)
    }

    /// Ende des spätesten kopierten Clips.
    pub fn last_end(&self) -> f64 {
        self.items
            .iter()
            .map(|(_, c)| c.end_beats())
            .fold(0.0, f64::max)
    }

    /// Nichts kopiert?
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snap_rounds_and_never_goes_negative() {
        assert_eq!(Snap::Quarter.apply(1.1), 1.0);
        assert_eq!(Snap::Quarter.apply(1.13), 1.25);
        assert_eq!(Snap::Beat.apply(2.6), 3.0);
        assert_eq!(Snap::Bar.apply(5.9), 4.0);
        assert_eq!(Snap::Bar.apply(6.1), 8.0);
        assert_eq!(Snap::Off.apply(1.2345), 1.2345);
        assert_eq!(Snap::Half.apply(-3.0), 0.0);
        assert_eq!(Snap::Off.apply(-1.0), 0.0);
    }

    #[test]
    fn bar_ruler_spreads_labels_with_zoom() {
        let wide = ruler_ticks(TimeUnit::Bars, 120.0, 40.0, 16.0);
        assert_eq!(wide.len(), 5, "every bar at 40 px/beat");
        assert_eq!(wide[1].beat, 4.0);
        assert_eq!(wide[1].label, "2");
        let narrow = ruler_ticks(TimeUnit::Bars, 120.0, 2.0, 64.0);
        assert!(narrow.len() < 6, "sparse at 2 px/beat: {}", narrow.len());
        assert_eq!(narrow[0].label, "1");
    }

    #[test]
    fn second_ruler_follows_the_tempo() {
        // 120 bpm: 2 beats per second. At 40 px/beat one second is 80 px: step 1 s.
        let t = ruler_ticks(TimeUnit::Seconds, 120.0, 40.0, 8.0);
        assert_eq!(t[1].beat, 2.0);
        assert_eq!(t[1].label, "0:01");
        assert_eq!(t.last().map(|x| x.beat), Some(8.0));
        // At 60 bpm a beat is a second (40 px): labels every 2 s = 2 beats.
        let slow = ruler_ticks(TimeUnit::Seconds, 60.0, 40.0, 8.0);
        assert_eq!(slow[1].beat, 2.0);
        assert_eq!(slow[1].label, "0:02");
    }

    #[test]
    fn time_format() {
        assert_eq!(format_time(0.0, false), "0:00");
        assert_eq!(format_time(75.0, false), "1:15");
        assert_eq!(format_time(0.5, true), "0:00.5");
    }
}
