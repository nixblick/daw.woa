//! Paket 26: Export-Formate und Stems, gemessen im Offline-Render und beim Zurücklesen.
//! Tests mit ffmpeg überspringen sich mit einer Zeile Log, wenn es fehlt (CI-Läufer).

use std::sync::Arc;

use woa_engine::decode::decode_file;
use woa_engine::export::{BitDepth, ExportFormat, ExportSpec, export_with};
use woa_engine::ffmpeg::{Codec, Ffmpeg, MISSING};
use woa_engine::stems::{StemSelect, render_stems, stem_file_names, write_stems};
use woa_model::{Clip, Command, Project, Send, Session, TrackId, TrackKind};

const RATE: u32 = 48_000;

fn sine(freq: f32, n: usize) -> Arc<[f32]> {
    (0..n)
        .map(|i| 0.3 * (std::f32::consts::TAU * freq * i as f32 / RATE as f32).sin())
        .collect::<Vec<_>>()
        .into()
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|s| s * s).sum::<f32>() / x.len().max(1) as f32).sqrt()
}

fn load(rel: &str) -> Option<Arc<[f32]>> {
    let f = match rel {
        "a" => 220.0,
        "b" => 330.0,
        "c" => 440.0,
        "d" => 550.0,
        _ => return None,
    };
    Some(sine(f, 4 * RATE as usize))
}

fn add_track(s: &mut Session, name: &str, kind: TrackKind, audio: Option<&str>) -> TrackId {
    let t = s.new_track_id();
    s.apply(Command::add_track(t, name, kind)).expect("track");
    if let Some(a) = audio {
        let c = s.new_clip_id();
        s.apply(Command::AddClip {
            track: t,
            clip: Clip::new(c, a, 0.0, 4.0, Some(a.into())),
            index: None,
        })
        .expect("clip");
    }
    t
}

/// A direkt, B über den Bus, D mit Send auf den Bus, C stumm, dazu Pan und Pegel.
fn project() -> Project {
    let mut s = Session::new("stems");
    let a = add_track(&mut s, "A", TrackKind::Audio, Some("a"));
    let b = add_track(&mut s, "B", TrackKind::Audio, Some("b"));
    let c = add_track(&mut s, "C", TrackKind::Audio, Some("c"));
    let d = add_track(&mut s, "D", TrackKind::Audio, Some("d"));
    let bus = add_track(&mut s, "Bus", TrackKind::Bus, None);
    s.apply(Command::SetOutput {
        track: b,
        output: Some(bus),
    })
    .expect("route");
    s.apply(Command::AddSend {
        track: d,
        send: Send {
            target: bus,
            level_db: -6.0,
            pre_fader: false,
        },
        index: None,
    })
    .expect("send");
    s.apply(Command::SetMute {
        track: c,
        mute: true,
    })
    .expect("mute");
    s.apply(Command::SetGain {
        track: a,
        gain_db: -3.0,
    })
    .expect("gain");
    s.apply(Command::SetPan { track: d, pan: 0.5 })
        .expect("pan");
    s.project().clone()
}

fn render(p: &Project) -> woa_engine::stems::StemRender {
    render_stems(p, RATE, 4.0 * 0.5, StemSelect::default(), &mut load).expect("stems")
}

#[test]
fn top_level_stems_sum_to_the_mixdown() {
    let p = project();
    let r = render(&p);
    let frames = r.mix_left.len();
    assert_eq!(frames, 2 * RATE as usize);
    let names: Vec<_> = r.stems.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["A", "B", "D", "Bus"], "C ist stumm");
    assert_eq!(r.skipped, ["C"]);
    for s in &r.stems {
        assert_eq!(s.left.len(), frames, "{} gleich lang", s.name);
        assert_eq!(s.right.len(), frames);
        assert!(rms(&s.left) > 0.0 || s.name == "Bus" && rms(&s.right) > 0.0);
    }
    // Obere Ebene: A (direkt) und der Bus (enthält B und den Send von D). D selbst hat
    // keinen Bus als Ausgang, geht also auch direkt in den Master.
    let top: Vec<_> = r
        .stems
        .iter()
        .filter(|s| s.top_level)
        .map(|s| s.name.as_str())
        .collect();
    assert_eq!(top, ["A", "D", "Bus"]);
    let mut worst = 0.0f32;
    for i in 0..frames {
        let (mut l, mut rr) = (0.0, 0.0);
        for s in r.stems.iter().filter(|s| s.top_level) {
            l += s.left[i];
            rr += s.right[i];
        }
        worst = worst
            .max((l - r.mix_left[i]).abs())
            .max((rr - r.mix_right[i]).abs());
    }
    assert!(worst < 1e-4, "Summe weicht um {worst} ab");
    // Der Bus-Stem ist nicht leer und anders als die Spur B allein.
    let bus = r.stems.iter().find(|s| s.name == "Bus").expect("bus");
    let b = r.stems.iter().find(|s| s.name == "B").expect("b");
    assert!(rms(&bus.left) > 0.0);
    assert_ne!(bus.left, b.left);
}

#[test]
fn selection_limits_the_stems() {
    let p = project();
    let only_tracks = render_stems(
        &p,
        RATE,
        0.25,
        StemSelect {
            tracks: true,
            buses: false,
        },
        &mut load,
    )
    .expect("render");
    assert!(only_tracks.stems.iter().all(|s| s.kind != TrackKind::Bus));
    let none = render_stems(
        &p,
        RATE,
        0.25,
        StemSelect {
            tracks: false,
            buses: false,
        },
        &mut load,
    )
    .expect("render");
    assert!(none.stems.is_empty());
    assert!(rms(&none.mix_left) > 0.0, "Mixdown klingt trotzdem");
}

#[test]
fn stems_land_in_files_of_equal_length_and_sum_back() {
    let p = project();
    let r = render(&p);
    let dir = tempfile::tempdir().expect("dir");
    let mut spec = ExportSpec::new(ExportFormat::Wav);
    spec.depth = Some(BitDepth::Float32);
    let paths = write_stems(dir.path(), &r, &spec, RATE, 0.0, true).expect("write");
    assert_eq!(paths.len(), r.stems.len() + 1);
    let expect = stem_file_names(&r.stems, "wav");
    for (p, n) in paths[1..].iter().zip(&expect) {
        assert_eq!(p.file_name().and_then(|f| f.to_str()), Some(n.as_str()));
    }
    assert_eq!(
        paths[0].file_name().and_then(|f| f.to_str()),
        Some("00 Mixdown.wav")
    );
    let read = |p: &std::path::Path| decode_file(p).expect("read").stereo();
    let (ml, mr) = read(&paths[0]);
    assert_eq!(ml.len(), r.mix_left.len());
    let mut sum_l = vec![0.0f32; ml.len()];
    let mut sum_r = vec![0.0f32; ml.len()];
    for (stem, path) in r.stems.iter().zip(&paths[1..]) {
        let (l, rr) = read(path);
        assert_eq!(l.len(), ml.len(), "{} gleich lang", stem.name);
        if stem.top_level {
            for i in 0..l.len() {
                sum_l[i] += l[i];
                sum_r[i] += rr[i];
            }
        }
    }
    for i in 0..ml.len() {
        assert!((sum_l[i] - ml[i]).abs() < 1e-4 && (sum_r[i] - mr[i]).abs() < 1e-4);
    }
}

#[test]
fn gain_applies_equally_so_the_sum_still_holds() {
    let p = project();
    let r = render(&p);
    let dir = tempfile::tempdir().expect("dir");
    let spec = ExportSpec::new(ExportFormat::Wav);
    let paths = write_stems(dir.path(), &r, &spec, RATE, -6.0, true).expect("write");
    let (ml, _) = decode_file(&paths[0]).expect("mix").stereo();
    let ratio = rms(&ml) / rms(&r.mix_left);
    assert!((ratio - 0.501_187).abs() < 1e-3, "{ratio}");
}

#[test]
fn failed_stem_export_leaves_no_new_files() {
    let p = project();
    let r = render(&p);
    let dir = tempfile::tempdir().expect("dir");
    let mut spec = ExportSpec::new(ExportFormat::Opus);
    spec.ffmpeg = Some("/nicht/vorhanden/ffmpeg".into());
    let err = write_stems(dir.path(), &r, &spec, RATE, 0.0, true).unwrap_err();
    assert!(err.to_string().contains(MISSING), "{err}");
    assert_eq!(std::fs::read_dir(dir.path()).expect("ls").count(), 0);
    // Zielordner fehlt.
    assert!(write_stems(&dir.path().join("fehlt"), &r, &spec, RATE, 0.0, true).is_err());
}

#[cfg(unix)]
#[test]
fn stem_target_symlink_is_refused() {
    let p = project();
    let r = render(&p);
    let dir = tempfile::tempdir().expect("dir");
    let victim = dir.path().join("opfer.txt");
    std::fs::write(&victim, "wichtig").expect("write");
    std::os::unix::fs::symlink(&victim, dir.path().join("00 Mixdown.wav")).expect("link");
    let spec = ExportSpec::new(ExportFormat::Wav);
    assert!(write_stems(dir.path(), &r, &spec, RATE, 0.0, true).is_err());
    assert_eq!(std::fs::read_to_string(&victim).expect("read"), "wichtig");
}

#[test]
fn wav_and_flac_depths_read_back() {
    let x = sine(1000.0, RATE as usize);
    let dir = tempfile::tempdir().expect("dir");
    for (fmt, depth, tol) in [
        (ExportFormat::Wav, BitDepth::Int16, 1.0 / 32_000.0),
        (ExportFormat::Wav, BitDepth::Int24, 1.0 / 8_000_000.0),
        (ExportFormat::Wav, BitDepth::Float32, 0.0),
        (ExportFormat::Flac, BitDepth::Int16, 1.0 / 32_000.0),
        (ExportFormat::Flac, BitDepth::Int24, 1.0 / 8_000_000.0),
    ] {
        let mut spec = ExportSpec::new(fmt);
        spec.depth = Some(depth);
        let path = dir.path().join(format!("t-{depth:?}.{}", fmt.extension()));
        export_with(&path, &spec, &x, &x, RATE).expect("export");
        let (l, r) = decode_file(&path).expect("read").stereo();
        assert_eq!(l.len(), x.len(), "{fmt:?} {depth:?}");
        let worst = l
            .iter()
            .zip(x.iter())
            .map(|(a, b)| (a - b).abs())
            .fold(0.0f32, f32::max);
        assert!(worst <= tol + 1e-9, "{fmt:?} {depth:?}: {worst}");
        assert_eq!(l, r);
    }
    // WAV 16 bit ist wirklich 16 bit breit (Dateigröße), Float 32.
    let size = |d| {
        let mut spec = ExportSpec::new(ExportFormat::Wav);
        spec.depth = Some(d);
        let p = dir.path().join(format!("s-{d:?}.wav"));
        export_with(&p, &spec, &x, &x, RATE).expect("export");
        std::fs::metadata(&p).expect("meta").len()
    };
    assert!(size(BitDepth::Int16) < size(BitDepth::Int24));
    assert!(size(BitDepth::Int24) < size(BitDepth::Float32));
}

#[test]
fn flac_refuses_float_and_leaves_no_file() {
    let x = [0.0f32; 100];
    let dir = tempfile::tempdir().expect("dir");
    let mut spec = ExportSpec::new(ExportFormat::Flac);
    spec.depth = Some(BitDepth::Float32);
    let path = dir.path().join("x.flac");
    let err = export_with(&path, &spec, &x, &x, RATE).unwrap_err();
    assert!(err.to_string().contains("Float"), "{err}");
    assert!(!path.exists());
    let mut wav = ExportSpec::new(ExportFormat::Wav);
    wav.depth = Some(BitDepth::Float32);
    assert!(export_with(&path, &wav, &x, &x[..50], RATE).is_err());
}

#[test]
fn missing_ffmpeg_gives_a_clear_message_for_every_codec() {
    let x = sine(440.0, 4800);
    let dir = tempfile::tempdir().expect("dir");
    for fmt in [ExportFormat::Opus, ExportFormat::Ogg, ExportFormat::Aac] {
        let mut spec = ExportSpec::new(fmt);
        spec.ffmpeg = Some(dir.path().join("kein-ffmpeg"));
        let path = dir.path().join(format!("a.{}", fmt.extension()));
        let err = export_with(&path, &spec, &x, &x, RATE).unwrap_err();
        assert!(err.to_string().contains(MISSING), "{fmt:?}: {err}");
        assert!(!path.exists(), "keine halbe Datei");
    }
}

/// ffmpeg, wenn vorhanden; sonst Logzeile und `None`.
fn ffmpeg_or_skip(test: &str) -> Option<(Ffmpeg, woa_engine::ffmpeg::Encoders)> {
    let Some(ff) = Ffmpeg::locate() else {
        eprintln!("{test}: übersprungen, kein ffmpeg gefunden");
        return None;
    };
    let enc = ff.encoders().expect("encoders");
    Some((ff, enc))
}

#[test]
fn lossy_formats_roundtrip_through_ffmpeg() {
    let Some((ff, enc)) = ffmpeg_or_skip("lossy_formats_roundtrip_through_ffmpeg") else {
        return;
    };
    let secs = 3;
    let x = sine(1000.0, secs * RATE as usize);
    let silent = vec![0.0f32; x.len()];
    let dir = tempfile::tempdir().expect("dir");
    let mut checked = 0;
    for fmt in [ExportFormat::Opus, ExportFormat::Ogg, ExportFormat::Aac] {
        let codec = fmt.codec().expect("codec");
        if !enc.supports(codec) {
            eprintln!("{}: dieses ffmpeg hat den Encoder nicht", codec.label());
            continue;
        }
        let mut spec = ExportSpec::new(fmt);
        spec.ffmpeg = Some(ff.path().to_path_buf());
        // Links 1 kHz, rechts Stille: Kanäle bleiben getrennt.
        let path = dir.path().join(format!("t.{}", fmt.extension()));
        export_with(&path, &spec, &x, &silent, RATE).expect("export");
        let bytes = std::fs::read(&path).expect("read");
        assert!(bytes.len() > 2000, "{fmt:?} zu klein");
        match fmt {
            ExportFormat::Opus | ExportFormat::Ogg => assert!(bytes.starts_with(b"OggS")),
            _ => assert!(bytes[0] == 0xFF && bytes[1] & 0xF0 == 0xF0, "ADTS-Kopf"),
        }
        // Zurücklesen: Opus nur über ffmpeg, Ogg und AAC auch über symphonia.
        let d = ff.decode(&path, RATE).expect("decode");
        let (l, r) = d.stereo();
        let diff = l.len().abs_diff(x.len());
        // Opus und AAC haben Vorlauf und Auffüllung: Länge auf 100 ms genau.
        assert!(
            diff < RATE as usize / 10,
            "{fmt:?}: Länge {} statt {}",
            l.len(),
            x.len()
        );
        let want = rms(&x);
        assert!(
            (rms(&l) / want - 1.0).abs() < 0.12,
            "{fmt:?} Pegel links {}",
            rms(&l)
        );
        assert!(rms(&r) < 0.02 * want, "{fmt:?} rechts bleibt leise");
        if fmt != ExportFormat::Opus {
            let s = decode_file(&path).expect("symphonia");
            assert_eq!(s.channels, 2, "{fmt:?}");
        }
        checked += 1;
    }
    assert!(checked > 0 || !enc.opus && !enc.vorbis && !enc.aac);
}

#[test]
fn encoder_without_support_is_reported() {
    let Some((ff, enc)) = ffmpeg_or_skip("encoder_without_support_is_reported") else {
        return;
    };
    let _ = enc;
    // Zu kleine Bitrate wird vor dem Start abgelehnt, ohne Prozess.
    let mut out = Vec::new();
    let err = ff
        .encode(Codec::Opus, 1, &[0.0; 10], &[0.0; 10], RATE, &mut out)
        .unwrap_err();
    assert!(err.contains("Bitrate"));
    // Datei, die kein Audio ist.
    let dir = tempfile::tempdir().expect("dir");
    let junk = dir.path().join("kaputt.opus");
    std::fs::write(&junk, b"das ist kein opus").expect("write");
    assert!(ff.decode(&junk, RATE).is_err());
}
