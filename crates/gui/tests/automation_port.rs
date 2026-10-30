//! Paket 18 ohne Ton: ein virtueller ALSA-Port fährt den Fader. Der Controller-Weg der
//! Oberfläche (Mapper, Aufnehmer, AppState) macht daraus eine Lautstärke-Kurve, und der
//! Offline-Render zeigt, dass der Pegel der Fahrt folgt. Es wird nichts abgespielt.
//! Ohne ALSA-Sequencer (z. B. CI-Container) wird der Test übersprungen.
#![cfg(target_os = "linux")]

use std::time::{Duration, Instant};

use midir::MidiOutput;
use midir::os::unix::VirtualOutput;
use woa_dsp::{Node, SineOsc};
use woa_engine::render_project;
use woa_gui::AppState;
use woa_gui::automation::AutoRecorder;
use woa_gui::controls::gain_from_norm;
use woa_io::controller::{Action, Mapper, Target, control_queue};
use woa_io::midi::{MidiIn, event_sink};
use woa_model::{AutoMode, AutoTarget};

const PORT: &str = "test-automation-p18";
const SR: f32 = 48_000.0;
/// Bei 120 BPM sind 24 000 Samples ein Beat.
const BEAT: usize = 24_000;

fn rms(x: &[f32]) -> f64 {
    (x.iter().map(|v| f64::from(*v).powi(2)).sum::<f64>() / x.len() as f64).sqrt()
}

fn db(x: f64) -> f64 {
    20.0 * x.log10()
}

#[test]
fn controller_ride_becomes_a_lane_and_the_render_follows_it() {
    let Some(mut dev) = MidiOutput::new(PORT)
        .ok()
        .and_then(|o| o.create_virtual(PORT).ok())
    else {
        eprintln!("kein ALSA-Sequencer, Test übersprungen");
        return;
    };
    let (sink, mut rx) = control_queue();
    let mut input = MidiIn::new(event_sink()).with_controls(sink);
    input.poll_now();
    let Some(name) = input.ports().iter().find(|p| p.starts_with(PORT)).cloned() else {
        panic!("virtueller Port nicht gelistet: {:?}", input.ports());
    };
    input.select(Some(name));

    let tmp = tempfile::tempdir().unwrap();
    let mut state = AppState::new(tmp.path().join("auto.woa"), Instant::now());
    let track = state.add_track().unwrap();
    // Schreiben bereit: Der Aufnehmer bekommt die Fahrt.
    state.set_automation(track, AutoTarget::Gain, Vec::new(), AutoMode::Write);
    // Eine leere Kurve im Modus Schreiben gilt als vorhanden.
    let range = AutoTarget::Gain
        .range_in(state.project().track(track).unwrap())
        .unwrap();
    let mut rec = AutoRecorder::new(AutoMode::Write, range);

    let mut mapper = Mapper::default();
    let frame = |mapper: &mut Mapper, rx: &mut _| mapper.pump(rx, |_| None);

    // Lernen: CC 7 steuert den Fader der ersten Spur.
    mapper.learn(Target::TrackGain(0));
    dev.send(&[0xB0, 7, 0]).unwrap();
    let end = Instant::now() + Duration::from_secs(3);
    let mut learned = false;
    while !learned && Instant::now() < end {
        learned = frame(&mut mapper, &mut rx)
            .iter()
            .any(|a| matches!(a, Action::Learned(_)));
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(learned, "Fader nicht gelernt");

    // Fahrt: Beat 0 bis 2 ganz unten gehalten, bei Beat 2 auf Anschlag hochgezogen.
    // Der Kopf wird in Schritten von 1/20 Beat simuliert, es läuft keine Wiedergabe.
    for step in 0..=80u32 {
        let beat = f64::from(step) / 20.0;
        let cc = match step {
            0 => Some(0u8),
            40 => Some(127),
            _ => None,
        };
        let mut touch = None;
        if let Some(v) = cc {
            dev.send(&[0xB0, 7, v]).unwrap();
            let end = Instant::now() + Duration::from_secs(3);
            while touch.is_none() && Instant::now() < end {
                touch = frame(&mut mapper, &mut rx)
                    .into_iter()
                    .find_map(|a| match a {
                        Action::Set {
                            target: Target::TrackGain(0),
                            value,
                        } => Some(gain_from_norm(value)),
                        _ => None,
                    });
                std::thread::sleep(Duration::from_millis(2));
            }
            assert!(touch.is_some(), "Fader-Wert kam nicht an");
        }
        rec.update(beat, touch, 0.0);
    }
    let points = rec.finish(&[]).expect("nichts aufgenommen");
    assert_eq!(points.first().unwrap().beat, 0.0);
    assert_eq!(points.last().unwrap().beat, 4.0);
    assert!(state.set_automation(track, AutoTarget::Gain, points, AutoMode::Read));
    // Ein Undo-Schritt für die ganze Fahrt.
    assert!(
        state.project().track(track).unwrap().automation[0]
            .points
            .len()
            >= 3
    );

    let mut source = |_: &woa_model::Track| -> Option<Box<dyn Node>> {
        Some(Box::new(SineOsc::new(1000.0, 0.5)))
    };
    let (l, _) = render_project(state.project(), &mut source, SR, 5 * BEAT, 256).unwrap();
    // Bis Beat 2 leise gehalten, ab Beat 2.1 am Anschlag.
    let quiet = db(rms(&l[BEAT / 2..BEAT * 3 / 2]));
    let loud = db(rms(&l[BEAT * 5 / 2..BEAT * 4]));
    assert!(quiet < -60.0, "leise Strecke {quiet:.1} dB");
    assert!(loud > -12.0, "laute Strecke {loud:.1} dB");

    // Rückgängig nimmt die ganze Fahrt auf einmal zurück.
    state.undo();
    let lane = state
        .project()
        .track(track)
        .unwrap()
        .lane(&AutoTarget::Gain)
        .unwrap();
    assert!(lane.points.is_empty());
    assert_eq!(lane.mode, AutoMode::Write);
}
