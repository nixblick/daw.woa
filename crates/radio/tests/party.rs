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
        assert_eq!(e, Effect::Play);
        c.play();
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

// ---- Transportknöpfe (#137) -------------------------------------------------------------

use woa_radio::control::Repeat;

/// Wie `RadioDesk::party_effect` in der App: der Effekt eines Befehls auf den Controller.
fn press(c: &mut Controller, state: &mut PartyState, cmd: PartyCommand) {
    for e in state.apply(cmd, c.is_running()) {
        match e {
            Effect::StartOrNext => c.start_or_next(),
            Effect::Stop => c.stop(),
            Effect::Pause => c.pause(),
            Effect::Play => c.play(),
            Effect::Previous => c.previous(),
            Effect::Restart => c.restart(),
            Effect::Seek(s) => c.seek(s),
            Effect::CycleRepeat => c.cycle_repeat(),
            Effect::ToggleShuffle => c.toggle_shuffle(),
            other => panic!("unerwartet: {other:?}"),
        }
    }
}

fn secs(s: f64) -> usize {
    (s * f64::from(SR)) as usize / BLOCK
}

/// `n` Titel zu je `len` Sekunden, ohne Überblendung.
fn tracks(tmp: &Path, n: usize, len: f64) -> (Controller, Mixer) {
    let paths: Vec<PathBuf> = (0..n)
        .map(|i| {
            let p = tmp.join(format!("T{i} - Titel.wav"));
            write_tone(&p, len);
            p
        })
        .collect();
    let (mut c, m) = Controller::new(show_of(&paths, 0.0), SR);
    c.measure_missing();
    (c, m)
}

#[test]
fn repeat_button_cycles_off_all_one_off() {
    let mut s = PartyState::default();
    let mut r = Repeat::Off;
    let tmp = tempfile::tempdir().unwrap();
    let (mut c, _m) = tracks(tmp.path(), 1, 1.0);
    for want in [Repeat::All, Repeat::One, Repeat::Off, Repeat::All] {
        press(&mut c, &mut s, PartyCommand::CycleRepeat);
        r = r.cycled();
        assert_eq!(r, want);
        assert_eq!(c.repeat(), want);
    }
    assert_eq!(Repeat::All.name(), "all");
    assert_eq!(Repeat::One.name(), "one");
}

#[test]
fn repeat_off_ends_the_show_after_the_last_title() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut c, mut m) = tracks(tmp.path(), 2, 1.0);
    c.start();
    step(&mut c, &mut m, secs(2.6));
    assert_eq!(c.current_index(), None);
    assert!(!c.is_running());
}

#[test]
fn repeat_one_restarts_the_same_title_at_the_end() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut c, mut m) = tracks(tmp.path(), 2, 1.0);
    let mut s = PartyState::default();
    press(&mut c, &mut s, PartyCommand::CycleRepeat);
    press(&mut c, &mut s, PartyCommand::CycleRepeat);
    assert_eq!(c.repeat(), Repeat::One);
    c.start();
    let plays0 = c.status().plays.load(std::sync::atomic::Ordering::Acquire);
    step(&mut c, &mut m, secs(3.5));
    assert_eq!(c.current_index(), Some(0), "bleibt beim ersten Titel");
    let plays = c.status().plays.load(std::sync::atomic::Ordering::Acquire);
    assert!(plays >= plays0 + 3, "{plays0} -> {plays}");
}

#[test]
fn next_still_jumps_to_the_next_title_in_repeat_one() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut c, mut m) = tracks(tmp.path(), 3, 5.0);
    let mut s = PartyState::default();
    c.set_repeat(Repeat::One);
    c.start();
    step(&mut c, &mut m, secs(0.5));
    assert_eq!(c.current_index(), Some(0));
    press(&mut c, &mut s, PartyCommand::Next);
    step(&mut c, &mut m, secs(0.5));
    assert_eq!(
        c.current_index(),
        Some(1),
        "Weiter springt trotz Wiederholen"
    );
    // Und der neue Titel wird wieder wiederholt, nicht der übernächste.
    step(&mut c, &mut m, secs(5.5));
    assert_eq!(c.current_index(), Some(1));
}

#[test]
fn repeat_all_starts_over_after_the_last_title() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut c, mut m) = tracks(tmp.path(), 2, 1.0);
    c.set_repeat(Repeat::All);
    c.start();
    let mut seen = Vec::new();
    for _ in 0..secs(4.6) {
        step(&mut c, &mut m, 1);
        let cur = c.current_index();
        if cur.is_some() && seen.last() != Some(&cur) {
            seen.push(cur);
        }
    }
    assert_eq!(
        &seen[..5],
        &[Some(0), Some(1), Some(0), Some(1), Some(0)],
        "{seen:?}"
    );
    assert!(c.is_running());
}

#[test]
fn shuffle_plays_every_title_once_in_some_order() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut c, mut m) = tracks(tmp.path(), 5, 0.5);
    let mut s = PartyState::default();
    press(&mut c, &mut s, PartyCommand::ToggleShuffle);
    assert!(c.shuffle());
    c.start();
    let mut seen = Vec::new();
    for _ in 0..secs(4.0) {
        step(&mut c, &mut m, 1);
        let cur = c.current_index();
        if cur.is_some() && seen.last() != Some(&cur) {
            seen.push(cur);
        }
    }
    let mut sorted: Vec<_> = seen.iter().flatten().copied().collect();
    sorted.sort_unstable();
    assert_eq!(sorted, vec![0, 1, 2, 3, 4], "jeder genau einmal: {seen:?}");
    assert!(!c.is_running(), "ohne Wiederholen endet die Runde");
    press(&mut c, &mut s, PartyCommand::ToggleShuffle);
    assert!(!c.shuffle());
}

#[test]
fn shuffle_with_repeat_all_keeps_going() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut c, mut m) = tracks(tmp.path(), 3, 0.5);
    c.set_shuffle(true);
    c.set_repeat(Repeat::All);
    c.start();
    step(&mut c, &mut m, secs(5.0));
    assert!(c.is_running() && c.current_index().is_some());
}

#[test]
fn previous_goes_back_and_restarts_at_the_first_title() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut c, mut m) = tracks(tmp.path(), 3, 5.0);
    let mut s = PartyState::default();
    c.set_cue(2);
    c.start();
    step(&mut c, &mut m, secs(0.5));
    assert_eq!(c.current_index(), Some(2));
    press(&mut c, &mut s, PartyCommand::Previous);
    step(&mut c, &mut m, secs(0.5));
    assert_eq!(c.current_index(), Some(1));
    press(&mut c, &mut s, PartyCommand::Previous);
    step(&mut c, &mut m, secs(0.5));
    assert_eq!(c.current_index(), Some(0));
    // Vor dem ersten Titel gibt es keinen: er beginnt neu.
    step(&mut c, &mut m, secs(2.0));
    let before = c.remaining_title();
    press(&mut c, &mut s, PartyCommand::Previous);
    step(&mut c, &mut m, secs(0.5));
    assert_eq!(c.current_index(), Some(0));
    assert!(c.remaining_title() > before + 1.0);
}

#[test]
fn restart_jumps_to_the_start_of_the_title() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut c, mut m) = tracks(tmp.path(), 1, 20.0);
    let mut s = PartyState::default();
    c.start();
    step(&mut c, &mut m, secs(8.0));
    assert!(c.remaining_title() < 12.5);
    press(&mut c, &mut s, PartyCommand::Restart);
    step(&mut c, &mut m, 2);
    assert!(c.remaining_title() > 19.0, "{}", c.remaining_title());
    assert_eq!(c.current_index(), Some(0));
}

#[test]
fn seek_buttons_jump_ten_seconds_and_stay_inside_the_title() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut c, mut m) = tracks(tmp.path(), 1, 60.0);
    let mut s = PartyState::default();
    c.start();
    step(&mut c, &mut m, secs(1.0));
    let r0 = c.remaining_title();
    press(&mut c, &mut s, PartyCommand::SeekForward);
    step(&mut c, &mut m, 1);
    let r1 = c.remaining_title();
    assert!((r0 - r1 - 10.0).abs() < 0.2, "{r0} -> {r1}");
    press(&mut c, &mut s, PartyCommand::SeekBack);
    step(&mut c, &mut m, 1);
    assert!((c.remaining_title() - r0).abs() < 0.2);
    // Zu weit zurück: Anfang des Titels. Zu weit vor: kurz vor das Ende, nicht darüber.
    c.seek(-1000.0);
    step(&mut c, &mut m, 1);
    assert!(c.remaining_title() > 59.5);
    c.seek(1000.0);
    step(&mut c, &mut m, 1);
    assert!(c.current_index().is_none() || c.remaining_title() < 0.5);
    // Ohne laufenden Titel passiert nichts.
    step(&mut c, &mut m, secs(1.0));
    c.seek(10.0);
}

#[test]
fn mute_remembers_the_old_level_and_volume_unmutes() {
    let mut s = PartyState::default();
    s.level_db = -4.0;
    assert_eq!(
        s.apply(PartyCommand::Mute, true),
        vec![Effect::MusicLevel(woa_radio::mixer::LEVEL_MIN_DB)]
    );
    assert!(s.muted());
    assert_eq!(
        s.apply(PartyCommand::Mute, true),
        vec![Effect::MusicLevel(-4.0)]
    );
    assert!(!s.muted());
    // Lauter bei Stumm: zurück auf den alten Pegel plus Schritt.
    s.apply(PartyCommand::Mute, true);
    assert_eq!(
        s.apply(PartyCommand::VolumeUp, true),
        vec![Effect::MusicLevel(-2.0)]
    );
    assert!(!s.muted());
}

#[test]
fn play_and_stop_buttons_are_idempotent() {
    let mut s = PartyState::default();
    assert_eq!(s.apply(PartyCommand::Play, false), vec![Effect::Play]);
    assert!(s.apply(PartyCommand::Play, true).is_empty());
    assert_eq!(s.apply(PartyCommand::Stop, true), vec![Effect::Stop]);
    // Auch im Stand räumt Stopp die Pausenstelle ab.
    assert_eq!(s.apply(PartyCommand::Stop, false), vec![Effect::Stop]);
    assert_eq!(s.apply(PartyCommand::PlayPause, true), vec![Effect::Pause]);
    assert_eq!(s.apply(PartyCommand::PlayPause, false), vec![Effect::Play]);
}

#[test]
fn layout_shows_the_active_modes() {
    let mut view = View::default();
    let state = PartyState::default();
    let t = layout(&view, &state).to_text();
    assert!(t.contains("Wdh. aus |") || t.contains("Wdh. aus\n"), "{t}");
    view.repeat = Repeat::One;
    view.shuffle = true;
    let l = layout(&view, &state);
    let t = l.to_text();
    assert!(t.contains("Titel*") && t.contains("Zufall*"), "{t}");
    view.repeat = Repeat::All;
    assert!(layout(&view, &state).to_text().contains("Playlist*"));
}

#[test]
fn pause_remembers_the_spot_and_play_continues_there() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut c, mut m) = tracks(tmp.path(), 2, 30.0);
    let mut s = PartyState::default();
    press(&mut c, &mut s, PartyCommand::PlayPause);
    step(&mut c, &mut m, secs(8.0));
    let before = c.remaining_title();
    assert!((before - 22.0).abs() < 0.6, "{before}");
    // Pause: blendet aus, Stelle gemerkt, Musik steht.
    press(&mut c, &mut s, PartyCommand::PlayPause);
    assert!(c.is_paused());
    step(&mut c, &mut m, secs(2.0));
    // Der Mixer hält den Titel fest (#169): er bleibt der aktuelle, läuft aber nicht.
    assert_eq!(c.current_index(), Some(0));
    assert!(!c.is_running());
    let (rest, len) = c.paused_times().unwrap();
    assert!(
        (rest - before).abs() < 0.1 && (len - 30.0).abs() < 0.1,
        "{rest} {len}"
    );
    // Das Layout zeigt „pausiert“ mit der gemerkten Restzeit, nicht den ganzen Titel.
    let mut v = view_of(&c);
    v.paused = c.paused_times();
    let l = layout(&v, &s);
    let now = l.now.unwrap();
    assert!(now.paused && !now.playing);
    assert_eq!(now.remaining, woa_radio::party::clock(rest));
    assert!(now.progress > 0.2);
    // Play: weiter an derselben Stelle (nicht von vorn), derselbe Titel.
    press(&mut c, &mut s, PartyCommand::PlayPause);
    assert!(!c.is_paused());
    step(&mut c, &mut m, 4);
    assert_eq!(c.current_index(), Some(0));
    let after = c.remaining_title();
    assert!(
        (after - before).abs() < 0.3,
        "vorher {before}, nachher {after}"
    );
    // Und es läuft weiter.
    step(&mut c, &mut m, secs(1.0));
    assert!(c.remaining_title() < after - 0.8);
}

#[test]
fn play_during_the_fade_out_just_keeps_playing_and_stop_forgets_the_spot() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut c, mut m) = tracks(tmp.path(), 1, 30.0);
    c.start();
    step(&mut c, &mut m, secs(3.0));
    c.pause();
    step(&mut c, &mut m, 2);
    assert!(
        c.is_paused() && c.current_index().is_some(),
        "Ausblendung läuft noch"
    );
    c.play();
    step(&mut c, &mut m, secs(1.0));
    assert!(c.is_running() && c.current_index() == Some(0));
    // Stopp nach Pause: Stelle vergessen, Play beginnt von vorn.
    c.pause();
    step(&mut c, &mut m, secs(1.0));
    assert!(c.is_paused());
    c.stop();
    assert!(!c.is_paused());
    c.play();
    step(&mut c, &mut m, 4);
    assert!(c.remaining_title() > 29.0, "{}", c.remaining_title());
}
