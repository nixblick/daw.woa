//! Live-Modus der KI in der App (siehe `woa_agent::live`): Die App hostet den MCP-Server nur
//! auf dieser Maschine und leiht ihm je Werkzeugaufruf ihre Sitzung. So erscheinen Spuren
//! und Clips von Claude sofort, und jeder Schritt steht als „Agent: …“ im Rückgängig-Verlauf.

use eframe::egui;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use woa_agent::live::LiveServer;

use crate::state::AppState;

/// So oft versucht die App den Start erneut, solange kein Token da ist (Schlüsselbund lesen).
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
    tried: Option<Instant>,
}

impl McpLive {
    /// `enabled = false` (Oberflächentests ohne Ton): kein Server, kein Schlüsselbund.
    pub fn new(data_dir: PathBuf, enabled: bool) -> Self {
        Self {
            enabled,
            data_dir,
            server: None,
            tried: None,
        }
    }

    /// Startet den Server, sobald ein Token da ist; bedient dann die wartenden Aufrufe.
    /// Gibt die Zahl der Agent-Schritte dieses Bildes zurück.
    pub fn frame(&mut self, ctx: &egui::Context, state: &mut AppState) -> usize {
        if !self.enabled {
            return 0;
        }
        if self.server.is_none() && self.tried.is_none_or(|t| t.elapsed() >= RETRY) {
            self.tried = Some(Instant::now());
            self.start(ctx);
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

    fn start(&mut self, ctx: &egui::Context) {
        let Some(token) = woa_secrets::reveal(&self.data_dir, woa_assistant::server::TOKEN_ACCOUNT)
        else {
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
            }
            Err(e) => tracing::warn!(error = %e, "mcp live server not started"),
        }
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

    /// Kleine Anzeige in der Statusleiste.
    pub fn badge(&self, ui: &mut egui::Ui) {
        use crate::theme;
        let (text, color, hint) = match self.status() {
            LiveStatus::Off => return,
            LiveStatus::Ready => (
                format!("{} KI bereit", theme::ic::PLUGS),
                theme::text_dim(),
                "Claude kann sich verbinden: `woa-headless mcp` reicht an diese App durch",
            ),
            LiveStatus::Connected => (
                format!("{} KI verbunden", theme::ic::PLUGS_CONNECTED),
                theme::ok(),
                "Claude arbeitet live auf diesem Projekt; jeder Schritt ist rückgängig zu machen",
            ),
        };
        ui.label(egui::RichText::new(text).small().color(color))
            .on_hover_text(hint);
    }
}
