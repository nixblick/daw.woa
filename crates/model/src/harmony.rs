//! Harmonie sichtbar machen (Design System 2, `ChordTrack`): Akkord je Takt und Tonart aus
//! den Noten erkennen.
//!
//! Das Projekt speichert keine Tonart; sie folgt aus den Noten. So stimmt die Anzeige auch
//! nach einem Tonartwechsel (Transponieren), für Clips vom Agenten und für eingespielte Noten.
//! Reine Funktionen, keine Allokation im Audio-Pfad (die Oberfläche ruft sie auf).

use crate::chords::KEYS;
use crate::midi::MidiNote;

/// Ein erkannter Dreiklang.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chord {
    /// Grundton als Halbtöne über C (0..12).
    pub root: u8,
    /// Kleine Terz.
    pub minor: bool,
    /// Verminderte Quinte (nur mit `minor`).
    pub dim: bool,
}

/// Eine Tonart (Dur oder natürliches Moll).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Key {
    /// Grundton als Halbtöne über C (0..12).
    pub tonic: u8,
    /// Moll statt Dur.
    pub minor: bool,
}

const MAJOR: [u8; 7] = [0, 2, 4, 5, 7, 9, 11];
const MINOR: [u8; 7] = [0, 2, 3, 5, 7, 8, 10];

impl Chord {
    /// Name wie im Browser: „C“, „Am“, „B°“.
    pub fn name(&self) -> String {
        let n = KEYS[usize::from(self.root % 12)];
        match (self.minor, self.dim) {
            (_, true) => format!("{n}°"),
            (true, false) => format!("{n}m"),
            (false, false) => n.to_owned(),
        }
    }

    /// Stufe in `key` als römische Zahl (Dur groß, Moll klein): „I“, „V“, „vi“, „vii°“.
    /// `None`, wenn der Akkord nicht leitereigen ist.
    pub fn degree(&self, key: Key) -> Option<&'static str> {
        const UP: [&str; 7] = ["I", "II", "III", "IV", "V", "VI", "VII"];
        const LOW: [&str; 7] = ["i", "ii", "iii", "iv", "v", "vi", "vii"];
        const DIM: [&str; 7] = ["i°", "ii°", "iii°", "iv°", "v°", "vi°", "vii°"];
        let scale = key.scale();
        let rel = (self.root + 12 - key.tonic % 12) % 12;
        let i = scale.iter().position(|&s| s == rel)?;
        let third = (scale[(i + 2) % 7] + 12 - rel) % 12;
        let fifth = (scale[(i + 4) % 7] + 12 - rel) % 12;
        let fits = (third == 3) == self.minor && (fifth == 6) == self.dim;
        fits.then(|| match (self.minor, self.dim) {
            (_, true) => DIM[i],
            (true, false) => LOW[i],
            (false, false) => UP[i],
        })
    }
}

impl Key {
    /// „C Dur“, „A Moll“.
    pub fn name(&self) -> String {
        let mode = if self.minor { "Moll" } else { "Dur" };
        format!("{} {mode}", KEYS[usize::from(self.tonic % 12)])
    }

    /// Gehört die Tonhöhe zur Leiter?
    pub fn contains(&self, pitch: u8) -> bool {
        let rel = (pitch % 12 + 12 - self.tonic % 12) % 12;
        self.scale().contains(&rel)
    }

    fn scale(&self) -> &'static [u8; 7] {
        if self.minor { &MINOR } else { &MAJOR }
    }
}

/// Dreiklang aus den Tonklassen (Bitmaske, Bit 0 = C). Passen mehrere, gewinnt der mit dem
/// Grundton `bass` (tiefster Ton), sonst der erste ab C.
pub fn detect_chord(classes: u16, bass: Option<u8>) -> Option<Chord> {
    let has = |pc: u8| classes & (1 << (pc % 12)) != 0;
    let mut first = None;
    for root in 0..12u8 {
        if !has(root) {
            continue;
        }
        let chord = if has(root + 4) && has(root + 7) {
            Chord {
                root,
                minor: false,
                dim: false,
            }
        } else if has(root + 3) && has(root + 7) {
            Chord {
                root,
                minor: true,
                dim: false,
            }
        } else if has(root + 3) && has(root + 6) {
            Chord {
                root,
                minor: true,
                dim: true,
            }
        } else {
            continue;
        };
        if bass.is_some_and(|b| b % 12 == root) {
            return Some(chord);
        }
        first = first.or(Some(chord));
    }
    first
}

/// Akkord je 4/4-Takt aus Noten mit absoluter Lage in Beats (`(beat, &note)`), für `bars`
/// Takte ab Beat 0. Ein Takt zählt die Töne, die in seinem ersten Schlag klingen.
pub fn bar_chords<'a>(
    notes: impl IntoIterator<Item = (f64, &'a MidiNote)>,
    bars: usize,
) -> Vec<Option<Chord>> {
    let mut classes = vec![0u16; bars];
    let mut bass: Vec<Option<u8>> = vec![None; bars];
    for (beat, n) in notes {
        let end = beat + n.length_beats;
        let first = (beat / 4.0).floor().max(0.0) as usize;
        let last = ((end - 1e-9) / 4.0).floor().max(0.0) as usize;
        for bar in first..=last {
            let b0 = bar as f64 * 4.0;
            // Klingt im ersten Schlag des Takts?
            if beat < b0 + 1.0
                && end > b0
                && let (Some(c), Some(b)) = (classes.get_mut(bar), bass.get_mut(bar))
            {
                *c |= 1 << (n.pitch % 12);
                *b = Some(b.map_or(n.pitch, |low| low.min(n.pitch)));
            }
        }
    }
    classes
        .into_iter()
        .zip(bass)
        .map(|(c, b)| detect_chord(c, b))
        .collect()
}

/// Ist das ein Akkord-Clip (mindestens drei Töne setzen gleichzeitig ein)?
pub fn is_chord_clip(notes: &[MidiNote]) -> bool {
    notes.iter().any(|n| {
        notes
            .iter()
            .filter(|m| (m.beat - n.beat).abs() < 1e-6)
            .count()
            >= 3
    })
}

/// Tonart nach Krumhansl-Schmuckler (Tondauer je Tonklasse gegen Dur- und Moll-Profil).
/// `None` ohne Noten.
pub fn detect_key<'a>(notes: impl IntoIterator<Item = &'a MidiNote>) -> Option<Key> {
    const MAJ: [f64; 12] = [
        6.35, 2.23, 3.48, 2.33, 4.38, 4.09, 2.52, 5.19, 2.39, 3.66, 2.29, 2.88,
    ];
    const MIN: [f64; 12] = [
        6.33, 2.68, 3.52, 5.38, 2.60, 3.53, 2.54, 4.75, 3.98, 2.69, 3.34, 3.17,
    ];
    let mut w = [0.0f64; 12];
    for n in notes {
        w[usize::from(n.pitch % 12)] += n.length_beats.max(0.0);
    }
    if w.iter().all(|&x| x <= 0.0) {
        return None;
    }
    let corr = |profile: &[f64; 12], tonic: usize| {
        let mp = profile.iter().sum::<f64>() / 12.0;
        let mw = w.iter().sum::<f64>() / 12.0;
        let (mut num, mut dp, mut dw) = (0.0, 0.0, 0.0);
        for i in 0..12 {
            let a = profile[(i + 12 - tonic) % 12] - mp;
            let b = w[i] - mw;
            num += a * b;
            dp += a * a;
            dw += b * b;
        }
        if dp * dw > 0.0 {
            num / (dp * dw).sqrt()
        } else {
            0.0
        }
    };
    let mut best = (
        f64::MIN,
        Key {
            tonic: 0,
            minor: false,
        },
    );
    for tonic in 0..12 {
        for (minor, profile) in [(false, &MAJ), (true, &MIN)] {
            let r = corr(profile, tonic);
            if r > best.0 + 1e-12 {
                best = (
                    r,
                    Key {
                        tonic: tonic as u8,
                        minor,
                    },
                );
            }
        }
    }
    Some(best.1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chords::{CHORD_PROGRESSIONS, generate, progression};

    fn mask(pcs: &[u8]) -> u16 {
        pcs.iter().fold(0, |m, p| m | 1 << (p % 12))
    }

    #[test]
    fn triads_are_named() {
        let c = detect_chord(mask(&[0, 4, 7]), Some(60)).unwrap();
        assert_eq!(c.name(), "C");
        let am = detect_chord(mask(&[9, 0, 4]), Some(57)).unwrap();
        assert_eq!(am.name(), "Am");
        assert_eq!(detect_chord(mask(&[11, 2, 5]), None).unwrap().name(), "B°");
        assert_eq!(detect_chord(mask(&[0, 2]), None), None);
        // Am7 = A C E G: der Bass A entscheidet.
        assert_eq!(
            detect_chord(mask(&[9, 0, 4, 7]), Some(45)).unwrap().name(),
            "Am"
        );
    }

    #[test]
    fn degrees_and_keys() {
        let c_dur = Key {
            tonic: 0,
            minor: false,
        };
        assert_eq!(c_dur.name(), "C Dur");
        assert_eq!(
            Key {
                tonic: 9,
                minor: true
            }
            .name(),
            "A Moll"
        );
        let ch = |root, minor| Chord {
            root,
            minor,
            dim: false,
        };
        assert_eq!(ch(0, false).degree(c_dur), Some("I"));
        assert_eq!(ch(7, false).degree(c_dur), Some("V"));
        assert_eq!(ch(9, true).degree(c_dur), Some("vi"));
        assert_eq!(ch(5, false).degree(c_dur), Some("IV"));
        assert_eq!(ch(1, false).degree(c_dur), None);
        assert!(c_dur.contains(64) && !c_dur.contains(61));
    }

    #[test]
    fn generated_progression_is_recognised_in_every_key() {
        let p = progression("I–V–vi–IV").unwrap_or(&CHORD_PROGRESSIONS[0]);
        for key in 0..12u8 {
            let made = generate(p, key, 4);
            let all: Vec<(f64, &MidiNote)> = made
                .chords
                .notes
                .iter()
                .chain(&made.bass.notes)
                .chain(&made.arp.notes)
                .map(|n| (n.beat, n))
                .collect();
            let names: Vec<String> = bar_chords(all.iter().copied(), 4)
                .into_iter()
                .map(|c| c.map(|c| c.name()).unwrap_or_default())
                .collect();
            assert_eq!(names, crate::chords::chord_names(p, key), "key {key}");
            let k = detect_key(all.iter().map(|(_, n)| *n)).unwrap();
            assert_eq!(
                k,
                Key {
                    tonic: key,
                    minor: p.minor
                },
                "key {key}"
            );
            assert!(is_chord_clip(&made.chords.notes));
            assert!(!is_chord_clip(&made.arp.notes));
            assert!(!is_chord_clip(&made.bass.notes));
        }
        assert_eq!(bar_chords(std::iter::empty(), 2), vec![None, None]);
        let one = MidiNote {
            beat: 0.0,
            length_beats: 64.0,
            pitch: 60,
            velocity: 90,
        };
        assert!(bar_chords([(0.0, &one)], 0).is_empty(), "null Takte");
        assert_eq!(detect_key(std::iter::empty()), None);
    }
}
