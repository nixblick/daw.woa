//! Ein CLAP-Plugin laden und im Audio-Graphen spielen (Paket 31).
//!
//! Thread-Regeln von CLAP: Anlegen, Regler lesen, Zustand speichern und laden, Abbauen geschieht
//! auf dem Control-Thread ([`ClapInstance`], nicht `Send`). Gerechnet wird im Audio-Thread über
//! einen [`ClapProcessor`] (`Send`), der sich mit der Instanz die Rechenstufe teilt. Ein
//! Umschalter (`busy`) sorgt dafür, dass nie beide gleichzeitig darauf zugreifen; im Audio-Thread
//! gibt es keinen Mutex und kein Warten: Ist die Stufe besetzt oder abgebaut, schreibt der Aufruf
//! Stille.
//!
//! Geprüfte Quellen für den Umgang mit `clack-host` 0.2: README und Tests der Crate
//! (MIT/Apache-2.0). Es ist kein Code daraus übernommen.

use std::cell::{Cell, RefCell, UnsafeCell};
use std::ffi::CString;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use clack_extensions::audio_ports::{AudioPortInfoBuffer, PluginAudioPorts};
use clack_extensions::gui::{
    GuiApiType, GuiConfiguration, GuiSize, HostGui, HostGuiImpl, PluginGui,
};
use clack_extensions::params::{
    HostParams, HostParamsImplMainThread, HostParamsImplShared, ParamClearFlags, ParamInfoBuffer,
    ParamInfoFlags, ParamRescanFlags, PluginParams,
};
#[cfg(unix)]
use clack_extensions::posix_fd::{FdFlags, HostPosixFd, HostPosixFdImpl, PluginPosixFd};
use clack_extensions::state::PluginState;
use clack_extensions::timer::{HostTimer, HostTimerImpl, PluginTimer, TimerId};
use clack_host::events::Match;
use clack_host::events::event_types::{MidiEvent, NoteOffEvent, NoteOnEvent, ParamValueEvent};
use clack_host::events::spaces::CoreEventSpace;
use clack_host::extensions::prelude::HostError as HostError2;
use clack_host::prelude::*;
use woa_dsp::instrument::{NoteEvent, NoteEventKind};
use woa_dsp::plugin::StereoProcessor;
use woa_model::plugin::MAX_PLUGIN_STATE_BYTES;

use crate::feedback::{ChangeKind, Feedback, ParamChange, ParamEdit};
use crate::scan::Catalog;

/// Längster Block, den das Plugin auf einmal bekommt; längere Blöcke werden zerlegt.
pub const MAX_FRAMES: u32 = 4096;
/// Höchste Zahl Regler, die wir uns von einem Plugin merken.
const MAX_PARAMS: u32 = 4096;
/// Platz für Events je Block (vorab angelegt); darüber hinaus fallen Events weg.
const EVENT_CAP: usize = 2048;
/// Platz für noch nicht abgeschickte Reglerwerte.
const PENDING_CAP: usize = 512;
/// Platz in der Queue der Reglermeldungen vom Audio-Thread zum Control-Thread. Ist sie voll,
/// fallen Meldungen weg (der Audio-Thread wartet nie).
const FEEDBACK_CAP: usize = 4096;
/// Platz in der Queue der Reglerwerte vom Control-Thread zum Plugin (Rücksetzen nach Rückgängig).
const SET_CAP: usize = 512;

/// Fehler beim Laden oder Benutzen eines Plugins.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostError {
    /// Die Kennung steht nicht in der Plugin-Liste dieses Rechners.
    Unknown(String),
    /// Das Plugin ließ sich nicht anlegen oder aktivieren.
    Load(String),
    /// Das Plugin hat zu viele Audio-Kanäle (mehr als 2) am ersten Anschluss.
    Channels(u32),
    /// Zustand speichern oder laden schlug fehl.
    State(String),
    /// Die Oberfläche des Plugins ließ sich nicht öffnen.
    Gui(String),
}

impl std::fmt::Display for HostError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unknown(id) => write!(f, "Plugin {id} ist auf diesem Rechner nicht installiert"),
            Self::Load(e) => write!(f, "Plugin ließ sich nicht laden: {e}"),
            Self::Channels(n) => write!(f, "Plugin mit {n} Kanälen wird nicht unterstützt"),
            Self::State(e) => write!(f, "Plugin-Zustand: {e}"),
            Self::Gui(e) => write!(f, "Plugin-Fenster: {e}"),
        }
    }
}

impl std::error::Error for HostError {}

/// Beschreibung eines Reglers des Plugins.
#[derive(Debug, Clone, PartialEq)]
pub struct ParamDesc {
    /// CLAP-Kennung (stabil), die das Projekt speichert.
    pub id: u32,
    /// Name zur Anzeige.
    pub name: String,
    /// Gruppe (`Oszillator/Welle 1`), kann leer sein.
    pub module: String,
    /// Kleinster Wert.
    pub min: f64,
    /// Größter Wert.
    pub max: f64,
    /// Grundwert.
    pub default: f64,
    /// Nur ganze Stufen.
    pub stepped: bool,
    /// Der Nutzer darf ihn nicht ändern.
    pub read_only: bool,
    /// Das Plugin will ihn nicht in Listen sehen.
    pub hidden: bool,
}

/// Zustand, den Plugin-Rückrufe aus beliebigen Threads setzen und der Control-Thread abholt.
#[derive(Default)]
struct Flags {
    /// Das Plugin will `on_main_thread` gerufen haben.
    callback: AtomicBool,
    /// Das Plugin meldet das Fenster als geschlossen.
    closed: AtomicBool,
    /// … und hat es dabei selbst zerstört (dann muss der Host `destroy` rufen).
    destroyed: AtomicBool,
    /// Das Plugin will, dass alle Reglerwerte neu gelesen werden (Preset).
    rescan: AtomicBool,
}

struct HostShared {
    flags: Arc<Flags>,
}

impl SharedHandler<'_> for HostShared {
    fn request_restart(&self) {
        tracing::debug!("plugin asked for a restart (ignored)");
    }
    fn request_process(&self) {}
    fn request_callback(&self) {
        self.flags.callback.store(true, Ordering::Relaxed);
    }
}

impl HostGuiImpl for HostShared {
    fn resize_hints_changed(&self) {}

    fn request_resize(&self, _new_size: GuiSize) -> Result<(), HostError2> {
        // Das schwebende Fenster gehört dem Plugin; es passt seine Größe selbst an.
        Ok(())
    }

    fn request_show(&self) -> Result<(), HostError2> {
        Ok(())
    }

    fn request_hide(&self) -> Result<(), HostError2> {
        Ok(())
    }

    fn closed(&self, was_destroyed: bool) {
        self.flags.destroyed.store(was_destroyed, Ordering::Relaxed);
        self.flags.closed.store(true, Ordering::Relaxed);
    }
}

impl HostParamsImplShared for HostShared {
    fn request_flush(&self) {
        // Das Plugin rechnet ohnehin jeden Block; einen eigenen Flush braucht es nicht.
    }
}

struct TimerReg {
    id: u32,
    period: Duration,
    next: Instant,
}

/// Control-Thread-Teil des Hosts: Zeitgeber und Dateikennungen, die das Plugin für seine
/// Oberfläche anmeldet (CLAP `timer-support`, `posix-fd-support`). Gerufen wird aus
/// [`ClapInstance::pump`] in jedem Bild der Oberfläche.
struct HostMain {
    flags: Arc<Flags>,
    timers: RefCell<Vec<TimerReg>>,
    next_timer: Cell<u32>,
    #[cfg(unix)]
    fds: RefCell<Vec<(std::os::unix::io::RawFd, FdFlags)>>,
}

impl MainThreadHandler<'_> for HostMain {}

impl HostTimerImpl for HostMain {
    fn register_timer(&self, period_ms: u32) -> Result<TimerId, HostError2> {
        let id = self.next_timer.get();
        self.next_timer.set(id.wrapping_add(1));
        // CLAP: mindestens 10 ms.
        let period = Duration::from_millis(u64::from(period_ms.max(10)));
        self.timers.borrow_mut().push(TimerReg {
            id,
            period,
            next: Instant::now() + period,
        });
        Ok(TimerId(id))
    }

    fn unregister_timer(&self, timer_id: TimerId) -> Result<(), HostError2> {
        self.timers.borrow_mut().retain(|t| t.id != timer_id.0);
        Ok(())
    }
}

#[cfg(unix)]
impl HostPosixFdImpl for HostMain {
    fn register_fd(&self, fd: std::os::unix::io::RawFd, flags: FdFlags) -> Result<(), HostError2> {
        let mut fds = self.fds.borrow_mut();
        fds.retain(|f| f.0 != fd);
        fds.push((fd, flags));
        Ok(())
    }

    fn modify_fd(&self, fd: std::os::unix::io::RawFd, flags: FdFlags) -> Result<(), HostError2> {
        self.register_fd(fd, flags)
    }

    fn unregister_fd(&self, fd: std::os::unix::io::RawFd) -> Result<(), HostError2> {
        self.fds.borrow_mut().retain(|f| f.0 != fd);
        Ok(())
    }
}

impl HostParamsImplMainThread for HostMain {
    fn rescan(&self, flags: ParamRescanFlags) {
        if flags.contains(ParamRescanFlags::VALUES) || flags.contains(ParamRescanFlags::ALL) {
            self.flags.rescan.store(true, Ordering::Relaxed);
        }
    }

    fn clear(&self, _param_id: ClapId, _flags: ParamClearFlags) {}
}

struct Host;

impl HostHandlers for Host {
    type Shared<'a> = HostShared;
    type MainThread<'a> = HostMain;
    type AudioProcessor<'a> = ();

    fn declare_extensions(builder: &mut HostExtensions<Self>, _shared: &HostShared) {
        builder
            .register::<HostGui>()
            .register::<HostParams>()
            .register::<HostTimer>();
        #[cfg(unix)]
        builder.register::<HostPosixFd>();
    }
}

enum Proc {
    Stopped(StoppedPluginAudioProcessor<Host>),
    Started(StartedPluginAudioProcessor<Host>),
    Gone,
}

/// Alles, was der Audio-Thread zum Rechnen braucht, vorab angelegt.
struct Inner {
    proc: Proc,
    in_ports: AudioPorts,
    out_ports: AudioPorts,
    ev_in: EventBuffer,
    ev_out: EventBuffer,
    in_l: Vec<f32>,
    in_r: Vec<f32>,
    out_l: Vec<f32>,
    out_r: Vec<f32>,
    has_input: bool,
    in_ch: usize,
    out_ch: usize,
    steady: u64,
    /// Reglermeldungen des Plugins (aus seiner Oberfläche) zum Control-Thread.
    feedback: rtrb::Producer<ParamChange>,
    /// Reglerwerte vom Control-Thread ans Plugin (Rücksetzen nach Rückgängig).
    sets: rtrb::Consumer<(u32, f64)>,
}

// SAFETY: `AudioPorts` hält rohe Zeiger, die bei jedem Aufruf neu gesetzt werden und nie über
// einen Aufruf hinaus gelten. Der Rest ist `Send`. Der Zugriff ist durch `busy` ausgeschlossen.
unsafe impl Send for Inner {}

struct ProcCell {
    busy: AtomicBool,
    failed: AtomicBool,
    errors: AtomicU64,
    inner: UnsafeCell<Option<Inner>>,
}

// SAFETY: `inner` wird nur mit gesetztem `busy` angefasst (siehe `lock`), also von höchstens
// einem Thread zugleich.
unsafe impl Sync for ProcCell {}
// SAFETY: siehe `Inner`; die Zelle wandert nur als `Arc` zwischen Threads.
unsafe impl Send for ProcCell {}

struct Guard<'a>(&'a ProcCell);

impl ProcCell {
    fn lock(&self) -> Option<Guard<'_>> {
        self.busy
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .ok()
            .map(|_| Guard(self))
    }
}

impl Guard<'_> {
    fn inner(&mut self) -> &mut Option<Inner> {
        // SAFETY: Wer einen `Guard` hat, hat `busy` gesetzt und ist der einzige Zugreifer.
        unsafe { &mut *self.0.inner.get() }
    }
}

impl Drop for Guard<'_> {
    fn drop(&mut self) {
        self.0.busy.store(false, Ordering::Release);
    }
}

/// Ein geladenes Plugin auf dem Control-Thread.
pub struct ClapInstance {
    instance: PluginInstance<Host>,
    cell: Arc<ProcCell>,
    clap_id: String,
    params: Vec<ParamDesc>,
    rate: f32,
    flags: Arc<Flags>,
    /// Reglermeldungen aus dem Audio-Thread.
    feedback_rx: rtrb::Consumer<ParamChange>,
    /// Werte ans Plugin.
    sets_tx: rtrb::Producer<(u32, f64)>,
    feedback: Feedback,
    gui_created: bool,
    gui_shown: bool,
}

fn lossy(b: &[u8]) -> String {
    String::from_utf8_lossy(b)
        .trim_end_matches('\0')
        .to_string()
}

impl ClapInstance {
    /// Legt das Plugin mit dieser Kennung an und macht es rechenbereit.
    pub fn new(cat: &Catalog, clap_id: &str, rate: f32) -> Result<Self, HostError> {
        let info = cat
            .find(clap_id)
            .ok_or_else(|| HostError::Unknown(clap_id.to_string()))?;
        let entry = cat
            .entry(&info.path)
            .ok_or_else(|| HostError::Unknown(clap_id.to_string()))?;
        let id = CString::new(clap_id).map_err(|e| HostError::Load(e.to_string()))?;
        let host = HostInfo::new(
            "WOA",
            "creaVisions",
            "https://github.com/nixblick/daw.woa",
            env!("CARGO_PKG_VERSION"),
        )
        .map_err(|e| HostError::Load(e.to_string()))?;
        let flags = Arc::new(Flags::default());
        let (f1, f2) = (Arc::clone(&flags), Arc::clone(&flags));
        let mut instance = PluginInstance::<Host>::new(
            move |_| HostShared { flags: f1 },
            move |_| HostMain {
                flags: f2,
                timers: RefCell::new(Vec::new()),
                next_timer: Cell::new(1),
                #[cfg(unix)]
                fds: RefCell::new(Vec::new()),
            },
            entry,
            &id,
            &host,
        )
        .map_err(|e| HostError::Load(e.to_string()))?;

        // Anschlüsse: nur der erste Audio-Ein- und -Ausgang wird benutzt.
        let (mut in_ch, mut out_ch) = (0u32, 0u32);
        let mut has_input = false;
        let ports = instance.plugin_handle().get_extension::<PluginAudioPorts>();
        if let Some(ports) = ports {
            let h = instance.plugin_handle();
            let mut buf = AudioPortInfoBuffer::new();
            if ports.count(&h, true) > 0
                && let Some(p) = ports.get(&h, 0, true, &mut buf)
            {
                in_ch = p.channel_count;
                has_input = true;
            }
            let mut buf = AudioPortInfoBuffer::new();
            if ports.count(&h, false) > 0
                && let Some(p) = ports.get(&h, 0, false, &mut buf)
            {
                out_ch = p.channel_count;
            }
        } else {
            // Ohne Anschlussbeschreibung gilt Stereo aus, kein Eingang.
            out_ch = 2;
        }
        if out_ch == 0 {
            return Err(HostError::Load("kein Audio-Ausgang".into()));
        }
        for n in [in_ch, out_ch] {
            if n > 2 {
                return Err(HostError::Channels(n));
            }
        }

        let params = read_params(&mut instance);

        let config = PluginAudioConfiguration {
            sample_rate: f64::from(rate),
            min_frames_count: 1,
            max_frames_count: MAX_FRAMES,
        };
        let stopped = instance
            .activate(|_, _| (), config)
            .map_err(|e| HostError::Load(e.to_string()))?;
        let n = MAX_FRAMES as usize;
        let (fb_tx, fb_rx) = rtrb::RingBuffer::new(FEEDBACK_CAP);
        let (set_tx, set_rx) = rtrb::RingBuffer::new(SET_CAP);
        let inner = Inner {
            proc: Proc::Stopped(stopped),
            in_ports: AudioPorts::with_capacity(2, 1),
            out_ports: AudioPorts::with_capacity(2, 1),
            ev_in: EventBuffer::with_capacity(EVENT_CAP),
            ev_out: EventBuffer::with_capacity(EVENT_CAP),
            in_l: vec![0.0; n],
            in_r: vec![0.0; n],
            out_l: vec![0.0; n],
            out_r: vec![0.0; n],
            has_input,
            in_ch: in_ch as usize,
            out_ch: out_ch as usize,
            steady: 0,
            feedback: fb_tx,
            sets: set_rx,
        };
        let mut this = Self {
            instance,
            cell: Arc::new(ProcCell {
                busy: AtomicBool::new(false),
                failed: AtomicBool::new(false),
                errors: AtomicU64::new(0),
                inner: UnsafeCell::new(Some(inner)),
            }),
            clap_id: clap_id.to_string(),
            params,
            rate,
            flags,
            feedback_rx: fb_rx,
            sets_tx: set_tx,
            feedback: Feedback::new(),
            gui_created: false,
            gui_shown: false,
        };
        this.refresh_values();
        Ok(this)
    }

    /// Alle Reglerwerte laut Plugin (Kennung, Wert).
    pub fn all_values(&mut self) -> Vec<(u32, f64)> {
        let ids: Vec<u32> = self.params.iter().map(|p| p.id).collect();
        ids.into_iter()
            .filter_map(|id| self.param_value(id).map(|v| (id, v)))
            .collect()
    }

    /// Liest alle Reglerwerte neu in den Merkspeicher der Rückmeldung.
    fn refresh_values(&mut self) {
        let values = self.all_values();
        self.feedback.clear();
        for (id, v) in values {
            self.feedback.set_known(id, v);
        }
    }

    /// Setzt einen Reglerwert im laufenden Plugin (über die Queue zum Audio-Thread; ein Wert,
    /// den das Plugin dann zurückmeldet, zählt nicht als Änderung des Nutzers). `false`, wenn die
    /// Queue voll ist oder der Wert ungültig.
    pub fn push_param(&mut self, id: u32, value: f64) -> bool {
        if !value.is_finite() {
            return false;
        }
        self.feedback.set_known(id, value);
        self.sets_tx.push((id, value)).is_ok()
    }

    /// Ob das Plugin ein eigenes (schwebendes) Fenster anbietet.
    pub fn has_gui(&mut self) -> bool {
        let h = self.instance.plugin_handle();
        h.get_extension::<PluginGui>()
            .is_some_and(|g| g.is_api_supported(&h, gui_config()))
    }

    /// Ob das Fenster des Plugins gerade offen ist.
    pub fn gui_open(&self) -> bool {
        self.gui_shown
    }

    /// Öffnet das Fenster des Plugins (schwebend; legt es beim ersten Mal an). Nur auf dem
    /// Control-Thread.
    pub fn open_gui(&mut self, title: &str) -> Result<(), HostError> {
        if self.gui_shown {
            return Ok(());
        }
        let h = self.instance.plugin_handle();
        let gui = h
            .get_extension::<PluginGui>()
            .ok_or_else(|| HostError::Gui("Das Plugin hat keine Oberfläche".into()))?;
        if !gui.is_api_supported(&h, gui_config()) {
            return Err(HostError::Gui(
                "Das Plugin bietet kein eigenes Fenster an (nur eingebettet)".into(),
            ));
        }
        if !self.gui_created {
            gui.create(&h, gui_config())
                .map_err(|e| HostError::Gui(e.to_string()))?;
            self.gui_created = true;
        }
        if let Ok(t) = CString::new(title) {
            gui.suggest_title(&h, &t);
        }
        gui.show(&h).map_err(|e| HostError::Gui(e.to_string()))?;
        self.gui_shown = true;
        Ok(())
    }

    /// Schließt das Fenster (das Plugin behält seine Oberfläche für das nächste Öffnen).
    pub fn close_gui(&mut self) {
        if !self.gui_shown {
            return;
        }
        self.gui_shown = false;
        let h = self.instance.plugin_handle();
        if let Some(gui) = h.get_extension::<PluginGui>()
            && let Err(e) = gui.hide(&h)
        {
            tracing::debug!(error = %e, "plugin window did not hide");
        }
    }

    /// Baut die Oberfläche ganz ab (vor dem Entladen des Plugins).
    fn destroy_gui(&mut self) {
        if !self.gui_created {
            return;
        }
        self.close_gui();
        let h = self.instance.plugin_handle();
        if let Some(gui) = h.get_extension::<PluginGui>() {
            gui.destroy(&h);
        }
        self.gui_created = false;
    }

    /// Bedient das Plugin auf dem Control-Thread: `on_main_thread`-Wünsche, Zeitgeber, Dateikennungen
    /// der Oberfläche, geschlossenes Fenster. In jedem Bild der Oberfläche rufen.
    pub fn pump(&mut self) {
        if self.flags.callback.swap(false, Ordering::Relaxed) {
            self.instance.call_on_main_thread_callback();
        }
        let now = Instant::now();
        let due: Vec<u32> = self.instance.access_handler(|m| {
            let mut due = Vec::new();
            for t in m.timers.borrow_mut().iter_mut() {
                if t.next <= now {
                    due.push(t.id);
                    t.next = now + t.period;
                }
            }
            due
        });
        if !due.is_empty() {
            let h = self.instance.plugin_handle();
            if let Some(t) = h.get_extension::<PluginTimer>() {
                for id in due {
                    t.on_timer(&h, TimerId(id));
                }
            }
        }
        #[cfg(unix)]
        self.pump_fds();
        if self.flags.closed.swap(false, Ordering::Relaxed) {
            self.gui_shown = false;
            if self.flags.destroyed.swap(false, Ordering::Relaxed) {
                // Das Plugin hat das Fenster zerstört; der Host bestätigt mit `destroy`.
                self.destroy_gui();
            }
        }
    }

    #[cfg(unix)]
    fn pump_fds(&mut self) {
        let regs: Vec<(std::os::unix::io::RawFd, FdFlags)> =
            self.instance.access_handler(|m| m.fds.borrow().clone());
        if regs.is_empty() {
            return;
        }
        let mut polls: Vec<libc::pollfd> = regs
            .iter()
            .map(|(fd, f)| {
                let mut events = 0;
                if f.contains(FdFlags::READ) {
                    events |= libc::POLLIN;
                }
                if f.contains(FdFlags::WRITE) {
                    events |= libc::POLLOUT;
                }
                libc::pollfd {
                    fd: *fd,
                    events,
                    revents: 0,
                }
            })
            .collect();
        // SAFETY: `polls` ist eine gültige Liste von `pollfd` in der angegebenen Länge; die
        // Wartezeit 0 blockiert nie.
        let n = unsafe { libc::poll(polls.as_mut_ptr(), polls.len() as libc::nfds_t, 0) };
        if n <= 0 {
            return;
        }
        let h = self.instance.plugin_handle();
        let Some(ext) = h.get_extension::<PluginPosixFd>() else {
            return;
        };
        for p in &polls {
            if p.revents == 0 {
                continue;
            }
            let mut fl = FdFlags::empty();
            if p.revents & (libc::POLLIN | libc::POLLHUP) != 0 {
                fl |= FdFlags::READ;
            }
            if p.revents & libc::POLLOUT != 0 {
                fl |= FdFlags::WRITE;
            }
            if p.revents & (libc::POLLERR | libc::POLLNVAL) != 0 {
                fl |= FdFlags::ERROR;
            }
            ext.on_fd(&h, p.fd, fl);
        }
    }

    /// Holt die Reglermeldungen aus dem Audio-Thread ab und macht Bearbeitungen für das Projekt
    /// daraus (siehe [`crate::feedback`]). Solange das Fenster zu ist, führt es nur die Merkwerte nach.
    pub fn poll_feedback(&mut self, out: &mut Vec<ParamEdit>) {
        let mut batch: Vec<ParamChange> = Vec::new();
        while let Ok(c) = self.feedback_rx.pop() {
            batch.push(c);
        }
        let rescan = self.flags.rescan.swap(false, Ordering::Relaxed);
        let emit = self.gui_shown || self.feedback.gesture_open();
        self.feedback.feed(&batch, rescan, emit, out);
        if rescan {
            self.refresh_values();
        }
    }

    /// CLAP-Kennung.
    pub fn clap_id(&self) -> &str {
        &self.clap_id
    }

    /// Abtastrate, mit der das Plugin aktiviert wurde.
    pub fn rate(&self) -> f32 {
        self.rate
    }

    /// Die Regler des Plugins.
    pub fn params(&self) -> &[ParamDesc] {
        &self.params
    }

    /// Ob das Plugin ausgefallen ist (Fehler beim Rechnen); es liefert dann nur noch Stille.
    pub fn failed(&self) -> bool {
        self.cell.failed.load(Ordering::Relaxed)
    }

    /// Wie oft das Plugin einen Rechenfehler gemeldet hat.
    pub fn error_count(&self) -> u64 {
        self.cell.errors.load(Ordering::Relaxed)
    }

    /// Ein neuer Rechner für den Graphen. Mehrere dürfen nebeneinander existieren (alter und
    /// neuer Graph beim Tausch); sie teilen sich die Stufe und schließen einander aus.
    pub fn processor(&self) -> Box<dyn StereoProcessor> {
        Box::new(ClapProcessor {
            cell: Arc::clone(&self.cell),
            pending: Vec::with_capacity(PENDING_CAP),
            flush: false,
        })
    }

    /// Aktueller Wert eines Reglers laut Plugin.
    pub fn param_value(&mut self, id: u32) -> Option<f64> {
        let h = self.instance.plugin_handle();
        let ext = h.get_extension::<PluginParams>()?;
        ext.get_value(&h, ClapId::from_raw(id)?)
    }

    /// Der Zustand des Plugins als Bytes (CLAP-State-Erweiterung).
    pub fn save_state(&mut self) -> Result<Vec<u8>, HostError> {
        let h = self.instance.plugin_handle();
        let ext = h.get_extension::<PluginState>().ok_or_else(|| {
            HostError::State("das Plugin kann seinen Zustand nicht speichern".into())
        })?;
        let mut sink = Capped(Vec::new());
        ext.save(&h, &mut sink)
            .map_err(|e| HostError::State(e.to_string()))?;
        Ok(sink.0)
    }

    /// Setzt den Zustand des Plugins aus Bytes.
    pub fn load_state(&mut self, bytes: &[u8]) -> Result<(), HostError> {
        let h = self.instance.plugin_handle();
        let ext = h
            .get_extension::<PluginState>()
            .ok_or_else(|| HostError::State("das Plugin kann keinen Zustand laden".into()))?;
        let mut r = bytes;
        let res = ext
            .load(&h, &mut r)
            .map_err(|e| HostError::State(e.to_string()));
        self.refresh_values();
        res
    }
}

/// Schreibziel mit Obergrenze, damit ein Plugin nicht beliebig viel in ein Projekt kippt.
struct Capped(Vec<u8>);

impl std::io::Write for Capped {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if self.0.len() + buf.len() > MAX_PLUGIN_STATE_BYTES {
            return Err(std::io::Error::other("Plugin-Zustand ist zu groß"));
        }
        self.0.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn gui_config() -> GuiConfiguration<'static> {
    GuiConfiguration {
        api_type: if cfg!(windows) {
            GuiApiType::WIN32
        } else {
            GuiApiType::X11
        },
        is_floating: true,
    }
}

fn read_params(instance: &mut PluginInstance<Host>) -> Vec<ParamDesc> {
    let Some(ext) = instance.plugin_handle().get_extension::<PluginParams>() else {
        return Vec::new();
    };
    let h = instance.plugin_handle();
    let count = ext.count(&h).min(MAX_PARAMS);
    let mut out = Vec::with_capacity(count as usize);
    for i in 0..count {
        let mut buf = ParamInfoBuffer::new();
        let Some(p) = ext.get_info(&h, i, &mut buf) else {
            continue;
        };
        if !(p.min_value.is_finite() && p.max_value.is_finite() && p.default_value.is_finite()) {
            continue;
        }
        out.push(ParamDesc {
            id: p.id.get(),
            name: lossy(p.name),
            module: lossy(p.module),
            min: p.min_value,
            max: p.max_value,
            default: p.default_value,
            stepped: p.flags.contains(ParamInfoFlags::IS_STEPPED),
            read_only: p.flags.contains(ParamInfoFlags::IS_READONLY),
            hidden: p.flags.contains(ParamInfoFlags::IS_HIDDEN),
        });
    }
    out
}

impl Drop for ClapInstance {
    fn drop(&mut self) {
        // Das Fenster zuerst abbauen: ein Plugin darf nach `deactivate` keine Oberfläche mehr haben.
        self.destroy_gui();
        // Auf den Audio-Thread warten, falls er gerade rechnet (höchstens ein Block).
        let mut spins = 0u32;
        let mut guard = loop {
            if let Some(g) = self.cell.lock() {
                break g;
            }
            spins += 1;
            if spins > 20_000 {
                // Der Audio-Thread hängt im Plugin. Leck statt Absturz: Instanz nicht abbauen.
                tracing::error!(id = %self.clap_id, "plugin busy on drop; leaking the instance");
                return;
            }
            std::thread::sleep(std::time::Duration::from_micros(100));
        };
        let inner = guard.inner().take();
        drop(guard);
        let Some(inner) = inner else { return };
        let stopped = match inner.proc {
            Proc::Started(p) => p.stop_processing(),
            Proc::Stopped(p) => p,
            Proc::Gone => return,
        };
        self.instance.deactivate(stopped);
    }
}

/// Der Rechner eines Plugins im Audio-Thread.
struct ClapProcessor {
    cell: Arc<ProcCell>,
    /// Reglerwerte, die mit dem nächsten Block hinausgehen (Kennung, Wert).
    pending: Vec<(u32, f64)>,
    /// `reset` wurde gerufen: der nächste Block beginnt mit „alle Noten aus“.
    flush: bool,
}

impl StereoProcessor for ClapProcessor {
    fn set_param(&mut self, id: u32, value: f64) {
        if !value.is_finite() {
            return;
        }
        if let Some(p) = self.pending.iter_mut().find(|p| p.0 == id) {
            p.1 = value;
        } else if self.pending.len() < PENDING_CAP {
            self.pending.push((id, value));
        }
    }

    fn process(
        &mut self,
        events: &[NoteEvent],
        in_left: &[f32],
        in_right: &[f32],
        out_left: &mut [f32],
        out_right: &mut [f32],
    ) {
        let len = out_left
            .len()
            .min(out_right.len())
            .min(in_left.len())
            .min(in_right.len());
        let silence = |l: &mut [f32], r: &mut [f32]| {
            l.fill(0.0);
            r.fill(0.0);
        };
        if self.cell.failed.load(Ordering::Relaxed) {
            silence(out_left, out_right);
            return;
        }
        let Some(mut guard) = self.cell.lock() else {
            silence(out_left, out_right);
            return;
        };
        let Some(inner) = guard.inner().as_mut() else {
            silence(out_left, out_right);
            return;
        };
        let mut flush = std::mem::take(&mut self.flush);
        let mut start = 0usize;
        while start < len {
            let n = (len - start).min(MAX_FRAMES as usize);
            let chunk = Chunk {
                start,
                len: n,
                total: len,
                flush,
            };
            flush = false;
            let ok = inner.run_chunk(
                &mut self.pending,
                events,
                &chunk,
                (in_left, in_right),
                (
                    &mut out_left[start..start + n],
                    &mut out_right[start..start + n],
                ),
            );
            if !ok {
                self.cell.errors.fetch_add(1, Ordering::Relaxed);
                self.cell.failed.store(true, Ordering::Relaxed);
                silence(&mut out_left[start..], &mut out_right[start..]);
                return;
            }
            start += n;
        }
        if len < out_left.len() {
            out_left[len..].fill(0.0);
            out_right[len..].fill(0.0);
        }
    }

    fn reset(&mut self) {
        self.flush = true;
    }
}

/// Ein Abschnitt eines Blocks (Blöcke über `MAX_FRAMES` werden zerlegt).
struct Chunk {
    start: usize,
    len: usize,
    /// Länge des ganzen Blocks (Events hinter dem Ende werden ans Ende geklemmt).
    total: usize,
    /// Der Abschnitt beginnt mit „alle Noten aus“ und einem Zurücksetzen des Plugins.
    flush: bool,
}

impl Inner {
    fn run_chunk(
        &mut self,
        pending: &mut Vec<(u32, f64)>,
        events: &[NoteEvent],
        chunk: &Chunk,
        input: (&[f32], &[f32]),
        output: (&mut [f32], &mut [f32]),
    ) -> bool {
        let (start, n) = (chunk.start, chunk.len);
        let (out_left, out_right) = output;

        // Lauf starten, wenn nötig (im Audio-Thread, wie CLAP es verlangt).
        if matches!(self.proc, Proc::Stopped(_)) {
            let Proc::Stopped(p) = std::mem::replace(&mut self.proc, Proc::Gone) else {
                return false;
            };
            match p.start_processing() {
                Ok(s) => self.proc = Proc::Started(s),
                Err(_) => {
                    // Kein Log im Audio-Thread: der Aufrufer setzt `failed`, der Control-Thread meldet es.
                    return false;
                }
            }
        }
        let Proc::Started(proc) = &mut self.proc else {
            return false;
        };
        if chunk.flush {
            proc.reset();
        }

        // Events dieses Abschnitts, nach Zeit geordnet: Reglerwerte und „alle aus“ zuerst.
        self.ev_in.clear();
        if chunk.flush {
            push_note(
                &mut self.ev_in,
                &NoteEvent {
                    offset: 0,
                    channel: 0,
                    kind: NoteEventKind::AllNotesOff,
                },
                0,
            );
        }
        if start == 0 {
            // Werte vom Control-Thread (Rücksetzen nach Rückgängig) zuerst.
            while let Ok((id, v)) = self.sets.pop() {
                if let Some(cid) = ClapId::from_raw(id)
                    && (self.ev_in.len() as usize) < EVENT_CAP
                {
                    self.ev_in
                        .push(&ParamValueEvent::new(0, cid, Pckn::match_all(), v));
                }
            }
            for (id, v) in pending.drain(..) {
                if let Some(cid) = ClapId::from_raw(id)
                    && (self.ev_in.len() as usize) < EVENT_CAP
                {
                    self.ev_in
                        .push(&ParamValueEvent::new(0, cid, Pckn::match_all(), v));
                }
            }
        }
        let last = chunk.total.saturating_sub(1) as u32;
        let (lo, hi) = (start as u32, (start + n) as u32);
        for e in events {
            let at = e.offset.min(last);
            if at >= lo && at < hi {
                push_note(&mut self.ev_in, e, at - lo);
            }
        }

        // Eingang.
        if self.has_input {
            let src_l = &input.0[start..start + n];
            let src_r = &input.1[start..start + n];
            if self.in_ch >= 2 {
                self.in_l[..n].copy_from_slice(src_l);
                self.in_r[..n].copy_from_slice(src_r);
            } else {
                for ((d, l), r) in self.in_l[..n].iter_mut().zip(src_l).zip(src_r) {
                    *d = 0.5 * (l + r);
                }
            }
        }

        self.ev_out.clear();
        let in_events = InputEvents::from_buffer(&self.ev_in);
        let mut out_events = OutputEvents::from_buffer(&mut self.ev_out);
        let steady = Some(self.steady);
        let result = {
            let (il, ir) = (&mut self.in_l[..n], &mut self.in_r[..n]);
            let (ol, or) = (&mut self.out_l[..n], &mut self.out_r[..n]);
            let inputs = if self.has_input {
                let chans = [InputChannel::variable(il), InputChannel::variable(ir)];
                self.in_ports.with_input_buffers([AudioPortBuffer {
                    latency: 0,
                    channels: AudioPortBufferType::f32_input_only(
                        chans.into_iter().take(self.in_ch.max(1)),
                    ),
                }])
            } else {
                InputAudioBuffers::empty()
            };
            let chans = [ol, or];
            let mut outputs = self.out_ports.with_output_buffers([AudioPortBuffer {
                latency: 0,
                channels: AudioPortBufferType::f32_output_only(chans.into_iter().take(self.out_ch)),
            }]);
            proc.process(
                &inputs,
                &mut outputs,
                &in_events,
                &mut out_events,
                steady,
                None,
            )
        };
        self.steady = self.steady.wrapping_add(n as u64);
        if result.is_err() {
            return false;
        }

        // Reglermeldungen der Plugin-Oberfläche nach draußen. Kein Warten: ist die Queue voll,
        // fallen Meldungen weg.
        for e in self.ev_out.iter() {
            let change = match e.as_core_event() {
                Some(CoreEventSpace::ParamValue(p)) => p.param_id().map(|id| ParamChange {
                    id: id.get(),
                    kind: ChangeKind::Value,
                    value: p.value(),
                }),
                Some(CoreEventSpace::ParamGestureBegin(g)) => g.param_id().map(|id| ParamChange {
                    id: id.get(),
                    kind: ChangeKind::GestureBegin,
                    value: 0.0,
                }),
                Some(CoreEventSpace::ParamGestureEnd(g)) => g.param_id().map(|id| ParamChange {
                    id: id.get(),
                    kind: ChangeKind::GestureEnd,
                    value: 0.0,
                }),
                _ => None,
            };
            if let Some(c) = change {
                let _ = self.feedback.push(c);
            }
        }

        // Ausgang, mit Schutz gegen NaN und Unendlich.
        let clean = |x: f32| if x.is_finite() { x } else { 0.0 };
        for (d, s) in out_left.iter_mut().zip(&self.out_l[..n]) {
            *d = clean(*s);
        }
        if self.out_ch >= 2 {
            for (d, s) in out_right.iter_mut().zip(&self.out_r[..n]) {
                *d = clean(*s);
            }
        } else {
            out_right.copy_from_slice(out_left);
        }
        true
    }
}

fn push_note(buf: &mut EventBuffer, e: &NoteEvent, time: u32) {
    // Kapazität nicht überschreiten: der Puffer wächst sonst im Audio-Thread.
    if buf.len() as usize >= EVENT_CAP {
        return;
    }
    match e.kind {
        NoteEventKind::NoteOn { note, velocity } => {
            let key = u16::from(note.min(127));
            buf.push(&NoteOnEvent::new(
                time,
                Pckn::new(0u16, u16::from(e.channel), key, u32::from(key)),
                f64::from(velocity.clamp(1, 127)) / 127.0,
            ));
        }
        NoteEventKind::NoteOff { note } => {
            let key = u16::from(note.min(127));
            buf.push(&NoteOffEvent::new(
                time,
                Pckn::new(0u16, u16::from(e.channel), key, u32::from(key)),
                0.0,
            ));
        }
        NoteEventKind::AllNotesOff => {
            buf.push(&NoteOffEvent::new(
                time,
                Pckn::new(Match::All, Match::All, Match::All, Match::All),
                0.0,
            ));
        }
        NoteEventKind::Sustain { on } => {
            buf.push(&MidiEvent::new(
                time,
                0,
                [0xB0 | (e.channel & 0x0F), 64, if on { 127 } else { 0 }],
            ));
        }
        NoteEventKind::PitchBend { value } => {
            let v = (i32::from(value) + 8192).clamp(0, 16383) as u16;
            buf.push(&MidiEvent::new(
                time,
                0,
                [
                    0xE0 | (e.channel & 0x0F),
                    (v & 0x7F) as u8,
                    ((v >> 7) & 0x7F) as u8,
                ],
            ));
        }
        NoteEventKind::ProgramChange { .. } => {}
    }
}
