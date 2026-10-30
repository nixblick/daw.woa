//! Ordner-Import im Hintergrund: sucht lesbare Audiodateien, bildet je Datei die Inhaltskennung,
//! liest die Dauer und trägt sie ein. Gleiche Inhalte werden erkannt und nicht doppelt gespeichert.
//!
//! Der Hintergrund-Thread öffnet seine eigene Verbindung zur Datenbank; die Oberfläche liest
//! den Fortschritt aus [`Progress`] (Atomics) und braucht keine Sperre.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread::JoinHandle;

use crate::{AddOutcome, Kind, Library, NewTrack, Result, file_hash};

/// Tiefste Ordnerverschachtelung, die der Import betritt.
pub const MAX_DEPTH: usize = 16;
/// Mehr Dateien nimmt ein Import nicht an (Schutz vor Endlosbäumen).
pub const MAX_FILES: usize = 200_000;
/// So viele Dateien je Transaktion.
const BATCH: usize = 50;

/// Fortschritt eines Imports, lock-frei lesbar.
#[derive(Debug, Default)]
pub struct Progress {
    /// Gefundene Dateien (steht erst nach dem Suchen fest).
    pub total: AtomicUsize,
    /// Bearbeitete Dateien.
    pub done: AtomicUsize,
    pub added: AtomicUsize,
    /// Schon vorhanden (gleicher Inhalt).
    pub duplicates: AtomicUsize,
    /// Aktualisiert oder verschoben.
    pub updated: AtomicUsize,
    /// Nicht lesbar oder fehlerhaft.
    pub failed: AtomicUsize,
    pub finished: AtomicBool,
    pub cancel: AtomicBool,
    /// Der Zielordner konnte nicht gelesen werden, oder die Datenbank nicht geöffnet.
    pub fatal: AtomicBool,
}

impl Progress {
    pub fn is_finished(&self) -> bool {
        self.finished.load(Ordering::Acquire)
    }

    pub fn get(a: &AtomicUsize) -> usize {
        a.load(Ordering::Relaxed)
    }
}

/// Ein laufender Import.
pub struct Import {
    pub progress: Arc<Progress>,
    thread: Option<JoinHandle<()>>,
}

impl Import {
    /// Bricht ab; bereits eingetragene Titel bleiben.
    pub fn cancel(&self) {
        self.progress.cancel.store(true, Ordering::Release);
    }
}

impl Drop for Import {
    fn drop(&mut self) {
        self.cancel();
        if let Some(t) = self.thread.take()
            && t.join().is_err()
        {
            tracing::error!("library import thread panicked");
        }
    }
}

/// Sammelt lesbare Audiodateien unter `dir`. Symlinks werden nicht verfolgt.
pub fn scan(dir: &Path, cancel: &AtomicBool) -> std::io::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    let mut stack = vec![(dir.to_path_buf(), 0usize)];
    let mut first = true;
    while let Some((d, depth)) = stack.pop() {
        let rd = match std::fs::read_dir(&d) {
            Ok(r) => r,
            // Nur der oberste Ordner ist ein Fehler; unlesbare Unterordner überspringen wir.
            Err(e) if first => return Err(e),
            Err(e) => {
                tracing::warn!(error = %e, "library scan: folder skipped");
                continue;
            }
        };
        first = false;
        for entry in rd.flatten() {
            if cancel.load(Ordering::Relaxed) || out.len() >= MAX_FILES {
                return Ok(out);
            }
            let Ok(ft) = entry.file_type() else { continue };
            let p = entry.path();
            if ft.is_dir() {
                if depth < MAX_DEPTH {
                    stack.push((p, depth + 1));
                }
            } else if ft.is_file() && woa_engine::decode::is_readable(&p) {
                out.push(p);
            }
        }
    }
    out.sort();
    Ok(out)
}

/// Titel aus dem Dateinamen: ohne Endung, Unterstriche als Leerzeichen.
pub fn title_of(path: &Path) -> String {
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().replace('_', " "))
        .unwrap_or_default();
    let t = stem.trim();
    if t.is_empty() {
        "Ohne Titel".into()
    } else {
        t.into()
    }
}

/// Liest Kennung und Dauer einer Datei.
fn probe(path: &Path, kind: Kind) -> std::result::Result<NewTrack, String> {
    let path_str = path
        .to_str()
        .ok_or("Pfad ist kein gültiger Text")?
        .to_string();
    let hash = file_hash(path).map_err(|e| e.to_string())?;
    Ok(NewTrack {
        path: path_str,
        hash,
        title: title_of(path),
        duration: 0.0,
        kind,
    })
}

fn duration_of(path: &Path) -> std::result::Result<f64, String> {
    let d = woa_engine::decode::decode_file(path)?;
    Ok(d.frames() as f64 / f64::from(d.rate.max(1)))
}

/// Trägt `files` in `lib` ein (synchron; der Hintergrund-Thread ruft das auf).
pub fn import_files(
    lib: &Library,
    files: &[PathBuf],
    kind: Kind,
    progress: &Progress,
) -> Result<()> {
    progress.total.store(files.len(), Ordering::Release);
    for batch in files.chunks(BATCH) {
        if progress.cancel.load(Ordering::Acquire) {
            break;
        }
        // Dekodieren vor der Transaktion: sie hält die Datenbank nur kurz gesperrt.
        let mut ready: Vec<NewTrack> = Vec::with_capacity(batch.len());
        for path in batch {
            if progress.cancel.load(Ordering::Acquire) {
                break;
            }
            match probe(path, kind) {
                Ok(mut t) => {
                    // Schon bekannt (gleicher Pfad und Inhalt)? Dann kein Dekodieren nötig.
                    match lib.by_path(&t.path)? {
                        Some(old) if old.hash == t.hash => {
                            progress.duplicates.fetch_add(1, Ordering::Relaxed);
                            progress.done.fetch_add(1, Ordering::Relaxed);
                            continue;
                        }
                        _ => {}
                    }
                    match duration_of(path) {
                        Ok(d) if d > 0.0 => {
                            t.duration = d;
                            ready.push(t);
                        }
                        Ok(_) => {
                            tracing::warn!("library import: empty audio file skipped");
                            progress.failed.fetch_add(1, Ordering::Relaxed);
                            progress.done.fetch_add(1, Ordering::Relaxed);
                        }
                        Err(e) => {
                            tracing::warn!(error = %e, "library import: decode failed");
                            progress.failed.fetch_add(1, Ordering::Relaxed);
                            progress.done.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!(error = %e, "library import: file skipped");
                    progress.failed.fetch_add(1, Ordering::Relaxed);
                    progress.done.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
        lib.transaction(|l| {
            for t in &ready {
                match l.add_track(t)? {
                    AddOutcome::Added(_) => progress.added.fetch_add(1, Ordering::Relaxed),
                    AddOutcome::Duplicate(_) => progress.duplicates.fetch_add(1, Ordering::Relaxed),
                    AddOutcome::Updated(_) => progress.updated.fetch_add(1, Ordering::Relaxed),
                };
                progress.done.fetch_add(1, Ordering::Relaxed);
            }
            Ok(())
        })?;
    }
    Ok(())
}

/// Startet den Import von `dir` in die Datenbank `db` in einem Hintergrund-Thread.
pub fn start(db: &Path, dir: &Path, kind: Kind) -> Import {
    let progress = Arc::new(Progress::default());
    let p = Arc::clone(&progress);
    let db = db.to_path_buf();
    let dir = dir.to_path_buf();
    let spawned = std::thread::Builder::new()
        .name("woa-library-import".into())
        .spawn(move || {
            let run = || -> Result<()> {
                let lib = Library::open(&db)?;
                let files = scan(&dir, &p.cancel)?;
                import_files(&lib, &files, kind, &p)
            };
            if let Err(e) = run() {
                tracing::error!(error = %e, "library import failed");
                p.fatal.store(true, Ordering::Release);
            }
            p.finished.store(true, Ordering::Release);
        });
    let thread = match spawned {
        Ok(h) => Some(h),
        Err(e) => {
            tracing::error!(error = %e, "library import thread not started");
            progress.fatal.store(true, Ordering::Release);
            progress.finished.store(true, Ordering::Release);
            None
        }
    };
    Import { progress, thread }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Query;
    use std::time::{Duration, Instant};

    fn write_sine(path: &Path, hz: f32, secs: f32) {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 8000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut w = hound::WavWriter::create(path, spec).unwrap();
        for i in 0..(8000.0 * secs) as usize {
            let s = (i as f32 * hz * std::f32::consts::TAU / 8000.0).sin();
            w.write_sample((s * 12000.0) as i16).unwrap();
        }
        w.finalize().unwrap();
    }

    fn wait(imp: &Import) {
        let t0 = Instant::now();
        while !imp.progress.is_finished() {
            assert!(t0.elapsed() < Duration::from_secs(300), "import hangs");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn imports_1000_generated_files_in_the_background() {
        let dir = tempfile::tempdir().unwrap();
        let music = dir.path().join("musik");
        for sub in 0..10 {
            std::fs::create_dir_all(music.join(format!("album_{sub}"))).unwrap();
        }
        for n in 0..1000usize {
            // Jede Datei klingt anders (Frequenz und Länge), sonst wären sie Doppelte.
            let p = music
                .join(format!("album_{}", n % 10))
                .join(format!("titel_{n:04}.wav"));
            write_sine(&p, 200.0 + n as f32, 0.05 + (n % 7) as f32 * 0.01);
        }
        std::fs::write(music.join("notiz.txt"), "kein Audio").unwrap();
        std::fs::write(music.join("album_0").join("kaputt.wav"), b"RIFFnope").unwrap();

        let db = dir.path().join("lib.sqlite3");
        let t0 = Instant::now();
        let imp = start(&db, &music, Kind::Music);
        wait(&imp);
        let took = t0.elapsed();
        let p = &imp.progress;
        eprintln!("1000 Dateien importiert in {took:?}");
        assert!(!p.fatal.load(Ordering::Relaxed));
        assert_eq!(
            Progress::get(&p.total),
            1001,
            "1000 gute + 1 kaputte .wav, kein .txt"
        );
        assert_eq!(Progress::get(&p.added), 1000);
        assert_eq!(Progress::get(&p.failed), 1);
        assert_eq!(Progress::get(&p.duplicates), 0);
        assert_eq!(Progress::get(&p.done), 1001);
        assert!(took < Duration::from_secs(120), "zu langsam: {took:?}");

        let lib = Library::open(&db).unwrap();
        assert_eq!(lib.count().unwrap(), 1000);
        let hit = lib
            .search(&Query {
                text: "titel 0042".into(),
                ..Query::default()
            })
            .unwrap();
        assert_eq!(hit.len(), 1);
        assert!((hit[0].duration - 0.05).abs() < 0.02, "{}", hit[0].duration);
        // Ordnername im Pfad: genau die zehn Alben-Ordner-Treffer, 100 je Album.
        let album = lib
            .search(&Query {
                text: "album_3".into(),
                limit: 1000,
                ..Query::default()
            })
            .unwrap();
        assert_eq!(album.len(), 100);

        // Zweiter Import desselben Ordners: nichts Neues, alles als Doppelte erkannt.
        let again = start(&db, &music, Kind::Music);
        wait(&again);
        assert_eq!(Progress::get(&again.progress.added), 0);
        assert_eq!(Progress::get(&again.progress.duplicates), 1000);
        assert_eq!(lib.count().unwrap(), 1000);
    }

    #[test]
    fn copy_with_other_name_is_recognised_by_hash() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a");
        std::fs::create_dir_all(&a).unwrap();
        write_sine(&a.join("eins.wav"), 440.0, 0.2);
        let db = dir.path().join("lib.sqlite3");
        let imp = start(&db, &a, Kind::Jingle);
        wait(&imp);
        assert_eq!(Progress::get(&imp.progress.added), 1);

        std::fs::copy(a.join("eins.wav"), a.join("kopie.wav")).unwrap();
        let imp = start(&db, &a, Kind::Jingle);
        wait(&imp);
        assert_eq!(Progress::get(&imp.progress.added), 0);
        assert_eq!(Progress::get(&imp.progress.duplicates), 2);
        let lib = Library::open(&db).unwrap();
        assert_eq!(lib.count().unwrap(), 1);
        assert_eq!(lib.search(&Query::default()).unwrap()[0].kind, Kind::Jingle);
    }

    #[test]
    fn missing_folder_reports_fatal_not_panic() {
        let dir = tempfile::tempdir().unwrap();
        let imp = start(
            &dir.path().join("l.sqlite3"),
            &dir.path().join("nope"),
            Kind::Music,
        );
        wait(&imp);
        assert!(imp.progress.fatal.load(Ordering::Relaxed));
    }

    #[test]
    fn cancel_stops_early_and_keeps_what_was_added() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a");
        std::fs::create_dir_all(&a).unwrap();
        for n in 0..300usize {
            write_sine(&a.join(format!("t{n:03}.wav")), 100.0 + n as f32, 0.05);
        }
        let db = dir.path().join("lib.sqlite3");
        let imp = start(&db, &a, Kind::Music);
        imp.cancel();
        wait(&imp);
        let lib = Library::open(&db).unwrap();
        assert!(lib.count().unwrap() <= 300);
        assert_eq!(lib.count().unwrap(), Progress::get(&imp.progress.added));
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_folders_are_not_followed() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a");
        std::fs::create_dir_all(&a).unwrap();
        write_sine(&a.join("x.wav"), 300.0, 0.1);
        std::os::unix::fs::symlink(&a, a.join("loop")).unwrap();
        let files = scan(&a, &AtomicBool::new(false)).unwrap();
        assert_eq!(files.len(), 1);
    }

    #[test]
    fn titles_come_from_file_names() {
        assert_eq!(title_of(Path::new("/x/Mein_Lied.mp3")), "Mein Lied");
        assert_eq!(title_of(Path::new("/x/.wav")), ".wav");
    }
}
