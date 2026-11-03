//! DJ-Paket: Hot Cues und Schleife je Titel (Schema 6). Schlüssel ist der Inhalts-Hash wie bei
//! den Wellenformen: verschiebt oder benennt man die Datei um, bleiben die Marken; ändert sich
//! der Inhalt, gelten sie nicht mehr (Sekunden passen dann nicht mehr sicher).
//!
//! Tabelle `dj_marks`: Platz 1–4 sind Hot Cues (nur `start`), Platz 0 ist die gespeicherte
//! Schleife (`start` und `end`). Alle Zeiten in Sekunden. Nie im Audio-Thread.

use rusqlite::params;

use crate::{Library, Result};

/// Anzahl der Hot Cues je Titel.
pub const HOT_CUES: usize = 4;

/// Gespeicherte DJ-Marken eines Titels.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct DjMarks {
    /// Hot Cues 1–4 (Sekunden).
    pub hot: [Option<f64>; HOT_CUES],
    /// Schleife (Anfang, Ende) in Sekunden.
    pub looped: Option<(f64, f64)>,
}

impl DjMarks {
    pub fn is_empty(&self) -> bool {
        self.hot.iter().all(Option::is_none) && self.looped.is_none()
    }
}

impl Library {
    /// Marken zum Inhalt `hash` (leer, wenn keine gespeichert sind).
    pub fn dj_marks(&self, hash: &str) -> Result<DjMarks> {
        let mut st = self
            .conn
            .prepare("SELECT slot, start, end FROM dj_marks WHERE hash = ?1")?;
        let rows = st.query_map([hash], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, f64>(1)?,
                r.get::<_, Option<f64>>(2)?,
            ))
        })?;
        let mut m = DjMarks::default();
        for row in rows {
            let (slot, start, end) = row?;
            match (slot, end) {
                (0, Some(e)) if e > start => m.looped = Some((start, e)),
                (s @ 1..=4, _) => m.hot[(s - 1) as usize] = Some(start),
                _ => {}
            }
        }
        Ok(m)
    }

    /// Ersetzt alle Marken zum Inhalt `hash` (ein Schritt, unteilbar).
    pub fn set_dj_marks(&self, hash: &str, m: &DjMarks) -> Result<()> {
        self.transaction(|lib| {
            lib.conn
                .execute("DELETE FROM dj_marks WHERE hash = ?1", [hash])?;
            for (i, h) in m.hot.iter().enumerate() {
                if let Some(s) = h.filter(|s| s.is_finite() && *s >= 0.0) {
                    lib.conn.execute(
                        "INSERT INTO dj_marks(hash, slot, start, end) VALUES (?1, ?2, ?3, NULL)",
                        params![hash, i as i64 + 1, s],
                    )?;
                }
            }
            if let Some((a, b)) = m.looped.filter(|(a, b)| a.is_finite() && b > a) {
                lib.conn.execute(
                    "INSERT INTO dj_marks(hash, slot, start, end) VALUES (?1, 0, ?2, ?3)",
                    params![hash, a, b],
                )?;
            }
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marks_roundtrip_and_replace() {
        let lib = Library::open_in_memory().unwrap();
        assert!(lib.dj_marks("h").unwrap().is_empty());
        let m = DjMarks {
            hot: [Some(1.5), None, Some(30.25), None],
            looped: Some((32.0, 34.0)),
        };
        lib.set_dj_marks("h", &m).unwrap();
        assert_eq!(lib.dj_marks("h").unwrap(), m);
        // Ersetzen: gelöschter Hot Cue und Schleife sind weg.
        let m2 = DjMarks {
            hot: [Some(1.5), None, None, None],
            looped: None,
        };
        lib.set_dj_marks("h", &m2).unwrap();
        assert_eq!(lib.dj_marks("h").unwrap(), m2);
        // Anderer Inhalt bleibt unberührt.
        assert!(lib.dj_marks("anders").unwrap().is_empty());
    }

    #[test]
    fn invalid_marks_are_not_stored() {
        let lib = Library::open_in_memory().unwrap();
        let m = DjMarks {
            hot: [Some(f64::NAN), Some(-1.0), None, None],
            looped: Some((5.0, 4.0)),
        };
        lib.set_dj_marks("h", &m).unwrap();
        assert!(lib.dj_marks("h").unwrap().is_empty());
    }

    #[test]
    fn migration_from_v5_adds_marks_and_requeues_tempo_tracks() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join(crate::DB_FILE);
        {
            let c = rusqlite::Connection::open(&p).unwrap();
            for m in &crate::MIGRATIONS[..5] {
                c.execute_batch(m).unwrap();
            }
            c.execute_batch(
                "INSERT INTO tracks(id, path, hash, title, analyzed_hash, bpm, key) \
                     VALUES (1, '/a.wav', 'ha', 'A', 'ha', 124.0, 'a-Moll');
                 INSERT INTO tracks(id, path, hash, title, analyzed_hash, bpm, key) \
                     VALUES (2, '/b.wav', 'hb', 'B', 'hb', NULL, 'C-Dur');",
            )
            .unwrap();
            c.pragma_update(None, "user_version", 5).unwrap();
        }
        let lib = Library::open(&p).unwrap();
        assert_eq!(lib.schema_version().unwrap(), crate::SCHEMA_VERSION);
        // Titel mit Tempo kommen für das Beatgrid noch einmal in die Analyse, die ohne nicht.
        let pending: Vec<i64> = lib
            .pending_analysis(10)
            .unwrap()
            .into_iter()
            .map(|(id, _, _)| id)
            .collect();
        assert_eq!(pending, vec![1]);
        lib.set_dj_marks("ha", &DjMarks::default()).unwrap();
        let a = crate::Analysis {
            bpm: Some(124.0),
            beat_offset: Some(0.12),
            ..Default::default()
        };
        assert!(lib.set_analysis(1, "ha", &a).unwrap());
        assert_eq!(
            lib.get(1).unwrap().unwrap().analysis.unwrap().beat_offset,
            Some(0.12)
        );
    }
}
