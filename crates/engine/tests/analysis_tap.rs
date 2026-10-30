//! Paket 19: Abgriffe fürs Analyse-Fenster im Projekt-Graphen. Sie ändern das Signal nicht,
//! liefern Master und jede Spur, und jeder Process-Aufruf läuft unter `assert_no_alloc`.

use assert_no_alloc::{AllocDisabler, assert_no_alloc};
use woa_dsp::analysis::loudness::LoudnessMeter;
use woa_dsp::analysis::stereo::correlation;
use woa_dsp::{Node, SineOsc};
use woa_engine::{
    AudioProcessor, Engine, ProjectGraph, build_project_graph, build_project_graph_taps,
};
use woa_model::{Command, Project, Session, Track, TrackId, TrackKind};

#[global_allocator]
static ALLOC: AllocDisabler = AllocDisabler;

const SR: u32 = 48_000;
const BLOCK: usize = 128;

fn sine_source(t: &Track) -> Option<Box<dyn Node>> {
    if t.kind == TrackKind::Bus {
        return None;
    }
    let freq = t.name.parse::<f32>().unwrap_or(1000.0);
    Some(Box::new(SineOsc::new(freq, 0.5)))
}

fn live(project: &Project, graph: ProjectGraph) -> (Engine, AudioProcessor, ProjectGraph) {
    let (mut engine, processor) = Engine::new(1024);
    let mut graph = graph;
    let builder = std::mem::replace(&mut graph.builder, woa_engine::GraphBuilder::new());
    engine
        .set_graph(builder, SR as f32, BLOCK)
        .expect("compiles");
    engine.set_tempo(project.tempo_bpm).expect("tempo");
    engine.play().expect("play");
    (engine, processor, graph)
}

fn run(processor: &mut AudioProcessor, frames: usize) -> (Vec<f32>, Vec<f32>) {
    let mut l = vec![0.0f32; frames];
    let mut r = vec![0.0f32; frames];
    for (a, b) in l.chunks_mut(BLOCK).zip(r.chunks_mut(BLOCK)) {
        assert_no_alloc(|| processor.process_stereo(a, b));
    }
    (l, r)
}

fn track(s: &mut Session, name: &str, pan: f64) -> TrackId {
    let id = s.new_track_id();
    s.apply(Command::add_track(id, name, TrackKind::Audio))
        .expect("add");
    s.apply(Command::SetPan { track: id, pan }).expect("pan");
    id
}

#[test]
fn taps_do_not_change_the_signal_and_deliver_master() {
    let mut s = Session::new("tap");
    track(&mut s, "440", 0.0);
    track(&mut s, "1000", 0.5);
    let p = s.project();
    let (_e, mut plain, _) = live(p, build_project_graph(p, &mut sine_source));
    let (_e2, mut tapped, mut g) = live(
        p,
        build_project_graph_taps(p, &mut sine_source, false, true),
    );
    let (pl, pr) = run(&mut plain, 24_000);
    let master = g.taps.get_mut(&None).expect("master tap");
    master.set_enabled(true);
    // Eingeschaltet wird zwischen den Blöcken; ab jetzt kommt alles an.
    let (tl, tr) = run(&mut tapped, 24_000);
    assert_eq!((&pl, &pr), (&tl, &tr), "Abgriffe verändern das Signal");
    let (mut a, mut b) = (Vec::new(), Vec::new());
    let lost = master.drain(&mut a, &mut b);
    assert_eq!(lost, 0);
    assert_eq!((a, b), (tl, tr));
}

#[test]
fn track_taps_follow_pan_and_stay_silent_when_off() {
    let mut s = Session::new("tap2");
    let left = track(&mut s, "1000", -1.0);
    let right = track(&mut s, "440", 1.0);
    let p = s.project();
    let (_e, mut proc, mut g) = live(
        p,
        build_project_graph_taps(p, &mut sine_source, false, true),
    );
    g.taps.get_mut(&Some(left)).expect("tap").set_enabled(true);
    run(&mut proc, 12_000);
    let (mut l, mut r) = (Vec::new(), Vec::new());
    g.taps
        .get_mut(&Some(left))
        .expect("tap")
        .drain(&mut l, &mut r);
    assert!(l.iter().any(|v| v.abs() > 0.1));
    assert!(r.iter().all(|v| v.abs() < 1e-6), "hart links: rechts still");
    // Die andere Spur war aus: nichts angekommen.
    let (mut l2, mut r2) = (Vec::new(), Vec::new());
    g.taps
        .get_mut(&Some(right))
        .expect("tap")
        .drain(&mut l2, &mut r2);
    assert!(l2.is_empty() && r2.is_empty());
}

/// Weites Stereobild: zwei verschiedene Töne links und rechts sind unkorreliert; derselbe Ton
/// in der Mitte ist Mono (+1) und misst die erwartete Lautheit.
#[test]
fn master_tap_measures_correlation_and_loudness() {
    let mut wide = Session::new("wide");
    track(&mut wide, "440", -1.0);
    track(&mut wide, "1000", 1.0);
    let p = wide.project();
    let (_e, mut proc, mut g) = live(
        p,
        build_project_graph_taps(p, &mut sine_source, false, true),
    );
    g.taps.get_mut(&None).expect("master").set_enabled(true);
    run(&mut proc, SR as usize);
    let (mut l, mut r) = (Vec::new(), Vec::new());
    g.taps.get_mut(&None).expect("master").drain(&mut l, &mut r);
    assert!(correlation(&l, &r).expect("signal").abs() < 0.1);

    let mut mono = Session::new("mono");
    track(&mut mono, "1000", 0.0);
    let p = mono.project();
    let (_e, mut proc, mut g) = live(
        p,
        build_project_graph_taps(p, &mut sine_source, false, true),
    );
    g.taps.get_mut(&None).expect("master").set_enabled(true);
    let mut meter = LoudnessMeter::new(SR);
    let (mut l, mut r) = (Vec::new(), Vec::new());
    for _ in 0..5 {
        run(&mut proc, SR as usize);
        g.taps.get_mut(&None).expect("master").drain(&mut l, &mut r);
        meter.push(&l, &r);
        l.clear();
        r.clear();
    }
    assert!(meter.integrated().is_some());
    // Der Ton in der Mitte ist auf beiden Seiten gleich: +1.
    run(&mut proc, 4800);
    g.taps.get_mut(&None).expect("master").drain(&mut l, &mut r);
    assert!((correlation(&l, &r).expect("signal") - 1.0).abs() < 1e-9);
}
