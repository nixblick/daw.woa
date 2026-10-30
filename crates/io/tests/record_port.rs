//! Paket 12 ohne Ton: Ein virtueller ALSA-Port spielt ein festes Notenmuster mit Pedal,
//! die Aufnahme stempelt jedes Event über die Sample-Uhr. Der Clip enthält dieselben
//! Noten, jede höchstens 1 ms neben der Stelle, an der sie gesendet wurde.
//!
//! Die Uhr läuft wie ein Audiogerät: Ein Thread meldet alle 256 Samples die Position
//! (aus der echten Zeit, also ohne Drift), genau wie der Audio-Thread der Engine.
//! Ohne ALSA-Sequencer (z. B. CI-Container) wird der Test übersprungen.
#![cfg(target_os = "linux")]

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use midir::MidiOutput;
use midir::os::unix::VirtualOutput;
use woa_dsp::clock::SampleClock;
use woa_io::midi::{MidiIn, event_sink};
use woa_io::record::MidiTake;

const PORT: &str = "test-keyboard-p12";
const RATE: f64 = 48_000.0;
const BLOCK: u64 = 256;
const TEMPO: f64 = 120.0;

#[test]
fn virtual_port_pattern_is_recorded_sample_accurate() {
    let Ok(out) = MidiOutput::new(PORT) else {
        eprintln!("kein ALSA-Sequencer, Test übersprungen");
        return;
    };
    let Ok(mut kbd) = out.create_virtual(PORT) else {
        eprintln!("kein ALSA-Sequencer, Test übersprungen");
        return;
    };
    let mut midi = MidiIn::new(event_sink());
    midi.poll_now();
    let name = midi
        .ports()
        .iter()
        .find(|p| p.contains(PORT))
        .cloned()
        .expect("virtual port listed");
    midi.select(Some(name));

    // "Audiogerät": Position = vergangene Zeit, gemeldet je Block am Blockanfang.
    let clock = Arc::new(SampleClock::new());
    let t0 = Instant::now();
    let start_sample = 96_000u64; // Aufnahme beginnt bei 2 s Transportzeit
    let run = Arc::new(AtomicBool::new(true));
    let device = std::thread::spawn({
        let clock = Arc::clone(&clock);
        let run = Arc::clone(&run);
        move || {
            while run.load(Ordering::Relaxed) {
                let now = Instant::now();
                let elapsed = now.duration_since(t0).as_secs_f64();
                let frames = (elapsed * RATE) as u64 / BLOCK * BLOCK;
                let block_time = t0 + Duration::from_secs_f64(frames as f64 / RATE);
                clock.publish_at(start_sample + frames, true, RATE as f32, block_time);
                std::thread::sleep(Duration::from_micros(1_500));
            }
        }
    });
    while clock.read().is_none() {
        std::thread::sleep(Duration::from_millis(1));
    }

    let mut take = MidiTake::start(&midi.record_tap(), Arc::clone(&clock), start_sample);
    // Festes Muster: (Abstand ms, Bytes). C-E-G mit Pedal, Running-Status-freie Nachrichten.
    let pattern: [(u64, [u8; 3]); 9] = [
        (50, [0x90, 60, 100]),
        (37, [0xB0, 64, 127]),
        (113, [0x90, 64, 70]),
        (5, [0x80, 60, 0]),
        (125, [0x90, 67, 40]),
        (90, [0x90, 64, 0]),
        (77, [0x80, 67, 0]),
        (31, [0xB0, 64, 0]),
        (60, [0x90, 72, 127]),
    ];
    let mut sent = Vec::new();
    for (wait, bytes) in pattern {
        std::thread::sleep(Duration::from_millis(wait));
        let at = Instant::now();
        kbd.send(&bytes).expect("send");
        // Transportposition beim Senden, relativ zum Aufnahmebeginn.
        sent.push((at.duration_since(t0).as_secs_f64() * RATE, bytes));
        take.drain();
    }
    std::thread::sleep(Duration::from_millis(100));
    take.drain();
    let r = take.finish(TEMPO);
    run.store(false, Ordering::Relaxed);
    device.join().expect("device thread");

    assert_eq!(r.dropped, 0);
    assert_eq!(r.events, pattern.len(), "every event arrived");
    let beats = |sample: f64| sample / RATE * TEMPO / 60.0;
    let on = |pitch: u8| {
        sent.iter()
            .find(|(_, b)| b[0] == 0x90 && b[1] == pitch && b[2] > 0)
            .map(|(s, b)| (*s, b[2]))
            .expect("sent")
    };
    let off = |pitch: u8| {
        sent.iter()
            .find(|(_, b)| b[1] == pitch && (b[0] == 0x80 || (b[0] == 0x90 && b[2] == 0)))
            .map(|(s, _)| *s)
    };
    let ms = TEMPO / 60.0 / 1000.0; // Beats je Millisekunde
    let mut worst = 0.0f64;
    assert_eq!(r.midi.notes.len(), 4, "{:?}", r.midi.notes);
    for n in &r.midi.notes {
        let (s, vel) = on(n.pitch);
        assert_eq!(n.velocity, vel, "velocity of {}", n.pitch);
        let want = beats(s);
        let d = (n.beat - want).abs();
        worst = worst.max(d / ms);
        assert!(
            d < ms,
            "note {} at {} beats, sent at {want} ({} ms off)",
            n.pitch,
            n.beat,
            d / ms
        );
        if let Some(o) = off(n.pitch) {
            let want_len = beats(o) - beats(s);
            let d = (n.length_beats - want_len).abs();
            assert!(
                d < 2.0 * ms,
                "length of {}: {} vs {want_len}",
                n.pitch,
                n.length_beats
            );
        }
    }
    assert_eq!(r.midi.cc.len(), 2, "pedal down and up");
    assert_eq!((r.midi.cc[0].value, r.midi.cc[1].value), (127, 0));
    let pedal = sent.iter().find(|(_, b)| b[0] == 0xB0).expect("pedal").0;
    assert!((r.midi.cc[0].beat - beats(pedal)).abs() < ms);
    eprintln!("worst onset deviation: {worst:.3} ms");
}
