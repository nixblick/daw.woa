//! Paket 32: Reglerbewegungen aus dem Plugin-Fenster im Zusammenspiel mit Projekt, Rückgängig und
//! Rack. Wie in `gui.rs` ohne echtes Fenster und ohne Ton.

use std::sync::Arc;

use assert_no_alloc::{AllocDisabler, assert_no_alloc};
use woa_dsp::plugin::StereoProcessor;
use woa_model::{Command, InsertId, PluginRole, PluginSlot, Session, TrackId, TrackKind};
use woa_plugins::testplugin::{self, GAIN, SimEvent};
use woa_plugins::{LOOSE_LIMIT, ParamEdit, PluginRack};

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

struct Rig {
    session: Session,
    rack: PluginRack,
    track: TrackId,
    fx: InsertId,
    proc: Box<dyn StereoProcessor>,
}

fn rig() -> Rig {
    let mut session = Session::new("Gui");
    let track = session.new_track_id();
    session
        .apply(Command::add_track(track, "Spur", TrackKind::Audio))
        .expect("track");
    let fx = session.new_insert_id();
    session
        .apply(Command::AddPlugin {
            track,
            plugin: PluginSlot::new(fx, testplugin::ID, "Test", PluginRole::Effect),
            index: None,
        })
        .expect("plugin");
    let mut rack = PluginRack::new(Arc::new(testplugin::catalog()), RATE);
    rack.sync(session.project());
    let t = session.project().track(track).expect("track").clone();
    let mut plugins = rack.track_plugins(&t);
    let proc = plugins.effects.remove(0).processor;
    Rig {
        session,
        rack,
        track,
        fx,
        proc,
    }
}

#[test]
fn undo_of_a_window_edit_puts_the_knob_back_in_the_plugin() {
    let _g = testplugin::serial();
    testplugin::gui_log_clear();
    let mut r = rig();
    r.rack.open_gui(r.fx, "Test").expect("open");
    assert!(r.rack.any_gui_open());
    testplugin::simulate_gui(&[
        SimEvent::Begin(GAIN),
        SimEvent::Value(GAIN, 1.8),
        SimEvent::End(GAIN),
    ]);
    block(&mut *r.proc);
    let edits = r.rack.poll_edits();
    assert_eq!(
        edits,
        vec![(
            r.fx,
            ParamEdit::Param {
                id: GAIN,
                old: 1.0,
                new: 1.8
            }
        )]
    );
    // Wie die Oberfläche es übernimmt: ein Befehl, ein Rückgängig-Schritt.
    r.session
        .apply(Command::SetPluginParam {
            track: r.track,
            plugin: r.fx,
            param: GAIN,
            value: 1.8,
        })
        .expect("apply");
    r.rack.reconcile(r.session.project());
    assert_eq!(r.rack.param_value(r.fx, GAIN), Some(1.8));

    r.session.undo().expect("undo");
    r.rack.reconcile(r.session.project());
    block(&mut *r.proc);
    assert_eq!(r.rack.param_value(r.fx, GAIN), Some(1.0));
    // Das Zurücksetzen kommt nicht als Änderung des Nutzers zurück.
    assert!(r.rack.poll_edits().is_empty());
}

#[test]
fn a_preset_change_is_one_state_step_and_undo_reloads_the_old_state() {
    let _g = testplugin::serial();
    testplugin::gui_log_clear();
    let mut r = rig();
    r.rack.open_gui(r.fx, "Test").expect("open");
    // Ein Preset setzt viele Regler auf einmal (hier: Gain und acht weitere).
    let mut preset = vec![SimEvent::Value(GAIN, 1.7)];
    preset.extend((0..LOOSE_LIMIT as u32).map(|i| SimEvent::Value(900 + i, 0.5)));
    testplugin::simulate_gui(&preset);
    block(&mut *r.proc);
    let edits = r.rack.poll_edits();
    assert_eq!(edits, vec![(r.fx, ParamEdit::WholeState)]);
    let cmd = r
        .rack
        .capture_slot(r.session.project(), r.fx)
        .expect("state");
    let steps_before = r.session.history().undo_entries().len();
    r.session.apply(cmd).expect("apply");
    assert_eq!(r.session.history().undo_entries().len(), steps_before + 1);
    r.rack.reconcile(r.session.project());
    assert_eq!(r.rack.param_value(r.fx, GAIN), Some(1.7));

    r.session.undo().expect("undo");
    r.rack.reconcile(r.session.project());
    assert_eq!(r.rack.param_value(r.fx, GAIN), Some(1.0));
    r.session.redo().expect("redo");
    r.rack.reconcile(r.session.project());
    assert_eq!(r.rack.param_value(r.fx, GAIN), Some(1.7));
}
