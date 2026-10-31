//! Stopp ist zuerst Pause, zweimal Stopp (#95). Mit „Stopp blendet aus“ (#227) blendet die
//! Pause über die eingestellte Dauer aus. Ohne Audiogerät, unter
//! `assert_no_alloc`.

use std::path::{Path, PathBuf};

use assert_no_alloc::{AllocDisabler, assert_no_alloc};
use woa_radio::{Controller, Mixer, PlaylistEntry, RadioShow};

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
fn first_stop_pauses_and_play_resumes() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut c, mut m, _) = setup(tmp.path());
    c.start();
    let full = level_after(&mut c, &mut m, 1.0);
    assert!(full > 0.2, "Musik läuft: {full}");
    c.stop_or_pause();
    assert!(c.is_paused(), "erster Stopp ist Pause");
    let quiet = level_after(&mut c, &mut m, 1.0);
    assert!(quiet < 0.01, "pausiert still: {quiet}");
    c.play();
    let again = level_after(&mut c, &mut m, 1.0);
    assert!(again > 0.2, "weiter an der Stelle: {again}");
}

#[test]
fn second_stop_stops() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut c, mut m, _) = setup(tmp.path());
    c.start();
    level_after(&mut c, &mut m, 1.0);
    c.stop_or_pause();
    level_after(&mut c, &mut m, 0.2);
    c.stop_or_pause();
    assert!(!c.is_paused(), "zweiter Stopp vergisst die Stelle");
    let quiet = level_after(&mut c, &mut m, 1.0);
    assert!(quiet < 0.01, "gestoppt still: {quiet}");
}

#[test]
fn stop_fade_also_fades_the_pause() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut c, mut m, _) = setup(tmp.path());
    c.set_stop_fade(true, 2.0);
    c.start();
    let full = level_after(&mut c, &mut m, 1.0);
    c.stop_or_pause();
    assert!(c.is_paused());
    let a = level_after(&mut c, &mut m, 0.5);
    assert!(a < full && a > 0.1, "blendet über die Dauer aus: {a}");
    let b = level_after(&mut c, &mut m, 2.0);
    assert!(b < 0.01, "danach still: {b}");
}
