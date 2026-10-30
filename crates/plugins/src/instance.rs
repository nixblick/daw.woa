//! Ein Plugin im Rack: entweder im Programm selbst ([`ClapInstance`]) oder im eigenen Prozess
//! ([`RemoteInstance`], Paket 33). Das Rack sieht nur diese Hülle.

use woa_dsp::plugin::StereoProcessor;

use crate::feedback::ParamEdit;
use crate::host::{ClapInstance, HostError, ParamDesc};
use crate::remote::{Health, RemoteInstance};

/// Ein geladenes Plugin, gleich wo es läuft.
pub enum Instance {
    /// Im Programm selbst.
    Local(Box<ClapInstance>),
    /// Im eigenen Prozess.
    Remote(Box<RemoteInstance>),
}

impl Instance {
    /// Alle Reglerwerte laut Plugin.
    pub fn all_values(&mut self) -> Vec<(u32, f64)> {
        match self {
            Self::Local(i) => i.all_values(),
            Self::Remote(i) => i.all_values(),
        }
    }

    /// Setzt einen Reglerwert.
    pub fn push_param(&mut self, id: u32, value: f64) -> bool {
        match self {
            Self::Local(i) => i.push_param(id, value),
            Self::Remote(i) => i.push_param(id, value),
        }
    }

    /// Ob das Plugin ein eigenes Fenster anbietet.
    pub fn has_gui(&mut self) -> bool {
        match self {
            Self::Local(i) => i.has_gui(),
            Self::Remote(i) => i.has_gui(),
        }
    }

    /// Ob das Fenster offen ist.
    pub fn gui_open(&self) -> bool {
        match self {
            Self::Local(i) => i.gui_open(),
            Self::Remote(i) => i.gui_open(),
        }
    }

    /// Öffnet das Fenster.
    pub fn open_gui(&mut self, title: &str) -> Result<(), HostError> {
        match self {
            Self::Local(i) => i.open_gui(title),
            Self::Remote(i) => i.open_gui(title),
        }
    }

    /// Schließt das Fenster.
    pub fn close_gui(&mut self) {
        match self {
            Self::Local(i) => i.close_gui(),
            Self::Remote(i) => i.close_gui(),
        }
    }

    /// Bedient das Plugin (Zeitgeber, Aufsicht).
    pub fn pump(&mut self) {
        match self {
            Self::Local(i) => i.pump(),
            Self::Remote(i) => i.pump(),
        }
    }

    /// Reglerbewegungen aus dem Plugin-Fenster.
    pub fn poll_feedback(&mut self, out: &mut Vec<ParamEdit>) {
        match self {
            Self::Local(i) => i.poll_feedback(out),
            Self::Remote(i) => i.poll_feedback(out),
        }
    }

    /// Die Regler.
    pub fn params(&self) -> &[ParamDesc] {
        match self {
            Self::Local(i) => i.params(),
            Self::Remote(i) => i.params(),
        }
    }

    /// Ob das Plugin ausgefallen ist.
    pub fn failed(&self) -> bool {
        match self {
            Self::Local(i) => i.failed(),
            Self::Remote(i) => i.failed(),
        }
    }

    /// Warum es ausgefallen ist (nur im eigenen Prozess bekannt).
    pub fn failure(&self) -> Option<String> {
        match self {
            Self::Local(i) => i.failed().then(|| "Das Plugin ist ausgefallen".to_string()),
            Self::Remote(i) => match i.health() {
                Health::Running => None,
                Health::Crashed(r) => Some(r.clone()),
            },
        }
    }

    /// Ein neuer Rechner für den Graphen.
    pub fn processor(&self) -> Box<dyn StereoProcessor> {
        match self {
            Self::Local(i) => i.processor(),
            Self::Remote(i) => i.processor(),
        }
    }

    /// Wert eines Reglers.
    pub fn param_value(&mut self, id: u32) -> Option<f64> {
        match self {
            Self::Local(i) => i.param_value(id),
            Self::Remote(i) => i.param_value(id),
        }
    }

    /// Zustand speichern.
    pub fn save_state(&mut self) -> Result<Vec<u8>, HostError> {
        match self {
            Self::Local(i) => i.save_state(),
            Self::Remote(i) => i.save_state(),
        }
    }

    /// Zustand laden.
    pub fn load_state(&mut self, bytes: &[u8]) -> Result<(), HostError> {
        match self {
            Self::Local(i) => i.load_state(bytes),
            Self::Remote(i) => i.load_state(bytes),
        }
    }

    /// Latenz in Abtastwerten.
    pub fn latency(&self) -> u32 {
        match self {
            Self::Local(i) => i.latency(),
            Self::Remote(i) => i.latency(),
        }
    }

    /// Startet ein ausgefallenes Plugin im eigenen Prozess neu. Ein Plugin im Programm selbst
    /// lässt sich nicht neu starten (`Err`).
    pub fn reload(&mut self, state: Option<&[u8]>) -> Result<(), HostError> {
        match self {
            Self::Local(_) => Err(HostError::Load(
                "ohne Schutzprozess nicht neu ladbar".into(),
            )),
            Self::Remote(i) => i.reload(state),
        }
    }

    /// Lässt den Plugin-Prozess abstürzen (Absturztest). Nur im eigenen Prozess.
    pub fn simulate_crash(&mut self) {
        if let Self::Remote(i) = self {
            i.simulate_crash();
        }
    }

    /// Ob das Plugin im eigenen Prozess läuft.
    pub fn is_remote(&self) -> bool {
        matches!(self, Self::Remote(_))
    }
}
