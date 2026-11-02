//! Design-Runde 3, Abschnitt 6: echter Eingangs-Trim je Spur, getrennt vom Spur-Gain (Fader).
//! Ein Command mit Undo, Bereichsprüfung, Projektformat ohne Trim unverändert.

use woa_model::*;

fn setup() -> (Session, TrackId) {
    let mut s = Session::new("Trim");
    let t = s.new_track_id();
    s.apply(Command::add_track(t, "Stimme", TrackKind::Audio))
        .unwrap();
    (s, t)
}

#[test]
fn set_trim_is_separate_from_gain_and_undoable() {
    let (mut s, t) = setup();
    s.apply(Command::SetGain {
        track: t,
        gain_db: -3.0,
    })
    .unwrap();
    s.apply(Command::SetTrim {
        track: t,
        trim_db: 6.0,
    })
    .unwrap();
    let tr = s.project().track(t).unwrap();
    assert_eq!((tr.gain_db, tr.trim_db), (-3.0, 6.0));
    s.undo().unwrap();
    let tr = s.project().track(t).unwrap();
    assert_eq!((tr.gain_db, tr.trim_db), (-3.0, 0.0));
    s.redo().unwrap();
    assert_eq!(s.project().track(t).unwrap().trim_db, 6.0);
}

#[test]
fn trim_out_of_range_is_rejected_and_changes_nothing() {
    let (mut s, t) = setup();
    for bad in [TRIM_RANGE.1 + 0.1, TRIM_RANGE.0 - 0.1, f64::NAN] {
        assert!(
            s.apply(Command::SetTrim {
                track: t,
                trim_db: bad,
            })
            .is_err()
        );
    }
    assert_eq!(s.project().track(t).unwrap().trim_db, 0.0);
    assert!(
        s.apply(Command::SetTrim {
            track: TrackId(999),
            trim_db: 1.0,
        })
        .is_err()
    );
}

#[test]
fn trim_is_saved_only_when_set() {
    let (mut s, t) = setup();
    let json = serde_json::to_string(s.project()).unwrap();
    assert!(!json.contains("trim_db"), "{json}");
    s.apply(Command::SetTrim {
        track: t,
        trim_db: -4.5,
    })
    .unwrap();
    let json = serde_json::to_string(s.project()).unwrap();
    assert!(json.contains("\"trim_db\":-4.5"), "{json}");
    let back: Project = serde_json::from_str(&json).unwrap();
    assert_eq!(back.track(t).unwrap().trim_db, -4.5);
}
