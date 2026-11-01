//! Paket J3: Akkordfolge legt drei MIDI-Spuren mit Clips an, Tonartwechsel transponiert alle.

use std::time::Instant;

use woa_gui::AppState;
use woa_model::chords::{generate, progression, transpose};

#[test]
fn progression_makes_three_clips_and_key_change_moves_all() {
    let tmp = tempfile::tempdir().expect("tmp");
    let mut st = AppState::new(tmp.path().join("rec"), Instant::now());
    let p = progression("I–V–vi–IV").expect("pop");
    let made = st.add_chord_tracks(p, 0, 4, 0.0).expect("made");
    assert_eq!(st.project().tracks.len(), 3);
    let want = generate(p, 0, 4);
    let midi = |st: &AppState, i: usize| {
        st.project()
            .clip(made[i].1)
            .and_then(|(_, c)| c.midi.clone())
            .expect("midi")
    };
    assert_eq!(midi(&st, 0), want.chords);
    assert_eq!(midi(&st, 1), want.bass);
    assert_eq!(midi(&st, 2), want.arp);
    for (t, _) in made {
        let track = st.project().track(t).expect("track");
        assert!(track.sound.is_some_and(|s| !s.drums), "GM-Klang je Rolle");
    }

    let clips: Vec<_> = made.iter().map(|m| m.1).collect();
    assert!(st.transpose_clips(&clips, 2));
    assert_eq!(midi(&st, 0), transpose(&want.chords, 2));
    assert_eq!(midi(&st, 2), transpose(&want.arp, 2));
    st.undo();
    assert_eq!(midi(&st, 1), want.bass, "ein Undo für alle drei");
    st.undo();
    assert!(
        st.project().tracks.is_empty(),
        "Anlegen ist ein Undo-Schritt"
    );
}
