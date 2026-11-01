//! Zugang: PINs, Sitzungen (Token), Rollen und Ratenbegrenzung.
//!
//! Bei jedem Einschalten der Fernbedienung entstehen zwei neue PINs: die Gast-PIN (steht im
//! QR-Code) und die DJ-PIN (nur auf dem Bildschirm, zum Eintippen). „Einmal“ heißt: pro
//! Einschalten; mehrere Gäste scannen denselben Code. Falsch geratene PINs werden je Adresse
//! gezählt (je volle Adresse) und sperren nach wenigen Fehlversuchen
//! für eine Weile. Zusätzlich werden falsche PINs insgesamt je Minute gezählt,
//! unabhängig von der Adresse (#294): wer die Adresse wechselt, wird darüber hinaus nur noch
//! alle paar Sekunden geprüft (Verlangsamung, keine Sperre für alle). Wer ein gültiges
//! DJ-Token mitschickt, ist von beiden Bremsen ausgenommen; die bloße Adresse genügt nicht. Es gibt keinen PIN-Wechsel nach Fehlversuchen: sonst könnte
//! ein Angreifer dem DJ die PIN wegwürfeln (#162).
//!
//! Alle Zeiten kommen als Parameter herein, damit die Regeln ohne Warten testbar sind.

use std::collections::HashMap;
use std::net::IpAddr;
use std::time::{Duration, Instant};

/// Wer das Handy bedient.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// Sieht die Anzeige, darf Wünsche einreichen.
    Guest,
    /// Darf zusätzlich steuern (Play, Weiter, Lautstärke, Wünsche einreihen und verwerfen).
    Dj,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Role::Guest => "guest",
            Role::Dj => "dj",
        }
    }
}

/// Grenzen der Fernbedienung.
#[derive(Debug, Clone)]
pub struct Limits {
    /// Gleichzeitige Verbindungen (HTTP und WebSocket zusammen).
    pub max_connections: usize,
    /// Falsche PINs je Adresse, bis gesperrt wird.
    pub login_failures: u32,
    /// Falsche PINs insgesamt (alle Adressen zusammen) je Minute. Darüber wird nur verlangsamt:
    /// höchstens ein Versuch je `SLOW_GAP` wird geprüft, die anderen bekommen 429, ohne dass die
    /// PIN verglichen wird. Wer ein gültiges DJ-Token mitschickt, ist davon nicht betroffen.
    pub global_failures_per_minute: u32,
    /// So lange gilt die Sperre (und das Zählfenster).
    pub lockout: Duration,
    /// Mindestabstand zwischen zwei Wünschen derselben Adresse (Gast).
    pub wish_interval: Duration,
    /// Befehle je Sekunde und Sitzung.
    pub commands_per_second: u32,
    /// So viele Gast-Sitzungen bleiben gespeichert; die ältesten Gäste fliegen raus.
    pub max_sessions: usize,
    /// So viele DJ-Sitzungen bleiben gespeichert; nur ältere DJ-Sitzungen fliegen dafür raus.
    pub max_dj_sessions: usize,
    /// So viele Sitzungen darf eine Adresse je Rolle haben; ältere derselben Adresse fliegen raus.
    pub max_sessions_per_ip: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_connections: 64,
            login_failures: 5,
            global_failures_per_minute: 30,
            lockout: Duration::from_secs(60),
            wish_interval: Duration::from_secs(10),
            commands_per_second: 10,
            max_sessions: 256,
            max_dj_sessions: 32,
            max_sessions_per_ip: 16,
        }
    }
}

/// Zählfenster der globalen Grenze für falsche PINs.
const GLOBAL_WINDOW: Duration = Duration::from_secs(60);

/// Abstand zwischen zwei geprüften Versuchen, solange die globale Grenze überschritten ist.
const SLOW_GAP: Duration = Duration::from_secs(2);

/// Sechsstellige PIN aus dem Zufallsgenerator des Systems (ohne Verzerrung).
pub fn random_pin() -> Option<String> {
    loop {
        let mut b = [0u8; 4];
        getrandom::fill(&mut b).ok()?;
        let v = u32::from_le_bytes(b);
        // Größtes Vielfaches von 1_000_000 unterhalb von 2^32, sonst wäre 0 etwas häufiger.
        if v < 4_294_000_000 {
            return Some(format!("{:06}", v % 1_000_000));
        }
    }
}

/// Achtstellige PIN für den DJ (ohne Verzerrung): 100-mal so schwer zu erraten wie die Gast-PIN.
pub fn random_dj_pin() -> Option<String> {
    loop {
        let mut b = [0u8; 8];
        getrandom::fill(&mut b).ok()?;
        let v = u64::from_le_bytes(b);
        // Größtes Vielfaches von 10^8 unterhalb von 2^64.
        if v < 18_446_744_073_700_000_000 {
            return Some(format!("{:08}", v % 100_000_000));
        }
    }
}

/// Quelle für die Fehlerzählung: je volle Adresse (IPv4-in-IPv6 als IPv4). Ein Zähler je /64
/// würde alle Geräte eines LANs in einen Topf werfen und einen Störer alle aussperren lassen.
/// Wer die Adresse wechselt, wird von der globalen Verlangsamung aufgefangen (#294).
fn source(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(v6) => v6.to_ipv4_mapped().map_or(ip, IpAddr::V4),
        _ => ip,
    }
}

/// 128-Bit-Token als Hex.
pub fn random_token() -> Option<String> {
    let mut b = [0u8; 16];
    getrandom::fill(&mut b).ok()?;
    Some(b.iter().map(|x| format!("{x:02x}")).collect())
}

/// Vergleich ohne frühen Ausstieg (Länge ist kein Geheimnis).
pub fn ct_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Fester Zeitraum, in dem höchstens `max` Dinge passieren dürfen.
#[derive(Debug, Clone)]
pub struct Bucket {
    start: Instant,
    count: u32,
}

impl Bucket {
    pub fn new(now: Instant) -> Self {
        Self {
            start: now,
            count: 0,
        }
    }

    /// `true`, wenn noch Platz ist (und zählt mit).
    pub fn take(&mut self, now: Instant, max: u32) -> bool {
        if now.duration_since(self.start) >= Duration::from_secs(1) {
            self.start = now;
            self.count = 0;
        }
        if self.count >= max {
            return false;
        }
        self.count += 1;
        true
    }
}

#[derive(Debug)]
struct Session {
    role: Role,
    ip: IpAddr,
    bucket: Bucket,
    created: Instant,
}

/// Alles, was zum Anmelden gehört.
#[derive(Debug)]
pub struct Auth {
    guest_pin: String,
    dj_pin: String,
    sessions: HashMap<String, Session>,
    fails: HashMap<IpAddr, (u32, Instant)>,
    /// Falsche PINs insgesamt im laufenden Fenster: (Anzahl, Beginn).
    global_fails: Option<(u32, Instant)>,
    /// Letzter geprüfter Versuch, während die globale Grenze überschritten war.
    slow_last: Option<Instant>,
    wishes: HashMap<IpAddr, Instant>,
    limits: Limits,
}

/// Ausgang eines Anmeldeversuchs.
#[derive(Debug, PartialEq)]
pub enum LoginError {
    /// PIN falsch (401).
    WrongPin,
    /// Gesperrt, noch so viele Sekunden (429).
    Locked(u64),
    /// Kein Zufall vom System (500).
    NoRandom,
}

impl Auth {
    /// Neue PINs. `None`, wenn das System keinen Zufall liefert.
    pub fn new(limits: Limits) -> Option<Self> {
        Some(Self {
            guest_pin: random_pin()?,
            dj_pin: random_dj_pin()?,
            sessions: HashMap::new(),
            fails: HashMap::new(),
            global_fails: None,
            slow_last: None,
            wishes: HashMap::new(),
            limits,
        })
    }

    /// Feste PINs, nur für Tests.
    pub fn with_pins(limits: Limits, guest: &str, dj: &str) -> Self {
        Self {
            guest_pin: guest.into(),
            dj_pin: dj.into(),
            sessions: HashMap::new(),
            fails: HashMap::new(),
            global_fails: None,
            slow_last: None,
            wishes: HashMap::new(),
            limits,
        }
    }

    pub fn guest_pin(&self) -> &str {
        &self.guest_pin
    }

    pub fn dj_pin(&self) -> &str {
        &self.dj_pin
    }

    pub fn limits(&self) -> &Limits {
        &self.limits
    }

    /// Neue PINs; alle alten Sitzungen sind damit ungültig.
    pub fn renew(&mut self) -> bool {
        let (Some(g), Some(d)) = (random_pin(), random_dj_pin()) else {
            return false;
        };
        self.guest_pin = g;
        self.dj_pin = d;
        self.sessions.clear();
        self.fails.clear();
        self.global_fails = None;
        self.slow_last = None;
        true
    }

    /// Meldet an. Bei Erfolg: Token und Rolle.
    pub fn login(
        &mut self,
        ip: IpAddr,
        pin: &str,
        now: Instant,
    ) -> Result<(String, Role), LoginError> {
        self.login_with_token(ip, pin, None, now)
    }

    /// Wie [`Auth::login`]; ein gültiges DJ-Token (`token`) nimmt die Sperren aus dem Weg, die
    /// bloße Adresse tut das nicht.
    pub fn login_with_token(
        &mut self,
        ip: IpAddr,
        pin: &str,
        token: Option<&str>,
        now: Instant,
    ) -> Result<(String, Role), LoginError> {
        let src = source(ip);
        let trusted = token.is_some_and(|t| self.role_of(t) == Some(Role::Dj));
        if !trusted {
            if let Some((n, since)) = self.fails.get(&src).copied() {
                let age = now.duration_since(since);
                if age >= self.limits.lockout {
                    self.fails.remove(&src);
                } else if n >= self.limits.login_failures {
                    let left = self.limits.lockout.saturating_sub(age).as_secs() + 1;
                    return Err(LoginError::Locked(left));
                }
            }
            if let Some((n, since)) = self.global_fails {
                let age = now.duration_since(since);
                if age >= GLOBAL_WINDOW {
                    self.global_fails = None;
                    self.slow_last = None;
                } else if n >= self.limits.global_failures_per_minute {
                    // Verlangsamung statt Sperre: je SLOW_GAP wird ein Versuch geprüft, so
                    // kommt auch die richtige PIN durch. Die übrigen werden nicht verglichen
                    // (ein Treffer wäre sonst ein Orakel).
                    if let Some(last) = self.slow_last {
                        let since_last = now.duration_since(last);
                        if since_last < SLOW_GAP {
                            return Err(LoginError::Locked(
                                SLOW_GAP.saturating_sub(since_last).as_secs() + 1,
                            ));
                        }
                    }
                    self.slow_last = Some(now);
                }
            }
        }
        // Beide PINs immer vergleichen, damit die Dauer nichts verrät.
        let is_dj = ct_eq(pin, &self.dj_pin);
        let is_guest = ct_eq(pin, &self.guest_pin);
        let role = if is_dj {
            Role::Dj
        } else if is_guest {
            Role::Guest
        } else {
            if trusted {
                return Err(LoginError::WrongPin);
            }
            let e = self.fails.entry(src).or_insert((0, now));
            e.0 += 1;
            let g = self.global_fails.get_or_insert((0, now));
            g.0 += 1;
            if self.fails.len() > 1024 {
                let lockout = self.limits.lockout;
                self.fails
                    .retain(|_, (_, t)| now.duration_since(*t) < lockout);
            }
            return Err(LoginError::WrongPin);
        };
        // Nur eine DJ-Anmeldung setzt den Zähler zurück. Eine Gast-Anmeldung darf es nicht:
        // sonst ließe sich die DJ-PIN zwischen zwei Gast-Anmeldungen ohne Sperre durchprobieren (#116).
        if role == Role::Dj {
            self.fails.remove(&src);
        }
        let token = random_token().ok_or(LoginError::NoRandom)?;
        self.make_room(ip, role);
        self.sessions.insert(
            token.clone(),
            Session {
                role,
                ip,
                bucket: Bucket::new(now),
                created: now,
            },
        );
        Ok((token, role))
    }

    /// Schafft Platz für eine neue Sitzung: erst je Adresse und Rolle, dann je Rolle.
    /// Verdrängt werden nur Sitzungen derselben Rolle, ein Gast wirft also nie einen DJ hinaus.
    fn make_room(&mut self, ip: IpAddr, role: Role) {
        let per_role = match role {
            Role::Guest => self.limits.max_sessions,
            Role::Dj => self.limits.max_dj_sessions,
        };
        let per_ip = self.limits.max_sessions_per_ip;
        loop {
            let same = |s: &&Session| s.role == role;
            let ip_n = self
                .sessions
                .values()
                .filter(|s| same(s) && s.ip == ip)
                .count();
            let role_n = self.sessions.values().filter(same).count();
            let by_ip = ip_n >= per_ip;
            if !by_ip && role_n < per_role {
                return;
            }
            let victim = self
                .sessions
                .iter()
                .filter(|(_, s)| s.role == role && (!by_ip || s.ip == ip))
                .min_by_key(|(_, s)| s.created)
                .map(|(k, _)| k.clone());
            match victim {
                Some(k) => {
                    self.sessions.remove(&k);
                }
                None => return,
            }
        }
    }

    /// Rolle zu einem Token, wenn es gilt.
    pub fn role_of(&self, token: &str) -> Option<Role> {
        // Kein Hash-Zugriff mit dem Eingabewert als Zeitkanal nötig: das Token ist 128 Bit Zufall.
        self.sessions.get(token).map(|s| s.role)
    }

    /// Zählt einen Befehl der Sitzung; `false`, wenn die Sitzung zu viele schickt.
    pub fn command_allowed(&mut self, token: &str, now: Instant) -> bool {
        let max = self.limits.commands_per_second;
        match self.sessions.get_mut(token) {
            Some(s) => s.bucket.take(now, max),
            None => false,
        }
    }

    /// Darf diese Adresse jetzt einen Wunsch einreichen? Zählt ihn bei Erfolg.
    /// Bei `Err` die Wartezeit in Sekunden.
    pub fn wish_allowed(&mut self, ip: IpAddr, now: Instant) -> Result<(), u64> {
        if let Some(last) = self.wishes.get(&ip) {
            let age = now.duration_since(*last);
            if age < self.limits.wish_interval {
                return Err(self.limits.wish_interval.saturating_sub(age).as_secs() + 1);
            }
        }
        if self.wishes.len() > 1024 {
            let iv = self.limits.wish_interval;
            self.wishes.retain(|_, t| now.duration_since(*t) < iv);
        }
        self.wishes.insert(ip, now);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const IP: IpAddr = IpAddr::V4(std::net::Ipv4Addr::new(192, 0, 2, 7));

    fn auth() -> Auth {
        Auth::with_pins(Limits::default(), "111111", "222222")
    }

    #[test]
    fn pins_are_six_digits_and_differ() {
        for _ in 0..50 {
            let p = random_pin().unwrap();
            assert_eq!(p.len(), 6);
            assert!(p.bytes().all(|b| b.is_ascii_digit()));
        }
        assert_ne!(random_token().unwrap(), random_token().unwrap());
    }

    #[test]
    fn roles_follow_the_pin() {
        let mut a = auth();
        let now = Instant::now();
        let (tg, rg) = a.login(IP, "111111", now).unwrap();
        let (td, rd) = a.login(IP, "222222", now).unwrap();
        assert_eq!((rg, rd), (Role::Guest, Role::Dj));
        assert_eq!(a.role_of(&tg), Some(Role::Guest));
        assert_eq!(a.role_of(&td), Some(Role::Dj));
        assert_eq!(a.role_of("nope"), None);
    }

    #[test]
    fn wrong_pins_lock_out_then_recover() {
        let mut a = auth();
        let t0 = Instant::now();
        for _ in 0..5 {
            assert_eq!(a.login(IP, "000000", t0), Err(LoginError::WrongPin));
        }
        // Gesperrt: auch die richtige PIN geht nicht.
        assert!(matches!(
            a.login(IP, "111111", t0),
            Err(LoginError::Locked(_))
        ));
        // Eine andere Adresse ist nicht betroffen.
        let other = IpAddr::V4(std::net::Ipv4Addr::new(192, 0, 2, 8));
        assert!(a.login(other, "111111", t0).is_ok());
        // Nach der Sperre geht es wieder.
        assert!(a.login(IP, "111111", t0 + Duration::from_secs(61)).is_ok());
    }

    #[test]
    fn wish_interval_per_address() {
        let mut a = auth();
        let t0 = Instant::now();
        assert!(a.wish_allowed(IP, t0).is_ok());
        assert!(a.wish_allowed(IP, t0 + Duration::from_secs(3)).is_err());
        assert!(a.wish_allowed(IP, t0 + Duration::from_secs(11)).is_ok());
    }

    #[test]
    fn command_bucket_resets_each_second() {
        let mut a = auth();
        let t0 = Instant::now();
        let (t, _) = a.login(IP, "222222", t0).unwrap();
        for _ in 0..10 {
            assert!(a.command_allowed(&t, t0));
        }
        assert!(!a.command_allowed(&t, t0));
        assert!(a.command_allowed(&t, t0 + Duration::from_millis(1100)));
    }

    #[test]
    fn renew_drops_sessions_and_old_pins() {
        let mut a = auth();
        let now = Instant::now();
        let (t, _) = a.login(IP, "222222", now).unwrap();
        assert!(a.renew());
        assert_eq!(a.role_of(&t), None);
        assert_ne!(a.dj_pin(), "222222");
    }

    fn ip(n: u8) -> IpAddr {
        IpAddr::V4(std::net::Ipv4Addr::new(192, 0, 2, n))
    }

    #[test]
    fn guest_login_does_not_reset_the_dj_lockout() {
        let mut a = auth();
        let t0 = Instant::now();
        // Vier Fehlversuche, dann ein Gast-Login, dann der fünfte: die Sperre muss greifen.
        for _ in 0..4 {
            assert_eq!(a.login(IP, "000000", t0), Err(LoginError::WrongPin));
        }
        assert!(a.login(IP, "111111", t0).is_ok());
        assert_eq!(a.login(IP, "000001", t0), Err(LoginError::WrongPin));
        assert!(matches!(
            a.login(IP, "222222", t0),
            Err(LoginError::Locked(_))
        ));
        // Durchprobieren im Wechsel mit Gast-Logins kommt nie über fünf Versuche hinaus.
        let mut b = auth();
        let mut tried = 0;
        for i in 0..40 {
            if b.login(IP, "111111", t0).is_err() {
                break;
            }
            match b.login(IP, &format!("{i:06}"), t0) {
                Err(LoginError::WrongPin) => tried += 1,
                _ => break,
            }
        }
        assert!(tried <= 5, "{tried} Versuche ohne Sperre");
    }

    #[test]
    fn dj_login_resets_the_counter() {
        let mut a = auth();
        let t0 = Instant::now();
        for _ in 0..4 {
            let _ = a.login(IP, "000000", t0);
        }
        assert!(a.login(IP, "222222", t0).is_ok());
        for _ in 0..5 {
            assert_eq!(a.login(IP, "000000", t0), Err(LoginError::WrongPin));
        }
    }

    #[test]
    fn many_wrong_pins_from_many_addresses_neither_change_the_pin_nor_lock_out_the_dj() {
        let mut a = auth();
        let t0 = Instant::now();
        let (td, _) = a.login(ip(1), "222222", t0).unwrap();
        for i in 0..200u32 {
            let src = IpAddr::V4(std::net::Ipv4Addr::new(
                198,
                51,
                (i / 250) as u8,
                (i % 250) as u8,
            ));
            for _ in 0..5 {
                let _ = a.login(src, "000000", t0);
            }
        }
        // Die PIN bleibt, der echte DJ kommt von seiner Adresse sofort wieder herein (#162).
        assert_eq!(a.dj_pin(), "222222");
        assert_eq!(
            a.login_with_token(ip(1), "222222", Some(&td), t0)
                .map(|(_, r)| r),
            Ok(Role::Dj)
        );
        assert_eq!(a.role_of(&td), Some(Role::Dj));
    }

    fn lan(n: u16) -> IpAddr {
        IpAddr::V6(std::net::Ipv6Addr::new(0xfd00, 1, 2, 3, 0, 0, 0, n))
    }

    #[test]
    fn ipv6_addresses_are_counted_individually() {
        let mut a = auth();
        let t0 = Instant::now();
        for _ in 0..10 {
            let _ = a.login(lan(1), "000000", t0);
        }
        assert!(matches!(
            a.login(lan(1), "222222", t0),
            Err(LoginError::Locked(_))
        ));
        // Ein Nachbar im selben /64 ist nicht betroffen.
        assert_eq!(a.login(lan(2), "222222", t0).map(|(_, r)| r), Ok(Role::Dj));
    }

    #[test]
    fn changing_ula_addresses_is_slowed_down_globally() {
        let mut a = auth();
        let t0 = Instant::now();
        let mut checked = 0;
        for n in 1..=2000u16 {
            if a.login(lan(n), "000000", t0) == Err(LoginError::WrongPin) {
                checked += 1;
            }
        }
        // 30 in der Minute plus ein geprüfter Versuch in der Verlangsamung.
        assert_eq!(checked, 31);
        // Dauerfeuer über 30 Sekunden mit immer neuen Adressen: höchstens ein Versuch je 2 s.
        let t1 = t0 + Duration::from_secs(120);
        let mut checked = 0;
        for s in 0..60u16 {
            for n in 0..50u16 {
                let ip = IpAddr::V6(std::net::Ipv6Addr::new(0xfd00, 1, 2, 4, 0, 0, s, n));
                let t = t1 + Duration::from_millis(u64::from(s) * 500);
                if a.login(ip, "000000", t) == Err(LoginError::WrongPin) {
                    checked += 1;
                }
            }
        }
        assert!(checked <= 30 + 15, "{checked} geprüfte Versuche in 30 s");
    }

    #[test]
    fn real_dj_gets_in_during_a_guessing_attack() {
        let mut a = auth();
        let t0 = Instant::now();
        let (dj, _) = a.login(lan(1), "222222", t0).unwrap();
        for n in 2..=2000u16 {
            let _ = a.login(lan(n), "000000", t0);
        }
        // Mit gültigem Token kommt der DJ trotz Angriff herein.
        assert_eq!(
            a.login_with_token(lan(1), "222222", Some(&dj), t0)
                .map(|(_, r)| r),
            Ok(Role::Dj)
        );
        // Ohne Token genügt die Adresse nicht.
        assert!(matches!(
            a.login(lan(1), "000000", t0),
            Err(LoginError::Locked(_))
        ));
        // Mit PIN ohne Token: langsamer, aber nach 2 s ist ein Versuch frei.
        assert_eq!(
            a.login(lan(5000), "222222", t0 + Duration::from_secs(3))
                .map(|(_, r)| r),
            Ok(Role::Dj)
        );
        assert_eq!(a.dj_pin(), "222222");
        assert_eq!(a.role_of(&dj), Some(Role::Dj));
    }

    #[test]
    fn dj_address_alone_does_not_exempt_from_the_brake() {
        let mut a = auth();
        let t0 = Instant::now();
        a.login(ip(1), "222222", t0).unwrap();
        for _ in 0..5 {
            let _ = a.login(ip(1), "000000", t0);
        }
        assert!(matches!(
            a.login(ip(1), "000000", t0),
            Err(LoginError::Locked(_))
        ));
    }

    #[test]
    fn dj_pin_has_eight_digits() {
        for _ in 0..50 {
            let p = random_dj_pin().unwrap();
            assert_eq!(p.len(), 8);
            assert!(p.bytes().all(|b| b.is_ascii_digit()));
        }
        assert_eq!(Auth::new(Limits::default()).unwrap().dj_pin().len(), 8);
    }

    #[test]
    fn guests_never_evict_a_dj_session() {
        let mut a = Auth::with_pins(
            Limits {
                max_sessions: 2,
                max_sessions_per_ip: 100,
                ..Limits::default()
            },
            "111111",
            "222222",
        );
        let t0 = Instant::now();
        let (dj, _) = a.login(ip(1), "222222", t0).unwrap();
        for i in 1..=50u64 {
            a.login(ip(2), "111111", t0 + Duration::from_millis(i))
                .unwrap();
        }
        assert_eq!(a.role_of(&dj), Some(Role::Dj));
        assert_eq!(
            a.sessions
                .values()
                .filter(|s| s.role == Role::Guest)
                .count(),
            2
        );
    }

    #[test]
    fn dj_sessions_have_their_own_cap_and_one_address_cannot_flood() {
        let mut a = Auth::with_pins(
            Limits {
                max_dj_sessions: 2,
                max_sessions_per_ip: 3,
                ..Limits::default()
            },
            "111111",
            "222222",
        );
        let t0 = Instant::now();
        let (d1, _) = a.login(ip(1), "222222", t0).unwrap();
        let (d2, _) = a
            .login(ip(1), "222222", t0 + Duration::from_millis(1))
            .unwrap();
        let (d3, _) = a
            .login(ip(1), "222222", t0 + Duration::from_millis(2))
            .unwrap();
        assert_eq!(a.role_of(&d1), None);
        assert!(a.role_of(&d2).is_some() && a.role_of(&d3).is_some());
        // Eine Adresse hält höchstens drei Gast-Sitzungen, andere Adressen bleiben unberührt.
        let (other, _) = a.login(ip(5), "111111", t0).unwrap();
        for i in 1..=10u64 {
            a.login(ip(6), "111111", t0 + Duration::from_millis(i))
                .unwrap();
        }
        let from6 = a.sessions.values().filter(|s| s.ip == ip(6)).count();
        assert_eq!(from6, 3);
        assert_eq!(a.role_of(&other), Some(Role::Guest));
    }

    #[test]
    fn oldest_session_is_evicted() {
        let mut a = Auth::with_pins(
            Limits {
                max_sessions: 2,
                ..Limits::default()
            },
            "111111",
            "222222",
        );
        let t0 = Instant::now();
        let (a1, _) = a.login(IP, "111111", t0).unwrap();
        let (a2, _) = a
            .login(IP, "111111", t0 + Duration::from_millis(1))
            .unwrap();
        let (a3, _) = a
            .login(IP, "111111", t0 + Duration::from_millis(2))
            .unwrap();
        assert_eq!(a.role_of(&a1), None);
        assert!(a.role_of(&a2).is_some() && a.role_of(&a3).is_some());
    }
}
