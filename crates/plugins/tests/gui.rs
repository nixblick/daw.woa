//! Selbstprüfung des Plugin-Fensters und der Reglerrückmeldung (Paket 32) mit dem Test-Plugin.
//! Das Plugin hat eine „Oberfläche“ ohne echtes Fenster: Es protokolliert, was der Host ruft,
//! meldet auf Wunsch Reglerbewegungen als Ausgangs-Events und meldet einen Zeitgeber an. So
//! läuft alles ohne Display und ohne Ton.

use assert_no_alloc::{AllocDisabler, assert_no_alloc};
use woa_dsp::plugin::StereoProcessor;
use woa_plugins::testplugin::{self, GAIN, LEVEL, SimEvent};
use woa_plugins::{ClapInstance, LOOSE_LIMIT, ParamEdit};

#[global_allocator]
static A: AllocDisabler = AllocDisabler;

const RATE: f32 = 48_000.0;
const N: usize = 256;

fn block(p: &mut dyn StereoProcessor) {
    let inl = vec![0.0f32; N];
    let inr = vec![0.0f32; N];
    let (mut ol, mut or) = (vec![0.0f32; N], vec![0.0f32; N]);
    assert_no_alloc(|| p.process(&[], &inl, &inr, &mut ol, &mut or));
}

fn load() -> ClapInstance {
    ClapInstance::new(&testplugin::catalog(), testplugin::ID, RATE).expect("load")
}

#[test]
fn window_opens_closes_reopens_and_is_destroyed_with_the_plugin() {
    let _g = testplugin::serial();
    testplugin::gui_log_clear();
    let mut inst = load();
    assert!(inst.has_gui());
    assert!(!inst.gui_open());
    inst.open_gui("Test").expect("open");
    assert!(inst.gui_open());
    inst.close_gui();
    assert!(!inst.gui_open());
    inst.open_gui("Test").expect("reopen");
    assert!(inst.gui_open());
    drop(inst);
    // Beim zweiten Öffnen wird nicht neu angelegt; beim Entladen erst zu, dann abgebaut.
    assert_eq!(
        testplugin::gui_log(),
        vec!["create", "show", "hide", "show", "hide", "destroy"]
    );
}

#[test]
fn plugin_timers_run_from_pump() {
    let _g = testplugin::serial();
    testplugin::gui_log_clear();
    let mut inst = load();
    inst.open_gui("Test").expect("open");
    std::thread::sleep(std::time::Duration::from_millis(60));
    inst.pump();
    assert!(
        testplugin::timer_ticks() >= 1,
        "ticks {}",
        testplugin::timer_ticks()
    );
}

#[test]
fn a_dragged_knob_is_one_edit_and_the_plugin_has_the_value() {
    let _g = testplugin::serial();
    testplugin::gui_log_clear();
    let mut inst = load();
    let mut p = inst.processor();
    inst.open_gui("Test").expect("open");
    testplugin::simulate_gui(&[
        SimEvent::Begin(GAIN),
        SimEvent::Value(GAIN, 1.5),
        SimEvent::Value(GAIN, 1.8),
        SimEvent::End(GAIN),
    ]);
    block(&mut *p);
    let mut edits = Vec::new();
    inst.poll_feedback(&mut edits);
    assert_eq!(
        edits,
        vec![ParamEdit::Param {
            id: GAIN,
            old: 1.0,
            new: 1.8
        }]
    );
    assert_eq!(inst.param_value(GAIN), Some(1.8));
}

#[test]
fn a_drag_that_is_still_going_reports_nothing_yet() {
    let _g = testplugin::serial();
    testplugin::gui_log_clear();
    let mut inst = load();
    let mut p = inst.processor();
    inst.open_gui("Test").expect("open");
    testplugin::simulate_gui(&[SimEvent::Begin(LEVEL), SimEvent::Value(LEVEL, 0.9)]);
    block(&mut *p);
    let mut edits = Vec::new();
    inst.poll_feedback(&mut edits);
    assert!(edits.is_empty());
    testplugin::simulate_gui(&[SimEvent::End(LEVEL)]);
    block(&mut *p);
    inst.poll_feedback(&mut edits);
    assert_eq!(
        edits,
        vec![ParamEdit::Param {
            id: LEVEL,
            old: 0.5,
            new: 0.9
        }]
    );
}

#[test]
fn a_preset_flood_is_one_whole_state_edit_and_never_allocates_in_the_audio_thread() {
    let _g = testplugin::serial();
    testplugin::gui_log_clear();
    let mut inst = load();
    let mut p = inst.processor();
    inst.open_gui("Test").expect("open");
    // Weit mehr Events als die 64 des alten Puffers.
    let flood: Vec<SimEvent> = (0..400u32)
        .map(|i| SimEvent::Value(1000 + i, f64::from(i)))
        .collect();
    assert!(flood.len() > LOOSE_LIMIT);
    testplugin::simulate_gui(&flood);
    block(&mut *p);
    let mut edits = Vec::new();
    inst.poll_feedback(&mut edits);
    assert_eq!(edits, vec![ParamEdit::WholeState]);
}

#[test]
fn nothing_is_reported_while_the_window_is_closed() {
    let _g = testplugin::serial();
    testplugin::gui_log_clear();
    let mut inst = load();
    let mut p = inst.processor();
    testplugin::simulate_gui(&[SimEvent::Value(GAIN, 0.25)]);
    block(&mut *p);
    let mut edits = Vec::new();
    inst.poll_feedback(&mut edits);
    assert!(edits.is_empty());
    // Der Merkwert ist nachgeführt: später meldet sich nur Neues.
    inst.open_gui("Test").expect("open");
    testplugin::simulate_gui(&[SimEvent::Value(GAIN, 0.5)]);
    block(&mut *p);
    inst.poll_feedback(&mut edits);
    assert_eq!(
        edits,
        vec![ParamEdit::Param {
            id: GAIN,
            old: 0.25,
            new: 0.5
        }]
    );
}

#[test]
fn a_value_the_host_pushes_reaches_the_plugin_and_is_not_reported_back() {
    let _g = testplugin::serial();
    testplugin::gui_log_clear();
    let mut inst = load();
    let mut p = inst.processor();
    inst.open_gui("Test").expect("open");
    assert!(inst.push_param(GAIN, 0.5));
    block(&mut *p);
    assert_eq!(inst.param_value(GAIN), Some(0.5));
    let mut edits = Vec::new();
    inst.poll_feedback(&mut edits);
    assert!(edits.is_empty());
}
