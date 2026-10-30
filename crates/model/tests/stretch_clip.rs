//! Paket 23: Dehnung, Tonhöhe und Tempo-Folgen im Modell: Befehle mit Undo und Redo, Prüfung
//! der Werte, Projektformat.

use woa_model::{
    Clip, ClipId, Command, FOLLOW_TEMPO_RANGE, ModelError, PITCH_RANGE, STRETCH_RANGE, Session,
    TrackKind, store,
};

fn session_with_clip() -> (Session, ClipId) {
    let mut s = Session::new("zeit");
    let t = s.new_track_id();
    s.apply(Command::add_track(t, "A", TrackKind::Audio))
        .expect("track");
    let c = s.new_clip_id();
    s.apply(Command::AddClip {
        track: t,
        clip: Clip::new(c, "c", 0.0, 8.0, Some("a.wav".into())),
        index: None,
    })
    .expect("clip");
    (s, c)
}

fn clip(s: &Session, c: ClipId) -> Clip {
    s.project().clip(c).expect("clip").1.clone()
}

fn stretch_cmd(c: ClipId, length: f64, stretch: f64) -> Command {
    Command::SetClipStretch {
        clip: c,
        length_beats: length,
        stretch,
        fade_in_beats: 0.0,
        fade_out_beats: 0.0,
    }
}

#[test]
fn stretch_pitch_and_follow_undo_and_redo() {
    let (mut s, c) = session_with_clip();
    s.apply(stretch_cmd(c, 8.8, 1.1)).expect("stretch");
    s.apply(Command::SetClipPitch {
        clip: c,
        semitones: 2.0,
    })
    .expect("pitch");
    s.apply(Command::SetClipFollowTempo {
        clip: c,
        bpm: Some(120.0),
    })
    .expect("follow");
    let k = clip(&s, c);
    assert_eq!(
        (
            k.length_beats,
            k.stretch,
            k.pitch_semitones,
            k.follow_tempo_bpm
        ),
        (8.8, 1.1, 2.0, Some(120.0))
    );
    // Die gezeigte Tonlänge bleibt bei 8 Beats (Quell-Beats), das Verhältnis ist 1,1.
    assert!((k.source_length_beats() - 8.0).abs() < 1e-9);
    assert!((k.time_ratio(120.0) - 1.1).abs() < 1e-9);
    // Bei 100 bpm und Folgen: 1,1 * 120/100.
    assert!((k.time_ratio(100.0) - 1.32).abs() < 1e-9);
    for _ in 0..3 {
        s.undo().expect("undo");
    }
    let k = clip(&s, c);
    assert_eq!(
        (
            k.length_beats,
            k.stretch,
            k.pitch_semitones,
            k.follow_tempo_bpm
        ),
        (8.0, 1.0, 0.0, None)
    );
    for _ in 0..3 {
        s.redo().expect("redo");
    }
    assert_eq!(clip(&s, c).stretch, 1.1);
    assert_eq!(clip(&s, c).follow_tempo_bpm, Some(120.0));
}

#[test]
fn stretch_restores_fades_on_undo() {
    let (mut s, c) = session_with_clip();
    s.apply(Command::EditClip {
        clip: c,
        length_beats: 8.0,
        offset_beats: 0.0,
        fade_in_beats: 3.0,
        fade_out_beats: 3.0,
    })
    .expect("fades");
    s.apply(Command::SetClipStretch {
        clip: c,
        length_beats: 4.0,
        stretch: 0.5,
        fade_in_beats: 2.0,
        fade_out_beats: 2.0,
    })
    .expect("stretch");
    s.undo().expect("undo");
    let k = clip(&s, c);
    assert_eq!(
        (k.length_beats, k.stretch, k.fade_in_beats, k.fade_out_beats),
        (8.0, 1.0, 3.0, 3.0)
    );
    // Fades, die nicht in die neue Länge passen, lehnt das Modell ab.
    assert!(
        s.apply(Command::SetClipStretch {
            clip: c,
            length_beats: 4.0,
            stretch: 0.5,
            fade_in_beats: 3.0,
            fade_out_beats: 3.0,
        })
        .is_err()
    );
    assert_eq!(clip(&s, c).stretch, 1.0, "refused command changes nothing");
}

#[test]
fn bad_values_are_refused() {
    let (mut s, c) = session_with_clip();
    let before = clip(&s, c);
    for bad in [
        STRETCH_RANGE.0 - 0.01,
        STRETCH_RANGE.1 + 0.01,
        f64::NAN,
        0.0,
        -1.0,
    ] {
        assert!(s.apply(stretch_cmd(c, 8.0, bad)).is_err(), "stretch {bad}");
    }
    assert!(s.apply(stretch_cmd(c, 0.0, 1.0)).is_err(), "zero length");
    for bad in [PITCH_RANGE.0 - 0.1, PITCH_RANGE.1 + 0.1, f64::INFINITY] {
        assert!(
            s.apply(Command::SetClipPitch {
                clip: c,
                semitones: bad
            })
            .is_err(),
            "pitch {bad}"
        );
    }
    for bad in [
        FOLLOW_TEMPO_RANGE.0 - 1.0,
        FOLLOW_TEMPO_RANGE.1 + 1.0,
        f64::NAN,
    ] {
        assert!(
            s.apply(Command::SetClipFollowTempo {
                clip: c,
                bpm: Some(bad)
            })
            .is_err(),
            "follow {bad}"
        );
    }
    assert_eq!(clip(&s, c), before, "refused commands change nothing");
    assert!(matches!(
        s.apply(Command::SetClipPitch {
            clip: ClipId(9999),
            semitones: 1.0
        }),
        Err(ModelError::ClipNotFound(_))
    ));
}

#[test]
fn project_format_stays_v2_without_time_fields_and_roundtrips_with_them() {
    let (mut s, c) = session_with_clip();
    assert_eq!(store::required_version(s.project()), 2);
    let json = store::to_json(s.project()).expect("json");
    assert!(!json.contains("stretch") && !json.contains("pitch_semitones"));
    assert!(!json.contains("follow_tempo_bpm"));

    s.apply(stretch_cmd(c, 8.8, 1.1)).expect("stretch");
    assert_eq!(store::required_version(s.project()), 3);
    s.apply(Command::SetClipPitch {
        clip: c,
        semitones: -2.5,
    })
    .expect("pitch");
    s.apply(Command::SetClipFollowTempo {
        clip: c,
        bpm: Some(96.0),
    })
    .expect("follow");
    let json = store::to_json(s.project()).expect("json");
    let back = store::from_json(&json, std::path::Path::new("project.json")).expect("load");
    assert_eq!(&back, s.project());
    let k = back.clip(c).expect("c").1;
    assert_eq!(
        (k.stretch, k.pitch_semitones, k.follow_tempo_bpm),
        (1.1, -2.5, Some(96.0))
    );
}
