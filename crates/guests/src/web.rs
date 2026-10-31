//! Browser-Gäste (Paket 38): verbindet Einladung und Warteraum ([`crate::gate`]), den kleinen
//! HTTPS-Server ([`crate::server`]) und den Netz-Thread ([`crate::net`]) mit den Gast-Kanälen
//! des [`GuestHub`].
//!
//! Der Control-Thread ruft [`GuestHub::web_start`], [`GuestHub::web_invite`],
//! [`GuestHub::web_accept`] und [`GuestHub::poll`]; nichts davon läuft im Audio-Thread.
//!
//! Ohne TURN-Relay: nur direkte Verbindungen im selben Netz. Ein Relay wäre ein bezahlter
//! Dienst und ist Andrés Entscheidung (siehe `docs/abnahme.md`).

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Instant;

use crate::gate::{Gate, InviteView, Waiting};
use crate::hub::GuestHub;
use crate::net::{GuestNet, NetConfig, NetEvent, PeerId, PeerShared};
use crate::server::WebServer;

/// Bevorzugte Ports; sind sie belegt, sucht das System freie.
pub const PREFERRED_UDP_PORT: u16 = 47600;
pub const PREFERRED_HTTPS_PORT: u16 = 47601;

/// Einstellungen beim Einschalten.
#[derive(Debug, Clone)]
pub struct WebConfig {
    /// Die LAN-Adresse dieses Rechners. Auf ihr hören HTTPS und UDP; sie steht im Link.
    pub ip: IpAddr,
    /// Bevorzugte Ports, 0: gleich das System wählen lassen.
    pub udp_port: u16,
    pub https_port: u16,
    /// Rate der Engine (Hz).
    pub engine_rate: u32,
}

/// Messwerte eines verbundenen Browser-Gastes für die Anzeige.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NetView {
    /// Die Verbindung steht (sonst: wird aufgebaut).
    pub connected: bool,
    /// Round-Trip-Zeit in ms, 0: noch unbekannt.
    pub rtt_ms: u32,
    /// Verlust des Gast-Mikros in Promille.
    pub loss_permille: u32,
}

/// Was die Oberfläche vom Browser-Zugang zeigt.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WebView {
    pub running: bool,
    /// Offene Einladungen: vollständiger Link und Restzeit.
    pub invites: Vec<(String, InviteView)>,
    pub waiting: Vec<Waiting>,
}

struct PeerInfo {
    ticket: String,
    slot: usize,
    shared: Arc<PeerShared>,
}

/// Der laufende Browser-Zugang.
pub struct WebGuests {
    gate: Gate,
    net: GuestNet,
    server: WebServer,
    peers: HashMap<PeerId, PeerInfo>,
    next_id: PeerId,
}

impl WebGuests {
    fn start(cfg: &WebConfig) -> Result<Self, String> {
        let bind = |port: u16| {
            GuestNet::start(NetConfig {
                engine_rate: cfg.engine_rate,
                bind: SocketAddr::new(cfg.ip, port),
            })
        };
        let net = bind(cfg.udp_port).or_else(|_| bind(0))?;
        let gate = Gate::new();
        let server = WebServer::start(cfg.ip, cfg.https_port, gate.clone())?;
        tracing::info!(udp = %net.local_addr(), https = %server.addr(), "guest web: on");
        Ok(Self {
            gate,
            net,
            server,
            peers: HashMap::new(),
            next_id: 1,
        })
    }

    fn link(&self, token: &str) -> String {
        format!("https://{}/gast?i={token}", self.server.addr())
    }

    fn slot_of(&self, ticket: &str) -> Option<PeerId> {
        self.peers
            .iter()
            .find(|(_, p)| p.ticket == ticket)
            .map(|(id, _)| *id)
    }
}

impl GuestHub {
    /// Schaltet den Zugang für Browser-Gäste ein. Zweimal einschalten ist harmlos.
    pub fn web_start(&mut self, cfg: &WebConfig) -> Result<(), String> {
        if self.web.is_some() {
            return Ok(());
        }
        self.web = Some(WebGuests::start(cfg)?);
        Ok(())
    }

    /// Schaltet den Zugang aus: alle Browser-Gäste werden getrennt, die Ports sind zu.
    pub fn web_stop(&mut self) {
        let Some(mut w) = self.web.take() else { return };
        for ev in w.net.stop() {
            self.on_net_event(&mut w, ev);
        }
        tracing::info!("guest web: off");
    }

    pub fn web_running(&self) -> bool {
        self.web.is_some()
    }

    /// Neue Einladung; gibt den Link zurück.
    pub fn web_invite(&mut self) -> Option<String> {
        let w = self.web.as_ref()?;
        let token = w.gate.create_invite(Instant::now())?;
        tracing::info!("guest web: invite created");
        Some(w.link(&token))
    }

    /// Zieht eine Einladung zurück (Zeichen nach `?i=` im Link).
    pub fn web_revoke(&mut self, token: &str) {
        if let Some(w) = &self.web {
            w.gate.revoke_invite(token);
        }
    }

    /// Nimmt einen Wartenden an: Kanal belegen, Verbindung vorbereiten, Antwort an den Gast.
    pub fn web_accept(&mut self, ticket: &str) -> Result<(), String> {
        let Some(mut w) = self.web.take() else {
            return Err("Browser-Zugang ist aus".into());
        };
        let r = self.accept_inner(&mut w, ticket);
        self.web = Some(w);
        if let Err(e) = &r {
            tracing::warn!(error = %e, "guest web: accept failed");
        }
        r
    }

    fn accept_inner(&mut self, w: &mut WebGuests, ticket: &str) -> Result<(), String> {
        let (name, offer) = w
            .gate
            .take_for_accept(ticket)
            .ok_or_else(|| "Der Gast ist nicht mehr im Warteraum".to_string())?;
        let guest = match self.add_network("Browser (WebRTC)", &name) {
            Ok(g) => g,
            Err(e) => {
                w.gate.fail(ticket);
                return Err(e);
            }
        };
        let slot = guest.slot;
        let id = w.next_id;
        w.next_id += 1;
        match w.net.add_peer(id, &offer, guest) {
            Ok((answer, shared)) => {
                w.peers.insert(
                    id,
                    PeerInfo {
                        ticket: ticket.to_string(),
                        slot,
                        shared,
                    },
                );
                w.gate.set_answer(ticket, answer);
                tracing::info!(slot, "guest web: accepted (waiting room)");
                Ok(())
            }
            Err((e, guest)) => {
                w.gate.fail(ticket);
                // Kanal wieder freigeben; ohne den Gast bleibt er zu (Feed fehlt).
                self.remove(slot);
                if let Some(g) = guest {
                    self.return_network(g);
                }
                Err(e)
            }
        }
    }

    /// Lehnt einen Wartenden ab.
    pub fn web_reject(&mut self, ticket: &str) {
        if let Some(w) = &self.web {
            w.gate.reject(ticket);
            tracing::info!("guest web: rejected");
        }
    }

    /// Anzeige: Einladungen und Warteraum.
    pub fn web_view(&self) -> WebView {
        let Some(w) = &self.web else {
            return WebView::default();
        };
        let now = Instant::now();
        WebView {
            running: true,
            invites: w
                .gate
                .invites(now)
                .into_iter()
                .map(|i| (w.link(&i.token), i))
                .collect(),
            waiting: w.gate.waiting(now),
        }
    }

    /// Verbindungswerte des Browser-Gastes auf diesem Kanal.
    pub(crate) fn net_view(&self, slot: usize) -> Option<NetView> {
        let w = self.web.as_ref()?;
        let p = w.peers.values().find(|p| p.slot == slot)?;
        Some(NetView {
            connected: p.shared.connected.load(Ordering::Relaxed),
            rtt_ms: p.shared.rtt_ms.load(Ordering::Relaxed),
            loss_permille: p.shared.loss_permille.load(Ordering::Relaxed),
        })
    }

    /// Trennt den Browser-Gast auf diesem Kanal (falls es einer ist).
    pub(crate) fn web_hangup_slot(&mut self, slot: usize) {
        if let Some(w) = &self.web
            && let Some((id, _)) = w.peers.iter().find(|(_, p)| p.slot == slot)
        {
            w.net.remove_peer(*id);
        }
    }

    /// Im Control-Takt: Ereignisse des Netz-Threads, aufgelegte Gäste, Aufräumen.
    pub(crate) fn web_tick(&mut self) {
        let Some(mut w) = self.web.take() else { return };
        for ticket in w.gate.take_left() {
            if let Some(id) = w.slot_of(&ticket) {
                w.net.remove_peer(id);
            }
        }
        for ev in w.net.poll() {
            self.on_net_event(&mut w, ev);
        }
        // Sendestand an die Seite des Gastes, Selbst-Stumm von ihr zurück.
        for p in w.peers.values() {
            if let Some(air) = self.air_of(p.slot) {
                w.gate.set_air(&p.ticket, air);
            }
            let me = w.gate.self_muted(&p.ticket);
            self.set_self_muted(p.slot, me);
        }
        for _ in w.gate.sweep(Instant::now()) {
            tracing::info!("guest web: waiting guest gone silent");
        }
        self.web = Some(w);
    }

    fn on_net_event(&mut self, w: &mut WebGuests, ev: NetEvent) {
        match ev {
            NetEvent::Connected { id } => {
                if let Some(p) = w.peers.get(&id) {
                    tracing::info!(slot = p.slot, "guest web: guest is live");
                }
            }
            NetEvent::Gone { id, reason, guest } => {
                let info = w.peers.remove(&id);
                if let Some(p) = &info {
                    w.gate.end(&p.ticket);
                }
                tracing::info!(id, %reason, "guest web: guest left");
                // Erst den Kanal abschalten, dann Feed und Rückweg zurückgeben.
                self.remove(guest.slot);
                self.return_network(*guest);
            }
        }
    }
}
