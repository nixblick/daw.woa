//! Der ganze Weg ohne Fenster: Handy wünscht, Party-Schicht nimmt den Befehl, die Wunschliste
//! wächst, das Handy sieht sie; der DJ reiht sie ein und die Party-Schicht liefert den Effekt.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

use serde_json::{Value, json};
use tungstenite::Message;
use woa_radio::party::{Effect, PartyState, layout};
use woa_radio::station::{View, ViewEntry};
use woa_remote::bridge::{catalog, snapshot};
use woa_remote::{Remote, RemoteConfig};

fn entry(title: &str, path: &str) -> ViewEntry {
    ViewEntry {
        title: title.into(),
        path: path.into(),
        seconds: Some(200.0),
        lufs: None,
        gain_db: 0.0,
        cues: Default::default(),
        voice: None,
    }
}

fn post(addr: SocketAddr, path: &str, token: Option<&str>, body: &Value) -> Value {
    let mut s = TcpStream::connect(addr).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let b = body.to_string();
    let auth = token
        .map(|t| format!("Authorization: Bearer {t}\r\n"))
        .unwrap_or_default();
    write!(
        s,
        "POST {path} HTTP/1.1\r\nHost: x\r\n{auth}Content-Length: {}\r\nConnection: close\r\n\r\n{b}",
        b.len()
    )
    .unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).ok();
    serde_json::from_str(out.split_once("\r\n\r\n").map_or("", |x| x.1)).unwrap_or(Value::Null)
}

#[test]
fn wish_from_the_phone_reaches_the_list_and_back() {
    let view = View {
        playlist: vec![
            entry("Abba - Waterloo", "/musik/abba.mp3"),
            entry("Queen - Radio Ga Ga", "/musik/queen.mp3"),
        ],
        current: Some(0),
        running: true,
        remaining_title: 100.0,
        ..View::default()
    };
    let mut state = PartyState::default();
    let remote = Remote::start(RemoteConfig {
        pins: Some(("111111".into(), "222222".into())),
        ..RemoteConfig::default()
    })
    .expect("start");
    let a = remote.addr();
    remote.set_catalog(&catalog(&view));
    remote.publish(&snapshot(&layout(&view, &state), &state));

    // DJ-Handy schaut zu.
    let dj = post(a, "/api/login", None, &json!({"pin": "222222"}));
    let dj_token = dj["token"].as_str().unwrap().to_string();
    let s = TcpStream::connect(a).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let (mut ws, _) = tungstenite::client(format!("ws://{a}/ws?token={dj_token}"), s).unwrap();
    let Message::Text(t) = ws.read().unwrap() else {
        panic!("kein Text")
    };
    let full: Value = serde_json::from_str(t.as_str()).unwrap();
    assert_eq!(full["state"]["now"]["title"], "Waterloo");
    assert_eq!(full["state"]["wishes"], json!([]));

    // Gast wünscht „Radio Ga Ga“ über die Suche.
    let g = post(a, "/api/login", None, &json!({"pin": "111111"}));
    let gt = g["token"].as_str().unwrap().to_string();
    let id = remote_search_id(a, &gt, "radio");
    assert_eq!(
        post(a, "/api/cmd", Some(&gt), &json!({"cmd": "wish", "id": id}))["ok"],
        true
    );

    // Die Oberfläche holt den Befehl ab, führt ihn aus und veröffentlicht den neuen Stand.
    let cmds = remote.poll();
    assert_eq!(cmds.len(), 1);
    for c in cmds {
        state.apply(c.command, true);
    }
    assert_eq!(state.wishes.len(), 1);
    assert_eq!(state.log(), ["wish Queen - Radio Ga Ga"]);
    remote.publish(&snapshot(&layout(&view, &state), &state));

    // Das DJ-Handy sieht den Wunsch als Änderung.
    let Message::Text(t) = ws.read().unwrap() else {
        panic!("kein Text")
    };
    let d: Value = serde_json::from_str(t.as_str()).unwrap();
    assert_eq!(d["type"], "delta");
    let wish = &d["changes"]["wishes"][0];
    assert_eq!(wish["title"], "Queen - Radio Ga Ga");
    let wid = wish["id"].as_str().unwrap().to_string();

    // DJ reiht ihn als Nächstes ein: die Party-Schicht liefert den Effekt mit dem echten Pfad.
    ws.send(Message::text(
        json!({"cmd": "play_wish", "id": wid}).to_string(),
    ))
    .unwrap();
    let Message::Text(ack) = ws.read().unwrap() else {
        panic!("kein Text")
    };
    assert!(ack.as_str().contains("ack"), "{ack}");
    let cmds = remote.poll();
    let effects = state.apply(cmds[0].command.clone(), true);
    assert_eq!(effects, vec![Effect::InsertNext("/musik/queen.mp3".into())]);
    assert!(state.wishes.is_empty());
}

fn remote_search_id(a: SocketAddr, token: &str, q: &str) -> String {
    let mut s = TcpStream::connect(a).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    write!(
        s,
        "GET /api/search?q={q} HTTP/1.1\r\nHost: x\r\nAuthorization: Bearer {token}\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).ok();
    let j: Value = serde_json::from_str(out.split_once("\r\n\r\n").unwrap().1).unwrap();
    j["results"][0]["id"].as_str().unwrap().to_string()
}

#[test]
fn transport_buttons_from_the_phone_reach_the_party_layer() {
    use woa_radio::party::PartyCommand;
    let remote = Remote::start(RemoteConfig {
        pins: Some(("111111".into(), "222222".into())),
        limits: woa_remote::Limits {
            commands_per_second: 100,
            ..woa_remote::Limits::default()
        },
        ..RemoteConfig::default()
    })
    .expect("start");
    let a = remote.addr();
    let dj = post(a, "/api/login", None, &json!({"pin": "222222"}));
    let dj_token = dj["token"].as_str().unwrap().to_string();
    let g = post(a, "/api/login", None, &json!({"pin": "111111"}));
    let guest_token = g["token"].as_str().unwrap().to_string();

    let all = [
        ("previous", PartyCommand::Previous),
        ("play_pause", PartyCommand::PlayPause),
        ("play", PartyCommand::Play),
        ("stop", PartyCommand::Stop),
        ("next", PartyCommand::Next),
        ("restart", PartyCommand::Restart),
        ("seek_back", PartyCommand::SeekBack),
        ("seek_forward", PartyCommand::SeekForward),
        ("mute", PartyCommand::Mute),
        ("repeat", PartyCommand::CycleRepeat),
        ("shuffle", PartyCommand::ToggleShuffle),
        ("volume_down", PartyCommand::VolumeDown),
        ("volume_up", PartyCommand::VolumeUp),
    ];
    for (name, expected) in all {
        // Der Gast darf keinen davon.
        let r = post(a, "/api/cmd", Some(&guest_token), &json!({"cmd": name}));
        assert_ne!(r["ok"], true, "{name}: Gast darf nicht");
        assert!(
            remote.poll().is_empty(),
            "{name}: nichts bei der Oberfläche"
        );
        // Der DJ darf alle, und der Befehl kommt unverändert an.
        let r = post(a, "/api/cmd", Some(&dj_token), &json!({"cmd": name}));
        assert_eq!(r["ok"], true, "{name}: {r}");
        let got = remote.poll();
        assert_eq!(got.len(), 1, "{name}");
        assert_eq!(got[0].command, expected, "{name}");
    }
    // Unbekanntes und kaputtes wird abgelehnt.
    for body in [
        json!({"cmd": "format_c"}),
        json!({"cmd": "seek_forward_x"}),
        json!({}),
    ] {
        let r = post(a, "/api/cmd", Some(&dj_token), &body);
        assert_ne!(r["ok"], true);
    }
    assert!(remote.poll().is_empty());
}
