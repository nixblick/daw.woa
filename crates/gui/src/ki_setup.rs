//! Seite „KI einrichten“ (#217): ein Dropdown „Anbieter“, darunter nur die Felder, die
//! dieser Anbieter braucht. Für Claude Desktop / Claude Code gibt es den fertigen Eintrag
//! zum Kopieren samt drei Sätzen Anleitung; für API-Anbieter Modell, Adresse und Schlüssel.
//!
//! Der Schlüsselbund wird nie im UI-Thread gelesen oder beschrieben: Status, Speichern und
//! „Modelle holen“ laufen in kurzen Hintergrund-Threads, die Antwort kommt über einen Kanal.
//! Der Schlüssel steht nie im Klartext im Fenster und nie im Log.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};

use eframe::egui;
use woa_assistant::server::{self, Transport};
use woa_assistant::{KiConfig, McpClient, Pick, ProviderConfig};
use woa_secrets::Ort;

use crate::theme;

/// Fragt, ob unter einem Eintrag ein Geheimnis liegt (Vorgabe: der Schlüsselbund).
pub type Lookup = fn(&Path, &str) -> Option<Ort>;
/// Legt ein Geheimnis ab (Vorgabe: der Schlüsselbund).
pub type Saver = fn(&Path, &str, &str) -> Result<Ort, String>;

/// Index der Voreinstellungen in [`ProviderConfig::presets`], die ohne Schlüssel auskommen.
const LOCAL_PRESETS: [usize; 2] = [3, 4];

/// Ob unter einem Eintrag ein Schlüssel liegt, im Hintergrund nachgesehen und gemerkt.
pub struct KeyStatus {
    known: HashMap<String, Option<Ort>>,
    asked: HashSet<String>,
    tx: Sender<(String, Option<Ort>)>,
    rx: Receiver<(String, Option<Ort>)>,
    lookup: Lookup,
}

impl Default for KeyStatus {
    fn default() -> Self {
        Self::with_lookup(woa_secrets::stored)
    }
}

impl KeyStatus {
    pub fn with_lookup(lookup: Lookup) -> Self {
        let (tx, rx) = channel();
        Self {
            known: HashMap::new(),
            asked: HashSet::new(),
            tx,
            rx,
            lookup,
        }
    }

    /// `None`: wird gerade nachgesehen. Sonst: liegt einer, und wo.
    pub fn get(&mut self, data_dir: &Path, account: &str) -> Option<Option<Ort>> {
        while let Ok((a, v)) = self.rx.try_recv() {
            self.known.insert(a, v);
        }
        if let Some(v) = self.known.get(account) {
            return Some(*v);
        }
        if self.asked.insert(account.to_string()) {
            let (tx, dir, acc, lookup) = (
                self.tx.clone(),
                data_dir.to_path_buf(),
                account.to_string(),
                self.lookup,
            );
            std::thread::spawn(move || {
                let v = lookup(&dir, &acc);
                let _ = tx.send((acc, v));
            });
        }
        None
    }

    pub fn set(&mut self, account: &str, v: Option<Ort>) {
        self.known.insert(account.to_string(), v);
    }
}

enum Msg {
    Saved(String, Result<Ort, String>),
    Models(String, Result<Vec<String>, String>),
}

pub struct KiSetup {
    /// Neuer Schlüssel im Eingabefeld (maskiert); leer = unverändert.
    key_input: String,
    keys: KeyStatus,
    models: HashMap<String, Vec<String>>,
    busy: bool,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    saver: Saver,
    note: Option<String>,
}

impl Default for KiSetup {
    fn default() -> Self {
        Self::with_store(woa_secrets::stored, woa_secrets::save)
    }
}

/// Text im Dropdown „Anbieter“.
pub fn pick_label(p: Pick) -> String {
    match p {
        Pick::Mcp(_) => "Claude Desktop / Claude Code (MCP)".into(),
        Pick::Preset(0) => "Anthropic API (eigener Schlüssel)".into(),
        Pick::Preset(3) => "Ollama lokal".into(),
        Pick::Preset(i) => ProviderConfig::presets()
            .get(i)
            .map_or_else(String::new, |(n, _, _)| format!("{n} (API)")),
    }
}

/// Reihenfolge im Dropdown: was die meisten brauchen, zuerst.
const ORDER: [Pick; 6] = [
    Pick::Mcp(McpClient::ClaudeDesktop),
    Pick::Preset(0),
    Pick::Preset(3),
    Pick::Preset(1),
    Pick::Preset(2),
    Pick::Preset(4),
];

/// Genau der Text, den „Kopieren“ für `client` in die Zwischenablage legt: für Claude Code
/// nur der Befehl `claude mcp add …`, sonst nur der Eintrag `"woa": {…}` (ohne `mcpServers`).
/// Ohne gewählten Projektordner wird das offene Projekt eingesetzt. `Err`: der Hinweis, warum
/// es keinen Text gibt (`woa-headless` fehlt, kein Projekt gespeichert).
pub fn mcp_block(
    cfg: &KiConfig,
    client: McpClient,
    exe: Option<&Path>,
    project: Option<&Path>,
) -> Result<String, String> {
    let Some(exe) = exe else {
        return Err(
            "woa-headless liegt nicht neben WOA. Installiere WOA vollständig, \
                    dann steht hier der Eintrag."
                .into(),
        );
    };
    let mut c = cfg.mcp_server.clone();
    if c.project.is_none() {
        c.project = project.map(Path::to_path_buf);
    }
    if c.project.is_none() {
        return Err(
            "Speichere zuerst dein Projekt (Datei → Speichern), dann steht hier der \
                    Eintrag mit seinem Ordner."
                .into(),
        );
    }
    let text = server::snippets(&c, Some(exe), "")
        .into_iter()
        .find(|s| s.transport == Transport::Stdio && s.client == client.snippet_client())
        .map(|s| s.masked())
        .ok_or_else(|| {
            "Der Pfad enthält Zeichen, die kein Terminal-Befehl verträgt.".to_string()
        })?;
    if client == McpClient::ClaudeCode {
        return Ok(text);
    }
    let v: serde_json::Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    let inner = serde_json::to_string_pretty(&v["mcpServers"]["woa"]).map_err(|e| e.to_string())?;
    Ok(format!("\"woa\": {inner}"))
}

/// Die drei Sätze Anleitung je Client.
pub fn mcp_howto(client: McpClient) -> &'static str {
    match client {
        McpClient::ClaudeDesktop => {
            "1. „Kopieren“ drücken und den Eintrag in Claude Desktop unter Einstellungen → \
             Entwickler → „Konfiguration bearbeiten“ (claude_desktop_config.json) innerhalb \
             von „mcpServers“ einfügen (fehlt es, „mcpServers“: { … } darum legen). 2. Claude Desktop \
             neu starten, WOA erscheint im Werkzeug-Menü. 3. Solange WOA offen ist, arbeitet \
             Claude live in deinem offenen Projekt, z. B. „Leg eine Bassdrum auf jede Zählzeit“."
        }
        McpClient::ClaudeCode => {
            "1. „Kopieren“ drücken und den Befehl einmal im Terminal ausführen. 2. In Claude \
             Code zeigt /mcp danach „woa“ als verbunden. 3. Solange WOA offen ist, arbeitet \
             Claude live in deinem offenen Projekt."
        }
        McpClient::Other => {
            "1. „Kopieren“ drücken und den Eintrag in die MCP-Einstellungen deines Clients \
             innerhalb von „mcpServers“ einfügen. 2. Den Client neu starten. 3. Solange WOA offen ist, arbeiten die \
             Werkzeuge live in deinem offenen Projekt."
        }
    }
}

impl KiSetup {
    pub fn with_store(lookup: Lookup, saver: Saver) -> Self {
        let (tx, rx) = channel();
        Self {
            key_input: String::new(),
            keys: KeyStatus::with_lookup(lookup),
            models: HashMap::new(),
            busy: false,
            tx,
            rx,
            saver,
            note: None,
        }
    }

    fn drain(&mut self) {
        while let Ok(m) = self.rx.try_recv() {
            self.busy = false;
            match m {
                Msg::Saved(acc, Ok(ort)) => {
                    self.keys.set(&acc, Some(ort));
                    self.note = Some("Schlüssel gespeichert.".into());
                }
                Msg::Saved(_, Err(e)) => {
                    self.note = Some(format!("Schlüssel nicht gespeichert: {e}"));
                }
                Msg::Models(id, Ok(list)) => {
                    self.note = Some(if list.is_empty() {
                        "Der Anbieter nennt keine Modelle; trag den Namen von Hand ein.".into()
                    } else {
                        format!("{} Modelle geholt.", list.len())
                    });
                    self.models.insert(id, list);
                }
                Msg::Models(_, Err(e)) => self.note = Some(format!("Keine Modelle: {e}")),
            }
        }
    }

    /// Legt den eingetippten Schlüssel im Hintergrund ab; das Feld ist danach leer.
    fn save_key(&mut self, data_dir: &Path, account: &str) {
        let key = std::mem::take(&mut self.key_input);
        if key.trim().is_empty() {
            return;
        }
        let (tx, dir, acc, saver) = (
            self.tx.clone(),
            data_dir.to_path_buf(),
            account.to_string(),
            self.saver,
        );
        self.busy = true;
        std::thread::spawn(move || {
            let r = saver(&dir, &acc, key.trim());
            if let Err(e) = &r {
                tracing::warn!(account = %acc, error = %e, "ki key not stored");
            } else {
                tracing::info!(account = %acc, "ki key stored");
            }
            let _ = tx.send(Msg::Saved(acc, r));
        });
    }

    fn fetch_models(&mut self, p: &ProviderConfig, data_dir: &Path) {
        let (tx, cfg, dir) = (self.tx.clone(), p.clone(), data_dir.to_path_buf());
        self.busy = true;
        std::thread::spawn(move || {
            let r = cfg.provider(&dir).models();
            let _ = tx.send(Msg::Models(cfg.id, r));
        });
    }

    /// Zeichnet die Seite. `true`: die Wahl hat sich geändert und soll gespeichert werden.
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        draft: &mut KiConfig,
        data_dir: &Path,
        project: Option<&Path>,
        exe: Option<PathBuf>,
    ) -> bool {
        self.drain();
        if self.busy {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(150));
        }
        let before = draft.clone();
        ui.label(
            egui::RichText::new(
                "Zwei Wege: Claude Desktop oder Claude Code bedienen WOA von außen (MCP). Oder \
                 der KI-Chat im Produzenten-Modus (Studio-Dock unten) spricht mit einem \
                 Sprachmodell deiner Wahl.",
            )
            .small()
            .color(theme::text_dim()),
        );
        ui.add_space(6.0);
        let current = draft.pick();
        egui::Grid::new("ki-setup-grid")
            .num_columns(2)
            .spacing([12.0, 8.0])
            .show(ui, |ui| {
                ui.label("Anbieter");
                let text = match current {
                    Some(p) => pick_label(p),
                    None => draft.active_provider().map_or_else(
                        || "bitte wählen".to_string(),
                        |p| format!("{} (unter „Erweitert“)", p.name),
                    ),
                };
                egui::ComboBox::from_id_salt("ki-setup-provider")
                    .selected_text(text)
                    .width(300.0)
                    .show_ui(ui, |ui| {
                        for p in ORDER {
                            let on = match (p, current) {
                                (Pick::Mcp(_), Some(Pick::Mcp(_))) => true,
                                _ => current == Some(p),
                            };
                            if ui.selectable_label(on, pick_label(p)).clicked() && !on {
                                draft.choose(p);
                            }
                        }
                        ui.add_enabled(
                            false,
                            egui::Button::selectable(
                                false,
                                "Claude-Abo direkt im Chat (kommt später)",
                            ),
                        )
                        .on_disabled_hover_text(
                            "Dein Claude-Abo gilt in Claude Desktop und Claude Code. Nimm dafür \
                             den ersten Eintrag: Claude bedient dann WOA.",
                        );
                    });
                ui.end_row();
                match draft.pick() {
                    Some(Pick::Mcp(client)) => {
                        ui.label("Client");
                        let mut c = client;
                        egui::ComboBox::from_id_salt("ki-setup-client")
                            .selected_text(c.label())
                            .show_ui(ui, |ui| {
                                for x in McpClient::ALL {
                                    ui.selectable_value(&mut c, x, x.label());
                                }
                            });
                        if c != client {
                            draft.choose(Pick::Mcp(c));
                        }
                        ui.end_row();
                    }
                    _ => self.provider_fields(ui, draft, data_dir),
                }
            });
        if let Some(Pick::Mcp(client)) = draft.pick() {
            ui.add_space(6.0);
            self.mcp_card(ui, draft, client, exe.as_deref(), project);
        }
        if let Some(n) = &self.note {
            ui.label(egui::RichText::new(n).small().color(theme::text_dim()));
        }
        *draft != before
    }

    fn provider_fields(&mut self, ui: &mut egui::Ui, draft: &mut KiConfig, data_dir: &Path) {
        let Some(idx) = draft
            .active_provider()
            .and_then(|a| draft.providers.iter().position(|p| p.id == a.id))
        else {
            return;
        };
        let local = matches!(draft.pick(), Some(Pick::Preset(i)) if LOCAL_PRESETS.contains(&i));
        let p = &mut draft.providers[idx];
        ui.label("Modell");
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut p.model)
                    .hint_text(match p.kind {
                        woa_assistant::llm::Kind::Anthropic => "z. B. claude-sonnet-4-5",
                        woa_assistant::llm::Kind::OpenAiCompat => "z. B. llama3.2",
                    })
                    .desired_width(220.0),
            );
            if let Some(list) = self.models.get(&p.id) {
                egui::ComboBox::from_id_salt("ki-setup-models")
                    .selected_text("Liste")
                    .show_ui(ui, |ui| {
                        for m in list {
                            if ui.selectable_label(&p.model == m, m).clicked() {
                                p.model.clone_from(m);
                            }
                        }
                    });
            }
            if ui
                .add_enabled(!self.busy, egui::Button::new("Modelle holen"))
                .clicked()
            {
                let cfg = p.clone();
                self.fetch_models(&cfg, data_dir);
            }
            if self.busy {
                ui.spinner();
            }
        });
        ui.end_row();
        if !local {
            let account = p.secret_account();
            ui.label("Schlüssel");
            ui.horizontal(|ui| {
                let hint = match self.keys.get(data_dir, &account) {
                    None => "wird nachgesehen …".to_string(),
                    Some(Some(o)) => format!("gespeichert {}", o.label()),
                    Some(None) => "API-Schlüssel einfügen".to_string(),
                };
                ui.add(
                    egui::TextEdit::singleline(&mut self.key_input)
                        .password(true)
                        .hint_text(hint)
                        .desired_width(220.0),
                );
                if ui
                    .add_enabled(
                        !self.key_input.trim().is_empty() && !self.busy,
                        egui::Button::new("Speichern"),
                    )
                    .clicked()
                {
                    self.save_key(data_dir, &account);
                }
            });
            ui.end_row();
        }
        ui.label("Adresse");
        ui.add(egui::TextEdit::singleline(&mut p.base_url).desired_width(300.0));
        ui.end_row();
    }

    fn mcp_card(
        &mut self,
        ui: &mut egui::Ui,
        draft: &KiConfig,
        client: McpClient,
        exe: Option<&Path>,
        project: Option<&Path>,
    ) {
        theme::card_frame().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(egui::RichText::new(mcp_howto(client)).small());
            ui.add_space(4.0);
            match mcp_block(draft, client, exe, project) {
                Ok(block) => {
                    let label = if client == McpClient::ClaudeCode {
                        "Befehl kopieren"
                    } else {
                        "JSON kopieren"
                    };
                    if ui.button(label).clicked() {
                        ui.ctx().copy_text(block.clone());
                        self.note = Some("In der Zwischenablage.".into());
                    }
                    egui::CollapsingHeader::new("Inhalt ansehen")
                        .id_salt("ki-setup-block")
                        .show(ui, |ui| {
                            ui.label(egui::RichText::new(block).monospace().small());
                        });
                }
                Err(hint) => {
                    ui.label(egui::RichText::new(hint).small().color(theme::warn()));
                }
            }
        });
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    fn none(_: &Path, _: &str) -> Option<Ort> {
        None
    }
    fn fake_save(_: &Path, _: &str, _: &str) -> Result<Ort, String> {
        Ok(Ort::Datei)
    }

    #[test]
    fn copied_text_has_only_the_woa_entry_and_the_open_project() {
        let cfg = KiConfig::default();
        let exe = Path::new("/opt/wir on air/woa-headless");
        let proj = Path::new("/tmp/mein projekt.woa");
        for c in [McpClient::ClaudeDesktop, McpClient::Other] {
            let b = mcp_block(&cfg, c, Some(exe), Some(proj)).unwrap();
            assert!(!b.contains("mcpServers"), "{b}");
            assert!(b.trim_start().starts_with("\"woa\": {"), "{b}");
            let v: serde_json::Value =
                serde_json::from_str(&format!("{{{b}}}")).expect("gültiges JSON");
            assert_eq!(v["woa"]["command"], "/opt/wir on air/woa-headless");
            assert_eq!(v["woa"]["args"][1], "/tmp/mein projekt.woa");
        }
        let cmd = mcp_block(&cfg, McpClient::ClaudeCode, Some(exe), Some(proj)).unwrap();
        assert!(cmd.starts_with("claude mcp add woa -- "));
        assert!(!cmd.contains('{'));
        assert!(mcp_block(&cfg, McpClient::ClaudeDesktop, None, Some(proj)).is_err());
    }

    #[test]
    fn without_a_project_there_is_a_hint_and_no_angle_brackets() {
        let cfg = KiConfig::default();
        let exe = Path::new("/opt/woa/woa-headless");
        for c in [
            McpClient::ClaudeDesktop,
            McpClient::ClaudeCode,
            McpClient::Other,
        ] {
            let e = mcp_block(&cfg, c, Some(exe), None).unwrap_err();
            assert!(e.contains("Projekt"), "{e}");
            assert!(!e.contains('<') && !e.contains('>'), "{e}");
        }
    }

    #[test]
    fn page_draws_for_every_choice_without_touching_the_keyring() {
        let tmp = tempfile::tempdir().unwrap();
        let ctx = theme::test_context();
        let mut s = KiSetup::with_store(none, fake_save);
        let mut cfg = KiConfig::default();
        for p in ORDER {
            cfg.choose(p);
            let _ = ctx.run_ui(egui::RawInput::default(), |u| {
                s.show(
                    u,
                    &mut cfg,
                    tmp.path(),
                    None,
                    Some(PathBuf::from("/x/woa-headless")),
                );
            });
        }
    }

    /// Fängt alles, was über `tracing` geloggt wird, als Text.
    pub(crate) struct Capture(pub Arc<Mutex<String>>);

    struct V<'a>(&'a mut String);
    impl tracing::field::Visit for V<'_> {
        fn record_debug(&mut self, f: &tracing::field::Field, v: &dyn std::fmt::Debug) {
            self.0.push_str(&format!(" {}={v:?}", f.name()));
        }
    }

    impl tracing::Subscriber for Capture {
        fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
            true
        }
        fn new_span(&self, a: &tracing::span::Attributes<'_>) -> tracing::span::Id {
            a.record(&mut V(&mut self.0.lock().unwrap()));
            tracing::span::Id::from_u64(1)
        }
        fn record(&self, _: &tracing::span::Id, r: &tracing::span::Record<'_>) {
            r.record(&mut V(&mut self.0.lock().unwrap()));
        }
        fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
        fn event(&self, e: &tracing::Event<'_>) {
            e.record(&mut V(&mut self.0.lock().unwrap()));
        }
        fn enter(&self, _: &tracing::span::Id) {}
        fn exit(&self, _: &tracing::span::Id) {}
    }

    #[test]
    fn saving_a_key_logs_no_key() {
        const KEY: &str = "sk-ant-fake-0123456789";
        let log = Arc::new(Mutex::new(String::new()));
        let dispatch = tracing::Dispatch::new(Capture(log.clone()));
        // Global, weil das Speichern im Hintergrund-Thread loggt.
        let _ = tracing::dispatcher::set_global_default(dispatch);
        let tmp = tempfile::tempdir().unwrap();
        let mut s = KiSetup::with_store(none, fake_save);
        s.key_input = KEY.into();
        s.save_key(tmp.path(), "ki-ab12");
        assert!(s.key_input.is_empty());
        let t0 = std::time::Instant::now();
        while !s.keys.known.contains_key("ki-ab12") && t0.elapsed().as_secs() < 5 {
            std::thread::sleep(std::time::Duration::from_millis(10));
            s.drain();
        }
        assert_eq!(s.keys.known.get("ki-ab12"), Some(&Some(Ort::Datei)));
        let text = log.lock().unwrap().clone();
        assert!(text.contains("ki key stored"), "Log: {text}");
        assert!(!text.contains(KEY) && !text.contains("0123456789"));
    }
}
