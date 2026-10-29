//! Offline-Klangerzeugung für den Agenten (Control-Thread, nie im Audio-Thread):
//! Noten mit dem [`PolySynth`] rendern und einfache synthetische Drums
//! (Kick: Sinus mit Tonhöhen-Sweep, Snare: Ton plus Rauschen, Hi-Hat: hochpassgefiltertes
//! Rauschen). Das Ergebnis ist ein Mono-Puffer, den der Aufrufer als WAV ablegt.

use crate::instrument::{Instrument, NoteEvent};
use crate::synth::{PolySynth, Waveform};

/// Höchster Spitzenwert eines erzeugten Puffers (−1 dBFS).
pub const MAX_PEAK: f32 = 0.891;

/// Eine Note, Zeiten in Samples.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Note {
    /// Beginn ab Pufferanfang.
    pub start: u64,
    /// Haltedauer bis Note aus.
    pub length: u64,
    /// MIDI-Note (69 = A4).
    pub pitch: u8,
    /// 1..=127.
    pub velocity: u8,
}

/// Rendert `notes` mit einem [`PolySynth`] (16 Stimmen) über `total` Samples.
/// Spitzen über [`MAX_PEAK`] werden heruntergeregelt.
pub fn render_notes(wave: Waveform, notes: &[Note], rate: f32, total: usize) -> Vec<f32> {
    let mut synth = PolySynth::new(16, wave);
    synth.prepare(rate);
    // (Zeitpunkt, Reihenfolge: aus vor an, Event)
    let mut events: Vec<(u64, u8, NoteEvent)> = Vec::with_capacity(notes.len() * 2);
    for n in notes {
        let v = n.velocity.clamp(1, 127);
        events.push((n.start, 1, NoteEvent::note_on(n.pitch, v)));
        events.push((n.start + n.length.max(1), 0, NoteEvent::note_off(n.pitch)));
    }
    events.sort_by_key(|e| (e.0, e.1));
    let mut out = vec![0.0f32; total];
    let mut pos = 0usize;
    for (at, _, ev) in &events {
        let at = (*at as usize).min(total);
        if at > pos {
            synth.render(&mut out[pos..at]);
            pos = at;
        }
        synth.handle_event(ev);
    }
    if pos < total {
        synth.render(&mut out[pos..]);
    }
    limit_peak(&mut out);
    out
}

/// Ein Drum-Instrument.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Drum {
    Kick,
    Snare,
    HiHat,
}

/// Rendert Drum-Schläge (Startsample, Instrument, Stärke 0..=1) über `total` Samples.
pub fn render_drums(hits: &[(u64, Drum, f32)], rate: f32, total: usize) -> Vec<f32> {
    let mut out = vec![0.0f32; total];
    let mut noise = Noise(0x9E37_79B9_7F4A_7C15);
    for &(start, drum, vel) in hits {
        let start = start as usize;
        if start >= total {
            continue;
        }
        let vel = vel.clamp(0.0, 1.0);
        let dst = &mut out[start..];
        match drum {
            Drum::Kick => kick(dst, rate, vel),
            Drum::Snare => snare(dst, rate, vel, &mut noise),
            Drum::HiHat => hihat(dst, rate, vel, &mut noise),
        }
    }
    limit_peak(&mut out);
    out
}

fn kick(dst: &mut [f32], rate: f32, vel: f32) {
    let n = ((0.35 * rate) as usize).min(dst.len());
    let mut phase = 0.0f32;
    for (i, s) in dst[..n].iter_mut().enumerate() {
        let t = i as f32 / rate;
        // Tonhöhe fällt von 150 auf 45 Hz.
        let f = 45.0 + 105.0 * (-t * 30.0).exp();
        phase += f / rate;
        let env = (-t * 9.0).exp();
        *s += (phase * std::f32::consts::TAU).sin() * env * vel * 0.9;
    }
}

fn snare(dst: &mut [f32], rate: f32, vel: f32, noise: &mut Noise) {
    let n = ((0.2 * rate) as usize).min(dst.len());
    let mut prev = 0.0f32;
    for (i, s) in dst[..n].iter_mut().enumerate() {
        let t = i as f32 / rate;
        let tone = (t * 185.0 * std::f32::consts::TAU).sin() * (-t * 30.0).exp();
        let w = noise.next();
        let hp = w - prev; // einfacher Hochpass
        prev = w;
        *s += (0.35 * tone + 0.45 * hp * (-t * 18.0).exp()) * vel;
    }
}

fn hihat(dst: &mut [f32], rate: f32, vel: f32, noise: &mut Noise) {
    let n = ((0.06 * rate) as usize).min(dst.len());
    let (mut p1, mut p2) = (0.0f32, 0.0f32);
    for (i, s) in dst[..n].iter_mut().enumerate() {
        let t = i as f32 / rate;
        let w = noise.next();
        // Zweite Differenz: kräftiger Hochpass.
        let hp = w - 2.0 * p1 + p2;
        p2 = p1;
        p1 = w;
        *s += 0.12 * hp * (-t * 70.0).exp() * vel;
    }
}

/// Regelt den Puffer herunter, wenn eine Spitze über [`MAX_PEAK`] liegt.
fn limit_peak(buf: &mut [f32]) {
    let peak = buf.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    if peak > MAX_PEAK {
        let g = MAX_PEAK / peak;
        for s in buf.iter_mut() {
            *s *= g;
        }
    }
    fade_tail(buf);
}

/// Blendet die letzten 5 ms (bei 48 kHz) linear aus, damit ein abgeschnittener
/// Ausklang an der Clip-Grenze nicht knackt (Loops).
fn fade_tail(buf: &mut [f32]) {
    const TAIL: usize = 240;
    let n = TAIL.min(buf.len());
    let start = buf.len() - n;
    for (i, s) in buf[start..].iter_mut().enumerate() {
        *s *= (n - 1 - i) as f32 / n as f32;
    }
}

/// Deterministisches Rauschen (xorshift64), −1..1.
struct Noise(u64);

impl Noise {
    fn next(&mut self) -> f32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        ((x >> 40) as f32 / (1u64 << 24) as f32) * 2.0 - 1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn peak(b: &[f32]) -> f32 {
        b.iter().fold(0.0f32, |m, s| m.max(s.abs()))
    }

    #[test]
    fn notes_sound_where_placed_and_stay_below_ceiling() {
        let notes: Vec<Note> = [60u8, 64, 67, 72]
            .iter()
            .map(|&p| Note {
                start: 24_000,
                length: 24_000,
                pitch: p,
                velocity: 127,
            })
            .collect();
        let b = render_notes(Waveform::Saw, &notes, 48_000.0, 96_000);
        assert_eq!(b.len(), 96_000);
        assert_eq!(peak(&b[..24_000]), 0.0);
        assert!(peak(&b[24_000..48_000]) > 0.1);
        assert!(peak(&b) <= MAX_PEAK + 1e-6);
        // Nach Release (0.2 s) still.
        assert!(peak(&b[70_000..]) < 1e-4);
    }

    #[test]
    fn cut_notes_fade_out_at_the_end() {
        let n = Note {
            start: 0,
            length: 48_000,
            pitch: 45,
            velocity: 127,
        };
        let b = render_notes(Waveform::Square, &[n], 48_000.0, 24_000);
        assert!(peak(&b[20_000..23_000]) > 0.1);
        assert_eq!(b[b.len() - 1], 0.0);
        assert!(peak(&b[b.len() - 10..]) < 0.05);
    }

    #[test]
    fn drums_are_deterministic_and_bounded() {
        let hits = [
            (0, Drum::Kick, 1.0),
            (12_000, Drum::HiHat, 1.0),
            (24_000, Drum::Snare, 1.0),
        ];
        let a = render_drums(&hits, 48_000.0, 48_000);
        let b = render_drums(&hits, 48_000.0, 48_000);
        assert_eq!(a, b);
        assert!(peak(&a[..1000]) > 0.1);
        assert!(peak(&a[24_000..25_000]) > 0.05);
        assert!(peak(&a) <= MAX_PEAK + 1e-6);
        // Schlag hinter dem Ende wird ignoriert.
        assert!(
            render_drums(&[(99_999, Drum::Kick, 1.0)], 48_000.0, 10)
                .iter()
                .all(|s| *s == 0.0)
        );
    }
}
