//! Paket 18: Agent-Werkzeuge add_automation, remove_automation und set_sidechain.

use rmcp::model::{CallToolRequestParams, CallToolResult, ClientConfig};
use rmcp::service::RunningService;
use rmcp::{ClientHandler, RoleClient, ServiceExt};
use woa_agent::WoaAgent;
use woa_model::{AutoTarget, TrackId};

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
async fn agent_automates_and_ducks() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("auto.woa");
    let agent = WoaAgent::open_or_create(&dir).unwrap();
    let session = agent.session();
    let (c_write, s_read) = tokio::io::duplex(1 << 16);
    let (s_write, c_read) = tokio::io::duplex(1 << 16);
    let server = tokio::spawn(woa_agent::serve(agent, (s_read, s_write)));
    let client = TestClient.serve((c_read, c_write)).await.unwrap();

    let track = |name: &'static str| {
        let client = &client;
        async move {
            json(&call(client, "add_track", serde_json::json!({ "name": name })).await)["track"]
                .as_u64()
                .unwrap()
        }
    };
    let music = track("Musik").await;
    let voice = track("Sprache").await;

    // Lautstärke-Automation mit drei Punkten, unsortiert angegeben.
    let r = json(
        &call(
            &client,
            "add_automation",
            serde_json::json!({"track": music, "target": "gain", "points": [
                {"beat": 8.0, "value": 0.0}, {"beat": 0.0, "value": 0.0}, {"beat": 4.0, "value": -18.0}
            ]}),
        )
        .await,
    );
    assert_eq!(r["points"], 3);
    {
        let s = session.lock().unwrap();
        let lane = s
            .project()
            .track(TrackId(music))
            .unwrap()
            .lane(&AutoTarget::Gain)
            .unwrap()
            .clone();
        assert_eq!(lane.points[1].beat, 4.0);
        assert_eq!(lane.value_at(2.0, false), Some(-9.0));
    }

    // Regler-Automation per Name.
    let comp = json(
        &call(
            &client,
            "insert_effect",
            serde_json::json!({"track": music, "kind": "compressor"}),
        )
        .await,
    )["insert"]
        .as_u64()
        .unwrap();
    json(
        &call(
            &client,
            "add_automation",
            serde_json::json!({"track": music, "target": "param", "insert": comp,
                "param": "threshold_db", "points": [{"beat": 0.0, "value": -10.0}]}),
        )
        .await,
    );

    // Falsche Eingaben ändern nichts.
    for args in [
        serde_json::json!({"track": music, "target": "gain", "points": [{"beat": 0.0, "value": 99.0}]}),
        serde_json::json!({"track": music, "target": "param", "insert": comp, "points": []}),
        serde_json::json!({"track": music, "target": "param", "insert": comp, "param": "nope", "points": []}),
        serde_json::json!({"track": voice, "target": "param", "insert": comp, "param": "ratio", "points": []}),
        serde_json::json!({"track": music, "target": "send", "points": []}),
        serde_json::json!({"track": music, "target": "pan", "points": [{"beat": -1.0, "value": 0.0}]}),
    ] {
        let bad = call(&client, "add_automation", args).await;
        assert_eq!(bad.is_error, Some(true), "{bad:?}");
    }
    assert_eq!(
        session
            .lock()
            .unwrap()
            .project()
            .track(TrackId(music))
            .unwrap()
            .automation
            .len(),
        2
    );

    // Sidechain: Sprache steuert den Kompressor der Musik; Schleife wird abgelehnt.
    json(
        &call(
            &client,
            "set_sidechain",
            serde_json::json!({"insert": comp, "source": voice}),
        )
        .await,
    );
    let voice_comp = json(
        &call(
            &client,
            "insert_effect",
            serde_json::json!({"track": voice, "kind": "gate"}),
        )
        .await,
    )["insert"]
        .as_u64()
        .unwrap();
    let cyc = call(
        &client,
        "set_sidechain",
        serde_json::json!({"insert": voice_comp, "source": music}),
    )
    .await;
    assert_eq!(cyc.is_error, Some(true));
    let eq = json(
        &call(
            &client,
            "insert_effect",
            serde_json::json!({"track": music, "kind": "eq"}),
        )
        .await,
    )["insert"]
        .as_u64()
        .unwrap();
    let no_input = call(
        &client,
        "set_sidechain",
        serde_json::json!({"insert": eq, "source": voice}),
    )
    .await;
    assert_eq!(no_input.is_error, Some(true));

    // Entfernen und Rückgängig.
    json(
        &call(
            &client,
            "remove_automation",
            serde_json::json!({"track": music, "target": "gain"}),
        )
        .await,
    );
    assert_eq!(
        session
            .lock()
            .unwrap()
            .project()
            .track(TrackId(music))
            .unwrap()
            .automation
            .len(),
        1
    );
    json(&call(&client, "undo", serde_json::json!({})).await);
    assert_eq!(
        session
            .lock()
            .unwrap()
            .project()
            .track(TrackId(music))
            .unwrap()
            .automation
            .len(),
        2
    );

    drop(client);
    let _ = server.await;
}
