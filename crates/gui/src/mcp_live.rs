//! Live-Modus der KI in der App (siehe `woa_agent::live`): Die App hostet den MCP-Server nur
//! auf dieser Maschine und leiht ihm je Werkzeugaufruf ihre Sitzung. So erscheinen Spuren
//! und Clips von Claude sofort, und jeder Schritt steht als „Agent: …“ im Rückgängig-Verlauf.

use eframe::egui;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use woa_agent::live::LiveServer;

use crate::state::AppState;

/// So oft prüft die App den Schlüsselbund auf ein neues oder gelöschtes Token.
const RETRY: Duration = Duration::from_secs(30);
/// So lange nach dem letzten Kontakt gilt die KI als verbunden.
const CONNECTED_FOR: u64 = 300;

/// Zustand für die Anzeige „KI verbunden“.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveStatus {
    Off,
    Ready,
    Connected,
}

pub struct McpLive {
    enabled: bool,
    data_dir: PathBuf,
    server: Option<LiveServer>,
    /// Abdruck des Tokens, mit dem der Server läuft (nie das Token selbst).
    token_hash: Option<u64>,
    tried: Option<Instant>,
    /// Schlüssel dieses App-Starts, wenn der Nutzer kein Token angelegt hat: Der Live-Server
    /// läuft trotzdem (nur 127.0.0.1, Lock-Datei 0600). Nie gespeichert, nie angezeigt.
    own_key: Option<String>,
}

fn hash(token: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    token.hash(&mut h);
    h.finish()
}

impl McpLive {
    /// `enabled = false` (Oberflächentests ohne Ton): kein Server, kein Schlüsselbund.
    pub fn new(data_dir: PathBuf, enabled: bool) -> Self {
        Self {
            enabled,
            data_dir,
            server: None,
            token_hash: None,
            tried: None,
            own_key: None,
        }
    }

    /// Startet den Server (mit Nutzertoken oder dem Schlüssel dieses Starts); bedient dann die wartenden Aufrufe.
    /// Gibt die Zahl der Agent-Schritte dieses Bildes zurück.
    pub fn frame(&mut self, ctx: &egui::Context, state: &mut AppState) -> usize {
        if !self.enabled {
            return 0;
        }
        if self.tried.is_none_or(|t| t.elapsed() >= RETRY) {
            self.tried = Some(Instant::now());
            // Auch bei laufendem Server: gelöschtes oder neues Token beendet bzw. startet neu.
            let token = woa_secrets::reveal(&self.data_dir, woa_assistant::server::TOKEN_ACCOUNT);
            self.sync_token(ctx, token);
        }
        let Some(server) = &self.server else {
            return 0;
        };
        let dir = state.project_dir();
        let n = server.serve_pending(state.lend_session(), &dir);
        if n > 0 {
            ctx.request_repaint();
        }
        n
    }

    /// Token im Schlüsselbund neu erzeugt (Hook aus dem KI-Fenster): sofort neu prüfen.
    pub fn restart(&mut self) {
        self.tried = None;
    }

    /// Läuft der Server mit `token`? Sonst beenden (Lock-Datei weg) und neu starten.
    /// Ohne Nutzertoken gilt der Schlüssel dieses App-Starts.
    pub fn sync_token(&mut self, ctx: &egui::Context, token: Option<String>) {
        let token = token.filter(|t| !t.is_empty()).or_else(|| self.own_key());
        let want = token.as_deref().map(hash);
        if self.server.is_some() && self.token_hash == want {
            return;
        }
        if self.server.take().is_some() {
            tracing::info!("mcp live server stopped (token changed)");
        }
        self.token_hash = None;
        let Some(token) = token else {
            return;
        };
        let wake_ctx = ctx.clone();
        match LiveServer::start(
            &self.data_dir,
            token,
            0,
            std::sync::Arc::new(move || wake_ctx.request_repaint()),
        ) {
            Ok(s) => {
                tracing::info!(port = s.port(), "mcp live server started");
                self.server = Some(s);
                self.token_hash = want;
            }
            Err(e) => tracing::warn!(error = %e, "mcp live server not started"),
        }
    }

    fn own_key(&mut self) -> Option<String> {
        if self.own_key.is_none() {
            match woa_agent::live::new_secret() {
                Ok(k) => self.own_key = Some(k),
                Err(e) => tracing::warn!(error = %e, "mcp live key not created"),
            }
        }
        self.own_key.clone()
    }

    pub fn status(&self) -> LiveStatus {
        match &self.server {
            None => LiveStatus::Off,
            Some(s) if s.idle_seconds().is_some_and(|i| i <= CONNECTED_FOR) => {
                LiveStatus::Connected
            }
            Some(_) => LiveStatus::Ready,
        }
    }

    /// Pille „KI“ in der Status-Gruppe der Kopfleiste: grün leuchtend, wenn Claude live
    /// verbunden ist, ruhig, wenn sie bereit ist; „KI aus“, wenn der Live-Server nicht starten konnte.
    pub fn chip(&self, ui: &mut egui::Ui, compact: bool) {
        use crate::theme;
        use crate::widgets::{ChipLook, topbar_chip};
        if !self.enabled {
            return;
        }
        let (icon, word, look, hint) = match self.status() {
            LiveStatus::Off => (
                theme::ic::PLUGS,
                "KI aus",
                ChipLook::Quiet,
                "Live-Modus nicht gestartet (siehe Protokoll). Claude arbeitet dann nur auf der \
                 gespeicherten Projektdatei, nicht live in diesem Fenster",
            ),
            LiveStatus::Ready => (
                theme::ic::PLUGS,
                "KI bereit",
                ChipLook::Quiet,
                "KI bereit – Claude kann sich verbinden: `woa-headless mcp` reicht an diese App durch",
            ),
            LiveStatus::Connected => (
                theme::ic::PLUGS_CONNECTED,
                "KI verbunden",
                ChipLook::Lit(theme::ok()),
                "KI verbunden – Claude arbeitet live auf diesem Projekt; jeder Schritt ist rückgängig zu machen",
            ),
        };
        topbar_chip(ui, icon, word, look, compact, false).on_hover_text(hint);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn init() -> &'static str {
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"t","version":"1"}}}"#
    }

    fn status_for(port: u16, bearer: &str) -> String {
        let mut out = Vec::new();
        woa_agent::live::forward(port, bearer, init().as_bytes(), &mut out).unwrap();
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn token_change_restarts_and_the_old_token_gets_401() {
        let tmp = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let mut live = McpLive::new(tmp.path().to_path_buf(), true);
        live.sync_token(&ctx, Some("alt-aaa".into()));
        let first = live.server.as_ref().unwrap().port();
        assert!(!status_for(first, "alt-aaa").contains("401"));
        // Gleiches Token: nichts passiert.
        live.sync_token(&ctx, Some("alt-aaa".into()));
        assert_eq!(live.server.as_ref().unwrap().port(), first);
        live.sync_token(&ctx, Some("neu-bbb".into()));
        let port = live.server.as_ref().unwrap().port();
        assert!(status_for(port, "alt-aaa").contains("HTTP 401"));
        assert!(!status_for(port, "neu-bbb").contains("401"));
        // Gelöscht: Server läuft mit dem Schlüssel dieses Starts weiter, das alte Token gilt nicht.
        live.sync_token(&ctx, None);
        let port = live.server.as_ref().unwrap().port();
        assert!(status_for(port, "neu-bbb").contains("HTTP 401"));
    }

    #[test]
    fn live_server_starts_without_a_user_token() {
        let tmp = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let mut live = McpLive::new(tmp.path().to_path_buf(), true);
        live.sync_token(&ctx, None);
        assert_eq!(live.status(), LiveStatus::Ready);
        // woa-headless findet die App über die Lock-Datei und kommt mit dem Live-Schlüssel durch.
        let app = woa_agent::live::running_app(tmp.path()).expect("Lock-Datei der App");
        assert!(!status_for(app.port, &app.key).contains("401"));
        // Kein Neustart je Prüfung: gleicher Port beim nächsten Abgleich.
        live.sync_token(&ctx, None);
        assert_eq!(live.server.as_ref().unwrap().port(), app.port);
    }
}
