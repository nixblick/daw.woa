//! Auto-Weiter in der Sende-Playlist abschaltbar (#148): aus, dann hält die Playlist nach
//! jedem Titel an und der nächste startet mit „Play“. Ohne Audiogerät, unter
//! `assert_no_alloc`.

use std::path::Path;

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

fn two_titles(dir: &Path, auto_next: bool) -> (Controller, Mixer) {
    let a = dir.join("eins.wav");
    let b = dir.join("zwei.wav");
    write_dc(&a, 2.0, 0.25);
    write_dc(&b, 2.0, 0.25);
    let show = RadioShow {
        crossfade_seconds: 0.0,
        bus_processing: false,
        mic_processing: false,
        auto_next,
        playlist: vec![PlaylistEntry::new(&a), PlaylistEntry::new(&b)],
        ..RadioShow::default()
    };
    let (mut c, m) = Controller::new(show, SR);
    c.measure_missing();
    (c, m)
}

#[test]
fn auto_next_is_on_by_default_and_saved() {
    assert!(RadioShow::default().auto_next);
    let mut c = Controller::new(RadioShow::default(), SR).0;
    c.set_auto_next(false);
    let json = serde_json::to_string(c.show()).unwrap();
    assert!(!RadioShow::from_json(&json).unwrap().auto_next);
}

#[test]
fn with_auto_next_the_second_title_follows() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut c, mut m) = two_titles(tmp.path(), true);
    c.start();
    let lvl = level_after(&mut c, &mut m, 3.0);
    assert!(lvl > 0.2, "zweiter Titel läuft: {lvl}");
}

#[test]
fn without_auto_next_it_stops_and_play_starts_the_next() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut c, mut m) = two_titles(tmp.path(), false);
    c.start();
    assert!(level_after(&mut c, &mut m, 1.0) > 0.2);
    let lvl = level_after(&mut c, &mut m, 2.0);
    assert!(lvl < 0.01, "nach dem Titel still: {lvl}");
    assert!(!c.is_running());
    assert_eq!(c.cue_index(), 1, "der nächste steht bereit");
    c.play();
    let lvl = level_after(&mut c, &mut m, 1.0);
    assert!(lvl > 0.2, "Play startet den nächsten: {lvl}");
    assert_eq!(c.current_index(), Some(1));
}
