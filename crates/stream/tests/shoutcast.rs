//! Quellclient gegen den Mini-Shoutcast: Anmeldung, MP3, Titel, Wiederverbinden,
//! abgelehnte Anmeldung.

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
    let c = StreamConfig {
        port: fake.port.wrapping_add(10) - 1,
        ..cfg(&fake)
    };
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
