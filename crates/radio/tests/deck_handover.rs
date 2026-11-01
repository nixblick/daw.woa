//! Deck und Playlist (André 31.10.2026): Ein Deck löst die Playlist ab (mit Blende oder
//! harter Schnitt), und nur mit „Zurück zur Playlist“ läuft sie nach dem Deck weiter.
//! Offline, ohne Ton, jeder Block unter `assert_no_alloc`. Gemessen wird der Pegel am
//! Ausgang: Playlist ist Gleichwert 0,5, das Deck 0,2, beide mit Auto-Gain (#297).

use std::path::Path;
use std::sync::atomic::Ordering::Acquire;

use assert_no_alloc::{AllocDisabler, assert_no_alloc};
use woa_radio::mixer::{DECK_PLAYING, DECK_READY};
use woa_radio::{Controller, Cues, Mixer, PlaylistEntry, RadioShow};

#[global_allocator]
static ALLOC: AllocDisabler = AllocDisabler;

const SR: u32 = 48_000;
const BLOCK: usize = 240; // 5 ms
const MUSIC: f32 = 0.5;
const DECK: f32 = 0.2;
/// Pegel des Decks am Ausgang: die Datei mit ihrem Auto-Gain wie in der Playlist (#297).
static DECK_OUT: std::sync::LazyLock<f32> = std::sync::LazyLock::new(|| {
    let tmp = tempfile::tempdir().unwrap();
    let f = tmp.path().join("deck.wav");
    write_dc(&f, 3.0, DECK);
    let m = woa_radio::audio::measure_file(&f, SR).unwrap();
    let g = woa_radio::level::auto_gain_db(m.lufs, m.peak_db, woa_radio::level::TARGET_LUFS);
    DECK * woa_radio::level::db_to_gain(g)
});

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

/// Playlist mit einem langen Titel, Deck A mit 3 s, Überblendung 1 s.
fn setup(dir: &Path) -> (Controller, Mixer) {
    let song = dir.join("song.wav");
    write_dc(&song, 60.0, MUSIC);
    let deck = dir.join("deck.wav");
    write_dc(&deck, 3.0, DECK);
    let show = RadioShow {
        crossfade_seconds: 1.0,
        bus_processing: false,
        mic_processing: false,
        playlist: vec![PlaylistEntry::new(&song)],
        ..RadioShow::default()
    };
    let (mut c, m) = Controller::new(show, SR);
    c.measure_missing();
    c.deck_load(0, deck, Cues::default()).unwrap();
    (c, m)
}

/// Spielt `secs` Sekunden und gibt den Pegel (linker Kanal) am Ende jedes Blocks zurück.
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

fn last(v: &[f32]) -> f32 {
    *v.last().unwrap()
}

/// Playlist läuft voll; gibt ihren Pegel zurück (mit Auto-Gain der Playlist).
fn music_running(c: &mut Controller, m: &mut Mixer) -> f32 {
    c.start();
    let v = run(c, m, 0.5);
    assert!(last(&v) > 0.3, "Playlist läuft: {}", last(&v));
    last(&v)
}

#[test]
fn deck_with_fade_takes_over_and_playlist_returns_with_fade() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut c, mut m) = setup(tmp.path());
    c.set_deck_return(0, true);
    let mu = music_running(&mut c, &mut m);

    c.deck_take_over(0, true).unwrap();
    // Die Blende: Playlist fällt, Deck steigt, nach 1 s nur noch das Deck.
    let v = run(&mut c, &mut m, 1.2);
    let half = v[v.len() * 5 / 12];
    assert!(
        half > *DECK_OUT && half < mu + *DECK_OUT,
        "mitten in der Blende: {half}"
    );
    // Seit #297 haben Deck und Playlist dieselbe Lautheit; die Reihenfolge der Blende ist
    // am Summenpegel nicht mehr abzulesen, nur dass sie über beiden liegt.
    assert!(
        (last(&v) - *DECK_OUT).abs() < 1e-3,
        "nur das Deck: {}",
        last(&v)
    );
    assert!(c.is_paused(), "Playlist pausiert");

    // Restliche 1,8 s des Decks: in der letzten Sekunde blendet die Playlist ein.
    let v = run(&mut c, &mut m, 2.2);
    assert!(!c.is_paused() && c.is_running(), "Playlist läuft wieder");
    // Pegelkurve der Rückkehr: über die letzte Sekunde steigt der Anteil der Musik stetig.
    let ramp: Vec<f32> = v
        .iter()
        .copied()
        .filter(|x| *x > *DECK_OUT + 0.02)
        .collect();
    assert!(
        ramp.len() > 50,
        "es gibt eine Blende ({} Blöcke)",
        ramp.len()
    );
    assert!((last(&v) - mu).abs() < 1e-3, "Playlist voll: {}", last(&v));
    assert_eq!(c.status().deck_state[0].load(Acquire), DECK_READY);
    assert_eq!(
        c.status().deck_done[0].load(Acquire),
        1,
        "Ereignis Deck fertig"
    );
}

#[test]
fn without_back_to_playlist_it_stays_silent_after_the_deck() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut c, mut m) = setup(tmp.path());
    assert!(!c.deck_return(0), "Vorgabe aus");
    let _mu = music_running(&mut c, &mut m);
    c.deck_take_over(0, true).unwrap();
    run(&mut c, &mut m, 3.5);
    let v = run(&mut c, &mut m, 1.0);
    assert!(v.iter().all(|x| *x == 0.0), "Stille nach dem Deck");
    assert!(c.is_paused(), "Playlist wartet an der Pausenstelle");
    assert_eq!(c.status().deck_done[0].load(Acquire), 1);
}

#[test]
fn deck_without_fade_cuts_hard() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut c, mut m) = setup(tmp.path());
    let mu = music_running(&mut c, &mut m);
    c.deck_take_over(0, false).unwrap();
    // Nach 50 ms (Klickschutz 30 ms) nur noch das Deck, voll von Anfang an.
    let v = run(&mut c, &mut m, 0.05);
    assert!(
        (v[0] - mu - *DECK_OUT).abs() < 0.2,
        "Deck sofort voll: {}",
        v[0]
    );
    assert!(
        (last(&v) - *DECK_OUT).abs() < 1e-3,
        "Playlist weg: {}",
        last(&v)
    );
    assert_eq!(c.status().deck_state[0].load(Acquire), DECK_PLAYING);
}

#[test]
fn loading_a_deck_starts_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut c, mut m) = setup(tmp.path());
    c.set_deck_return(0, true);
    let v = run(&mut c, &mut m, 0.5);
    assert!(v.iter().all(|x| *x == 0.0), "nichts läuft");
    assert_eq!(c.status().deck_state[0].load(Acquire), DECK_READY);
    // Neu laden, während die Playlist läuft: die Playlist bleibt, das Deck steht.
    let mu = music_running(&mut c, &mut m);
    let deck = tmp.path().join("deck.wav");
    c.deck_load(0, deck, Cues::default()).unwrap();
    let v = run(&mut c, &mut m, 0.5);
    assert!((last(&v) - mu).abs() < 1e-3);
    assert_eq!(c.status().deck_state[0].load(Acquire), DECK_READY);
    assert!(c.is_running());
}

#[test]
fn deck_play_on_the_deck_stays_parallel() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut c, mut m) = setup(tmp.path());
    c.set_deck_return(0, true);
    let mu = music_running(&mut c, &mut m);
    c.deck_play(0);
    let v = run(&mut c, &mut m, 0.5);
    assert!(
        (last(&v) - mu - *DECK_OUT).abs() < 1e-3,
        "beide: {}",
        last(&v)
    );
    run(&mut c, &mut m, 3.0);
    assert!(c.is_running(), "Playlist lief durch");
}

#[test]
fn take_over_with_empty_deck_says_why() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut c, _m) = setup(tmp.path());
    let e = c.deck_take_over(1, true).unwrap_err();
    assert!(e.contains("Deck B ist leer"), "{e}");
}
