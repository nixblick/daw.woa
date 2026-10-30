//! Paket 23: Time-Stretch, Tonhöhe und Tempo-Folgen gegen den Offline-Render. Gemessen,
//! nicht gehört: Länge und Frequenz eines Sinus nach dem Dehnen, FFT-Spitze nach der
//! Tonhöhenänderung, Tempo 120 auf 100, Undo bitgleich, Echtzeit unter `assert_no_alloc`.

use std::sync::Arc;

use assert_no_alloc::{AllocDisabler, assert_no_alloc};
use woa_dsp::analysis::spectrum::{SpectrumAnalyzer, bin_hz, peak_bin};
use woa_engine::Engine;
use woa_engine::clip::{clip_graph_mode, render_clips};
use woa_model::{Clip, ClipId, Command, Project, Session, TrackId, TrackKind};

#[global_allocator]
static ALLOC: AllocDisabler = AllocDisabler;

const RATE: u32 = 48_000;
/// 120 bpm: ein Beat = 24 000 Samples.
const BEAT: usize = 24_000;
const FREQ: f32 = 440.0;

fn sine(n: usize) -> Arc<[f32]> {
    (0..n)
        .map(|i| 0.5 * (std::f32::consts::TAU * FREQ * i as f32 / RATE as f32).sin())
        .collect::<Vec<_>>()
        .into()
}

fn session() -> (Session, TrackId, ClipId) {
    let mut s = Session::new("stretch");
    let t = s.new_track_id();
    s.apply(Command::add_track(t, "A", TrackKind::Audio))
        .expect("track");
    let c = s.new_clip_id();
    s.apply(Command::AddClip {
        track: t,
        clip: Clip::new(c, "sine", 0.0, 4.0, Some("sine".into())),
        index: None,
    })
    .expect("clip");
    (s, t, c)
}

fn render(p: &Project, seconds: f64, file: &Arc<[f32]>) -> Vec<f32> {
    let f = Arc::clone(file);
    render_clips(p, RATE, seconds, &mut |_: &str| Some(Arc::clone(&f)))
        .expect("render")
        .0
}

/// Länge des hörbaren Teils: Anzahl Samples bis zum letzten Sample über der Schwelle.
fn audible_len(x: &[f32]) -> usize {
    x.iter().rposition(|v| v.abs() > 0.02).map_or(0, |i| i + 1)
}

fn peak_hz(x: &[f32]) -> f32 {
    const N: usize = 1 << 15;
    let from = (x.len() - N) / 2;
    let mut an = SpectrumAnalyzer::new(N);
    let db = an.analyze(&x[from..from + N]);
    bin_hz(peak_bin(db).expect("peak"), N, RATE)
}

fn stretch(s: &mut Session, c: ClipId, length: f64) {
    let clip = s.project().clip(c).expect("clip").1.clone();
    s.apply(Command::SetClipStretch {
        clip: c,
        length_beats: length,
        stretch: clip.stretch * length / clip.length_beats,
        fade_in_beats: 0.0,
        fade_out_beats: 0.0,
    })
    .expect("stretch");
}

#[test]
fn stretch_by_ten_percent_keeps_frequency_and_hits_the_length() {
    let file = sine(4 * BEAT);
    let (mut s, _, c) = session();
    let base = render(s.project(), 8.0, &file);
    assert_eq!(audible_len(&base), 4 * BEAT, "before: four beats");
    assert!((peak_hz(&base[..4 * BEAT]) - FREQ).abs() < 3.0);

    stretch(&mut s, c, 4.4);
    let out = render(s.project(), 8.0, &file);
    let want = (4.4 * BEAT as f64) as usize;
    let got = audible_len(&out);
    // Länge ±1 % laut Plan.
    assert!(
        (got as f64 - want as f64).abs() < want as f64 * 0.01,
        "length {got} vs {want}"
    );
    let f = peak_hz(&out[..want]);
    assert!((f - FREQ).abs() < FREQ * 0.01, "frequency {f}");

    // Rückgängig: bitgleich zum Original.
    s.undo().expect("undo");
    assert!(render(s.project(), 8.0, &file) == base, "undo is bitwise");
}

#[test]
fn pitch_shift_moves_the_fft_peak_and_keeps_the_length() {
    let file = sine(4 * BEAT);
    let (mut s, _, c) = session();
    let base = render(s.project(), 8.0, &file);
    s.apply(Command::SetClipPitch {
        clip: c,
        semitones: 2.0,
    })
    .expect("pitch");
    let out = render(s.project(), 8.0, &file);
    let got = audible_len(&out);
    assert!(
        (got as f64 - (4 * BEAT) as f64).abs() < (4 * BEAT) as f64 * 0.01,
        "length {got}"
    );
    let want = FREQ * 2f32.powf(2.0 / 12.0);
    let f = peak_hz(&out[..4 * BEAT]);
    assert!((f - want).abs() < want * 0.01, "{f} Hz vs {want}");

    // Wieder auf 0: das Original, bitgleich.
    s.apply(Command::SetClipPitch {
        clip: c,
        semitones: 0.0,
    })
    .expect("zero");
    let back = render(s.project(), 8.0, &file);
    assert!(back == base, "0 st is transparent");
}

#[test]
fn tempo_follow_stretches_to_the_new_grid() {
    let file = sine(4 * BEAT);
    let (mut s, _, c) = session();
    let base = render(s.project(), 8.0, &file);
    s.apply(Command::SetClipFollowTempo {
        clip: c,
        bpm: Some(120.0),
    })
    .expect("follow");
    // Gleiches Tempo: nichts verändert.
    let same = render(s.project(), 8.0, &file);
    assert!(same == base, "unchanged at the same tempo");
    s.apply(Command::SetTempo { bpm: 100.0 }).expect("tempo");
    // Bei 100 bpm dauert ein Beat 28 800 Samples; vier Beats sind 115 200.
    let out = render(s.project(), 8.0, &file);
    let want = 4 * 28_800;
    let got = audible_len(&out);
    assert!(
        (got as f64 - want as f64).abs() < want as f64 * 0.01,
        "length {got} vs {want}"
    );
    let f = peak_hz(&out[..want]);
    assert!((f - FREQ).abs() < FREQ * 0.01, "frequency {f}");

    // Ohne Folgen bleibt der Ton in seiner Länge (und wird bei 100 bpm nur anders zugeschnitten).
    let (mut s2, _, _) = session();
    s2.apply(Command::SetTempo { bpm: 100.0 }).expect("tempo");
    let plain = render(s2.project(), 8.0, &file);
    assert_eq!(
        audible_len(&plain),
        file.len(),
        "no follow: audio keeps its speed"
    );
}

#[test]
fn stretch_pitch_and_tempo_together_are_deterministic() {
    let file = sine(4 * BEAT);
    let (mut s, _, c) = session();
    stretch(&mut s, c, 5.0);
    s.apply(Command::SetClipPitch {
        clip: c,
        semitones: -3.0,
    })
    .expect("pitch");
    let a = render(s.project(), 8.0, &file);
    let b = render(s.project(), 8.0, &file);
    assert!(a == b, "same project renders the same");
    let want = FREQ * 2f32.powf(-3.0 / 12.0);
    let f = peak_hz(&a[..5 * BEAT]);
    assert!((f - want).abs() < want * 0.01, "{f} vs {want}");
}

#[test]
fn processed_clip_plays_without_allocation() {
    let file = sine(4 * BEAT);
    let (mut s, _, c) = session();
    stretch(&mut s, c, 4.4);
    s.apply(Command::SetClipPitch {
        clip: c,
        semitones: 2.0,
    })
    .expect("pitch");
    let (mut engine, mut proc_) = Engine::new(16);
    let f = Arc::clone(&file);
    let graph = clip_graph_mode(
        s.project(),
        &engine.transport(),
        RATE as f32,
        &mut |_: &str| Some(Arc::clone(&f)),
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
    for _ in 0..(4 * BEAT / 128) {
        assert_no_alloc(|| proc_.process_stereo(&mut l, &mut r));
        peak = l.iter().fold(peak, |m, v| m.max(v.abs()));
    }
    assert!(peak > 0.05, "sound came out: {peak}");
}

/// M6-Abnahme „ganzer Song“ ohne Gerät: drei Aufnahmen auf zwei Spuren, geteilt, mit Blenden,
/// einer überlappenden Überblendung, einer gedehnten und einer transponierten Strecke und
/// geändertem Tempo; exportiert als WAV und wieder eingelesen. Gemessen: Länge, keine
/// Knackser, kein NaN, Pegel in sinnvollem Bereich, Frequenzen stimmen.
#[test]
fn whole_song_exports_clean() {
    let a: Arc<[f32]> = sine(8 * BEAT); // 440 Hz, Strophe
    let b: Arc<[f32]> = (0..8 * BEAT)
        .map(|i| 0.4 * (std::f32::consts::TAU * 330.0 * i as f32 / RATE as f32).sin())
        .collect::<Vec<_>>()
        .into();
    let (mut s, t, c1) = session();
    // Strophe: 8 Beats, geteilt, der zweite Teil um 10 % gedehnt und 2 Halbtöne höher.
    s.apply(Command::EditClip {
        clip: c1,
        length_beats: 8.0,
        offset_beats: 0.0,
        fade_in_beats: 0.25,
        fade_out_beats: 0.0,
    })
    .expect("edit");
    let right = s.new_clip_id();
    let mut part = s.project().clip(c1).expect("c").1.clone();
    part.id = right;
    part.start_beats = 4.0;
    part.length_beats = 4.0;
    part.offset_beats = 4.0;
    part.fade_in_beats = 0.0;
    s.apply(Command::EditClip {
        clip: c1,
        length_beats: 4.0,
        offset_beats: 0.0,
        fade_in_beats: 0.25,
        fade_out_beats: 0.0,
    })
    .expect("left");
    s.apply(Command::AddClip {
        track: t,
        clip: part,
        index: None,
    })
    .expect("right");
    s.apply(Command::SetClipStretch {
        clip: right,
        length_beats: 4.4,
        stretch: 1.1,
        fade_in_beats: 0.0,
        fade_out_beats: 0.5,
    })
    .expect("stretch");
    s.apply(Command::SetClipPitch {
        clip: right,
        semitones: 2.0,
    })
    .expect("pitch");
    // Refrain: zweite Datei, überlappt das Ende der Strophe (automatische Überblendung).
    let chorus = s.new_clip_id();
    s.apply(Command::AddClip {
        track: t,
        clip: Clip::new(chorus, "chorus", 7.9, 6.0, Some("b".into())),
        index: None,
    })
    .expect("chorus");
    s.apply(Command::SetTempo { bpm: 100.0 }).expect("tempo");

    let files: [(&str, Arc<[f32]>); 2] = [("sine", Arc::clone(&a)), ("b", Arc::clone(&b))];
    let mut load = |n: &str| {
        files
            .iter()
            .find(|(f, _)| *f == n)
            .map(|(_, d)| Arc::clone(d))
    };
    let seconds = 14.0 * 60.0 / 100.0;
    let (l, r) = render_clips(s.project(), RATE, seconds, &mut load).expect("render");
    let dir = tempfile::tempdir().expect("tmp");
    let path = dir.path().join("song.wav");
    woa_engine::clip::write_wav_stereo(&path, &l, &r, RATE).expect("export");
    let mut reader = hound::WavReader::open(&path).expect("read back");
    let spec = reader.spec();
    assert_eq!((spec.channels, spec.sample_rate), (2, RATE));
    let all: Vec<f32> = reader
        .samples::<f32>()
        .map(|v| v.expect("sample"))
        .collect();
    assert_eq!(all.len(), 2 * l.len());
    let left: Vec<f32> = all.iter().step_by(2).copied().collect();
    assert!(left.iter().all(|v| v.is_finite()), "no NaN or inf");
    let peak = left.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    assert!((0.1..=1.0).contains(&peak), "peak {peak}");
    // Keine Knackser: Sinus bis 660 Hz bei 0,5 Amplitude springt nie weiter als dieser Schritt.
    let step = left
        .windows(2)
        .map(|w| (w[1] - w[0]).abs())
        .fold(0.0f32, f32::max);
    assert!(step < 0.2, "max sample step {step}");
    // Der Song ist da und endet, wo er soll: letzter hörbarer Punkt nahe Beat 13,9 bei 100 bpm.
    let end = left.iter().rposition(|v| v.abs() > 0.02).expect("sound");
    let want = (13.9 * 28_800.0) as usize;
    assert!(
        (end as f64 - want as f64).abs() < 0.02 * want as f64,
        "ends at {end}, expected about {want}"
    );
    // Frequenzen: die gedehnte, transponierte Strecke liegt bei 440 * 2^(2/12) Hz.
    let from = (5.0 * 28_800.0) as usize;
    let f = peak_hz(&left[from..from + 3 * 28_800]);
    let want_f = 440.0 * 2f32.powf(2.0 / 12.0);
    assert!((f - want_f).abs() < want_f * 0.015, "{f} vs {want_f}");
}
