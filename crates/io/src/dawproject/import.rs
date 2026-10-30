//! Liest eine DAWproject-Datei (von WOA oder einem anderen Programm) als WOA-Projekt.
//!
//! Die Datei ist fremde Eingabe: Tiefe und Größe von XML und ZIP sind begrenzt, ZIP-Pfade
//! werden nie zum Schreiben benutzt (die Audiodateien bekommen eigene Namen im Ordner
//! `audio/` des neuen Projekts), und das fertige Projekt geht durch dieselbe Prüfung wie eine
//! geladene `project.json`. Was WOA nicht kennt, wird im [`Report`] genannt und geloggt.

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use woa_model::{
    AutoMode, AutoPoint, AutoTarget, AutomationLane, Clip, ClipId, FOLLOW_TEMPO_RANGE, FadeCurve,
    Insert, InsertId, MAX_AUTOMATION_POINTS, MAX_GAIN_DB, MAX_ID, MAX_TEMPO_BPM, MIN_GAIN_DB,
    MIN_TEMPO_BPM, MidiCc, MidiData, MidiNote, PITCH_RANGE, Project, STRETCH_RANGE, Send, Track,
    TrackId, TrackKind, is_safe_audio_path, store,
};
use zip::ZipArchive;

use super::export::safe_name;
use super::xml::{self, Node};
use super::{
    CLIP_NOTE_PREFIX, DEVICE_PREFIX, DawError, METADATA_ENTRY, PROJECT_ENTRY, Report,
    effect_from_id, linear_to_db,
};

/// Höchste Zahl Einträge im ZIP.
pub const MAX_ENTRIES: usize = 20_000;
/// Größte `project.xml` (unkomprimiert).
pub const MAX_PROJECT_XML: u64 = 64 << 20;
/// Größte einzelne Audiodatei (unkomprimiert).
pub const MAX_AUDIO_ENTRY: u64 = 4 << 30;
/// Alle Audiodateien zusammen.
pub const MAX_AUDIO_TOTAL: u64 = 8 << 30;
/// Höchste Zahl Spuren.
pub const MAX_TRACKS: usize = 2_000;
/// Höchste Zahl Clips je Datei.
pub const MAX_CLIPS: usize = 200_000;

/// Verweis einer Kennung in der Datei auf etwas im neuen Projekt.
#[derive(Debug, Clone, Copy)]
enum Ref {
    /// Spur oder Kanal der Spur (Index in `project.tracks`).
    Track(usize),
    /// Master: Ziel ohne eigene Spur.
    Master,
    Volume(usize),
    Pan(usize),
    SendVolume(usize, TrackId),
    Param(usize, InsertId, usize),
}

struct Imp {
    project: Project,
    tempo: f64,
    report: Report,
    next_id: u64,
    archive: ZipArchive<std::fs::File>,
    names: HashMap<String, String>,
    audio_dir: PathBuf,
    extracted: HashMap<String, Option<String>>,
    audio_total: u64,
    refs: HashMap<String, Ref>,
    clips_seen: usize,
}

fn rel_close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-6 * a.abs().max(b.abs()).max(1e-12)
}

/// Werte aus der Clip-Notiz `woa: key=value …`.
#[derive(Debug, Default)]
struct ClipNote {
    gain: Option<f64>,
    pitch: Option<f64>,
    follow: Option<f64>,
    stretch: Option<f64>,
    fade_in: Option<FadeCurve>,
    fade_out: Option<FadeCurve>,
}

fn curve_from(s: &str) -> Option<FadeCurve> {
    match s {
        "linear" => Some(FadeCurve::Linear),
        "equal_power" => Some(FadeCurve::EqualPower),
        "s_curve" => Some(FadeCurve::SCurve),
        _ => None,
    }
}

impl ClipNote {
    fn parse(comment: Option<&str>) -> Self {
        let mut n = Self::default();
        let Some(rest) = comment.and_then(|c| c.trim().strip_prefix(CLIP_NOTE_PREFIX)) else {
            return n;
        };
        for part in rest.split_whitespace() {
            let Some((k, v)) = part.split_once('=') else {
                continue;
            };
            let f = || v.parse::<f64>().ok().filter(|x| x.is_finite());
            match k {
                "gain" => n.gain = f(),
                "pitch" => n.pitch = f(),
                "follow" => n.follow = f(),
                "stretch" => n.stretch = f(),
                "fic" => n.fade_in = curve_from(v),
                "foc" => n.fade_out = curve_from(v),
                _ => {}
            }
        }
        n
    }
}

impl Imp {
    fn alloc(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// Wert einer Zeitangabe in Beats.
    fn beats(&self, v: f64, unit: &str) -> f64 {
        if unit == "seconds" {
            v * self.tempo / 60.0
        } else {
            v
        }
    }

    fn note(&mut self, msg: impl Into<String>) {
        self.report.note(msg);
    }

    // ---------------------------------------------------------------- Audio aus dem ZIP

    fn extract(&mut self, raw_path: &str) -> Option<String> {
        let norm = raw_path.replace('\\', "/");
        if let Some(v) = self.extracted.get(&norm) {
            return v.clone();
        }
        let r = self.extract_inner(&norm);
        self.extracted.insert(norm, r.clone());
        r
    }

    fn extract_inner(&mut self, norm: &str) -> Option<String> {
        if !is_safe_audio_path(norm) {
            self.note("Audiodatei mit unzulässigem Pfad übersprungen");
            return None;
        }
        let Some(orig) = self.names.get(norm).cloned() else {
            self.note(format!("Audiodatei „{norm}“ fehlt in der Datei"));
            return None;
        };
        let name = format!("{:03}-{}", self.report.audio_files + 1, safe_name(norm));
        let dest = self.audio_dir.join(&name);
        let mut zf = match self.archive.by_name(&orig) {
            Ok(z) => z,
            Err(e) => {
                self.report
                    .note(format!("Audiodatei „{norm}“ nicht lesbar: {e}"));
                return None;
            }
        };
        let size = zf.size();
        if zf.is_dir()
            || size > MAX_AUDIO_ENTRY
            || self.audio_total.saturating_add(size) > MAX_AUDIO_TOTAL
        {
            self.report
                .note(format!("Audiodatei „{norm}“ zu groß oder ein Ordner"));
            return None;
        }
        let result = (|| -> std::io::Result<u64> {
            let mut out = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&dest)?;
            // Die Größenangabe im ZIP kann lügen: beim Lesen begrenzen.
            std::io::copy(&mut (&mut zf).take(MAX_AUDIO_ENTRY + 1), &mut out)
        })();
        match result {
            Ok(n) if n <= MAX_AUDIO_ENTRY => {
                self.audio_total += n;
                self.report.audio_files += 1;
                Some(format!("{}/{name}", store::AUDIO_DIR))
            }
            Ok(_) => {
                let _ = std::fs::remove_file(&dest);
                self.report
                    .note(format!("Audiodatei „{norm}“ größer als erlaubt"));
                None
            }
            Err(e) => {
                let _ = std::fs::remove_file(&dest);
                self.report
                    .note(format!("Audiodatei „{norm}“ nicht geschrieben: {e}"));
                None
            }
        }
    }

    // ---------------------------------------------------------------- Struktur

    fn structure(&mut self, root: &Node) {
        let Some(st) = root.first("Structure") else {
            self.note("Keine Spuren (Structure) in der Datei");
            return;
        };
        let mut work: Vec<(usize, Node)> = Vec::new();
        for n in &st.children {
            self.collect(n, &mut work);
        }
        // Zweiter Durchgang: Mischpult, erst jetzt sind alle Ziele bekannt.
        for (idx, channel) in &work {
            self.mixer(*idx, channel);
        }
    }

    /// Legt die Spur an, merkt Kennungen; Kinder (verschachtelte Spuren) folgen.
    fn collect(&mut self, n: &Node, work: &mut Vec<(usize, Node)>) {
        match n.name.as_str() {
            "Track" => {
                let channel = n.first("Channel");
                let role = channel.and_then(|c| c.attr("role")).unwrap_or("regular");
                if role == "master" {
                    self.register_master(n, channel);
                } else if let Some(idx) = self.add_track(Some(n), channel) {
                    work.push((
                        idx,
                        channel.cloned().unwrap_or_else(|| Node::new("Channel")),
                    ));
                }
                for child in n.all("Track") {
                    self.collect(child, work);
                }
            }
            "Channel" => {
                let role = n.attr("role").unwrap_or("regular");
                if role == "master" {
                    self.register_master(n, None);
                } else if let Some(idx) = self.add_track(None, Some(n)) {
                    work.push((idx, n.clone()));
                }
            }
            _ => {}
        }
    }

    fn register_master(&mut self, a: &Node, b: Option<&Node>) {
        for id in [a.attr("id"), b.and_then(|c| c.attr("id"))]
            .into_iter()
            .flatten()
        {
            self.refs.insert(id.to_owned(), Ref::Master);
        }
    }

    fn add_track(&mut self, track: Option<&Node>, channel: Option<&Node>) -> Option<usize> {
        if self.project.tracks.len() >= MAX_TRACKS {
            self.note(format!(
                "Mehr als {MAX_TRACKS} Spuren: der Rest wird ausgelassen"
            ));
            return None;
        }
        let ct: Vec<&str> = track
            .and_then(|t| t.attr("contentType"))
            .unwrap_or("")
            .split_whitespace()
            .collect();
        let role = channel.and_then(|c| c.attr("role")).unwrap_or("regular");
        let kind = if matches!(role, "effect" | "submix")
            || (ct.contains(&"tracks") && !ct.contains(&"audio") && !ct.contains(&"notes"))
        {
            TrackKind::Bus
        } else if ct.contains(&"notes") && !ct.contains(&"audio") {
            TrackKind::Midi
        } else {
            TrackKind::Audio
        };
        let idx = self.project.tracks.len();
        let name = track
            .and_then(|t| t.attr("name"))
            .or_else(|| channel.and_then(|c| c.attr("name")))
            .map(|s| s.trim().chars().take(200).collect::<String>())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| format!("Spur {}", idx + 1));
        let id = TrackId(self.alloc());
        self.project.tracks.push(Track::new(id, name, kind));
        for n in [track, channel].into_iter().flatten() {
            if let Some(i) = n.attr("id") {
                self.refs.insert(i.to_owned(), Ref::Track(idx));
            }
        }
        // Volume, Pan und Sends tragen Kennungen für Automation.
        if let Some(ch) = channel {
            if let Some(i) = ch.first("Volume").and_then(|v| v.attr("id")) {
                self.refs.insert(i.to_owned(), Ref::Volume(idx));
            }
            if let Some(i) = ch.first("Pan").and_then(|v| v.attr("id")) {
                self.refs.insert(i.to_owned(), Ref::Pan(idx));
            }
        }
        Some(idx)
    }

    fn bus_target(&self, id: Option<&str>) -> Option<TrackId> {
        match self.refs.get(id?)? {
            Ref::Track(i) => {
                let t = &self.project.tracks[*i];
                (t.kind == TrackKind::Bus).then_some(t.id)
            }
            _ => None,
        }
    }

    fn mixer(&mut self, idx: usize, ch: &Node) {
        let tid = self.project.tracks[idx].id;
        // Lautstärke
        if let Some(v) = ch.first("Volume")
            && let Some(db) = self.level_db(v)
        {
            self.project.tracks[idx].gain_db = db.clamp(MIN_GAIN_DB, MAX_GAIN_DB);
        }
        // Pan
        if let Some(p) = ch.first("Pan")
            && let Some(x) = p.num("value")
        {
            let (min, max) = (p.num("min").unwrap_or(0.0), p.num("max").unwrap_or(1.0));
            if max > min {
                self.project.tracks[idx].pan =
                    ((x - min) / (max - min) * 2.0 - 1.0).clamp(-1.0, 1.0);
            }
        }
        if let Some(m) = ch.first("Mute").and_then(|m| m.flag("value")) {
            self.project.tracks[idx].mute = m;
        }
        self.project.tracks[idx].solo = ch.flag("solo").unwrap_or(false);
        // Ausgang
        if let Some(dest) = ch.attr("destination") {
            match self.bus_target(Some(dest)) {
                Some(b) if b != tid && !self.project.reaches(b, tid) => {
                    self.project.tracks[idx].output = Some(b);
                }
                Some(_) => {
                    let name = self.project.tracks[idx].name.clone();
                    self.note(format!(
                        "Spur „{name}“: Ausgang würde eine Schleife bilden, bleibt Master"
                    ));
                }
                None => {}
            }
        }
        // Sends
        if let Some(sends) = ch.first("Sends") {
            for s in sends.all("Send") {
                self.send(idx, s);
            }
        }
        // Geräte
        if let Some(devs) = ch.first("Devices") {
            for d in &devs.children {
                self.device(idx, d);
            }
        }
    }

    /// dB-Wert eines Lautstärke-Reglers; `None` bei unbekannter Einheit (dann gemeldet).
    fn level_db(&mut self, p: &Node) -> Option<f64> {
        let v = p.num("value")?;
        match p.attr("unit").unwrap_or("linear") {
            "linear" => Some(linear_to_db(v)),
            "decibel" => Some(v),
            other => {
                self.note(format!("Lautstärke in Einheit „{other}“ nicht übernommen"));
                None
            }
        }
    }

    fn send(&mut self, idx: usize, s: &Node) {
        let tid = self.project.tracks[idx].id;
        let Some(target) = self.bus_target(s.attr("destination")) else {
            self.note("Send ohne bekannten Ziel-Bus übersprungen");
            return;
        };
        if target == tid
            || self.project.reaches(target, tid)
            || self.project.tracks[idx]
                .sends
                .iter()
                .any(|x| x.target == target)
        {
            self.note("Send würde eine Schleife bilden oder doppelt sein, übersprungen");
            return;
        }
        let mut level = s
            .first("Volume")
            .and_then(|v| self.level_db(v))
            .unwrap_or(0.0)
            .clamp(MIN_GAIN_DB, woa_model::MAX_SEND_DB);
        if s.first("Enable").and_then(|e| e.flag("value")) == Some(false) {
            level = MIN_GAIN_DB;
        }
        self.project.tracks[idx].sends.push(Send {
            target,
            level_db: level,
            pre_fader: s.attr("type") == Some("pre"),
        });
        if let Some(i) = s.first("Volume").and_then(|v| v.attr("id")) {
            self.refs.insert(i.to_owned(), Ref::SendVolume(idx, target));
        }
    }

    fn device(&mut self, idx: usize, d: &Node) {
        let kind = d
            .attr("deviceID")
            .and_then(|id| id.strip_prefix(DEVICE_PREFIX))
            .and_then(effect_from_id)
            .filter(|_| d.name == "Device");
        let Some(kind) = kind else {
            let what = d
                .attr("deviceName")
                .or_else(|| d.attr("name"))
                .unwrap_or(&d.name);
            let tname = self.project.tracks[idx].name.clone();
            self.note(format!(
                "Spur „{tname}“: Gerät „{what}“ ({}) nicht übernommen",
                d.name
            ));
            return;
        };
        let id = InsertId(self.alloc());
        let mut ins = Insert::new(id, kind);
        if let Some(params) = d.first("Parameters") {
            for (pos, p) in params.children.iter().enumerate() {
                let index = p
                    .attr("name")
                    .and_then(|n| kind.param_index(n))
                    .or_else(|| {
                        p.attr("parameterID")
                            .and_then(|s| s.parse::<usize>().ok())
                            .filter(|i| *i < kind.params().len())
                    })
                    .unwrap_or(pos);
                let Some(spec) = kind.params().get(index) else {
                    continue;
                };
                if let Some(v) = p.num("value") {
                    let v = v.clamp(spec.min, spec.max);
                    ins.params[index] = if spec.choices.is_empty() {
                        v
                    } else {
                        v.round()
                    };
                }
                if let Some(i) = p.attr("id") {
                    self.refs.insert(i.to_owned(), Ref::Param(idx, id, index));
                }
            }
        }
        if d.first("Enabled").and_then(|e| e.flag("value")) == Some(false) {
            ins.bypass = true;
        }
        self.project.tracks[idx].inserts.push(ins);
    }

    // ---------------------------------------------------------------- Arrangement

    fn arrangement(&mut self, root: &Node) {
        let Some(arr) = root.first("Arrangement") else {
            return;
        };
        for k in ["TempoAutomation", "TimeSignatureAutomation"] {
            if arr.first(k).is_some() {
                self.note(format!(
                    "{k} wird nicht übernommen (WOA hat ein festes Tempo)"
                ));
            }
        }
        if arr.first("Markers").is_some() {
            self.note("Marker werden nicht übernommen");
        }
        if let Some(l) = arr.first("Lanes") {
            self.walk(l, "beats", None);
        }
        if root.first("Scenes").is_some_and(|s| !s.children.is_empty()) {
            self.note("Szenen und Session-Clips werden nicht übernommen");
        }
    }

    fn track_of(&self, node: &Node) -> Option<usize> {
        match self.refs.get(node.attr("track")?)? {
            Ref::Track(i) => Some(*i),
            _ => None,
        }
    }

    fn walk(&mut self, node: &Node, unit: &str, track: Option<usize>) {
        let unit = node.attr("timeUnit").unwrap_or(unit);
        let track = self.track_of(node).or(track);
        match node.name.as_str() {
            "Lanes" => {
                for c in &node.children {
                    self.walk(c, unit, track);
                }
            }
            "Clips" => match track {
                Some(t) => {
                    for c in node.all("Clip") {
                        self.clip(t, c, unit);
                    }
                }
                None => self.note("Clips ohne bekannte Spur übersprungen"),
            },
            "Points" => self.points(node, unit),
            "Markers" | "markers" => self.note("Marker werden nicht übernommen"),
            "ClipSlot" => self.note("Session-Clips werden nicht übernommen"),
            other => self.note(format!("Arrangement-Element „{other}“ nicht übernommen")),
        }
    }

    // ---------------------------------------------------------------- Clips

    fn clip(&mut self, t: usize, c: &Node, unit: &str) {
        self.clips_seen += 1;
        if self.clips_seen > MAX_CLIPS {
            if self.clips_seen == MAX_CLIPS + 1 {
                self.note(format!(
                    "Mehr als {MAX_CLIPS} Clips: der Rest wird ausgelassen"
                ));
            }
            return;
        }
        let (Some(time), true) = (c.num("time"), c.attr("time").is_some()) else {
            self.note("Clip ohne Startzeit übersprungen");
            return;
        };
        let start = self.beats(time, unit);
        if start < 0.0 {
            self.note("Clip vor dem Anfang übersprungen");
            return;
        }
        let cu = c.attr("contentTimeUnit");
        let note = ClipNote::parse(c.attr("comment"));
        for k in ["loopStart", "loopEnd"] {
            if c.attr(k).is_some() {
                self.note("Clip-Schleife (loopStart/loopEnd) wird nicht übernommen");
                break;
            }
        }
        if c.flag("enable") == Some(false) {
            self.note("Stummgeschalteter Clip bleibt hörbar (WOA hat kein Clip-Mute)");
        }
        let content = c.children.first();
        let mut audio: Option<String> = None;
        let mut midi: Option<MidiData> = None;
        let mut offset_beats = 0.0;
        let mut stretch = 1.0;
        let mut follow = None;
        let mut length_hint: Option<f64> = None;
        match content.map(|n| n.name.as_str()) {
            Some("Warps") => {
                if let Some(r) = self.audio_warps(c, content.unwrap_or(c), cu, &note) {
                    (audio, offset_beats, stretch, follow) = r;
                }
            }
            Some("Audio") => {
                let a = content.unwrap_or(c);
                if let Some(path) = self.audio_file(a) {
                    audio = Some(path);
                    let ps = c.num("playStart").unwrap_or(0.0);
                    let ps_sec = if cu == Some("beats") {
                        ps * 60.0 / self.tempo
                    } else {
                        ps
                    };
                    offset_beats = (ps_sec * self.tempo / 60.0).max(0.0);
                    if let Some(stop) = c.num("playStop") {
                        let stop_sec = if cu == Some("beats") {
                            stop * 60.0 / self.tempo
                        } else {
                            stop
                        };
                        length_hint = Some(((stop_sec - ps_sec) * self.tempo / 60.0).max(0.0));
                    }
                }
            }
            Some("Notes") => midi = self.notes_of(c, content.unwrap_or(c), None, cu),
            Some("Lanes") => {
                let lanes = content.unwrap_or(c);
                let notes = lanes.first("Notes");
                midi = notes.and_then(|n| self.notes_of(c, n, Some(lanes), cu));
                if notes.is_none() {
                    self.note("Clip mit Spuren ohne Noten nicht übernommen");
                }
            }
            Some("Points") => self.note("Clip mit Automation statt Inhalt nicht übernommen"),
            Some(other) => self.note(format!("Clip-Inhalt „{other}“ nicht übernommen")),
            None => {}
        }
        let duration = c.num("duration").map(|d| self.beats(d, unit));
        let Some(length) = duration.or(length_hint).filter(|l| *l > 0.0) else {
            self.note("Clip ohne Länge übersprungen");
            return;
        };
        let length = length.min(1.0e6);
        let id = ClipId(self.alloc());
        let name = c
            .attr("name")
            .map(|s| s.chars().take(200).collect::<String>())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "Clip".to_owned());
        let mut clip = Clip::new(id, name, start, length, audio);
        clip.offset_beats = offset_beats;
        clip.midi = midi;
        clip.stretch = stretch.clamp(STRETCH_RANGE.0, STRETCH_RANGE.1);
        clip.follow_tempo_bpm = follow;
        // Blenden
        let fu = c.attr("fadeTimeUnit").unwrap_or("beats");
        let mut fi = c
            .num("fadeInTime")
            .map_or(0.0, |v| self.beats(v, fu))
            .max(0.0);
        let mut fo = c
            .num("fadeOutTime")
            .map_or(0.0, |v| self.beats(v, fu))
            .max(0.0);
        if fi + fo > length {
            let k = length / (fi + fo);
            fi *= k;
            fo *= k;
        }
        clip.fade_in_beats = fi;
        clip.fade_out_beats = fo;
        if let Some(g) = note.gain {
            clip.gain_db = g.clamp(MIN_GAIN_DB, MAX_GAIN_DB);
        }
        if let Some(p) = note.pitch {
            clip.pitch_semitones = p.clamp(PITCH_RANGE.0, PITCH_RANGE.1);
        }
        clip.fade_in_curve = note.fade_in.unwrap_or_default();
        clip.fade_out_curve = note.fade_out.unwrap_or_default();
        self.report.clips += 1;
        self.project.tracks[t].clips.push(clip);
    }

    /// Pfad des `Audio`-Elements, aus dem ZIP herausgelöst.
    fn audio_file(&mut self, audio: &Node) -> Option<String> {
        let f = audio.first("File")?;
        if f.flag("external") == Some(true) {
            self.note("Audio liegt außerhalb der Datei (external) und wird nicht geholt");
            return None;
        }
        self.extract(f.attr("path")?)
    }

    #[allow(clippy::type_complexity)]
    fn audio_warps(
        &mut self,
        clip: &Node,
        warps: &Node,
        cu: Option<&str>,
        note: &ClipNote,
    ) -> Option<(Option<String>, f64, f64, Option<f64>)> {
        let audio = warps.first("Audio");
        let Some(audio) = audio else {
            self.note("Warps ohne Audio nicht übernommen");
            return None;
        };
        if warps.attr("contentTimeUnit").unwrap_or("seconds") != "seconds" {
            self.note("Warps mit Inhalt in Beats nicht übernommen");
            return None;
        }
        let path = self.audio_file(audio)?;
        let tu = warps.attr("timeUnit").or(cu).unwrap_or("beats");
        let mut pts: Vec<(f64, f64)> = warps
            .all("Warp")
            .filter_map(|w| Some((self.beats(w.num("time")?, tu), w.num("contentTime")?)))
            .collect();
        pts.sort_by(|a, b| a.1.total_cmp(&b.1));
        let natural = self.tempo / 60.0;
        let slope = match (pts.first(), pts.last()) {
            (Some(f), Some(l)) if l.1 - f.1 > 1e-9 && l.0 > f.0 => {
                if pts.len() > 2 {
                    let s = (l.0 - f.0) / (l.1 - f.1);
                    let off = pts
                        .iter()
                        .map(|p| (f.0 + (p.1 - f.1) * s - p.0).abs())
                        .fold(0.0, f64::max);
                    if off > 1e-3 {
                        self.note("Warp-Punkte: nur Anfang und Ende werden übernommen (gleichmäßige Dehnung)");
                    }
                }
                (l.0 - f.0) / (l.1 - f.1)
            }
            _ => {
                self.note("Warps ohne brauchbare Punkte: Audio läuft in Originalgeschwindigkeit");
                natural
            }
        };
        let first = pts.first().copied().unwrap_or((0.0, 0.0));
        let ps = clip.num("playStart").unwrap_or(0.0);
        let ps_beats = self.beats(ps, cu.unwrap_or(tu));
        let offset_sec = (first.1 + (ps_beats - first.0) / slope).max(0.0);
        let follow = note
            .follow
            .filter(|b| (FOLLOW_TEMPO_RANGE.0..=FOLLOW_TEMPO_RANGE.1).contains(b));
        let source_bpm = follow.unwrap_or(self.tempo);
        let calc = slope * 60.0 / source_bpm;
        let stretch = match note.stretch {
            Some(k) if rel_close(k * source_bpm / 60.0, slope) => k,
            _ => calc,
        };
        Some((Some(path), offset_sec * source_bpm / 60.0, stretch, follow))
    }

    fn notes_of(
        &mut self,
        clip: &Node,
        notes: &Node,
        lanes: Option<&Node>,
        cu: Option<&str>,
    ) -> Option<MidiData> {
        let nu = notes
            .attr("timeUnit")
            .or(cu)
            .or_else(|| lanes.and_then(|l| l.attr("timeUnit")))
            .unwrap_or("beats");
        let ps = clip
            .num("playStart")
            .map_or(0.0, |v| self.beats(v, cu.unwrap_or(nu)));
        let mut data = MidiData::default();
        for n in notes.all("Note") {
            if data.notes.len() >= woa_model::midi::MAX_MIDI_EVENTS {
                break;
            }
            let (Some(t), Some(d), Some(key)) = (n.num("time"), n.num("duration"), n.num("key"))
            else {
                continue;
            };
            let t = self.beats(t, nu) - ps;
            let d = self.beats(d, nu);
            if t < -1e-9 || d <= 0.0 || !(0.0..=127.0).contains(&key) {
                continue;
            }
            let vel = n.num("vel").unwrap_or(0.8).clamp(0.0, 1.0);
            data.notes.push(MidiNote {
                beat: t.max(0.0),
                length_beats: d,
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                pitch: key as u8,
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                velocity: ((vel * 127.0).round() as u8).clamp(1, 127),
            });
        }
        if let Some(l) = lanes {
            for p in l.all("Points") {
                self.clip_controller(&mut data, p, nu, ps);
            }
        }
        data.sort();
        Some(data)
    }

    fn clip_controller(&mut self, data: &mut MidiData, p: &Node, nu: &str, ps: f64) {
        let target = p.first("Target");
        let expr = target.and_then(|t| t.attr("expression"));
        let ctl = target.and_then(|t| t.num("controller"));
        let (Some("channelController"), Some(ctl)) = (expr, ctl) else {
            self.note("Automation im MIDI-Clip (nicht Controller) nicht übernommen");
            return;
        };
        if !(0.0..=127.0).contains(&ctl) {
            return;
        }
        let nu = p.attr("timeUnit").unwrap_or(nu);
        for pt in &p.children {
            if !pt.name.ends_with("Point") {
                continue;
            }
            let (Some(t), Some(v)) = (pt.num("time"), pt.num("value")) else {
                continue;
            };
            let t = self.beats(t, nu) - ps;
            if t < -1e-9 || data.cc.len() >= woa_model::midi::MAX_MIDI_EVENTS {
                continue;
            }
            data.cc.push(MidiCc {
                beat: t.max(0.0),
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                controller: ctl as u8,
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                value: (v.clamp(0.0, 1.0) * 127.0).round() as u8,
            });
        }
    }

    // ---------------------------------------------------------------- Automation

    fn points(&mut self, p: &Node, unit: &str) {
        let Some(target) = p.first("Target") else {
            self.note("Automation ohne Ziel übersprungen");
            return;
        };
        let Some(r) = target
            .attr("parameter")
            .and_then(|id| self.refs.get(id))
            .copied()
        else {
            self.note("Automation eines unbekannten Ziels (nicht Lautstärke, Pan, Send, Effekt-Regler) nicht übernommen");
            return;
        };
        let (t, at) = match r {
            Ref::Volume(i) => (i, AutoTarget::Gain),
            Ref::Pan(i) => (i, AutoTarget::Pan),
            Ref::SendVolume(i, target) => (i, AutoTarget::Send { target }),
            Ref::Param(i, insert, param) => (i, AutoTarget::InsertParam { insert, param }),
            Ref::Track(_) | Ref::Master => {
                self.note("Automation des Master oder einer ganzen Spur nicht übernommen");
                return;
            }
        };
        let Ok(range) = at.range_in(&self.project.tracks[t]) else {
            return;
        };
        let funit = p.attr("unit").unwrap_or(match at {
            AutoTarget::Pan => "normalized",
            AutoTarget::InsertParam { .. } => "linear",
            _ => "linear",
        });
        let conv: Option<fn(f64) -> f64> = match (at, funit) {
            (AutoTarget::Gain | AutoTarget::Send { .. }, "linear") => Some(linear_to_db),
            (AutoTarget::Gain | AutoTarget::Send { .. }, "decibel") => Some(|v| v),
            (AutoTarget::Pan, "normalized") => Some(|v| v * 2.0 - 1.0),
            (AutoTarget::InsertParam { .. }, "linear" | "decibel" | "hertz") => Some(|v| v),
            _ => None,
        };
        let Some(conv) = conv else {
            self.note(format!("Automation in Einheit „{funit}“ nicht übernommen"));
            return;
        };
        let pu = p.attr("timeUnit").unwrap_or(unit);
        let mut pts: Vec<AutoPoint> = Vec::new();
        for pt in &p.children {
            if !pt.name.ends_with("Point") {
                continue;
            }
            let (Some(time), Some(v)) = (pt.num("time"), pt.num("value")) else {
                continue;
            };
            let beat = self.beats(time, pu);
            if beat < 0.0 {
                continue;
            }
            let mut value = conv(v).clamp(range.min, range.max);
            if range.step {
                value = value.round();
            }
            pts.push(AutoPoint { beat, value });
        }
        pts.sort_by(|a, b| a.beat.total_cmp(&b.beat));
        // Doppelte Beats: der letzte gilt.
        let mut dedup: Vec<AutoPoint> = Vec::with_capacity(pts.len());
        for pt in pts {
            match dedup.last_mut() {
                Some(last) if last.beat == pt.beat => *last = pt,
                _ => dedup.push(pt),
            }
        }
        if dedup.len() > MAX_AUTOMATION_POINTS {
            dedup.truncate(MAX_AUTOMATION_POINTS);
            self.note(format!(
                "Automation auf {MAX_AUTOMATION_POINTS} Punkte gekürzt"
            ));
        }
        if dedup.is_empty() {
            return;
        }
        let tr = &mut self.project.tracks[t];
        tr.automation.retain(|l| l.target != at);
        tr.automation.push(AutomationLane {
            target: at,
            mode: AutoMode::Read,
            points: dedup,
        });
    }
}

/// Liest die DAWproject-Datei `file` und löst ihr Audio in `dir/audio/` heraus. Das Projekt
/// selbst wird nicht gespeichert, s. [`import_to_folder`].
///
/// # Errors
/// Datei nicht lesbar, kein gültiges DAWproject, zu groß, oder Projekt unzulässig.
pub fn import(file: &Path, dir: &Path) -> Result<(Project, Report), DawError> {
    let f = std::fs::File::open(file)?;
    let mut archive = ZipArchive::new(f)?;
    if archive.len() > MAX_ENTRIES {
        return Err(DawError::Zip(format!("mehr als {MAX_ENTRIES} Einträge")));
    }
    let names: HashMap<String, String> = archive
        .file_names()
        .filter_map(Result::ok)
        .map(|n| (n.replace('\\', "/"), n.into_owned()))
        .collect();
    let mut xml_bytes = Vec::new();
    {
        let mut e = archive
            .by_name(PROJECT_ENTRY)
            .map_err(|_| DawError::Invalid("project.xml fehlt".into()))?;
        e.by_ref()
            .take(MAX_PROJECT_XML + 1)
            .read_to_end(&mut xml_bytes)?;
    }
    if xml_bytes.len() as u64 > MAX_PROJECT_XML {
        return Err(DawError::Zip("project.xml zu groß".into()));
    }
    let title = archive.by_name(METADATA_ENTRY).ok().and_then(|mut m| {
        let mut b = Vec::new();
        m.by_ref().take(1 << 20).read_to_end(&mut b).ok()?;
        xml::metadata_title(&b)
    });
    let root = xml::parse(&xml_bytes)?;
    drop(xml_bytes);
    if root.name != "Project" {
        return Err(DawError::Invalid(format!(
            "Wurzel ist „{}“, nicht „Project“",
            root.name
        )));
    }
    if let Some(v) = root.attr("version")
        && !v.starts_with("1.")
    {
        tracing::warn!(
            version = v,
            "dawproject: newer major version, reading anyway"
        );
    }
    let stem = file
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("DAWproject")
        .to_owned();
    let name = title
        .or_else(|| Some(stem).filter(|s| !s.is_empty()))
        .unwrap_or_else(|| "DAWproject".into());
    let audio_dir = dir.join(store::AUDIO_DIR);
    std::fs::create_dir_all(&audio_dir)?;
    let tempo = root
        .first("Transport")
        .and_then(|t| t.first("Tempo"))
        .and_then(|t| t.num("value"))
        .filter(|v| *v > 0.0)
        .unwrap_or(woa_model::DEFAULT_TEMPO_BPM);
    let mut project = Project::new(name.chars().take(200).collect::<String>());
    let tempo_clamped = tempo.clamp(MIN_TEMPO_BPM, MAX_TEMPO_BPM);
    project.tempo_bpm = tempo_clamped;
    let mut imp = Imp {
        project,
        tempo: tempo_clamped,
        report: Report::default(),
        next_id: 1,
        archive,
        names,
        audio_dir,
        extracted: HashMap::new(),
        audio_total: 0,
        refs: HashMap::new(),
        clips_seen: 0,
    };
    if (tempo - tempo_clamped).abs() > f64::EPSILON {
        imp.note(format!(
            "Tempo {tempo:.1} BPM liegt außerhalb {MIN_TEMPO_BPM} bis {MAX_TEMPO_BPM}, begrenzt"
        ));
    }
    imp.structure(&root);
    imp.arrangement(&root);
    // Eine Spur mit nur MIDI-Clips ist eine MIDI-Spur, auch wenn die Datei „audio“ sagte.
    for t in &mut imp.project.tracks {
        if t.kind == TrackKind::Audio
            && !t.clips.is_empty()
            && t.clips.iter().all(|c| c.midi.is_some())
        {
            t.kind = TrackKind::Midi;
        }
    }
    imp.report.tracks = imp.project.tracks.len();
    imp.project.next_id = imp.next_id.min(MAX_ID + 1);
    // Dieselbe Prüfung wie beim Laden einer project.json (Ids, Routing, Automation, MIDI).
    let json = store::to_json(&imp.project).map_err(|e| DawError::Model(e.to_string()))?;
    let project = store::from_json(&json, Path::new(PROJECT_ENTRY))
        .map_err(|e| DawError::Model(e.to_string()))?;
    Ok((project, imp.report))
}

/// Importiert `file` in einen neuen Projektordner `dir` (muss fehlen oder leer sein) und
/// speichert das Projekt dort. Bei einem Fehler wird ein von hier angelegter Ordner wieder
/// entfernt.
///
/// # Errors
/// Wie [`import`], dazu Speichern.
pub fn import_to_folder(file: &Path, dir: &Path) -> Result<(Project, Report), DawError> {
    let existed = dir.exists();
    if existed && std::fs::read_dir(dir)?.next().is_some() {
        return Err(DawError::Io("Zielordner ist nicht leer".into()));
    }
    let result = import(file, dir).and_then(|(project, report)| {
        store::save(&project, dir).map_err(|e| DawError::Model(e.to_string()))?;
        Ok((project, report))
    });
    if result.is_err() && !existed {
        // Nur der eigene, halb gefüllte Ordner dieses Aufrufs.
        let _ = std::fs::remove_dir_all(dir);
    }
    result
}
