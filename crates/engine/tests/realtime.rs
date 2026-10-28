//! Echtzeit- und Klangtests: jeder Process-Aufruf unter `assert_no_alloc`.

use std::path::PathBuf;

use assert_no_alloc::{AllocDisabler, assert_no_alloc};
use woa_dsp::{Gain, SineOsc, gain_params, sine_params};
use woa_engine::{AudioProcessor, Engine, GraphBuilder, NodeId, read_wav, write_wav};

#[global_allocator]
static ALLOC: AllocDisabler = AllocDisabler;

const SR: u32 = 48_000;
const BLOCK: usize = 128;

fn sine_gain_engine(freq: f32, gain: f32) -> (Engine, AudioProcessor, NodeId, NodeId) {
    let (mut engine, proc_) = Engine::new(64);
    let mut b = GraphBuilder::new();
    let osc = b.add_node(SineOsc::new(freq, 1.0));
    let g = b.add_node(Gain::new(gain));
    b.connect(osc, g);
    b.set_output(g);
    engine
        .set_graph(b, SR as f32, BLOCK)
        .expect("graph compiles");
    (engine, proc_, osc, g)
}

/// Rendert block-weise, jeder Process-Aufruf ohne Allokation.
fn render_no_alloc(p: &mut AudioProcessor, frames: usize) -> Vec<f32> {
    let mut out = vec![0.0f32; frames];
    for chunk in out.chunks_mut(BLOCK) {
        assert_no_alloc(|| p.process(chunk));
    }
    out
}

fn golden_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/golden")
}

#[test]
fn golden_sine_1s() {
    let (_e, mut p, _, _) = sine_gain_engine(440.0, 0.5);
    let rendered = render_no_alloc(&mut p, SR as usize);
    let path = golden_dir().join("sine_440hz_gain05_1s.wav");
    if std::env::var_os("WOA_BLESS").is_some() {
        write_wav(&path, &rendered, SR).expect("write golden");
    }
    let (golden, rate) = read_wav(&path).expect("golden file exists (WOA_BLESS=1 to create)");
    assert_eq!(rate, SR);
    assert_eq!(golden.len(), rendered.len());
    let max_err = golden
        .iter()
        .zip(&rendered)
        .map(|(a, b)| (a - b).abs())
        .fold(0.0f32, f32::max);
    assert!(max_err < 1e-5, "max deviation {max_err}");
    let peak = rendered.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    assert!((peak - 0.5).abs() < 1e-3, "peak {peak}");
}

#[test]
fn gain_change_is_smoothed_without_jump() {
    // Gain springt von 0 auf 1: |Δ| zwischen Samples muss klein bleiben.
    let (mut e, mut p, _, g) = sine_gain_engine(100.0, 0.0);
    let _ = render_no_alloc(&mut p, 1024);
    e.set_param(g, gain_params::GAIN, 1.0).expect("queue");
    let out = render_no_alloc(&mut p, 4096);
    // Ungeglättet: Sprung von 0 auf bis zu sin(...)≈1 in einem Sample.
    // Geglättet (20 ms = 960 Samples): Δ ≤ Sinussteigung + Rampenschritt.
    let max_slope = (std::f32::consts::TAU * 100.0 / SR as f32) + 1.0 / 960.0 + 1e-4;
    let mut prev = 0.0f32;
    for (i, &s) in out.iter().enumerate() {
        assert!((s - prev).abs() <= max_slope, "jump at {i}: {prev} -> {s}");
        prev = s;
    }
    let peak = out[2048..].iter().fold(0.0f32, |m, s| m.max(s.abs()));
    assert!(peak > 0.99, "gain reached target, peak {peak}");
}

#[test]
fn frequency_change_is_continuous() {
    let (mut e, mut p, osc, _) = sine_gain_engine(100.0, 1.0);
    let before = render_no_alloc(&mut p, 1000);
    e.set_param(osc, sine_params::FREQUENCY, 2000.0)
        .expect("queue");
    let after = render_no_alloc(&mut p, 2000);
    let max_slope = std::f32::consts::TAU * 2000.0 / SR as f32 + 1e-3;
    let mut prev = before[before.len() - 1];
    for &s in &after {
        assert!((s - prev).abs() <= max_slope);
        prev = s;
    }
}

#[test]
fn graph_swap_in_audio_thread_does_not_allocate() {
    let (mut e, mut p, _, _) = sine_gain_engine(440.0, 0.5);
    let _ = render_no_alloc(&mut p, 512);
    let mut b = GraphBuilder::new();
    let osc = b.add_node(SineOsc::new(220.0, 0.25));
    b.set_output(osc);
    e.set_graph(b, SR as f32, BLOCK).expect("graph compiles");
    // Tausch + Drop des alten Graphen passieren im process-Aufruf.
    let out = render_no_alloc(&mut p, 512);
    let peak = out.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    assert!(peak <= 0.2501 && peak > 0.2);
    e.collect();
}

#[test]
fn no_graph_renders_silence() {
    let (_e, mut p) = Engine::new(4);
    let out = render_no_alloc(&mut p, 256);
    assert!(out.iter().all(|&s| s == 0.0));
}

#[test]
fn offline_render_and_wav_roundtrip() {
    let (_e, mut p, _, _) = sine_gain_engine(440.0, 0.5);
    let rendered = woa_engine::render_offline(&mut p, 4800, 333);
    let dir = std::env::temp_dir().join(format!("woa-engine-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("tmp dir");
    let path = dir.join("roundtrip.wav");
    write_wav(&path, &rendered, SR).expect("write");
    let (back, rate) = read_wav(&path).expect("read");
    std::fs::remove_file(&path).ok();
    std::fs::remove_dir(&dir).ok();
    assert_eq!(rate, SR);
    assert_eq!(back, rendered);
}
