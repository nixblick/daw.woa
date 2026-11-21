//! Vorab-Dekodieren (#595): Cue-Titel und nächster Titel werden im Hintergrund geladen; der
//! Start nimmt den fertigen Puffer und dekodiert nicht mehr im Control-Thread. Ohne enge
//! Zeitgrenze: der Test wartet auf das Ereignis „fertig“, nicht auf eine Uhrzeit.

#![allow(clippy::unwrap_used, clippy::expect_used)] // Tests duerfen abbrechen (Regel 6 gilt fuer Produktivcode).

use std::path::Path;
use std::time::{Duration, Instant};

use woa_radio::{Controller, Cues, PlaylistEntry, RadioShow};

const SR: u32 = 48_000;
const BLOCK: usize = 256;

/// Sinus mit `silence` Sekunden Stille davor.
fn write_tone(path: &Path, seconds: f64, silence: f64) {
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: SR,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut w = hound::WavWriter::create(path, spec).unwrap();
    let skip = (silence * f64::from(SR)) as usize;
    for i in 0..(seconds * f64::from(SR)) as usize {
        let v = if i < skip {
            0.0
        } else {
            0.25 * (i as f32 * 440.0 * std::f32::consts::TAU / SR as f32).sin()
        };
        w.write_sample(v).unwrap();
        w.write_sample(v).unwrap();
    }
    w.finalize().unwrap();
}

fn wait_until(c: &mut Controller, what: &str, mut done: impl FnMut(&Controller) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(120);
    while !done(c) {
        assert!(Instant::now() < deadline, "{what}: nicht fertig geworden");
        c.pump();
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn show_of(files: &[&Path]) -> RadioShow {
    RadioShow {
        crossfade_seconds: 0.0,
        bus_processing: false,
        mic_processing: false,
        playlist: files.iter().map(PlaylistEntry::new).collect(),
        ..RadioShow::default()
    }
}

#[test]
fn start_takes_the_preloaded_buffer_and_sounds_in_the_first_blocks() {
    let tmp = tempfile::tempdir().unwrap();
    let a = tmp.path().join("lang.wav");
    // Lange Datei: Dekodieren und Messen sind merklich, der Start soll es nicht mehr sein.
    write_tone(&a, 240.0, 0.0);
    let (mut c, mut m) = Controller::new(show_of(&[&a]), SR);

    wait_until(&mut c, "Vorab-Laden", |c| c.is_preloaded(0));
    assert!(!c.show().playlist[0].is_measured(), "noch nicht übernommen");

    c.start();
    assert!(
        c.last_load_preloaded(),
        "Start hat dekodiert statt zu übernehmen"
    );
    assert!(!c.is_preloaded(0), "Puffer wurde herausgenommen");
    assert!(c.show().playlist[0].is_measured(), "Messung übernommen");

    // Erstes Sample ungleich null innerhalb weniger Blöcke.
    let (mut l, mut r) = (vec![0.0f32; BLOCK], vec![0.0f32; BLOCK]);
    let mut first = None;
    for block in 0..64 {
        c.pump();
        m.process(&mut l, &mut r);
        if l.iter().any(|v| *v != 0.0) {
            first = Some(block);
            break;
        }
    }
    assert!(first.is_some(), "kein Ton nach 64 Blöcken");
}

#[test]
fn next_title_is_preloaded_while_the_first_plays() {
    let tmp = tempfile::tempdir().unwrap();
    let (a, b) = (tmp.path().join("a.wav"), tmp.path().join("b.wav"));
    write_tone(&a, 30.0, 0.0);
    write_tone(&b, 30.0, 0.0);
    let (mut c, mut m) = Controller::new(show_of(&[&a, &b]), SR);
    wait_until(&mut c, "Cue-Titel", |c| c.is_preloaded(0));
    c.start();
    // Der Mixer beginnt den ersten Titel; der zweite wird dann vorab geladen und ohne
    // Dekodieren im Control-Thread bereitgelegt.
    let (mut l, mut r) = (vec![0.0f32; BLOCK], vec![0.0f32; BLOCK]);
    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        c.pump();
        m.process(&mut l, &mut r);
        if c.last_load_preloaded() && c.show().playlist[1].is_measured() {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "zweiter Titel nicht vorab geladen"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn changed_file_is_not_taken_from_the_cache() {
    let tmp = tempfile::tempdir().unwrap();
    let a = tmp.path().join("a.wav");
    write_tone(&a, 5.0, 0.0);
    let (mut c, _m) = Controller::new(show_of(&[&a]), SR);
    wait_until(&mut c, "Vorab-Laden", |c| c.is_preloaded(0));
    // Datei ändert sich (andere Länge, damit der Stand sicher abweicht).
    write_tone(&a, 6.0, 0.0);
    c.start();
    assert!(!c.last_load_preloaded(), "veralteter Puffer wurde gespielt");
}

#[test]
fn deck_without_cues_takes_the_suggested_cue_in() {
    let tmp = tempfile::tempdir().unwrap();
    let a = tmp.path().join("stille.wav");
    write_tone(&a, 10.0, 2.0);
    let (mut c, _m) = Controller::new(show_of(&[]), SR);
    c.deck_load(0, a, Cues::default()).unwrap();
    let cues = c.show().decks[0].as_ref().unwrap().cues;
    assert!(
        cues.cue_in.is_some_and(|s| (1.5..=2.1).contains(&s)),
        "Cue-In aus der Stille: {:?}",
        cues.cue_in
    );
}
