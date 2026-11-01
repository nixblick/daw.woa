//! Drum-Raster (Paket J2, Issue #261): 16tel-Schritte auf GM-Drumnoten.
//!
//! Das Raster ist nur eine Sicht auf normale MIDI-Noten: Eine Zeile ist eine GM-Drumnote,
//! ein Schritt eine 16tel. Alles hier liefert neue [`MidiData`]; Noten außerhalb der
//! Raster-Zeilen oder zwischen den Schritten bleiben unverändert erhalten. Oberfläche
//! und MCP (`generate_drums` mit `preset`) nutzen dieselben Presets.

use crate::midi::{MidiData, MidiNote};

/// Velocity eines lauten Schritts (`x`).
pub const LOUD: u8 = 110;
/// Velocity eines leisen Schritts (`o`).
pub const SOFT: u8 = 60;
/// Schritte je 4/4-Takt.
pub const STEPS_PER_BAR: usize = 16;

/// Eine Zeile des Rasters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DrumRow {
    /// Anzeigename.
    pub name: &'static str,
    /// GM-Drumnote (Kanal 10).
    pub pitch: u8,
}

/// Die Zeilen von oben nach unten.
pub const DRUM_ROWS: [DrumRow; 6] = [
    DrumRow {
        name: "Becken",
        pitch: 49,
    },
    DrumRow {
        name: "Hi-Hat offen",
        pitch: 46,
    },
    DrumRow {
        name: "Hi-Hat zu",
        pitch: 42,
    },
    DrumRow {
        name: "Clap",
        pitch: 39,
    },
    DrumRow {
        name: "Snare",
        pitch: 38,
    },
    DrumRow {
        name: "Kick",
        pitch: 36,
    },
];

/// Ein Starter-Muster: ein Takt je Zeile in [`DRUM_ROWS`]-Reihenfolge, `x` laut, `o` leise.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DrumPreset {
    /// Name für Oberfläche und MCP.
    pub name: &'static str,
    /// Becken, Hi-Hat offen, Hi-Hat zu, Clap, Snare, Kick.
    pub rows: [&'static str; 6],
}

const fn p(name: &'static str, rows: [&'static str; 6]) -> DrumPreset {
    DrumPreset { name, rows }
}

/// Die Presets (Reihenfolge = Anzeige).
pub const DRUM_PRESETS: &[DrumPreset] = &[
    p(
        "Pop",
        [
            "x...............",
            "................",
            "x.x.x.x.x.x.x.x.",
            "................",
            "....x.......x...",
            "x.......x.x.....",
        ],
    ),
    p(
        "Four on the Floor",
        [
            "x...............",
            "..x...x...x...x.",
            "x.o.x.o.x.o.x.o.",
            "....x.......x...",
            "................",
            "x...x...x...x...",
        ],
    ),
    p(
        "Rock",
        [
            "x...............",
            "................",
            "x.x.x.x.x.x.x.x.",
            "................",
            "....x.......x...",
            "x.....x.x.......",
        ],
    ),
    p(
        "Hip-Hop",
        [
            "................",
            "..............x.",
            "x.x.x.x.x.x.x...",
            "................",
            "....x.......x...",
            "x......x..x.....",
        ],
    ),
    p(
        "Nachrichten",
        [
            "x...............",
            "................",
            "xoxoxoxoxoxoxoxo",
            "................",
            "....o.......o...",
            "x.......x.......",
        ],
    ),
    p(
        "Party",
        [
            "x...............",
            "..x...x...x...x.",
            "xxxxxxxxxxxxxxxx",
            "....x.......x...",
            "....x.......x...",
            "x...x...x...x...",
        ],
    ),
    p(
        "Halbes Tempo",
        [
            "x...............",
            "................",
            "x...x...x...x...",
            "................",
            "........x.......",
            "x.........x.....",
        ],
    ),
    p(
        "Shuffle",
        [
            "x...............",
            "................",
            "x..xx..xx..xx..x",
            "................",
            "....x.......x...",
            "x......xx.......",
        ],
    ),
];

/// Preset nach Namen (Groß-/Kleinschreibung egal).
pub fn drum_preset(name: &str) -> Option<&'static DrumPreset> {
    DRUM_PRESETS
        .iter()
        .find(|p| p.name.eq_ignore_ascii_case(name.trim()))
}

/// Anzahl der 16tel-Schritte eines Clips von `length_beats`, mindestens einer.
pub fn steps(length_beats: f64) -> usize {
    if length_beats.is_finite() && length_beats > 0.0 {
        ((length_beats * 4.0).round() as usize).max(1)
    } else {
        1
    }
}

/// Index der Note auf (`pitch`, `step`), falls es eine gibt.
fn find(midi: &MidiData, pitch: u8, step: usize) -> Option<usize> {
    let beat = step as f64 / 4.0;
    midi.notes
        .iter()
        .position(|n| n.pitch == pitch && (n.beat - beat).abs() < 1e-6)
}

fn on_grid(n: &MidiNote, end_step: usize) -> bool {
    let s = n.beat * 4.0;
    DRUM_ROWS.iter().any(|r| r.pitch == n.pitch)
        && (s - s.round()).abs() < 1e-6
        && (s.round() as usize) < end_step
}

fn note(pitch: u8, step: usize, velocity: u8) -> MidiNote {
    MidiNote {
        beat: step as f64 / 4.0,
        length_beats: 0.25,
        pitch,
        velocity,
    }
}

fn sorted(mut midi: MidiData) -> MidiData {
    midi.notes.sort_by(|a, b| a.beat.total_cmp(&b.beat));
    midi
}

/// Velocity des Schritts, `None` wenn er leer ist.
pub fn step_velocity(midi: &MidiData, pitch: u8, step: usize) -> Option<u8> {
    find(midi, pitch, step)
        .and_then(|i| midi.notes.get(i))
        .map(|n| n.velocity)
}

/// Schaltet einen Schritt. Klick (`soft = false`): leer wird laut, belegt wird leer.
/// Rechtsklick (`soft = true`): leer wird leise, laut wird leise, leise wird leer.
pub fn toggle_step(midi: &MidiData, pitch: u8, step: usize, soft: bool) -> MidiData {
    let mut out = midi.clone();
    match find(midi, pitch, step) {
        Some(i) => {
            let loud = out.notes.get(i).is_some_and(|n| n.velocity > SOFT);
            if soft && loud {
                if let Some(n) = out.notes.get_mut(i) {
                    n.velocity = SOFT;
                }
            } else {
                out.notes.remove(i);
            }
        }
        None => out
            .notes
            .push(note(pitch, step, if soft { SOFT } else { LOUD })),
    }
    sorted(out)
}

/// Muster eines Presets mit Dichte: `-1` weniger (nur laute Schläge, Hi-Hat nur auf
/// Achteln), `0` wie das Preset, `+1` mehr (Hi-Hat zu auf jeder 16tel, leise ergänzt).
pub fn preset_rows(preset: &DrumPreset, density: i8) -> [Vec<Option<u8>>; 6] {
    let mut rows: [Vec<Option<u8>>; 6] = Default::default();
    for (r, pattern) in preset.rows.iter().enumerate() {
        let hat = DRUM_ROWS[r].pitch == 42;
        let Some(row) = rows.get_mut(r) else { continue };
        for (i, c) in pattern.chars().enumerate() {
            let mut v = match c {
                'x' | 'X' => Some(LOUD),
                'o' | 'O' => Some(SOFT),
                _ => None,
            };
            if density < 0 && (v == Some(SOFT) || (hat && i % 2 == 1)) {
                v = None;
            }
            if density > 0 && hat && v.is_none() {
                v = Some(SOFT);
            }
            row.push(v);
        }
    }
    rows
}

fn fill(midi: &MidiData, length_beats: f64, rows: &[Vec<Option<u8>>; 6]) -> MidiData {
    let total = steps(length_beats);
    let mut out = midi.clone();
    out.notes.retain(|n| !on_grid(n, total));
    for (r, row) in rows.iter().enumerate() {
        if row.is_empty() {
            continue;
        }
        for step in 0..total {
            if let Some(Some(v)) = row.get(step % row.len()) {
                out.notes.push(note(DRUM_ROWS[r].pitch, step, *v));
            }
        }
    }
    sorted(out)
}

/// Ersetzt die Raster-Schritte des Clips durch das Preset (wiederholt über die ganze
/// Länge). Noten außerhalb des Rasters bleiben.
pub fn apply_preset(
    midi: &MidiData,
    length_beats: f64,
    preset: &DrumPreset,
    density: i8,
) -> MidiData {
    fill(midi, length_beats, &preset_rows(preset, density))
}

/// Würfel: ein Preset nach `seed`, dazu einige zufällige Hi-Hat- und Kick-Schläge.
/// Gleicher `seed`, gleiches Ergebnis.
pub fn dice(midi: &MidiData, length_beats: f64, seed: u64) -> MidiData {
    let mut x = seed ^ 0x9E37_79B9_7F4A_7C15;
    let mut next = || {
        // xorshift64
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x
    };
    let pick = (next() % DRUM_PRESETS.len() as u64) as usize;
    let preset = DRUM_PRESETS.get(pick).unwrap_or(&DRUM_PRESETS[0]);
    let density = (next() % 3) as i8 - 1;
    let mut rows = preset_rows(preset, density);
    for (r, row) in rows.iter_mut().enumerate() {
        let pitch = DRUM_ROWS[r].pitch;
        if pitch != 42 && pitch != 36 {
            continue;
        }
        for (i, v) in row.iter_mut().enumerate() {
            // Kick nie auf die Zählzeiten der Snare, damit es nicht matscht.
            if next() % 8 == 0 && !(pitch == 36 && (i == 4 || i == 12)) {
                *v = match v {
                    Some(_) if i != 0 => None,
                    Some(s) => Some(*s),
                    None => Some(SOFT),
                };
            }
        }
    }
    fill(midi, length_beats, &rows)
}
