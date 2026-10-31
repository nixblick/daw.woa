//! Party mit zwei Decks (André 31.10.2026): echte Zwei-Deck-Wiedergabe, Crossfader mit
//! konstanter Leistung, Auto-Übergang an und aus, die Hand gewinnt gegen die Automatik.
//! Offline, ohne Ton, jeder Block unter `assert_no_alloc`.

use std::path::Path;
use std::sync::atomic::Ordering::Acquire;

use assert_no_alloc::{AllocDisabler, assert_no_alloc};
use woa_radio::mixer::DECK_PLAYING;
use woa_radio::{Controller, Mixer, PlaylistEntry, RadioShow};

#[global_allocator]
static ALLOC: AllocDisabler = AllocDisabler;

const SR: u32 = 48_000;
const BLOCK: usize = 240; // 5 ms

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

/// Drei Titel zu 3 s, Überblendung 1 s.
fn setup(dir: &Path) -> (Controller, Mixer) {
    let playlist = (0..3)
        .map(|k| {
            let p = dir.join(format!("t{k}.wav"));
            write_dc(&p, 3.0, 0.3);
            PlaylistEntry::new(&p)
        })
        .collect();
    let show = RadioShow {
        crossfade_seconds: 1.0,
        bus_processing: false,
        mic_processing: false,
        playlist,
        ..RadioShow::default()
    };
    let (mut c, m) = Controller::new(show, SR);
    c.measure_missing();
    (c, m)
}

/// Spielt `secs` Sekunden; gibt den Pegel (links) am Ende jedes Blocks zurück.
fn run(c: &mut Controller, m: &mut Mixer, secs: f64) -> Vec<f32> {
    let n = (secs * f64::from(SR)) as usize / BLOCK;
    let (mut l, mut r) = (vec![0.0f32; BLOCK], vec![0.0f32; BLOCK]);
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        c.pump();
        assert_no_alloc(|| m.process(&mut l, &mut r));
        out.push(l[BLOCK - 1]);
    }
    out
}

fn playing(c: &Controller, side: usize) -> bool {
    c.status().party_state[side].load(Acquire) == DECK_PLAYING
}

fn index(c: &Controller, side: usize) -> Option<usize> {
    c.party_view().unwrap().sides[side].index
}

fn fader(c: &Controller) -> f32 {
    c.party_view().unwrap().fader
}

#[test]
fn both_sides_are_loaded_from_the_playlist() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut c, mut m) = setup(tmp.path());
    c.party_enter();
    run(&mut c, &mut m, 0.05);
    assert_eq!(index(&c, 0), Some(0), "links der erste Titel");
    assert_eq!(index(&c, 1), Some(1), "rechts vorgeladen der nächste");
    assert!(
        !playing(&c, 0) && !playing(&c, 1),
        "nichts startet von selbst"
    );
    assert!(c.party_auto(), "Auto-Übergang ist Vorgabe");
}

#[test]
fn auto_transition_starts_the_other_side_and_reloads_the_free_one() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut c, mut m) = setup(tmp.path());
    c.party_enter();
    c.party_toggle();
    let v = run(&mut c, &mut m, 1.5);
    assert!(v.last().unwrap().abs() > 0.05, "links spielt");
    assert!(playing(&c, 0) && !playing(&c, 1));
    assert_eq!(fader(&c), 0.0);

    // Ab 2 s (Crossfade-Länge vor dem Ende) startet rechts, der Fader fährt hinüber.
    run(&mut c, &mut m, 1.0);
    assert!(
        playing(&c, 0) && playing(&c, 1),
        "beide spielen in der Blende"
    );
    let f = fader(&c);
    assert!(f > 0.2 && f < 0.8, "Fader mitten in der Fahrt: {f}");
    // Constant Power in der Blende: keine Delle (gleich laute Titel).
    let v = run(&mut c, &mut m, 0.6);
    assert!(
        v.iter().all(|x| x.abs() > 0.1),
        "kein Lautstärkeloch in der Blende"
    );
    assert!(!playing(&c, 0), "links ist durch");
    assert_eq!(fader(&c), 1.0, "Fader exakt rechts");
    run(&mut c, &mut m, 0.05);
    assert_eq!(c.party_view().unwrap().live, 1, "rechts ist hörbar");
    assert_eq!(
        index(&c, 0),
        Some(2),
        "die freie Seite lädt den übernächsten"
    );
    assert!(!playing(&c, 0), "und startet nicht");
}

#[test]
fn without_auto_the_end_preloads_and_does_not_start() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut c, mut m) = setup(tmp.path());
    c.set_party_auto(false);
    c.party_enter();
    c.party_toggle();
    run(&mut c, &mut m, 2.5);
    assert!(!playing(&c, 1), "ohne Automatik startet rechts nicht");
    let v = run(&mut c, &mut m, 1.0);
    assert!(!playing(&c, 0) && !playing(&c, 1), "Ende: alles steht");
    assert!(v.last().unwrap().abs() < 1e-6, "still");
    assert_eq!(
        c.party_view().unwrap().live,
        1,
        "rechts ist als Nächstes dran"
    );
    assert_eq!(index(&c, 1), Some(1));
    assert_eq!(index(&c, 0), Some(2), "links lädt vor");
    // Der DJ startet selbst.
    c.party_play(1);
    let v = run(&mut c, &mut m, 0.2);
    assert!(playing(&c, 1));
    // Der Fader steht noch links: von Hand hinüber.
    assert!(v.last().unwrap().abs() < 1e-6, "Fader links, rechts stumm");
    c.party_fader(1.0);
    let v = run(&mut c, &mut m, 0.1);
    assert!(v.last().unwrap().abs() > 0.05, "jetzt hörbar");
}

#[test]
fn the_hand_beats_the_automatic() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut c, mut m) = setup(tmp.path());
    c.party_enter();
    c.party_toggle();
    run(&mut c, &mut m, 2.3);
    assert!(playing(&c, 1), "die Automatik blendet");
    let f = fader(&c);
    assert!(f > 0.0 && f < 0.6, "unterwegs: {f}");
    // Der DJ zieht zurück nach links und hält dort.
    c.party_fader(0.0);
    run(&mut c, &mut m, 0.5);
    assert_eq!(fader(&c), 0.0, "die Automatik fährt nicht dagegen");
    assert!(playing(&c, 1), "rechts läuft weiter, nur leise");
}

#[test]
fn dragging_over_the_middle_is_not_next() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut c, mut m) = setup(tmp.path());
    c.party_enter();
    c.party_toggle();
    run(&mut c, &mut m, 0.3);
    c.party_fader(0.9);
    run(&mut c, &mut m, 0.3);
    assert!(playing(&c, 0) && !playing(&c, 1), "kein Weiter");
    assert_eq!(index(&c, 0), Some(0));
    assert!((fader(&c) - 0.9).abs() < 1e-6);
}

#[test]
fn next_blends_now_and_view_follows_the_louder_side() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut c, mut m) = setup(tmp.path());
    c.party_enter();
    c.party_toggle();
    run(&mut c, &mut m, 0.3);
    assert_eq!(c.party_live_index(), Some(0));
    c.party_next();
    run(&mut c, &mut m, 1.1);
    assert_eq!(fader(&c), 1.0);
    assert_eq!(c.party_live_index(), Some(1), "läuft = die hörbare Seite");
}

#[test]
fn entering_takes_over_the_running_playlist_and_leaving_hands_back() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut c, mut m) = setup(tmp.path());
    c.start();
    run(&mut c, &mut m, 1.0);
    assert!(c.is_running());
    c.party_enter();
    let v = run(&mut c, &mut m, 0.3);
    assert!(!c.is_running(), "der Playlist-Abspieler ist aus");
    assert!(playing(&c, 0), "links spielt weiter");
    assert!(v.iter().all(|x| x.abs() > 0.05), "keine Lücke beim Wechsel");
    let pos = c.party_view().unwrap().sides[0].position;
    assert!(pos > 1.0, "an derselben Stelle: {pos}");
    c.party_leave();
    let v = run(&mut c, &mut m, 0.3);
    assert!(c.is_running() && !c.party_active());
    assert_eq!(c.current_index(), Some(0));
    assert!(v.iter().all(|x| x.abs() > 0.05), "keine Lücke zurück");
    assert!(!playing(&c, 0));
}
