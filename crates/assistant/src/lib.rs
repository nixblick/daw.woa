//! KI- und MCP-Anbindung von WOA.
//!
//! - [`server`]: WOA als MCP-Server für beliebige Clients (Einstellungen, Zugriffstoken,
//!   Konfigurations-Schnipsel, Hintergrundprozess).
//! - [`llm`]: Anbieter für den eigenen Assistenten (Anthropic und OpenAI-kompatibel).
//! - [`mcp_client`]: Client für beliebige externe MCP-Server als Werkzeuge des Assistenten.
//! - [`chat`]: Gespräch mit Werkzeug-Schleife, im Hintergrund-Thread.
//!
//! Geheimnisse (Zugriffstoken, API-Schlüssel, Zugangsdaten externer Server) stehen nie in
//! der Konfiguration: sie liegen über [`woa_secrets`] im Schlüsselbund, die Konfiguration
//! kennt nur die Kennung.

pub mod chat;
pub mod llm;
pub mod mcp_client;
pub mod server;
#[cfg(test)]
mod testutil;

use std::path::Path;

use serde::{Deserialize, Serialize};

/// Ein Sprachmodell-Anbieter. Der Schlüssel liegt unter [`ProviderConfig::secret_account`].
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProviderConfig {
    pub id: String,
    pub name: String,
    pub kind: llm::Kind,
    pub base_url: String,
    /// Freier Modellname.
    pub model: String,
}

impl ProviderConfig {
    pub fn secret_account(&self) -> String {
        format!("ki-{}", self.id)
    }

    /// Voreinstellungen zum Antippen; alles bleibt änderbar.
    pub fn presets() -> [(&'static str, llm::Kind, &'static str); 5] {
        [
            (
                "Anthropic",
                llm::Kind::Anthropic,
                "https://api.anthropic.com",
            ),
            (
                "OpenAI",
                llm::Kind::OpenAiCompat,
                "https://api.openai.com/v1",
            ),
            (
                "OpenRouter",
                llm::Kind::OpenAiCompat,
                "https://openrouter.ai/api/v1",
            ),
            (
                "Ollama (lokal)",
                llm::Kind::OpenAiCompat,
                "http://localhost:11434/v1",
            ),
            (
                "LM Studio (lokal)",
                llm::Kind::OpenAiCompat,
                "http://localhost:1234/v1",
            ),
        ]
    }

    /// Der Anbieter mit Schlüssel aus dem Schlüsselbund (falls einer hinterlegt ist).
    pub fn provider(&self, data_dir: &Path) -> llm::Provider {
        llm::Provider::new(
            self.kind,
            self.base_url.clone(),
            self.model.clone(),
            woa_secrets::reveal(data_dir, &self.secret_account()),
        )
    }
}

/// Wie ein externer MCP-Server erreicht wird.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "transport", rename_all = "snake_case")]
pub enum Endpoint {
    Stdio {
        command: String,
        args: Vec<String>,
        /// Umgebungsvariablen ohne Geheimnis.
        env: Vec<(String, String)>,
    },
    Http {
        url: String,
        /// Kopfzeilen ohne Geheimnis.
        headers: Vec<(String, String)>,
    },
}

impl Default for Endpoint {
    fn default() -> Self {
        Self::Stdio {
            command: String::new(),
            args: Vec::new(),
            env: Vec::new(),
        }
    }
}

/// Ein externer MCP-Server, dessen Werkzeuge der Assistent nutzt.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ExternalServer {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub endpoint: Endpoint,
    /// Name der geheimen Kopfzeile (HTTP, z. B. `Authorization`) oder Umgebungsvariable
    /// (Stdio). Der Wert liegt unter [`ExternalServer::secret_account`]; leer = keiner.
    pub secret_name: String,
}

impl Default for ExternalServer {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            enabled: true,
            endpoint: Endpoint::default(),
            secret_name: String::new(),
        }
    }
}

impl ExternalServer {
    pub fn secret_account(&self) -> String {
        format!("mcp-{}", self.id)
    }

    /// Die Verbindungsangaben mit eingesetztem Geheimnis.
    pub fn spec(&self, data_dir: &Path) -> mcp_client::Spec {
        let secret = if self.secret_name.trim().is_empty() {
            None
        } else {
            woa_secrets::reveal(data_dir, &self.secret_account())
                .map(|v| (self.secret_name.trim().to_string(), v))
        };
        match &self.endpoint {
            Endpoint::Stdio { command, args, env } => {
                let mut env = env.clone();
                env.extend(secret);
                mcp_client::Spec::Stdio {
                    command: command.clone(),
                    args: args.clone(),
                    env,
                }
            }
            Endpoint::Http { url, headers } => {
                let mut headers = headers.clone();
                headers.extend(secret);
                mcp_client::Spec::Http {
                    url: url.clone(),
                    headers,
                }
            }
        }
    }
}

/// Eine neue, kurze Kennung (`[a-z0-9]`, taugt als Teil eines Kontonamens).
pub fn new_id() -> String {
    let mut b = [0u8; 4];
    if getrandom::fill(&mut b).is_err() {
        b = [7, 7, 7, 7];
    }
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Zerlegt eine Zeile in Argumente: getrennt durch Leerraum, Anführungszeichen (`"`) halten
/// Teile mit Leerzeichen zusammen. Für das Eingabefeld „Argumente“.
pub fn split_args(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut any = false;
    for c in s.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                any = true;
            }
            c if c.is_whitespace() && !quoted => {
                if any {
                    out.push(std::mem::take(&mut cur));
                    any = false;
                }
            }
            c => {
                cur.push(c);
                any = true;
            }
        }
    }
    if any {
        out.push(cur);
    }
    out
}

/// Gegenstück zu [`split_args`].
pub fn join_args(args: &[String]) -> String {
    args.iter()
        .map(|a| {
            if a.is_empty() || a.contains(char::is_whitespace) {
                format!("\"{a}\"")
            } else {
                a.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Zeilen `Name<sep>Wert` (Leerzeilen weg) für Umgebungsvariablen (`=`) und Kopfzeilen (`:`).
pub fn parse_pairs(s: &str, sep: char) -> Vec<(String, String)> {
    s.lines()
        .filter_map(|l| {
            let (k, v) = l.split_once(sep)?;
            let k = k.trim();
            (!k.is_empty()).then(|| (k.to_string(), v.trim().to_string()))
        })
        .collect()
}

/// Gegenstück zu [`parse_pairs`].
pub fn join_pairs(pairs: &[(String, String)], sep: char) -> String {
    pairs
        .iter()
        .map(|(k, v)| format!("{k}{sep}{v}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn yes() -> bool {
    true
}

/// Alles, was `config.json` zu „KI & MCP“ hält. Ohne Geheimnisse.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct KiConfig {
    /// WOA als MCP-Server.
    pub mcp_server: server::McpServerConfig,
    /// Anbieter für den eigenen Assistenten.
    pub providers: Vec<ProviderConfig>,
    /// Kennung des gewählten Anbieters.
    pub active: Option<String>,
    /// Externe MCP-Server als Werkzeuge.
    pub servers: Vec<ExternalServer>,
    /// WOAs eigene Werkzeuge (Spuren, Clips, Mischen …) dem Assistenten geben.
    #[serde(default = "yes")]
    pub woa_tools: bool,
    /// Auf der Seite „KI“ gewählt: ein MCP-Client (Claude Desktop/Code) statt eines
    /// API-Anbieters für den Chat in WOA. `None`: der aktive Anbieter gilt.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mcp_client: Option<McpClient>,
}

/// Ein MCP-Client, der WOA von außen bedient.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum McpClient {
    ClaudeDesktop,
    ClaudeCode,
    Other,
}

impl McpClient {
    pub const ALL: [Self; 3] = [Self::ClaudeDesktop, Self::ClaudeCode, Self::Other];

    pub fn label(self) -> &'static str {
        match self {
            Self::ClaudeDesktop => "Claude Desktop",
            Self::ClaudeCode => "Claude Code",
            Self::Other => "anderer MCP-Client",
        }
    }

    /// Name des passenden Schnipsels in [`server::snippets`] (Stdio).
    pub fn snippet_client(self) -> &'static str {
        match self {
            Self::ClaudeDesktop => "Claude Desktop",
            Self::ClaudeCode => "Claude Code",
            Self::Other => "Anderer Client (Stdio)",
        }
    }
}

/// Eine Wahl im Dropdown „Anbieter“ der Seite „KI“.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pick {
    /// Claude Desktop / Claude Code bedienen WOA über MCP.
    Mcp(McpClient),
    /// Eine Voreinstellung aus [`ProviderConfig::presets`] (Index).
    Preset(usize),
}

impl KiConfig {
    /// Was im Dropdown „Anbieter“ gerade gilt.
    pub fn pick(&self) -> Option<Pick> {
        if let Some(c) = self.mcp_client {
            return Some(Pick::Mcp(c));
        }
        let p = self.active_provider()?;
        ProviderConfig::presets()
            .iter()
            .position(|(_, k, url)| *k == p.kind && *url == p.base_url)
            .map(Pick::Preset)
    }

    /// Übernimmt eine Wahl aus dem Dropdown. Eine Voreinstellung nimmt einen schon
    /// angelegten Anbieter mit gleicher Adresse wieder, sonst legt sie einen an.
    pub fn choose(&mut self, pick: Pick) {
        match pick {
            Pick::Mcp(c) => self.mcp_client = Some(c),
            Pick::Preset(i) => {
                self.mcp_client = None;
                let Some((name, kind, url)) = ProviderConfig::presets().get(i).copied() else {
                    return;
                };
                if let Some(p) = self
                    .providers
                    .iter()
                    .find(|p| p.kind == kind && p.base_url == url)
                {
                    self.active = Some(p.id.clone());
                    return;
                }
                let id = new_id();
                self.providers.push(ProviderConfig {
                    id: id.clone(),
                    name: name.to_string(),
                    kind,
                    base_url: url.to_string(),
                    model: String::new(),
                });
                self.active = Some(id);
            }
        }
    }
}

impl Default for KiConfig {
    fn default() -> Self {
        Self {
            mcp_server: server::McpServerConfig::default(),
            providers: Vec::new(),
            active: None,
            servers: Vec::new(),
            woa_tools: true,
            mcp_client: None,
        }
    }
}

impl KiConfig {
    /// Nichts verstellt? Dann schreibt `config.json` den Abschnitt nicht.
    pub fn is_default(&self) -> bool {
        self == &Self::default()
    }

    /// Der gewählte Anbieter (oder der erste, wenn die Wahl fehlt).
    pub fn active_provider(&self) -> Option<&ProviderConfig> {
        self.active
            .as_ref()
            .and_then(|id| self.providers.iter().find(|p| &p.id == id))
            .or_else(|| self.providers.first())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_not_written_and_old_config_loads() {
        assert!(KiConfig::default().is_default());
        let old: KiConfig = serde_json::from_str(r#"{"mcp_server":{"enabled":true}}"#).unwrap();
        assert!(old.mcp_server.enabled);
        assert!(old.woa_tools && old.providers.is_empty());
        assert!(old.mcp_client.is_none());
    }

    #[test]
    fn config_round_trip_has_no_secret_values() {
        let mut c = KiConfig::default();
        c.providers.push(ProviderConfig {
            id: "ab12".into(),
            name: "Mein Server".into(),
            base_url: "http://localhost:11434/v1".into(),
            model: "llama3".into(),
            ..Default::default()
        });
        c.servers.push(ExternalServer {
            id: "cd34".into(),
            name: "Wetter".into(),
            endpoint: Endpoint::Http {
                url: "https://example.org/mcp".into(),
                headers: vec![],
            },
            secret_name: "Authorization".into(),
            ..Default::default()
        });
        let json = serde_json::to_string(&c).unwrap();
        let back: KiConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(back, c);
        assert_eq!(c.providers[0].secret_account(), "ki-ab12");
        assert_eq!(c.servers[0].secret_account(), "mcp-cd34");
        assert!(woa_secrets::valid_account(&c.servers[0].secret_account()));
        assert_eq!(c.active_provider().unwrap().id, "ab12");
    }

    #[test]
    fn secret_goes_into_the_spec_not_the_config() {
        let dir = tempfile::tempdir().unwrap();
        let s = ExternalServer {
            id: "ff00".into(),
            endpoint: Endpoint::Http {
                url: "http://x".into(),
                headers: vec![("X-A".into(), "1".into())],
            },
            secret_name: "Authorization".into(),
            ..Default::default()
        };
        let none = s.spec(dir.path());
        assert!(matches!(&none, mcp_client::Spec::Http { headers, .. } if headers.len() == 1));
        // Wert über die Ersatzdatei (kein Zugriff auf den Schlüsselbund im Test).
        std::fs::write(
            woa_secrets::fallback_path(dir.path(), "mcp-ff00"),
            "Bearer platzhalter\n",
        )
        .unwrap();
        if let mcp_client::Spec::Http { headers, .. } = s.spec(dir.path()) {
            assert_eq!(headers.len(), 2);
            assert_eq!(
                headers[1],
                ("Authorization".into(), "Bearer platzhalter".into())
            );
        }
    }

    #[test]
    fn args_and_pairs_round_trip() {
        let a = split_args(r#"-y  @scope/server "C:\Mein Ordner" """#);
        assert_eq!(a, vec!["-y", "@scope/server", "C:\\Mein Ordner", ""]);
        assert_eq!(split_args(&join_args(&a)), a);
        assert!(split_args("   ").is_empty());
        let p = parse_pairs("A=1\n\nB = x=y\nohne", '=');
        assert_eq!(
            p,
            vec![("A".into(), "1".into()), ("B".into(), "x=y".into())]
        );
        assert_eq!(parse_pairs(&join_pairs(&p, '='), '='), p);
    }

    #[test]
    fn ids_are_valid_account_parts() {
        let id = new_id();
        assert_eq!(id.len(), 8);
        assert!(woa_secrets::valid_account(&format!("ki-{id}")));
    }

    #[test]
    fn dropdown_pick_is_stored_and_survives_a_round_trip() {
        let mut c = KiConfig::default();
        assert_eq!(c.pick(), None);
        c.choose(Pick::Mcp(McpClient::ClaudeDesktop));
        assert_eq!(c.pick(), Some(Pick::Mcp(McpClient::ClaudeDesktop)));
        assert!(!c.is_default());
        let back: KiConfig = serde_json::from_str(&serde_json::to_string(&c).unwrap()).unwrap();
        assert_eq!(back, c);
        // Ollama ist Voreinstellung 3; zweimal wählen legt nur einen Anbieter an.
        c.choose(Pick::Preset(3));
        c.choose(Pick::Preset(0));
        c.choose(Pick::Preset(3));
        assert_eq!(c.pick(), Some(Pick::Preset(3)));
        assert_eq!(c.providers.len(), 2);
        assert!(c.mcp_client.is_none());
        let back: KiConfig = serde_json::from_str(&serde_json::to_string(&c).unwrap()).unwrap();
        assert_eq!(back.pick(), Some(Pick::Preset(3)));
    }

    #[test]
    fn provider_debug_never_shows_the_key() {
        let p = llm::Provider::new(
            llm::Kind::Anthropic,
            "https://api.anthropic.com".to_string(),
            "m".to_string(),
            Some("sk-ant-geheim-123".to_string()),
        );
        assert!(!format!("{p:?}").contains("geheim"));
    }
}
