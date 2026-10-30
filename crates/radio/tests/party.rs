//! Party-Layout (Paket 35): die angezeigte Restzeit stimmt mit dem Offline-Render überein,
//! „Weiter“ blendet über, die Wunschliste reiht einen Titel hinter dem laufenden ein.
//! Ohne Audiogerät.

use std::path::{Path, PathBuf};

use woa_radio::party::{Effect, PartyCommand, PartyState, layout};
use woa_radio::station::{View, ViewEntry};
use woa_radio::{Controller, Mixer, PlaylistEntry, RadioShow, render};

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

fn show_of(paths: &[PathBuf], crossfade: f64) -> RadioShow {
    RadioShow {
        crossfade_seconds: crossfade,
        bus_processing: false,
        mic_processing: false,
        playlist: paths.iter().map(PlaylistEntry::new).collect(),
        ..RadioShow::default()
    }
}

fn step(c: &mut Controller, m: &mut Mixer, blocks: usize) {
    let mut l = vec![0.0f32; BLOCK];
    let mut r = vec![0.0f32; BLOCK];
    for _ in 0..blocks {
        c.pump();
        m.process(&mut l, &mut r);
        c.pump();
    }
}

fn view_of(c: &Controller) -> View {
    View {
        playlist: c
            .show()
            .playlist
            .iter()
            .map(|e| ViewEntry {
                title: e.title(),
                path: e.path.clone(),
                seconds: e.seconds,
                lufs: e.lufs,
                gain_db: 0.0,
                cues: e.cues,
                voice: None,
            })
            .collect(),
        current: c.current_index(),
        cue: c.cue_index(),
        running: c.is_running(),
        remaining_title: c.remaining_title(),
        ..View::default()
    }
}

#[test]
fn remaining_time_matches_the_offline_render() {
    let tmp = tempfile::tempdir().unwrap();
    let a = tmp.path().join("Band - Eins.wav");
    write_tone(&a, 20.0);
    let show = show_of(&[a], 0.0);
    // Referenz: Länge des Offline-Renders.
    let total = render(&show, SR).unwrap().left.len() as f64 / f64::from(SR);
    assert!((total - 20.0).abs() < 0.05, "render length {total}");

    let (mut c, mut m) = Controller::new(show, SR);
    c.measure_missing();
    c.start();
    let state = PartyState::default();
    let mut played = 0usize;
    for secs in [1.0, 5.0, 12.5] {
        let blocks = (secs * f64::from(SR)) as usize / BLOCK;
        step(&mut c, &mut m, blocks - played);
        played = blocks;
        let t = played as f64 * BLOCK as f64 / f64::from(SR);
        let l = layout(&view_of(&c), &state);
        let now = l.now.unwrap();
        // Die Anzeige darf nur um die Pufferung des Controllers (wenige Blöcke) abweichen.
        let shown = now
            .remaining
            .split(':')
            .fold(0.0, |s, p| s * 60.0 + p.parse::<f64>().unwrap());
        let expected = total - t;
        assert!(
            (shown - expected).abs() <= 1.5,
            "t={t}: shown {shown}, expected {expected}"
        );
        assert!(now.playing && now.title == "Eins" && now.artist == "Band");
        assert!((f64::from(now.progress) - t / total).abs() < 0.08);
    }
}

#[test]
fn next_button_crossfades_and_wish_is_played_after_the_current_title() {
    let tmp = tempfile::tempdir().unwrap();
    let mk = |n: &str, s: f64| {
        let p = tmp.path().join(format!("{n}.wav"));
        write_tone(&p, s);
        p
    };
    let (a, b, wish) = (
        mk("A - eins", 12.0),
        mk("B - zwei", 12.0),
        mk("W - Wunsch", 12.0),
    );
    let (mut c, mut m) = Controller::new(show_of(&[a, b], 1.0), SR);
    c.measure_missing();
    let mut state = PartyState::default();

    // Play: Taste -> Effekt -> Controller.
    for e in state.apply(PartyCommand::PlayPause, c.is_running()) {
        assert_eq!(e, Effect::StartOrNext);
        c.start_or_next();
    }
    step(&mut c, &mut m, 20);
    assert_eq!(c.current_index(), Some(0));

    // Wunsch: zweimal (eine Stimme mehr), dann einreihen hinter dem laufenden Titel.
    for _ in 0..2 {
        state.apply(
            PartyCommand::Wish {
                path: wish.clone(),
                title: "W - Wunsch".into(),
            },
            true,
        );
    }
    assert_eq!(state.wishes.len(), 1);
    assert_eq!(state.wishes.items()[0].votes, 2);
    for e in state.apply(PartyCommand::PlayWish(wish.clone()), true) {
        let Effect::InsertNext(p) = e else {
            panic!("{e:?}")
        };
        let at = c.current_index().map_or(c.cue_index(), |i| i + 1);
        c.add(&[p], Some(at));
    }
    assert_eq!(c.show().playlist[1].title(), "W - Wunsch");

    // Weiter: Crossfade (1 s) zum Wunsch.
    for e in state.apply(PartyCommand::Next, c.is_running()) {
        assert_eq!(e, Effect::StartOrNext);
        c.start_or_next();
    }
    step(&mut c, &mut m, (1.5 * f64::from(SR)) as usize / BLOCK);
    assert_eq!(c.current_index(), Some(1));
    let l = layout(&view_of(&c), &state);
    assert_eq!(l.now.as_ref().unwrap().title, "Wunsch");
    assert_eq!(l.next.as_deref(), Some("B – zwei"));
    assert_eq!(
        state.log(),
        [
            "play_pause",
            "wish W - Wunsch",
            "wish W - Wunsch",
            "play_wish W - Wunsch",
            "next"
        ]
    );
}
