//! Quellclient gegen den Mini-Shoutcast: Anmeldung, MP3, Titel, Wiederverbinden,
//! abgelehnte Anmeldung.

#![allow(clippy::unwrap_used, clippy::expect_used)] // Tests duerfen abbrechen (Regel 6 gilt fuer Produktivcode).

mod fake_server;

use std::sync::atomic::Ordering;
use std::time::Duration;

use fake_server::{start, wait_for};
use woa_stream::mp3::{Mp3Encoder, sine};
use woa_stream::shoutcast::{Source, StreamError, listeners, update_title};
use woa_stream::uplink::{Phase, Uplink};
use woa_stream::{Kennwort, StreamConfig};

const KW: &str = "test-kw-123";

fn cfg(fake: &fake_server::Fake) -> StreamConfig {
    StreamConfig {
        protocol: woa_stream::Protocol::ShoutcastV2,
        host: "127.0.0.1".into(),
        port: fake.port - 1,
        admin_port: Some(fake.admin),
        stream_id: 2,
        ..StreamConfig::default()
    }
}

/// Schickt `seconds` Ton in 100-ms-Happen.
fn feed(up: &Uplink, enc: &mut Mp3Encoder, seconds: f32) {
    let n = 4410;
    for k in 0..(seconds * 10.0) as usize {
        let s = sine(44_100, 440.0, k * n, n, 0.5);
        up.send(enc.encode(&s, &s).unwrap());
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn logs_in_sends_mp3_and_title() {
    let fake = start(&format!("{KW}:#2"));
    let up = Uplink::spawn(cfg(&fake), Kennwort::from_value(KW)).unwrap();
    up.set_title("Probe – Äh & Co");
    let mut enc = Mp3Encoder::new(44_100, 128).unwrap();
    feed(&up, &mut enc, 1.0);
    assert!(wait_for(Duration::from_secs(5), || {
        fake.seen.bytes.lock().unwrap().len() > 8000
    }));
    assert_eq!(fake.seen.logins.load(Ordering::Relaxed), 1);
    let bytes = fake.seen.bytes.lock().unwrap().clone();
    assert!(
        bytes
            .windows(2)
            .any(|w| w[0] == 0xFF && w[1] & 0xE0 == 0xE0)
    );
    let headers = fake.seen.headers.lock().unwrap()[0].clone();
    assert!(headers.contains("content-type:audio/mpeg"));
    assert!(headers.contains("icy-br:128"));
    assert!(headers.contains("icy-pub:0"));
    assert!(wait_for(Duration::from_secs(5), || {
        fake.seen.requests.lock().unwrap().iter().any(|r| {
            r.contains("mode=updinfo")
                && r.contains("sid=2")
                && r.contains("song=Probe%20%E2%80%93%20%C3%84h%20%26%20Co")
        })
    }));
    assert!(wait_for(Duration::from_secs(3), || up.state().listeners == Some(3)));
    assert_eq!(up.state().phase, Phase::Connected);
    assert_eq!(listeners(&cfg(&fake)), Some(3));
    let st = woa_stream::shoutcast::listener_stats(&cfg(&fake)).unwrap();
    assert_eq!((st.peak, st.unique), (Some(5), Some(3)));
}

#[test]
fn reconnects_after_drop() {
    let fake = start(&format!("{KW}:#2"));
    fake.seen.drop_after.store(4000, Ordering::Relaxed);
    let up = Uplink::spawn(cfg(&fake), Kennwort::from_value(KW)).unwrap();
    up.set_title("Vorher");
    let mut enc = Mp3Encoder::new(44_100, 128).unwrap();
    let ok = wait_for(Duration::from_secs(10), || {
        feed(&up, &mut enc, 0.2);
        fake.seen.logins.load(Ordering::Relaxed) >= 2
    });
    assert!(ok, "no second login");
    assert!(up.state().reconnects >= 1);
    assert!(wait_for(Duration::from_secs(3), || up.state().phase
        == Phase::Connected));
    // Titel nach dem Wiederverbinden erneut gesetzt.
    assert!(wait_for(Duration::from_secs(3), || {
        fake.seen
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r.contains("song=Vorher"))
            .count()
            >= 2
    }));
}

#[test]
fn wrong_password_is_a_clear_error() {
    let fake = start(&format!("{KW}:#2"));
    let e = Source::connect(&cfg(&fake), &Kennwort::from_value("falsch")).unwrap_err();
    assert!(matches!(e, StreamError::AuthFailed), "{e:?}");
    assert!(e.to_string().contains("Kennwort"));
    let up = Uplink::spawn(cfg(&fake), Kennwort::from_value("falsch")).unwrap();
    assert!(wait_for(Duration::from_secs(3), || up.state().auth_failed));
    let st = up.state();
    assert!(st.last_error.unwrap().contains("Kennwort"));
    assert_ne!(st.phase, Phase::Connected);
    assert_eq!(fake.seen.logins.load(Ordering::Relaxed), 0);
    // Kein Hämmern: nach einer Ablehnung wartet der Client lange.
    std::thread::sleep(Duration::from_millis(1500));
    assert_eq!(fake.seen.rejected.load(Ordering::Relaxed), 2);
}

#[test]
fn unreachable_server_is_a_connect_error() {
    let fake = start("x");
    // Freien Port vom System holen; ein fester Abstand traf unter Windows den Schein-Server
    // eines parallel laufenden Tests (Verbindung gelang).
    let free = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    // Der Client verbindet sich auf `port + 1` (Shoutcast-Quellport), siehe `cfg`.
    let port = free.local_addr().unwrap().port() - 1;
    drop(free);
    let c = StreamConfig { port, ..cfg(&fake) };
    drop(fake);
    let e = Source::connect(&c, &Kennwort::from_value(KW)).unwrap_err();
    assert!(matches!(e, StreamError::Connect(_)), "{e:?}");
}

#[test]
fn titles_only_on_loopback() {
    use woa_stream::shoutcast::{is_loopback, titles_possible};
    assert!(is_loopback("127.0.0.1") && is_loopback("localhost") && is_loopback("::1"));
    assert!(!is_loopback("stream.wironair.de") && !is_loopback("10.0.0.1"));
    let remote = StreamConfig {
        host: "stream.wironair.de".into(),
        admin_port: Some(8000),
        ..StreamConfig::default()
    };
    assert!(!titles_possible(&remote));
    // Refused before any connection: the password never leaves the machine.
    let e = update_title(&remote, &Kennwort::from_value(KW), "x").unwrap_err();
    assert!(matches!(e, StreamError::Refused(_)), "{e:?}");
}

/// Entfernter Server: Titel geht als POST mit Bearer-Kopf an den Sendeserver, nicht an admin.cgi.
#[test]
fn remote_title_goes_to_server_endpoint() {
    use std::io::{Read, Write};
    use woa_stream::shoutcast::update_title_via_server;
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let srv = std::thread::spawn(move || {
        let mut got = Vec::new();
        for code in ["204 No Content", "401 Unauthorized"] {
            let (mut s, _) = l.accept().unwrap();
            s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            let mut buf = vec![0u8; 4096];
            let mut req = Vec::new();
            loop {
                let n = s.read(&mut buf).unwrap();
                req.extend_from_slice(&buf[..n]);
                let t = String::from_utf8_lossy(&req);
                if let Some((head, body)) = t.split_once("\r\n\r\n") {
                    let len = head
                        .lines()
                        .find_map(|l| {
                            l.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .map(|v| v.trim().parse::<usize>().unwrap())
                        })
                        .unwrap_or(0);
                    if body.len() >= len || n == 0 {
                        break;
                    }
                }
            }
            got.push(String::from_utf8_lossy(&req).to_string());
            write!(
                s,
                "HTTP/1.1 {code}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            )
            .unwrap();
        }
        got
    });
    let url = format!("http://127.0.0.1:{port}/api/titel");
    let kw = Kennwort::from_value(KW);
    update_title_via_server(&url, &kw, "Probe – Äh").unwrap();
    assert!(matches!(
        update_title_via_server(&url, &Kennwort::from_value("falsch"), "x"),
        Err(woa_stream::shoutcast::StreamError::AuthFailed)
    ));
    let got = srv.join().unwrap();
    assert!(got[0].starts_with("POST /api/titel "));
    assert!(got[0].contains(&format!("Bearer {KW}")));
    assert!(got[0].contains("\"titel\":\"Probe – Äh\""));
    assert!(!got[0].contains("admin.cgi"));
    // Kein Klartext übers Netz: http nur auf Loopback.
    assert!(update_title_via_server("http://198.51.100.1/api/titel", &kw, "x").is_err());
}
