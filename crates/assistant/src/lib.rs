//! KI- und MCP-Anbindung von WOA.
//!
//! - [`server`]: WOA als MCP-Server für beliebige Clients (Einstellungen, Zugriffstoken,
//!   Konfigurations-Schnipsel, Hintergrundprozess).
//!
//! Geheimnisse (Zugriffstoken, API-Schlüssel) stehen nie in der Konfiguration: sie liegen
//! über [`woa_secrets`] im Schlüsselbund, die Konfiguration kennt nur die Kennung.

pub mod server;

use serde::{Deserialize, Serialize};

/// Alles, was `config.json` zu „KI & MCP“ hält. Ohne Geheimnisse.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct KiConfig {
    /// WOA als MCP-Server.
    pub mcp_server: server::McpServerConfig,
}

impl KiConfig {
    /// Nichts verstellt? Dann schreibt `config.json` den Abschnitt nicht.
    pub fn is_default(&self) -> bool {
        self == &Self::default()
    }
}
