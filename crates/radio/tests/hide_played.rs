//! Gespielte ausblenden (#215) und Pegel je Zweig für die LED-Meter (#216). Ohne Audiogerät,
//! ohne Ton, nur in einem Wegwerf-Ordner.

use std::path::{Path, PathBuf};

use woa_radio::{Controller, Mixer, PlaylistEntry, RadioShow};

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

fn setup(dir: &Path) -> (Controller, Mixer) {
    let paths: Vec<PathBuf> = ["eins", "zwei", "drei"]
        .iter()
        .map(|n| {
            let p = dir.join(format!("{n}.wav"));
            write_tone(&p, 3.0);
            p
        })
        .collect();
    let show = RadioShow {
        crossfade_seconds: 0.0,
        bus_processing: false,
        mic_processing: false,
        playlist: paths.iter().map(PlaylistEntry::new).collect(),
        ..RadioShow::default()
    };
    let (mut c, m) = Controller::new(show, SR);
    c.measure_missing();
    (c, m)
}

fn step(c: &mut Controller, m: &mut Mixer, seconds: f64) {
    let mut l = vec![0.0f32; BLOCK];
    let mut r = vec![0.0f32; BLOCK];
    for _ in 0..(seconds * f64::from(SR)) as usize / BLOCK {
        c.pump();
        m.process(&mut l, &mut r);
        c.pump();
    }
}

#[test]
fn played_titles_are_before_the_open_ones_and_not_in_the_rest_time() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut c, mut m) = setup(tmp.path());
    assert_eq!(c.first_open_index(), 0, "vor dem Start ist nichts gespielt");
    assert!(
        (c.remaining_total() - 9.0).abs() < 0.1,
        "{}",
        c.remaining_total()
    );
    c.start();
    step(&mut c, &mut m, 4.0);
    // Titel 1 ist gespielt, Titel 2 läuft seit etwa 1 s: offen sind 2 s + 3 s.
    assert_eq!(c.first_open_index(), 1);
    let rest = c.remaining_total();
    assert!((rest - 5.0).abs() < 0.3, "Restzeit nur der offenen: {rest}");
}

#[test]
fn hide_played_is_saved_per_show_and_off_by_default() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("radio.json");
    assert!(!RadioShow::default().hide_played, "Vorgabe aus");
    let (mut c, _m) = setup(tmp.path());
    c.set_hide_played(true);
    c.show().save(&path).unwrap();
    assert!(
        RadioShow::load(&path).unwrap().hide_played,
        "geladen wie gespeichert"
    );
    c.set_hide_played(false);
    c.show().save(&path).unwrap();
    assert!(!RadioShow::load(&path).unwrap().hide_played);
    assert!(
        !std::fs::read_to_string(&path)
            .unwrap()
            .contains("hide_played"),
        "aus steht nicht in der Datei (alte Dateien bleiben gleich)"
    );
}

#[test]
fn bus_peaks_show_the_playlist_and_the_master() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut c, mut m) = setup(tmp.path());
    c.start();
    step(&mut c, &mut m, 1.0);
    let st = c.status();
    let [music, carts, decks] = st.take_bus_peaks();
    assert!(music > 0.1, "Playlist hat Pegel: {music}");
    assert_eq!((carts, decks), (0.0, 0.0), "Carts und Decks still");
    assert!(st.take_peak() > 0.1, "Master hat Pegel");
    assert_eq!(st.take_bus_peaks(), [0.0; 3], "Abholen setzt zurück");
}
