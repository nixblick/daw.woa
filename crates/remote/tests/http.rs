//! Integrationstest der Fernbedienung gegen 127.0.0.1: Anmeldung, Rollen, Wünsche,
//! Ratenbegrenzung, Vollstand und Änderungen per WebSocket, 20 Handys, Port zu nach dem
//! Ausschalten. Kein Ton, kein Audiogerät.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream, UdpSocket};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tungstenite::{Message, WebSocket};
use woa_radio::party::PartyCommand;
use woa_remote::snapshot::short_id;
use woa_remote::{
    CatalogItem, Limits, NowSnap, PartySnapshot, Remote, RemoteConfig, Role, WishSnap,
};

const GUEST: &str = "111111";
const DJ: &str = "222222";

fn start_with(limits: Limits, osc: bool) -> Remote {
    Remote::start(RemoteConfig {
        osc: osc.then(|| "127.0.0.1:0".parse().unwrap()),
        limits,
        pins: Some((GUEST.into(), DJ.into())),
        ..RemoteConfig::default()
    })
    .expect("start")
}

fn start() -> Remote {
    start_with(Limits::default(), false)
}

fn http(
    addr: SocketAddr,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: Option<&str>,
) -> (u16, Value, String) {
    let mut s = TcpStream::connect(addr).expect("connect");
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let body = body.unwrap_or("");
    let auth = token
        .map(|t| format!("Authorization: Bearer {t}\r\n"))
        .unwrap_or_default();
    let req = format!(
        "{method} {path} HTTP/1.1\r\nHost: x\r\n{auth}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    s.write_all(req.as_bytes()).unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).ok();
    let (head, body) = out.split_once("\r\n\r\n").unwrap_or((&out, ""));
    let code = head
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    let json = serde_json::from_str(body).unwrap_or(Value::Null);
    (code, json, head.to_string())
}

fn login(addr: SocketAddr, pin: &str) -> (u16, Value) {
    let (c, j, _) = http(
        addr,
        "POST",
        "/api/login",
        None,
        Some(&json!({"pin": pin}).to_string()),
    );
    (c, j)
}

fn token(addr: SocketAddr, pin: &str) -> String {
    let (c, j) = login(addr, pin);
    assert_eq!(c, 200, "{j}");
    j["token"].as_str().unwrap().to_string()
}

fn cmd(addr: SocketAddr, token: &str, body: Value) -> (u16, Value) {
    let (c, j, _) = http(
        addr,
        "POST",
        "/api/cmd",
        Some(token),
        Some(&body.to_string()),
    );
    (c, j)
}

fn ws_connect(addr: SocketAddr, token: &str) -> WebSocket<TcpStream> {
    let s = TcpStream::connect(addr).expect("connect");
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let (ws, resp) = tungstenite::client(format!("ws://{addr}/ws?token={token}"), s)
        .expect("websocket handshake");
    assert_eq!(resp.status().as_u16(), 101);
    ws
}

fn next_json(ws: &mut WebSocket<TcpStream>) -> Value {
    loop {
        match ws.read().expect("read") {
            Message::Text(t) => return serde_json::from_str(t.as_str()).unwrap(),
            Message::Close(_) => panic!("unexpected close"),
            _ => {}
        }
    }
}

/// Wartet, bis das Handy ein Close bekommt oder die Verbindung endet.
fn expect_close(ws: &mut WebSocket<TcpStream>) {
    loop {
        match ws.read() {
            Ok(Message::Close(_)) | Err(_) => return,
            Ok(_) => {}
        }
    }
}

fn wait_for(what: &str, mut f: impl FnMut() -> bool) {
    let until = Instant::now() + Duration::from_secs(5);
    while Instant::now() < until {
        if f() {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("Zeit abgelaufen: {what}");
}

fn snap(title: &str, remaining: &str) -> PartySnapshot {
    PartySnapshot {
        now: Some(NowSnap {
            title: title.into(),
            artist: "A".into(),
            remaining: remaining.into(),
            progress: 0.25,
            playing: true,
        }),
        next: Some("Nächster".into()),
        wishes: vec![],
        level_db: 0.0,
    }
}

fn item(title: &str, path: &str) -> CatalogItem {
    CatalogItem {
        title: title.into(),
        path: path.into(),
    }
}

#[test]
fn wrong_pin_is_401_and_right_pins_give_roles() {
    let r = start();
    let a = r.addr();
    assert_eq!(login(a, "000000").0, 401);
    assert_eq!(login(a, "").0, 401);
    assert_eq!(
        http(a, "POST", "/api/login", None, Some("kein json")).0,
        401
    );
    let (c, j) = login(a, GUEST);
    assert_eq!((c, j["role"].as_str()), (200, Some("guest")));
    let (c, j) = login(a, DJ);
    assert_eq!((c, j["role"].as_str()), (200, Some("dj")));
    // Ohne Token kein Zustand, keine Suche, kein Befehl, kein WebSocket.
    assert_eq!(http(a, "GET", "/api/state", None, None).0, 401);
    assert_eq!(http(a, "GET", "/api/state", Some("falsch"), None).0, 401);
    assert_eq!(http(a, "GET", "/api/search?q=ab", None, None).0, 401);
    assert_eq!(cmd(a, "falsch", json!({"cmd": "next"})).0, 401);
    assert_eq!(http(a, "GET", "/ws", None, None).0, 401);
    // Die Seite selbst ist ohne Anmeldung zu haben und enthält keine Daten.
    let (c, _, head) = http(a, "GET", "/", None, None);
    assert_eq!(c, 200);
    assert!(head.contains("text/html"));
}

#[test]
fn guest_cannot_control_but_wish_lands_in_the_list() {
    let r = start();
    let a = r.addr();
    r.set_catalog(&[item("Abba - Waterloo", "/m/abba.mp3")]);
    let g = token(a, GUEST);
    for c in ["play_pause", "next", "volume_up", "volume_down"] {
        assert_eq!(cmd(a, &g, json!({"cmd": c})).0, 403, "{c}");
    }
    assert_eq!(cmd(a, &g, json!({"cmd": "play_wish", "id": "x"})).0, 403);
    assert_eq!(cmd(a, &g, json!({"cmd": "drop_wish", "id": "x"})).0, 403);
    assert!(r.poll().is_empty(), "Gast hat nichts ausgelöst");

    let (c, hits, _) = http(a, "GET", "/api/search?q=waterloo", Some(&g), None);
    assert_eq!(c, 200);
    let id = hits["results"][0]["id"]
        .as_str()
        .expect("treffer")
        .to_string();
    assert!(!hits.to_string().contains("/m/abba"), "kein Pfad am Handy");
    assert_eq!(cmd(a, &g, json!({"cmd": "wish", "id": id})).0, 200);
    let got = r.poll();
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].role, Role::Guest);
    assert_eq!(
        got[0].command,
        PartyCommand::Wish {
            path: "/m/abba.mp3".into(),
            title: "Abba - Waterloo".into()
        }
    );
}

#[test]
fn dj_commands_reach_the_command_layer() {
    let r = start();
    let a = r.addr();
    let d = token(a, DJ);
    for (c, want) in [
        ("play_pause", PartyCommand::PlayPause),
        ("next", PartyCommand::Next),
        ("volume_up", PartyCommand::VolumeUp),
        ("volume_down", PartyCommand::VolumeDown),
    ] {
        assert_eq!(cmd(a, &d, json!({"cmd": c})).0, 200);
        let got = r.poll();
        assert_eq!(got.len(), 1, "{c}");
        assert_eq!((got[0].role, &got[0].command), (Role::Dj, &want));
    }
    // Vollbild gehört dem Rechner, nicht dem Handy; Unbekanntes und Kaputtes wird abgelehnt.
    assert_eq!(cmd(a, &d, json!({"cmd": "fullscreen"})).0, 400);
    assert_eq!(cmd(a, &d, json!({"cmd": "format_c"})).0, 400);
    assert_eq!(cmd(a, &d, json!({"kein": "cmd"})).0, 400);
    assert_eq!(
        http(a, "POST", "/api/cmd", Some(&d), Some("{kaputt")).0,
        400
    );
    assert_eq!(
        cmd(a, &d, json!({"cmd": "play_wish", "id": "gibtsnicht"})).0,
        404
    );
    assert!(r.poll().is_empty());
}

#[test]
fn dj_can_play_and_drop_wishes_by_id() {
    let r = start();
    let a = r.addr();
    let mut s = snap("T", "1:00");
    s.wishes = vec![WishSnap {
        title: "X".into(),
        votes: 1,
        path: "/m/x.wav".into(),
    }];
    r.publish(&s);
    let d = token(a, DJ);
    let (_, st, _) = http(a, "GET", "/api/state", Some(&d), None);
    let id = st["state"]["wishes"][0]["id"].as_str().unwrap().to_string();
    assert_eq!(id, short_id("/m/x.wav".as_ref()));
    assert!(!st.to_string().contains("/m/x.wav"));
    assert_eq!(cmd(a, &d, json!({"cmd": "play_wish", "id": id})).0, 200);
    assert_eq!(cmd(a, &d, json!({"cmd": "drop_wish", "id": id})).0, 200);
    let got = r.poll();
    assert_eq!(got[0].command, PartyCommand::PlayWish("/m/x.wav".into()));
    assert_eq!(got[1].command, PartyCommand::DropWish("/m/x.wav".into()));
}

#[test]
fn wish_rate_limit_and_free_text() {
    let r = start_with(
        Limits {
            wish_interval: Duration::from_secs(30),
            ..Limits::default()
        },
        false,
    );
    let a = r.addr();
    let g = token(a, GUEST);
    assert_eq!(
        cmd(a, &g, json!({"cmd": "wish", "title": "Queen - Bohemian"})).0,
        200
    );
    let (c, j) = cmd(a, &g, json!({"cmd": "wish", "title": "Noch einer"}));
    assert_eq!(c, 429, "{j}");
    assert!(j["error"].as_str().unwrap().contains("Wunsch"));
    // Eine zweite Anmeldung von derselben Adresse umgeht die Grenze nicht.
    let g2 = token(a, GUEST);
    assert_eq!(
        cmd(a, &g2, json!({"cmd": "wish", "title": "Und noch einer"})).0,
        429
    );
    let got = r.poll();
    assert_eq!(got.len(), 1);
    assert!(
        matches!(&got[0].command, PartyCommand::Wish { title, .. } if title == "Queen - Bohemian")
    );
    // Zu langer Titel, leerer Titel.
    let d = token(a, DJ);
    assert_eq!(
        cmd(a, &d, json!({"cmd": "wish", "title": "x".repeat(200)})).0,
        400
    );
    assert_eq!(cmd(a, &d, json!({"cmd": "wish", "title": "  "})).0, 400);
}

#[test]
fn command_rate_limit_for_the_dj() {
    let r = start_with(
        Limits {
            commands_per_second: 3,
            ..Limits::default()
        },
        false,
    );
    let a = r.addr();
    let d = token(a, DJ);
    let codes: Vec<u16> = (0..12)
        .map(|_| cmd(a, &d, json!({"cmd": "volume_up"})).0)
        .collect();
    let ok = codes.iter().filter(|c| **c == 200).count();
    assert!(codes.contains(&429), "{codes:?}");
    assert!((3..=6).contains(&ok), "{codes:?}");
}

#[test]
fn repeated_wrong_pins_lock_the_address_out() {
    let r = start();
    let a = r.addr();
    for _ in 0..5 {
        assert_eq!(login(a, "999999").0, 401);
    }
    // Gesperrt: auch die richtige PIN bekommt 429 mit Wartezeit.
    let (c, j, head) = http(
        a,
        "POST",
        "/api/login",
        None,
        Some(&json!({"pin": GUEST}).to_string()),
    );
    assert_eq!(c, 429);
    assert!(j["retry_after"].as_u64().unwrap() > 0);
    assert!(head.contains("Retry-After"));
}

#[test]
fn full_state_on_connect_then_only_changes() {
    let r = start();
    let a = r.addr();
    r.publish(&snap("Waterloo", "3:00"));
    let t = token(a, GUEST);
    let mut ws = ws_connect(a, &t);
    let first = next_json(&mut ws);
    assert_eq!(first["type"], "full");
    assert_eq!(first["role"], "guest");
    assert_eq!(first["state"]["now"]["title"], "Waterloo");
    assert_eq!(first["state"]["next"], "Nächster");
    r.publish(&snap("Waterloo", "2:59"));
    let d = next_json(&mut ws);
    assert_eq!(d["type"], "delta");
    // Nur das Feld, das sich geändert hat, kommt an: „now“, nicht „next“ oder „wishes“.
    let changes = d["changes"].as_object().unwrap();
    assert_eq!(changes.keys().collect::<Vec<_>>(), ["now"]);
    assert_eq!(d["changes"]["now"]["remaining"], "2:59");
    // Gleicher Stand: nichts wird gesendet.
    r.publish(&snap("Waterloo", "2:59"));
    ws.get_ref()
        .set_read_timeout(Some(Duration::from_millis(400)))
        .unwrap();
    assert!(ws.read().is_err(), "unveränderter Stand sendet nichts");
}

#[test]
fn websocket_commands_follow_the_same_rules() {
    let r = start();
    let a = r.addr();
    r.set_catalog(&[item("Abba - Waterloo", "/m/abba.mp3")]);
    let gt = token(a, GUEST);
    let mut g = ws_connect(a, &gt);
    assert_eq!(next_json(&mut g)["type"], "full");
    g.send(Message::text(json!({"cmd": "next"}).to_string()))
        .unwrap();
    let e = next_json(&mut g);
    assert_eq!(
        (e["type"].as_str(), e["status"].as_u64()),
        (Some("error"), Some(403))
    );
    g.send(Message::text(
        json!({"cmd": "wish", "title": "Hallo"}).to_string(),
    ))
    .unwrap();
    assert_eq!(next_json(&mut g)["type"], "ack");

    let dt = token(a, DJ);
    let mut d = ws_connect(a, &dt);
    assert_eq!(next_json(&mut d)["role"], "dj");
    d.send(Message::text(json!({"cmd": "next"}).to_string()))
        .unwrap();
    assert_eq!(next_json(&mut d)["type"], "ack");
    let got = r.poll();
    assert_eq!(got.len(), 2);
    assert!(matches!(got[0].command, PartyCommand::Wish { .. }));
    assert_eq!(got[1].command, PartyCommand::Next);
    // Zu große Nachricht beendet die Verbindung.
    g.send(Message::text("x".repeat(10_000))).ok();
    expect_close(&mut g);
}

#[test]
fn twenty_clients_get_full_state_and_updates() {
    let mut r = start();
    let a = r.addr();
    r.publish(&snap("Start", "1:00"));
    let tokens: Vec<String> = (0..20)
        .map(|i| token(a, if i % 5 == 0 { DJ } else { GUEST }))
        .collect();
    let mut sockets: Vec<WebSocket<TcpStream>> = tokens.iter().map(|t| ws_connect(a, t)).collect();
    for ws in &mut sockets {
        assert_eq!(next_json(ws)["type"], "full");
    }
    wait_for("20 Clients gezählt", || r.clients() == 20);
    r.publish(&snap("Start", "0:59"));
    for ws in &mut sockets {
        let d = next_json(ws);
        assert_eq!(d["changes"]["now"]["remaining"], "0:59");
    }
    // Alle 20 zugleich mit Wünschen von verschiedenen Sitzungen (Adresse ist dieselbe: nur einer
    // kommt durch, der Rest wird begrenzt; der Server bleibt dabei ansprechbar).
    std::thread::scope(|sc| {
        for t in &tokens {
            sc.spawn(move || cmd(a, t, json!({"cmd": "wish", "title": "Sammelwunsch"})).0);
        }
    });
    assert!(!r.poll().is_empty());
    // Ausschalten: alle 20 bekommen ein Close, der Port ist zu.
    r.stop();
    for ws in &mut sockets {
        expect_close(ws);
    }
    assert_eq!(r.clients(), 0);
    assert!(TcpStream::connect_timeout(&a, Duration::from_millis(500)).is_err());
}

#[test]
fn no_port_is_open_when_off() {
    let mut r = start();
    let a = r.addr();
    assert!(TcpStream::connect(a).is_ok(), "läuft: Port offen");
    r.stop();
    assert!(
        TcpStream::connect_timeout(&a, Duration::from_millis(500)).is_err(),
        "aus: Port zu"
    );
    // Auch nach dem Wegwerfen und bei neuem Start auf demselben Port geht es wieder.
    drop(r);
    let r2 = Remote::start(RemoteConfig {
        bind: a,
        pins: Some((GUEST.into(), DJ.into())),
        ..RemoteConfig::default()
    })
    .expect("zweiter Start auf demselben Port");
    assert_eq!(login(r2.addr(), GUEST).0, 200);
}

#[test]
fn renewing_pins_disconnects_everybody() {
    let r = start();
    let a = r.addr();
    let t = token(a, GUEST);
    let mut ws = ws_connect(a, &t);
    assert_eq!(next_json(&mut ws)["type"], "full");
    assert!(r.renew_pins());
    expect_close(&mut ws);
    assert_eq!(http(a, "GET", "/api/state", Some(&t), None).0, 401);
    assert_eq!(login(a, GUEST).0, 401, "alte PIN gilt nicht mehr");
    assert_eq!(login(a, &r.guest_pin()).0, 200);
    assert_eq!(r.guest_pin().len(), 6);
}

#[test]
fn connection_cap_and_oversize_requests() {
    let r = start_with(
        Limits {
            max_connections: 3,
            ..Limits::default()
        },
        false,
    );
    let a = r.addr();
    // Drei Verbindungen halten, die vierte wird abgewiesen.
    let held: Vec<TcpStream> = (0..3).map(|_| TcpStream::connect(a).unwrap()).collect();
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(http(a, "GET", "/", None, None).0, 503);
    drop(held);
    wait_for("Platz frei", || http(a, "GET", "/", None, None).0 == 200);
    // Zu großer Körper.
    let big = "x".repeat(10_000);
    assert_eq!(http(a, "POST", "/api/login", None, Some(&big)).0, 413);
    assert_eq!(http(a, "GET", "/nope", None, None).0, 404);
    assert_eq!(http(a, "DELETE", "/api/cmd", None, None).0, 405);
}

#[test]
fn search_needs_two_characters_and_finds_all_words() {
    let r = start();
    let a = r.addr();
    r.set_catalog(&[
        item("Abba - Waterloo", "/m/1.mp3"),
        item("Queen - Radio Ga Ga", "/m/2.mp3"),
        item("Queen - Under Pressure", "/m/3.mp3"),
    ]);
    let g = token(a, GUEST);
    let n = |q: &str| {
        let (_, j, _) = http(a, "GET", &format!("/api/search?q={q}"), Some(&g), None);
        j["results"].as_array().unwrap().len()
    };
    assert_eq!(n("q"), 0);
    assert_eq!(n("queen"), 2);
    assert_eq!(n("queen%20radio"), 1);
    assert_eq!(n("QUEEN+under"), 1);
    assert_eq!(n("zzz"), 0);
}

#[test]
fn osc_is_off_by_default_and_maps_paths_when_on() {
    let r = start();
    assert!(r.osc_addr().is_none());
    drop(r);

    let r = start_with(Limits::default(), true);
    let osc = r.osc_addr().expect("osc");
    let sock = UdpSocket::bind("127.0.0.1:0").unwrap();
    let send = |addr: &str, args: Vec<rosc::OscType>| {
        let p = rosc::OscPacket::Message(rosc::OscMessage {
            addr: addr.into(),
            args,
        });
        sock.send_to(&rosc::encoder::encode(&p).unwrap(), osc)
            .unwrap();
    };
    send("/woa/party/next", vec![rosc::OscType::Float(1.0)]);
    send("/woa/party/next", vec![rosc::OscType::Float(0.0)]);
    send("/woa/party/unbekannt", vec![]);
    send("/woa/party/volume_down", vec![]);
    let mut got = Vec::new();
    wait_for("zwei OSC-Befehle", || {
        got.extend(r.poll());
        got.len() >= 2
    });
    std::thread::sleep(Duration::from_millis(200));
    got.extend(r.poll());
    let cmds: Vec<_> = got.iter().map(|c| c.command.clone()).collect();
    assert_eq!(cmds, vec![PartyCommand::Next, PartyCommand::VolumeDown]);
    assert!(got.iter().all(|c| c.role == Role::Dj));
}

#[test]
fn lan_url_carries_the_guest_pin() {
    let r = start();
    let url = r.join_url(&r.guest_pin());
    assert!(url.starts_with("http://127.0.0.1:"), "{url}");
    assert!(url.ends_with(&format!("/?pin={GUEST}")), "{url}");
}
