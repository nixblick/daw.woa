//! Paket J1 ohne Ton: Eine MIDI-Spur mit GM-Klang spielt über die SoundFont und klingt
//! anders als der eingebaute Synth. Die SoundFont ist eine winzige, im Test erzeugte SF2,
//! gefunden über `WOA_SOUNDFONT` wie in der App. Jeder Process-Aufruf unter `assert_no_alloc`.

use assert_no_alloc::{AllocDisabler, assert_no_alloc};
use woa_engine::clip::{clip_graph, project_end_seconds};
use woa_engine::{AudioProcessor, Engine};
use woa_model::{
    Clip, Command, MidiData, MidiNote, Project, Session, TrackId, TrackKind, TrackSound,
};

#[global_allocator]
static ALLOC: AllocDisabler = AllocDisabler;

const SR: u32 = 48_000;
const BLOCK: usize = 256;

fn project(sound: Option<TrackSound>) -> Project {
    let mut s = Session::new("klang");
    let t: TrackId = s.new_track_id();
    s.apply(Command::add_track(t, "Keys", TrackKind::Midi))
        .unwrap();
    let midi = MidiData {
        notes: vec![MidiNote {
            beat: 0.0,
            length_beats: 1.0,
            pitch: 69,
            velocity: 110,
        }],
        cc: Vec::new(),
    };
    let c = s.new_clip_id();
    s.apply(Command::AddClip {
        track: t,
        clip: Clip::new_midi(c, "Ton", 0.0, 2.0, midi),
        index: None,
    })
    .unwrap();
    s.apply(Command::SetTrackSound { track: t, sound }).unwrap();
    s.project().clone()
}

fn render(project: &Project) -> Vec<f32> {
    let (mut engine, mut proc_) = Engine::new(16);
    let graph = clip_graph(project, &engine.transport(), SR as f32, &mut |_| None, None);
    engine.set_graph(graph.builder, SR as f32, BLOCK).unwrap();
    engine.set_tempo(project.tempo_bpm).unwrap();
    engine.play().unwrap();
    let frames = (project_end_seconds(project) * f64::from(SR)) as usize;
    let out = run(&mut proc_, frames);
    engine.collect();
    out
}

fn run(proc_: &mut AudioProcessor, frames: usize) -> Vec<f32> {
    let mut l = vec![0.0f32; frames];
    let mut r = vec![0.0f32; frames];
    for (a, b) in l.chunks_mut(BLOCK).zip(r.chunks_mut(BLOCK)) {
        assert_no_alloc(|| proc_.process_stereo(a, b));
    }
    l
}

fn peak(x: &[f32]) -> f32 {
    x.iter().fold(0.0f32, |m, v| m.max(v.abs()))
}

#[test]
fn gm_sound_plays_the_soundfont_not_the_synth() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tiny.sf2");
    std::fs::write(&path, woa_dsp::soundfont::tiny_sf2()).unwrap();
    // Einziger Test dieser Datei: kein anderer Thread liest die Umgebung.
    unsafe { std::env::set_var(woa_engine::soundfont::SOUNDFONT_ENV, &path) };
    assert!(woa_engine::soundfont::default_soundfont().is_some());

    let synth = render(&project(None));
    let font = render(&project(Some(TrackSound {
        program: 0,
        drums: false,
    })));
    assert_eq!(synth.len(), font.len());
    assert!(peak(&synth) > 0.05, "synth audible");
    assert!(peak(&font) > 0.01, "soundfont audible: {}", peak(&font));
    let diff = synth
        .iter()
        .zip(&font)
        .map(|(a, b)| (a - b).abs())
        .fold(0.0f32, f32::max);
    assert!(
        diff > 0.05,
        "soundfont must sound different, max diff {diff}"
    );
}
