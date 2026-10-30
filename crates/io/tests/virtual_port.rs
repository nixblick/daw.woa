//! Paket 11 ohne Ton: ein virtueller ALSA-Port spielt das Keyboard. Die Events müssen in
//! der Queue ankommen, die Instrument-Node rendert sie offline (nicht still, Pegel folgt
//! der Anschlagstärke), und Schließen des Ports wird erkannt und wieder verbunden.
//! Ohne ALSA-Sequencer (z. B. CI-Container) wird der Test übersprungen.
#![cfg(target_os = "linux")]

use std::time::{Duration, Instant};

use assert_no_alloc::{AllocDisabler, assert_no_alloc};
use midir::os::unix::VirtualOutput;
use midir::{MidiOutput, MidiOutputConnection};
use woa_dsp::instrument::{InstrumentNode, note_queue};
use woa_dsp::synth::{PolySynth, Waveform};
use woa_dsp::{Node, ProcessBuffers};
use woa_io::midi::{MidiIn, MidiStatus, event_sink};

#[global_allocator]
static A: AllocDisabler = AllocDisabler;

const PORT: &str = "test-keyboard-p11";

fn virtual_keyboard() -> Option<MidiOutputConnection> {
    let out = MidiOutput::new(PORT).ok()?;
    out.create_virtual(PORT).ok()
}

fn wait_until(mut f: impl FnMut() -> bool) -> bool {
    let end = Instant::now() + Duration::from_secs(3);
    while Instant::now() < end {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    false
}

fn render_peak(node: &mut InstrumentNode<PolySynth>, frames: usize) -> f32 {
    let input = vec![0.0f32; 256];
    let mut out = vec![0.0f32; 256];
    let mut peak = 0.0f32;
    for _ in 0..frames / 256 {
        assert_no_alloc(|| {
            node.process(ProcessBuffers {
                input: &input,
                output: &mut out,
            });
        });
        peak = out.iter().fold(peak, |m, x| m.max(x.abs()));
    }
    peak
}

#[test]
fn virtual_keyboard_plays_and_hotplugs() {
    let Some(mut kbd) = virtual_keyboard() else {
        eprintln!("kein ALSA-Sequencer, Test übersprungen");
        return;
    };
    let sink = event_sink();
    let (prod, cons) = note_queue(256);
    *sink.lock().expect("lock") = Some(prod);
    let mut midi = MidiIn::new(sink.clone());
    midi.poll_now();
    let name = midi
        .ports()
        .iter()
        .find(|p| p.contains(PORT))
        .cloned()
        .expect("virtual port listed");
    midi.select(Some(name.clone()));
    assert_eq!(midi.status(), MidiStatus::Connected(name.clone()));

    let mut node = InstrumentNode::new(PolySynth::new(8, Waveform::Sine), cons);
    node.prepare(48_000.0);

    // Leise Note.
    kbd.send(&[0x90, 69, 20]).expect("send");
    let counters = midi.counters();
    assert!(wait_until(|| counters
        .events
        .load(std::sync::atomic::Ordering::Relaxed)
        >= 1));
    let soft = render_peak(&mut node, 4800);
    kbd.send(&[0x80, 69, 0]).expect("send");
    // Laute Note mit Pedal; Note aus, Pedal hält.
    kbd.send(&[0xB0, 64, 127]).expect("send");
    kbd.send(&[0x90, 69, 127]).expect("send");
    kbd.send(&[0x80, 69, 0]).expect("send");
    assert!(wait_until(|| counters
        .events
        .load(std::sync::atomic::Ordering::Relaxed)
        >= 5));
    let loud = render_peak(&mut node, 9600);
    assert!(soft > 0.01, "nicht still: {soft}");
    assert!(
        loud > soft * 3.0,
        "Anschlag hörbar: leise {soft}, laut {loud}"
    );
    assert!(node.instrument().active_voices() > 0, "Pedal hält");
    kbd.send(&[0xB0, 64, 0]).expect("send");
    assert!(wait_until(|| counters
        .events
        .load(std::sync::atomic::Ordering::Relaxed)
        >= 6));
    render_peak(&mut node, 48_000);
    assert_eq!(node.instrument().active_voices(), 0, "Pedal los");
    assert_eq!(
        counters.dropped.load(std::sync::atomic::Ordering::Relaxed),
        0
    );

    // Hotplug: Port schließen -> Warten, wieder öffnen -> verbunden.
    kbd.send(&[0x90, 72, 100]).expect("send");
    drop(kbd);
    midi.poll_now();
    assert_eq!(midi.status(), MidiStatus::Waiting(name.clone()));
    // Hängende Note nach dem Abziehen: release_all schickt Pedal los + alle aus.
    render_peak(&mut node, 48_000);
    assert_eq!(node.instrument().active_voices(), 0, "nichts hängt");
    let mut kbd = virtual_keyboard().expect("port again");
    midi.poll_now();
    assert!(
        matches!(midi.status(), MidiStatus::Connected(ref n) if n.contains(PORT)),
        "{:?}",
        midi.status()
    );
    let before = counters.events.load(std::sync::atomic::Ordering::Relaxed);
    kbd.send(&[0x90, 60, 100]).expect("send");
    assert!(wait_until(|| counters
        .events
        .load(std::sync::atomic::Ordering::Relaxed)
        > before));
}
