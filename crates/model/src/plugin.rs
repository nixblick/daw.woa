//! Fremd-Plugins auf einer Spur (Paket 31): welches Plugin, in welcher Rolle, mit welchem
//! gespeicherten Zustand. Das Modell kennt nur die CLAP-Kennung, nie einen Dateipfad: Ein
//! Projekt aus fremder Hand darf keine Bibliothek zum Laden vorgeben. Welche Datei zu einer
//! Kennung gehört, steht im Plugin-Verzeichnis des Rechners (`woa-plugins`).

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use serde::{Deserialize, Serialize};

use crate::effect::InsertId;
use crate::error::{ModelError, Result};

/// Längste erlaubte CLAP-Kennung oder Anzeigename.
pub const MAX_PLUGIN_TEXT: usize = 256;
/// Größter gespeicherter Plugin-Zustand in Bytes (vor der Base64-Umwandlung).
pub const MAX_PLUGIN_STATE_BYTES: usize = 32 * 1024 * 1024;
/// Höchste Zahl gespeicherter Reglerwerte je Plugin.
pub const MAX_PLUGIN_PARAMS: usize = 4096;

/// Rolle eines Plugins auf einer Spur.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginRole {
    /// Klangerzeuger einer MIDI-Spur, ersetzt den eingebauten Synth.
    Instrument,
    /// Effekt in der Kette einer Audio- oder MIDI-Spur.
    Effect,
}

/// Ein Reglerwert, den der Nutzer gesetzt hat, mit der Kennung des Plugins.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PluginParam {
    /// CLAP-Parameterkennung (nicht die Position in der Liste).
    pub id: u32,
    /// Wert im Bereich des Plugins.
    pub value: f64,
}

/// Ein Plugin auf einer Spur.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PluginSlot {
    /// Kennung des Eintrags; teilt sich den Zahlenraum mit Spuren, Clips und Inserts.
    pub id: InsertId,
    /// CLAP-Kennung des Plugins, z. B. `org.surge-synth-team.surge-xt`.
    pub clap_id: String,
    /// Name zur Anzeige, auch wenn das Plugin auf diesem Rechner fehlt.
    pub name: String,
    /// Instrument oder Effekt.
    pub role: PluginRole,
    /// Umgangen: ein Effekt lässt das Signal unverändert durch.
    #[serde(default, skip_serializing_if = "is_false")]
    pub bypass: bool,
    /// Reglerwerte, die nach dem Laden des Zustands gelten (zuletzt vom Nutzer gesetzt).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub params: Vec<PluginParam>,
    /// Zustand des Plugins (CLAP-State-Erweiterung), Base64. `None`: Grundzustand.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_false(v: &bool) -> bool {
    !*v
}

impl PluginSlot {
    /// Neuer Eintrag im Grundzustand.
    pub fn new(
        id: InsertId,
        clap_id: impl Into<String>,
        name: impl Into<String>,
        role: PluginRole,
    ) -> Self {
        Self {
            id,
            clap_id: clap_id.into(),
            name: name.into(),
            role,
            bypass: false,
            params: Vec::new(),
            state: None,
        }
    }

    /// Der gespeicherte Zustand als Bytes; `None` ohne Zustand oder bei kaputtem Base64.
    pub fn state_bytes(&self) -> Option<Vec<u8>> {
        STANDARD.decode(self.state.as_deref()?).ok()
    }

    /// Kodiert Zustandsbytes für das Projekt (Base64).
    pub fn encode_state(bytes: &[u8]) -> String {
        STANDARD.encode(bytes)
    }

    /// Prüft Längen und Werte; ein kaputter Eintrag lässt das Projekt nicht laden.
    pub(crate) fn check(&self) -> Result<()> {
        let text_ok = |s: &str| !s.trim().is_empty() && s.len() <= MAX_PLUGIN_TEXT;
        if !text_ok(&self.clap_id) || !text_ok(&self.name) {
            return Err(ModelError::InvalidArgument(
                "plugin id and name must be 1..=256 bytes".into(),
            ));
        }
        if self.params.len() > MAX_PLUGIN_PARAMS {
            return Err(ModelError::InvalidArgument(format!(
                "a plugin takes at most {MAX_PLUGIN_PARAMS} stored parameters"
            )));
        }
        for p in &self.params {
            if !p.value.is_finite() {
                return Err(ModelError::InvalidValue {
                    field: "plugin parameter",
                    value: p.value,
                });
            }
        }
        // Base64 hat 4 Zeichen je 3 Bytes; so wird nichts Riesiges erst dekodiert.
        if let Some(s) = &self.state {
            if s.len() > MAX_PLUGIN_STATE_BYTES / 3 * 4 + 4 {
                return Err(ModelError::InvalidArgument(
                    "plugin state is too large".into(),
                ));
            }
            if STANDARD.decode(s).is_err() {
                return Err(ModelError::InvalidArgument(
                    "plugin state is not valid base64".into(),
                ));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slot() -> PluginSlot {
        PluginSlot::new(
            InsertId(7),
            "org.example.synth",
            "Synth",
            PluginRole::Instrument,
        )
    }

    #[test]
    fn state_roundtrips_through_base64() {
        let mut s = slot();
        let bytes: Vec<u8> = (0..=255).collect();
        s.state = Some(PluginSlot::encode_state(&bytes));
        assert!(s.check().is_ok());
        assert_eq!(s.state_bytes().as_deref(), Some(&bytes[..]));
    }

    #[test]
    fn bad_entries_are_rejected() {
        let mut s = slot();
        s.clap_id.clear();
        assert!(s.check().is_err());
        let mut s = slot();
        s.state = Some("***".into());
        assert!(s.check().is_err());
        let mut s = slot();
        s.params.push(PluginParam {
            id: 1,
            value: f64::NAN,
        });
        assert!(s.check().is_err());
    }
}
