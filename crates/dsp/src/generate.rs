//! Offline-Klangerzeugung für den Agenten (Control-Thread, nie im Audio-Thread):
//! Noten mit dem [`PolySynth`] rendern und einfache synthetische Drums
//! (Kick: Sinus mit Tonhöhen-Sweep, Snare: Ton plus Rauschen, Hi-Hat: hochpassgefiltertes
//! Rauschen) sowie Jingle-FX (Riser, Whoosh, Downlifter: Rauschen durch einen
//! wandernden Tiefpass, Paket J8). Das Ergebnis ist ein Mono-Puffer, den der Aufrufer als WAV ablegt.

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

/// Ein Jingle-Effekt aus Rauschen und Filter-Sweep (Paket J8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FxSweep {
    /// Rauschen schwillt an, der Filter öffnet sich (vor dem Einsatz).
    Riser,
    /// Filter öffnet und schließt sich, laut in der Mitte (Übergang).
    Whoosh,
    /// Umgekehrter Riser: setzt laut ein, Filter schließt sich (nach dem Einsatz).
    Downlifter,
}

/// Spitzenwert eines FX-Puffers (−3 dBFS), Platz für den Mix.
const FX_PEAK: f32 = 0.708;

/// Rendert einen Jingle-Effekt über `total` Samples. Spitze genau bei −3 dBFS,
/// Anfang und Ende ohne Knacken.
pub fn render_fx(kind: FxSweep, rate: f32, total: usize) -> Vec<f32> {
    let mut out = vec![0.0f32; total];
    if total == 0 {
        return out;
    }
    let mut noise = Noise(0x2545_F491_4F6C_DD1D);
    let (mut lp1, mut lp2) = (0.0f32, 0.0f32);
    let (lo, hi) = (150.0f32, 12_000.0f32.min(rate * 0.45));
    let last = (total.max(2) - 1) as f32;
    for (i, s) in out.iter_mut().enumerate() {
        let x = i as f32 / last; // 0..1
        // Lage des Sweeps (0 = dunkel, 1 = hell) und Lautstärke.
        let (sweep, env) = match kind {
            FxSweep::Riser => (x, x * x),
            FxSweep::Downlifter => (1.0 - x, (1.0 - x) * (1.0 - x)),
            FxSweep::Whoosh => {
                let b = (x * std::f32::consts::PI).sin();
                (b, b * b)
            }
        };
        let fc = lo * (hi / lo).powf(sweep);
        let a = 1.0 - (-std::f32::consts::TAU * fc / rate).exp();
        lp1 += a * (noise.next() - lp1);
        lp2 += a * (lp1 - lp2);
        *s = lp2 * env;
    }
    // Kurzer Einstieg (5 ms), damit ein Downlifter nicht knackt.
    let n = ((0.005 * rate) as usize).min(total);
    for (i, s) in out[..n].iter_mut().enumerate() {
        *s *= i as f32 / n as f32;
    }
    let peak = out.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    if peak > 0.0 {
        let g = FX_PEAK / peak;
        for s in out.iter_mut() {
            *s *= g;
        }
    }
    fade_tail(&mut out);
    out
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

    #[test]
    fn fx_sweeps_have_their_shape_and_stay_below_ceiling() {
        let rate = 48_000.0;
        let total = 96_000;
        let rms = |b: &[f32]| (b.iter().map(|s| s * s).sum::<f32>() / b.len() as f32).sqrt();
        for kind in [FxSweep::Riser, FxSweep::Whoosh, FxSweep::Downlifter] {
            let b = render_fx(kind, rate, total);
            assert_eq!(b.len(), total);
            assert!(peak(&b) <= MAX_PEAK, "{kind:?}");
            assert!(peak(&b) > 0.5, "{kind:?} too quiet");
            assert!(
                b[0].abs() < 1e-3 && b[total - 1].abs() < 1e-3,
                "{kind:?} clicks"
            );
            let (first, mid, end) = (
                rms(&b[..24_000]),
                rms(&b[36_000..60_000]),
                rms(&b[72_000..]),
            );
            match kind {
                FxSweep::Riser => assert!(end > 3.0 * first, "{kind:?} {first} {end}"),
                FxSweep::Downlifter => assert!(first > 3.0 * end, "{kind:?} {first} {end}"),
                FxSweep::Whoosh => assert!(mid > 2.0 * first && mid > 2.0 * end, "{kind:?}"),
            }
        }
        assert_eq!(render_fx(FxSweep::Riser, rate, 0).len(), 0);
        assert_eq!(render_fx(FxSweep::Riser, rate, 1).len(), 1);
    }
}
