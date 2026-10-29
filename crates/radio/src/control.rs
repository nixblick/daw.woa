//! Control-Seite des Sendeablaufs: Playlist-Zeiger, Vorladen, Carts, Mitschnitt.
//!
//! Läuft nie im Audio-Thread. Dekodiert Dateien, legt den nächsten Titel beim [`Mixer`]
//! bereit, schreibt den Mitschnitt aus dem Ring in eine WAV-Datei und gibt verworfene
//! Puffer über den `basedrop`-Collector frei ([`Controller::pump`] regelmäßig aufrufen).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering;

use basedrop::{Collector, Owned, Shared};

use crate::audio::{Stereo, load_stereo, measure};
use crate::level::db_to_gain;
use crate::mixer::{Cmd, Mixer, NONE, Status};
use crate::show::{CartEntry, PlaylistEntry, RadioShow};
use crate::{CARTS, RECORDINGS_DIR};

/// Kommandos im Flug zum Audio-Thread.
const QUEUE: usize = 256;
/// Sekunden Mitschnitt-Puffer zwischen Audio- und Control-Thread.
const RECORD_QUEUE_SECONDS: usize = 8;
/// Eine Mitschnitt-Datei je so vielen Sekunden (WAV-Grenze 4 GiB).
pub const RECORD_FILE_SECONDS: u64 = 3600;

type WavOut = hound::WavWriter<std::io::BufWriter<std::fs::File>>;

/// Ein laufender Mitschnitt.
struct Recording {
    dir: PathBuf,
    consumer: rtrb::Consumer<f32>,
    writer: Option<(PathBuf, WavOut)>,
    frames_in_file: u64,
    rate: u32,
    files: Vec<PathBuf>,
}

impl Recording {
    fn open_file(&mut self) -> Result<(), String> {
        std::fs::create_dir_all(&self.dir).map_err(|e| format!("{}: {e}", self.dir.display()))?;
        let stamp = crate::utc_now_stamp();
        let mut path = self.dir.join(format!("sendung-{stamp}.wav"));
        let mut n = 1;
        while path.exists() {
            path = self.dir.join(format!("sendung-{stamp}-{n}.wav"));
            n += 1;
        }
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: self.rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        let w = hound::WavWriter::new(std::io::BufWriter::new(file), spec)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        tracing::info!(file = %path.display(), "mitschnitt file opened");
        self.files.push(path.clone());
        self.writer = Some((path, w));
        self.frames_in_file = 0;
        Ok(())
    }

    fn close_file(&mut self) -> Result<(), String> {
        if let Some((p, w)) = self.writer.take() {
            w.finalize().map_err(|e| format!("{}: {e}", p.display()))?;
        }
        Ok(())
    }

    /// Schreibt alles, was im Ring liegt.
    fn drain(&mut self) -> Result<(), String> {
        let limit = RECORD_FILE_SECONDS * u64::from(self.rate);
        while self.consumer.slots() >= 2 {
            if self.writer.is_none() || self.frames_in_file >= limit {
                self.close_file()?;
                self.open_file()?;
            }
            let (Ok(l), Ok(r)) = (self.consumer.pop(), self.consumer.pop()) else {
                break;
            };
            if let Some((p, w)) = self.writer.as_mut() {
                for s in [l, r] {
                    let v = (s.clamp(-1.0, 1.0) * 32_767.0).round() as i16;
                    w.write_sample(v)
                        .map_err(|e| format!("{}: {e}", p.display()))?;
                }
            }
            self.frames_in_file += 1;
        }
        if let Some((p, w)) = self.writer.as_mut() {
            w.flush().map_err(|e| format!("{}: {e}", p.display()))?;
        }
        Ok(())
    }
}

/// Steuert einen [`Mixer`].
pub struct Controller {
    collector: Collector,
    cmds: rtrb::Producer<Cmd>,
    status: Arc<Status>,
    rate: u32,
    show: RadioShow,
    next_uid: u32,
    /// Bereitgelegter, vom Mixer noch nicht begonnener Titel.
    pending: Option<u32>,
    /// Titel, mit dem „Start“ beginnt (uid), falls nichts läuft.
    cue: Option<u32>,
    running: bool,
    carts: Vec<Option<Shared<Stereo>>>,
    recording: Option<Recording>,
    message: Option<String>,
    /// Titel, die sich nicht laden ließen (werden übersprungen).
    failed: Vec<u32>,
    /// Zuletzt geladener Titel und wie lange das Dekodieren dauerte (für das Log, #51).
    last_load: Option<(PathBuf, std::time::Duration)>,
}

impl Controller {
    /// Zuletzt geladener Titel und Dauer des Dekodierens.
    pub fn last_load(&self) -> Option<&(PathBuf, std::time::Duration)> {
        self.last_load.as_ref()
    }

    /// Legt Controller und Mixer an. Der Mixer gehört danach in den Audio-Callback.
    pub fn new(show: RadioShow, rate: u32) -> (Controller, Mixer) {
        let (producer, consumer) = rtrb::RingBuffer::new(QUEUE);
        let status = Arc::new(Status::default());
        let mixer = Mixer::new(consumer, Arc::clone(&status), rate);
        let mut c = Controller {
            collector: Collector::new(),
            cmds: producer,
            status,
            rate: rate.max(1),
            show: RadioShow::default(),
            next_uid: 1,
            pending: None,
            cue: None,
            running: false,
            carts: vec![None; CARTS],
            recording: None,
            message: None,
            failed: Vec::new(),
            last_load: None,
        };
        c.replace_show(show);
        (c, mixer)
    }

    fn send(&mut self, cmd: Cmd) {
        if self.cmds.push(cmd).is_err() {
            tracing::warn!("radio command queue full");
            self.message = Some("Befehl verloren (Warteschlange voll)".into());
        }
    }

    /// Ersetzt Playlist, Carts und Einstellungen (etwa nach Laden von radio.json).
    /// Was gerade läuft, läuft weiter, wenn es noch in der Playlist steht.
    pub fn replace_show(&mut self, mut show: RadioShow) {
        show.sanitize();
        let old: Vec<(PathBuf, u32)> = self
            .show
            .playlist
            .iter()
            .map(|e| (e.path.clone(), e.uid))
            .collect();
        for e in &mut show.playlist {
            // Laufzeit-Kennung behalten, damit der laufende Titel erkannt bleibt.
            e.uid = match old.iter().position(|(p, _)| *p == e.path) {
                Some(i) => old[i].1,
                None => {
                    let u = self.next_uid;
                    self.next_uid += 1;
                    u
                }
            };
        }
        let carts_changed = show.carts != self.show.carts;
        self.show = show;
        if carts_changed {
            self.carts = vec![None; CARTS];
            for slot in 0..CARTS {
                self.load_cart(slot);
            }
        }
        self.apply_settings();
        self.requeue();
    }

    fn apply_settings(&mut self) {
        let frames = (self.show.crossfade_seconds * f64::from(self.rate)) as usize;
        self.send(Cmd::SetCrossfade(frames));
        self.send(Cmd::SetDuck(db_to_gain(self.show.duck_db)));
    }

    pub fn show(&self) -> &RadioShow {
        &self.show
    }

    pub fn status(&self) -> &Arc<Status> {
        &self.status
    }

    pub fn rate(&self) -> u32 {
        self.rate
    }

    /// Letzte Meldung für die Oberfläche (einmal abholen).
    pub fn take_message(&mut self) -> Option<String> {
        self.message.take()
    }

    fn index_of(&self, uid: u32) -> Option<usize> {
        self.show.playlist.iter().position(|e| e.uid == uid)
    }

    /// Index des laufenden Titels.
    pub fn current_index(&self) -> Option<usize> {
        let cur = self.status.current.load(Ordering::Acquire);
        if cur == NONE {
            return None;
        }
        self.index_of(cur)
    }

    /// Index, mit dem „Start“ beginnt.
    pub fn cue_index(&self) -> usize {
        self.cue.and_then(|u| self.index_of(u)).unwrap_or(0)
    }

    /// Wählt den Titel, mit dem „Start“ beginnt.
    pub fn set_cue(&mut self, index: usize) {
        self.cue = self.show.playlist.get(index).map(|e| e.uid);
    }

    pub fn is_running(&self) -> bool {
        self.running
    }

    // ---- Playlist bearbeiten ---------------------------------------------------------

    /// Hängt Dateien an (oder fügt bei `at` ein). Gibt die Indizes zurück.
    pub fn add(&mut self, paths: &[PathBuf], at: Option<usize>) -> Vec<usize> {
        let mut at = at.unwrap_or(self.show.playlist.len());
        let mut out = Vec::new();
        for p in paths {
            let mut e = PlaylistEntry::new(p.clone());
            e.uid = self.next_uid;
            self.next_uid += 1;
            at = at.min(self.show.playlist.len());
            self.show.playlist.insert(at, e);
            out.push(at);
            at += 1;
        }
        self.requeue();
        out
    }

    pub fn remove(&mut self, index: usize) -> bool {
        if index >= self.show.playlist.len() {
            return false;
        }
        self.show.playlist.remove(index);
        self.requeue();
        true
    }

    pub fn move_entry(&mut self, from: usize, to: usize) -> bool {
        let ok = self.show.move_entry(from, to);
        if ok {
            self.requeue();
        }
        ok
    }

    /// Übernimmt eine Messung (aus dem Hintergrund-Thread) für alle Einträge mit `path`.
    pub fn set_measured(&mut self, path: &Path, m: crate::audio::Measured) {
        for e in &mut self.show.playlist {
            if e.path == path {
                e.set_measured(m);
            }
        }
    }

    /// Misst alle Titel ohne gültige Messung sofort (blockiert; für Offline und Agent).
    pub fn measure_missing(&mut self) {
        let rate = self.rate;
        for e in &mut self.show.playlist {
            if !e.is_measured() {
                match load_stereo(&e.path, rate) {
                    Ok(s) => e.set_measured(measure(&s, rate)),
                    Err(err) => tracing::warn!(error = %err, "measure failed"),
                }
            }
        }
    }

    // ---- Einstellungen ---------------------------------------------------------------

    pub fn set_crossfade(&mut self, seconds: f64) {
        self.show.crossfade_seconds = seconds;
        self.show.sanitize();
        self.apply_settings();
    }

    pub fn set_duck(&mut self, db: f64) {
        self.show.duck_db = db;
        self.show.sanitize();
        self.apply_settings();
    }

    // ---- Transport -------------------------------------------------------------------

    /// Start (wenn nichts läuft) oder Weiter (Überblenden zum nächsten Titel).
    pub fn start_or_next(&mut self) {
        if self.running && self.status.current.load(Ordering::Acquire) != NONE {
            self.send(Cmd::Next);
        } else {
            self.start();
        }
    }

    /// Startet ab dem Cue-Titel.
    pub fn start(&mut self) {
        let from = self.cue_index();
        if from >= self.show.playlist.len() {
            self.message = Some("Playlist ist leer".into());
            return;
        }
        self.send(Cmd::ClearNext);
        self.pending = None;
        self.failed.clear();
        self.running = true;
        if self.queue_index(from) {
            self.send(Cmd::Play);
        } else {
            self.running = false;
        }
    }

    /// Blendet die Musik aus und hält an. „Start“ beginnt danach mit dem Titel, der lief.
    pub fn stop(&mut self) {
        if let Some(i) = self.current_index() {
            self.set_cue(i);
        }
        self.running = false;
        self.pending = None;
        self.send(Cmd::Stop);
    }

    /// Legt Titel `index` beim Mixer bereit. `false` bei Fehler (Titel wird übersprungen
    /// gemeldet).
    fn queue_index(&mut self, index: usize) -> bool {
        let Some(e) = self.show.playlist.get(index) else {
            return false;
        };
        let (uid, path) = (e.uid, e.path.clone());
        let t0 = std::time::Instant::now();
        let loaded = load_stereo(&path, self.rate);
        let took = t0.elapsed();
        tracing::debug!(file = %path.display(), ms = took.as_millis() as u64, "track decoded");
        self.last_load = Some((path.clone(), took));
        match loaded {
            Ok(s) => {
                let m = measure(&s, self.rate);
                if let Some(e) = self.show.playlist.get_mut(index)
                    && !e.is_measured()
                {
                    e.set_measured(m);
                }
                let gain = self
                    .show
                    .playlist
                    .get(index)
                    .map_or(0.0, |e| e.gain_db(self.show.target_lufs));
                let buf = Shared::new(&self.collector.handle(), s);
                self.send(Cmd::Queue {
                    uid,
                    buf,
                    gain: db_to_gain(gain),
                });
                self.pending = Some(uid);
                true
            }
            Err(err) => {
                tracing::warn!(error = %err, "track load failed");
                self.message = Some(err);
                false
            }
        }
    }

    /// Nach einer Änderung der Playlist: den bereitgelegten Titel neu bestimmen.
    fn requeue(&mut self) {
        if self.pending.is_some() && self.running {
            self.send(Cmd::ClearNext);
            self.pending = None;
        }
    }

    // ---- Carts -----------------------------------------------------------------------

    fn load_cart(&mut self, slot: usize) {
        let Some(Some(c)) = self.show.carts.get(slot) else {
            return;
        };
        let path = c.path.clone();
        match load_stereo(&path, self.rate) {
            Ok(s) => self.carts[slot] = Some(Shared::new(&self.collector.handle(), s)),
            Err(e) => {
                tracing::warn!(error = %e, "cart load failed");
                self.message = Some(e);
            }
        }
    }

    /// Belegt eine Cart-Taste (`None` = frei).
    pub fn assign_cart(&mut self, slot: usize, path: Option<PathBuf>) -> Result<(), String> {
        if slot >= CARTS {
            return Err(format!("Cart {slot} gibt es nicht"));
        }
        match path {
            None => {
                self.show.carts[slot] = None;
                self.carts[slot] = None;
                Ok(())
            }
            Some(p) => {
                let s = load_stereo(&p, self.rate)?;
                self.carts[slot] = Some(Shared::new(&self.collector.handle(), s));
                self.show.carts[slot] = Some(CartEntry::new(p));
                Ok(())
            }
        }
    }

    /// Spielt Cart `slot` (überlagert die Musik); läuft er schon, hält er an.
    pub fn trigger_cart(&mut self, slot: usize) {
        if self.status.carts.load(Ordering::Acquire) & (1 << slot) != 0 {
            self.send(Cmd::CartStop(slot));
            return;
        }
        if let Some(Some(buf)) = self.carts.get(slot) {
            let buf = buf.clone();
            self.send(Cmd::Cart {
                slot,
                buf,
                gain: 1.0,
            });
        }
    }

    // ---- Mikro -----------------------------------------------------------------------

    /// Hängt einen Mikro-Ring ein (Stereo verschachtelt in Mixer-Rate) oder aus.
    pub fn set_mic_input(&mut self, consumer: Option<rtrb::Consumer<f32>>) {
        let owned = consumer.map(|c| Owned::new(&self.collector.handle(), c));
        self.send(Cmd::MicInput(owned));
    }

    pub fn set_mic_open(&mut self, open: bool) {
        self.send(Cmd::MicOpen(open));
    }

    pub fn set_mic_monitor(&mut self, on: bool) {
        self.send(Cmd::MicMonitor(on));
    }

    // ---- Sendung (woa-streamd) -------------------------------------------------------

    /// Hängt den Sende-Ring ein (Sendeausgang Stereo verschachtelt in Mixer-Rate) oder aus.
    /// Den Ring liest der Feeder aus `woa-stream` in einem eigenen Thread.
    pub fn set_stream_output(&mut self, producer: Option<rtrb::Producer<f32>>) {
        let owned = producer.map(|p| Owned::new(&self.collector.handle(), p));
        self.send(Cmd::Stream(owned));
    }

    // ---- Mitschnitt ------------------------------------------------------------------

    /// Startet den Mitschnitt nach `<data_dir>/mitschnitte/`.
    pub fn start_recording(&mut self, data_dir: &Path) {
        if self.recording.is_some() {
            return;
        }
        let cap = self.rate as usize * 2 * RECORD_QUEUE_SECONDS;
        let (producer, consumer) = rtrb::RingBuffer::new(cap);
        self.recording = Some(Recording {
            dir: data_dir.join(RECORDINGS_DIR),
            consumer,
            writer: None,
            frames_in_file: 0,
            rate: self.rate,
            files: Vec::new(),
        });
        let owned = Owned::new(&self.collector.handle(), producer);
        self.send(Cmd::Record(Some(owned)));
    }

    /// Beendet den Mitschnitt; liefert die geschriebenen Dateien.
    pub fn stop_recording(&mut self) -> Vec<PathBuf> {
        self.send(Cmd::Record(None));
        let Some(mut r) = self.recording.take() else {
            return Vec::new();
        };
        if let Err(e) = r.drain().and_then(|()| r.close_file()) {
            tracing::warn!(error = %e, "mitschnitt failed");
            self.message = Some(e);
        }
        r.files
    }

    /// Datei, in die gerade mitgeschnitten wird.
    pub fn recording_file(&self) -> Option<&Path> {
        self.recording
            .as_ref()
            .and_then(|r| r.writer.as_ref().map(|(p, _)| p.as_path()))
    }

    pub fn is_recording(&self) -> bool {
        self.recording.is_some()
    }

    // ---- Takt des Control-Threads ----------------------------------------------------

    /// Regelmäßig aufrufen (alle paar Millisekunden): nächsten Titel bereitlegen,
    /// Mitschnitt schreiben, alte Puffer freigeben.
    pub fn pump(&mut self) {
        self.collector.collect();
        if let Some(r) = self.recording.as_mut()
            && let Err(e) = r.drain()
        {
            tracing::warn!(error = %e, "mitschnitt failed, stopped");
            self.message = Some(format!("Mitschnitt angehalten: {e}"));
            self.stop_recording();
        }
        if !self.running {
            return;
        }
        let cur = self.status.current.load(Ordering::Acquire);
        if let Some(p) = self.pending {
            if cur != p {
                // Noch bereitgelegt oder unterwegs: warten.
                return;
            }
            // Mixer hat ihn begonnen.
            self.pending = None;
        }
        // Nichts bereit: den Titel nach dem laufenden bereitlegen.
        let after = match self.index_of(cur) {
            Some(i) => i + 1,
            None if cur == NONE => {
                if self.status.playing.load(Ordering::Acquire) {
                    // Musik zu Ende, nichts mehr bereit: Sendung endet.
                    self.running = false;
                    self.cue = None;
                    self.send(Cmd::Stop);
                }
                return;
            }
            // Laufender Titel wurde aus der Playlist entfernt: mit dem Cue weiter.
            None => self.cue_index(),
        };
        for i in after..self.show.playlist.len() {
            let uid = self.show.playlist[i].uid;
            if self.failed.contains(&uid) {
                continue;
            }
            if self.queue_index(i) {
                break;
            }
            // Kaputte Datei: überspringen (Meldung steht in `message`).
            self.failed.push(uid);
        }
    }

    /// Restzeit des laufenden Titels (Sekunden).
    pub fn remaining_title(&self) -> f64 {
        if self.status.current.load(Ordering::Acquire) == NONE {
            return 0.0;
        }
        let end = self.status.end.load(Ordering::Acquire);
        let pos = self.status.position.load(Ordering::Acquire);
        end.saturating_sub(pos) as f64 / f64::from(self.rate)
    }

    /// Restzeit bis zum Ende der Playlist (Sekunden): Rest des laufenden Titels plus die
    /// folgenden, abzüglich Überblendungen.
    pub fn remaining_total(&self) -> f64 {
        match self.current_index() {
            Some(i) => {
                let rest = self.remaining_title();
                let after = self.show.total_seconds_from(i + 1);
                let fade = match (
                    self.show.playlist.get(i).and_then(|e| e.seconds),
                    self.show.playlist.get(i + 1).and_then(|e| e.seconds),
                ) {
                    (Some(a), Some(b)) => self.show.crossfade_between(a, b).min(rest),
                    _ => 0.0,
                };
                rest + after - fade
            }
            None => self.show.total_seconds_from(self.cue_index()),
        }
    }

    /// Ist die Sendung durch (für Offline-Render)?
    pub fn finished(&self) -> bool {
        !self.running
            && self.pending.is_none()
            && self.status.current.load(Ordering::Acquire) == NONE
    }
}

/// Rendert die Playlist von `show` offline mit Crossfades und Auto-Gain (dieselben
/// Klassen wie live). Gibt Stereo bis zum Ende des letzten Titels zurück.
pub fn render(show: &RadioShow, rate: u32) -> Result<Stereo, String> {
    const BLOCK: usize = 512;
    if show.playlist.is_empty() {
        return Err("Playlist ist leer".into());
    }
    let (mut c, mut mixer) = Controller::new(show.clone(), rate);
    c.measure_missing();
    let total: f64 = c.show().total_seconds_from(0);
    let limit = ((total + 10.0) * f64::from(rate)) as usize;
    c.start();
    if let Some(m) = c.take_message() {
        return Err(m);
    }
    let mut out = Stereo::default();
    let mut l = vec![0.0f32; BLOCK];
    let mut r = vec![0.0f32; BLOCK];
    loop {
        c.pump();
        mixer.process(&mut l, &mut r);
        out.left.extend_from_slice(&l);
        out.right.extend_from_slice(&r);
        c.pump();
        if c.finished() && !c.status().playing.load(Ordering::Acquire) {
            break;
        }
        if out.left.len() > limit {
            return Err("Render läuft über die erwartete Länge".into());
        }
    }
    let end = c.status().music_end.load(Ordering::Acquire) as usize;
    out.left.truncate(end);
    out.right.truncate(end);
    if let Some(m) = c.take_message() {
        tracing::warn!(message = %m, "radio render");
    }
    Ok(out)
}
