//! WOA als MCP-Server: Einstellungen, Zugriffstoken, Konfigurations-Schnipsel für Clients
//! und der Hintergrundprozess `woa-headless mcp … --http`.

use std::net::{IpAddr, Ipv4Addr};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use serde::{Deserialize, Serialize};

/// Kennung des Zugriffstokens im Schlüsselbund.
pub const TOKEN_ACCOUNT: &str = "mcp-token";
/// Vorgabe-Port.
pub const DEFAULT_PORT: u16 = 8765;
/// Pfad des Servers, wie ihn die Clients eintragen.
pub const MCP_PATH: &str = "/mcp";

/// Wer den Server erreichen darf.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reach {
    /// Nur diese Maschine (`127.0.0.1`). Vorgabe.
    #[default]
    Local,
    /// Auch andere Rechner im Netz (`0.0.0.0`), weiterhin nur mit Token.
    Network,
}

/// Wie ein Client WOA erreicht.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Transport {
    /// Der Client startet `woa-headless mcp <Projekt>` selbst und spricht über stdin/stdout.
    #[default]
    Stdio,
    /// WOA läuft im Hintergrund und antwortet über HTTP (Streamable HTTP) mit Token.
    Http,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct McpServerConfig {
    /// Der HTTP-Server läuft im Hintergrund, solange WOA offen ist.
    pub enabled: bool,
    /// Welche Art von Schnipseln zuerst steht.
    pub transport: Transport,
    pub reach: Reach,
    pub port: u16,
    /// Projektordner, auf dem der MCP-Server arbeitet. Leer = keiner gewählt.
    pub project: Option<PathBuf>,
}

impl Default for McpServerConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            transport: Transport::Stdio,
            reach: Reach::Local,
            port: DEFAULT_PORT,
            project: None,
        }
    }
}

impl McpServerConfig {
    /// Adresse, auf der der Server hört.
    pub fn bind_ip(&self) -> IpAddr {
        match self.reach {
            Reach::Local => IpAddr::V4(Ipv4Addr::LOCALHOST),
            Reach::Network => IpAddr::V4(Ipv4Addr::UNSPECIFIED),
        }
    }

    /// Prüft die Eingaben; die Meldung sagt, was zu ändern ist.
    pub fn check(&self) -> Result<(), String> {
        if self.port < 1024 {
            return Err("Port unter 1024 braucht Administratorrechte, bitte 1024 bis 65535".into());
        }
        Ok(())
    }
}

/// Neues Zugriffstoken: 256 Bit aus dem Zufallsgenerator des Systems, hexadezimal.
pub fn new_token() -> Result<String, String> {
    let mut buf = [0u8; 32];
    getrandom::fill(&mut buf).map_err(|e| format!("Zufallsgenerator nicht verfügbar: {e}"))?;
    Ok(buf.iter().map(|b| format!("{b:02x}")).collect())
}

/// Maske für die Anzeige.
pub const MASK: &str = "••••••••";

/// Ein fertiger Konfigurations-Schnipsel für einen Client.
#[derive(Debug, Clone, PartialEq)]
pub struct Snippet {
    /// Client, z. B. „Claude Code“.
    pub client: &'static str,
    /// Wohin der Schnipsel gehört („Terminal“, „claude_desktop_config.json“ …).
    pub target: &'static str,
    pub transport: Transport,
    /// Text mit dem Platzhalter [`TOKEN_SLOT`] statt des Tokens.
    template: String,
}

/// Stelle im Schnipsel, an die das Token kommt.
const TOKEN_SLOT: &str = "@@WOA_TOKEN@@";

impl Snippet {
    /// Zum Anzeigen: das Token ist maskiert.
    pub fn masked(&self) -> String {
        self.template.replace(TOKEN_SLOT, MASK)
    }

    /// Zum Kopieren: mit dem echten Token.
    pub fn with_token(&self, token: &str) -> String {
        self.template.replace(TOKEN_SLOT, token)
    }

    /// Braucht dieser Schnipsel ein Token?
    pub fn uses_token(&self) -> bool {
        self.template.contains(TOKEN_SLOT)
    }
}

/// Eine Kommandozeilen-Angabe für das Terminal dieses Systems quoten (Windows: PowerShell,
/// sonst bash/zsh). Einfache Anführungszeichen, damit beim Einfügen nichts ausgewertet wird
/// (`$(…)`, Backticks, `$VAR`). `None` bei Zeilenumbruch: der bräche den Befehl beim Einfügen.
fn quote(s: &str) -> Option<String> {
    if cfg!(windows) {
        quote_powershell(s)
    } else {
        quote_posix(s)
    }
}

fn is_plain(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"/._-:=@+".contains(&b))
}

/// bash/zsh: `'…'`, ein `'` wird zu `'\''`.
fn quote_posix(s: &str) -> Option<String> {
    if s.contains(['\n', '\r']) {
        return None;
    }
    Some(if is_plain(s) {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    })
}

/// PowerShell: `'…'` ist wörtlich, ein `'` (auch typografisch) wird verdoppelt.
fn quote_powershell(s: &str) -> Option<String> {
    if s.contains(['\n', '\r']) {
        return None;
    }
    Some(if is_plain(s) {
        s.to_string()
    } else {
        let mut out = String::from("'");
        for c in s.chars() {
            if matches!(c, '\'' | '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{201B}') {
                out.push(c);
            }
            out.push(c);
        }
        out.push('\'');
        out
    })
}

fn pretty(v: &serde_json::Value) -> String {
    serde_json::to_string_pretty(v).unwrap_or_default()
}

/// Adresse, die ein Client eintragen soll.
pub fn url(cfg: &McpServerConfig, host_for_network: &str) -> String {
    let host = match cfg.reach {
        Reach::Local => "127.0.0.1".to_string(),
        Reach::Network => host_for_network.to_string(),
    };
    format!("http://{host}:{}{MCP_PATH}", cfg.port)
}

/// Alle Schnipsel für `cfg`. `exe` ist der Pfad zu `woa-headless` (`None`: nicht gefunden,
/// dann gibt es keine Stdio-Schnipsel), `network_host` der Name oder die IP dieses Rechners
/// für Clients im Netz. Die Schnipsel entstehen zur Laufzeit; kein Pfad steht fest im Code.
pub fn snippets(cfg: &McpServerConfig, exe: Option<&Path>, network_host: &str) -> Vec<Snippet> {
    let mut out = Vec::new();
    let project = cfg.project.as_ref().map_or_else(
        || "<Projektordner>".to_string(),
        |p| p.display().to_string(),
    );
    let u = url(cfg, network_host);
    if let Some(exe) = exe {
        let exe = exe.display().to_string();
        // Pfad mit Zeilenumbruch: kein Terminal-Befehl, nur die JSON-Einträge.
        if let (Some(e), Some(p)) = (quote(&exe), quote(&project)) {
            out.push(Snippet {
                client: "Claude Code",
                target: "Terminal",
                transport: Transport::Stdio,
                template: format!("claude mcp add woa -- {e} mcp {p}"),
            });
        }
        out.push(Snippet {
            client: "Claude Desktop",
            target: "claude_desktop_config.json",
            transport: Transport::Stdio,
            template: pretty(&serde_json::json!({
                "mcpServers": { "woa": { "command": exe, "args": ["mcp", project] } }
            })),
        });
        out.push(Snippet {
            client: "Anderer Client (Stdio)",
            target: "MCP-Einstellungen des Clients",
            transport: Transport::Stdio,
            template: pretty(&serde_json::json!({
                "mcpServers": { "woa": { "type": "stdio", "command": exe, "args": ["mcp", project] } }
            })),
        });
    }
    out.push(Snippet {
        client: "Claude Code",
        target: "Terminal",
        transport: Transport::Http,
        template: format!(
            "claude mcp add --transport http woa {} --header \"Authorization: Bearer {TOKEN_SLOT}\"",
            quote(&u).unwrap_or_default()
        ),
    });
    out.push(Snippet {
        client: "Claude Desktop",
        target: "claude_desktop_config.json (über den Vermittler mcp-remote, braucht Node.js)",
        transport: Transport::Http,
        template: pretty(&serde_json::json!({
            "mcpServers": { "woa": {
                "command": "npx",
                "args": ["-y", "mcp-remote", u, "--header", "Authorization:${WOA_AUTH}"],
                "env": { "WOA_AUTH": format!("Bearer {TOKEN_SLOT}") }
            } }
        })),
    });
    out.push(Snippet {
        client: "Anderer Client (HTTP)",
        target: "MCP-Einstellungen des Clients",
        transport: Transport::Http,
        template: pretty(&serde_json::json!({
            "mcpServers": { "woa": {
                "type": "http",
                "url": u,
                "headers": { "Authorization": format!("Bearer {TOKEN_SLOT}") }
            } }
        })),
    });
    out
}

/// `woa-headless` neben dem laufenden Programm.
pub fn headless_exe() -> Option<PathBuf> {
    let me = std::env::current_exe().ok()?;
    sibling_headless(me.parent()?)
}

fn sibling_headless(dir: &Path) -> Option<PathBuf> {
    let name = if cfg!(windows) {
        "woa-headless.exe"
    } else {
        "woa-headless"
    };
    let p = dir.join(name);
    p.is_file().then_some(p)
}

/// Der Hintergrundprozess `woa-headless mcp <Projekt> --http`. Endet mit der Oberfläche.
pub struct ServerProcess {
    child: Child,
    pub addr: String,
}

impl std::fmt::Debug for ServerProcess {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServerProcess")
            .field("addr", &self.addr)
            .finish()
    }
}

impl ServerProcess {
    /// Startet den Server. Das Token liest der Prozess selbst aus dem Schlüsselbund; es
    /// geht weder über die Kommandozeile noch über die Umgebung.
    pub fn start(exe: &Path, cfg: &McpServerConfig) -> Result<Self, String> {
        cfg.check()?;
        let project = cfg.project.as_deref().ok_or("Kein Projektordner gewählt")?;
        let ip = cfg.bind_ip();
        let child = Command::new(exe)
            .arg("mcp")
            .arg(project)
            .arg("--http")
            .arg("--bind")
            .arg(ip.to_string())
            .arg("--port")
            .arg(cfg.port.to_string())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("{} lässt sich nicht starten: {e}", exe.display()))?;
        Ok(Self {
            child,
            addr: format!("{ip}:{}", cfg.port),
        })
    }

    /// Läuft der Prozess noch? `Err`: er ist beendet, mit dem Grund.
    pub fn poll(&mut self) -> Result<(), String> {
        match self.child.try_wait() {
            Ok(None) => Ok(()),
            Ok(Some(st)) => Err(format!(
                "Der MCP-Server hat sich beendet ({st}). Meist ist der Port belegt, das Token fehlt oder der Projektordner ist nicht lesbar."
            )),
            Err(e) => Err(e.to_string()),
        }
    }

    pub fn stop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for ServerProcess {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> McpServerConfig {
        McpServerConfig {
            project: Some(PathBuf::from("/tmp/mein projekt.woa")),
            ..McpServerConfig::default()
        }
    }

    #[test]
    fn token_is_random_hex() {
        let a = new_token().unwrap();
        let b = new_token().unwrap();
        assert_eq!(a.len(), 64);
        assert!(a.bytes().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b);
    }

    #[test]
    fn snippets_are_valid_and_hide_the_token_while_masked() {
        let exe = PathBuf::from("/opt/wir on air/woa-headless");
        let list = snippets(&cfg(), Some(&exe), "studio.example");
        assert_eq!(list.len(), 6);
        let token = "platzhalter-fuer-den-test";
        for s in &list {
            let shown = s.masked();
            assert!(!shown.contains(token));
            assert!(!shown.contains("@@"));
            let real = s.with_token(token);
            assert_eq!(s.uses_token(), real.contains(token), "{}", s.client);
            if real.trim_start().starts_with('{') {
                let v: serde_json::Value = serde_json::from_str(&real).unwrap_or_else(|e| {
                    panic!("{} {}: kein JSON: {e}\n{real}", s.client, s.target)
                });
                assert!(v["mcpServers"]["woa"].is_object());
            } else {
                assert!(real.starts_with("claude mcp add"));
            }
        }
        // Stdio nutzt kein Token, HTTP immer.
        assert!(
            list.iter()
                .all(|s| (s.transport == Transport::Http) == s.uses_token())
        );
        // Pfade mit Leerzeichen bleiben ein Argument.
        let cc = &list[0];
        assert_eq!(
            cc.masked(),
            "claude mcp add woa -- '/opt/wir on air/woa-headless' mcp '/tmp/mein projekt.woa'"
        );
    }

    #[test]
    fn terminal_quoting_never_evaluates_anything() {
        // bash/zsh: nichts in einfachen Anführungszeichen wird ausgewertet.
        assert_eq!(quote_posix("/a/b.woa").as_deref(), Some("/a/b.woa"));
        assert_eq!(
            quote_posix("/x/$(rm -rf ~)/`id`/$HOME").as_deref(),
            Some("'/x/$(rm -rf ~)/`id`/$HOME'")
        );
        assert_eq!(quote_posix("/x/it's").as_deref(), Some("'/x/it'\\''s'"));
        // PowerShell: '…' ist wörtlich, ' wird verdoppelt.
        assert_eq!(
            quote_powershell("C:\\Pro gramme\\$(calc)\\it's").as_deref(),
            Some("'C:\\Pro gramme\\$(calc)\\it''s'")
        );
        assert_eq!(
            quote_powershell("C:\\a\u{2019}b").as_deref(),
            Some("'C:\\a\u{2019}\u{2019}b'")
        );
        // Zeilenumbruch: abgelehnt, kein Terminal-Befehl.
        assert_eq!(quote_posix("/a\nrm -rf ~"), None);
        assert_eq!(quote_powershell("C:\\a\r\nx"), None);
        let mut c = cfg();
        c.project = Some("/tmp/a\nb.woa".into());
        let list = snippets(&c, Some(Path::new("/opt/woa-headless")), "x");
        assert!(
            !list
                .iter()
                .any(|s| s.transport == Transport::Stdio && s.target == "Terminal")
        );
        assert!(
            list.iter()
                .any(|s| s.client == "Claude Desktop" && s.transport == Transport::Stdio)
        );
    }

    #[test]
    fn without_headless_there_is_only_http() {
        let list = snippets(&cfg(), None, "x");
        assert_eq!(list.len(), 3);
        assert!(list.iter().all(|s| s.transport == Transport::Http));
    }

    #[test]
    fn network_reach_uses_the_given_host_local_never() {
        let mut c = cfg();
        assert_eq!(url(&c, "studio.example"), "http://127.0.0.1:8765/mcp");
        assert_eq!(c.bind_ip().to_string(), "127.0.0.1");
        c.reach = Reach::Network;
        assert_eq!(url(&c, "studio.example"), "http://studio.example:8765/mcp");
        assert_eq!(c.bind_ip().to_string(), "0.0.0.0");
    }

    #[test]
    fn ports_below_1024_are_refused() {
        let mut c = cfg();
        c.port = 80;
        assert!(c.check().is_err());
        c.port = 8765;
        assert!(c.check().is_ok());
    }

    #[test]
    fn defaults_are_not_written() {
        let k = crate::KiConfig::default();
        assert!(k.is_default());
        let s = serde_json::to_string(&k).unwrap();
        let back: crate::KiConfig = serde_json::from_str("{}").unwrap();
        assert_eq!(back, k);
        assert!(!s.contains("token"));
    }
}
