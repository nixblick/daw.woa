//! MCP über HTTP: ohne gültiges Token kommt nichts durch, mit Token geht `initialize`.

use std::net::SocketAddr;

use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;
use woa_agent::WoaAgent;
use woa_agent::http::{HttpOptions, serve_http};

const INIT: &str = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"test","version":"1"}}}"#;

struct Reply {
    status: u16,
    body: String,
}

fn post(addr: SocketAddr, auth: Option<&str>, origin: Option<&str>, path: &str) -> Reply {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .build()
        .into();
    let mut req = agent
        .post(format!("http://{addr}{path}"))
        .header("Content-Type", "application/json")
        .header("Accept", "application/json, text/event-stream");
    if let Some(a) = auth {
        req = req.header("Authorization", a);
    }
    if let Some(o) = origin {
        req = req.header("Origin", o);
    }
    let mut resp = req.send(INIT).expect("request");
    Reply {
        status: resp.status().as_u16(),
        body: resp.body_mut().read_to_string().unwrap_or_default(),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn token_is_required_and_initialize_works() {
    let tmp = tempfile::tempdir().unwrap();
    let agent = WoaAgent::open_or_create(tmp.path().join("p.woa")).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let cancel = CancellationToken::new();
    let server = tokio::spawn(serve_http(
        listener,
        agent,
        HttpOptions {
            token: "geheim-token-123".into(),
            reachable_from_network: false,
            live_secret: None,
            cancel: cancel.clone(),
        },
    ));

    let r = tokio::task::spawn_blocking(move || {
        let none = post(addr, None, None, "/mcp");
        let wrong = post(addr, Some("Bearer falsch"), None, "/mcp");
        let wrong_scheme = post(addr, Some("geheim-token-123"), None, "/mcp");
        let other_path = post(addr, Some("Bearer geheim-token-123"), None, "/anders");
        let evil_origin = post(
            addr,
            Some("Bearer geheim-token-123"),
            Some("https://boese.example"),
            "/mcp",
        );
        let ok = post(addr, Some("Bearer geheim-token-123"), None, "/mcp");
        let local_origin = post(
            addr,
            Some("Bearer geheim-token-123"),
            Some("http://localhost:3000"),
            "/mcp",
        );
        (
            none,
            wrong,
            wrong_scheme,
            other_path,
            evil_origin,
            ok,
            local_origin,
        )
    })
    .await
    .unwrap();
    let (none, wrong, wrong_scheme, other_path, evil_origin, ok, local_origin) = r;

    assert_eq!(none.status, 401);
    assert_eq!(wrong.status, 401);
    assert_eq!(wrong_scheme.status, 401);
    assert!(!none.body.contains("geheim"), "{}", none.body);
    assert_eq!(other_path.status, 404);
    assert_eq!(evil_origin.status, 403, "{}", evil_origin.body);
    assert_eq!(ok.status, 200, "{}", ok.body);
    assert!(ok.body.contains("\"woa\""), "{}", ok.body);
    assert_eq!(local_origin.status, 200, "{}", local_origin.body);

    cancel.cancel();
    let _ = server.await;
}

#[tokio::test(flavor = "current_thread")]
async fn empty_token_never_starts() {
    let tmp = tempfile::tempdir().unwrap();
    let agent = WoaAgent::open_or_create(tmp.path().join("p.woa")).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let err = serve_http(
        listener,
        agent,
        HttpOptions {
            token: String::new(),
            reachable_from_network: false,
            live_secret: None,
            cancel: CancellationToken::new(),
        },
    )
    .await;
    assert!(err.is_err());
}
