//! Paket 17: die eingebauten Effekte. Messung statt Hören: jeder Effekt wird mit erzeugten
//! Testsignalen (Sinus, Rauschen, Impuls) gefüttert und seine Wirkung gemessen. Dazu Golden-File,
//! bit-genauer Bypass und jeder Process-Aufruf unter `assert_no_alloc`.

use std::path::PathBuf;

use assert_no_alloc::{AllocDisabler, assert_no_alloc};
use woa_dsp::effects::delay::delay_seconds;
use woa_dsp::fx::{BYPASS, FxContext, FxKind, FxNode, FxStereoNode};
use woa_dsp::{Node, ProcessBuffers, StereoBuffers, StereoNode};

#[global_allocator]
static ALLOC: AllocDisabler = AllocDisabler;

const SR: f32 = 48_000.0;
const BLOCK: usize = 128;

// Vorgaben je Effekt, Reihenfolge wie im Modell (`EffectKind::params`).
const EQ: [f64; 11] = [
    0.0, 1000.0, 0.0, 1.0, 120.0, 0.0, 8000.0, 0.0, 4000.0, 0.0, 1.0,
];
const COMPRESSOR: [f64; 5] = [-18.0, 2.0, 10.0, 120.0, 0.0];
const LIMITER: [f64; 2] = [-1.0, 50.0];
const GATE: [f64; 5] = [-45.0, 40.0, 1.0, 100.0, 150.0];
const DEESSER: [f64; 4] = [7000.0, -30.0, 4.0, 12.0];
const REVERB: [f64; 4] = [1.8, 15.0, 40.0, 25.0];
const DELAY: [f64; 6] = [350.0, 0.0, 35.0, 6000.0, 0.0, 30.0];
const CHORUS: [f64; 4] = [0.8, 3.0, 15.0, 50.0];
const SATURATION: [f64; 4] = [12.0, 0.0, 0.0, 100.0];
const DENOISE: [f64; 1] = [100.0];

fn all_kinds() -> Vec<(FxKind, Vec<f64>)> {
    vec![
        (FxKind::Eq, EQ.to_vec()),
        (FxKind::Compressor, COMPRESSOR.to_vec()),
        (FxKind::Limiter, LIMITER.to_vec()),
        (FxKind::Gate, GATE.to_vec()),
        (FxKind::DeEsser, DEESSER.to_vec()),
        (FxKind::Reverb, REVERB.to_vec()),
        (FxKind::Delay, DELAY.to_vec()),
        (FxKind::Chorus, CHORUS.to_vec()),
        (FxKind::Saturation, SATURATION.to_vec()),
        (FxKind::Denoise, DENOISE.to_vec()),
    ]
}

fn with(base: &[f64], changes: &[(usize, f64)]) -> Vec<f64> {
    let mut v = base.to_vec();
    for (i, x) in changes {
        v[*i] = *x;
    }
    v
}

fn sine(freq: f32, amp: f32, n: usize) -> Vec<f32> {
    (0..n)
        .map(|i| amp * (std::f32::consts::TAU * freq * i as f32 / SR).sin())
        .collect()
}

/// Gleichverteiltes Rauschen in -amp..amp, bei jedem Lauf gleich (lineare Kongruenz).
fn noise(amp: f32, n: usize, seed: u32) -> Vec<f32> {
    let mut s = seed;
    (0..n)
        .map(|_| {
            s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            amp * ((s >> 8) as f32 / (1u32 << 23) as f32 - 1.0)
        })
        .collect()
}

fn add(a: &[f32], b: &[f32]) -> Vec<f32> {
    a.iter().zip(b).map(|(x, y)| x + y).collect()
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|s| s * s).sum::<f32>() / x.len() as f32).sqrt()
}

fn db(x: f32) -> f32 {
    20.0 * x.max(1e-9).log10()
}

/// Amplitude der Frequenz `freq` im Signal (Goertzel, auf ganze Perioden bezogen).
fn amp_at(x: &[f32], freq: f32) -> f32 {
    let w = std::f64::consts::TAU * f64::from(freq) / f64::from(SR);
    let (mut re, mut im) = (0.0f64, 0.0f64);
    for (i, s) in x.iter().enumerate() {
        let a = w * i as f64;
        re += f64::from(*s) * a.cos();
        im -= f64::from(*s) * a.sin();
    }
    (2.0 * (re * re + im * im).sqrt() / x.len() as f64) as f32
}

fn run_mono_ctx(kind: FxKind, params: &[f64], input: &[f32], ctx: FxContext) -> Vec<f32> {
    let mut n = FxNode::with_context(kind, params, false, ctx);
    n.prepare(SR);
    let mut out = vec![0.0; input.len()];
    for (i, o) in input.chunks(BLOCK).zip(out.chunks_mut(BLOCK)) {
        let buffers = ProcessBuffers {
            input: i,
            output: o,
        };
        if ctx.offline {
            // Export rechnet im Aufrufer-Thread und darf beim ersten Mal Speicher anlegen.
            n.process(buffers);
        } else {
            assert_no_alloc(|| n.process(buffers));
        }
    }
    out
}

fn run_mono(kind: FxKind, params: &[f64], input: &[f32]) -> Vec<f32> {
    run_mono_ctx(kind, params, input, FxContext::default())
}

fn run_stereo(
    kind: FxKind,
    params: &[f64],
    l: &[f32],
    r: &[f32],
    ctx: FxContext,
) -> (Vec<f32>, Vec<f32>) {
    let mut n = FxStereoNode::with_context(kind, params, false, ctx);
    n.prepare(SR);
    let (mut ol, mut or) = (vec![0.0; l.len()], vec![0.0; l.len()]);
    for (((i, j), o), p) in l
        .chunks(BLOCK)
        .zip(r.chunks(BLOCK))
        .zip(ol.chunks_mut(BLOCK))
        .zip(or.chunks_mut(BLOCK))
    {
        assert_no_alloc(|| {
            n.process(StereoBuffers {
                in_left: i,
                in_right: j,
                out_left: o,
                out_right: p,
            })
        });
    }
    (ol, or)
}

fn offline() -> FxContext {
    FxContext {
        offline: true,
        ..FxContext::default()
    }
}

// ---------------------------------------------------------------------------------------------
// Bypass und Echtzeit

#[test]
fn bypass_is_bit_exact_for_every_effect() {
    let input = add(&sine(440.0, 0.3, 24_000), &noise(0.05, 24_000, 7));
    for (kind, params) in all_kinds() {
        // Von Anfang an umgangen.
        let mut n = FxNode::with_context(kind, &params, true, offline());
        n.prepare(SR);
        let mut out = vec![0.0; input.len()];
        for (i, o) in input.chunks(BLOCK).zip(out.chunks_mut(BLOCK)) {
            n.process(ProcessBuffers {
                input: i,
                output: o,
            });
        }
        assert!(
            out.iter()
                .zip(&input)
                .all(|(a, b)| a.to_bits() == b.to_bits()),
            "{kind:?}: bypass from the start is not bit-exact"
        );

        // Mitten im Betrieb umgangen: nach der Rampe (20 ms) bit-genau.
        let mut n = FxNode::with_context(kind, &params, false, offline());
        n.prepare(SR);
        let mut out = vec![0.0; input.len()];
        let half = input.len() / 2;
        for (i, o) in input[..half]
            .chunks(BLOCK)
            .zip(out[..half].chunks_mut(BLOCK))
        {
            n.process(ProcessBuffers {
                input: i,
                output: o,
            });
        }
        n.set_param(BYPASS, 1.0);
        for (i, o) in input[half..]
            .chunks(BLOCK)
            .zip(out[half..].chunks_mut(BLOCK))
        {
            n.process(ProcessBuffers {
                input: i,
                output: o,
            });
        }
        let settled = half + 2000;
        assert!(
            out[settled..]
                .iter()
                .zip(&input[settled..])
                .all(|(a, b)| a.to_bits() == b.to_bits()),
            "{kind:?}: bypass after the ramp is not bit-exact"
        );
        assert!(out.iter().all(|s| s.is_finite()), "{kind:?}: not finite");
    }
}

#[test]
fn no_allocation_in_the_audio_thread_for_every_effect() {
    let tone = add(&sine(1000.0, 0.25, 4096), &noise(0.05, 4096, 3));
    for (kind, params) in all_kinds() {
        // Live-Betrieb: Die Rauschunterdrückung rechnet in ihrem Arbeits-Thread.
        for stereo in [false, true] {
            let ctx = FxContext::default();
            let mut mono = FxNode::with_context(kind, &params, false, ctx);
            let mut st = FxStereoNode::with_context(kind, &params, false, ctx);
            mono.prepare(SR);
            st.prepare(SR);
            let (mut ol, mut or) = (vec![0.0; BLOCK], vec![0.0; BLOCK]);
            for (b, chunk) in tone.chunks(BLOCK).enumerate() {
                assert_no_alloc(|| {
                    // Jeder Regler wird bewegt, dazu Bypass ein und aus.
                    for (idx, value) in params.iter().enumerate() {
                        let v = *value as f32 * (1.0 + 0.01 * (b % 5) as f32);
                        if stereo {
                            st.set_param(idx as u32, v);
                        } else {
                            mono.set_param(idx as u32, v);
                        }
                    }
                    let by = if b % 16 == 8 { 1.0 } else { 0.0 };
                    if stereo {
                        st.set_param(BYPASS, by);
                        st.process(StereoBuffers {
                            in_left: chunk,
                            in_right: chunk,
                            out_left: &mut ol,
                            out_right: &mut or,
                        });
                    } else {
                        mono.set_param(BYPASS, by);
                        mono.process(ProcessBuffers {
                            input: chunk,
                            output: &mut ol,
                        });
                    }
                });
                assert!(
                    ol.iter().all(|s| s.is_finite()),
                    "{kind:?} stereo={stereo}: not finite"
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// EQ

#[test]
fn eq_shelves_and_second_bell_reach_their_gain() {
    let n = 48_000;
    let gain = |params: &[f64], freq: f32| {
        let tone = sine(freq, 0.1, n);
        let out = run_mono(FxKind::Eq, params, &tone);
        db(amp_at(&out[n / 2..], freq) / amp_at(&tone[n / 2..], freq))
    };
    // Tiefen +12 dB unter 200 Hz: weit darunter voll, weit darüber nichts.
    let low = with(&EQ, &[(4, 200.0), (5, 12.0)]);
    assert!(
        (gain(&low, 30.0) - 12.0).abs() < 1.0,
        "{}",
        gain(&low, 30.0)
    );
    assert!(gain(&low, 4000.0).abs() < 0.5);
    // Höhen -12 dB über 6 kHz.
    let high = with(&EQ, &[(6, 6000.0), (7, -12.0)]);
    assert!(
        (gain(&high, 20_000.0) + 12.0).abs() < 1.5,
        "{}",
        gain(&high, 20_000.0)
    );
    assert!(gain(&high, 200.0).abs() < 0.5);
    // Zweite Glocke +9 dB bei 4 kHz, erste bleibt flach.
    let bell2 = with(&EQ, &[(8, 4000.0), (9, 9.0)]);
    assert!((gain(&bell2, 4000.0) - 9.0).abs() < 0.5);
    assert!(gain(&bell2, 200.0).abs() < 0.5);
    // Alles flach: der EQ ändert nichts.
    let tone = sine(1234.0, 0.3, 4800);
    let flat = run_mono(FxKind::Eq, &EQ, &tone);
    assert!(flat.iter().zip(&tone).all(|(a, b)| (a - b).abs() < 1e-6));
}

// ---------------------------------------------------------------------------------------------
// Gate

#[test]
fn gate_closes_below_the_threshold_and_opens_above() {
    // 0,5 s laut (-6 dBFS), 0,5 s leise (-60 dBFS), Schwelle -40 dB, Absenkung 40 dB.
    let mut sig = sine(1000.0, 0.5, 24_000);
    sig.extend(sine(1000.0, 0.001, 24_000));
    let out = run_mono(
        FxKind::Gate,
        &with(&GATE, &[(0, -40.0), (1, 40.0), (3, 10.0), (4, 10.0)]),
        &sig,
    );
    let loud = db(rms(&out[2400..24_000]) / rms(&sig[2400..24_000]));
    assert!(loud.abs() < 0.5, "open gate changed level by {loud} dB");
    let quiet = db(rms(&out[40_000..]) / rms(&sig[40_000..]));
    assert!(
        (quiet + 40.0).abs() < 2.0,
        "closed gate lowered by {quiet} dB"
    );
    // Mit Haltezeit 300 ms bleibt das Tor nach dem Ende des lauten Teils noch offen.
    let held = run_mono(FxKind::Gate, &with(&GATE, &[(0, -40.0), (3, 300.0)]), &sig);
    let just_after =
        db(rms(&held[24_000 + 2400..24_000 + 9600]) / rms(&sig[24_000 + 2400..24_000 + 9600]));
    assert!(just_after > -3.0, "hold ignored: {just_after} dB");
}

// ---------------------------------------------------------------------------------------------
// De-Esser

#[test]
fn deesser_lowers_the_sibilance_band_and_leaves_the_voice() {
    let n = 48_000;
    let voice = sine(500.0, 0.2, n);
    let hiss = sine(7000.0, 0.3, n);
    let sig = add(&voice, &hiss);
    let out = run_mono(FxKind::DeEsser, &with(&DEESSER, &[(2, 10.0)]), &sig);
    let tail = n / 2..;
    let hiss_change = db(amp_at(&out[tail.clone()], 7000.0) / amp_at(&sig[tail.clone()], 7000.0));
    let voice_change = db(amp_at(&out[tail.clone()], 500.0) / amp_at(&sig[tail], 500.0));
    assert!(hiss_change < -6.0, "7 kHz only lowered by {hiss_change} dB");
    assert!(
        voice_change.abs() < 0.5,
        "500 Hz changed by {voice_change} dB"
    );
    // Ohne Zischen (nur Stimme) bleibt alles stehen.
    let calm = run_mono(FxKind::DeEsser, &DEESSER, &voice);
    let d = db(rms(&calm[n / 2..]) / rms(&voice[n / 2..]));
    assert!(d.abs() < 0.2, "{d}");
    // Höchste Absenkung begrenzt: 3 dB erlaubt, also nie mehr als etwa 3 dB.
    let limited = run_mono(
        FxKind::DeEsser,
        &with(&DEESSER, &[(2, 20.0), (3, 3.0)]),
        &hiss,
    );
    let l = db(amp_at(&limited[n / 2..], 7000.0) / amp_at(&hiss[n / 2..], 7000.0));
    assert!((-3.6..-2.4).contains(&l), "limit 3 dB, got {l}");
}

// ---------------------------------------------------------------------------------------------
// Reverb

/// Nachhallzeit RT60 aus dem Schroeder-Integral (-5 bis -25 dB, auf 60 dB hochgerechnet).
fn rt60(ir: &[f32]) -> f32 {
    let mut edc: Vec<f64> = ir.iter().map(|s| f64::from(*s).powi(2)).collect();
    for i in (0..edc.len() - 1).rev() {
        edc[i] += edc[i + 1];
    }
    let total = edc[0];
    let at = |level_db: f64| {
        edc.iter()
            .position(|e| 10.0 * (e / total).log10() < level_db)
            .expect("decays that far") as f32
            / SR
    };
    3.0 * (at(-25.0) - at(-5.0))
}

#[test]
fn reverb_decay_time_matches_the_setting() {
    let mut impulse = vec![0.0f32; 6 * 48_000];
    impulse[0] = 1.0;
    for decay in [0.8f64, 1.5, 3.0] {
        let ir = run_mono(
            FxKind::Reverb,
            &with(&REVERB, &[(0, decay), (1, 0.0), (2, 0.0), (3, 100.0)]),
            &impulse,
        );
        let measured = f64::from(rt60(&ir));
        assert!(
            (measured / decay - 1.0).abs() < 0.25,
            "decay {decay} s measured {measured} s"
        );
    }
    // Dämpfung verkürzt den Nachhall der Höhen: Gesamtnachhall wird kürzer.
    let open = rt60(&run_mono(
        FxKind::Reverb,
        &with(&REVERB, &[(0, 2.0), (1, 0.0), (2, 0.0), (3, 100.0)]),
        &impulse,
    ));
    let damped = rt60(&run_mono(
        FxKind::Reverb,
        &with(&REVERB, &[(0, 2.0), (1, 0.0), (2, 90.0), (3, 100.0)]),
        &impulse,
    ));
    assert!(damped < open, "damped {damped} s, open {open} s");
}

#[test]
fn reverb_predelay_and_mix_behave() {
    let mut impulse = vec![0.0f32; 24_000];
    impulse[0] = 1.0;
    // 50 ms Vorverzögerung: vorher ist der Hall still (nur Direktsignal bei Mix 100 weg).
    let ir = run_mono(
        FxKind::Reverb,
        &with(&REVERB, &[(1, 50.0), (3, 100.0)]),
        &impulse,
    );
    let early = ir[..2390].iter().fold(0.0f32, |m, s| m.max(s.abs()));
    assert!(early < 1e-6, "sound before the pre-delay: {early}");
    let later = ir[2400..6000].iter().fold(0.0f32, |m, s| m.max(s.abs()));
    assert!(later > 1e-3, "no reverb after the pre-delay");
    // Mix 0: unverändert.
    let dry = run_mono(FxKind::Reverb, &with(&REVERB, &[(3, 0.0)]), &impulse);
    assert!(dry.iter().zip(&impulse).all(|(a, b)| (a - b).abs() < 1e-7));
    // Weißes Rauschen: ungefähr gleicher Pegel hinten heraus (Normierung).
    let nz = noise(0.2, 96_000, 11);
    let wet = run_mono(FxKind::Reverb, &with(&REVERB, &[(1, 0.0), (3, 100.0)]), &nz);
    let ratio = db(rms(&wet[48_000..]) / rms(&nz[48_000..]));
    assert!(ratio.abs() < 4.0, "reverb level off by {ratio} dB");
}

// ---------------------------------------------------------------------------------------------
// Delay

fn echo_positions(x: &[f32], min: f32) -> Vec<usize> {
    x.iter()
        .enumerate()
        .filter(|(_, s)| s.abs() > min)
        .map(|(i, _)| i)
        .collect()
}

#[test]
fn delay_time_is_exact_on_an_impulse() {
    let mut impulse = vec![0.0f32; 60_000];
    impulse[0] = 1.0;
    // Frei: 250 ms = 12000 Samples, nur Echo (Mix 100), keine Rückkopplung.
    let free = with(&DELAY, &[(0, 250.0), (2, 0.0), (5, 100.0)]);
    let out = run_mono(FxKind::Delay, &free, &impulse);
    assert_eq!(echo_positions(&out, 1e-4), vec![12_000]);
    assert!((out[12_000] - 1.0).abs() < 1e-6);
    // Synchron zum Tempo 120: 1/4 = 500 ms, 1/8 punktiert = 375 ms, 1/16 = 125 ms.
    for (sync, expect) in [(2.0, 24_000usize), (5.0, 18_000), (6.0, 6_000)] {
        let p = with(&DELAY, &[(1, sync), (2, 0.0), (5, 100.0)]);
        let out = run_mono(FxKind::Delay, &p, &impulse);
        assert_eq!(echo_positions(&out, 1e-4), vec![expect], "sync {sync}");
    }
    // Anderes Tempo: bei 90 BPM ist 1/4 = 666,67 ms = 32000 Samples.
    let p = with(&DELAY, &[(1, 2.0), (2, 0.0), (5, 100.0)]);
    let ctx = FxContext {
        tempo_bpm: 90.0,
        ..FxContext::default()
    };
    let out = run_mono_ctx(FxKind::Delay, &p, &impulse, ctx);
    assert_eq!(echo_positions(&out, 1e-4), vec![32_000]);
    assert!((delay_seconds(0.0, 2.0, 90.0) - 2.0 / 3.0).abs() < 1e-6);
}

#[test]
fn delay_feedback_repeats_and_decays() {
    let mut impulse = vec![0.0f32; 60_000];
    impulse[0] = 1.0;
    let p = with(&DELAY, &[(0, 100.0), (2, 50.0), (3, 20_000.0), (5, 100.0)]);
    let out = run_mono(FxKind::Delay, &p, &impulse);
    let band = |c: usize| out[c * 4800 - 40..c * 4800 + 40].iter().sum::<f32>();
    let (e1, e2, e3) = (band(1), band(2), band(3));
    assert!((e1 - 1.0).abs() < 0.02, "{e1}");
    assert!((e2 - 0.5).abs() < 0.05, "{e2}");
    assert!((e3 - 0.25).abs() < 0.05, "{e3}");
}

#[test]
fn delay_ping_pong_alternates_the_sides() {
    let mut l = vec![0.0f32; 48_000];
    l[0] = 1.0;
    let r = vec![0.0f32; 48_000];
    let p = with(
        &DELAY,
        &[(0, 100.0), (2, 80.0), (3, 20_000.0), (4, 1.0), (5, 100.0)],
    );
    let (ol, or) = run_stereo(FxKind::Delay, &p, &l, &r, FxContext::default());
    let sum = |x: &[f32], c: usize| x[c * 4800 - 40..c * 4800 + 40].iter().sum::<f32>().abs();
    // Erstes Echo links (Mono-Summe 0,5), zweites rechts, drittes links.
    assert!(
        sum(&ol, 1) > 0.4 && sum(&or, 1) < 0.05,
        "1st: {} {}",
        sum(&ol, 1),
        sum(&or, 1)
    );
    assert!(
        sum(&or, 2) > 0.2 && sum(&ol, 2) < 0.05,
        "2nd: {} {}",
        sum(&ol, 2),
        sum(&or, 2)
    );
    assert!(
        sum(&ol, 3) > 0.1 && sum(&or, 3) < 0.05,
        "3rd: {} {}",
        sum(&ol, 3),
        sum(&or, 3)
    );
}

#[test]
fn delay_time_change_does_not_click() {
    let tone = sine(300.0, 0.4, 48_000);
    let mut n = FxNode::new(FxKind::Delay, &with(&DELAY, &[(5, 100.0), (2, 0.0)]), false);
    n.prepare(SR);
    let mut out = vec![0.0; tone.len()];
    for (b, (i, o)) in tone.chunks(BLOCK).zip(out.chunks_mut(BLOCK)).enumerate() {
        if b == 150 {
            n.set_param(0, 600.0);
        }
        n.process(ProcessBuffers {
            input: i,
            output: o,
        });
    }
    // 300 Hz bei 0,4: größter Schritt je Sample 0,4 * 2 pi * 300 / 48000 = 0,0157. Beim
    // Verschieben der Zeit zieht die Tonhöhe kurz, der Schritt bleibt klein (kein Sprung).
    let step = out
        .windows(2)
        .map(|w| (w[1] - w[0]).abs())
        .fold(0.0, f32::max);
    assert!(step < 0.15, "click of {step}");
}

// ---------------------------------------------------------------------------------------------
// Chorus

#[test]
fn chorus_without_depth_is_a_pure_delay() {
    let tone = add(&sine(440.0, 0.3, 9600), &noise(0.05, 9600, 5));
    // 10 ms = 480 Samples, Tiefe 0, Mix 100.
    let out = run_mono(
        FxKind::Chorus,
        &with(&CHORUS, &[(1, 0.0), (2, 10.0), (3, 100.0)]),
        &tone,
    );
    for i in 480..tone.len() {
        assert!(
            (out[i] - tone[i - 480]).abs() < 1e-5,
            "sample {i}: out {} tone {} tone-1 {} tone+1 {}",
            out[i],
            tone[i - 480],
            tone[i - 481],
            tone[i - 479]
        );
    }
}

#[test]
fn chorus_modulates_and_keeps_the_level() {
    let n = 96_000;
    let tone = sine(1000.0, 0.3, n);
    let plain = run_mono(
        FxKind::Chorus,
        &with(&CHORUS, &[(1, 0.0), (2, 15.0), (3, 100.0)]),
        &tone,
    );
    let chorus = run_mono(
        FxKind::Chorus,
        &with(&CHORUS, &[(0, 1.0), (1, 5.0), (3, 100.0)]),
        &tone,
    );
    // Die Verzögerung schwankt, also weicht das Signal merklich vom reinen Delay ab ...
    let diff = rms(&chorus[48_000..]
        .iter()
        .zip(&plain[48_000..])
        .map(|(a, b)| a - b)
        .collect::<Vec<_>>());
    assert!(diff > 0.05, "no modulation: {diff}");
    // ... ohne dass der Pegel wegbricht oder übersteuert.
    let level = db(rms(&chorus[48_000..]) / rms(&tone[48_000..]));
    assert!((-6.0..1.0).contains(&level), "level {level} dB");
    assert!(chorus.iter().all(|s| s.abs() < 0.4));
}

#[test]
fn chorus_stereo_voices_differ_between_the_channels() {
    let tone = sine(800.0, 0.3, 48_000);
    let (l, r) = run_stereo(FxKind::Chorus, &CHORUS, &tone, &tone, FxContext::default());
    let diff = rms(&l[24_000..]
        .iter()
        .zip(&r[24_000..])
        .map(|(a, b)| a - b)
        .collect::<Vec<_>>());
    assert!(diff > 0.01, "channels identical: {diff}");
}

// ---------------------------------------------------------------------------------------------
// Sättigung

#[test]
fn saturation_adds_harmonics_with_drive() {
    let n = 48_000;
    let tone = sine(1000.0, 0.3, n);
    let thd = |params: &[f64]| {
        let out = run_mono(FxKind::Saturation, params, &tone);
        let out = &out[n / 2..];
        let h1 = amp_at(out, 1000.0);
        (amp_at(out, 2000.0) / h1, amp_at(out, 3000.0) / h1)
    };
    let (_, h3_low) = thd(&with(&SATURATION, &[(0, 0.0)]));
    let (_, h3_high) = thd(&with(&SATURATION, &[(0, 24.0)]));
    assert!(
        h3_high > 10.0 * h3_low,
        "3rd harmonic {h3_low} -> {h3_high}"
    );
    assert!(h3_high > 0.05, "{h3_high}");
    // Weiche Kennlinie ist symmetrisch (kaum 2. Oberton), Röhre nicht.
    let (h2_soft, _) = thd(&with(&SATURATION, &[(0, 12.0), (1, 0.0)]));
    let (h2_tube, _) = thd(&with(&SATURATION, &[(0, 12.0), (1, 1.0)]));
    assert!(h2_soft < 0.002, "soft 2nd harmonic {h2_soft}");
    assert!(h2_tube > 0.02, "tube 2nd harmonic {h2_tube}");
    // Kein Gleichspannungsanteil trotz verschobenem Arbeitspunkt, nie über Vollaussteuerung.
    let tube = run_mono(
        FxKind::Saturation,
        &with(&SATURATION, &[(0, 18.0), (1, 1.0)]),
        &tone,
    );
    let dc = tube[n / 2..].iter().sum::<f32>() / (n / 2) as f32;
    assert!(dc.abs() < 0.01, "dc {dc}");
    for shape in [0.0, 1.0, 2.0] {
        let hot = run_mono(
            FxKind::Saturation,
            &with(&SATURATION, &[(0, 36.0), (1, shape)]),
            &sine(200.0, 1.0, 9600),
        );
        let peak = hot.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        // Weich und hart nie über Vollaussteuerung; die Röhre klingelt am Gleichstromfilter um bis zu 10 %.
        let limit = if shape == 1.0 { 1.12 } else { 1.0001 };
        assert!(peak <= limit, "shape {shape} overshoots: {peak}");
    }
}

// ---------------------------------------------------------------------------------------------
// Rauschunterdrückung

#[test]
fn denoise_lowers_steady_noise_and_runs_identically_live_and_offline() {
    let n = 48_000 * 2;
    let ctx = offline();
    // Ton mit Rauschen: das Rauschen abseits des Tons fällt deutlich, der Ton bleibt.
    let mixed = add(&sine(300.0, 0.1, n), &noise(0.02, n, 5));
    let cleaned = run_mono_ctx(FxKind::Denoise, &DENOISE, &mixed, ctx);
    let change = |f: f32| db(amp_at(&cleaned[n / 2..], f) / amp_at(&mixed[n / 2..], f));
    assert!(
        change(300.0).abs() < 1.0,
        "tone changed by {} dB",
        change(300.0)
    );
    for f in [6000.0, 10_000.0, 15_000.0] {
        assert!(change(f) < -12.0, "{f} Hz only lowered by {} dB", change(f));
    }

    // Live (Arbeits-Thread) gegen Export (Aufrufer-Thread): derselbe Klang, solange der
    // Arbeits-Thread rechtzeitig fertig wird. Wir geben ihm Echtzeit.
    let short = &mixed[..9600];
    let mut live = FxNode::new(FxKind::Denoise, &DENOISE, false);
    live.prepare(SR);
    let mut live_out = vec![0.0; short.len()];
    for (i, o) in short.chunks(480).zip(live_out.chunks_mut(480)) {
        live.process(ProcessBuffers {
            input: i,
            output: o,
        });
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(live.underruns(), 0, "worker thread was too slow");
    let inline = run_mono_ctx(FxKind::Denoise, &DENOISE, short, ctx);
    assert!(
        live_out
            .iter()
            .zip(&inline)
            .all(|(a, b)| (a - b).abs() < 1e-6),
        "live and offline differ"
    );
}

#[test]
fn denoise_delay_is_fixed_and_the_dry_path_matches_it() {
    // Stärke 0: nur der trockene Anteil, um genau die Latenz verzögert.
    let mut impulse = vec![0.0f32; 4800];
    impulse[100] = 1.0;
    let p = with(&DENOISE, &[(0, 0.0)]);
    let out = run_mono_ctx(FxKind::Denoise, &p, &impulse, offline());
    let lat = woa_dsp::effects::denoise::LATENCY;
    assert_eq!(echo_positions(&out, 1e-6), vec![100 + lat]);
    {
        // Tonstoß: Kantenlage von Eingang und Ausgang vergleichen.
        let n = 48_000;
        let mut burst = vec![0.0f32; n];
        let tone = sine(250.0, 0.2, 24_000);
        burst[12_000..36_000].copy_from_slice(&tone);
        let full = run_mono_ctx(FxKind::Denoise, &DENOISE, &burst, offline());
        let env = |x: &[f32], at: usize| x[at..at + 96].iter().fold(0.0f32, |m, s| m.max(s.abs()));
        let rise_in = (0..n - 96).find(|i| env(&burst, *i) > 0.1).unwrap();
        let rise_out = (0..n - 96).find(|i| env(&full, *i) > 0.1).unwrap();
        let fall_in = (20_000..n - 96).find(|i| env(&burst, *i) < 0.02).unwrap();
        let fall_out = (20_000..n - 96).find(|i| env(&full, *i) < 0.02).unwrap();
        // Der nasse Anteil kommt wirklich um LATENCY verspätet (Kanten eines Tonstoßes).
        let want = lat as i64;
        assert!(
            (rise_out as i64 - rise_in as i64 - want).abs() < 120,
            "rise lag {}",
            rise_out as i64 - rise_in as i64
        );
        assert!(
            (fall_out as i64 - fall_in as i64 - want).abs() < 200,
            "fall lag {}",
            fall_out as i64 - fall_in as i64
        );
    }
    // Nass- und Trockenanteil liegen auf demselben Sample: Ein 250-Hz-Ton (Periode 192 Samples)
    // mit Stärke 50 % behält seinen Pegel. Läge der nasse Anteil einen Block (480 Samples = 180°)
    // daneben, löschte er den trockenen fast aus.
    let n = 48_000;
    let mixed = add(&sine(250.0, 0.1, n), &noise(0.005, n, 17));
    let half = run_mono_ctx(
        FxKind::Denoise,
        &with(&DENOISE, &[(0, 50.0)]),
        &mixed,
        offline(),
    );
    let full = run_mono_ctx(FxKind::Denoise, &DENOISE, &mixed, offline());
    let lvl = |x: &[f32]| db(amp_at(&x[n / 2..], 250.0) / amp_at(&mixed[n / 2..], 250.0));
    assert!(lvl(&full).abs() < 1.5, "full: {} dB", lvl(&full));
    assert!(
        lvl(&half).abs() < 1.5,
        "50 %: {} dB (wet path not aligned with dry?)",
        lvl(&half)
    );
    // Zweite Quelle für die Ausrichtung: Phase des Tons im Ausgang gegen die Eingabe um LATENCY.
    let shifted = &mixed[..n - lat];
    let corr: f32 = full[lat..].iter().zip(shifted).map(|(a, b)| a * b).sum();
    let energy: f32 = shifted.iter().map(|b| b * b).sum();
    assert!(
        corr / energy > 0.8,
        "output is not the input delayed by {lat}: {}",
        corr / energy
    );

    // Andere Abtastrate als 48 kHz: Durchgang ohne Wirkung.
    let mut n = FxNode::with_context(FxKind::Denoise, &DENOISE, false, offline());
    n.prepare(44_100.0);
    let input = noise(0.1, 2000, 2);
    let mut out = vec![0.0; 2000];
    n.process(ProcessBuffers {
        input: &input,
        output: &mut out,
    });
    assert_eq!(out, input);
}

// ---------------------------------------------------------------------------------------------
// Golden-File

/// Testsignal: Sinus 220 Hz, Zischen 6,8 kHz, Rauschen, ein Impuls; zweite Hälfte leise.
fn golden_input() -> Vec<f32> {
    let n = 12_000;
    let mut x = add(&sine(220.0, 0.4, n), &sine(6800.0, 0.2, n));
    x = add(&x, &noise(0.05, n, 99));
    x[1000] += 0.8;
    for s in &mut x[n / 2..] {
        *s *= 0.02;
    }
    x
}

#[test]
fn golden_effects_render() {
    let input = golden_input();
    let presets: Vec<(FxKind, Vec<f64>)> = vec![
        // Sprache klar
        (
            FxKind::Eq,
            with(
                &EQ,
                &[
                    (0, 80.0),
                    (1, 3000.0),
                    (2, 3.0),
                    (4, 150.0),
                    (5, -2.0),
                    (6, 10_000.0),
                    (7, 2.0),
                ],
            ),
        ),
        (FxKind::Compressor, vec![-20.0, 3.0, 8.0, 120.0, 3.0]),
        (FxKind::Limiter, LIMITER.to_vec()),
        (FxKind::Gate, with(&GATE, &[(0, -40.0), (1, 30.0)])),
        (FxKind::DeEsser, DEESSER.to_vec()),
        (FxKind::Reverb, REVERB.to_vec()),
        (FxKind::Delay, with(&DELAY, &[(0, 60.0)])),
        (FxKind::Chorus, CHORUS.to_vec()),
        (FxKind::Saturation, SATURATION.to_vec()),
    ];
    let mut rendered = Vec::new();
    for (kind, params) in &presets {
        let out = run_mono(*kind, params, &input);
        assert!(out.iter().all(|s| s.is_finite()), "{kind:?}");
        rendered.extend(out);
    }
    golden("effects_p17_chain.wav", &rendered);
}

fn golden(name: &str, rendered: &[f32]) {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/golden")
        .join(name);
    if std::env::var_os("WOA_BLESS").is_some() {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: SR as u32,
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
    assert_eq!(r.spec().sample_rate, SR as u32);
    let g: Vec<f32> = r.samples::<f32>().map(|s| s.expect("sample")).collect();
    assert_eq!(g.len(), rendered.len());
    let max_err = g
        .iter()
        .zip(rendered)
        .map(|(a, b)| (a - b).abs())
        .fold(0.0f32, f32::max);
    assert!(max_err < 1e-3, "{name}: max deviation {max_err}");
}
