//! Hörer: Zahl oben in der Kopfleiste und Fenster „Hörer-Statistik“. Beide lesen nur den
//! Endpunkt `/api/hoerer` des eigenen Servers (`server/hoerer`, sammelt auch ohne App; André
//! 03.11.2026: „die daten sollten vom server kommen“). Der Shoutcast-Admin-Port ist von außen
//! zu, darum fragt der Chip nicht mehr den Sendeserver selbst (0.41.12).

use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::time::{Duration, Instant};

use eframe::egui;
use serde::Deserialize;

use crate::fragen::{fenster, kopf};
use crate::theme;

/// Vorgabe: Endpunkt neben dem Feedback-Server (eigener Server).
pub const DEFAULT_HOERER_URL: &str = "https://feedback.wironair.de/api/hoerer";
/// So oft liest das offene Fenster neu (der Server sammelt alle 30 s).
const REFRESH: Duration = Duration::from_secs(30);

/// Zahlen von gerade (letzte Probe des Servers).
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Jetzt {
    pub t: u64,
    pub aktuell: u32,
    #[serde(default)]
    pub spitze_server: Option<u32>,
    #[serde(default)]
    pub eindeutig: Option<u32>,
    /// Offene Verbindungen (ein Gerät hat oft zwei); ältere Server liefern das Feld nicht.
    #[serde(default)]
    pub verbindungen: Option<u32>,
    #[serde(default)]
    pub quelle: bool,
}

impl Jetzt {
    /// Hörer = eindeutige Hörer; `aktuell` zählt Verbindungen und gilt nur ohne `eindeutig`.
    pub fn hoerer(&self) -> u32 {
        self.eindeutig.unwrap_or(self.aktuell)
    }
}

/// Eine Sendung (zusammenhängend „Quelle verbunden“).
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Sendung {
    pub start: u64,
    pub ende: u64,
    pub dauer_s: u64,
    pub spitze: u32,
    pub durchschnitt: f32,
    #[serde(default)]
    pub laeuft: bool,
}

/// Antwort von `/api/hoerer`.
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(default)]
pub struct Auswertung {
    pub jetzt: Option<Jetzt>,
    /// (Unix-Sekunden, Hörer) der neuesten Sendung.
    pub verlauf: Vec<(u64, u32)>,
    /// Neueste zuerst.
    pub sendungen: Vec<Sendung>,
}

pub fn parse(text: &str) -> Result<Auswertung, String> {
    serde_json::from_str(text).map_err(|e| format!("Antwort nicht lesbar: {e}"))
}

/// Holt die Auswertung; Fehler als lesbarer Satz.
pub fn fetch(url: &str) -> Result<Auswertung, String> {
    if url.trim().is_empty() {
        return Err("Kein Statistik-Server eingetragen (config.json: hoerer_url).".into());
    }
    if !crate::feedback::may_send(url) {
        return Err("In Tests nur lokale Attrappen.".into());
    }
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(8)))
        .build()
        .into();
    let text = agent
        .get(url)
        .call()
        .map_err(|e| format!("Statistik-Server nicht erreichbar ({e})."))?
        .body_mut()
        .with_config()
        .limit(1024 * 1024)
        .read_to_string()
        .map_err(|e| format!("Statistik-Server nicht erreichbar ({e})."))?;
    parse(&text)
}

/// Text hinter „Hörer“ im Chip: `aktuell | Spitze der Sendung`, „–“ ohne Zahl.
pub fn chip_suffix(current: Option<u32>, peak: Option<u32>) -> String {
    match (current, peak) {
        (Some(c), Some(p)) => format!("{c} | {}", p.max(c)),
        (Some(c), None) => format!("{c} | {c}"),
        _ => "–".into(),
    }
}

/// Spitze der laufenden Sendung, aus den gelesenen Zahlen (die Spitze des Servers gilt seit
/// dessen Start, nicht je Sendung).
#[derive(Debug, Default)]
pub struct SessionPeak {
    on_air: bool,
    peak: Option<u32>,
}

impl SessionPeak {
    pub fn update(&mut self, on_air: bool, current: Option<u32>) -> Option<u32> {
        if on_air && !self.on_air {
            self.peak = None;
        }
        self.on_air = on_air;
        if let Some(c) = current {
            self.peak = Some(self.peak.map_or(c, |p| p.max(c)));
        }
        self.peak
    }
}

/// `h:mm` aus Sekunden.
pub fn dauer(s: u64) -> String {
    format!("{}:{:02}", s / 3600, s % 3600 / 60)
}

/// Datum und Uhrzeit (UTC-frei: lokale Zone über die Systemzeit ist ohne Abhängigkeit nicht
/// zu haben; der Server liefert Unix-Sekunden, angezeigt wird UTC mit Kennzeichen).
pub fn datum(unix: u64) -> String {
    let days = (unix / 86_400) as i64;
    let secs = unix % 86_400;
    // Tage seit 1970 → Kalenderdatum (Howard Hinnant, civil_from_days).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{d:02}.{m:02}.{y} {:02}:{:02} UTC",
        secs / 3600,
        secs % 3600 / 60
    )
}

/// Holt `/api/hoerer` im Hintergrund, höchstens alle [`REFRESH`]; blockiert nie den UI-Thread.
/// Gemeinsam für Fenster und Chip.
#[derive(Default)]
pub struct Abruf {
    rx: Option<Receiver<Result<Auswertung, String>>>,
    geholt: Option<Instant>,
}

impl Abruf {
    /// Liefert ein fertiges Ergebnis, sonst `None`; stößt bei Fälligkeit den nächsten Abruf an.
    pub fn poll(&mut self, url: &str) -> Option<Result<Auswertung, String>> {
        let mut out = None;
        if let Some(rx) = &self.rx {
            match rx.try_recv() {
                Ok(r) => {
                    out = Some(r);
                    self.rx = None;
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => self.rx = None,
            }
        }
        let due = self.geholt.is_none_or(|t| t.elapsed() >= REFRESH);
        if self.rx.is_none() && due {
            self.geholt = Some(Instant::now());
            let (tx, rx) = channel();
            let url = url.to_string();
            let spawned = std::thread::Builder::new()
                .name("woa-hoerer".into())
                .spawn(move || {
                    let _ = tx.send(fetch(&url));
                });
            match spawned {
                Ok(_) => self.rx = Some(rx),
                Err(e) => out = Some(Err(e.to_string())),
            }
        }
        out
    }
}

/// Zahlen für den Chip aus einer Antwort: (jetzt, Spitze der laufenden Sendung).
pub fn chip_zahlen(a: &Auswertung) -> (Option<u32>, Option<u32>) {
    let jetzt = a.jetzt.as_ref().map(Jetzt::hoerer);
    let spitze = a.sendungen.first().filter(|s| s.laeuft).map(|s| s.spitze);
    (jetzt, spitze)
}

/// Stand des Chips oben: letzte gültige Zahl vom Server, „–“ bei Fehler.
#[derive(Default)]
pub struct ChipStand {
    abruf: Abruf,
    pub aktuell: Option<u32>,
    pub spitze_server: Option<u32>,
}

impl ChipStand {
    /// Nur auf Sendung aufrufen; fragt höchstens alle 30 s im Hintergrund.
    pub fn poll(&mut self, url: &str) {
        match self.abruf.poll(url) {
            Some(Ok(a)) => (self.aktuell, self.spitze_server) = chip_zahlen(&a),
            Some(Err(_)) => (self.aktuell, self.spitze_server) = (None, None),
            None => {}
        }
    }
}

enum Stand {
    Leer,
    Fehler(String),
    Daten(Auswertung),
}

/// Fenster „Hörer-Statistik“.
pub struct HoererWindow {
    pub open: bool,
    maximized: bool,
    stand: Stand,
    abruf: Abruf,
}

impl Default for HoererWindow {
    fn default() -> Self {
        Self {
            open: false,
            maximized: false,
            stand: Stand::Leer,
            abruf: Abruf::default(),
        }
    }
}

impl HoererWindow {
    /// Fotomodus: Beispielzahlen statt Server.
    pub fn demo(&mut self) {
        self.open = true;
        self.abruf.geholt = Some(Instant::now());
        let start = 1_793_700_000;
        let verlauf = (0..60u64)
            .map(|i| {
                (
                    start + i * 60,
                    (8.0 + 6.0 * ((i as f32) / 9.0).sin()) as u32 + i as u32 / 6,
                )
            })
            .collect();
        self.stand = Stand::Daten(Auswertung {
            jetzt: Some(Jetzt {
                t: start + 3600,
                aktuell: 14,
                spitze_server: Some(31),
                eindeutig: Some(14),
                verbindungen: Some(16),
                quelle: true,
            }),
            verlauf,
            sendungen: vec![
                Sendung {
                    start,
                    ende: start + 3600,
                    dauer_s: 3600,
                    spitze: 18,
                    durchschnitt: 11.4,
                    laeuft: true,
                },
                Sendung {
                    start: start - 86_400,
                    ende: start - 86_400 + 7200,
                    dauer_s: 7200,
                    spitze: 31,
                    durchschnitt: 17.2,
                    laeuft: false,
                },
                Sendung {
                    start: start - 3 * 86_400,
                    ende: start - 3 * 86_400 + 5400,
                    dauer_s: 5400,
                    spitze: 12,
                    durchschnitt: 7.9,
                    laeuft: false,
                },
            ],
        });
    }

    fn poll(&mut self, url: &str) {
        if let Some(r) = self.abruf.poll(url) {
            self.stand = match r {
                Ok(a) => Stand::Daten(a),
                Err(e) => Stand::Fehler(e),
            };
        }
    }

    pub fn show(&mut self, ctx: &egui::Context, url: &str, photo: bool) {
        if !self.open {
            return;
        }
        if !photo {
            self.poll(url);
            ctx.request_repaint_after(Duration::from_secs(1));
        }
        let sub = match &self.stand {
            Stand::Leer => "Lade vom Statistik-Server …".to_string(),
            Stand::Fehler(_) => "Statistik-Server nicht erreichbar".to_string(),
            Stand::Daten(_) => {
                "Gesammelt vom eigenen Server alle 30 s, auch ohne laufende App".into()
            }
        };
        let mut open = self.open;
        let mut max = self.maximized;
        fenster(ctx, "woa-hoerer", max).show(ctx, |ui| {
            kopf(
                ui,
                theme::ic::USERS_THREE,
                "Hörer-Statistik",
                &sub,
                &mut open,
                &mut max,
            );
            match &self.stand {
                Stand::Leer => {
                    ui.spinner();
                }
                Stand::Fehler(e) => {
                    ui.label(egui::RichText::new(e).color(theme::danger_text()));
                    ui.label(
                        egui::RichText::new("Neuer Versuch in 30 s.")
                            .small()
                            .color(theme::text_dim()),
                    );
                }
                Stand::Daten(a) => inhalt(ui, a),
            }
        });
        self.open = open;
        self.maximized = max;
    }
}

fn kachel(ui: &mut egui::Ui, titel: &str, wert: &str) {
    theme::card_frame().show(ui, |ui| {
        ui.set_min_width(130.0);
        ui.vertical(|ui| {
            ui.label(theme::panel_label(titel.to_uppercase()));
            ui.label(
                egui::RichText::new(wert)
                    .font(egui::FontId::monospace(26.0))
                    .color(theme::text()),
            );
        });
    });
}

fn opt(v: Option<u32>) -> String {
    v.map_or("–".into(), |v| v.to_string())
}

fn inhalt(ui: &mut egui::Ui, a: &Auswertung) {
    let laufend = a.sendungen.first().filter(|s| s.laeuft);
    ui.horizontal(|ui| {
        kachel(ui, "Jetzt", &opt(a.jetzt.as_ref().map(Jetzt::hoerer)));
        kachel(ui, "Spitze Sendung", &opt(laufend.map(|s| s.spitze)));
        kachel(
            ui,
            "Schnitt Sendung",
            &laufend.map_or("–".into(), |s| format!("{:.1}", s.durchschnitt)),
        );
        kachel(
            ui,
            "Verbindungen",
            &opt(a.jetzt.as_ref().map(|j| j.verbindungen.unwrap_or(j.aktuell))),
        );
    });
    ui.add_space(theme::SPACE_3);
    theme::card_frame().show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        let titel = if laufend.is_some() {
            "Verlauf laufende Sendung"
        } else {
            "Verlauf letzte Sendung"
        };
        ui.label(theme::panel_label(titel.to_uppercase()));
        verlauf(ui, &a.verlauf);
    });
    ui.add_space(theme::SPACE_3);
    theme::card_frame().show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        ui.label(theme::panel_label("SENDUNGEN (30 TAGE)"));
        if a.sendungen.is_empty() {
            ui.label(egui::RichText::new("Noch keine Sendung gezählt.").color(theme::text_dim()));
            return;
        }
        egui::ScrollArea::vertical()
            .max_height(260.0)
            .show(ui, |ui| {
                egui::Grid::new("hoerer-sendungen")
                    .striped(true)
                    .spacing([theme::SPACE_5, 4.0])
                    .show(ui, |ui| {
                        for h in ["Beginn", "Dauer", "Spitze", "Schnitt"] {
                            ui.label(egui::RichText::new(h).small().color(theme::text_dim()));
                        }
                        ui.end_row();
                        let mono =
                            |s: String| egui::RichText::new(s).font(egui::FontId::monospace(12.0));
                        for s in &a.sendungen {
                            let mut b = datum(s.start);
                            if s.laeuft {
                                b.push_str(" · läuft");
                            }
                            ui.label(mono(b));
                            ui.label(mono(dauer(s.dauer_s)));
                            ui.label(mono(s.spitze.to_string()));
                            ui.label(mono(format!("{:.1}", s.durchschnitt)));
                            ui.end_row();
                        }
                    });
            });
    });
}

/// Punkte des Linienverlaufs in `rect` (rein, damit Tests sie prüfen können).
pub fn line_points(werte: &[(u64, u32)], rect: egui::Rect) -> Vec<egui::Pos2> {
    let (Some(first), Some(last)) = (werte.first(), werte.last()) else {
        return Vec::new();
    };
    let span = (last.0.saturating_sub(first.0)).max(1) as f32;
    let top = werte.iter().map(|w| w.1).max().unwrap_or(0).max(1) as f32;
    werte
        .iter()
        .map(|&(t, n)| {
            egui::pos2(
                rect.left() + rect.width() * (t - first.0) as f32 / span,
                rect.bottom() - rect.height() * n as f32 / top,
            )
        })
        .collect()
}

fn verlauf(ui: &mut egui::Ui, werte: &[(u64, u32)]) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), 120.0),
        egui::Sense::hover(),
    );
    let p = ui.painter();
    p.rect_filled(
        rect,
        egui::CornerRadius::same(theme::RADIUS_SM),
        theme::well(),
    );
    let inner = rect.shrink2(egui::vec2(8.0, 10.0));
    if werte.len() < 2 {
        p.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "Noch zu wenig Proben",
            egui::FontId::proportional(12.0),
            theme::text_dim(),
        );
        return;
    }
    let pts = line_points(werte, inner);
    p.add(egui::Shape::line(pts, egui::Stroke::new(2.0, theme::ok())));
    let top = werte.iter().map(|w| w.1).max().unwrap_or(0);
    p.text(
        inner.left_top(),
        egui::Align2::LEFT_TOP,
        format!("{top}"),
        egui::FontId::monospace(11.0),
        theme::text_dim(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    #[test]
    fn chip_shows_current_and_session_peak_or_dash() {
        assert_eq!(chip_suffix(Some(12), Some(18)), "12 | 18");
        assert_eq!(chip_suffix(Some(20), Some(18)), "20 | 20");
        assert_eq!(chip_suffix(Some(3), None), "3 | 3");
        assert_eq!(chip_suffix(None, Some(5)), "–");
    }

    #[test]
    fn chip_numbers_come_from_the_server_answer() {
        let a = parse(
            r#"{"jetzt":{"t":1,"aktuell":7},"sendungen":[{"start":0,"ende":1,"dauer_s":1,"spitze":12,"durchschnitt":5.0,"laeuft":true}]}"#,
        )
        .unwrap();
        assert_eq!(chip_zahlen(&a), (Some(7), Some(12)));
        let alt = parse(
            r#"{"jetzt":{"t":1,"aktuell":0},"sendungen":[{"start":0,"ende":1,"dauer_s":1,"spitze":12,"durchschnitt":5.0}]}"#,
        )
        .unwrap();
        assert_eq!(chip_zahlen(&alt), (Some(0), None));
        assert_eq!(chip_zahlen(&Auswertung::default()), (None, None));
    }

    #[test]
    fn chip_stays_dash_without_test_server() {
        let mut c = ChipStand::default();
        c.poll("https://feedback.wironair.de/api/hoerer");
        for _ in 0..100 {
            c.poll("https://feedback.wironair.de/api/hoerer");
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(c.aktuell, None);
        assert_eq!(chip_suffix(c.aktuell, None), "–");
    }

    #[test]
    fn session_peak_resets_with_each_broadcast() {
        let mut s = SessionPeak::default();
        assert_eq!(s.update(true, Some(4)), Some(4));
        assert_eq!(s.update(true, Some(9)), Some(9));
        assert_eq!(s.update(true, None), Some(9));
        s.update(false, None);
        assert_eq!(s.update(true, Some(2)), Some(2));
    }

    #[test]
    fn date_and_duration() {
        assert_eq!(datum(0), "01.01.1970 00:00 UTC");
        assert_eq!(datum(1_793_664_000), "03.11.2026 00:00 UTC");
        assert_eq!(dauer(5400), "1:30");
    }

    #[test]
    fn line_fills_the_rect() {
        let r = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(100.0, 50.0));
        let p = line_points(&[(10, 0), (20, 5), (30, 10)], r);
        assert_eq!(p[0], egui::pos2(0.0, 50.0));
        assert_eq!(p[2], egui::pos2(100.0, 0.0));
        assert!(line_points(&[], r).is_empty());
    }

    /// Attrappe des Statistik-Servers auf 127.0.0.1, eine Antwort.
    fn attrappe(status: &str, body: &'static str) -> String {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let status = status.to_string();
        std::thread::spawn(move || {
            if let Ok((mut s, _)) = l.accept() {
                let mut buf = [0u8; 2048];
                let _ = s.read(&mut buf);
                let _ = write!(
                    s,
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
            }
        });
        format!("http://127.0.0.1:{port}/api/hoerer")
    }

    #[test]
    fn fetch_reads_the_server_answer() {
        let url = attrappe(
            "200 OK",
            r#"{"jetzt":{"t":5,"aktuell":3,"spitze_server":7,"eindeutig":4,"quelle":true},
               "verlauf":[[1,1],[2,5],[3,3]],
               "sendungen":[{"start":1,"ende":3,"dauer_s":2,"spitze":5,"durchschnitt":3.0,"laeuft":true}]}"#,
        );
        let a = fetch(&url).unwrap();
        let j = a.jetzt.unwrap();
        assert_eq!(j.aktuell, 3);
        // 04.11.2026: Hörer = eindeutig, nicht Verbindungen.
        assert_eq!(j.hoerer(), 4);
        assert_eq!(chip_zahlen(&parse(r#"{"jetzt":{"t":5,"aktuell":2,"eindeutig":1}}"#).unwrap()).0, Some(1));
        assert_eq!(chip_zahlen(&parse(r#"{"jetzt":{"t":5,"aktuell":2}}"#).unwrap()).0, Some(2));
        assert_eq!(a.verlauf, vec![(1, 1), (2, 5), (3, 3)]);
        assert_eq!(a.sendungen[0].spitze, 5);
    }

    #[test]
    fn offline_or_broken_gives_a_clear_message() {
        let url = attrappe("502 Bad Gateway", "{}");
        assert!(fetch(&url).unwrap_err().contains("nicht erreichbar"));
        assert!(
            fetch("http://127.0.0.1:9/api/hoerer")
                .unwrap_err()
                .contains("nicht erreichbar")
        );
        assert!(fetch("").unwrap_err().contains("Kein Statistik-Server"));
        assert!(fetch("https://example.org/api/hoerer").is_err());
        let url = attrappe("200 OK", "kein json");
        assert!(fetch(&url).unwrap_err().contains("nicht lesbar"));
    }
}
