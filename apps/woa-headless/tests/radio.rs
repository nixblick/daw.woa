//! `woa-headless radio-render`: Playlist mit Crossfades offline rendern.
//! Länge = Summe der Titel − Überblendungen, Lautheit −16 LUFS ± 1 LU.

use assert_cmd::Command;
use predicates::str::contains;

const SR: u32 = 48_000;

fn tone(path: &std::path::Path, seconds: u32, freq: f32, amp: f32) {
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: SR,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut w = hound::WavWriter::create(path, spec).unwrap();
    for i in 0..seconds * SR {
        let v = (i as f32 * freq * std::f32::consts::TAU / SR as f32).sin() * amp;
        let s = (v * 32_767.0) as i16;
        w.write_sample(s).unwrap();
        w.write_sample(s).unwrap();
    }
    w.finalize().unwrap();
}

#[test]
fn radio_render_length_and_loudness() {
    let tmp = tempfile::tempdir().unwrap();
    // Very different levels: quiet, loud, medium. Auto-gain brings all to −16 LUFS.
    let tracks = [(10, 330.0, 0.03), (8, 440.0, 0.5), (9, 523.0, 0.12)];
    let mut list = Vec::new();
    for (i, (secs, f, a)) in tracks.iter().enumerate() {
        let p = tmp.path().join(format!("titel{i}.wav"));
        tone(&p, *secs, *f, *a);
        list.push(serde_json::json!({ "path": p }));
    }
    let radio = tmp.path().join("radio.json");
    let json = serde_json::json!({
        "version": 1,
        "crossfade_seconds": 3.0,
        "playlist": list,
    });
    std::fs::write(&radio, json.to_string()).unwrap();
    let out = tmp.path().join("sendung.wav");

    Command::cargo_bin("woa-headless")
        .unwrap()
        .args(["radio-render"])
        .arg(&radio)
        .arg(&out)
        .assert()
        .success()
        .stdout(contains("radio-render: 3 Titel"));

    let (l, r, rate) = woa_engine::read_wav_stereo(&out).unwrap();
    assert_eq!(rate, SR);
    let expect = ((10 + 8 + 9) - 3 - 3) * SR as usize;
    assert_eq!(l.len(), expect, "sum minus two crossfades");
    let lufs = woa_engine::loudness::integrated_lufs(&l, &r, SR).unwrap();
    assert!((lufs + 16.0).abs() <= 1.0, "{lufs} LUFS");
    let peak = l.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    assert!(peak < 1.0, "no clipping: {peak}");

    // Unknown file and newer format version fail cleanly.
    Command::cargo_bin("woa-headless")
        .unwrap()
        .args(["radio-render", "/nicht/da.json"])
        .arg(&out)
        .assert()
        .failure();
    std::fs::write(&radio, r#"{"version": 9}"#).unwrap();
    Command::cargo_bin("woa-headless")
        .unwrap()
        .args(["radio-render"])
        .arg(&radio)
        .arg(&out)
        .assert()
        .failure()
        .stderr(contains("Version 9"));
}
