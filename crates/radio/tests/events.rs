//! Feste Zeit-Events (Paket 29): eine Stunde Sendung mit simulierter Uhr durch den echten
//! Mixer, ohne Ton ausgeben (nur Puffer). Gemessen wird am Ausgang: Zeit des Event-Beginns
//! (Frequenz des Event-Tons) und Lücken (gefensterter Pegel).

use std::path::{Path, PathBuf};

use assert_no_alloc::{AllocDisabler, assert_no_alloc};
use woa_radio::events::{EventTable, Kind, Pattern, TimeEvent, Timing};
use woa_radio::{Controller, PlaylistEntry, RadioShow};

#[global_allocator]
static ALLOC: AllocDisabler = AllocDisabler;

/// Niedrige Rate, damit eine Stunde schnell rechnet.
const SR: u32 = 8_000;
const BLOCK: usize = 256;
const MUSIC_HZ: f32 = 440.0;
const EVENT_HZ: f32 = 1000.0;
/// Montag 2026-10-26 12:00:00 UTC.
const T0: i64 = 1_793_016_000 - 1_793_016_000 % 86_400 + 12 * 3600;

fn write_tone(path: &Path, seconds: f64, freq: f32, amp: f32) {
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: SR,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut w = hound::WavWriter::create(path, spec).unwrap();
    for i in 0..(seconds * f64::from(SR)) as usize {
        let v = (i as f32 * freq * std::f32::consts::TAU / SR as f32).sin() * amp;
        w.write_sample(v).unwrap();
        w.write_sample(v).unwrap();
    }
    w.finalize().unwrap();
}

fn event(id: u32, name: &str, path: &Path, pattern: Pattern, timing: Timing) -> TimeEvent {
    TimeEvent {
        id,
        name: name.into(),
        kind: Kind::Jingle,
        path: path.to_path_buf(),
        pattern,
        timing,
        enabled: true,
    }
}

fn table(events: Vec<TimeEvent>) -> EventTable {
    EventTable {
        utc_offset_min: 0,
        auto_dst: false,
        events,
        ..EventTable::default()
    }
}

struct Run {
    left: Vec<f32>,
    controller: Controller,
}

/// Sendet `seconds` ab `T0 + offset`; die Uhr des Controllers folgt den gerechneten Frames.
fn run(show: RadioShow, events: EventTable, start: i64, seconds: usize) -> Run {
    let (mut c, mut m) = Controller::new(show, SR);
    c.measure_missing();
    c.set_events(events);
    let mut now = start as f64;
    c.pump_at(now);
    c.start();
    let mut l = vec![0.0f32; BLOCK];
    let mut r = vec![0.0f32; BLOCK];
    let mut out = Vec::with_capacity(seconds * SR as usize);
    while out.len() < seconds * SR as usize {
        c.pump_at(now);
        assert_no_alloc(|| m.process(&mut l, &mut r));
        out.extend_from_slice(&l);
        now += BLOCK as f64 / f64::from(SR);
    }
    Run {
        left: out,
        controller: c,
    }
}

fn music_show(dir: &Path, titles: usize, seconds: f64) -> (RadioShow, PathBuf) {
    let p = dir.join("musik.wav");
    write_tone(&p, seconds, MUSIC_HZ, 0.3);
    let show = RadioShow {
        crossfade_seconds: 3.0,
        bus_processing: false,
        mic_processing: false,
        // Ohne Auto-Gain-Eingriff: die Töne haben gleichen Pegel.
        playlist: (0..titles).map(|_| PlaylistEntry::new(&p)).collect(),
        ..RadioShow::default()
    };
    (show, p)
}

/// Frequenz in Hz aus Nulldurchgängen eines Fensters.
fn freq(w: &[f32]) -> f32 {
    let z = w.windows(2).filter(|p| p[0] <= 0.0 && p[1] > 0.0).count();
    z as f32 * SR as f32 / w.len() as f32
}

/// Sekunde (ab Start), in der der Event-Ton zum ersten Mal ab `from_s` anliegt.
fn first_event_after(out: &[f32], from_s: f64) -> Option<f64> {
    let win = SR as usize / 20;
    let mut i = (from_s * f64::from(SR)) as usize;
    while i + win <= out.len() {
        let w = &out[i..i + win];
        let rms = (w.iter().map(|v| v * v).sum::<f32>() / win as f32).sqrt();
        if rms > 0.05 && (freq(w) - EVENT_HZ).abs() < 120.0 {
            return Some(i as f64 / f64::from(SR));
        }
        i += win / 2;
    }
    None
}

/// Kleinster Pegel (RMS über 50 ms) in `[from_s, to_s)`.
fn min_rms(out: &[f32], from_s: f64, to_s: f64) -> f32 {
    let win = SR as usize / 20;
    let (mut i, end) = (
        (from_s * f64::from(SR)) as usize,
        ((to_s * f64::from(SR)) as usize).min(out.len()),
    );
    let mut min = f32::MAX;
    while i + win <= end {
        let w = &out[i..i + win];
        min = min.min((w.iter().map(|v| v * v).sum::<f32>() / win as f32).sqrt());
        i += win;
    }
    min
}

#[test]
fn one_hour_hard_and_soft_events_on_time_without_gap() {
    let tmp = tempfile::tempdir().unwrap();
    let (show, _) = music_show(tmp.path(), 300, 20.0);
    let jingle = tmp.path().join("jingle.wav");
    write_tone(&jingle, 3.0, EVENT_HZ, 0.3);
    // Start 11:50:00 Ortszeit (UTC), eine Stunde: 12:00 hart, 12:30:10 weich.
    let start = T0 - 600;
    let t = table(vec![
        event(
            1,
            "Stundenjingle",
            &jingle,
            Pattern::hourly(0),
            Timing::Hard,
        ),
        event(
            2,
            "Werbung",
            &jingle,
            Pattern {
                second: 10,
                ..Pattern::hourly(30)
            },
            Timing::Soft,
        ),
    ]);
    let r = run(show, t, start, 3600);
    let out = &r.left;

    // Hart: um 12:00:00 (600 s nach Start) und um 13:00:00 (4200 s) liegt das Jingle an.
    for hard_at in [600.0, 4200.0] {
        if hard_at + 3.0 > 3600.0 {
            continue;
        }
        let s = first_event_after(out, hard_at - 5.0).expect("hard event heard");
        let dev = s - hard_at;
        eprintln!("hard event at {hard_at}: heard {dev:.2} s after");
        assert!(
            (0.0..=2.0).contains(&dev),
            "hard event deviation {dev:.2} s"
        );
    }
    // Weich: 12:30:10 = 2410 s nach Start. Der laufende Titel läuft aus, dann kommt das Jingle.
    let soft_at = 2410.0;
    let s = first_event_after(out, soft_at).expect("soft event heard");
    assert!(
        s >= soft_at && s - soft_at <= 20.0,
        "soft event starts {:.2} s after its time",
        s - soft_at
    );
    // Das zuletzt gespielte Event (das weiche) meldet dieselbe Abweichung wie die Messung.
    let (name, dev) = r.controller.last_event().cloned().expect("event reported");
    assert_eq!(name, "Werbung");
    assert!(
        (dev - (s - soft_at)).abs() < 0.8,
        "reported {dev}, measured {}",
        s - soft_at
    );
    // Der laufende Titel wurde nicht angeschnitten: vor dem weichen Event nur Musik, ohne Lücke.
    assert!(min_rms(out, soft_at, s) > 0.05);

    // Keine Lücke in der ganzen Stunde (Titelwechsel, Event-Wechsel, Schnitte).
    let gap = min_rms(out, 5.0, 3590.0);
    assert!(gap > 0.05, "quietest 50 ms window: {gap}");
    assert_eq!(r.controller.events_missed(), 0);
}

#[test]
fn hard_event_during_a_running_crossfade_is_still_on_time() {
    let tmp = tempfile::tempdir().unwrap();
    // Titel von 20 s: Überblendungen bei 17 s, 34 s, ... Das Event liegt mitten in einer.
    let (show, _) = music_show(tmp.path(), 40, 20.0);
    let jingle = tmp.path().join("jingle.wav");
    write_tone(&jingle, 2.0, EVENT_HZ, 0.3);
    // Titel 1: 0..20, Titel 2 beginnt bei 17 s. Das Event um 18,5 s: mitten in der Überblendung.
    let start = T0;
    let t = table(vec![event(
        1,
        "Jingle",
        &jingle,
        Pattern {
            second: 18,
            ..Pattern::hourly(0)
        },
        Timing::Hard,
    )]);
    // Der erste mögliche Zeitpunkt ist 12:00:18; wir starten 12:00:00 -> 18 s nach Start.
    let r = run(show, t, start, 60);
    let s = first_event_after(&r.left, 10.0).expect("event heard");
    let dev = s - 18.0;
    eprintln!("hard event in crossfade: heard {dev:.2} s after");
    assert!((0.0..=2.0).contains(&dev), "deviation {dev:.2} s");
    assert!(min_rms(&r.left, 2.0, 50.0) > 0.05);
}

#[test]
fn event_start_with_the_mixer_is_allocation_free() {
    // `run` ruft den Mixer in `assert_no_alloc`; dieser Lauf enthält Event-Start,
    // Überblendung und Abblenden des alten Titels.
    let tmp = tempfile::tempdir().unwrap();
    let (show, _) = music_show(tmp.path(), 10, 12.0);
    let jingle = tmp.path().join("jingle.wav");
    write_tone(&jingle, 1.0, EVENT_HZ, 0.3);
    let t = table(vec![event(
        1,
        "Jingle",
        &jingle,
        Pattern {
            second: 5,
            ..Pattern::hourly(0)
        },
        Timing::Hard,
    )]);
    let r = run(show, t, T0, 30);
    assert!(first_event_after(&r.left, 0.0).is_some());
}
