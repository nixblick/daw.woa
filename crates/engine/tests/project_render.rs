//! Projekt-Render (woa-model -> Stereo-Mix), Transport und Golden-Test.
//! Jeder Process-Aufruf läuft unter `assert_no_alloc`.

use std::path::PathBuf;

use assert_no_alloc::{AllocDisabler, assert_no_alloc};
use woa_dsp::{Node, SineOsc};
use woa_engine::{
    AudioProcessor, Engine, LoopRange, ProjectGraph, build_project_graph, read_wav_stereo,
    render_project, write_wav_stereo,
};
use woa_model::{Command, Project, Session, Track, TrackKind};

#[global_allocator]
static ALLOC: AllocDisabler = AllocDisabler;

const SR: u32 = 48_000;
const BLOCK: usize = 128;

/// Testquelle: Frequenz aus dem Spurnamen, Bus-Spuren ohne Quelle.
fn sine_source(t: &Track) -> Option<Box<dyn Node>> {
    if t.kind == TrackKind::Bus {
        return None;
    }
    let freq = t.name.parse::<f32>().unwrap_or(1000.0);
    Some(Box::new(SineOsc::new(freq, 0.5)))
}

/// Projekt: 220 Hz (-6 dB, Pan -0.5), 330 Hz (Pan +0.7), 440 Hz stumm, ein Bus.
fn demo_project() -> Project {
    let mut s = Session::new("golden");
    let a = s.new_track_id();
    s.apply(Command::add_track(a, "220", TrackKind::Audio))
        .expect("add");
    s.apply(Command::SetGain {
        track: a,
        gain_db: -6.0,
    })
    .expect("gain");
    s.apply(Command::SetPan {
        track: a,
        pan: -0.5,
    })
    .expect("pan");
    let b = s.new_track_id();
    s.apply(Command::add_track(b, "330", TrackKind::Audio))
        .expect("add");
    s.apply(Command::SetPan { track: b, pan: 0.7 })
        .expect("pan");
    let c = s.new_track_id();
    s.apply(Command::add_track(c, "440", TrackKind::Audio))
        .expect("add");
    s.apply(Command::SetMute {
        track: c,
        mute: true,
    })
    .expect("mute");
    let d = s.new_track_id();
    s.apply(Command::add_track(d, "Bus", TrackKind::Bus))
        .expect("add");
    s.project().clone()
}

fn setup(project: &Project) -> (Engine, AudioProcessor, ProjectGraph) {
    let (mut e, p) = Engine::new(64);
    let g = build_project_graph(project, &mut sine_source);
    let tracks = g.tracks.clone();
    e.set_graph(g.builder, SR as f32, BLOCK).expect("compiles");
    e.set_tempo(project.tempo_bpm).expect("tempo");
    (
        e,
        p,
        ProjectGraph {
            builder: woa_engine::GraphBuilder::new(),
            tracks,
            bus_meters: Default::default(),
            reductions: Default::default(),
        },
    )
}

fn render_no_alloc(p: &mut AudioProcessor, frames: usize, block: usize) -> (Vec<f32>, Vec<f32>) {
    let mut l = vec![0.0f32; frames];
    let mut r = vec![0.0f32; frames];
    for (a, b) in l.chunks_mut(block).zip(r.chunks_mut(block)) {
        assert_no_alloc(|| p.process_stereo(a, b));
    }
    (l, r)
}

fn peak(x: &[f32]) -> f32 {
    x.iter().fold(0.0f32, |m, s| m.max(s.abs()))
}

#[test]
fn golden_project_mix() {
    let project = demo_project();
    let (mut e, mut p, _) = setup(&project);
    e.play().expect("play");
    let (l, r) = render_no_alloc(&mut p, SR as usize / 2, BLOCK);
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/golden/project_mix_2tracks_05s.wav");
    if std::env::var_os("WOA_BLESS").is_some() {
        write_wav_stereo(&path, &l, &r, SR).expect("write golden");
    }
    let (gl, gr, rate) = read_wav_stereo(&path).expect("golden exists (WOA_BLESS=1)");
    assert_eq!(rate, SR);
    assert_eq!(gl.len(), l.len());
    let err = gl
        .iter()
        .zip(&l)
        .chain(gr.iter().zip(&r))
        .map(|(a, b)| (a - b).abs())
        .fold(0.0f32, f32::max);
    assert!(err < 1e-5, "max deviation {err}");
    // Convenience-Pfad liefert dasselbe.
    let (l2, r2) = render_project(
        &project,
        &mut sine_source,
        SR as f32,
        SR as usize / 2,
        BLOCK,
    )
    .expect("render");
    assert_eq!(l2, l);
    assert_eq!(r2, r);
    assert_eq!(e.transport().position(), SR as u64 / 2);
}

#[test]
fn hard_left_is_silent_right() {
    let mut s = Session::new("pan");
    let a = s.new_track_id();
    s.apply(Command::add_track(a, "300", TrackKind::Audio))
        .expect("add");
    s.apply(Command::SetPan {
        track: a,
        pan: -1.0,
    })
    .expect("pan");
    let (_e, mut p, _) = setup(s.project());
    let (l, r) = render_no_alloc(&mut p, 4800, BLOCK);
    assert!(peak(&r) < 1e-6, "right {}", peak(&r));
    assert!((peak(&l) - 0.5).abs() < 1e-2);
}

#[test]
fn solo_silences_others_and_update_is_smoothed() {
    let mut s = Session::new("solo");
    let a = s.new_track_id();
    s.apply(Command::add_track(a, "200", TrackKind::Audio))
        .expect("add");
    let b = s.new_track_id();
    s.apply(Command::add_track(b, "5000", TrackKind::Audio))
        .expect("add");
    s.apply(Command::SetSolo {
        track: a,
        solo: true,
    })
    .expect("solo");
    let (mut e, mut p, g) = setup(s.project());
    let (l, _) = render_no_alloc(&mut p, 4800, BLOCK);
    // Nur 200 Hz bei Pan Mitte: Spitze 0.5 * cos(pi/4).
    assert!((peak(&l) - 0.5 * std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-2);
    // Solo aus: beide Spuren, geglättet.
    s.apply(Command::SetSolo {
        track: a,
        solo: false,
    })
    .expect("solo off");
    ProjectGraph::update(&g.tracks, &mut e, s.project()).expect("update");
    let (l2, _) = render_no_alloc(&mut p, 4800, BLOCK);
    let max_slope = std::f32::consts::TAU * (200.0 + 5000.0) / SR as f32 * 0.36 + 1e-3;
    let mut prev = l[l.len() - 1];
    for &x in &l2[..960] {
        assert!((x - prev).abs() <= max_slope, "jump {prev} -> {x}");
        prev = x;
    }
    assert!(peak(&l2[2400..]) > 0.5);
}

#[test]
fn transport_position_play_stop_seek() {
    let project = demo_project();
    let (mut e, mut p, _) = setup(&project);
    let t = e.transport();
    let _ = render_no_alloc(&mut p, 1000, 333);
    assert_eq!(t.position(), 0, "stopped transport stays");
    assert!(!t.is_playing());
    e.play().expect("play");
    let _ = render_no_alloc(&mut p, 1000, 333);
    assert!(t.is_playing());
    assert_eq!(t.position(), 1000);
    e.stop().expect("stop");
    let _ = render_no_alloc(&mut p, 500, 333);
    assert_eq!(t.position(), 1000);
    e.seek(24_000).expect("seek");
    let _ = render_no_alloc(&mut p, 1, 1);
    assert_eq!(t.position(), 24_000);
    assert!((t.tempo_bpm() - 120.0).abs() < 1e-12);
}

#[test]
fn loop_wraps_exactly_with_odd_blocks() {
    let project = demo_project();
    let (mut e, mut p, _) = setup(&project);
    let t = e.transport();
    // 1 Beat bei 120 BPM = 24000 Samples; Loop Beat 1..2.
    e.set_loop_beats(1.0, 2.0, SR as f32).expect("loop");
    e.seek(24_000).expect("seek");
    e.play().expect("play");
    let _ = render_no_alloc(&mut p, 24_000 + 777, 333);
    assert_eq!(t.position(), 24_000 + 777);
    assert!(e.set_loop_beats(2.0, 1.0, SR as f32).is_err());
    e.set_loop(LoopRange::new(0, 100)).expect("loop");
    e.seek(0).expect("seek");
    let _ = render_no_alloc(&mut p, 250, 333);
    assert_eq!(t.position(), 50);
}

#[test]
fn underrun_counter_starts_at_zero() {
    let (e, _p) = Engine::new(4);
    assert_eq!(e.underruns(), 0);
    e.transport().count_underrun();
    assert_eq!(e.underruns(), 1);
}
