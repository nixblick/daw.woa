//! #297: Die Verstärkung eines Titels bleibt innerhalb des Titels konstant, mit
//! eingeschaltetem Sendebus (Vorgabe). Erlaubt sind nur Blenden am Anfang und Ende und das
//! Ducking bei offenem Mikro oder laufendem Cart. Offline, ohne Ton, jeder Block unter
//! `assert_no_alloc`.

use std::path::Path;

use assert_no_alloc::{AllocDisabler, assert_no_alloc};
use woa_radio::{Controller, Cues, Mixer, PlaylistEntry, RadioShow};

#[global_allocator]
static ALLOC: AllocDisabler = AllocDisabler;

const SR: u32 = 48_000;
const BLOCK: usize = 256;
/// Abschnitte des Titels: leise, laut, leise (Sekunden).
const PART: f64 = 6.0;
const QUIET: f32 = 0.03; // etwa −31 LUFS
const LOUD: f32 = 0.15; // etwa −17 LUFS, unter der Kompressor-Schwelle

/// 1-kHz-Sinus mit leisem, lautem und wieder leisem Teil.
fn write_dynamic(path: &Path) {
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: SR,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut w = hound::WavWriter::create(path, spec).unwrap();
    let part = (PART * f64::from(SR)) as usize;
    for i in 0..3 * part {
        let amp = if (part..2 * part).contains(&i) {
            LOUD
        } else {
            QUIET
        };
        let v = (i as f32 * 1_000.0 * std::f32::consts::TAU / SR as f32).sin() * amp;
        w.write_sample(v).unwrap();
        w.write_sample(v).unwrap();
    }
    w.finalize().unwrap();
}

fn amp_at(frame: usize) -> f32 {
    let part = (PART * f64::from(SR)) as usize;
    if (part..2 * part).contains(&frame) {
        LOUD
    } else {
        QUIET
    }
}

fn run(c: &mut Controller, m: &mut Mixer, seconds: f64) -> Vec<f32> {
    let (mut l, mut r) = (vec![0.0f32; BLOCK], vec![0.0f32; BLOCK]);
    let mut out = Vec::new();
    for _ in 0..(seconds * f64::from(SR)) as usize / BLOCK {
        c.pump();
        assert_no_alloc(|| m.process(&mut l, &mut r));
        out.extend_from_slice(&l);
    }
    out
}

/// Verstärkung in dB je 50-ms-Fenster (Spitze Ausgang / Spitze Quelle), ab dem Einsatz des
/// Titels, ohne die erste und letzte Sekunde (Blenden) und ohne die Fenster an den
/// Übergängen zwischen den Teilen.
fn gains_db(out: &[f32]) -> Vec<f32> {
    let start = out
        .iter()
        .position(|v| v.abs() > 1e-4)
        .expect("Titel spielt");
    let win = SR as usize / 20;
    let part = (PART * f64::from(SR)) as usize;
    let len = 3 * part;
    let mut g = Vec::new();
    let mut f = SR as usize;
    while f + win < len - SR as usize {
        let near_edge = [part, 2 * part].iter().any(|&e| f + win > e && f < e + win);
        if !near_edge && start + f + win <= out.len() {
            let peak = out[start + f..start + f + win]
                .iter()
                .fold(0.0f32, |a, v| a.max(v.abs()));
            g.push(20.0 * (peak / amp_at(f)).log10());
        }
        f += win;
    }
    g
}

fn spread(g: &[f32]) -> f32 {
    let lo = g.iter().copied().fold(f32::INFINITY, f32::min);
    let hi = g.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    hi - lo
}

fn show() -> RadioShow {
    RadioShow {
        mic_processing: false,
        // Sendebus an wie in der Sendung (Vorgabe).
        bus_processing: true,
        ..RadioShow::default()
    }
}

#[test]
fn playlist_title_keeps_one_gain_from_start_to_end() {
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("dyn.wav");
    write_dynamic(&file);
    let mut s = show();
    s.crossfade_seconds = 0.5;
    s.playlist = vec![PlaylistEntry::new(&file)];
    let (mut c, mut m) = Controller::new(s, SR);
    c.start();
    let out = run(&mut c, &mut m, 3.0 * PART + 1.0);
    let g = gains_db(&out);
    assert!(g.len() > 100);
    assert!(
        spread(&g) < 0.5,
        "Playlist: Verstärkung schwankt um {} dB",
        spread(&g)
    );
}

#[test]
fn deck_title_keeps_one_gain_and_is_levelled_like_the_playlist() {
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("dyn.wav");
    write_dynamic(&file);
    let (mut c, mut m) = Controller::new(show(), SR);
    c.deck_load(0, file.clone(), Cues::default()).unwrap();
    run(&mut c, &mut m, 0.05);
    c.deck_play(0);
    let out = run(&mut c, &mut m, 3.0 * PART + 1.0);
    let g = gains_db(&out);
    assert!(g.len() > 100);
    assert!(
        spread(&g) < 0.5,
        "Deck: Verstärkung schwankt um {} dB",
        spread(&g)
    );
    // Der Deck-Titel bekommt dieselbe feste Verstärkung wie in der Playlist (Auto-Gain).
    let mut e = PlaylistEntry::new(&file);
    e.set_measured(woa_radio::audio::measure_file(&file, SR).unwrap());
    let want = e.gain_db(-16.0) as f32;
    let mid = g[g.len() / 2];
    assert!((mid - want).abs() < 0.5, "Deck: {mid} dB statt {want} dB");
}

#[test]
fn music_returns_to_its_level_after_a_cart() {
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("dyn.wav");
    write_dynamic(&file);
    let jingle = tmp.path().join("jingle.wav");
    {
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: SR,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };
        let mut w = hound::WavWriter::create(&jingle, spec).unwrap();
        for i in 0..(3 * SR) as usize {
            let v = (i as f32 * 440.0 * std::f32::consts::TAU / SR as f32).sin() * 0.05;
            w.write_sample(v).unwrap();
            w.write_sample(v).unwrap();
        }
        w.finalize().unwrap();
    }
    let mut s = show();
    s.playlist = vec![PlaylistEntry::new(&file)];
    let (mut c, mut m) = Controller::new(s, SR);
    c.assign_cart(0, Some(jingle)).unwrap();
    c.start();
    // Im lauten Teil: vorher messen, Cart (3 s) ducken lassen, danach wieder messen.
    let before = run(&mut c, &mut m, PART + 1.0);
    c.trigger_cart(0);
    run(&mut c, &mut m, 3.5);
    let after = run(&mut c, &mut m, 0.5);
    let peak = |x: &[f32]| x.iter().fold(0.0f32, |a, v| a.max(v.abs()));
    let b = peak(&before[before.len() - SR as usize / 2..]);
    let a = peak(&after);
    let diff = 20.0 * (a / b).log10();
    assert!(
        diff.abs() < 0.5,
        "nach dem Cart {diff} dB anders als vorher"
    );
}
