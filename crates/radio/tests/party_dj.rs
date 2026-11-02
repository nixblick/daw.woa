//! DJ-Ansicht der Party (DJ-Paket 0.36.0): Kommandos ohne Ton geprüft, jeder Mixer-Block unter
//! `assert_no_alloc`. Kein Ton geht hinaus, die Ausgabe landet nur in Puffern.

use std::path::Path;
use std::sync::atomic::Ordering::Acquire;

use assert_no_alloc::{AllocDisabler, assert_no_alloc};
use woa_radio::control::{Band, DjCommand, DjTrackInfo};
use woa_radio::mixer::DECK_PLAYING;
use woa_radio::{Controller, Mixer, PlaylistEntry, RadioShow};

#[global_allocator]
static ALLOC: AllocDisabler = AllocDisabler;

const SR: u32 = 48_000;
const BLOCK: usize = 240;

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

fn setup(dir: &Path) -> (Controller, Mixer) {
    let playlist = (0..3)
        .map(|k| {
            let p = dir.join(format!("t{k}.wav"));
            write_dc(&p, 6.0, 0.3);
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

fn pos(c: &Controller, side: usize) -> f64 {
    c.dj_view().unwrap().decks[side].position
}

fn info(c: &Controller, side: usize, bpm: f64, key: &str) -> DjTrackInfo {
    DjTrackInfo {
        index: c.dj_view().unwrap().decks[side].index.unwrap(),
        bpm: Some(bpm),
        beat_offset: Some(0.0),
        key: Some(key.into()),
        hash: Some(format!("hash{side}")),
        ..Default::default()
    }
}

fn dj_party(dir: &Path) -> (Controller, Mixer) {
    let (mut c, mut m) = setup(dir);
    c.party_enter();
    run(&mut c, &mut m, 0.02);
    c.dj(DjCommand::View { dj: true }).unwrap();
    run(&mut c, &mut m, 0.02);
    (c, m)
}

#[test]
fn dj_view_turns_auto_and_preload_off_and_loads_by_hand() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut c, mut m) = dj_party(tmp.path());
    assert!(c.dj_active());
    assert!(c.party_auto(), "die Einstellung der Sendung bleibt");
    // In B laden: Titel 2 statt des vorgeladenen 1.
    c.dj(DjCommand::Load { side: 1, index: 2 }).unwrap();
    c.dj(DjCommand::Play { side: 0 }).unwrap();
    run(&mut c, &mut m, 6.3);
    assert!(!playing(&c, 1), "kein Auto-Übergang in der DJ-Ansicht");
    assert_eq!(
        c.dj_view().unwrap().decks[1].index,
        Some(2),
        "die Hand-Ladung bleibt liegen"
    );
    assert!(c.dj(DjCommand::Load { side: 1, index: 99 }).is_err());
    // Zurück nach Einfach: die Automatik gilt wieder.
    c.dj(DjCommand::View { dj: false }).unwrap();
    assert!(!c.dj_active());
}

#[test]
fn tempo_fader_and_keylock_change_the_speed() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut c, mut m) = dj_party(tmp.path());
    c.dj(DjCommand::Tempo {
        side: 0,
        value: 1.0,
    })
    .unwrap();
    c.dj(DjCommand::Play { side: 0 }).unwrap();
    run(&mut c, &mut m, 1.0);
    let p = pos(&c, 0);
    assert!((p - 1.08).abs() < 0.02, "+8 %: {p}");
    assert!((c.dj_view().unwrap().decks[0].tempo_percent - 8.0).abs() < 1e-9);
    c.dj(DjCommand::Keylock { side: 0, on: true }).unwrap();
    c.dj(DjCommand::Tempo {
        side: 0,
        value: -1.0,
    })
    .unwrap();
    let v = run(&mut c, &mut m, 1.0);
    let p2 = pos(&c, 0);
    assert!((p2 - p - 0.92).abs() < 0.03, "−8 % mit Keylock: {}", p2 - p);
    assert!(
        v.iter().skip(20).all(|x| x.abs() > 0.1),
        "Keylock bleibt hörbar"
    );
    // Anschieben nur, solange gehalten.
    c.dj(DjCommand::Nudge {
        side: 0,
        value: 1.0,
    })
    .unwrap();
    assert!((c.dj_view().unwrap().decks[0].tempo_percent - (-4.0)).abs() < 1e-9);
    c.dj(DjCommand::Nudge {
        side: 0,
        value: 0.0,
    })
    .unwrap();
    assert!((c.dj_view().unwrap().decks[0].tempo_percent - (-8.0)).abs() < 1e-9);
}

#[test]
fn cue_sets_when_standing_and_holding_previews() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut c, mut m) = dj_party(tmp.path());
    c.dj(DjCommand::Seek {
        side: 0,
        seconds: 2.0,
    })
    .unwrap();
    run(&mut c, &mut m, 0.05);
    c.dj(DjCommand::CueDown { side: 0 }).unwrap();
    run(&mut c, &mut m, 0.5);
    assert!(playing(&c, 0), "halten spielt ab Cue");
    assert!((c.dj_view().unwrap().decks[0].cue_point.unwrap() - 2.0).abs() < 0.01);
    c.dj(DjCommand::CueUp { side: 0 }).unwrap();
    run(&mut c, &mut m, 0.1);
    assert!(!playing(&c, 0), "loslassen hält an");
    assert!((pos(&c, 0) - 2.0).abs() < 0.01, "und springt zurück");
    // Läuft das Deck, springt Cue zurück und hält an.
    c.dj(DjCommand::Play { side: 0 }).unwrap();
    run(&mut c, &mut m, 0.5);
    c.dj(DjCommand::CueDown { side: 0 }).unwrap();
    c.dj(DjCommand::CueUp { side: 0 }).unwrap();
    run(&mut c, &mut m, 0.1);
    assert!(!playing(&c, 0));
    assert!((pos(&c, 0) - 2.0).abs() < 0.01);
}

#[test]
fn hot_cues_and_auto_loop_are_marks_for_the_library() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut c, mut m) = dj_party(tmp.path());
    let i = info(&c, 0, 120.0, "a-Moll");
    c.dj_track_info(0, i);
    c.dj(DjCommand::Seek {
        side: 0,
        seconds: 1.0,
    })
    .unwrap();
    run(&mut c, &mut m, 0.05);
    c.dj(DjCommand::HotCue {
        side: 0,
        slot: 1,
        clear: false,
    })
    .unwrap();
    let marks = c.dj_take_marks();
    assert_eq!(marks.len(), 1);
    assert_eq!(marks[0].hash, "hash0");
    assert!((marks[0].hot[1].unwrap() - 1.0).abs() < 0.01);
    // Anspringen.
    c.dj(DjCommand::Seek {
        side: 0,
        seconds: 4.0,
    })
    .unwrap();
    run(&mut c, &mut m, 0.05);
    c.dj(DjCommand::HotCue {
        side: 0,
        slot: 1,
        clear: false,
    })
    .unwrap();
    run(&mut c, &mut m, 0.05);
    assert!((pos(&c, 0) - 1.0).abs() < 0.01);
    assert!(c.dj_take_marks().is_empty(), "anspringen ändert nichts");
    // Auto-Loop über 2 Schläge bei 120 BPM = 1 s ab dem Schlag.
    c.dj(DjCommand::Loop {
        side: 0,
        beats: Some(2),
    })
    .unwrap();
    c.dj(DjCommand::Play { side: 0 }).unwrap();
    run(&mut c, &mut m, 2.5);
    let p = pos(&c, 0);
    assert!((1.0..2.0).contains(&p), "bleibt in der Schleife: {p}");
    let d = &c.dj_view().unwrap().decks[0];
    assert_eq!(d.looped, Some((1.0, 2.0)));
    assert_eq!(d.loop_beats, Some(2));
    assert_eq!(c.dj_take_marks()[0].looped, Some((1.0, 2.0)));
    c.dj(DjCommand::Loop {
        side: 0,
        beats: None,
    })
    .unwrap();
    run(&mut c, &mut m, 1.2);
    assert!(pos(&c, 0) > 2.0, "ohne Schleife läuft es weiter");
    assert!(
        c.dj(DjCommand::Loop {
            side: 0,
            beats: Some(3)
        })
        .is_err()
    );
}

#[test]
fn sync_matches_tempo_and_beat_or_refuses_far_tempos() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut c, mut m) = dj_party(tmp.path());
    let a = info(&c, 0, 120.0, "a-Moll");
    let b = info(&c, 1, 125.0, "C-Dur");
    c.dj_track_info(0, a);
    c.dj_track_info(1, b);
    let v = c.dj_view().unwrap();
    assert_eq!(v.decks[0].camelot.as_deref(), Some("8A"));
    assert_eq!(v.decks[1].camelot.as_deref(), Some("8B"));
    assert!(
        v.decks[0].key_match && v.decks[1].key_match,
        "8A und 8B passen"
    );
    c.dj(DjCommand::Play { side: 0 }).unwrap();
    c.dj(DjCommand::Seek {
        side: 1,
        seconds: 0.13,
    })
    .unwrap();
    run(&mut c, &mut m, 0.3);
    c.dj(DjCommand::Sync { side: 1, on: true }).unwrap();
    c.dj(DjCommand::Play { side: 1 }).unwrap();
    run(&mut c, &mut m, 0.5);
    let v = c.dj_view().unwrap();
    assert!(v.decks[1].sync);
    assert!((v.decks[1].bpm_now.unwrap() - 120.0).abs() < 1e-6);
    let (pa, pb) = (v.decks[0].phase.unwrap(), v.decks[1].phase.unwrap());
    let d = (pa - pb + 0.5).rem_euclid(1.0) - 0.5;
    assert!(d.abs() < 0.03, "im Takt: {pa} gegen {pb}");
    // Sync folgt dem Fader des anderen Decks.
    c.dj(DjCommand::Tempo {
        side: 0,
        value: 0.5,
    })
    .unwrap();
    run(&mut c, &mut m, 0.05);
    let v = c.dj_view().unwrap();
    assert!((v.decks[1].bpm_now.unwrap() - 124.8).abs() < 1e-6);
    // Eigener Fader schaltet Sync aus.
    c.dj(DjCommand::Tempo {
        side: 1,
        value: 0.0,
    })
    .unwrap();
    assert!(!c.dj_view().unwrap().decks[1].sync);
    // 100 gegen 125 BPM: zu weit, auch doppelt oder halb.
    let far = DjTrackInfo {
        bpm: Some(100.0),
        ..info(&c, 1, 100.0, "C-Dur")
    };
    c.dj_track_info(1, far);
    c.dj(DjCommand::Tempo {
        side: 0,
        value: 0.0,
    })
    .unwrap();
    let e = c.dj(DjCommand::Sync { side: 1, on: true }).unwrap_err();
    assert!(e.contains("±8 %"), "{e}");
    assert!(!c.dj_view().unwrap().decks[1].sync);
}

#[test]
fn mixer_eq_kill_fader_and_crossfader_curve() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut c, mut m) = dj_party(tmp.path());
    c.dj(DjCommand::Play { side: 0 }).unwrap();
    let v = run(&mut c, &mut m, 0.3);
    let base = *v.last().unwrap();
    assert!(base > 0.05, "spielt: {base}");
    // Gleichspannung liegt im Tiefen-Band: Kill Lo macht still.
    c.dj(DjCommand::Eq {
        side: 0,
        band: Band::Lo,
        value: -1.0,
    })
    .unwrap();
    let v = run(&mut c, &mut m, 0.5);
    assert!(
        v.last().unwrap().abs() < base * 0.03,
        "Kill: {}",
        v.last().unwrap()
    );
    c.dj(DjCommand::Eq {
        side: 0,
        band: Band::Lo,
        value: 0.0,
    })
    .unwrap();
    run(&mut c, &mut m, 0.3);
    c.dj(DjCommand::Fader {
        side: 0,
        value: 0.0,
    })
    .unwrap();
    let v = run(&mut c, &mut m, 0.2);
    assert!(v.last().unwrap().abs() < 1e-6, "Kanalfader zu");
    c.dj(DjCommand::Fader {
        side: 0,
        value: 1.0,
    })
    .unwrap();
    // Harte Kurve: in der Mitte beide voll.
    c.dj(DjCommand::Curve { hard: true }).unwrap();
    c.dj(DjCommand::Crossfader { value: 0.5 }).unwrap();
    let v = run(&mut c, &mut m, 0.3);
    assert!(
        (v.last().unwrap() - base).abs() < 1e-3,
        "hart: A voll in der Mitte"
    );
    c.dj(DjCommand::Curve { hard: false }).unwrap();
    let v = run(&mut c, &mut m, 0.1);
    let want = base * std::f32::consts::FRAC_1_SQRT_2;
    assert!((v.last().unwrap() - want).abs() < 1e-3, "weich: −3 dB");
    c.dj(DjCommand::Master { value: 0.5 }).unwrap();
    let v = run(&mut c, &mut m, 0.2);
    assert!((v.last().unwrap() - want * 0.5).abs() < 1e-3, "Master");
    let lv = c.dj_take_levels();
    assert!(
        lv[0] > 0.1 && lv[2] > 0.05,
        "Pegel für die LED-Kette: {lv:?}"
    );
}

#[test]
fn cue_needs_a_headphone_output() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut c, _m) = dj_party(tmp.path());
    assert!(!c.dj_view().unwrap().cue_available);
    let e = c.dj(DjCommand::Cue { side: 0, on: true }).unwrap_err();
    assert!(e.contains("Vorhör-Ausgang"), "{e}");
}

#[test]
fn headphone_cue_reaches_only_the_headphones() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut c, mut m) = dj_party(tmp.path());
    let (h, mut player) = woa_radio::pfl::channel_with_bus(SR, SR);
    c.set_pfl(Some(h));
    c.dj(DjCommand::Cue { side: 0, on: true }).unwrap();
    c.dj(DjCommand::Fader {
        side: 0,
        value: 0.0,
    })
    .unwrap();
    c.dj(DjCommand::Phones {
        mix: 0.0,
        level: 1.0,
    })
    .unwrap();
    c.dj(DjCommand::Play { side: 0 }).unwrap();
    let (mut l, mut r) = (vec![0.0f32; BLOCK], vec![0.0f32; BLOCK]);
    let (mut hl, mut hr) = (vec![0.0f32; BLOCK], vec![0.0f32; BLOCK]);
    let (mut main_peak, mut phones_peak) = (0.0f32, 0.0f32);
    for k in 0..400 {
        c.pump();
        assert_no_alloc(|| m.process(&mut l, &mut r));
        assert_no_alloc(|| player.process(&mut hl, &mut hr));
        if k > 50 {
            main_peak = l.iter().chain(&r).fold(main_peak, |a, x| a.max(x.abs()));
            phones_peak = hl.iter().fold(phones_peak, |a, x| a.max(x.abs()));
        }
    }
    assert!(main_peak < 1e-6, "Sendung still (Fader zu): {main_peak}");
    assert!(phones_peak > 0.2, "Kopfhörer hört den Cue: {phones_peak}");
    assert!(c.dj_view().unwrap().decks[0].cue);
}

#[test]
fn commands_need_the_party() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut c, _m) = setup(tmp.path());
    assert!(c.dj(DjCommand::View { dj: true }).is_err());
    assert!(c.dj_view().is_none());
}
