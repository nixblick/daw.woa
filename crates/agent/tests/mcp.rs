//! MCP client in the test talks to the server over an in-memory pipe, builds a
//! project and saves it.

use rmcp::model::{CallToolRequestParams, CallToolResult, ClientConfig};
use rmcp::service::RunningService;
use rmcp::{ClientHandler, RoleClient, ServiceExt};
use woa_agent::{AGENT_LABEL, WoaAgent};
use woa_model::{Session, store};

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
async fn agent_builds_and_saves_project() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("jingle.woa");
    let agent = WoaAgent::open_or_create(&dir).unwrap();
    let session = agent.session();

    let (c_write, s_read) = tokio::io::duplex(1 << 16);
    let (s_write, c_read) = tokio::io::duplex(1 << 16);
    let server = tokio::spawn(woa_agent::serve(agent, (s_read, s_write)));
    let client = TestClient.serve((c_read, c_write)).await.unwrap();

    let tools = client.list_all_tools().await.unwrap();
    let names: Vec<_> = tools.iter().map(|t| t.name.to_string()).collect();
    for n in [
        "project_info",
        "add_track",
        "rename_track",
        "set_gain",
        "set_pan",
        "set_mute",
        "set_tempo",
        "add_clip",
        "undo",
        "redo",
        "save",
    ] {
        assert!(names.iter().any(|x| x == n), "missing tool {n}");
    }
    let gain = tools.iter().find(|t| t.name == "set_gain").unwrap();
    let props = gain.input_schema.get("properties").unwrap();
    assert!(props.get("gain_db").is_some() && props.get("track").is_some());

    let t = json(&call(&client, "add_track", serde_json::json!({"name": "Stimme"})).await)["track"]
        .as_u64()
        .unwrap();
    json(
        &call(
            &client,
            "rename_track",
            serde_json::json!({"track": t, "name": "Moderation"}),
        )
        .await,
    );
    json(
        &call(
            &client,
            "set_gain",
            serde_json::json!({"track": t, "gain_db": -6.0}),
        )
        .await,
    );
    json(
        &call(
            &client,
            "set_pan",
            serde_json::json!({"track": t, "pan": 0.25}),
        )
        .await,
    );
    json(
        &call(
            &client,
            "set_mute",
            serde_json::json!({"track": t, "mute": true}),
        )
        .await,
    );
    json(&call(&client, "set_tempo", serde_json::json!({"bpm": 128.0})).await);
    json(
        &call(
            &client,
            "add_clip",
            serde_json::json!({"track": t, "name": "Intro", "start_beats": 0.0, "length_beats": 8.0}),
        )
        .await,
    );

    // Each tool call is exactly one undo step labelled "Agent".
    {
        let s = session.lock().unwrap();
        assert_eq!(s.history().undo_entries().len(), 7);
        assert!(
            s.history()
                .undo_entries()
                .iter()
                .all(|e| e.label == AGENT_LABEL)
        );
    }

    // Invalid value: tool error, nothing changes.
    let bad = call(
        &client,
        "set_gain",
        serde_json::json!({"track": t, "gain_db": 1.0e9}),
    )
    .await;
    assert_eq!(bad.is_error, Some(true));
    assert_eq!(session.lock().unwrap().history().undo_entries().len(), 7);

    // Undo clip and tempo, then redo the tempo.
    json(&call(&client, "undo", serde_json::json!({})).await);
    json(&call(&client, "undo", serde_json::json!({})).await);
    json(&call(&client, "redo", serde_json::json!({})).await);

    let info = json(&call(&client, "project_info", serde_json::json!({})).await);
    assert_eq!(info["dirty"], true);
    assert_eq!(info["project"]["tempo_bpm"], 128.0);

    let saved = json(&call(&client, "save", serde_json::json!({})).await);
    assert!(saved["saved"].as_str().unwrap().ends_with("jingle.woa"));

    client.cancel().await.unwrap();
    server.await.unwrap().unwrap();

    let p = store::load(&dir).unwrap();
    assert_eq!(p.tempo_bpm, 128.0);
    let track = &p.tracks[0];
    assert_eq!(track.name, "Moderation");
    assert_eq!(track.gain_db, -6.0);
    assert_eq!(track.pan, 0.25);
    assert!(track.mute);
    assert!(track.clips.is_empty(), "clip was undone");
    assert!(Session::open(&dir).is_ok());
}

#[tokio::test(flavor = "current_thread")]
async fn reopen_existing_project() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("a.woa");
    Session::new("Alt").save_as(&dir).unwrap();
    let agent = WoaAgent::open_or_create(&dir).unwrap();
    assert_eq!(agent.session().lock().unwrap().project().meta.name, "Alt");
}

/// Manche Clients (der StreamWriter von .NET) schreiben ein UTF-8-BOM vor die erste Zeile.
/// `initialize` darf dadurch nicht verloren gehen (#243).
#[tokio::test(flavor = "current_thread")]
async fn bom_before_initialize_is_ignored() {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    let tmp = tempfile::tempdir().unwrap();
    let agent = WoaAgent::open_or_create(tmp.path().join("bom.woa")).unwrap();
    let (mut c_write, s_read) = tokio::io::duplex(1 << 16);
    let (s_write, c_read) = tokio::io::duplex(1 << 16);
    let _server = tokio::spawn(woa_agent::serve(agent, (s_read, s_write)));
    let init = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"t","version":"1"}}}"#;
    c_write.write_all(b"\xEF\xBB\xBF").await.unwrap();
    c_write.write_all(init.as_bytes()).await.unwrap();
    c_write.write_all(b"\n").await.unwrap();
    let mut line = String::new();
    let read = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        BufReader::new(c_read).read_line(&mut line),
    )
    .await;
    assert!(read.is_ok(), "keine Antwort auf initialize mit BOM");
    assert!(
        line.contains("\"id\":1") && line.contains("protocolVersion"),
        "{line}"
    );
}
