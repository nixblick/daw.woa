//! Paket 23: Agent-Werkzeuge stretch_clip, set_clip_pitch und set_clip_follow_tempo.

use rmcp::model::{CallToolRequestParams, CallToolResult, ClientConfig};
use rmcp::service::RunningService;
use rmcp::{ClientHandler, RoleClient, ServiceExt};
use woa_agent::WoaAgent;
use woa_model::ClipId;

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
async fn agent_stretches_shifts_pitch_and_follows_tempo() {
    let tmp = tempfile::tempdir().unwrap();
    let agent = WoaAgent::open_or_create(tmp.path().join("zeit.woa")).unwrap();
    let session = agent.session();
    let (c_write, s_read) = tokio::io::duplex(1 << 16);
    let (s_write, c_read) = tokio::io::duplex(1 << 16);
    let server = tokio::spawn(woa_agent::serve(agent, (s_read, s_write)));
    let client = TestClient.serve((c_read, c_write)).await.unwrap();

    let track = json(&call(&client, "add_track", serde_json::json!({"name": "A"})).await)["track"]
        .as_u64()
        .unwrap();
    let clip = json(
        &call(
            &client,
            "add_clip",
            serde_json::json!({"track": track, "name": "c", "start_beats": 0.0, "length_beats": 8.0}),
        )
        .await,
    )["clip"]
        .as_u64()
        .unwrap();

    json(
        &call(
            &client,
            "stretch_clip",
            serde_json::json!({"clip": clip, "length_beats": 8.8}),
        )
        .await,
    );
    json(
        &call(
            &client,
            "set_clip_pitch",
            serde_json::json!({"clip": clip, "semitones": 2.0}),
        )
        .await,
    );
    json(
        &call(
            &client,
            "set_clip_follow_tempo",
            serde_json::json!({"clip": clip, "follow": true}),
        )
        .await,
    );
    {
        let s = session.lock().unwrap();
        let c = s.project().clip(ClipId(clip)).unwrap().1.clone();
        assert!((c.length_beats - 8.8).abs() < 1e-9 && (c.stretch - 1.1).abs() < 1e-9);
        assert_eq!(c.pitch_semitones, 2.0);
        assert_eq!(c.follow_tempo_bpm, Some(s.project().tempo_bpm));
    }
    // Ungültiges wird abgelehnt und ändert nichts.
    for (tool, args) in [
        (
            "stretch_clip",
            serde_json::json!({"clip": clip, "length_beats": 100.0}),
        ),
        (
            "stretch_clip",
            serde_json::json!({"clip": clip, "length_beats": -1.0}),
        ),
        (
            "set_clip_pitch",
            serde_json::json!({"clip": clip, "semitones": 40.0}),
        ),
        (
            "set_clip_pitch",
            serde_json::json!({"clip": 9999, "semitones": 1.0}),
        ),
    ] {
        assert_eq!(
            call(&client, tool, args).await.is_error,
            Some(true),
            "{tool}"
        );
    }
    {
        let s = session.lock().unwrap();
        let c = s.project().clip(ClipId(clip)).unwrap().1.clone();
        assert_eq!((c.stretch, c.pitch_semitones), (1.1, 2.0));
    }
    drop(client);
    let _ = server.await;
}
