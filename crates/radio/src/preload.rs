//! Vorab-Dekodieren (#595): ein Hintergrund-Thread liest und misst die Titel, die als
//! Nächstes gespielt werden (Cue-Titel, nächster Titel). Der Control-Thread nimmt den fertigen
//! Puffer, statt beim Drücken von „Start“ selbst zu dekodieren und alle anderen Befehle
//! (Deck, Carts, Stopp) so lange warten zu lassen.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::audio::{Measured, Stereo, file_stamp, load_stereo, measure};

/// Höchstens so viele fertige Titel liegen im Speicher (Cue-Titel, nächster, ein Reserve).
const MAX_READY: usize = 3;

/// So lange wartet der Sendeablauf auf einen laufenden Vorab-Auftrag, danach lädt er selbst
/// (Thread hängt oder ist tot).
const PENDING_MAX: std::time::Duration = std::time::Duration::from_secs(30);

/// Ein fertig dekodierter und gemessener Titel.
pub struct Preloaded {
    pub stereo: Stereo,
    pub measured: Measured,
    /// Dauer von Dekodieren und Messen in ms (fürs Log).
    pub decode_ms: u64,
    pub measure_ms: u64,
}

enum Slot {
    Pending(Instant),
    Ready {
        stamp: Option<String>,
        item: Box<Preloaded>,
    },
}

struct Cache {
    slots: HashMap<PathBuf, Slot>,
    /// Reihenfolge der Anforderung, älteste vorn.
    order: Vec<PathBuf>,
}

/// Cache plus Thread. Der Thread startet erst bei der ersten Anforderung und endet, wenn
/// der Controller verschwindet.
pub struct Preloader {
    rate: u32,
    cache: Arc<Mutex<Cache>>,
    tx: Option<Sender<PathBuf>>,
}

impl Preloader {
    pub fn new(rate: u32) -> Self {
        Self {
            rate,
            cache: Arc::new(Mutex::new(Cache {
                slots: HashMap::new(),
                order: Vec::new(),
            })),
            tx: None,
        }
    }

    /// Fordert `path` an, falls er nicht schon angefordert oder fertig ist.
    pub fn request(&mut self, path: &Path) {
        {
            let Ok(mut c) = self.cache.lock() else {
                return;
            };
            if c.slots.contains_key(path) {
                return;
            }
            // Platz schaffen: älteste angeforderte Einträge raus (laufende Aufträge
            // landen dann nirgends, siehe Thread).
            while c.order.len() >= MAX_READY {
                let old = c.order.remove(0);
                c.slots.remove(&old);
            }
            c.slots
                .insert(path.to_path_buf(), Slot::Pending(Instant::now()));
            c.order.push(path.to_path_buf());
        }
        if self.tx.is_none() {
            self.tx = self.spawn();
        }
        if let Some(tx) = &self.tx
            && tx.send(path.to_path_buf()).is_err()
        {
            // Thread beendet: beim nächsten Mal neu starten.
            self.tx = None;
            if let Ok(mut c) = self.cache.lock() {
                c.slots.remove(path);
                c.order.retain(|p| p != path);
            }
        }
    }

    fn spawn(&self) -> Option<Sender<PathBuf>> {
        let (tx, rx) = channel::<PathBuf>();
        let (cache, rate) = (Arc::clone(&self.cache), self.rate);
        let spawned = std::thread::Builder::new()
            .name("woa-radio-preload".into())
            .spawn(move || {
                for p in rx {
                    // Schon verdrängt: nicht mehr dekodieren.
                    let wanted = cache.lock().is_ok_and(|c| c.slots.contains_key(&p));
                    if !wanted {
                        continue;
                    }
                    let stamp = file_stamp(&p);
                    let t0 = Instant::now();
                    let loaded = load_stereo(&p, rate);
                    let decode_ms = t0.elapsed().as_millis() as u64;
                    let Ok(mut c) = cache.lock() else {
                        return;
                    };
                    match loaded {
                        Ok(stereo) => {
                            drop(c);
                            let t1 = Instant::now();
                            let measured = measure(&stereo, rate);
                            let measure_ms = t1.elapsed().as_millis() as u64;
                            let Ok(mut c) = cache.lock() else {
                                return;
                            };
                            if let Some(slot) = c.slots.get_mut(&p) {
                                *slot = Slot::Ready {
                                    stamp,
                                    item: Box::new(Preloaded {
                                        stereo,
                                        measured,
                                        decode_ms,
                                        measure_ms,
                                    }),
                                };
                            }
                        }
                        Err(e) => {
                            // Der Fehler wird beim echten Laden gemeldet.
                            tracing::debug!(error = %e, "preload failed");
                            c.slots.remove(&p);
                            c.order.retain(|q| q != &p);
                        }
                    }
                }
            });
        match spawned {
            Ok(_) => Some(tx),
            Err(e) => {
                tracing::warn!(error = %e, "preload thread failed");
                None
            }
        }
    }

    /// Nimmt den fertigen Titel heraus. `None`: nicht (mehr) fertig oder die Datei hat sich
    /// geändert; dann lädt der Aufrufer selbst.
    pub fn take(&mut self, path: &Path) -> Option<Preloaded> {
        let mut c = self.cache.lock().ok()?;
        if !matches!(c.slots.get(path), Some(Slot::Ready { .. })) {
            return None;
        }
        c.order.retain(|p| p != path);
        match c.slots.remove(path) {
            Some(Slot::Ready { stamp, item }) if stamp == file_stamp(path) => Some(*item),
            _ => None,
        }
    }

    /// Wird `path` gerade vorab geladen (höchstens [`PENDING_MAX`] lang gilt das)? Dann wartet
    /// der Aufrufer, statt selbst zu dekodieren und den Control-Thread zu blockieren.
    pub fn is_pending(&self, path: &Path) -> bool {
        self.cache.lock().is_ok_and(
            |c| matches!(c.slots.get(path), Some(Slot::Pending(t)) if t.elapsed() < PENDING_MAX),
        )
    }

    /// Ist `path` fertig (ohne ihn herauszunehmen)?
    pub fn is_ready(&self, path: &Path) -> bool {
        self.cache
            .lock()
            .is_ok_and(|c| matches!(c.slots.get(path), Some(Slot::Ready { .. })))
    }
}
