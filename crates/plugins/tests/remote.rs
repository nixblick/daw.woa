//! Selbstprüfung des Absturzschutzes (Paket 33): Das Test-Plugin läuft im eigenen Prozess
//! (`woa-plugin-host`), stürzt absichtlich ab oder hängt; die Oberfläche läuft weiter. Alles
//! rechnet in Puffer, es kommt kein Ton aus einem Gerät.

use std::time::{Duration, Instant};

use assert_no_alloc::{AllocDisabler, assert_no_alloc};
use woa_dsp::instrument::NoteEvent;
use woa_dsp::plugin::StereoProcessor;
use woa_plugins::testplugin::{self, CRASH, GAIN, HANG, LEVEL};
use woa_plugins::{Catalog, ClapInstance, Health, HostConfig, RemoteInstance};

#[global_allocator]
static A: AllocDisabler = AllocDisabler;

const RATE: f32 = 48_000.0;
const N: usize = 512;

fn config(extra: &[&str], offline: bool) -> HostConfig {
    HostConfig {
        exe: env!("CARGO_BIN_EXE_woa-plugin-host").into(),
        prefix: Vec::new(),
        extra: extra.iter().map(|s| (*s).to_string()).collect(),
        offline,
        wait_ms: if offline { 0 } else { 100 },
    }
}

fn remote(extra: &[&str], offline: bool) -> RemoteInstance {
    let cat = testplugin::catalog();
    let info = cat.find(testplugin::ID).expect("test plugin").clone();
    RemoteInstance::new(&config(extra, offline), &info, RATE).expect("remote load")
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt()
}

/// Ein Block, ohne Allokation gerechnet; liefert den linken Ausgang.
fn block(p: &mut dyn StereoProcessor, events: &[NoteEvent], input: &[f32]) -> Vec<f32> {
    let (mut ol, mut or) = (vec![0.0f32; input.len()], vec![0.0f32; input.len()]);
    assert_no_alloc(|| p.process(events, input, input, &mut ol, &mut or));
    ol
}

fn wait_until(limit: Duration, mut f: impl FnMut() -> bool) -> bool {
    let t0 = Instant::now();
    while t0.elapsed() < limit {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    false
}

#[test]
fn remote_render_equals_in_process_render() {
    let _g = testplugin::serial();
    let cat = testplugin::catalog();
    let local = ClapInstance::new(&cat, testplugin::ID, RATE).expect("local");
    let mut lp = local.processor();
    let inst = remote(&[], true);
    let mut rp = inst.processor();
    let silence = vec![0.0f32; N];
    for b in 0..6 {
        let ev: &[NoteEvent] = if b == 0 {
            &[NoteEvent::note_on(69, 100)]
        } else {
            &[]
        };
        let l = block(&mut *lp, ev, &silence);
        let r = block(&mut *rp, ev, &silence);
        assert_eq!(l.len(), r.len());
        for (i, (x, y)) in l.iter().zip(&r).enumerate() {
            assert!((x - y).abs() < 1e-6, "block {b} sample {i}: {x} vs {y}");
        }
        if b == 3 {
            assert!(rms(&r) > 0.01, "remote plays the note");
        }
    }
    assert_eq!(inst.late_blocks(), 0);
}

#[test]
fn latency_is_reported_and_real() {
    let inst = remote(&["--test-faults", "--test-latency", "64"], true);
    assert_eq!(inst.latency(), 64);
    let mut p = inst.processor();
    assert_eq!(p.latency(), 64);
    // Ein Impuls kommt genau 64 Abtastwerte später heraus (Offline-Render, ohne Ton).
    let mut input = vec![0.0f32; N];
    input[0] = 1.0;
    let out = block(&mut *p, &[], &input);
    let peak = out
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))
        .map(|(i, _)| i);
    assert_eq!(peak, Some(64), "impulse position");
}

#[test]
fn crash_mutes_only_that_plugin_and_reload_brings_it_back() {
    let mut a = remote(&["--test-faults"], false);
    let b = remote(&[], false);
    let (mut pa, mut pb) = (a.processor(), b.processor());
    let silence = vec![0.0f32; N];
    let on = [NoteEvent::note_on(69, 100)];
    for i in 0..4 {
        let ev: &[NoteEvent] = if i == 0 { &on } else { &[] };
        let _ = block(&mut *pa, ev, &silence);
        let _ = block(&mut *pb, ev, &silence);
        // Das Echtzeit-Fenster ist knapp; dem Plugin-Prozess Zeit lassen wie ein Audiogerät.
        std::thread::sleep(Duration::from_millis(8));
    }
    // Absturz: der Prozess von `a` bricht mitten im Block ab.
    pa.set_param(CRASH, 1.0);
    let crashed = block(&mut *pa, &[], &silence);
    assert_eq!(rms(&crashed), 0.0, "the crashing block is silent");
    // Der Rest läuft weiter, `a` bleibt stumm, nichts hängt oder stürzt mit.
    let t0 = Instant::now();
    for _ in 0..20 {
        let out_a = block(&mut *pa, &[], &silence);
        assert_eq!(rms(&out_a), 0.0);
        let out_b = block(&mut *pb, &[], &silence);
        assert!(out_b.iter().all(|v| v.is_finite()));
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(
        t0.elapsed() < Duration::from_secs(3),
        "blocks never wait long"
    );
    assert!(
        wait_until(Duration::from_secs(5), || a.supervise() || a.failed()),
        "the crash is noticed"
    );
    assert!(matches!(a.health(), Health::Crashed(_)));
    assert!(a.save_state().is_err());
    // `b` spielt weiter.
    let _ = block(&mut *pb, &on, &silence);
    std::thread::sleep(Duration::from_millis(8));
    let out_b = block(&mut *pb, &[], &silence);
    assert!(rms(&out_b) > 0.01, "the other plugin still plays");

    // Neu laden: derselbe Rechner `pa` spielt wieder.
    a.reload(None).expect("reload");
    assert_eq!(*a.health(), Health::Running);
    let _ = block(&mut *pa, &on, &silence);
    let mut loud = 0.0f32;
    for _ in 0..6 {
        std::thread::sleep(Duration::from_millis(8));
        loud = loud.max(rms(&block(&mut *pa, &[], &silence)));
    }
    assert!(loud > 0.01, "reloaded plugin plays again, rms {loud}");
}

#[test]
fn hang_is_noticed_and_killed() {
    let mut inst = remote(&["--test-faults"], false);
    let mut p = inst.processor();
    let silence = vec![0.0f32; N];
    p.set_param(HANG, 1.0);
    let started = Instant::now();
    let out = block(&mut *p, &[], &silence);
    assert_eq!(rms(&out), 0.0);
    assert!(
        started.elapsed() < Duration::from_millis(1000),
        "the block does not wait for the hang"
    );
    assert!(
        wait_until(Duration::from_secs(6), || inst.supervise() || inst.failed()),
        "the hang is noticed"
    );
    assert!(matches!(inst.health(), Health::Crashed(_)));
    assert!(wait_until(Duration::from_secs(2), || inst.pid().is_none()));
    assert_eq!(rms(&block(&mut *p, &[], &silence)), 0.0);
}

#[test]
fn state_and_params_cross_the_process_boundary() {
    let mut a = remote(&[], true);
    assert!(a.params().iter().any(|p| p.id == LEVEL));
    assert!(a.params().iter().any(|p| p.id == GAIN));
    assert!(a.push_param(LEVEL, 0.125));
    // Der Wert wird im Plugin-Prozess angewendet und im Spiegel sichtbar.
    let mut p = a.processor();
    let silence = vec![0.0f32; N];
    assert!(wait_until(Duration::from_secs(3), || {
        let _ = block(&mut *p, &[], &silence);
        a.param_value(LEVEL)
            .is_some_and(|v| (v - 0.125).abs() < 1e-6)
    }));
    let state = a.save_state().expect("save");
    let mut b = remote(&[], true);
    b.load_state(&state).expect("load");
    assert!(
        a.param_value(LEVEL)
            .is_some_and(|v| (v - 0.125).abs() < 1e-6),
        "source unchanged"
    );
    assert!(
        b.param_value(LEVEL)
            .is_some_and(|v| (v - 0.125).abs() < 1e-6)
    );
}

#[cfg(unix)]
#[test]
fn a_scan_that_dies_becomes_a_problem() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("kaputt.clap");
    std::fs::write(&file, b"kein Plugin").expect("write");
    // Ein Suchprozess, der sofort mit Fehler endet (statt abzustürzen), ergibt ein Problem.
    let cfg = HostConfig {
        exe: "/bin/false".into(),
        prefix: Vec::new(),
        extra: Vec::new(),
        offline: false,
        wait_ms: 0,
    };
    let cat = Catalog::scan_isolated(&[dir.path().to_path_buf()], &cfg);
    assert!(cat.plugins().is_empty());
    assert_eq!(cat.problems().len(), 1);
    // Mit dem echten Plugin-Prozess: eine Datei, die keine Bibliothek ist, ist ein Problem.
    let cat = Catalog::scan_isolated(&[dir.path().to_path_buf()], &config(&[], false));
    assert!(cat.plugins().is_empty());
    assert_eq!(cat.problems().len(), 1);
}
