//! Plugin im eigenen Prozess, Seite der Oberfläche (Paket 33).
//!
//! Ein fremdes Plugin ist fremder Code: Stürzt es ab oder hängt es, soll nicht die ganze App mit.
//! Darum läuft jedes Plugin in einem eigenen Hilfsprozess (`woa-plugin-host`, siehe
//! [`crate::child`]), den die Oberfläche startet und beaufsichtigt. Das Muster stammt von LMMS
//! `RemotePlugin` (GPL-2.0-or-later, `include/RemotePluginBase.h` und `src/core/RemotePlugin.cpp`,
//! Commit bed447b): Audio über gemeinsamen Speicher ([`crate::shm`]), Steuerung über einen
//! Nachrichtenkanal, ein ausgefallener Prozess macht das Plugin stumm, ohne dass die Oberfläche
//! wartet. Übernommen ist die Struktur, kein Code.
//!
//! # Audio-Block
//!
//! [`RemoteProcessor`] gibt dem Graphen die Schnittstelle [`StereoProcessor`]. Je Block kopiert er
//! Eingang, Events und Reglerwerte in den gemeinsamen Speicher, erhöht den Zähler und wartet kurz
//! auf die Antwort des Plugin-Prozesses. Das kostet **keine** zusätzliche Latenz. Das Warten ist
//! begrenzt (Frist ungefähr zwei Drittel der Blockdauer, beim Export zwei Sekunden); kommt die Antwort
//! nicht, gibt der Block Stille aus, und beim nächsten Block gilt „alle Noten aus“. Im Audio-Thread
//! gibt es keine Sperre, keine Allokation, kein Log und keine Datei- oder Pipe-Operation; gewartet
//! wird auf einem Zähler im gemeinsamen Speicher.
//!
//! # Aufsicht
//!
//! [`RemoteInstance::supervise`] läuft auf dem Control-Thread (jedes Bild der Oberfläche) und
//! erkennt: Prozess beendet (Absturz), Rechenfehler im Plugin und einen Block, der länger als
//! [`HANG_AFTER`] hängt. In allen Fällen wird der Prozess beendet, das Plugin ist stumm
//! ([`Health::Crashed`]), die Oberfläche läuft weiter. [`RemoteInstance::reload`] startet es neu.

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use woa_dsp::instrument::{NoteEvent, NoteEventKind};
use woa_dsp::plugin::StereoProcessor;

use crate::feedback::ParamEdit;
use crate::host::{HostError, MAX_FRAMES, ParamDesc};
use crate::scan::{Catalog, PluginInfo};
use crate::shm::{
    EV_CAP, MAILBOX_CAP, MIRROR_CAP, Mapping, RawEvent, RawSet, Reader, SET_CAP, STATE_ERROR,
    STATE_READY, Shared, Writer,
};

/// Kennzeichen, mit dem sich die App selbst als Plugin-Prozess aufrufen lässt.
pub const HOST_FLAG: &str = "--woa-plugin-host";
/// Pfad, unter dem das Test-Plugin in der Plugin-Liste steht (es hat keine Datei).
pub const TEST_PLUGIN_PATH: &str = "/woa/test-plugin";
/// So lange darf ein Block im Plugin-Prozess hängen, bevor er als hängend gilt.
pub const HANG_AFTER: Duration = Duration::from_millis(1500);
/// Wie lange das Laden eines Plugins im Plugin-Prozess dauern darf.
const LOAD_TIMEOUT: Duration = Duration::from_secs(30);
/// Wie lange eine Anfrage an den Plugin-Prozess dauern darf (Zustand, Fenster).
const CALL_TIMEOUT: Duration = Duration::from_secs(5);

/// Briefkasten: Art der Anfrage.
pub(crate) mod tag {
    /// Info: Regler, Fenster, Latenz.
    pub const INFO: u32 = 1;
    /// Zustand speichern.
    pub const SAVE: u32 = 2;
    /// Zustand laden.
    pub const LOAD: u32 = 3;
    /// Fenster öffnen.
    pub const OPEN_GUI: u32 = 4;
    /// Fenster schließen.
    pub const CLOSE_GUI: u32 = 5;
}

/// Wie ein Plugin-Prozess gestartet wird.
#[derive(Debug, Clone)]
pub struct HostConfig {
    /// Programm.
    pub exe: PathBuf,
    /// Argumente vor den eigenen (z. B. [`HOST_FLAG`] beim Selbstaufruf).
    pub prefix: Vec<String>,
    /// Weitere Argumente (Tests).
    pub extra: Vec<String>,
    /// Export oder Offline-Render: lange Frist je Block, damit nichts ausfällt.
    pub offline: bool,
    /// Feste Frist je Block in Millisekunden (Tests); 0 heißt: aus der Blockdauer errechnen.
    pub wait_ms: u32,
}

impl HostConfig {
    /// Sucht `woa-plugin-host`: erst die Umgebungsvariable `WOA_PLUGIN_HOST`, dann neben dem
    /// laufenden Programm.
    pub fn discover() -> Option<Self> {
        let name = if cfg!(windows) {
            "woa-plugin-host.exe"
        } else {
            "woa-plugin-host"
        };
        let candidates = [
            std::env::var_os("WOA_PLUGIN_HOST").map(PathBuf::from),
            std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|d| d.join(name))),
        ];
        candidates
            .into_iter()
            .flatten()
            .find(|p| p.is_file())
            .map(|exe| Self {
                exe,
                prefix: Vec::new(),
                extra: Vec::new(),
                offline: false,
                wait_ms: 0,
            })
    }

    /// Das laufende Programm selbst als Plugin-Prozess. Nur für Programme, die beim Start
    /// [`dispatch`](crate::child::dispatch) rufen.
    pub fn multicall() -> Option<Self> {
        let exe = std::env::current_exe().ok()?;
        Some(Self {
            exe,
            prefix: vec![HOST_FLAG.to_string()],
            extra: Vec::new(),
            offline: false,
            wait_ms: 0,
        })
    }

    /// Dieselbe Einstellung mit langer Frist (Export).
    pub fn for_export(&self) -> Self {
        Self {
            offline: true,
            ..self.clone()
        }
    }

    pub(crate) fn command(&self) -> Command {
        let mut c = Command::new(&self.exe);
        c.args(&self.prefix);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // Kein Konsolenfenster.
            c.creation_flags(0x0800_0000);
        }
        c
    }
}

/// Wo Plugins laufen.
#[derive(Debug, Clone, Default)]
pub enum Isolation {
    /// Im Programm selbst (kein Absturzschutz).
    #[default]
    InProcess,
    /// Je Plugin ein eigener Prozess.
    Process(HostConfig),
}

/// Wie es dem Plugin-Prozess geht.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Health {
    /// Läuft.
    Running,
    /// Ausgefallen; der Text sagt, wie.
    Crashed(String),
}

/// Gemeinsamer Zustand von [`RemoteInstance`] und [`RemoteProcessor`].
pub(crate) struct Link {
    map: Mapping,
    /// Schließt aus, dass zwei Audio-Aufrufe (alter und neuer Graph) zugleich den Block benutzen
    /// oder der Control-Thread den Speicher zurücksetzt, während ein Aufruf läuft.
    busy: AtomicBool,
    /// Der Plugin-Prozess nimmt Blöcke an.
    running: AtomicBool,
    /// Aufeinanderfolgende Blöcke ohne rechtzeitige Antwort.
    misses: AtomicU32,
    /// Alle Blöcke ohne rechtzeitige Antwort.
    late_total: AtomicU64,
    /// Zuletzt bekannte Latenz (bleibt über einen Neustart stehen, damit der Ausgleich im Graphen
    /// stimmt).
    latency: AtomicU32,
    offline: bool,
    wait_ms: u32,
    rate: f32,
}

struct BusyGuard<'a>(&'a Link);

impl Link {
    fn lock(&self) -> Option<BusyGuard<'_>> {
        self.busy
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .ok()
            .map(|_| BusyGuard(self))
    }

    fn shared(&self) -> &Shared {
        self.map.shared()
    }

    /// Frist für einen Block dieser Länge.
    fn deadline(&self, frames: usize) -> Duration {
        if self.offline {
            return Duration::from_secs(5);
        }
        if self.wait_ms > 0 {
            return Duration::from_millis(u64::from(self.wait_ms));
        }
        let block = frames as f64 / f64::from(self.rate.max(1.0));
        Duration::from_secs_f64((block * 0.66).clamp(0.001, 0.05))
    }
}

impl Drop for BusyGuard<'_> {
    fn drop(&mut self) {
        self.0.busy.store(false, Ordering::Release);
    }
}

/// Ein Plugin in einem eigenen Prozess, auf dem Control-Thread.
pub struct RemoteInstance {
    link: Arc<Link>,
    cfg: HostConfig,
    clap_id: String,
    clap_path: PathBuf,
    rate: f32,
    child: Option<Child>,
    params: Vec<ParamDesc>,
    index: std::collections::HashMap<u32, usize>,
    health: Health,
    last_state: Option<Vec<u8>>,
    stuck_since: Option<Instant>,
    watchdog_stop: Arc<AtomicBool>,
    watchdog: Option<std::thread::JoinHandle<()>>,
}

impl RemoteInstance {
    /// Startet den Plugin-Prozess für dieses Plugin und wartet, bis es geladen ist.
    pub fn new(cfg: &HostConfig, info: &PluginInfo, rate: f32) -> Result<Self, HostError> {
        let map = Mapping::create(&std::env::temp_dir(), &info.clap_id)
            .map_err(|e| HostError::Load(format!("gemeinsamer Speicher: {e}")))?;
        let link = Arc::new(Link {
            map,
            busy: AtomicBool::new(false),
            running: AtomicBool::new(false),
            misses: AtomicU32::new(0),
            late_total: AtomicU64::new(0),
            latency: AtomicU32::new(0),
            offline: cfg.offline,
            wait_ms: cfg.wait_ms,
            rate,
        });
        let stop = Arc::new(AtomicBool::new(false));
        let watchdog = {
            // Lebenszeichen: Bleibt es aus, beendet sich der Plugin-Prozess selbst (Absturz der App).
            let (l, s) = (Arc::clone(&link), Arc::clone(&stop));
            std::thread::Builder::new()
                .name("woa-plugin-beat".into())
                .spawn(move || {
                    while !s.load(Ordering::Relaxed) {
                        l.shared().beat.fetch_add(1, Ordering::Release);
                        std::thread::sleep(Duration::from_millis(100));
                    }
                })
                .ok()
        };
        let mut this = Self {
            link,
            cfg: cfg.clone(),
            clap_id: info.clap_id.clone(),
            clap_path: info.path.clone(),
            rate,
            child: None,
            params: Vec::new(),
            index: std::collections::HashMap::new(),
            health: Health::Crashed("noch nicht gestartet".into()),
            last_state: None,
            stuck_since: None,
            watchdog_stop: stop,
            watchdog,
        };
        this.start(None)?;
        Ok(this)
    }

    /// Wartet, bis kein Audio-Aufruf mehr im gemeinsamen Speicher steckt.
    fn quiesce(&self) -> bool {
        let deadline = Instant::now() + Duration::from_secs(6);
        loop {
            if let Some(g) = self.link.lock() {
                // Sofort wieder freigeben: `running` ist aus, neue Aufrufe kehren sofort zurück.
                drop(g);
                return true;
            }
            if Instant::now() > deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    fn kill_child(&mut self) {
        if let Some(mut c) = self.child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }

    /// Startet den Prozess neu und lädt das Plugin; `state` ist der Zustand, den es danach hat.
    fn start(&mut self, state: Option<&[u8]>) -> Result<(), HostError> {
        self.link.running.store(false, Ordering::Release);
        self.kill_child();
        if !self.quiesce() {
            return Err(HostError::Load(
                "der Audio-Thread hängt im gemeinsamen Speicher".into(),
            ));
        }
        let sh = self.link.shared();
        self.link.map.reset();
        sh.rate.store(self.rate as u32, Ordering::Relaxed);
        self.link.misses.store(0, Ordering::Relaxed);
        self.stuck_since = None;

        let mut cmd = self.cfg.command();
        cmd.arg("--shm")
            .arg(self.link.map.path())
            .arg("--file")
            .arg(&self.clap_path)
            .arg("--id")
            .arg(&self.clap_id)
            .arg("--rate")
            .arg(format!("{}", self.rate as u32));
        if self.clap_path == std::path::Path::new(TEST_PLUGIN_PATH) {
            cmd.arg("--test-plugin");
        }
        cmd.args(&self.cfg.extra)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit());
        let child = cmd
            .spawn()
            .map_err(|e| HostError::Load(format!("Plugin-Prozess startet nicht: {e}")))?;
        tracing::info!(plugin = %self.clap_id, pid = child.id(), "plugin process started");
        self.child = Some(child);

        let t0 = Instant::now();
        loop {
            match sh.state.load(Ordering::Acquire) {
                STATE_READY => break,
                STATE_ERROR => {
                    let n = (sh.mb_len.load(Ordering::Acquire) as usize).min(MAILBOX_CAP);
                    // SAFETY: Der Plugin-Prozess hat den Text vor `state` geschrieben und fasst den
                    // Briefkasten danach nicht mehr an.
                    let text =
                        unsafe { std::slice::from_raw_parts(self.link.map.mailbox_ptr(), n) };
                    let msg = String::from_utf8_lossy(text).into_owned();
                    self.kill_child();
                    return Err(HostError::Load(msg));
                }
                _ => {}
            }
            if let Some(c) = self.child.as_mut()
                && let Ok(Some(status)) = c.try_wait()
            {
                self.child = None;
                return Err(HostError::Load(format!(
                    "Plugin-Prozess endete beim Laden ({status})"
                )));
            }
            if t0.elapsed() > LOAD_TIMEOUT {
                self.kill_child();
                return Err(HostError::Load("Laden dauert zu lange".into()));
            }
            std::thread::sleep(Duration::from_millis(2));
        }

        // Info: Regler, Fenster, Latenz.
        let info = self.call(tag::INFO, &[], LOAD_TIMEOUT).map_err(|e| {
            self.kill_child();
            HostError::Load(e)
        })?;
        let mut r = Reader::new(&info);
        let _has_gui = r.u8();
        let latency = r.u32();
        let n = (r.u32() as usize).min(MIRROR_CAP);
        let mut params = Vec::with_capacity(n);
        for _ in 0..n {
            let id = r.u32();
            let name = r.string();
            let module = r.string();
            let (min, max, default) = (r.f64(), r.f64(), r.f64());
            let flags = r.u8();
            params.push(ParamDesc {
                id,
                name,
                module,
                min,
                max,
                default,
                stepped: flags & 1 != 0,
                read_only: flags & 2 != 0,
                hidden: flags & 4 != 0,
            });
        }
        self.index = params.iter().enumerate().map(|(i, p)| (p.id, i)).collect();
        self.params = params;
        self.link.latency.store(latency, Ordering::Relaxed);

        if let Some(bytes) = state {
            match self.call(tag::LOAD, bytes, CALL_TIMEOUT) {
                Ok(_) => self.last_state = Some(bytes.to_vec()),
                Err(e) => {
                    tracing::warn!(plugin = %self.clap_id, error = %e, "plugin state not applied");
                }
            }
        }
        self.health = Health::Running;
        self.link.running.store(true, Ordering::Release);
        Ok(())
    }

    /// Schickt eine Anfrage und wartet auf die Antwort (höchstens `timeout`). Bei Zeitüberschreitung
    /// oder Tod des Prozesses gilt das Plugin als ausgefallen.
    fn call(&mut self, tag: u32, payload: &[u8], timeout: Duration) -> Result<Vec<u8>, String> {
        if payload.len() > MAILBOX_CAP {
            return Err("Nachricht zu groß".into());
        }
        let sh = self.link.shared();
        let mb = self.link.map.mailbox_ptr();
        // SAFETY: Der Briefkasten gehört der Oberfläche, solange keine Anfrage läuft
        // (`mb_req == mb_rsp`); es läuft nie mehr als eine.
        unsafe { std::ptr::copy_nonoverlapping(payload.as_ptr(), mb, payload.len()) };
        sh.mb_tag.store(tag, Ordering::Relaxed);
        sh.mb_len.store(payload.len() as u32, Ordering::Relaxed);
        let req = sh.mb_req.load(Ordering::Relaxed) + 1;
        sh.mb_req.store(req, Ordering::Release);
        let t0 = Instant::now();
        loop {
            if sh.mb_rsp.load(Ordering::Acquire) == req {
                let n = (sh.mb_len.load(Ordering::Relaxed) as usize).min(MAILBOX_CAP);
                // SAFETY: Der Plugin-Prozess hat die Antwort vor `mb_rsp` geschrieben.
                let out = unsafe { std::slice::from_raw_parts(mb, n) }.to_vec();
                return if sh.mb_status.load(Ordering::Relaxed) == 0 {
                    Ok(out)
                } else {
                    Err(String::from_utf8_lossy(&out).into_owned())
                };
            }
            if let Some(c) = self.child.as_mut()
                && let Ok(Some(status)) = c.try_wait()
            {
                self.crash(format!("Plugin-Prozess beendet ({status})"));
                return Err("Plugin ist abgestürzt".into());
            }
            if t0.elapsed() > timeout {
                self.crash("Plugin antwortet nicht".into());
                return Err("Plugin antwortet nicht".into());
            }
            std::thread::sleep(Duration::from_micros(300));
        }
    }

    /// Das Plugin ist ausgefallen: Prozess beenden, stumm schalten, merken.
    fn crash(&mut self, reason: String) {
        if matches!(self.health, Health::Crashed(_)) {
            return;
        }
        self.link.running.store(false, Ordering::Release);
        self.kill_child();
        tracing::error!(plugin = %self.clap_id, reason = %reason, late_blocks = self.link.late_total.load(Ordering::Relaxed), "plugin process failed; track is muted");
        self.health = Health::Crashed(reason);
    }

    /// Prüft den Plugin-Prozess. Jedes Bild der Oberfläche rufen. Liefert `true`, wenn das Plugin
    /// in diesem Aufruf ausgefallen ist.
    pub fn supervise(&mut self) -> bool {
        if matches!(self.health, Health::Crashed(_)) {
            return false;
        }
        if let Some(c) = self.child.as_mut()
            && let Ok(Some(status)) = c.try_wait()
        {
            self.crash(format!("Plugin-Prozess beendet ({status})"));
            return true;
        }
        let sh = self.link.shared();
        if sh.failed.load(Ordering::Acquire) != 0 {
            self.crash("Plugin ist beim Rechnen ausgefallen".into());
            return true;
        }
        if sh.seq_sub.load(Ordering::Acquire) != sh.seq_done.load(Ordering::Acquire) {
            let since = *self.stuck_since.get_or_insert_with(Instant::now);
            if since.elapsed() > HANG_AFTER {
                self.crash("Plugin hängt".into());
                return true;
            }
        } else {
            self.stuck_since = None;
        }
        false
    }

    /// Wie es dem Plugin-Prozess geht.
    pub fn health(&self) -> &Health {
        &self.health
    }

    /// Prozesskennung des Plugin-Prozesses (zur Anzeige und für den Absturztest).
    pub fn pid(&self) -> Option<u32> {
        self.child.as_ref().map(Child::id)
    }

    /// Beendet den Plugin-Prozess hart, als wäre das Plugin abgestürzt (Absturztest im Menü).
    pub fn simulate_crash(&mut self) {
        tracing::warn!(plugin = %self.clap_id, "plugin crash simulated");
        if let Some(c) = self.child.as_mut() {
            let _ = c.kill();
        }
    }

    /// Startet das Plugin neu, mit dem zuletzt bekannten Zustand; kennt die Instanz keinen,
    /// gilt `fallback` (der Zustand aus dem Projekt).
    pub fn reload(&mut self, fallback: Option<&[u8]>) -> Result<(), HostError> {
        let bytes = self
            .last_state
            .clone()
            .or_else(|| fallback.map(<[u8]>::to_vec));
        let r = self.start(bytes.as_deref());
        if let Err(e) = &r {
            self.health = Health::Crashed(e.to_string());
            self.link.running.store(false, Ordering::Release);
        }
        r
    }

    /// Wie oft ein Block nicht rechtzeitig fertig wurde.
    pub fn late_blocks(&self) -> u64 {
        self.link.late_total.load(Ordering::Relaxed)
    }

    /// Latenz des Plugins in Abtastwerten.
    pub fn latency(&self) -> u32 {
        self.link.latency.load(Ordering::Relaxed)
    }

    /// Die Regler des Plugins.
    pub fn params(&self) -> &[ParamDesc] {
        &self.params
    }

    /// Ob das Plugin ausgefallen ist.
    pub fn failed(&self) -> bool {
        matches!(self.health, Health::Crashed(_))
    }

    /// Wert eines Reglers (aus dem Spiegel im gemeinsamen Speicher; höchstens 0,1 s alt).
    pub fn param_value(&mut self, id: u32) -> Option<f64> {
        let i = *self.index.get(&id)?;
        let bits = self.link.shared().mirror.get(i)?.load(Ordering::Relaxed);
        let v = f64::from_bits(bits);
        v.is_finite().then_some(v)
    }

    /// Alle Reglerwerte (Kennung, Wert).
    pub fn all_values(&mut self) -> Vec<(u32, f64)> {
        let ids: Vec<u32> = self.params.iter().map(|p| p.id).collect();
        ids.into_iter()
            .filter_map(|id| self.param_value(id).map(|v| (id, v)))
            .collect()
    }

    /// Setzt einen Reglerwert im Plugin (über den Ring im gemeinsamen Speicher).
    pub fn push_param(&mut self, id: u32, value: f64) -> bool {
        if !value.is_finite() || !self.link.running.load(Ordering::Acquire) {
            return false;
        }
        self.link
            .shared()
            .set_ring
            .push(RawSet { id, pad: 0, value })
    }

    /// Ob das Plugin ein schwebendes Fenster anbietet.
    pub fn has_gui(&mut self) -> bool {
        cfg!(unix) && self.link.shared().has_gui.load(Ordering::Acquire) != 0
    }

    /// Ob das Fenster offen ist.
    pub fn gui_open(&self) -> bool {
        self.link.running.load(Ordering::Acquire)
            && self.link.shared().gui_open.load(Ordering::Acquire) != 0
    }

    /// Öffnet das Fenster (es gehört dem Plugin-Prozess).
    pub fn open_gui(&mut self, title: &str) -> Result<(), HostError> {
        if !cfg!(unix) {
            return Err(HostError::Gui(
                "unter Windows noch nicht im Schutzprozess".into(),
            ));
        }
        if self.failed() {
            return Err(HostError::Gui("Das Plugin ist ausgefallen".into()));
        }
        self.call(tag::OPEN_GUI, title.as_bytes(), CALL_TIMEOUT)
            .map(|_| ())
            .map_err(HostError::Gui)
    }

    /// Schließt das Fenster.
    pub fn close_gui(&mut self) {
        if !self.failed()
            && let Err(e) = self.call(tag::CLOSE_GUI, &[], CALL_TIMEOUT)
        {
            tracing::debug!(error = %e, "plugin window did not close");
        }
    }

    /// Nichts zu tun: Der Plugin-Prozess bedient seine Zeitgeber selbst. Hält nur die Aufsicht.
    pub fn pump(&mut self) {
        self.supervise();
    }

    /// Reglerbewegungen aus dem Plugin-Fenster.
    pub fn poll_feedback(&mut self, out: &mut Vec<ParamEdit>) {
        let sh = self.link.shared();
        while let Some(e) = sh.edit_ring.pop() {
            out.push(if e.kind == 0 {
                ParamEdit::Param {
                    id: e.id,
                    old: e.old,
                    new: e.new,
                }
            } else {
                ParamEdit::WholeState
            });
        }
    }

    /// Kennung des Plugins.
    pub fn clap_id(&self) -> &str {
        &self.clap_id
    }

    /// Abtastrate.
    pub fn rate(&self) -> f32 {
        self.rate
    }

    /// Ein neuer Rechner für den Graphen (er bleibt über einen Neustart des Prozesses gültig).
    pub fn processor(&self) -> Box<dyn StereoProcessor> {
        Box::new(RemoteProcessor {
            link: Arc::clone(&self.link),
            pending: Vec::with_capacity(SET_CAP),
            flush: false,
        })
    }

    /// Der Zustand des Plugins als Bytes.
    pub fn save_state(&mut self) -> Result<Vec<u8>, HostError> {
        if self.failed() {
            return Err(HostError::State("Das Plugin ist ausgefallen".into()));
        }
        let bytes = self
            .call(tag::SAVE, &[], CALL_TIMEOUT)
            .map_err(HostError::State)?;
        self.last_state = Some(bytes.clone());
        Ok(bytes)
    }

    /// Setzt den Zustand des Plugins.
    pub fn load_state(&mut self, bytes: &[u8]) -> Result<(), HostError> {
        if self.failed() {
            return Err(HostError::State("Das Plugin ist ausgefallen".into()));
        }
        self.call(tag::LOAD, bytes, CALL_TIMEOUT)
            .map_err(HostError::State)?;
        self.last_state = Some(bytes.to_vec());
        Ok(())
    }
}

impl Drop for RemoteInstance {
    fn drop(&mut self) {
        self.link.running.store(false, Ordering::Release);
        self.link.shared().shutdown.store(1, Ordering::Release);
        self.watchdog_stop.store(true, Ordering::Relaxed);
        if let Some(mut c) = self.child.take() {
            // Der Prozess beendet sich auf `shutdown` selbst; hängt er, wird er beendet.
            let t0 = Instant::now();
            loop {
                match c.try_wait() {
                    Ok(Some(_)) => break,
                    Ok(None) if t0.elapsed() < Duration::from_millis(500) => {
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    _ => {
                        let _ = c.kill();
                        let _ = c.wait();
                        break;
                    }
                }
            }
        }
        if let Some(h) = self.watchdog.take() {
            let _ = h.join();
        }
    }
}

/// Der Rechner eines Plugins in einem eigenen Prozess, im Audio-Thread.
struct RemoteProcessor {
    link: Arc<Link>,
    /// Reglerwerte, die mit dem nächsten angenommenen Block hinausgehen.
    pending: Vec<(u32, f64)>,
    /// Der nächste Block beginnt mit „alles zurücksetzen“ (nach `reset` oder verpasstem Block).
    flush: bool,
}

/// Ein Event als Zahlen im gemeinsamen Speicher.
fn encode_event(e: &NoteEvent, offset: u32) -> RawEvent {
    let (tag, a, b) = match e.kind {
        NoteEventKind::NoteOn { note, velocity } => (1, i32::from(note), i32::from(velocity)),
        NoteEventKind::NoteOff { note } => (2, i32::from(note), 0),
        NoteEventKind::AllNotesOff => (3, 0, 0),
        NoteEventKind::ProgramChange { program } => (4, i32::from(program), 0),
        NoteEventKind::Sustain { on } => (5, i32::from(on), 0),
        NoteEventKind::PitchBend { value } => (6, i32::from(value), 0),
    };
    RawEvent {
        offset,
        tag: tag | (u32::from(e.channel) << 8),
        a,
        b,
    }
}

/// Gegenstück zu [`encode_event`] (Plugin-Prozess).
pub(crate) fn decode_event(r: &RawEvent) -> Option<NoteEvent> {
    let kind = match r.tag & 0xFF {
        1 => NoteEventKind::NoteOn {
            note: r.a.clamp(0, 127) as u8,
            velocity: r.b.clamp(0, 127) as u8,
        },
        2 => NoteEventKind::NoteOff {
            note: r.a.clamp(0, 127) as u8,
        },
        3 => NoteEventKind::AllNotesOff,
        4 => NoteEventKind::ProgramChange {
            program: r.a.clamp(0, 127) as u8,
        },
        5 => NoteEventKind::Sustain { on: r.a != 0 },
        6 => NoteEventKind::PitchBend {
            value: r.a.clamp(-8192, 8191) as i16,
        },
        _ => return None,
    };
    Some(NoteEvent {
        offset: r.offset,
        channel: ((r.tag >> 8) & 0x0F) as u8,
        kind,
    })
}

impl RemoteProcessor {
    /// Ein Block höchstens `MAX_FRAMES` lang. `false`: nicht angenommen oder nicht rechtzeitig fertig.
    #[allow(clippy::too_many_arguments)]
    fn run_chunk(
        &mut self,
        events: &[NoteEvent],
        total: usize,
        start: usize,
        n: usize,
        input: (&[f32], &[f32]),
        output: (&mut [f32], &mut [f32]),
    ) -> bool {
        let link = &*self.link;
        let sh = link.shared();
        let seq = sh.seq_sub.load(Ordering::Relaxed);
        if sh.seq_done.load(Ordering::Acquire) != seq {
            // Der Plugin-Prozess rechnet noch am vorigen (verspäteten) Block.
            link.misses.fetch_add(1, Ordering::Relaxed);
            link.late_total.fetch_add(1, Ordering::Relaxed);
            self.flush = true;
            return false;
        }
        // SAFETY: `seq_done == seq_sub`: der Block gehört der Oberfläche; `busy` schließt einen
        // zweiten Schreiber aus. Die Puffer sind `MAX_FRAMES` lang, `n <= MAX_FRAMES`.
        unsafe {
            std::ptr::copy_nonoverlapping(input.0.as_ptr().add(start), sh.in_l_ptr(), n);
            std::ptr::copy_nonoverlapping(input.1.as_ptr().add(start), sh.in_r_ptr(), n);
            let last = total.saturating_sub(1) as u32;
            let (lo, hi) = (start as u32, (start + n) as u32);
            let mut count = 0usize;
            let base = sh.events_ptr();
            for e in events {
                let at = e.offset.min(last);
                if at >= lo && at < hi && count < EV_CAP {
                    base.add(count).write(encode_event(e, at - lo));
                    count += 1;
                }
            }
            sh.n_events.store(count as u32, Ordering::Relaxed);
            let sets = sh.sets_ptr();
            let k = self.pending.len().min(SET_CAP);
            for (i, (id, value)) in self.pending.iter().take(k).enumerate() {
                sets.add(i).write(RawSet {
                    id: *id,
                    pad: 0,
                    value: *value,
                });
            }
            sh.n_sets.store(k as u32, Ordering::Relaxed);
        }
        self.pending.clear();
        sh.frames.store(n as u32, Ordering::Relaxed);
        sh.flush.store(
            u32::from(std::mem::take(&mut self.flush)),
            Ordering::Relaxed,
        );
        sh.seq_sub.store(seq + 1, Ordering::Release);

        // Warten, begrenzt.
        let started = Instant::now();
        let deadline = link.deadline(total);
        let mut spins = 0u32;
        loop {
            if sh.seq_done.load(Ordering::Acquire) == seq + 1 {
                break;
            }
            spins += 1;
            if spins < 2000 {
                std::hint::spin_loop();
                continue;
            }
            if started.elapsed() > deadline || !link.running.load(Ordering::Acquire) {
                link.misses.fetch_add(1, Ordering::Relaxed);
                link.late_total.fetch_add(1, Ordering::Relaxed);
                self.flush = true;
                return false;
            }
            std::thread::yield_now();
        }
        link.misses.store(0, Ordering::Relaxed);
        // SAFETY: Der Block ist fertig (`seq_done == seq + 1`, Acquire); der Plugin-Prozess schreibt
        // erst beim nächsten Block wieder.
        unsafe {
            std::ptr::copy_nonoverlapping(sh.out_l_ptr(), output.0.as_mut_ptr(), n);
            std::ptr::copy_nonoverlapping(sh.out_r_ptr(), output.1.as_mut_ptr(), n);
        }
        true
    }
}

impl StereoProcessor for RemoteProcessor {
    fn set_param(&mut self, id: u32, value: f64) {
        if !value.is_finite() {
            return;
        }
        if let Some(p) = self.pending.iter_mut().find(|p| p.0 == id) {
            p.1 = value;
        } else if self.pending.len() < SET_CAP {
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
        out_left.fill(0.0);
        out_right.fill(0.0);
        if !self.link.running.load(Ordering::Acquire) {
            return;
        }
        let link = Arc::clone(&self.link);
        let Some(_guard) = link.lock() else {
            return;
        };
        // Zwischen Prüfung und Sperre kann ein Neustart begonnen haben.
        if !link.running.load(Ordering::Acquire) {
            return;
        }
        let mut start = 0usize;
        while start < len {
            let n = (len - start).min(MAX_FRAMES as usize);
            let ok = self.run_chunk(
                events,
                len,
                start,
                n,
                (in_left, in_right),
                (
                    &mut out_left[start..start + n],
                    &mut out_right[start..start + n],
                ),
            );
            if !ok {
                return;
            }
            start += n;
        }
    }

    fn reset(&mut self) {
        self.flush = true;
    }

    fn latency(&self) -> u32 {
        self.link.latency.load(Ordering::Relaxed)
    }
}

/// Aufruf aus einem Anfrageteil: Antwort schreiben (Plugin-Prozess).
pub(crate) fn write_reply(map: &Mapping, req: u64, ok: bool, body: &[u8]) {
    let sh = map.shared();
    let n = body.len().min(MAILBOX_CAP);
    // SAFETY: Der Briefkasten gehört dem Plugin-Prozess, solange `mb_rsp != mb_req`.
    unsafe { std::ptr::copy_nonoverlapping(body.as_ptr(), map.mailbox_ptr(), n) };
    sh.mb_len.store(n as u32, Ordering::Relaxed);
    sh.mb_status.store(u32::from(!ok), Ordering::Relaxed);
    sh.mb_rsp.store(req, Ordering::Release);
}

/// Hilfe für den Plugin-Prozess: Info-Antwort aufbauen.
pub(crate) fn info_reply(has_gui: bool, latency: u32, params: &[ParamDesc]) -> Vec<u8> {
    let mut w = Writer::default();
    w.u8(u8::from(has_gui));
    w.u32(latency);
    w.u32(params.len().min(MIRROR_CAP) as u32);
    for p in params.iter().take(MIRROR_CAP) {
        w.u32(p.id);
        w.str(&p.name);
        w.str(&p.module);
        w.f64(p.min);
        w.f64(p.max);
        w.f64(p.default);
        w.u8(u8::from(p.stepped) | (u8::from(p.read_only) << 1) | (u8::from(p.hidden) << 2));
    }
    w.0
}

impl Catalog {
    /// Wie [`Catalog::scan`], aber jede Datei in einem eigenen Prozess: Eine Bibliothek, die beim
    /// Laden abstürzt oder hängt, wird zum [`crate::ScanProblem`], die App läuft weiter. Die
    /// gefundenen Plugins lassen sich nur über [`Isolation::Process`] laden.
    pub fn scan_isolated(roots: &[PathBuf], host: &HostConfig) -> Self {
        let mut cat = Self::empty();
        for file in crate::scan::find_files(roots) {
            match scan_one(&file, host) {
                Ok(list) => {
                    if list.is_empty() {
                        cat.add_problem(file, "keine Plugin-Fabrik".into());
                    }
                    for info in list {
                        cat.add_unloaded(info);
                    }
                }
                Err(reason) => {
                    tracing::warn!(file = %file.display(), error = %reason, "clap scan failed");
                    cat.add_problem(file, reason);
                }
            }
        }
        cat.sort();
        tracing::info!(
            plugins = cat.plugins().len(),
            problems = cat.problems().len(),
            "clap scan done (isolated)"
        );
        cat
    }

    /// Sucht in den Standardordnern, jede Datei in einem eigenen Prozess.
    pub fn scan_default_isolated(host: &HostConfig) -> Self {
        Self::scan_isolated(&crate::scan::search_paths(), host)
    }
}

/// So lange darf das Laden einer einzelnen Datei bei der Suche dauern.
const SCAN_TIMEOUT: Duration = Duration::from_secs(20);

fn scan_one(file: &std::path::Path, host: &HostConfig) -> Result<Vec<PluginInfo>, String> {
    let out = std::env::temp_dir().join(format!(
        "woa-scan-{}-{}.txt",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos())
    ));
    let mut cmd = host.command();
    cmd.arg("--scan")
        .arg(file)
        .arg("--out")
        .arg(&out)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Suchprozess startet nicht: {e}"))?;
    let t0 = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) if t0.elapsed() < SCAN_TIMEOUT => {
                std::thread::sleep(Duration::from_millis(5));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = std::fs::remove_file(&out);
                return Err("Laden dauert zu lange (Plugin hängt)".into());
            }
            Err(e) => return Err(format!("Suchprozess: {e}")),
        }
    };
    let text = std::fs::read_to_string(&out).unwrap_or_default();
    let _ = std::fs::remove_file(&out);
    if !status.success() {
        return Err(format!("Plugin ist beim Laden abgestürzt ({status})"));
    }
    let mut list = Vec::new();
    for line in text.lines() {
        let mut f = line.split('\t');
        if f.next() != Some("I") {
            continue;
        }
        let (Some(id), Some(name), Some(vendor), Some(features)) =
            (f.next(), f.next(), f.next(), f.next())
        else {
            continue;
        };
        list.push(PluginInfo {
            clap_id: id.to_string(),
            name: name.to_string(),
            vendor: vendor.to_string(),
            features: features
                .split(',')
                .filter(|x| !x.is_empty())
                .map(str::to_string)
                .collect(),
            path: file.to_path_buf(),
        });
    }
    Ok(list)
}
