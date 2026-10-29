//! Audio am echten Gerät: Geräte auflisten und Testton mit Underrun-Zählung (M1, #2).
//!
//! Ohne Feature `device` melden beide Befehle sauber, dass sie nicht eingebaut sind.

use super::Outcome;

#[cfg(not(feature = "device"))]
const NOT_BUILT: &str =
    "ohne Feature `device` gebaut: cargo run -p woa-headless --features device -- …";

#[cfg(not(feature = "device"))]
pub fn cmd_devices(_: bool) -> Outcome {
    Err(NOT_BUILT.to_string())
}

#[cfg(not(feature = "device"))]
pub fn cmd_record(_: &std::path::Path, _: u64, _: Option<String>) -> Outcome {
    Err(NOT_BUILT.to_string())
}

#[cfg(not(feature = "device"))]
pub fn cmd_play(
    _: Option<&std::path::Path>,
    _: u64,
    _: u32,
    _: Option<String>,
    _: f32,
    _: f32,
) -> Outcome {
    Err(NOT_BUILT.to_string())
}

#[cfg(feature = "device")]
pub fn cmd_devices(input: bool) -> Outcome {
    let list = if input {
        woa_engine::record::list_input_devices()
    } else {
        woa_engine::device::list_output_devices()
    }
    .map_err(|e| e.to_string())?;
    if list.is_empty() {
        return Err("kein Ausgabegerät gefunden".to_string());
    }
    for d in list {
        let mark = if d.is_default { "*" } else { " " };
        println!("{mark} {}  {}", d.id, d.name);
    }
    Ok(())
}

#[cfg(feature = "device")]
pub fn cmd_play(
    project: Option<&std::path::Path>,
    seconds: u64,
    buffer: u32,
    device: Option<String>,
    freq: f32,
    volume: f32,
) -> Outcome {
    use std::sync::mpsc;
    use std::time::{Duration, Instant};
    use woa_engine::device::{OutputConfig, output_sample_rate, start_output_with};
    use woa_engine::{Engine, GraphBuilder};

    if !(0.0..=1.0).contains(&volume) {
        return Err(format!("--volume {volume} liegt nicht in 0..1"));
    }
    let config = OutputConfig {
        device_id: device,
        buffer_frames: (buffer > 0).then_some(buffer),
    };
    let rate = output_sample_rate(&config).map_err(|e| e.to_string())?;

    let (mut engine, processor) = Engine::new(64);
    let graph = match project {
        Some(dir) => project_graph(dir, rate, &engine)?,
        None => {
            let mut graph = GraphBuilder::new();
            let osc = graph.add_node(woa_dsp::SineOsc::new(freq, volume));
            graph.set_stereo_output(osc, osc);
            graph
        }
    };
    engine
        .set_graph(graph, rate as f32, 4096)
        .map_err(|e| e.to_string())?;
    engine.play().map_err(|e| e.to_string())?;

    // Fehler aus dem Audio-Thread nur weiterreichen, gedruckt wird hier.
    let (err_tx, err_rx) = mpsc::channel::<String>();
    let stream = start_output_with(processor, engine.transport(), &config, move |e| {
        let _ = err_tx.send(e.to_string());
    })
    .map_err(|e| e.to_string())?;

    let puffer = if buffer > 0 {
        buffer.to_string()
    } else {
        "Treiber".to_string()
    };
    let was = match project {
        Some(dir) => dir.display().to_string(),
        None => format!("{freq} Hz"),
    };
    println!("spiele: {was}, {rate} Hz, Puffer {puffer}, {seconds} s");
    let ende = Instant::now() + Duration::from_secs(seconds);
    while Instant::now() < ende {
        std::thread::sleep(Duration::from_millis(250));
        engine.collect();
        while let Ok(msg) = err_rx.try_recv() {
            eprintln!("audio: {msg}");
        }
    }
    drop(stream);
    let underruns = engine.underruns();
    println!("underruns={underruns}");
    if underruns > 0 {
        return Err(format!("{underruns} Underrun(s) in {seconds} s"));
    }
    Ok(())
}

#[cfg(feature = "device")]
pub fn cmd_record(out: &std::path::Path, seconds: u64, device: Option<String>) -> Outcome {
    use std::sync::mpsc;
    use std::time::Duration;
    use woa_engine::record::{InputConfig, start_recording};

    let (err_tx, err_rx) = mpsc::channel::<String>();
    let rec = start_recording(&InputConfig { device_id: device }, out, move |e| {
        let _ = err_tx.send(e.to_string());
    })
    .map_err(|e| e.to_string())?;
    println!("nehme auf: {} ({seconds} s)", out.display());
    let ende = Duration::from_secs(seconds);
    while rec.elapsed() < ende {
        std::thread::sleep(Duration::from_millis(500));
        let level = rec.take_level();
        let balken = "#".repeat((level * 40.0).round() as usize);
        println!("{:>4.1} s  {balken}", rec.elapsed().as_secs_f32());
        while let Ok(msg) = err_rx.try_recv() {
            eprintln!("audio: {msg}");
        }
    }
    let stats = rec.stop().map_err(|e| e.to_string())?;
    println!(
        "fertig: {:.1} s, {} Hz, {} Kanal/Kanäle, peak={:.3}, overruns={}",
        stats.seconds(),
        stats.sample_rate,
        stats.channels,
        stats.peak,
        stats.overruns
    );
    if stats.overruns > 0 {
        return Err(format!("{} Samples verloren", stats.overruns));
    }
    Ok(())
}

/// Lädt Clip-Dateien eines Projektordners, nur sichere relative Pfade, mit Fehlerliste.
fn loader(
    dir: &std::path::Path,
    rate: u32,
    errors: &mut Vec<String>,
) -> impl FnMut(&str) -> Option<std::sync::Arc<[f32]>> {
    move |rel: &str| {
        if !woa_model::is_safe_audio_path(rel) {
            errors.push(format!("{rel}: Pfad verlässt den Projektordner"));
            return None;
        }
        woa_engine::clip::load_wav_mono(&dir.join(rel), rate)
            .map_err(|e| errors.push(format!("{rel}: {e}")))
            .ok()
    }
}

/// Graph eines gespeicherten Projekts mit seinen Audio-Clips (Pfade relativ zum Ordner).
#[cfg(feature = "device")]
fn project_graph(
    dir: &std::path::Path,
    rate: u32,
    engine: &woa_engine::Engine,
) -> Result<woa_engine::GraphBuilder, String> {
    let project = woa_model::store::load(dir).map_err(|e| e.to_string())?;
    let mut errors = Vec::new();
    let graph = woa_engine::clip::clip_graph(
        &project,
        &engine.transport(),
        rate as f32,
        &mut loader(dir, rate, &mut errors),
        None,
    );
    if let Some(e) = errors.first() {
        return Err(e.clone());
    }
    let clips: usize = project.tracks.iter().map(|t| t.clips.len()).sum();
    println!(
        "projekt: {} Spur(en), {clips} Clip(s)",
        project.tracks.len()
    );
    Ok(graph.builder)
}

/// Rendert ein Projekt offline (ohne Gerät). `seconds` 0 = bis zum Ende des letzten Clips.
pub fn cmd_render(
    dir: &std::path::Path,
    out: &std::path::Path,
    seconds: u64,
    rate: u32,
) -> Outcome {
    use woa_engine::clip::{project_end_seconds, render_clips};
    let project = woa_model::store::load(dir).map_err(|e| e.to_string())?;
    let length = if seconds == 0 {
        project_end_seconds(&project)
    } else {
        seconds as f64
    };
    let mut errors = Vec::new();
    let (left, right) = render_clips(&project, rate, length, &mut loader(dir, rate, &mut errors))
        .map_err(|e| e.to_string())?;
    if let Some(e) = errors.first() {
        return Err(e.clone());
    }
    woa_engine::export::export_stereo(out, &left, &right, rate).map_err(|e| e.to_string())?;
    let peak = left
        .iter()
        .chain(&right)
        .fold(0.0f32, |m, s| m.max(s.abs()));
    println!(
        "gerendert: {} ({length:.1} s), peak={peak:.3}",
        out.display()
    );
    Ok(())
}
