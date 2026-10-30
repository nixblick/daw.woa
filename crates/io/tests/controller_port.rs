//! Paket 14 ohne Ton: ein virtueller ALSA-Port spielt den Controller. Fader und Pads kommen
//! über die Controller-Queue an, werden gelernt und lösen Aktionen aus; 1000 CC je Sekunde
//! gehen ohne Verlust und ohne lange Bilder durch; die LEDs bekommen Note-Out.
//! Ohne ALSA-Sequencer (z. B. CI-Container) wird der Test übersprungen.
#![cfg(target_os = "linux")]

use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use midir::os::unix::{VirtualInput, VirtualOutput};
use midir::{MidiInput, MidiOutput, MidiOutputConnection};
use woa_io::controller::{Action, Bus, LedOut, Mapper, Target, control_queue};
use woa_io::midi::{MidiIn, MidiStatus, event_sink};

const PORT: &str = "test-controller-p14";

fn virtual_controller() -> Option<MidiOutputConnection> {
    let out = MidiOutput::new(PORT).ok()?;
    out.create_virtual(PORT).ok()
}

fn wait_until(mut f: impl FnMut() -> bool) -> bool {
    let end = Instant::now() + Duration::from_secs(3);
    while Instant::now() < end {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    false
}

#[test]
fn virtual_controller_learns_triggers_and_survives_load() {
    let Some(mut dev) = virtual_controller() else {
        eprintln!("kein ALSA-Sequencer, Test übersprungen");
        return;
    };
    let (sink, mut rx) = control_queue();
    let mut input = MidiIn::new(event_sink()).with_controls(sink);
    input.poll_now();
    let Some(name) = input.ports().iter().find(|p| p.starts_with(PORT)).cloned() else {
        panic!("virtueller Port nicht gelistet: {:?}", input.ports());
    };
    input.select(Some(name.clone()));
    assert!(matches!(input.status(), MidiStatus::Connected(_)));
    let counters = input.counters();

    let mut mapper = Mapper::default();
    let mut frame =
        |mapper: &mut Mapper, ui: f32| -> Vec<Action> { mapper.pump(&mut rx, |_| Some(ui)) };

    // Lernen: Fader CC 7 → Musik-Pegel, Pad Note 36 (Kanal 10) → Cart 1, CC 20 → Start.
    for (target, bytes) in [
        (Target::Level(Bus::Music), [0xB0u8, 7, 0]),
        (Target::Cart(0), [0x99, 36, 100]),
        (Target::StartNext, [0xB0, 20, 127]),
    ] {
        mapper.learn(target);
        dev.send(&bytes).unwrap();
        let mut got = Vec::new();
        assert!(wait_until(|| {
            got.extend(frame(&mut mapper, 0.0));
            !got.is_empty()
        }));
        assert!(matches!(got[0], Action::Learned(m) if m.target == target));
    }
    dev.send(&[0x89, 36, 0]).unwrap();
    dev.send(&[0xB0, 20, 0]).unwrap();

    // Pad zweimal, Start-Taster einmal, Fader hoch: Zähler je Aktion.
    let (mut carts, mut starts) = (0, 0);
    let mut last = 0.0f32;
    dev.send(&[0x99, 36, 90]).unwrap();
    dev.send(&[0x89, 36, 0]).unwrap();
    dev.send(&[0x99, 36, 90]).unwrap();
    dev.send(&[0xB0, 20, 127]).unwrap();
    for v in (0..=127u8).step_by(8) {
        dev.send(&[0xB0, 7, v]).unwrap();
    }
    assert!(wait_until(|| {
        for a in frame(&mut mapper, last) {
            match a {
                Action::Press(Target::Cart(0)) => carts += 1,
                Action::Press(Target::StartNext) => starts += 1,
                Action::Set {
                    target: Target::Level(Bus::Music),
                    value,
                } => {
                    last = value;
                }
                other => panic!("unerwartet: {other:?}"),
            }
        }
        last > 0.9
    }));
    assert_eq!((carts, starts), (2, 1));
    assert!((last - 120.0 / 127.0).abs() < 1e-6);

    // Last: 1000 CC/s für 2 s. Die Oberfläche holt alle 16 ms ab (wie bei 60 Bildern).
    let before = counters.controls.load(Ordering::Relaxed);
    let sender = std::thread::spawn(move || {
        let start = Instant::now();
        for i in 0..2000u32 {
            let due = start + Duration::from_micros(u64::from(i) * 1000);
            if let Some(wait) = due.checked_duration_since(Instant::now()) {
                std::thread::sleep(wait);
            }
            dev.send(&[0xB0, 7, (i % 128) as u8]).unwrap();
        }
        dev
    });
    let mut worst = Duration::ZERO;
    let mut frames = 0usize;
    let mut applied = 0usize;
    while !sender.is_finished() {
        let t = Instant::now();
        let actions = frame(&mut mapper, last);
        worst = worst.max(t.elapsed());
        for a in actions {
            if let Action::Set { value, .. } = a {
                last = value;
                applied += 1;
            }
        }
        frames += 1;
        std::thread::sleep(Duration::from_millis(16));
    }
    let _dev = sender.join().unwrap();
    std::thread::sleep(Duration::from_millis(50));
    frame(&mut mapper, last);
    assert_eq!(
        counters.controls.load(Ordering::Relaxed) - before,
        2000,
        "alle Meldungen angekommen"
    );
    assert_eq!(counters.controls_dropped.load(Ordering::Relaxed), 0);
    // Höchstens ein Fader-Wert je Bild landet in der Oberfläche.
    assert!(applied <= frames, "{applied} Werte in {frames} Bildern");
    assert!(
        applied > frames / 2,
        "Fader folgt: {applied} Werte in {frames} Bildern"
    );
    assert!(
        worst < Duration::from_millis(4),
        "längstes Abholen {worst:?}"
    );
    eprintln!(
        "1000 CC/s: 2000 Meldungen, {frames} Bilder, {applied} Fader-Werte, längstes Abholen {worst:?}"
    );
}

#[test]
fn leds_follow_state_via_note_out() {
    const LED_PORT: &str = "test-controller-led-p14";
    let received = Arc::new(Mutex::new(Vec::<Vec<u8>>::new()));
    let rec = Arc::clone(&received);
    let Ok(midi_in) = MidiInput::new(LED_PORT) else {
        eprintln!("kein ALSA-Sequencer, Test übersprungen");
        return;
    };
    let Ok(_dev) = midi_in.create_virtual(
        LED_PORT,
        move |_, bytes, _| {
            if let Ok(mut v) = rec.lock() {
                v.push(bytes.to_vec());
            }
        },
        (),
    ) else {
        eprintln!("kein ALSA-Sequencer, Test übersprungen");
        return;
    };
    // Eingangsname wie ihn das Gerät hätte; der Ausgang wird über den Namen gefunden.
    let mut led = LedOut::open(&format!("{LED_PORT}:{LED_PORT} 130:0")).expect("ausgang");
    let mut mapper = Mapper::default();
    mapper.learn(Target::Cart(3));
    mapper.handle(
        woa_io::controller::CtrlMsg::from_raw((0x99, 40, 100)).unwrap(),
        |_| None,
    );
    led.update(&mapper.leds(|_| true));
    led.update(&mapper.leds(|_| true)); // keine Änderung, nichts gesendet
    led.update(&mapper.leds(|_| false));
    assert!(wait_until(|| received.lock().unwrap().len() >= 2));
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(
        *received.lock().unwrap(),
        vec![vec![0x99, 40, 127], vec![0x99, 40, 0]]
    );
}
