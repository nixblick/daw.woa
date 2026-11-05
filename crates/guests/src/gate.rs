//! Einladungen und Warteraum der Browser-Gäste. Reine Logik ohne Netz und ohne Uhrzeit-Zugriff
//! von außen: jede Methode bekommt die Zeit mit, damit die Tests sie vorspulen können.
//!
//! Ablauf: Der Moderator erzeugt einen gemeinsamen Link für die Sendung (ein Zufallswert,
//! einige Stunden gültig) und dazu eine PIN. Der Link gilt für alle Gäste der Sendung; die PIN
//! steht nicht im Link, der Moderator sagt sie an. Der Gast öffnet den Link und schickt Name,
//! PIN und SDP-Angebot. Er steht dann im Warteraum; erst wenn der Moderator annimmt, entsteht
//! Kanal und Verbindung. Lehnt er ab oder meldet sich der Gast nicht mehr (Tab zu), verschwindet
//! der Eintrag.
//!
//! Schutz: Fehlversuche bei der PIN zählen je Adresse (Sperre auf Zeit) und je Link (zu viele:
//! Link und PIN sind verbrannt, der Moderator erzeugt neue). Ein Erfolg setzt keinen Zähler
//! zurück (Lehre aus #116). Mehr als [`crate::MAX_GUESTS`] Gäste (wartend oder angenommen)
//! kommen nicht hinein; ein neuer Gast verdrängt nie einen vorhandenen.
//!
//! Der Gate liegt hinter einem `Mutex`. Er wird nur vom Web-Server-Thread und vom Control-Thread
//! benutzt, nie vom Audio-Thread.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// So lange gilt der Link einer Sendung.
pub const INVITE_TTL: Duration = Duration::from_secs(6 * 60 * 60);
/// So lange darf ein Gast im Warteraum bleiben.
pub const WAIT_TTL: Duration = Duration::from_secs(10 * 60);
/// Meldet sich die Seite so lange nicht, ist der Gast weg.
pub const SILENCE_TTL: Duration = Duration::from_secs(20);
/// Höchstens so viele Gäste gleichzeitig im Warteraum oder angenommen.
pub const MAX_WAITING: usize = crate::MAX_GUESTS;
/// Stellen der PIN.
pub const PIN_LEN: usize = 6;
/// So viele falsche PINs je Adresse, dann ist die Adresse gesperrt.
pub const MAX_PEER_FAILS: u32 = 5;
/// So lange bleibt eine Adresse nach zu vielen Fehlversuchen gesperrt.
pub const PEER_LOCK: Duration = Duration::from_secs(10 * 60);
/// So viele falsche PINs über alle Adressen, dann sind Link und PIN verbrannt.
pub const MAX_LINK_FAILS: u32 = 20;
/// Höchstens so viele Adressen merkt sich der Zähler (gegen Speicherflut).
const MAX_PEERS_TRACKED: usize = 256;
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

/// Der Link der Sendung, wie die Oberfläche ihn sieht.
#[derive(Debug, Clone, PartialEq)]
pub struct InviteView {
    pub token: String,
    /// Die PIN, die der Moderator den Gästen sagt.
    pub pin: String,
    /// Gültig noch so viele Sekunden.
    pub left_s: u64,
}

struct Invite {
    token: String,
    pin: String,
    expires: Instant,
    /// Falsche PINs über alle Adressen.
    fails: u32,
}

/// Fehlversuche einer Adresse.
struct PeerFails {
    count: u32,
    last: Instant,
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
    /// Der eine Link der Sendung (oder keiner).
    invite: Option<Invite>,
    /// Fehlversuche je Adresse (nur die IP, ohne Port).
    peer_fails: HashMap<String, PeerFails>,
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

/// Zufällige PIN aus [`PIN_LEN`] Ziffern. `None` ohne Zufall.
fn random_pin() -> Option<String> {
    let mut b = [0u8; PIN_LEN];
    getrandom::fill(&mut b).ok()?;
    // 250 ist durch 10 teilbar: Werte darüber werden neu gezogen, sonst wären kleine Ziffern
    // häufiger.
    let mut out = String::with_capacity(PIN_LEN);
    for mut x in b {
        while x >= 250 {
            let mut one = [0u8; 1];
            getrandom::fill(&mut one).ok()?;
            x = one[0];
        }
        out.push(char::from(b'0' + x % 10));
    }
    Some(out)
}

/// Vergleich in konstanter Zeit (gleiche Länge vorausgesetzt, sonst sofort falsch).
fn same(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0u8, |acc, (x, y)| acc | (x ^ y))
            == 0
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

    /// Neuer Link mit neuer PIN für die Sendung. Ein alter Link samt PIN gilt danach nicht mehr;
    /// schon angenommene Gäste bleiben verbunden. `None`, wenn kein Zufall da ist.
    pub fn create_invite(&self, now: Instant) -> Option<String> {
        let token = random_hex()?;
        let pin = random_pin()?;
        let mut g = self.lock();
        g.invite = Some(Invite {
            token: token.clone(),
            pin,
            expires: now + INVITE_TTL,
            fails: 0,
        });
        g.peer_fails.clear();
        Some(token)
    }

    /// Zieht den Link zurück (nur, wenn `token` der aktuelle ist).
    pub fn revoke_invite(&self, token: &str) {
        let mut g = self.lock();
        if g.invite.as_ref().is_some_and(|i| same(&i.token, token)) {
            g.invite = None;
        }
    }

    /// Prüft, ob der Link noch gilt. Prüft nicht die PIN.
    pub fn invite_valid(&self, token: &str, now: Instant) -> bool {
        self.lock()
            .invite
            .as_ref()
            .is_some_and(|i| i.expires > now && same(&i.token, token))
    }

    /// Ein Gast tritt bei: Link und PIN prüfen, dann in den Warteraum. Der Link bleibt für
    /// weitere Gäste gültig.
    pub fn join(
        &self,
        token: &str,
        pin: &str,
        name: &str,
        offer: &str,
        peer: &str,
        now: Instant,
    ) -> Result<String, &'static str> {
        if offer.len() > MAX_SDP || !offer.starts_with("v=0") {
            return Err("Ungültiges Angebot");
        }
        let mut g = self.lock();
        let link_ok = g
            .invite
            .as_ref()
            .is_some_and(|i| i.expires > now && same(&i.token, token));
        if !link_ok {
            return Err("Link abgelaufen oder ungültig");
        }
        let peer_key: String = peer.chars().take(64).collect();
        // Gesperrte Adresse: nicht einmal die PIN wird angesehen.
        if let Some(f) = g.peer_fails.get(&peer_key)
            && f.count >= MAX_PEER_FAILS
            && now.saturating_duration_since(f.last) < PEER_LOCK
        {
            return Err("Zu viele Fehlversuche. Bitte später noch einmal.");
        }
        let pin_ok = g.invite.as_ref().is_some_and(|i| same(&i.pin, pin));
        if !pin_ok {
            if g.peer_fails.len() >= MAX_PEERS_TRACKED && !g.peer_fails.contains_key(&peer_key) {
                // Voll: die älteste Adresse fällt heraus, gesperrte bleiben.
                let oldest = g
                    .peer_fails
                    .iter()
                    .filter(|(_, f)| f.count < MAX_PEER_FAILS)
                    .min_by_key(|(_, f)| f.last)
                    .map(|(k, _)| k.clone());
                if let Some(k) = oldest {
                    g.peer_fails.remove(&k);
                }
            }
            let f = g.peer_fails.entry(peer_key).or_insert(PeerFails {
                count: 0,
                last: now,
            });
            // Eine abgelaufene Sperre beginnt neu.
            if f.count >= MAX_PEER_FAILS {
                f.count = 0;
            }
            f.count += 1;
            f.last = now;
            let burned = g.invite.as_mut().is_some_and(|i| {
                i.fails += 1;
                i.fails >= MAX_LINK_FAILS
            });
            if burned {
                g.invite = None;
                tracing::warn!("guest web: too many wrong PINs, link revoked");
            }
            return Err("PIN falsch");
        }
        let present = g
            .tickets
            .values()
            .filter(|t| {
                !t.left && matches!(t.state, TicketState::Waiting | TicketState::Accepted(_))
            })
            .count();
        if present >= MAX_WAITING {
            return Err("Die Sendung ist voll");
        }
        let ticket = random_hex().ok_or("Kein Zufall verfügbar")?;
        g.tickets.insert(
            ticket.clone(),
            Entry {
                name: clean_name(name),
                peer: peer_key,
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

    /// Der Link der Sendung für die Oberfläche (keiner oder einer).
    pub fn invites(&self, now: Instant) -> Vec<InviteView> {
        self.lock()
            .invite
            .iter()
            .filter(|i| i.expires > now)
            .map(|i| InviteView {
                token: i.token.clone(),
                pin: i.pin.clone(),
                left_s: i.expires.saturating_duration_since(now).as_secs(),
            })
            .collect()
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
        if g.invite.as_ref().is_some_and(|i| i.expires <= now) {
            g.invite = None;
        }
        g.peer_fails
            .retain(|_, f| now.saturating_duration_since(f.last) < PEER_LOCK);
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

    /// Link und PIN der Sendung.
    fn link(g: &Gate, now: Instant) -> (String, String) {
        let tok = g.create_invite(now).expect("invite");
        let pin = g.invites(now)[0].pin.clone();
        (tok, pin)
    }

    #[test]
    fn one_link_for_several_guests() {
        let g = Gate::new();
        let now = t0();
        let (tok, pin) = link(&g, now);
        assert_eq!(pin.len(), PIN_LEN);
        assert!(pin.bytes().all(|b| b.is_ascii_digit()));
        let a = g
            .join(&tok, &pin, "Anna", OFFER, "10.0.0.5", now)
            .expect("a");
        let b = g
            .join(&tok, &pin, "Bert", OFFER, "10.0.0.6", now)
            .expect("b");
        assert_ne!(a, b, "jeder Gast hat sein eigenes Ticket");
        assert!(g.invite_valid(&tok, now), "der Link bleibt gültig");
        assert_eq!(g.waiting(now).len(), 2);
    }

    #[test]
    fn new_link_burns_the_old_but_keeps_guests() {
        let g = Gate::new();
        let now = t0();
        let (old, old_pin) = link(&g, now);
        let t = g
            .join(&old, &old_pin, "Anna", OFFER, "x", now)
            .expect("join");
        let (new, _) = link(&g, now);
        assert_ne!(old, new);
        assert!(!g.invite_valid(&old, now));
        assert!(g.join(&old, &old_pin, "Bert", OFFER, "y", now).is_err());
        assert_eq!(g.state(&t, now), TicketState::Waiting);
        assert_eq!(g.invites(now).len(), 1);
    }

    #[test]
    fn invite_expires() {
        let g = Gate::new();
        let now = t0();
        let (tok, pin) = link(&g, now);
        let later = now + INVITE_TTL + Duration::from_secs(1);
        assert!(!g.invite_valid(&tok, later));
        assert!(g.join(&tok, &pin, "Anna", OFFER, "x", later).is_err());
        g.sweep(later);
        assert!(g.invites(later).is_empty());
    }

    #[test]
    fn unknown_token_and_bad_offer_are_refused() {
        let g = Gate::new();
        let now = t0();
        assert!(g.join("nope", "000000", "Anna", OFFER, "x", now).is_err());
        let (tok, pin) = link(&g, now);
        assert!(
            g.join(&tok, &pin, "Anna", "GET / HTTP/1.1", "x", now)
                .is_err()
        );
        assert!(
            g.join(
                &tok,
                &pin,
                "Anna",
                &format!("v=0{}", "x".repeat(MAX_SDP)),
                "x",
                now
            )
            .is_err()
        );
        assert!(g.invite_valid(&tok, now));
    }

    #[test]
    fn wrong_pin_locks_the_address_and_success_does_not_reset() {
        let g = Gate::new();
        let now = t0();
        let (tok, pin) = link(&g, now);
        let wrong = if pin == "000000" { "111111" } else { "000000" };
        for _ in 0..MAX_PEER_FAILS - 1 {
            assert_eq!(
                g.join(&tok, wrong, "M", OFFER, "6.6.6.6", now),
                Err("PIN falsch")
            );
        }
        // Ein Erfolg setzt den Zähler nicht zurück (#116).
        g.join(&tok, &pin, "M", OFFER, "6.6.6.6", now)
            .expect("richtig");
        assert_eq!(
            g.join(&tok, wrong, "M", OFFER, "6.6.6.6", now),
            Err("PIN falsch")
        );
        // Jetzt gesperrt: auch die richtige PIN hilft nicht.
        assert!(g.join(&tok, &pin, "M", OFFER, "6.6.6.6", now).is_err());
        // Andere Adressen sind nicht betroffen.
        g.join(&tok, &pin, "Anna", OFFER, "10.0.0.5", now)
            .expect("anna");
        // Nach der Sperrzeit geht es wieder.
        let later = now + PEER_LOCK + Duration::from_secs(1);
        g.join(&tok, &pin, "M", OFFER, "6.6.6.6", later)
            .expect("später");
    }

    #[test]
    fn many_wrong_pins_from_many_addresses_burn_the_link() {
        let g = Gate::new();
        let now = t0();
        let (tok, pin) = link(&g, now);
        let wrong = if pin == "000000" { "111111" } else { "000000" };
        for i in 0..MAX_LINK_FAILS {
            assert!(
                g.join(&tok, wrong, "M", OFFER, &format!("10.1.0.{i}"), now)
                    .is_err()
            );
        }
        assert!(!g.invite_valid(&tok, now));
        assert!(g.join(&tok, &pin, "Anna", OFFER, "10.0.0.5", now).is_err());
        assert!(g.invites(now).is_empty());
    }

    #[test]
    fn full_show_refuses_new_guests_and_never_evicts() {
        let g = Gate::new();
        let now = t0();
        let (tok, pin) = link(&g, now);
        let mut tickets = Vec::new();
        for i in 0..crate::MAX_GUESTS {
            tickets.push(
                g.join(
                    &tok,
                    &pin,
                    &format!("G{i}"),
                    OFFER,
                    &format!("10.0.0.{i}"),
                    now,
                )
                .expect("join"),
            );
        }
        // Einer angenommen: zählt weiter mit.
        g.take_for_accept(&tickets[0]).expect("take");
        g.set_answer(&tickets[0], "v=0".into());
        assert_eq!(
            g.join(&tok, &pin, "Zu viel", OFFER, "10.0.9.9", now),
            Err("Die Sendung ist voll")
        );
        for t in &tickets[1..] {
            assert_eq!(g.state(t, now), TicketState::Waiting);
        }
        // Geht einer, ist wieder Platz.
        g.reject(&tickets[1]);
        g.join(&tok, &pin, "Nachrücker", OFFER, "10.0.9.9", now)
            .expect("Platz frei");
    }

    #[test]
    fn waiting_room_accept_flow() {
        let g = Gate::new();
        let now = t0();
        let (tok, pin) = link(&g, now);
        let t = g
            .join(&tok, &pin, "  Anna\n", OFFER, "10.0.0.5", now)
            .expect("join");
        let w = g.waiting(now + Duration::from_secs(3));
        assert_eq!(w.len(), 1);
        assert_eq!(w[0].name, "Anna");
        assert_eq!(w[0].waited_s, 3);
        assert_eq!(g.state(&t, now), TicketState::Waiting);
        let (name, offer) = g.take_for_accept(&t).expect("take");
        assert_eq!(name, "Anna");
        assert_eq!(offer, OFFER);
        assert!(g.take_for_accept(&t).is_none());
        g.set_answer(&t, "v=0 answer".into());
        assert_eq!(g.state(&t, now), TicketState::Accepted("v=0 answer".into()));
        assert!(g.waiting(now).is_empty());
    }

    #[test]
    fn reject_and_end() {
        let g = Gate::new();
        let now = t0();
        let (a, pin) = link(&g, now);
        let ta = g.join(&a, &pin, "A", OFFER, "x", now).expect("ja");
        g.reject(&ta);
        assert_eq!(g.state(&ta, now), TicketState::Rejected);
        assert!(g.take_for_accept(&ta).is_none());
        assert_eq!(g.state("unbekannt", now), TicketState::Ended);
    }

    #[test]
    fn silent_guest_drops_out_of_the_waiting_room() {
        let g = Gate::new();
        let now = t0();
        let (tok, pin) = link(&g, now);
        let t = g.join(&tok, &pin, "Anna", OFFER, "x", now).expect("join");
        let t10 = now + Duration::from_secs(10);
        assert_eq!(g.state(&t, t10), TicketState::Waiting);
        assert!(g.sweep(t10 + Duration::from_secs(15)).is_empty());
        let gone = g.sweep(t10 + SILENCE_TTL + Duration::from_secs(1));
        assert_eq!(gone, vec![t.clone()]);
        assert!(g.waiting(t10).is_empty());
        assert_eq!(g.state(&t, t10), TicketState::Ended);
    }

    #[test]
    fn guest_hangup_is_reported_once() {
        let g = Gate::new();
        let now = t0();
        let (tok, pin) = link(&g, now);
        let t = g.join(&tok, &pin, "Anna", OFFER, "x", now).expect("join");
        g.take_for_accept(&t).expect("take");
        g.set_answer(&t, "v=0".into());
        g.leave(&t);
        assert_eq!(g.take_left(), vec![t.clone()]);
        assert!(g.take_left().is_empty());
        assert_eq!(g.state(&t, now), TicketState::Ended);
    }

    #[test]
    fn pin_compare_and_digits() {
        assert!(same("123456", "123456"));
        assert!(!same("123456", "123457"));
        assert!(!same("123456", "12345"));
        assert!(!same("", "1"));
        for _ in 0..50 {
            let p = random_pin().expect("pin");
            assert_eq!(p.len(), PIN_LEN);
            assert!(p.bytes().all(|b| b.is_ascii_digit()));
        }
    }

    #[test]
    fn names_are_cleaned() {
        assert_eq!(clean_name("  Anna\u{7}\n "), "Anna");
        assert_eq!(clean_name(&"ä".repeat(40)).chars().count(), MAX_NAME);
        assert_eq!(clean_name("\u{0}\u{1}"), "");
    }
}
