//! Spielhistorie (Paket 28): was wann gespielt wurde. Sie liegt in der Tabelle `plays` der
//! Bibliotheksdatenbank und übersteht damit einen Neustart; die Rotation liest daraus ihre
//! Sperrfristen.

use std::collections::HashMap;

use rusqlite::params;

use crate::{Library, Result};

/// Eine gespielte Zeile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Play {
    pub track_id: i64,
    pub artist: String,
    pub category: String,
    /// Unix-Sekunden.
    pub played: i64,
}

impl Library {
    /// Trägt einen gespielten Titel ein.
    pub fn record_play(&self, track_id: i64, artist: &str, category: &str, at: i64) -> Result<()> {
        self.conn.execute(
            "INSERT INTO plays(track_id, artist, category, played) VALUES (?1, ?2, ?3, ?4)",
            params![track_id, artist, category, at],
        )?;
        Ok(())
    }

    /// Letzte Spielzeit je Titel.
    pub fn last_plays_by_track(&self) -> Result<HashMap<i64, i64>> {
        let mut st = self
            .conn
            .prepare("SELECT track_id, max(played) FROM plays GROUP BY track_id")?;
        let rows = st.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))?;
        Ok(rows.collect::<std::result::Result<HashMap<_, _>, _>>()?)
    }

    /// Letzte Spielzeit je Interpret (leere Namen zählen nicht).
    pub fn last_plays_by_artist(&self) -> Result<HashMap<String, i64>> {
        let mut st = self
            .conn
            .prepare("SELECT artist, max(played) FROM plays WHERE artist != '' GROUP BY artist")?;
        let rows = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
        Ok(rows.collect::<std::result::Result<HashMap<_, _>, _>>()?)
    }

    /// Anzahl der Spiele im Bereich `from <= played < to`.
    pub fn plays_between(&self, from: i64, to: i64) -> Result<usize> {
        let n: i64 = self.conn.query_row(
            "SELECT count(*) FROM plays WHERE played >= ?1 AND played < ?2",
            [from, to],
            |r| r.get(0),
        )?;
        Ok(n.max(0) as usize)
    }

    /// Alle Spiele ab `since`, älteste zuerst.
    pub fn plays_since(&self, since: i64) -> Result<Vec<Play>> {
        let mut st = self.conn.prepare(
            "SELECT track_id, artist, category, played FROM plays WHERE played >= ?1 \
             ORDER BY played, id",
        )?;
        let rows = st.query_map([since], |r| {
            Ok(Play {
                track_id: r.get(0)?,
                artist: r.get(1)?,
                category: r.get(2)?,
                played: r.get(3)?,
            })
        })?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }
}
