//! Paket 16: Busse, Sends, Inserts im Modell. Routing, Zyklen, Undo/Redo, Projektformat.

use woa_model::store;
use woa_model::*;

/// Spur "Gitarre", Bus "Hall", Bus "Gruppe".
fn setup() -> (Session, TrackId, TrackId, TrackId) {
    let mut s = Session::new("Mix");
    let t = s.new_track_id();
    s.apply(Command::add_track(t, "Gitarre", TrackKind::Audio))
        .unwrap();
    let hall = s.new_track_id();
    s.apply(Command::add_track(hall, "Hall", TrackKind::Bus))
        .unwrap();
    let grp = s.new_track_id();
    s.apply(Command::add_track(grp, "Gruppe", TrackKind::Bus))
        .unwrap();
    (s, t, hall, grp)
}

fn send(target: TrackId, level_db: f64, pre: bool) -> Send {
    Send {
        target,
        level_db,
        pre_fader: pre,
    }
}

#[test]
fn every_routing_command_is_one_undo_step() {
    let (mut s, t, hall, grp) = setup();
    let before = s.project().clone();
    let steps = [
        Command::SetOutput {
            track: t,
            output: Some(grp),
        },
        Command::AddSend {
            track: t,
            send: send(hall, -12.0, false),
            index: None,
        },
        Command::SetSend {
            track: t,
            target: hall,
            level_db: -6.0,
            pre_fader: true,
        },
    ];
    let mut states = vec![before];
    for c in steps {
        s.apply(c).unwrap();
        states.push(s.project().clone());
    }
    assert_eq!(s.project().track(t).unwrap().sends.len(), 1);
    s.apply(Command::RemoveSend {
        track: t,
        target: hall,
    })
    .unwrap();
    s.undo().unwrap();
    assert_eq!(s.project(), &states[3]);
    for i in (0..3).rev() {
        s.undo().unwrap();
        assert_eq!(s.project(), &states[i], "step {i}");
    }
    for state in &states[1..] {
        s.redo().unwrap();
        assert_eq!(s.project(), state);
    }
}

#[test]
fn inserts_add_edit_remove_with_undo() {
    let (mut s, t, ..) = setup();
    let id = s.new_insert_id();
    let eq = Insert::new(id, EffectKind::Eq);
    s.apply(Command::AddInsert {
        track: t,
        insert: eq,
        index: None,
    })
    .unwrap();
    let id2 = s.new_insert_id();
    s.apply(Command::AddInsert {
        track: t,
        insert: Insert::new(id2, EffectKind::Compressor),
        index: Some(0),
    })
    .unwrap();
    let kinds: Vec<_> = s.project().track(t).unwrap().inserts.iter().map(|i| i.kind).collect();
    assert_eq!(kinds, [EffectKind::Compressor, EffectKind::Eq]);

    let bell = EffectKind::Eq.param_index("bell_db").unwrap();
    s.apply(Command::SetInsertParam {
        track: t,
        insert: id,
        param: bell,
        value: 4.5,
    })
    .unwrap();
    s.apply(Command::SetInsertBypass {
        track: t,
        insert: id,
        bypass: true,
    })
    .unwrap();
    let ins = &s.project().insert(id).unwrap().1.clone();
    assert_eq!(ins.param("bell_db"), Some(4.5));
    assert!(ins.bypass);
    s.undo().unwrap();
    s.undo().unwrap();
    let ins = s.project().insert(id).unwrap().1.clone();
    assert_eq!(ins.param("bell_db"), Some(0.0));
    assert!(!ins.bypass);

    s.apply(Command::RemoveInsert {
        track: t,
        insert: id2,
    })
    .unwrap();
    s.undo().unwrap();
    assert_eq!(s.project().track(t).unwrap().inserts[0].id, id2, "same place");
}

#[test]
fn bad_effect_values_change_nothing() {
    let (mut s, t, ..) = setup();
    let id = s.new_insert_id();
    s.apply(Command::AddInsert {
        track: t,
        insert: Insert::new(id, EffectKind::Limiter),
        index: None,
    })
    .unwrap();
    let broken_id = s.new_insert_id();
    let before = s.project().clone();
    for (param, value) in [(0, 3.0), (0, f64::NAN), (7, 0.0), (1, 0.0)] {
        let r = s.apply(Command::SetInsertParam {
            track: t,
            insert: id,
            param,
            value,
        });
        assert!(r.is_err(), "{param} {value}");
    }
    let mut broken = Insert::new(broken_id, EffectKind::Eq);
    broken.params.pop();
    assert!(
        s.apply(Command::AddInsert {
            track: t,
            insert: broken,
            index: None
        })
        .is_err()
    );
    // doppelte Id
    assert!(matches!(
        s.apply(Command::AddInsert {
            track: t,
            insert: Insert::new(id, EffectKind::Eq),
            index: None
        }),
        Err(ModelError::DuplicateId(_))
    ));
    assert_eq!(s.project(), &before);
}

#[test]
fn bus_cannot_feed_itself_or_loop_back() {
    let (mut s, t, hall, grp) = setup();
    // Auf sich selbst.
    assert!(matches!(
        s.apply(Command::SetOutput {
            track: hall,
            output: Some(hall)
        }),
        Err(ModelError::RoutingCycle { .. })
    ));
    assert!(matches!(
        s.apply(Command::AddSend {
            track: grp,
            send: send(grp, 0.0, false),
            index: None
        }),
        Err(ModelError::RoutingCycle { .. })
    ));
    // Hall -> Gruppe, dann Gruppe -> Hall wäre ein Kreis (Ausgang und Send).
    s.apply(Command::SetOutput {
        track: hall,
        output: Some(grp),
    })
    .unwrap();
    let before = s.project().clone();
    assert!(matches!(
        s.apply(Command::SetOutput {
            track: grp,
            output: Some(hall)
        }),
        Err(ModelError::RoutingCycle { .. })
    ));
    assert!(matches!(
        s.apply(Command::AddSend {
            track: grp,
            send: send(hall, 0.0, false),
            index: None
        }),
        Err(ModelError::RoutingCycle { .. })
    ));
    assert_eq!(s.project(), &before);
    // Eine Spur ist kein Ziel.
    assert!(matches!(
        s.apply(Command::SetOutput {
            track: hall,
            output: Some(t)
        }),
        Err(ModelError::NotABus(_))
    ));
    // Unbekanntes Ziel, doppelter Send, Pegel außerhalb.
    assert!(
        s.apply(Command::AddSend {
            track: t,
            send: send(TrackId(999), 0.0, false),
            index: None
        })
        .is_err()
    );
    s.apply(Command::AddSend {
        track: t,
        send: send(hall, 0.0, false),
        index: None,
    })
    .unwrap();
    assert!(matches!(
        s.apply(Command::AddSend {
            track: t,
            send: send(hall, 0.0, false),
            index: None
        }),
        Err(ModelError::DuplicateSend { .. })
    ));
    assert!(
        s.apply(Command::SetSend {
            track: t,
            target: hall,
            level_db: 13.0,
            pre_fader: false
        })
        .is_err()
    );
}

#[test]
fn routed_bus_is_removed_only_with_its_connections() {
    let (mut s, t, hall, _) = setup();
    s.apply(Command::AddSend {
        track: t,
        send: send(hall, -3.0, true),
        index: None,
    })
    .unwrap();
    s.apply(Command::SetOutput {
        track: t,
        output: Some(hall),
    })
    .unwrap();
    assert!(matches!(
        s.apply(Command::RemoveTrack { track: hall }),
        Err(ModelError::TrackInUse(_))
    ));
    let before = s.project().clone();
    s.remove_track_routed(hall).unwrap();
    assert!(s.project().track(hall).is_none());
    assert!(s.project().track(t).unwrap().sends.is_empty());
    assert_eq!(s.project().track(t).unwrap().output, None);
    // Ein Schritt zurück bringt Bus und beide Verbindungen wieder.
    s.undo().unwrap();
    assert_eq!(s.project(), &before);
}

#[test]
fn removing_and_restoring_a_track_keeps_its_chain() {
    let (mut s, t, hall, _) = setup();
    let id = s.new_insert_id();
    s.apply(Command::AddInsert {
        track: t,
        insert: Insert::new(id, EffectKind::Compressor),
        index: None,
    })
    .unwrap();
    s.apply(Command::AddSend {
        track: t,
        send: send(hall, -9.0, false),
        index: None,
    })
    .unwrap();
    let before = s.project().clone();
    s.apply(Command::RemoveTrack { track: t }).unwrap();
    s.undo().unwrap();
    assert_eq!(s.project(), &before);
}

fn mixed_session() -> Session {
    let (mut s, t, hall, _) = setup();
    s.apply(Command::AddSend {
        track: t,
        send: send(hall, -12.0, true),
        index: None,
    })
    .unwrap();
    let id = s.new_insert_id();
    s.apply(Command::AddInsert {
        track: t,
        insert: Insert::new(id, EffectKind::Eq),
        index: None,
    })
    .unwrap();
    s.apply(Command::SetOutput {
        track: t,
        output: None,
    })
    .unwrap();
    s
}

#[test]
fn mixer_project_roundtrips_through_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("mix.woa");
    let mut s = mixed_session();
    s.save_as(&path).unwrap();
    let loaded = Session::open(&path).unwrap();
    assert_eq!(loaded.project(), s.project());
}

#[test]
fn mixer_json_snapshot() {
    insta::assert_snapshot!(store::to_json(mixed_session().project()).unwrap());
}

#[test]
fn version_1_file_loads_and_gets_empty_routing() {
    let v1 = r#"{"format_version":1,"project":{"meta":{"name":"Alt"},"tempo_bpm":120.0,
        "tracks":[{"id":1,"name":"Spur","kind":"audio","gain_db":-3.0,"pan":0.0,
        "mute":false,"solo":false,"clips":[]}],"next_id":2}}"#;
    let p = store::from_json(v1, std::path::Path::new("alt.json")).unwrap();
    let t = &p.tracks[0];
    assert_eq!(t.gain_db, -3.0);
    assert!(t.output.is_none() && t.sends.is_empty() && t.inserts.is_empty());
}

#[test]
fn hand_edited_files_with_bad_routing_are_rejected() {
    let base = |extra: &str| {
        format!(
            r#"{{"format_version":2,"project":{{"meta":{{"name":"X"}},"tempo_bpm":120.0,
            "tracks":[{extra}],"next_id":10}}}}"#
        )
    };
    let track = |id: u64, kind: &str, more: &str| {
        format!(
            r#"{{"id":{id},"name":"T","kind":"{kind}","gain_db":0.0,"pan":0.0,"mute":false,
            "solo":false,"clips":[]{more}}}"#
        )
    };
    let load = |json: String| store::from_json(&json, std::path::Path::new("x.json"));
    // Kreis zwischen zwei Bussen.
    let cyc = format!(
        "{},{}",
        track(1, "bus", r#","output":2"#),
        track(2, "bus", r#","output":1"#)
    );
    assert!(matches!(load(base(&cyc)), Err(ModelError::RoutingCycle { .. })));
    // Ziel fehlt.
    assert!(load(base(&track(1, "audio", r#","output":9"#))).is_err());
    // Ziel ist keine Bus-Spur.
    let notbus = format!("{},{}", track(1, "audio", r#","output":2"#), track(2, "audio", ""));
    assert!(matches!(load(base(&notbus)), Err(ModelError::NotABus(_))));
    // Regler außerhalb des Bereichs.
    let bad = track(
        1,
        "audio",
        r#","inserts":[{"id":5,"kind":"limiter","params":[3.0,50.0]}]"#,
    );
    assert!(load(base(&bad)).is_err());
    // Fehlende Regler werden mit Vorgaben aufgefüllt.
    let short = track(
        1,
        "audio",
        r#","inserts":[{"id":5,"kind":"limiter","params":[-3.0]}]"#,
    );
    let p = load(base(&short)).unwrap();
    assert_eq!(p.tracks[0].inserts[0].params, [-3.0, 50.0]);
}
