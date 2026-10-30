//! Wunschtitel: ein beobachteter Ordner. Was dort hineinkommt, steht in der Liste und lässt sich
//! mit einem Klick in die Playlist oder auf ein Deck legen. Kein Streamingdienst, nur Dateien.
//!
//! Ein kleiner Hintergrundfaden liest den Ordner jede Sekunde (ein `read_dir`, keine
//! Abhängigkeit, kaum Last auf einem alten Rechner); die Oberfläche holt nur den letzten
//! Stand ab. Offline testbar, ohne Fenster.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime};

/// So lange nach der letzten Änderung gilt eine Datei als „wird noch kopiert“.
const COPYING: Duration = Duration::from_secs(2);
/// So lange nach dem Auftauchen gilt ein Wunsch als neu.
const FRESH: Duration = Duration::from_secs(120);

/// Eine Datei im Wunschordner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WishFile {
    pub path: PathBuf,
    /// Dateiname ohne Endung.
    pub name: String,
    pub modified: SystemTime,
    /// Sie wird noch geschrieben (gerade geändert oder leer): noch nicht laden.
    pub copying: bool,
}

/// Der Stand des Ordners.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WishList {
    /// Neueste zuerst.
    pub files: Vec<WishFile>,
    /// Seit dem Start des Beobachters neu dazugekommen und noch jung.
    pub fresh: HashSet<PathBuf>,
    /// Meldung, wenn der Ordner fehlt oder nicht lesbar ist.
    pub error: Option<String>,
}

/// Liest den Ordner: nur Audiodateien, die WOA lesen kann, keine versteckten, neueste zuerst.
pub fn scan(dir: &Path, now: SystemTime) -> Result<Vec<WishFile>, String> {
    let rd = std::fs::read_dir(dir)
        .map_err(|e| format!("Der Wunschordner {} ist nicht lesbar ({e}).", dir.display()))?;
    let mut files = Vec::new();
    for entry in rd.flatten() {
        let path = entry.path();
        let Some(file_name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if file_name.starts_with('.') || !woa_engine::decode::is_readable(&path) {
            continue;
        }
        let Ok(meta) = entry.metadata() else { continue };
        if !meta.is_file() {
            continue;
        }
        let modified = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        let young = now.duration_since(modified).is_ok_and(|d| d < COPYING);
        files.push(WishFile {
            name: path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default(),
            copying: young || meta.len() == 0,
            modified,
            path,
        });
    }
    files.sort_by(|a, b| b.modified.cmp(&a.modified).then(a.name.cmp(&b.name)));
    Ok(files)
}

/// Beobachter eines Ordners.
pub struct WishWatcher {
    dir: PathBuf,
    shared: Arc<Mutex<WishList>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl WishWatcher {
    /// Legt den Ordner an, wenn er fehlt, und beginnt zu beobachten (`every`: Abstand der Läufe).
    pub fn start(dir: PathBuf, every: Duration) -> Self {
        let mut list = WishList::default();
        if let Err(e) = std::fs::create_dir_all(&dir) {
            list.error = Some(format!(
                "Der Wunschordner {} lässt sich nicht anlegen ({e}).",
                dir.display()
            ));
        }
        let shared = Arc::new(Mutex::new(list));
        let stop = Arc::new(AtomicBool::new(false));
        let thread = {
            let (shared, stop, dir) = (Arc::clone(&shared), Arc::clone(&stop), dir.clone());
            std::thread::Builder::new()
                .name("wish-folder".into())
                .spawn(move || watch(&dir, every, &shared, &stop))
                .ok()
        };
        Self {
            dir,
            shared,
            stop,
            thread,
        }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Letzter Stand.
    pub fn list(&self) -> WishList {
        self.shared.lock().map(|l| l.clone()).unwrap_or_default()
    }
}

impl Drop for WishWatcher {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn watch(dir: &Path, every: Duration, shared: &Mutex<WishList>, stop: &AtomicBool) {
    // Was beim Start schon da ist, gilt nicht als neu.
    let mut seen: Option<HashSet<PathBuf>> = None;
    let mut first_seen: Vec<(PathBuf, Instant)> = Vec::new();
    while !stop.load(Ordering::Relaxed) {
        let mut next = WishList::default();
        match scan(dir, SystemTime::now()) {
            Ok(files) => {
                let known =
                    seen.get_or_insert_with(|| files.iter().map(|f| f.path.clone()).collect());
                for f in &files {
                    if known.insert(f.path.clone()) {
                        first_seen.push((f.path.clone(), Instant::now()));
                    }
                }
                first_seen
                    .retain(|(p, t)| t.elapsed() < FRESH && files.iter().any(|f| f.path == *p));
                next.fresh = first_seen.iter().map(|(p, _)| p.clone()).collect();
                next.files = files;
            }
            Err(e) => next.error = Some(e),
        }
        if let Ok(mut l) = shared.lock() {
            *l = next;
        }
        // In kleinen Schritten schlafen, damit das Beenden nicht wartet.
        let mut slept = Duration::ZERO;
        while slept < every && !stop.load(Ordering::Relaxed) {
            let step = Duration::from_millis(20).min(every - slept);
            std::thread::sleep(step);
            slept += step;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(dir: &Path, name: &str, bytes: usize, age: Duration) {
        let p = dir.join(name);
        std::fs::write(&p, vec![1u8; bytes]).unwrap();
        let f = std::fs::File::options().write(true).open(&p).unwrap();
        f.set_modified(SystemTime::now() - age).unwrap();
    }

    #[test]
    fn scan_lists_only_audio_newest_first() {
        let tmp = tempfile::tempdir().unwrap();
        let d = tmp.path();
        touch(d, "alt.mp3", 10, Duration::from_secs(600));
        touch(d, "neu.flac", 10, Duration::from_secs(60));
        touch(d, "Gross.WAV", 10, Duration::from_secs(300));
        touch(d, "notiz.txt", 10, Duration::from_secs(1));
        touch(d, ".versteckt.mp3", 10, Duration::from_secs(1));
        std::fs::create_dir(d.join("ordner.mp3")).unwrap();
        let files = scan(d, SystemTime::now()).unwrap();
        let names: Vec<&str> = files.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["neu", "Gross", "alt"]);
        assert!(files.iter().all(|f| !f.copying));
    }

    #[test]
    fn a_file_that_is_still_being_written_is_marked_and_an_empty_one_too() {
        let tmp = tempfile::tempdir().unwrap();
        touch(tmp.path(), "kopiere.mp3", 10, Duration::ZERO);
        touch(tmp.path(), "leer.mp3", 0, Duration::from_secs(60));
        let files = scan(tmp.path(), SystemTime::now()).unwrap();
        assert!(files.iter().all(|f| f.copying), "{files:?}");
    }

    #[test]
    fn missing_folder_gives_a_clear_message() {
        let tmp = tempfile::tempdir().unwrap();
        let e = scan(&tmp.path().join("weg"), SystemTime::now()).unwrap_err();
        assert!(e.contains("Wunschordner") && e.contains("weg"));
    }

    fn wait_for(w: &WishWatcher, ok: impl Fn(&WishList) -> bool) -> WishList {
        for _ in 0..300 {
            let l = w.list();
            if ok(&l) {
                return l;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("nichts erschienen: {:?}", w.list());
    }

    #[test]
    fn a_new_file_appears_by_itself_and_is_fresh_while_old_ones_are_not() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("wuensche");
        std::fs::create_dir_all(&dir).unwrap();
        touch(&dir, "schon-da.mp3", 10, Duration::from_secs(900));
        let w = WishWatcher::start(dir.clone(), Duration::from_millis(30));
        let l = wait_for(&w, |l| l.files.len() == 1);
        assert!(l.fresh.is_empty(), "vorhandene Dateien sind nicht neu");
        touch(&dir, "wunsch.mp3", 10, Duration::from_secs(5));
        let l = wait_for(&w, |l| l.files.len() == 2);
        assert!(l.fresh.contains(&dir.join("wunsch.mp3")));
        assert!(!l.fresh.contains(&dir.join("schon-da.mp3")));
        assert_eq!(l.files[0].name, "wunsch");
        std::fs::remove_file(dir.join("wunsch.mp3")).unwrap();
        wait_for(&w, |l| l.files.len() == 1 && l.fresh.is_empty());
    }

    #[test]
    fn a_missing_folder_is_created_and_dropping_the_watcher_stops_the_thread() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("neu").join("wuensche");
        let w = WishWatcher::start(dir.clone(), Duration::from_millis(20));
        assert!(dir.is_dir());
        let l = wait_for(&w, |l| l.error.is_none());
        assert!(l.files.is_empty());
        drop(w);
    }
}
