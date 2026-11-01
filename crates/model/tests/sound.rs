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
        bank: None,
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
                drums: false,
                bank: None,
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

/// Paket J7 (#271): Bank je Spur, bleibt nach Speichern, alte Projekte laden weiter.
#[test]
fn bank_survives_save_and_unknown_banks_still_load() {
    let mut s = Session::new("Bank");
    let midi = s.new_track_id();
    s.apply(Command::add_track(midi, "Klavier", TrackKind::Midi))
        .unwrap();
    // Ein reines Drum-Bank-Programm ohne `drums` ist ein Fehler, kein stiller Kit-Klang.
    assert!(
        s.apply(Command::SetTrackSound {
            track: midi,
            sound: Some(TrackSound {
                program: 0,
                drums: false,
                bank: Some(SoundBank::AvlRedZeppelin),
            }),
        })
        .is_err()
    );
    let piano = TrackSound {
        program: 0,
        drums: false,
        bank: Some(SoundBank::MusescoreGeneral),
    };
    s.apply(Command::SetTrackSound {
        track: midi,
        sound: Some(piano),
    })
    .unwrap();
    assert_eq!(preset_name(piano), Some("Klavier"));
    let dir = tempfile::tempdir().unwrap();
    store::save(s.project(), dir.path()).unwrap();
    let json = store::to_json(s.project()).unwrap();
    assert!(json.contains("\"bank\": \"musescore-general\""), "{json}");
    let loaded = store::load(dir.path()).unwrap();
    assert_eq!(loaded.track(midi).unwrap().sound, Some(piano));
    assert_eq!(required_version(&loaded), 6);

    // Eine Bank, die dieser Bau nicht kennt, lädt als `Unknown` (Rückfall GeneralUser GS).
    let foreign = json.replace("musescore-general", "kommt-erst-spaeter");
    let p = store::from_json(&foreign, std::path::Path::new("p.json")).unwrap();
    assert_eq!(
        p.track(midi).unwrap().sound.and_then(|x| x.bank),
        Some(SoundBank::Unknown)
    );
    // Ohne Bank bleibt das Format bei Version 5 und ohne `bank`-Feld.
    s.apply(Command::SetTrackSound {
        track: midi,
        sound: Some(TrackSound {
            bank: None,
            ..piano
        }),
    })
    .unwrap();
    assert_eq!(required_version(s.project()), 5);
    assert!(!store::to_json(s.project()).unwrap().contains("\"bank\""));
}
