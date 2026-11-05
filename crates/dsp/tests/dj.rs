//! DJ-Paket: Deck-Leser (Tempo, Keylock, Scratch, Loop), EQ mit Kill, Filter, Crossfader-Kurve.
//! Offline und ohne Ton; jeder Process-Aufruf läuft unter `assert_no_alloc`.

#![allow(clippy::unwrap_used, clippy::expect_used)] // Tests duerfen abbrechen (Regel 6 gilt fuer Produktivcode).

use std::path::PathBuf;

use assert_no_alloc::{AllocDisabler, assert_no_alloc};
use woa_dsp::dj::{
    Curve, DeckReader, DjEq, DjFilter, ReadCtl, eq_gain, headphone_gains, xfader_gains,
};

#[cfg(debug_assertions)]
#[global_allocator]
static A: AllocDisabler = AllocDisabler;

const SR: u32 = 48_000;

fn sine(hz: f32, secs: f32) -> Vec<f32> {
    (0..(secs * SR as f32) as usize)
        .map(|i| 0.5 * (i as f32 * hz * std::f32::consts::TAU / SR as f32).sin())
        .collect()
}

/// Frequenz aus den Nulldurchgängen (steigend) im Bereich.
fn freq_of(x: &[f32]) -> f32 {
    let mut n = 0usize;
    let (mut first, mut last) = (None, 0usize);
    for i in 1..x.len() {
        if x[i - 1] < 0.0 && x[i] >= 0.0 {
            n += 1;
            first.get_or_insert(i);
            last = i;
        }
    }
    let Some(f) = first else { return 0.0 };
    if n < 2 {
        return 0.0;
    }
    (n - 1) as f32 * SR as f32 / (last - f) as f32
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
}

fn render(reader: &mut DeckReader, src: &[f32], ctl: &ReadCtl, frames: usize) -> Vec<f32> {
    let mut out = vec![0.0f32; frames];
    assert_no_alloc(|| {
        for o in out.iter_mut() {
            let (l, _r) = reader.next(src, src, ctl);
            *o = l;
        }
    });
    out
}

#[test]
fn normal_speed_is_bit_exact_in_both_modes() {
    let src = sine(440.0, 1.0);
    for keylock in [false, true] {
        let mut r = DeckReader::new();
        r.seek(1000.0);
        let ctl = ReadCtl {
            speed: 1.0,
            keylock,
            ..ReadCtl::whole(src.len())
        };
        let out = render(&mut r, &src, &ctl, 20_000);
        let err = out
            .iter()
            .zip(&src[1000..])
            .map(|(a, b)| (a - b).abs())
            .fold(0.0f32, f32::max);
        assert!(err < 1e-5, "keylock {keylock}: {err}");
        assert!((r.position() - 21_000.0).abs() < 1e-6);
    }
}

#[test]
fn tempo_without_keylock_shifts_pitch() {
    let src = sine(440.0, 2.0);
    let mut r = DeckReader::new();
    let ctl = ReadCtl {
        speed: 1.08,
        keylock: false,
        ..ReadCtl::whole(src.len())
    };
    let out = render(&mut r, &src, &ctl, 48_000);
    let f = freq_of(&out[1000..40_000]);
    assert!((f - 440.0 * 1.08).abs() < 2.0, "{f}");
    assert!((r.position() - 48_000.0 * 1.08).abs() < 1.0);
}

#[test]
fn keylock_keeps_pitch_and_moves_at_tempo() {
    let src = sine(440.0, 3.0);
    for speed in [0.92, 1.05, 1.08] {
        let mut r = DeckReader::new();
        let ctl = ReadCtl {
            speed,
            keylock: true,
            ..ReadCtl::whole(src.len())
        };
        let out = render(&mut r, &src, &ctl, 96_000);
        let f = freq_of(&out[4000..90_000]);
        assert!((f - 440.0).abs() < 3.0, "speed {speed}: {f}");
        assert!((r.position() - 96_000.0 * speed).abs() < 2.0);
        // Kein Einbruch: der Pegel bleibt nahe dem der Quelle.
        let level = rms(&out[4000..90_000]);
        assert!(
            (level - rms(&src)).abs() < 0.05,
            "speed {speed}: rms {level}"
        );
    }
}

#[test]
fn scratch_backwards_reads_backwards() {
    let ramp: Vec<f32> = (0..10_000).map(|i| i as f32 / 10_000.0).collect();
    let mut r = DeckReader::new();
    r.seek(5000.0);
    let ctl = ReadCtl {
        speed: -1.0,
        keylock: true,
        ..ReadCtl::whole(ramp.len())
    };
    let out = render(&mut r, &ramp, &ctl, 100);
    assert!(out.windows(2).all(|w| w[1] < w[0]), "rückwärts fallend");
    assert!((r.position() - 4900.0).abs() < 1e-6);
}

#[test]
fn loop_wraps_and_end_stops() {
    let src = sine(100.0, 1.0);
    let mut r = DeckReader::new();
    r.seek(1000.0);
    let ctl = ReadCtl {
        speed: 1.0,
        keylock: false,
        looped: Some((1000.0, 2000.0)),
        ..ReadCtl::whole(src.len())
    };
    let _ = render(&mut r, &src, &ctl, 2500);
    assert!((r.position() - 1500.0).abs() < 1e-6, "{}", r.position());
    let ctl = ReadCtl {
        speed: 1.0,
        keylock: false,
        ..ReadCtl::whole(src.len())
    };
    r.seek(47_990.0);
    let out = render(&mut r, &src, &ctl, 100);
    assert!(r.at_end(&ctl));
    assert!(out[20..].iter().all(|v| *v == 0.0));
}

#[test]
fn eq_is_transparent_at_zero_and_kill_removes_the_band() {
    let mut eq = DjEq::new(SR);
    eq.set(1.0, 1.0, 1.0);
    eq.snap();
    let low = sine(60.0, 1.0);
    let mut out = vec![0.0f32; low.len()];
    assert_no_alloc(|| {
        for (i, x) in low.iter().enumerate() {
            out[i] = eq.process(*x, *x).0;
        }
    });
    let err = out
        .iter()
        .zip(&low)
        .map(|(a, b)| (a - b).abs())
        .fold(0.0f32, f32::max);
    assert!(err < 1e-5, "transparent: {err}");
    // Kill Lo: 60 Hz verschwindet (mehr als 30 dB leiser), 5 kHz bleibt.
    eq.set(0.0, 1.0, 1.0);
    eq.snap();
    let mut out = vec![0.0f32; low.len()];
    for (i, x) in low.iter().enumerate() {
        out[i] = eq.process(*x, *x).0;
    }
    assert!(
        rms(&out[24_000..]) < rms(&low) * 0.03,
        "{}",
        rms(&out[24_000..])
    );
    let hi = sine(5000.0, 1.0);
    let mut out = vec![0.0f32; hi.len()];
    for (i, x) in hi.iter().enumerate() {
        out[i] = eq.process(*x, *x).0;
    }
    assert!((rms(&out[24_000..]) - rms(&hi)).abs() < rms(&hi) * 0.1);
}

#[test]
fn eq_knob_maps_center_to_unity_and_left_to_kill() {
    assert_eq!(eq_gain(0.0), 1.0);
    assert_eq!(eq_gain(-1.0), 0.0);
    assert!((eq_gain(1.0) - 10f32.powf(6.0 / 20.0)).abs() < 1e-4);
    assert!(eq_gain(-0.5) < 1.0 && eq_gain(-0.5) > 0.0);
}

#[test]
fn filter_lowpass_left_highpass_right_center_bypass() {
    let mixed: Vec<f32> = sine(80.0, 1.0)
        .iter()
        .zip(sine(6000.0, 1.0))
        .map(|(a, b)| a + b)
        .collect();
    let run = |v: f32| {
        let mut f = DjFilter::new(SR);
        f.set(v);
        f.snap();
        let mut out = vec![0.0f32; mixed.len()];
        assert_no_alloc(|| {
            for (i, x) in mixed.iter().enumerate() {
                out[i] = f.process(*x, *x).0;
            }
        });
        out
    };
    let center = run(0.0);
    assert_eq!(center, mixed, "Mitte = unverändert");
    let lp = run(-0.8);
    let hp = run(0.8);
    let tail = 24_000;
    // Tiefpass: der Rest ist fast nur noch 80 Hz.
    assert!((rms(&lp[tail..]) - rms(&sine(80.0, 0.5))).abs() < 0.06);
    assert!((rms(&hp[tail..]) - rms(&sine(6000.0, 0.5))).abs() < 0.06);
}

#[test]
fn crossfader_curves() {
    // weich: konstante Leistung, Mitte -3 dB je Seite.
    let (a, b) = xfader_gains(0.5, Curve::Soft);
    assert!((a - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-4 && (a - b).abs() < 1e-6);
    assert_eq!(xfader_gains(0.0, Curve::Soft), (1.0, 0.0));
    assert_eq!(xfader_gains(1.0, Curve::Soft), (0.0, 1.0));
    // hart: beide voll bis kurz vor den Rand, dann sofort weg.
    assert_eq!(xfader_gains(0.5, Curve::Hard), (1.0, 1.0));
    assert_eq!(xfader_gains(0.0, Curve::Hard), (1.0, 0.0));
    assert_eq!(xfader_gains(1.0, Curve::Hard), (0.0, 1.0));
    assert_eq!(xfader_gains(0.9, Curve::Hard).0, 1.0);
}

#[test]
fn headphone_mix_ends() {
    assert_eq!(headphone_gains(0.0), (1.0, 0.0));
    assert_eq!(headphone_gains(1.0), (0.0, 1.0));
}

// ---- Golden Files ---------------------------------------------------------------------------

fn golden_input() -> Vec<f32> {
    // Akkord mit Bass und Klick alle 0,5 s: hat Tief-, Mittel- und Hochanteil.
    (0..SR as usize)
        .map(|i| {
            let t = i as f32 / SR as f32;
            let tau = std::f32::consts::TAU;
            let click = if i % 24_000 < 200 {
                0.3 * (-((i % 24_000) as f32) / 40.0).exp()
            } else {
                0.0
            };
            0.25 * (tau * 55.0 * t).sin()
                + 0.2 * (tau * 440.0 * t).sin()
                + 0.1 * (tau * 3520.0 * t).sin()
                + click
        })
        .collect()
}

fn golden(name: &str, rendered: &[f32]) {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/golden")
        .join(name);
    if std::env::var_os("WOA_BLESS").is_some() {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: SR,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };
        let mut w = hound::WavWriter::create(&path, spec).expect("create golden");
        for s in rendered {
            w.write_sample(*s).expect("write");
        }
        w.finalize().expect("finalize");
    }
    let mut r = hound::WavReader::open(&path).expect("golden exists (WOA_BLESS=1 to create)");
    assert_eq!(r.spec().sample_rate, SR);
    let g: Vec<f32> = r.samples::<f32>().map(|s| s.expect("sample")).collect();
    assert_eq!(g.len(), rendered.len());
    let max_err = g
        .iter()
        .zip(rendered)
        .map(|(a, b)| (a - b).abs())
        .fold(0.0f32, f32::max);
    assert!(max_err < 1e-3, "{name}: max deviation {max_err}");
}

/// EQ (Hi +3 dB, Mid −6 dB, Lo Kill) und danach Filter (Tiefpass halb) als eine Kette.
#[test]
fn golden_dj_eq_filter() {
    let input = golden_input();
    let mut eq = DjEq::new(SR);
    eq.set(0.0, eq_gain(-0.2), eq_gain(0.5));
    eq.snap();
    let mut f = DjFilter::new(SR);
    f.set(-0.5);
    f.snap();
    let mut out = vec![0.0f32; input.len()];
    assert_no_alloc(|| {
        for (i, x) in input.iter().enumerate() {
            let (l, r) = eq.process(*x, *x);
            out[i] = f.process(l, r).0;
        }
    });
    golden("dj_eq_filter_1s.wav", &out);
}

/// Crossfader „hart“ und „weich“ über eine Fahrt von A nach B (A = Eingang, B = invertiert).
#[test]
fn golden_dj_crossfader() {
    let input = golden_input();
    let n = input.len();
    let mut out = Vec::with_capacity(2 * n);
    for curve in [Curve::Soft, Curve::Hard] {
        for (i, x) in input.iter().enumerate() {
            let (ga, gb) = xfader_gains(i as f32 / n as f32, curve);
            out.push(ga * x - gb * 0.5 * x);
        }
    }
    golden("dj_crossfader_2x1s.wav", &out);
}

/// Time-Stretch mit Keylock bei +6 % und ohne Keylock bei −4 %.
#[test]
fn golden_dj_stretch() {
    let input = golden_input();
    let mut out = Vec::new();
    for (speed, keylock) in [(1.06, true), (0.96, false)] {
        let mut r = DeckReader::new();
        let ctl = ReadCtl {
            speed,
            keylock,
            ..ReadCtl::whole(input.len())
        };
        out.extend(render(&mut r, &input, &ctl, 40_000));
    }
    golden("dj_stretch_2x40k.wav", &out);
}
