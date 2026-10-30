//! Paket 22 ohne Gerät und ohne Ton: ein Fake-Eingang mit künstlicher Latenz, Loop-Takes bei
//! Null-Ausgang und der Render des Comps.
//!
//! Der "Sänger" spielt in Runde k einen Impuls an Beat k+1 der Schleife. Der Eingang
//! liefert ihn `LATENCY` Frames zu spät. Der Latenzausgleich muss ihn auf dem Sample genau
//! dorthin zurücksetzen, wo gespielt wurde.

use std::collections::HashMap;
use std::sync::Arc;

use assert_no_alloc::{AllocDisabler, assert_no_alloc};
use woa_engine::clip::{clip_graph, render_clips};
use woa_engine::{Engine, LoopRange, beats_to_samples};
use woa_model::{
    CompSegment, RecordInput, RecordPlan, Session, TakeEntry, TrackKind, plan_recording,
};

#[global_allocator]
static ALLOC: AllocDisabler = AllocDisabler;

const RATE: u32 = 48_000;
/// 120 bpm: ein Beat = 0,5 s = 24 000 Samples.
const BEAT: usize = 24_000;
/// Schleife von Beat 4 bis 8 = 96 000 Samples je Runde.
const ROUND: usize = 4 * BEAT;
/// Künstliche Latenz: 20 ms.
const LATENCY: usize = 960;
const ROUNDS: usize = 3;

/// Was der Eingang liefert: drei Runden, Impuls von Runde k an Beat k+1, um `LATENCY` verspätet.
/// Die Amplitude kennzeichnet die Runde (0,25 / 0,5 / 0,75).
fn fake_input() -> Vec<f32> {
    let mut v = vec![0.0f32; ROUNDS * ROUND + 5_000];
    for k in 0..ROUNDS {
        v[k * ROUND + (k + 1) * BEAT + LATENCY] = 0.25 * (k + 1) as f32;
    }
    v
}

fn input_plan(frames: usize, latency: usize) -> Option<RecordPlan> {
    plan_recording(&RecordInput {
        start_beats: 4.0,
        tempo_bpm: 120.0,
        rate: f64::from(RATE),
        frames: frames as u64,
        latency_frames: latency as u64,
        loop_beats: Some((4.0, 8.0)),
        punch_beats: None,
    })
}

/// Projekt mit einer Spur, einem Take-Ordner aus den drei Runden der Aufnahme. Jede Runde
/// ist eine eigene "Datei", in Wirklichkeit dieselbe Aufnahme (`take.wav`), die Takes
/// unterscheiden sich nur im Versatz, genau wie bei der echten Aufnahme in einer Datei.
fn project(latency: usize) -> (Session, woa_model::TrackId, u64) {
    let Some(RecordPlan::Folder {
        start_beats,
        length_beats,
        offsets,
    }) = input_plan(fake_input().len(), latency)
    else {
        panic!("folder expected");
    };
    assert_eq!(offsets.len(), ROUNDS, "one take per round");
    let mut s = Session::new("rec");
    let t = s.new_track_id();
    s.apply(woa_model::Command::add_track(t, "Gesang", TrackKind::Audio))
        .expect("track");
    let takes = offsets
        .iter()
        .enumerate()
        .map(|(k, o)| TakeEntry {
            name: format!("Take {}", k + 1),
            audio: "aufnahmen/aufnahme-001.wav".into(),
            offset_beats: *o,
        })
        .collect();
    let id = s
        .add_take_folder(t, "Loop", start_beats, length_beats, takes)
        .expect("folder");
    (s, t, id)
}

fn load(path: &str) -> Option<Arc<[f32]>> {
    (path == "aufnahmen/aufnahme-001.wav").then(|| fake_input().into())
}

/// Alle Stellen, an denen der linke Kanal über der Schwelle liegt (ein Impuls, ein Treffer).
fn hits(l: &[f32]) -> Vec<(usize, f32)> {
    l.iter()
        .enumerate()
        .filter(|(_, v)| v.abs() > 0.02)
        .map(|(i, v)| (i, *v))
        .collect()
}

#[test]
fn compensated_take_lies_on_the_played_sample() {
    let (s, _, _) = project(LATENCY);
    // Standard-Comp: neueste Runde (Runde 3, Impuls an Beat 3 der Schleife = Beat 7).
    let (l, _) = render_clips(s.project(), RATE, 12.0, &mut |p| load(p)).expect("render");
    let h = hits(&l);
    assert_eq!(h.len(), 1, "{h:?}");
    assert_eq!(h[0].0, 7 * BEAT, "exactly where it was played");
    // Ohne Ausgleich liegt er genau die Latenz zu spät: der Test misst also wirklich etwas.
    let (s0, _, _) = project(0);
    let (l0, _) = render_clips(s0.project(), RATE, 12.0, &mut |p| load(p)).expect("render");
    assert_eq!(hits(&l0)[0].0, 7 * BEAT + LATENCY);
}

#[test]
fn comp_plays_the_chosen_parts() {
    let (mut s, t, id) = project(LATENCY);
    let f = s.project().track(t).expect("track").take_folders[0].clone();
    // Beat 4 bis 5,5 aus Take 1 (Impuls an Beat 5), 5,5 bis 8 aus Take 3 (Impuls an Beat 7).
    // Take 2 (Impuls an Beat 6) fällt weg.
    let mut g = f.clone();
    g.comp = g.comp_with(4.0, 5.5, 0);
    s.set_comp(t, id, g.comp.clone()).expect("comp");
    let (l, _) = render_clips(s.project(), RATE, 12.0, &mut |p| load(p)).expect("render");
    let h = hits(&l);
    assert_eq!(h.len(), 2, "{h:?}");
    assert_eq!(h[0].0, 5 * BEAT);
    assert_eq!(h[1].0, 7 * BEAT);
    // Amplituden zeigen die Runde: Take 1 = 0,25, Take 3 = 0,75 (gleiche Pan-Gewichtung).
    assert!(((h[1].1 / h[0].1) - 3.0).abs() < 0.01, "{h:?}");

    // Mitte aus Take 2 dazu: 5,5 bis 6,5.
    g.comp = g.comp_with(5.5, 6.5, 1);
    s.set_comp(t, id, g.comp.clone()).expect("comp 2");
    let (l, _) = render_clips(s.project(), RATE, 12.0, &mut |p| load(p)).expect("render");
    let at: Vec<usize> = hits(&l).iter().map(|h| h.0).collect();
    assert_eq!(at, vec![5 * BEAT, 6 * BEAT, 7 * BEAT]);

    // Undo stellt den Render wieder her.
    s.undo().expect("undo");
    let (l, _) = render_clips(s.project(), RATE, 12.0, &mut |p| load(p)).expect("render");
    assert_eq!(hits(&l).len(), 2);
}

#[test]
fn comp_with_a_gap_is_silent_there() {
    let (mut s, t, id) = project(LATENCY);
    let comp = vec![CompSegment {
        start_beats: 4.0,
        end_beats: 5.5,
        take: 0,
    }];
    s.set_comp(t, id, comp).expect("comp");
    let (l, _) = render_clips(s.project(), RATE, 12.0, &mut |p| load(p)).expect("render");
    assert_eq!(hits(&l).iter().map(|h| h.0).collect::<Vec<_>>(), [5 * BEAT]);
}

#[test]
fn punch_range_keeps_only_its_part() {
    // Ohne Schleife: ein Clip ab Beat 4, Impulse an Beat 5 und 7 der Aufnahme; Punch 6 bis 8.
    let mut v = vec![0.0f32; 8 * BEAT];
    v[BEAT + LATENCY] = 0.5; // gespielt an Beat 5
    v[3 * BEAT + LATENCY] = 0.5; // gespielt an Beat 7
    let plan = plan_recording(&RecordInput {
        start_beats: 4.0,
        tempo_bpm: 120.0,
        rate: f64::from(RATE),
        frames: v.len() as u64,
        latency_frames: LATENCY as u64,
        loop_beats: None,
        punch_beats: Some((6.0, 8.0)),
    });
    let Some(RecordPlan::Single(p)) = plan else {
        panic!("single expected");
    };
    assert_eq!((p.start_beats, p.length_beats), (6.0, 2.0));
    let mut s = Session::new("punch");
    let t = s.new_track_id();
    s.apply(woa_model::Command::add_track(t, "A", TrackKind::Audio))
        .expect("track");
    let id = s.new_clip_id();
    let mut clip = woa_model::Clip::new(id, "p", p.start_beats, p.length_beats, Some("x".into()));
    clip.offset_beats = p.offset_beats;
    s.apply(woa_model::Command::AddClip {
        track: t,
        clip,
        index: None,
    })
    .expect("clip");
    let file: Arc<[f32]> = v.into();
    let (l, _) =
        render_clips(s.project(), RATE, 12.0, &mut |_| Some(Arc::clone(&file))).expect("render");
    assert_eq!(
        hits(&l).iter().map(|h| h.0).collect::<Vec<_>>(),
        [7 * BEAT],
        "only the impulse inside the punch range, on its sample"
    );
}

/// Null-Ausgang: der Transport läuft über die Schleife, jede Runde wird gezählt. Die Zahl
/// stimmt mit der Zahl der Takes überein, die der Plan für dieselbe Aufnahmelänge ergibt.
#[test]
fn rounds_counted_at_the_null_output_match_the_takes() {
    let (s, _, _) = project(LATENCY);
    let (mut engine, mut proc_) = Engine::new(16);
    let graph = clip_graph(
        s.project(),
        &engine.transport(),
        RATE as f32,
        &mut |p| load(p),
        None,
    );
    engine
        .set_graph(graph.builder, RATE as f32, 256)
        .expect("graph");
    engine.set_tempo(120.0).expect("tempo");
    let start = beats_to_samples(4.0, 120.0, RATE as f32);
    let end = beats_to_samples(8.0, 120.0, RATE as f32);
    engine.set_loop(LoopRange::new(start, end)).expect("loop");
    engine.seek(start).expect("seek");
    engine.play().expect("play");
    let handle = engine.transport();
    let (mut l, mut r) = (vec![0.0f32; 256], vec![0.0f32; 256]);
    let (mut wraps, mut last, mut total) = (0usize, start, 0usize);
    let want = fake_input().len();
    while total < want {
        assert_no_alloc(|| proc_.process_stereo(&mut l, &mut r));
        total += 256;
        let pos = handle.position();
        if pos < last {
            wraps += 1;
        }
        last = pos;
    }
    let takes = match input_plan(total, LATENCY) {
        Some(RecordPlan::Folder { offsets, .. }) => offsets.len(),
        _ => 0,
    };
    assert_eq!(takes, ROUNDS);
    assert_eq!(wraps, ROUNDS, "rounds at the null output = takes");
}

#[test]
fn hash_of_takes_is_unique() {
    // Jede Runde hat ihren eigenen Versatz in der Datei.
    let (s, t, _) = project(LATENCY);
    let f = &s.project().track(t).expect("track").take_folders[0];
    let mut seen: HashMap<u64, usize> = HashMap::new();
    for (k, e) in f.takes.iter().enumerate() {
        assert!(
            seen.insert((e.offset_beats * 24_000.0) as u64, k).is_none(),
            "offsets differ"
        );
    }
}
