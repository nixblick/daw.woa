//! Chat-Fenster des Assistenten. Das Gespräch läuft in einem Hintergrund-Thread
//! ([`woa_assistant::chat::Worker`]); die Oberfläche wartet nie. Werkzeuge: WOA selbst
//! (über `woa-headless mcp` auf dem offenen Projektordner) und die eingetragenen
//! MCP-Server.

use std::path::Path;

use eframe::egui;
use woa_assistant::KiConfig;
use woa_assistant::chat::{Cmd, Ev, Worker};
use woa_assistant::mcp_client::Spec;

use crate::theme;

enum Line {
    You(String),
    Assistant(String),
    Tool(String),
    Note(String),
    Error(String),
}

#[derive(Default)]
pub struct ChatUi {
    open: bool,
    worker: Option<Worker>,
    lines: Vec<Line>,
    input: String,
    busy: bool,
    ready: bool,
    /// Nichts da, womit man chatten könnte: sagt warum.
    problem: Option<String>,
}

impl ChatUi {
    pub fn open(&mut self, cfg: &KiConfig, data_dir: &Path, project: Option<&Path>) {
        self.open = true;
        if self.worker.is_none() {
            self.start(cfg, data_dir, project);
        }
    }

    /// Neues Gespräch mit dem aktuellen Stand der Einstellungen.
    pub fn start(&mut self, cfg: &KiConfig, data_dir: &Path, project: Option<&Path>) {
        self.worker = None;
        self.lines.clear();
        self.busy = false;
        self.ready = false;
        self.problem = None;
        let Some(p) = cfg.active_provider() else {
            self.problem =
                Some("Noch kein Anbieter eingetragen (Einstellungen, KI & MCP, Assistent).".into());
            return;
        };
        if p.base_url.trim().is_empty() || p.model.trim().is_empty() {
            self.problem = Some("Beim Anbieter fehlen Adresse oder Modell.".into());
            return;
        }
        let provider = p.provider(data_dir);
        let mut specs: Vec<(String, Spec)> = Vec::new();
        if cfg.woa_tools {
            match (woa_assistant::server::headless_exe(), project) {
                (Some(exe), Some(dir)) => specs.push((
                    "WOA".into(),
                    Spec::Stdio {
                        command: exe.display().to_string(),
                        args: vec!["mcp".into(), dir.display().to_string()],
                        env: vec![],
                    },
                )),
                (None, _) => self.lines.push(Line::Note(
                    "woa-headless liegt nicht neben WOA, deshalb fehlen WOAs eigene Werkzeuge."
                        .into(),
                )),
                (_, None) => self.lines.push(Line::Note(
                    "Kein Projekt offen, deshalb fehlen WOAs eigene Werkzeuge.".into(),
                )),
            }
        }
        for s in cfg.servers.iter().filter(|s| s.enabled) {
            specs.push((s.name.clone(), s.spec(data_dir)));
        }
        self.worker = Some(Worker::spawn(provider, specs));
    }

    fn drain(&mut self) {
        let Some(w) = &self.worker else { return };
        while let Ok(ev) = w.rx.try_recv() {
            match ev {
                Ev::Notice(n) => self.lines.push(Line::Note(n)),
                Ev::Ready(n) => {
                    self.ready = true;
                    self.lines.push(Line::Note(format!(
                        "Bereit, {n} {} verfügbar.",
                        if n == 1 { "Werkzeug" } else { "Werkzeuge" }
                    )));
                }
                Ev::Tool(t) => self.lines.push(Line::Tool(t)),
                Ev::Reply(r) => {
                    self.busy = false;
                    self.lines.push(Line::Assistant(r));
                }
                Ev::Error(e) => {
                    self.busy = false;
                    self.lines.push(Line::Error(e));
                }
            }
        }
    }

    fn send(&mut self) {
        let text = self.input.trim().to_string();
        if text.is_empty() || self.busy || !self.ready {
            return;
        }
        if let Some(w) = &self.worker
            && w.tx.send(Cmd::Send(text.clone())).is_ok()
        {
            self.lines.push(Line::You(text));
            self.input.clear();
            self.busy = true;
        }
    }

    pub fn show(
        &mut self,
        ctx: &egui::Context,
        cfg: &KiConfig,
        data_dir: &Path,
        project: Option<&Path>,
    ) {
        if !self.open {
            return;
        }
        self.drain();
        if self.busy || (self.worker.is_some() && !self.ready) {
            ctx.request_repaint_after(std::time::Duration::from_millis(150));
        }
        let mut open = true;
        let mut restart = false;
        egui::Window::new("Assistent")
            .open(&mut open)
            .collapsible(false)
            .default_size([520.0, 520.0])
            .show(ctx, |ui| {
                if let Some(p) = &self.problem {
                    ui.label(egui::RichText::new(p).color(theme::warn()));
                    if ui.button("Erneut versuchen").clicked() {
                        restart = true;
                    }
                    return;
                }
                let model = cfg
                    .active_provider()
                    .map_or(String::new(), |p| format!("{} · {}", p.name, p.model));
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(model).small().color(theme::text_dim()));
                    if ui.button("Neues Gespräch").clicked() {
                        restart = true;
                    }
                });
                egui::ScrollArea::vertical()
                    .max_height(380.0)
                    .auto_shrink([false, false])
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        for l in &self.lines {
                            line(ui, l);
                        }
                        if self.busy {
                            ui.spinner();
                        }
                    });
                ui.separator();
                ui.horizontal(|ui| {
                    let resp = ui.add_enabled(
                        self.ready && !self.busy,
                        egui::TextEdit::singleline(&mut self.input)
                            .hint_text("Schreib dem Assistenten …")
                            .desired_width(ui.available_width() - 70.0),
                    );
                    let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    if (ui
                        .add_enabled(self.ready && !self.busy, egui::Button::new("Senden"))
                        .clicked()
                        || enter)
                        && !self.input.trim().is_empty()
                    {
                        self.send();
                        resp.request_focus();
                    }
                });
            });
        if restart {
            self.start(cfg, data_dir, project);
        }
        if !open {
            self.open = false;
            // Fenster zu: Thread und Kind-Prozesse enden.
            self.worker = None;
        }
    }
}

fn line(ui: &mut egui::Ui, l: &Line) {
    match l {
        Line::You(t) => {
            ui.label(egui::RichText::new(format!("Du: {t}")).strong());
        }
        Line::Assistant(t) => {
            ui.label(t);
        }
        Line::Tool(t) => {
            ui.label(
                egui::RichText::new(format!("→ Werkzeug {t}"))
                    .small()
                    .monospace()
                    .color(theme::text_dim()),
            );
        }
        Line::Note(t) => {
            ui.label(egui::RichText::new(t).small().color(theme::text_dim()));
        }
        Line::Error(t) => {
            ui.label(egui::RichText::new(t).color(theme::on_air()));
        }
    }
    ui.add_space(4.0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn without_provider_it_says_what_is_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let ctx = theme::test_context();
        let mut c = ChatUi::default();
        c.open(&KiConfig::default(), tmp.path(), None);
        assert!(c.problem.is_some() && c.worker.is_none());
        let _ = ctx.run_ui(egui::RawInput::default(), |u| {
            c.show(u.ctx(), &KiConfig::default(), tmp.path(), None);
        });
        assert!(c.open);
    }

    #[test]
    fn provider_without_model_is_refused_before_connecting() {
        let tmp = tempfile::tempdir().unwrap();
        let mut cfg = KiConfig::default();
        cfg.providers.push(woa_assistant::ProviderConfig {
            id: "a1".into(),
            base_url: "http://x".into(),
            ..Default::default()
        });
        let mut c = ChatUi::default();
        c.start(&cfg, tmp.path(), None);
        assert!(c.problem.as_deref().unwrap().contains("Modell"));
    }
}
