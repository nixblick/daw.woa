//! Paket 12 ohne Ton: MIDI-Clips im Projekt rendern offline über das Spur-Instrument,
//! jede Note setzt an der richtigen Stelle ein (Onset-Prüfung auf der exportierten WAV),
//! Stopp lässt nichts hängen, und der Audio-Thread meldet die Sample-Uhr für die Aufnahme.
//! Jeder Process-Aufruf läuft unter `assert_no_alloc`.

use std::sync::Arc;

use assert_no_alloc::{AllocDisabler, assert_no_alloc};
use woa_engine::clip::{clip_graph, project_end_seconds, render_clips};
use woa_engine::{AudioProcessor, Engine, read_wav_stereo};
use woa_model::{Clip, Command, MidiCc, MidiData, MidiNote, Project, Session, TrackKind};

#[global_allocator]
static ALLOC: AllocDisabler = AllocDisabler;

const SR: u32 = 48_000;
const BLOCK: usize = 256;

/// 120 bpm: ein Beat = 0,5 s = 24 000 Samples. Clip ab Beat 2 (1 s), Noten auf den
/// Beats 0, 1, 2 und 3 des Clips, je eine Zweiunddreißigstel lang.
fn project() -> (Project, Vec<u64>) {
    let mut s = Session::new("midi");
    let t = s.new_track_id();
    s.apply(Command::add_track(t, "Piano", TrackKind::Midi))
        .unwrap();
    let beats = [0.0, 1.0, 2.0, 3.0];
    let notes = beats
        .iter()
        .zip([60u8, 64, 67, 72])
        .map(|(&beat, pitch)| MidiNote {
            beat,
            length_beats: 0.125,
            pitch,
            velocity: 100,
        })
        .collect();
    let midi = MidiData {
        notes,
        cc: vec![MidiCc {
            beat: 3.5,
            controller: 64,
            value: 0,
        }],
    };
    let c = s.new_clip_id();
    s.apply(Command::AddClip {
        track: t,
        clip: Clip::new_midi(c, "Phrase", 2.0, 4.0, midi),
        index: None,
    })
    .unwrap();
    let expected = beats
        .iter()
        .map(|b| ((2.0 + b) * 24_000.0) as u64)
        .collect();
    (s.project().clone(), expected)
}

/// Erster Sample-Index ab `from`, dessen Betrag über `threshold` liegt.
fn onset(x: &[f32], from: usize, threshold: f32) -> Option<usize> {
    x[from..]
        .iter()
        .position(|v| v.abs() > threshold)
        .map(|i| i + from)
}

fn render(proc_: &mut AudioProcessor, frames: usize) -> (Vec<f32>, Vec<f32>) {
    let mut l = vec![0.0f32; frames];
    let mut r = vec![0.0f32; frames];
    for (a, b) in l.chunks_mut(BLOCK).zip(r.chunks_mut(BLOCK)) {
        assert_no_alloc(|| proc_.process_stereo(a, b));
    }
    (l, r)
}

#[test]
fn midi_clip_notes_start_on_their_samples() {
    let (project, expected) = project();
    let (mut engine, mut proc_) = Engine::new(16);
    let graph = clip_graph(
        &project,
        &engine.transport(),
        SR as f32,
        &mut |_| None,
        None,
    );
    assert_eq!(graph.tracks.len(), 1, "MIDI track has a source");
    engine.set_graph(graph.builder, SR as f32, BLOCK).unwrap();
    engine.set_tempo(project.tempo_bpm).unwrap();
    engine.play().unwrap();
    let seconds = project_end_seconds(&project);
    assert!((seconds - 3.0).abs() < 1e-9);
    let (l, _) = render(&mut proc_, (seconds * f64::from(SR)) as usize);
    assert!(
        l[..47_990].iter().all(|v| *v == 0.0),
        "silent before the clip"
    );
    for &e in &expected {
        let e = e as usize;
        let got = onset(&l, e - 2_000, 1e-4).unwrap();
        assert!(
            got.abs_diff(e) <= 2,
            "note expected at {e}, starts at {got}"
        );
    }
    engine.collect();
}

#[test]
fn exported_wav_has_the_notes_in_place() {
    let (project, expected) = project();
    let (l, r) = render_clips(&project, SR, project_end_seconds(&project), &mut |_| None).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("midi.wav");
    woa_engine::export::export_stereo(&path, &l, &r, SR).unwrap();
    let (fl, fr, rate) = read_wav_stereo(&path).unwrap();
    assert_eq!(rate, SR);
    assert_eq!(fl.len(), 144_000);
    let peak = fl.iter().chain(&fr).fold(0.0f32, |m, v| m.max(v.abs()));
    assert!(peak > 0.05, "audible: {peak}");
    for &e in &expected {
        let e = e as usize;
        let got = onset(&fl, e - 2_000, 1e-4).unwrap();
        // 1 ms = 48 Samples; tatsächlich liegt der Einsatz auf dem Sample.
        assert!(
            got.abs_diff(e) <= 48,
            "note expected at {e}, starts at {got}"
        );
    }
}

#[test]
fn stop_and_seek_leave_no_hanging_note() {
    let (mut project, _) = project();
    // Eine lange Note über den ganzen Clip.
    project.tracks[0].clips[0].midi = Some(MidiData {
        notes: vec![MidiNote {
            beat: 0.0,
            length_beats: 4.0,
            pitch: 60,
            velocity: 100,
        }],
        cc: vec![],
    });
    let (mut engine, mut proc_) = Engine::new(16);
    let graph = clip_graph(
        &project,
        &engine.transport(),
        SR as f32,
        &mut |_| None,
        None,
    );
    engine.set_graph(graph.builder, SR as f32, BLOCK).unwrap();
    engine.set_tempo(project.tempo_bpm).unwrap();
    engine.seek(60_000).unwrap();
    engine.play().unwrap();
    let (l, _) = render(&mut proc_, 4_800);
    assert!(
        l.iter().all(|v| v.abs() < 1e-6),
        "a note that began before the seek point is not restarted"
    );
    engine.seek(40_000).unwrap();
    let (l, _) = render(&mut proc_, 48_000);
    assert!(
        l[9_000..].iter().any(|v| v.abs() > 0.01),
        "note plays from 48 000"
    );
    engine.stop().unwrap();
    let (l, _) = render(&mut proc_, 24_000);
    assert!(
        l[l.len() - 2_000..].iter().all(|v| v.abs() < 1e-6),
        "stopped: note released"
    );
    engine.collect();
}

#[test]
fn audio_thread_publishes_the_sample_clock() {
    let (mut engine, mut proc_) = Engine::new(16);
    let clock = engine.transport().clock();
    let mut b = woa_engine::GraphBuilder::new();
    let n = b.add_node(woa_dsp::Gain::new(1.0));
    b.set_output(n);
    engine.set_graph(b, SR as f32, BLOCK).unwrap();
    engine.seek(1_000).unwrap();
    engine.play().unwrap();
    let mut out = vec![0.0f32; BLOCK];
    for _ in 0..4 {
        assert_no_alloc(|| proc_.process(&mut out));
    }
    let r = clock.read().expect("published");
    assert!(r.playing);
    assert_eq!(
        r.position,
        1_000 + 3 * BLOCK as u64,
        "block start of the last call"
    );
    assert_eq!(r.rate, SR as f32);
    let _ = Arc::strong_count(&clock);
}

/// Paket 13: Offline-Render vor und nach dem Quantisieren. Vorher setzen die Noten
/// daneben ein, nachher auf dem Viertel-Raster (höchstens 1 ms = 48 Samples daneben).
#[test]
fn quantize_moves_rendered_onsets_onto_the_grid() {
    let (mut project, expected) = project();
    let off = [0.07, 1.12, 1.93, 3.04];
    let midi = project.tracks[0].clips[0].midi.as_mut().unwrap();
    for (n, b) in midi.notes.iter_mut().zip(off) {
        n.beat = b;
    }
    let (q, _) = midi.quantize(&[], woa_model::grid_beats(4, false), false);
    let before = project.clone();
    project.tracks[0].clips[0].midi = Some(q);
    let end = project_end_seconds(&before);
    let (l0, _) = render_clips(&before, SR, end, &mut |_| None).unwrap();
    let (l1, _) = render_clips(&project, SR, end, &mut |_| None).unwrap();
    for (i, (&e, b)) in expected.iter().zip(off).enumerate() {
        let e = e as usize;
        let was = ((2.0 + b) * 24_000.0) as usize;
        let from = e.min(was) - 2_000;
        let got0 = onset(&l0, from, 1e-4).unwrap();
        let got1 = onset(&l1, from, 1e-4).unwrap();
        assert!(got0.abs_diff(was) <= 48, "note {i} before: {got0} vs {was}");
        assert!(got0.abs_diff(e) > 500, "note {i} was off the grid");
        assert!(got1.abs_diff(e) <= 48, "note {i} after: {got1} vs {e}");
    }
}
