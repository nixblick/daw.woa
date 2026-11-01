//! Paket J1: GM-Klang je MIDI-Spur. Regeln, Undo, Speichern und Laden.

use woa_model::store;
use woa_model::*;

#[test]
fn sound_survives_save_and_load_and_undo_restores_synth() {
    let mut s = Session::new("Klang");
    let midi = s.new_track_id();
    s.apply(Command::add_track(midi, "Bass", TrackKind::Midi))
        .unwrap();
    let audio = s.new_track_id();
    s.apply(Command::add_track(audio, "Gesang", TrackKind::Audio))
        .unwrap();
    let bass = TrackSound {
        program: 33,
        drums: false,
    };
    assert!(
        s.apply(Command::SetTrackSound {
            track: audio,
            sound: Some(bass)
        })
        .is_err()
    );
    assert!(
        s.apply(Command::SetTrackSound {
            track: midi,
            sound: Some(TrackSound {
                program: 200,
                drums: false
            })
        })
        .is_err()
    );
    s.apply(Command::SetTrackSound {
        track: midi,
        sound: Some(bass),
    })
    .unwrap();

    let dir = tempfile::tempdir().unwrap();
    store::save(s.project(), dir.path()).unwrap();
    let loaded = store::load(dir.path()).unwrap();
    assert_eq!(loaded.track(midi).unwrap().sound, Some(bass));
    assert_eq!(required_version(&loaded), 5);

    s.undo().unwrap();
    assert_eq!(s.project().track(midi).unwrap().sound, None);
    assert!(!store::to_json(s.project()).unwrap().contains("\"sound\""));
}
