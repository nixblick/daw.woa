//! Paket 21: Schnitt und Fades gegen den Offline-Render. Alles gemessen, nichts gehört:
//! Nulltest nach dem Teilen, RMS beim Crossfade, Knackserkennung an Schnittstellen,
//! Clip-Gain und Echtzeit (jeder Process-Aufruf unter `assert_no_alloc`).

use std::sync::Arc;

use assert_no_alloc::{AllocDisabler, assert_no_alloc};
use woa_engine::Engine;
use woa_engine::clip::{ClipPlayer, PlacedClip, clip_graph_mode, render_clips};
use woa_model::{Clip, ClipId, Command, FadeCurve, Project, Session, TrackId, TrackKind};

#[global_allocator]
static ALLOC: AllocDisabler = AllocDisabler;

const RATE: u32 = 48_000;
/// 120 bpm: ein Beat = 24 000 Samples.
const BEAT: usize = 24_000;

fn noise(seed: u32, n: usize) -> Arc<[f32]> {
    let mut x = seed;
    (0..n)
        .map(|_| {
            x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            ((x >> 8) as f32 / 8_388_608.0 - 1.0) * 0.5
        })
        .collect::<Vec<_>>()
        .into()
}

fn sine(freq: f32, phase: f32, amp: f32, n: usize, rate: u32) -> Arc<[f32]> {
    (0..n)
        .map(|i| amp * (phase + std::f32::consts::TAU * freq * i as f32 / rate as f32).sin())
        .collect::<Vec<_>>()
        .into()
}

fn new_session() -> (Session, TrackId) {
    let mut s = Session::new("cuts");
    let t = s.new_track_id();
    s.apply(Command::add_track(t, "A", TrackKind::Audio))
        .expect("track");
    (s, t)
}

fn add_clip(s: &mut Session, t: TrackId, file: &str, start: f64, len: f64) -> ClipId {
    let id = s.new_clip_id();
    s.apply(Command::AddClip {
        track: t,
        clip: Clip::new(id, file, start, len, Some(file.into())),
        index: None,
    })
    .expect("clip");
    id
}

/// Wie `AppState::split_clip` und der Agent: Teilen ist ein Edit plus ein neuer Clip.
fn split(s: &mut Session, clip: ClipId, at: f64) -> ClipId {
    let (track, c) = {
        let (t, c) = s.project().clip(clip).expect("clip");
        (t.id, c.clone())
    };
    let left = at - c.start_beats;
    let right = c.end_beats() - at;
    let id = s.new_clip_id();
    let mut part = c.clone();
    part.id = id;
    part.start_beats = at;
    part.length_beats = right;
    part.offset_beats = c.offset_beats + left;
    part.fade_in_beats = 0.0;
    s.apply(Command::EditClip {
        clip,
        length_beats: left,
        offset_beats: c.offset_beats,
        fade_in_beats: c.fade_in_beats,
        fade_out_beats: 0.0,
    })
    .expect("edit");
    s.apply(Command::AddClip {
        track,
        clip: part,
        index: None,
    })
    .expect("add");
    id
}

type Loader<'a> = Box<dyn FnMut(&str) -> Option<Arc<[f32]>> + 'a>;

fn loader<'a>(files: &'a [(&'a str, Arc<[f32]>)]) -> Loader<'a> {
    Box::new(move |name: &str| {
        files
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, d)| Arc::clone(d))
    })
}

fn render(p: &Project, rate: u32, beats: f64, files: &[(&str, Arc<[f32]>)]) -> Vec<f32> {
    let secs = beats * 60.0 / p.tempo_bpm;
    render_clips(p, rate, secs, &mut *loader(files))
        .expect("render")
        .0
}

fn max_step(x: &[f32]) -> f32 {
    x.windows(2)
        .map(|w| (w[1] - w[0]).abs())
        .fold(0.0, f32::max)
}

fn rms(x: &[f32]) -> f64 {
    (x.iter().map(|v| f64::from(*v).powi(2)).sum::<f64>() / x.len() as f64).sqrt()
}

fn db(a: f64, b: f64) -> f64 {
    20.0 * (a / b).log10()
}

#[test]
fn split_renders_bit_identically() {
    // Nulltest: Original minus geteiltes Projekt muss auf jedem Sample exakt 0 sein.
    let file = noise(7, 8 * BEAT);
    let files = [("a", Arc::clone(&file))];
    for (start, offset) in [(1.0, 0.0), (2.0, 0.75)] {
        let (mut s, t) = new_session();
        let c = add_clip(&mut s, t, "a", start, 6.0);
        if offset > 0.0 {
            s.apply(Command::EditClip {
                clip: c,
                length_beats: 6.0,
                offset_beats: offset,
                fade_in_beats: 0.0,
                fade_out_beats: 0.0,
            })
            .expect("offset");
        }
        let before = render(s.project(), RATE, 10.0, &files);
        assert!(before.iter().any(|v| v.abs() > 0.01), "there is sound");
        let right = split(&mut s, c, start + 2.5);
        split(&mut s, right, start + 4.25);
        assert_eq!(s.project().tracks[0].clips.len(), 3);
        let after = render(s.project(), RATE, 10.0, &files);
        let diff = before
            .iter()
            .zip(&after)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0f32, f32::max);
        assert_eq!(diff, 0.0, "null test, start {start} offset {offset}");
        assert!(before == after, "bitwise equal");
        // Undo der beiden Teilungen stellt das Original wieder her.
        for _ in 0..4 {
            s.undo().expect("undo");
        }
        assert_eq!(render(s.project(), RATE, 10.0, &files), before);
    }
}

#[test]
fn split_at_awkward_tempo_does_not_click() {
    // 97 bpm bei 44,1 kHz: ein Beat ist keine ganze Zahl von Samples. Die Teile schließen
    // trotzdem ohne Lücke an; ein Versatz von höchstens einem Sample bleibt unter dem
    // Doppelten der größten Sinus-Steigung.
    let rate = 44_100;
    let file = sine(300.0, 0.0, 0.5, 40 * rate as usize, rate);
    let files = [("a", file)];
    let (mut s, t) = new_session();
    s.apply(Command::SetTempo { bpm: 97.0 }).expect("tempo");
    let c = add_clip(&mut s, t, "a", 0.0, 16.0);
    let before = render(s.project(), rate, 18.0, &files);
    let base = max_step(&before);
    let r1 = split(&mut s, c, 3.3);
    split(&mut s, r1, 9.1);
    let after = render(s.project(), rate, 18.0, &files);
    assert!(
        max_step(&after) <= base * 2.2,
        "jump {} vs sine slope {base}",
        max_step(&after)
    );
    let drift = before
        .iter()
        .zip(&after)
        .map(|(a, b)| (a - b).abs())
        .fold(0.0f32, f32::max);
    assert!(drift < 0.05, "at most one sample off, drift {drift}");
}

#[test]
fn deleting_the_middle_keeps_the_outer_parts_in_place() {
    let file = noise(3, 8 * BEAT);
    let files = [("a", file)];
    let (mut s, t) = new_session();
    let c = add_clip(&mut s, t, "a", 0.0, 8.0);
    let full = render(s.project(), RATE, 10.0, &files);
    let mid = split(&mut s, c, 2.0);
    let right = split(&mut s, mid, 5.0);
    s.apply(Command::RemoveClip { clip: mid }).expect("remove");
    let cut = render(s.project(), RATE, 10.0, &files);
    assert_eq!(cut[..2 * BEAT], full[..2 * BEAT], "left part untouched");
    assert!(cut[2 * BEAT..5 * BEAT].iter().all(|v| *v == 0.0), "gap");
    assert_eq!(
        cut[5 * BEAT..8 * BEAT],
        full[5 * BEAT..8 * BEAT],
        "right part in place"
    );
    let _ = right;
}

#[test]
fn fade_in_and_out_follow_the_curve_tables() {
    // Gleichspannung 1.0, Fade über 2 Beats: Sample für Sample gegen die Kurventabelle.
    let dc: Arc<[f32]> = vec![1.0f32; 8 * BEAT].into();
    let files = [("a", dc)];
    for curve in FadeCurve::ALL {
        let (mut s, t) = new_session();
        let c = add_clip(&mut s, t, "a", 0.0, 8.0);
        s.apply(Command::EditClip {
            clip: c,
            length_beats: 8.0,
            offset_beats: 0.0,
            fade_in_beats: 2.0,
            fade_out_beats: 2.0,
        })
        .expect("fades");
        s.apply(Command::SetClipFadeCurves {
            clip: c,
            fade_in: curve,
            fade_out: curve,
        })
        .expect("curves");
        // Spur auf Mitte: Pan-Gesetz herausrechnen über den Wert in der Clip-Mitte.
        let out = render(s.project(), RATE, 9.0, &files);
        let plateau = out[4 * BEAT];
        assert!(plateau > 0.5, "plateau {plateau}");
        let n = 2 * BEAT;
        for i in (0..n).step_by(97) {
            let want = curve.gain(i as f32 / n as f32) * plateau;
            assert!(
                (out[i] - want).abs() < 2e-4,
                "{curve:?} in at {i}: {} vs {want}",
                out[i]
            );
            let j = 8 * BEAT - n + i;
            let want_out = curve.gain(1.0 - i as f32 / n as f32) * plateau;
            assert!(
                (out[j] - want_out).abs() < 2e-4,
                "{curve:?} out at {i}: {} vs {want_out}",
                out[j]
            );
        }
    }
}

#[test]
fn crossfade_keeps_the_level_constant() {
    // Zwei unabhängige Rauschspuren überlappen 2 Beats. Mit gleicher Leistung bleibt der
    // RMS in der Überlappung beim Wert außerhalb (linear fiele er in der Mitte um 3 dB).
    let files = [("a", noise(11, 8 * BEAT)), ("b", noise(99, 8 * BEAT))];
    let (mut s, t) = new_session();
    add_clip(&mut s, t, "a", 0.0, 8.0);
    add_clip(&mut s, t, "b", 6.0, 8.0);
    let out = render(s.project(), RATE, 15.0, &files);
    let ref_a = rms(&out[..6 * BEAT]);
    let ref_b = rms(&out[8 * BEAT..14 * BEAT]);
    assert!(db(ref_a, ref_b).abs() < 0.3, "equal reference levels");
    let block = 4_800;
    let mut worst = 0.0f64;
    for k in 0..(2 * BEAT / block) {
        let from = 6 * BEAT + k * block;
        worst = worst.max(db(rms(&out[from..from + block]), ref_a).abs());
    }
    assert!(worst < 0.7, "level in the overlap deviates by {worst} dB");

    // Gegenprobe: derselbe Aufbau mit linearer Überblendung hat in der Mitte ein Loch.
    let (mut proc_engine, mut proc_) = Engine::new(16);
    let mk = |start: u64, d: &Arc<[f32]>, fin: u64, fout: u64| {
        let mut p = PlacedClip::plain(start, Arc::clone(d));
        p.fade_in = fin;
        p.fade_out = fout;
        p
    };
    let n = 2 * BEAT as u64;
    let clips = vec![
        mk(0, &files[0].1, 0, n),
        mk(6 * BEAT as u64, &files[1].1, n, 0),
    ];
    let mut b = woa_engine::GraphBuilder::new();
    let node = b.add_node(ClipPlayer::new(clips, proc_engine.transport()));
    b.set_output(node);
    proc_engine.set_graph(b, RATE as f32, 128).expect("graph");
    proc_engine.play().expect("play");
    let mut lin = vec![0.0f32; 15 * BEAT];
    for chunk in lin.chunks_mut(128) {
        proc_.process(chunk);
    }
    let mid_from = 7 * BEAT - block / 2;
    let dip = db(rms(&lin[mid_from..mid_from + block]), rms(&lin[..6 * BEAT]));
    assert!(dip < -2.0, "linear crossfade dips in the middle: {dip} dB");
    let mid_eq = db(rms(&out[mid_from..mid_from + block]), ref_a);
    assert!(mid_eq.abs() < 0.7, "equal power at the middle: {mid_eq} dB");
}

#[test]
fn crossfade_has_no_click_but_a_hard_join_does() {
    // Zwei Sinus-Clips, der zweite mit anderer Phase. Der Sprungdetektor (größte Differenz
    // zweier Nachbar-Samples) darf an der Überblendung nicht über die Sinus-Steigung steigen.
    let a = sine(440.0, 0.0, 0.5, 6 * BEAT, RATE);
    let b = sine(440.0, std::f32::consts::FRAC_PI_2, 0.5, 6 * BEAT, RATE);
    let files = [("a", a), ("b", b)];
    let slope = 0.5 * std::f32::consts::TAU * 440.0 / RATE as f32;

    let (mut s, t) = new_session();
    add_clip(&mut s, t, "a", 0.0, 4.0);
    add_clip(&mut s, t, "b", 3.0, 4.0);
    let out = render(s.project(), RATE, 8.0, &files);
    assert!(out.iter().any(|v| v.abs() > 0.1));
    let soft = max_step(&out[..6 * BEAT]); // vor dem harten Ende von b
    assert!(soft < slope * 1.6, "crossfade step {soft} vs slope {slope}");

    // Ohne Überlappung: der Clip b setzt hart mit 0,5 ein, wo a bei 0 steht.
    let (mut s, t) = new_session();
    add_clip(&mut s, t, "a", 0.0, 3.0);
    add_clip(&mut s, t, "b", 3.0, 4.0);
    let hard = max_step(&render(s.project(), RATE, 8.0, &files));
    assert!(hard > 0.2, "the detector sees a hard join: {hard}");
}

#[test]
fn clip_gain_scales_and_zero_is_transparent() {
    let file = noise(5, 4 * BEAT);
    let files = [("a", file)];
    let (mut s, t) = new_session();
    let c = add_clip(&mut s, t, "a", 0.0, 4.0);
    let base = render(s.project(), RATE, 5.0, &files);
    s.apply(Command::SetClipGain {
        clip: c,
        gain_db: -6.0,
    })
    .expect("gain");
    let quiet = render(s.project(), RATE, 5.0, &files);
    let want = 10f64.powf(-6.0 / 20.0);
    let ratio = rms(&quiet) / rms(&base);
    assert!((ratio - want).abs() < 1e-4, "ratio {ratio} vs {want}");
    s.undo().expect("undo");
    assert!(
        render(s.project(), RATE, 5.0, &files) == base,
        "undo is bitwise"
    );
    s.apply(Command::SetClipGain {
        clip: c,
        gain_db: 0.0,
    })
    .expect("zero");
    assert!(
        render(s.project(), RATE, 5.0, &files) == base,
        "0 dB is transparent"
    );
}

#[test]
fn processing_with_fades_gain_and_crossfade_does_not_allocate() {
    let files = [("a", noise(1, 4 * BEAT)), ("b", noise(2, 4 * BEAT))];
    let (mut s, t) = new_session();
    let a = add_clip(&mut s, t, "a", 0.0, 4.0);
    add_clip(&mut s, t, "b", 3.0, 4.0);
    s.apply(Command::EditClip {
        clip: a,
        length_beats: 4.0,
        offset_beats: 0.0,
        fade_in_beats: 1.0,
        fade_out_beats: 0.0,
    })
    .expect("fade");
    s.apply(Command::SetClipFadeCurves {
        clip: a,
        fade_in: FadeCurve::SCurve,
        fade_out: FadeCurve::EqualPower,
    })
    .expect("curves");
    s.apply(Command::SetClipGain {
        clip: a,
        gain_db: -3.0,
    })
    .expect("gain");
    let (mut engine, mut proc_) = Engine::new(16);
    let graph = clip_graph_mode(
        s.project(),
        &engine.transport(),
        RATE as f32,
        &mut *loader(&files),
        None,
        None,
        true,
    );
    engine
        .set_graph(graph.builder, RATE as f32, 128)
        .expect("graph");
    engine.play().expect("play");
    let (mut l, mut r) = (vec![0.0f32; 128], vec![0.0f32; 128]);
    let mut peak = 0.0f32;
    for _ in 0..(8 * BEAT / 128) {
        assert_no_alloc(|| proc_.process_stereo(&mut l, &mut r));
        peak = l.iter().fold(peak, |m, v| m.max(v.abs()));
    }
    assert!(peak > 0.05, "sound came out: {peak}");
}
