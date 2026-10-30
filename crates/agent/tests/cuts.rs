//! Paket 21: Agent-Werkzeuge split_clip, set_fade_curves und set_clip_gain.

use rmcp::model::{CallToolRequestParams, CallToolResult, ClientConfig};
use rmcp::service::RunningService;
use rmcp::{ClientHandler, RoleClient, ServiceExt};
use woa_agent::WoaAgent;
use woa_model::{ClipId, FadeCurve};

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
async fn agent_splits_shapes_fades_and_sets_gain() {
    let tmp = tempfile::tempdir().unwrap();
    let agent = WoaAgent::open_or_create(tmp.path().join("cuts.woa")).unwrap();
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

    // Teilen: rechter Teil beginnt am Schnitt und setzt das Audio dort fort.
    let right = json(
        &call(
            &client,
            "split_clip",
            serde_json::json!({"clip": clip, "at_beats": 3.0}),
        )
        .await,
    )["clip"]
        .as_u64()
        .unwrap();
    {
        let s = session.lock().unwrap();
        let l = s.project().clip(ClipId(clip)).unwrap().1.clone();
        let r = s.project().clip(ClipId(right)).unwrap().1.clone();
        assert_eq!((l.start_beats, l.length_beats), (0.0, 3.0));
        assert_eq!(
            (r.start_beats, r.length_beats, r.offset_beats),
            (3.0, 5.0, 3.0)
        );
    }
    // Außerhalb des Clips und an der Kante: Fehler, nichts ändert sich.
    for at in [0.0, 3.0, 20.0] {
        let r = call(
            &client,
            "split_clip",
            serde_json::json!({"clip": clip, "at_beats": at}),
        )
        .await;
        assert_eq!(r.is_error, Some(true), "at {at}");
    }
    assert_eq!(session.lock().unwrap().project().tracks[0].clips.len(), 2);

    // Kurven: nur eine angeben lässt die andere unverändert; falscher Name ist ein Fehler.
    json(
        &call(
            &client,
            "set_fade_curves",
            serde_json::json!({"clip": right, "fade_in": "s_curve"}),
        )
        .await,
    );
    json(
        &call(
            &client,
            "set_fade_curves",
            serde_json::json!({"clip": right, "fade_out": "equal_power"}),
        )
        .await,
    );
    let bad = call(
        &client,
        "set_fade_curves",
        serde_json::json!({"clip": right, "fade_in": "zickzack"}),
    )
    .await;
    assert_eq!(bad.is_error, Some(true));
    json(
        &call(
            &client,
            "set_clip_gain",
            serde_json::json!({"clip": right, "gain_db": -3.5}),
        )
        .await,
    );
    let too_loud = call(
        &client,
        "set_clip_gain",
        serde_json::json!({"clip": right, "gain_db": 40.0}),
    )
    .await;
    assert_eq!(too_loud.is_error, Some(true));
    {
        let s = session.lock().unwrap();
        let r = s.project().clip(ClipId(right)).unwrap().1.clone();
        assert_eq!(
            (r.fade_in_curve, r.fade_out_curve, r.gain_db),
            (FadeCurve::SCurve, FadeCurve::EqualPower, -3.5)
        );
    }
    drop(client);
    let _ = server.await;
}
