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

/// Automatische Überblendung je Clip (Paket 21), als (Einblendung, Ausblendung) in Beats.
/// Dieselbe Regel wie in der Engine (`woa_engine::clip::apply_crossfades`): Überlappt ein
/// späterer Audio-Clip das Ende eines früheren und endet danach, blendet der frühere über die
/// Überlappung aus und der spätere ein. Nur Clips einer Spur übergeben.
pub fn crossfade_overlaps(
    clips: &[Clip],
) -> std::collections::HashMap<woa_model::ClipId, (f64, f64)> {
    let mut out = std::collections::HashMap::new();
    let mut audio: Vec<&Clip> = clips.iter().filter(|c| c.audio.is_some()).collect();
    audio.sort_by(|a, b| a.start_beats.total_cmp(&b.start_beats));
    for w in audio.windows(2) {
        let (a, b) = (w[0], w[1]);
        let overlap = a.end_beats() - b.start_beats;
        if overlap <= 1e-9 || b.end_beats() <= a.end_beats() + 1e-9 {
            continue;
        }
        out.entry(a.id).or_insert((0.0, 0.0)).1 = overlap;
        out.entry(b.id).or_insert((0.0, 0.0)).0 = overlap;
    }
    out
}

/// Punkte einer Fade-Kurve für die Anzeige: `n + 1` Paare (x, Höhe) mit x und Höhe in 0..1,
/// von links nach rechts. Beim Einblenden steigt die Höhe, beim Ausblenden fällt sie.
pub fn fade_points(curve: woa_model::FadeCurve, fade_out: bool, n: usize) -> Vec<(f32, f32)> {
    let n = n.max(1);
    (0..=n)
        .map(|k| {
            let x = k as f32 / n as f32;
            let g = if fade_out {
                curve.gain(1.0 - x)
            } else {
                curve.gain(x)
            };
            (x, g)
        })
        .collect()
}

/// Zielpegel beim Normalisieren eines Clips (dBFS, etwas Luft für Inter-Sample-Spitzen).
pub const NORMALIZE_TARGET_DB: f64 = -1.0;

/// Clip-Gain in dB, damit eine Spitze von `peak` (linear) auf `target_db` kommt.
/// `None` bei Stille oder unsinnigem Wert.
pub fn normalize_gain_db(peak: f32, target_db: f64) -> Option<f64> {
    if !(peak.is_finite() && peak > 1e-6) {
        return None;
    }
    Some(target_db - 20.0 * f64::from(peak).log10())
}

/// Größte Spitze eines Clip-Ausschnitts aus den Wellenform-Punkten (je Block ein Maximum,
/// `ppb` Punkte je Beat). Block-genau und damit eher zu hoch als zu niedrig geschätzt.
pub fn region_peak(peaks: &[f32], ppb: f64, offset_beats: f64, length_beats: f64) -> f32 {
    let a = ((offset_beats * ppb).floor().max(0.0) as usize).min(peaks.len());
    let b = (((offset_beats + length_beats) * ppb).ceil().max(0.0) as usize).clamp(a, peaks.len());
    peaks[a..b].iter().fold(0.0f32, |m, s| m.max(s.abs()))
}

/// Harmonie ab `origin` (Beat): Tonart und Akkord je Takt aus allen Noten der MIDI-Spuren
/// ohne Kit (Design `ChordTrack`). `key` ist `None`, wenn es keine solchen Noten gibt.
#[derive(Debug, Clone, PartialEq)]
pub struct Harmony {
    pub key: Option<woa_model::harmony::Key>,
    pub bars: Vec<Option<woa_model::harmony::Chord>>,
}

/// Siehe [`Harmony`]; `bars` Takte ab `origin`.
pub fn harmony(project: &woa_model::Project, origin: f64, bars: usize) -> Harmony {
    use woa_model::MidiNote;
    let mut notes: Vec<(f64, &MidiNote)> = Vec::new();
    for t in &project.tracks {
        if t.kind != woa_model::TrackKind::Midi || t.sound.is_some_and(|s| s.drums) {
            continue;
        }
        for c in &t.clips {
            let Some(midi) = &c.midi else { continue };
            for n in &midi.notes {
                // Nur der hörbare Teil des Clips zählt.
                if n.beat + n.length_beats <= c.offset_beats
                    || n.beat >= c.offset_beats + c.length_beats
                {
                    continue;
                }
                notes.push((c.start_beats + n.beat - c.offset_beats - origin, n));
            }
        }
    }
    Harmony {
        key: woa_model::harmony::detect_key(notes.iter().map(|(_, n)| *n)),
        bars: woa_model::harmony::bar_chords(notes.iter().copied(), bars),
    }
}

/// Klang im Clip-Kopf (Design `ChordTrack`): Preset-Name in Versalien, „SYNTH“ ohne Klang.
pub fn sound_caption(sound: Option<woa_model::TrackSound>) -> String {
    match sound {
        None => "SYNTH".to_owned(),
        Some(s) => {
            woa_model::preset_name(s).map_or_else(|| format!("GM {}", s.program), str::to_uppercase)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn harmony_reads_chords_and_key_from_midi_tracks() {
        use woa_model::{Clip, ClipId, Project, Track, TrackKind, TrackSound};
        let p = woa_model::chords::progression("I–V–vi–IV").expect("pop");
        let made = woa_model::chords::generate(p, 2, 4);
        let mut project = Project::new("Test");
        let mut t = Track::new(TrackId(1), "Akkorde", TrackKind::Midi);
        t.clips
            .push(Clip::new_midi(ClipId(1), "Akkorde", 4.0, 16.0, made.chords));
        project.tracks.push(t);
        let mut d = Track::new(TrackId(2), "Drums", TrackKind::Midi);
        d.sound = Some(TrackSound {
            program: 0,
            drums: true,
            bank: None,
        });
        d.clips.push(Clip::new_midi(
            ClipId(2),
            "Beat",
            0.0,
            16.0,
            woa_model::chords::generate(p, 3, 4).chords,
        ));
        project.tracks.push(d);
        let h = harmony(&project, 0.0, 6);
        let names: Vec<String> = h
            .bars
            .iter()
            .map(|c| c.map(|c| c.name()).unwrap_or_default())
            .collect();
        assert_eq!(names, ["", "D", "A", "Bm", "G", ""]);
        assert_eq!(h.key.map(|k| k.name()).as_deref(), Some("D Dur"));
        // Ab dem Clip-Anfang gezählt (Piano-Roll).
        assert_eq!(
            harmony(&project, 4.0, 1).bars[0]
                .map(|c| c.name())
                .as_deref(),
            Some("D")
        );
        assert_eq!(
            harmony(&Project::new("Leer"), 0.0, 2),
            Harmony {
                key: None,
                bars: vec![None, None]
            }
        );
    }

    #[test]
    fn sound_caption_is_upper_case() {
        let s = |program, drums| {
            Some(woa_model::TrackSound {
                program,
                drums,
                bank: None,
            })
        };
        assert_eq!(sound_caption(s(48, false)), "STREICHER");
        assert_eq!(sound_caption(s(33, false)), "E-BASS");
        assert_eq!(sound_caption(s(0, true)), "STANDARD-KIT");
        assert_eq!(sound_caption(s(100, false)), "GM 100");
        assert_eq!(sound_caption(None), "SYNTH");
    }

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

    fn audio_clip(id: u64, start: f64, len: f64) -> Clip {
        Clip::new(woa_model::ClipId(id), "c", start, len, Some("a.wav".into()))
    }

    #[test]
    fn crossfade_overlap_matches_the_engine_rule() {
        let a = audio_clip(1, 0.0, 4.0);
        let b = audio_clip(2, 3.0, 4.0);
        let m = crossfade_overlaps(&[b.clone(), a.clone()]);
        assert_eq!(m.get(&a.id), Some(&(0.0, 1.0)), "earlier clip fades out");
        assert_eq!(m.get(&b.id), Some(&(1.0, 0.0)), "later clip fades in");
        // Aneinander ohne Überlappung, ganz enthalten und MIDI: keine Überblendung.
        assert!(crossfade_overlaps(&[a.clone(), audio_clip(3, 4.0, 2.0)]).is_empty());
        assert!(crossfade_overlaps(&[a.clone(), audio_clip(4, 1.0, 2.0)]).is_empty());
        let mut midi = audio_clip(5, 3.0, 4.0);
        midi.audio = None;
        assert!(crossfade_overlaps(&[a, midi]).is_empty());
    }

    #[test]
    fn fade_points_run_from_zero_to_one() {
        use woa_model::FadeCurve;
        for c in FadeCurve::ALL {
            let up = fade_points(c, false, 8);
            assert_eq!((up[0], up[8]), ((0.0, 0.0), (1.0, 1.0)), "{c:?}");
            let down = fade_points(c, true, 8);
            assert_eq!((down[0].1, down[8].1), (1.0, 0.0), "{c:?}");
        }
    }

    #[test]
    fn normalize_math() {
        let g = normalize_gain_db(0.5, -1.0).expect("gain");
        assert!((g - 5.0206).abs() < 1e-3);
        assert!(normalize_gain_db(0.0, -1.0).is_none());
        assert!(normalize_gain_db(f32::NAN, -1.0).is_none());
        // Spitze 1.0 -> -1 dB: das Signal wird leiser.
        assert!((normalize_gain_db(1.0, -1.0).expect("g") + 1.0).abs() < 1e-9);
    }

    #[test]
    fn region_peak_covers_the_clip_part_only() {
        // 4 Punkte je Beat; Spitze 0.9 liegt in Beat 2, 0.3 in Beat 0.
        let mut peaks = vec![0.1f32; 16];
        peaks[0] = 0.3;
        peaks[9] = 0.9;
        assert!((region_peak(&peaks, 4.0, 0.0, 1.0) - 0.3).abs() < 1e-6);
        assert!((region_peak(&peaks, 4.0, 2.0, 1.0) - 0.9).abs() < 1e-6);
        assert!((region_peak(&peaks, 4.0, 3.0, 1.0) - 0.1).abs() < 1e-6);
        assert_eq!(region_peak(&peaks, 4.0, 50.0, 1.0), 0.0, "beyond the end");
    }

    #[test]
    fn time_format() {
        assert_eq!(format_time(0.0, false), "0:00");
        assert_eq!(format_time(75.0, false), "1:15");
        assert_eq!(format_time(0.5, true), "0:00.5");
    }
}
