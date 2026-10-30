//! Paket 18: Automation und Sidechain im Modell. Undo, Prüfungen, Zyklen, Projektformat 3.

use woa_model::store;
use woa_model::*;

fn setup() -> (Session, TrackId, TrackId) {
    let mut s = Session::new("Auto");
    let a = s.new_track_id();
    s.apply(Command::add_track(a, "Musik", TrackKind::Audio))
        .unwrap();
    let b = s.new_track_id();
    s.apply(Command::add_track(b, "Sprache", TrackKind::Audio))
        .unwrap();
    (s, a, b)
}

fn pt(beat: f64, value: f64) -> AutoPoint {
    AutoPoint { beat, value }
}

fn gain_lane(s: &mut Session, t: TrackId) {
    s.apply(Command::SetAutomation {
        track: t,
        target: AutoTarget::Gain,
        points: vec![pt(4.0, -6.0), pt(0.0, 0.0), pt(8.0, -20.0)],
        mode: AutoMode::Read,
    })
    .unwrap();
}

#[test]
fn automation_is_sorted_and_one_undo_step() {
    let (mut s, a, _) = setup();
    let before = s.project().clone();
    gain_lane(&mut s, a);
    let lane = s
        .project()
        .track(a)
        .unwrap()
        .lane(&AutoTarget::Gain)
        .unwrap();
    assert_eq!(lane.points[0].beat, 0.0);
    assert_eq!(lane.value_at(2.0, false), Some(-3.0));
    s.undo().unwrap();
    assert_eq!(s.project(), &before);
    s.redo().unwrap();
    // Leere Kurve im Lesen-Modus entfernt die Lane.
    s.apply(Command::SetAutomation {
        track: a,
        target: AutoTarget::Gain,
        points: vec![],
        mode: AutoMode::Read,
    })
    .unwrap();
    assert!(s.project().track(a).unwrap().automation.is_empty());
    s.undo().unwrap();
    assert_eq!(s.project().track(a).unwrap().automation.len(), 1);
}

#[test]
fn bad_automation_changes_nothing() {
    let (mut s, a, b) = setup();
    let before = s.project().clone();
    for (target, points) in [
        (AutoTarget::Gain, vec![pt(0.0, 99.0)]),
        (AutoTarget::Gain, vec![pt(-1.0, 0.0)]),
        (AutoTarget::Pan, vec![pt(0.0, 2.0)]),
        (AutoTarget::Send { target: b }, vec![pt(0.0, 0.0)]),
        (
            AutoTarget::InsertParam {
                insert: InsertId(77),
                param: 0,
            },
            vec![pt(0.0, 0.0)],
        ),
    ] {
        let r = s.apply(Command::SetAutomation {
            track: a,
            target,
            points,
            mode: AutoMode::Read,
        });
        assert!(r.is_err(), "{target:?}");
    }
    assert_eq!(s.project(), &before);
}

#[test]
fn effect_params_are_automatable() {
    let (mut s, a, _) = setup();
    let id = s.new_insert_id();
    s.apply(Command::AddInsert {
        track: a,
        insert: Insert::new(id, EffectKind::Eq),
        index: None,
    })
    .unwrap();
    let p = EffectKind::Eq.param_index("bell_db").unwrap();
    s.apply(Command::SetAutomation {
        track: a,
        target: AutoTarget::InsertParam {
            insert: id,
            param: p,
        },
        points: vec![pt(0.0, 3.0), pt(4.0, -3.0)],
        mode: AutoMode::Write,
    })
    .unwrap();
    // Außerhalb des Reglerbereichs wird abgelehnt.
    assert!(
        s.apply(Command::SetAutomation {
            track: a,
            target: AutoTarget::InsertParam {
                insert: id,
                param: p,
            },
            points: vec![pt(0.0, 500.0)],
            mode: AutoMode::Read,
        })
        .is_err()
    );
}

fn comp(s: &mut Session, t: TrackId) -> InsertId {
    let id = s.new_insert_id();
    s.apply(Command::AddInsert {
        track: t,
        insert: Insert::new(id, EffectKind::Compressor),
        index: None,
    })
    .unwrap();
    id
}

#[test]
fn sidechain_set_undo_and_unsupported_kinds() {
    let (mut s, a, b) = setup();
    let c = comp(&mut s, a);
    let before = s.project().clone();
    s.apply(Command::SetInsertSidechain {
        track: a,
        insert: c,
        source: Some(b),
    })
    .unwrap();
    assert_eq!(s.project().insert(c).unwrap().1.sidechain, Some(b));
    s.undo().unwrap();
    assert_eq!(s.project(), &before);
    // Ein EQ hat keinen Sidechain-Eingang.
    let e = s.new_insert_id();
    s.apply(Command::AddInsert {
        track: a,
        insert: Insert::new(e, EffectKind::Eq),
        index: None,
    })
    .unwrap();
    assert!(
        s.apply(Command::SetInsertSidechain {
            track: a,
            insert: e,
            source: Some(b),
        })
        .is_err()
    );
}

#[test]
fn sidechain_cycles_are_refused() {
    let (mut s, a, b) = setup();
    let ca = comp(&mut s, a);
    let cb = comp(&mut s, b);
    assert!(
        s.apply(Command::SetInsertSidechain {
            track: a,
            insert: ca,
            source: Some(a),
        })
        .is_err()
    );
    s.apply(Command::SetInsertSidechain {
        track: a,
        insert: ca,
        source: Some(b),
    })
    .unwrap();
    // b hört auf a, a hört auf b: Kreis.
    let r = s.apply(Command::SetInsertSidechain {
        track: b,
        insert: cb,
        source: Some(a),
    });
    assert!(matches!(r, Err(ModelError::RoutingCycle { .. })));
}

#[test]
fn removing_a_sidechain_source_cuts_it_in_one_undo_step() {
    let (mut s, a, b) = setup();
    let c = comp(&mut s, a);
    s.apply(Command::SetInsertSidechain {
        track: a,
        insert: c,
        source: Some(b),
    })
    .unwrap();
    assert!(s.apply(Command::RemoveTrack { track: b }).is_err());
    let before = s.project().clone();
    s.remove_track_routed(b).unwrap();
    assert!(s.project().track(b).is_none());
    assert_eq!(s.project().insert(c).unwrap().1.sidechain, None);
    s.undo().unwrap();
    assert_eq!(s.project(), &before);
}

fn full_session() -> Session {
    let (mut s, a, b) = setup();
    gain_lane(&mut s, a);
    let c = comp(&mut s, a);
    s.apply(Command::SetInsertSidechain {
        track: a,
        insert: c,
        source: Some(b),
    })
    .unwrap();
    s
}

#[test]
fn format_version_follows_content() {
    let (plain, ..) = setup();
    let json = store::to_json(plain.project()).unwrap();
    assert!(json.contains("\"format_version\": 2"));
    let json = store::to_json(full_session().project()).unwrap();
    assert!(json.contains("\"format_version\": 3"));
}

#[test]
fn automation_project_roundtrips_and_matches_the_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("auto.woa");
    let mut s = full_session();
    s.save_as(&path).unwrap();
    let loaded = Session::open(&path).unwrap();
    assert_eq!(loaded.project(), s.project());
    insta::assert_snapshot!(store::to_json(s.project()).unwrap());
}

#[test]
fn hand_edited_automation_is_rejected() {
    let file = |lane: &str| {
        format!(
            r#"{{"format_version":3,"project":{{"meta":{{"name":"X"}},"tempo_bpm":120.0,
            "tracks":[{{"id":1,"name":"T","kind":"audio","gain_db":0.0,"pan":0.0,"mute":false,
            "solo":false,"clips":[],"automation":[{lane}]}}],"next_id":10}}}}"#
        )
    };
    let load = |j: String| store::from_json(&j, std::path::Path::new("x.json"));
    let ok = r#"{"target":{"kind":"gain"},"points":[{"beat":0.0,"value":0.0}]}"#;
    assert!(load(file(ok)).is_ok());
    let unsorted = r#"{"target":{"kind":"gain"},"points":[{"beat":4.0,"value":0.0},{"beat":1.0,"value":0.0}]}"#;
    assert!(load(file(unsorted)).is_err());
    let loud = r#"{"target":{"kind":"gain"},"points":[{"beat":0.0,"value":90.0}]}"#;
    assert!(load(file(loud)).is_err());
    assert!(load(file(&format!("{ok},{ok}"))).is_err());
}
