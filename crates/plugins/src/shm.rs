//! Gemeinsamer Speicher zwischen der Oberfläche und dem Plugin-Prozess (Paket 33).
//!
//! Aufbau nach dem Muster von LMMS `RemotePlugin` (GPL-2.0-or-later, `include/RemotePluginBase.h`,
//! Commit bed447b): Audio läuft über einen gemeinsamen Speicherbereich, Steuerung über einen
//! eigenen Nachrichtenkanal. Hier ist der Nachrichtenkanal ein Briefkasten im selben Bereich; ein
//! Audio-Block ist ein Paar Zähler (`seq_sub`, `seq_done`). Übernommen ist die Struktur, kein Code.
//!
//! Die Datei liegt im Temp-Ordner und wird mit `memmap2` eingeblendet (läuft auf Linux und
//! Windows). Alle Felder sind Atomics oder `UnsafeCell`; wer schreiben darf, regelt das Protokoll:
//!
//! - Audio-Block: Die Oberfläche füllt die Puffer und erhöht dann `seq_sub` (Release). Der
//!   Plugin-Prozess rechnet, schreibt den Ausgang und setzt `seq_done = seq_sub` (Release). Solange
//!   `seq_done != seq_sub` gehört der Block dem Plugin-Prozess; die Oberfläche fasst ihn nicht an.
//! - Briefkasten: Die Oberfläche schreibt Anfrage und erhöht `mb_req`; der Plugin-Prozess schreibt
//!   die Antwort und setzt `mb_rsp = mb_req`. Es läuft höchstens eine Anfrage zugleich.
//! - Ringe: ein Erzeuger, ein Verbraucher, je ein Zähler.

use std::cell::UnsafeCell;
use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use memmap2::MmapMut;
use woa_model::plugin::MAX_PLUGIN_STATE_BYTES;

use crate::host::MAX_FRAMES;

/// Kennzahl am Anfang der Datei.
pub const MAGIC: u64 = 0x574f_4150_4c47_3033;
/// Plätze für Note-Events je Block.
pub const EV_CAP: usize = 2048;
/// Plätze für Reglerwerte je Block (vom Mixer).
pub const SET_CAP: usize = 512;
/// Plätze der Ringe (Zweierpotenz).
pub const RING_CAP: usize = 1024;
/// Größe des Briefkastens: ein ganzer Plugin-Zustand plus Kopf.
pub const MAILBOX_CAP: usize = MAX_PLUGIN_STATE_BYTES + 64 * 1024;
/// Höchste Zahl Regler, deren Wert im Speicher gespiegelt wird.
pub const MIRROR_CAP: usize = 4096;

/// Zustand des Plugin-Prozesses.
pub const STATE_STARTING: u32 = 0;
/// Bereit.
pub const STATE_READY: u32 = 1;
/// Laden fehlgeschlagen (Text im Briefkasten).
pub const STATE_ERROR: u32 = 2;

/// Ein Note-Event im Speicher (16 Byte).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct RawEvent {
    /// Versatz im Block.
    pub offset: u32,
    /// Art (siehe `remote::encode_event`).
    pub tag: u32,
    /// Erster Wert (Note, Programm, Kanal …).
    pub a: i32,
    /// Zweiter Wert.
    pub b: i32,
}

/// Ein Reglerwert im Speicher (16 Byte).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct RawSet {
    /// CLAP-Kennung.
    pub id: u32,
    /// Füllbytes.
    pub pad: u32,
    /// Wert.
    pub value: f64,
}

/// Eine Rückmeldung aus der Plugin-Oberfläche (24 Byte).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct RawEdit {
    /// 0 = ein Regler, 1 = ganzer Zustand.
    pub kind: u32,
    /// Reglerkennung.
    pub id: u32,
    /// Wert vor der Änderung.
    pub old: f64,
    /// Wert danach.
    pub new: f64,
}

/// Ringpuffer mit einem Erzeuger und einem Verbraucher im gemeinsamen Speicher.
#[repr(C)]
pub struct Ring<T: Copy> {
    head: AtomicU32,
    tail: AtomicU32,
    items: UnsafeCell<[T; RING_CAP]>,
}

impl<T: Copy> Ring<T> {
    /// Legt einen Eintrag ab; `false`, wenn voll (kein Warten). Nur der Erzeuger.
    pub fn push(&self, item: T) -> bool {
        let head = self.head.load(Ordering::Relaxed);
        let tail = self.tail.load(Ordering::Acquire);
        if head.wrapping_sub(tail) as usize >= RING_CAP {
            return false;
        }
        // SAFETY: Der Platz `head % RING_CAP` gehört dem Erzeuger, bis `head` erhöht ist;
        // der Verbraucher liest nur Plätze unterhalb von `head`.
        unsafe {
            let base = self.items.get().cast::<T>();
            base.add(head as usize % RING_CAP).write(item);
        }
        self.head.store(head.wrapping_add(1), Ordering::Release);
        true
    }

    /// Nimmt den ältesten Eintrag. Nur der Verbraucher.
    pub fn pop(&self) -> Option<T> {
        let tail = self.tail.load(Ordering::Relaxed);
        let head = self.head.load(Ordering::Acquire);
        if head == tail {
            return None;
        }
        // SAFETY: Der Platz liegt unterhalb von `head`, der Erzeuger schreibt ihn nicht mehr, bis
        // `tail` ihn freigibt.
        let item = unsafe {
            self.items
                .get()
                .cast::<T>()
                .add(tail as usize % RING_CAP)
                .read()
        };
        self.tail.store(tail.wrapping_add(1), Ordering::Release);
        Some(item)
    }

    /// Leert den Ring (nur wenn kein anderer Thread ihn benutzt).
    pub fn clear(&self) {
        self.tail
            .store(self.head.load(Ordering::Acquire), Ordering::Release);
    }
}

/// Der gemeinsame Bereich. Wird nie auf dem Stapel angelegt, nur über [`Mapping`] eingeblendet.
#[repr(C)]
pub struct Shared {
    /// [`MAGIC`].
    pub magic: AtomicU64,
    /// Lebenszeichen der Oberfläche (zählt hoch); bleibt es aus, beendet sich der Plugin-Prozess.
    pub beat: AtomicU64,
    /// Oberfläche bittet um Beenden.
    pub shutdown: AtomicU32,
    /// Zustand des Plugin-Prozesses (`STATE_*`).
    pub state: AtomicU32,
    /// Latenz des Plugins in Abtastwerten.
    pub latency: AtomicU32,
    /// Prozesskennung des Plugin-Prozesses.
    pub child_pid: AtomicU32,
    /// 1, wenn das Plugin ein schwebendes Fenster anbietet.
    pub has_gui: AtomicU32,
    /// 1, solange das Fenster offen ist.
    pub gui_open: AtomicU32,
    /// 1, wenn das Plugin beim Rechnen ausgefallen ist.
    pub failed: AtomicU32,
    /// Abtastrate (für den Plugin-Prozess, von der Oberfläche gesetzt).
    pub rate: AtomicU32,
    /// Block: Zähler der abgeschickten Blöcke.
    pub seq_sub: AtomicU64,
    /// Block: Zähler der fertigen Blöcke.
    pub seq_done: AtomicU64,
    /// Länge des Blocks in Abtastwerten.
    pub frames: AtomicU32,
    /// Zahl der Events im Block.
    pub n_events: AtomicU32,
    /// Zahl der Reglerwerte im Block.
    pub n_sets: AtomicU32,
    /// 1: vor dem Block alles zurücksetzen (Noten aus).
    pub flush: AtomicU32,
    /// Briefkasten: Zähler der Anfragen.
    pub mb_req: AtomicU64,
    /// Briefkasten: Zähler der Antworten.
    pub mb_rsp: AtomicU64,
    /// Briefkasten: Art der Anfrage.
    pub mb_tag: AtomicU32,
    /// Briefkasten: Länge der Anfrage oder Antwort in Bytes.
    pub mb_len: AtomicU32,
    /// Briefkasten: 0 = gut, 1 = Fehler (Antwort ist ein Text).
    pub mb_status: AtomicU32,
    events: UnsafeCell<[RawEvent; EV_CAP]>,
    sets: UnsafeCell<[RawSet; SET_CAP]>,
    in_l: UnsafeCell<[f32; MAX_FRAMES as usize]>,
    in_r: UnsafeCell<[f32; MAX_FRAMES as usize]>,
    out_l: UnsafeCell<[f32; MAX_FRAMES as usize]>,
    out_r: UnsafeCell<[f32; MAX_FRAMES as usize]>,
    /// Gespiegelte Reglerwerte (Bits von `f64`) in der Reihenfolge der Reglerliste.
    pub mirror: [AtomicU64; MIRROR_CAP],
    /// Reglerwerte von der Oberfläche zum Plugin (Rücksetzen nach „Rückgängig“).
    pub set_ring: Ring<RawSet>,
    /// Rückmeldungen aus dem Plugin-Fenster zur Oberfläche.
    pub edit_ring: Ring<RawEdit>,
}

// SAFETY: Alle veränderlichen Teile sind Atomics oder `UnsafeCell`, auf die das Protokoll (siehe
// Kopf der Datei) höchstens einen Schreiber zugleich zulässt.
unsafe impl Sync for Shared {}
// SAFETY: wie `Sync`; die Struktur wird nur über Zeiger in einem Mapping benutzt.
unsafe impl Send for Shared {}

macro_rules! raw_ptr {
    ($name:ident, $field:ident, $t:ty) => {
        /// Zeiger auf den Bereich; nur nach dem Protokoll lesen oder schreiben.
        pub fn $name(&self) -> *mut $t {
            self.$field.get().cast::<$t>()
        }
    };
}

impl Shared {
    raw_ptr!(events_ptr, events, RawEvent);
    raw_ptr!(sets_ptr, sets, RawSet);
    raw_ptr!(in_l_ptr, in_l, f32);
    raw_ptr!(in_r_ptr, in_r, f32);
    raw_ptr!(out_l_ptr, out_l, f32);
    raw_ptr!(out_r_ptr, out_r, f32);
}

/// Die eingeblendete Datei.
pub struct Mapping {
    map: MmapMut,
    path: PathBuf,
    owner: bool,
}

impl Mapping {
    fn total_len() -> usize {
        std::mem::size_of::<Shared>() + MAILBOX_CAP
    }

    /// Legt die Datei an (nur die Oberfläche) und blendet sie ein. Die Datei ist zuerst leer
    /// (spärlich), Seiten kosten erst beim Benutzen Speicher.
    pub fn create(dir: &Path, label: &str) -> std::io::Result<Self> {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.subsec_nanos());
        let name = format!(
            "woa-plugin-{}-{}-{}-{}.shm",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed),
            nanos,
            label
                .chars()
                .filter(|c| c.is_ascii_alphanumeric())
                .take(16)
                .collect::<String>()
        );
        let path = dir.join(name);
        let mut opts = OpenOptions::new();
        opts.read(true).write(true).create_new(true);
        // Nur der eigene Nutzer darf mitlesen (Audio, Plugin-Zustand); der Plugin-Prozess läuft
        // unter demselben Nutzer. Windows: der Temp-Ordner gehört dem Nutzer.
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let file = opts.open(&path)?;
        file.set_len(Self::total_len() as u64)?;
        // SAFETY: Die Datei gehört diesem Programm und dem Plugin-Prozess; niemand sonst
        // verkürzt sie. Zugriffe laufen nur über Atomics und das Protokoll.
        let map = unsafe { MmapMut::map_mut(&file)? };
        let m = Self {
            map,
            path,
            owner: true,
        };
        m.shared().magic.store(MAGIC, Ordering::Release);
        Ok(m)
    }

    /// Blendet eine vorhandene Datei ein (Plugin-Prozess).
    pub fn open(path: &Path) -> std::io::Result<Self> {
        let file = OpenOptions::new().read(true).write(true).open(path)?;
        if file.metadata()?.len() < Self::total_len() as u64 {
            return Err(std::io::Error::other("Datei zu kurz"));
        }
        // SAFETY: siehe `create`.
        let map = unsafe { MmapMut::map_mut(&file)? };
        let m = Self {
            map,
            path: path.to_path_buf(),
            owner: false,
        };
        if m.shared().magic.load(Ordering::Acquire) != MAGIC {
            return Err(std::io::Error::other("falsche Kennzahl"));
        }
        Ok(m)
    }

    /// Pfad der Datei.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Der gemeinsame Bereich.
    pub fn shared(&self) -> &Shared {
        // SAFETY: Das Mapping ist seitenausgerichtet, mindestens `size_of::<Shared>()` lang und
        // lebt, solange `self` lebt. Nullen sind für alle Felder gültig.
        unsafe { &*self.map.as_ptr().cast::<Shared>() }
    }

    /// Zeiger auf den Briefkasten (`MAILBOX_CAP` Bytes).
    pub fn mailbox_ptr(&self) -> *mut u8 {
        // SAFETY: Der Briefkasten beginnt hinter `Shared` und liegt innerhalb der Datei. Der
        // Zeiger kommt aus dem schreibbaren Mapping.
        unsafe { (self.map.as_ptr() as *mut u8).add(std::mem::size_of::<Shared>()) }
    }

    /// Setzt alles außer der Kennzahl zurück (vor dem Start eines neuen Plugin-Prozesses; es darf
    /// kein Prozess und kein Audio-Aufruf mehr darauf zugreifen).
    pub fn reset(&self) {
        let s = self.shared();
        for a in [
            &s.shutdown,
            &s.state,
            &s.latency,
            &s.child_pid,
            &s.has_gui,
            &s.gui_open,
            &s.failed,
            &s.frames,
            &s.n_events,
            &s.n_sets,
            &s.flush,
            &s.mb_tag,
            &s.mb_len,
            &s.mb_status,
        ] {
            a.store(0, Ordering::Relaxed);
        }
        s.seq_sub.store(0, Ordering::Relaxed);
        s.seq_done.store(0, Ordering::Relaxed);
        s.mb_req.store(0, Ordering::Relaxed);
        s.mb_rsp.store(0, Ordering::Relaxed);
        s.set_ring.clear();
        s.edit_ring.clear();
        s.beat.fetch_add(1, Ordering::Release);
        s.shutdown.store(0, Ordering::Release);
    }
}

impl Drop for Mapping {
    fn drop(&mut self) {
        if self.owner {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

/// Privates Arbeitsverzeichnis (Modus 0700) mit nicht vorhersehbarem Namen; wird beim Freigeben samt
/// Inhalt gelöscht. Für Ergebnisdateien des Suchprozesses.
pub struct PrivateDir {
    path: PathBuf,
}

impl PrivateDir {
    /// Legt `<parent>/woa-scan-<pid>-<zähler>-<nanos>` neu an (schlägt fehl, wenn es schon existiert,
    /// also auch bei einem vorab untergeschobenen Symlink).
    pub fn create(parent: &Path) -> std::io::Result<Self> {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let path = parent.join(format!(
            "woa-scan-{}-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed),
            nanos
        ));
        #[cfg_attr(not(unix), allow(unused_mut))]
        let mut b = std::fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            b.mode(0o700);
        }
        b.create(&path)?;
        Ok(Self { path })
    }

    /// Pfad des Verzeichnisses.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for PrivateDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// Kleiner Schreiber für Briefkasten-Nachrichten.
#[derive(Default)]
pub struct Writer(pub Vec<u8>);

impl Writer {
    /// Hängt `u32` an.
    pub fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    /// Hängt `f64` an.
    pub fn f64(&mut self, v: f64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    /// Hängt ein Byte an.
    pub fn u8(&mut self, v: u8) {
        self.0.push(v);
    }
    /// Hängt Bytes mit Länge an.
    pub fn bytes(&mut self, b: &[u8]) {
        self.u32(b.len() as u32);
        self.0.extend_from_slice(b);
    }
    /// Hängt Text an.
    pub fn str(&mut self, s: &str) {
        self.bytes(s.as_bytes());
    }
}

/// Leser für Briefkasten-Nachrichten; schlägt nie fehl, sondern liefert Nullwerte am Ende.
pub struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    /// Liest aus `data`.
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }
    fn take(&mut self, n: usize) -> &'a [u8] {
        let end = (self.pos + n).min(self.data.len());
        let s = &self.data[self.pos..end];
        self.pos = end;
        s
    }
    /// Liest `u32`.
    pub fn u32(&mut self) -> u32 {
        let b = self.take(4);
        <[u8; 4]>::try_from(b).map_or(0, u32::from_le_bytes)
    }
    /// Liest `f64`.
    pub fn f64(&mut self) -> f64 {
        let b = self.take(8);
        <[u8; 8]>::try_from(b).map_or(0.0, f64::from_le_bytes)
    }
    /// Liest ein Byte.
    pub fn u8(&mut self) -> u8 {
        self.take(1).first().copied().unwrap_or(0)
    }
    /// Liest Bytes mit Länge.
    pub fn bytes(&mut self) -> &'a [u8] {
        let n = self.u32() as usize;
        self.take(n)
    }
    /// Liest Text.
    pub fn string(&mut self) -> String {
        String::from_utf8_lossy(self.bytes()).into_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_keeps_order_and_reports_full() {
        let dir = std::env::temp_dir();
        let m = Mapping::create(&dir, "ring").expect("create");
        let s = m.shared();
        for i in 0..RING_CAP {
            assert!(s.set_ring.push(RawSet {
                id: i as u32,
                pad: 0,
                value: i as f64,
            }));
        }
        assert!(!s.set_ring.push(RawSet {
            id: 0,
            pad: 0,
            value: 0.0
        }));
        for i in 0..RING_CAP {
            assert_eq!(s.set_ring.pop().map(|r| r.id), Some(i as u32));
        }
        assert!(s.set_ring.pop().is_none());
    }

    #[test]
    fn second_mapping_sees_the_same_memory_and_file_is_removed() {
        let dir = std::env::temp_dir();
        let m = Mapping::create(&dir, "two").expect("create");
        let path = m.path().to_path_buf();
        let m2 = Mapping::open(&path).expect("open");
        m.shared().latency.store(77, Ordering::Release);
        assert_eq!(m2.shared().latency.load(Ordering::Acquire), 77);
        drop(m2);
        drop(m);
        assert!(!path.exists());
    }

    #[cfg(unix)]
    #[test]
    fn shared_memory_file_is_mode_0600() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir();
        let m = Mapping::create(&dir, "mode").expect("create");
        let mode = std::fs::metadata(m.path())
            .expect("meta")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[cfg(unix)]
    #[test]
    fn private_dir_is_mode_0700_unique_and_removed() {
        use std::os::unix::fs::PermissionsExt;
        let parent = std::env::temp_dir();
        let a = PrivateDir::create(&parent).expect("a");
        let b = PrivateDir::create(&parent).expect("b");
        assert_ne!(a.path(), b.path());
        let mode = std::fs::metadata(a.path())
            .expect("meta")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o700);
        let p = a.path().to_path_buf();
        drop(a);
        assert!(!p.exists());
    }

    #[test]
    fn writer_and_reader_round_trip() {
        let mut w = Writer::default();
        w.u32(7);
        w.f64(1.5);
        w.str("hä");
        w.u8(3);
        let mut r = Reader::new(&w.0);
        assert_eq!(r.u32(), 7);
        assert_eq!(r.f64(), 1.5);
        assert_eq!(r.string(), "hä");
        assert_eq!(r.u8(), 3);
        assert_eq!(r.u32(), 0);
    }
}
