//! Wellenform je Titel für die Party-Decks (Design-Runde 3): einmal im Hintergrund dekodieren,
//! auf [`PEAKS`] Spitzenwerte verdichten und in der Bibliothek ablegen (Tabelle `waveforms`,
//! Schlüssel ist der Inhalts-Hash). Ändert sich die Datei, passt der Hash nicht mehr und die
//! Wellenform wird neu gerechnet. Nie im Audio-Thread.

use std::path::Path;

use crate::{Library, Result};

/// So viele Spitzenwerte hat eine Wellenform.
pub const PEAKS: usize = 1000;

/// Verdichtet verschachtelte Samples auf `n` Spitzenwerte (Betrag, über alle Kanäle),
/// 0 = Stille, 255 = Vollaussteuerung. Weniger Frames als `n` ergeben weniger Werte.
pub fn peaks(samples: &[f32], channels: usize, n: usize) -> Vec<u8> {
    let ch = channels.max(1);
    let frames = samples.len() / ch;
    if frames == 0 || n == 0 {
        return Vec::new();
    }
    let n = n.min(frames);
    (0..n)
        .map(|i| {
            let a = i * frames / n * ch;
            let b = ((i + 1) * frames / n * ch).min(samples.len());
            let m =
                samples[a..b].iter().fold(
                    0.0f32,
                    |m, s| if s.is_finite() { m.max(s.abs()) } else { m },
                );
            (m.min(1.0) * 255.0).round() as u8
        })
        .collect()
}

/// Dekodiert `path` und verdichtet ihn auf [`PEAKS`] Werte.
pub fn compute(path: &Path) -> std::result::Result<Vec<u8>, String> {
    let d = woa_engine::decode::decode_file(path)?;
    Ok(peaks(&d.samples, d.channels, PEAKS))
}

/// Was ein Party-Deck zu seinem Titel braucht.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DeckInfo {
    /// Spitzenwerte (leer, wenn die Datei nicht lesbar ist).
    pub peaks: Vec<u8>,
    /// Tempo aus der Bibliotheksanalyse; `None` ohne (aktuelle) Analyse.
    pub bpm: Option<f64>,
    /// DJ-Paket: Beatgrid-Lage (Sekunde des ersten Schlags) und Tonart aus der Analyse.
    pub beat_offset: Option<f64>,
    /// Tonart als Anzeigename („a-Moll“).
    pub key: Option<String>,
    /// Inhalts-Hash (Schlüssel der DJ-Marken).
    pub hash: String,
    /// Hot Cues und Schleife.
    pub marks: crate::dj::DjMarks,
}

/// Wellenform und Tempo zu `path`: Tempo aus der Analyse, Wellenform aus dem Cache oder einmal
/// neu gerechnet und abgelegt. Für einen Hintergrund-Thread (dekodiert ggf. die ganze Datei).
pub fn deck_info(lib: &Library, path: &Path) -> std::result::Result<DeckInfo, String> {
    let key = path.to_string_lossy();
    let track = lib.by_path(&key).map_err(|e| e.to_string())?;
    let analysis = track.as_ref().and_then(|t| t.analysis.clone());
    let hash = match &track {
        Some(t) => t.hash.clone(),
        None => crate::file_hash(path).map_err(|e| e.to_string())?,
    };
    let marks = lib.dj_marks(&hash).unwrap_or_else(|e| {
        tracing::warn!(error = %e, "dj marks not readable");
        Default::default()
    });
    let peaks = match lib.waveform(&hash) {
        Ok(Some(p)) => p,
        _ => {
            let p = compute(path)?;
            if let Err(e) = lib.set_waveform(&hash, &p) {
                tracing::warn!(error = %e, "waveform not cached");
            }
            p
        }
    };
    Ok(DeckInfo {
        peaks,
        bpm: analysis.as_ref().and_then(|a| a.bpm),
        beat_offset: analysis.as_ref().and_then(|a| a.beat_offset),
        key: analysis.and_then(|a| a.key),
        hash,
        marks,
    })
}

impl Library {
    /// Gespeicherte Wellenform zum Inhalt `hash`.
    pub fn waveform(&self, hash: &str) -> Result<Option<Vec<u8>>> {
        use rusqlite::OptionalExtension;
        Ok(self
            .conn
            .query_row("SELECT peaks FROM waveforms WHERE hash = ?1", [hash], |r| {
                r.get(0)
            })
            .optional()?)
    }

    /// Legt die Wellenform zum Inhalt `hash` ab (ersetzt eine alte).
    pub fn set_waveform(&self, hash: &str, peaks: &[u8]) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO waveforms(hash, peaks) VALUES (?1, ?2)",
            rusqlite::params![hash, peaks],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peaks_take_the_loudest_sample_of_each_bucket() {
        // Stereo, 4 Frames, 2 Eimer: (0.1, -0.5), (0.2, 0.0) | (1.5, 0.0), (0.0, -0.25)
        let s = [0.1, -0.5, 0.2, 0.0, 1.5, 0.0, 0.0, -0.25];
        assert_eq!(peaks(&s, 2, 2), vec![128, 255]);
        assert!(peaks(&[], 2, 10).is_empty());
        assert_eq!(peaks(&[0.5, f32::NAN], 1, 10), vec![128, 0]);
        assert_eq!(peaks(&vec![0.0; 48_000], 1, PEAKS).len(), PEAKS);
    }

    #[test]
    fn deck_info_caches_waveform_and_reads_bpm() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("ton.wav");
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 8000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut w = hound::WavWriter::create(&f, spec).unwrap();
        for i in 0..8000 {
            w.write_sample(if i < 4000 { 16000i16 } else { 0 }).unwrap();
        }
        w.finalize().unwrap();
        let lib = Library::open(&dir.path().join(crate::DB_FILE)).unwrap();
        // Nicht in der Bibliothek: Wellenform ja, Tempo nein.
        let a = deck_info(&lib, &f).unwrap();
        assert_eq!(a.peaks.len(), PEAKS);
        assert!(a.peaks[0] > 100 && a.peaks[PEAKS - 1] == 0);
        assert_eq!(a.bpm, None);
        // Beim zweiten Mal aus dem Cache (Datei weg, Wellenform bleibt).
        let hash = crate::file_hash(&f).unwrap();
        assert_eq!(lib.waveform(&hash).unwrap(), Some(a.peaks.clone()));
        lib.set_waveform(&hash, &[7, 8]).unwrap();
        assert_eq!(deck_info(&lib, &f).unwrap().peaks, vec![7, 8]);
        // Mit Analyse: Tempo aus der Spalte bpm.
        let id = match lib
            .add_track(&crate::NewTrack {
                path: f.to_string_lossy().into_owned(),
                hash: hash.clone(),
                title: "Ton".into(),
                duration: 1.0,
                kind: crate::Kind::Music,
            })
            .unwrap()
        {
            crate::AddOutcome::Added(id) => id,
            o => panic!("{o:?}"),
        };
        let an = crate::Analysis {
            bpm: Some(124.0),
            beat_offset: Some(0.25),
            key: Some("a-Moll".into()),
            ..Default::default()
        };
        assert!(lib.set_analysis(id, &hash, &an).unwrap());
        let marks = crate::dj::DjMarks {
            hot: [Some(0.5), None, None, None],
            looped: None,
        };
        lib.set_dj_marks(&hash, &marks).unwrap();
        let info = deck_info(&lib, &f).unwrap();
        assert_eq!(info.bpm, Some(124.0));
        assert_eq!(info.beat_offset, Some(0.25));
        assert_eq!(info.key.as_deref(), Some("a-Moll"));
        assert_eq!(info.hash, hash);
        assert_eq!(info.marks, marks);
    }

    #[test]
    fn migration_from_v4_adds_waveforms() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join(crate::DB_FILE);
        {
            let c = rusqlite::Connection::open(&p).unwrap();
            for m in &crate::MIGRATIONS[..4] {
                c.execute_batch(m).unwrap();
            }
            c.pragma_update(None, "user_version", 4).unwrap();
        }
        let lib = Library::open(&p).unwrap();
        assert_eq!(lib.schema_version().unwrap(), crate::SCHEMA_VERSION);
        lib.set_waveform("h", &[1, 2, 3]).unwrap();
        assert_eq!(lib.waveform("h").unwrap(), Some(vec![1, 2, 3]));
    }
}
