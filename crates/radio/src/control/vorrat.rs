//! Vorrat vordekodierter Titel (#568/#570/#572): Ein Hintergrund-Thread dekodiert die Titel,
//! die als Nächstes gebraucht werden. Laden nimmt sie von hier, statt im Control-Thread zu
//! dekodieren (das hielt jeden Befehl auf). Höchstens [`KEEP`] Puffer, der älteste fliegt.

use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};

use basedrop::Shared;

use crate::audio::{Measured, Stereo};

/// So viele dekodierte Titel hält der Vorrat (ein 4-Minuten-Titel braucht etwa 90 MB).
pub(super) const KEEP: usize = 2;

#[derive(Default)]
pub(super) struct Vorrat {
    ready: VecDeque<(PathBuf, Shared<Stereo>, Measured)>,
    /// Beim Dekodier-Thread angefragt, noch nicht zurück.
    asked: HashSet<PathBuf>,
    /// Dekodieren ging nicht: nicht noch einmal anfragen (das Laden meldet den Fehler selbst).
    failed: HashSet<PathBuf>,
}

impl Vorrat {
    /// Dekodierter Puffer und Messung für `path`, falls vorrätig.
    pub(super) fn get(&self, path: &Path) -> Option<(Shared<Stereo>, Measured)> {
        self.ready
            .iter()
            .find(|(p, _, _)| p == path)
            .map(|(_, b, m)| (b.clone(), *m))
    }

    /// Legt ein Ergebnis des Dekodier-Threads ab.
    pub(super) fn put(&mut self, path: PathBuf, buf: Shared<Stereo>, m: Measured) {
        self.asked.remove(&path);
        self.ready.retain(|(p, _, _)| *p != path);
        self.ready.push_back((path, buf, m));
        while self.ready.len() > KEEP {
            self.ready.pop_front();
        }
    }

    /// Dekodieren ging nicht.
    pub(super) fn fail(&mut self, path: PathBuf) {
        self.asked.remove(&path);
        self.failed.insert(path);
    }

    /// Von `wanted` die Pfade, die weder vorrätig noch angefragt noch gescheitert sind; sie
    /// gelten danach als angefragt.
    pub(super) fn ask(&mut self, wanted: &[PathBuf]) -> Vec<PathBuf> {
        let mut out = Vec::new();
        for p in wanted.iter().take(KEEP) {
            let known = self.asked.contains(p)
                || self.failed.contains(p)
                || self.ready.iter().any(|(q, _, _)| q == p);
            if !known {
                self.asked.insert(p.clone());
                out.push(p.clone());
            }
        }
        out
    }

    #[cfg(test)]
    pub(super) fn len(&self) -> usize {
        self.ready.len()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn buf(c: &basedrop::Collector) -> Shared<Stereo> {
        Shared::new(&c.handle(), Stereo::default())
    }

    fn m() -> Measured {
        Measured {
            lufs: None,
            peak_db: None,
            seconds: 1.0,
            auto_cues: crate::cues::Cues::default(),
            vocal: None,
        }
    }

    #[test]
    fn keeps_two_drops_the_oldest_and_asks_each_path_once() {
        let c = basedrop::Collector::new();
        let mut v = Vorrat::default();
        let (a, b, d) = (PathBuf::from("a"), PathBuf::from("b"), PathBuf::from("d"));
        assert_eq!(v.ask(&[a.clone(), b.clone()]), vec![a.clone(), b.clone()]);
        assert!(
            v.ask(&[a.clone(), b.clone()]).is_empty(),
            "nicht doppelt anfragen"
        );
        v.put(a.clone(), buf(&c), m());
        v.put(b.clone(), buf(&c), m());
        assert!(v.ask(std::slice::from_ref(&a)).is_empty(), "vorrätig");
        v.put(d.clone(), buf(&c), m());
        assert_eq!(v.len(), KEEP);
        assert!(v.get(&a).is_none(), "der älteste fliegt");
        assert!(v.get(&b).is_some() && v.get(&d).is_some());
        v.fail(PathBuf::from("x"));
        assert!(
            v.ask(&[PathBuf::from("x")]).is_empty(),
            "gescheitert: nicht wieder"
        );
    }
}
