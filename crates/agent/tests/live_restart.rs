//! #527: Nach einem Neustart der App (neuer Port, neues Live-Geheimnis) findet die
//! stdio-Weiterleitung sie über die Lock-Datei wieder, ohne neue Claude-Sitzung.

#![allow(clippy::unwrap_used, clippy::expect_used)] // Tests duerfen abbrechen (Regel 6 gilt fuer Produktivcode).

use std::cell::RefCell;
use std::io::{BufRead, Cursor, Read};
use std::sync::Arc;

use woa_agent::live::{self, LiveServer};

const TOKEN: &str = "test-token-restart-1";

const INIT: &str = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"test","version":"1"}}}"#;

fn call(id: u32) -> String {
    format!(
        r#"{{"jsonrpc":"2.0","id":{id},"method":"tools/call","params":{{"name":"output_levels","arguments":{{}}}}}}"#
    )
}

/// Eingabe, die vor der Zeile mit Index `at` die App „neu startet“.
struct RestartBefore<F: FnMut()> {
    lines: Vec<String>,
    next: usize,
    at: usize,
    restart: F,
    cur: Cursor<Vec<u8>>,
}

impl<F: FnMut()> Read for RestartBefore<F> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = {
            let b = self.fill_buf()?;
            let n = b.len().min(buf.len());
            buf[..n].copy_from_slice(&b[..n]);
            n
        };
        self.consume(n);
        Ok(n)
    }
}

impl<F: FnMut()> BufRead for RestartBefore<F> {
    fn fill_buf(&mut self) -> std::io::Result<&[u8]> {
        if self.cur.position() as usize >= self.cur.get_ref().len() && self.next < self.lines.len()
        {
            if self.next == self.at {
                (self.restart)();
            }
            let l = &self.lines[self.next];
            self.next += 1;
            self.cur = Cursor::new(format!("{l}\n").into_bytes());
        }
        self.cur.fill_buf()
    }
    fn consume(&mut self, n: usize) {
        self.cur.consume(n);
    }
}

#[test]
fn call_survives_an_app_restart_with_new_port_and_key() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tmp.path().to_path_buf();
    let first = LiveServer::start(&data, TOKEN.into(), 0, Arc::new(|| {})).unwrap();
    let app = live::running_app(&data).unwrap();
    let mut slot = Some(first);
    let second: RefCell<Option<LiveServer>> = RefCell::new(None);
    let input = RestartBefore {
        lines: vec![INIT.to_string(), call(2), call(3)],
        next: 0,
        at: 2,
        restart: || {
            drop(slot.take());
            *second.borrow_mut() =
                Some(LiveServer::start(&data, TOKEN.into(), 0, Arc::new(|| {})).unwrap());
        },
        cur: Cursor::new(Vec::new()),
    };
    let mut out = Vec::new();
    live::forward_resolving(
        app.port,
        &app.key,
        Some(&|| live::running_app(&data)),
        input,
        &mut out,
    )
    .unwrap();
    let out = String::from_utf8(out).unwrap();
    let l: Vec<&str> = out.lines().collect();
    assert_eq!(l.len(), 3, "{out}");
    assert!(!out.contains("nicht erreichbar"), "{out}");
    assert!(l[2].contains("\"id\":3"), "{out}");
    assert!(!l[2].contains("\"error\""), "{out}");

    // Keine App mehr: klare Meldung statt Winsock-Text.
    drop(second.borrow_mut().take());
    let mut out = Vec::new();
    live::forward_resolving(
        app.port,
        &app.key,
        Some(&|| live::running_app(&data)),
        call(5).as_bytes(),
        &mut out,
    )
    .unwrap();
    let out = String::from_utf8(out).unwrap();
    assert!(out.contains("WOA läuft nicht"), "{out}");
}
