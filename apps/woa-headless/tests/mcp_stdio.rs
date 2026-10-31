//! `woa-headless mcp <path>`: plain JSON-RPC over the child's stdin/stdout pipes,
//! exactly as Claude Code talks to it.

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

use serde_json::{Value, json};

#[test]
fn mcp_over_stdio_builds_and_saves() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("stdio.woa");
    let mut child = Command::new(env!("CARGO_BIN_EXE_woa-headless"))
        .arg("mcp")
        .arg(&dir)
        // Eine laufende WOA-App auf dieser Maschine darf den Test nicht übernehmen.
        .env("WOA_MCP_HEADLESS", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut out = BufReader::new(child.stdout.take().unwrap());

    let mut send = move |msg: Value| {
        writeln!(stdin, "{msg}").unwrap();
        stdin.flush().unwrap();
    };
    let mut recv = move |id: u64| -> Value {
        loop {
            let mut line = String::new();
            assert!(out.read_line(&mut line).unwrap() > 0, "server closed");
            let v: Value = serde_json::from_str(&line).unwrap();
            if v["id"] == id {
                return v;
            }
        }
    };

    send(
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
        "protocolVersion": "2025-06-18", "capabilities": {},
        "clientInfo": {"name": "test", "version": "0"}}}),
    );
    let init = recv(1);
    assert_eq!(init["result"]["serverInfo"]["name"], "woa");
    send(json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));

    send(json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call",
        "params": {"name": "add_track", "arguments": {"name": "Musik", "kind": "midi"}}}));
    let r = recv(2);
    assert_ne!(r["result"]["isError"], true, "{r}");
    send(json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call",
        "params": {"name": "set_tempo", "arguments": {"bpm": 100.0}}}));
    assert_ne!(recv(3)["result"]["isError"], true);
    send(json!({"jsonrpc": "2.0", "id": 4, "method": "tools/call",
        "params": {"name": "save", "arguments": {}}}));
    assert_ne!(recv(4)["result"]["isError"], true);

    drop(send);
    let status = child.wait().unwrap();
    assert!(status.success());

    let p = woa_model::store::load(&dir).unwrap();
    assert_eq!(p.tempo_bpm, 100.0);
    assert_eq!(p.tracks.len(), 1);
    assert_eq!(p.tracks[0].name, "Musik");
}
