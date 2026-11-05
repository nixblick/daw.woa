//! #324: Ein Cart duckt die Musik nur, solange er hörbar ist. Stille am Ende einer Cart-Datei
//! (typisch hinter Jingles) hielt die Musik bisher bis zum Dateiende unten. Ohne Audiogerät.

#![allow(clippy::unwrap_used, clippy::expect_used)] // Tests duerfen abbrechen (Regel 6 gilt fuer Produktivcode).

use std::path::Path;

use woa_radio::{Controller, Mixer, PlaylistEntry, RadioShow};

const SR: u32 = 48_000;
const BLOCK: usize = 256;

/// Schreibt `loud` Sekunden Gleichspannung `amp`, danach `quiet` Sekunden Stille.
fn write(path: &Path, loud: f64, quiet: f64, amp: f32) {
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: SR,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut w = hound::WavWriter::create(path, spec).unwrap();
    for _ in 0..(loud * f64::from(SR)) as usize * 2 {
        w.write_sample(amp).unwrap();
    }
    for _ in 0..(quiet * f64::from(SR)) as usize * 2 {
        w.write_sample(0.0f32).unwrap();
    }
    w.finalize().unwrap();
}

fn level_after(c: &mut Controller, m: &mut Mixer, seconds: f64) -> f32 {
    let (mut l, mut r) = (vec![0.0f32; BLOCK], vec![0.0f32; BLOCK]);
    let n = ((seconds * f64::from(SR)) as usize / BLOCK).max(1);
    for _ in 0..n {
        c.pump();
        m.process(&mut l, &mut r);
    }
    l.iter().fold(0.0f32, |p, v| p.max(v.abs()))
}

#[test]
fn silent_cart_tail_does_not_hold_the_music_down() {
    let tmp = tempfile::tempdir().unwrap();
    let music = tmp.path().join("musik.wav");
    let jingle = tmp.path().join("jingle.wav");
    write(&music, 20.0, 0.0, 0.25);
    write(&jingle, 1.0, 4.0, 0.1);
    let show = RadioShow {
        crossfade_seconds: 0.0,
        bus_processing: false,
        mic_processing: false,
        playlist: vec![PlaylistEntry::new(&music)],
        ..RadioShow::default()
    };
    let (mut c, mut m) = Controller::new(show, SR);
    c.measure_missing();
    c.assign_cart(0, Some(jingle)).unwrap();
    c.start();
    let free = level_after(&mut c, &mut m, 1.0);
    assert!(free > 0.2, "Musik läuft: {free}");
    c.trigger_cart(0);
    let ducked = level_after(&mut c, &mut m, 0.8);
    assert!(ducked < free * 0.7, "Cart duckt: {ducked} gegen {free}");
    // 1,5 s nach dem Ende des Hörbaren: Musik wieder oben, obwohl der Cart noch 2,7 s Stille hat.
    let after = level_after(&mut c, &mut m, 1.7);
    assert!(
        (after - free).abs() < 0.02,
        "Musik nach dem Jingle wieder oben: {after} gegen {free}"
    );
}
