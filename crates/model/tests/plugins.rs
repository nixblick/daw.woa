//! Paket 31: Plugins auf Spuren im Modell. Regeln, Undo, Projektdatei.

use woa_model::store;
use woa_model::*;

fn setup() -> (Session, TrackId, TrackId, TrackId) {
    let mut s = Session::new("Plug");
    let midi = s.new_track_id();
    s.apply(Command::add_track(midi, "Keys", TrackKind::Midi))
        .unwrap();
    let audio = s.new_track_id();
    s.apply(Command::add_track(audio, "Gesang", TrackKind::Audio))
        .unwrap();
    let bus = s.new_track_id();
    s.apply(Command::add_track(bus, "Bus", TrackKind::Bus))
        .unwrap();
    (s, midi, audio, bus)
}

fn slot(s: &mut Session, role: PluginRole) -> PluginSlot {
    let id = InsertId(s.new_track_id().0);
    PluginSlot::new(id, "org.example.plug", "Plug", role)
}

#[test]
fn instrument_only_on_midi_one_per_track_effects_not_on_buses() {
    let (mut s, midi, audio, bus) = setup();
    let inst = slot(&mut s, PluginRole::Instrument);
    assert!(
        s.apply(Command::AddPlugin {
            track: audio,
            plugin: inst.clone(),
            index: None
        })
        .is_err()
    );
    s.apply(Command::AddPlugin {
        track: midi,
        plugin: inst,
        index: None,
    })
    .unwrap();
    let second = slot(&mut s, PluginRole::Instrument);
    assert!(
        s.apply(Command::AddPlugin {
            track: midi,
            plugin: second,
            index: None
        })
        .is_err()
    );
    let fx = slot(&mut s, PluginRole::Effect);
    assert!(
        s.apply(Command::AddPlugin {
            track: bus,
            plugin: fx.clone(),
            index: None
        })
        .is_err()
    );
    s.apply(Command::AddPlugin {
        track: audio,
        plugin: fx,
        index: None,
    })
    .unwrap();
}

#[test]
fn every_plugin_command_undoes_exactly() {
    let (mut s, midi, _, _) = setup();
    let inst = slot(&mut s, PluginRole::Instrument);
    let id = inst.id;
    let mut steps = vec![Command::AddPlugin {
        track: midi,
        plugin: inst,
        index: None,
    }];
    steps.push(Command::SetPluginParam {
        track: midi,
        plugin: id,
        param: 3,
        value: 0.25,
    });
    steps.push(Command::SetPluginParam {
        track: midi,
        plugin: id,
        param: 3,
        value: 0.75,
    });
    steps.push(Command::SetPluginState {
        track: midi,
        plugin: id,
        state: Some(PluginSlot::encode_state(b"zustand")),
        params: vec![],
    });
    steps.push(Command::SetPluginBypass {
        track: midi,
        plugin: id,
        bypass: true,
    });
    steps.push(Command::RemovePlugin {
        track: midi,
        plugin: id,
    });
    for c in steps {
        let before = s.project().clone();
        s.apply(c.clone()).unwrap();
        assert_ne!(*s.project(), before, "{c:?}");
        s.undo().unwrap();
        assert_eq!(*s.project(), before, "{c:?}");
        s.redo().unwrap();
    }
}

#[test]
fn plugin_ids_share_the_id_space() {
    let (mut s, midi, audio, _) = setup();
    let mut p = slot(&mut s, PluginRole::Effect);
    p.id = InsertId(audio.0);
    assert!(
        s.apply(Command::AddPlugin {
            track: midi,
            plugin: p,
            index: None
        })
        .is_err()
    );
    let fresh = s.new_track_id();
    assert!(!s.project().id_in_use(fresh.0));
}

#[test]
fn plugins_survive_save_and_load_with_state() {
    let (mut s, midi, _, _) = setup();
    let mut inst = slot(&mut s, PluginRole::Instrument);
    let blob: Vec<u8> = (0..=255u8).cycle().take(5000).collect();
    inst.state = Some(PluginSlot::encode_state(&blob));
    inst.params.push(PluginParam { id: 9, value: 0.5 });
    let id = inst.id;
    s.apply(Command::AddPlugin {
        track: midi,
        plugin: inst,
        index: None,
    })
    .unwrap();
    assert_eq!(store::required_version(s.project()), 4);
    let json = store::to_json(s.project()).unwrap();
    let back = store::from_json(&json, std::path::Path::new("project.json")).unwrap();
    assert_eq!(&back, s.project());
    let p = back.track(midi).unwrap().plugin(id).unwrap();
    assert_eq!(p.state_bytes().unwrap(), blob);
}

#[test]
fn projects_without_plugins_keep_their_version_and_shape() {
    let (s, ..) = setup();
    assert!(store::required_version(s.project()) <= 3);
    assert!(!store::to_json(s.project()).unwrap().contains("plugins"));
}

#[test]
fn hostile_files_are_rejected() {
    let (mut s, midi, _, _) = setup();
    let mut p = slot(&mut s, PluginRole::Instrument);
    p.state = Some("kein base64!".into());
    let mut project = s.project().clone();
    project
        .tracks
        .iter_mut()
        .find(|t| t.id == midi)
        .unwrap()
        .plugins
        .push(p);
    let json = serde_json::to_string(&ProjectFile {
        format_version: 4,
        project,
    })
    .unwrap();
    assert!(store::from_json(&json, std::path::Path::new("p.json")).is_err());
}
