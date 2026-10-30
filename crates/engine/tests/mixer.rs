//! Paket 16: Busse, Sends und Inserts im Graphen. Jeder Process-Aufruf läuft unter
//! `assert_no_alloc`; der Golden-Test hält den Klang eines Mixes mit Bus, Send und Effekten fest.

use std::path::PathBuf;

use assert_no_alloc::{AllocDisabler, assert_no_alloc};
use woa_dsp::{Node, SineOsc};
use woa_engine::{
    AudioProcessor, Engine, ProjectGraph, build_project_graph, read_wav_stereo, write_wav_stereo,
};
use woa_model::{
    Command, EffectKind, Insert, InsertId, Project, Send, Session, Track, TrackId, TrackKind,
};

#[global_allocator]
static ALLOC: AllocDisabler = AllocDisabler;

const SR: u32 = 48_000;
const BLOCK: usize = 128;

/// Frequenz aus dem Spurnamen, Amplitude 0.5; Busse ohne Quelle.
fn sine_source(t: &Track) -> Option<Box<dyn Node>> {
    if t.kind == TrackKind::Bus {
        return None;
    }
    let freq = t.name.parse::<f32>().unwrap_or(1000.0);
    Some(Box::new(SineOsc::new(freq, 0.5)))
}

struct Rig {
    engine: Engine,
    processor: AudioProcessor,
    graph: ProjectGraph,
}

fn rig(project: &Project) -> Rig {
    let (mut engine, processor) = Engine::new(1024);
    let g = build_project_graph(project, &mut sine_source);
    let tracks = g.tracks.clone();
    let bus_meters = g.bus_meters.clone();
    engine
        .set_graph(g.builder, SR as f32, BLOCK)
        .expect("compiles");
    engine.set_tempo(project.tempo_bpm).expect("tempo");
    engine.play().expect("play");
    Rig {
        engine,
        processor,
        graph: ProjectGraph {
            builder: woa_engine::GraphBuilder::new(),
            tracks,
            bus_meters,
        },
    }
}

fn render(r: &mut Rig, frames: usize) -> (Vec<f32>, Vec<f32>) {
    let mut l = vec![0.0f32; frames];
    let mut rr = vec![0.0f32; frames];
    for (a, b) in l.chunks_mut(BLOCK).zip(rr.chunks_mut(BLOCK)) {
        assert_no_alloc(|| r.processor.process_stereo(a, b));
    }
    (l, rr)
}

fn render_project(project: &Project, frames: usize) -> (Vec<f32>, Vec<f32>) {
    let mut r = rig(project);
    render(&mut r, frames)
}

fn peak(x: &[f32]) -> f32 {
    x.iter().fold(0.0f32, |m, s| m.max(s.abs()))
}

fn add_track(s: &mut Session, name: &str, kind: TrackKind) -> TrackId {
    let id = s.new_track_id();
    s.apply(Command::add_track(id, name, kind)).expect("add");
    id
}

fn add_insert(s: &mut Session, track: TrackId, kind: EffectKind) -> InsertId {
    let id = s.new_insert_id();
    s.apply(Command::AddInsert {
        track,
        insert: Insert::new(id, kind),
        index: None,
    })
    .expect("insert");
    id
}

fn send(target: TrackId, level_db: f64, pre_fader: bool) -> Send {
    Send {
        target,
        level_db,
        pre_fader,
    }
}

fn max_diff(a: &[f32], b: &[f32]) -> f32 {
    a.iter()
        .zip(b)
        .map(|(x, y)| (x - y).abs())
        .fold(0.0, f32::max)
}

/// 220 Hz mit EQ und Kompressor, Send (-6 dB, nach dem Fader) an den Bus "Hall" mit Limiter;
/// 330 Hz läuft über den Bus "Gruppe"; "Hall" und "Gruppe" gehen auf den Master.
fn golden_project() -> Project {
    let mut s = Session::new("mixer-golden");
    let a = add_track(&mut s, "220", TrackKind::Audio);
    let b = add_track(&mut s, "330", TrackKind::Audio);
    let hall = add_track(&mut s, "Hall", TrackKind::Bus);
    let grp = add_track(&mut s, "Gruppe", TrackKind::Bus);
    s.apply(Command::SetPan {
        track: a,
        pan: -0.4,
    })
    .expect("pan");
    s.apply(Command::SetGain {
        track: a,
        gain_db: -3.0,
    })
    .expect("gain");
    let eq = add_insert(&mut s, a, EffectKind::Eq);
    let bell_hz = EffectKind::Eq.param_index("bell_hz").expect("param");
    let bell_db = EffectKind::Eq.param_index("bell_db").expect("param");
    for (param, value) in [(bell_hz, 220.0), (bell_db, 6.0)] {
        s.apply(Command::SetInsertParam {
            track: a,
            insert: eq,
            param,
            value,
        })
        .expect("eq");
    }
    let comp = add_insert(&mut s, a, EffectKind::Compressor);
    let thr = EffectKind::Compressor
        .param_index("threshold_db")
        .expect("param");
    s.apply(Command::SetInsertParam {
        track: a,
        insert: comp,
        param: thr,
        value: -20.0,
    })
    .expect("comp");
    s.apply(Command::AddSend {
        track: a,
        send: send(hall, -6.0, false),
        index: None,
    })
    .expect("send");
    add_insert(&mut s, hall, EffectKind::Limiter);
    s.apply(Command::SetOutput {
        track: b,
        output: Some(grp),
    })
    .expect("out");
    s.apply(Command::SetPan { track: b, pan: 0.6 })
        .expect("pan");
    s.project().clone()
}

#[test]
fn golden_mixer_with_bus_send_and_inserts() {
    let (l, r) = render_project(&golden_project(), SR as usize / 2);
    assert!(peak(&l) > 0.05 && peak(&r) > 0.05, "silent mix");
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/golden/mixer_bus_send_05s.wav");
    if std::env::var_os("WOA_BLESS").is_some() {
        write_wav_stereo(&path, &l, &r, SR).expect("write golden");
    }
    let (gl, gr, rate) = read_wav_stereo(&path).expect("golden exists (WOA_BLESS=1)");
    assert_eq!(rate, SR);
    assert_eq!(gl.len(), l.len());
    let err = max_diff(&gl, &l).max(max_diff(&gr, &r));
    assert!(err < 1e-5, "max deviation {err}");
}

#[test]
fn bus_in_between_is_transparent() {
    // Spur -> Bus (Fader 0, Balance Mitte) -> Master klingt wie Spur -> Master.
    let mut s = Session::new("thru");
    let a = add_track(&mut s, "440", TrackKind::Audio);
    s.apply(Command::SetPan {
        track: a,
        pan: -0.3,
    })
    .expect("pan");
    let (dl, dr) = render_project(s.project(), 4800);
    let bus = add_track(&mut s, "Bus", TrackKind::Bus);
    s.apply(Command::SetOutput {
        track: a,
        output: Some(bus),
    })
    .expect("out");
    let (bl, br) = render_project(s.project(), 4800);
    assert!(max_diff(&dl, &bl) < 1e-6 && max_diff(&dr, &br) < 1e-6);
}

#[test]
fn post_fader_send_adds_the_send_level() {
    let mut s = Session::new("send");
    let a = add_track(&mut s, "440", TrackKind::Audio);
    s.apply(Command::SetPan { track: a, pan: 0.5 })
        .expect("pan");
    let (dl, dr) = render_project(s.project(), 4800);
    let hall = add_track(&mut s, "Hall", TrackKind::Bus);
    s.apply(Command::AddSend {
        track: a,
        send: send(hall, -6.0, false),
        index: None,
    })
    .expect("send");
    let (l, r) = render_project(s.project(), 4800);
    let k = 1.0 + 10f32.powf(-6.0 / 20.0);
    let want_l: Vec<f32> = dl.iter().map(|x| x * k).collect();
    let want_r: Vec<f32> = dr.iter().map(|x| x * k).collect();
    assert!(max_diff(&l, &want_l) < 1e-5 && max_diff(&r, &want_r) < 1e-5);
}

#[test]
fn pre_fader_send_ignores_the_fader_post_follows_it() {
    // Der Hauptweg geht in einen stummen Bus: es bleibt nur der Send hörbar.
    let build = |pre: bool| {
        let mut s = Session::new("prepost");
        let a = add_track(&mut s, "440", TrackKind::Audio);
        let dead = add_track(&mut s, "Dead", TrackKind::Bus);
        let hall = add_track(&mut s, "Hall", TrackKind::Bus);
        s.apply(Command::SetMute {
            track: dead,
            mute: true,
        })
        .expect("mute");
        s.apply(Command::SetOutput {
            track: a,
            output: Some(dead),
        })
        .expect("out");
        s.apply(Command::SetGain {
            track: a,
            gain_db: -20.0,
        })
        .expect("gain");
        s.apply(Command::SetPan {
            track: a,
            pan: -1.0,
        })
        .expect("pan");
        s.apply(Command::AddSend {
            track: a,
            send: send(hall, 0.0, pre),
            index: None,
        })
        .expect("send");
        render_project(s.project(), 4800).0
    };
    let pre = peak(&build(true)[2400..]);
    let post = peak(&build(false)[2400..]);
    assert!((pre - 0.5).abs() < 0.02, "pre {pre}");
    assert!((post - 0.05).abs() < 0.005, "post {post}");
}

#[test]
fn solo_on_a_track_keeps_its_bus_audible() {
    let mut s = Session::new("solo");
    let a = add_track(&mut s, "220", TrackKind::Audio);
    let b = add_track(&mut s, "5000", TrackKind::Audio);
    let hall = add_track(&mut s, "Hall", TrackKind::Bus);
    s.apply(Command::AddSend {
        track: a,
        send: send(hall, 0.0, false),
        index: None,
    })
    .expect("send");
    s.apply(Command::SetOutput {
        track: b,
        output: Some(hall),
    })
    .expect("out");
    s.apply(Command::SetSolo {
        track: a,
        solo: true,
    })
    .expect("solo");
    let (l, _) = render_project(s.project(), 9600);
    // 5000 Hz (Spur b) ist stumm, 220 Hz kommt zweimal an (direkt und über den Bus).
    let tail = &l[4800..];
    let hi: f32 = tail
        .windows(2)
        .map(|w| (w[1] - w[0]).abs())
        .fold(0.0, f32::max);
    assert!(hi < 0.06, "5 kHz leaks through: {hi}");
    assert!(peak(tail) > 0.4);
}

#[test]
fn bus_insert_filters_the_group() {
    let mut s = Session::new("busfx");
    let a = add_track(&mut s, "30", TrackKind::Audio);
    let grp = add_track(&mut s, "Gruppe", TrackKind::Bus);
    s.apply(Command::SetOutput {
        track: a,
        output: Some(grp),
    })
    .expect("out");
    let (before, _) = render_project(s.project(), 24_000);
    let eq = add_insert(&mut s, grp, EffectKind::Eq);
    let hp = EffectKind::Eq.param_index("highpass_hz").expect("param");
    s.apply(Command::SetInsertParam {
        track: grp,
        insert: eq,
        param: hp,
        value: 200.0,
    })
    .expect("hp");
    let (after, _) = render_project(s.project(), 24_000);
    assert!(peak(&after[12_000..]) < 0.2 * peak(&before[12_000..]));
}

#[test]
fn live_updates_of_inserts_and_sends_stay_continuous() {
    let mut s = Session::new("live");
    let a = add_track(&mut s, "440", TrackKind::Audio);
    let hall = add_track(&mut s, "Hall", TrackKind::Bus);
    let comp = add_insert(&mut s, a, EffectKind::Compressor);
    s.apply(Command::AddSend {
        track: a,
        send: send(hall, -60.0, false),
        index: None,
    })
    .expect("send");
    let mut r = rig(s.project());
    let (before, _) = render(&mut r, 4800);
    s.apply(Command::SetInsertBypass {
        track: a,
        insert: comp,
        bypass: true,
    })
    .expect("bypass");
    s.apply(Command::SetSend {
        track: a,
        target: hall,
        level_db: 0.0,
        pre_fader: false,
    })
    .expect("send level");
    ProjectGraph::update(&r.graph.tracks, &mut r.engine, s.project()).expect("update");
    let (after, _) = render(&mut r, 4800);
    // Der Send springt von -60 dB auf 0 dB, geglättet über 20 ms: kein Sprung je Sample.
    let mut prev = before[before.len() - 1];
    let mut worst = 0.0f32;
    for &x in &after {
        worst = worst.max((x - prev).abs());
        prev = x;
    }
    assert!(worst < 0.12, "jump {worst}");
    assert!(peak(&after[2400..]) > 0.5, "send level did not arrive");
}
