//! Paket J2: Drum-Raster schreibt über `set_notes` normale MIDI-Noten, ein Undo je Klick.

use std::time::Instant;

use woa_gui::AppState;
use woa_model::drums::{apply_preset, step_velocity, toggle_step};
use woa_model::{MidiData, TrackSound, drum_preset};

#[test]
fn preset_and_step_are_undo_steps_on_a_drum_track() {
    let tmp = tempfile::tempdir().expect("tmp");
    let mut st = AppState::new(tmp.path().join("rec"), Instant::now());
    let track = st.add_midi_track().expect("track");
    assert!(st.set_track_sound(
        track,
        Some(TrackSound {
            program: 0,
            drums: true
        })
    ));
    let clip = st
        .add_midi_clip(Some(track), "Beat", 0.0, 4.0, MidiData::default())
        .expect("clip");
    let notes = |st: &AppState| {
        st.project()
            .clip(clip)
            .and_then(|(_, c)| c.midi.clone())
            .expect("midi")
    };

    let pop = drum_preset("Pop").expect("preset");
    assert!(st.set_notes(clip, apply_preset(&notes(&st), 4.0, pop, 0)));
    let with_preset = notes(&st);
    assert_eq!(step_velocity(&with_preset, 38, 4), Some(110));

    assert!(st.set_notes(clip, toggle_step(&with_preset, 38, 4, false)));
    assert_eq!(step_velocity(&notes(&st), 38, 4), None);

    st.undo();
    assert_eq!(notes(&st), with_preset, "Undo nimmt nur den Klick zurück");
    st.undo();
    assert!(notes(&st).notes.is_empty());
}
