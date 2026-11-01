//! Sample-Suche über Freesound (J10 aus #262).
//!
//! Nur Klänge unter CC0 oder CC BY: die Anfrage filtert danach, und jede Antwort wird beim
//! Lesen noch einmal geprüft ([`license_kind`]); CC BY-NC, Sampling+ und alles Unbekannte
//! fallen raus. Der API-Schlüssel ist der eigene des Nutzers (Schlüsselbund-Eintrag
//! [`KEY_ACCOUNT`]); er steht nie im Projekt, nie im Log und nie in einer Antwort an das
//! Modell. Geladen wird die Vorschau (MP3), denn das Original braucht OAuth2.

use std::io::Read;
use std::path::Path;
use std::time::Duration;

/// Eintrag im Schlüsselbund für den Freesound-API-Schlüssel.
pub const KEY_ACCOUNT: &str = "freesound";

/// Adresse der Freesound-API v2.
pub const API: &str = "https://freesound.org/apiv2";

/// Datei im Projektordner, in die jede Herkunftszeile kommt.
pub const NOTES_FILE: &str = "notizen.md";

/// Filter der Suchanfrage: nur CC0 und CC BY.
pub const LICENSE_FILTER: &str = "license:(\"Creative Commons 0\" OR \"Attribution\")";

const FIELDS: &str = "id,name,username,license,duration,previews,url,tags";

/// Größte Vorschau, die geladen wird.
const MAX_PREVIEW_BYTES: u64 = 50 * 1024 * 1024;

/// Ein Treffer, der weitergegeben werden darf.
#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    pub id: u64,
    pub name: String,
    pub author: String,
    /// `CC0` oder `CC BY`.
    pub license: &'static str,
    pub license_url: String,
    pub duration: f64,
    /// Seite des Klangs bei Freesound.
    pub url: String,
    pub preview: String,
    pub tags: Vec<String>,
}

impl Hit {
    pub fn json(&self) -> serde_json::Value {
        serde_json::json!({
            "id": self.id,
            "name": self.name,
            "author": self.author,
            "license": self.license,
            "license_url": self.license_url,
            "duration": (self.duration * 100.0).round() / 100.0,
            "url": self.url,
            "tags": self.tags,
        })
    }

    /// Die Herkunftszeile für die Projektnotizen (Titel, Urheber, Lizenz, Quelle).
    pub fn credit_line(&self) -> String {
        format!(
            "- „{}“ von {} (Freesound), {}: {} · Quelle: {}",
            self.name, self.author, self.license, self.license_url, self.url
        )
    }
}

/// `CC0` oder `CC BY` für eine Lizenz-Adresse, sonst `None`.
pub fn license_kind(url: &str) -> Option<&'static str> {
    let u = url.to_ascii_lowercase();
    if u.contains("creativecommons.org/publicdomain/zero/") {
        Some("CC0")
    } else if u.contains("creativecommons.org/licenses/by/") {
        Some("CC BY")
    } else {
        None
    }
}

/// Liest einen Klang aus der Antwort; `None`, wenn Lizenz oder Vorschau nicht passen.
pub fn parse_sound(v: &serde_json::Value) -> Option<Hit> {
    let license_url = v["license"].as_str()?.to_string();
    let license = license_kind(&license_url)?;
    let preview = v["previews"]["preview-hq-mp3"]
        .as_str()
        .or_else(|| v["previews"]["preview-lq-mp3"].as_str())?
        .to_string();
    Some(Hit {
        id: v["id"].as_u64()?,
        name: v["name"].as_str().unwrap_or_default().to_string(),
        author: v["username"].as_str().unwrap_or_default().to_string(),
        license,
        license_url,
        duration: v["duration"].as_f64().unwrap_or(0.0),
        url: v["url"].as_str().unwrap_or_default().to_string(),
        preview,
        tags: v["tags"]
            .as_array()
            .map(|t| {
                t.iter()
                    .filter_map(|x| x.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default(),
    })
}

/// Treffer einer Suchantwort, ohne alles außer CC0 und CC BY.
pub fn parse_search(v: &serde_json::Value) -> Vec<Hit> {
    v["results"]
        .as_array()
        .map(|a| a.iter().filter_map(parse_sound).collect())
        .unwrap_or_default()
}

/// Hängt die Herkunftszeile an `notizen.md` im Projektordner an (legt sie bei Bedarf an).
pub fn append_credit(project_dir: &Path, hit: &Hit) -> std::io::Result<()> {
    use std::io::Write;
    let path = project_dir.join(NOTES_FILE);
    let fresh = !path.exists();
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    if fresh {
        writeln!(f, "# Notizen\n\n## Herkunft der Samples\n")?;
    }
    writeln!(f, "{}", hit.credit_line())
}

/// Zugang zur API: Adresse und Schlüssel-Quelle. Tests setzen eine lokale Adresse und einen
/// festen Schlüssel, damit sie weder das Netz noch den Schlüsselbund anfassen.
#[derive(Clone)]
pub struct Source {
    pub api: String,
    pub key: std::sync::Arc<dyn Fn() -> Option<String> + Send + Sync>,
}

impl Default for Source {
    fn default() -> Self {
        Self {
            api: API.to_string(),
            key: std::sync::Arc::new(|| {
                woa_secrets::reveal(&woa_radio::data_dir(), KEY_ACCOUNT)
                    .filter(|k| !k.trim().is_empty())
            }),
        }
    }
}

/// Nur https, außer für die lokale Testadresse.
fn local(url: &str) -> bool {
    url.starts_with("http://127.0.0.1:")
}

fn http(url: &str) -> Result<ureq::Agent, String> {
    if !url.starts_with("https://") && !local(url) {
        return Err(format!("nur https erlaubt: {url}"));
    }
    Ok(ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(15)))
        .timeout_global(Some(Duration::from_secs(120)))
        .https_only(!local(url))
        .max_redirects(3)
        .http_status_as_error(false)
        .build()
        .into())
}

impl Source {
    fn key(&self) -> Result<String, String> {
        (self.key)().ok_or_else(|| {
            "Kein Freesound-Schlüssel. Leg deinen eigenen unter Einstellungen → KI → \
             „Freesound-Schlüssel“ ab (kostenlos unter freesound.org/apiv2/apply)."
                .to_string()
        })
    }

    fn get_json(&self, url: &str, query: &[(&str, &str)]) -> Result<serde_json::Value, String> {
        let key = self.key()?;
        let mut req = http(url)?
            .get(url)
            .header("Authorization", &format!("Token {key}"));
        for (k, v) in query {
            req = req.query(*k, *v);
        }
        let mut resp = req.call().map_err(|e| format!("Freesound: {e}"))?;
        let status = resp.status().as_u16();
        if status == 401 {
            return Err("Freesound lehnt den Schlüssel ab (HTTP 401)".into());
        }
        if status != 200 {
            return Err(format!("Freesound: HTTP {status}"));
        }
        resp.body_mut()
            .with_config()
            .limit(4 * 1024 * 1024)
            .read_json()
            .map_err(|e| format!("Freesound: Antwort unlesbar ({e})"))
    }

    /// Sucht `text`; höchstens `limit` Treffer, nur CC0 und CC BY.
    pub fn search(&self, text: &str, limit: usize) -> Result<Vec<Hit>, String> {
        let size = limit.to_string();
        let v = self.get_json(
            &format!("{}/search/text/", self.api),
            &[
                ("query", text),
                ("filter", LICENSE_FILTER),
                ("fields", FIELDS),
                ("page_size", &size),
            ],
        )?;
        let mut hits = parse_search(&v);
        hits.truncate(limit);
        Ok(hits)
    }

    /// Ein Klang nach Nummer; `Err`, wenn er nicht CC0 oder CC BY ist.
    pub fn sound(&self, id: u64) -> Result<Hit, String> {
        let v = self.get_json(&format!("{}/sounds/{id}/", self.api), &[("fields", FIELDS)])?;
        parse_sound(&v).ok_or_else(|| {
            format!("Klang {id}: nur CC0 und CC BY sind erlaubt (oder keine Vorschau)")
        })
    }

    /// Lädt die Vorschau von `hit` nach `dest`. Bei Fehler bleibt keine Datei liegen.
    pub fn download(&self, hit: &Hit, dest: &Path) -> Result<(), String> {
        let result = (|| {
            let mut resp = http(&hit.preview)?
                .get(&hit.preview)
                .call()
                .map_err(|e| format!("Vorschau: {e}"))?;
            let status = resp.status().as_u16();
            if status != 200 {
                return Err(format!("Vorschau: HTTP {status}"));
            }
            let mut buf = Vec::new();
            resp.body_mut()
                .with_config()
                .limit(MAX_PREVIEW_BYTES)
                .reader()
                .read_to_end(&mut buf)
                .map_err(|e| format!("Vorschau: {e}"))?;
            std::fs::write(dest, &buf).map_err(|e| format!("{}: {e}", dest.display()))
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(dest);
        }
        result
    }
}

/// Dateiendung der Vorschau (`mp3`, im Test auch `wav`).
pub fn preview_ext(url: &str) -> String {
    let path = url.split(['?', '#']).next().unwrap_or_default();
    match path.rsplit_once('.') {
        Some((_, e))
            if !e.is_empty() && e.len() <= 4 && e.chars().all(|c| c.is_ascii_alphanumeric()) =>
        {
            e.to_ascii_lowercase()
        }
        _ => "mp3".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sound(id: u64, license: &str) -> serde_json::Value {
        serde_json::json!({
            "id": id, "name": format!("riser {id}"), "username": "someone",
            "license": license, "duration": 3.5, "url": format!("https://freesound.org/s/{id}/"),
            "previews": {"preview-hq-mp3": format!("https://cdn.freesound.org/p/{id}.mp3")},
            "tags": ["riser"]
        })
    }

    #[test]
    fn only_cc0_and_cc_by_pass() {
        assert_eq!(
            license_kind("http://creativecommons.org/publicdomain/zero/1.0/"),
            Some("CC0")
        );
        assert_eq!(
            license_kind("https://creativecommons.org/licenses/by/4.0/"),
            Some("CC BY")
        );
        assert_eq!(
            license_kind("http://creativecommons.org/licenses/by-nc/3.0/"),
            None
        );
        assert_eq!(
            license_kind("http://creativecommons.org/licenses/sampling+/1.0/"),
            None
        );
        assert_eq!(license_kind(""), None);
        let v = serde_json::json!({"results": [
            sound(1, "http://creativecommons.org/publicdomain/zero/1.0/"),
            sound(2, "http://creativecommons.org/licenses/by-nc/4.0/"),
            sound(3, "https://creativecommons.org/licenses/by/4.0/"),
            sound(4, "http://creativecommons.org/licenses/sampling+/1.0/"),
        ]});
        let ids: Vec<u64> = parse_search(&v).iter().map(|h| h.id).collect();
        assert_eq!(ids, [1, 3]);
    }

    #[test]
    fn credit_line_names_title_author_license_and_source() {
        let h = parse_sound(&sound(7, "https://creativecommons.org/licenses/by/4.0/")).unwrap();
        let dir = tempfile::tempdir().unwrap();
        append_credit(dir.path(), &h).unwrap();
        append_credit(dir.path(), &h).unwrap();
        let text = std::fs::read_to_string(dir.path().join(NOTES_FILE)).unwrap();
        assert_eq!(text.matches("## Herkunft der Samples").count(), 1, "{text}");
        let line = h.credit_line();
        assert_eq!(text.matches(&line).count(), 2);
        for part in [
            "riser 7",
            "someone",
            "CC BY",
            "licenses/by/4.0",
            "freesound.org/s/7/",
        ] {
            assert!(line.contains(part), "{line}");
        }
    }

    #[test]
    fn no_key_gives_a_hint_and_no_request() {
        let s = Source {
            api: "http://127.0.0.1:9/".into(),
            key: std::sync::Arc::new(|| None),
        };
        let e = s.search("riser", 5).unwrap_err();
        assert!(e.contains("Freesound-Schlüssel"), "{e}");
    }

    #[test]
    fn plain_http_is_refused() {
        assert!(http("http://freesound.org/x").is_err());
        assert!(http("https://freesound.org/x").is_ok());
        assert_eq!(
            preview_ext("https://cdn.freesound.org/p/1-hq.mp3?x=1"),
            "mp3"
        );
        assert_eq!(preview_ext("https://cdn.freesound.org/p/noext"), "mp3");
    }
}
