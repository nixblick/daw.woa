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
    /// Voice-Track: Beginn der Stimme, ob der Eintrag auf „prüfen“ steht (Paket 30).
    pub voice: Option<VoiceBrief>,
}

/// Kurzform eines Voice-Tracks für die Anzeige.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VoiceBrief {
    pub params: crate::voicetrack::VoiceParams,
    pub check: bool,
}

/// Zustand eines Decks für die Anzeige.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DeckView {
    /// Titelname, `None` = leer.
    pub title: Option<String>,
    pub path: Option<PathBuf>,
    pub cues: crate::Cues,
    pub playing: bool,
    /// Sekunden bis zum Ende (Cue-Out).
    pub remaining: f64,
    /// Länge von Cue-In bis Cue-Out in Sekunden.
    pub length: f64,
    /// „Zurück zur Playlist“ an (gilt auch für ein leeres Deck).
    pub back_to_playlist: bool,
}

/// Schnappschuss für die Oberfläche.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct View {
    pub playlist: Vec<ViewEntry>,
    /// Namen aller Playlisten und Index der aktiven (crate::playlists).
    pub playlists: Vec<String>,
    pub active_playlist: usize,
    pub current: Option<usize>,
    pub cue: usize,
    pub running: bool,
    pub remaining_title: f64,
    /// Wiederholen: aus, Playlist, Titel.
    pub repeat: crate::control::Repeat,
    /// Zufällige Reihenfolge.
    pub shuffle: bool,
    /// Pausiert: (Restzeit, Länge) des angehaltenen Titels in Sekunden.
    pub paused: Option<(f64, f64)>,
    pub remaining_total: f64,
    pub crossfade_seconds: f64,
    /// Stopp mit Ausblenden an (#227) und seine Dauer (Sekunden).
    pub stop_fade: bool,
    pub stop_fade_seconds: f64,
    /// Auto-Weiter in der Sende-Playlist (#148).
    pub auto_next: bool,
    pub duck_db: f64,
    /// Carts senken die Musik ab (#149).
    pub cart_duck: bool,
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
    /// Taste je Cart (Name wie `egui::Key::name`), `None` = keine.
    pub cart_keys: Vec<Option<String>>,
    /// Datei je Cart.
    pub cart_paths: Vec<Option<std::path::PathBuf>>,
    /// Start und Ende je Cart.
    pub cart_cues: Vec<crate::Cues>,
    pub carts_playing: u32,
    /// Restzeit je Cart in Sekunden (0 = steht).
    pub cart_remaining: Vec<f64>,
    /// Die Decks A bis C.
    pub decks: Vec<DeckView>,
    /// Party mit zwei Decks (`None`: aus). Dann sind `current`, `running`, `paused` und
    /// `remaining_title` die der hörbaren Seite.
    pub party: Option<crate::control::PartyDuoView>,
    pub mic_open: bool,
    /// Telefon auf Sendung.
    pub phone_on: bool,
    /// Spitze des Telefon-Eingangs seit der letzten Ansicht (0..1).
    pub phone_peak: f32,
    /// Spitzen seit der letzten Ansicht (linear) für die LED-Meter: Playlist, Carts, Decks,
    /// Mikro nach ihrem Fader und der Sendemix (Master), #216.
    pub meters: [f32; 5],
    /// Gespielte Titel ausblenden (#215).
    pub hide_played: bool,
    /// Erster offener Titel; die davor sind gespielt.
    pub first_open: usize,
    pub recording: Option<PathBuf>,
    /// Mitschnitt läuft (auch bevor die erste Datei offen ist).
    pub recording_on: bool,
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
    /// Was gerade vorgehört wird (nur solange es läuft), für „Vorhören stoppen“ (#205).
    pub pfl_source: Option<crate::control::PflSource>,
    /// Cue-Bearbeitungen lassen sich zurücknehmen / wiederholen.
    pub cues_undo: bool,
    pub cues_redo: bool,
    /// Nächstes Zeit-Event: Name, Sekunden bis dahin, hart (Paket 29).
    pub next_event: Option<(String, f64, bool)>,
    /// Zuletzt gespieltes Zeit-Event und Abweichung in Sekunden (positiv: zu spät).
    pub last_event: Option<(String, f64)>,
    /// Verpasste Zeit-Events seit dem Start.
    pub events_missed: u32,
    /// Gast-Kanäle mit Pegel und Pufferzustand (Paket 37).
    pub guests: Vec<woa_guests::GuestView>,
    /// Einladungen und Warteraum der Browser-Gäste (Paket 38).
    pub web: woa_guests::WebView,
}

/// Griff auf den laufenden Sende-Thread.
pub struct Station {
    tx: Sender<Msg>,
    view: Arc<Mutex<View>>,
    show: Arc<Mutex<RadioShow>>,
    thread: Option<JoinHandle<()>>,
    rate: u32,
    measuring: Arc<AtomicUsize>,
    /// Warteschlange des einen Mess-Threads (siehe [`Self::measure`]).
    measure_tx: Mutex<Option<Sender<PathBuf>>>,
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
            measure_tx: Mutex::new(None),
        })
    }

    /// Führt `f` im Control-Thread aus.
    pub fn with(&self, f: impl FnOnce(&mut Controller) + Send + 'static) {
        if self.tx.send(Msg::Job(Box::new(f))).is_err() {
            tracing::warn!("radio control thread gone");
        }
    }

    /// Fügt Dateien hinzu und misst sie im Hintergrund (Ergebnis wird gecacht).
    #[tracing::instrument(level = "info", skip_all, name = "station_add_files")]
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

    /// Misst nacheinander in einem einzigen Hintergrund-Thread. Früher bekam jede Datei einen
    /// eigenen Thread; elf Titel auf einmal dekodierten dann elf Threads parallel und ließen
    /// auf einem schwachen Notebook der Oberfläche keine CPU mehr (Hänger beim Hinzufügen).
    fn measure(&self, paths: Vec<PathBuf>) {
        if paths.is_empty() {
            return;
        }
        let Ok(mut guard) = self.measure_tx.lock() else {
            return;
        };
        for p in paths {
            self.measuring.fetch_add(1, Ordering::Relaxed);
            let p = match guard.as_ref() {
                Some(q) => match q.send(p) {
                    Ok(()) => continue,
                    Err(e) => e.0,
                },
                None => p,
            };
            // Kein Mess-Thread (noch keiner oder beendet): einen starten.
            let (qtx, qrx) = channel::<PathBuf>();
            let _ = qtx.send(p);
            let (tx, rate) = (self.tx.clone(), self.rate);
            let spawned = std::thread::Builder::new()
                .name("woa-radio-measure".into())
                .spawn(move || {
                    for p in qrx {
                        let r = measure_file(&p, rate);
                        if tx.send(Msg::Measured(p, r)).is_err() {
                            return;
                        }
                    }
                });
            match spawned {
                Ok(_) => *guard = Some(qtx),
                Err(e) => {
                    self.measuring.fetch_sub(1, Ordering::Relaxed);
                    tracing::warn!(error = %e, "measure thread failed");
                }
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

/// Zustand von Deck `k` aus Sendung und Mixer-Status.
fn deck_view(c: &Controller, k: usize) -> DeckView {
    let st = c.status();
    let rate = f64::from(c.rate());
    let entry = c.show().decks.get(k).and_then(Option::as_ref);
    let state = st.deck_state[k].load(Ordering::Acquire);
    if entry.is_none() || state == crate::mixer::DECK_EMPTY {
        return DeckView {
            back_to_playlist: c.deck_return(k),
            ..DeckView::default()
        };
    }
    let (pos, end) = (
        st.deck_pos[k].load(Ordering::Acquire),
        st.deck_end[k].load(Ordering::Acquire),
    );
    let start = entry.map_or(0.0, |e| e.cues.start());
    DeckView {
        title: entry.map(|e| e.label.clone()),
        path: entry.map(|e| e.path.clone()),
        cues: entry.map(|e| e.cues).unwrap_or_default(),
        playing: state == crate::mixer::DECK_PLAYING,
        remaining: end.saturating_sub(pos) as f64 / rate,
        length: (end as f64 / rate - start).max(0.0),
        back_to_playlist: c.deck_return(k),
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
        let party = c.party_view();
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
                    voice: e.voice.as_ref().map(|v| VoiceBrief {
                        params: v.params,
                        check: v.check,
                    }),
                })
                .collect(),
            playlists: c.show().playlist_names(),
            active_playlist: c.show().active,
            current: if party.is_some() {
                party
                    .as_ref()
                    .filter(|p| p.sides[p.live].playing)
                    .and_then(|p| p.sides[p.live].index)
            } else {
                c.current_index()
            },
            cue: party
                .as_ref()
                .and_then(|p| p.sides[p.live].index)
                .unwrap_or_else(|| c.cue_index()),
            running: c.party_playing()
                || (c.is_running() && st.current.load(Ordering::Acquire) != NONE),
            remaining_title: match &party {
                Some(p) => (p.sides[p.live].end - p.sides[p.live].position).max(0.0),
                None => c.remaining_title(),
            },
            repeat: c.repeat(),
            shuffle: c.shuffle(),
            paused: match &party {
                Some(p) => {
                    let s = &p.sides[p.live];
                    (!c.party_playing() && s.index.is_some() && s.position > 0.0)
                        .then(|| ((s.end - s.position).max(0.0), s.end))
                }
                None => c.paused_times(),
            },
            remaining_total: c.remaining_total(),
            crossfade_seconds: c.show().crossfade_seconds,
            stop_fade: c.show().stop_fade,
            stop_fade_seconds: c.show().stop_fade_seconds,
            auto_next: c.show().auto_next,
            duck_db: c.show().duck_db,
            cart_duck: c.show().cart_duck,
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
            cart_keys: (0..CARTS).map(|i| c.show().cart_key(i)).collect(),
            cart_paths: (0..CARTS)
                .map(|i| {
                    c.show()
                        .carts
                        .get(i)
                        .and_then(Option::as_ref)
                        .map(|e| e.path.clone())
                })
                .collect(),
            cart_cues: (0..CARTS)
                .map(|i| {
                    c.show()
                        .carts
                        .get(i)
                        .and_then(Option::as_ref)
                        .map(|e| e.cues)
                        .unwrap_or_default()
                })
                .collect(),
            carts_playing: st.carts.load(Ordering::Acquire),
            cart_remaining: st
                .cart_rest
                .iter()
                .map(|r| r.load(Ordering::Acquire) as f64 / f64::from(c.rate()))
                .collect(),
            decks: (0..crate::DECKS).map(|k| deck_view(&c, k)).collect(),
            party,
            mic_open: st.mic_open.load(Ordering::Acquire),
            phone_on: st.phone_on.load(Ordering::Acquire),
            phone_peak: st.take_phone_peak(),
            meters: {
                let [m, c, d] = st.take_bus_peaks();
                [m, c, d, st.take_mic_peak(), st.take_peak()]
            },
            hide_played: c.show().hide_played,
            first_open: c.first_open_index(),
            recording: c.recording_file().map(|p| p.to_path_buf()),
            recording_on: c.is_recording(),
            messages: messages.clone(),
            measuring: measuring.load(Ordering::Relaxed),
            revision,
            last_load: c
                .last_load()
                .map(|(p, d)| (p.clone(), d.as_millis() as u64)),
            ramp_remaining: c.ramp_remaining(),
            pfl_available: c.has_pfl(),
            pfl_playing: c.pfl_playing(),
            pfl_source: c.pfl_current(),
            cues_undo: c.can_undo_cues(),
            cues_redo: c.can_redo_cues(),
            next_event: c.next_event(crate::control::wall_now()),
            last_event: c.last_event().cloned(),
            events_missed: c.events_missed(),
            guests: c.guests_view(),
            web: c.guests_web_view(),
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
