//! Voice-Tracking (Paket 30): A, Stimme, B als erzeugte Töne offline durch den echten Sendemixer
//! (kein Gerät, kein Ton, kein Mikrofon). Gemessen wird am Ausgang mit dem Goertzel-Filter je
//! Tonfrequenz: Beginn der Stimme gegen den Übergang, Tiefe der Absenkung, Rückkehr der Musik,
//! keine Lücke. Dazu: Nachbar tauschen setzt „prüfen“, Speichern und Laden als Schnappschuss.

use std::path::{Path, PathBuf};

use assert_no_alloc::{AllocDisabler, assert_no_alloc};
use woa_radio::voicetrack::{VoiceParams, VoiceTrack};
use woa_radio::{Controller, PlaylistEntry, RadioShow};

#[global_allocator]
static ALLOC: AllocDisabler = AllocDisabler;

const SR: u32 = 8_000;
const BLOCK: usize = 256;
const A_HZ: f32 = 440.0;
const V_HZ: f32 = 1000.0;
const B_HZ: f32 = 1500.0;
const C_HZ: f32 = 1800.0;
/// Überblendung zwischen A und B (Sekunden).
const XF: f64 = 3.0;
const A_LEN: f64 = 30.0;

fn write_tone(path: &Path, seconds: f64, freq: f32, amp: f32) {
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: SR,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut w = hound::WavWriter::create(path, spec).unwrap();
    for i in 0..(seconds * f64::from(SR)) as usize {
        let v = (i as f32 * freq * std::f32::consts::TAU / SR as f32).sin() * amp;
        w.write_sample(v).unwrap();
        w.write_sample(v).unwrap();
    }
    w.finalize().unwrap();
}

/// Amplitude der Frequenz `hz` im Fenster ab Sekunde `from` (Länge `len` Sekunden).
fn mag(out: &[f32], hz: f32, from: f64, len: f64) -> f32 {
    let a = (from * f64::from(SR)).round() as usize;
    let n = (len * f64::from(SR)).round() as usize;
    let w = &out[a..(a + n).min(out.len())];
    let (mut re, mut im) = (0.0f64, 0.0f64);
    for (i, v) in w.iter().enumerate() {
        let ph = std::f64::consts::TAU * f64::from(hz) * i as f64 / f64::from(SR);
        re += f64::from(*v) * ph.cos();
        im -= f64::from(*v) * ph.sin();
    }
    (2.0 * (re * re + im * im).sqrt() / w.len().max(1) as f64) as f32
}

/// Erste Sekunde, ab der `hz` im 50-ms-Fenster über `level` liegt.
fn onset(out: &[f32], hz: f32, level: f32, from: f64) -> Option<f64> {
    let mut t = from;
    while (t + 0.05) * f64::from(SR) < out.len() as f64 {
        if mag(out, hz, t, 0.05) > level {
            return Some(t);
        }
        t += 0.01;
    }
    None
}

fn min_rms(out: &[f32], from_s: f64, to_s: f64) -> f32 {
    let win = SR as usize / 20;
    let (mut i, end) = (
        (from_s * f64::from(SR)) as usize,
        ((to_s * f64::from(SR)) as usize).min(out.len()),
    );
    let mut min = f32::MAX;
    while i + win <= end {
        let w = &out[i..i + win];
        min = min.min((w.iter().map(|v| v * v).sum::<f32>() / win as f32).sqrt());
        i += win;
    }
    min
}

struct Files {
    a: PathBuf,
    v: PathBuf,
    b: PathBuf,
    c: PathBuf,
}

fn files(dir: &Path, voice_seconds: f64) -> Files {
    let f = Files {
        a: dir.join("a.wav"),
        v: dir.join("stimme.wav"),
        b: dir.join("b.wav"),
        c: dir.join("c.wav"),
    };
    write_tone(&f.a, A_LEN, A_HZ, 0.3);
    write_tone(&f.v, voice_seconds, V_HZ, 0.3);
    write_tone(&f.b, 30.0, B_HZ, 0.3);
    write_tone(&f.c, 30.0, C_HZ, 0.3);
    f
}

fn voice_entry(path: &Path, params: VoiceParams) -> PlaylistEntry {
    let mut e = PlaylistEntry::new(path);
    e.voice = Some(VoiceTrack::new(params));
    e
}

fn show_of(playlist: Vec<PlaylistEntry>) -> RadioShow {
    let mut s = RadioShow {
        crossfade_seconds: XF,
        bus_processing: false,
        mic_processing: false,
        playlist,
        ..RadioShow::default()
    };
    // Bindung an die Nachbarn, wie sie beim Einfügen entsteht.
    s.relink_voices();
    s
}

/// Sendet `seconds` durch den echten Mixer; `press_next_at` drückt „Weiter“ zu dieser Zeit.
fn run(show: RadioShow, seconds: f64, press_next_at: Option<f64>) -> (Vec<f32>, Controller) {
    let (mut c, mut m) = Controller::new(show, SR);
    c.measure_missing();
    c.start();
    let mut l = vec![0.0f32; BLOCK];
    let mut r = vec![0.0f32; BLOCK];
    let total = (seconds * f64::from(SR)) as usize;
    let mut out = Vec::with_capacity(total);
    let mut pressed = press_next_at.is_none();
    while out.len() < total {
        c.pump();
        if !pressed && out.len() as f64 / f64::from(SR) >= press_next_at.unwrap_or(0.0) {
            c.start_or_next();
            pressed = true;
        }
        assert_no_alloc(|| m.process(&mut l, &mut r));
        out.extend_from_slice(&l);
    }
    (out, c)
}

fn params(offset: f64) -> VoiceParams {
    VoiceParams {
        offset,
        gain_db: 0.0,
        duck_db: -12.0,
        attack: 0.4,
        release: 0.8,
    }
}

#[test]
fn offline_render_a_voice_b_offset_depth_and_no_gap() {
    let tmp = tempfile::tempdir().unwrap();
    let f = files(tmp.path(), 4.0);
    let show = show_of(vec![
        PlaylistEntry::new(&f.a),
        voice_entry(&f.v, params(-2.0)),
        PlaylistEntry::new(&f.b),
    ]);
    // Übergang bei A_LEN - XF = 27 s; die Stimme beginnt 2 s davor.
    let t0 = A_LEN - XF;
    let (out, c) = run(show, 45.0, None);
    assert!(!c.show().playlist[1].voice.as_ref().unwrap().check);

    // Versatz: Stimme bei 25 s, B setzt bei 27 s ein.
    let v_on = onset(&out, V_HZ, 0.02, 10.0).expect("Stimme fehlt");
    assert!(
        (v_on - (t0 - 2.0)).abs() < 0.06,
        "Stimme beginnt bei {v_on}"
    );
    let b_on = onset(&out, B_HZ, 0.002, 10.0).expect("B fehlt");
    assert!(
        (t0..t0 + 0.3).contains(&b_on),
        "B beginnt bei {b_on}, erwartet {t0}"
    );
    // Stimme endet nach 4 s (bei 29 s).
    assert!(mag(&out, V_HZ, 28.5, 0.3) > 0.02);
    assert!(mag(&out, V_HZ, 29.3, 0.3) < 0.002, "Stimme läuft zu lang");

    // Tiefe: A unter der Stimme gegen A allein (12 dB = Faktor 0,2512).
    let free = mag(&out, A_HZ, 15.0, 0.4);
    let ducked = mag(&out, A_HZ, 25.6, 0.4);
    let ratio = ducked / free;
    assert!(
        (ratio - 0.2512).abs() < 0.2512 * 0.03,
        "Absenkung {:.2} dB statt -12 dB",
        20.0 * ratio.log10()
    );
    // Absenkung beginnt vor der Stimme (0,4 s), vor dieser Zeit ist A unberührt.
    assert!((mag(&out, A_HZ, 24.3, 0.1) / free - 1.0).abs() < 0.03);
    // B unter der Stimme gegen B allein: Überblendkurve (sin) mal Absenkung.
    let b_free = mag(&out, B_HZ, 36.0, 0.4);
    let b_ducked = mag(&out, B_HZ, 28.5, 0.2);
    let expect = (std::f32::consts::FRAC_PI_2 * (1.6 / XF as f32)).sin() * 0.2512;
    assert!(
        ((b_ducked / b_free) / expect - 1.0).abs() < 0.1,
        "B unter der Stimme {:.3}, erwartet {expect:.3}",
        b_ducked / b_free
    );
    // Rückkehr: nach Stimme und Rückholzeit (29,8 s) ist B wieder unberührt.
    let b_back = mag(&out, B_HZ, 30.5, 0.4);
    assert!(
        (b_back / b_free - 1.0).abs() < 0.03,
        "{b_back} gegen {b_free}"
    );

    // Keine Lücke: durchgehend hörbar von 1 s bis kurz vor Schluss.
    let floor = min_rms(&out, 1.0, 44.0);
    assert!(floor > 0.02, "kleinster Pegel {floor}");
}

#[test]
fn voice_after_the_transition_starts_over_the_intro() {
    let tmp = tempfile::tempdir().unwrap();
    let f = files(tmp.path(), 3.0);
    let show = show_of(vec![
        PlaylistEntry::new(&f.a),
        voice_entry(&f.v, params(1.5)),
        PlaylistEntry::new(&f.b),
    ]);
    let (out, _) = run(show, 40.0, None);
    let v_on = onset(&out, V_HZ, 0.02, 10.0).expect("Stimme fehlt");
    assert!((v_on - (A_LEN - XF + 1.5)).abs() < 0.06, "{v_on}");
    let free = mag(&out, B_HZ, 36.0, 0.4);
    // Mitte der Stimme (28,5 + 1,5 s ab 27 s = 28,5 bis 31,5 s): B abgesenkt.
    assert!(mag(&out, B_HZ, 30.5, 0.4) / free < 0.3);
    assert!(mag(&out, B_HZ, 33.5, 0.4) / free > 0.97);
}

#[test]
fn jingle_bed_starts_the_title_quietly_and_raises_it_after_the_jingle() {
    let tmp = tempfile::tempdir().unwrap();
    let f = files(tmp.path(), 6.0);
    let show = show_of(vec![
        PlaylistEntry::new(&f.a),
        voice_entry(&f.v, VoiceParams::jingle_bed()),
        PlaylistEntry::new(&f.b),
    ]);
    let t0 = A_LEN - XF;
    let (out, _) = run(show, 45.0, None);
    // Das Jingle beginnt zeitgenau mit B.
    let v_on = onset(&out, V_HZ, 0.02, 10.0).expect("Jingle fehlt");
    assert!((v_on - t0).abs() < 0.06, "Jingle bei {v_on}, erwartet {t0}");
    let free = mag(&out, B_HZ, 40.0, 0.4);
    // B unter dem Jingle (nach der Überblendung, 4 s nach Beginn): −20 dB.
    let under = mag(&out, B_HZ, t0 + 4.0, 0.4) / free;
    assert!(
        (under - 0.1).abs() < 0.1 * 0.05,
        "B unter dem Jingle {under}"
    );
    // Jingle endet nach 6 s; 1,5 s später ist B wieder voll.
    assert!(
        mag(&out, V_HZ, t0 + 6.4, 0.3) < 0.002,
        "Jingle läuft zu lang"
    );
    let back = mag(&out, B_HZ, t0 + 8.0, 0.4) / free;
    assert!((back - 1.0).abs() < 0.03, "B danach {back}");
    // Mitten im Hochfahren liegt B dazwischen, ohne Sprung.
    let mid = mag(&out, B_HZ, t0 + 6.75, 0.1) / free;
    assert!(mid > 0.2 && mid < 0.9, "B im Hochfahren {mid}");
}

#[test]
fn next_pressed_early_still_plays_the_whole_voice_from_its_start() {
    let tmp = tempfile::tempdir().unwrap();
    let f = files(tmp.path(), 4.0);
    let show = show_of(vec![
        PlaylistEntry::new(&f.a),
        voice_entry(&f.v, params(-2.0)),
        PlaylistEntry::new(&f.b),
    ]);
    // „Weiter“ bei 10 s: die Überblendung kommt 17 s früher als geplant.
    let (out, _) = run(show, 30.0, Some(10.0));
    let v_on = onset(&out, V_HZ, 0.02, 5.0).expect("Stimme fehlt");
    assert!((10.0..10.6).contains(&v_on), "Stimme beginnt bei {v_on}");
    // Die Stimme läuft vollständig (4 s), nicht mittendrin gestartet.
    assert!(mag(&out, V_HZ, v_on + 3.5, 0.3) > 0.02);
    assert!(mag(&out, V_HZ, v_on + 4.4, 0.3) < 0.002);
}

#[test]
fn swapping_a_neighbour_sets_check_and_the_voice_is_not_sent() {
    let tmp = tempfile::tempdir().unwrap();
    let f = files(tmp.path(), 4.0);
    let mut show = show_of(vec![
        PlaylistEntry::new(&f.a),
        voice_entry(&f.v, params(-2.0)),
        PlaylistEntry::new(&f.b),
    ]);
    // Der Mensch tauscht den zweiten Titel gegen einen anderen.
    show.playlist[2] = PlaylistEntry::new(&f.c);
    let (out, mut c) = run(show, 45.0, None);
    let v = c.show().playlist[1].voice.as_ref().unwrap();
    assert!(v.check, "Voice-Track muss auf prüfen stehen");
    assert!(
        c.take_message()
            .is_some_and(|m| m.contains("prüfen") || m.contains("bestätigen")),
        "Meldung fehlt"
    );
    // Nichts gesendet: kein Ton der Stimme, die Musik ist nicht abgesenkt.
    assert!(
        onset(&out, V_HZ, 0.01, 0.0).is_none(),
        "Stimme lief trotz prüfen"
    );
    let free = mag(&out, A_HZ, 15.0, 0.4);
    let at = mag(&out, A_HZ, 25.6, 0.4);
    assert!(
        (at / free - 1.0).abs() < 0.03,
        "A bei 25,6 s: {at} gegen frei {free}"
    );
    assert!(
        onset(&out, C_HZ, 0.01, 10.0).is_some(),
        "neuer Titel spielt"
    );

    // Bestätigen: danach geht die Stimme auf Sendung.
    let mut show = c.show().clone();
    show.confirm_voice(1).unwrap();
    let (out, c) = run(show, 45.0, None);
    assert!(!c.show().playlist[1].voice.as_ref().unwrap().check);
    assert!(
        onset(&out, V_HZ, 0.02, 10.0).is_some(),
        "Stimme fehlt nach Bestätigung"
    );
}

#[test]
fn voice_entries_never_start_or_end_the_show() {
    let tmp = tempfile::tempdir().unwrap();
    let f = files(tmp.path(), 2.0);
    // Voice-Track steht vorn: die Sendung beginnt mit dem ersten Titel, die Stimme liegt
    // über seinem Anfang. Am Ende steht ein Voice-Track ohne Nachfolger: er spielt nicht.
    let show = show_of(vec![
        voice_entry(&f.v, params(0.5)),
        PlaylistEntry::new(&f.a),
        voice_entry(&f.v, params(0.0)),
    ]);
    let (out, c) = run(show, 12.0, None);
    assert!(mag(&out, A_HZ, 5.0, 0.4) > 0.01, "Musik läuft");
    let v_on = onset(&out, V_HZ, 0.02, 0.0).expect("Stimme fehlt");
    assert!((0.0..1.0).contains(&v_on), "Stimme beginnt bei {v_on}");
    assert!(mag(&out, V_HZ, 5.0, 0.4) < 0.002);
    assert!(c.show().playlist.len() == 3);
}

#[test]
fn save_and_load_snapshot() {
    let mut show = show_of(vec![
        {
            let mut a = PlaylistEntry::new("musik/a.wav");
            a.seconds = Some(200.0);
            a.cues.cue_out = Some(195.5);
            a
        },
        {
            let mut v = voice_entry(
                Path::new("moderation/ansage-1.wav"),
                VoiceParams {
                    offset: -3.25,
                    gain_db: 1.5,
                    duck_db: -14.0,
                    attack: 0.5,
                    release: 1.0,
                },
            );
            v.seconds = Some(6.0);
            v
        },
        {
            let mut b = PlaylistEntry::new("musik/b.wav");
            b.seconds = Some(180.0);
            b.cues.ramp = Some(8.0);
            b
        },
    ]);
    show.relink_voices();
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path().join("radio.json");
    show.save(&p).unwrap();
    let text = std::fs::read_to_string(&p).unwrap();
    insta::assert_snapshot!("radio_json_with_voice_track", text);
    let back = RadioShow::load(&p).unwrap();
    assert_eq!(back, show);
    // Nach dem Laden stimmen die Nachbarn noch: kein „prüfen“.
    let mut back = back;
    assert!(back.relink_voices().is_empty());
    // Ändert sich nach dem Laden ein Nachbar (anderer Titel), steht der Eintrag auf prüfen.
    back.playlist[0].path = PathBuf::from("musik/anderer.wav");
    assert_eq!(back.relink_voices(), vec![1]);
}
