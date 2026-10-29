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

/// Graph eines gespeicherten Projekts mit seinen Audio-Clips (Pfade relativ zum Ordner).
fn project_graph(
    dir: &std::path::Path,
    rate: u32,
    engine: &woa_engine::Engine,
) -> Result<woa_engine::GraphBuilder, String> {
    use woa_engine::clip::{ClipPlayer, PlacedClip, load_wav_mono};
    use woa_engine::transport::beats_to_samples;

    let project = woa_model::store::load(dir).map_err(|e| e.to_string())?;
    let transport = engine.transport();
    let mut err = None;
    let mut source = |t: &woa_model::Track| -> Option<Box<dyn woa_dsp::Node>> {
        if t.kind != woa_model::TrackKind::Audio {
            return None;
        }
        let mut clips = Vec::new();
        for c in &t.clips {
            let Some(rel) = &c.audio else { continue };
            match load_wav_mono(&dir.join(rel), rate) {
                Ok(samples) => clips.push(PlacedClip {
                    start: beats_to_samples(c.start_beats, project.tempo_bpm, rate as f32),
                    samples,
                }),
                Err(e) => err = Some(format!("{rel}: {e}")),
            }
        }
        Some(Box::new(ClipPlayer::new(clips, transport.clone())))
    };
    let graph = woa_engine::project::build_project_graph(&project, &mut source);
    if let Some(e) = err {
        return Err(e);
    }
    let clips: usize = project.tracks.iter().map(|t| t.clips.len()).sum();
    println!(
        "projekt: {} Spur(en), {clips} Clip(s)",
        project.tracks.len()
    );
    Ok(graph.builder)
}

/// Rendert ein Projekt offline (ohne Gerät) und meldet den Spitzenpegel.
pub fn cmd_render(
    dir: &std::path::Path,
    out: &std::path::Path,
    seconds: u64,
    rate: u32,
) -> Outcome {
    let (mut engine, mut processor) = woa_engine::Engine::new(16);
    let graph = project_graph(dir, rate, &engine)?;
    engine
        .set_graph(graph, rate as f32, 512)
        .map_err(|e| e.to_string())?;
    engine.play().map_err(|e| e.to_string())?;
    let frames = rate as usize * seconds as usize;
    let mut left = vec![0.0f32; frames];
    let mut right = vec![0.0f32; frames];
    for (l, r) in left.chunks_mut(512).zip(right.chunks_mut(512)) {
        processor.process_stereo(l, r);
    }
    let peak = left
        .iter()
        .chain(&right)
        .fold(0.0f32, |m, s| m.max(s.abs()));
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: rate,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut w = hound::WavWriter::create(out, spec).map_err(|e| e.to_string())?;
    for (l, r) in left.iter().zip(&right) {
        w.write_sample(*l).map_err(|e| e.to_string())?;
        w.write_sample(*r).map_err(|e| e.to_string())?;
    }
    w.finalize().map_err(|e| e.to_string())?;
    println!("gerendert: {} ({seconds} s), peak={peak:.3}", out.display());
    Ok(())
}
