//! Misst Dekodieren und Analyse je Datei: `cargo run --release -p woa-library --example analyse_zeit -- <dateien>`.
use std::time::Instant;

fn main() {
    for p in std::env::args().skip(1) {
        let path = std::path::Path::new(&p);
        let t = Instant::now();
        let d = match woa_engine::decode::decode_file(path) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("{p}: {e}");
                continue;
            }
        };
        let dec = t.elapsed();
        let secs = d.frames() as f64 / f64::from(d.rate.max(1));
        let (l, r) = d.stereo();
        use woa_dsp::analysis::{loudness, track};
        let mono: Vec<f32> = l.iter().zip(&r).map(|(a, b)| 0.5 * (a + b)).collect();
        let part = |name: &str, f: &dyn Fn()| {
            let t = Instant::now();
            f();
            print!("{name} {} ms · ", t.elapsed().as_millis());
        };
        part("LUFS", &|| {
            std::hint::black_box(loudness::integrated_lufs(&l, &r, d.rate));
        });
        part("True Peak", &|| {
            std::hint::black_box(loudness::true_peak_dbtp(&l, &r));
        });
        part("BPM", &|| {
            std::hint::black_box(track::detect_bpm(&mono, d.rate));
        });
        part("Tonart", &|| {
            std::hint::black_box(track::detect_key(&mono, d.rate));
        });
        part("Stille", &|| {
            std::hint::black_box(track::detect_silence(&l, &r, d.rate, -50.0));
        });
        part("Cues", &|| {
            std::hint::black_box(track::suggest_cues(&l, &r, d.rate));
        });
        println!();
        let t = Instant::now();
        let _ = track::analyze(&l, &r, d.rate);
        let ana = t.elapsed();
        println!(
            "{secs:7.1} s Audio, {} Hz: dekodieren {:6} ms, analysieren {:6} ms  {p}",
            d.rate,
            dec.as_millis(),
            ana.as_millis()
        );
    }
}
