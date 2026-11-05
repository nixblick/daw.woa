//! Fade-Taste (Design Fassung 7, #320, #321, #274): blendet fest in 3 s aus und stoppt, je
//! Sende-Player, Deck, Cart oder alles zusammen; ein zweiter Druck stoppt sofort. Die
//! Stopp-Blende (#227) bleibt davon unberührt. Ohne Audiogerät, unter `assert_no_alloc`.

#![allow(clippy::unwrap_used, clippy::expect_used)] // Tests duerfen abbrechen (Regel 6 gilt fuer Produktivcode).

use std::path::{Path, PathBuf};

use assert_no_alloc::{AllocDisabler, assert_no_alloc};
use woa_radio::{Controller, Cues, FADE_SECONDS, Mixer, PlaylistEntry, RadioShow};

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
fn fade_is_fixed_at_three_seconds() {
    assert_eq!(FADE_SECONDS, 3.0);
}

#[test]
fn player_fade_takes_three_seconds_even_with_stop_fade_off() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut c, mut m, _) = setup(tmp.path());
    c.set_stop_fade(false, 2.0);
    c.start();
    let full = level_after(&mut c, &mut m, 1.0);
    assert!(full > 0.2);
    c.fade_music();
    let a = level_after(&mut c, &mut m, 1.0);
    let b = level_after(&mut c, &mut m, 1.8);
    assert!(a < full && a > 0.12, "nach 1 s noch gut hörbar: {a}");
    assert!(b > 0.0 && b < full * 0.3, "kurz vor 3 s fast still: {b}");
    assert_eq!(level_after(&mut c, &mut m, 0.4), 0.0, "nach 3 s still");
    assert!(!c.is_running(), "Playlist steht");
    // Danach startet nichts von selbst.
    assert_eq!(level_after(&mut c, &mut m, 1.0), 0.0);
}

#[test]
fn second_fade_stops_at_once() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut c, mut m, _) = setup(tmp.path());
    c.start();
    assert!(level_after(&mut c, &mut m, 1.0) > 0.2);
    c.fade_music();
    assert!(level_after(&mut c, &mut m, 0.5) > 0.1);
    c.fade_music();
    assert_eq!(level_after(&mut c, &mut m, 0.02), 0.0, "sofort aus");
}

#[test]
fn deck_fade_takes_three_seconds_and_second_press_stops() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut c, mut m, p) = setup(tmp.path());
    c.deck_load(1, p, Cues::default()).unwrap();
    c.deck_play(1);
    let full = level_after(&mut c, &mut m, 1.0);
    assert!(full > 0.2);
    c.fade_deck(1);
    let a = level_after(&mut c, &mut m, 2.0);
    assert!(a > 0.0 && a < full * 0.5, "Deck blendet aus: {a}");
    assert_eq!(level_after(&mut c, &mut m, 1.2), 0.0, "nach 3 s still");
    c.deck_play(1);
    assert!(level_after(&mut c, &mut m, 0.5) > 0.2);
    c.fade_deck(1);
    level_after(&mut c, &mut m, 0.3);
    c.fade_deck(1);
    assert_eq!(
        level_after(&mut c, &mut m, 0.02),
        0.0,
        "zweiter Druck: sofort aus"
    );
}

#[test]
fn cart_fade_takes_three_seconds() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut c, mut m, p) = setup(tmp.path());
    c.assign_cart(2, Some(p)).unwrap();
    c.trigger_cart(2);
    let full = level_after(&mut c, &mut m, 1.0);
    assert!(full > 0.2);
    c.fade_cart(2);
    let a = level_after(&mut c, &mut m, 1.0);
    assert!(a < full && a > 0.12, "Cart blendet aus: {a}");
    assert_eq!(level_after(&mut c, &mut m, 2.2), 0.0, "nach 3 s still");
}

#[test]
fn fade_all_fades_playlist_decks_and_carts() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut c, mut m, p) = setup(tmp.path());
    c.deck_load(0, p.clone(), Cues::default()).unwrap();
    c.assign_cart(0, Some(p)).unwrap();
    c.start();
    c.deck_play(0);
    c.trigger_cart(0);
    let full = level_after(&mut c, &mut m, 1.0);
    assert!(full > 0.5, "drei Quellen: {full}");
    assert!(c.anything_running());
    c.fade_all();
    let a = level_after(&mut c, &mut m, 1.5);
    assert!(a < full && a > 0.2, "alles blendet gemeinsam aus: {a}");
    assert_eq!(
        level_after(&mut c, &mut m, 1.7),
        0.0,
        "nach 3 s alles still"
    );
    c.pump();
    assert!(!c.anything_running());
}

#[test]
fn send_stop_and_fade_leave_a_manually_started_deck_alone() {
    // Moderator 05.11.2026 (nimmt #325 zurück): Stopp und Fade des Sende-Decks gelten nur
    // der Playlist; ein Deck stoppt nur an seiner eigenen Taste.
    let tmp = tempfile::tempdir().unwrap();
    let (mut c, mut m, p) = setup(tmp.path());
    c.deck_load(0, p, Cues::default()).unwrap();
    c.deck_play(0);
    assert!(level_after(&mut c, &mut m, 0.5) > 0.2);
    c.stop_or_pause();
    assert!(
        level_after(&mut c, &mut m, 0.5) > 0.2,
        "Stopp lässt das Deck laufen"
    );
    c.fade_music();
    assert!(
        level_after(&mut c, &mut m, 3.2) > 0.2,
        "Fade Sende-Deck lässt das Deck laufen"
    );
    c.deck_stop(0);
}
