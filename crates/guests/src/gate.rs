//! Einladungen und Warteraum der Browser-Gäste. Reine Logik ohne Netz und ohne Uhrzeit-Zugriff
//! von außen: jede Methode bekommt die Zeit mit, damit die Tests sie vorspulen können.
//!
//! Ablauf: Der Moderator erzeugt eine Einladung (ein Zufallswert, 30 Minuten gültig, einmal
//! benutzbar). Der Gast öffnet den Link und schickt Name und SDP-Angebot. Er steht dann im
//! Warteraum; erst wenn der Moderator annimmt, entsteht Kanal und Verbindung. Lehnt er ab oder
//! meldet sich der Gast nicht mehr (Tab zu), verschwindet der Eintrag.
//!
//! Der Gate liegt hinter einem `Mutex`. Er wird nur vom Web-Server-Thread und vom Control-Thread
//! benutzt, nie vom Audio-Thread.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// So lange gilt eine Einladung.
pub const INVITE_TTL: Duration = Duration::from_secs(30 * 60);
/// So lange darf ein Gast im Warteraum bleiben.
pub const WAIT_TTL: Duration = Duration::from_secs(10 * 60);
/// Meldet sich die Seite so lange nicht, ist der Gast weg.
pub const SILENCE_TTL: Duration = Duration::from_secs(20);
/// Warteraum und offene Einladungen sind begrenzt.
pub const MAX_WAITING: usize = 8;
pub const MAX_INVITES: usize = 16;
/// Längste Namen (Zeichen).
pub const MAX_NAME: usize = 24;
/// Größtes SDP-Angebot (Bytes).
pub const MAX_SDP: usize = 24 * 1024;

/// Stand eines Gastes aus Sicht seiner Seite.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TicketState {
    /// Wartet auf die Entscheidung des Moderators.
    Waiting,
    /// Angenommen; die Antwort (SDP) liegt bei.
    Accepted(String),
    /// Abgelehnt.
    Rejected,
    /// Zu Ende (aufgelegt, Verbindung weg, abgelaufen).
    Ended,
}

/// Ein Gast im Warteraum, wie die Oberfläche ihn sieht.
#[derive(Debug, Clone, PartialEq)]
pub struct Waiting {
    pub ticket: String,
    pub name: String,
    /// Adresse des Handys (nur Anzeige).
    pub peer: String,
    /// Wartet seit so vielen Sekunden.
    pub waited_s: u64,
}

/// Eine offene Einladung, wie die Oberfläche sie sieht.
#[derive(Debug, Clone, PartialEq)]
pub struct InviteView {
    pub token: String,
    /// Gültig noch so viele Sekunden.
    pub left_s: u64,
}

struct Invite {
    expires: Instant,
}

struct Entry {
    name: String,
    peer: String,
    offer: String,
    since: Instant,
    seen: Instant,
    state: TicketState,
    /// Vom Moderator angenommen, die Verbindung wird gerade aufgebaut.
    taken: bool,
    /// Der Gast hat aufgelegt.
    left: bool,
    /// Sendestand für die Seite des Gastes (vom Control-Thread gesetzt).
    air: Air,
    /// Der Gast hat sich selbst stumm geschaltet (von seiner Seite gemeldet).
    self_muted: bool,
}

/// Sendestand eines angenommenen Gastes, wie seine Seite ihn zeigt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Air {
    /// Verbunden, hört die Sendung, ist aber nicht auf Sendung (Warteraum).
    #[default]
    Waiting,
    /// Auf Sendung.
    OnAir,
    /// Vom Moderator stumm geschaltet.
    Muted,
}

impl Air {
    pub fn as_str(self) -> &'static str {
        match self {
            Air::Waiting => "wait",
            Air::OnAir => "air",
            Air::Muted => "muted",
        }
    }
}

#[derive(Default)]
struct Inner {
    invites: HashMap<String, Invite>,
    tickets: HashMap<String, Entry>,
}

/// Geteilter Griff auf Einladungen und Warteraum.
#[derive(Clone, Default)]
pub struct Gate {
    inner: Arc<Mutex<Inner>>,
}

/// Zufallswert als Hex (16 Bytes). `None`, wenn das System keinen liefert.
fn random_hex() -> Option<String> {
    let mut b = [0u8; 16];
    getrandom::fill(&mut b).ok()?;
    Some(b.iter().map(|x| format!("{x:02x}")).collect())
}

/// Macht aus einem Gastnamen einen anzeigbaren: ohne Steuerzeichen, gekürzt.
pub fn clean_name(raw: &str) -> String {
    raw.chars()
        .filter(|c| !c.is_control())
        .collect::<String>()
        .trim()
        .chars()
        .take(MAX_NAME)
        .collect()
}

impl Gate {
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        // Ein vergifteter Lock hält trotzdem gültige Daten (nur einfache Einträge).
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Neue Einladung. `None`, wenn es schon zu viele offene gibt oder kein Zufall da ist.
    pub fn create_invite(&self, now: Instant) -> Option<String> {
        let mut g = self.lock();
        g.invites.retain(|_, i| i.expires > now);
        if g.invites.len() >= MAX_INVITES {
            return None;
        }
        let token = random_hex()?;
        g.invites.insert(
            token.clone(),
            Invite {
                expires: now + INVITE_TTL,
            },
        );
        Some(token)
    }

    /// Zieht eine Einladung zurück.
    pub fn revoke_invite(&self, token: &str) {
        self.lock().invites.remove(token);
    }

    /// Prüft, ob eine Einladung noch gilt (ohne sie zu verbrauchen).
    pub fn invite_valid(&self, token: &str, now: Instant) -> bool {
        self.lock()
            .invites
            .get(token)
            .is_some_and(|i| i.expires > now)
    }

    /// Der Gast tritt bei: verbraucht die Einladung und stellt ihn in den Warteraum.
    pub fn join(
        &self,
        token: &str,
        name: &str,
        offer: &str,
        peer: &str,
        now: Instant,
    ) -> Result<String, &'static str> {
        if offer.len() > MAX_SDP || !offer.starts_with("v=0") {
            return Err("Ungültiges Angebot");
        }
        let mut g = self.lock();
        match g.invites.get(token) {
            Some(i) if i.expires > now => {}
            _ => return Err("Einladung abgelaufen oder schon benutzt"),
        }
        let waiting = g
            .tickets
            .values()
            .filter(|t| t.state == TicketState::Waiting)
            .count();
        if waiting >= MAX_WAITING {
            return Err("Warteraum voll");
        }
        let ticket = random_hex().ok_or("Kein Zufall verfügbar")?;
        g.invites.remove(token);
        g.tickets.insert(
            ticket.clone(),
            Entry {
                name: clean_name(name),
                peer: peer.chars().take(64).collect(),
                offer: offer.to_string(),
                since: now,
                seen: now,
                state: TicketState::Waiting,
                taken: false,
                left: false,
                air: Air::Waiting,
                self_muted: false,
            },
        );
        Ok(ticket)
    }

    /// Stand für die Seite des Gastes; zählt zugleich als Lebenszeichen.
    pub fn state(&self, ticket: &str, now: Instant) -> TicketState {
        let mut g = self.lock();
        match g.tickets.get_mut(ticket) {
            Some(t) => {
                t.seen = now;
                t.state.clone()
            }
            None => TicketState::Ended,
        }
    }

    /// Sendestand und Selbst-Stumm eines Tickets (für die Seite des Gastes).
    pub fn air(&self, ticket: &str) -> (Air, bool) {
        self.lock()
            .tickets
            .get(ticket)
            .map_or((Air::Waiting, false), |t| (t.air, t.self_muted))
    }

    /// Der Control-Thread setzt den Sendestand.
    pub fn set_air(&self, ticket: &str, air: Air) {
        if let Some(t) = self.lock().tickets.get_mut(ticket) {
            t.air = air;
        }
    }

    /// Der Gast schaltet sich selbst stumm. Nur für ein angenommenes Ticket; jede Seite kennt
    /// nur ihr eigenes Ticket und kann so keinen anderen Gast steuern.
    pub fn set_self_mute(&self, ticket: &str, on: bool) -> bool {
        match self.lock().tickets.get_mut(ticket) {
            Some(t) if matches!(t.state, TicketState::Accepted(_)) => {
                t.self_muted = on;
                true
            }
            _ => false,
        }
    }

    /// Hat sich der Gast selbst stumm geschaltet?
    pub fn self_muted(&self, ticket: &str) -> bool {
        self.lock()
            .tickets
            .get(ticket)
            .is_some_and(|t| t.self_muted)
    }

    /// Der Gast legt auf (oder schließt die Seite).
    pub fn leave(&self, ticket: &str) {
        if let Some(t) = self.lock().tickets.get_mut(ticket) {
            t.left = true;
            if t.state == TicketState::Waiting {
                t.state = TicketState::Ended;
            }
        }
    }

    /// Der Warteraum für die Oberfläche, älteste zuerst.
    pub fn waiting(&self, now: Instant) -> Vec<Waiting> {
        let g = self.lock();
        let mut v: Vec<(Instant, Waiting)> = g
            .tickets
            .iter()
            .filter(|(_, t)| t.state == TicketState::Waiting)
            .map(|(id, t)| {
                (
                    t.since,
                    Waiting {
                        ticket: id.clone(),
                        name: t.name.clone(),
                        peer: t.peer.clone(),
                        waited_s: now.saturating_duration_since(t.since).as_secs(),
                    },
                )
            })
            .collect();
        v.sort_by_key(|(s, _)| *s);
        v.into_iter().map(|(_, w)| w).collect()
    }

    /// Offene Einladungen für die Oberfläche.
    pub fn invites(&self, now: Instant) -> Vec<InviteView> {
        let g = self.lock();
        let mut v: Vec<(Instant, InviteView)> = g
            .invites
            .iter()
            .filter(|(_, i)| i.expires > now)
            .map(|(t, i)| {
                (
                    i.expires,
                    InviteView {
                        token: t.clone(),
                        left_s: i.expires.saturating_duration_since(now).as_secs(),
                    },
                )
            })
            .collect();
        v.sort_by_key(|(e, _)| *e);
        v.into_iter().map(|(_, i)| i).collect()
    }

    /// Moderator nimmt an: gibt Name und Angebot heraus (einmal). `None`, wenn der Eintrag
    /// fehlt oder schon bearbeitet ist.
    pub fn take_for_accept(&self, ticket: &str) -> Option<(String, String)> {
        let mut g = self.lock();
        let t = g.tickets.get_mut(ticket)?;
        if t.state != TicketState::Waiting || t.taken {
            return None;
        }
        t.taken = true;
        Some((t.name.clone(), t.offer.clone()))
    }

    /// Die Verbindung ist vorbereitet: die Antwort geht an die Seite.
    pub fn set_answer(&self, ticket: &str, answer: String) {
        if let Some(t) = self.lock().tickets.get_mut(ticket) {
            t.state = TicketState::Accepted(answer);
            // Das Angebot wird nicht mehr gebraucht.
            t.offer = String::new();
        }
    }

    /// Annahme gescheitert (kein Kanal frei, Fehler): die Seite erfährt es als Ablehnung.
    pub fn fail(&self, ticket: &str) {
        if let Some(t) = self.lock().tickets.get_mut(ticket) {
            t.state = TicketState::Rejected;
            t.offer = String::new();
        }
    }

    /// Moderator lehnt ab.
    pub fn reject(&self, ticket: &str) {
        self.fail(ticket);
    }

    /// Verbindung zu Ende (vom Netz-Thread gemeldet oder vom Moderator aufgelegt).
    pub fn end(&self, ticket: &str) {
        if let Some(t) = self.lock().tickets.get_mut(ticket) {
            t.state = TicketState::Ended;
            t.offer = String::new();
        }
    }

    /// Tickets, bei denen der Gast aufgelegt hat und die noch als angenommen gelten.
    pub fn take_left(&self) -> Vec<String> {
        let mut g = self.lock();
        g.tickets
            .iter_mut()
            .filter(|(_, t)| t.left && matches!(t.state, TicketState::Accepted(_)))
            .map(|(id, t)| {
                t.state = TicketState::Ended;
                id.clone()
            })
            .collect()
    }

    /// Räumt auf: abgelaufene Einladungen, vergessene Wartende, beendete Tickets nach einer
    /// Weile. Gibt die Tickets zurück, die wegen Stille aus dem Warteraum gefallen sind
    /// (nur zur Protokollierung).
    pub fn sweep(&self, now: Instant) -> Vec<String> {
        let mut g = self.lock();
        g.invites.retain(|_, i| i.expires > now);
        let mut dropped = Vec::new();
        for (id, t) in g.tickets.iter_mut() {
            if t.state == TicketState::Waiting
                && (now.saturating_duration_since(t.seen) > SILENCE_TTL
                    || now.saturating_duration_since(t.since) > WAIT_TTL)
            {
                t.state = TicketState::Ended;
                t.offer = String::new();
                dropped.push(id.clone());
            }
        }
        // Beendete und abgelehnte Tickets bleiben kurz stehen, damit die Seite den Stand liest.
        g.tickets.retain(|_, t| {
            !matches!(t.state, TicketState::Ended | TicketState::Rejected)
                || now.saturating_duration_since(t.seen) < Duration::from_secs(60)
        });
        dropped
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OFFER: &str = "v=0\r\no=- 1 1 IN IP4 0.0.0.0\r\n";

    fn t0() -> Instant {
        Instant::now()
    }

    #[test]
    fn invite_is_single_use() {
        let g = Gate::new();
        let now = t0();
        let tok = g.create_invite(now).expect("invite");
        assert!(g.invite_valid(&tok, now));
        let t = g
            .join(&tok, "Anna", OFFER, "10.0.0.5:4000", now)
            .expect("join");
        assert!(!t.is_empty());
        assert!(!g.invite_valid(&tok, now));
        assert!(g.join(&tok, "Bert", OFFER, "x", now).is_err());
    }

    #[test]
    fn invite_expires() {
        let g = Gate::new();
        let now = t0();
        let tok = g.create_invite(now).expect("invite");
        let later = now + INVITE_TTL + Duration::from_secs(1);
        assert!(!g.invite_valid(&tok, later));
        assert!(g.join(&tok, "Anna", OFFER, "x", later).is_err());
    }

    #[test]
    fn unknown_token_and_bad_offer_are_refused() {
        let g = Gate::new();
        let now = t0();
        assert!(g.join("nope", "Anna", OFFER, "x", now).is_err());
        let tok = g.create_invite(now).expect("invite");
        assert!(g.join(&tok, "Anna", "GET / HTTP/1.1", "x", now).is_err());
        assert!(
            g.join(
                &tok,
                "Anna",
                &format!("v=0{}", "x".repeat(MAX_SDP)),
                "x",
                now
            )
            .is_err()
        );
        // Ein ungültiges Angebot verbraucht die Einladung nicht.
        assert!(g.invite_valid(&tok, now));
    }

    #[test]
    fn waiting_room_accept_flow() {
        let g = Gate::new();
        let now = t0();
        let tok = g.create_invite(now).expect("invite");
        let t = g
            .join(&tok, "  Anna\n", OFFER, "10.0.0.5:4000", now)
            .expect("join");
        let w = g.waiting(now + Duration::from_secs(3));
        assert_eq!(w.len(), 1);
        assert_eq!(w[0].name, "Anna");
        assert_eq!(w[0].waited_s, 3);
        assert_eq!(g.state(&t, now), TicketState::Waiting);
        let (name, offer) = g.take_for_accept(&t).expect("take");
        assert_eq!(name, "Anna");
        assert_eq!(offer, OFFER);
        // Zweimal annehmen geht nicht.
        assert!(g.take_for_accept(&t).is_none());
        g.set_answer(&t, "v=0 answer".into());
        assert_eq!(g.state(&t, now), TicketState::Accepted("v=0 answer".into()));
        assert!(g.waiting(now).is_empty());
    }

    #[test]
    fn reject_and_end() {
        let g = Gate::new();
        let now = t0();
        let a = g.create_invite(now).expect("a");
        let ta = g.join(&a, "A", OFFER, "x", now).expect("ja");
        g.reject(&ta);
        assert_eq!(g.state(&ta, now), TicketState::Rejected);
        assert!(g.take_for_accept(&ta).is_none());
        assert_eq!(g.state("unbekannt", now), TicketState::Ended);
    }

    #[test]
    fn silent_guest_drops_out_of_the_waiting_room() {
        let g = Gate::new();
        let now = t0();
        let tok = g.create_invite(now).expect("invite");
        let t = g.join(&tok, "Anna", OFFER, "x", now).expect("join");
        // Lebenszeichen halten ihn drin.
        let t10 = now + Duration::from_secs(10);
        assert_eq!(g.state(&t, t10), TicketState::Waiting);
        assert!(g.sweep(t10 + Duration::from_secs(15)).is_empty());
        // Ohne Lebenszeichen fällt er heraus.
        let gone = g.sweep(t10 + SILENCE_TTL + Duration::from_secs(1));
        assert_eq!(gone, vec![t.clone()]);
        assert!(g.waiting(t10).is_empty());
        assert_eq!(g.state(&t, t10), TicketState::Ended);
    }

    #[test]
    fn guest_hangup_is_reported_once() {
        let g = Gate::new();
        let now = t0();
        let tok = g.create_invite(now).expect("invite");
        let t = g.join(&tok, "Anna", OFFER, "x", now).expect("join");
        g.take_for_accept(&t).expect("take");
        g.set_answer(&t, "v=0".into());
        g.leave(&t);
        assert_eq!(g.take_left(), vec![t.clone()]);
        assert!(g.take_left().is_empty());
        assert_eq!(g.state(&t, now), TicketState::Ended);
    }

    #[test]
    fn waiting_room_and_invites_are_limited() {
        let g = Gate::new();
        let now = t0();
        for _ in 0..MAX_INVITES {
            assert!(g.create_invite(now).is_some());
        }
        assert!(g.create_invite(now).is_none());
        let views = g.invites(now);
        assert_eq!(views.len(), MAX_INVITES);
        let g = Gate::new();
        for i in 0..MAX_WAITING {
            let tok = g.create_invite(now).expect("invite");
            g.join(&tok, &format!("G{i}"), OFFER, "x", now)
                .expect("join");
        }
        let tok = g.create_invite(now).expect("invite");
        assert_eq!(
            g.join(&tok, "Zu viel", OFFER, "x", now),
            Err("Warteraum voll")
        );
    }

    #[test]
    fn names_are_cleaned() {
        assert_eq!(clean_name("  Anna\u{7}\n "), "Anna");
        assert_eq!(clean_name(&"ä".repeat(40)).chars().count(), MAX_NAME);
        assert_eq!(clean_name("\u{0}\u{1}"), "");
    }
}
