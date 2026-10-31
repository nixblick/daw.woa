//! Stopp mit Ausblenden (#227): Ist es an, blenden Stopp am Sende-Player und an den Decks über
//! die eingestellte Dauer aus; aus bleibt der Stopp hart. Ohne Audiogerät, unter
//! `assert_no_alloc`.

use std::path::{Path, PathBuf};

use assert_no_alloc::{AllocDisabler, assert_no_alloc};
use woa_radio::{Controller, Cues, Mixer, PlaylistEntry, RadioShow};

#[global_allocator]
static ALLOC: AllocDisabler = AllocDisabler;

const SR: u32 = 48_000;
const BLOCK: usize = 256;

fn write_dc(path: &Path, seconds: f64, amp: f32) {
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: SR,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut w = hound::WavWriter::create(path, spec).unwrap();
    for _ in 0..(seconds * f64::from(SR)) as usize * 2 {
        w.write_sample(amp).unwrap();
    }
    w.finalize().unwrap();
}

fn setup(dir: &Path) -> (Controller, Mixer, PathBuf) {
    let p = dir.join("eins.wav");
    write_dc(&p, 20.0, 0.25);
    let show = RadioShow {
        crossfade_seconds: 0.0,
        bus_processing: false,
        mic_processing: false,
        playlist: vec![PlaylistEntry::new(&p)],
        ..RadioShow::default()
    };
    let (mut c, m) = Controller::new(show, SR);
    c.measure_missing();
    (c, m, p)
}

/// Spielt so viele Sekunden und gibt den Pegel des letzten Blocks zurück.
fn level_after(c: &mut Controller, m: &mut Mixer, seconds: f64) -> f32 {
    let (mut l, mut r) = (vec![0.0f32; BLOCK], vec![0.0f32; BLOCK]);
    let n = ((seconds * f64::from(SR)) as usize / BLOCK).max(1);
    for _ in 0..n {
        c.pump();
        assert_no_alloc(|| m.process(&mut l, &mut r));
    }
    l.iter().fold(0.0f32, |p, v| p.max(v.abs()))
}

#[test]
fn stop_fade_is_off_by_default_and_saved() {
    let show = RadioShow::default();
    assert!(!show.stop_fade);
    let mut c = Controller::new(show, SR).0;
    c.set_stop_fade(true, 4.0);
    let json = serde_json::to_string(c.show()).unwrap();
    let back = RadioShow::from_json(&json).unwrap();
    assert!(back.stop_fade);
    assert_eq!(back.stop_fade_seconds, 4.0);
}

#[test]
fn player_stop_fades_out_over_the_set_time() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut c, mut m, _) = setup(tmp.path());
    c.set_stop_fade(true, 2.0);
    c.start();
    let full = level_after(&mut c, &mut m, 1.0);
    assert!(full > 0.2, "Musik läuft: {full}");
    c.stop();
    let a = level_after(&mut c, &mut m, 0.5);
    let b = level_after(&mut c, &mut m, 0.5);
    assert!(a < full && a > 0.1, "nach 0,5 s noch laut: {a}");
    assert!(b < a && b > 0.02, "nach 1 s leiser: {b}");
    assert_eq!(level_after(&mut c, &mut m, 1.5), 0.0, "danach still");
}

#[test]
fn player_stop_stays_hard_when_off() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut c, mut m, _) = setup(tmp.path());
    c.set_stop_fade(false, 2.0);
    c.start();
    assert!(level_after(&mut c, &mut m, 1.0) > 0.2);
    c.stop();
    assert_eq!(level_after(&mut c, &mut m, 0.1), 0.0, "hart aus");
}

#[test]
fn deck_stop_fades_out_and_stays_hard_when_off() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut c, mut m, p) = setup(tmp.path());
    c.deck_load(0, p, Cues::default()).unwrap();
    c.set_stop_fade(true, 2.0);
    c.deck_play(0);
    let full = level_after(&mut c, &mut m, 1.0);
    assert!(full > 0.2);
    c.deck_stop(0);
    let a = level_after(&mut c, &mut m, 1.0);
    assert!(a < full && a > 0.05, "Deck blendet aus: {a}");
    assert_eq!(level_after(&mut c, &mut m, 1.5), 0.0, "danach still");

    c.set_stop_fade(false, 2.0);
    c.deck_play(0);
    assert!(level_after(&mut c, &mut m, 0.5) > 0.2);
    c.deck_stop(0);
    assert_eq!(level_after(&mut c, &mut m, 0.1), 0.0, "Deck hart aus");
}
