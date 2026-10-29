use proptest::prelude::*;
use woa_model::store;
use woa_model::*;

fn sample() -> Session {
    let mut s = Session::new("Jingle");
    let voice = s.new_track_id();
    let music = s.new_track_id();
    s.apply(Command::add_track(voice, "Voice", TrackKind::Audio))
        .unwrap();
    s.apply(Command::add_track(music, "Music", TrackKind::Midi))
        .unwrap();
    let clip = s.new_clip_id();
    s.apply(Command::AddClip {
        track: voice,
        clip: Clip {
            id: clip,
            name: "Intro".into(),
            start_beats: 4.0,
            length_beats: 8.0,
            audio: None,
        },
        index: None,
    })
    .unwrap();
    s.apply(Command::SetGain {
        track: music,
        gain_db: -6.5,
    })
    .unwrap();
    s.apply(Command::SetPan {
        track: voice,
        pan: -0.25,
    })
    .unwrap();
    s.apply(Command::SetTempo { bpm: 128.0 }).unwrap();
    s
}

#[test]
fn save_load_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("jingle.woa");
    let mut s = sample();
    assert!(s.is_dirty());
    s.save_as(&path).unwrap();
    assert!(!s.is_dirty());
    assert!(path.join("audio").is_dir());
    assert!(!path.join("project.json.tmp").exists());
    let loaded = Session::open(&path).unwrap();
    assert_eq!(loaded.project(), s.project());
}

#[test]
fn json_snapshot() {
    insta::assert_snapshot!(store::to_json(sample().project()).unwrap());
}

#[test]
fn newer_format_is_rejected() {
    let json = r#"{"format_version": 999, "project": {}}"#;
    let err = store::from_json(json, std::path::Path::new("x")).unwrap_err();
    assert!(matches!(
        err,
        ModelError::UnsupportedVersion { found: 999, .. }
    ));
}

#[test]
fn undo_redo_and_groups() {
    let mut s = sample();
    let before = s.project().clone();
    let t = s.project().tracks[0].id;
    s.group("Agent", |s| {
        s.apply(Command::SetMute {
            track: t,
            mute: true,
        })?;
        s.apply(Command::RenameTrack {
            track: t,
            name: "Sprecher".into(),
        })
    })
    .unwrap();
    assert_eq!(s.history().undo_label(), Some("Agent"));
    let after = s.project().clone();
    s.undo().unwrap();
    assert_eq!(s.project(), &before);
    s.redo().unwrap();
    assert_eq!(s.project(), &after);
    assert!(matches!(s.redo(), Err(ModelError::NothingToRedo)));
}

#[test]
fn invalid_commands_change_nothing() {
    let mut s = sample();
    let before = s.project().clone();
    let t = s.project().tracks[0].id;
    assert!(
        s.apply(Command::SetGain {
            track: t,
            gain_db: f64::NAN
        })
        .is_err()
    );
    assert!(s.apply(Command::SetPan { track: t, pan: 2.0 }).is_err());
    assert!(
        s.apply(Command::RemoveTrack {
            track: TrackId(999)
        })
        .is_err()
    );
    assert!(
        s.apply(Command::RenameTrack {
            track: t,
            name: " ".into()
        })
        .is_err()
    );
    assert_eq!(s.project(), &before);
}

#[test]
fn autosave_only_when_changed() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.woa");
    let mut s = sample();
    s.save_as(&path).unwrap();
    assert!(!s.autosave().unwrap());
    s.apply(Command::SetTempo { bpm: 90.0 }).unwrap();
    assert!(s.autosave().unwrap());
    assert!(!s.autosave().unwrap());
    let rec = store::load_autosave(&path).unwrap().unwrap();
    assert_eq!(rec.tempo_bpm, 90.0);
    s.save().unwrap();
    assert!(store::load_autosave(&path).unwrap().is_none());
}

#[test]
fn command_json_shape() {
    let c: Command =
        serde_json::from_str(r#"{"type":"set_gain","track":3,"gain_db":-6.0}"#).unwrap();
    assert_eq!(
        c,
        Command::SetGain {
            track: TrackId(3),
            gain_db: -6.0
        }
    );
}

/// Commands that reference existing objects by index, so most of them succeed.
fn op() -> impl Strategy<Value = (u8, usize, usize, f64, bool)> {
    (0u8..10, 0usize..8, 0usize..8, -2.0f64..2.0, any::<bool>())
}

fn build(s: &mut Session, (kind, a, b, x, flag): (u8, usize, usize, f64, bool)) -> Command {
    let p = s.project();
    let track = |i: usize| p.tracks.get(i % p.tracks.len().max(1)).map(|t| t.id);
    let clips: Vec<ClipId> = p
        .tracks
        .iter()
        .flat_map(|t| t.clips.iter().map(|c| c.id))
        .collect();
    let clip = clips.get(a % clips.len().max(1)).copied();
    let t = track(a).unwrap_or(TrackId(0));
    match kind {
        0 => {
            let id = s.new_track_id();
            Command::AddTrack {
                track: Track::new(id, "T", TrackKind::Audio),
                index: Some(b),
            }
        }
        1 => Command::RemoveTrack { track: t },
        2 => Command::RenameTrack {
            track: t,
            name: format!("n{b}"),
        },
        3 => Command::SetGain {
            track: t,
            gain_db: x * 10.0,
        },
        4 => Command::SetPan { track: t, pan: x },
        5 => Command::SetMute {
            track: t,
            mute: flag,
        },
        6 => Command::SetSolo {
            track: t,
            solo: flag,
        },
        7 => {
            let id = s.new_clip_id();
            Command::AddClip {
                track: t,
                clip: Clip {
                    id,
                    name: "c".into(),
                    start_beats: x.abs(),
                    length_beats: 1.0,
                    audio: None,
                },
                index: Some(b),
            }
        }
        8 => Command::RemoveClip {
            clip: clip.unwrap_or(ClipId(0)),
        },
        _ => Command::MoveClip {
            clip: clip.unwrap_or(ClipId(0)),
            to_track: track(b).unwrap_or(TrackId(0)),
            start_beats: x.abs(),
            index: if flag { Some(b) } else { None },
        },
    }
}

fn content(p: &Project) -> (f64, Vec<Track>) {
    (p.tempo_bpm, p.tracks.clone())
}

proptest! {
    #[test]
    fn apply_then_undo_restores(ops in prop::collection::vec(op(), 1..40)) {
        let mut s = sample();
        let start = content(s.project());
        let mut states = vec![start.clone()];
        let mut applied = 0;
        for o in ops {
            let c = build(&mut s, o);
            if s.apply(c).is_ok() {
                applied += 1;
                states.push(content(s.project()));
            } else {
                prop_assert_eq!(&content(s.project()), states.last().unwrap());
            }
        }
        for i in (0..applied).rev() {
            s.undo().unwrap();
            prop_assert_eq!(&content(s.project()), &states[i]);
        }
        for state in states.iter().skip(1) {
            s.redo().unwrap();
            prop_assert_eq!(&content(s.project()), state);
        }
    }

    #[test]
    fn group_undo_restores(ops in prop::collection::vec(op(), 1..20)) {
        let mut s = sample();
        let start = content(s.project());
        s.begin_group("G");
        for o in ops {
            let c = build(&mut s, o);
            let _ = s.apply(c);
        }
        s.end_group().unwrap();
        if s.history().undo_label() == Some("G") {
            s.undo().unwrap();
        }
        prop_assert_eq!(content(s.project()), start);
    }
}
