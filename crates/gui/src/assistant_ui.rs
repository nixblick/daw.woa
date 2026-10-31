//! Reiter „Assistent“ im Fenster „KI & MCP“: frei wählbare Anbieter (Anthropic oder
//! OpenAI-kompatibel: OpenAI, OpenRouter, Ollama, LM Studio, vLLM …) und beliebige externe
//! MCP-Server als Werkzeuge. Schlüssel und Zugangsdaten liegen im Schlüsselbund
//! ([`woa_secrets`]), nie in `config.json`; im Feld sind sie maskiert.

use std::collections::HashMap;
use std::path::Path;
use std::sync::mpsc::{Receiver, channel};

use eframe::egui;
use woa_assistant::llm::Kind;
use woa_assistant::mcp_client;
use woa_assistant::{Endpoint, ExternalServer, KiConfig, ProviderConfig};

use crate::theme;

type Done = (String, Result<String, String>);

/// Eingabepuffer, die nicht 1:1 in der Konfiguration stehen.
#[derive(Default, Clone)]
struct Bufs {
    args: String,
    env: String,
    headers: String,
    /// Neues Geheimnis; leer = unverändert.
    secret: String,
}

#[derive(Default)]
pub struct AssistantTab {
    /// Aufgeklappter Server (Kennung).
    server_open: Option<String>,
    bufs: HashMap<String, Bufs>,
    /// Ergebnis des letzten Tests je Kennung.
    results: HashMap<String, Result<String, String>>,
    models: HashMap<String, Vec<String>>,
    running: Option<Receiver<Done>>,
    running_key: Option<String>,
    note: Option<String>,
}

fn kind_label(k: Kind) -> &'static str {
    match k {
        Kind::Anthropic => "Anthropic (Claude)",
        Kind::OpenAiCompat => "OpenAI-kompatibel",
    }
}

impl AssistantTab {
    pub fn reset(&mut self, cfg: &KiConfig) {
        *self = Self::default();
        for s in &cfg.servers {
            self.load_bufs(s);
        }
    }

    fn load_bufs(&mut self, s: &ExternalServer) {
        let b = match &s.endpoint {
            Endpoint::Stdio { args, env, .. } => Bufs {
                args: woa_assistant::join_args(args),
                env: woa_assistant::join_pairs(env, '='),
                ..Default::default()
            },
            Endpoint::Http { headers, .. } => Bufs {
                headers: woa_assistant::join_pairs(headers, ':'),
                ..Default::default()
            },
        };
        self.bufs.insert(s.id.clone(), b);
    }

    fn poll(&mut self, ctx: &egui::Context) {
        if let Some(rx) = &self.running {
            match rx.try_recv() {
                Ok((key, r)) => {
                    self.results.insert(key, r);
                    self.running = None;
                    self.running_key = None;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    ctx.request_repaint_after(std::time::Duration::from_millis(200));
                }
                Err(_) => {
                    self.running = None;
                    self.running_key = None;
                }
            }
        }
    }

    fn busy(&self, key: &str) -> bool {
        self.running_key.as_deref() == Some(key)
    }

    fn start<F>(&mut self, key: &str, work: F)
    where
        F: FnOnce() -> Result<String, String> + Send + 'static,
    {
        if self.running.is_some() {
            return;
        }
        let (tx, rx) = channel();
        let k = key.to_string();
        std::thread::spawn(move || {
            let _ = tx.send((k, work()));
        });
        self.running = Some(rx);
        self.running_key = Some(key.to_string());
        self.results.remove(key);
    }

    pub fn show(&mut self, ui: &mut egui::Ui, draft: &mut KiConfig, data_dir: &Path) {
        self.poll(ui.ctx());
        ui.label(
            egui::RichText::new(
                "Der Assistent im Chat-Fenster nutzt ein Sprachmodell deiner Wahl und kann WOA \
                 sowie beliebige weitere MCP-Server als Werkzeuge bedienen. Nichts davon ist \
                 fest verdrahtet: Adresse, Modell und Schlüssel trägst du selbst ein. Schlüssel \
                 liegen im Schlüsselbund des Systems, nie in der Konfiguration.",
            )
            .small()
            .color(theme::text_dim()),
        );
        ui.add_space(6.0);
        ui.label(egui::RichText::new("Sprachmodell").strong());
        self.providers(ui, draft, data_dir);
        ui.add_space(10.0);
        ui.separator();
        ui.label(egui::RichText::new("Werkzeuge").strong());
        ui.checkbox(
            &mut draft.woa_tools,
            "WOAs eigene Werkzeuge (Spuren, Clips, Mischen …) dem Assistenten geben",
        )
        .on_hover_text("Der Assistent startet dafür woa-headless auf dem Projektordner.");
        self.servers(ui, draft, data_dir);
        if let Some(n) = &self.note {
            ui.label(egui::RichText::new(n).small().color(theme::warn()));
        }
    }

    fn providers(&mut self, ui: &mut egui::Ui, draft: &mut KiConfig, data_dir: &Path) {
        // Auswahl und Neu.
        ui.horizontal(|ui| {
            let current = draft
                .active_provider()
                .map_or("keiner angelegt".to_string(), |p| p.name.clone());
            egui::ComboBox::from_id_salt("ki-active")
                .selected_text(current)
                .show_ui(ui, |ui| {
                    for p in draft.providers.clone() {
                        if ui
                            .selectable_label(
                                draft.active_provider().is_some_and(|a| a.id == p.id),
                                &p.name,
                            )
                            .clicked()
                        {
                            draft.active = Some(p.id);
                        }
                    }
                });
            ui.menu_button("Neu …", |ui| {
                for (name, kind, url) in ProviderConfig::presets() {
                    if ui.button(name).clicked() {
                        let id = woa_assistant::new_id();
                        draft.providers.push(ProviderConfig {
                            id: id.clone(),
                            name: name.to_string(),
                            kind,
                            base_url: url.to_string(),
                            model: String::new(),
                        });
                        draft.active = Some(id);
                        ui.close();
                    }
                }
                ui.separator();
                if ui.button("Eigener Server (leer)").clicked() {
                    let id = woa_assistant::new_id();
                    draft.providers.push(ProviderConfig {
                        id: id.clone(),
                        name: "Eigener Server".into(),
                        ..Default::default()
                    });
                    draft.active = Some(id);
                    ui.close();
                }
            });
        });
        let Some(active) = draft.active_provider().map(|p| p.id.clone()) else {
            ui.label(
                egui::RichText::new("Lege mit „Neu …“ einen Anbieter an.")
                    .small()
                    .color(theme::text_dim()),
            );
            return;
        };
        let Some(idx) = draft.providers.iter().position(|p| p.id == active) else {
            return;
        };
        let mut remove = false;
        {
            let p = &mut draft.providers[idx];
            let b = self.bufs.entry(format!("ki-{}", p.id)).or_default();
            let stored = woa_secrets::stored(data_dir, &p.secret_account());
            egui::Grid::new("ki-provider-grid")
                .num_columns(2)
                .spacing([12.0, 8.0])
                .show(ui, |ui| {
                    ui.label("Name");
                    ui.text_edit_singleline(&mut p.name);
                    ui.end_row();
                    ui.label("Format");
                    egui::ComboBox::from_id_salt("ki-kind")
                        .selected_text(kind_label(p.kind))
                        .show_ui(ui, |ui| {
                            for k in Kind::ALL {
                                ui.selectable_value(&mut p.kind, k, k.label());
                            }
                        });
                    ui.end_row();
                    ui.label("Adresse");
                    ui.add(
                        egui::TextEdit::singleline(&mut p.base_url)
                            .hint_text("z. B. http://localhost:11434/v1")
                            .desired_width(320.0),
                    );
                    ui.end_row();
                    ui.label("Modell");
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut p.model)
                                .hint_text("Name, wie ihn der Anbieter kennt")
                                .desired_width(220.0),
                        );
                        if let Some(list) = self.models.get(&p.id) {
                            egui::ComboBox::from_id_salt("ki-models")
                                .selected_text("Liste")
                                .show_ui(ui, |ui| {
                                    for m in list {
                                        if ui.selectable_label(&p.model == m, m).clicked() {
                                            p.model.clone_from(m);
                                        }
                                    }
                                });
                        }
                    });
                    ui.end_row();
                    ui.label("Schlüssel");
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut b.secret)
                                .password(true)
                                .hint_text(match stored {
                                    Some(o) => format!("gespeichert ({})", o.label()),
                                    None => "leer, wenn der Server keinen braucht".to_string(),
                                })
                                .desired_width(220.0),
                        );
                        if ui
                            .add_enabled(!b.secret.is_empty(), egui::Button::new("Speichern"))
                            .clicked()
                        {
                            match woa_secrets::save(data_dir, &p.secret_account(), &b.secret) {
                                Ok(_) => b.secret.clear(),
                                Err(e) => {
                                    self.note = Some(format!("Schlüssel nicht gespeichert: {e}"))
                                }
                            }
                        }
                        if stored.is_some() && ui.button("Entfernen").clicked() {
                            let _ = woa_secrets::clear(data_dir, &p.secret_account());
                        }
                    });
                    ui.end_row();
                });
        }
        let p = draft.providers[idx].clone();
        let typed = self
            .bufs
            .get(&format!("ki-{}", p.id))
            .map(|b| b.secret.clone())
            .filter(|s| !s.is_empty());
        ui.horizontal(|ui| {
            let busy = self.busy(&p.id);
            if ui
                .add_enabled(
                    !busy && !p.base_url.trim().is_empty(),
                    egui::Button::new("Verbindung testen"),
                )
                .clicked()
            {
                let provider = provider_with(&p, data_dir, typed.clone());
                self.start(&p.id, move || provider.test());
            }
            if ui
                .add_enabled(
                    !busy && !p.base_url.trim().is_empty(),
                    egui::Button::new("Modelle holen"),
                )
                .clicked()
            {
                let provider = provider_with(&p, data_dir, typed);
                let id = p.id.clone();
                self.start(&format!("{id}#modelle"), move || {
                    provider.models().map(|m| m.join("\n"))
                });
            }
            if busy || self.busy(&format!("{}#modelle", p.id)) {
                ui.spinner();
            }
            if ui.button("Anbieter löschen").clicked() {
                remove = true;
            }
        });
        if let Some(Ok(list)) = self.results.get(&format!("{}#modelle", p.id)).cloned() {
            let v: Vec<String> = list.lines().map(str::to_string).collect();
            self.results.insert(
                p.id.clone(),
                Ok(if v.is_empty() {
                    "Der Server nennt keine Modelle. Trag den Namen von Hand ein.".to_string()
                } else {
                    format!("{} Modelle geholt, wähle eins aus „Liste“.", v.len())
                }),
            );
            self.models.insert(p.id.clone(), v);
            self.results.remove(&format!("{}#modelle", p.id));
        } else if let Some(Err(e)) = self.results.remove(&format!("{}#modelle", p.id)) {
            self.results.insert(p.id.clone(), Err(e));
        }
        result_line(ui, self.results.get(&p.id));
        if remove {
            let _ = woa_secrets::clear(data_dir, &p.secret_account());
            draft.providers.remove(idx);
            draft.active = draft.providers.first().map(|p| p.id.clone());
        }
    }

    fn servers(&mut self, ui: &mut egui::Ui, draft: &mut KiConfig, data_dir: &Path) {
        let mut remove: Option<usize> = None;
        for i in 0..draft.servers.len() {
            let id = draft.servers[i].id.clone();
            let open = self.server_open.as_deref() == Some(id.as_str());
            theme::card_frame().show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    let s = &mut draft.servers[i];
                    ui.checkbox(&mut s.enabled, "");
                    ui.label(egui::RichText::new(&s.name).strong());
                    ui.label(
                        egui::RichText::new(match s.endpoint {
                            Endpoint::Stdio { .. } => "Stdio",
                            Endpoint::Http { .. } => "HTTP",
                        })
                        .small()
                        .color(theme::text_dim()),
                    );
                    if ui
                        .button(if open { "Zuklappen" } else { "Bearbeiten" })
                        .clicked()
                    {
                        self.server_open = if open { None } else { Some(id.clone()) };
                    }
                });
                if open {
                    self.server_form(ui, &mut draft.servers[i], data_dir, &mut remove, i);
                }
            });
        }
        if let Some(i) = remove {
            let s = draft.servers.remove(i);
            let _ = woa_secrets::clear(data_dir, &s.secret_account());
            self.bufs.remove(&s.id);
            self.server_open = None;
        }
        ui.menu_button("MCP-Server hinzufügen …", |ui| {
            if ui.button("Lokal starten (Stdio)").clicked() {
                self.add_server(draft, Endpoint::default());
                ui.close();
            }
            if ui.button("Über Adresse (HTTP)").clicked() {
                self.add_server(
                    draft,
                    Endpoint::Http {
                        url: String::new(),
                        headers: Vec::new(),
                    },
                );
                ui.close();
            }
        });
    }

    fn add_server(&mut self, draft: &mut KiConfig, endpoint: Endpoint) {
        let s = ExternalServer {
            id: woa_assistant::new_id(),
            name: "Neuer Server".into(),
            endpoint,
            ..Default::default()
        };
        self.load_bufs(&s);
        self.server_open = Some(s.id.clone());
        draft.servers.push(s);
    }

    fn server_form(
        &mut self,
        ui: &mut egui::Ui,
        s: &mut ExternalServer,
        data_dir: &Path,
        remove: &mut Option<usize>,
        index: usize,
    ) {
        let b = self.bufs.entry(s.id.clone()).or_default();
        let stored = woa_secrets::stored(data_dir, &s.secret_account());
        let is_http = matches!(s.endpoint, Endpoint::Http { .. });
        egui::Grid::new(format!("ki-server-{}", s.id))
            .num_columns(2)
            .spacing([12.0, 8.0])
            .show(ui, |ui| {
                ui.label("Name");
                ui.text_edit_singleline(&mut s.name);
                ui.end_row();
                match &mut s.endpoint {
                    Endpoint::Stdio { command, args, env } => {
                        ui.label("Startbefehl");
                        ui.add(
                            egui::TextEdit::singleline(command)
                                .hint_text("z. B. npx")
                                .desired_width(320.0),
                        );
                        ui.end_row();
                        ui.label("Argumente");
                        ui.add(
                            egui::TextEdit::singleline(&mut b.args)
                                .hint_text("z. B. -y @scope/server-name")
                                .desired_width(320.0),
                        );
                        ui.end_row();
                        ui.label("Umgebung");
                        ui.add(
                            egui::TextEdit::multiline(&mut b.env)
                                .hint_text("NAME=Wert, eine pro Zeile")
                                .desired_rows(2)
                                .desired_width(320.0),
                        );
                        ui.end_row();
                        *args = woa_assistant::split_args(&b.args);
                        *env = woa_assistant::parse_pairs(&b.env, '=');
                    }
                    Endpoint::Http { url, headers } => {
                        ui.label("Adresse");
                        ui.add(
                            egui::TextEdit::singleline(url)
                                .hint_text("https://…/mcp")
                                .desired_width(320.0),
                        );
                        ui.end_row();
                        ui.label("Kopfzeilen");
                        ui.add(
                            egui::TextEdit::multiline(&mut b.headers)
                                .hint_text("Name: Wert, eine pro Zeile (ohne Geheimnis)")
                                .desired_rows(2)
                                .desired_width(320.0),
                        );
                        ui.end_row();
                        *headers = woa_assistant::parse_pairs(&b.headers, ':');
                    }
                }
                ui.label(if is_http {
                    "Geheime Kopfzeile"
                } else {
                    "Geheime Variable"
                });
                ui.add(
                    egui::TextEdit::singleline(&mut s.secret_name)
                        .hint_text(if is_http {
                            "z. B. Authorization"
                        } else {
                            "z. B. API_TOKEN"
                        })
                        .desired_width(220.0),
                );
                ui.end_row();
                ui.label("Wert");
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut b.secret)
                            .password(true)
                            .hint_text(match stored {
                                Some(o) => format!("gespeichert ({})", o.label()),
                                None => "z. B. Bearer …".to_string(),
                            })
                            .desired_width(220.0),
                    );
                    if ui
                        .add_enabled(!b.secret.is_empty(), egui::Button::new("Speichern"))
                        .clicked()
                    {
                        match woa_secrets::save(data_dir, &s.secret_account(), &b.secret) {
                            Ok(_) => b.secret.clear(),
                            Err(e) => self.note = Some(format!("Wert nicht gespeichert: {e}")),
                        }
                    }
                    if stored.is_some() && ui.button("Entfernen").clicked() {
                        let _ = woa_secrets::clear(data_dir, &s.secret_account());
                    }
                });
                ui.end_row();
            });
        let typed = self
            .bufs
            .get(&s.id)
            .map(|b| b.secret.clone())
            .filter(|v| !v.is_empty());
        ui.horizontal(|ui| {
            let busy = self.busy(&s.id);
            if ui
                .add_enabled(!busy, egui::Button::new("Verbindung testen"))
                .clicked()
            {
                let mut spec_server = s.clone();
                let spec = spec_with(&mut spec_server, data_dir, typed);
                self.start(&s.id, move || mcp_client::test(&spec));
            }
            if busy {
                ui.spinner();
            }
            if ui.button("Server löschen").clicked() {
                *remove = Some(index);
            }
        });
        result_line(ui, self.results.get(&s.id));
    }
}

fn provider_with(
    p: &ProviderConfig,
    data_dir: &Path,
    typed: Option<String>,
) -> woa_assistant::llm::Provider {
    match typed {
        Some(t) => {
            woa_assistant::llm::Provider::new(p.kind, p.base_url.clone(), p.model.clone(), Some(t))
        }
        None => p.provider(data_dir),
    }
}

/// Der Zugang mit dem eben eingetippten Wert, falls noch nicht gespeichert.
fn spec_with(s: &mut ExternalServer, data_dir: &Path, typed: Option<String>) -> mcp_client::Spec {
    let mut spec = s.spec(data_dir);
    if let Some(t) = typed
        && !s.secret_name.trim().is_empty()
    {
        let name = s.secret_name.trim().to_string();
        match &mut spec {
            mcp_client::Spec::Stdio { env, .. } => {
                env.retain(|(k, _)| *k != name);
                env.push((name, t));
            }
            mcp_client::Spec::Http { headers, .. } => {
                headers.retain(|(k, _)| *k != name);
                headers.push((name, t));
            }
        }
    }
    spec
}

fn result_line(ui: &mut egui::Ui, r: Option<&Result<String, String>>) {
    match r {
        Some(Ok(t)) => {
            ui.label(egui::RichText::new(t).small().color(theme::ok()));
        }
        Some(Err(e)) => {
            ui.label(egui::RichText::new(e).small().color(theme::on_air()));
        }
        None => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tab_draws_with_provider_and_server() {
        let tmp = tempfile::tempdir().unwrap();
        let ctx = theme::test_context();
        let mut cfg = KiConfig::default();
        let mut tab = AssistantTab::default();
        tab.add_server(&mut cfg, Endpoint::default());
        cfg.providers.push(ProviderConfig {
            id: "ab12".into(),
            name: "Lokal".into(),
            base_url: "http://localhost:11434/v1".into(),
            ..Default::default()
        });
        let _ = ctx.run_ui(egui::RawInput::default(), |u| {
            egui::CentralPanel::default().show(u, |ui| {
                tab.show(ui, &mut cfg, tmp.path());
            });
        });
        assert_eq!(cfg.servers.len(), 1);
        assert_eq!(cfg.active_provider().unwrap().id, "ab12");
    }

    #[test]
    fn typed_secret_replaces_the_stored_one_for_the_test() {
        let tmp = tempfile::tempdir().unwrap();
        let mut s = ExternalServer {
            id: "ee11".into(),
            endpoint: Endpoint::Http {
                url: "http://x".into(),
                headers: vec![],
            },
            secret_name: "Authorization".into(),
            ..Default::default()
        };
        let spec = spec_with(&mut s, tmp.path(), Some("Bearer neu".into()));
        match spec {
            mcp_client::Spec::Http { headers, .. } => {
                assert_eq!(
                    headers,
                    vec![("Authorization".to_string(), "Bearer neu".to_string())]
                );
            }
            mcp_client::Spec::Stdio { .. } => panic!("http erwartet"),
        }
    }
}
