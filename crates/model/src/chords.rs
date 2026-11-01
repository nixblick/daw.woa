//! Akkordfolgen (Paket J3, Issue #261): Katalog als Daten, daraus drei MIDI-Clips.
//!
//! Eine Folge ist eine Liste von Stufen der Tonleiter (1 bis 7), ein Akkord je 4/4-Takt.
//! Daraus entstehen normale [`MidiData`] für Akkorde (Dreiklänge), Bass (Grundton und
//! Oktave) und Arpeggio (Achtel aufwärts). Alle Töne sind leitereigen, keiner klingt
//! falsch. Oberfläche und MCP (`generate_chords`) nutzen denselben Katalog.

use crate::midi::{MidiData, MidiNote};

/// Tonarten (Grundton als Halbtöne über C).
pub const KEYS: [&str; 12] = [
    "C", "C#", "D", "Eb", "E", "F", "F#", "G", "Ab", "A", "Bb", "B",
];

const MAJOR: [u8; 7] = [0, 2, 4, 5, 7, 9, 11];
const MINOR: [u8; 7] = [0, 2, 3, 5, 7, 8, 10];

/// Eine Akkordfolge des Katalogs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChordProgression {
    /// Stufen-Schreibweise, z. B. „I–V–vi–IV“.
    pub name: &'static str,
    /// Klartext für Laien.
    pub hint: &'static str,
    /// Stimmung (fröhlich, seriös, Nachrichten, Party), für Vorlagen.
    pub mood: &'static str,
    /// `true`: natürliches Moll, sonst Dur.
    pub minor: bool,
    /// Stufen 1..=7, ein Akkord je Takt.
    pub degrees: &'static [u8],
}

const fn dur(
    name: &'static str,
    hint: &'static str,
    mood: &'static str,
    degrees: &'static [u8],
) -> ChordProgression {
    ChordProgression {
        name,
        hint,
        mood,
        minor: false,
        degrees,
    }
}

const fn moll(
    name: &'static str,
    hint: &'static str,
    mood: &'static str,
    degrees: &'static [u8],
) -> ChordProgression {
    ChordProgression {
        name,
        hint,
        mood,
        minor: true,
        degrees,
    }
}

/// Der Katalog (Reihenfolge = Anzeige).
pub const CHORD_PROGRESSIONS: &[ChordProgression] = &[
    dur(
        "I–V–vi–IV",
        "hymnisch, der Pop-Klassiker",
        "fröhlich",
        &[1, 5, 6, 4],
    ),
    dur("I–IV–V–I", "klar und fröhlich", "fröhlich", &[1, 4, 5, 1]),
    dur("I–vi–IV–V", "50er, freundlich", "fröhlich", &[1, 6, 4, 5]),
    dur("vi–IV–I–V", "gefühlvoll", "seriös", &[6, 4, 1, 5]),
    dur("I–IV–vi–V", "aufbauend, nach vorn", "seriös", &[1, 4, 6, 5]),
    dur("I–V–IV–V", "Rock, geradeaus", "Party", &[1, 5, 4, 5]),
    dur("I–IV", "schwebend, ruhig", "Nachrichten", &[1, 4]),
    moll("i–VI–III–VII", "episch, groß", "seriös", &[1, 6, 3, 7]),
    moll("i–iv–v–i", "ernst, dunkel", "Nachrichten", &[1, 4, 5, 1]),
    moll(
        "i–VII–VI–VII",
        "spannend, treibend",
        "Nachrichten",
        &[1, 7, 6, 7],
    ),
    moll("i–VI–VII–i", "Club, Party", "Party", &[1, 6, 7, 1]),
];

/// Die drei erzeugten Clips.
#[derive(Debug, Clone, PartialEq)]
pub struct ChordClips {
    pub chords: MidiData,
    pub bass: MidiData,
    pub arp: MidiData,
}

fn norm(s: &str) -> String {
    s.trim().replace(['-', '—', ' '], "–")
}

/// Folge nach Namen: Bindestrich statt Halbgeviertstrich geht, Groß-/Kleinschreibung
/// zählt nur, wenn sie zwei Folgen unterscheidet.
pub fn progression(name: &str) -> Option<&'static ChordProgression> {
    let n = norm(name);
    CHORD_PROGRESSIONS.iter().find(|p| p.name == n).or_else(|| {
        CHORD_PROGRESSIONS
            .iter()
            .find(|p| p.name.to_lowercase() == n.to_lowercase())
    })
}

/// Tonart nach Namen („C“, „F#“, „Bb“, auch „Db“ oder „A#“), Halbtöne über C.
pub fn key_index(name: &str) -> Option<u8> {
    let n = name.trim();
    let alias = match n {
        "Db" => "C#",
        "D#" => "Eb",
        "Gb" => "F#",
        "G#" => "Ab",
        "A#" => "Bb",
        other => other,
    };
    KEYS.iter()
        .position(|k| k.eq_ignore_ascii_case(alias))
        .map(|i| i as u8)
}

/// Halbtöne der Leiterstufe `i` (0-basiert, darf über 6 gehen).
fn semis(scale: &[u8; 7], i: usize) -> u8 {
    scale[i % 7] + 12 * (i / 7) as u8
}

fn note(beat: f64, length_beats: f64, pitch: u8, velocity: u8) -> MidiNote {
    MidiNote {
        beat,
        length_beats,
        pitch,
        velocity,
    }
}

/// Erzeugt Akkorde, Bass und Arpeggio über `bars` Takte in `key` (0 = C); die Folge
/// wiederholt sich, wenn sie kürzer ist.
pub fn generate(p: &ChordProgression, key: u8, bars: u32) -> ChordClips {
    let scale = if p.minor { &MINOR } else { &MAJOR };
    let key = key % 12;
    let mut out = ChordClips {
        chords: MidiData::default(),
        bass: MidiData::default(),
        arp: MidiData::default(),
    };
    if p.degrees.is_empty() {
        return out;
    }
    for bar in 0..bars.clamp(1, 64) as usize {
        let d = usize::from(p.degrees[bar % p.degrees.len()].clamp(1, 7) - 1);
        let r = semis(scale, d);
        // Grundton im Bereich G3..F#4 um das mittlere C, relativ zur Tonart.
        let base = 60 + key + r - if r >= 5 { 12 } else { 0 };
        let triad = [
            base,
            base + semis(scale, d + 2) - r,
            base + semis(scale, d + 4) - r,
        ];
        let t = bar as f64 * 4.0;
        for &pitch in &triad {
            out.chords.notes.push(note(t, 4.0, pitch, 80));
        }
        // Zwei Oktaven unter dem Akkord-Grundton, die Oktave bleibt unter dem Akkord.
        let low = base - 24;
        for (b, len, pitch) in [
            (0.0, 1.0, low),
            (1.5, 0.5, low),
            (2.0, 1.0, low),
            (3.0, 0.5, low + 12),
        ] {
            out.bass.notes.push(note(t + b, len, pitch, 100));
        }
        let up = [triad[0] + 12, triad[1] + 12, triad[2] + 12, triad[0] + 24];
        for i in 0..8 {
            out.arp
                .notes
                .push(note(t + i as f64 * 0.5, 0.5, up[i % 4], 90));
        }
    }
    out
}

/// Kürzester Weg von Tonart `from` nach `to` in Halbtönen (-5..=6).
pub fn key_shift(from: u8, to: u8) -> i8 {
    let d = (i16::from(to % 12) - i16::from(from % 12)).rem_euclid(12);
    (if d > 6 { d - 12 } else { d }) as i8
}

/// Alle Noten um `semitones` verschoben, begrenzt auf 0..=127.
pub fn transpose(midi: &MidiData, semitones: i8) -> MidiData {
    let mut out = midi.clone();
    for n in &mut out.notes {
        n.pitch = (i16::from(n.pitch) + i16::from(semitones)).clamp(0, 127) as u8;
    }
    out
}
