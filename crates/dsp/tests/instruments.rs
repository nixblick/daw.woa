//! Instrumente: jeder Process-Aufruf unter `assert_no_alloc`, Golden-Renders
//! in `tests/golden/` (neu schreiben mit `WOA_BLESS=1`).

use std::path::PathBuf;
use std::sync::Arc;

use assert_no_alloc::{AllocDisabler, assert_no_alloc};
use woa_dsp::instrument::{Instrument, InstrumentNode, NoteEvent, NoteEventKind, note_queue};
use woa_dsp::sampler::{SampleData, Sampler};
use woa_dsp::soundfont::{SoundFontPlayer, load_soundfont};
use woa_dsp::synth::{PolySynth, Waveform};
use woa_dsp::{Node, ProcessBuffers};

#[global_allocator]
static ALLOC: AllocDisabler = AllocDisabler;

const SR: u32 = 48_000;
const BLOCK: usize = 128;

/// Spielt `events` (Block-Index, Event) ab und rendert `frames` Samples.
fn render<I: Instrument>(inst: I, events: &[(usize, NoteEvent)], frames: usize) -> Vec<f32> {
    let (mut tx, rx) = note_queue(256);
    let mut node = InstrumentNode::new(inst, rx);
    node.prepare(SR as f32);
    let input = [0.0f32; BLOCK];
    let mut out = vec![0.0f32; frames];
    for (b, chunk) in out.chunks_mut(BLOCK).enumerate() {
        for (_, ev) in events.iter().filter(|(eb, _)| *eb == b) {
            tx.push(*ev).expect("queue has room");
        }
        let n = chunk.len();
        assert_no_alloc(|| {
            node.process(ProcessBuffers {
                input: &input[..n],
                output: chunk,
            })
        });
    }
    out
}

fn golden(name: &str, rendered: &[f32]) {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/golden")
        .join(name);
    if std::env::var_os("WOA_BLESS").is_some() {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: SR,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };
        let mut w = hound::WavWriter::create(&path, spec).expect("create golden");
        for s in rendered {
            w.write_sample(*s).expect("write");
        }
        w.finalize().expect("finalize");
    }
    let mut r = hound::WavReader::open(&path).expect("golden exists (WOA_BLESS=1 to create)");
    assert_eq!(r.spec().sample_rate, SR);
    let g: Vec<f32> = r.samples::<f32>().map(|s| s.expect("sample")).collect();
    assert_eq!(g.len(), rendered.len());
    let max_err = g
        .iter()
        .zip(rendered)
        .map(|(a, b)| (a - b).abs())
        .fold(0.0f32, f32::max);
    assert!(max_err < 1e-4, "{name}: max deviation {max_err}");
}

fn peak(x: &[f32]) -> f32 {
    x.iter().fold(0.0f32, |m, s| m.max(s.abs()))
}

fn chord_events() -> Vec<(usize, NoteEvent)> {
    vec![
        (0, NoteEvent::note_on(60, 100)),
        (0, NoteEvent::note_on(64, 100).at(32)),
        (0, NoteEvent::note_on(67, 100).at(64)),
        (200, NoteEvent::note_off(60)),
        (200, NoteEvent::note_off(64)),
        (200, NoteEvent::note_off(67)),
    ]
}

#[test]
fn golden_synth_saw_chord() {
    let out = render(
        PolySynth::new(8, Waveform::Saw),
        &chord_events(),
        SR as usize,
    );
    assert!(peak(&out) > 0.1);
    golden("synth_saw_chord_1s.wav", &out);
}

#[test]
fn golden_synth_square_chord() {
    let out = render(
        PolySynth::new(8, Waveform::Square),
        &chord_events(),
        SR as usize,
    );
    assert!(peak(&out) > 0.1);
    golden("synth_square_chord_1s.wav", &out);
}

#[test]
fn synth_event_offset_is_sample_accurate() {
    let out = render(
        PolySynth::new(1, Waveform::Square),
        &[(0, NoteEvent::note_on(69, 127).at(50))],
        BLOCK,
    );
    assert!(out[..50].iter().all(|x| *x == 0.0));
    assert!(out[51] != 0.0);
}

#[test]
fn synth_stealing_under_no_alloc() {
    let events: Vec<_> = (0..32u8)
        .map(|i| (usize::from(i), NoteEvent::note_on(40 + i, 100)))
        .collect();
    let out = render(PolySynth::new(4, Waveform::Saw), &events, BLOCK * 40);
    assert!(out.iter().all(|x| x.is_finite()));
}

fn sine_sample() -> Arc<SampleData> {
    // 440 Hz, 100 Perioden, Loop über die letzten 50 Perioden.
    let period = SR as usize / 440;
    let n = period * 100;
    let frames = (0..n)
        .map(|i| (i as f32 / period as f32 * std::f32::consts::TAU).sin())
        .collect();
    Arc::new(SampleData::new(frames, SR as f32, 69, Some((period * 50, n))).expect("valid"))
}

#[test]
fn golden_sampler_loop() {
    let events = vec![
        (0, NoteEvent::note_on(69, 127)),
        (0, NoteEvent::note_on(76, 90).at(10)),
        (300, NoteEvent::note_off(69)),
        (300, NoteEvent::note_off(76)),
    ];
    let out = render(Sampler::new(sine_sample(), 8), &events, SR as usize);
    assert!(peak(&out) > 0.5);
    // Loop trägt über die Sample-Länge (~0,23 s) hinaus.
    assert!(peak(&out[SR as usize / 2..SR as usize * 3 / 4]) > 0.5);
    golden("sampler_loop_1s.wav", &out);
}

// --- SoundFont: minimale SF2 im Test erzeugt (ein Sinus-Sample, ein Preset).

use woa_dsp::soundfont::tiny_sf2;

#[test]
fn golden_soundfont_note() {
    let bytes = tiny_sf2();
    let font = load_soundfont(&mut bytes.as_slice()).expect("tiny sf2 parses");
    let player = SoundFontPlayer::new(font, SR, 64).expect("synth");
    let events = vec![
        (
            0,
            NoteEvent {
                offset: 0,
                channel: 0,
                kind: NoteEventKind::ProgramChange { program: 0 },
            },
        ),
        (0, NoteEvent::note_on(69, 120).at(5)),
        (1, NoteEvent::note_on(72, 100).on_channel(1)),
        (200, NoteEvent::note_off(69)),
        (200, NoteEvent::note_off(72).on_channel(1)),
    ];
    let out = render(player, &events, SR as usize);
    assert!(peak(&out) > 0.01, "peak {}", peak(&out));
    assert!(out.iter().all(|x| x.is_finite()));
    golden("soundfont_note_1s.wav", &out);
}
