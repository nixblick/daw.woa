//! Entwickler-Modus (André, 02.11.2026): Fenster „Abnahme“. Je Prüfpunkt steht, was zu tun ist
//! und was passieren soll, nach Ansicht gruppiert; Daumen hoch oder runter geht sofort als
//! Rückmeldung an das Projekt, damit Claude sie zeitnah auswertet.
//!
//! Inhalt: `docs/abnahme.json`, eingebettet als Rückfall und beim Öffnen im Hintergrund von
//! GitHub nachgeladen (wie `docs/fragen.json`). Gesendet wird über den Feedback-Weg mit Quelle
//! [`SOURCE`] und derselben Kennwort-Kopfzeile wie bei den Fragen.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::time::Duration;

use eframe::egui;
use serde::Deserialize;
use woa_telemetry::{DevKey, FeedbackError, FeedbackMeta, FeedbackType, submit_feedback};

use crate::feedback::GuiConfig;
use crate::fragen::{self, block_kopf, fenster, keine_treffer, kopf, passt, suchfeld};
use crate::theme;

/// Eingebettete Fassung (Rückfall).
pub const EMBEDDED: &str = include_str!("../../../docs/abnahme.json");
/// Aktuelle Fassung auf GitHub.
pub const REMOTE_URL: &str =
    "https://raw.githubusercontent.com/nixblick/daw.woa/main/docs/abnahme.json";
/// Quelle im Feedback.
pub const SOURCE: &str = "entwickler-abnahme";
/// Ansichten in der Reihenfolge der Oberfläche.
pub const ANSICHTEN: [&str; 8] = [
    "Radio",
    "Party Einfach",
    "Party DJ",
    "Produzent",
    "Piano-Roll",
    "Jingle-Assistent",
    "Einstellungen",
    "Allgemein",
];

/// Inhalt von `docs/abnahme.json`.
#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
pub struct Abnahme {
    #[serde(default)]
    pub stand: String,
    pub punkte: Vec<Punkt>,
}

/// Ein Prüfpunkt.
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct Punkt {
    pub id: String,
    pub ansicht: String,
    pub titel: String,
    /// Was tun.
    pub tun: String,
    /// Was soll passieren.
    pub erwartet: String,
    /// Version, mit der es kam.
    #[serde(default)]
    pub version: String,
    /// Pull Request oder Ticket.
    #[serde(default)]
    pub ticket: Option<u32>,
}

/// Liest und prüft eine Fassung: bekannte Ansichten, Titel und Schritte, eindeutige IDs.
pub fn parse(text: &str) -> Result<Abnahme, String> {
    let a: Abnahme = serde_json::from_str(text).map_err(|e| e.to_string())?;
    if a.punkte.is_empty() {
        return Err("keine Prüfpunkte".into());
    }
    let mut ids = HashSet::new();
    for p in &a.punkte {
        if !ANSICHTEN.contains(&p.ansicht.as_str()) {
            return Err(format!("{}: unbekannte Ansicht {}", p.id, p.ansicht));
        }
        if p.id.trim().is_empty() || p.titel.trim().is_empty() {
            return Err("Prüfpunkt ohne ID oder Titel".into());
        }
        if p.tun.trim().is_empty() || p.erwartet.trim().is_empty() {
            return Err(format!("{}: ohne Schritt oder Erwartung", p.id));
        }
        if !ids.insert(p.id.clone()) {
            return Err(format!("{}: doppelte ID", p.id));
        }
    }
    Ok(a)
}

/// Die eingebettete Fassung (im Test geprüft).
pub fn embedded() -> Abnahme {
    parse(EMBEDDED).unwrap_or_else(|e| {
        tracing::error!(error = %e, "embedded checklist invalid");
        Abnahme::default()
    })
}

/// Gültige Netzfassung, sonst die eingebettete; `true` = aus dem Netz.
pub fn from_fetch(result: Result<String, String>) -> (Abnahme, bool) {
    match result.and_then(|t| parse(&t)) {
        Ok(a) => (a, true),
        Err(e) => {
            tracing::info!(error = %e, "checklist: using embedded copy");
            (embedded(), false)
        }
    }
}

fn fetch_remote() -> Result<String, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(10)))
        .build()
        .into();
    agent
        .get(REMOTE_URL)
        .call()
        .map_err(|e| e.to_string())?
        .body_mut()
        .with_config()
        .limit(512 * 1024)
        .read_to_string()
        .map_err(|e| e.to_string())
}

/// Ergebnis eines Prüfpunkts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Urteil {
    Ok,
    Fehler,
    /// Nur eine Anmerkung, jederzeit, auch nach „Geht“ (André 02.11.2026).
    Anmerkung,
}

impl Urteil {
    pub fn wire(self) -> &'static str {
        match self {
            Urteil::Ok => "ok",
            Urteil::Fehler => "fehler",
            Urteil::Anmerkung => "anmerkung",
        }
    }
}

/// Text der Meldung: Titelzeile mit Quelle, ID und Urteil, dann die Einzelheiten.
pub fn feedback_text(p: &Punkt, urteil: Urteil, text: &str) -> String {
    let mut s = format!(
        "[{SOURCE}] {}: {}\n\nQuelle: {SOURCE}\nPrüfpunkt: {} ({})\nTun: {}\nErwartet: {}\nUrteil: {}",
        p.id,
        urteil.wire(),
        p.titel,
        p.ansicht,
        p.tun,
        p.erwartet,
        urteil.wire()
    );
    if !p.version.is_empty() {
        s.push_str(&format!("\nVersion: {}", p.version));
    }
    if let Some(n) = p.ticket {
        s.push_str(&format!("\nTicket: #{n}"));
    }
    let t = text.trim();
    if !t.is_empty() {
        s.push_str(&format!("\nText: {t}"));
    }
    s
}

/// Begleitdaten: ok = Lob, fehler = Bug, Quelle und Kennwort wie bei den Fragen.
pub fn meta(config: &GuiConfig, urteil: Urteil, dev_key: Option<String>) -> FeedbackMeta {
    FeedbackMeta {
        url: config.feedback_url.clone(),
        kind: match urteil {
            Urteil::Ok => FeedbackType::Lob,
            Urteil::Fehler => FeedbackType::Bug,
            Urteil::Anmerkung => FeedbackType::Allgemein,
        },
        install_id: config.install_id.clone(),
        source: Some(SOURCE.into()),
        dev_key: dev_key.map(DevKey),
        ..FeedbackMeta::default()
    }
}

/// Passt ein Prüfpunkt zur Suche?
pub fn punkt_passt(p: &Punkt, suche: &str) -> bool {
    let ticket = p.ticket.map(|n| format!("#{n} {n}")).unwrap_or_default();
    passt(
        suche,
        &[
            &p.titel,
            &p.tun,
            &p.erwartet,
            &p.ansicht,
            &p.version,
            &ticket,
        ],
    )
}

/// Was ein Klick auf 👎 auslöst: ohne Text erst nach dem Fehler fragen, sonst senden.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Daumen {
    Senden(Urteil),
    NachTextFragen,
}

/// 👍 sendet immer sofort; 👎 sofort mit Text, ohne Text beim ersten Klick nur das Textfeld,
/// beim zweiten Klick („ohne Text senden“) doch.
pub fn daumen(urteil: Urteil, text: &str, schon_gefragt: bool) -> Daumen {
    match urteil {
        Urteil::Fehler if text.trim().is_empty() && !schon_gefragt => Daumen::NachTextFragen,
        u => Daumen::Senden(u),
    }
}

/// Merkt „Geht“ je Prüfpunkt und Fassung: neue Version = neuer Schlüssel, Punkt erscheint wieder.
pub fn erledigt_key(p: &Punkt) -> String {
    format!("{}@{}", p.id, p.version)
}

/// Datei im Datenordner mit den auf diesem Gerät abgenommenen Punkten.
pub const ERLEDIGT_DATEI: &str = "abnahme-erledigt.json";

pub fn erledigt_laden(data_dir: &Path) -> HashSet<String> {
    std::fs::read_to_string(data_dir.join(ERLEDIGT_DATEI))
        .ok()
        .and_then(|t| serde_json::from_str::<Vec<String>>(&t).ok())
        .map(|v| v.into_iter().collect())
        .unwrap_or_default()
}

fn erledigt_speichern(data_dir: &Path, e: &HashSet<String>) {
    let mut v: Vec<&String> = e.iter().collect();
    v.sort();
    let r = serde_json::to_string_pretty(&v)
        .map_err(|e| e.to_string())
        .and_then(|t| {
            std::fs::create_dir_all(data_dir).map_err(|e| e.to_string())?;
            std::fs::write(data_dir.join(ERLEDIGT_DATEI), t).map_err(|e| e.to_string())
        });
    if let Err(e) = r {
        tracing::warn!(error = %e, "checklist: done list not saved");
    }
}

/// Antwort des Sende-Threads.
type Antwort = Receiver<Result<Option<u64>, FeedbackError>>;

#[derive(Debug, Default)]
struct Eingabe {
    text: String,
    gefragt: bool,
}

/// Fenster „Abnahme“.
#[derive(Default)]
pub struct AbnahmeWindow {
    pub open: bool,
    pub maximized: bool,
    pub suche: String,
    liste: Option<(Abnahme, bool)>,
    loader: Option<Receiver<Result<String, String>>>,
    eingaben: HashMap<String, Eingabe>,
    sent: HashMap<String, Urteil>,
    pending: HashMap<String, (Urteil, Antwort)>,
    notes: HashMap<String, String>,
    /// Auf diesem Gerät mit „Geht“ abgenommen (id@version), beim ersten Zeichnen geladen.
    erledigt: Option<HashSet<String>>,
    /// Schalter „Erledigte zeigen“.
    pub zeige_erledigte: bool,
}

impl AbnahmeWindow {
    /// Öffnet das Fenster; beim ersten Mal lädt ein Hintergrund-Thread die Netzfassung.
    pub fn open_window(&mut self, ctx: &egui::Context) {
        self.open = true;
        if self.liste.is_none() && self.loader.is_none() {
            let (tx, rx) = channel();
            let ctx = ctx.clone();
            let spawned = std::thread::Builder::new()
                .name("woa-abnahme".into())
                .spawn(move || {
                    let _ = tx.send(fetch_remote());
                    ctx.request_repaint();
                });
            match spawned {
                Ok(_) => self.loader = Some(rx),
                Err(e) => {
                    tracing::warn!(error = %e, "checklist loader thread failed");
                    self.liste = Some((embedded(), false));
                }
            }
        }
    }

    fn current(&self) -> (Abnahme, bool) {
        self.liste.clone().unwrap_or_else(|| (embedded(), false))
    }

    fn poll(&mut self, data_dir: &Path) {
        if let Some(rx) = &self.loader {
            match rx.try_recv() {
                Ok(r) => {
                    self.liste = Some(from_fetch(r));
                    self.loader = None;
                }
                Err(TryRecvError::Disconnected) => {
                    self.liste = Some(from_fetch(Err("abgebrochen".into())));
                    self.loader = None;
                }
                Err(TryRecvError::Empty) => {}
            }
        }
        let mut done = Vec::new();
        for (k, (u, rx)) in &self.pending {
            match rx.try_recv() {
                Ok(r) => done.push((k.clone(), *u, r)),
                Err(TryRecvError::Disconnected) => done.push((
                    k.clone(),
                    *u,
                    Err(FeedbackError::Transport("abgebrochen".into())),
                )),
                Err(TryRecvError::Empty) => {}
            }
        }
        for (k, u, r) in done {
            self.pending.remove(&k);
            let note = match r {
                Ok(n) => {
                    if u != Urteil::Anmerkung {
                        self.sent.insert(k.clone(), u);
                    }
                    if u == Urteil::Ok {
                        let (liste, _) = self.current();
                        if let Some(p) = liste.punkte.iter().find(|p| p.id == k) {
                            let e = self
                                .erledigt
                                .get_or_insert_with(|| erledigt_laden(data_dir));
                            e.insert(erledigt_key(p));
                            erledigt_speichern(data_dir, e);
                        }
                    }
                    let was = match u {
                        Urteil::Ok => "Abgenommen",
                        Urteil::Fehler => "Fehler gemeldet",
                        Urteil::Anmerkung => "Anmerkung gesendet",
                    };
                    match n {
                        Some(n) => format!("{was}, danke (#{n})"),
                        None => format!("{was}, danke"),
                    }
                }
                Err(FeedbackError::NoEndpoint) => crate::feedback::NOT_CONFIGURED.to_string(),
                Err(e) => {
                    tracing::warn!(error = %e, "checklist vote failed");
                    format!("Senden fehlgeschlagen: {e}")
                }
            };
            self.notes.insert(k, note);
        }
    }

    /// Ein Klick auf 👍 oder 👎.
    fn klick(&mut self, p: &Punkt, urteil: Urteil, config: &GuiConfig, data_dir: &Path) {
        if self.pending.contains_key(&p.id)
            || (urteil != Urteil::Anmerkung && self.sent.contains_key(&p.id))
        {
            return;
        }
        let e = self.eingaben.entry(p.id.clone()).or_default();
        if urteil == Urteil::Anmerkung && e.text.trim().is_empty() {
            return;
        }
        match daumen(urteil, &e.text, e.gefragt) {
            Daumen::NachTextFragen => {
                e.gefragt = true;
                self.notes.insert(
                    p.id.clone(),
                    "Was ist passiert? Kurz ins Feld schreiben und Enter, oder „Ohne Text senden“."
                        .into(),
                );
            }
            Daumen::Senden(u) => {
                let text = feedback_text(p, u, &e.text);
                if u == Urteil::Anmerkung {
                    e.text.clear();
                }
                e.gefragt = false;
                let config = config.clone();
                let data_dir: PathBuf = data_dir.to_path_buf();
                let (tx, rx) = channel();
                let spawned = std::thread::Builder::new()
                    .name("woa-abnahme-senden".into())
                    .spawn(move || {
                        let key = woa_secrets::reveal(&data_dir, fragen::SECRET_ACCOUNT);
                        let _ = tx.send(if !crate::feedback::may_send(&config.feedback_url) {
                            Err(FeedbackError::NoEndpoint)
                        } else {
                            submit_feedback(&text, Vec::new(), &meta(&config, u, key))
                        });
                    });
                match spawned {
                    Ok(_) => {
                        self.pending.insert(p.id.clone(), (u, rx));
                        self.notes.insert(p.id.clone(), "Wird gesendet".into());
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "checklist vote thread failed");
                        self.notes
                            .insert(p.id.clone(), format!("Senden fehlgeschlagen: {e}"));
                    }
                }
            }
        }
    }

    /// Zeichnet das Fenster.
    pub fn show(&mut self, ctx: &egui::Context, config: &GuiConfig, data_dir: &Path) {
        if !self.open {
            return;
        }
        if self.erledigt.is_none() {
            self.erledigt = Some(erledigt_laden(data_dir));
        }
        self.poll(data_dir);
        if !self.pending.is_empty() || self.loader.is_some() {
            ctx.request_repaint_after(Duration::from_millis(200));
        }
        let (liste, netz) = self.current();
        let fertig = liste
            .punkte
            .iter()
            .filter(|p| self.sent.contains_key(&p.id) || self.ist_erledigt(p))
            .count();
        let sub = format!(
            "Stand {}{} · {fertig} von {} geprüft · „Geht“ und „Geht nicht“ gehen sofort an das Projekt.",
            liste.stand,
            if netz {
                ""
            } else {
                " (Fassung dieser Version)"
            },
            liste.punkte.len()
        );
        fenster(ctx, "woa-abnahme", self.maximized).show(ctx, |ui| {
            kopf(
                ui,
                theme::ic::LIST_CHECKS,
                "Abnahme",
                &sub,
                &mut self.open,
                &mut self.maximized,
            );
            suchfeld(ui, &mut self.suche);
            ui.checkbox(&mut self.zeige_erledigte, "Erledigte zeigen");
            let suche = self.suche.clone();
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let mut any = false;
                    for ansicht in ANSICHTEN {
                        let ps: Vec<&Punkt> = liste
                            .punkte
                            .iter()
                            .filter(|p| {
                                p.ansicht == ansicht
                                    && punkt_passt(p, &suche)
                                    && (self.zeige_erledigte
                                        || !self.ist_erledigt(p)
                                        || self.sent.contains_key(&p.id))
                            })
                            .collect();
                        if ps.is_empty() {
                            continue;
                        }
                        any = true;
                        block_kopf(ui, ansicht, ps.len());
                        for p in ps {
                            self.punkt_ui(ui, p, config, data_dir);
                            ui.add_space(theme::SPACE_3);
                        }
                    }
                    if !any {
                        keine_treffer(ui);
                    }
                });
        });
    }

    /// Auf diesem Gerät in dieser Fassung schon abgenommen?
    fn ist_erledigt(&self, p: &Punkt) -> bool {
        self.erledigt
            .as_ref()
            .is_some_and(|e| e.contains(&erledigt_key(p)))
    }

    fn punkt_ui(&mut self, ui: &mut egui::Ui, p: &Punkt, config: &GuiConfig, data_dir: &Path) {
        use crate::widgets::segment::Segment;
        let sent = self.sent.get(&p.id).copied();
        let busy = self.pending.contains_key(&p.id);
        let mut frame = theme::card_frame();
        if let Some(u) = sent {
            let c = match u {
                Urteil::Ok => theme::ok(),
                Urteil::Fehler | Urteil::Anmerkung => theme::danger_text(),
            };
            frame = frame.stroke(egui::Stroke::new(1.0, c));
        }
        frame.show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    egui::RichText::new(&p.titel)
                        .size(15.0)
                        .strong()
                        .color(theme::text()),
                );
                if !p.version.is_empty() {
                    theme::badge_pill(ui, &p.version, theme::Badge::Neutral);
                }
                if let Some(n) = p.ticket {
                    fragen::ticket_pill(ui, n);
                }
            });
            ui.add_space(theme::SPACE_2);
            for (marke, text) in [("TUN", &p.tun), ("SOLL", &p.erwartet)] {
                ui.horizontal_wrapped(|ui| {
                    ui.add_sized([44.0, 16.0], egui::Label::new(theme::panel_label(marke)));
                    ui.label(egui::RichText::new(text).color(theme::text()));
                });
            }
            ui.add_space(theme::SPACE_3);
            let mut klick = None;
            ui.add_enabled_ui(!busy, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    let e = self.eingaben.entry(p.id.clone()).or_default();
                    let offen = sent.is_none();
                    if Segment::new(theme::ic::THUMBS_UP, "Geht")
                        .on(sent == Some(Urteil::Ok))
                        .height(28.0)
                        .ui(ui)
                        .on_hover_text("Abgenommen, sofort senden")
                        .clicked()
                        && offen
                    {
                        klick = Some(Urteil::Ok);
                    }
                    let runter = if e.gefragt && e.text.trim().is_empty() {
                        "Ohne Text senden"
                    } else {
                        "Geht nicht"
                    };
                    if Segment::new(theme::ic::THUMBS_DOWN, runter)
                        .on(sent == Some(Urteil::Fehler) || e.gefragt)
                        .height(28.0)
                        .ui(ui)
                        .on_hover_text("Fehler melden, sofort senden")
                        .clicked()
                        && offen
                    {
                        klick = Some(Urteil::Fehler);
                    }
                    ui.add_space(theme::SPACE_3);
                    let hint = if e.gefragt {
                        "Was ist passiert?"
                    } else if offen {
                        "Anmerkung (optional)"
                    } else {
                        "Anmerkung nachschicken"
                    };
                    let r = ui.add(
                        egui::TextEdit::singleline(&mut e.text)
                            .hint_text(hint)
                            .desired_width((ui.available_width() - 170.0).max(160.0)),
                    );
                    r.widget_info(|| {
                        egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, "Anmerkung")
                    });
                    if e.gefragt && !r.has_focus() && e.text.is_empty() {
                        r.request_focus();
                    }
                    if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) && e.gefragt
                    {
                        klick = Some(Urteil::Fehler);
                    }
                    let can = !e.text.trim().is_empty() && !e.gefragt;
                    if ui
                        .add_enabled(
                            can,
                            egui::Button::new(format!(
                                "{} Anmerkung senden",
                                theme::ic::PAPER_PLANE_RIGHT
                            ))
                            .min_size(egui::vec2(150.0, 28.0)),
                        )
                        .on_hover_text("Nur die Anmerkung senden, ohne Urteil")
                        .clicked()
                    {
                        klick = Some(Urteil::Anmerkung);
                    }
                });
            });
            if let Some(u) = klick {
                self.klick(p, u, config, data_dir);
            }
            if let Some(n) = self.notes.get(&p.id) {
                ui.add_space(theme::SPACE_2);
                let (icon, color) = match sent {
                    _ if n.starts_with("Anmerkung gesendet") => {
                        (theme::ic::CHECK_CIRCLE, theme::ok())
                    }
                    Some(Urteil::Ok) => (theme::ic::CHECK_CIRCLE, theme::ok()),
                    Some(Urteil::Anmerkung) => (theme::ic::INFO, theme::text_dim()),
                    Some(Urteil::Fehler) => (theme::ic::BUG, theme::danger_text()),
                    None if n.starts_with("Senden fehlgeschlagen") => {
                        (theme::ic::WARNING, theme::danger_text())
                    }
                    None => (theme::ic::INFO, theme::text_dim()),
                };
                ui.label(egui::RichText::new(format!("{icon} {n}")).color(color));
            }
        });
    }
}

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)]
mod tests {
    use super::*;
    use egui_kittest::kittest::Queryable;

    #[test]
    fn embedded_checklist_parses_and_covers_the_views() {
        let a = parse(EMBEDDED).expect("docs/abnahme.json gültig");
        assert!(a.punkte.len() >= 20);
        for v in ["Radio", "Party DJ", "Produzent", "Allgemein"] {
            assert!(a.punkte.iter().any(|p| p.ansicht == v), "Ansicht {v} leer");
        }
    }

    #[test]
    fn embedded_checklist_holds_nothing_internal() {
        let low = EMBEDDED.to_lowercase();
        for bad in [
            "token",
            "/home",
            "~/",
            "192.168",
            ".secrets",
            "passwort",
            "kennwort:",
            "ssh ",
            "github_pat",
            "lfs0",
            "nano",
            "claude",
            "crates/",
            "::",
        ] {
            assert!(!low.contains(bad), "{bad} in docs/abnahme.json");
        }
    }

    #[test]
    fn unknown_view_or_missing_step_is_refused() {
        assert!(
            parse(
                r#"{"punkte":[{"id":"a","ansicht":"Kino","titel":"T","tun":"x","erwartet":"y"}]}"#
            )
            .is_err()
        );
        assert!(
            parse(
                r#"{"punkte":[{"id":"a","ansicht":"Radio","titel":"T","tun":"","erwartet":"y"}]}"#
            )
            .is_err()
        );
        let (_, netz) = from_fetch(Ok("<html>".into()));
        assert!(!netz);
    }

    #[test]
    fn payload_is_marked_as_developer_checklist() {
        let a = embedded();
        let p = &a.punkte[0];
        let cfg = GuiConfig::default();
        let text = feedback_text(p, Urteil::Fehler, "hängt");
        let pl =
            woa_telemetry::build_payload(&text, &[], &meta(&cfg, Urteil::Fehler, Some("x".into())))
                .unwrap();
        let title = pl["title"].as_str().unwrap();
        assert!(
            title.starts_with(&format!("[entwickler-abnahme] {}: fehler", p.id)),
            "{title}"
        );
        assert_eq!(pl["source"], SOURCE);
        assert_eq!(pl["type"], "bug");
        let t = feedback_text(p, Urteil::Ok, "");
        assert!(t.starts_with(&format!("[entwickler-abnahme] {}: ok", p.id)));
        let pl = woa_telemetry::build_payload(&t, &[], &meta(&cfg, Urteil::Ok, None)).unwrap();
        assert_eq!(pl["type"], "lob");
    }

    #[test]
    fn thumbs_down_without_text_asks_first_comment_does_not_choose() {
        assert_eq!(daumen(Urteil::Ok, "", false), Daumen::Senden(Urteil::Ok));
        assert_eq!(daumen(Urteil::Fehler, "", false), Daumen::NachTextFragen);
        assert_eq!(
            daumen(Urteil::Fehler, "", true),
            Daumen::Senden(Urteil::Fehler)
        );
        assert_eq!(
            daumen(Urteil::Fehler, "stockt", false),
            Daumen::Senden(Urteil::Fehler)
        );
    }

    #[test]
    fn search_filters_points() {
        let a = embedded();
        let p = &a.punkte[0];
        assert!(punkt_passt(p, ""));
        assert!(punkt_passt(p, &p.titel.to_uppercase()));
        assert!(!punkt_passt(p, "xyzzy-gibt-es-nicht"));
    }

    fn harness(w: AbnahmeWindow) -> egui_kittest::Harness<'static, AbnahmeWindow> {
        let cfg = GuiConfig::default();
        let dir = std::env::temp_dir();
        let mut h = egui_kittest::Harness::builder()
            .with_size(egui::vec2(1400.0, 900.0))
            .build_ui_state(
                move |ui, w: &mut AbnahmeWindow| {
                    if !theme::install_fonts(ui.ctx()) {
                        w.show(ui.ctx(), &cfg, &dir);
                    }
                },
                w,
            );
        h.run_steps(3);
        h
    }

    #[test]
    fn window_has_search_maximize_and_typing_does_not_vote() {
        let mut w = AbnahmeWindow::default();
        w.open = true;
        w.liste = Some((embedded(), false));
        let mut h = harness(w);
        h.get_by_label(theme::ic::CORNERS_OUT);
        h.get_by_label(theme::ic::X);
        let n = h.get_all_by_label("Anmerkung").count();
        assert!(n >= 20);
        h.get_all_by_label("Anmerkung").next().unwrap().click();
        h.run_steps(1);
        h.get_all_by_label("Anmerkung")
            .next()
            .unwrap()
            .type_text("x");
        h.run_steps(2);
        assert!(h.state().pending.is_empty() && h.state().sent.is_empty());
        h.get_by_label("Suchen").click();
        h.run_steps(1);
        h.get_by_label("Suchen").type_text("xyzzy-gibt-es-nicht");
        h.run_steps(2);
        assert_eq!(h.query_all_by_label("Anmerkung").count(), 0);
        assert!(h.query_by_label("Nichts gefunden").is_some());
    }

    /// André 02.11.2026: Nach „Geht“ muss eine Anmerkung zum Punkt noch rausgehen.
    #[test]
    fn note_payload_is_marked_anmerkung() {
        let a = embedded();
        let p = &a.punkte[0];
        let t = feedback_text(p, Urteil::Anmerkung, "lieber maximiert");
        assert!(t.starts_with(&format!("[entwickler-abnahme] {}: anmerkung", p.id)));
        assert!(t.contains("Text: lieber maximiert"));
        let pl = woa_telemetry::build_payload(
            &t,
            &[],
            &meta(&GuiConfig::default(), Urteil::Anmerkung, None),
        )
        .unwrap();
        assert_eq!(pl["source"], SOURCE);
    }

    #[test]
    fn note_can_be_sent_after_thumbs_up() {
        let mut w = AbnahmeWindow::default();
        w.open = true;
        let a = embedded();
        // Der oberste Punkt im Fenster: erste Ansicht mit Punkten.
        let id = ANSICHTEN
            .iter()
            .find_map(|v| a.punkte.iter().find(|p| p.ansicht == *v))
            .unwrap()
            .id
            .clone();
        w.liste = Some((a, false));
        w.sent.insert(id.clone(), Urteil::Ok);
        let mut h = harness(w);
        h.get_all_by_label("Anmerkung").next().unwrap().click();
        h.run_steps(1);
        h.get_all_by_label("Anmerkung")
            .next()
            .unwrap()
            .type_text("maximiert");
        h.run_steps(2);
        assert_eq!(
            h.state().eingaben[&id].text,
            "maximiert",
            "Feld bleibt offen"
        );
        h.get_all_by_label_contains("Anmerkung senden")
            .next()
            .unwrap()
            .click();
        h.run_steps(2);
        assert!(h.state().eingaben[&id].text.is_empty(), "Text ging raus");
        assert!(h.state().notes.contains_key(&id));
        assert_eq!(h.state().sent.get(&id), Some(&Urteil::Ok), "Urteil bleibt");
    }

    /// André 02.11.2026: mit „Geht“ abgenommene Punkte verschwinden, neue Fassung kommt wieder.
    #[test]
    fn approved_points_hide_until_a_new_version() {
        let dir = tempfile::tempdir().unwrap();
        let a = embedded();
        let mut p = a.punkte[0].clone();
        let (tx, rx) = channel();
        let mut w = AbnahmeWindow::default();
        w.liste = Some((a.clone(), false));
        w.pending.insert(p.id.clone(), (Urteil::Ok, rx));
        tx.send(Ok(None)).unwrap();
        w.poll(dir.path());
        assert!(erledigt_laden(dir.path()).contains(&erledigt_key(&p)));
        let w2 = AbnahmeWindow {
            erledigt: Some(erledigt_laden(dir.path())),
            ..Default::default()
        };
        assert!(w2.ist_erledigt(&p));
        p.version = "9.9.9".into();
        assert!(!w2.ist_erledigt(&p), "neue Fassung erscheint wieder");
    }
}
