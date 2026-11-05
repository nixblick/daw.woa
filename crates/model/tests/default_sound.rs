//! Klangqualität (André 02.11.2026): neue MIDI-Spuren spielen den Flügel, alte Projekte
//! behalten ihren Klang.

#![allow(clippy::unwrap_used, clippy::expect_used)] // Tests duerfen abbrechen (Regel 6 gilt fuer Produktivcode).

use woa_model::{Command, Session, SoundCategory, TrackKind, TrackSound, store};

#[test]
fn new_midi_track_plays_the_grand_piano() {
    let mut s = Session::new("Neu");
    let t = s.new_track_id();
    s.apply(Command::add_midi_track(t, "Flügel 1")).unwrap();
    let track = s.project().track(t).unwrap();
    assert_eq!(track.kind, TrackKind::Midi);
    assert_eq!(track.sound, Some(TrackSound::GRAND));
    assert_eq!(woa_model::sound_name(track.sound), "Flügel");
    // Ein Undo-Schritt nimmt die ganze Spur zurück.
    s.undo().unwrap();
    assert!(s.project().track(t).is_none());
}

#[test]
fn old_projects_keep_the_builtin_synth() {
    // Projektdatei von vor 0.38.0: MIDI-Spur ohne `sound`.
    let mut s = Session::new("Alt");
    let t = s.new_track_id();
    s.apply(Command::add_track(t, "MIDI 1", TrackKind::Midi))
        .unwrap();
    let json = store::to_json(s.project()).unwrap();
    assert!(!json.contains("\"sound\""), "{json}");
    let loaded = store::from_json(&json, std::path::Path::new("p.json")).unwrap();
    assert_eq!(loaded.track(t).unwrap().sound, None);
    assert_eq!(woa_model::sound_name(None), "Synth (einfach)");
}

#[test]
fn every_category_has_sounds_and_old_names_still_work() {
    for c in SoundCategory::ALL {
        assert!(
            woa_model::SOUND_PRESETS.iter().any(|p| p.category == c),
            "{c:?} leer"
        );
    }
    assert_eq!(
        woa_model::preset_by_name("Klavier").map(|p| p.sound),
        Some(TrackSound::GRAND)
    );
}
