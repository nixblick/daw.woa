//! Hintergrundanalyse der Bibliothek (Paket 25): je Titel Lautheit, True Peak, Tempo, Tonart,
//! Stille und Cue-Vorschläge. Ein eigener Thread arbeitet die Titel ohne Analyse nacheinander
//! ab (ein Titel gleichzeitig, wegen des Arbeitsspeichers) und schreibt in kleinen
//! Transaktionen; die Oberfläche liest solange weiter. Nie im Audio-Thread.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread::JoinHandle;

use woa_dsp::analysis::track::{self, TrackAnalysis};

use crate::{Analysis, Library, Result};

/// So viele Titel holt der Thread auf einmal aus der Datenbank.
const BATCH: usize = 50;

/// Fortschritt, von mehreren Threads lesbar.
#[derive(Debug, Default)]
pub struct Progress {
    pub total: AtomicUsize,
    pub done: AtomicUsize,
    /// Titel, die sich nicht lesen ließen (sie gelten trotzdem als erledigt).
    pub failed: AtomicUsize,
    pub cancel: AtomicBool,
    pub finished: AtomicBool,
    pub fatal: AtomicBool,
}

impl Progress {
    pub fn is_finished(&self) -> bool {
        self.finished.load(Ordering::Acquire)
    }
}

/// Laufende Analyse.
pub struct Job {
    pub progress: Arc<Progress>,
    thread: Option<JoinHandle<()>>,
}

impl Job {
    pub fn cancel(&self) {
        self.progress.cancel.store(true, Ordering::Release);
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        self.cancel();
        if let Some(t) = self.thread.take()
            && t.join().is_err()
        {
            tracing::error!("library analysis thread panicked");
        }
    }
}

/// Wandelt das Messergebnis in die gespeicherte Form.
pub fn to_analysis(a: &TrackAnalysis) -> Analysis {
    let silent = a.silence.all_silent;
    Analysis {
        bpm: a.tempo.map(|t| (t.bpm * 10.0).round() / 10.0),
        key: a.key.map(|k| k.name()),
        lufs: a.lufs,
        true_peak_db: a.true_peak_db,
        peak_db: a.peak_db,
        lead_silence: (!silent).then_some(a.silence.lead),
        tail_silence: (!silent).then_some(a.silence.tail),
        cue_in: a.cues.cue_in,
        fade_out: a.cues.fade_out,
        cue_out: a.cues.cue_out,
    }
}

/// Liest und misst eine Datei.
pub fn analyze_file(path: &Path) -> std::result::Result<Analysis, String> {
    let d = woa_engine::decode::decode_file(path)?;
    if d.frames() == 0 {
        return Err("leere Audiodatei".into());
    }
    let (l, r) = d.stereo();
    Ok(to_analysis(&track::analyze(&l, &r, d.rate)))
}

/// Analysiert alle offenen Titel von `lib` (synchron; der Hintergrund-Thread ruft das auf).
pub fn analyze_pending(lib: &Library, progress: &Progress) -> Result<()> {
    progress
        .total
        .store(lib.pending_analysis_count()?, Ordering::Release);
    loop {
        if progress.cancel.load(Ordering::Acquire) {
            break;
        }
        let batch = lib.pending_analysis(BATCH)?;
        if batch.is_empty() {
            break;
        }
        for (id, path, hash) in batch {
            if progress.cancel.load(Ordering::Acquire) {
                return Ok(());
            }
            let a = match analyze_file(Path::new(&path)) {
                Ok(a) => a,
                Err(e) => {
                    tracing::warn!(error = %e, "library analysis: file not analysed");
                    progress.failed.fetch_add(1, Ordering::Relaxed);
                    // Leer eintragen, damit der Titel nicht in jedem Lauf wiederkommt.
                    Analysis::default()
                }
            };
            lib.set_analysis(id, &hash, &a)?;
            progress.done.fetch_add(1, Ordering::Relaxed);
        }
    }
    Ok(())
}

/// Startet die Analyse der Datenbank `db` in einem Hintergrund-Thread.
pub fn start(db: &Path) -> Job {
    let progress = Arc::new(Progress::default());
    let p = Arc::clone(&progress);
    let db = db.to_path_buf();
    let spawned = std::thread::Builder::new()
        .name("woa-library-analysis".into())
        .spawn(move || {
            let run = || -> Result<()> {
                let lib = Library::open(&db)?;
                analyze_pending(&lib, &p)
            };
            if let Err(e) = run() {
                tracing::error!(error = %e, "library analysis failed");
                p.fatal.store(true, Ordering::Release);
            }
            p.finished.store(true, Ordering::Release);
        });
    let thread = match spawned {
        Ok(h) => Some(h),
        Err(e) => {
            tracing::error!(error = %e, "library analysis thread not started");
            progress.fatal.store(true, Ordering::Release);
            progress.finished.store(true, Ordering::Release);
            None
        }
    };
    Job { progress, thread }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import;
    use crate::{Kind, Query};
    use std::time::{Duration, Instant};

    const RATE: u32 = 8000;

    fn write_wav(path: &Path, samples: &[f32]) {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: RATE,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut w = hound::WavWriter::create(path, spec).unwrap();
        for s in samples {
            w.write_sample((s.clamp(-1.0, 1.0) * 32767.0) as i16)
                .unwrap();
        }
        w.finalize().unwrap();
    }

    fn sine(hz: f32, amp: f32, secs: f32) -> Vec<f32> {
        (0..(secs * RATE as f32) as usize)
            .map(|i| amp * (i as f32 * hz * std::f32::consts::TAU / RATE as f32).sin())
            .collect()
    }

    fn click_track(bpm: f64, secs: f64) -> Vec<f32> {
        let n = (secs * f64::from(RATE)) as usize;
        let mut x = vec![0.0f32; n];
        let period = 60.0 / bpm * f64::from(RATE);
        let mut t = 0.0;
        while (t as usize) < n {
            for i in 0..160 {
                if t as usize + i < n {
                    let env = (-(i as f32) / 30.0).exp();
                    x[t as usize + i] +=
                        0.8 * env * (i as f32 * 1500.0 * std::f32::consts::TAU / RATE as f32).sin();
                }
            }
            t += period;
        }
        x
    }

    fn wait(job: &Job) {
        let t0 = Instant::now();
        while !job.progress.is_finished() {
            assert!(t0.elapsed() < Duration::from_secs(600), "analysis hangs");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn import_dir(db: &Path, dir: &Path) {
        let imp = import::start(db, dir, Kind::Music);
        let t0 = Instant::now();
        while !imp.progress.is_finished() {
            assert!(t0.elapsed() < Duration::from_secs(300));
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn click_chord_and_silence_files_are_measured() {
        let dir = tempfile::tempdir().unwrap();
        let m = dir.path().join("m");
        std::fs::create_dir_all(&m).unwrap();
        write_wav(&m.join("klick.wav"), &click_track(120.0, 20.0));
        // C-Dur-Akkord mit einer Sekunde Stille vorn und zwei hinten.
        let mut chord: Vec<f32> = vec![0.0; RATE as usize];
        let body: Vec<f32> = (0..8 * RATE as usize)
            .map(|i| {
                [261.63f32, 329.63, 392.0]
                    .iter()
                    .map(|f| 0.2 * (i as f32 * f * std::f32::consts::TAU / RATE as f32).sin())
                    .sum()
            })
            .collect();
        chord.extend(body);
        chord.extend(vec![0.0; 2 * RATE as usize]);
        write_wav(&m.join("akkord.wav"), &chord);
        let db = dir.path().join("lib.sqlite3");
        import_dir(&db, &m);
        let lib = Library::open(&db).unwrap();
        assert_eq!(lib.pending_analysis_count().unwrap(), 2);

        let job = start(&db);
        wait(&job);
        assert!(!job.progress.fatal.load(Ordering::Relaxed));
        assert_eq!(lib.pending_analysis_count().unwrap(), 0);

        let all = lib
            .search(&Query {
                limit: 10,
                ..Query::default()
            })
            .unwrap();
        let by = |n: &str| all.iter().find(|t| t.title.contains(n)).unwrap();
        let k = by("klick").analysis.clone().unwrap();
        assert!((k.bpm.unwrap() - 120.0).abs() <= 1.0, "{:?}", k.bpm);
        assert!(k.lufs.is_some() && k.true_peak_db.is_some());
        let a = by("akkord").analysis.clone().unwrap();
        assert_eq!(a.key.as_deref(), Some("C-Dur"));
        assert!(a.bpm.is_none());
        assert!((a.lead_silence.unwrap() - 1.0).abs() < 0.05);
        assert!((a.tail_silence.unwrap() - 2.0).abs() < 0.05);
        assert!((a.cue_in.unwrap() - 0.98).abs() < 0.05, "{:?}", a.cue_in);
        assert!((a.cue_out.unwrap() - 9.05).abs() < 0.1, "{:?}", a.cue_out);
    }

    /// EBU Tech 3341, Fall 1: 1-kHz-Sinus mit −23 dBFS auf beiden Kanälen misst −23,0 LUFS.
    #[test]
    fn ebu_reference_sine_through_the_file_path() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("ebu.wav");
        write_wav(&p, &sine(1000.0, 10f32.powf(-23.0 / 20.0), 6.0));
        let a = analyze_file(&p).unwrap();
        assert!((a.lufs.unwrap() + 23.0).abs() < 0.1, "{:?}", a.lufs);
        assert!(
            (a.true_peak_db.unwrap() + 23.0).abs() < 0.3,
            "{:?}",
            a.true_peak_db
        );
    }

    #[test]
    fn broken_file_is_marked_and_not_retried() {
        let dir = tempfile::tempdir().unwrap();
        let m = dir.path().join("m");
        std::fs::create_dir_all(&m).unwrap();
        write_wav(&m.join("ok.wav"), &sine(440.0, 0.3, 1.0));
        let db = dir.path().join("lib.sqlite3");
        import_dir(&db, &m);
        let lib = Library::open(&db).unwrap();
        std::fs::remove_file(m.join("ok.wav")).unwrap();
        let job = start(&db);
        wait(&job);
        assert_eq!(job.progress.failed.load(Ordering::Relaxed), 1);
        assert_eq!(lib.pending_analysis_count().unwrap(), 0);
        let t = lib.search(&Query::default()).unwrap();
        assert_eq!(t[0].analysis, Some(Analysis::default()));
    }

    #[test]
    fn changed_content_makes_the_title_pending_again() {
        let lib = Library::open_in_memory().unwrap();
        let nt = |hash: &str| crate::NewTrack {
            path: "/x/a.wav".into(),
            hash: hash.into(),
            title: "a".into(),
            duration: 1.0,
            kind: Kind::Music,
        };
        let id = match lib.add_track(&nt("h1")).unwrap() {
            crate::AddOutcome::Added(id) => id,
            o => panic!("{o:?}"),
        };
        let a = Analysis {
            bpm: Some(100.0),
            ..Analysis::default()
        };
        // Falscher Hash: nichts gespeichert.
        assert!(!lib.set_analysis(id, "alt", &a).unwrap());
        assert!(lib.set_analysis(id, "h1", &a).unwrap());
        assert_eq!(lib.get(id).unwrap().unwrap().analysis, Some(a));
        lib.add_track(&nt("h2")).unwrap();
        assert!(lib.get(id).unwrap().unwrap().analysis.is_none());
        assert_eq!(lib.pending_analysis_count().unwrap(), 1);
        lib.clear_analysis(id).unwrap();
        assert!(lib.clear_analysis(999).is_err());
    }

    #[test]
    fn migration_from_v2_keeps_tracks() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("l.sqlite3");
        {
            let c = rusqlite::Connection::open(&db).unwrap();
            c.execute_batch(crate::V1).unwrap();
            c.execute_batch(crate::V2).unwrap();
            c.execute(
                "INSERT INTO tracks(path, hash, title) VALUES ('/a.wav','h','Alt')",
                [],
            )
            .unwrap();
            c.pragma_update(None, "user_version", 2).unwrap();
        }
        let lib = Library::open(&db).unwrap();
        assert_eq!(lib.schema_version().unwrap(), crate::SCHEMA_VERSION);
        let t = lib.by_path("/a.wav").unwrap().unwrap();
        assert_eq!(t.title, "Alt");
        assert!(t.analysis.is_none());
        assert_eq!(lib.pending_analysis_count().unwrap(), 1);
    }

    /// Abnahme: 1.000 erzeugte Titel mit sehr verschiedener Lautstärke werden im Hintergrund
    /// analysiert, ohne Absturz; mit dem Gain aus der Analyse liegen alle auf dem Ziel.
    #[test]
    fn thousand_tracks_are_analysed_and_levelled() {
        let dir = tempfile::tempdir().unwrap();
        let m = dir.path().join("m");
        std::fs::create_dir_all(&m).unwrap();
        for n in 0..1000usize {
            // Lautstärke von etwa −40 bis −12 dBFS, jede Datei anders (sonst Doppelte).
            let amp = 0.01 + 0.2 * ((n % 20) as f32 / 20.0);
            write_wav(
                &m.join(format!("t{n:04}.wav")),
                &sine(200.0 + n as f32, amp, 1.0 + (n % 5) as f32 * 0.1),
            );
        }
        let db = dir.path().join("lib.sqlite3");
        import_dir(&db, &m);
        let lib = Library::open(&db).unwrap();
        assert_eq!(lib.count().unwrap(), 1000);

        let t0 = Instant::now();
        let job = start(&db);
        wait(&job);
        eprintln!("1000 Titel analysiert in {:?}", t0.elapsed());
        assert!(!job.progress.fatal.load(Ordering::Relaxed));
        assert_eq!(job.progress.done.load(Ordering::Relaxed), 1000);
        assert_eq!(job.progress.failed.load(Ordering::Relaxed), 0);
        assert_eq!(lib.pending_analysis_count().unwrap(), 0);

        let all = lib
            .search(&Query {
                limit: 5000,
                ..Query::default()
            })
            .unwrap();
        assert_eq!(all.len(), 1000);
        let target = -23.0;
        for t in &all {
            let a = t.analysis.as_ref().expect("analysed");
            let lufs = a.lufs.expect("lufs");
            let gain = a.gain_db(target);
            // Nach dem Gain liegt die Lautheit auf dem Ziel (Spitzengrenze greift bei Sinus
            // erst weit darüber nicht ein: Sinus hat 3 dB Spitzenabstand).
            assert!(
                (lufs + gain - target).abs() < 0.05,
                "{}: {lufs} + {gain}",
                t.title
            );
        }
    }
}
