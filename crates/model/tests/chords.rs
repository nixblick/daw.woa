//! Paket J3: Akkordfolgen als Daten, daraus Akkord-, Bass- und Arpeggio-Clips.

use woa_model::chords::{CHORD_PROGRESSIONS, KEYS, generate, key_shift, progression, transpose};

fn pcs(m: &woa_model::MidiData) -> Vec<u8> {
    m.notes.iter().map(|n| n.pitch % 12).collect()
}

#[test]
fn catalog_is_data_with_known_names() {
    assert_eq!(KEYS.len(), 12);
    assert!(CHORD_PROGRESSIONS.len() >= 8);
    for p in CHORD_PROGRESSIONS {
        assert!(!p.degrees.is_empty(), "{}", p.name);
        assert!(p.degrees.iter().all(|d| (1..=7).contains(d)), "{}", p.name);
        assert!(!p.hint.is_empty());
    }
    assert!(progression("i-v-vi-iv").is_some());
    assert!(progression("gibt es nicht").is_none());
}

#[test]
fn pop_progression_in_c_gives_three_clips_in_key() {
    let p = progression("I–V–vi–IV").expect("pop");
    let c = generate(p, 0, 4);
    // Akkorde: ein Dreiklang je Takt.
    let bar = |m: &woa_model::MidiData, b: f64| {
        let mut v: Vec<u8> = m
            .notes
            .iter()
            .filter(|n| (n.beat - b * 4.0).abs() < 1e-6)
            .map(|n| n.pitch)
            .collect();
        v.sort_unstable();
        v
    };
    assert_eq!(bar(&c.chords, 0.0), [60, 64, 67]);
    assert_eq!(bar(&c.chords, 1.0), [55, 59, 62]);
    assert_eq!(bar(&c.chords, 2.0), [57, 60, 64]);
    assert_eq!(bar(&c.chords, 3.0), [53, 57, 60]);
    // Bass: Grundton je Takt, tief.
    let roots: Vec<u8> = [0.0, 1.0, 2.0, 3.0]
        .iter()
        .map(|b| bar(&c.bass, *b)[0] % 12)
        .collect();
    assert_eq!(roots, [0, 7, 9, 5]);
    assert!(c.bass.notes.iter().all(|n| n.pitch < 55));
    // Arpeggio: mehr Noten als Akkorde, alle über dem Bass.
    assert!(c.arp.notes.len() > c.chords.notes.len());
    // Alles leitereigen (C-Dur), nichts klingt falsch.
    let scale = [0, 2, 4, 5, 7, 9, 11];
    for m in [&c.chords, &c.bass, &c.arp] {
        assert!(pcs(m).iter().all(|p| scale.contains(p)));
        assert!(
            m.notes
                .iter()
                .all(|n| n.beat + n.length_beats <= 16.0 + 1e-9)
        );
    }
}

#[test]
fn short_clip_cycles_and_minor_uses_minor_scale() {
    let p = progression("i–VI–III–VII").expect("moll");
    let c = generate(p, 9, 2); // a-Moll, zwei Takte
    assert!(c.chords.notes.iter().all(|n| n.beat < 8.0));
    let a_minor = [9, 11, 0, 2, 4, 5, 7];
    assert!(pcs(&c.chords).iter().all(|p| a_minor.contains(p)));
    assert_eq!(c.bass.notes[0].pitch % 12, 9);
}

#[test]
fn key_change_transposes_all_three() {
    let p = progression("I–V–vi–IV").expect("pop");
    let c = generate(p, 0, 4);
    let d = generate(p, 2, 4);
    assert_eq!(key_shift(0, 2), 2);
    assert_eq!(key_shift(0, 11), -1);
    assert_eq!(key_shift(11, 0), 1);
    assert_eq!(transpose(&c.chords, 2), d.chords);
    assert_eq!(transpose(&c.bass, 2), d.bass);
    assert_eq!(transpose(&c.arp, 2), d.arp);
    let up = transpose(&c.chords, 120);
    assert!(up.notes.iter().all(|n| n.pitch <= 127));
}
