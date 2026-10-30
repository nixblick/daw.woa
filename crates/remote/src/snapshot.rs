//! Was das Handy sieht: der öffentliche Zustand der Party (ohne Dateipfade) und die
//! Änderungen gegenüber dem zuletzt gesendeten Stand.
//!
//! Muster aus Ardours OSC-Oberfläche (nur die Idee): wer sich verbindet, bekommt sofort den
//! Vollstand; danach gehen nur noch die Felder raus, die sich geändert haben.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};

/// Der laufende Titel.
#[derive(Debug, Clone, PartialEq)]
pub struct NowSnap {
    pub title: String,
    pub artist: String,
    /// Restzeit als Text („3:05“).
    pub remaining: String,
    /// 0..1.
    pub progress: f32,
    pub playing: bool,
}

/// Ein Wunsch in der Liste. Der Pfad bleibt im Programm und geht nie ans Handy.
#[derive(Debug, Clone, PartialEq)]
pub struct WishSnap {
    pub title: String,
    pub votes: u32,
    pub path: PathBuf,
}

/// Ein Titel, den Gäste suchen und wünschen dürfen.
#[derive(Debug, Clone, PartialEq)]
pub struct CatalogItem {
    pub title: String,
    pub path: PathBuf,
}

/// Der Zustand der Party, wie die Oberfläche ihn der Fernbedienung übergibt.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PartySnapshot {
    pub now: Option<NowSnap>,
    pub next: Option<String>,
    pub wishes: Vec<WishSnap>,
    pub level_db: f64,
}

/// Kurze, stabile Kennung eines Pfads (FNV-1a, 64 Bit, hex). Das Handy kennt nur sie.
pub fn short_id(path: &Path) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in path.to_string_lossy().as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{h:016x}")
}

impl PartySnapshot {
    /// Der Zustand als JSON für das Handy.
    pub fn to_public(&self) -> Value {
        let now = match &self.now {
            Some(n) => json!({
                "title": n.title,
                "artist": n.artist,
                "remaining": n.remaining,
                // Auf ganze Prozent, damit der Strom nicht bei jedem Bruchteil sendet.
                "progress": (f64::from(n.progress) * 100.0).round() / 100.0,
                "playing": n.playing,
            }),
            None => Value::Null,
        };
        let wishes: Vec<Value> = self
            .wishes
            .iter()
            .map(|w| json!({"id": short_id(&w.path), "title": w.title, "votes": w.votes}))
            .collect();
        json!({
            "now": now,
            "next": self.next,
            "wishes": wishes,
            "level_db": self.level_db.round(),
        })
    }
}

/// Die Felder von `new`, die sich gegenüber `old` geändert haben (entfernte Felder als `null`).
/// Leer, wenn nichts anders ist.
pub fn delta(old: &Value, new: &Value) -> Map<String, Value> {
    let mut out = Map::new();
    let (Some(o), Some(n)) = (old.as_object(), new.as_object()) else {
        if old != new
            && let Some(n) = new.as_object()
        {
            return n.clone();
        }
        return out;
    };
    for (k, v) in n {
        if o.get(k) != Some(v) {
            out.insert(k.clone(), v.clone());
        }
    }
    for k in o.keys() {
        if !n.contains_key(k) {
            out.insert(k.clone(), Value::Null);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_state_has_no_paths() {
        let s = PartySnapshot {
            now: Some(NowSnap {
                title: "T".into(),
                artist: "A".into(),
                remaining: "1:00".into(),
                progress: 0.5,
                playing: true,
            }),
            next: Some("N".into()),
            wishes: vec![WishSnap {
                title: "W".into(),
                votes: 2,
                path: "/geheim/ordner/w.mp3".into(),
            }],
            level_db: -2.0,
        };
        let text = s.to_public().to_string();
        assert!(!text.contains("geheim"), "{text}");
        assert!(text.contains(&short_id(Path::new("/geheim/ordner/w.mp3"))));
    }

    #[test]
    fn delta_lists_only_changes() {
        let a = json!({"now": {"t": 1}, "next": "x", "gone": 1});
        let b = json!({"now": {"t": 2}, "next": "x", "new": true});
        let d = delta(&a, &b);
        assert_eq!(d.len(), 3);
        assert_eq!(d["now"], json!({"t": 2}));
        assert_eq!(d["gone"], Value::Null);
        assert_eq!(d["new"], json!(true));
        assert!(delta(&b, &b).is_empty());
    }

    #[test]
    fn short_id_is_stable_and_distinct() {
        assert_eq!(short_id(Path::new("/a")), short_id(Path::new("/a")));
        assert_ne!(short_id(Path::new("/a")), short_id(Path::new("/b")));
    }
}
