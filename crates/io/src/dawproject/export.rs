//! Schreibt ein WOA-Projekt als DAWproject-Datei (ZIP mit `project.xml`, `metadata.xml` und
//! `audio/…`). Aufbau und Einheiten folgen dem Referenz-Testcode von Bitwig: Lautstärke
//! linear, Pan normalisiert (0,5 = Mitte), Audio-Clips mit `Warps` (Beats auf Sekunden),
//! Noten mit Anschlagstärke 0…1.

use std::collections::{HashMap, HashSet};
use std::io::{Seek, Write};
use std::path::{Path, PathBuf};

use woa_model::{
    AutoTarget, AutomationLane, Clip, FadeCurve, Insert, MAX_GAIN_DB, MAX_TEMPO_BPM, MIN_GAIN_DB,
    MIN_TEMPO_BPM, MidiData, Project, Track, TrackId, TrackKind,
};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use super::xml::{self, Node};
use super::{
    AUDIO_PREFIX, AudioInfo, CLIP_NOTE_PREFIX, DEVICE_PREFIX, DawError, FORMAT_VERSION,
    METADATA_ENTRY, PROJECT_ENTRY, Report, db_to_linear, effect_id, num,
};

/// Löst den Audiopfad eines Clips (relativ zum Projekt) in eine Datei auf.
pub type Resolve<'a> = &'a dyn Fn(&str) -> Option<PathBuf>;
/// Liefert Rate, Kanäle und Länge einer Audiodatei.
pub type Probe<'a> = &'a dyn Fn(&Path) -> Option<AudioInfo>;

struct AudioEntry {
    entry: String,
    path: PathBuf,
    info: AudioInfo,
}

struct Ctx<'a> {
    project: &'a Project,
    resolve: Resolve<'a>,
    probe: Probe<'a>,
    audio: HashMap<String, Option<AudioEntry>>,
    order: Vec<String>,
    report: Report,
}

/// Dateiname für den Eintrag im ZIP: nur ASCII-Buchstaben, Ziffern, `-`, `_`, `.`.
pub(super) fn safe_name(path: &str) -> String {
    let base = path.rsplit(['/', '\\']).next().unwrap_or("audio");
    let (stem, ext) = match base.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s, e),
        _ => (base, ""),
    };
    let clean = |s: &str, max: usize| -> String {
        s.chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' {
                    c
                } else {
                    '_'
                }
            })
            .take(max)
            .collect()
    };
    let stem = clean(stem, 60);
    let stem = if stem.is_empty() {
        "audio".to_owned()
    } else {
        stem
    };
    let ext = clean(ext, 5).to_ascii_lowercase();
    if ext.is_empty() {
        stem
    } else {
        format!("{stem}.{ext}")
    }
}

fn track_ref(id: TrackId) -> String {
    format!("trk{}", id.0)
}

fn channel_ref(id: TrackId) -> String {
    format!("ch{}", id.0)
}

fn volume_ref(id: TrackId) -> String {
    format!("vol{}", id.0)
}

fn pan_ref(id: TrackId) -> String {
    format!("pan{}", id.0)
}

fn send_volume_ref(track: TrackId, target: TrackId) -> String {
    format!("sv{}_{}", track.0, target.0)
}

fn param_ref(insert: woa_model::InsertId, index: usize) -> String {
    format!("dp{}_{index}", insert.0)
}

const MASTER_CHANNEL: &str = "chmaster";

/// Einheit eines Effekt-Reglers in DAWproject-Schreibweise.
fn unit_of(wu: &str) -> &'static str {
    match wu {
        "dB" => "decibel",
        "Hz" => "hertz",
        _ => "linear",
    }
}

impl Ctx<'_> {
    fn audio_for(&mut self, rel: &str) -> Option<&AudioEntry> {
        if !self.audio.contains_key(rel) {
            let found = (self.resolve)(rel).and_then(|path| {
                let info = (self.probe)(&path)?;
                (info.seconds.is_finite() && info.seconds > 0.0).then_some((path, info))
            });
            let entry = found.map(|(path, info)| AudioEntry {
                entry: format!(
                    "{AUDIO_PREFIX}{:03}-{}",
                    self.order.len() + 1,
                    safe_name(rel)
                ),
                path,
                info,
            });
            if entry.is_some() {
                self.order.push(rel.to_owned());
            } else {
                self.report
                    .note(format!("Audiodatei „{rel}“ fehlt oder ist nicht lesbar"));
            }
            self.audio.insert(rel.to_owned(), entry);
        }
        self.audio.get(rel).and_then(Option::as_ref)
    }

    fn real(name: &str, id: Option<String>, unit: &str, value: f64, min: f64, max: f64) -> Node {
        Node::new(name)
            .with_opt("id", id)
            .with("unit", unit)
            .with("value", num(value))
            .with("min", num(min))
            .with("max", num(max))
    }

    fn insert_device(&mut self, track: &Track, ins: &Insert) -> Node {
        if ins.sidechain.is_some() {
            self.report.note(format!(
                "Spur „{}“: Sidechain von {} wird nicht übertragen",
                track.name,
                ins.kind.label()
            ));
        }
        let mut params = Node::new("Parameters");
        for (i, spec) in ins.kind.params().iter().enumerate() {
            let value = ins.params.get(i).copied().unwrap_or(spec.default);
            params = params.child(
                Node::new("RealParameter")
                    .with("id", param_ref(ins.id, i))
                    .with("name", spec.name)
                    .with("parameterID", i.to_string())
                    .with("unit", unit_of(spec.unit))
                    .with("value", num(value))
                    .with("min", num(spec.min))
                    .with("max", num(spec.max)),
            );
        }
        Node::new("Device")
            .with("id", format!("dev{}", ins.id.0))
            .with("name", ins.kind.label())
            .with("deviceRole", "audioFX")
            .with("deviceName", ins.kind.label())
            .with(
                "deviceID",
                format!("{DEVICE_PREFIX}{}", effect_id(ins.kind)),
            )
            .with("deviceVendor", "WOA")
            .with("loaded", "true")
            .child(params)
            .child(
                Node::new("Enabled")
                    .with("id", format!("en{}", ins.id.0))
                    .with("value", (!ins.bypass).to_string()),
            )
    }

    fn channel(&mut self, track: &Track, effect_buses: &HashSet<TrackId>) -> Node {
        let role = match track.kind {
            TrackKind::Bus if effect_buses.contains(&track.id) => "effect",
            TrackKind::Bus => "submix",
            _ => "regular",
        };
        let destination = track
            .output
            .map_or_else(|| MASTER_CHANNEL.to_owned(), channel_ref);
        let mut ch = Node::new("Channel")
            .with("id", channel_ref(track.id))
            .with("role", role)
            .with("audioChannels", "2")
            .with("destination", destination)
            .with("solo", track.solo.to_string());
        if !track.inserts.is_empty() {
            let mut devices = Node::new("Devices");
            for ins in &track.inserts {
                devices = devices.child(self.insert_device(track, ins));
            }
            ch = ch.child(devices);
        }
        ch = ch
            .child(
                Node::new("Mute")
                    .with("id", format!("mute{}", track.id.0))
                    .with("value", track.mute.to_string()),
            )
            .child(Self::real(
                "Pan",
                Some(pan_ref(track.id)),
                "normalized",
                (track.pan + 1.0) / 2.0,
                0.0,
                1.0,
            ));
        if !track.sends.is_empty() {
            let mut sends = Node::new("Sends");
            for s in &track.sends {
                let off = s.level_db <= MIN_GAIN_DB;
                sends = sends.child(
                    Node::new("Send")
                        .with("id", format!("snd{}_{}", track.id.0, s.target.0))
                        .with("destination", channel_ref(s.target))
                        .with("type", if s.pre_fader { "pre" } else { "post" })
                        .child(
                            Node::new("Enable")
                                .with("id", format!("sen{}_{}", track.id.0, s.target.0))
                                .with("value", (!off).to_string()),
                        )
                        .child(Self::real(
                            "Volume",
                            Some(send_volume_ref(track.id, s.target)),
                            "linear",
                            s.linear(),
                            0.0,
                            db_to_linear(woa_model::MAX_SEND_DB),
                        )),
                );
            }
            ch = ch.child(sends);
        }
        ch.child(Self::real(
            "Volume",
            Some(volume_ref(track.id)),
            "linear",
            db_to_linear(track.gain_db),
            0.0,
            db_to_linear(MAX_GAIN_DB),
        ))
    }

    fn structure_track(&mut self, track: &Track, effect_buses: &HashSet<TrackId>) -> Node {
        let content = match track.kind {
            TrackKind::Audio => "audio",
            TrackKind::Midi => "notes",
            TrackKind::Bus => "tracks",
        };
        if !track.take_folders.is_empty() {
            self.report.note(format!(
                "Spur „{}“: {} Take-Ordner werden nicht übertragen (die Clips bleiben)",
                track.name,
                track.take_folders.len()
            ));
        }
        Node::new("Track")
            .with("id", track_ref(track.id))
            .with("name", track.name.clone())
            .with("contentType", content)
            .with("loaded", "true")
            .child(self.channel(track, effect_buses))
    }

    /// Clip-Notiz mit dem, was DAWproject nicht kennt.
    fn clip_note(clip: &Clip) -> Option<String> {
        let mut parts: Vec<String> = Vec::new();
        if clip.gain_db != 0.0 {
            parts.push(format!("gain={}", num(clip.gain_db)));
        }
        if clip.pitch_semitones != 0.0 {
            parts.push(format!("pitch={}", num(clip.pitch_semitones)));
        }
        if let Some(b) = clip.follow_tempo_bpm {
            parts.push(format!("follow={}", num(b)));
        }
        if clip.stretch != 1.0 {
            parts.push(format!("stretch={}", num(clip.stretch)));
        }
        if clip.fade_in_curve != FadeCurve::Linear {
            parts.push(format!("fic={}", curve_id(clip.fade_in_curve)));
        }
        if clip.fade_out_curve != FadeCurve::Linear {
            parts.push(format!("foc={}", curve_id(clip.fade_out_curve)));
        }
        (!parts.is_empty()).then(|| format!("{CLIP_NOTE_PREFIX} {}", parts.join(" ")))
    }

    fn midi_content(midi: &MidiData) -> Node {
        let mut notes = Node::new("Notes").with("timeUnit", "beats");
        for n in &midi.notes {
            notes = notes.child(
                Node::new("Note")
                    .with("time", num(n.beat))
                    .with("duration", num(n.length_beats))
                    .with("channel", "0")
                    .with("key", n.pitch.to_string())
                    .with("vel", num(f64::from(n.velocity) / 127.0)),
            );
        }
        if midi.cc.is_empty() {
            return notes;
        }
        let mut controllers: Vec<u8> = midi.cc.iter().map(|c| c.controller).collect();
        controllers.sort_unstable();
        controllers.dedup();
        let mut lanes = Node::new("Lanes").with("timeUnit", "beats").child(notes);
        for ctl in controllers {
            let mut points = Node::new("Points")
                .with("timeUnit", "beats")
                .with("unit", "normalized")
                .child(
                    Node::new("Target")
                        .with("expression", "channelController")
                        .with("channel", "0")
                        .with("controller", ctl.to_string()),
                );
            for c in midi.cc.iter().filter(|c| c.controller == ctl) {
                points = points.child(
                    Node::new("RealPoint")
                        .with("time", num(c.beat))
                        .with("value", num(f64::from(c.value) / 127.0))
                        .with("interpolation", "hold"),
                );
            }
            lanes = lanes.child(points);
        }
        lanes
    }

    fn clip(&mut self, clip: &Clip) -> Node {
        let mut n = Node::new("Clip")
            .with("name", clip.name.clone())
            .with("time", num(clip.start_beats))
            .with("duration", num(clip.length_beats))
            .with("contentTimeUnit", "beats");
        if clip.fade_in_beats > 0.0 || clip.fade_out_beats > 0.0 {
            n = n.with("fadeTimeUnit", "beats");
            if clip.fade_in_beats > 0.0 {
                n = n.with("fadeInTime", num(clip.fade_in_beats));
            }
            if clip.fade_out_beats > 0.0 {
                n = n.with("fadeOutTime", num(clip.fade_out_beats));
            }
        }
        if let Some(note) = Self::clip_note(clip) {
            n = n.with("comment", note);
        }
        if let Some(midi) = &clip.midi {
            return n.with("playStart", "0").child(Self::midi_content(midi));
        }
        let Some(rel) = clip.audio.clone() else {
            return n; // Platzhalter-Clip ohne Ton
        };
        let tempo = self.project.tempo_bpm;
        let Some(entry) = self.audio_for(&rel) else {
            return n;
        };
        let (entry_name, info) = (entry.entry.clone(), entry.info);
        let slope = clip.stretch * clip.source_bpm(tempo) / 60.0;
        let play_start = clip.offset_beats * clip.stretch;
        let audio = Node::new("Audio")
            .with("timeUnit", "seconds")
            .with("duration", num(info.seconds))
            .with("sampleRate", info.sample_rate.to_string())
            .with("channels", info.channels.to_string())
            .with("algorithm", "stretch")
            .child(Node::new("File").with("path", entry_name));
        let warps = Node::new("Warps")
            .with("timeUnit", "beats")
            .with("contentTimeUnit", "seconds")
            .child(audio)
            .child(Node::new("Warp").with("time", "0").with("contentTime", "0"))
            .child(
                Node::new("Warp")
                    .with("time", num(slope * info.seconds))
                    .with("contentTime", num(info.seconds)),
            );
        n.with("playStart", num(play_start))
            .with("playStop", num(play_start + clip.length_beats))
            .child(warps)
    }

    fn lane(track: &Track, lane: &AutomationLane) -> Option<Node> {
        let range = lane.target.range_in(track).ok()?;
        if lane.points.is_empty() {
            return None;
        }
        let (target_ref, unit, conv): (String, &str, Box<dyn Fn(f64) -> f64>) = match lane.target {
            AutoTarget::Gain => (volume_ref(track.id), "linear", Box::new(db_to_linear)),
            AutoTarget::Pan => (
                pan_ref(track.id),
                "normalized",
                Box::new(|v| (v + 1.0) / 2.0),
            ),
            AutoTarget::Send { target } => (
                send_volume_ref(track.id, target),
                "linear",
                Box::new(db_to_linear),
            ),
            AutoTarget::InsertParam { insert, param } => (
                param_ref(insert, param),
                unit_of(range.unit),
                Box::new(|v| v),
            ),
        };
        let interpolation = if range.step { "hold" } else { "linear" };
        let mut points = Node::new("Points")
            .with("timeUnit", "beats")
            .with("unit", unit)
            .child(Node::new("Target").with("parameter", target_ref));
        for p in &lane.points {
            points = points.child(
                Node::new("RealPoint")
                    .with("time", num(p.beat))
                    .with("value", num(conv(p.value)))
                    .with("interpolation", interpolation),
            );
        }
        Some(points)
    }

    fn arrangement(&mut self) -> Node {
        let mut lanes = Node::new("Lanes")
            .with("id", "arrangementlanes")
            .with("timeUnit", "beats");
        let project = self.project;
        for t in &project.tracks {
            let mut tl = Node::new("Lanes")
                .with("id", format!("lanes{}", t.id.0))
                .with("track", track_ref(t.id))
                .with("timeUnit", "beats");
            let mut used = false;
            if !t.clips.is_empty() {
                let mut clips = Node::new("Clips")
                    .with("id", format!("clips{}", t.id.0))
                    .with("timeUnit", "beats");
                for c in &t.clips {
                    clips = clips.child(self.clip(c));
                    self.report.clips += 1;
                }
                tl = tl.child(clips);
                used = true;
            }
            for l in &t.automation {
                match Self::lane(t, l) {
                    Some(p) => {
                        tl = tl.child(p);
                        used = true;
                    }
                    None if l.points.is_empty() => {}
                    None => self.report.note(format!(
                        "Spur „{}“: Automation eines nicht mehr vorhandenen Ziels übersprungen",
                        t.name
                    )),
                }
            }
            if used {
                lanes = lanes.child(tl);
            }
        }
        Node::new("Arrangement")
            .with("id", "arrangement")
            .child(lanes)
    }
}

fn curve_id(c: FadeCurve) -> &'static str {
    match c {
        FadeCurve::Linear => "linear",
        FadeCurve::EqualPower => "equal_power",
        FadeCurve::SCurve => "s_curve",
    }
}

/// Baut `project.xml` und sammelt die Audiodateien. Gibt den Baum, die Dateien
/// (Eintrag im ZIP, Quelle) und den Bericht zurück.
fn build(
    project: &Project,
    resolve: Resolve<'_>,
    probe: Probe<'_>,
) -> (Node, Vec<(String, PathBuf)>, Report) {
    let mut cx = Ctx {
        project,
        resolve,
        probe,
        audio: HashMap::new(),
        order: Vec::new(),
        report: Report::default(),
    };
    cx.report.tracks = project.tracks.len();
    let effect_buses: HashSet<TrackId> = project
        .tracks
        .iter()
        .flat_map(|t| t.sends.iter().map(|s| s.target))
        .collect();
    let tempo = project.tempo_bpm.clamp(MIN_TEMPO_BPM, MAX_TEMPO_BPM);
    let mut structure = Node::new("Structure").child(
        Node::new("Track")
            .with("id", "trkmaster")
            .with("name", "Master")
            .with("loaded", "true")
            .child(
                Node::new("Channel")
                    .with("id", MASTER_CHANNEL)
                    .with("role", "master")
                    .with("audioChannels", "2")
                    .child(Ctx::real(
                        "Volume",
                        Some("volmaster".into()),
                        "linear",
                        1.0,
                        0.0,
                        2.0,
                    )),
            ),
    );
    for t in &project.tracks {
        structure = structure.child(cx.structure_track(t, &effect_buses));
    }
    let arrangement = cx.arrangement();
    if project.tracks.iter().any(|t| !t.inserts.is_empty()) {
        cx.report.note(
            "Effekte stehen als WOA-Geräte in der Datei; andere Programme ignorieren sie"
                .to_owned(),
        );
    }
    let special = project
        .tracks
        .iter()
        .flat_map(|t| &t.clips)
        .filter(|c| {
            c.pitch_semitones != 0.0
                || c.gain_db != 0.0
                || !c.fade_in_curve.is_linear()
                || !c.fade_out_curve.is_linear()
        })
        .count();
    if special > 0 {
        cx.report.note(format!(
            "{special} Clips mit Tonhöhe, Clip-Verstärkung oder Blendenform: nur WOA liest das zurück, andere Programme ignorieren es"
        ));
    }
    let root = Node::new("Project")
        .with("version", FORMAT_VERSION)
        .child(
            Node::new("Application")
                .with("name", "WOA")
                .with("version", env!("CARGO_PKG_VERSION")),
        )
        .child(
            Node::new("Transport")
                .child(Ctx::real(
                    "Tempo",
                    Some("tempo".into()),
                    "bpm",
                    tempo,
                    MIN_TEMPO_BPM,
                    MAX_TEMPO_BPM,
                ))
                .child(
                    Node::new("TimeSignature")
                        .with("id", "timesignature")
                        .with("numerator", "4")
                        .with("denominator", "4"),
                ),
        )
        .child(structure)
        .child(arrangement);
    let files: Vec<(String, PathBuf)> = cx
        .order
        .iter()
        .filter_map(|rel| cx.audio.get(rel).and_then(Option::as_ref))
        .map(|e| (e.entry.clone(), e.path.clone()))
        .collect();
    cx.report.audio_files = files.len();
    (root, files, cx.report)
}

/// `project.xml` als Text (Tests, Schema-Prüfung).
///
/// # Errors
/// Nur wenn der Schreiber im Speicher scheitert.
pub fn project_xml(
    project: &Project,
    resolve: Resolve<'_>,
    probe: Probe<'_>,
) -> Result<(Vec<u8>, Report), DawError> {
    let (root, _, report) = build(project, resolve, probe);
    Ok((xml::write(&root)?, report))
}

/// Schreibt das ZIP in `w`.
///
/// # Errors
/// Schreib- oder Lesefehler, ZIP-Fehler.
pub fn export<W: Write + Seek>(
    project: &Project,
    resolve: Resolve<'_>,
    probe: Probe<'_>,
    w: W,
) -> Result<Report, DawError> {
    let (root, files, report) = build(project, resolve, probe);
    let mut zip = ZipWriter::new(w);
    let text = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    zip.start_file(METADATA_ENTRY, text)?;
    zip.write_all(&xml::write_metadata(&project.meta.name))?;
    zip.start_file(PROJECT_ENTRY, text)?;
    zip.write_all(&xml::write(&root)?)?;
    // Audio ist schon komprimiert oder zu groß für den Gewinn: unverändert ablegen.
    let raw = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .large_file(true);
    for (entry, path) in &files {
        zip.start_file(entry.as_str(), raw)?;
        let mut f = std::fs::File::open(path)?;
        std::io::copy(&mut f, &mut zip)?;
    }
    zip.finish()?;
    Ok(report)
}

/// Schreibt `out` atomar (Temp-Datei im selben Ordner, dann umbenennen).
///
/// # Errors
/// Wie [`export`], dazu Zugriff auf den Zielordner.
pub fn export_file(
    project: &Project,
    resolve: Resolve<'_>,
    probe: Probe<'_>,
    out: &Path,
) -> Result<Report, DawError> {
    let dir = match out.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    let report = export(project, resolve, probe, tmp.as_file_mut())?;
    tmp.as_file().sync_all()?;
    tmp.persist(out)
        .map_err(|e| DawError::Io(e.error.to_string()))?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_for_the_zip_are_plain() {
        assert_eq!(safe_name("aufnahmen/Über mich!.WAV"), "_ber_mich_.wav");
        assert_eq!(safe_name("../../etc/passwd"), "passwd");
        assert_eq!(safe_name(""), "audio");
        assert_eq!(safe_name("a.b.flac"), "a_b.flac");
    }
}
