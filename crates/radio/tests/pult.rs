//! Pult-Modus (#301) offline und ohne Ton: Die Sendung (Stream, Mitschnitt) kommt nur vom
//! Pult-Eingang, WOAs interne Mischung geht nur auf die Ausgänge. Ohne Schalter bleibt alles
//! bit-gleich. Jeder Block läuft unter `assert_no_alloc`.

use std::path::Path;

use assert_no_alloc::{AllocDisabler, assert_no_alloc};
use woa_engine::pairs::{LinkTx, link};
use woa_radio::{Controller, RadioShow};

#[global_allocator]
static ALLOC: AllocDisabler = AllocDisabler;

const SR: u32 = 48_000;
const BLOCK: usize = 256;
const MUSIC: f32 = 0.25;
const SINE: f32 = 0.5;

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

/// Sinus 1 kHz vom Pult, ab Sample `from`.
fn sine(from: usize, len: usize) -> Vec<f32> {
    (from..from + len)
        .map(|k| SINE * (2.0 * std::f32::consts::PI * 1000.0 * k as f32 / SR as f32).sin())
        .collect()
}

struct Heard {
    /// Sendung (Stream-Ring), links.
    stream: Vec<f32>,
    /// Hauptausgang (Richtung Pult), links.
    main: Vec<f32>,
}

/// Playlist mit Gleichwert-Musik; `pult`: ein Pult-Eingang hängt (Sinus), `mode`: Pult-Modus.
fn run(bus: bool, pult: bool, mode: bool, blocks: usize) -> Heard {
    let dir = tempfile::tempdir().unwrap();
    let music = dir.path().join("musik.wav");
    write_dc(&music, 20.0, MUSIC);
    let show = RadioShow {
        bus_processing: bus,
        mic_processing: false,
        playlist: vec![woa_radio::PlaylistEntry::new(&music)],
        ..RadioShow::default()
    };
    let (mut c, mut m) = Controller::new(show, SR);
    let (prod, mut cons) = rtrb::RingBuffer::new(2 * BLOCK * (blocks + 1));
    c.set_stream_output(Some(prod));
    let mut tx: Option<LinkTx> = None;
    if pult {
        let (mut t, rx, _s) = link(SR, SR);
        let pre = sine(0, 4800);
        t.push(&pre, &pre);
        c.set_pult_input(Some(rx));
        tx = Some(t);
    }
    c.set_pult_mode(mode);
    c.start();
    let (mut l, mut r) = (vec![0.0f32; BLOCK], vec![0.0f32; BLOCK]);
    let mut heard = Heard {
        stream: Vec::new(),
        main: Vec::new(),
    };
    let mut fed = 4800;
    for _ in 0..blocks {
        if let Some(t) = tx.as_mut() {
            let s = sine(fed, BLOCK);
            t.push(&s, &s);
            fed += BLOCK;
        }
        c.pump();
        assert_no_alloc(|| m.process(&mut l, &mut r));
        heard.main.extend_from_slice(&l);
        while let Ok(v) = cons.pop() {
            heard.stream.push(v);
            let _ = cons.pop();
        }
    }
    heard
}

fn mean(v: &[f32]) -> f32 {
    v.iter().sum::<f32>() / v.len() as f32
}

fn rms(v: &[f32]) -> f32 {
    (v.iter().map(|x| x * x).sum::<f32>() / v.len() as f32).sqrt()
}

#[test]
fn pult_mode_sends_only_the_pult_input_and_the_music_goes_to_the_outputs() {
    let h = run(false, true, true, 400);
    let tail = &h.stream[h.stream.len() - 48_000..];
    // Der Sinus vom Pult ist auf Sendung, die Playlist-Musik (Gleichwert) nicht.
    assert!(
        mean(tail).abs() < 0.01,
        "Musik in der Sendung: Mittel {}",
        mean(tail)
    );
    let want = SINE / 2f32.sqrt();
    assert!(
        (rms(tail) - want).abs() < 0.02,
        "Sinus {} statt {want}",
        rms(tail)
    );
    // Der Ausgang Richtung Pult trägt die Musik, genau wie bisher ohne Sendebus.
    let old = run(false, false, false, 400);
    assert_eq!(h.main, old.main);
    assert!(mean(&h.main[h.main.len() - 4800..]) > 0.1);
}

#[test]
fn with_bus_on_the_bus_works_on_the_pult_signal_and_the_outputs_stay_unprocessed() {
    let h = run(true, true, true, 400);
    let tail = &h.stream[h.stream.len() - 48_000..];
    assert!(
        mean(tail).abs() < 0.01,
        "Musik in der Sendung: Mittel {}",
        mean(tail)
    );
    assert!(rms(tail) > 0.05, "Sendung still: {}", rms(tail));
    // Kein Sendebus auf dem Ausgang Richtung Pult: genau die Musik ohne Bearbeitung.
    let old = run(false, false, false, 400);
    assert_eq!(h.main, old.main);
}

#[test]
fn without_the_switch_a_hanging_pult_input_changes_nothing_bit_for_bit() {
    for bus in [false, true] {
        let plain = run(bus, false, false, 200);
        let with = run(bus, true, false, 200);
        assert_eq!(plain.stream.len(), with.stream.len());
        assert!(
            plain
                .stream
                .iter()
                .zip(&with.stream)
                .all(|(a, b)| a.to_bits() == b.to_bits()),
            "Sendung nicht bit-gleich (Bus {bus})"
        );
        assert!(
            plain
                .main
                .iter()
                .zip(&with.main)
                .all(|(a, b)| a.to_bits() == b.to_bits()),
            "Ausgang nicht bit-gleich (Bus {bus})"
        );
        // Die Musik ist auf Sendung wie bisher.
        assert!(mean(&plain.stream[plain.stream.len() - 4800..]) > 0.05);
    }
}

#[test]
fn pult_input_is_metered_even_when_the_mode_is_off() {
    let dir = tempfile::tempdir().unwrap();
    let (mut c, mut m) = Controller::new(RadioShow::default(), SR);
    let (mut t, rx, _s) = link(SR, SR);
    let pre = sine(0, 4800);
    t.push(&pre, &pre);
    c.set_pult_input(Some(rx));
    let (mut l, mut r) = (vec![0.0f32; BLOCK], vec![0.0f32; BLOCK]);
    for b in 0..20 {
        let s = sine(4800 + b * BLOCK, BLOCK);
        t.push(&s, &s);
        c.pump();
        assert_no_alloc(|| m.process(&mut l, &mut r));
    }
    let peak = c.status().take_pult_peak();
    assert!((peak - SINE).abs() < 0.01, "Pegel {peak}");
    drop(dir);
}
