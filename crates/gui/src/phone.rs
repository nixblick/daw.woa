//! Telefon: Eingang für den Anrufer (Gerät und Kanalpaar).
//!
//! Der Anrufer kommt über ein Handy am Pult. Der Eingang gilt nur, wenn er gewählt ist (nie
//! das Standardmikrofon). Gewählt wird er im Fenster „Audio-Geräte“ unter Eingänge, der
//! Rückweg zum Anrufer dort bei den Ausgängen (Quelle „Telefon-Rückweg“).

use serde::{Deserialize, Serialize};
use woa_engine::record::InputConfig;

/// Gewählter Telefon-Eingang, gespeichert in `config.json`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PhoneInput {
    /// Kennung des Geräts, `None` = kein Telefon-Eingang.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_id: Option<String>,
    /// Name bei der Wahl, für verständliche Meldungen, wenn das Gerät fehlt.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub device_name: String,
    /// Kanalpaar: 0 = Kanal 1/2, 1 = Kanal 3/4 …
    #[serde(skip_serializing_if = "is_zero")]
    pub pair: u8,
}

fn is_zero(v: &u8) -> bool {
    *v == 0
}

impl PhoneInput {
    pub fn is_default(&self) -> bool {
        *self == PhoneInput::default()
    }

    /// Name für Meldungen.
    pub fn label(&self) -> String {
        if self.device_name.is_empty() {
            self.device_id.clone().unwrap_or_default()
        } else {
            self.device_name.clone()
        }
    }

    /// Wie `start_pair_input` das Gerät öffnet; `None` = kein Eingang gewählt.
    pub fn input_config(&self) -> Option<InputConfig> {
        self.device_id.as_ref().map(|id| InputConfig {
            device_id: Some(id.clone()),
            pair: self.pair,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_off_and_not_saved() {
        let p = PhoneInput::default();
        assert!(p.is_default() && p.input_config().is_none());
        assert_eq!(serde_json::to_string(&p).unwrap(), "{}");
    }

    #[test]
    fn choice_round_trips_with_the_pair() {
        let p = PhoneInput {
            device_id: Some("karte-1".into()),
            device_name: "USB Audio".into(),
            pair: 1,
        };
        let back: PhoneInput = serde_json::from_str(&serde_json::to_string(&p).unwrap()).unwrap();
        assert_eq!(back, p);
        let cfg = p.input_config().unwrap();
        assert_eq!((cfg.device_id.as_deref(), cfg.pair), (Some("karte-1"), 1));
    }
}
