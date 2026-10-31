//! Das Plugin-Rack: hält die geladenen Plugins eines Projekts (ein [`ClapInstance`] je Platz),
//! gibt der Engine ihre Rechner und nimmt den Zustand der laufenden Plugins zurück ins Projekt.
//!
//! Das Rack lebt im Control-Thread. Das Projekt kennt nur die CLAP-Kennung; fehlt das Plugin
//! auf diesem Rechner, bleibt der Platz im Projekt, die Spur klingt nicht (Instrument) oder
//! reicht durch (Effekt), und der Platz meldet [`SlotStatus::Missing`].

use std::collections::HashMap;
use std::sync::Arc;

use woa_dsp::plugin::{PluginEffect, TrackPlugins};
use woa_model::{Command, InsertId, PluginParam, PluginRole, PluginSlot, Project, Track};

use crate::feedback::ParamEdit;
use crate::host::{ClapInstance, HostError, ParamDesc};
use crate::instance::Instance;
use crate::remote::{Isolation, RemoteInstance};
use crate::scan::Catalog;

/// Text im Status eines Plugins, dessen Prozess ausgefallen ist.
pub const CRASHED: &str = "Plugin abgestürzt – neu laden";

/// Wie es einem Plugin-Platz geht.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlotStatus {
    /// Geladen und rechenbereit.
    Loaded,
    /// Das Plugin ist auf diesem Rechner nicht installiert.
    Missing,
    /// Das Plugin ließ sich nicht laden oder ist ausgefallen (Text für den Nutzer).
    Failed(String),
}

struct Entry {
    clap_id: String,
    status: SlotStatus,
    instance: Option<Instance>,
    /// Reglerwerte nach dem Laden: dorthin geht ein Regler zurück, wenn „Rückgängig“ seinen
    /// gespeicherten Wert aus dem Projekt nimmt.
    baseline: HashMap<u32, f64>,
    /// Wert vor der ersten Änderung aus der Plugin-Oberfläche (genauer als `baseline`).
    origin: HashMap<u32, f64>,
    /// Zustand des Plugins vor jedem Laden eines Projektzustands (der „Grundzustand“).
    initial_state: Option<Vec<u8>>,
    /// Was das Projekt zuletzt für diesen Platz hielt (um Änderungen durch Rückgängig zu merken).
    seen_params: Vec<PluginParam>,
    seen_state: Option<String>,
    /// Der Ausfall wurde der Oberfläche schon gemeldet.
    reported: bool,
}

/// Geladene Plugins eines Projekts.
pub struct PluginRack {
    catalog: Arc<Catalog>,
    rate: f32,
    isolation: Isolation,
    slots: HashMap<u64, Entry>,
}

impl PluginRack {
    /// Neues, leeres Rack für diese Abtastrate.
    pub fn new(catalog: Arc<Catalog>, rate: f32) -> Self {
        Self {
            catalog,
            rate,
            isolation: Isolation::InProcess,
            slots: HashMap::new(),
        }
    }

    /// Lässt jedes Plugin in einem eigenen Prozess laufen (Absturzschutz, Paket 33).
    #[must_use]
    pub fn with_isolation(mut self, isolation: Isolation) -> Self {
        self.isolation = isolation;
        self
    }

    /// Wo die Plugins laufen.
    pub fn isolation(&self) -> &Isolation {
        &self.isolation
    }

    /// Die Plugin-Liste dieses Rechners.
    pub fn catalog(&self) -> &Catalog {
        &self.catalog
    }

    /// Ein zweites, leeres Rack mit derselben Plugin-Liste und Rate. Der Export rechnet damit
    /// eigene Instanzen, damit er die laufende Wiedergabe nicht stört.
    pub fn fresh(&self) -> Self {
        // Der Export hat lange Fristen: Ein langsamer Block ergibt dort nie Stille.
        let isolation = match &self.isolation {
            Isolation::InProcess => Isolation::InProcess,
            Isolation::Process(c) => Isolation::Process(c.for_export()),
        };
        Self::new(Arc::clone(&self.catalog), self.rate).with_isolation(isolation)
    }

    /// Wie [`Self::fresh`], aber erst im Ziel-Thread gebaut: Ein Rack selbst darf den Thread
    /// nicht wechseln (Plugin-Instanzen), seine Bauteile schon (Spur-Export, Ticket 229).
    pub fn fresh_later(&self) -> impl FnOnce() -> Self + Send + 'static {
        let isolation = match &self.isolation {
            Isolation::InProcess => Isolation::InProcess,
            Isolation::Process(c) => Isolation::Process(c.for_export()),
        };
        let catalog = Arc::clone(&self.catalog);
        let rate = self.rate;
        move || Self::new(catalog, rate).with_isolation(isolation)
    }

    /// Lädt alle Plätze neu (nach einer neuen Suche, wenn ein Plugin dazugekommen sein kann).
    pub fn reset(&mut self, catalog: Arc<Catalog>) {
        self.catalog = catalog;
        self.slots.clear();
    }

    /// Ob es für diesen Platz eine Instanz gibt.
    pub fn is_loaded(&self, id: InsertId) -> bool {
        self.slots.get(&id.0).is_some_and(|e| e.instance.is_some())
    }

    /// Gleicht das Rack mit dem Projekt ab: neue Plätze werden geladen (mit ihrem gespeicherten
    /// Zustand), entfernte abgebaut. Ein Platz, der schon geladen ist, bleibt unberührt.
    /// Liefert `true`, wenn sich etwas geändert hat (dann braucht die Engine einen neuen Graphen).
    pub fn sync(&mut self, project: &Project) -> bool {
        let mut changed = false;
        let mut wanted: Vec<u64> = Vec::new();
        for t in &project.tracks {
            for slot in &t.plugins {
                wanted.push(slot.id.0);
                let keep = self
                    .slots
                    .get(&slot.id.0)
                    .is_some_and(|e| e.clap_id == slot.clap_id);
                if !keep {
                    let entry = self.load(slot);
                    self.slots.insert(slot.id.0, entry);
                    changed = true;
                }
            }
        }
        let before = self.slots.len();
        self.slots.retain(|id, _| wanted.contains(id));
        changed || self.slots.len() != before
    }

    fn load(&self, slot: &PluginSlot) -> Entry {
        let mut entry = Entry {
            clap_id: slot.clap_id.clone(),
            status: SlotStatus::Missing,
            instance: None,
            baseline: HashMap::new(),
            origin: HashMap::new(),
            initial_state: None,
            seen_params: slot.params.clone(),
            seen_state: slot.state.clone(),
            reported: false,
        };
        match self.open(&slot.clap_id) {
            Ok(mut inst) => {
                entry.initial_state = inst.save_state().ok();
                if let Some(bytes) = slot.state_bytes()
                    && let Err(e) = inst.load_state(&bytes)
                {
                    tracing::warn!(plugin = %slot.clap_id, error = %e, "plugin state not applied");
                }
                entry.baseline = inst.all_values().into_iter().collect();
                entry.status = SlotStatus::Loaded;
                entry.instance = Some(inst);
            }
            Err(HostError::Unknown(_)) => {
                tracing::warn!(plugin = %slot.clap_id, "plugin is not installed");
            }
            Err(e) => {
                tracing::error!(plugin = %slot.clap_id, error = %e, "plugin failed to load");
                entry.status = SlotStatus::Failed(e.to_string());
            }
        }
        entry
    }

    fn open(&self, clap_id: &str) -> Result<Instance, HostError> {
        match &self.isolation {
            Isolation::InProcess => ClapInstance::new(&self.catalog, clap_id, self.rate)
                .map(|i| Instance::Local(Box::new(i))),
            Isolation::Process(cfg) => {
                let info = self
                    .catalog
                    .find(clap_id)
                    .ok_or_else(|| HostError::Unknown(clap_id.to_string()))?;
                RemoteInstance::new(cfg, info, self.rate).map(|i| Instance::Remote(Box::new(i)))
            }
        }
    }

    /// Status eines Platzes (`None`: das Rack kennt ihn nicht; erst [`PluginRack::sync`]).
    pub fn status(&self, id: InsertId) -> Option<SlotStatus> {
        let e = self.slots.get(&id.0)?;
        if let Some(i) = &e.instance
            && i.failed()
        {
            let text = if i.is_remote() {
                CRASHED.to_string()
            } else {
                "Das Plugin ist ausgefallen".to_string()
            };
            return Some(SlotStatus::Failed(text));
        }
        Some(e.status.clone())
    }

    /// Die Regler eines geladenen Plugins.
    pub fn params(&self, id: InsertId) -> &[ParamDesc] {
        self.slots
            .get(&id.0)
            .and_then(|e| e.instance.as_ref())
            .map_or(&[], |i| i.params())
    }

    /// Wert eines Reglers laut Plugin.
    pub fn param_value(&mut self, id: InsertId, param: u32) -> Option<f64> {
        self.slots
            .get_mut(&id.0)?
            .instance
            .as_mut()?
            .param_value(param)
    }

    /// Die Plugin-Rechner einer Spur für den Graph-Bau. Gespeicherte Reglerwerte des Projekts
    /// gehen als Anfangswerte mit.
    pub fn track_plugins(&self, track: &Track) -> TrackPlugins {
        let mut out = TrackPlugins::default();
        for slot in &track.plugins {
            let Some(inst) = self.slots.get(&slot.id.0).and_then(|e| e.instance.as_ref()) else {
                continue;
            };
            // Ein Plugin im eigenen Prozess bleibt im Graphen (stumm), damit ein Neuladen ohne
            // neuen Graphen wirkt.
            if inst.failed() && !inst.is_remote() {
                continue;
            }
            let mut proc = inst.processor();
            for p in &slot.params {
                proc.set_param(p.id, p.value);
            }
            match slot.role {
                PluginRole::Instrument => out.instrument = Some(proc),
                PluginRole::Effect => out.effects.push(PluginEffect {
                    id: slot.id.0,
                    processor: proc,
                    bypass: slot.bypass,
                }),
            }
        }
        out
    }

    /// Ob dieses Plugin ein eigenes Fenster anbietet.
    pub fn has_gui(&mut self, id: InsertId) -> bool {
        self.instance_mut(id).is_some_and(Instance::has_gui)
    }

    /// Ob das Fenster dieses Plugins offen ist.
    pub fn gui_open(&self, id: InsertId) -> bool {
        self.slots
            .get(&id.0)
            .and_then(|e| e.instance.as_ref())
            .is_some_and(Instance::gui_open)
    }

    /// Ob irgendein Plugin-Fenster offen ist (dann muss die Oberfläche ständig neu zeichnen).
    pub fn any_gui_open(&self) -> bool {
        self.slots
            .values()
            .filter_map(|e| e.instance.as_ref())
            .any(Instance::gui_open)
    }

    /// Öffnet das Fenster eines Plugins.
    pub fn open_gui(&mut self, id: InsertId, title: &str) -> Result<(), HostError> {
        match self.instance_mut(id) {
            Some(i) => i.open_gui(title),
            None => Err(HostError::Gui("Das Plugin ist nicht geladen".into())),
        }
    }

    /// Schließt das Fenster eines Plugins.
    pub fn close_gui(&mut self, id: InsertId) {
        if let Some(i) = self.instance_mut(id) {
            i.close_gui();
        }
    }

    fn instance_mut(&mut self, id: InsertId) -> Option<&mut Instance> {
        self.slots.get_mut(&id.0)?.instance.as_mut()
    }

    /// Bedient alle Plugins (Zeitgeber, Oberflächen-Ereignisse). Jedes Bild der Oberfläche.
    pub fn pump(&mut self) {
        for e in self.slots.values_mut() {
            if let Some(i) = e.instance.as_mut() {
                i.pump();
            }
        }
    }

    /// Startet ein ausgefallenes Plugin neu (eigener Prozess): Zustand und Reglerwerte kommen
    /// zurück, der Graph bleibt derselbe. Läuft das Plugin noch, ändert sich nichts.
    pub fn reload(&mut self, project: &Project, id: InsertId) -> Result<(), HostError> {
        let slot = project
            .tracks
            .iter()
            .find_map(|t| t.plugin(id))
            .ok_or_else(|| HostError::Load("Platz unbekannt".into()))?;
        let e = self
            .slots
            .get_mut(&id.0)
            .ok_or_else(|| HostError::Load("Platz nicht geladen".into()))?;
        let inst = e
            .instance
            .as_mut()
            .ok_or_else(|| HostError::Load("Plugin nicht geladen".into()))?;
        if !inst.failed() {
            return Ok(());
        }
        let bytes = slot.state_bytes();
        inst.reload(bytes.as_deref())?;
        for p in &slot.params {
            inst.push_param(p.id, p.value);
        }
        e.baseline = inst.all_values().into_iter().collect();
        e.origin.clear();
        e.status = SlotStatus::Loaded;
        e.reported = false;
        tracing::info!(plugin = %slot.clap_id, "plugin reloaded after failure");
        Ok(())
    }

    /// Beendet den Plugin-Prozess dieses Platzes hart (Absturztest im Menü, nur mit
    /// Schutzprozess).
    pub fn simulate_crash(&mut self, id: InsertId) {
        if let Some(i) = self.instance_mut(id) {
            i.simulate_crash();
        }
    }

    /// Plätze, die seit dem letzten Aufruf ausgefallen sind (jeder nur einmal), damit die
    /// Oberfläche es melden kann.
    pub fn new_failures(&mut self) -> Vec<InsertId> {
        let mut out = Vec::new();
        for (id, e) in &mut self.slots {
            if !e.reported && e.instance.as_ref().is_some_and(Instance::failed) {
                e.reported = true;
                out.push(InsertId(*id));
            }
        }
        out
    }

    /// Ob ein Plugin im eigenen Prozess läuft (dann braucht die Oberfläche den Takt für die
    /// Aufsicht).
    pub fn has_isolated(&self) -> bool {
        self.slots
            .values()
            .filter_map(|e| e.instance.as_ref())
            .any(Instance::is_remote)
    }

    /// Ob dieser Platz in einem eigenen Prozess läuft.
    pub fn is_isolated(&self, id: InsertId) -> bool {
        self.slots
            .get(&id.0)
            .and_then(|e| e.instance.as_ref())
            .is_some_and(Instance::is_remote)
    }

    /// Latenz eines Platzes in Abtastwerten.
    pub fn latency(&self, id: InsertId) -> u32 {
        self.slots
            .get(&id.0)
            .and_then(|e| e.instance.as_ref())
            .map_or(0, Instance::latency)
    }

    /// Reglerbewegungen aus den Plugin-Fenstern seit dem letzten Aufruf, als Bearbeitungen je Platz.
    pub fn poll_edits(&mut self) -> Vec<(InsertId, ParamEdit)> {
        let mut out = Vec::new();
        for (id, e) in &mut self.slots {
            let Some(i) = e.instance.as_mut() else {
                continue;
            };
            let mut edits = Vec::new();
            i.poll_feedback(&mut edits);
            for ed in edits {
                if let ParamEdit::Param { id: p, old, .. } = ed {
                    e.origin.entry(p).or_insert(old);
                }
                out.push((InsertId(*id), ed));
            }
        }
        out
    }

    /// Gleicht die laufenden Plugins mit dem Projekt ab, wenn „Rückgängig“ oder „Wiederholen“ Reglerwerte
    /// oder den Zustand eines Platzes geändert hat: Ein Regler, der aus der Liste des Projekts
    /// verschwindet, geht auf seinen früheren Wert zurück; ein anderer gespeicherter Zustand wird
    /// ins Plugin geladen. Gespeicherte Reglerwerte setzt sonst der Mixer.
    pub fn reconcile(&mut self, project: &Project) {
        for t in &project.tracks {
            for slot in &t.plugins {
                let Some(e) = self.slots.get_mut(&slot.id.0) else {
                    continue;
                };
                let Some(inst) = e.instance.as_mut() else {
                    continue;
                };
                if e.seen_state == slot.state && e.seen_params == slot.params {
                    continue;
                }
                if e.seen_state != slot.state {
                    let wanted = slot.state_bytes().or_else(|| e.initial_state.clone());
                    if let Some(bytes) = wanted
                        && inst.save_state().ok().as_deref() != Some(bytes.as_slice())
                    {
                        match inst.load_state(&bytes) {
                            Ok(()) => {
                                // Der Zustand setzt alle Regler; die gespeicherten gelten darüber.
                                for p in &slot.params {
                                    inst.push_param(p.id, p.value);
                                }
                            }
                            Err(err) => {
                                tracing::warn!(plugin = %slot.clap_id, error = %err, "plugin state not restored");
                            }
                        }
                    }
                }
                for old in &e.seen_params {
                    if !slot.params.iter().any(|p| p.id == old.id) {
                        let back = e
                            .origin
                            .remove(&old.id)
                            .or_else(|| e.baseline.get(&old.id).copied());
                        if let Some(v) = back {
                            inst.push_param(old.id, v);
                        }
                    }
                }
                e.seen_params = slot.params.clone();
                e.seen_state = slot.state.clone();
            }
        }
    }

    /// Der Zustand eines Plugins als Befehl, wenn er vom Projekt abweicht; die gespeicherten
    /// Reglerwerte werden dabei verworfen (der Zustand enthält sie alle, auch nach einem Preset).
    pub fn capture_slot(&mut self, project: &Project, id: InsertId) -> Option<Command> {
        let (track, slot) = project
            .tracks
            .iter()
            .find_map(|t| t.plugin(id).map(|s| (t.id, s)))?;
        let e = self.slots.get_mut(&id.0)?;
        let inst = e.instance.as_mut()?;
        let bytes = match inst.save_state() {
            Ok(b) => b,
            Err(e) => {
                tracing::warn!(plugin = %slot.clap_id, error = %e, "plugin state not saved");
                return None;
            }
        };
        let encoded = PluginSlot::encode_state(&bytes);
        // Der Abgleich nach „Rückgängig“ soll diese Übernahme nicht als fremde Änderung sehen.
        e.seen_params.clear();
        e.seen_state = Some(encoded.clone());
        e.origin.clear();
        Some(Command::SetPluginState {
            track,
            plugin: id,
            state: Some(encoded),
            params: Vec::new(),
        })
    }

    /// Befehle, die den Zustand der laufenden Plugins ins Projekt übernehmen (vor Speichern,
    /// Autosave, Export). Nur wo er vom gespeicherten abweicht; die Reglerwerte des Projekts
    /// bleiben stehen und gelten beim Laden nach dem Zustand.
    pub fn capture_state(&mut self, project: &Project) -> Vec<Command> {
        let mut cmds = Vec::new();
        for t in &project.tracks {
            for slot in &t.plugins {
                let Some(inst) = self
                    .slots
                    .get_mut(&slot.id.0)
                    .and_then(|e| e.instance.as_mut())
                else {
                    continue;
                };
                let bytes = match inst.save_state() {
                    Ok(b) => b,
                    Err(e) => {
                        tracing::warn!(plugin = %slot.clap_id, error = %e, "plugin state not saved");
                        continue;
                    }
                };
                if slot.state_bytes().as_deref() == Some(bytes.as_slice()) {
                    continue;
                }
                cmds.push(Command::SetPluginState {
                    track: t.id,
                    plugin: slot.id,
                    state: Some(PluginSlot::encode_state(&bytes)),
                    params: slot.params.clone(),
                });
            }
        }
        cmds
    }
}
