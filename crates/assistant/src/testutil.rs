//! Kleiner HTTP-Mock für die Tests: antwortet der Reihe nach oder je Pfad und merkt sich,
//! was ankam. Nur für Tests.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Default)]
pub struct Req {
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

impl Req {
    pub fn header(&self, name: &str) -> Option<String> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.clone())
    }
}

enum Plan {
    Sequence(Vec<String>),
    Fixed(u16, String),
    Routes(Vec<(String, u16, String)>),
    /// Antwort mit Inhaltstyp (z. B. Ereignisstrom).
    Typed(String, String),
}

pub struct MockServer {
    url: String,
    seen: Arc<Mutex<Vec<Req>>>,
}

fn read_req(stream: &mut std::net::TcpStream) -> Option<Req> {
    let mut r = BufReader::new(stream.try_clone().ok()?);
    let mut line = String::new();
    r.read_line(&mut line).ok()?;
    let mut parts = line.split_whitespace();
    let _method = parts.next()?;
    let path = parts.next()?.to_string();
    let mut headers = Vec::new();
    let mut len = 0usize;
    loop {
        let mut h = String::new();
        r.read_line(&mut h).ok()?;
        let h = h.trim_end();
        if h.is_empty() {
            break;
        }
        if let Some((k, v)) = h.split_once(':') {
            if k.eq_ignore_ascii_case("content-length") {
                len = v.trim().parse().unwrap_or(0);
            }
            headers.push((k.trim().to_string(), v.trim().to_string()));
        }
    }
    let mut body = vec![0u8; len];
    r.read_exact(&mut body).ok()?;
    Some(Req {
        path,
        headers,
        body: String::from_utf8_lossy(&body).into_owned(),
    })
}

impl MockServer {
    fn run(plan: Plan) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = seen.clone();
        std::thread::spawn(move || {
            let mut n = 0usize;
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { break };
                let Some(req) = read_req(&mut stream) else {
                    continue;
                };
                let (status, ctype, body) = match &plan {
                    Plan::Sequence(v) => (
                        200,
                        "application/json".to_string(),
                        v.get(n).or(v.last()).cloned().unwrap_or_default(),
                    ),
                    Plan::Fixed(s, b) => (*s, "application/json".to_string(), b.clone()),
                    Plan::Typed(t, b) => (200, t.clone(), b.clone()),
                    Plan::Routes(rs) => rs
                        .iter()
                        .find(|(p, _, _)| *p == req.path)
                        .map(|(_, s, b)| (*s, "application/json".to_string(), b.clone()))
                        .unwrap_or((404, "application/json".to_string(), "{}".into())),
                };
                n += 1;
                log.lock().unwrap().push(req);
                let resp = format!(
                    "HTTP/1.1 {status} X\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(resp.as_bytes());
            }
        });
        Self { url, seen }
    }

    /// Antwortet der Reihe nach (die letzte wiederholt sich).
    pub fn start(responses: Vec<String>) -> Self {
        Self::run(Plan::Sequence(responses))
    }

    pub fn with_status(status: u16, body: String) -> Self {
        Self::run(Plan::Fixed(status, body))
    }

    pub fn start_routes(routes: Vec<(&str, u16, String)>) -> Self {
        Self::run(Plan::Routes(
            routes
                .into_iter()
                .map(|(p, s, b)| (p.to_string(), s, b))
                .collect(),
        ))
    }

    pub fn start_typed(content_type: &str, body: String) -> Self {
        Self::run(Plan::Typed(content_type.to_string(), body))
    }

    pub fn url(&self) -> String {
        self.url.clone()
    }

    pub fn requests(&self) -> Vec<Req> {
        self.seen.lock().unwrap().clone()
    }
}
