//! M2 acceptance: an agent builds an 8-bar loop only through MCP tools, exports it at
//! -14 LUFS, and the file is measured again. Plus error cases of the new tools.

use rmcp::model::{CallToolRequestParams, CallToolResult, ClientConfig};
use rmcp::service::RunningService;
use rmcp::{ClientHandler, RoleClient, ServiceExt};
use serde_json::json;
use woa_agent::{AGENT_LABEL, WoaAgent};
use woa_engine::loudness::integrated_lufs;

#[derive(Clone, Default)]
struct TestClient;

impl ClientHandler for TestClient {
    fn get_info(&self) -> ClientConfig {
        ClientConfig::default()
    }
}

type Client = RunningService<RoleClient, TestClient>;

async fn call(client: &Client, name: &'static str, args: serde_json::Value) -> CallToolResult {
    let mut params = CallToolRequestParams::new(name);
    if let serde_json::Value::Object(map) = args {
        params = params.with_arguments(map);
    }
    client.call_tool(params).await.unwrap()
}

fn ok(r: &CallToolResult) -> serde_json::Value {
    assert_ne!(r.is_error, Some(true), "tool error: {r:?}");
    serde_json::from_str(&r.content[0].as_text().unwrap().text).unwrap()
}

async fn start(dir: &std::path::Path) -> (Client, WoaAgent, tokio::task::JoinHandle<()>) {
    let agent = WoaAgent::open_or_create(dir).unwrap();
    let (c_write, s_read) = tokio::io::duplex(1 << 16);
    let (s_write, c_read) = tokio::io::duplex(1 << 16);
    let server = tokio::spawn({
        let agent = agent.clone();
        async move { woa_agent::serve(agent, (s_read, s_write)).await.unwrap() }
    });
    let client = TestClient.serve((c_read, c_write)).await.unwrap();
    (client, agent, server)
}

async fn track(client: &Client, name: &str) -> u64 {
    ok(&call(client, "add_track", json!({ "name": name })).await)["track"]
        .as_u64()
        .unwrap()
}

/// Two-bar chord progression Am F C G, one chord per half bar.
fn chords() -> Vec<serde_json::Value> {
    let prog: [[u8; 3]; 4] = [[57, 60, 64], [53, 57, 60], [48, 52, 55], [55, 59, 62]];
    let mut notes = Vec::new();
    for (i, chord) in prog.iter().enumerate() {
        for &p in chord {
            notes.push(
                json!({"beat": i as f64 * 2.0, "pitch": p, "length_beats": 1.75, "velocity": 80}),
            );
        }
    }
    notes
}

fn bass() -> Vec<serde_json::Value> {
    let roots = [33u8, 29, 36, 31];
    let mut notes = Vec::new();
    for (i, r) in roots.iter().enumerate() {
        for k in 0..4 {
            notes.push(
                json!({"beat": i as f64 * 2.0 + k as f64 * 0.5, "pitch": r, "length_beats": 0.4}),
            );
        }
    }
    notes
}

#[tokio::test(flavor = "current_thread")]
async fn agent_builds_8_bar_loop_at_minus_14_lufs() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("loop.woa");
    let (client, agent, server) = start(&dir).await;

    ok(&call(&client, "set_tempo", json!({"bpm": 120.0})).await);
    let drums = track(&client, "Drums").await;
    let bass_t = track(&client, "Bass").await;
    let keys = track(&client, "Akkorde").await;

    // One bar of drums, repeated 7 times = 8 bars.
    let d = ok(&call(
        &client,
        "generate_drums",
        json!({
            "track": drums,
            "kick":  "x.......x.x.....",
            "snare": "....x.......x...",
            "hihat": "x.o.x.o.x.o.x.o.",
        }),
    )
    .await);
    assert_eq!(d["length_beats"], 4.0);
    let rep = ok(&call(
        &client,
        "repeat_clip",
        json!({"clip": d["clip"], "times": 7}),
    )
    .await);
    assert_eq!(rep["clips"].as_array().unwrap().len(), 7);

    // Two bars of bass and chords, repeated 3 times = 8 bars.
    let b = ok(&call(
        &client,
        "generate_notes",
        json!({"track": bass_t, "name": "Bass", "length_beats": 8.0, "waveform": "square", "notes": bass()}),
    )
    .await);
    ok(&call(
        &client,
        "repeat_clip",
        json!({"clip": b["clip"], "times": 3}),
    )
    .await);
    let k = ok(&call(
        &client,
        "generate_notes",
        json!({"track": keys, "name": "Akkorde", "length_beats": 8.0, "waveform": "saw", "notes": chords()}),
    )
    .await);
    ok(&call(
        &client,
        "repeat_clip",
        json!({"clip": k["clip"], "times": 3}),
    )
    .await);
    ok(&call(&client, "set_gain", json!({"track": keys, "gain_db": -6.0})).await);
    ok(&call(
        &client,
        "set_gain",
        json!({"track": drums, "gain_db": -5.0}),
    )
    .await);

    let a = ok(&call(&client, "analyze", json!({})).await);
    assert!((a["seconds"].as_f64().unwrap() - 16.0).abs() < 1e-6, "{a}");
    assert!(a["lufs"].as_f64().is_some(), "{a}");
    assert_eq!(a["tracks"].as_array().unwrap().len(), 3);
    for t in a["tracks"].as_array().unwrap() {
        assert!(t["peak_dbfs"].as_f64().is_some(), "silent track {t}");
    }

    let e = ok(&call(
        &client,
        "export",
        json!({"path": "export/loop.wav", "target_lufs": -14.0}),
    )
    .await);
    assert_eq!(e["normalized"]["peak_limited"], false, "{e}");
    assert_eq!(e["target_reached"], true, "{e}");
    ok(&call(&client, "save", json!({})).await);

    // Every edit is one undo step labelled "Agent"; analyze/export add none.
    {
        let s = agent.session();
        let s = s.lock().unwrap();
        let entries = s.history().undo_entries();
        assert_eq!(entries.len(), 12);
        assert!(entries.iter().all(|e| e.label == AGENT_LABEL));
    }

    client.cancel().await.unwrap();
    server.await.unwrap();

    // Measure the exported file independently.
    let file = dir.join("export/loop.wav");
    let (l, r, rate) = woa_engine::read_wav_stereo(&file).unwrap();
    assert_eq!(rate, 48_000);
    let seconds = l.len() as f64 / f64::from(rate);
    assert!((seconds - 16.0).abs() < 0.01, "{seconds} s");
    let lufs = integrated_lufs(&l, &r, rate).unwrap();
    assert!((lufs + 14.0).abs() <= 0.5, "{lufs} LUFS");
    let peak = l.iter().chain(&r).fold(0.0f32, |m, s| m.max(s.abs()));
    assert!(peak < 1.0, "clipping, peak {peak}");
    println!(
        "loop: {lufs:.2} LUFS, {seconds:.2} s, peak {:.2} dBFS",
        20.0 * peak.log10()
    );

    // The project reopens with the generated audio inside its folder.
    let p = woa_model::store::load(&dir).unwrap();
    assert_eq!(p.tracks.len(), 3);
    for c in p.tracks.iter().flat_map(|t| &t.clips) {
        let rel = c.audio.as_deref().unwrap();
        assert!(rel.starts_with("aufnahmen/"));
        assert!(dir.join(rel).exists());
    }
}

#[tokio::test(flavor = "current_thread")]
async fn editing_tools_and_error_cases() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("edit.woa");
    let (client, agent, server) = start(&dir).await;
    let session = agent.session();
    let a = track(&client, "A").await;
    let midi = ok(&call(&client, "add_track", json!({"name": "M", "kind": "midi"})).await)["track"]
        .as_u64()
        .unwrap();
    let c = ok(&call(
        &client,
        "generate_notes",
        json!({"track": a, "length_beats": 4.0, "waveform": "sine",
               "notes": [{"beat": 0.0, "pitch": 69, "length_beats": 1.0}]}),
    )
    .await)["clip"]
        .as_u64()
        .unwrap();

    let before = session.lock().unwrap().project().clone();
    let steps = session.lock().unwrap().history().undo_entries().len();
    let src = dir.join("aufnahmen");
    let files = std::fs::read_dir(&src).unwrap().count();

    // Every bad call is a tool error and changes nothing.
    for (tool, args) in [
        (
            "generate_notes",
            json!({"track": 999, "length_beats": 4.0, "notes": [{"beat": 0.0, "pitch": 60, "length_beats": 1.0}]}),
        ),
        (
            "generate_notes",
            json!({"track": midi, "length_beats": 4.0, "notes": [{"beat": 0.0, "pitch": 60, "length_beats": 1.0}]}),
        ),
        (
            "generate_notes",
            json!({"track": a, "length_beats": 4.0, "notes": []}),
        ),
        (
            "generate_notes",
            json!({"track": a, "length_beats": 4.0, "notes": [{"beat": 5.0, "pitch": 60, "length_beats": 1.0}]}),
        ),
        (
            "generate_notes",
            json!({"track": a, "length_beats": -1.0, "notes": [{"beat": 0.0, "pitch": 60, "length_beats": 1.0}]}),
        ),
        ("generate_drums", json!({"track": a, "kick": "x..y"})),
        ("generate_drums", json!({"track": a})),
        (
            "generate_drums",
            json!({"track": a, "kick": "x...", "start_beats": -2.0}),
        ),
        (
            "import_audio",
            json!({"track": a, "path": tmp.path().join("fehlt.wav").display().to_string()}),
        ),
        ("move_clip", json!({"clip": 999, "start_beats": 1.0})),
        ("move_clip", json!({"clip": c, "start_beats": -1.0})),
        ("trim_clip", json!({"clip": c, "length_beats": 0.0})),
        (
            "set_fades",
            json!({"clip": c, "fade_in_beats": 3.0, "fade_out_beats": 3.0}),
        ),
        ("repeat_clip", json!({"clip": c, "times": 0})),
        ("repeat_clip", json!({"clip": 999, "times": 1})),
        ("remove_clip", json!({"clip": 999})),
        ("remove_track", json!({"track": 999})),
        ("set_solo", json!({"track": 999, "solo": true})),
        ("export", json!({"path": "../x.wav"})),
        ("export", json!({"path": "/tmp/x.wav"})),
        ("export", json!({"path": "sub/../../x.wav"})),
        ("export", json!({"path": "x.xyz"})),
        ("export", json!({"path": "x.wav", "target_lufs": 5.0})),
    ] {
        let r = call(&client, tool, args.clone()).await;
        assert_eq!(r.is_error, Some(true), "{tool} {args} should fail: {r:?}");
        let s = session.lock().unwrap();
        assert_eq!(
            s.project().tracks,
            before.tracks,
            "{tool} changed the project"
        );
        assert_eq!(
            s.history().undo_entries().len(),
            steps,
            "{tool} added a step"
        );
    }
    assert_eq!(std::fs::read_dir(&src).unwrap().count(), files);
    assert!(!dir.join("x.wav").exists());
    assert!(!tmp.path().join("x.wav").exists());

    // The happy paths, each one undo step.
    ok(&call(&client, "move_clip", json!({"clip": c, "start_beats": 2.0})).await);
    ok(&call(
        &client,
        "trim_clip",
        json!({"clip": c, "length_beats": 2.0, "offset_beats": 0.5}),
    )
    .await);
    ok(&call(
        &client,
        "set_fades",
        json!({"clip": c, "fade_in_beats": 0.25, "fade_out_beats": 0.5}),
    )
    .await);
    ok(&call(&client, "set_solo", json!({"track": a, "solo": true})).await);
    let imported = ok(&call(
        &client,
        "import_audio",
        json!({"track": a, "path": dir.join(before.tracks[0].clips[0].audio.as_deref().unwrap()).display().to_string(), "start_beats": 8.0}),
    )
    .await);
    assert!((imported["length_beats"].as_f64().unwrap() - 4.0).abs() < 1e-3);
    assert_ne!(
        imported["audio"],
        before.tracks[0].clips[0].audio.as_deref().unwrap()
    );
    {
        let s = session.lock().unwrap();
        let (_, clip) = s.project().clip(woa_model::ClipId(c)).unwrap();
        assert_eq!(
            (
                clip.start_beats,
                clip.length_beats,
                clip.offset_beats,
                clip.fade_in_beats,
                clip.fade_out_beats
            ),
            (2.0, 2.0, 0.5, 0.25, 0.5)
        );
        assert!(s.project().tracks[0].solo);
        assert_eq!(s.history().undo_entries().len(), steps + 5);
    }
    ok(&call(&client, "remove_clip", json!({"clip": c})).await);
    ok(&call(&client, "remove_track", json!({"track": midi})).await);
    ok(&call(&client, "undo", json!({})).await);
    {
        let s = session.lock().unwrap();
        assert_eq!(s.project().tracks.len(), 2, "track is back");
        assert!(s.project().clip(woa_model::ClipId(c)).is_none());
    }

    // Analyze without clips is an error, not a panic.
    ok(&call(&client, "remove_track", json!({"track": a})).await);
    let r = call(&client, "analyze", json!({})).await;
    assert_eq!(r.is_error, Some(true));

    client.cancel().await.unwrap();
    server.await.unwrap();
}

/// A project with one short audio clip, ready to export.
async fn export_project(dir: &std::path::Path) -> (Client, tokio::task::JoinHandle<()>) {
    let (client, _agent, server) = start(dir).await;
    let a = track(&client, "A").await;
    ok(&call(
        &client,
        "generate_notes",
        json!({"track": a, "length_beats": 2.0, "waveform": "sine",
               "notes": [{"beat": 0.0, "pitch": 69, "length_beats": 1.0}]}),
    )
    .await);
    (client, server)
}

#[tokio::test(flavor = "current_thread")]
async fn export_into_new_subfolder() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("neu.woa");
    let (client, server) = export_project(&dir).await;
    ok(&call(&client, "export", json!({"path": "a/b/mix.wav"})).await);
    assert!(dir.join("a/b/mix.wav").is_file());
    // Overwriting a regular file works, and no temp file is left behind.
    ok(&call(&client, "export", json!({"path": "a/b/mix.wav"})).await);
    assert_eq!(std::fs::read_dir(dir.join("a/b")).unwrap().count(), 1);
    client.cancel().await.unwrap();
    server.await.unwrap();
}

#[cfg(unix)]
#[tokio::test(flavor = "current_thread")]
async fn export_rejects_symlinks() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("sym.woa");
    let outside = tmp.path().join("draussen");
    std::fs::create_dir(&outside).unwrap();
    let (client, server) = export_project(&dir).await;

    // Symlinked ancestor with a missing subfolder below it.
    std::os::unix::fs::symlink(&outside, dir.join("link")).unwrap();
    let r = call(&client, "export", json!({"path": "link/neu/x.wav"})).await;
    assert_eq!(r.is_error, Some(true), "{r:?}");
    assert!(!outside.join("neu").exists());

    // Target itself is a symlink to a file outside.
    let victim = outside.join("opfer.wav");
    std::fs::write(&victim, b"unveraendert").unwrap();
    std::os::unix::fs::symlink(&victim, dir.join("ziel.wav")).unwrap();
    let r = call(&client, "export", json!({"path": "ziel.wav"})).await;
    assert_eq!(r.is_error, Some(true), "{r:?}");
    assert_eq!(std::fs::read(&victim).unwrap(), b"unveraendert");
    assert!(
        std::fs::symlink_metadata(dir.join("ziel.wav"))
            .unwrap()
            .file_type()
            .is_symlink()
    );

    client.cancel().await.unwrap();
    server.await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn failed_export_leaves_no_new_folders() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("leer.woa");
    let (client, server) = export_project(&dir).await;

    // Fails after the folders exist (target_lufs out of range is checked after render).
    let r = call(
        &client,
        "export",
        json!({"path": "neu/tief/x.wav", "target_lufs": 5.0}),
    )
    .await;
    assert_eq!(r.is_error, Some(true), "{r:?}");
    assert!(
        !dir.join("neu").exists(),
        "created folders are removed again"
    );

    // An existing folder stays, only the new one below it goes.
    std::fs::create_dir(dir.join("da")).unwrap();
    let r = call(
        &client,
        "export",
        json!({"path": "da/tief/x.wav", "target_lufs": 5.0}),
    )
    .await;
    assert_eq!(r.is_error, Some(true), "{r:?}");
    assert!(dir.join("da").is_dir());
    assert!(!dir.join("da/tief").exists());

    // Unknown format: nothing is created at all.
    let r = call(&client, "export", json!({"path": "neu2/x.xyz"})).await;
    assert_eq!(r.is_error, Some(true), "{r:?}");
    assert!(!dir.join("neu2").exists());

    client.cancel().await.unwrap();
    server.await.unwrap();
}
