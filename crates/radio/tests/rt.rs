//! Sendemixer: jeder Process-Aufruf unter `assert_no_alloc`, dazu Crossfade, Ducking,
//! Carts, Mikro, Stopp, Mitschnitt und Offline-Render (Länge, Lautheit).

use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

use assert_no_alloc::{AllocDisabler, assert_no_alloc};
use woa_engine::loudness::integrated_lufs;
use woa_radio::mixer::NONE;
use woa_radio::{Controller, Mixer, PlaylistEntry, RadioShow, render};

#[global_allocator]
static ALLOC: AllocDisabler = AllocDisabler;

const SR: u32 = 48_000;
const BLOCK: usize = 256;

/// Schreibt `seconds` Sinus (oder Gleichwert bei `freq == 0`) mit Amplitude `amp`.
fn write_tone(path: &Path, seconds: f64, freq: f32, amp: f32) {
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: SR,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut w = hound::WavWriter::create(path, spec).unwrap();
    let n = (seconds * f64::from(SR)) as usize;
    for i in 0..n {
        let v = if freq == 0.0 {
            amp
        } else {
            (i as f32 * freq * std::f32::consts::TAU / SR as f32).sin() * amp
        };
        w.write_sample(v).unwrap();
        w.write_sample(v).unwrap();
    }
    w.finalize().unwrap();
}

fn show_of(paths: &[PathBuf], crossfade: f64) -> RadioShow {
    RadioShow {
        crossfade_seconds: crossfade,
        // Diese Tests prüfen die Mechanik des Mixers; das Processing hat eigene Tests.
        bus_processing: false,
        mic_processing: false,
        playlist: paths.iter().map(PlaylistEntry::new).collect(),
        ..RadioShow::default()
    }
}

/// Ein Block unter `assert_no_alloc`, vorher und nachher ein Control-Takt.
fn block(c: &mut Controller, m: &mut Mixer, l: &mut [f32], r: &mut [f32]) {
    c.pump();
    assert_no_alloc(|| m.process(l, r));
    c.pump();
}

#[test]
fn crossfade_is_constant_power_and_handover_is_alloc_free() {
    let tmp = tempfile::tempdir().unwrap();
    let a = tmp.path().join("a.wav");
    let b = tmp.path().join("b.wav");
    // DC 0.5 on both: auto-gain off (no measurement possible for DC? it is measured),
    // so compare shapes relative to the steady level.
    write_tone(&a, 1.0, 0.0, 0.5);
    write_tone(&b, 1.0, 0.0, 0.5);
    let mut show = show_of(&[a, b], 0.25);
    show.target_lufs = -16.0;
    let (mut c, mut m) = Controller::new(show, SR);
    c.start();
    let mut l = vec![0.0f32; BLOCK];
    let mut r = vec![0.0f32; BLOCK];
    let mut out = Vec::new();
    for _ in 0..(2 * SR as usize / BLOCK) {
        block(&mut c, &mut m, &mut l, &mut r);
        out.extend_from_slice(&l);
    }
    let xf = SR as usize / 4;
    let steady = out[1000];
    assert!(steady > 0.0);
    // Crossfade starts at 1 s − 0.25 s; in the middle both are at −3 dB: sum = √2 · steady.
    let start = SR as usize - xf;
    let mid = out[start + xf / 2];
    assert!(
        (mid / steady - std::f32::consts::SQRT_2).abs() < 0.01,
        "{mid} vs {steady}"
    );
    // Before and after the fade: steady level, no gap.
    assert!((out[start - 1] / steady - 1.0).abs() < 1e-3);
    assert!((out[start + xf + 10] / steady - 1.0).abs() < 1e-3);
    // Ends at 2 s − 0.25 s.
    let end = 2 * SR as usize - xf;
    assert!(out[end - 1] > 0.0 && out[end + 1] == 0.0);
    assert_eq!(c.status().music_end.load(Ordering::Acquire), end as u64);
    assert!(c.finished());
}

#[test]
fn ducking_cart_mic_stop_and_recording() {
    let tmp = tempfile::tempdir().unwrap();
    let a = tmp.path().join("musik.wav");
    let jingle = tmp.path().join("jingle.wav");
    write_tone(&a, 3.0, 0.0, 0.2);
    write_tone(&jingle, 0.1, 0.0, 0.1);
    let mut show = show_of(&[a], 3.0);
    show.duck_db = -12.0;
    let (mut c, mut m) = Controller::new(show, SR);
    c.assign_cart(4, Some(jingle)).unwrap();
    let (mut mic_tx, mic_rx) = rtrb::RingBuffer::new(SR as usize);
    c.set_mic_input(Some(mic_rx));
    c.start_recording(tmp.path());
    c.start();

    let mut l = vec![0.0f32; BLOCK];
    let mut r = vec![0.0f32; BLOCK];
    for _ in 0..20 {
        block(&mut c, &mut m, &mut l, &mut r);
    }
    let music = l[BLOCK - 1];
    assert!(music > 0.0);

    // Cart overlays the music.
    c.trigger_cart(4);
    block(&mut c, &mut m, &mut l, &mut r);
    let with_cart = l[BLOCK - 1];
    assert!(with_cart > music * 1.1, "{with_cart} vs {music}");
    assert_ne!(c.status().carts.load(Ordering::Acquire) & (1 << 4), 0);

    // Mic open: music glides down by 12 dB, no step larger than a small fraction.
    c.set_mic_open(true);
    let mut prev = l[BLOCK - 1];
    let mut max_step = 0.0f32;
    let mut last = 0.0;
    for _ in 0..(SR as usize / BLOCK) {
        for _ in 0..BLOCK {
            let _ = mic_tx.push(0.0);
            let _ = mic_tx.push(0.0);
        }
        block(&mut c, &mut m, &mut l, &mut r);
        for &v in l.iter() {
            max_step = max_step.max((v - prev).abs());
            prev = v;
        }
        last = l[BLOCK - 1];
    }
    // The cart (0.1 s) is over by now; only ducked music remains.
    assert_eq!(c.status().carts.load(Ordering::Acquire) & (1 << 4), 0);
    let ratio = 20.0 * (last / music).log10();
    assert!((ratio + 12.0).abs() < 0.2, "ducked by {ratio} dB");
    assert!(max_step < 0.12, "smooth: {max_step}");
    assert!(c.status().mic_open.load(Ordering::Acquire));

    // Mic closed: back up.
    c.set_mic_open(false);
    for _ in 0..(SR as usize / BLOCK) {
        block(&mut c, &mut m, &mut l, &mut r);
    }
    assert!((l[BLOCK - 1] / music - 1.0).abs() < 0.01);

    // Next without a following title fades out; Stop is quiet and alloc-free.
    c.stop();
    for _ in 0..10 {
        block(&mut c, &mut m, &mut l, &mut r);
    }
    assert!(l.iter().all(|v| *v == 0.0));
    assert_eq!(c.status().current.load(Ordering::Acquire), NONE);

    let files = c.stop_recording();
    assert_eq!(files.len(), 1);
    let name = files[0].file_name().unwrap().to_string_lossy().into_owned();
    assert!(
        name.starts_with("sendung-20") && name.ends_with(".wav"),
        "{name}"
    );
    assert!(files[0].starts_with(tmp.path().join("mitschnitte")));
    let reader = hound::WavReader::open(&files[0]).unwrap();
    assert_eq!(reader.spec().channels, 2);
    assert_eq!(reader.spec().bits_per_sample, 16);
    assert!(reader.duration() as usize > 2 * SR as usize);
}

#[test]
fn next_crossfades_early() {
    let tmp = tempfile::tempdir().unwrap();
    let a = tmp.path().join("a.wav");
    let b = tmp.path().join("b.wav");
    write_tone(&a, 10.0, 0.0, 0.3);
    write_tone(&b, 2.0, 0.0, 0.3);
    let (mut c, mut m) = Controller::new(show_of(&[a, b], 0.5), SR);
    c.start();
    let mut l = vec![0.0f32; BLOCK];
    let mut r = vec![0.0f32; BLOCK];
    for _ in 0..10 {
        block(&mut c, &mut m, &mut l, &mut r);
    }
    assert_eq!(c.current_index(), Some(0));
    c.start_or_next();
    for _ in 0..(SR as usize / BLOCK) {
        block(&mut c, &mut m, &mut l, &mut r);
    }
    assert_eq!(c.current_index(), Some(1), "second title after 0.5 s fade");
    assert!(c.remaining_title() < 2.0 && c.remaining_title() > 0.5);
}

#[test]
fn render_length_and_loudness() {
    let tmp = tempfile::tempdir().unwrap();
    let mut paths = Vec::new();
    for (i, (secs, freq, amp)) in [(8.0, 440.0, 0.05), (6.0, 660.0, 0.4), (7.0, 550.0, 0.15)]
        .into_iter()
        .enumerate()
    {
        let p = tmp.path().join(format!("t{i}.wav"));
        write_tone(&p, secs, freq, amp);
        paths.push(p);
    }
    let mut show = show_of(&paths, 3.0);
    show.bus_processing = true;
    let out = render(&show, SR).unwrap();
    let expect = (8 + 6 + 7 - 3 - 3) * SR as usize;
    assert_eq!(out.left.len(), expect);
    let lufs = integrated_lufs(&out.left, &out.right, SR).unwrap();
    assert!((lufs + 16.0).abs() <= 1.0, "{lufs}");
    let peak = out.left.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    assert!(peak <= 0.8913, "ceiling -1 dBFS: {peak}");
}

#[test]
fn bus_processing_limits_and_levels_without_alloc() {
    let tmp = tempfile::tempdir().unwrap();
    let a = tmp.path().join("laut.wav");
    let jingle = tmp.path().join("jingle.wav");
    // Lauter Titel und ein Cart darüber: die rohe Summe liegt über 0 dBFS.
    write_tone(&a, 12.0, 440.0, 0.9);
    write_tone(&jingle, 2.0, 660.0, 0.9);
    let mut show = show_of(&[a], 0.0);
    show.bus_processing = true;
    show.mic_processing = true;
    let (mut c, mut m) = Controller::new(show, SR);
    c.assign_cart(0, Some(jingle)).unwrap();
    let (mut mic_tx, mic_rx) = rtrb::RingBuffer::new(SR as usize);
    c.set_mic_input(Some(mic_rx));
    let (producer, mut consumer) = rtrb::RingBuffer::new(SR as usize * 2);
    c.set_stream_output(Some(producer));
    c.start();
    c.trigger_cart(0);
    c.set_mic_open(true);
    let mut l = vec![0.0f32; BLOCK];
    let mut r = vec![0.0f32; BLOCK];
    let mut sent = Vec::new();
    let mut peak = 0.0f32;
    for b in 0..(10 * SR as usize / BLOCK) {
        for i in 0..BLOCK {
            let t = (b * BLOCK + i) as f32;
            let v = (t * 1000.0 * std::f32::consts::TAU / SR as f32).sin() * 0.5;
            let _ = mic_tx.push(v);
            let _ = mic_tx.push(v);
        }
        block(&mut c, &mut m, &mut l, &mut r);
        while let Ok(v) = consumer.pop() {
            peak = peak.max(v.abs());
            sent.push(v);
        }
    }
    assert!(peak <= 0.8913, "stream never above -1 dBFS: {peak}");
    assert!(
        c.status().take_bus_reduction() > 0.0,
        "limiter/compressor worked"
    );
    assert!(c.status().mic_gate_open.load(Ordering::Relaxed));
    // After 10 s close to the target: loudness of the last 3 s.
    let tail = &sent[sent.len() - 3 * 2 * SR as usize..];
    let (tl, tr): (Vec<f32>, Vec<f32>) = tail.chunks(2).map(|p| (p[0], p[1])).unzip();
    let lufs = integrated_lufs(&tl, &tr, SR).unwrap();
    assert!((lufs + 16.0).abs() < 2.0, "{lufs} LUFS");

    // Switched off: the AGC reports 0 dB.
    c.set_processing(false, false);
    block(&mut c, &mut m, &mut l, &mut r);
    block(&mut c, &mut m, &mut l, &mut r);
    assert_eq!(c.status().bus_agc_db(), 0.0);
}

#[test]
fn stream_ring_gets_the_broadcast_without_alloc_and_counts_drops() {
    let tmp = tempfile::tempdir().unwrap();
    let a = tmp.path().join("a.wav");
    write_tone(&a, 1.0, 0.0, 0.5);
    let (mut c, mut m) = Controller::new(show_of(&[a], 0.0), SR);
    let (producer, mut consumer) = rtrb::RingBuffer::new(BLOCK * 2 * 3);
    c.set_stream_output(Some(producer));
    c.start();
    let mut l = vec![0.0f32; BLOCK];
    let mut r = vec![0.0f32; BLOCK];
    for _ in 0..20 {
        block(&mut c, &mut m, &mut l, &mut r);
    }
    // The ring holds 3 blocks; the rest is counted and never blocks the callback.
    assert_eq!(consumer.slots(), BLOCK * 2 * 3);
    let dropped = c.status().stream_dropped.load(Ordering::Relaxed);
    assert_eq!(dropped as usize, BLOCK * 2 * 17);
    let mut got = Vec::new();
    while let Ok(v) = consumer.pop() {
        got.push(v);
    }
    assert!(got.iter().any(|v| *v > 0.1), "music in stream");
    // Detached: no more pushes.
    c.set_stream_output(None);
    block(&mut c, &mut m, &mut l, &mut r);
    block(&mut c, &mut m, &mut l, &mut r);
    assert_eq!(consumer.slots(), 0);
}

/// Paket 14: Pegelregler für Musik und Carts (vom Controller-Fader), geglättet und ohne
/// Allokation im Process-Aufruf.
#[test]
fn level_faders_scale_music_and_carts_smoothly() {
    use woa_radio::mixer::LevelBus;
    let tmp = tempfile::tempdir().unwrap();
    let a = tmp.path().join("musik.wav");
    let jingle = tmp.path().join("jingle.wav");
    write_tone(&a, 3.0, 0.0, 0.4);
    write_tone(&jingle, 2.0, 0.0, 0.2);
    let (mut c, mut m) = Controller::new(show_of(&[a], 3.0), SR);
    c.assign_cart(0, Some(jingle)).unwrap();
    c.start();
    let mut l = vec![0.0f32; BLOCK];
    let mut r = vec![0.0f32; BLOCK];
    for _ in 0..20 {
        block(&mut c, &mut m, &mut l, &mut r);
    }
    let full = l[BLOCK - 1];
    // Die Musik hat ihre Titel-Verstärkung (Lautheitsausgleich); gerechnet wird relativ.
    assert!(full > 0.1, "{full}");

    // Musik auf −6 dB: gleitet hin (kein Sprung), landet bei der Hälfte.
    c.set_level(LevelBus::Music, -6.0206);
    let mut prev = full;
    let mut max_step = 0.0f32;
    for _ in 0..40 {
        block(&mut c, &mut m, &mut l, &mut r);
        for &x in l.iter() {
            max_step = max_step.max((x - prev).abs());
            prev = x;
        }
    }
    assert!((l[BLOCK - 1] - full / 2.0).abs() < 2e-3, "{}", l[BLOCK - 1]);
    assert!(max_step < 0.002, "Sprung {max_step}");

    // Cart dazu, Carts-Regler ganz unten: nur die Musik bleibt.
    c.set_level(LevelBus::Carts, -100.0);
    c.trigger_cart(0);
    for _ in 0..40 {
        block(&mut c, &mut m, &mut l, &mut r);
    }
    assert_ne!(
        c.status().carts.load(Ordering::Acquire) & 1,
        0,
        "Cart läuft"
    );
    assert!((l[BLOCK - 1] - full / 2.0).abs() < 2e-3, "{}", l[BLOCK - 1]);
    // Carts wieder auf 0 dB: Cart hörbar.
    c.set_level(LevelBus::Carts, 0.0);
    for _ in 0..40 {
        block(&mut c, &mut m, &mut l, &mut r);
    }
    assert!(
        (l[BLOCK - 1] - (full / 2.0 + 0.2)).abs() < 2e-3,
        "{}",
        l[BLOCK - 1]
    );
}

#[test]
fn cue_points_shape_the_offline_render() {
    let tmp = tempfile::tempdir().unwrap();
    let mut paths = Vec::new();
    for (i, secs) in [8.0, 6.0].into_iter().enumerate() {
        let p = tmp.path().join(format!("c{i}.wav"));
        write_tone(&p, secs, 440.0, 0.2);
        paths.push(p);
    }
    let mut show = show_of(&paths, 3.0);
    // Titel 1: 1 s bis 7 s, Fade-Out ab 6 s (1 s Überblendung); Titel 2 ab 2 s.
    show.playlist[0].cues = woa_radio::Cues {
        cue_in: Some(1.0),
        cue_out: Some(7.0),
        fade_out: Some(6.0),
        ramp: Some(3.0),
        ..Default::default()
    };
    show.playlist[1].cues = woa_radio::Cues {
        cue_in: Some(2.0),
        ..Default::default()
    };
    let out = render(&show, SR).unwrap();
    // 6 s + 4 s - 1 s = 9 s.
    assert_eq!(out.left.len(), 9 * SR as usize);
    // Die Vorhersage der Restzeit stimmt mit dem Render überein.
    let mut measured = show.clone();
    measured.playlist[0].seconds = Some(8.0);
    measured.playlist[1].seconds = Some(6.0);
    assert!((measured.total_seconds_from(0) - 9.0).abs() < 1e-6);
}

#[test]
fn pfl_never_reaches_the_broadcast() {
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path().join("pfl.wav");
    write_tone(&p, 2.0, 440.0, 0.5);
    let show = show_of(std::slice::from_ref(&p), 0.0);
    let (mut c, mut m) = Controller::new(show, SR);
    let (h, mut pfl) = woa_radio::pfl::channel(SR);
    c.set_pfl(Some(h));
    c.pfl_title(0, false).unwrap();
    let (mut l, mut r) = (vec![0.0f32; BLOCK], vec![0.0f32; BLOCK]);
    let (mut hl, mut hr) = (vec![0.0f32; BLOCK], vec![0.0f32; BLOCK]);
    let mut heard = 0.0f32;
    for _ in 0..20 {
        block(&mut c, &mut m, &mut l, &mut r);
        assert_no_alloc(|| pfl.process(&mut hl, &mut hr));
        assert!(
            l.iter().chain(r.iter()).all(|v| *v == 0.0),
            "broadcast silent"
        );
        heard = hl.iter().fold(heard, |a, v| a.max(v.abs()));
    }
    assert!(heard > 0.4, "headphones hear the title: {heard}");
    assert!(c.pfl_playing());
    c.pfl_stop();
    assert_no_alloc(|| pfl.process(&mut hl, &mut hr));
    assert!(!c.pfl_playing());
    assert!(c.pfl_cart(0).is_err());
}

#[test]
fn cue_edits_undo_and_redo() {
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path().join("u.wav");
    write_tone(&p, 4.0, 440.0, 0.2);
    let (mut c, _m) = Controller::new(show_of(std::slice::from_ref(&p), 0.0), SR);
    let cues = woa_radio::Cues {
        hook: Some(2.0),
        ..Default::default()
    };
    c.set_cues(0, cues).unwrap();
    let bad = woa_radio::Cues {
        cue_in: Some(9.0),
        cue_out: Some(1.0),
        ..Default::default()
    };
    assert!(c.set_cues(0, bad).is_err());
    assert!(c.undo_cues());
    assert!(c.show().playlist[0].cues.is_empty());
    assert!(c.redo_cues());
    assert_eq!(c.show().playlist[0].cues, cues);
}

/// Zahl der Frames, in denen am linken Ausgang etwas liegt, bis das Cart durch ist.
fn cart_frames(c: &mut Controller, m: &mut Mixer, slot: usize, seconds: f64) -> usize {
    let mut l = vec![0.0f32; BLOCK];
    let mut r = vec![0.0f32; BLOCK];
    c.trigger_cart(slot);
    let (mut sounding, mut first) = (0usize, None);
    for b in 0..((seconds * f64::from(SR)) as usize / BLOCK) {
        block(c, m, &mut l, &mut r);
        for (i, v) in l.iter().enumerate() {
            if v.abs() > 0.01 {
                sounding += 1;
                first.get_or_insert(b * BLOCK + i);
            }
        }
    }
    let _ = first;
    sounding
}

#[test]
fn cart_plays_only_between_its_cue_points() {
    use woa_radio::Cues;
    let tmp = tempfile::tempdir().unwrap();
    let jingle = tmp.path().join("jingle.wav");
    write_tone(&jingle, 2.0, 0.0, 0.3);
    let show = RadioShow {
        bus_processing: false,
        mic_processing: false,
        ..RadioShow::default()
    };
    let (mut c, mut m) = Controller::new(show, SR);
    c.assign_cart(2, Some(jingle)).unwrap();
    let whole = cart_frames(&mut c, &mut m, 2, 2.5);
    assert!((whole as f64 / f64::from(SR) - 2.0).abs() < 0.02, "{whole}");
    c.set_cart_cues(
        2,
        Cues {
            cue_in: Some(0.5),
            cue_out: Some(1.25),
            ..Cues::default()
        },
    )
    .unwrap();
    let cut = cart_frames(&mut c, &mut m, 2, 2.5);
    assert!((cut as f64 / f64::from(SR) - 0.75).abs() < 0.02, "{cut}");
    assert_eq!(c.status().carts.load(Ordering::Acquire) & (1 << 2), 0);
    // Ein Ende hinter der Datei oder vor dem Anfang wird abgelehnt.
    let past = Cues {
        cue_out: Some(5.0),
        ..Cues::default()
    };
    assert!(c.set_cart_cues(2, past).is_err());
}

#[test]
fn playlists_switch_and_delete_never_stop_the_running_title() {
    let tmp = tempfile::tempdir().unwrap();
    let a = tmp.path().join("a.wav");
    let b = tmp.path().join("b.wav");
    write_tone(&a, 1.0, 0.0, 0.3);
    write_tone(&b, 2.0, 0.0, 0.3);
    let (mut c, mut m) = Controller::new(show_of(std::slice::from_ref(&a), 0.0), SR);
    let other = c.new_playlist("Abend");
    assert!(c.playlist_add(other, std::slice::from_ref(&b)));
    c.start();
    let mut l = vec![0.0f32; BLOCK];
    let mut r = vec![0.0f32; BLOCK];
    for _ in 0..10 {
        block(&mut c, &mut m, &mut l, &mut r);
    }
    let playing = c.status().current.load(Ordering::Acquire);
    assert_ne!(playing, NONE);
    // Die laufende Playlist lässt sich auf Sendung nicht löschen.
    assert!(c.delete_playlist(0).is_err());
    assert_eq!(c.show().playlists.len(), 2);
    // Wechsel: der laufende Titel spielt weiter, danach kommt die neue Playlist.
    assert!(c.select_playlist(other));
    block(&mut c, &mut m, &mut l, &mut r);
    assert_eq!(c.status().current.load(Ordering::Acquire), playing);
    assert!(c.on_air());
    // Die alte (nicht mehr aktive) darf jetzt weg, ohne Abbruch, und kommt per Undo zurück.
    let d = c.delete_playlist(0).unwrap();
    assert_eq!(c.status().current.load(Ordering::Acquire), playing);
    assert_eq!(c.show().active_name(), "Abend");
    c.restore_playlist(d);
    assert_eq!(c.show().playlist_names(), vec!["Playlist 1", "Abend"]);
    assert_eq!(c.show().active_name(), "Abend");
    for _ in 0..(SR as usize * 3 / 2 / BLOCK) {
        block(&mut c, &mut m, &mut l, &mut r);
    }
    assert_eq!(c.current_index(), Some(0), "b aus „Abend“ läuft nach a");
    assert_eq!(c.show().playlist[0].path, b);
}
