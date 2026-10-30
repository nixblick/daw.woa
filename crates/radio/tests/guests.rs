//! Gäste im Sendemixer: Sendung enthält alle, der Abhörausgang trägt auf Wunsch den Rückweg
//! eines Gastes (Mix-Minus). Offline, ohne Audiogerät.

use assert_no_alloc::{AllocDisabler, assert_no_alloc};
use woa_guests::source::SineGen;
use woa_radio::{Controller, RadioShow};

#[global_allocator]
static ALLOC: AllocDisabler = AllocDisabler;

const RATE: u32 = 48_000;
const BLOCK: usize = 480;

fn tone(x: &[f32], freq: f32) -> f32 {
    let w = std::f64::consts::TAU * f64::from(freq) / f64::from(RATE);
    let (mut re, mut im) = (0.0f64, 0.0f64);
    for (i, &v) in x.iter().enumerate() {
        re += f64::from(v) * (w * i as f64).cos();
        im += f64::from(v) * (w * i as f64).sin();
    }
    (2.0 * (re * re + im * im).sqrt() / x.len() as f64) as f32
}

fn db(x: f32) -> f32 {
    20.0 * x.max(1e-12).log10()
}

#[test]
fn monitor_output_is_the_return_of_the_chosen_guest() {
    let (mut c, mut mixer) = Controller::new(RadioShow::default(), RATE);
    c.set_processing(false, false);
    let (_, mut fa) = c.guests().add_external("A").unwrap();
    let (_, mut fb) = c.guests().add_external("B").unwrap();
    let mut ga = SineGen::new(440.0, RATE, 0.3);
    let mut gb = SineGen::new(660.0, RATE, 0.3);
    let mut buf = vec![0.0f32; BLOCK * 2];
    let mut l = vec![0.0f32; BLOCK];
    let mut r = vec![0.0f32; BLOCK];
    let mut feed = |fa: &mut woa_guests::GuestFeed, fb: &mut woa_guests::GuestFeed| {
        ga.fill(&mut buf);
        fa.push(&buf);
        gb.fill(&mut buf);
        fb.push(&buf);
    };
    for _ in 0..7 {
        feed(&mut fa, &mut fb);
    }
    let mut run = |mixer: &mut woa_radio::Mixer,
                   fa: &mut woa_guests::GuestFeed,
                   fb: &mut woa_guests::GuestFeed,
                   blocks: usize| {
        let mut out = Vec::new();
        for _ in 0..blocks {
            feed(fa, fb);
            assert_no_alloc(|| mixer.process(&mut l, &mut r));
            out.extend_from_slice(&l);
        }
        out
    };
    // Normaler Abhörmix: beide Gäste.
    let mix = run(&mut mixer, &mut fa, &mut fb, 150);
    let mix = &mix[24_000..];
    assert!(db(tone(mix, 440.0)) > -12.0 && db(tone(mix, 660.0)) > -12.0);

    // Rückweg von Gast 1 (Kanal 0): B hörbar, A unter -90 dB.
    c.guests().set_monitor(Some(0));
    let ret = run(&mut mixer, &mut fa, &mut fb, 150);
    let ret = &ret[24_000..];
    assert!(db(tone(ret, 660.0)) > -12.0, "B fehlt im Rückweg");
    assert!(db(tone(ret, 440.0)) < -90.0, "A hört sich selbst");

    // Zurück zum normalen Abhörmix.
    c.guests().set_monitor(None);
    let back = run(&mut mixer, &mut fa, &mut fb, 150);
    let back = &back[24_000..];
    assert!(db(tone(back, 440.0)) > -12.0);
}
