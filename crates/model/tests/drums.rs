//! Paket J2: Drum-Raster als Sicht auf normale MIDI-Noten.

use woa_model::drums::{LOUD, SOFT, apply_preset, dice, step_velocity, steps, toggle_step};
use woa_model::{DRUM_PRESETS, DRUM_ROWS, MidiData, MidiNote, drum_preset};

fn empty() -> MidiData {
    MidiData::default()
}

#[test]
fn rows_are_gm_drum_notes_and_presets_fit_one_bar() {
    let pitches: Vec<u8> = DRUM_ROWS.iter().map(|r| r.pitch).collect();
    assert_eq!(pitches, [49, 46, 42, 39, 38, 36]);
    assert!(DRUM_PRESETS.len() >= 6);
    for p in DRUM_PRESETS {
        for r in p.rows {
            assert_eq!(r.chars().count(), 16, "{}", p.name);
        }
    }
    assert_eq!(drum_preset("pop").map(|p| p.name), Some("Pop"));
    assert!(drum_preset("gibt es nicht").is_none());
}

#[test]
fn click_and_right_click_cycle_a_step() {
    let a = toggle_step(&empty(), 38, 4, false);
    assert_eq!(step_velocity(&a, 38, 4), Some(LOUD));
    assert_eq!(a.notes[0].beat, 1.0);
    assert_eq!(a.notes[0].length_beats, 0.25);
    let b = toggle_step(&a, 38, 4, true);
    assert_eq!(step_velocity(&b, 38, 4), Some(SOFT));
    let c = toggle_step(&b, 38, 4, true);
    assert_eq!(step_velocity(&c, 38, 4), None);
    let d = toggle_step(&a, 38, 4, false);
    assert!(d.notes.is_empty());
    assert_eq!(
        step_velocity(&toggle_step(&empty(), 36, 0, true), 36, 0),
        Some(SOFT)
    );
}

#[test]
fn preset_repeats_over_the_clip_and_keeps_foreign_notes() {
    let melody = MidiNote {
        beat: 0.0,
        length_beats: 1.0,
        pitch: 60,
        velocity: 90,
    };
    let offgrid = MidiNote {
        beat: 0.1,
        length_beats: 0.1,
        pitch: 36,
        velocity: 90,
    };
    let old = toggle_step(&empty(), 36, 3, false);
    let mut start = old.clone();
    start.notes.push(melody);
    start.notes.push(offgrid);
    let pop = drum_preset("Pop").unwrap();
    let out = apply_preset(&start, 8.0, pop, 0);
    assert!(out.notes.contains(&melody), "Melodienote bleibt");
    assert!(
        out.notes.contains(&offgrid),
        "Note zwischen den Schritten bleibt"
    );
    assert_eq!(
        step_velocity(&out, 36, 3),
        None,
        "alter Rasterschritt ersetzt"
    );
    // Zwei Takte: Snare auf 4 und 12 in beiden.
    for s in [4, 12, 20, 28] {
        assert_eq!(step_velocity(&out, 38, s), Some(LOUD), "Snare {s}");
    }
    assert!(out.notes.windows(2).all(|w| w[0].beat <= w[1].beat));
    assert_eq!(steps(8.0), 32);
}

#[test]
fn density_less_and_more() {
    let pop = drum_preset("Pop").unwrap();
    let count = |m: &MidiData, pitch| m.notes.iter().filter(|n| n.pitch == pitch).count();
    let less = apply_preset(&empty(), 4.0, pop, -1);
    let norm = apply_preset(&empty(), 4.0, pop, 0);
    let more = apply_preset(&empty(), 4.0, pop, 1);
    assert_eq!(count(&norm, 42), 8);
    assert_eq!(count(&more, 42), 16);
    assert!(count(&less, 42) <= count(&norm, 42));
    assert_eq!(count(&less, 38), 2, "Snare bleibt");
}

#[test]
fn dice_is_repeatable_and_valid() {
    let a = dice(&empty(), 4.0, 7);
    assert_eq!(a, dice(&empty(), 4.0, 7));
    assert!(!a.notes.is_empty());
    assert!(a.notes.iter().all(|n| n.beat < 4.0 && n.velocity > 0));
    let differ = (0..20).any(|s| dice(&empty(), 4.0, s) != a);
    assert!(differ);
}
