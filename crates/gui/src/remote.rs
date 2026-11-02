//! Fernbedienung im Party-Layout (Paket 36): Karte mit An/Aus, QR-Code und PINs. Der Server
//! selbst steht in `woa-remote`; hier wird nur gezeichnet und der Zustand hin- und hergereicht.
//!
//! Befehle vom Handy kommen als `PartyCommand` und laufen durch dieselbe Schicht wie die großen
//! Tasten (`PartyView::run`). Die Fernbedienung ist bei jedem Programmstart aus.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui;
use woa_radio::party::{PartyCommand, PartyState};
use woa_remote::bridge::{CatalogSig, catalog, catalog_signature, snapshot};
use woa_remote::qr::{QrMatrix, qr};
use woa_remote::{Limits, Remote, RemoteConfig};

use crate::radio::RadioDesk;
use crate::theme;

/// Fester Port, damit der QR-Code vom letzten Mal weiter passt; ist er belegt, sucht das System einen.
const PREFERRED_PORT: u16 = 8787;
/// OSC-Port (TouchOSC & Co.).
const OSC_PORT: u16 = 9000;
/// So oft geht der Zustand höchstens an die Handys.
const PUBLISH_EVERY: Duration = Duration::from_millis(250);
/// Kantenlänge des QR-Codes in der Karte.
const QR_SIDE: f32 = 120.0;
/// Kantenlänge des QR-Codes in der kompakten Karte.
const QR_SMALL: f32 = 64.0;
/// Kantenlänge des großen QR-Codes über der Ansicht.
const QR_BIG: f32 = 300.0;
/// Unter dieser Fensterhöhe ist die Karte kompakt (eine Zeile).
pub const COMPACT_BELOW: f32 = 900.0;

/// Was die Karte zeigt: Adresse, PIN, Handyzahl. Im Fotomodus (`WOA_SCREENSHOT`) stehen
/// Platzhalter da, damit keine LAN-Adresse und keine PIN in Bildern landen.
struct Shown {
    url: String,
    host: String,
    pin: String,
    phones: usize,
}

/// Platzhalter für Bilder.
fn photo_mode() -> bool {
    std::env::var_os("WOA_SCREENSHOT").is_some()
}

/// Kompakte Karte bei dieser Fensterhöhe?
pub fn compact(window_h: f32) -> bool {
    window_h < COMPACT_BELOW
}

/// Zustand der Karte.
#[derive(Default)]
pub struct RemoteView {
    remote: Option<Remote>,
    error: Option<String>,
    osc_on: bool,
    show_dj: bool,
    last_publish: Option<Instant>,
    signature: Option<CatalogSig>,
    qr: Option<(String, QrMatrix)>,
    /// QR-Code groß über der Ansicht (RemoteCard: Klick auf den QR, Symbol oder Taste Q).
    qr_big: bool,
}

impl RemoteView {
    pub fn is_on(&self) -> bool {
        self.remote.is_some()
    }

    /// Schaltet ein. `ctx` weckt die Oberfläche, wenn ein Befehl eintrifft.
    pub fn start(&mut self, ctx: &egui::Context) {
        self.error = None;
        let Some(ip) = woa_remote::lan_ip() else {
            self.error =
                Some("Kein Netzwerk gefunden. Mit dem WLAN verbinden und noch einmal.".into());
            return;
        };
        let wake_ctx = ctx.clone();
        let make = |port: u16| RemoteConfig {
            bind: SocketAddr::new(ip, port),
            osc: self.osc_on.then(|| SocketAddr::new(ip, OSC_PORT)),
            limits: Limits::default(),
            wake: Some(Arc::new({
                let c = wake_ctx.clone();
                move || c.request_repaint()
            })),
            pins: None,
        };
        let started = Remote::start(make(PREFERRED_PORT)).or_else(|e| {
            tracing::warn!(error = %e, "remote: preferred port busy, letting the system choose");
            Remote::start(make(0))
        });
        match started {
            Ok(r) => {
                self.remote = Some(r);
                self.signature = None;
                self.last_publish = None;
                self.qr = None;
            }
            Err(e) => {
                tracing::error!(error = %e, "remote: start failed");
                self.error = Some(format!("Fernbedienung startet nicht: {e}"));
            }
        }
    }

    /// Schaltet aus: Port zu, Handys bekommen „getrennt“.
    pub fn stop(&mut self) {
        if let Some(mut r) = self.remote.take() {
            r.stop();
        }
        self.qr = None;
        self.show_dj = false;
    }

    /// Jeden Frame: Befehle der Handys abholen, Zustand veröffentlichen.
    pub fn pump(
        &mut self,
        state: &PartyState,
        desk: &RadioDesk,
        ctx: &egui::Context,
    ) -> Vec<PartyCommand> {
        let Some(r) = &self.remote else {
            return Vec::new();
        };
        let cmds: Vec<PartyCommand> = r.poll().into_iter().map(|c| c.command).collect();
        let due = self
            .last_publish
            .is_none_or(|t| t.elapsed() >= PUBLISH_EVERY);
        if due || !cmds.is_empty() {
            let view = desk.view();
            let sig = catalog_signature(view);
            if self.signature.as_ref() != Some(&sig) {
                r.set_catalog(&catalog(view));
                self.signature = Some(sig);
            }
            r.publish(&snapshot(&woa_radio::party::layout(view, state), state));
            self.last_publish = Some(Instant::now());
        }
        // Fortschritt und Restzeit laufen auch ohne Eingabe weiter.
        ctx.request_repaint_after(PUBLISH_EVERY);
        cmds
    }

    /// Zeichnet die Karte immer kompakt (DJ-Ansicht: unten ist wenig Platz).
    pub fn ui_small(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        self.qr_keys(&ctx);
        self.qr_overlay(&ctx);
        self.ui_compact(ui);
    }

    /// Zeichnet die Karte.
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        self.qr_keys(&ctx);
        self.qr_overlay(&ctx);
        if compact(ctx.content_rect().height()) {
            self.ui_compact(ui);
            return;
        }
        theme::card_frame()
            .inner_margin(egui::Margin::same(14))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(theme::panel_label("FERNBEDIENUNG").color(theme::accent_2()));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        // An/Aus als ein Schalter: an = gefüllt (Entwurf ScreenParty).
                        let on = self.remote.is_some();
                        let label = format!("{} AN", theme::ic::WIFI_HIGH);
                        let r = crate::widgets::Key::new(&label, egui::vec2(72.0, 32.0))
                            .font(theme::label_font(14.0))
                            .radius(theme::RADIUS_MD)
                            .color(theme::accent_2())
                            .ink(theme::on_accent_2())
                            .on(on)
                            .ui(ui)
                            .on_hover_text(if on {
                                "Fernbedienung im WLAN ist an. Klick: aus (Handys werden getrennt)"
                            } else {
                                "Fernbedienung im WLAN einschalten"
                            });
                        if r.clicked() {
                            if on {
                                self.stop();
                            } else {
                                self.start(&ctx);
                            }
                        }
                    });
                });
                if self.remote.is_none() {
                    self.ui_off(ui);
                } else {
                    self.ui_on(ui);
                }
                if let Some(e) = &self.error {
                    ui.label(egui::RichText::new(e).color(theme::danger_text()));
                }
            });
    }

    /// Adresse, PIN und Handyzahl (Platzhalter im Fotomodus).
    fn shown(&self) -> Option<Shown> {
        let r = self.remote.as_ref()?;
        if photo_mode() {
            return Some(Shown {
                url: "http://woa.local:8080/?pin=4711".into(),
                host: "woa.local:8080".into(),
                pin: "4711".into(),
                phones: 9,
            });
        }
        let pin = r.guest_pin();
        let url = r.join_url(&pin);
        let host = url
            .split("/?")
            .next()
            .unwrap_or("")
            .trim_start_matches("http://")
            .trim_start_matches("https://")
            .trim_end_matches('/')
            .to_string();
        Some(Shown {
            url,
            host,
            pin,
            phones: r.clients(),
        })
    }

    /// Taste Q zeigt den QR groß, Esc schließt (nur ohne Textfokus und mit laufender Karte).
    fn qr_keys(&mut self, ctx: &egui::Context) {
        if self.remote.is_none() {
            self.qr_big = false;
            return;
        }
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        ctx.input_mut(|i| {
            if i.consume_key(egui::Modifiers::NONE, egui::Key::Q) {
                self.qr_big = !self.qr_big;
            }
            if self.qr_big && i.consume_key(egui::Modifiers::NONE, egui::Key::Escape) {
                self.qr_big = false;
            }
        });
    }

    /// Großer QR-Code (300 px) über der Ansicht; Klick irgendwo schließt.
    fn qr_overlay(&mut self, ctx: &egui::Context) {
        if !self.qr_big {
            return;
        }
        let Some(sh) = self.shown() else { return };
        self.refresh_qr(&sh.url);
        let Some((_, m)) = &self.qr else { return };
        let screen = ctx.content_rect();
        let mut close = false;
        egui::Area::new(egui::Id::new("remote-qr-big"))
            .order(egui::Order::Foreground)
            .fixed_pos(screen.min)
            .show(ctx, |ui| {
                let (rect, resp) = ui.allocate_exact_size(screen.size(), egui::Sense::click());
                ui.painter()
                    .rect_filled(rect, 0.0, egui::Color32::from_black_alpha(170));
                let card = egui::Rect::from_center_size(
                    rect.center(),
                    egui::vec2(QR_BIG + 40.0, QR_BIG + 90.0),
                );
                ui.painter().rect_filled(card, 8.0, theme::panel());
                let mut child = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(card.shrink(20.0))
                        .layout(egui::Layout::top_down(egui::Align::Center)),
                );
                draw_qr(&mut child, m, QR_BIG);
                child.label(
                    egui::RichText::new(format!("{} · PIN {}", sh.host, sh.pin))
                        .monospace()
                        .size(16.0)
                        .color(theme::text()),
                );
                child.label(
                    egui::RichText::new("Klick oder Esc schließt")
                        .size(12.0)
                        .color(theme::text_dim()),
                );
                close = resp.clicked();
            });
        if close {
            self.qr_big = false;
        }
    }

    fn refresh_qr(&mut self, url: &str) {
        if self.qr.as_ref().map(|(u, _)| u.as_str()) != Some(url) {
            self.qr = qr(url).map(|m| (url.to_string(), m));
        }
    }

    /// Kompakte Karte (Fensterhöhe unter 900 px): eine Zeile mit QR 64 px, Adresse · PIN,
    /// Handyzahl, „QR groß“ und „An“.
    fn ui_compact(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        theme::card_frame()
            .inner_margin(egui::Margin::same(10))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 12.0;
                    let shown = self.shown();
                    if let Some(sh) = &shown {
                        self.refresh_qr(&sh.url);
                        if let Some((_, m)) = &self.qr {
                            let r = draw_qr(ui, m, QR_SMALL);
                            let r = ui
                                .interact(
                                    r.rect,
                                    egui::Id::new("remote-qr-small"),
                                    egui::Sense::click(),
                                )
                                .on_hover_text("QR groß zeigen (Taste Q)");
                            if r.clicked() {
                                self.qr_big = true;
                            }
                        }
                    }
                    ui.vertical(|ui| {
                        ui.label(theme::panel_label("FERNBEDIENUNG").color(theme::accent_2()));
                        match &shown {
                            Some(sh) => {
                                ui.label(
                                    egui::RichText::new(format!("{} · PIN {}", sh.host, sh.pin))
                                        .monospace()
                                        .size(12.0)
                                        .color(theme::text_dim()),
                                );
                                theme::status_pill(
                                    ui,
                                    &format!(
                                        "{} {}",
                                        sh.phones,
                                        if sh.phones == 1 { "Handy" } else { "Handys" }
                                    ),
                                    theme::ok(),
                                    sh.phones > 0,
                                );
                            }
                            None => {
                                ui.label(
                                    egui::RichText::new("Handys im WLAN wünschen Titel")
                                        .size(12.0)
                                        .color(theme::text_dim()),
                                );
                            }
                        }
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let on = self.remote.is_some();
                        let label = format!("{} AN", theme::ic::WIFI_HIGH);
                        let r = crate::widgets::Key::new(&label, egui::vec2(72.0, 32.0))
                            .font(theme::label_font(14.0))
                            .radius(theme::RADIUS_MD)
                            .color(theme::accent_2())
                            .ink(theme::on_accent_2())
                            .on(on)
                            .ui(ui)
                            .on_hover_text(if on {
                                "Fernbedienung im WLAN ist an. Klick: aus (Handys werden getrennt)"
                            } else {
                                "Fernbedienung im WLAN einschalten"
                            });
                        if r.clicked() {
                            if on {
                                self.stop();
                            } else {
                                self.start(&ctx);
                            }
                        }
                        if on
                            && ui
                                .add(egui::Button::new(theme::ic::ARROWS_OUT_SIMPLE))
                                .on_hover_text("QR groß (Taste Q)")
                                .clicked()
                        {
                            self.qr_big = true;
                        }
                    });
                });
                if let Some(e) = &self.error {
                    ui.label(egui::RichText::new(e).color(theme::danger_text()));
                }
            });
    }

    fn ui_off(&mut self, ui: &mut egui::Ui) {
        ui.label(
            egui::RichText::new("Handys im WLAN zeigen die Party und wünschen Titel.")
                .color(theme::text_dim()),
        );
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(egui::RichText::new("OSC-Eingang").size(13.0));
                ui.label(
                    egui::RichText::new(format!("Port {OSC_PORT}, ohne PIN, volle DJ-Rechte"))
                        .size(11.0)
                        .color(theme::text_dim()),
                );
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add(egui::Checkbox::without_text(&mut self.osc_on))
                    .on_hover_text("OSC-Eingang beim Einschalten mit öffnen (TouchOSC & Co.)");
            });
        });
    }

    fn ui_on(&mut self, ui: &mut egui::Ui) {
        let Some(r) = &self.remote else { return };
        let (dj_pin, dj_url, osc) = if photo_mode() {
            (
                "0000".to_string(),
                "http://woa.local:8080/?pin=0000".to_string(),
                None,
            )
        } else {
            let p = r.dj_pin();
            let u = r.join_url(&p);
            (p, u, r.osc_addr())
        };
        let Some(sh) = self.shown() else { return };
        let (guest_url, pin, phones, host) = (sh.url, sh.pin, sh.phones, sh.host);
        let plain = guest_url.split("/?").next().unwrap_or("").to_string();
        self.refresh_qr(&guest_url);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 14.0;
            if let Some((_, m)) = &self.qr {
                draw_qr(ui, m, QR_SIDE).on_hover_text(&plain);
            }
            ui.vertical(|ui| {
                ui.label(egui::RichText::new("Handy scannen").size(15.0).strong());
                ui.label(
                    egui::RichText::new(format!("{host} · PIN {pin}"))
                        .monospace()
                        .size(12.0)
                        .color(theme::text_dim()),
                );
                theme::status_pill(
                    ui,
                    &format!("{phones} {}", if phones == 1 { "Handy" } else { "Handys" }),
                    theme::ok(),
                    phones > 0,
                );
                if let Some(o) = osc {
                    ui.label(
                        egui::RichText::new(format!("OSC an {o}"))
                            .small()
                            .color(theme::danger_text()),
                    );
                }
            });
        });
        if self.show_dj {
            ui.label(
                egui::RichText::new(format!("DJ-PIN {dj_pin}"))
                    .size(18.0)
                    .monospace()
                    .strong()
                    .color(theme::accent()),
            );
            ui.label(
                egui::RichText::new("Nur für dich: wer die DJ-PIN kennt, steuert die Party.")
                    .small()
                    .color(theme::text_dim()),
            );
            ui.add(
                egui::Label::new(
                    egui::RichText::new(&dj_url)
                        .small()
                        .monospace()
                        .color(theme::text_dim()),
                )
                .truncate(),
            );
        }
        let mut renew = false;
        ui.horizontal(|ui| {
            let t = if self.show_dj {
                "DJ-PIN verbergen"
            } else {
                "DJ-PIN zeigen"
            };
            if ui.button(t).clicked() {
                self.show_dj = !self.show_dj;
            }
            if ui
                .button("Neue PINs")
                .on_hover_text("Alle Handys werden getrennt und müssen neu scannen")
                .clicked()
            {
                renew = true;
            }
        });
        if renew && let Some(r) = &self.remote {
            r.renew_pins();
        }
    }
}

/// Zeichnet den QR-Code mit hellem Rand (ohne ihn lesen Kameras schlecht).
pub(crate) fn draw_qr(ui: &mut egui::Ui, m: &QrMatrix, side: f32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(side, side), egui::Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 4.0, egui::Color32::WHITE);
    let quiet = 4.0;
    let cell = side / (m.size as f32 + 2.0 * quiet);
    for y in 0..m.size {
        for x in 0..m.size {
            if m.dark(x, y) {
                let min =
                    rect.min + egui::vec2((x as f32 + quiet) * cell, (y as f32 + quiet) * cell);
                p.rect_filled(
                    egui::Rect::from_min_size(min, egui::vec2(cell + 0.5, cell + 0.5)),
                    0.0,
                    egui::Color32::BLACK,
                );
            }
        }
    }
    resp
}

#[cfg(test)]
mod compact_tests {
    use super::*;

    /// RemoteCard: unter 900 px Fensterhöhe kompakt, bei 1030 px (1920 × 1030) groß.
    #[test]
    fn remote_card_is_compact_below_900_px() {
        assert!(compact(768.0));
        assert!(compact(899.0));
        assert!(!compact(900.0));
        assert!(!compact(1030.0));
        assert_eq!(QR_SMALL, 64.0);
        assert_eq!(QR_BIG, 300.0);
    }
}
