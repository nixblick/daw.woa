//! Decks A, B, C offline und ohne Ton: Restzeit, Start und Stopp, Schnitt und eigener Ausgang.
//! Jeder Block läuft unter `assert_no_alloc`.

use std::path::Path;

use assert_no_alloc::{AllocDisabler, assert_no_alloc};
use woa_engine::pairs::PairBuf;
use woa_radio::mixer::{DECK_EMPTY, DECK_PLAYING, DECK_READY};
use woa_radio::routing::{Router, Routes};
use woa_radio::{Controller, Cues, Mixer, RadioShow};

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

fn setup(dir: &Path) -> (Controller, Mixer, std::path::PathBuf) {
    let file = dir.join("deck.wav");
    write_dc(&file, 2.0, 0.25);
    let show = RadioShow {
        bus_processing: false,
        mic_processing: false,
        ..RadioShow::default()
    };
    let (c, m) = Controller::new(show, SR);
    (c, m, file)
}

fn blocks(c: &mut Controller, m: &mut Mixer, n: usize, out: &mut Vec<f32>) {
    let (mut l, mut r) = (vec![0.0f32; BLOCK], vec![0.0f32; BLOCK]);
    for _ in 0..n {
        c.pump();
        assert_no_alloc(|| m.process(&mut l, &mut r));
        out.extend_from_slice(&l);
    }
}

fn state(c: &Controller, k: usize) -> u32 {
    use std::sync::atomic::Ordering::Acquire;
    c.status().deck_state[k].load(Acquire)
}

fn pos(c: &Controller, k: usize) -> u64 {
    c.status().deck_pos[k].load(std::sync::atomic::Ordering::Acquire)
}

#[test]
fn deck_starts_stops_and_counts_down() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut c, mut m, file) = setup(tmp.path());
    let mut out = Vec::new();
    blocks(&mut c, &mut m, 2, &mut out);
    assert_eq!(state(&c, 0), DECK_EMPTY);
    c.deck_load(0, file, Cues::default()).unwrap();
    blocks(&mut c, &mut m, 2, &mut out);
    assert_eq!(state(&c, 0), DECK_READY);
    // Ein stehendes Deck ist stumm und läuft nicht.
    let before = out.len();
    assert!(out[before - 2 * BLOCK..].iter().all(|v| *v == 0.0));
    assert_eq!(pos(&c, 0), 0);

    c.deck_play(0);
    blocks(&mut c, &mut m, 100, &mut out);
    assert_eq!(state(&c, 0), DECK_PLAYING);
    let played = pos(&c, 0);
    assert_eq!(played, 100 * BLOCK as u64);
    assert!((out[out.len() - 1] - 0.25).abs() < 1e-5);

    // Stopp: kurz ausgeblendet, danach stumm, die Stelle bleibt.
    c.deck_stop(0);
    blocks(&mut c, &mut m, 20, &mut out);
    assert_eq!(state(&c, 0), DECK_READY);
    let held = pos(&c, 0);
    assert!(held > played && held < played + 20 * BLOCK as u64);
    assert!(out[out.len() - BLOCK..].iter().all(|v| *v == 0.0));
    blocks(&mut c, &mut m, 10, &mut out);
    assert_eq!(pos(&c, 0), held);

    // Weiter ab der Stelle bis zum Ende (2 s), danach steht es wieder.
    c.deck_play(0);
    blocks(&mut c, &mut m, 400, &mut out);
    assert_eq!(state(&c, 0), DECK_READY);
    assert_eq!(pos(&c, 0), 2 * u64::from(SR));
    // Cue: zurück an den Anfang.
    c.deck_cue(0);
    blocks(&mut c, &mut m, 2, &mut out);
    assert_eq!(pos(&c, 0), 0);
}

#[test]
fn deck_plays_only_between_cue_in_and_cue_out() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut c, mut m, file) = setup(tmp.path());
    let cues = Cues {
        cue_in: Some(0.5),
        cue_out: Some(1.25),
        ..Cues::default()
    };
    c.deck_load(1, file, cues).unwrap();
    let mut out = Vec::new();
    blocks(&mut c, &mut m, 2, &mut out);
    assert_eq!(pos(&c, 1), SR as u64 / 2);
    out.clear();
    c.deck_play(1);
    blocks(&mut c, &mut m, 300, &mut out);
    let loud = out.iter().filter(|v| (**v - 0.25).abs() < 1e-5).count();
    assert_eq!(loud, (0.75 * f64::from(SR)) as usize);
    assert_eq!(state(&c, 1), DECK_READY);
    // Schnitt ändern, solange es steht: springt an den neuen Anfang.
    c.set_deck_cues(
        1,
        Cues {
            cue_in: Some(1.0),
            cue_out: Some(1.5),
            ..Cues::default()
        },
    )
    .unwrap();
    blocks(&mut c, &mut m, 2, &mut out);
    assert_eq!(pos(&c, 1), u64::from(SR));
}

#[test]
fn deck_on_its_own_pair_is_absent_from_the_main_pair() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut c, m, file) = setup(tmp.path());
    // Deck B auf Ziel 1 (anderes Kanalpaar), alles andere im Hauptausgang.
    c.set_routes(Routes {
        dest: [0, 0, 0, 1, 0, 0],
    });
    let mut router = Router::new(m, 2, vec![(0, 0), (1, 1)], vec![]);
    c.deck_load(1, file, Cues::default()).unwrap();
    let mut pairs = vec![PairBuf::new(4096), PairBuf::new(4096)];
    let (mut main, mut deck) = (Vec::new(), Vec::new());
    c.deck_play(1);
    for _ in 0..50 {
        c.pump();
        assert_no_alloc(|| router.render(&mut pairs, BLOCK));
        main.extend_from_slice(&pairs[0].l[..BLOCK]);
        deck.extend_from_slice(&pairs[1].l[..BLOCK]);
    }
    assert!(main.iter().all(|v| v.abs() < 1e-6), "Deck im Hauptausgang");
    assert!((deck[deck.len() - 1] - 0.25).abs() < 1e-5);
}

#[test]
fn unrouted_deck_is_part_of_the_main_output_and_three_decks_add_up() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut c, mut m, file) = setup(tmp.path());
    for k in 0..3 {
        c.deck_load(k, file.clone(), Cues::default()).unwrap();
    }
    let mut out = Vec::new();
    c.deck_play(0);
    c.deck_play(2);
    blocks(&mut c, &mut m, 20, &mut out);
    assert!((out[out.len() - 1] - 0.5).abs() < 1e-5);
    c.deck_play(1);
    blocks(&mut c, &mut m, 20, &mut out);
    assert!((out[out.len() - 1] - 0.75).abs() < 1e-5);
    c.deck_eject(2);
    blocks(&mut c, &mut m, 2, &mut out);
    assert_eq!(state(&c, 2), DECK_EMPTY);
    assert!((out[out.len() - 1] - 0.5).abs() < 1e-5);
}

#[test]
fn decks_survive_a_saved_show() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut c, _m, file) = setup(tmp.path());
    c.deck_load(2, file, Cues::default()).unwrap();
    let text = serde_json::to_string(c.show()).unwrap();
    let back: RadioShow = serde_json::from_str(&text).unwrap();
    assert_eq!(back.decks.len(), 3);
    assert!(back.decks[2].is_some() && back.decks[0].is_none());
    // Eine leere Sendung schreibt nichts Neues, alte Dateien laden.
    let empty = serde_json::to_string(&RadioShow::default()).unwrap();
    assert!(!empty.contains("decks"));
    let old: RadioShow = serde_json::from_str(&empty).unwrap();
    assert_eq!(old.decks.len(), 0);
}
