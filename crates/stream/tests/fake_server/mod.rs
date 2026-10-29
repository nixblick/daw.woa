//! Mini-Shoutcast (DNAS v2, Legacy-Quelle) mit `std::net` für die Tests: Quelle auf
//! `port`, `admin.cgi` und `7.html` auf `admin`.

#![allow(dead_code)]

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Default)]
pub struct Seen {
    pub logins: AtomicUsize,
    pub rejected: AtomicUsize,
    pub bytes: Mutex<Vec<u8>>,
    pub headers: Mutex<Vec<String>>,
    pub requests: Mutex<Vec<String>>,
    /// Nach so vielen Bytes einmal die Quellverbindung trennen (0 = nie).
    pub drop_after: AtomicUsize,
    pub stop: AtomicBool,
}

pub struct Fake {
    /// Quell-Port.
    pub port: u16,
    /// Admin-Port.
    pub admin: u16,
    pub seen: Arc<Seen>,
}

impl Drop for Fake {
    fn drop(&mut self) {
        self.seen.stop.store(true, Ordering::Relaxed);
    }
}

/// Startet den Fake; `expected_line` ist die erwartete Anmeldezeile ohne Zeilenende.
pub fn start(expected_line: &str) -> Fake {
    let admin = TcpListener::bind("127.0.0.1:0").unwrap();
    let source = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = source.local_addr().unwrap().port();
    let admin_port = admin.local_addr().unwrap().port();
    let seen = Arc::new(Seen::default());
    admin.set_nonblocking(true).unwrap();
    source.set_nonblocking(true).unwrap();
    {
        let seen = Arc::clone(&seen);
        std::thread::spawn(move || {
            while !seen.stop.load(Ordering::Relaxed) {
                match admin.accept() {
                    Ok((s, _)) => handle_admin(s, &seen),
                    Err(_) => std::thread::sleep(Duration::from_millis(10)),
                }
            }
        });
    }
    {
        let seen = Arc::clone(&seen);
        let expected = expected_line.to_string();
        std::thread::spawn(move || {
            while !seen.stop.load(Ordering::Relaxed) {
                match source.accept() {
                    Ok((s, _)) => {
                        let seen = Arc::clone(&seen);
                        let expected = expected.clone();
                        std::thread::spawn(move || handle_source(s, &expected, &seen));
                    }
                    Err(_) => std::thread::sleep(Duration::from_millis(10)),
                }
            }
        });
    }
    Fake {
        port,
        admin: admin_port,
        seen,
    }
}

fn read_until_blank(s: &mut TcpStream) -> String {
    let mut buf = Vec::new();
    let mut b = [0u8; 1];
    while !buf.ends_with(b"\r\n\r\n") && buf.len() < 8192 {
        match s.read(&mut b) {
            Ok(1) => buf.push(b[0]),
            _ => break,
        }
    }
    String::from_utf8_lossy(&buf).into_owned()
}

fn handle_admin(mut s: TcpStream, seen: &Seen) {
    s.set_nonblocking(false).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    let req = read_until_blank(&mut s);
    let line = req.lines().next().unwrap_or("").to_string();
    seen.requests.lock().unwrap().push(line.clone());
    let reply = if line.contains("/7.html") {
        "HTTP/1.0 200 OK\r\nContent-Type: text/html\r\n\r\n<html><body>3,1,5,100,3,128,Test</body></html>"
            .to_string()
    } else {
        "HTTP/1.0 200 OK\r\n\r\n<html><body>OK</body></html>".to_string()
    };
    let _ = s.write_all(reply.as_bytes());
}

fn handle_source(mut s: TcpStream, expected: &str, seen: &Seen) {
    s.set_nonblocking(false).unwrap();
    s.set_read_timeout(Some(Duration::from_millis(200)))
        .unwrap();
    let mut line = Vec::new();
    let mut b = [0u8; 1];
    let start = Instant::now();
    while !line.ends_with(b"\r\n") && start.elapsed() < Duration::from_secs(2) {
        if let Ok(1) = s.read(&mut b) {
            line.push(b[0]);
        }
    }
    let got = String::from_utf8_lossy(&line).trim_end().to_string();
    if got != expected {
        seen.rejected.fetch_add(1, Ordering::Relaxed);
        let _ = s.write_all(b"invalid password\r\n");
        return;
    }
    s.write_all(b"OK2\r\nicy-caps:11\r\n\r\n").unwrap();
    s.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    let headers = read_until_blank(&mut s);
    seen.headers.lock().unwrap().push(headers);
    seen.logins.fetch_add(1, Ordering::Relaxed);
    s.set_read_timeout(Some(Duration::from_millis(200)))
        .unwrap();
    let mut buf = [0u8; 4096];
    let mut here = 0usize;
    while !seen.stop.load(Ordering::Relaxed) {
        match s.read(&mut buf) {
            Ok(0) => return,
            Ok(n) => {
                seen.bytes.lock().unwrap().extend_from_slice(&buf[..n]);
                here += n;
                let lim = seen.drop_after.load(Ordering::Relaxed);
                if lim > 0 && here >= lim {
                    seen.drop_after.store(0, Ordering::Relaxed);
                    let _ = s.shutdown(std::net::Shutdown::Both);
                    return;
                }
            }
            Err(_) => {}
        }
    }
}

/// Wartet bis `f` wahr ist oder `limit` vorbei.
pub fn wait_for(limit: Duration, mut f: impl FnMut() -> bool) -> bool {
    let t = Instant::now();
    while t.elapsed() < limit {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    f()
}
