//! Paket 12: `.mid` schreiben und wieder lesen ergibt dieselben Noten, und eine Datei,
//! wie andere Programme sie schreiben (Format 1, Tempospur, Running Status, Note On mit
//! Velocity 0 als Note aus, Pedal, SysEx, Program Change), wird richtig gelesen.
//!
//! Die fremde Datei ist Byte für Byte nach der SMF-Spezifikation von Hand aufgeschrieben
//! (kein Programm zum Erzeugen auf dem Prüfrechner); sie benutzt bewusst die
//! Eigenheiten, die WOAs eigener Schreiber nicht erzeugt.

use woa_io::smf::{WRITE_PPQ, read_smf, write_smf};
use woa_model::{MidiCc, MidiData, MidiNote};

fn phrase() -> MidiData {
    let n = |beat, length_beats, pitch, velocity| MidiNote {
        beat,
        length_beats,
        pitch,
        velocity,
    };
    MidiData {
        notes: vec![
            n(0.0, 0.5, 60, 100),
            n(0.0, 1.0, 64, 90),
            n(0.5, 0.25, 60, 30),
            n(1.0, 2.0, 67, 127),
            n(3.0 + 1.0 / 3.0, 1.0 / 3.0, 72, 1),
        ],
        cc: vec![
            MidiCc {
                beat: 0.0,
                controller: 64,
                value: 127,
            },
            MidiCc {
                beat: 2.75,
                controller: 64,
                value: 0,
            },
        ],
    }
}

#[test]
fn write_then_read_gives_the_same_notes() {
    let bytes = write_smf(&phrase(), 93.0, "Phrase");
    assert_eq!(&bytes[..4], b"MThd");
    let back = read_smf(&bytes).unwrap();
    assert_eq!(back.name.as_deref(), Some("Phrase"));
    assert!((back.tempo_bpm.unwrap() - 93.0).abs() < 1e-3);
    let tick = 1.0 / f64::from(WRITE_PPQ);
    let want = phrase();
    assert_eq!(back.midi.notes.len(), want.notes.len());
    for (a, b) in back.midi.notes.iter().zip(&want.notes) {
        assert_eq!((a.pitch, a.velocity), (b.pitch, b.velocity));
        assert!((a.beat - b.beat).abs() <= tick / 2.0, "{a:?} vs {b:?}");
        assert!(
            (a.length_beats - b.length_beats).abs() <= tick,
            "{a:?} vs {b:?}"
        );
    }
    assert_eq!(back.midi.cc, want.cc);
    assert!(
        (back.end_beats - 1760.0 / 480.0).abs() < 1e-9,
        "end = last note off"
    );
    // Schreiben ist stabil: noch einmal schreiben ergibt dieselben Bytes.
    assert_eq!(write_smf(&back.midi, 93.0, "Phrase"), bytes);
}

/// Format 1, 96 Ticks je Viertel, Tempo 100 BPM in der Tempospur.
fn foreign_file() -> Vec<u8> {
    let mut f = Vec::new();
    f.extend_from_slice(b"MThd\x00\x00\x00\x06\x00\x01\x00\x02\x00\x60");
    let tempo_track: &[u8] = &[
        0x00, 0xFF, 0x58, 0x04, 0x04, 0x02, 0x18, 0x08, // Taktart 4/4
        0x00, 0xFF, 0x51, 0x03, 0x09, 0x27, 0xC0, // 600 000 µs = 100 BPM
        0x00, 0xFF, 0x2F, 0x00,
    ];
    let notes: &[u8] = &[
        0x00, 0xFF, 0x03, 0x05, b'P', b'i', b'a', b'n', b'o', // Spurname
        0x00, 0xF0, 0x05, 0x7E, 0x7F, 0x09, 0x01, 0xF7, // GM-Reset (SysEx)
        0x00, 0xC0, 0x00, // Program Change
        0x00, 0xB0, 0x40, 0x7F, // Pedal an
        0x00, 0x90, 0x3C, 0x50, // C4 an, Velocity 80
        0x00, 0x40, 0x46, // E4 an, Running Status
        0x60, 0x3C, 0x00, // nach 1 Beat: C4 aus (Velocity 0)
        0x00, 0xE0, 0x00, 0x50, // Pitchbend (wird ignoriert)
        0x30, 0x90, 0x43, 0x64, // nach 1/2 Beat: G4 an
        0x30, 0x80, 0x40, 0x40, // E4 aus (Note Off)
        0x00, 0x43, 0x40, // G4 aus, Running Status 0x80
        0x00, 0xB0, 0x40, 0x00, // Pedal los
        0x81, 0x40, 0x99, 0x24, 0x64, // nach 2 Beats (0xC0 = 192 Ticks): Kick auf Kanal 10
        0x18, 0x24, 0x00, // 1/4 Beat später aus
        0x00, 0xFF, 0x2F, 0x00,
    ];
    for t in [tempo_track, notes] {
        f.extend_from_slice(b"MTrk");
        f.extend_from_slice(&(t.len() as u32).to_be_bytes());
        f.extend_from_slice(t);
    }
    f
}

#[test]
fn foreign_file_is_read_like_other_programs_write_it() {
    let r = read_smf(&foreign_file()).unwrap();
    assert!((r.tempo_bpm.unwrap() - 100.0).abs() < 1e-9);
    assert_eq!(r.name.as_deref(), Some("Piano"));
    let got: Vec<(f64, f64, u8, u8)> = r
        .midi
        .notes
        .iter()
        .map(|n| (n.beat, n.length_beats, n.pitch, n.velocity))
        .collect();
    assert_eq!(
        got,
        vec![
            (0.0, 1.0, 60, 80),
            (0.0, 2.0, 64, 70),
            (1.5, 0.5, 67, 100),
            (4.0, 0.25, 36, 100),
        ]
    );
    let cc: Vec<(f64, u8, u8)> = r
        .midi
        .cc
        .iter()
        .map(|c| (c.beat, c.controller, c.value))
        .collect();
    assert_eq!(cc, vec![(0.0, 64, 127), (2.0, 64, 0)]);
    assert!((r.end_beats - 4.25).abs() < 1e-9);
    // Und wieder hinaus: WOA schreibt dieselben Noten in seinem eigenen Format 0.
    let again = read_smf(&write_smf(&r.midi, 100.0, "Piano")).unwrap();
    assert_eq!(again.midi, r.midi);
}
