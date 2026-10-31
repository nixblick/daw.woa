//! Neuladen der Sendung (`replace_show`) und Meldung verlorener Mitschnitt-Samples, offline
//! und ohne Ton.

use std::path::Path;
use std::sync::atomic::Ordering;

use woa_radio::mixer::DECK_PLAYING;
use woa_radio::{CartEntry, Controller, Cues, Mixer, PlaylistEntry, RadioShow};

const SR: u32 = 48_000;
const BLOCK: usize = 256;

fn write_dc(path: &Path, seconds: f64) {
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: SR,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut w = hound::WavWriter::create(path, spec).unwrap();
    for _ in 0..(seconds * f64::from(SR)) as usize * 2 {
        w.write_sample(0.25f32).unwrap();
    }
    w.finalize().unwrap();
}

fn quiet_show() -> RadioShow {
    RadioShow {
        bus_processing: false,
        mic_processing: false,
        ..RadioShow::default()
    }
}

fn blocks(c: &mut Controller, m: &mut Mixer, n: usize) {
    let (mut l, mut r) = (vec![0.0f32; BLOCK], vec![0.0f32; BLOCK]);
    for _ in 0..n {
        c.pump();
        m.process(&mut l, &mut r);
    }
}

#[test]
fn duplicate_paths_keep_distinct_uids() {
    let (mut c, _m) = Controller::new(quiet_show(), SR);
    let mut show = quiet_show();
    show.playlist = vec![
        PlaylistEntry::new("/x/a.wav"),
        PlaylistEntry::new("/x/a.wav"),
        PlaylistEntry::new("/x/b.wav"),
    ];
    c.replace_show(show.clone());
    let first: Vec<u32> = c.show().playlist.iter().map(|e| e.uid).collect();
    assert_ne!(first[0], first[1], "doppelter Pfad braucht eigene uid");
    // Erneut laden: die Kennungen bleiben erhalten und eindeutig.
    c.replace_show(show);
    let again: Vec<u32> = c.show().playlist.iter().map(|e| e.uid).collect();
    assert_eq!(first, again);
}

#[test]
fn reload_keeps_unchanged_deck_running() {
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("deck.wav");
    write_dc(&file, 2.0);
    let (mut c, mut m) = Controller::new(quiet_show(), SR);
    c.deck_load(0, file.clone(), Cues::default()).unwrap();
    blocks(&mut c, &mut m, 2);
    c.deck_play(0);
    blocks(&mut c, &mut m, 50);
    let before = c.status().deck_pos[0].load(Ordering::Acquire);
    assert!(before > 0);

    // Nur Deck B und ein Cart ändern sich.
    let mut show = c.show().clone();
    show.decks[1] = Some(CartEntry::new(file.clone()));
    show.carts[0] = Some(CartEntry::new(file));
    c.replace_show(show);
    blocks(&mut c, &mut m, 10);
    assert_eq!(
        c.status().deck_state[0].load(Ordering::Acquire),
        DECK_PLAYING
    );
    assert!(c.status().deck_pos[0].load(Ordering::Acquire) > before);
}

#[test]
fn dropped_record_samples_are_reported() {
    let (mut c, _m) = Controller::new(quiet_show(), SR);
    c.pump();
    assert!(c.take_message().is_none());
    // 4800 Frames = 100 ms.
    c.status().record_dropped.fetch_add(9600, Ordering::Relaxed);
    c.pump();
    assert_eq!(
        c.take_message().as_deref(),
        Some("Mitschnitt: 100 ms verloren")
    );
    c.pump();
    assert!(c.take_message().is_none(), "nur neue Verluste melden");
}
