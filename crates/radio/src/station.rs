//! Der Controller in einem eigenen Thread: Die Sendung läuft weiter, auch wenn die
//! Oberfläche gerade nicht zeichnet (Fenster minimiert). Die Oberfläche schickt Aufträge
//! als Closures und liest einen Schnappschuss ([`View`]).

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use crate::audio::{Measured, measure_file};
use crate::control::Controller;
use crate::show::RadioShow;
use crate::{CARTS, mixer::NONE};

/// Takt des Control-Threads.
const TICK: Duration = Duration::from_millis(10);

type Job = Box<dyn FnOnce(&mut Controller) + Send>;

enum Msg {
    Job(Job),
    Measured(PathBuf, Result<Measured, String>),
    Quit,
}

/// Ein Titel in der Anzeige.
#[derive(Debug, Clone, PartialEq)]
pub struct ViewEntry {
    pub title: String,
    pub path: PathBuf,
    pub seconds: Option<f64>,
    pub lufs: Option<f64>,
    pub gain_db: f64,
    /// Cue-Punkte (Paket 15).
    pub cues: crate::cues::Cues,
}

/// Schnappschuss für die Oberfläche.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct View {
    pub playlist: Vec<ViewEntry>,
    pub current: Option<usize>,
    pub cue: usize,
    pub running: bool,
    pub remaining_title: f64,
    pub remaining_total: f64,
    pub crossfade_seconds: f64,
    pub duck_db: f64,
    /// Sendebus-Processing an.
    pub bus_processing: bool,
    /// Mikro-Kette an.
    pub mic_processing: bool,
    /// Ziel-Lautheit (LUFS).
    pub target_lufs: f64,
    /// Lautheitsregelung des Sendebusses (dB).
    pub bus_agc_db: f32,
    /// Größte Absenkung Kompressor + Limiter seit dem letzten Bild (dB).
    pub bus_reduction_db: f32,
    /// Kurzzeit-Lautheit am Sendeausgang (LUFS).
    pub bus_lufs: f32,
    /// Mikro-Gate offen.
    pub mic_gate_open: bool,
    pub carts: Vec<Option<String>>,
    pub carts_playing: u32,
    pub mic_open: bool,
    pub recording: Option<PathBuf>,
    pub messages: Vec<String>,
    /// Wird gerade gemessen.
    pub measuring: usize,
    /// Zähler für Änderungen an Playlist/Carts/Einstellungen (zum Speichern).
    pub revision: u64,
    /// Zuletzt geladener Titel und Dauer des Dekodierens in Millisekunden.
    pub last_load: Option<(PathBuf, u64)>,
    /// Sekunden bis zum Ramp-Punkt (Intro-Ende) des laufenden Titels.
    pub ramp_remaining: Option<f64>,
    /// Kopfhörer-Ausgang für das Vorhören eingehängt.
    pub pfl_available: bool,
    /// Vorhören läuft.
    pub pfl_playing: bool,
    /// Cue-Bearbeitungen lassen sich zurücknehmen / wiederholen.
    pub cues_undo: bool,
    pub cues_redo: bool,
}

/// Griff auf den laufenden Sende-Thread.
pub struct Station {
    tx: Sender<Msg>,
    view: Arc<Mutex<View>>,
    show: Arc<Mutex<RadioShow>>,
    thread: Option<JoinHandle<()>>,
    rate: u32,
    measuring: Arc<AtomicUsize>,
}

impl Station {
    /// Startet den Thread mit `controller`.
    pub fn spawn(controller: Controller) -> Result<Self, String> {
        let (tx, rx) = channel();
        let view = Arc::new(Mutex::new(View::default()));
        let show = Arc::new(Mutex::new(controller.show().clone()));
        let rate = controller.rate();
        let measuring = Arc::new(AtomicUsize::new(0));
        let (v, s, m) = (Arc::clone(&view), Arc::clone(&show), Arc::clone(&measuring));
        let thread = std::thread::Builder::new()
            .name("woa-radio-control".into())
            .spawn(move || run(controller, rx, v, s, m))
            .map_err(|e| e.to_string())?;
        Ok(Self {
            tx,
            view,
            show,
            thread: Some(thread),
            rate,
            measuring,
        })
    }

    /// Führt `f` im Control-Thread aus.
    pub fn with(&self, f: impl FnOnce(&mut Controller) + Send + 'static) {
        if self.tx.send(Msg::Job(Box::new(f))).is_err() {
            tracing::warn!("radio control thread gone");
        }
    }

    /// Fügt Dateien hinzu und misst sie im Hintergrund (Ergebnis wird gecacht).
    pub fn add_files(&self, paths: Vec<PathBuf>) {
        let todo = paths.clone();
        self.with(move |c| {
            c.add(&paths, None);
        });
        self.measure(todo);
    }

    /// Misst im Hintergrund alle Titel, die noch keine (gültige) Messung haben.
    pub fn measure_missing(&self, show: &RadioShow) {
        let todo: Vec<PathBuf> = show
            .playlist
            .iter()
            .filter(|e| !e.is_measured())
            .map(|e| e.path.clone())
            .collect();
        self.measure(todo);
    }

    fn measure(&self, paths: Vec<PathBuf>) {
        let rate = self.rate;
        for p in paths {
            let tx = self.tx.clone();
            self.measuring.fetch_add(1, Ordering::Relaxed);
            let spawned = std::thread::Builder::new()
                .name("woa-radio-measure".into())
                .spawn(move || {
                    let r = measure_file(&p, rate);
                    let _ = tx.send(Msg::Measured(p, r));
                });
            if let Err(e) = spawned {
                tracing::warn!(error = %e, "measure thread failed");
            }
        }
    }

    /// Letzter Schnappschuss.
    pub fn view(&self) -> View {
        self.view.lock().map(|v| v.clone()).unwrap_or_default()
    }

    /// Aktueller Inhalt für `radio.json`.
    pub fn show(&self) -> RadioShow {
        self.show.lock().map(|s| s.clone()).unwrap_or_default()
    }
}

impl Drop for Station {
    fn drop(&mut self) {
        let _ = self.tx.send(Msg::Quit);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn run(
    mut c: Controller,
    rx: Receiver<Msg>,
    view: Arc<Mutex<View>>,
    show_out: Arc<Mutex<RadioShow>>,
    measuring: Arc<AtomicUsize>,
) {
    let mut revision = 0u64;
    let mut last_show = c.show().clone();
    let mut messages: Vec<String> = Vec::new();
    loop {
        loop {
            match rx.try_recv() {
                Ok(Msg::Job(f)) => f(&mut c),
                Ok(Msg::Measured(p, r)) => {
                    measuring.fetch_sub(1, Ordering::Relaxed);
                    match r {
                        Ok(m) => c.set_measured(&p, m),
                        Err(e) => messages.push(e),
                    }
                }
                Ok(Msg::Quit) | Err(TryRecvError::Disconnected) => {
                    if c.is_recording() {
                        c.stop_recording();
                    }
                    return;
                }
                Err(TryRecvError::Empty) => break,
            }
        }
        c.pump();
        if let Some(m) = c.take_message() {
            messages.push(m);
        }
        if *c.show() != last_show {
            last_show = c.show().clone();
            revision += 1;
            if let Ok(mut s) = show_out.lock() {
                *s = last_show.clone();
            }
        }
        let st = c.status();
        let target = c.show().target_lufs;
        let v = View {
            playlist: c
                .show()
                .playlist
                .iter()
                .map(|e| ViewEntry {
                    title: e.title(),
                    path: e.path.clone(),
                    seconds: e.seconds,
                    lufs: e.lufs,
                    gain_db: e.gain_db(target),
                    cues: e.cues,
                })
                .collect(),
            current: c.current_index(),
            cue: c.cue_index(),
            running: c.is_running() && st.current.load(Ordering::Acquire) != NONE,
            remaining_title: c.remaining_title(),
            remaining_total: c.remaining_total(),
            crossfade_seconds: c.show().crossfade_seconds,
            duck_db: c.show().duck_db,
            bus_processing: c.show().bus_processing,
            mic_processing: c.show().mic_processing,
            target_lufs: c.show().target_lufs,
            bus_agc_db: st.bus_agc_db(),
            bus_reduction_db: st.take_bus_reduction(),
            bus_lufs: st.bus_lufs(),
            mic_gate_open: st.mic_gate_open.load(Ordering::Relaxed),
            carts: (0..CARTS)
                .map(|i| {
                    c.show()
                        .carts
                        .get(i)
                        .and_then(|x| x.as_ref().map(|e| e.label.clone()))
                })
                .collect(),
            carts_playing: st.carts.load(Ordering::Acquire),
            mic_open: st.mic_open.load(Ordering::Acquire),
            recording: c.recording_file().map(|p| p.to_path_buf()),
            messages: messages.clone(),
            measuring: measuring.load(Ordering::Relaxed),
            revision,
            last_load: c
                .last_load()
                .map(|(p, d)| (p.clone(), d.as_millis() as u64)),
            ramp_remaining: c.ramp_remaining(),
            pfl_available: c.has_pfl(),
            pfl_playing: c.pfl_playing(),
            cues_undo: c.can_undo_cues(),
            cues_redo: c.can_redo_cues(),
        };
        if let Ok(mut out) = view.lock() {
            *out = v;
        }
        if messages.len() > 5 {
            messages.drain(..messages.len() - 5);
        }
        std::thread::sleep(TICK);
    }
}
