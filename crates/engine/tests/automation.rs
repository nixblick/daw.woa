//! Paket 18: Automation und Sidechain im Graphen. Alles offline gerechnet, kein Gerät, kein
//! Ton. Jeder Process-Aufruf der Echtzeit-Probe läuft unter `assert_no_alloc`.

use assert_no_alloc::{AllocDisabler, assert_no_alloc};
use woa_dsp::{Node, ProcessBuffers, SineOsc};
use woa_engine::clip::take_peak;
use woa_engine::{Engine, LoopRange, build_project_graph, render_project};
use woa_model::{
    AutoMode, AutoPoint, AutoTarget, Command, EffectKind, Insert, InsertId, Project, Send, Session,
    Track, TrackId, TrackKind,
};

#[global_allocator]
static ALLOC: AllocDisabler = AllocDisabler;

const SR: f32 = 48_000.0;
/// Bei 120 BPM sind 24 000 Samples ein Beat.
const BEAT: usize = 24_000;

/// Sinus 200 Hz setzt nach `delay` Samples ein (Name "voice") oder läuft durch.
struct Tone {
    sine: SineOsc,
    delay: usize,
    at: usize,
}

impl Node for Tone {
    fn prepare(&mut self, sample_rate: f32) {
        self.sine.prepare(sample_rate);
    }

    fn set_param(&mut self, _param: u32, _value: f32) {}

    fn process(&mut self, b: ProcessBuffers<'_>) {
        self.sine.process(ProcessBuffers {
            input: b.input,
            output: b.output,
        });
        for o in b.output.iter_mut() {
            if self.at < self.delay {
                *o = 0.0;
            }
            self.at += 1;
        }
    }
}

fn source(t: &Track) -> Option<Box<dyn Node>> {
    match t.kind {
        TrackKind::Bus => None,
        _ if t.name == "voice" => Some(Box::new(Tone {
            sine: SineOsc::new(500.0, 0.5),
            delay: BEAT,
            at: 0,
        })),
        _ => {
            let freq = t.name.parse::<f32>().unwrap_or(1000.0);
            Some(Box::new(Tone {
                sine: SineOsc::new(freq, 0.5),
                delay: 0,
                at: 0,
            }))
        }
    }
}

fn one_track(name: &str) -> (Session, TrackId) {
    let mut s = Session::new("auto");
    let t = s.new_track_id();
    s.apply(Command::add_track(t, name, TrackKind::Audio))
        .unwrap();
    (s, t)
}

fn lane(s: &mut Session, t: TrackId, target: AutoTarget, pts: &[(f64, f64)]) {
    s.apply(Command::SetAutomation {
        track: t,
        target,
        points: pts
            .iter()
            .map(|&(beat, value)| AutoPoint { beat, value })
            .collect(),
        mode: AutoMode::Read,
    })
    .unwrap();
}

fn render(p: &Project, frames: usize, block: usize) -> (Vec<f32>, Vec<f32>) {
    render_project(p, &mut source, SR, frames, block).unwrap()
}

fn rms(x: &[f32]) -> f64 {
    (x.iter().map(|v| f64::from(*v).powi(2)).sum::<f64>() / x.len() as f64).sqrt()
}

fn db(x: f64) -> f64 {
    20.0 * x.log10()
}

/// dB-Wert der Kurve (0 dB bei Beat 0, -20 dB bei Beat 2, -6 dB bei Beat 4) im Sample `i`.
fn expected_db(i: usize) -> f64 {
    let (b2, b4) = (2 * BEAT, 4 * BEAT);
    if i == 0 {
        0.0
    } else if i < b2 {
        -20.0 * i as f64 / b2 as f64
    } else if i < b4 {
        -20.0 + 14.0 * (i - b2) as f64 / (b4 - b2) as f64
    } else {
        -6.0
    }
}

#[test]
fn gain_lane_with_three_points_is_sample_exact_in_every_block_size() {
    let (mut s, t) = one_track("1000");
    let plain = s.project().clone();
    lane(
        &mut s,
        t,
        AutoTarget::Gain,
        &[(0.0, 0.0), (2.0, -20.0), (4.0, -6.0)],
    );
    let frames = 5 * BEAT;
    let (pl, _) = render(&plain, frames, 128);
    let (a, _) = render(s.project(), frames, 128);
    let (b, _) = render(s.project(), frames, 1000);
    let (c, _) = render(s.project(), frames, 1);
    assert_eq!(a, b, "block 128 vs 1000");
    assert_eq!(a[..4000], c[..4000], "block 128 vs 1");
    // Jedes Sample: Spur ohne Kurve mal der dB-Kurve.
    let mut worst = 0.0f64;
    for i in 0..frames {
        let want = f64::from(pl[i]) * 10f64.powf(expected_db(i) / 20.0);
        worst = worst.max((f64::from(a[i]) - want).abs());
    }
    assert!(worst < 2e-5, "größte Abweichung {worst}");
}

#[test]
fn a_track_without_automation_is_unchanged() {
    let (s, _) = one_track("1000");
    let (l, _) = render(s.project(), BEAT, 128);
    assert!(rms(&l) > 0.1);
}

#[test]
fn pan_lane_sweeps_from_left_to_right() {
    let (mut s, t) = one_track("1000");
    lane(&mut s, t, AutoTarget::Pan, &[(0.0, -1.0), (2.0, 1.0)]);
    let (l, r) = render(s.project(), 3 * BEAT, 128);
    // Am Anfang nur links, am Ende nur rechts, in der Mitte gleich laut.
    assert!(rms(&l[..400]) > 20.0 * rms(&r[..400]));
    assert!(rms(&r[2 * BEAT..]) > 1000.0 * rms(&l[2 * BEAT..]) || rms(&l[2 * BEAT..]) < 1e-6);
    let mid = BEAT - 200..BEAT + 200;
    let ratio = rms(&l[mid.clone()]) / rms(&r[mid]);
    assert!((ratio - 1.0).abs() < 0.05, "Mitte {ratio}");
}

#[test]
fn send_lane_matches_a_static_send_level() {
    let build = |automated: bool| {
        let (mut s, t) = one_track("1000");
        let bus = s.new_track_id();
        s.apply(Command::add_track(bus, "Hall", TrackKind::Bus))
            .unwrap();
        s.apply(Command::AddSend {
            track: t,
            send: Send {
                target: bus,
                level_db: -6.0,
                pre_fader: false,
            },
            index: None,
        })
        .unwrap();
        if automated {
            lane(
                &mut s,
                t,
                AutoTarget::Send { target: bus },
                &[(0.0, -6.0), (3.0, -6.0)],
            );
        }
        s.project().clone()
    };
    let (a, _) = render(&build(false), 2 * BEAT, 128);
    let (b, _) = render(&build(true), 2 * BEAT, 128);
    let worst = a
        .iter()
        .zip(&b)
        .map(|(x, y)| (x - y).abs())
        .fold(0.0f32, f32::max);
    assert!(worst < 1e-5, "{worst}");
}

fn insert(s: &mut Session, t: TrackId, kind: EffectKind, set: &[(&str, f64)]) -> InsertId {
    let id = s.new_insert_id();
    s.apply(Command::AddInsert {
        track: t,
        insert: Insert::new(id, kind),
        index: None,
    })
    .unwrap();
    for &(name, value) in set {
        s.apply(Command::SetInsertParam {
            track: t,
            insert: id,
            param: kind.param_index(name).unwrap(),
            value,
        })
        .unwrap();
    }
    id
}

#[test]
fn effect_parameter_lane_moves_the_knob() {
    let (mut s, t) = one_track("1000");
    // Kompressor, der nichts tut (Verhältnis 1), mit Ausgleich von 0 auf 12 dB in 8 Beats.
    let c = insert(&mut s, t, EffectKind::Compressor, &[("ratio", 1.0)]);
    let param = EffectKind::Compressor.param_index("makeup_db").unwrap();
    lane(
        &mut s,
        t,
        AutoTarget::InsertParam { insert: c, param },
        &[(0.0, 0.0), (8.0, 12.0)],
    );
    let (l, _) = render(s.project(), 9 * BEAT, 128);
    let early = rms(&l[..4800]);
    let late = rms(&l[8 * BEAT..]);
    let gain = db(late / early);
    assert!((gain - 12.0).abs() < 1.0, "Zuwachs {gain} dB");
}

/// Projekt "music" mit Kompressor, dessen Schlüssel "voice" ist; "voice" setzt nach einem
/// Beat ein.
fn ducking(with_sidechain: bool, with_comp: bool) -> (Project, Option<InsertId>) {
    let mut s = Session::new("duck");
    let music = s.new_track_id();
    s.apply(Command::add_track(music, "music", TrackKind::Audio))
        .unwrap();
    s.apply(Command::SetGain {
        track: music,
        gain_db: -6.0,
    })
    .unwrap();
    let voice = s.new_track_id();
    s.apply(Command::add_track(voice, "voice", TrackKind::Audio))
        .unwrap();
    let comp = with_comp.then(|| {
        insert(
            &mut s,
            music,
            EffectKind::Compressor,
            &[
                ("threshold_db", -40.0),
                ("ratio", 10.0),
                ("attack_ms", 1.0),
                ("release_ms", 100.0),
            ],
        )
    });
    if let (true, Some(c)) = (with_sidechain, comp) {
        s.apply(Command::SetInsertSidechain {
            track: music,
            insert: c,
            source: Some(voice),
        })
        .unwrap();
    }
    (s.project().clone(), comp)
}

#[test]
fn sidechain_ducks_the_music_when_the_voice_speaks() {
    let (with, _) = ducking(true, true);
    let (without, _) = ducking(false, false);
    // Nur die Sprache, damit sich die Musik allein messen lässt.
    let mut voice_only = without.clone();
    voice_only.tracks.retain(|t| t.name == "voice");
    let frames = 3 * BEAT;
    let (a, _) = render(&with, frames, 128);
    let (r, _) = render(&without, frames, 128);
    let (v, _) = render(&voice_only, frames, 128);
    // Vor dem Einsatz der Sprache ist der Schlüssel still: Musik unverändert.
    let worst = a[..BEAT - 100]
        .iter()
        .zip(&r)
        .map(|(x, y)| (x - y).abs())
        .fold(0.0f32, f32::max);
    assert!(worst < 1e-6, "vor der Sprache {worst}");
    // Danach ist die Musik (Mischung minus Sprache) deutlich leiser als ohne Sidechain.
    let span = 2 * BEAT..3 * BEAT;
    let music_with: Vec<f32> = a[span.clone()]
        .iter()
        .zip(&v[span.clone()])
        .map(|(x, y)| x - y)
        .collect();
    let music_ref: Vec<f32> = r[span.clone()]
        .iter()
        .zip(&v[span])
        .map(|(x, y)| x - y)
        .collect();
    let drop = db(rms(&music_with) / rms(&music_ref));
    assert!(drop < -6.0, "Musik nur {drop} dB leiser");
}

#[test]
fn gain_reduction_meter_swings_with_the_sidechain() {
    let (project, comp) = ducking(true, true);
    let comp = comp.unwrap();
    let g = build_project_graph(&project, &mut source);
    let meter = std::sync::Arc::clone(&g.reductions[&comp]);
    let (mut engine, mut processor) = Engine::new(16);
    engine.set_graph(g.builder, SR, 128).unwrap();
    engine.play().unwrap();
    let (mut l, mut r) = (vec![0.0; 128], vec![0.0; 128]);
    // Vor der Sprache: keine Absenkung.
    for _ in 0..(BEAT / 128 - 2) {
        processor.process_stereo(&mut l, &mut r);
    }
    assert!(take_peak(&meter) < 0.1);
    // Nach dem Einsatz: deutliche Absenkung.
    for _ in 0..(BEAT / 128) {
        processor.process_stereo(&mut l, &mut r);
    }
    let reduction = take_peak(&meter);
    assert!(reduction > 6.0, "Absenkung nur {reduction} dB");
}

#[test]
fn stopped_transport_holds_the_curve_value_at_the_cursor() {
    let (mut s, t) = one_track("1000");
    let plain = s.project().clone();
    lane(&mut s, t, AutoTarget::Gain, &[(0.0, 0.0), (4.0, -20.0)]);
    let run = |p: &Project| {
        let g = build_project_graph(p, &mut source);
        let (mut engine, mut processor) = Engine::new(16);
        engine.set_graph(g.builder, SR, 128).unwrap();
        engine.seek(2 * BEAT as u64).unwrap();
        let (mut l, mut r) = (vec![0.0; 4800], vec![0.0; 4800]);
        processor.process_stereo(&mut l, &mut r);
        l
    };
    let ratio = db(rms(&run(s.project())) / rms(&run(&plain)));
    assert!((ratio + 10.0).abs() < 0.1, "{ratio} dB");
}

#[test]
fn loop_wrap_restarts_the_curve_exactly() {
    let (mut s, t) = one_track("1000");
    lane(&mut s, t, AutoTarget::Gain, &[(0.0, 0.0), (1.0, -12.0)]);
    let g = build_project_graph(s.project(), &mut source);
    let (mut engine, mut processor) = Engine::new(16);
    engine.set_graph(g.builder, SR, 128).unwrap();
    engine.set_loop(LoopRange::new(0, BEAT as u64)).unwrap();
    engine.play().unwrap();
    let frames = 2 * BEAT;
    let (mut l, mut r) = (vec![0.0; frames], vec![0.0; frames]);
    for (a, b) in l.chunks_mut(300).zip(r.chunks_mut(300)) {
        processor.process_stereo(a, b);
    }
    // 1000 Hz sind in einem Beat eine ganze Zahl Perioden: beide Runden gleich.
    let worst = (0..BEAT)
        .map(|i| (l[i] - l[i + BEAT]).abs())
        .fold(0.0f32, f32::max);
    assert!(worst < 1e-4, "{worst}");
}

#[test]
fn everything_automated_runs_without_allocation() {
    let mut s = Session::new("rt");
    let music = s.new_track_id();
    s.apply(Command::add_track(music, "music", TrackKind::Audio))
        .unwrap();
    let voice = s.new_track_id();
    s.apply(Command::add_track(voice, "voice", TrackKind::Audio))
        .unwrap();
    let bus = s.new_track_id();
    s.apply(Command::add_track(bus, "Hall", TrackKind::Bus))
        .unwrap();
    s.apply(Command::AddSend {
        track: music,
        send: Send {
            target: bus,
            level_db: -9.0,
            pre_fader: true,
        },
        index: None,
    })
    .unwrap();
    let comp = insert(&mut s, music, EffectKind::Compressor, &[("ratio", 4.0)]);
    s.apply(Command::SetInsertSidechain {
        track: music,
        insert: comp,
        source: Some(voice),
    })
    .unwrap();
    let eq = insert(&mut s, music, EffectKind::Eq, &[]);
    let bell = EffectKind::Eq.param_index("bell_db").unwrap();
    lane(
        &mut s,
        music,
        AutoTarget::InsertParam {
            insert: eq,
            param: bell,
        },
        &[(0.0, -6.0), (2.0, 6.0)],
    );
    lane(&mut s, music, AutoTarget::Gain, &[(0.0, 0.0), (1.0, -10.0)]);
    lane(&mut s, music, AutoTarget::Pan, &[(0.0, -1.0), (2.0, 1.0)]);
    lane(
        &mut s,
        music,
        AutoTarget::Send { target: bus },
        &[(0.0, -20.0), (2.0, 0.0)],
    );
    lane(&mut s, bus, AutoTarget::Gain, &[(0.0, 0.0), (2.0, -3.0)]);
    lane(&mut s, bus, AutoTarget::Pan, &[(0.0, 0.5), (2.0, -0.5)]);
    let g = build_project_graph(s.project(), &mut source);
    let (mut engine, mut processor) = Engine::new(16);
    engine.set_graph(g.builder, SR, 128).unwrap();
    engine
        .set_loop(LoopRange::new(0, BEAT as u64 + 77))
        .unwrap();
    engine.play().unwrap();
    let (mut l, mut r) = (vec![0.0f32; 333], vec![0.0f32; 333]);
    for _ in 0..400 {
        assert_no_alloc(|| processor.process_stereo(&mut l, &mut r));
    }
    assert!(l.iter().chain(&r).all(|v| v.is_finite()));
    drop(processor);
    engine.collect();
}

#[test]
fn lanes_in_write_mode_are_not_played() {
    let (mut s, t) = one_track("1000");
    let plain = s.project().clone();
    s.apply(Command::SetAutomation {
        track: t,
        target: AutoTarget::Gain,
        points: vec![
            AutoPoint {
                beat: 0.0,
                value: -30.0,
            },
            AutoPoint {
                beat: 1.0,
                value: -30.0,
            },
        ],
        mode: AutoMode::Write,
    })
    .unwrap();
    let (a, _) = render(&plain, BEAT / 4, 128);
    let (b, _) = render(s.project(), BEAT / 4, 128);
    assert_eq!(a, b, "Schreiben-Modus spielt die Kurve nicht ab");
}
