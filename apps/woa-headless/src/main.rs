//! `woa-headless`: Engine + MCP, für Agent und Tests.
//!
//! In M0 ist das Werkzeug das Abnahmewerkzeug für das Fundament (#1): Projekte anlegen,
//! ansehen, ändern, den Roundtrip prüfen und je ein Test-Ereignis an Telemetrie und
//! Feedback schicken. Jeder Unterbefehl endet mit Exit-Code 0 bei Erfolg, 1 bei einem
//! fachlichen Fehlschlag (z. B. Roundtrip ungleich, Server sagt nein) und 2 bei
//! falschem Aufruf (clap).

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use clap::{Parser, Subcommand, ValueEnum};
use woa_model::{Command, Session, TrackKind, store};
use woa_telemetry::{FeedbackMeta, FeedbackType, Level, UploadConfig, Uploader, submit_feedback};

#[derive(Parser)]
#[command(
    name = "woa-headless",
    version,
    about = "WOA ohne Oberfläche: Projekte, Abnahme, Tests"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Clone, Copy, ValueEnum)]
enum Kind {
    Audio,
    Midi,
    Bus,
}

impl From<Kind> for TrackKind {
    fn from(k: Kind) -> Self {
        match k {
            Kind::Audio => TrackKind::Audio,
            Kind::Midi => TrackKind::Midi,
            Kind::Bus => TrackKind::Bus,
        }
    }
}

#[derive(Subcommand)]
enum Cmd {
    /// Legt ein leeres Projekt im Ordner <PATH> an (z. B. jingle.woa).
    New {
        path: PathBuf,
        /// Projektname; Standard ist der Ordnername ohne Endung.
        #[arg(long)]
        name: Option<String>,
    },
    /// Zeigt Name, Tempo und Spuren eines Projekts.
    Info {
        path: PathBuf,
        /// Ausgabe als JSON (project.json-Inhalt).
        #[arg(long)]
        json: bool,
    },
    /// Fügt eine Spur hinzu und speichert.
    AddTrack {
        path: PathBuf,
        name: String,
        #[arg(long, value_enum, default_value = "audio")]
        kind: Kind,
    },
    /// Führt Commands im Speicher aus, macht sie rückgängig und wieder her und prüft die Zustände.
    UndoDemo,
    /// Lädt, speichert in einen Temp-Ordner, lädt erneut und vergleicht. Exit 1 bei Abweichung.
    Roundtrip { path: PathBuf },
    /// Startet den MCP-Server über stdin/stdout für das Projekt <PATH> (legt es beim ersten `save` an).
    Mcp { path: PathBuf },
    /// Listet die Audio-Ausgabegeräte, mit `--input` die Eingänge (nur mit Feature `device`).
    Devices {
        #[arg(long)]
        input: bool,
    },
    /// Spielt einen Testton auf dem Gerät und zählt Underruns (M1-Abnahme, #2).
    /// Exit 1, wenn mindestens ein Underrun auftrat. Nur mit Feature `device`.
    Play {
        /// Spielt dieses Projekt (Ordner .woa) statt des Testtons.
        #[arg(long)]
        project: Option<PathBuf>,
        /// Dauer in Sekunden.
        #[arg(long, default_value_t = 10)]
        seconds: u64,
        /// Puffergröße in Frames; 0 = Vorgabe des Treibers.
        #[arg(long, default_value_t = 128)]
        buffer: u32,
        /// Geräte-Id aus `devices`; ohne = Standardgerät.
        #[arg(long)]
        device: Option<String>,
        /// Frequenz des Testtons in Hz.
        #[arg(long, default_value_t = 440.0)]
        freq: f32,
        /// Lautstärke 0..1 (Vorgabe 0.3; bei 0.1 war es mit 25 % Systemlautstärke unhörbar).
        #[arg(long, default_value_t = 0.3)]
        volume: f32,
    },
    /// Nimmt vom Eingang (Mikrofon) als WAV auf und meldet Pegel und Überläufe.
    /// Exit 1 bei Überläufen. Nur mit Feature `device`.
    Record {
        /// Zieldatei (.wav, .flac oder .mp3). der Ordner muss existieren.
        out: PathBuf,
        /// Dauer in Sekunden.
        #[arg(long, default_value_t = 10)]
        seconds: u64,
        /// Eingangs-Id aus `devices --input`; ohne = Standard-Eingang.
        #[arg(long)]
        device: Option<String>,
    },
    /// Listet Quellen aus PipeWire/Pulse: Eingänge, Systemklang, Programme (nur Linux, `device`).
    Sources,
    /// Nimmt eine Pulse-Quelle auf (Teil von Name oder Schlüssel aus `sources`).
    Capture {
        /// Teil des Namens oder Schlüssels der Quelle.
        source: String,
        /// Zieldatei (.wav).
        out: PathBuf,
        #[arg(long, default_value_t = 5)]
        seconds: u64,
        /// Quelle dabei hörbar machen.
        #[arg(long)]
        monitor: bool,
    },
    /// Rendert ein Projekt mit seinen Audio-Clips offline in eine WAV-Datei (ohne Gerät).
    Render {
        /// Projektordner (.woa).
        project: PathBuf,
        /// Zieldatei (.wav, .flac oder .mp3).
        out: PathBuf,
        /// Länge in Sekunden; 0 = bis zum Ende des letzten Clips.
        #[arg(long, default_value_t = 0)]
        seconds: u64,
        /// Abtastrate.
        #[arg(long, default_value_t = 48_000)]
        rate: u32,
        /// Lautheit angleichen auf diesen Wert (LUFS, z. B. -16), Spitzen höchstens -1 dBFS.
        #[arg(long, allow_hyphen_values = true)]
        lufs: Option<f64>,
    },
    /// Rendert die Playlist aus radio.json offline mit Crossfades und Auto-Gain (ohne Gerät).
    RadioRender {
        /// radio.json (Playlist).
        radio: PathBuf,
        /// Zieldatei (.wav, .flac oder .mp3).
        out: PathBuf,
        /// Abtastrate.
        #[arg(long, default_value_t = 48_000)]
        rate: u32,
    },
    /// Schickt ein Test-Ereignis an den Telemetrie-Endpunkt (/api/logs/ingest).
    LogTest {
        #[arg(long, env = "WOA_TELEMETRY_URL", default_value = woa_telemetry::DEFAULT_INGEST_URL)]
        url: String,
        /// HTTP-Timeout in Sekunden.
        #[arg(long, default_value_t = 15)]
        timeout: u64,
    },
    /// Schickt eine Test-Meldung an den Feedback-Endpunkt (/api/feedback).
    FeedbackTest {
        #[arg(long, env = "WOA_FEEDBACK_URL")]
        url: String,
        #[arg(long, default_value = "woa-headless feedback-test")]
        text: String,
        /// HTTP-Timeout in Sekunden.
        #[arg(long, default_value_t = 30)]
        timeout: u64,
    },
}

type Outcome = Result<(), String>;

fn cmd_radio_render(radio: &std::path::Path, out: &std::path::Path, rate: u32) -> Outcome {
    if !radio.is_file() {
        return Err(format!("{}: nicht gefunden", radio.display()));
    }
    let show = woa_radio::RadioShow::load(radio)?;
    let mix = woa_radio::render(&show, rate)?;
    woa_engine::export::export_stereo(out, &mix.left, &mix.right, rate)
        .map_err(|e| e.to_string())?;
    let seconds = mix.frames() as f64 / f64::from(rate);
    let lufs = woa_engine::loudness::integrated_lufs(&mix.left, &mix.right, rate);
    println!(
        "radio-render: {} Titel, {seconds:.3} s, crossfade {:.1} s",
        show.playlist.len(),
        show.crossfade_seconds
    );
    if let Some(l) = lufs {
        println!("lufs={l:.1}");
    }
    println!("{}", out.display());
    Ok(())
}

mod audio;

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.cmd {
        Cmd::New { path, name } => cmd_new(&path, name),
        Cmd::Info { path, json } => cmd_info(&path, json),
        Cmd::AddTrack { path, name, kind } => cmd_add_track(&path, &name, kind.into()),
        Cmd::UndoDemo => cmd_undo_demo(),
        Cmd::Roundtrip { path } => cmd_roundtrip(&path),
        Cmd::Mcp { path } => woa_agent::run_stdio(&path).map_err(|e| e.to_string()),
        Cmd::Devices { input } => audio::cmd_devices(input),
        Cmd::Record {
            out,
            seconds,
            device,
        } => audio::cmd_record(&out, seconds, device),
        Cmd::Sources => audio::cmd_sources(),
        Cmd::Capture {
            source,
            out,
            seconds,
            monitor,
        } => audio::cmd_capture(&source, &out, seconds, monitor),
        Cmd::Play {
            project,
            seconds,
            buffer,
            device,
            freq,
            volume,
        } => audio::cmd_play(project.as_deref(), seconds, buffer, device, freq, volume),
        Cmd::Render {
            project,
            out,
            seconds,
            rate,
            lufs,
        } => audio::cmd_render(&project, &out, seconds, rate, lufs),
        Cmd::RadioRender { radio, out, rate } => cmd_radio_render(&radio, &out, rate),
        Cmd::LogTest { url, timeout } => cmd_log_test(&url, timeout),
        Cmd::FeedbackTest { url, text, timeout } => cmd_feedback_test(&url, &text, timeout),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("woa-headless: {e}");
            ExitCode::FAILURE
        }
    }
}

fn cmd_new(path: &Path, name: Option<String>) -> Outcome {
    if path.join(store::PROJECT_FILE).exists() {
        return Err(format!("{} enthält schon ein Projekt", path.display()));
    }
    let name = name.unwrap_or_else(|| {
        path.file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Projekt".to_string())
    });
    let mut session = Session::new(name);
    session.save_as(path).map_err(|e| e.to_string())?;
    println!("neu: {}", path.display());
    Ok(())
}

fn cmd_info(path: &Path, json: bool) -> Outcome {
    let session = Session::open(path).map_err(|e| e.to_string())?;
    let p = session.project();
    if json {
        println!("{}", store::to_json(p).map_err(|e| e.to_string())?);
        return Ok(());
    }
    println!("name: {}", p.meta.name);
    println!("tempo_bpm: {}", p.tempo_bpm);
    println!("tracks: {}", p.tracks.len());
    for t in &p.tracks {
        println!(
            "  {} {:?} \"{}\" gain_db={} pan={} mute={} solo={} clips={}",
            t.id,
            t.kind,
            t.name,
            t.gain_db,
            t.pan,
            t.mute,
            t.solo,
            t.clips.len()
        );
    }
    Ok(())
}

fn cmd_add_track(path: &Path, name: &str, kind: TrackKind) -> Outcome {
    let mut session = Session::open(path).map_err(|e| e.to_string())?;
    let id = session.new_track_id();
    session
        .apply(Command::add_track(id, name, kind))
        .map_err(|e| e.to_string())?;
    session.save().map_err(|e| e.to_string())?;
    println!("spur: {id} \"{name}\"");
    Ok(())
}

fn check(ok: bool, what: &str) -> Outcome {
    if ok {
        println!("ok   {what}");
        Ok(())
    } else {
        Err(format!("undo-demo: {what} fehlgeschlagen"))
    }
}

fn cmd_undo_demo() -> Outcome {
    let e = |e: woa_model::ModelError| e.to_string();
    let mut s = Session::new("Undo-Demo");
    let id = s.new_track_id();
    let start = s.project().clone();
    s.apply(Command::add_track(id, "Stimme", TrackKind::Audio))
        .map_err(e)?;
    s.apply(Command::SetGain {
        track: id,
        gain_db: -6.0,
    })
    .map_err(e)?;
    let after = s.project().clone();
    check(after.tracks.len() == 1, "add_track")?;
    check(
        s.project().track(id).map(|t| t.gain_db) == Some(-6.0),
        "set_gain",
    )?;
    s.undo().map_err(e)?;
    check(
        s.project().track(id).map(|t| t.gain_db) == Some(0.0),
        "undo set_gain",
    )?;
    s.undo().map_err(e)?;
    check(s.project() == &start, "undo add_track = Ausgangszustand")?;
    s.redo().map_err(e)?;
    s.redo().map_err(e)?;
    check(s.project() == &after, "redo = Endzustand")?;
    println!("undo-demo: alles gleich");
    Ok(())
}

fn unique_temp_dir(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!(
        "woa-headless-{tag}-{}-{nanos}.woa",
        std::process::id()
    ))
}

fn cmd_roundtrip(path: &Path) -> Outcome {
    let original = store::load(path).map_err(|e| e.to_string())?;
    let tmp = unique_temp_dir("roundtrip");
    let result = (|| -> Outcome {
        store::save(&original, &tmp).map_err(|e| e.to_string())?;
        let reloaded = store::load(&tmp).map_err(|e| e.to_string())?;
        if reloaded != original {
            return Err("roundtrip: Projekt nach Speichern/Laden ungleich".to_string());
        }
        let a = store::to_json(&original).map_err(|e| e.to_string())?;
        let b = store::to_json(&reloaded).map_err(|e| e.to_string())?;
        if a != b {
            return Err("roundtrip: JSON nach Speichern/Laden ungleich".to_string());
        }
        Ok(())
    })();
    if let Err(e) = std::fs::remove_dir_all(&tmp) {
        eprintln!("woa-headless: Temp-Ordner {} bleibt: {e}", tmp.display());
    }
    result?;
    println!("roundtrip: gleich ({} Spuren)", original.tracks.len());
    Ok(())
}

fn cmd_log_test(url: &str, timeout: u64) -> Outcome {
    if url.trim().is_empty() {
        return Err("log-test: keine URL (--url oder WOA_TELEMETRY_URL)".to_string());
    }
    let cfg = UploadConfig {
        enabled: true,
        url: url.to_string(),
        install_id: "woa-headless-log-test".to_string(),
        interval: Duration::from_secs(3600),
        timeout: Duration::from_secs(timeout),
        ..UploadConfig::default()
    };
    let uploader = Uploader::start(cfg).ok_or("log-test: Upload-Senke startet nicht")?;
    if !uploader.record(Level::Info, "headless", "log_test", 1) {
        return Err("log-test: Ereignis nicht angenommen".to_string());
    }
    if uploader.flush() {
        println!("log-test: Server hat angenommen");
        Ok(())
    } else {
        Err("log-test: Server hat nicht angenommen".to_string())
    }
}

fn cmd_feedback_test(url: &str, text: &str, timeout: u64) -> Outcome {
    let meta = FeedbackMeta {
        url: url.to_string(),
        kind: FeedbackType::Allgemein,
        install_id: "woa-headless-feedback-test".to_string(),
        timeout: Duration::from_secs(timeout),
        ..FeedbackMeta::default()
    };
    match submit_feedback(text, None, &meta) {
        Ok(Some(issue)) => {
            println!("feedback-test: Issue #{issue}");
            Ok(())
        }
        Ok(None) => {
            println!("feedback-test: angenommen (ohne Issue-Nummer)");
            Ok(())
        }
        Err(e) => Err(format!("feedback-test: {e}")),
    }
}
