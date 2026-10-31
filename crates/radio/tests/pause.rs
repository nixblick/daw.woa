//! Pause hält den Titel fest (#169, #95): Ausblenden, Stelle stehen lassen, Play macht ohne
//! Neuladen dort weiter. Stopp nach Pause verwirft den Titel. Ohne Audiogerät.

use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering::Acquire;

use woa_radio::{Controller, Mixer, PlaylistEntry, RadioShow};

const SR: u32 = 48_000;
const BLOCK: usize = 256;

fn write_tone(path: &Path, seconds: f64) {
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: SR,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut w = hound::WavWriter::create(path, spec).unwrap();
    for i in 0..(seconds * f64::from(SR)) as usize {
        let v = (i as f32 * 440.0 * std::f32::consts::TAU / SR as f32).sin() * 0.3;
        w.write_sample(v).unwrap();
        w.write_sample(v).unwrap();
    }
    w.finalize().unwrap();
}

fn setup(dir: &Path) -> (Controller, Mixer) {
    let paths: Vec<PathBuf> = ["eins", "zwei"]
        .iter()
        .map(|n| {
            let p = dir.join(format!("{n}.wav"));
            write_tone(&p, 20.0);
            p
        })
        .collect();
    let show = RadioShow {
        crossfade_seconds: 0.0,
        bus_processing: false,
        mic_processing: false,
        playlist: paths.iter().map(PlaylistEntry::new).collect(),
        ..RadioShow::default()
    };
    let (mut c, m) = Controller::new(show, SR);
    c.measure_missing();
    (c, m)
}

/// Spielt `blocks` Blöcke und gibt die lauteste Probe zurück.
fn step(c: &mut Controller, m: &mut Mixer, blocks: usize) -> f32 {
    let mut l = vec![0.0f32; BLOCK];
    let mut r = vec![0.0f32; BLOCK];
    let mut peak = 0.0f32;
    for _ in 0..blocks {
        c.pump();
        m.process(&mut l, &mut r);
        c.pump();
        peak = l.iter().fold(peak, |p, v| p.max(v.abs()));
    }
    peak
}

fn secs(s: f64) -> usize {
    (s * f64::from(SR)) as usize / BLOCK
}

#[test]
fn pause_holds_the_title_and_play_resumes_without_reloading() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut c, mut m) = setup(tmp.path());
    c.start();
    assert!(step(&mut c, &mut m, secs(3.0)) > 0.1, "Musik läuft");
    let uid = c.status().current.load(Acquire);
    let plays = c.status().plays.load(Acquire);

    c.pause();
    // Ausblenden, dann Stille; der Titel bleibt im Mixer und die Stelle steht.
    step(&mut c, &mut m, secs(2.0));
    let held_at = c.status().position.load(Acquire);
    assert_eq!(step(&mut c, &mut m, secs(2.0)), 0.0, "pausiert ist still");
    assert_eq!(
        c.status().current.load(Acquire),
        uid,
        "Titel bleibt gehalten"
    );
    assert_eq!(c.status().position.load(Acquire), held_at, "Stelle steht");
    assert!(c.is_paused() && !c.is_running());

    c.play();
    // Sofort wieder Ton, kein Neuladen (derselbe Titel, kein neuer Start im Mixer).
    assert!(step(&mut c, &mut m, 2) > 0.05, "Play spielt sofort weiter");
    assert_eq!(c.status().current.load(Acquire), uid);
    assert_eq!(
        c.status().plays.load(Acquire),
        plays,
        "kein Neustart des Titels"
    );
    assert!(c.status().position.load(Acquire) >= held_at);
    assert!(c.is_running() && !c.is_paused());
}

#[test]
fn stop_after_pause_drops_the_title_and_start_begins_fresh() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut c, mut m) = setup(tmp.path());
    c.start();
    step(&mut c, &mut m, secs(3.0));
    c.pause();
    step(&mut c, &mut m, secs(2.0));
    c.stop();
    step(&mut c, &mut m, 4);
    assert_eq!(
        c.status().current.load(Acquire),
        woa_radio::mixer::NONE,
        "Stopp verwirft den gehaltenen Titel"
    );
    assert!(!c.is_paused());

    // Start nach Pause (ohne Play): beginnt den Cue-Titel neu, nicht den gehaltenen weiter.
    c.start();
    step(&mut c, &mut m, secs(0.5));
    c.pause();
    step(&mut c, &mut m, secs(2.0));
    c.start();
    assert!(
        step(&mut c, &mut m, secs(0.5)) > 0.1,
        "Start nach Pause spielt"
    );
    assert!(
        c.status().position.load(Acquire) < u64::from(SR),
        "von vorn"
    );
}
