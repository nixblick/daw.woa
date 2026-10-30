//! Das Plugin-Rack: hält die geladenen Plugins eines Projekts (ein [`ClapInstance`] je Platz),
//! gibt der Engine ihre Rechner und nimmt den Zustand der laufenden Plugins zurück ins Projekt.
//!
//! Das Rack lebt im Control-Thread. Das Projekt kennt nur die CLAP-Kennung; fehlt das Plugin
//! auf diesem Rechner, bleibt der Platz im Projekt, die Spur klingt nicht (Instrument) oder
//! reicht durch (Effekt), und der Platz meldet [`SlotStatus::Missing`].

use std::collections::HashMap;
use std::sync::Arc;

use woa_dsp::plugin::{PluginEffect, TrackPlugins};
use woa_model::{Command, InsertId, PluginRole, PluginSlot, Project, Track};

use crate::host::{ClapInstance, HostError, ParamDesc};
use crate::scan::Catalog;

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
    instance: Option<ClapInstance>,
}

/// Geladene Plugins eines Projekts.
pub struct PluginRack {
    catalog: Arc<Catalog>,
    rate: f32,
    slots: HashMap<u64, Entry>,
}

impl PluginRack {
    /// Neues, leeres Rack für diese Abtastrate.
    pub fn new(catalog: Arc<Catalog>, rate: f32) -> Self {
        Self {
            catalog,
            rate,
            slots: HashMap::new(),
        }
    }

    /// Die Plugin-Liste dieses Rechners.
    pub fn catalog(&self) -> &Catalog {
        &self.catalog
    }

    /// Ein zweites, leeres Rack mit derselben Plugin-Liste und Rate. Der Export rechnet damit
    /// eigene Instanzen, damit er die laufende Wiedergabe nicht stört.
    pub fn fresh(&self) -> Self {
        Self::new(Arc::clone(&self.catalog), self.rate)
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
        };
        match ClapInstance::new(&self.catalog, &slot.clap_id, self.rate) {
            Ok(mut inst) => {
                if let Some(bytes) = slot.state_bytes()
                    && let Err(e) = inst.load_state(&bytes)
                {
                    tracing::warn!(plugin = %slot.clap_id, error = %e, "plugin state not applied");
                }
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

    /// Status eines Platzes (`None`: das Rack kennt ihn nicht; erst [`PluginRack::sync`]).
    pub fn status(&self, id: InsertId) -> Option<SlotStatus> {
        let e = self.slots.get(&id.0)?;
        if let Some(i) = &e.instance
            && i.failed()
        {
            return Some(SlotStatus::Failed("Das Plugin ist ausgefallen".to_string()));
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
            if inst.failed() {
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
