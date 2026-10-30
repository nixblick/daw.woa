//! Die Gast-Bank im Audio-Thread: je Kanal Jitterpuffer, Fader, Stumm und Pegel; dazu die
//! Rückwege (Mix-Minus). Keine Allokation, kein Lock, kein I/O, kein Logging.

use std::sync::Arc;

use crate::MAX_GUESTS;
use crate::jitter::{JitterReader, SlotStats};

/// Glättung der Fader gegen Zipper-Noise (Sekunden).
const FADER_TAU: f32 = 0.015;
/// Ein- und Ausblenden beim Start und nach einem Aussetzer (Sekunden).
const GATE_TAU: f32 = 0.005;

/// Befehle vom Control-Thread.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GuestCmd {
    /// Kanal an (Puffer beginnt beim Vorlauf) oder aus (Puffer wird geleert).
    Activate {
        slot: usize,
        on: bool,
    },
    /// Fader (linear).
    Gain {
        slot: usize,
        gain: f32,
    },
    Mute {
        slot: usize,
        on: bool,
    },
    /// Rückweg dieses Gastes im Abhörausgang (Kopfhörer); `None`: normaler Abhörmix.
    Monitor(Option<usize>),
}

/// Einpoliger Glätter.
#[derive(Debug, Clone, Copy)]
struct Ramp {
    value: f32,
    target: f32,
    coef: f32,
}

impl Ramp {
    fn new(value: f32, tau: f32, rate: f32) -> Self {
        let n = (tau * rate).max(1.0);
        Self {
            value,
            target: value,
            coef: 1.0 - (-1.0 / n).exp(),
        }
    }

    #[inline]
    fn tick(&mut self) -> f32 {
        self.value += (self.target - self.value) * self.coef;
        if (self.target - self.value).abs() < 1e-5 {
            self.value = self.target;
        }
        self.value
    }
}

struct Slot {
    reader: JitterReader,
    stats: Arc<SlotStats>,
    active: bool,
    muted: bool,
    gain: f32,
    fader: Ramp,
    gate: Ramp,
}

impl Slot {
    fn update_fader(&mut self) {
        self.fader.target = if self.muted { 0.0 } else { self.gain };
    }
}

/// Alle Gast-Kanäle. Lebt im Audio-Thread.
pub struct GuestBank {
    cmds: rtrb::Consumer<GuestCmd>,
    slots: [Slot; MAX_GUESTS],
    /// Beitrag jedes Kanals im aktuellen Frame (nach Fader).
    own: [(f32, f32); MAX_GUESTS],
    /// Spitzen im aktuellen Block.
    block_peak: [f32; MAX_GUESTS],
    active_count: usize,
    monitor: Option<usize>,
}

impl GuestBank {
    pub(crate) fn new(
        cmds: rtrb::Consumer<GuestCmd>,
        readers: Vec<(JitterReader, Arc<SlotStats>)>,
        rate: u32,
    ) -> Option<Self> {
        let r = rate.max(1) as f32;
        let slots: [Slot; MAX_GUESTS] = readers
            .into_iter()
            .map(|(reader, stats)| Slot {
                reader,
                stats,
                active: false,
                muted: false,
                gain: 1.0,
                fader: Ramp::new(1.0, FADER_TAU, r),
                gate: Ramp::new(0.0, GATE_TAU, r),
            })
            .collect::<Vec<_>>()
            .try_into()
            .ok()?;
        Some(Self {
            cmds,
            slots,
            own: [(0.0, 0.0); MAX_GUESTS],
            block_peak: [0.0; MAX_GUESTS],
            active_count: 0,
            monitor: None,
        })
    }

    /// Mindestens ein Gast aktiv.
    pub fn any_active(&self) -> bool {
        self.active_count > 0
    }

    /// Kanal, dessen Rückweg gerade im Abhörausgang liegt.
    pub fn monitor(&self) -> Option<usize> {
        self.monitor.filter(|&s| s < MAX_GUESTS)
    }

    fn drain(&mut self) {
        while let Ok(cmd) = self.cmds.pop() {
            match cmd {
                GuestCmd::Activate { slot, on } => {
                    if let Some(s) = self.slots.get_mut(slot) {
                        if on && !s.active {
                            // Nicht leeren: was das Netz seit dem Anlegen geschrieben hat, gilt.
                            s.gate.value = 0.0;
                            s.active = true;
                        } else if !on && s.active {
                            s.reader.flush();
                            s.active = false;
                            self.own[slot] = (0.0, 0.0);
                        }
                        s.stats
                            .active
                            .store(s.active, std::sync::atomic::Ordering::Relaxed);
                    }
                }
                GuestCmd::Gain { slot, gain } => {
                    if let Some(s) = self.slots.get_mut(slot) {
                        s.gain = gain.clamp(0.0, 4.0);
                        s.update_fader();
                    }
                }
                GuestCmd::Mute { slot, on } => {
                    if let Some(s) = self.slots.get_mut(slot) {
                        s.muted = on;
                        s.update_fader();
                    }
                }
                GuestCmd::Monitor(m) => self.monitor = m,
            }
        }
        self.active_count = self.slots.iter().filter(|s| s.active).count();
    }

    /// Am Blockanfang: Befehle, Puffer kürzen, Füllstand melden.
    pub fn begin_block(&mut self) {
        self.drain();
        for s in self.slots.iter_mut().filter(|s| s.active) {
            s.reader.begin_block();
        }
    }

    /// Ein Frame: Summe aller Gäste (nach Fader). Die Beiträge je Kanal merkt sich die Bank für
    /// [`Self::return_bus`].
    #[inline]
    pub fn frame(&mut self) -> (f32, f32) {
        let (mut sl, mut sr) = (0.0f32, 0.0f32);
        for (i, s) in self.slots.iter_mut().enumerate() {
            if !s.active {
                continue;
            }
            let (l, r) = s.reader.pop_frame();
            s.gate.target = if s.reader.is_prebuffering() { 0.0 } else { 1.0 };
            let g = s.fader.tick() * s.gate.tick();
            let (l, r) = (l * g, r * g);
            self.own[i] = (l, r);
            self.block_peak[i] = self.block_peak[i].max(l.abs()).max(r.abs());
            sl += l;
            sr += r;
        }
        (sl, sr)
    }

    /// Rückweg des Gastes `slot` im aktuellen Frame: `base` (Musik, Carts, Moderator) plus alle
    /// Gäste außer ihm. Die anderen werden einzeln addiert (keine Summe minus eigener Anteil),
    /// damit von der eigenen Stimme nichts übrig bleibt, auch nicht als Rundungsrest.
    #[inline]
    pub fn return_bus(&self, slot: usize, base: (f32, f32)) -> (f32, f32) {
        let (mut l, mut r) = base;
        for (j, s) in self.slots.iter().enumerate() {
            if j != slot && s.active {
                l += self.own[j].0;
                r += self.own[j].1;
            }
        }
        (l, r)
    }

    /// Am Blockende: Spitzen für die Anzeige.
    pub fn end_block(&mut self) {
        for (s, p) in self.slots.iter().zip(self.block_peak.iter_mut()) {
            if s.active {
                s.stats.store_peak(*p);
            }
            *p = 0.0;
        }
    }
}
