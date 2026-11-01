//! Fenster „KI“: vorn die einfache Seite „KI einrichten“ ([`crate::ki_setup`]), dahinter
//! unter „Erweitert“ WOA als MCP-Server für beliebige Clients (Claude Desktop, Claude Code,
//! andere). Das Zugriffstoken liegt im Schlüsselbund des Systems, nie in `config.json`
//! ([`woa_secrets`]); im Fenster ist es maskiert, „Kopieren“ legt es in die Zwischenablage.

use std::path::{Path, PathBuf};

use eframe::egui;
use woa_assistant::KiConfig;
use woa_assistant::server::{
    self, McpServerConfig, Reach, ServerProcess, Snippet, TOKEN_ACCOUNT, Transport,
};
use woa_secrets::Ort;

use crate::theme;

/// Platzhalter für den Namen dieses Rechners in Schnipseln für Clients im Netz.
const NETWORK_HOST: &str = "<IP-dieses-Rechners>";

#[derive(Default, Clone, Copy, PartialEq)]
enum Tab {
    #[default]
    Setup,
    Server,
    Assistant,
}

#[derive(Default)]
pub struct KiUi {
    open: bool,
    draft: KiConfig,
    token: Option<Ort>,
    /// Das Token im Klartext, nur solange „Anzeigen“ an ist.
    token_text: Option<String>,
    snippet_transport: Transport,
    tab: Tab,
    assistant: crate::assistant_ui::AssistantTab,
    setup: crate::ki_setup::KiSetup,
    note: Option<String>,
    proc: Option<ServerProcess>,
    proc_error: Option<String>,
    /// Das Token wurde neu erzeugt: der Live-Server der App muss neu starten.
    token_changed: bool,
}

impl KiUi {
    /// Öffnet das Fenster mit dem gespeicherten Stand.
    pub fn open(&mut self, saved: &KiConfig, data_dir: &Path) {
        self.open = true;
        self.draft = saved.clone();
        self.snippet_transport = saved.mcp_server.transport;
        self.token = woa_secrets::stored(data_dir, TOKEN_ACCOUNT);
        self.token_text = None;
        self.note = None;
        self.assistant.reset(&self.draft);
        self.tab = Tab::Setup;
    }

    /// Wurde das Token seit dem letzten Aufruf neu erzeugt? (Hook für den Live-Server.)
    pub fn take_token_changed(&mut self) -> bool {
        std::mem::take(&mut self.token_changed)
    }

    fn close(&mut self) {
        self.open = false;
        // Das Token bleibt nicht im Fensterzustand liegen.
        self.token_text = None;
    }

    /// Beim Start der App: Der HTTP-Server startet nicht von selbst (#243, André 01.11.2026:
    /// „nur auf Wunsch“). Ein Haken aus einer früheren Sitzung wird zurückgenommen, damit das
    /// Fenster den echten Zustand zeigt; Port, Reichweite und Projektordner bleiben.
    pub fn app_started(&mut self, cfg: &mut KiConfig) {
        self.proc = None;
        self.proc_error = None;
        if cfg.mcp_server.enabled {
            tracing::info!("mcp http server not started at app start (only on request)");
            cfg.mcp_server.enabled = false;
        }
    }

    /// Bringt den Hintergrundprozess in Einklang mit `cfg`: läuft, wenn eingeschaltet und
    /// alles da ist, sonst nicht. Nach „Speichern“ aufrufen.
    pub fn apply(&mut self, cfg: &KiConfig, data_dir: &Path) {
        self.proc = None;
        self.proc_error = None;
        let c = &cfg.mcp_server;
        if !c.enabled {
            return;
        }
        let Some(exe) = server::headless_exe() else {
            self.proc_error = Some("woa-headless liegt nicht neben WOA".into());
            return;
        };
        if woa_secrets::stored(data_dir, TOKEN_ACCOUNT).is_none() {
            self.proc_error = Some("Erst ein Zugriffstoken erzeugen".into());
            return;
        }
        match ServerProcess::start(&exe, c) {
            Ok(p) => {
                tracing::info!(addr = %p.addr, "mcp http server started");
                self.proc = Some(p);
            }
            Err(e) => {
                tracing::warn!(error = %e, "mcp http server not started");
                self.proc_error = Some(e);
            }
        }
    }

    /// Meldet ein vorzeitiges Ende des Hintergrundprozesses.
    fn check_proc(&mut self) {
        if let Some(p) = &mut self.proc
            && let Err(e) = p.poll()
        {
            tracing::warn!("mcp http server ended");
            self.proc_error = Some(e);
            self.proc = None;
        }
    }

    fn running(&self) -> Option<&str> {
        self.proc.as_ref().map(|p| p.addr.as_str())
    }

    /// Zeichnet das Fenster. `project`: der Ordner des offenen Projekts. Gibt den Stand
    /// zurück, wenn „Speichern“ gedrückt wurde und die Werte gültig sind.
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        data_dir: &Path,
        project: Option<&Path>,
    ) -> Option<KiConfig> {
        self.check_proc();
        if !self.open {
            return None;
        }
        let mut open = true;
        let mut save = false;
        let mut changed = false;
        egui::Window::new("KI")
            .open(&mut open)
            .collapsible(false)
            .default_width(600.0)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .max_height(560.0)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.selectable_value(&mut self.tab, Tab::Setup, "KI einrichten");
                            ui.selectable_value(
                                &mut self.tab,
                                Tab::Server,
                                "Erweitert: MCP-Server",
                            );
                            ui.selectable_value(
                                &mut self.tab,
                                Tab::Assistant,
                                "Erweitert: Anbieter und Werkzeuge",
                            );
                        });
                        ui.separator();
                        match self.tab {
                            Tab::Setup => {
                                changed = self.setup.show(
                                    ui,
                                    &mut self.draft,
                                    data_dir,
                                    project,
                                    server::headless_exe(),
                                );
                            }
                            Tab::Server => self.server_tab(ui, data_dir, project, &mut save),
                            Tab::Assistant => {
                                self.assistant.show(ui, &mut self.draft, data_dir);
                                ui.add_space(8.0);
                                if ui.button("Speichern").clicked() {
                                    save = true;
                                }
                                if let Some(n) = &self.note {
                                    ui.colored_label(ui.visuals().warn_fg_color, n);
                                }
                            }
                        }
                    });
            });
        let mut applied = None;
        if changed && !save && open {
            // Die Wahl im Dropdown gilt sofort; das Fenster bleibt offen.
            return Some(self.draft.clone());
        }
        if save {
            match self.draft.mcp_server.check() {
                Ok(()) => {
                    self.draft.mcp_server.transport = self.snippet_transport;
                    applied = Some(self.draft.clone());
                }
                Err(e) => self.note = Some(e),
            }
        }
        if !open || applied.is_some() {
            self.close();
        }
        applied
    }

    fn server_tab(
        &mut self,
        ui: &mut egui::Ui,
        data_dir: &Path,
        project: Option<&Path>,
        save: &mut bool,
    ) {
        ui.label(
            egui::RichText::new(
                "WOA als MCP-Server: Claude Desktop, Claude Code oder jeder andere MCP-Client \
                 kann WOA bedienen (Spuren, Effekte, Export, Analyse). Du wählst, ob der \
                 Client WOA selbst startet (Stdio) oder WOA im Hintergrund wartet (HTTP mit \
                 Zugriffstoken).",
            )
            .small()
            .color(theme::text_dim()),
        );
        ui.add_space(6.0);
        let mut cfg = self.draft.mcp_server.clone();
        ui.checkbox(
            &mut cfg.enabled,
            "HTTP-Server im Hintergrund, bis WOA schließt",
        )
        .on_hover_text(
            "Startet erst mit „Speichern“ und nie von selbst beim Start von WOA. Aus: nichts hört \
             auf einem Port. Die Stdio-Schnipsel unten brauchen den Server nicht.",
        );
        egui::Grid::new("ki-server-grid")
            .num_columns(2)
            .spacing([12.0, 8.0])
            .show(ui, |ui| {
                self.server_fields(ui, &mut cfg, data_dir, project);
            });
        self.draft.mcp_server = cfg;
        if self.draft.mcp_server.reach == Reach::Network {
            ui.label(
                egui::RichText::new(
                    "Achtung: Jeder Rechner im Netz kann sich mit dem Token verbinden. Wer das \
                     Token hat, darf Dateien des Projekts schreiben und lesen. Nur in einem \
                     Netz, dem du traust, und nie unverschlüsselt über das Internet.",
                )
                .small()
                .color(theme::warn()),
            );
        }
        ui.label(
            egui::RichText::new(
                "Dieser Hintergrund-Server (HTTP, eigener Port) arbeitet nur auf der Projektdatei \
                 im Ordner, nicht live im offenen Fenster. Live arbeitet Claude über den \
                 Stdio-Eintrag (`woa-headless mcp`), der an die laufende App durchreicht \
                 („KI bereit“ in der Kopfleiste). Speichere nicht gleichzeitig im Fenster, sonst \
                 gewinnt der letzte Schreiber.",
            )
            .small()
            .color(theme::text_dim()),
        );
        ui.add_space(6.0);
        self.status_line(ui);
        ui.add_space(8.0);
        ui.separator();
        ui.label(egui::RichText::new("Eintrag für deinen Client").strong());
        ui.horizontal(|ui| {
            ui.selectable_value(
                &mut self.snippet_transport,
                Transport::Stdio,
                "Stdio (Client startet WOA)",
            );
            ui.selectable_value(
                &mut self.snippet_transport,
                Transport::Http,
                "HTTP (WOA läuft im Hintergrund)",
            );
        });
        self.snippets(ui, data_dir);
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.button("Speichern").clicked() {
                *save = true;
            }
        });
        if let Some(n) = &self.note {
            ui.add_space(4.0);
            ui.label(egui::RichText::new(n).small().color(theme::warn()));
        }
    }

    fn server_fields(
        &mut self,
        ui: &mut egui::Ui,
        cfg: &mut McpServerConfig,
        data_dir: &Path,
        project: Option<&Path>,
    ) {
        ui.label("Projektordner");
        ui.horizontal(|ui| {
            let text = cfg
                .project
                .as_ref()
                .map_or("keiner gewählt".to_string(), |p| p.display().to_string());
            ui.label(egui::RichText::new(text).monospace());
            if ui.button("Wählen …").clicked()
                && let Some(dir) = rfd::FileDialog::new().pick_folder()
            {
                cfg.project = Some(dir);
            }
            if let Some(p) = project
                && ui
                    .button("Offenes Projekt")
                    .on_hover_text("Den Ordner des gerade offenen Projekts nehmen")
                    .clicked()
            {
                cfg.project = Some(PathBuf::from(p));
            }
        });
        ui.end_row();

        ui.label("Erreichbar");
        egui::ComboBox::from_id_salt("ki-reach")
            .selected_text(match cfg.reach {
                Reach::Local => "nur dieser Rechner",
                Reach::Network => "auch andere Rechner im Netz",
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut cfg.reach, Reach::Local, "nur dieser Rechner");
                ui.selectable_value(
                    &mut cfg.reach,
                    Reach::Network,
                    "auch andere Rechner im Netz",
                );
            });
        ui.end_row();

        ui.label("Port");
        ui.add(egui::DragValue::new(&mut cfg.port).range(1024..=65_535));
        ui.end_row();

        ui.label("Zugriffstoken");
        ui.horizontal(|ui| {
            let shown = match (&self.token_text, self.token) {
                (Some(t), _) => t.clone(),
                (None, Some(_)) => server::MASK.to_string(),
                (None, None) => "noch keins".to_string(),
            };
            ui.label(egui::RichText::new(shown).monospace());
            if self.token.is_some() {
                let label = if self.token_text.is_some() {
                    "Verbergen"
                } else {
                    "Anzeigen"
                };
                if ui.button(label).clicked() {
                    self.token_text = if self.token_text.is_some() {
                        None
                    } else {
                        woa_secrets::reveal(data_dir, TOKEN_ACCOUNT)
                    };
                }
                if ui.button("Kopieren").clicked()
                    && let Some(t) = woa_secrets::reveal(data_dir, TOKEN_ACCOUNT)
                {
                    ui.ctx().copy_text(t);
                }
            }
            let new_label = if self.token.is_some() {
                "Neu erzeugen"
            } else {
                "Erzeugen"
            };
            if ui
                .button(new_label)
                .on_hover_text(
                    "Ein altes Token gilt danach nicht mehr; Clients brauchen den neuen Eintrag",
                )
                .clicked()
            {
                self.new_token(data_dir);
            }
        });
        ui.end_row();
        if let Some(ort) = self.token {
            ui.label("");
            ui.label(
                egui::RichText::new(format!("Token liegt {}", ort.label()))
                    .small()
                    .color(theme::text_dim()),
            );
            ui.end_row();
        }
    }

    fn new_token(&mut self, data_dir: &Path) {
        let result =
            server::new_token().and_then(|t| woa_secrets::save(data_dir, TOKEN_ACCOUNT, &t));
        match result {
            Ok(ort) => {
                self.token = Some(ort);
                self.token_text = None;
                self.note = Some(
                    "Neues Token gespeichert. Mit „Speichern“ startet der Server damit neu.".into(),
                );
                // Ein laufender Server kennt noch das alte Token.
                self.proc = None;
                self.token_changed = true;
            }
            Err(e) => self.note = Some(format!("Token nicht gespeichert: {e}")),
        }
    }

    fn status_line(&self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            if let Some(addr) = self.running() {
                ui.label(egui::RichText::new("läuft").strong().color(theme::ok()));
                ui.label(format!("auf {addr}"));
            } else if let Some(e) = &self.proc_error {
                ui.label(
                    egui::RichText::new("gestoppt")
                        .strong()
                        .color(theme::danger_text()),
                );
                ui.label(e);
            } else {
                ui.label(egui::RichText::new("Server aus").color(theme::text_dim()));
            }
        });
    }

    fn snippets(&mut self, ui: &mut egui::Ui, data_dir: &Path) {
        let exe = server::headless_exe();
        let list = server::snippets(&self.draft.mcp_server, exe.as_deref(), NETWORK_HOST);
        let mut any = false;
        for s in list
            .iter()
            .filter(|s| s.transport == self.snippet_transport)
        {
            any = true;
            snippet_card(ui, s, data_dir);
        }
        if !any {
            ui.label(
                egui::RichText::new(
                    "woa-headless liegt nicht neben WOA, deshalb gibt es keinen Stdio-Eintrag. \
                     Nimm HTTP oder installiere WOA vollständig.",
                )
                .small()
                .color(theme::warn()),
            );
        }
        if self.draft.mcp_server.project.is_none() {
            ui.label(
                egui::RichText::new("Wähle oben einen Projektordner; er steht in den Einträgen.")
                    .small()
                    .color(theme::text_dim()),
            );
        }
    }
}

fn snippet_card(ui: &mut egui::Ui, s: &Snippet, data_dir: &Path) {
    theme::card_frame().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(s.client).strong());
            ui.label(
                egui::RichText::new(s.target)
                    .small()
                    .color(theme::text_dim()),
            );
        });
        ui.label(egui::RichText::new(s.masked()).monospace().small());
        if ui.button("Kopieren").clicked() {
            let text = if s.uses_token() {
                match woa_secrets::reveal(data_dir, TOKEN_ACCOUNT) {
                    Some(t) => s.with_token(&t),
                    // Ohne Token bleibt die Maske stehen; der Hinweis steht im Fenster.
                    None => s.masked(),
                }
            } else {
                s.masked()
            };
            ui.ctx().copy_text(text);
        }
    });
    ui.add_space(4.0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_draws_and_closing_forgets_the_token() {
        let tmp = tempfile::tempdir().unwrap();
        let ctx = theme::test_context();
        let mut ui = KiUi::default();
        ui.open(&KiConfig::default(), tmp.path());
        let _ = ctx.run_ui(egui::RawInput::default(), |u| {
            assert!(
                ui.show(u.ctx(), tmp.path(), Some(Path::new("/x")))
                    .is_none()
            );
        });
        assert!(ui.open);
        ui.token_text = Some("geheim".into());
        ui.close();
        assert!(ui.token_text.is_none());
    }

    #[test]
    fn disabled_server_starts_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let mut ui = KiUi::default();
        ui.apply(&KiConfig::default(), tmp.path());
        assert!(ui.running().is_none());
        assert!(ui.proc_error.is_none());
    }

    /// #243: Der HTTP-Server (Port 8765) startet nie beim Start der App, auch wenn er in
    /// einer früheren Sitzung eingeschaltet war. Nur „Speichern“ mit Haken startet ihn.
    #[test]
    fn app_start_never_starts_http_server() {
        let tmp = tempfile::tempdir().unwrap();
        let mut cfg = KiConfig::default();
        cfg.mcp_server.enabled = true;
        cfg.mcp_server.project = Some(tmp.path().to_path_buf());
        let mut ui = KiUi::default();
        ui.app_started(&mut cfg);
        assert!(ui.running().is_none());
        assert!(ui.proc_error.is_none());
        assert!(!cfg.mcp_server.enabled, "Haken zeigt den echten Zustand");
        assert_eq!(cfg.mcp_server.project.as_deref(), Some(tmp.path()));
    }

    #[test]
    fn enabled_server_without_token_says_why() {
        let tmp = tempfile::tempdir().unwrap();
        let mut cfg = KiConfig::default();
        cfg.mcp_server.enabled = true;
        let mut ui = KiUi::default();
        ui.apply(&cfg, tmp.path());
        assert!(ui.running().is_none());
        assert!(ui.proc_error.is_some());
    }
}
