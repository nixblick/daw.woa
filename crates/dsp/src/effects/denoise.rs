//! Rauschunterdrückung mit RNNoise (`nnnoiseless`, BSD-3-Clause), nur bei 48 kHz.
//!
//! Das Netz arbeitet in Blöcken zu 480 Samples (10 ms). Die erste Berechnung auf einem Thread
//! legt FFT-Pläne an, also allokiert sie; das darf der Audio-Thread nicht. Darum rechnet im
//! Live-Betrieb ein eigener Arbeits-Thread (gestartet in `prepare`, Control-Thread); der
//! Audio-Thread tauscht Blöcke nur über lock-freie Warteschlangen (`rtrb`) aus. Beim Export
//! (`offline`) wird im Aufrufer-Thread gerechnet, ohne Arbeits-Thread, mit demselben Ergebnis.
//!
//! Die Verzögerung beträgt in beiden Betriebsarten genau [`LATENCY`] Samples (30 ms). Der
//! trockene Anteil wird um denselben Betrag verzögert, so dass `amount_pct` < 100 keine
//! Kammfilter-Färbung erzeugt. Kommt der Arbeits-Thread einmal nicht rechtzeitig, bleibt der
//! Block still und `underruns` zählt es.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::Duration;

use nnnoiseless::{DenoiseState, FRAME_SIZE};
use rtrb::{Consumer, Producer, RingBuffer};

/// Verzögerung in Samples: ein Block Sammeln, ein Block Rechenverzug, ein Block Verzug im Netz
/// selbst (RNNoise gibt ein Ergebnis um einen Block verspätet aus; gemessen, siehe Test).
pub const LATENCY: usize = 3 * FRAME_SIZE;

/// Einzige unterstützte Abtastrate.
pub const SAMPLE_RATE: f32 = 48_000.0;

const SCALE: f32 = 32768.0;

type Frame = [f32; FRAME_SIZE];

/// Rechner für einen Kanal.
struct Channel {
    state: Box<DenoiseState<'static>>,
}

impl Channel {
    fn new() -> Self {
        Self {
            state: DenoiseState::new(),
        }
    }

    fn run(&mut self, input: &Frame, output: &mut Frame) {
        let mut scaled = [0.0f32; FRAME_SIZE];
        for (d, s) in scaled.iter_mut().zip(input) {
            *d = *s * SCALE;
        }
        self.state.process_frame(output, &scaled);
        for o in output.iter_mut() {
            *o = (*o / SCALE).clamp(-1.0, 1.0);
        }
    }
}

enum Engine {
    /// Rechnen im Aufrufer-Thread (Export, Tests).
    Inline(Vec<Channel>),
    /// Rechnen im Arbeits-Thread.
    Worker {
        to_worker: Producer<f32>,
        from_worker: Consumer<f32>,
        stop: Arc<AtomicBool>,
    },
}

/// Rauschunterdrückung für einen oder zwei Kanäle.
pub struct Denoiser {
    channels: usize,
    offline: bool,
    engine: Option<Engine>,
    amount: f32,
    /// Sammelpuffer des laufenden Blocks, je Kanal.
    acc: [Frame; 2],
    /// Ausgabe des vorigen Blocks, je Kanal.
    out: [Frame; 2],
    /// Im Inline-Betrieb: Ergebnis des gerade berechneten Blocks, je Kanal.
    ready: [Frame; 2],
    pos: usize,
    /// Wartet der Arbeits-Thread noch auf ein Ergebnis?
    pending: bool,
    /// Trockener Anteil, um [`LATENCY`] verzögert (Ringpuffer je Kanal).
    dry: [Vec<f32>; 2],
    dry_pos: usize,
    underruns: Arc<AtomicU32>,
}

impl std::fmt::Debug for Denoiser {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Denoiser")
            .field("channels", &self.channels)
            .field("offline", &self.offline)
            .field("running", &self.engine.is_some())
            .finish()
    }
}

impl Clone for Denoiser {
    /// Ein Klon ist unvorbereitet (ohne Netz und Thread) und reicht das Signal durch, bis
    /// [`Denoiser::prepare`] lief.
    fn clone(&self) -> Self {
        Self::new(self.channels, self.offline, self.amount * 100.0)
    }
}

impl Drop for Denoiser {
    fn drop(&mut self) {
        if let Some(Engine::Worker { stop, .. }) = &self.engine {
            stop.store(true, Ordering::Release);
        }
    }
}

fn worker_loop(
    channels: usize,
    mut input: Consumer<f32>,
    mut output: Producer<f32>,
    stop: &AtomicBool,
) {
    let mut chans: Vec<Channel> = (0..channels).map(|_| Channel::new()).collect();
    let mut inp = [[0.0f32; FRAME_SIZE]; 2];
    let mut out = [[0.0f32; FRAME_SIZE]; 2];
    while !stop.load(Ordering::Acquire) {
        if input.slots() < FRAME_SIZE * channels {
            std::thread::sleep(Duration::from_millis(1));
            continue;
        }
        for frame in inp.iter_mut().take(channels) {
            for s in frame.iter_mut() {
                *s = input.pop().unwrap_or(0.0);
            }
        }
        for ((chan, i), o) in chans.iter_mut().zip(&inp).zip(&mut out) {
            chan.run(i, o);
        }
        if output.slots() < FRAME_SIZE * channels {
            continue; // Audio-Thread liest nicht mehr: Block verwerfen.
        }
        for frame in out.iter().take(channels) {
            for s in frame {
                if output.push(*s).is_err() {
                    break;
                }
            }
        }
    }
}

impl Denoiser {
    /// Unvorbereitet; `amount_pct` 0..100.
    pub fn new(channels: usize, offline: bool, amount_pct: f32) -> Self {
        Self {
            channels: channels.clamp(1, 2),
            offline,
            engine: None,
            amount: (amount_pct / 100.0).clamp(0.0, 1.0),
            acc: [[0.0; FRAME_SIZE]; 2],
            out: [[0.0; FRAME_SIZE]; 2],
            ready: [[0.0; FRAME_SIZE]; 2],
            pos: 0,
            pending: false,
            dry: [Vec::new(), Vec::new()],
            dry_pos: 0,
            underruns: Arc::new(AtomicU32::new(0)),
        }
    }

    /// Legt Netz, Puffer und gegebenenfalls den Arbeits-Thread an (Control-Thread). Bei einer
    /// anderen Rate als 48 kHz bleibt der Effekt ein Durchgang.
    pub fn prepare(&mut self, rate: f32) {
        if let Some(Engine::Worker { stop, .. }) = &self.engine {
            stop.store(true, Ordering::Release);
        }
        self.engine = None;
        self.pos = 0;
        self.pending = false;
        self.acc = [[0.0; FRAME_SIZE]; 2];
        self.out = [[0.0; FRAME_SIZE]; 2];
        self.ready = [[0.0; FRAME_SIZE]; 2];
        self.dry_pos = 0;
        self.dry = [vec![0.0; LATENCY], vec![0.0; LATENCY]];
        if (rate - SAMPLE_RATE).abs() > 1.0 {
            return;
        }
        if self.offline {
            self.engine = Some(Engine::Inline(
                (0..self.channels).map(|_| Channel::new()).collect(),
            ));
            return;
        }
        let n = FRAME_SIZE * self.channels * 4;
        let (to_worker, input) = RingBuffer::<f32>::new(n);
        let (output, from_worker) = RingBuffer::<f32>::new(n);
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let channels = self.channels;
        let spawned = std::thread::Builder::new()
            .name("woa-denoise".into())
            .spawn(move || worker_loop(channels, input, output, &flag));
        match spawned {
            Ok(_) => {
                self.engine = Some(Engine::Worker {
                    to_worker,
                    from_worker,
                    stop,
                });
            }
            Err(e) => eprintln!("woa: noise suppression thread could not start: {e}"),
        }
    }

    /// Stärke 0..100 %.
    pub fn set_amount(&mut self, amount_pct: f32) {
        self.amount = (amount_pct / 100.0).clamp(0.0, 1.0);
    }

    /// Wie oft ein Ergebnis zu spät kam (Block blieb still).
    pub fn underruns(&self) -> u32 {
        self.underruns.load(Ordering::Relaxed)
    }

    /// Ob die Rauschunterdrückung rechnet (Rate 48 kHz und vorbereitet).
    pub fn is_active(&self) -> bool {
        self.engine.is_some()
    }

    fn finish_block(&mut self) {
        let ch = self.channels;
        let Some(engine) = &mut self.engine else {
            return;
        };
        match engine {
            Engine::Inline(chans) => {
                self.out = self.ready;
                let jobs = chans.iter_mut().zip(&self.acc).zip(&mut self.ready);
                for ((chan, input), output) in jobs.take(ch) {
                    chan.run(input, output);
                }
            }
            Engine::Worker {
                to_worker,
                from_worker,
                ..
            } => {
                if self.pending {
                    if from_worker.slots() >= FRAME_SIZE * ch {
                        for frame in self.out.iter_mut().take(ch) {
                            for s in frame.iter_mut() {
                                *s = from_worker.pop().unwrap_or(0.0);
                            }
                        }
                    } else {
                        self.out = [[0.0; FRAME_SIZE]; 2];
                        self.underruns.fetch_add(1, Ordering::Relaxed);
                    }
                }
                if to_worker.slots() >= FRAME_SIZE * ch {
                    for frame in self.acc.iter().take(ch) {
                        for s in frame {
                            let _ = to_worker.push(*s);
                        }
                    }
                    self.pending = true;
                } else {
                    self.pending = false;
                }
            }
        }
    }

    /// Ein Sample-Paar; bei `stereo = false` zählt nur `l`. Ohne Netz: Durchgang.
    #[inline]
    pub fn step(&mut self, l: f32, r: f32, stereo: bool) -> (f32, f32) {
        if self.engine.is_none() {
            return (l, r);
        }
        let wanted: usize = if stereo { 2 } else { 1 };
        let ch = wanted.min(self.channels);
        let input = [l, r];
        let mut wet = [l, r];
        let mut dry = [l, r];
        for c in 0..ch {
            self.acc[c][self.pos] = input[c];
            wet[c] = self.out[c][self.pos];
            let slot = &mut self.dry[c][self.dry_pos];
            dry[c] = *slot;
            *slot = input[c];
        }
        self.pos += 1;
        self.dry_pos += 1;
        if self.dry_pos == LATENCY {
            self.dry_pos = 0;
        }
        if self.pos == FRAME_SIZE {
            self.pos = 0;
            self.finish_block();
        }
        let a = self.amount;
        (
            dry[0] + (wet[0] - dry[0]) * a,
            if stereo {
                dry[1] + (wet[1] - dry[1]) * a
            } else {
                r
            },
        )
    }
}
