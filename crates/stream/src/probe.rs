//! „Verbindung testen“: meldet sich einmal an dem eingestellten Server an und übersetzt das
//! Ergebnis in einen Satz, den man ohne Protokollwissen versteht. Sendet keinen Ton.
//!
//! Der Test funktioniert für jeden Servertyp aus [`crate::config::Protocol`] und prüft in
//! dieser Reihenfolge: Name auflösbar, Port offen, Server spricht das gewählte Protokoll,
//! Kennwort (und Benutzer) stimmen.

use std::time::Instant;

use crate::config::{Protocol, StreamConfig};
use crate::kennwort::Kennwort;
use crate::shoutcast::{Source, StreamError, login_shoutcast};

/// Ausgang des Tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ausgang {
    /// Erreichbar und angemeldet.
    Ok,
    /// Erreichbar und Kennwort stimmt, aber die Quelle ist belegt (läuft schon eine Sendung).
    Belegt,
    /// Server nicht erreichbar (Name, Port, Netz, Firewall).
    Unerreichbar,
    /// Erreichbar, Kennwort oder Benutzer falsch.
    Kennwort,
    /// Erreichbar, aber der Server spricht nicht das gewählte Protokoll oder lehnt ab.
    Abgelehnt,
    /// Eingaben unvollständig; nichts wurde gesendet.
    Eingabe,
}

impl Ausgang {
    /// `true`, wenn man senden kann.
    pub fn gut(self) -> bool {
        matches!(self, Self::Ok | Self::Belegt)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Befund {
    pub ausgang: Ausgang,
    /// Ein bis zwei Sätze auf Deutsch, ohne Kennwort.
    pub text: String,
    pub millis: u64,
}

/// Testet `cfg` mit dem Kennwort `kw`.
pub fn run(cfg: &StreamConfig, kw: &Kennwort) -> Befund {
    let t0 = Instant::now();
    let (ausgang, text) = judge(cfg, kw);
    Befund {
        ausgang,
        text,
        millis: t0.elapsed().as_millis() as u64,
    }
}

fn judge(cfg: &StreamConfig, kw: &Kennwort) -> (Ausgang, String) {
    if let Err(e) = cfg.validate() {
        return (Ausgang::Eingabe, e);
    }
    let ziel = cfg.source_url();
    let result = if cfg.protocol == Protocol::Icecast {
        Source::connect(cfg, kw).map(drop)
    } else {
        // Nur anmelden, keine Kopfzeilen: der Server sieht keine Sendung beginnen.
        login_shoutcast(cfg, kw).map(drop)
    };
    match result {
        Ok(()) => (
            Ausgang::Ok,
            format!(
                "Verbunden mit {ziel}. Der Server nimmt das Kennwort an. Es wurde kein Ton gesendet."
            ),
        ),
        Err(StreamError::InUse) => (
            Ausgang::Belegt,
            format!(
                "Verbunden mit {ziel}, das Kennwort stimmt. Auf dem Mountpoint sendet aber \
                 schon jemand; eine zweite Quelle würde abgelehnt."
            ),
        ),
        Err(StreamError::AuthFailed) => (
            Ausgang::Kennwort,
            match cfg.protocol {
                Protocol::Icecast => format!(
                    "Der Server ist erreichbar, aber Benutzer oder Kennwort stimmen nicht \
                     (Benutzer: {}). Bei Icecast ist es das Quell-Kennwort aus der icecast.xml.",
                    cfg.source_user()
                ),
                Protocol::ShoutcastV2 => format!(
                    "Der Server ist erreichbar, aber das Kennwort stimmt nicht. Es muss das \
                     Quell-Kennwort (DJ-Kennwort) des Streams {} sein, nicht das Admin-Kennwort.",
                    cfg.stream_id
                ),
                Protocol::Shoutcast => "Der Server ist erreichbar, aber das Kennwort stimmt \
                     nicht. Es muss das Quell-Kennwort sein, nicht das Admin-Kennwort."
                    .into(),
            },
        ),
        Err(StreamError::Connect(e)) => (
            Ausgang::Unerreichbar,
            format!(
                "Keine Verbindung zu {ziel}: {e}. Prüfe Adresse und Port, ob der Server läuft und \
                 ob eine Firewall den Quell-Port {} sperrt.",
                cfg.source_port()
            ),
        ),
        Err(StreamError::Refused(l)) => (Ausgang::Abgelehnt, format!("{ziel} lehnt ab: {l}")),
        Err(StreamError::Io(e)) => (
            Ausgang::Abgelehnt,
            format!(
                "Die Verbindung zu {ziel} brach ab: {e}. Stimmt der Servertyp \
                 (Shoutcast oder Icecast) und der Port?"
            ),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    /// Ein Server, der eine feste Antwort gibt, nachdem er die erste Anfrage gelesen hat.
    fn server(answer: &'static str) -> u16 {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        std::thread::spawn(move || {
            if let Ok((mut s, _)) = l.accept() {
                let mut buf = [0u8; 1024];
                let _ = s.read(&mut buf);
                let _ = s.write_all(answer.as_bytes());
                let _ = s.flush();
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
        });
        port
    }

    fn cfg(protocol: Protocol, server_port: u16) -> StreamConfig {
        // Shoutcast hängt eins an den Port.
        let port = if protocol.is_shoutcast() {
            server_port - 1
        } else {
            server_port
        };
        StreamConfig {
            protocol,
            host: "127.0.0.1".into(),
            port,
            ..StreamConfig::default()
        }
    }

    #[test]
    fn shoutcast_ok_wrong_password_and_wrong_server() {
        let kw = Kennwort::from_value("kw");
        let p = server("OK2\r\nicy-caps:11\r\n\r\n");
        let b = run(&cfg(Protocol::Shoutcast, p), &kw);
        assert_eq!(b.ausgang, Ausgang::Ok, "{}", b.text);

        let p = server("invalid password\r\n");
        let b = run(&cfg(Protocol::ShoutcastV2, p), &kw);
        assert_eq!(b.ausgang, Ausgang::Kennwort);
        assert!(b.text.contains("Quell-Kennwort"));
        assert!(!b.text.contains("kw."), "kein Kennwort im Text");

        let p = server("HTTP/1.0 400 Bad Request\r\n\r\n");
        let b = run(&cfg(Protocol::Shoutcast, p), &kw);
        assert_eq!(b.ausgang, Ausgang::Abgelehnt);
        assert!(b.text.contains("Webserver"));
    }

    #[test]
    fn icecast_ok_unauthorized_in_use_and_not_http() {
        let kw = Kennwort::from_value("kw");
        let ok = server("HTTP/1.1 100 Continue\r\n\r\n");
        assert_eq!(run(&cfg(Protocol::Icecast, ok), &kw).ausgang, Ausgang::Ok);
        let p = server("HTTP/1.1 401 Authentication Required\r\n\r\n");
        let b = run(&cfg(Protocol::Icecast, p), &kw);
        assert_eq!(b.ausgang, Ausgang::Kennwort);
        assert!(b.text.contains("source"));
        let p = server("HTTP/1.0 403 Forbidden\r\n\r\nMountpoint in use");
        assert_eq!(
            run(&cfg(Protocol::Icecast, p), &kw).ausgang,
            Ausgang::Belegt
        );
        let p = server("OK2\r\n\r\n");
        assert_eq!(
            run(&cfg(Protocol::Icecast, p), &kw).ausgang,
            Ausgang::Abgelehnt
        );
    }

    #[test]
    fn unreachable_and_bad_input_are_explained() {
        let kw = Kennwort::from_value("kw");
        // Ein gerade frei gewordener Port: nichts hört zu.
        let port = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let b = run(&cfg(Protocol::Icecast, port), &kw);
        assert_eq!(b.ausgang, Ausgang::Unerreichbar);
        assert!(b.text.contains("Firewall"));
        let leer = StreamConfig {
            host: " ".into(),
            ..StreamConfig::default()
        };
        assert_eq!(run(&leer, &kw).ausgang, Ausgang::Eingabe);
    }
}
