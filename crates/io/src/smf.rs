//! Standard MIDI Files (`.mid`) lesen und schreiben (Paket 12).
//!
//! Eigener kleiner Leser/Schreiber statt `midly` (Unlicense ist in `deny.toml` nicht
//! erlaubt, offene Frage an André in #5). Unterstützt Format 0 und 1 mit
//! Ticks-pro-Viertel, Running Status, Meta- und SysEx-Events (überlesen), Note On mit
//! Velocity 0 als Note aus und Controller (CC). Zeiten werden in Beats (Viertel)
//! umgerechnet; Tempo-Wechsel ändern Beats nicht, das erste Tempo wird gemeldet.

use woa_model::{MidiCc, MidiData, MidiNote, midi::MAX_MIDI_EVENTS};

/// Auflösung beim Schreiben (Ticks je Viertel).
pub const WRITE_PPQ: u16 = 480;
/// Größte Datei, die gelesen wird (Speicherschutz).
pub const MAX_FILE_BYTES: usize = 32 << 20;

/// Ergebnis von [`read_smf`].
#[derive(Debug, Clone, PartialEq)]
pub struct SmfImport {
    /// Erstes Tempo der Datei (BPM), falls angegeben.
    pub tempo_bpm: Option<f64>,
    /// Name der ersten Spur (Meta 0x03), falls angegeben.
    pub name: Option<String>,
    /// Alle Noten und Controller aller Spuren und Kanäle, sortiert.
    pub midi: MidiData,
    /// Ende der Datei in Beats (letztes Event, mindestens Ende der letzten Note).
    pub end_beats: f64,
}

/// Fehler beim Lesen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmfError(pub String);

impl std::fmt::Display for SmfError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "midi file: {}", self.0)
    }
}

impl std::error::Error for SmfError {}

fn err<T>(what: impl Into<String>) -> Result<T, SmfError> {
    Err(SmfError(what.into()))
}

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], SmfError> {
        let end = self
            .pos
            .checked_add(n)
            .filter(|&e| e <= self.data.len())
            .ok_or_else(|| SmfError("unexpected end of data".into()))?;
        let s = &self.data[self.pos..end];
        self.pos = end;
        Ok(s)
    }

    fn u8(&mut self) -> Result<u8, SmfError> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, SmfError> {
        let b = self.take(2)?;
        Ok(u16::from_be_bytes([b[0], b[1]]))
    }

    fn u32(&mut self) -> Result<u32, SmfError> {
        let b = self.take(4)?;
        Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }

    /// Variable-length quantity, höchstens 4 Bytes (28 Bit).
    fn vlq(&mut self) -> Result<u32, SmfError> {
        let mut v = 0u32;
        for _ in 0..4 {
            let b = self.u8()?;
            v = (v << 7) | u32::from(b & 0x7F);
            if b & 0x80 == 0 {
                return Ok(v);
            }
        }
        err("variable-length number too long")
    }

    fn done(&self) -> bool {
        self.pos >= self.data.len()
    }
}

/// Liest eine `.mid`-Datei aus dem Speicher.
pub fn read_smf(bytes: &[u8]) -> Result<SmfImport, SmfError> {
    if bytes.len() > MAX_FILE_BYTES {
        return err("file too large");
    }
    let mut r = Reader {
        data: bytes,
        pos: 0,
    };
    if r.take(4)? != b"MThd" {
        return err("not a MIDI file (MThd missing)");
    }
    let hlen = r.u32()? as usize;
    if hlen < 6 {
        return err("header too short");
    }
    let format = r.u16()?;
    let ntracks = r.u16()?;
    let division = r.u16()?;
    r.take(hlen - 6)?;
    if format > 1 {
        return err(format!("format {format} not supported (only 0 and 1)"));
    }
    if division & 0x8000 != 0 || division == 0 {
        return err("SMPTE time division not supported");
    }
    let ppq = f64::from(division);
    let mut tempo_bpm = None;
    let mut name = None;
    let mut notes: Vec<MidiNote> = Vec::new();
    let mut cc: Vec<MidiCc> = Vec::new();
    let mut end_ticks = 0u64;
    let mut seen = 0;
    while seen < ntracks && !r.done() {
        let id = r.take(4)?;
        let len = r.u32()? as usize;
        let chunk = r.take(len)?;
        if id != b"MTrk" {
            continue; // unbekannte Chunks überlesen
        }
        seen += 1;
        let mut t = Reader {
            data: chunk,
            pos: 0,
        };
        let mut tick = 0u64;
        let mut status: Option<u8> = None;
        // Offene Noten je (Kanal, Tonhöhe): Start-Tick und Velocity, FIFO.
        let mut open: Vec<Vec<(u64, u8)>> = vec![Vec::new(); 16 * 128];
        while !t.done() {
            tick += u64::from(t.vlq()?);
            let first = t.u8()?;
            let (st, d0) = if first & 0x80 != 0 {
                (first, None)
            } else {
                match status {
                    Some(s) => (s, Some(first)),
                    None => return err("data byte without status"),
                }
            };
            match st {
                0xFF => {
                    status = None;
                    let kind = t.u8()?;
                    let l = t.vlq()? as usize;
                    let data = t.take(l)?;
                    match kind {
                        0x51 if l == 3 && tempo_bpm.is_none() => {
                            let us = u32::from(data[0]) << 16
                                | u32::from(data[1]) << 8
                                | u32::from(data[2]);
                            if us > 0 {
                                tempo_bpm = Some(60_000_000.0 / f64::from(us));
                            }
                        }
                        0x03 if name.is_none() && !data.is_empty() => {
                            name = Some(String::from_utf8_lossy(data).trim().to_string());
                        }
                        0x2F => break,
                        _ => {}
                    }
                }
                0xF0 | 0xF7 => {
                    status = None;
                    let l = t.vlq()? as usize;
                    t.take(l)?;
                }
                0xF1..=0xFE => return err(format!("unexpected status {st:#x} in file")),
                _ => {
                    status = Some(st);
                    let a = match d0 {
                        Some(a) => a,
                        None => t.u8()?,
                    };
                    let b = if matches!(st & 0xF0, 0xC0 | 0xD0) {
                        0
                    } else {
                        t.u8()?
                    };
                    let (a, b) = (a & 0x7F, b & 0x7F);
                    let ch = usize::from(st & 0x0F);
                    let slot = ch * 128 + usize::from(a);
                    match st & 0xF0 {
                        0x90 if b > 0 => open[slot].push((tick, b)),
                        0x80 | 0x90 => {
                            if !open[slot].is_empty() {
                                let (start, vel) = open[slot].remove(0);
                                push_note(&mut notes, start, tick, a, vel, ppq)?;
                            }
                        }
                        0xB0 => {
                            if cc.len() >= MAX_MIDI_EVENTS {
                                return err("too many controller events");
                            }
                            cc.push(MidiCc {
                                beat: tick as f64 / ppq,
                                controller: a,
                                value: b,
                            });
                        }
                        _ => {} // Aftertouch, Program Change, Pitchbend: noch nicht im Clip
                    }
                }
            }
            end_ticks = end_ticks.max(tick);
        }
        // Nie beendete Noten enden am Spurende.
        for (slot, list) in open.iter().enumerate() {
            for &(start, vel) in list {
                push_note(&mut notes, start, tick, (slot % 128) as u8, vel, ppq)?;
            }
        }
    }
    if seen == 0 {
        return err("no track (MTrk) found");
    }
    let mut midi = MidiData { notes, cc };
    midi.sort();
    let end_beats = (end_ticks as f64 / ppq).max(midi.end_beats());
    Ok(SmfImport {
        tempo_bpm,
        name: name.filter(|n| !n.is_empty()),
        midi,
        end_beats,
    })
}

fn push_note(
    notes: &mut Vec<MidiNote>,
    start: u64,
    end: u64,
    pitch: u8,
    velocity: u8,
    ppq: f64,
) -> Result<(), SmfError> {
    if notes.len() >= MAX_MIDI_EVENTS {
        return err("too many notes");
    }
    // Note mit Länge 0 (an und aus im selben Tick) bekommt einen Tick.
    let len = end.saturating_sub(start).max(1);
    notes.push(MidiNote {
        beat: start as f64 / ppq,
        length_beats: len as f64 / ppq,
        pitch,
        velocity,
    });
    Ok(())
}

fn write_vlq(out: &mut Vec<u8>, mut v: u32) {
    let mut buf = [0u8; 4];
    let mut n = 0;
    loop {
        buf[n] = (v & 0x7F) as u8;
        n += 1;
        v >>= 7;
        if v == 0 || n == 4 {
            break;
        }
    }
    for i in (0..n).rev() {
        out.push(buf[i] | if i > 0 { 0x80 } else { 0 });
    }
}

fn ticks(beat: f64) -> u64 {
    (beat.max(0.0) * f64::from(WRITE_PPQ)).round() as u64
}

/// Schreibt Noten und Controller als SMF Format 0 (ein Track, Kanal 1) mit Tempo.
/// `name` wird als Spurname (Meta 0x03) eingetragen.
pub fn write_smf(midi: &MidiData, tempo_bpm: f64, name: &str) -> Vec<u8> {
    // (Tick, Reihenfolge, Bytes): bei gleichem Tick erst Note aus, dann CC, dann Note an.
    let mut events: Vec<(u64, u8, [u8; 3])> = Vec::new();
    for n in &midi.notes {
        let p = n.pitch.min(127);
        let start = ticks(n.beat);
        let end = ticks(n.beat + n.length_beats).max(start + 1);
        events.push((start, 2, [0x90, p, n.velocity.clamp(1, 127)]));
        events.push((end, 0, [0x80, p, 0x40]));
    }
    for c in &midi.cc {
        events.push((
            ticks(c.beat),
            1,
            [0xB0, c.controller.min(127), c.value.min(127)],
        ));
    }
    events.sort_by_key(|&(t, order, b)| (t, order, b[1]));

    let mut trk = Vec::new();
    // Spurname
    let name = name.as_bytes();
    if !name.is_empty() {
        trk.push(0);
        trk.extend_from_slice(&[0xFF, 0x03]);
        write_vlq(&mut trk, name.len().min(0x0FFF_FFFF) as u32);
        trk.extend_from_slice(name);
    }
    // Tempo
    let bpm = if tempo_bpm.is_finite() && tempo_bpm > 0.0 {
        tempo_bpm
    } else {
        120.0
    };
    let us = (60_000_000.0 / bpm).round().clamp(1.0, 16_777_215.0) as u32;
    trk.extend_from_slice(&[
        0,
        0xFF,
        0x51,
        3,
        (us >> 16) as u8,
        (us >> 8) as u8,
        us as u8,
    ]);
    let mut last = 0u64;
    for (t, _, b) in events {
        let delta = (t - last).min(0x0FFF_FFFF) as u32;
        write_vlq(&mut trk, delta);
        trk.extend_from_slice(&b);
        last = t;
    }
    trk.extend_from_slice(&[0, 0xFF, 0x2F, 0]);

    let mut out = Vec::with_capacity(trk.len() + 22);
    out.extend_from_slice(b"MThd");
    out.extend_from_slice(&6u32.to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&1u16.to_be_bytes());
    out.extend_from_slice(&WRITE_PPQ.to_be_bytes());
    out.extend_from_slice(b"MTrk");
    out.extend_from_slice(&(trk.len() as u32).to_be_bytes());
    out.extend_from_slice(&trk);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vlq_examples_from_the_spec() {
        for (v, bytes) in [
            (0u32, &[0x00u8][..]),
            (0x40, &[0x40]),
            (0x7F, &[0x7F]),
            (0x80, &[0x81, 0x00]),
            (0x2000, &[0xC0, 0x00]),
            (0x3FFF, &[0xFF, 0x7F]),
            (0x1F_FFFF, &[0xFF, 0xFF, 0x7F]),
            (0x0FFF_FFFF, &[0xFF, 0xFF, 0xFF, 0x7F]),
        ] {
            let mut out = Vec::new();
            write_vlq(&mut out, v);
            assert_eq!(out, bytes, "{v:#x}");
            let mut r = Reader {
                data: bytes,
                pos: 0,
            };
            assert_eq!(r.vlq().unwrap(), v);
        }
    }

    #[test]
    fn garbage_is_an_error_not_a_panic() {
        for bad in [
            &b""[..],
            b"MThd",
            b"RIFF\0\0\0\x06\0\0\0\x01\x01\xe0",
            b"MThd\0\0\0\x06\0\x02\0\x01\x01\xe0",
            b"MThd\0\0\0\x06\0\0\0\x01\xe7\x28",
            b"MThd\0\0\0\x06\0\0\0\x01\x01\xe0MTrk\0\0\0\x04\0\x40\x40\x40",
            b"MThd\0\0\0\x06\0\0\0\x01\x01\xe0MTrk\xff\xff\xff\xff",
        ] {
            assert!(read_smf(bad).is_err(), "{bad:?}");
        }
    }
}
