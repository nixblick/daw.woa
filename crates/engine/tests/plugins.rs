//! Paket 31 ohne Ton: Plugins auf Spuren, offline durch den echten Graphen gerechnet.
//! Gespielt wird das Test-Plugin (gebaut mit `clack-plugin`), geladen über den Host-Weg.
//! Jeder Process-Aufruf läuft unter `assert_no_alloc`.

use std::sync::Arc;

use assert_no_alloc::{AllocDisabler, assert_no_alloc};
use woa_engine::clip::{clip_graph_plugins, render_clips_plugins};
use woa_engine::{AudioProcessor, Engine};
use woa_model::{
    Clip, Command, InsertId, MidiData, MidiNote, PluginParam, PluginRole, PluginSlot, Project,
    Session, TrackId, TrackKind,
};
use woa_plugins::testplugin::{self, GAIN, LEVEL};
use woa_plugins::{PluginRack, SlotStatus};

#[global_allocator]
static ALLOC: AllocDisabler = AllocDisabler;

const SR: u32 = 48_000;
const BLOCK: usize = 256;

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
}

struct Setup {
    session: Session,
    midi: TrackId,
    audio: TrackId,
    inst: InsertId,
    fx: InsertId,
}

/// Eine MIDI-Spur mit Instrument-Plugin (eine lange Note) und eine Audio-Spur mit einem
/// Effekt-Plugin (Gleichspannung 0,25 als Clip).
fn setup() -> Setup {
    let mut s = Session::new("Plug");
    let midi = s.new_track_id();
    s.apply(Command::add_track(midi, "Keys", TrackKind::Midi))
        .unwrap();
    let c = s.new_clip_id();
    let midi_data = MidiData {
        notes: vec![MidiNote {
            beat: 0.0,
            length_beats: 3.0,
            pitch: 69,
            velocity: 100,
        }],
        cc: vec![],
    };
    s.apply(Command::AddClip {
        track: midi,
        clip: Clip::new_midi(c, "Note", 0.0, 4.0, midi_data),
        index: None,
    })
    .unwrap();
    let inst = s.new_insert_id();
    s.apply(Command::AddPlugin {
        track: midi,
        plugin: PluginSlot::new(
            inst,
            testplugin::ID,
            "WOA Test Plugin",
            PluginRole::Instrument,
        ),
        index: None,
    })
    .unwrap();

    let audio = s.new_track_id();
    s.apply(Command::add_track(audio, "Gesang", TrackKind::Audio))
        .unwrap();
    let ac = s.new_clip_id();
    s.apply(Command::AddClip {
        track: audio,
        clip: Clip::new(ac, "dc", 0.0, 4.0, Some("dc.wav".into())),
        index: None,
    })
    .unwrap();
    let fx = s.new_insert_id();
    s.apply(Command::AddPlugin {
        track: audio,
        plugin: PluginSlot::new(fx, testplugin::ID, "WOA Test Plugin", PluginRole::Effect),
        index: None,
    })
    .unwrap();
    Setup {
        session: s,
        midi,
        audio,
        inst,
        fx,
    }
}

fn dc() -> Arc<[f32]> {
    vec![0.25f32; SR as usize * 8].into()
}

fn render(project: &Project, rack: &mut PluginRack, seconds: f64) -> (Vec<f32>, Vec<f32>) {
    rack.sync(project);
    let rack = &*rack;
    render_clips_plugins(
        project,
        SR,
        seconds,
        &mut |p| (p == "dc.wav").then(dc),
        &mut |t| rack.track_plugins(t),
    )
    .expect("render")
}

fn only_track(project: &Project, keep: TrackId) -> Project {
    let mut p = project.clone();
    p.tracks.retain(|t| t.id == keep);
    p
}

#[test]
fn instrument_plugin_plays_the_midi_clip_and_params_change_the_level() {
    let su = setup();
    let project = only_track(su.session.project(), su.midi);
    let mut rack = PluginRack::new(Arc::new(testplugin::catalog()), SR as f32);
    let (l, r) = render(&project, &mut rack, 1.0);
    let loud = rms(&l[2000..]);
    assert!(loud > 0.005, "instrument is silent: {loud}");
    assert!(rms(&r[2000..]) > 0.005);

    // Lautstärke-Regler des Plugins per Projekt (wie die Oberfläche es setzt).
    let mut s = su.session;
    s.apply(Command::SetPluginParam {
        track: su.midi,
        plugin: su.inst,
        param: LEVEL,
        value: 0.05,
    })
    .unwrap();
    let project = only_track(s.project(), su.midi);
    let mut rack2 = PluginRack::new(Arc::new(testplugin::catalog()), SR as f32);
    let (l2, _) = render(&project, &mut rack2, 1.0);
    let quiet = rms(&l2[2000..]);
    assert!(quiet < loud * 0.3, "loud {loud} quiet {quiet}");
}

#[test]
fn effect_plugin_scales_audio_and_bypass_restores_it() {
    let su = setup();
    let base = only_track(su.session.project(), su.audio);
    let mut rack = PluginRack::new(Arc::new(testplugin::catalog()), SR as f32);
    let (l, _) = render(&base, &mut rack, 0.5);
    let one = rms(&l[4000..]);
    assert!(one > 0.01, "no signal: {one}");

    let mut s = su.session;
    s.apply(Command::SetPluginParam {
        track: su.audio,
        plugin: su.fx,
        param: GAIN,
        value: 2.0,
    })
    .unwrap();
    let doubled = only_track(s.project(), su.audio);
    let mut rack2 = PluginRack::new(Arc::new(testplugin::catalog()), SR as f32);
    let (l2, _) = render(&doubled, &mut rack2, 0.5);
    let two = rms(&l2[4000..]);
    assert!((two / one - 2.0).abs() < 0.02, "one {one} two {two}");

    s.apply(Command::SetPluginBypass {
        track: su.audio,
        plugin: su.fx,
        bypass: true,
    })
    .unwrap();
    let byp = only_track(s.project(), su.audio);
    let mut rack3 = PluginRack::new(Arc::new(testplugin::catalog()), SR as f32);
    let (l3, _) = render(&byp, &mut rack3, 0.5);
    let dry = rms(&l3[4000..]);
    assert!((dry / one - 1.0).abs() < 0.02, "one {one} dry {dry}");
}

#[test]
fn missing_plugin_keeps_its_slot_instrument_is_silent_effect_passes_through() {
    let su = setup();
    let empty = Arc::new(woa_plugins::Catalog::empty());
    let mut rack = PluginRack::new(empty, SR as f32);
    let project = su.session.project().clone();
    let (l, _) = render(&project, &mut rack, 0.5);
    // Der Effekt reicht das Signal durch, das Instrument schweigt: nur Gleichspannung bleibt.
    assert!(rms(&l[4000..]) > 0.01);
    assert_eq!(rack.status(su.inst), Some(SlotStatus::Missing));
    assert_eq!(rack.status(su.fx), Some(SlotStatus::Missing));
    let only_midi = only_track(&project, su.midi);
    let (m, _) = render(&only_midi, &mut rack, 0.5);
    assert_eq!(rms(&m), 0.0);
    // Die Plätze sind im Projekt geblieben.
    assert_eq!(project.track(su.midi).map(|t| t.plugins.len()), Some(1));
}

#[test]
fn state_is_captured_into_the_project_and_restored() {
    let su = setup();
    let mut rack = PluginRack::new(Arc::new(testplugin::catalog()), SR as f32);
    let mut s = su.session;
    // Reglerwert im laufenden Plugin setzen, wie ihn der Audio-Thread anwendet.
    s.apply(Command::SetPluginParam {
        track: su.audio,
        plugin: su.fx,
        param: GAIN,
        value: 1.5,
    })
    .unwrap();
    let project = s.project().clone();
    let _ = render(&only_track(&project, su.audio), &mut rack, 0.1);
    let cmds = rack.capture_state(&project);
    assert!(!cmds.is_empty());
    for c in cmds {
        s.apply(c).unwrap();
    }
    let slot = s.project().track(su.audio).unwrap().plugins[0].clone();
    assert!(slot.state.is_some());
    // Zweites Erfassen ändert nichts mehr.
    assert!(rack.capture_state(s.project()).is_empty());

    // Neues Rack (anderer Start), Zustand kommt aus dem Projekt.
    let mut fresh = PluginRack::new(Arc::new(testplugin::catalog()), SR as f32);
    fresh.sync(s.project());
    assert_eq!(fresh.param_value(su.fx, GAIN), Some(1.5));
    let _ = PluginParam {
        id: GAIN,
        value: 1.5,
    };
}

#[test]
fn plugin_graph_does_not_allocate_in_the_audio_thread() {
    let su = setup();
    let project = su.session.project().clone();
    let mut rack = PluginRack::new(Arc::new(testplugin::catalog()), SR as f32);
    rack.sync(&project);
    let (mut engine, mut proc_): (Engine, AudioProcessor) = Engine::new(16);
    let graph = clip_graph_plugins(
        &project,
        &engine.transport(),
        SR as f32,
        &mut |p| (p == "dc.wav").then(dc),
        None,
        None,
        false,
        false,
        &mut |t| rack.track_plugins(t),
    );
    engine.set_graph(graph.builder, SR as f32, BLOCK).unwrap();
    engine.set_tempo(project.tempo_bpm).unwrap();
    engine.play().unwrap();
    let mut l = vec![0.0f32; BLOCK];
    let mut r = vec![0.0f32; BLOCK];
    // Anwärmen: das Plugin startet beim ersten Block.
    proc_.process_stereo(&mut l, &mut r);
    let mut peak = 0.0f32;
    for _ in 0..200 {
        assert_no_alloc(|| proc_.process_stereo(&mut l, &mut r));
        peak = peak.max(l.iter().fold(0.0, |a, v| a.max(v.abs())));
    }
    assert!(peak > 0.01, "silent graph");
}
