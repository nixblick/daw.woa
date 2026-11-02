//! DJ-Paket, Audio-Seite: was der Mixer je Party-Seite in der DJ-Ansicht rechnet.
//!
//! In der Ansicht „Einfach“ bleibt alles wie bisher (bitgenau derselbe Weg). Schaltet der
//! Controller die DJ-Ansicht ein ([`DjCmd::Mode`]), liest jede Seite über einen
//! [`DeckReader`] (Tempo, Keylock, Scratch, Schleife) und läuft durch Trim, EQ, Filter und
//! Kanalfader; der Crossfader nimmt die eingestellte Kurve, der Master-Pegel wirkt auf die
//! Summe der Party.
//!
//! Der **Kopfhörer-Bus** (Cue der Kanäle vor dem Fader, gemischt mit dem Master) geht nur über
//! eine eigene Taktverbindung zum Vorhör-Ausgang ([`crate::pfl`]). Er wird nie in den Musikzweig,
//! den Mitschnitt oder den Sende-Ring gemischt.
//!
//! Alles hier ist echtzeitfest: feste Puffer, keine Allokation, kein Lock, kein I/O.

use woa_dsp::dj::{Curve, DeckReader, DjEq, DjFilter, ReadCtl, eq_gain, headphone_gains};

use crate::level::Smoother;

/// Glättung der Pegelregler (Sekunden).
const LEVEL_TAU: f32 = 0.01;

/// Einstellungen der DJ-Ansicht für den Mixer (flüchtig, nichts davon wird gespeichert).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DjCmd {
    /// DJ-Ansicht an (an: Leser, EQ, Filter, Fader; aus: der alte Weg).
    Mode(bool),
    /// Tempo einer Seite (1 = Originaltempo, 1,08 = +8 %). Schließt Anschieben ein.
    Speed {
        side: usize,
        speed: f64,
    },
    /// Scratch: `Some(v)` überschreibt das Tempo (auch negativ), `None` gibt es zurück.
    Scratch {
        side: usize,
        speed: Option<f64>,
    },
    Keylock {
        side: usize,
        on: bool,
    },
    /// Schleife in Frames (Anfang, Ende) oder aus.
    Loop {
        side: usize,
        frames: Option<(f64, f64)>,
    },
    /// Stelle setzen (Frames, gebrochen; Hot Cue, Cue).
    Jump {
        side: usize,
        frame: f64,
    },
    /// Trim als Faktor.
    Trim {
        side: usize,
        gain: f32,
    },
    /// EQ als Reglerstellung −1..1 (−1 = Kill, 0 = neutral, +1 = +6 dB).
    Eq {
        side: usize,
        lo: f32,
        mid: f32,
        hi: f32,
    },
    /// Filter −1 (Tiefpass) … 0 (aus) … +1 (Hochpass).
    Filter {
        side: usize,
        value: f32,
    },
    /// Kanalfader als Faktor 0..1.
    Fader {
        side: usize,
        gain: f32,
    },
    /// Cue-Taste (Kanal im Kopfhörer).
    Cue {
        side: usize,
        on: bool,
    },
    /// Master-Pegel der Party (Faktor).
    Master(f32),
    /// Kopfhörer: Mix 0 = Cue, 1 = Master; Pegel als Faktor.
    Phones {
        mix: f32,
        level: f32,
    },
    /// Kurve des Crossfaders.
    Curve(Curve),
}

/// Ein Kanal (eine Party-Seite) in der DJ-Ansicht.
pub(crate) struct DjChan {
    pub reader: Box<DeckReader>,
    pub ctl: ReadCtl,
    /// Tempo vom Fader (ohne Scratch).
    pub speed: f64,
    pub scratch: Option<f64>,
    pub eq: DjEq,
    pub filter: DjFilter,
    pub trim: Smoother,
    pub fader: Smoother,
    pub cue: bool,
    /// Stelle, die der Mixer zuletzt aus dem Leser übernommen hat (erkennt Sprünge von außen).
    pub synced: usize,
    /// Spitze nach dem Kanalfader seit dem letzten Abholen.
    pub peak: f32,
    /// Sprung, der nach der laufenden Stopp-Blende gilt (Cue loslassen: erst ausblenden).
    pub pending_jump: Option<f64>,
}

impl DjChan {
    fn new(rate: f32) -> Self {
        Self {
            reader: Box::new(DeckReader::new()),
            ctl: ReadCtl::whole(0),
            speed: 1.0,
            scratch: None,
            eq: DjEq::new(rate as u32),
            filter: DjFilter::new(rate as u32),
            trim: Smoother::new(1.0, LEVEL_TAU, rate),
            fader: Smoother::new(1.0, LEVEL_TAU, rate),
            cue: false,
            synced: usize::MAX,
            peak: 0.0,
            pending_jump: None,
        }
    }

    /// Wirksames Tempo (Scratch geht vor).
    #[inline]
    pub fn effective_speed(&self) -> f64 {
        self.scratch.unwrap_or(self.speed)
    }
}

/// Der DJ-Teil des Mixers: zwei Kanäle, Master, Kopfhörer, Kurve, Kopfhörer-Ring.
pub(crate) struct DjMix {
    pub on: bool,
    pub chans: [DjChan; 2],
    pub master: Smoother,
    pub phones_mix: f32,
    pub phones_level: Smoother,
    pub curve: Curve,
    /// Taktverbindung zum Vorhör-Ausgang (nur der Kopfhörer-Bus).
    pub cue_bus: Option<basedrop::Owned<woa_engine::pairs::LinkTx>>,
    /// Spitze des Kopfhörer-Busses seit dem letzten Abholen.
    pub phones_peak: f32,
    /// Spitze der Party-Summe nach dem Master.
    pub master_peak: f32,
}

impl DjMix {
    pub fn new(rate: f32) -> Self {
        Self {
            on: false,
            chans: [DjChan::new(rate), DjChan::new(rate)],
            master: Smoother::new(1.0, LEVEL_TAU, rate),
            phones_mix: 0.5,
            phones_level: Smoother::new(1.0, LEVEL_TAU, rate),
            curve: Curve::Soft,
            cue_bus: None,
            phones_peak: 0.0,
            master_peak: 0.0,
        }
    }

    /// Nach dem Veröffentlichen: Spitzen zurücksetzen.
    pub fn reset_peaks(&mut self) {
        for ch in &mut self.chans {
            ch.peak = 0.0;
        }
        self.phones_peak = 0.0;
        self.master_peak = 0.0;
    }

    /// Wendet ein Kommando an (im Audio-Thread, ohne Allokation).
    pub fn apply(&mut self, c: DjCmd) {
        let fin = |v: f32, d: f32| if v.is_finite() { v } else { d };
        match c {
            DjCmd::Mode(on) => {
                if on && !self.on {
                    for ch in &mut self.chans {
                        ch.synced = usize::MAX;
                        ch.eq.snap();
                        ch.filter.snap();
                    }
                }
                self.on = on;
            }
            DjCmd::Speed { side, speed } => {
                if let Some(ch) = self.chans.get_mut(side)
                    && speed.is_finite()
                {
                    ch.speed = speed.clamp(0.0, 4.0);
                }
            }
            DjCmd::Scratch { side, speed } => {
                if let Some(ch) = self.chans.get_mut(side) {
                    ch.scratch = speed.filter(|v| v.is_finite()).map(|v| v.clamp(-8.0, 8.0));
                }
            }
            DjCmd::Keylock { side, on } => {
                if let Some(ch) = self.chans.get_mut(side) {
                    ch.ctl.keylock = on;
                }
            }
            DjCmd::Loop { side, frames } => {
                if let Some(ch) = self.chans.get_mut(side) {
                    ch.ctl.looped = frames.filter(|(a, b)| a.is_finite() && b > a);
                }
            }
            DjCmd::Jump { side, frame } => {
                if let Some(ch) = self.chans.get_mut(side) {
                    ch.reader.seek(frame);
                    // Der Mixer stellt die Seite auf dieselbe Stelle (siehe `Mixer`).
                    ch.synced = frame.max(0.0) as usize;
                }
            }
            DjCmd::Trim { side, gain } => {
                if let Some(ch) = self.chans.get_mut(side) {
                    ch.trim.set_target(fin(gain, 1.0).clamp(0.0, 4.0));
                }
            }
            DjCmd::Eq { side, lo, mid, hi } => {
                if let Some(ch) = self.chans.get_mut(side) {
                    ch.eq.set(
                        eq_gain(fin(lo, 0.0)),
                        eq_gain(fin(mid, 0.0)),
                        eq_gain(fin(hi, 0.0)),
                    );
                }
            }
            DjCmd::Filter { side, value } => {
                if let Some(ch) = self.chans.get_mut(side) {
                    ch.filter.set(fin(value, 0.0));
                }
            }
            DjCmd::Fader { side, gain } => {
                if let Some(ch) = self.chans.get_mut(side) {
                    ch.fader.set_target(fin(gain, 1.0).clamp(0.0, 1.0));
                }
            }
            DjCmd::Cue { side, on } => {
                if let Some(ch) = self.chans.get_mut(side) {
                    ch.cue = on;
                }
            }
            DjCmd::Master(g) => self.master.set_target(fin(g, 1.0).clamp(0.0, 2.0)),
            DjCmd::Phones { mix, level } => {
                self.phones_mix = fin(mix, 0.5).clamp(0.0, 1.0);
                self.phones_level
                    .set_target(fin(level, 1.0).clamp(0.0, 2.0));
            }
            DjCmd::Curve(c) => self.curve = c,
        }
    }

    /// Kopfhörer-Frame aus Cue-Summe und Master; geht nur in den Kopfhörer-Ring.
    #[inline]
    pub fn phones(&mut self, cue: (f32, f32), master: (f32, f32)) {
        let (gc, gm) = headphone_gains(self.phones_mix);
        let lv = self.phones_level.tick();
        let l = (cue.0 * gc + master.0 * gm) * lv;
        let r = (cue.1 * gc + master.1 * gm) * lv;
        self.phones_peak = self.phones_peak.max(l.abs()).max(r.abs());
        if let Some(tx) = self.cue_bus.as_mut() {
            tx.push(&[l], &[r]);
        }
    }
}
