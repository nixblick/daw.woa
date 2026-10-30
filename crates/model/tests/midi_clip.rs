//! Paket 12: MIDI clips survive save/load, notes are edited through `SetNotes`
//! with undo, and broken note data is rejected on apply and on load.

use woa_model::*;

fn note(beat: f64, pitch: u8, velocity: u8) -> MidiNote {
    MidiNote {
        beat,
        length_beats: 0.5,
        pitch,
        velocity,
    }
}

fn phrase() -> MidiData {
    MidiData {
        notes: vec![note(0.0, 60, 100), note(0.5, 64, 80), note(1.0, 67, 64)],
        cc: vec![
            MidiCc {
                beat: 0.0,
                controller: CC_SUSTAIN,
                value: 127,
            },
            MidiCc {
                beat: 1.75,
                controller: CC_SUSTAIN,
                value: 0,
            },
        ],
    }
}

fn session_with_clip() -> (Session, ClipId, ClipId) {
    let mut s = Session::new("Midi");
    let t = s.new_track_id();
    s.apply(Command::add_track(t, "Piano", TrackKind::Midi))
        .unwrap();
    let c = s.new_clip_id();
    s.apply(Command::AddClip {
        track: t,
        clip: Clip::new_midi(c, "Phrase", 4.0, 2.0, phrase()),
        index: None,
    })
    .unwrap();
    let a = s.new_clip_id();
    s.apply(Command::AddClip {
        track: t,
        clip: Clip::new(a, "Platzhalter", 8.0, 1.0, None),
        index: None,
    })
    .unwrap();
    (s, c, a)
}

#[test]
fn midi_clip_roundtrip_through_project_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("midi.woa");
    let (mut s, c, _) = session_with_clip();
    s.save_as(&path).unwrap();
    let loaded = Session::open(&path).unwrap();
    assert_eq!(loaded.project(), s.project());
    let (_, clip) = loaded.project().clip(c).unwrap();
    assert_eq!(clip.midi.as_ref().unwrap(), &phrase());
    let json = store::to_json(loaded.project()).unwrap();
    assert!(json.contains("\"midi\""), "{json}");
    assert!(json.contains("\"velocity\": 80"), "{json}");
}

#[test]
fn set_notes_sorts_and_undoes() {
    let (mut s, c, a) = session_with_clip();
    let mut next = phrase();
    next.notes.reverse();
    next.notes.push(note(1.5, 72, 127));
    s.apply(Command::SetNotes {
        clip: c,
        midi: next,
    })
    .unwrap();
    let got = s.project().clip(c).unwrap().1.midi.clone().unwrap();
    assert_eq!(got.notes.len(), 4);
    assert!(
        got.notes.windows(2).all(|w| w[0].beat <= w[1].beat),
        "sorted"
    );
    s.undo().unwrap();
    assert_eq!(
        s.project().clip(c).unwrap().1.midi.as_ref(),
        Some(&phrase())
    );
    s.redo().unwrap();
    assert_eq!(
        s.project()
            .clip(c)
            .unwrap()
            .1
            .midi
            .as_ref()
            .unwrap()
            .notes
            .len(),
        4
    );

    let before = s.project().clone();
    assert!(matches!(
        s.apply(Command::SetNotes {
            clip: a,
            midi: phrase()
        }),
        Err(ModelError::NotMidiClip(_))
    ));
    let mut bad = phrase();
    bad.notes[0].velocity = 0;
    assert!(s.apply(Command::SetNotes { clip: c, midi: bad }).is_err());
    assert_eq!(s.project(), &before, "failed commands change nothing");
}

#[test]
fn broken_notes_in_file_are_rejected() {
    let (s, _, _) = session_with_clip();
    let json = store::to_json(s.project())
        .unwrap()
        .replace("\"pitch\": 67", "\"pitch\": 200");
    assert!(store::from_json(&json, std::path::Path::new("x")).is_err());
}

#[test]
fn set_notes_json_shape() {
    let cmd: Command = serde_json::from_str(
        r#"{"type":"set_notes","clip":3,"midi":{"notes":[{"beat":0.0,"length_beats":1.0,"pitch":60,"velocity":90}]}}"#,
    )
    .unwrap();
    assert!(matches!(cmd, Command::SetNotes { .. }));
}
