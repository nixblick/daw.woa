//! Vorhören (PFL, Paket 15): ein Titel oder Cart im Kopfhörer, getrennt von der Sendung.
//!
//! [`PflPlayer`] ist ein eigener kleiner Spieler im Audio-Thread mit eigener `rtrb`-Queue.
//! Er hat keinen Zugang zum [`crate::Mixer`], zum Mitschnitt oder zum Sende-Ring: Was hier
//! läuft, kann den Sendebus nicht erreichen. Er läuft entweder im selben Callback wie der
//! Mixer (Kanal 3/4 desselben Geräts) oder in einem eigenen Strom auf einem zweiten Gerät.
//! Keine Allokation, kein Lock, kein I/O im [`PflPlayer::process`].

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use basedrop::Shared;

use crate::audio::Stereo;

/// Kommandos im Flug zum PFL-Spieler.
const QUEUE: usize = 32;
/// Ein- und Ausblenden gegen Knacken (Sekunden).
const RAMP_SECONDS: f32 = 0.005;

/// Kommandos an den PFL-Spieler.
pub enum PflCmd {
    /// Ab Frame `start` spielen (ersetzt, was gerade vorgehört wird).
    Play { buf: Shared<Stereo>, start: usize },
    /// Anhalten.
    Stop,
}

/// Lock-frei lesbarer Zustand.
#[derive(Debug, Default)]
pub struct PflStatus {
    pub playing: AtomicBool,
    /// Position im vorgehörten Puffer (Frames, in der PFL-Rate).
    pub position: AtomicU64,
}

/// Steuerseite (Control-Thread): schickt Puffer, die in der PFL-Rate dekodiert sind.
pub struct PflHandle {
    pub(crate) cmds: rtrb::Producer<PflCmd>,
    pub status: Arc<PflStatus>,
    /// Abtastrate des PFL-Ausgangs.
    pub rate: u32,
}

/// Der Spieler im Audio-Thread.
pub struct PflPlayer {
    cmds: rtrb::Consumer<PflCmd>,
    status: Arc<PflStatus>,
    voice: Option<(Shared<Stereo>, usize)>,
    /// Ausblenden läuft noch so viele Frames, danach Stille.
    stopping: Option<usize>,
    ramp: usize,
    fade_in: usize,
}

/// Legt Steuerseite und Spieler für einen Ausgang mit `rate` an.
pub fn channel(rate: u32) -> (PflHandle, PflPlayer) {
    let (p, c) = rtrb::RingBuffer::new(QUEUE);
    let status = Arc::new(PflStatus::default());
    let ramp = ((RAMP_SECONDS * rate.max(1) as f32) as usize).max(1);
    (
        PflHandle {
            cmds: p,
            status: Arc::clone(&status),
            rate: rate.max(1),
        },
        PflPlayer {
            cmds: c,
            status,
            voice: None,
            stopping: None,
            ramp,
            fade_in: 0,
        },
    )
}

impl PflHandle {
    pub fn is_playing(&self) -> bool {
        self.status.playing.load(Ordering::Acquire)
    }

    /// Position in Sekunden.
    pub fn position_seconds(&self) -> f64 {
        self.status.position.load(Ordering::Acquire) as f64 / f64::from(self.rate)
    }
}

impl PflPlayer {
    fn drain(&mut self) {
        while let Ok(cmd) = self.cmds.pop() {
            match cmd {
                PflCmd::Play { buf, start } => {
                    let start = start.min(buf.frames());
                    self.voice = Some((buf, start));
                    self.stopping = None;
                    self.fade_in = self.ramp;
                }
                PflCmd::Stop => {
                    if self.voice.is_some() && self.stopping.is_none() {
                        self.stopping = Some(self.ramp);
                    }
                }
            }
        }
    }

    /// Rechnet einen Block Kopfhörer-Signal (überschreibt `left`/`right`).
    pub fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        self.drain();
        let n = left.len().min(right.len());
        for i in 0..n {
            let (mut l, mut r) = (0.0, 0.0);
            if let Some((buf, pos)) = self.voice.as_mut() {
                if *pos < buf.frames() {
                    let mut g = 1.0f32;
                    if self.fade_in > 0 {
                        g = 1.0 - self.fade_in as f32 / self.ramp as f32;
                        self.fade_in -= 1;
                    }
                    if let Some(k) = self.stopping {
                        g *= k as f32 / self.ramp as f32;
                    }
                    l = buf.left[*pos] * g;
                    r = buf.right[*pos] * g;
                    *pos += 1;
                }
                let ended = *pos >= buf.frames();
                match self.stopping {
                    Some(k) if k <= 1 => self.voice = None,
                    Some(k) => self.stopping = Some(k - 1),
                    None if ended => self.voice = None,
                    None => {}
                }
                if self.voice.is_none() {
                    self.stopping = None;
                }
            }
            left[i] = l;
            right[i] = r;
        }
        let pos = self.voice.as_ref().map_or(0, |(_, p)| *p as u64);
        self.status
            .playing
            .store(self.voice.is_some(), Ordering::Release);
        self.status.position.store(pos, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plays_from_start_and_stops() {
        let collector = basedrop::Collector::new();
        let (mut h, mut p) = channel(1000);
        let buf = Stereo {
            left: vec![1.0; 100],
            right: vec![-1.0; 100],
        };
        let buf = Shared::new(&collector.handle(), buf);
        assert!(
            h.cmds
                .push(PflCmd::Play {
                    buf: buf.clone(),
                    start: 40
                })
                .is_ok()
        );
        let mut l = vec![0.0f32; 32];
        let mut r = vec![0.0f32; 32];
        assert_no_alloc::assert_no_alloc(|| p.process(&mut l, &mut r));
        assert!(h.is_playing());
        assert_eq!(h.status.position.load(Ordering::Acquire), 72);
        assert!((l[31] - 1.0).abs() < 1e-6 && (r[31] + 1.0).abs() < 1e-6);
        assert!(l[0].abs() < 1e-6, "fade in");
        assert!(h.cmds.push(PflCmd::Stop).is_ok());
        assert_no_alloc::assert_no_alloc(|| p.process(&mut l, &mut r));
        assert!(!h.is_playing());
        assert_eq!(l[31], 0.0);
        // Ende des Puffers beendet das Vorhören.
        assert!(h.cmds.push(PflCmd::Play { buf, start: 90 }).is_ok());
        p.process(&mut l, &mut r);
        assert!(!h.is_playing());
    }
}
