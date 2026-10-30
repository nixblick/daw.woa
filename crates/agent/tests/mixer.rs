//! Paket 16: Mixer-Werkzeuge des Agenten (insert_effect, set_param, Sends, Ausgänge).

use rmcp::model::{CallToolRequestParams, CallToolResult, ClientConfig};
use rmcp::service::RunningService;
use rmcp::{ClientHandler, RoleClient, ServiceExt};
use woa_agent::WoaAgent;

#[derive(Clone, Default)]
struct TestClient;

impl ClientHandler for TestClient {
    fn get_info(&self) -> ClientConfig {
        ClientConfig::default()
    }
}

async fn call(
    client: &RunningService<RoleClient, TestClient>,
    name: &'static str,
    args: serde_json::Value,
) -> CallToolResult {
    let mut params = CallToolRequestParams::new(name);
    if let serde_json::Value::Object(map) = args {
        params = params.with_arguments(map);
    }
    client.call_tool(params).await.unwrap()
}

fn json(r: &CallToolResult) -> serde_json::Value {
    assert_ne!(r.is_error, Some(true), "tool error: {r:?}");
    match r.content[0].as_text() {
        Some(t) => serde_json::from_str(&t.text).unwrap(),
        None => panic!("unexpected content {r:?}"),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn agent_routes_and_inserts_effects() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("mix.woa");
    let agent = WoaAgent::open_or_create(&dir).unwrap();
    let session = agent.session();
    let (c_write, s_read) = tokio::io::duplex(1 << 16);
    let (s_write, c_read) = tokio::io::duplex(1 << 16);
    let server = tokio::spawn(woa_agent::serve(agent, (s_read, s_write)));
    let client = TestClient.serve((c_read, c_write)).await.unwrap();

    let t = json(&call(&client, "add_track", serde_json::json!({"name": "Stimme"})).await)["track"]
        .as_u64()
        .unwrap();
    let bus = json(
        &call(
            &client,
            "add_track",
            serde_json::json!({"name": "Hall", "kind": "bus"}),
        )
        .await,
    )["track"]
        .as_u64()
        .unwrap();

    let fx = json(
        &call(
            &client,
            "insert_effect",
            serde_json::json!({"track": t, "kind": "eq", "params": {"bell_db": 4.0}}),
        )
        .await,
    )["insert"]
        .as_u64()
        .unwrap();
    json(
        &call(
            &client,
            "set_param",
            serde_json::json!({"insert": fx, "param": "highpass_hz", "value": 80.0}),
        )
        .await,
    );
    {
        let s = session.lock().unwrap();
        let ins = &s.project().track(woa_model::TrackId(t)).unwrap().inserts[0];
        assert_eq!(ins.param("bell_db"), Some(4.0));
        assert_eq!(ins.param("highpass_hz"), Some(80.0));
    }

    // Wrong name and out-of-range value are tool errors and change nothing.
    let bad = call(
        &client,
        "set_param",
        serde_json::json!({"insert": fx, "param": "nope", "value": 1.0}),
    )
    .await;
    assert_eq!(bad.is_error, Some(true));
    let bad = call(
        &client,
        "set_param",
        serde_json::json!({"insert": fx, "param": "bell_db", "value": 99.0}),
    )
    .await;
    assert_eq!(bad.is_error, Some(true));

    json(
        &call(
            &client,
            "set_send",
            serde_json::json!({"track": t, "target": bus, "level_db": -8.0}),
        )
        .await,
    );
    // Second call changes the existing send.
    json(
        &call(
            &client,
            "set_send",
            serde_json::json!({"track": t, "target": bus, "level_db": -4.0, "pre_fader": true}),
        )
        .await,
    );
    {
        let s = session.lock().unwrap();
        let sends = &s.project().track(woa_model::TrackId(t)).unwrap().sends;
        assert_eq!(sends.len(), 1);
        assert_eq!(sends[0].level_db, -4.0);
        assert!(sends[0].pre_fader);
    }

    // A bus onto itself is rejected.
    let cyc = call(
        &client,
        "set_output",
        serde_json::json!({"track": bus, "output": bus}),
    )
    .await;
    assert_eq!(cyc.is_error, Some(true));

    // Removing the bus cuts the send in one undo step; undo brings both back.
    json(&call(&client, "remove_track", serde_json::json!({"track": bus})).await);
    assert!(
        session
            .lock()
            .unwrap()
            .project()
            .track(woa_model::TrackId(t))
            .unwrap()
            .sends
            .is_empty()
    );
    json(&call(&client, "undo", serde_json::json!({})).await);
    assert_eq!(
        session
            .lock()
            .unwrap()
            .project()
            .track(woa_model::TrackId(t))
            .unwrap()
            .sends
            .len(),
        1
    );

    drop(client);
    let _ = server.await;
}
