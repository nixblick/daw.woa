//! Selbstprüfung des CLAP-Hosts mit dem Test-Plugin (gebaut mit `clack-plugin`), ohne Ton:
//! alles rechnet in Puffer.

use assert_no_alloc::{AllocDisabler, assert_no_alloc};
use woa_dsp::instrument::NoteEvent;
use woa_plugins::testplugin::{self, GAIN, LEVEL};
use woa_plugins::{ClapInstance, HostError};

#[global_allocator]
static A: AllocDisabler = AllocDisabler;

const RATE: f32 = 48_000.0;
const N: usize = 512;

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt()
}

/// Rechnet `blocks` Blöcke und liefert den linken Ausgang des letzten.
fn run(
    p: &mut dyn woa_dsp::plugin::StereoProcessor,
    events: &[NoteEvent],
    input: f32,
    blocks: usize,
) -> Vec<f32> {
    let inl = vec![input; N];
    let inr = vec![input; N];
    let (mut ol, mut or) = (vec![0.0; N], vec![0.0; N]);
    for b in 0..blocks {
        let ev: &[NoteEvent] = if b == 0 { events } else { &[] };
        p.process(ev, &inl, &inr, &mut ol, &mut or);
    }
    ol
}

#[test]
fn unknown_plugin_is_an_error() {
    let cat = testplugin::catalog();
    assert!(matches!(
        ClapInstance::new(&cat, "nicht.da", RATE),
        Err(HostError::Unknown(_))
    ));
}

#[test]
fn lists_params_and_plays_a_note() {
    let cat = testplugin::catalog();
    assert_eq!(cat.plugins().len(), 1);
    let inst = ClapInstance::new(&cat, testplugin::ID, RATE).expect("load");
    let ids: Vec<u32> = inst.params().iter().map(|p| p.id).collect();
    assert_eq!(ids, vec![GAIN, LEVEL]);
    let mut p = inst.processor();
    let silent = run(&mut *p, &[], 0.0, 2);
    assert_eq!(rms(&silent), 0.0);
    let loud = run(&mut *p, &[NoteEvent::note_on(69, 100)], 0.0, 4);
    assert!(rms(&loud) > 0.01, "rms {}", rms(&loud));
}

#[test]
fn param_changes_the_level() {
    let cat = testplugin::catalog();
    let inst = ClapInstance::new(&cat, testplugin::ID, RATE).expect("load");
    let mut p = inst.processor();
    let a = rms(&run(&mut *p, &[NoteEvent::note_on(69, 100)], 0.0, 4));
    p.set_param(LEVEL, 0.1);
    let b = rms(&run(&mut *p, &[], 0.0, 2));
    assert!(b < a * 0.5, "a {a} b {b}");
}

#[test]
fn effect_gain_scales_the_input() {
    let cat = testplugin::catalog();
    let inst = ClapInstance::new(&cat, testplugin::ID, RATE).expect("load");
    let mut p = inst.processor();
    let one = rms(&run(&mut *p, &[], 0.25, 2));
    p.set_param(GAIN, 2.0);
    let two = rms(&run(&mut *p, &[], 0.25, 2));
    assert!((one - 0.25).abs() < 1e-4, "one {one}");
    assert!((two - 0.5).abs() < 1e-4, "two {two}");
}

#[test]
fn state_roundtrip_restores_params() {
    let cat = testplugin::catalog();
    let mut a = ClapInstance::new(&cat, testplugin::ID, RATE).expect("load");
    let mut pa = a.processor();
    pa.set_param(GAIN, 1.5);
    pa.set_param(LEVEL, 0.2);
    run(&mut *pa, &[], 0.0, 1);
    let bytes = a.save_state().expect("save");
    assert_eq!(bytes.len(), 16);

    let mut b = ClapInstance::new(&cat, testplugin::ID, RATE).expect("load");
    b.load_state(&bytes).expect("load state");
    assert_eq!(b.param_value(GAIN), Some(1.5));
    assert_eq!(b.param_value(LEVEL), Some(0.2));
    assert!(b.load_state(&[1, 2, 3]).is_err());
}

#[test]
fn long_blocks_are_split() {
    let cat = testplugin::catalog();
    let inst = ClapInstance::new(&cat, testplugin::ID, RATE).expect("load");
    let mut p = inst.processor();
    let n = 10_000;
    let inl = vec![0.25; n];
    let (mut ol, mut or) = (vec![0.0; n], vec![0.0; n]);
    p.process(&[], &inl, &inl, &mut ol, &mut or);
    assert!((rms(&ol) - 0.25).abs() < 1e-4);
    assert!((rms(&or) - 0.25).abs() < 1e-4);
}

#[test]
fn process_does_not_allocate() {
    let cat = testplugin::catalog();
    let inst = ClapInstance::new(&cat, testplugin::ID, RATE).expect("load");
    let mut p = inst.processor();
    // Anwärmen: der erste Aufruf startet das Plugin.
    run(&mut *p, &[NoteEvent::note_on(60, 90)], 0.1, 2);
    let inl = vec![0.1; N];
    let (mut ol, mut or) = (vec![0.0; N], vec![0.0; N]);
    assert_no_alloc(|| {
        for _ in 0..50 {
            p.set_param(GAIN, 1.2);
            p.process(&[NoteEvent::note_on(64, 100)], &inl, &inl, &mut ol, &mut or);
        }
        p.reset();
        p.process(&[], &inl, &inl, &mut ol, &mut or);
    });
}

#[test]
fn dropping_the_instance_silences_processors() {
    let cat = testplugin::catalog();
    let inst = ClapInstance::new(&cat, testplugin::ID, RATE).expect("load");
    let mut p = inst.processor();
    run(&mut *p, &[], 0.25, 1);
    drop(inst);
    let out = run(&mut *p, &[], 0.25, 1);
    assert_eq!(rms(&out), 0.0);
}
