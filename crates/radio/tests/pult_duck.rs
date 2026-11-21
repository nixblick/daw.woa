//! Ducking im Pult-Modus (#596) offline und ohne Ton, im geschlossenen Kreis: Was der Mixer
//! Richtung Pult ausgibt, kommt mit Laufzeit und Verstärkung (die „Pult-Regler“) und optional
//! einer künstlichen Stimme als Pult-Eingang zurück. Jeder Block läuft unter `assert_no_alloc`.
//!
//! - A1: eigener Auslöser-Eingang über der Schwelle duckt die Ausgabe Richtung Pult und
//!   kommt zurück; aus oder außerhalb des Pult-Modus duckt er nicht; er geht nie auf Sendung.
//! - A2: automatische Erkennung im Pult-Eingang: Musik allein löst nicht aus (auch nach
//!   Reglerwechsel), Musik plus Stimme löst aus, kein Pumpen über 30 s, Laufzeit 0/50/200 ms.

#![allow(clippy::unwrap_used, clippy::expect_used)] // Tests duerfen abbrechen (Regel 6 gilt fuer Produktivcode).

use std::collections::VecDeque;
use std::path::Path;

use assert_no_alloc::{AllocDisabler, assert_no_alloc};
use woa_engine::pairs::{LinkTx, link};
use woa_radio::duckdetect::{TriggerCfg, TriggerMode};
use woa_radio::mixer::{DUCK_AUTO, DUCK_TRIGGER};
use woa_radio::{Controller, RadioShow};

#[global_allocator]
static ALLOC: AllocDisabler = AllocDisabler;

const SR: u32 = 48_000;
const BLOCK: usize = 256;
/// Vorlauf der Taktverbindungen (Samples), damit sie sofort laufen.
const PRE: usize = 4800;

fn block_s() -> f32 {
    BLOCK as f32 / SR as f32
}

/// Musik zum Testen: Rauschen mit Schlägen alle 0,5 s und Lautstärkewechsel alle 4 s.
fn music(seconds: f32) -> Vec<f32> {
    let n = (seconds * SR as f32) as usize;
    let mut seed = 0x1234_5678u32;
    let phrases = [1.0f32, 0.6, 0.9, 0.5, 1.0, 0.7, 0.8, 0.55, 1.0, 0.65];
    (0..n)
        .map(|k| {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let white = (seed >> 8) as f32 / (1u32 << 24) as f32 * 2.0 - 1.0;
            let t = k as f32 / SR as f32;
            let beat = t % 0.5;
            let env = 0.55 + 0.45 * (-beat / 0.12).exp();
            let phrase = phrases[(t / 4.0) as usize % phrases.len()];
            0.3 * white * env * phrase
        })
        .collect()
}

fn write_music(path: &Path, seconds: f32) {
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: SR,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut w = hound::WavWriter::create(path, spec).unwrap();
    for s in music(seconds) {
        w.write_sample(s).unwrap();
        w.write_sample(s).unwrap();
    }
    w.finalize().unwrap();
}

/// Künstliche Stimme: Grundton 140 Hz mit Obertönen, Silben 180 ms an, 80 ms Pause,
/// Effektivwert `rms` in den Silben. `phrases`: (von, bis) in Sekunden.
fn voice(k: usize, rms: f32, phrases: &[(f32, f32)]) -> f32 {
    let t = k as f32 / SR as f32;
    if !phrases.iter().any(|&(a, b)| t >= a && t < b) {
        return 0.0;
    }
    let syl = t % 0.26;
    if syl > 0.18 {
        return 0.0;
    }
    let shape = (std::f32::consts::PI * syl / 0.18).sin();
    let mut v = 0.0;
    for h in 1..=6 {
        v += (2.0 * std::f32::consts::PI * 140.0 * h as f32 * t).sin() / h as f32;
    }
    // Effektivwert der Obertonreihe: sqrt(sum 1/(2 h²)) ≈ 0.87.
    v / 0.87 * rms * shape * std::f32::consts::SQRT_2
}

struct Rig {
    c: Controller,
    m: woa_radio::Mixer,
    pult: LinkTx,
    trig: LinkTx,
    stream: rtrb::Consumer<f32>,
    _dir: tempfile::TempDir,
}

fn rig(seconds: f32, cfg: TriggerCfg, pult_mode: bool) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("musik.wav");
    write_music(&path, seconds + 2.0);
    let show = RadioShow {
        bus_processing: false,
        mic_processing: false,
        playlist: vec![woa_radio::PlaylistEntry::new(&path)],
        ..RadioShow::default()
    };
    let (mut c, m) = Controller::new(show, SR);
    let (prod, cons) = rtrb::RingBuffer::new(SR as usize * 2);
    c.set_stream_output(Some(prod));
    let (mut pult, rx, _s) = link(SR, SR);
    let z = vec![0.0f32; PRE];
    pult.push(&z, &z);
    c.set_pult_input(Some(rx));
    let (mut trig, rx, _s) = link(SR, SR);
    trig.push(&z, &z);
    c.set_trigger_input(Some(rx));
    c.set_pult_mode(pult_mode);
    c.set_trigger(cfg);
    c.start();
    Rig {
        c,
        m,
        pult,
        trig,
        stream: cons,
        _dir: dir,
    }
}

/// Was je Block beobachtet wurde.
#[derive(Default)]
struct Trace {
    cause: Vec<u32>,
    gain: Vec<f32>,
    /// Effektivwert je Block der Ausgabe Richtung Pult.
    main_rms: Vec<f32>,
    /// Effektivwert der Sendung (Stream) insgesamt.
    stream_rms: f32,
}

impl Trace {
    fn blocks_with(&self, bit: u32, from_s: f32, to_s: f32) -> usize {
        let (a, b) = ((from_s / block_s()) as usize, (to_s / block_s()) as usize);
        self.cause[a.min(self.cause.len())..b.min(self.cause.len())]
            .iter()
            .filter(|c| **c & bit != 0)
            .count()
    }

    /// Wie oft das Ducking aus `bit` angeht.
    fn onsets(&self, bit: u32) -> usize {
        self.cause
            .windows(2)
            .filter(|w| w[0] & bit == 0 && w[1] & bit != 0)
            .count()
    }

    fn min_gain(&self, from_s: f32, to_s: f32) -> f32 {
        let (a, b) = ((from_s / block_s()) as usize, (to_s / block_s()) as usize);
        self.gain[a..b.min(self.gain.len())]
            .iter()
            .fold(1.0f32, |x, &y| x.min(y))
    }
}

/// Geschlossener Kreis: Ausgabe Richtung Pult → Laufzeit `delay_ms` → Verstärkung `gain(t)`
/// → plus Stimme → Pult-Eingang. `trigger(k)`: Signal am Auslöser-Eingang.
fn run(
    r: &mut Rig,
    seconds: f32,
    delay_ms: f32,
    gain: &dyn Fn(f32) -> f32,
    pult_voice: &dyn Fn(usize) -> f32,
    trigger: &dyn Fn(usize) -> f32,
) -> Trace {
    let blocks = (seconds / block_s()) as usize;
    let delay = (delay_ms / 1000.0 * SR as f32) as usize;
    let mut line: VecDeque<f32> = std::iter::repeat_n(0.0, delay).collect();
    let (mut l, mut rr) = (vec![0.0f32; BLOCK], vec![0.0f32; BLOCK]);
    let mut back = vec![0.0f32; BLOCK];
    let mut kin = vec![0.0f32; BLOCK];
    let mut t = Trace::default();
    let mut sq = 0.0f64;
    let mut cnt = 0usize;
    let mut k0 = 0usize;
    for _ in 0..blocks {
        r.c.pump();
        assert_no_alloc(|| r.m.process(&mut l, &mut rr));
        let st = r.c.status();
        t.cause.push(st.duck_cause());
        t.gain.push(st.duck_gain());
        t.main_rms
            .push((l.iter().map(|x| x * x).sum::<f32>() / BLOCK as f32).sqrt());
        for (j, s) in l.iter().enumerate() {
            line.push_back(*s);
            let k = k0 + j;
            let d = line.pop_front().unwrap_or(0.0);
            back[j] = d * gain(k as f32 / SR as f32) + pult_voice(k);
            kin[j] = trigger(k);
        }
        k0 += BLOCK;
        r.pult.push(&back, &back);
        r.trig.push(&kin, &kin);
        while let Ok(v) = r.stream.pop() {
            sq += f64::from(v * v);
            cnt += 1;
        }
    }
    t.stream_rms = (sq / cnt.max(1) as f64).sqrt() as f32;
    t
}

fn silent(_: usize) -> f32 {
    0.0
}

fn cfg(mode: TriggerMode) -> TriggerCfg {
    TriggerCfg {
        mode,
        ..TriggerCfg::default()
    }
}

/// Sinus 300 Hz, Spitze 0,3 (−10 dBFS), am Auslöser von 3 s bis 6 s.
fn talk_3_to_6(k: usize) -> f32 {
    let t = k as f32 / SR as f32;
    if (3.0..6.0).contains(&t) {
        0.3 * (2.0 * std::f32::consts::PI * 300.0 * t).sin()
    } else {
        0.0
    }
}

// ---------- A1: eigener Auslöser-Eingang ----------

#[test]
fn trigger_input_ducks_the_music_to_the_pult_and_returns() {
    let mut r = rig(12.0, cfg(TriggerMode::Input), true);
    let t = run(&mut r, 10.0, 50.0, &|_| 0.5, &silent, &talk_3_to_6);
    assert_eq!(t.blocks_with(DUCK_TRIGGER, 0.0, 3.0), 0, "vor dem Sprechen");
    assert!(
        t.blocks_with(DUCK_TRIGGER, 3.3, 6.0) > 0,
        "Auslöser erkannt"
    );
    // −12 dB Ducking-Vorgabe: die Musik Richtung Pult geht auf etwa 0,25 herunter.
    assert!(
        t.min_gain(3.0, 6.5) < 0.3,
        "geduckt: {}",
        t.min_gain(3.0, 6.5)
    );
    let a = (2.5 / block_s()) as usize;
    let b = (5.5 / block_s()) as usize;
    let mean = |s: usize, e: usize| t.main_rms[s..e].iter().sum::<f32>() / (e - s) as f32;
    let before = mean(a - 40, a);
    let during = mean(b - 40, b);
    assert!(
        during < before * 0.4,
        "Ausgabe Richtung Pult nicht abgesenkt: {before} → {during}"
    );
    // Nach 0,5 s Haltezeit und 1 s Einblenden wieder voll.
    assert_eq!(t.blocks_with(DUCK_TRIGGER, 6.7, 10.0), 0);
    assert!(
        t.min_gain(8.0, 10.0) > 0.99,
        "zurück: {}",
        t.min_gain(8.0, 10.0)
    );
}

#[test]
fn trigger_off_or_outside_pult_mode_never_ducks() {
    let mut r = rig(8.0, cfg(TriggerMode::Off), true);
    let t = run(&mut r, 7.0, 50.0, &|_| 0.5, &silent, &talk_3_to_6);
    assert_eq!(t.onsets(DUCK_TRIGGER | DUCK_AUTO), 0, "aus");
    assert!(t.min_gain(0.0, 7.0) > 0.99);
    let mut r = rig(8.0, cfg(TriggerMode::Input), false);
    let t = run(&mut r, 7.0, 50.0, &|_| 0.5, &silent, &talk_3_to_6);
    assert_eq!(t.onsets(DUCK_TRIGGER), 0, "ohne Pult-Modus");
    assert!(t.min_gain(0.0, 7.0) > 0.99);
}

#[test]
fn trigger_input_is_never_on_air() {
    // Pult-Modus, Pult-Eingang still (Gain 0), Auslöser spricht: die Sendung bleibt still.
    let mut r = rig(8.0, cfg(TriggerMode::Input), true);
    let t = run(&mut r, 7.0, 0.0, &|_| 0.0, &silent, &talk_3_to_6);
    assert!(t.blocks_with(DUCK_TRIGGER, 3.3, 6.0) > 0);
    assert!(
        t.stream_rms < 1e-6,
        "Auslöser auf Sendung: {}",
        t.stream_rms
    );
}

#[test]
fn trigger_below_threshold_does_not_duck() {
    let quiet = |k: usize| talk_3_to_6(k) * 0.01; // −50 dBFS, Schwelle −40
    let mut r = rig(8.0, cfg(TriggerMode::Input), true);
    let t = run(&mut r, 7.0, 0.0, &|_| 0.5, &silent, &quiet);
    assert_eq!(t.onsets(DUCK_TRIGGER), 0);
}

// ---------- A2: automatisch aus dem Pult-Eingang ----------

/// Stimme in Phrasen: 3 s sprechen, 5 s Pause.
const PHRASES: [(f32, f32); 4] = [(4.0, 7.0), (12.0, 15.0), (20.0, 23.0), (27.0, 29.5)];

#[test]
fn auto_music_alone_does_not_trigger_at_any_delay() {
    for delay in [0.0, 50.0, 200.0] {
        let mut r = rig(32.0, cfg(TriggerMode::Auto), true);
        let t = run(&mut r, 30.0, delay, &|_| 0.5, &silent, &silent);
        let n = t.blocks_with(DUCK_AUTO, 0.0, 30.0);
        eprintln!("Musik allein, Laufzeit {delay} ms: {n} Blöcke geduckt");
        assert_eq!(n, 0, "Laufzeit {delay} ms: Musik allein duckt");
    }
}

#[test]
fn auto_music_alone_survives_fader_moves_at_the_pult() {
    // Regler am Pult: −10 dB sprunghaft bei 8 s, bei 14 s in 1 s um +14 dB hoch, bei 22 s +3 dB
    // sprunghaft.
    let g = |t: f32| -> f32 {
        let db = if t < 8.0 {
            -6.0
        } else if t < 14.0 {
            -16.0
        } else if t < 15.0 {
            -16.0 + 14.0 * (t - 14.0)
        } else if t < 22.0 {
            -2.0
        } else {
            1.0
        };
        10f32.powf(db / 20.0)
    };
    let mut r = rig(32.0, cfg(TriggerMode::Auto), true);
    let t = run(&mut r, 30.0, 50.0, &g, &silent, &silent);
    let n = t.blocks_with(DUCK_AUTO, 0.0, 30.0);
    eprintln!("Reglerwechsel: {n} Blöcke geduckt");
    assert_eq!(n, 0, "Musik allein duckt nach Reglerwechsel");
}

#[test]
fn auto_recovers_after_a_sudden_big_fader_jump() {
    // +12 dB sprunghaft bei 6 s: kurz darf es ducken, nach höchstens 3 s ist es vorbei und
    // kommt nicht wieder.
    let g = |t: f32| if t < 6.0 { 0.25 } else { 1.0 };
    let mut r = rig(22.0, cfg(TriggerMode::Auto), true);
    let t = run(&mut r, 20.0, 50.0, &g, &silent, &silent);
    let early = t.blocks_with(DUCK_AUTO, 0.0, 6.0);
    let late = t.blocks_with(DUCK_AUTO, 9.0, 20.0);
    let during = t.blocks_with(DUCK_AUTO, 6.0, 9.0);
    eprintln!(
        "Sprung +12 dB: geduckt {:.2} s, danach {late} Blöcke",
        during as f32 * block_s()
    );
    assert_eq!(early, 0);
    assert_eq!(late, 0, "bleibt nach dem Reglersprung hängen");
}

#[test]
fn auto_music_plus_voice_triggers_and_does_not_pump() {
    for delay in [0.0, 50.0, 200.0] {
        let mut r = rig(32.0, cfg(TriggerMode::Auto), true);
        let v = |k: usize| voice(k, 0.15, &PHRASES);
        let t = run(&mut r, 30.0, delay, &|_| 0.5, &v, &silent);
        let mut caught = 0;
        for &(a, b) in &PHRASES {
            if t.blocks_with(DUCK_AUTO, a, b.min(a + 1.0)) > 0 {
                caught += 1;
            }
            // In der Pause nach Haltezeit und Einblenden: nicht geduckt.
            let pause_from = b + 0.9;
            let pause_to = PHRASES
                .iter()
                .find(|p| p.0 > b)
                .map_or(30.0, |p| p.0)
                .min(30.0);
            if pause_to > pause_from {
                assert_eq!(
                    t.blocks_with(DUCK_AUTO, pause_from, pause_to),
                    0,
                    "Laufzeit {delay} ms: geduckt in der Pause {pause_from}–{pause_to} s"
                );
            }
        }
        let onsets = t.onsets(DUCK_AUTO);
        eprintln!(
            "Musik+Stimme, Laufzeit {delay} ms: {caught}/{} Phrasen erkannt, {onsets} Einsätze",
            PHRASES.len()
        );
        assert_eq!(
            caught,
            PHRASES.len(),
            "Laufzeit {delay} ms: Stimme verpasst"
        );
        // Kein Pumpen: je Phrase höchstens zwei Einsätze (einmal kurz in der Phrase abgesetzt).
        assert!(
            onsets <= 2 * PHRASES.len(),
            "Laufzeit {delay} ms: pumpt ({onsets} Einsätze)"
        );
        // Während der Phrasen geduckt.
        let a = PHRASES[1].0 + 1.0;
        assert!(t.min_gain(a, PHRASES[1].1) < 0.3);
    }
}

#[test]
fn auto_voice_with_silent_reference_triggers() {
    // Playlist zu Ende / still: nur Stimme am Pult.
    let mut r = rig(12.0, cfg(TriggerMode::Auto), true);
    let v = |k: usize| voice(k, 0.1, &[(3.0, 6.0)]);
    let t = run(&mut r, 8.0, 50.0, &|_| 0.0, &v, &silent);
    assert!(t.blocks_with(DUCK_AUTO, 3.0, 6.0) > 0);
}

#[test]
fn auto_sensitivity_is_reported_and_voice_10_db_over_music_is_caught() {
    // Musik kommt mit −6 dB zurück (Effektivwert ≈ 0,05). Stimme +0, +3, +6, +10 dB darüber.
    let mut caught_at_10 = 0;
    for (label, rms) in [
        ("+0 dB", 0.05f32),
        ("+3 dB", 0.07),
        ("+6 dB", 0.1),
        ("+10 dB", 0.16),
    ] {
        let mut r = rig(32.0, cfg(TriggerMode::Auto), true);
        let v = |k: usize| voice(k, rms, &PHRASES);
        let t = run(&mut r, 30.0, 50.0, &|_| 0.5, &v, &silent);
        let caught = PHRASES
            .iter()
            .filter(|&&(a, b)| t.blocks_with(DUCK_AUTO, a, b) > 0)
            .count();
        // Erster Einsatz nach Beginn der zweiten Phrase.
        let a = (PHRASES[1].0 / block_s()) as usize;
        let first = t.cause[a..]
            .iter()
            .position(|c| c & DUCK_AUTO != 0)
            .map(|p| p as f32 * block_s() * 1000.0);
        eprintln!(
            "Empfindlichkeit Stimme {label}: {caught}/{} Phrasen, Einsatz nach {first:?} ms",
            PHRASES.len()
        );
        if label == "+10 dB" {
            caught_at_10 = caught;
        }
    }
    // Ehrliche Grenze (Vorgabe Spanne 6 dB): Die Stimme muss im Pult deutlich lauter sein als
    // die zurückkommende Musik. Leiser fängt nur der eigene Auslöser-Eingang sicher.
    assert_eq!(caught_at_10, PHRASES.len());
}
