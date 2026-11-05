//! Entwickler-Modus (André, 02.11.2026): Fenster „Fragen und To-dos“ mit den inhaltlichen
//! Entscheidungsfragen je Oberfläche, Abstimmung je Vorschlag und Freischaltung per Kennwort.
//!
//! Inhalt: `docs/fragen.json`, eingebettet als Rückfall und zur Laufzeit im Hintergrund von
//! GitHub nachgeladen (neue Fragen ohne Release). Netzfehler oder kaputtes JSON = eingebettete
//! Fassung. Abstimmungen gehen über den Feedback-Weg mit Quelle [`SOURCE`]; das Kennwort liegt
//! im Schlüsselbund (`woa_secrets`), `config.json` hält nur „freigeschaltet“. Es geht nur als
//! Kopfzeile an den eigenen Server, der es prüft.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::time::Duration;

use eframe::egui;
use serde::Deserialize;
use woa_telemetry::{DevKey, FeedbackError, FeedbackMeta, FeedbackType};

use crate::feedback::GuiConfig;
use crate::theme;

/// Eingebettete Fassung (Rückfall).
pub const EMBEDDED: &str = include_str!("../../../docs/fragen.json");
/// Aktuelle Fassung auf GitHub.
pub const REMOTE_URL: &str =
    "https://raw.githubusercontent.com/nixblick/daw.woa/main/docs/fragen.json";
/// Tickets: Link = diese Basis plus Nummer (nie eine URL aus dem JSON).
pub const ISSUE_BASE: &str = "https://github.com/nixblick/daw.woa/issues/";
/// Quelle im Feedback.
pub const SOURCE: &str = "entwickler-fragen";
/// Eintrag im Schlüsselbund für das Entwickler-Kennwort.
pub const SECRET_ACCOUNT: &str = "entwickler-kennwort";
/// Blöcke in der Reihenfolge der Oberfläche.
pub const BLOCKS: [&str; 6] = [
    "Radio",
    "Gäste/Telefon",
    "Party/DJ",
    "Produzent",
    "Jingles",
    "Allgemein",
];

/// Inhalt von `docs/fragen.json`.
#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
pub struct Fragen {
    #[serde(default)]
    pub stand: String,
    pub fragen: Vec<Frage>,
}

/// Eine Entscheidungsfrage.
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct Frage {
    pub id: String,
    pub block: String,
    pub titel: String,
    #[serde(default)]
    pub erklaerung: String,
    pub vorschlaege: Vec<String>,
    #[serde(default)]
    pub ticket: Option<u32>,
    #[serde(default)]
    pub status: String,
}

/// Liest und prüft eine Fassung: bekannte Blöcke, Titel, mindestens ein Vorschlag, eindeutige IDs.
pub fn parse(text: &str) -> Result<Fragen, String> {
    let f: Fragen = serde_json::from_str(text).map_err(|e| e.to_string())?;
    if f.fragen.is_empty() {
        return Err("keine Fragen".into());
    }
    let mut ids = HashSet::new();
    for q in &f.fragen {
        if !BLOCKS.contains(&q.block.as_str()) {
            return Err(format!("{}: unbekannter Block {}", q.id, q.block));
        }
        if q.id.trim().is_empty() || q.titel.trim().is_empty() {
            return Err("Frage ohne ID oder Titel".into());
        }
        if q.vorschlaege.iter().all(|v| v.trim().is_empty()) {
            return Err(format!("{}: kein Vorschlag", q.id));
        }
        if !ids.insert(q.id.clone()) {
            return Err(format!("{}: doppelte ID", q.id));
        }
    }
    Ok(f)
}

/// Die eingebettete Fassung (im Test geprüft, daher nie leer).
pub fn embedded() -> Fragen {
    parse(EMBEDDED).unwrap_or_else(|e| {
        tracing::error!(error = %e, "embedded questions invalid");
        Fragen::default()
    })
}

/// Woher die angezeigte Fassung stammt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Herkunft {
    Netz,
    Eingebettet,
}

/// Ergebnis des Nachladens: gültige Netzfassung, sonst die eingebettete.
pub fn from_fetch(result: Result<String, String>) -> (Fragen, Herkunft) {
    match result.and_then(|t| parse(&t)) {
        Ok(f) => (f, Herkunft::Netz),
        Err(e) => {
            tracing::info!(error = %e, "questions: using embedded copy");
            (embedded(), Herkunft::Eingebettet)
        }
    }
}

/// Lädt die Netzfassung (blockierend; nur im Hintergrund-Thread).
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

/// Link zum Ticket.
pub fn issue_url(n: u32) -> String {
    format!("{ISSUE_BASE}{n}")
}

/// Wahl zu einem Vorschlag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wahl {
    Dafuer,
    Dagegen,
    Anders,
    /// Nachtrag nach dem Senden (André 02.11.2026).
    Anmerkung,
}

impl Wahl {
    pub fn wire(self) -> &'static str {
        match self {
            Wahl::Dafuer => "dafür",
            Wahl::Dagegen => "dagegen",
            Wahl::Anders => "anders",
            Wahl::Anmerkung => "anmerkung",
        }
    }
}

/// Text der Meldung: Titelzeile mit Quelle, Fragen-ID und Wahl, dann die Einzelheiten.
/// Der Server übernimmt nur Titel und Text ins Ticket, daher steht die Kennzeichnung hier.
pub fn feedback_text(f: &Frage, vorschlag: usize, wahl: Wahl, text: &str) -> String {
    let v = f.vorschlaege.get(vorschlag).map_or("", String::as_str);
    let mut s = format!(
        "[{SOURCE}] {}: {} (Vorschlag {})\n\nQuelle: {SOURCE}\nFrage: {} ({})\nVorschlag: {}\nWahl: {}",
        f.id,
        wahl.wire(),
        vorschlag + 1,
        f.titel,
        f.block,
        v,
        wahl.wire()
    );
    if let Some(n) = f.ticket {
        s.push_str(&format!("\nTicket: #{n}"));
    }
    let t = text.trim();
    if !t.is_empty() {
        s.push_str(&format!("\nText: {t}"));
    }
    s
}

/// Begleitdaten: Art „frage“, Quelle, Kennwort als Kopfzeile.
pub fn meta(config: &GuiConfig, dev_key: Option<String>) -> FeedbackMeta {
    FeedbackMeta {
        url: config.feedback_url.clone(),
        kind: FeedbackType::Frage,
        install_id: config.install_id.clone(),
        source: Some(SOURCE.into()),
        dev_key: dev_key.map(DevKey),
        ..FeedbackMeta::default()
    }
}

type Key = (String, usize);

#[derive(Debug, Default)]
struct Eingabe {
    wahl: Option<Wahl>,
    text: String,
}

impl Eingabe {
    /// #338 (André 02.11.2026): Ein Kommentar heißt „anders“; Tippen wählt es automatisch.
    fn text_changed(&mut self) {
        if !self.text.trim().is_empty() {
            self.wahl = Some(Wahl::Anders);
        }
    }
}

/// Fenster „Fragen und To-dos“.
#[derive(Default)]
pub struct FragenWindow {
    pub open: bool,
    fragen: Option<(Fragen, Herkunft)>,
    loader: Option<Receiver<Result<String, String>>>,
    eingaben: HashMap<Key, Eingabe>,
    sent: HashSet<Key>,
    pending: HashMap<Key, Receiver<Result<Option<u64>, FeedbackError>>>,
    notes: HashMap<Key, String>,
    /// Nachtrag nach dem Senden, je Vorschlag (André 02.11.2026).
    nachtrag: HashMap<Key, String>,
    nachtrag_pending: HashMap<Key, Receiver<Result<Option<u64>, FeedbackError>>>,
    /// Fenster füllt die App (Knopf neben Schließen, #337).
    pub maximized: bool,
    /// Suchtext ganz oben.
    pub suche: String,
    /// Filter Offen/Gesendet/Alle.
    pub ansicht: Ansicht,
    /// Zeitpunkt der Ankunft je Vorschlag (Leuchtrahmen 1,5 s).
    sent_at: HashMap<Key, f64>,
}

impl FragenWindow {
    /// Öffnet das Fenster; beim ersten Mal lädt ein Hintergrund-Thread die Netzfassung.
    pub fn open_window(&mut self, ctx: &egui::Context) {
        self.open = true;
        if self.fragen.is_none() && self.loader.is_none() {
            let (tx, rx) = channel();
            let ctx = ctx.clone();
            let spawned = std::thread::Builder::new()
                .name("woa-fragen".into())
                .spawn(move || {
                    let _ = tx.send(fetch_remote());
                    ctx.request_repaint();
                });
            match spawned {
                Ok(_) => self.loader = Some(rx),
                Err(e) => {
                    tracing::warn!(error = %e, "questions loader thread failed");
                    self.fragen = Some((embedded(), Herkunft::Eingebettet));
                }
            }
        }
    }

    /// Angezeigte Fassung; bis das Netz antwortet, die eingebettete.
    pub fn current(&self) -> (Fragen, Herkunft) {
        self.fragen
            .clone()
            .unwrap_or_else(|| (embedded(), Herkunft::Eingebettet))
    }

    /// Reserviert einen Versand; `false`, wenn er schon läuft oder angekommen ist.
    fn begin(&mut self, key: &Key) -> bool {
        !self.sent.contains(key) && !self.pending.contains_key(key)
    }

    fn poll(&mut self, now: f64) {
        if let Some(rx) = &self.loader {
            match rx.try_recv() {
                Ok(r) => {
                    self.fragen = Some(from_fetch(r));
                    self.loader = None;
                }
                Err(TryRecvError::Disconnected) => {
                    self.fragen = Some(from_fetch(Err("abgebrochen".into())));
                    self.loader = None;
                }
                Err(TryRecvError::Empty) => {}
            }
        }
        let mut done = Vec::new();
        for (k, rx) in &self.pending {
            match rx.try_recv() {
                Ok(r) => done.push((k.clone(), r)),
                Err(TryRecvError::Disconnected) => {
                    done.push((
                        k.clone(),
                        Err(FeedbackError::Transport("abgebrochen".into())),
                    ));
                }
                Err(TryRecvError::Empty) => {}
            }
        }
        let mut nach = Vec::new();
        for (k, rx) in &self.nachtrag_pending {
            match rx.try_recv() {
                Ok(r) => nach.push((k.clone(), r)),
                Err(TryRecvError::Disconnected) => nach.push((
                    k.clone(),
                    Err(FeedbackError::Transport("abgebrochen".into())),
                )),
                Err(TryRecvError::Empty) => {}
            }
        }
        for (k, r) in nach {
            self.nachtrag_pending.remove(&k);
            let note = match r {
                Ok(_) => "Anmerkung gesendet, danke".to_string(),
                Err(FeedbackError::DailyLimit) => crate::feedback::DAILY_LIMIT.to_string(),
                Err(FeedbackError::NoEndpoint) => crate::feedback::NOT_CONFIGURED.to_string(),
                Err(e) => {
                    tracing::warn!(error = %e, "question note failed");
                    format!("Anmerkung: Senden fehlgeschlagen: {e}")
                }
            };
            self.notes.insert(k, note);
        }
        for (k, r) in done {
            self.pending.remove(&k);
            let note = match r {
                Ok(Some(n)) => {
                    self.sent.insert(k.clone());
                    self.sent_at.insert(k.clone(), now);
                    format!("Gesendet, danke (#{n})")
                }
                Ok(None) => {
                    self.sent.insert(k.clone());
                    self.sent_at.insert(k.clone(), now);
                    "Gesendet, danke".to_string()
                }
                // Held back in the outbox (submit_or_hold): counts as answered, goes out later.
                Err(FeedbackError::DailyLimit) => {
                    self.sent.insert(k.clone());
                    self.sent_at.insert(k.clone(), now);
                    crate::feedback::DAILY_LIMIT.to_string()
                }
                Err(FeedbackError::NoEndpoint) => crate::feedback::NOT_CONFIGURED.to_string(),
                Err(e) => {
                    tracing::warn!(error = %e, "question vote failed");
                    format!("Senden fehlgeschlagen: {e}")
                }
            };
            self.notes.insert(k, note);
        }
    }

    /// Schickt einen Nachtrag zu einem schon gesendeten Vorschlag ab.
    fn send_nachtrag(&mut self, f: &Frage, i: usize, config: &GuiConfig, data_dir: &Path) {
        let key = (f.id.clone(), i);
        if self.nachtrag_pending.contains_key(&key) {
            return;
        }
        let t = self.nachtrag.remove(&key).unwrap_or_default();
        if t.trim().is_empty() {
            return;
        }
        let text = feedback_text(f, i, Wahl::Anmerkung, &t);
        let config = config.clone();
        let data_dir: PathBuf = data_dir.to_path_buf();
        let (tx, rx) = channel();
        let spawned = std::thread::Builder::new()
            .name("woa-fragen-nachtrag".into())
            .spawn(move || {
                let key = woa_secrets::reveal(&data_dir, SECRET_ACCOUNT);
                let _ = tx.send(if !crate::feedback::may_send(&config.feedback_url) {
                    Err(FeedbackError::NoEndpoint)
                } else {
                    crate::feedback::submit_or_hold(&text, &meta(&config, key), &data_dir)
                });
            });
        match spawned {
            Ok(_) => {
                self.nachtrag_pending.insert(key.clone(), rx);
                self.notes.insert(key, "Anmerkung wird gesendet".into());
            }
            Err(e) => {
                tracing::error!(error = %e, "question note thread failed");
                self.notes
                    .insert(key, format!("Anmerkung: Senden fehlgeschlagen: {e}"));
            }
        }
    }

    /// Schickt eine Wahl im Hintergrund ab; das Kennwort liest erst der Sende-Thread.
    fn send(&mut self, f: &Frage, i: usize, config: &GuiConfig, data_dir: &Path) {
        let key = (f.id.clone(), i);
        if !self.begin(&key) {
            return;
        }
        let Some(e) = self.eingaben.get(&key) else {
            return;
        };
        let Some(wahl) = e.wahl else {
            self.notes.insert(key, "Bitte zuerst wählen".into());
            return;
        };
        if wahl == Wahl::Anders && e.text.trim().is_empty() {
            self.notes
                .insert(key, "Bei „anders“ bitte kurz schreiben, wie".into());
            return;
        }
        let text = feedback_text(f, i, wahl, &e.text);
        let config = config.clone();
        let data_dir: PathBuf = data_dir.to_path_buf();
        let (tx, rx) = channel();
        let spawned = std::thread::Builder::new()
            .name("woa-fragen-senden".into())
            .spawn(move || {
                let key = woa_secrets::reveal(&data_dir, SECRET_ACCOUNT);
                let _ = tx.send(if !crate::feedback::may_send(&config.feedback_url) {
                    Err(FeedbackError::NoEndpoint)
                } else {
                    crate::feedback::submit_or_hold(&text, &meta(&config, key), &data_dir)
                });
            });
        match spawned {
            Ok(_) => {
                self.pending.insert(key.clone(), rx);
                self.notes.insert(key, "Wird gesendet".into());
            }
            Err(e) => {
                tracing::error!(error = %e, "question vote thread failed");
                self.notes
                    .insert(key, format!("Senden fehlgeschlagen: {e}"));
            }
        }
    }

    /// Zeichnet das Fenster: eigene Kopfzeile mit Maximieren und Schließen, Suche, Blöcke als Karten.
    pub fn show(&mut self, ctx: &egui::Context, config: &GuiConfig, data_dir: &Path) {
        if !self.open {
            return;
        }
        let now = ctx.input(|i| i.time);
        self.poll(now);
        if self.sent_at.values().any(|t| leuchtet(*t, now)) {
            ctx.request_repaint();
        }
        if !self.pending.is_empty() || self.loader.is_some() {
            ctx.request_repaint_after(Duration::from_millis(200));
        }
        let (fragen, herkunft) = self.current();
        let sub = format!(
            "Stand {}{} · Deine Wahl geht als Rückmeldung aus dem Entwickler-Modus an das Projekt.",
            fragen.stand,
            match herkunft {
                Herkunft::Netz => "",
                Herkunft::Eingebettet => " (Fassung dieser Version)",
            }
        );
        let beantwortet = self.sent.iter().map(|k| &k.0).collect::<HashSet<_>>().len();
        let stand = format!("{beantwortet} von {} beantwortet", fragen.fragen.len());
        fenster(ctx, "woa-fragen", self.maximized).show(ctx, |ui| {
            kopf_stand(
                ui,
                theme::ic::CHAT_CIRCLE_TEXT,
                "Fragen und To-dos",
                &sub,
                Some(&stand),
                &mut self.open,
                &mut self.maximized,
            );
            suchfeld(ui, &mut self.suche);
            filter_leiste(ui, &mut self.ansicht);
            let suche = self.suche.clone();
            let ansicht = self.ansicht;
            let sent = self.sent.clone();
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let mut any = false;
                    for block in BLOCKS {
                        let qs: Vec<&Frage> = fragen
                            .fragen
                            .iter()
                            .filter(|q| {
                                let gesendet = (0..q.vorschlaege.len())
                                    .any(|i| sent.contains(&(q.id.clone(), i)));
                                q.block == block
                                    && frage_passt(q, &suche)
                                    && ansicht.zeigt(gesendet)
                            })
                            .collect();
                        if qs.is_empty() {
                            continue;
                        }
                        any = true;
                        block_kopf(ui, block, qs.len());
                        for q in qs {
                            self.frage_ui(ui, q, config, data_dir, now);
                            ui.add_space(theme::SPACE_3);
                        }
                    }
                    if !any {
                        keine_treffer(ui);
                    }
                });
        });
    }

    fn frage_ui(
        &mut self,
        ui: &mut egui::Ui,
        q: &Frage,
        config: &GuiConfig,
        data_dir: &Path,
        now: f64,
    ) {
        use crate::widgets::segment::Segment;
        theme::card_frame().show(ui, |ui| {
            ui.set_width(ui.available_width());
            // Kartenkopf: Titel, Ticket als Pille, Status als Badge.
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    egui::RichText::new(&q.titel)
                        .size(15.0)
                        .strong()
                        .color(theme::text()),
                );
                if let Some(n) = q.ticket {
                    ticket_pill(ui, n);
                }
                if !q.status.is_empty() {
                    crate::widgets::StateChip::new(&q.status, crate::widgets::ChipTone::Neutral)
                        .show(ui);
                }
            });
            if !q.erklaerung.is_empty() {
                ui.add_space(theme::SPACE_2);
                ui.label(egui::RichText::new(&q.erklaerung).color(theme::text_dim()));
            }
            for (i, v) in q.vorschlaege.iter().enumerate() {
                let key = (q.id.clone(), i);
                let done = self.sent.contains(&key);
                let busy = self.pending.contains_key(&key);
                ui.add_space(theme::SPACE_3);
                let mut go = false;
                let mut go_nachtrag = false;
                let fill = if done {
                    theme::mix(theme::ok(), theme::well(), 0.85)
                } else {
                    theme::well()
                };
                let glow = self.sent_at.get(&key).is_some_and(|t| leuchtet(*t, now));
                let edge = if done { theme::ok() } else { theme::border() };
                let width = if glow { 2.5 } else { 1.0 };
                egui::Frame::new()
                    .fill(fill)
                    .stroke(egui::Stroke::new(width, edge))
                    .shadow(if glow {
                        egui::epaint::Shadow {
                            spread: 2,
                            blur: 10,
                            offset: [0, 0],
                            color: crate::widgets::with_alpha(theme::ok(), 120),
                        }
                    } else {
                        egui::epaint::Shadow::NONE
                    })
                    .corner_radius(egui::CornerRadius::same(theme::RADIUS_MD))
                    .inner_margin(egui::Margin::symmetric(10, 8))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal_wrapped(|ui| {
                            ui.label(
                                egui::RichText::new(format!("{}", i + 1))
                                    .font(theme::label_font(14.0))
                                    .color(theme::accent()),
                            );
                            ui.label(egui::RichText::new(v).color(theme::text()));
                        });
                        ui.add_space(theme::SPACE_2);
                        ui.add_enabled_ui(!done && !busy, |ui| {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 4.0;
                                let e = self.eingaben.entry(key.clone()).or_default();
                                for (w, icon, label) in [
                                    (Wahl::Dafuer, theme::ic::THUMBS_UP, "Dafür"),
                                    (Wahl::Dagegen, theme::ic::THUMBS_DOWN, "Dagegen"),
                                    (Wahl::Anders, theme::ic::PENCIL_SIMPLE, "anders"),
                                ] {
                                    if Segment::new(icon, label)
                                        .on(e.wahl == Some(w))
                                        .height(28.0)
                                        .ui(ui)
                                        .clicked()
                                    {
                                        e.wahl = Some(w);
                                    }
                                }
                                ui.add_space(theme::SPACE_3);
                                let r = ui.add(
                                    egui::TextEdit::singleline(&mut e.text)
                                        .hint_text("Anmerkung (wählt „anders“)")
                                        .desired_width((ui.available_width() - 110.0).max(160.0)),
                                );
                                r.widget_info(|| {
                                    egui::WidgetInfo::labeled(
                                        egui::WidgetType::TextEdit,
                                        true,
                                        "Anmerkung",
                                    )
                                });
                                if r.changed() {
                                    e.text_changed();
                                }
                                let can = e.wahl.is_some();
                                let label = format!("{} Senden", theme::ic::PAPER_PLANE_RIGHT);
                                let btn =
                                    egui::Button::new(egui::RichText::new(label).color(if can {
                                        theme::on_accent()
                                    } else {
                                        theme::text_dim()
                                    }))
                                    .fill(if can {
                                        theme::accent()
                                    } else {
                                        theme::surface()
                                    })
                                    .corner_radius(egui::CornerRadius::same(theme::RADIUS_SM))
                                    .min_size(egui::vec2(96.0, 28.0));
                                go = ui.add_enabled(can, btn).clicked();
                            });
                        });
                        if done {
                            ui.add_space(theme::SPACE_2);
                            let nb = self.nachtrag_pending.contains_key(&key);
                            ui.add_enabled_ui(!nb, |ui| {
                                ui.horizontal(|ui| {
                                    let t = self.nachtrag.entry(key.clone()).or_default();
                                    let r = ui.add(
                                        egui::TextEdit::singleline(t)
                                            .hint_text("Anmerkung nachtragen")
                                            .desired_width(
                                                (ui.available_width() - 170.0).max(160.0),
                                            ),
                                    );
                                    r.widget_info(|| {
                                        egui::WidgetInfo::labeled(
                                            egui::WidgetType::TextEdit,
                                            true,
                                            "Nachtrag",
                                        )
                                    });
                                    let can = !t.trim().is_empty();
                                    go_nachtrag = ui
                                        .add_enabled(
                                            can,
                                            egui::Button::new(format!(
                                                "{} Anmerkung senden",
                                                theme::ic::PAPER_PLANE_RIGHT
                                            ))
                                            .min_size(egui::vec2(150.0, 28.0)),
                                        )
                                        .clicked()
                                        || (can
                                            && r.lost_focus()
                                            && ui.input(|i| i.key_pressed(egui::Key::Enter)));
                                });
                            });
                        }
                        if let Some(n) = self.notes.get(&key) {
                            ui.add_space(theme::SPACE_2);
                            let (icon, color) = if n.starts_with("Anmerkung: Senden") {
                                (theme::ic::WARNING, theme::danger_text())
                            } else if done {
                                (theme::ic::CHECK_CIRCLE, theme::ok())
                            } else if busy {
                                (theme::ic::PAPER_PLANE_RIGHT, theme::text_dim())
                            } else if n.starts_with("Senden fehlgeschlagen") {
                                (theme::ic::WARNING, theme::danger_text())
                            } else {
                                (theme::ic::INFO, theme::text_dim())
                            };
                            let retry = erneut_moeglich(Some(n.as_str()), busy);
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new(format!("{icon} {n}")).color(color));
                                if retry
                                    && ui
                                        .button(format!(
                                            "{} Erneut senden",
                                            theme::ic::ARROW_CLOCKWISE
                                        ))
                                        .clicked()
                                {
                                    go = true;
                                }
                            });
                        }
                    });
                if go {
                    self.send(q, i, config, data_dir);
                }
                if go_nachtrag {
                    self.send_nachtrag(q, i, config, data_dir);
                }
            }
        });
    }
}

/// Filter über der Liste (QuestionStates Fassung 7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Ansicht {
    /// Noch ohne gesendete Wahl.
    Offen,
    /// Mindestens ein Vorschlag gesendet.
    Gesendet,
    /// Vorgabe: sonst verschwände eine Karte samt Leuchtrahmen sofort nach dem Senden.
    #[default]
    Alle,
}

impl Ansicht {
    pub const ALLE: [Ansicht; 3] = [Ansicht::Offen, Ansicht::Gesendet, Ansicht::Alle];

    pub fn wort(self) -> &'static str {
        match self {
            Ansicht::Offen => "Offen",
            Ansicht::Gesendet => "Gesendet",
            Ansicht::Alle => "Alle",
        }
    }

    /// Zeigt der Filter eine Frage, von der schon etwas gesendet ist (`gesendet`)?
    pub fn zeigt(self, gesendet: bool) -> bool {
        match self {
            Ansicht::Offen => !gesendet,
            Ansicht::Gesendet => gesendet,
            Ansicht::Alle => true,
        }
    }
}

/// Leuchtrahmen nach dem Senden: 1,5 s ab `t0`.
pub const LEUCHT_SEKUNDEN: f64 = 1.5;

/// Leuchtet der Rahmen noch?
pub fn leuchtet(t0: f64, now: f64) -> bool {
    (0.0..LEUCHT_SEKUNDEN).contains(&(now - t0))
}

/// Darf „Erneut senden“ erscheinen? Nur nach einem Fehlschlag, nicht während des Sendens.
pub fn erneut_moeglich(note: Option<&str>, busy: bool) -> bool {
    !busy && note.is_some_and(|n| n.starts_with("Senden fehlgeschlagen"))
}

/// Passt eine Frage zur Suche? Titel, Erklärung, Vorschläge, Block, Status und Ticketnummer,
/// Groß/klein egal; jedes Wort muss vorkommen. Leere Suche passt immer.
pub fn frage_passt(q: &Frage, suche: &str) -> bool {
    let ticket = q.ticket.map(|n| format!("#{n} {n}")).unwrap_or_default();
    let mut felder = vec![
        q.titel.as_str(),
        q.erklaerung.as_str(),
        q.block.as_str(),
        q.status.as_str(),
        ticket.as_str(),
    ];
    felder.extend(q.vorschlaege.iter().map(String::as_str));
    passt(suche, &felder)
}

/// Jedes Wort der Suche steht in einem der Felder (Groß/klein egal).
pub fn passt(suche: &str, felder: &[&str]) -> bool {
    let heu = felder.join("\n").to_lowercase();
    suche
        .split_whitespace()
        .all(|w| heu.contains(&w.to_lowercase()))
}

/// Fenster der Entwickler-Werkzeuge: ohne egui-Titelleiste (eigene Kopfzeile), maximiert über
/// die ganze App. Eigene ID je Zustand: das normale Fenster behält Lage und Größe.
pub(crate) fn fenster(
    ctx: &egui::Context,
    id: &'static str,
    maximized: bool,
) -> egui::Window<'static> {
    let win = egui::Window::new(id)
        .id(egui::Id::new((id, maximized)))
        .title_bar(false)
        .collapsible(false)
        .frame(
            egui::Frame::window(&ctx.global_style())
                .fill(theme::bg())
                .stroke(egui::Stroke::new(1.0, theme::border_strong()))
                .corner_radius(egui::CornerRadius::same(theme::RADIUS_XL))
                .inner_margin(egui::Margin::same(theme::SPACE_5 as i8)),
        );
    if maximized {
        win.fixed_rect(ctx.content_rect().shrink(4.0))
            .resizable(false)
    } else {
        win.default_size([780.0, 700.0])
            .min_width(520.0)
            .min_height(320.0)
            .resizable(true)
    }
}

/// Kopfzeile: Symbol, Titel, Unterzeile; rechts Maximieren und Schließen (#337).
pub(crate) fn kopf(
    ui: &mut egui::Ui,
    icon: &str,
    titel: &str,
    sub: &str,
    open: &mut bool,
    maximized: &mut bool,
) {
    kopf_stand(ui, icon, titel, sub, None, open, maximized);
}

/// Kopf mit Stand rechts als neutraler Chip (Fassung 8, Punkt 5), etwa „12 von 40 geprüft“.
pub(crate) fn kopf_stand(
    ui: &mut egui::Ui,
    icon: &str,
    titel: &str,
    sub: &str,
    stand: Option<&str>,
    open: &mut bool,
    maximized: &mut bool,
) {
    // Fassung 7: eigener Kopf 44 px; Doppelklick maximiert, Esc schließt.
    // Erst die Kopffläche anmelden, dann die Knöpfe darüber: so gewinnen die Knöpfe den Klick.
    let rect = egui::Rect::from_min_size(ui.cursor().min, egui::vec2(ui.available_width(), KOPF_H));
    let r = ui.interact(rect, ui.id().with("kopf"), egui::Sense::click());
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), KOPF_H),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_min_height(KOPF_H);
            kopf_inhalt(ui, icon, titel, sub, stand, open, maximized)
        },
    );
    if r.double_clicked() {
        *maximized = !*maximized;
    }
    // #441: Esc über die gemeinsame Regel: nur das oberste Fenster schließt.
    if crate::esc::fenster(ui.ctx(), ui.layer_id(), ui.id()) {
        *open = false;
    }
    ui.add_space(theme::SPACE_2);
}

fn kopf_inhalt(
    ui: &mut egui::Ui,
    icon: &str,
    titel: &str,
    sub: &str,
    stand: Option<&str>,
    open: &mut bool,
    maximized: &mut bool,
) {
    use crate::widgets::icon_button::{IconButton, IconSize};
    {
        ui.label(egui::RichText::new(icon).size(22.0).color(theme::accent()));
        ui.vertical(|ui| {
            ui.label(
                egui::RichText::new(titel)
                    .size(theme::TYPE_TITLE * 0.85)
                    .strong()
                    .color(theme::text()),
            );
            ui.label(egui::RichText::new(sub).small().color(theme::text_dim()));
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
            // Fassung 8, 0b: Loslassen über dem X schließt immer, auch wenn egui das Drücken
            // einem anderen Ziel zugeordnet hat (Kopffläche, Fenster ziehen). Einfacher Klick
            // ohne Mausbewegung schloss Fragen und Abnahme sonst nicht immer.
            let x = IconButton::new(theme::ic::X, "Schließen")
                .size(IconSize::Md)
                .ui(ui);
            if released_on(ui, x) {
                *open = false;
            }
            let (icon, tip) = if *maximized {
                (theme::ic::CORNERS_IN, "Wiederherstellen")
            } else {
                (theme::ic::CORNERS_OUT, "Maximieren")
            };
            let m = IconButton::new(icon, tip).size(IconSize::Md).ui(ui);
            if released_on(ui, m) {
                *maximized = !*maximized;
            }
            if let Some(t) = stand {
                ui.add_space(theme::SPACE_2);
                crate::widgets::StateChip::new(t, crate::widgets::ChipTone::Neutral).show(ui);
            }
        });
    }
}

/// Klick oder Loslassen der linken Taste über dem Knopf (genau einmal je Klick).
fn released_on(ui: &egui::Ui, r: egui::Response) -> bool {
    r.clicked() || (r.contains_pointer() && ui.input(|i| i.pointer.primary_released()))
}

/// Höhe des Suchfelds (QuestionsWindow Fassung 7).
pub const SUCH_H: f32 = 48.0;
/// Höhe des Fensterkopfs (Window Fassung 7).
pub const KOPF_H: f32 = 44.0;

/// Suchfeld ganz oben, volle Breite, mit Lupe und Leeren-Knopf.
pub(crate) fn suchfeld(ui: &mut egui::Ui, suche: &mut String) {
    use crate::widgets::icon_button::{IconButton, IconSize};
    egui::Frame::new()
        .fill(theme::well())
        .stroke(egui::Stroke::new(1.0, theme::border()))
        .corner_radius(egui::CornerRadius::same(theme::RADIUS_MD))
        .inner_margin(egui::Margin::symmetric(12, 0))
        .show(ui, |ui| {
            // Fassung 7: Suchfeld 48 px hoch.
            ui.allocate_ui_with_layout(
                egui::vec2(ui.available_width(), SUCH_H - 2.0),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.set_min_height(SUCH_H - 2.0);
                    ui.label(
                        egui::RichText::new(theme::ic::MAGNIFYING_GLASS)
                            .size(16.0)
                            .color(theme::text_dim()),
                    );
                    let w = ui.available_width() - if suche.is_empty() { 0.0 } else { 32.0 };
                    let r = ui.add(
                        egui::TextEdit::singleline(suche)
                            .frame(egui::Frame::NONE)
                            .hint_text("Suchen: Titel, Text, Ticketnummer")
                            .desired_width(w),
                    );
                    r.widget_info(|| {
                        egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, "Suchen")
                    });
                    if !suche.is_empty()
                        && IconButton::new(theme::ic::X, "Suche leeren")
                            .size(IconSize::Sm)
                            .ui(ui)
                            .clicked()
                    {
                        suche.clear();
                    }
                },
            );
        });
    ui.add_space(theme::SPACE_2);
}

/// Filterleiste Offen | Gesendet | Alle.
pub(crate) fn filter_leiste(ui: &mut egui::Ui, ansicht: &mut Ansicht) {
    use crate::widgets::segment::Segment;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        for a in Ansicht::ALLE {
            let icon = match a {
                Ansicht::Offen => theme::ic::CIRCLE_DASHED,
                Ansicht::Gesendet => theme::ic::CHECK_CIRCLE,
                Ansicht::Alle => theme::ic::LIST,
            };
            if Segment::new(icon, a.wort())
                .on(*ansicht == a)
                .height(28.0)
                .ui(ui)
                .clicked()
            {
                *ansicht = a;
            }
        }
    });
    ui.add_space(theme::SPACE_2);
}

/// Kopf eines Blocks: Name als Pult-Beschriftung plus Anzahl.
pub(crate) fn block_kopf(ui: &mut egui::Ui, name: &str, n: usize) {
    ui.add_space(theme::SPACE_3);
    ui.horizontal(|ui| {
        ui.label(theme::panel_label(name.to_uppercase()));
        ui.label(
            egui::RichText::new(format!("{n}"))
                .font(theme::label_font(12.0))
                .color(theme::text_dim()),
        );
    });
    ui.add_space(theme::SPACE_2);
}

/// Hinweis, wenn die Suche nichts findet.
pub(crate) fn keine_treffer(ui: &mut egui::Ui) {
    ui.add_space(theme::SPACE_5);
    ui.vertical_centered(|ui| {
        ui.label(egui::RichText::new("Nichts gefunden").color(theme::text_dim()));
    });
}

/// Ticket als Pille (`#337 ↗`), Klick öffnet es im Browser.
pub(crate) fn ticket_pill(ui: &mut egui::Ui, n: u32) {
    let text = format!("#{n} {}", theme::ic::ARROW_SQUARE_OUT);
    let r = ui
        .add(
            egui::Button::new(
                egui::RichText::new(text)
                    .font(theme::label_font(13.0))
                    .color(theme::text()),
            )
            .fill(theme::well())
            // Neutral (Fassung 8): Tickets sind kein Zustand.
            .stroke(egui::Stroke::new(1.0, theme::border_strong()))
            .corner_radius(egui::CornerRadius::same(theme::RADIUS_MD)),
        )
        .on_hover_text(format!("Ticket #{n} öffnen"));
    if r.clicked() {
        ui.ctx().open_url(egui::OpenUrl::new_tab(issue_url(n)));
    }
}

/// Fenster zum Freischalten in den Einstellungen.
#[derive(Default)]
pub struct EntwicklerTor {
    pub open: bool,
    kennwort: String,
    note: Option<String>,
}

/// Was das Freischalt-Fenster ausgelöst hat.
#[derive(Debug, PartialEq, Eq)]
pub enum TorAktion {
    Keine,
    Freischalten(String),
    Beenden,
}

impl EntwicklerTor {
    /// Speichert das Kennwort im Schlüsselbund; `config.entwickler` setzt der Aufrufer.
    pub fn unlock(data_dir: &Path, kennwort: &str) -> Result<(), String> {
        let k = kennwort.trim();
        if k.is_empty() {
            return Err("Bitte ein Kennwort eingeben".into());
        }
        woa_secrets::save(data_dir, SECRET_ACCOUNT, k).map(|_| ())
    }

    /// Entfernt das Kennwort wieder.
    pub fn lock(data_dir: &Path) {
        if let Err(e) = woa_secrets::clear(data_dir, SECRET_ACCOUNT) {
            tracing::warn!(error = %e, "developer key not cleared");
        }
    }

    pub fn set_note(&mut self, note: impl Into<String>) {
        self.note = Some(note.into());
    }

    /// Zeichnet das Fenster; liefert, was zu tun ist.
    pub fn show(&mut self, ctx: &egui::Context, unlocked: bool) -> TorAktion {
        if !self.open {
            return TorAktion::Keine;
        }
        let mut aktion = TorAktion::Keine;
        let mut open = self.open;
        let esc_resp = egui::Window::new("Entwickler-Modus")
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .show(ctx, |ui| {
                ui.label(
                    "Für alle, die WOA mitentwickeln: Menü „Entwickler“ mit Update-Suche und \
                     „Fragen und To-dos“. Das Kennwort bekommst du von André; der Server prüft \
                     es beim Senden.",
                );
                ui.add_space(6.0);
                if unlocked {
                    ui.label("Der Entwickler-Modus ist an.");
                    if ui.button("Entwickler-Modus beenden").clicked() {
                        aktion = TorAktion::Beenden;
                    }
                } else {
                    let r = ui.add(
                        egui::TextEdit::singleline(&mut self.kennwort)
                            .password(true)
                            .hint_text("Kennwort"),
                    );
                    let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    if ui.button("Freischalten").clicked() || enter {
                        aktion = TorAktion::Freischalten(std::mem::take(&mut self.kennwort));
                    }
                }
                if let Some(n) = &self.note {
                    ui.label(egui::RichText::new(n).small().color(theme::text_dim()));
                }
            });
        // #441: Esc schließt das oberste Fenster (gemeinsame Regel).
        if crate::esc::nach(ctx, &esc_resp, "entwickler-tor") {
            open = false;
        }
        self.open = open;
        if !open {
            self.kennwort.clear();
            self.note = None;
        }
        aktion
    }
}

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)]
mod tests {
    #[test]
    fn filter_open_sent_all() {
        use super::Ansicht;
        assert!(Ansicht::Offen.zeigt(false));
        assert!(!Ansicht::Offen.zeigt(true));
        assert!(Ansicht::Gesendet.zeigt(true));
        assert!(!Ansicht::Gesendet.zeigt(false));
        assert!(Ansicht::Alle.zeigt(true) && Ansicht::Alle.zeigt(false));
        assert_eq!(Ansicht::default(), Ansicht::Alle);
    }

    #[test]
    fn sent_glows_for_one_and_a_half_seconds() {
        assert!(super::leuchtet(10.0, 10.0));
        assert!(super::leuchtet(10.0, 11.49));
        assert!(!super::leuchtet(10.0, 11.5));
        assert!(!super::leuchtet(10.0, 9.0));
    }

    #[test]
    fn retry_only_after_failure() {
        use super::erneut_moeglich;
        assert!(erneut_moeglich(Some("Senden fehlgeschlagen: 500"), false));
        assert!(!erneut_moeglich(Some("Senden fehlgeschlagen: 500"), true));
        assert!(!erneut_moeglich(Some("Gesendet, danke"), false));
        assert!(!erneut_moeglich(Some("Bitte zuerst wählen"), false));
        assert!(!erneut_moeglich(None, false));
    }

    use super::*;

    #[test]
    fn embedded_questions_parse_and_are_complete() {
        let f = parse(EMBEDDED).expect("docs/fragen.json gültig");
        assert!(f.fragen.len() >= 10);
        for q in &f.fragen {
            assert!(BLOCKS.contains(&q.block.as_str()), "{}", q.id);
            assert!(!q.titel.trim().is_empty(), "{}", q.id);
            assert!(!q.vorschlaege.is_empty(), "{}", q.id);
        }
        for b in BLOCKS {
            assert!(f.fragen.iter().any(|q| q.block == b), "Block {b} leer");
        }
    }

    #[test]
    fn embedded_questions_hold_nothing_internal() {
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
        ] {
            assert!(!low.contains(bad), "{bad} in docs/fragen.json");
        }
    }

    #[test]
    fn failed_or_broken_fetch_falls_back_to_embedded() {
        let (f, h) = from_fetch(Err("offline".into()));
        assert_eq!(h, Herkunft::Eingebettet);
        assert_eq!(f, embedded());
        let (f, h) = from_fetch(Ok("<html>404</html>".into()));
        assert_eq!(h, Herkunft::Eingebettet);
        assert_eq!(f, embedded());
        let (_, h) = from_fetch(Ok(r#"{"fragen":[]}"#.into()));
        assert_eq!(h, Herkunft::Eingebettet, "leere Fassung zählt nicht");
        let neu = r#"{"stand":"x","fragen":[{"id":"a","block":"Radio","titel":"T","vorschlaege":["V"]}]}"#;
        let (f, h) = from_fetch(Ok(neu.into()));
        assert_eq!(h, Herkunft::Netz);
        assert_eq!(f.fragen[0].id, "a");
    }

    #[test]
    fn unknown_block_or_missing_proposal_is_refused() {
        assert!(
            parse(r#"{"fragen":[{"id":"a","block":"Kino","titel":"T","vorschlaege":["V"]}]}"#)
                .is_err()
        );
        assert!(
            parse(r#"{"fragen":[{"id":"a","block":"Radio","titel":"T","vorschlaege":[]}]}"#)
                .is_err()
        );
    }

    #[test]
    fn vote_payload_is_marked_as_developer_questions() {
        let f = embedded();
        let q = f.fragen.iter().find(|q| q.ticket.is_some()).unwrap();
        let text = feedback_text(q, 0, Wahl::Anders, "lieber 5 Sekunden");
        let cfg = GuiConfig::default();
        let p = woa_telemetry::build_payload(&text, &[], &meta(&cfg, Some("x".into()))).unwrap();
        let title = p["title"].as_str().unwrap();
        assert!(title.starts_with("[entwickler-fragen] "), "{title}");
        assert!(title.contains(&q.id) && title.contains("anders"), "{title}");
        let body = p["body"].as_str().unwrap();
        assert!(body.contains("Quelle: entwickler-fragen"), "{body}");
        assert!(body.contains(&format!("Frage: {}", q.titel)), "{body}");
        assert!(body.contains("Wahl: anders") && body.contains("lieber 5 Sekunden"));
        assert_eq!(p["source"], SOURCE);
        assert_eq!(p["type"], "frage");
        // Daumen hoch ohne Text ergibt trotzdem eine gültige Meldung.
        let t = feedback_text(q, 1, Wahl::Dafuer, "");
        assert!(woa_telemetry::build_payload(&t, &[], &meta(&cfg, None)).is_ok());
    }

    #[test]
    fn issue_links_come_from_the_number_only() {
        assert_eq!(
            issue_url(320),
            "https://github.com/nixblick/daw.woa/issues/320"
        );
    }

    #[test]
    fn a_vote_is_sent_only_once() {
        let mut w = FragenWindow::default();
        let key = ("a".to_string(), 0);
        assert!(w.begin(&key));
        let (_tx, rx) = channel();
        w.pending.insert(key.clone(), rx);
        assert!(!w.begin(&key), "läuft schon");
        w.pending.remove(&key);
        w.sent.insert(key.clone());
        assert!(!w.begin(&key), "schon angekommen");
    }

    #[test]
    fn successful_send_locks_the_proposal() {
        let mut w = FragenWindow::default();
        let key = ("a".to_string(), 1);
        let (tx, rx) = channel();
        w.pending.insert(key.clone(), rx);
        tx.send(Ok(Some(7))).unwrap();
        w.poll(0.0);
        assert!(w.sent.contains(&key));
        assert_eq!(w.notes[&key], "Gesendet, danke (#7)");
        let (tx, rx) = channel();
        let other = ("b".to_string(), 0);
        w.pending.insert(other.clone(), rx);
        tx.send(Err(FeedbackError::Status(500))).unwrap();
        w.poll(0.0);
        assert!(!w.sent.contains(&other), "Fehler: nochmal senden erlaubt");
    }

    /// #338: Wer etwas ins Kommentarfeld schreibt, meint „anders“.
    #[test]
    fn typing_a_comment_selects_anders() {
        let mut e = Eingabe {
            wahl: Some(Wahl::Dafuer),
            text: String::new(),
        };
        e.text = "lieber blau".into();
        e.text_changed();
        assert_eq!(e.wahl, Some(Wahl::Anders));
        let mut leer = Eingabe::default();
        leer.text = "   ".into();
        leer.text_changed();
        assert_eq!(leer.wahl, None, "nur Leerzeichen wählen nichts");
    }

    use egui_kittest::kittest::Queryable;

    fn harness(w: FragenWindow) -> egui_kittest::Harness<'static, FragenWindow> {
        let cfg = GuiConfig::default();
        let dir = std::env::temp_dir();
        let mut h = egui_kittest::Harness::builder()
            .with_size(egui::vec2(1400.0, 900.0))
            .build_ui_state(
                move |ui, w: &mut FragenWindow| {
                    if !theme::install_fonts(ui.ctx()) {
                        w.show(ui.ctx(), &cfg, &dir);
                    }
                },
                w,
            );
        h.run_steps(3);
        h
    }

    /// #338 im Fenster: Tippen ins Kommentarfeld schaltet das Segment „anders“ ein.
    #[test]
    fn typing_in_the_window_turns_anders_on() {
        let mut w = FragenWindow::default();
        w.open = true;
        w.fragen = Some((embedded(), Herkunft::Eingebettet));
        let mut h = harness(w);
        let q = embedded().fragen[0].id.clone();
        h.get_all_by_label("Anmerkung")
            .next()
            .expect("Kommentarfeld")
            .click();
        h.run_steps(1);
        h.get_all_by_label("Anmerkung")
            .next()
            .expect("Kommentarfeld")
            .type_text("x");
        h.run_steps(2);
        let key = (q, 0);
        assert_eq!(h.state().eingaben[&key].wahl, Some(Wahl::Anders));
    }

    /// Suche (André 02.11.2026): filtert Titel, Erklärung, Vorschläge und Ticketnummer.
    #[test]
    fn search_matches_title_text_proposals_and_ticket() {
        let f = embedded();
        let q = f.fragen.iter().find(|q| q.ticket.is_some()).unwrap();
        assert!(frage_passt(q, ""));
        assert!(frage_passt(q, &q.titel.to_uppercase()));
        assert!(frage_passt(q, &format!("#{}", q.ticket.unwrap())));
        assert!(frage_passt(q, &q.vorschlaege[0]));
        assert!(!frage_passt(q, "xyzzy-gibt-es-nicht"));
    }

    #[test]
    fn search_hides_everything_that_does_not_match() {
        let mut w = FragenWindow::default();
        w.open = true;
        w.fragen = Some((embedded(), Herkunft::Eingebettet));
        let mut h = harness(w);
        assert!(h.get_all_by_label("Anmerkung").count() > 0);
        h.get_by_label("Suchen").click();
        h.run_steps(1);
        h.get_by_label("Suchen").type_text("xyzzy-gibt-es-nicht");
        h.run_steps(2);
        assert_eq!(h.query_all_by_label("Anmerkung").count(), 0);
        assert!(h.query_by_label("Nichts gefunden").is_some());
    }

    /// #337: Neben Schließen ein Maximieren-Knopf; er füllt den Bildschirm und schaltet zurück.
    #[test]
    fn window_has_maximize_next_to_close() {
        let mut w = FragenWindow::default();
        w.open = true;
        w.fragen = Some((embedded(), Herkunft::Eingebettet));
        let mut h = harness(w);
        let close = h.get_by_label(theme::ic::X).rect();
        let max = h.get_by_label(theme::ic::CORNERS_OUT).rect();
        assert!(max.right() <= close.left() && (max.center().y - close.center().y).abs() < 2.0);
        h.get_by_label(theme::ic::CORNERS_OUT).click();
        h.run_steps(3);
        assert!(h.state().maximized);
        h.get_by_label(theme::ic::CORNERS_IN).click();
        h.run_steps(2);
        assert!(!h.state().maximized);
        h.get_by_label(theme::ic::X).click();
        h.run_steps(2);
        assert!(!h.state().open);
    }

    /// André 02.11.2026: Nach dem Senden lässt sich eine Anmerkung nachtragen.
    #[test]
    fn note_can_be_added_after_sending() {
        let f = embedded();
        let q = &f.fragen[0];
        let t = feedback_text(q, 0, Wahl::Anmerkung, "noch was");
        assert!(t.starts_with(&format!("[entwickler-fragen] {}: anmerkung", q.id)));
        let mut w = FragenWindow::default();
        w.open = true;
        w.fragen = Some((embedded(), Herkunft::Eingebettet));
        let key = (q.id.clone(), 0);
        w.sent.insert(key.clone());
        let mut h = harness(w);
        h.get_all_by_label("Nachtrag")
            .next()
            .expect("Nachtragsfeld")
            .click();
        h.run_steps(1);
        h.get_all_by_label("Nachtrag")
            .next()
            .unwrap()
            .type_text("noch was");
        h.run_steps(2);
        h.get_all_by_label_contains("Anmerkung senden")
            .next()
            .unwrap()
            .click();
        h.run_steps(2);
        assert!(h.state().nachtrag.get(&key).is_none_or(|t| t.is_empty()));
        assert!(h.state().notes.contains_key(&key));
        assert!(h.state().sent.contains(&key));
    }
}
