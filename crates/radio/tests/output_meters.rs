//! Pegel je Ausgang (Notebook, Stream) für die LED-Ketten der Pegelkarte. Ohne Audiogerät,
//! ohne Ton: bekannter Pegel rein, gemessene Spitze raus.

use std::path::{Path, PathBuf};

use assert_no_alloc::{AllocDisabler, assert_no_alloc};

use woa_radio::{Controller, Mixer, PlaylistEntry, RadioShow};

#[global_allocator]
static ALLOC: AllocDisabler = AllocDisabler;

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

fn step(c: &mut Controller, m: &mut Mixer, enc: &mut Option<rtrb::Consumer<f32>>, seconds: f64) {
    let mut l = vec![0.0f32; BLOCK];
    let mut r = vec![0.0f32; BLOCK];
    for _ in 0..(seconds * f64::from(SR)) as usize / BLOCK {
        c.pump();
        assert_no_alloc(|| m.process(&mut l, &mut r));
        if let Some(rx) = enc.as_mut() {
            while rx.pop().is_ok() {}
        }
        c.pump();
    }
}

fn db(x: f32) -> f32 {
    20.0 * x.max(1e-9).log10()
}

#[test]
fn output_peaks_follow_their_fader_and_mute() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut c, mut m) = setup(tmp.path());
    // Ohne Encoder geht nichts hinaus: Stream bleibt dunkel.
    let mut enc = None;
    c.start();
    step(&mut c, &mut m, &mut enc, 0.5);
    let st = c.status().clone();
    assert_eq!(
        st.take_output_peaks()[1],
        0.0,
        "ohne Encoder kein Stream-Pegel"
    );
    let (tx, rx) = rtrb::RingBuffer::new(48_000);
    c.set_stream_output(Some(tx));
    enc = Some(rx);
    step(&mut c, &mut m, &mut enc, 1.0);
    let _ = st.take_output_peaks();
    let _ = st.take_peak();
    step(&mut c, &mut m, &mut enc, 0.5);
    let master = st.take_peak();
    let [notebook, stream] = st.take_output_peaks();
    assert!(master > 0.1, "Master hat Pegel: {master}");
    assert!(
        (db(notebook) - db(master)).abs() < 0.5,
        "Notebook {notebook} ~ Master {master}"
    );
    assert!(
        (db(stream) - db(master)).abs() < 0.5,
        "Stream {stream} ~ Master {master}"
    );
    assert_eq!(st.take_output_peaks(), [0.0; 2], "Abholen setzt zurück");

    // Notebook -12 dB, Stream stumm: gemessen wird hinter Regler und Stumm.
    c.set_output(
        woa_radio::show::Output::Monitor,
        woa_radio::show::OutLevel {
            db: -12.0,
            mute: false,
        },
    );
    c.set_output(
        woa_radio::show::Output::Stream,
        woa_radio::show::OutLevel {
            db: 0.0,
            mute: true,
        },
    );
    step(&mut c, &mut m, &mut enc, 0.5);
    let _ = st.take_output_peaks();
    let _ = st.take_peak();
    step(&mut c, &mut m, &mut enc, 0.5);
    let master = st.take_peak();
    let [notebook, stream] = st.take_output_peaks();
    assert!(
        (db(notebook) - (db(master) - 12.0)).abs() < 0.5,
        "Notebook {} dB, Master {} dB",
        db(notebook),
        db(master)
    );
    assert_eq!(stream, 0.0, "Stream stumm");
}
