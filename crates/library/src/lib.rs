//! Bibliothek (Paket 24): Titel, Samples und Jingles in SQLite, mit Volltextsuche (FTS5), Tags
//! und Ordner-Import im Hintergrund.
//!
//! Alles hier läuft im Control-Thread oder in einem eigenen Hintergrund-Thread, nie im
//! Audio-Thread. Die Datenbank trägt ihre Schemaversion in `PRAGMA user_version`; ältere
//! Datenbanken werden beim Öffnen schrittweise angehoben, eine neuere als diese Programmversion
//! wird abgelehnt statt beschädigt.

pub mod analysis;
mod hash;
pub mod import;
pub mod plays;
pub mod rotation;
pub mod tags;

use std::fmt;
use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params, params_from_iter, types::Value};

pub use hash::file_hash;

/// Aktuelle Schemaversion.
pub const SCHEMA_VERSION: i64 = 4;
/// Längste Tag-Bezeichnung (Zeichen).
pub const MAX_TAG_CHARS: usize = 40;
/// Mehr Tags je Titel nimmt die Bibliothek nicht an.
pub const MAX_TAGS_PER_TRACK: usize = 50;
/// Obergrenze für Treffer je Suche.
pub const MAX_RESULTS: usize = 5000;

const V1: &str = "
CREATE TABLE tracks (
    id       INTEGER PRIMARY KEY,
    path     TEXT NOT NULL UNIQUE,
    hash     TEXT NOT NULL,
    title    TEXT NOT NULL,
    duration REAL NOT NULL DEFAULT 0,
    kind     TEXT NOT NULL DEFAULT 'music',
    added    INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX tracks_hash ON tracks(hash);
";

const V2: &str = "
CREATE TABLE tags (
    track_id INTEGER NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
    tag      TEXT NOT NULL,
    PRIMARY KEY (track_id, tag)
) WITHOUT ROWID;
CREATE INDEX tags_by_tag ON tags(tag, track_id);
CREATE VIRTUAL TABLE tracks_fts USING fts5(
    title, tags, path,
    tokenize = 'unicode61 remove_diacritics 2'
);
INSERT INTO tracks_fts(rowid, title, tags, path) SELECT id, title, '', path FROM tracks;
";

/// Paket 25: Analyse je Titel. `analyzed_hash` hält den Inhalt, zu dem die Werte gehören;
/// ändert sich die Datei, passt er nicht mehr und der Titel kommt wieder an die Reihe.
const V3: &str = "
ALTER TABLE tracks ADD COLUMN analyzed_hash TEXT;
ALTER TABLE tracks ADD COLUMN bpm REAL;
ALTER TABLE tracks ADD COLUMN key TEXT;
ALTER TABLE tracks ADD COLUMN lufs REAL;
ALTER TABLE tracks ADD COLUMN true_peak REAL;
ALTER TABLE tracks ADD COLUMN peak_db REAL;
ALTER TABLE tracks ADD COLUMN lead REAL;
ALTER TABLE tracks ADD COLUMN tail REAL;
ALTER TABLE tracks ADD COLUMN cue_in REAL;
ALTER TABLE tracks ADD COLUMN fade_out REAL;
ALTER TABLE tracks ADD COLUMN cue_out REAL;
";

/// Paket 28: Spielhistorie. Eine Zeile je gespielten Titel; `artist` und `category` sind
/// Abzüge zum Spielzeitpunkt (der Titel kann später umbenannt oder umgetaggt werden).
const V4: &str = "
CREATE TABLE plays (
    id       INTEGER PRIMARY KEY,
    track_id INTEGER NOT NULL,
    artist   TEXT NOT NULL DEFAULT '',
    category TEXT NOT NULL DEFAULT '',
    played   INTEGER NOT NULL
);
CREATE INDEX plays_by_time ON plays(played);
CREATE INDEX plays_by_track ON plays(track_id, played);
";

const MIGRATIONS: [&str; 4] = [V1, V2, V3, V4];

/// Fehler der Bibliothek, Text ohne Pfade.
#[derive(Debug)]
pub enum Error {
    Db(rusqlite::Error),
    Io(std::io::Error),
    /// Die Datenbank stammt von einer neueren WOA-Version.
    TooNew(i64),
    /// Eingabe abgelehnt (leerer Tag, zu lang, zu viele Tags …).
    Invalid(String),
    /// Titel gibt es nicht.
    NotFound(i64),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Db(e) => write!(f, "Datenbank: {e}"),
            Self::Io(e) => write!(f, "Datei: {e}"),
            Self::TooNew(v) => write!(
                f,
                "Die Bibliothek stammt von einer neueren WOA-Version (Schema {v}, bekannt {SCHEMA_VERSION})"
            ),
            Self::Invalid(m) => f.write_str(m),
            Self::NotFound(id) => write!(f, "Titel {id} gibt es nicht"),
        }
    }
}

impl std::error::Error for Error {}

impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Self {
        Self::Db(e)
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// Art eines Eintrags.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Kind {
    #[default]
    Music,
    Sample,
    Jingle,
}

impl Kind {
    pub const ALL: [Kind; 3] = [Kind::Music, Kind::Sample, Kind::Jingle];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Music => "music",
            Self::Sample => "sample",
            Self::Jingle => "jingle",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "sample" => Self::Sample,
            "jingle" => Self::Jingle,
            _ => Self::Music,
        }
    }

    /// Name für die Oberfläche.
    pub fn label(self) -> &'static str {
        match self {
            Self::Music => "Musik",
            Self::Sample => "Sample",
            Self::Jingle => "Jingle",
        }
    }
}

/// Ein Eintrag der Bibliothek.
#[derive(Debug, Clone, PartialEq)]
pub struct Track {
    pub id: i64,
    pub path: String,
    pub hash: String,
    pub title: String,
    pub duration: f64,
    pub kind: Kind,
    /// Unix-Sekunden des Imports.
    pub added: i64,
    /// Alphabetisch.
    pub tags: Vec<String>,
    /// Ergebnis der Analyse (Paket 25); `None`, solange der Titel noch nicht dran war oder die
    /// Datei sich seitdem geändert hat.
    pub analysis: Option<Analysis>,
}

/// Messwerte eines Titels (Paket 25). Fehlt ein Wert, ließ er sich nicht bestimmen
/// (Sinus ohne Einsätze hat kein Tempo, Stille keine Lautheit).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Analysis {
    pub bpm: Option<f64>,
    /// Anzeigename, z. B. „C-Dur“.
    pub key: Option<String>,
    /// Integrierte Lautheit in LUFS.
    pub lufs: Option<f64>,
    /// True Peak in dBTP.
    pub true_peak_db: Option<f64>,
    /// Sample-Spitze in dBFS.
    pub peak_db: Option<f64>,
    /// Sekunden Stille am Anfang und am Ende.
    pub lead_silence: Option<f64>,
    pub tail_silence: Option<f64>,
    /// Vorgeschlagene Cue-Punkte in Sekunden (Cue-In, Fade-Out, Cue-Out).
    pub cue_in: Option<f64>,
    pub fade_out: Option<f64>,
    pub cue_out: Option<f64>,
}

impl Analysis {
    /// Verstärkung in dB, die den Titel auf `target` LUFS bringt (Spitze höchstens −1 dBFS).
    pub fn gain_db(&self, target: f64) -> f64 {
        woa_dsp::analysis::track::gain_to_target_db(self.lufs, self.peak_db, target, -1.0)
    }
}

/// Was beim Eintragen mit einer Datei geschah.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddOutcome {
    Added(i64),
    /// Gleicher Inhalt (oder gleicher Pfad mit gleichem Inhalt) ist schon drin.
    Duplicate(i64),
    /// Eintrag aktualisiert (Datei am selben Pfad geändert, oder an neuen Pfad verschoben).
    Updated(i64),
}

/// Neuer Eintrag.
#[derive(Debug, Clone)]
pub struct NewTrack {
    pub path: String,
    pub hash: String,
    pub title: String,
    pub duration: f64,
    pub kind: Kind,
}

/// Spalte, nach der die Trefferliste sortiert wird (#124).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortBy {
    /// Mit Suchtext nach Treffergüte, sonst nach Titel.
    #[default]
    Relevance,
    Title,
    Bpm,
    Key,
    Lufs,
    Kind,
    Duration,
}

impl SortBy {
    /// Feste ORDER-BY-Ausdrücke, nie aus Eingaben zusammengesetzt. Titel ohne Messwert stehen
    /// in beiden Richtungen am Ende.
    fn order(self, desc: bool) -> Option<String> {
        let dir = if desc { "DESC" } else { "ASC" };
        let col = match self {
            Self::Relevance => return None,
            Self::Title => return Some(format!("t.title COLLATE NOCASE {dir}, t.id")),
            Self::Kind => return Some(format!("t.kind {dir}, t.title COLLATE NOCASE, t.id")),
            Self::Duration => {
                return Some(format!("t.duration {dir}, t.title COLLATE NOCASE, t.id"));
            }
            Self::Bpm => "t.bpm",
            Self::Key => "t.key",
            Self::Lufs => "t.lufs",
        };
        // Messwerte gelten nur zum aktuellen Inhalt (wie in der Anzeige).
        Some(format!(
            "({col} IS NULL OR t.analyzed_hash IS NULL OR t.analyzed_hash != t.hash), \
             {col} {dir}, t.title COLLATE NOCASE, t.id"
        ))
    }
}

/// Suchanfrage. Leerer Text liefert alles (nach Titel sortiert).
#[derive(Debug, Clone)]
pub struct Query {
    pub text: String,
    pub tag: Option<String>,
    pub kind: Option<Kind>,
    pub limit: usize,
    pub sort: SortBy,
    /// Absteigend sortieren.
    pub desc: bool,
}

impl Default for Query {
    fn default() -> Self {
        Self {
            text: String::new(),
            tag: None,
            kind: None,
            limit: 200,
            sort: SortBy::Relevance,
            desc: false,
        }
    }
}

/// Die Bibliothek.
pub struct Library {
    conn: Connection,
}

/// Schreibt Text als FTS5-Ausdruck: jedes Wort ein Präfix, alle müssen vorkommen. Sonderzeichen
/// der FTS-Syntax können so nicht eingeschleust werden.
fn fts_expression(text: &str) -> Option<String> {
    let words: Vec<String> = text
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .take(8)
        .map(|w| format!("\"{w}\"*"))
        .collect();
    (!words.is_empty()).then(|| words.join(" "))
}

/// Tag in Normalform: getrimmt, Kleinbuchstaben, Leerraum zusammengezogen.
pub fn normalize_tag(raw: &str) -> Result<String> {
    let t = raw
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    if t.is_empty() {
        return Err(Error::Invalid("Tag ist leer".into()));
    }
    if t.chars().count() > MAX_TAG_CHARS {
        return Err(Error::Invalid(format!(
            "Tag länger als {MAX_TAG_CHARS} Zeichen"
        )));
    }
    if t.chars().any(char::is_control) {
        return Err(Error::Invalid("Tag enthält Steuerzeichen".into()));
    }
    Ok(t)
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

const SELECT: &str = "SELECT t.id, t.path, t.hash, t.title, t.duration, t.kind, t.added, \
    (SELECT group_concat(tag, char(31)) FROM (SELECT tag FROM tags WHERE track_id = t.id ORDER BY tag)), \
    (t.analyzed_hash IS NOT NULL AND t.analyzed_hash = t.hash), \
    t.bpm, t.key, t.lufs, t.true_peak, t.peak_db, t.lead, t.tail, t.cue_in, t.fade_out, t.cue_out \
    FROM tracks t";

fn row_track(r: &rusqlite::Row<'_>) -> rusqlite::Result<Track> {
    let kind: String = r.get(5)?;
    let tags: Option<String> = r.get(7)?;
    Ok(Track {
        id: r.get(0)?,
        path: r.get(1)?,
        hash: r.get(2)?,
        title: r.get(3)?,
        duration: r.get(4)?,
        kind: Kind::parse(&kind),
        added: r.get(6)?,
        tags: tags
            .map(|s| s.split('\u{1f}').map(str::to_string).collect())
            .unwrap_or_default(),
        analysis: if r.get::<_, bool>(8)? {
            Some(Analysis {
                bpm: r.get(9)?,
                key: r.get(10)?,
                lufs: r.get(11)?,
                true_peak_db: r.get(12)?,
                peak_db: r.get(13)?,
                lead_silence: r.get(14)?,
                tail_silence: r.get(15)?,
                cue_in: r.get(16)?,
                fade_out: r.get(17)?,
                cue_out: r.get(18)?,
            })
        } else {
            None
        },
    })
}

impl Library {
    /// Öffnet (oder legt an) die Datenbank in `path` und hebt das Schema an.
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        // WAL: die Oberfläche liest, während der Import schreibt.
        conn.pragma_update(None, "journal_mode", "WAL")?;
        Self::prepare(conn)
    }

    /// Flüchtige Bibliothek im Speicher (Tests).
    pub fn open_in_memory() -> Result<Self> {
        Self::prepare(Connection::open_in_memory()?)
    }

    fn prepare(conn: Connection) -> Result<Self> {
        conn.busy_timeout(std::time::Duration::from_secs(10))?;
        conn.pragma_update(None, "foreign_keys", true)?;
        let lib = Self { conn };
        lib.migrate(SCHEMA_VERSION)?;
        Ok(lib)
    }

    /// Schemaversion der Datenbank.
    pub fn schema_version(&self) -> Result<i64> {
        Ok(self
            .conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))?)
    }

    /// Hebt das Schema schrittweise bis `target` an; jeder Schritt ist eine Transaktion.
    fn migrate(&self, target: i64) -> Result<()> {
        let current = self.schema_version()?;
        if current > SCHEMA_VERSION {
            return Err(Error::TooNew(current));
        }
        for v in current..target.min(SCHEMA_VERSION) {
            let script = MIGRATIONS[v as usize];
            tracing::info!(from = v, to = v + 1, "library migration");
            self.conn.execute_batch(&format!(
                "BEGIN IMMEDIATE;\n{script}\nPRAGMA user_version = {};\nCOMMIT;",
                v + 1
            ))?;
        }
        Ok(())
    }

    /// Führt `f` in einer Transaktion aus (ganz oder gar nicht). Beschleunigt Stapel stark.
    pub fn transaction<T>(&self, f: impl FnOnce(&Self) -> Result<T>) -> Result<T> {
        self.conn.execute_batch("BEGIN IMMEDIATE")?;
        match f(self) {
            Ok(v) => {
                self.conn.execute_batch("COMMIT")?;
                Ok(v)
            }
            Err(e) => {
                if let Err(r) = self.conn.execute_batch("ROLLBACK") {
                    tracing::warn!(error = %r, "library rollback failed");
                }
                Err(e)
            }
        }
    }

    /// Anzahl der Titel.
    pub fn count(&self) -> Result<usize> {
        let n: i64 = self
            .conn
            .query_row("SELECT count(*) FROM tracks", [], |r| r.get(0))?;
        Ok(n.max(0) as usize)
    }

    /// Titel, deren Analyse fehlt oder zu einem älteren Inhalt gehört: Nummer, Pfad, Hash.
    pub fn pending_analysis(&self, limit: usize) -> Result<Vec<(i64, String, String)>> {
        let mut st = self.conn.prepare(
            "SELECT id, path, hash FROM tracks \
             WHERE analyzed_hash IS NULL OR analyzed_hash != hash ORDER BY id LIMIT ?1",
        )?;
        let rows = st.query_map([limit as i64], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    /// Anzahl der Titel, die noch analysiert werden müssen.
    pub fn pending_analysis_count(&self) -> Result<usize> {
        let n: i64 = self.conn.query_row(
            "SELECT count(*) FROM tracks WHERE analyzed_hash IS NULL OR analyzed_hash != hash",
            [],
            |r| r.get(0),
        )?;
        Ok(n.max(0) as usize)
    }

    /// Speichert die Analyse, wenn die Datei noch zu `hash` gehört. Gibt `false` zurück, wenn
    /// der Eintrag fehlt oder sich der Inhalt inzwischen geändert hat.
    pub fn set_analysis(&self, id: i64, hash: &str, a: &Analysis) -> Result<bool> {
        let n = self.conn.execute(
            "UPDATE tracks SET analyzed_hash = ?2, bpm = ?3, key = ?4, lufs = ?5, true_peak = ?6, \
             peak_db = ?7, lead = ?8, tail = ?9, cue_in = ?10, fade_out = ?11, cue_out = ?12 \
             WHERE id = ?1 AND hash = ?2",
            params![
                id,
                hash,
                a.bpm,
                a.key,
                a.lufs,
                a.true_peak_db,
                a.peak_db,
                a.lead_silence,
                a.tail_silence,
                a.cue_in,
                a.fade_out,
                a.cue_out
            ],
        )?;
        Ok(n > 0)
    }

    /// Trägt die genaue Länge aus der Analyse ein, wenn die Datei noch zu `hash` gehört.
    pub fn set_duration(&self, id: i64, hash: &str, secs: f64) -> Result<bool> {
        let n = self.conn.execute(
            "UPDATE tracks SET duration = ?3 WHERE id = ?1 AND hash = ?2",
            params![id, hash, secs],
        )?;
        Ok(n > 0)
    }

    /// Nach dem Schreiben neuer Titelinfos: neuer Inhalt `new_hash`, neuer Anzeigetitel. Die
    /// Audiodaten sind unverändert (nur die Tags wurden geschrieben), deshalb bleibt eine
    /// Analyse zum alten Inhalt gültig und zieht mit, statt den Titel neu messen zu lassen.
    pub fn retagged(&self, id: i64, old_hash: &str, new_hash: &str, title: &str) -> Result<()> {
        self.transaction(|l| {
            let n = l.conn.execute(
                "UPDATE tracks SET hash = ?3, title = ?4, \
                 analyzed_hash = CASE WHEN analyzed_hash = ?2 THEN ?3 ELSE analyzed_hash END \
                 WHERE id = ?1",
                params![id, old_hash, new_hash, title],
            )?;
            if n == 0 {
                return Err(Error::NotFound(id));
            }
            l.conn.execute(
                "UPDATE tracks_fts SET title = ?1 WHERE rowid = ?2",
                params![title, id],
            )?;
            Ok(())
        })
    }

    /// Verwirft die Analyse eines Titels (er wird beim nächsten Lauf neu gemessen).
    pub fn clear_analysis(&self, id: i64) -> Result<()> {
        let n = self
            .conn
            .execute("UPDATE tracks SET analyzed_hash = NULL WHERE id = ?1", [id])?;
        if n == 0 {
            return Err(Error::NotFound(id));
        }
        Ok(())
    }

    /// Ein Titel nach Nummer.
    pub fn get(&self, id: i64) -> Result<Option<Track>> {
        Ok(self
            .conn
            .query_row(&format!("{SELECT} WHERE t.id = ?1"), [id], row_track)
            .optional()?)
    }

    /// Titel nach Pfad.
    pub fn by_path(&self, path: &str) -> Result<Option<Track>> {
        Ok(self
            .conn
            .query_row(&format!("{SELECT} WHERE t.path = ?1"), [path], row_track)
            .optional()?)
    }

    /// Trägt eine Datei ein; erkennt Doppelte am Inhalt (Hash) und am Pfad.
    pub fn add_track(&self, t: &NewTrack) -> Result<AddOutcome> {
        if let Some((id, hash)) = self
            .conn
            .query_row(
                "SELECT id, hash FROM tracks WHERE path = ?1",
                [&t.path],
                |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)),
            )
            .optional()?
        {
            if hash == t.hash {
                return Ok(AddOutcome::Duplicate(id));
            }
            self.conn.execute(
                "UPDATE tracks SET hash = ?1, duration = ?2 WHERE id = ?3",
                params![t.hash, t.duration, id],
            )?;
            return Ok(AddOutcome::Updated(id));
        }
        if let Some((id, old)) = self
            .conn
            .query_row(
                "SELECT id, path FROM tracks WHERE hash = ?1 LIMIT 1",
                [&t.hash],
                |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)),
            )
            .optional()?
        {
            // Gleicher Inhalt: war die alte Datei verschoben, folgt der Eintrag ihr.
            if Path::new(&old).exists() {
                return Ok(AddOutcome::Duplicate(id));
            }
            self.conn.execute(
                "UPDATE tracks SET path = ?1 WHERE id = ?2",
                params![t.path, id],
            )?;
            self.conn.execute(
                "UPDATE tracks_fts SET path = ?1 WHERE rowid = ?2",
                params![t.path, id],
            )?;
            return Ok(AddOutcome::Updated(id));
        }
        self.conn.execute(
            "INSERT INTO tracks(path, hash, title, duration, kind, added) VALUES (?1,?2,?3,?4,?5,?6)",
            params![t.path, t.hash, t.title, t.duration, t.kind.as_str(), now_secs()],
        )?;
        let id = self.conn.last_insert_rowid();
        self.conn.execute(
            "INSERT INTO tracks_fts(rowid, title, tags, path) VALUES (?1, ?2, '', ?3)",
            params![id, t.title, t.path],
        )?;
        Ok(AddOutcome::Added(id))
    }

    /// Ändert die Art (Musik, Sample, Jingle).
    pub fn set_kind(&self, id: i64, kind: Kind) -> Result<()> {
        let n = self.conn.execute(
            "UPDATE tracks SET kind = ?1 WHERE id = ?2",
            params![kind.as_str(), id],
        )?;
        if n == 0 {
            return Err(Error::NotFound(id));
        }
        Ok(())
    }

    fn refresh_fts_tags(&self, id: i64) -> Result<()> {
        let tags: Option<String> = self.conn.query_row(
            "SELECT group_concat(tag, ' ') FROM tags WHERE track_id = ?1",
            [id],
            |r| r.get(0),
        )?;
        self.conn.execute(
            "UPDATE tracks_fts SET tags = ?1 WHERE rowid = ?2",
            params![tags.unwrap_or_default(), id],
        )?;
        Ok(())
    }

    /// Vergibt einen Tag (Normalform siehe [`normalize_tag`]). Doppelt vergeben ist kein Fehler.
    /// Gibt den gespeicherten Tag zurück.
    pub fn add_tag(&self, id: i64, tag: &str) -> Result<String> {
        let tag = normalize_tag(tag)?;
        self.transaction(|lib| {
            if lib.get(id)?.is_none() {
                return Err(Error::NotFound(id));
            }
            let n: i64 =
                lib.conn
                    .query_row("SELECT count(*) FROM tags WHERE track_id = ?1", [id], |r| {
                        r.get(0)
                    })?;
            if n as usize >= MAX_TAGS_PER_TRACK {
                return Err(Error::Invalid(format!(
                    "Mehr als {MAX_TAGS_PER_TRACK} Tags je Titel sind nicht erlaubt"
                )));
            }
            lib.conn.execute(
                "INSERT OR IGNORE INTO tags(track_id, tag) VALUES (?1, ?2)",
                params![id, tag],
            )?;
            lib.refresh_fts_tags(id)?;
            Ok(())
        })?;
        Ok(tag)
    }

    /// Nimmt einen Tag von einem Titel.
    pub fn remove_tag(&self, id: i64, tag: &str) -> Result<()> {
        let tag = normalize_tag(tag)?;
        self.transaction(|lib| {
            lib.conn.execute(
                "DELETE FROM tags WHERE track_id = ?1 AND tag = ?2",
                params![id, tag],
            )?;
            lib.refresh_fts_tags(id)
        })
    }

    /// Nimmt einen Titel aus der Bibliothek (mit Tags und Suchindex). Die Datei auf der Platte
    /// bleibt, die Spielhistorie auch (#124).
    pub fn remove_track(&self, id: i64) -> Result<()> {
        self.transaction(|lib| {
            let n = lib.conn.execute("DELETE FROM tracks WHERE id = ?1", [id])?;
            if n == 0 {
                return Err(Error::NotFound(id));
            }
            lib.conn
                .execute("DELETE FROM tracks_fts WHERE rowid = ?1", [id])?;
            Ok(())
        })
    }

    /// Alle Tags mit der Zahl der Titel, häufigste zuerst.
    pub fn all_tags(&self) -> Result<Vec<(String, usize)>> {
        let mut st = self.conn.prepare(
            "SELECT tag, count(*) AS n FROM tags GROUP BY tag ORDER BY n DESC, tag LIMIT 500",
        )?;
        let rows = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
        let mut out = Vec::new();
        for r in rows {
            let (t, n) = r?;
            out.push((t, n.max(0) as usize));
        }
        Ok(out)
    }

    /// Sucht Titel. Text: jedes Wort als Präfix in Titel, Tags und Pfad (Groß-/Kleinschreibung
    /// und Akzente egal), alle Wörter müssen passen.
    pub fn search(&self, q: &Query) -> Result<Vec<Track>> {
        let limit = q.limit.clamp(1, MAX_RESULTS);
        let mut sql = String::from(SELECT);
        let mut args: Vec<Value> = Vec::new();
        let mut conds: Vec<String> = Vec::new();
        let order;
        if let Some(expr) = fts_expression(&q.text) {
            sql.push_str(" JOIN tracks_fts ON tracks_fts.rowid = t.id");
            args.push(Value::Text(expr));
            conds.push(format!("tracks_fts MATCH ?{}", args.len()));
            order = "ORDER BY tracks_fts.rank, t.title COLLATE NOCASE";
        } else {
            order = "ORDER BY t.title COLLATE NOCASE, t.id";
        }
        if let Some(k) = q.kind {
            args.push(Value::Text(k.as_str().into()));
            conds.push(format!("t.kind = ?{}", args.len()));
        }
        if let Some(tag) = &q.tag {
            args.push(Value::Text(normalize_tag(tag)?));
            conds.push(format!(
                "EXISTS (SELECT 1 FROM tags g WHERE g.track_id = t.id AND g.tag = ?{})",
                args.len()
            ));
        }
        let custom = q.sort.order(q.desc).map(|o| format!("ORDER BY {o}"));
        let order = custom.as_deref().unwrap_or(order);
        if !conds.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&conds.join(" AND "));
        }
        args.push(Value::Integer(limit as i64));
        sql.push_str(&format!(" {order} LIMIT ?{}", args.len()));
        let mut st = self.conn.prepare(&sql)?;
        let rows = st.query_map(params_from_iter(args), row_track)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nt(path: &str, hash: &str, title: &str) -> NewTrack {
        NewTrack {
            path: path.into(),
            hash: hash.into(),
            title: title.into(),
            duration: 3.0,
            kind: Kind::Music,
        }
    }

    fn q(text: &str) -> Query {
        Query {
            text: text.into(),
            ..Query::default()
        }
    }

    fn id_of(o: AddOutcome) -> i64 {
        match o {
            AddOutcome::Added(i) | AddOutcome::Duplicate(i) | AddOutcome::Updated(i) => i,
        }
    }

    #[test]
    fn search_finds_prefixes_ignores_case_and_accents() {
        let lib = Library::open_in_memory().unwrap();
        lib.add_track(&nt("/m/Sommer Nacht.wav", "h1", "Sommer Nacht"))
            .unwrap();
        lib.add_track(&nt("/m/Winter Tag.wav", "h2", "Winter Tag"))
            .unwrap();
        lib.add_track(&nt("/m/café.wav", "h3", "Café del Mar"))
            .unwrap();
        let r = lib.search(&q("somm")).unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].title, "Sommer Nacht");
        assert_eq!(lib.search(&q("NACHT sommer")).unwrap().len(), 1);
        assert_eq!(lib.search(&q("cafe")).unwrap().len(), 1);
        assert_eq!(lib.search(&q("winter nacht")).unwrap().len(), 0);
        // Ordnername steht im Pfad und findet mit.
        assert_eq!(lib.search(&q("m")).unwrap().len(), 3);
        // Leer: alles, nach Titel sortiert.
        let all = lib.search(&q("")).unwrap();
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].title, "Café del Mar");
    }

    #[test]
    fn hostile_search_text_cannot_break_the_query() {
        let lib = Library::open_in_memory().unwrap();
        lib.add_track(&nt("/m/a.wav", "h1", "Alpha")).unwrap();
        for text in [
            "\"",
            "alpha OR",
            "* NEAR(",
            "title:alpha",
            "'; DROP TABLE tracks;--",
            "\"\"\"*",
        ] {
            assert!(lib.search(&q(text)).is_ok(), "{text}");
        }
        assert_eq!(lib.count().unwrap(), 1);
    }

    #[test]
    fn tags_filter_and_feed_the_search() {
        let lib = Library::open_in_memory().unwrap();
        let a = id_of(lib.add_track(&nt("/m/a.wav", "h1", "Alpha")).unwrap());
        let b = id_of(lib.add_track(&nt("/m/b.wav", "h2", "Beta")).unwrap());
        assert_eq!(lib.add_tag(a, "  Morgen  Show ").unwrap(), "morgen show");
        lib.add_tag(a, "morgen show").unwrap();
        lib.add_tag(b, "Nacht").unwrap();
        lib.add_tag(a, "nacht").unwrap();
        assert_eq!(lib.get(a).unwrap().unwrap().tags, ["morgen show", "nacht"]);
        let by_tag = |t: &str| {
            lib.search(&Query {
                tag: Some(t.into()),
                ..Query::default()
            })
            .unwrap()
        };
        assert_eq!(by_tag("NACHT").len(), 2);
        assert_eq!(by_tag("morgen show").len(), 1);
        assert_eq!(by_tag("gibts nicht").len(), 0);
        // Tag-Text steht auch in der Volltextsuche; Tag und Text zusammen.
        assert_eq!(lib.search(&q("morgen")).unwrap().len(), 1);
        let both = lib
            .search(&Query {
                text: "beta".into(),
                tag: Some("nacht".into()),
                ..Query::default()
            })
            .unwrap();
        assert_eq!(both.len(), 1);
        lib.remove_tag(a, "morgen show").unwrap();
        assert_eq!(lib.search(&q("morgen")).unwrap().len(), 0);
        assert_eq!(lib.all_tags().unwrap(), [("nacht".to_string(), 2)]);
    }

    #[test]
    fn bad_tags_and_unknown_tracks_are_refused() {
        let lib = Library::open_in_memory().unwrap();
        let a = id_of(lib.add_track(&nt("/m/a.wav", "h1", "Alpha")).unwrap());
        assert!(lib.add_tag(a, "   ").is_err());
        assert!(lib.add_tag(a, &"x".repeat(MAX_TAG_CHARS + 1)).is_err());
        assert!(lib.add_tag(a, "a\u{7}b").is_err());
        assert!(matches!(lib.add_tag(999, "ok"), Err(Error::NotFound(999))));
        for i in 0..MAX_TAGS_PER_TRACK {
            lib.add_tag(a, &format!("t{i}")).unwrap();
        }
        assert!(lib.add_tag(a, "one too many").is_err());
        assert_eq!(lib.get(a).unwrap().unwrap().tags.len(), MAX_TAGS_PER_TRACK);
    }

    #[test]
    fn kinds_filter_and_roundtrip() {
        let lib = Library::open_in_memory().unwrap();
        let a = id_of(lib.add_track(&nt("/m/a.wav", "h1", "Alpha")).unwrap());
        lib.add_track(&nt("/m/b.wav", "h2", "Beta")).unwrap();
        lib.set_kind(a, Kind::Jingle).unwrap();
        let j = lib
            .search(&Query {
                kind: Some(Kind::Jingle),
                ..Query::default()
            })
            .unwrap();
        assert_eq!(j.len(), 1);
        assert_eq!(j[0].kind, Kind::Jingle);
        assert!(lib.set_kind(77, Kind::Sample).is_err());
    }

    #[test]
    fn same_hash_or_path_is_a_duplicate() {
        let lib = Library::open_in_memory().unwrap();
        let first = lib.add_track(&nt("/nonexistent/a.wav", "h1", "A")).unwrap();
        let AddOutcome::Added(id) = first else {
            panic!("{first:?}")
        };
        assert_eq!(
            lib.add_track(&nt("/nonexistent/a.wav", "h1", "A")).unwrap(),
            AddOutcome::Duplicate(id)
        );
        // Anderer Pfad, gleicher Inhalt, alter Pfad existiert nicht: Eintrag folgt der Datei.
        assert_eq!(
            lib.add_track(&nt("/nonexistent/moved.wav", "h1", "A"))
                .unwrap(),
            AddOutcome::Updated(id)
        );
        assert_eq!(lib.count().unwrap(), 1);
        assert_eq!(lib.get(id).unwrap().unwrap().path, "/nonexistent/moved.wav");
        assert_eq!(lib.search(&q("moved")).unwrap().len(), 1);
        // Gleicher Pfad, neuer Inhalt: aktualisiert.
        assert_eq!(
            lib.add_track(&nt("/nonexistent/moved.wav", "h2", "A"))
                .unwrap(),
            AddOutcome::Updated(id)
        );
        assert_eq!(lib.count().unwrap(), 1);
    }

    #[test]
    fn existing_copy_stays_a_duplicate() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.wav");
        std::fs::write(&a, b"x").unwrap();
        let lib = Library::open_in_memory().unwrap();
        lib.add_track(&nt(a.to_str().unwrap(), "h1", "A")).unwrap();
        let r = lib
            .add_track(&nt("/nonexistent/copy.wav", "h1", "A"))
            .unwrap();
        assert!(matches!(r, AddOutcome::Duplicate(_)));
        assert_eq!(lib.count().unwrap(), 1);
    }

    #[test]
    fn migration_from_v1_keeps_data_and_builds_the_index() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("lib.sqlite3");
        {
            // Datenbank so, wie Schema 1 sie anlegte.
            let c = Connection::open(&db).unwrap();
            c.execute_batch(V1).unwrap();
            c.execute(
                "INSERT INTO tracks(path, hash, title, duration, kind, added) \
                 VALUES ('/m/old.wav', 'hh', 'Alter Titel', 2.5, 'jingle', 7)",
                [],
            )
            .unwrap();
            c.pragma_update(None, "user_version", 1).unwrap();
        }
        let lib = Library::open(&db).unwrap();
        assert_eq!(lib.schema_version().unwrap(), SCHEMA_VERSION);
        let r = lib.search(&q("alter")).unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].kind, Kind::Jingle);
        assert_eq!(r[0].duration, 2.5);
        lib.add_tag(r[0].id, "alt").unwrap();
        drop(lib);
        // Zweites Öffnen ändert nichts mehr.
        let lib = Library::open(&db).unwrap();
        assert_eq!(lib.count().unwrap(), 1);
        assert_eq!(lib.get(r[0].id).unwrap().unwrap().tags, ["alt"]);
    }

    #[test]
    fn newer_database_is_refused_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("lib.sqlite3");
        {
            let c = Connection::open(&db).unwrap();
            c.pragma_update(None, "user_version", SCHEMA_VERSION + 1)
                .unwrap();
        }
        assert!(matches!(
            Library::open(&db),
            Err(Error::TooNew(v)) if v == SCHEMA_VERSION + 1
        ));
    }

    #[test]
    fn transaction_rolls_back_on_error() {
        let lib = Library::open_in_memory().unwrap();
        let r: Result<()> = lib.transaction(|l| {
            l.add_track(&nt("/m/a.wav", "h1", "A"))?;
            Err(Error::Invalid("nein".into()))
        });
        assert!(r.is_err());
        assert_eq!(lib.count().unwrap(), 0);
        assert_eq!(lib.search(&q("a")).unwrap().len(), 0);
    }

    #[test]
    fn removed_track_leaves_search_tags_and_count() {
        let lib = Library::open_in_memory().unwrap();
        let a = id_of(lib.add_track(&nt("/m/a.wav", "ha", "Sommer")).unwrap());
        let b = id_of(lib.add_track(&nt("/m/b.wav", "hb", "Winter")).unwrap());
        lib.add_tag(a, "Hit").unwrap();
        lib.remove_track(a).unwrap();
        assert_eq!(lib.count().unwrap(), 1);
        assert!(lib.search(&q("sommer")).unwrap().is_empty());
        assert!(lib.all_tags().unwrap().is_empty());
        assert_eq!(lib.search(&q("")).unwrap()[0].id, b);
        assert!(matches!(lib.remove_track(a), Err(Error::NotFound(_))));
        // Wieder importierbar wie neu.
        assert!(matches!(
            lib.add_track(&nt("/m/a.wav", "ha", "Sommer")).unwrap(),
            AddOutcome::Added(_)
        ));
    }

    #[test]
    fn sort_by_columns_with_unmeasured_last() {
        let lib = Library::open_in_memory().unwrap();
        let mut ids = Vec::new();
        for (p, h, t, d) in [
            ("/a", "1", "Bravo", 30.0),
            ("/b", "2", "alpha", 10.0),
            ("/c", "3", "Charlie", 20.0),
        ] {
            let mut n = nt(p, h, t);
            n.duration = d;
            ids.push(id_of(lib.add_track(&n).unwrap()));
        }
        let with_bpm = |bpm| Analysis {
            bpm: Some(bpm),
            ..Analysis::default()
        };
        lib.set_analysis(ids[0], "1", &with_bpm(120.0)).unwrap();
        lib.set_analysis(ids[2], "3", &with_bpm(90.0)).unwrap();
        let titles = |sort, desc| -> Vec<String> {
            lib.search(&Query {
                sort,
                desc,
                ..Query::default()
            })
            .unwrap()
            .into_iter()
            .map(|t| t.title)
            .collect()
        };
        assert_eq!(
            titles(SortBy::Relevance, false),
            ["alpha", "Bravo", "Charlie"]
        );
        assert_eq!(titles(SortBy::Title, true), ["Charlie", "Bravo", "alpha"]);
        assert_eq!(
            titles(SortBy::Duration, false),
            ["alpha", "Charlie", "Bravo"]
        );
        assert_eq!(titles(SortBy::Bpm, false), ["Charlie", "Bravo", "alpha"]);
        assert_eq!(titles(SortBy::Bpm, true), ["Bravo", "Charlie", "alpha"]);
        // Mit Suchtext gewinnt die gewählte Spalte über die Treffergüte.
        let hits: Vec<String> = lib
            .search(&Query {
                text: "a".into(),
                sort: SortBy::Duration,
                desc: true,
                ..Query::default()
            })
            .unwrap()
            .into_iter()
            .map(|t| t.title)
            .collect();
        assert_eq!(
            hits,
            ["Bravo", "alpha"],
            "Pfad /a und Titel alpha, nach Dauer absteigend"
        );
    }
}
