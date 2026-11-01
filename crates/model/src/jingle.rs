//! Assistent „Jingle bauen“ (Paket J4, Issue #261): Stimmungs-Vorlagen, Längen-Rechnung
//! und Schluss-Stab. Nur Daten und reine Funktionen; die Spuren entstehen über dieselben
//! Bausteine wie bei J2 (Drum-Raster), J3 (Akkordfolge), J5 (Stimme über Bett) und J6
//! (Ausgabe). Ergebnis ist ein normales Projekt mit normalen MIDI-Clips.

use crate::midi::{MidiData, MidiNote};

/// Längen zur Wahl, in Sekunden.
pub const LENGTHS: [u32; 3] = [5, 10, 30];

/// Eine Stimmung setzt Tempo, Tonart, Beat und Akkordfolge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mood {
    /// Name auf der Karte.
    pub name: &'static str,
    /// Klartext für Laien.
    pub hint: &'static str,
    /// Grundtempo in BPM; die Längen-Rechnung passt es auf ganze Takte an.
    pub bpm: u32,
    /// Tonart (Halbtöne über C, siehe [`crate::chords::KEYS`]).
    pub key: u8,
    /// Name eines [`crate::drums::DRUM_PRESETS`]-Eintrags.
    pub drums: &'static str,
    /// Name einer [`crate::chords::CHORD_PROGRESSIONS`]-Folge.
    pub progression: &'static str,
}

/// Die vier Stimmungen (Reihenfolge = Anzeige). Die Namen sind dieselben wie
/// [`crate::chords::ChordProgression::mood`].
pub const MOODS: [Mood; 4] = [
    Mood {
        name: "fröhlich",
        hint: "hell und freundlich, für Morgen und Gute Laune",
        bpm: 118,
        key: 0,
        drums: "Pop",
        progression: "I–V–vi–IV",
    },
    Mood {
        name: "seriös",
        hint: "groß und ruhig, für Sendungs-Idents",
        bpm: 92,
        key: 2,
        drums: "Halbes Tempo",
        progression: "i–VI–III–VII",
    },
    Mood {
        name: "Nachrichten",
        hint: "spannend und klar, für Nachrichten und Verkehr",
        bpm: 104,
        key: 9,
        drums: "Nachrichten",
        progression: "i–VII–VI–VII",
    },
    Mood {
        name: "Party",
        hint: "treibend, für Abend und Wochenende",
        bpm: 126,
        key: 5,
        drums: "Four on the Floor",
        progression: "i–VI–VII–i",
    },
];

/// Stimmung nach Namen (Groß-/Kleinschreibung egal).
pub fn mood(name: &str) -> Option<&'static Mood> {
    MOODS
        .iter()
        .find(|m| m.name.eq_ignore_ascii_case(name.trim()))
}

/// Tempo-Grenzen der Längen-Rechnung (BPM).
pub const MIN_BPM: f64 = 70.0;
pub const MAX_BPM: f64 = 160.0;

/// Längen-Rechnung: ganze 4/4-Takte, deren Dauer bei einem Tempo nahe `bpm` genau
/// `seconds` ergibt. Gibt (Takte, Tempo) zurück; das Tempo liegt in [`MIN_BPM`]..=[`MAX_BPM`].
pub fn fit(seconds: f64, bpm: f64) -> (u32, f64) {
    let seconds = seconds.clamp(1.0, 600.0);
    let tempo = |bars: u32| f64::from(bars) * 240.0 / seconds;
    let mut bars = (seconds * bpm.clamp(MIN_BPM, MAX_BPM) / 240.0)
        .round()
        .max(1.0) as u32;
    while bars > 1 && tempo(bars) > MAX_BPM {
        bars -= 1;
    }
    while tempo(bars) < MIN_BPM {
        bars += 1;
    }
    (bars, tempo(bars))
}

/// Dauer von `bars` Takten bei `bpm`, in Sekunden.
pub fn seconds(bars: u32, bpm: f64) -> f64 {
    f64::from(bars) * 240.0 / bpm.max(1.0)
}

/// Becken und Kick für den Schluss-Stab auf der Drum-Spur (GM-Drumnoten).
pub const STAB_DRUMS: [u8; 2] = [49, 36];

/// Länge des Schluss-Stabs in Beats.
pub const STAB_BEATS: f64 = 2.0;

/// Schluss-Stab: Der letzte von `bars` Takten wird ein einzelner Schlag, damit das Jingle
/// nicht abgeschnitten klingt. Was im letzten Takt lag, fällt weg; Noten aus dem Takt davor
/// enden an der Taktgrenze. Der Schlag sind die Töne vom Anfang des Clips (Grundakkord,
/// Grundton) plus `extra` (etwa [`STAB_DRUMS`]), laut und [`STAB_BEATS`] lang.
/// Bei einem Takt bleibt der Clip unverändert.
pub fn stab(midi: &MidiData, bars: u32, extra: &[u8]) -> MidiData {
    if bars < 2 {
        return midi.clone();
    }
    let last = f64::from(bars - 1) * 4.0;
    let mut out = midi.clone();
    out.notes.retain(|n| n.beat < last - 1e-9);
    for n in &mut out.notes {
        n.length_beats = n.length_beats.min(last - n.beat);
    }
    let mut pitches: Vec<u8> = midi
        .notes
        .iter()
        .filter(|n| n.beat < 1e-9)
        .map(|n| n.pitch)
        .chain(extra.iter().copied())
        .collect();
    pitches.sort_unstable();
    pitches.dedup();
    for pitch in pitches {
        let velocity = midi
            .notes
            .iter()
            .filter(|n| n.pitch == pitch)
            .map(|n| n.velocity)
            .max()
            .unwrap_or(100)
            .max(110);
        out.notes.push(MidiNote {
            beat: last,
            length_beats: STAB_BEATS,
            pitch,
            velocity,
        });
    }
    out.notes
        .sort_by(|a, b| a.beat.total_cmp(&b.beat).then(a.pitch.cmp(&b.pitch)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{chords, drums};

    #[test]
    fn every_mood_points_to_existing_presets() {
        for m in &MOODS {
            assert!(drums::drum_preset(m.drums).is_some(), "{}", m.drums);
            let p = chords::progression(m.progression).expect(m.progression);
            assert_eq!(p.mood, m.name, "Folge passt zur Stimmung");
            assert!(m.key < 12);
        }
        assert_eq!(mood("party").map(|m| m.bpm), Some(126));
        assert!(mood("Schlager").is_none());
    }

    #[test]
    fn fit_hits_the_length_exactly_on_whole_bars() {
        for m in &MOODS {
            for s in LENGTHS {
                let (bars, bpm) = fit(f64::from(s), f64::from(m.bpm));
                assert!(bars >= 1);
                assert!(
                    (MIN_BPM..=MAX_BPM).contains(&bpm),
                    "{s} s {}: {bpm}",
                    m.name
                );
                assert!((seconds(bars, bpm) - f64::from(s)).abs() < 1e-9);
            }
        }
        // 10 s bei 120 BPM sind genau 5 Takte.
        assert_eq!(fit(10.0, 120.0), (5, 120.0));
        // 5 s bei 92 BPM: 2 Takte bei 96 BPM.
        assert_eq!(fit(5.0, 92.0), (2, 96.0));
    }

    #[test]
    fn stab_turns_the_last_bar_into_one_hit() {
        let p = chords::progression("I–V–vi–IV").unwrap();
        let made = chords::generate(p, 0, 4);
        let s = stab(&made.chords, 4, &[]);
        let last: Vec<_> = s.notes.iter().filter(|n| n.beat >= 12.0).collect();
        let mut pitches: Vec<u8> = last.iter().map(|n| n.pitch).collect();
        pitches.sort_unstable();
        assert_eq!(pitches, vec![60, 64, 67], "Grundakkord C-Dur");
        assert!(
            last.iter()
                .all(|n| n.beat == 12.0 && n.length_beats == STAB_BEATS)
        );
        assert!(last.iter().all(|n| n.velocity >= 110));
        assert!(
            s.notes
                .iter()
                .all(|n| n.beat + n.length_beats <= 14.0 + 1e-9)
        );

        let arp = stab(&made.arp, 4, &[]);
        assert_eq!(arp.notes.iter().filter(|n| n.beat >= 12.0).count(), 1);

        let kit = drums::apply_preset(
            &MidiData::default(),
            16.0,
            drums::drum_preset("Nachrichten").unwrap(),
            0,
        );
        let d = stab(&kit, 4, &STAB_DRUMS);
        let hit: Vec<u8> = d
            .notes
            .iter()
            .filter(|n| n.beat >= 12.0)
            .map(|n| n.pitch)
            .collect();
        assert!(hit.contains(&49) && hit.contains(&36), "{hit:?}");
        assert!(
            d.notes
                .iter()
                .filter(|n| n.beat >= 12.0)
                .all(|n| n.beat == 12.0)
        );
        // Zweimal anwenden ändert nichts mehr.
        assert_eq!(stab(&d, 4, &STAB_DRUMS), d);
    }

    #[test]
    fn stab_needs_two_bars() {
        let p = chords::progression("I–IV").unwrap();
        let one = chords::generate(p, 0, 1).chords;
        assert_eq!(stab(&one, 1, &STAB_DRUMS), one);
    }
}
