//! Paket 17: Effekt-Werkzeuge des Agenten (list_effects, Voreinstellungen, Vorlage Sprachkette).

use rmcp::model::{CallToolRequestParams, CallToolResult, ClientConfig};
use rmcp::service::RunningService;
use rmcp::{ClientHandler, RoleClient, ServiceExt};
use woa_agent::WoaAgent;
use woa_model::{EffectKind, TrackId};

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
async fn agent_lists_effects_uses_presets_and_the_voice_chain() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("fx.woa");
    let agent = WoaAgent::open_or_create(&dir).unwrap();
    let session = agent.session();
    let (c_write, s_read) = tokio::io::duplex(1 << 16);
    let (s_write, c_read) = tokio::io::duplex(1 << 16);
    let server = tokio::spawn(woa_agent::serve(agent, (s_read, s_write)));
    let client = TestClient.serve((c_read, c_write)).await.unwrap();

    // Alle zehn Arten mit Reglern und Voreinstellungen.
    let all = json(&call(&client, "list_effects", serde_json::json!({})).await);
    let effects = all["effects"].as_array().unwrap();
    assert_eq!(effects.len(), EffectKind::ALL.len());
    for e in effects {
        assert!(!e["params"].as_array().unwrap().is_empty());
        assert!(!e["presets"].as_array().unwrap().is_empty());
    }
    let one = json(
        &call(
            &client,
            "list_effects",
            serde_json::json!({"kind": "reverb"}),
        )
        .await,
    );
    assert_eq!(one["effects"].as_array().unwrap().len(), 1);
    assert_eq!(one["effects"][0]["kind"], "reverb");

    let t = json(&call(&client, "add_track", serde_json::json!({"name": "Stimme"})).await)["track"]
        .as_u64()
        .unwrap();

    // Eine Voreinstellung, darüber ein einzelner Wert.
    let preset = EffectKind::Reverb.presets()[0].name;
    let rv = json(
        &call(
            &client,
            "insert_effect",
            serde_json::json!({"track": t, "kind": "reverb", "preset": preset,
                               "params": {"mix_pct": 12.0}}),
        )
        .await,
    )["insert"]
        .as_u64()
        .unwrap();
    {
        let s = session.lock().unwrap();
        let ins = s
            .project()
            .track(TrackId(t))
            .unwrap()
            .inserts
            .iter()
            .find(|i| i.id.0 == rv)
            .unwrap()
            .clone();
        let expected = EffectKind::Reverb.preset_values(preset).unwrap();
        for (i, spec) in EffectKind::Reverb.params().iter().enumerate() {
            if spec.name == "mix_pct" {
                assert_eq!(ins.params[i], 12.0);
            } else {
                assert_eq!(ins.params[i], expected[i], "{}", spec.name);
            }
        }
    }
    // Unbekannte Voreinstellung und Wert außerhalb: Fehler, nichts ändert sich.
    let bad = call(
        &client,
        "insert_effect",
        serde_json::json!({"track": t, "kind": "gate", "preset": "gibt es nicht"}),
    )
    .await;
    assert_eq!(bad.is_error, Some(true));
    assert_eq!(
        session
            .lock()
            .unwrap()
            .project()
            .track(TrackId(t))
            .unwrap()
            .inserts
            .len(),
        1
    );

    // Neue Arten lassen sich einfügen und einstellen (Auswahl-Regler nur ganzzahlig).
    let dl = json(
        &call(
            &client,
            "insert_effect",
            serde_json::json!({"track": t, "kind": "delay", "params": {"sync": 2.0}}),
        )
        .await,
    )["insert"]
        .as_u64()
        .unwrap();
    let bad = call(
        &client,
        "set_param",
        serde_json::json!({"insert": dl, "param": "sync", "value": 2.5}),
    )
    .await;
    assert_eq!(bad.is_error, Some(true));
    json(
        &call(
            &client,
            "set_param",
            serde_json::json!({"insert": dl, "param": "feedback_pct", "value": 40.0}),
        )
        .await,
    );

    // Sprachkette: fünf Effekte in fester Reihenfolge, ein Undo-Schritt.
    let bus = json(
        &call(
            &client,
            "add_track",
            serde_json::json!({"name": "Sprecher"}),
        )
        .await,
    )["track"]
        .as_u64()
        .unwrap();
    let chain = json(
        &call(
            &client,
            "insert_voice_chain",
            serde_json::json!({"track": bus}),
        )
        .await,
    );
    assert_eq!(chain["inserts"].as_array().unwrap().len(), 5);
    let kinds: Vec<EffectKind> = session
        .lock()
        .unwrap()
        .project()
        .track(TrackId(bus))
        .unwrap()
        .inserts
        .iter()
        .map(|i| i.kind)
        .collect();
    assert_eq!(
        kinds,
        [
            EffectKind::Eq,
            EffectKind::Gate,
            EffectKind::Eq,
            EffectKind::Compressor,
            EffectKind::DeEsser
        ]
    );
    json(&call(&client, "undo", serde_json::json!({})).await);
    assert!(
        session
            .lock()
            .unwrap()
            .project()
            .track(TrackId(bus))
            .unwrap()
            .inserts
            .is_empty(),
        "the template is one undo step"
    );
    let bad = call(
        &client,
        "insert_voice_chain",
        serde_json::json!({"track": 9999}),
    )
    .await;
    assert_eq!(bad.is_error, Some(true));

    drop(client);
    let _ = server.await;
}
