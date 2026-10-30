//! Paket 27: DAWproject export, import and round trip.

use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};

use woa_io::dawproject::export::{export, export_file, project_xml};
use woa_io::dawproject::import::{import, import_to_folder};
use woa_io::dawproject::xml::{self, Node};
use woa_io::dawproject::{AudioInfo, DawError, Report, probe_wav};
use woa_model::{
    AutoMode, AutoPoint, AutoTarget, AutomationLane, Clip, ClipId, EffectKind, Insert, InsertId,
    MidiData, MidiNote, Project, Send, Track, TrackId, TrackKind,
};

fn write_wav(path: &Path, secs: f64) {
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: 48_000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut w = hound::WavWriter::create(path, spec).expect("wav");
    for i in 0..(48_000.0 * secs) as usize {
        let s = ((i as f32 * 0.05).sin() * 8000.0) as i16;
        w.write_sample(s).expect("sample");
        w.write_sample(s).expect("sample");
    }
    w.finalize().expect("finalize");
}

fn sample_project() -> Project {
    let mut p = Project::new("Probe <1> & Co");
    p.tempo_bpm = 100.0;
    let mut bus = Track::new(TrackId(1), "Hall", TrackKind::Bus);
    bus.inserts
        .push(Insert::new(InsertId(2), EffectKind::Reverb));
    let mut a = Track::new(TrackId(3), "Sprecher", TrackKind::Audio);
    a.gain_db = -6.0;
    a.pan = -0.5;
    a.mute = true;
    a.output = Some(TrackId(1));
    a.sends.push(Send {
        target: TrackId(1),
        level_db: -12.0,
        pre_fader: false,
    });
    let mut c = Clip::new(ClipId(4), "Intro", 4.0, 8.0, Some("audio/a.wav".into()));
    c.offset_beats = 2.0;
    c.gain_db = 3.0;
    c.pitch_semitones = 2.0;
    c.fade_in_beats = 1.0;
    c.fade_out_beats = 0.5;
    a.clips.push(c);
    a.inserts
        .push(Insert::new(InsertId(5), EffectKind::Compressor));
    a.automation.push(AutomationLane {
        target: AutoTarget::Gain,
        mode: AutoMode::Read,
        points: vec![
            AutoPoint {
                beat: 0.0,
                value: -12.0,
            },
            AutoPoint {
                beat: 8.0,
                value: 0.0,
            },
        ],
    });
    let mut m = Track::new(TrackId(6), "Keys", TrackKind::Midi);
    m.clips.push(Clip::new_midi(
        ClipId(7),
        "Akkord",
        0.0,
        4.0,
        MidiData {
            notes: vec![MidiNote {
                beat: 1.0,
                length_beats: 2.0,
                pitch: 64,
                velocity: 100,
            }],
            cc: vec![],
        },
    ));
    p.tracks = vec![bus, a, m];
    p.next_id = 8;
    p
}

fn resolver(dir: PathBuf) -> impl Fn(&str) -> Option<PathBuf> {
    move |rel| Some(dir.join(rel)).filter(|p| p.exists())
}

fn probe(p: &Path) -> Option<AudioInfo> {
    probe_wav(p)
}

fn exported() -> (tempfile::TempDir, Project, PathBuf) {
    let dir = tempfile::tempdir().expect("tmp");
    std::fs::create_dir_all(dir.path().join("audio")).expect("mkdir");
    write_wav(&dir.path().join("audio/a.wav"), 6.0);
    let p = sample_project();
    let out = dir.path().join("probe.dawproject");
    let r = resolver(dir.path().to_path_buf());
    let rep = export_file(&p, &r, &probe, &out).expect("export");
    assert_eq!(rep.audio_files, 1);
    (dir, p, out)
}

#[test]
fn round_trip_keeps_the_arrangement() {
    let (dir, orig, out) = exported();
    let dest = dir.path().join("neu");
    let (back, rep) = import_to_folder(&out, &dest).expect("import");
    assert_eq!(rep.audio_files, 1);
    assert_eq!(back.meta.name, "Probe <1> & Co");
    assert!((back.tempo_bpm - 100.0).abs() < 1e-9);
    assert_eq!(back.tracks.len(), 3);
    let by = |n: &str| back.tracks.iter().find(|t| t.name == n).expect("track");
    let (bus, a, m) = (by("Hall"), by("Sprecher"), by("Keys"));
    assert_eq!(bus.kind, TrackKind::Bus);
    assert_eq!(m.kind, TrackKind::Midi);
    assert!((a.gain_db + 6.0).abs() < 1e-6);
    assert!((a.pan + 0.5).abs() < 1e-9);
    assert!(a.mute);
    assert_eq!(a.output, Some(bus.id));
    assert_eq!(a.sends.len(), 1);
    assert_eq!(a.sends[0].target, bus.id);
    assert!((a.sends[0].level_db + 12.0).abs() < 1e-6);
    assert_eq!(a.inserts.len(), 1);
    assert_eq!(a.inserts[0].kind, EffectKind::Compressor);
    assert_eq!(a.inserts[0].params, orig.tracks[1].inserts[0].params);
    assert_eq!(bus.inserts[0].kind, EffectKind::Reverb);
    let c = &a.clips[0];
    let o = &orig.tracks[1].clips[0];
    assert!((c.start_beats - o.start_beats).abs() < 1e-6);
    assert!((c.length_beats - o.length_beats).abs() < 1e-6);
    assert!(
        (c.offset_beats - o.offset_beats).abs() < 1e-6,
        "{}",
        c.offset_beats
    );
    assert!((c.stretch - o.stretch).abs() < 1e-9);
    assert!((c.gain_db - 3.0).abs() < 1e-9 && (c.pitch_semitones - 2.0).abs() < 1e-9);
    assert!((c.fade_in_beats - 1.0).abs() < 1e-9 && (c.fade_out_beats - 0.5).abs() < 1e-9);
    let audio = c.audio.as_deref().expect("audio");
    assert!(dest.join(audio).is_file());
    assert_eq!(a.automation.len(), 1);
    assert_eq!(a.automation[0].points.len(), 2);
    assert!((a.automation[0].points[0].value + 12.0).abs() < 1e-6);
    let want = orig.tracks[2].clips[0].midi.as_ref().expect("midi");
    assert_eq!(m.clips[0].midi.as_ref().expect("midi").notes, want.notes);
}

#[test]
fn stretched_clip_round_trips() {
    let dir = tempfile::tempdir().expect("tmp");
    std::fs::create_dir_all(dir.path().join("audio")).expect("mkdir");
    write_wav(&dir.path().join("audio/a.wav"), 6.0);
    let mut p = Project::new("S");
    p.tempo_bpm = 120.0;
    let mut t = Track::new(TrackId(1), "T", TrackKind::Audio);
    let mut c = Clip::new(ClipId(2), "c", 0.0, 8.0, Some("audio/a.wav".into()));
    c.stretch = 2.0;
    c.offset_beats = 1.0;
    c.follow_tempo_bpm = Some(90.0);
    t.clips.push(c.clone());
    p.tracks = vec![t];
    p.next_id = 3;
    let out = dir.path().join("s.dawproject");
    export_file(&p, &resolver(dir.path().to_path_buf()), &probe, &out).expect("export");
    let (back, _) = import_to_folder(&out, &dir.path().join("neu")).expect("import");
    let b = &back.tracks[0].clips[0];
    assert!((b.stretch - c.stretch).abs() < 1e-9, "{}", b.stretch);
    assert!(
        (b.offset_beats - c.offset_beats).abs() < 1e-6,
        "{}",
        b.offset_beats
    );
    assert_eq!(b.follow_tempo_bpm, Some(90.0));
}

/// Rundet Kommazahlen auf 9 Stellen: pow/log10 liefern je nach Plattform (glibc, MSVC) in
/// der letzten Stelle andere Werte, der Schnappschuss darf daran nicht hängen.
fn round_floats(n: &mut Node) {
    for (_, v) in &mut n.attrs {
        if v.contains('.')
            && let Ok(x) = v.parse::<f64>()
        {
            *v = format!("{x:.9}");
        }
    }
    for c in &mut n.children {
        round_floats(c);
    }
}

#[test]
fn project_xml_snapshot_is_stable() {
    let (dir, p, _) = exported();
    let r = resolver(dir.path().to_path_buf());
    let (bytes, _) = project_xml(&p, &r, &probe).expect("xml");
    let mut tree = xml::parse(&bytes).expect("parse");
    round_floats(&mut tree);
    let text = String::from_utf8(xml::write(&tree).expect("write")).expect("utf8");
    // Die Programmversion steigt mit jedem Release und gehört nicht in den Schnappschuss.
    let text = text.replace(
        concat!("version=\"", env!("CARGO_PKG_VERSION"), "\"/>"),
        "version=\"X\"/>",
    );
    insta::assert_snapshot!(text);
}

#[test]
fn exported_xml_is_valid_against_the_schema() {
    if std::process::Command::new("xmllint")
        .arg("--version")
        .output()
        .is_err()
    {
        eprintln!("xmllint fehlt: Schema-Prüfung übersprungen");
        return;
    }
    let (dir, p, _) = exported();
    let (bytes, _) = project_xml(&p, &resolver(dir.path().to_path_buf()), &probe).expect("xml");
    let file = dir.path().join("project.xml");
    std::fs::write(&file, bytes).expect("write");
    let xsd = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/schema/Project.xsd");
    let out = std::process::Command::new("xmllint")
        .args(["--noout", "--schema"])
        .arg(&xsd)
        .arg(&file)
        .output()
        .expect("xmllint");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn zip_with(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (n, b) in entries {
        z.start_file(*n, zip::write::SimpleFileOptions::default())
            .expect("start");
        z.write_all(b).expect("write");
    }
    z.finish().expect("finish").into_inner()
}

fn import_bytes(bytes: &[u8]) -> (tempfile::TempDir, Result<(Project, Report), DawError>) {
    let dir = tempfile::tempdir().expect("tmp");
    let f = dir.path().join("x.dawproject");
    std::fs::write(&f, bytes).expect("write");
    let r = import(&f, &dir.path().join("p"));
    (dir, r)
}

#[test]
fn foreign_file_in_bitwig_conventions() {
    // Handgebaute Datei nach dem Muster der Referenztests: Pan normalisiert, Volume linear,
    // Master als Track mit Channel role=master, Audio über Warps.
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<Project version="1.0">
  <Application name="Fremd" version="1"/>
  <Transport><Tempo unit="bpm" value="140"/><TimeSignature numerator="4" denominator="4"/></Transport>
  <Structure>
    <Track id="t1" name="Gitarre" contentType="audio" loaded="true">
      <Channel id="c1" role="regular" audioChannels="2" destination="cm">
        <Volume id="v1" unit="linear" value="0.5"/>
        <Pan id="p1" unit="normalized" value="1.0"/>
        <Mute value="false"/>
        <Devices><Vst3Plugin id="d1" deviceName="Fremdplugin" deviceRole="effect"/></Devices>
      </Channel>
    </Track>
    <Track id="tm" name="Master" contentType="audio tracks">
      <Channel id="cm" role="master" audioChannels="2"><Volume unit="linear" value="1"/></Channel>
    </Track>
  </Structure>
  <Arrangement id="a1">
    <Lanes timeUnit="beats" id="l0">
      <Lanes track="t1" id="l1">
        <Clips id="cl1">
          <Clip time="0" duration="4" contentTimeUnit="seconds" name="Riff">
            <Warps contentTimeUnit="seconds" id="w1">
              <Audio algorithm="stretch" channels="2" duration="3" sampleRate="48000" id="au1">
                <File path="audio/riff.wav"/>
              </Audio>
              <Warp time="0" contentTime="0"/>
              <Warp time="4" contentTime="1.7142857142857142"/>
            </Warps>
          </Clip>
        </Clips>
      </Lanes>
    </Lanes>
  </Arrangement>
</Project>"#;
    let dir = tempfile::tempdir().expect("tmp");
    write_wav(&dir.path().join("riff.wav"), 1.0);
    let wav = std::fs::read(dir.path().join("riff.wav")).expect("read");
    let bytes = zip_with(&[("project.xml", xml.as_bytes()), ("audio/riff.wav", &wav)]);
    let (_d, r) = import_bytes(&bytes);
    let (p, rep) = r.expect("import");
    assert_eq!(p.tracks.len(), 1);
    let t = &p.tracks[0];
    assert!((p.tempo_bpm - 140.0).abs() < 1e-9);
    assert!((t.gain_db - (-6.0206)).abs() < 1e-3);
    assert!((t.pan - 1.0).abs() < 1e-9);
    assert_eq!(t.clips.len(), 1);
    assert!((t.clips[0].length_beats - 4.0).abs() < 1e-9);
    assert!(t.clips[0].audio.is_some());
    assert!(
        rep.notes.iter().any(|n| n.contains("Fremdplugin")),
        "{:?}",
        rep.notes
    );
}

#[test]
fn hostile_files_fail_cleanly() {
    assert!(import_bytes(b"not a zip").1.is_err());
    assert!(import_bytes(&zip_with(&[("a.txt", b"x")])).1.is_err());
    let deep = format!("<Project>{}</Project>", "<a>".repeat(200));
    assert!(
        import_bytes(&zip_with(&[("project.xml", deep.as_bytes())]))
            .1
            .is_err()
    );
    assert!(
        import_bytes(&zip_with(&[("project.xml", b"<Other/>")]))
            .1
            .is_err()
    );
    // Pfad-Trick im Audiopfad: wird nicht gelesen, nichts außerhalb geschrieben.
    let xml = r#"<Project version="1.0"><Transport><Tempo value="120"/></Transport><Structure>
<Track id="t" name="x" contentType="audio"><Channel id="c" role="regular"/></Track></Structure>
<Arrangement><Lanes timeUnit="beats"><Lanes track="t"><Clips><Clip time="0" duration="2" contentTimeUnit="seconds">
<Audio duration="1" sampleRate="48000" channels="2"><File path="../../evil.wav"/></Audio></Clip></Clips></Lanes></Lanes></Arrangement></Project>"#;
    let (d, r) = import_bytes(&zip_with(&[
        ("project.xml", xml.as_bytes()),
        ("../../evil.wav", b"RIFF"),
    ]));
    let (p, rep) = r.expect("import");
    assert!(p.tracks[0].clips[0].audio.is_none());
    assert!(!rep.notes.is_empty());
    assert!(!d.path().join("../evil.wav").exists());
}

#[test]
fn export_zip_has_the_expected_entries() {
    let dir = tempfile::tempdir().expect("tmp");
    std::fs::create_dir_all(dir.path().join("audio")).expect("mkdir");
    write_wav(&dir.path().join("audio/a.wav"), 1.0);
    let mut buf = Cursor::new(Vec::new());
    export(
        &sample_project(),
        &resolver(dir.path().to_path_buf()),
        &probe,
        &mut buf,
    )
    .expect("export");
    let z = zip::ZipArchive::new(Cursor::new(buf.into_inner())).expect("zip");
    let names: Vec<_> = z
        .file_names()
        .filter_map(Result::ok)
        .map(std::borrow::Cow::into_owned)
        .collect();
    assert!(names.contains(&"project.xml".to_owned()));
    assert!(names.contains(&"metadata.xml".to_owned()));
    assert!(names.iter().any(|n| n.starts_with("audio/")));
}
