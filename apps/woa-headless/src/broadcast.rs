//! Dauertest der Sendung (Paket 10): `broadcast-test` und sein Kindprozess `broadcast-ui`.
//!
//! `broadcast-test` lässt den Kern von woa-streamd (`woa_stream::daemon::run`) in einem
//! Thread laufen und startet die „Oberfläche“ als eigenen Prozess (`broadcast-ui`). Dieser
//! rechnet den echten Sendemixer (Playlist, Crossfade, Sendebus-Processing) im Takt der Uhr,
//! aber **ohne Audiogerät** (Null-Ausgabe: nichts wird hörbar abgespielt), und schickt den
//! Sendemix über die lokale Verbindung an den Sendeprozess. Einmal (Vorgabe: zur Hälfte)
//! wird die Oberfläche hart beendet; die Notfall-Playlist muss binnen 2,5 s übernehmen,
//! danach startet die Oberfläche neu und übernimmt wieder.
//!
//! Gezählt wird:
//! - `late_blocks`: Blöcke, die der Mixer später als einen Block nach ihrer Zeit fertig
//!   hatte. Mit einem echten Gerät wäre das ein Aussetzer (Underrun).
//! - `stream_dropped`: Samples, die nicht in den Sende-Ring passten.
//! - `live_underruns`: Der Sendeprozess hatte bei verbundener Oberfläche keinen Sendemix
//!   mehr im Puffer.
//! - `reconnects`: Verbindungsabbrüche zum Server.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use woa_radio::{Controller, RadioShow};
use woa_stream::link::{self, Feeder};
use woa_stream::status::{self, StreamStatus};
use woa_stream::{StreamConfig, daemon};

/// Mixer-Block (Frames).
const BLOCK: usize = 512;
/// Höchstens so lange darf die Übernahme durch die Notfall-Playlist dauern.
pub const FAILOVER_LIMIT: Duration = Duration::from_millis(2500);

/// Was die Oberfläche je Zeile meldet (`ui-stand late=… dropped=… frames=…`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct UiCounts {
    pub late: u64,
    pub dropped: u64,
    pub frames: u64,
}

impl UiCounts {
    fn line(&self) -> String {
        format!(
            "ui-stand late={} dropped={} frames={}",
            self.late, self.dropped, self.frames
        )
    }

    /// Liest eine `ui-stand`-Zeile.
    pub fn parse(line: &str) -> Option<Self> {
        let rest = line.strip_prefix("ui-stand ")?;
        let mut c = Self::default();
        for part in rest.split_whitespace() {
            let (k, v) = part.split_once('=')?;
            let v: u64 = v.parse().ok()?;
            match k {
                "late" => c.late = v,
                "dropped" => c.dropped = v,
                "frames" => c.frames = v,
                _ => {}
            }
        }
        Some(c)
    }
}

/// Ein zu spät gerechneter Block: Position (Frames) und Verspätung (µs).
#[derive(Debug, Clone, Copy)]
struct Late {
    frame: u64,
    micros: u64,
}

/// Zähler, die der Takt-Thread schreibt und der Steuer-Thread liest.
#[derive(Default)]
struct Clock {
    frames: AtomicU64,
    late: AtomicU64,
    stop: AtomicBool,
}

/// Takt-Thread: rechnet den Mixer im Takt der Uhr, wie ein Audio-Callback. Hier geschieht nichts außer `process` und Warten: kein Dekodieren,
/// kein Logging.
/// Verspätungen gehen über eine `rtrb`-Queue an den Steuer-Thread.
fn run_clock(
    mut mixer: woa_radio::Mixer,
    rate: u32,
    total: u64,
    clock: &Clock,
    mut late_tx: rtrb::Producer<Late>,
) {
    let mut l = vec![0.0f32; BLOCK];
    let mut r = vec![0.0f32; BLOCK];
    let block = Duration::from_secs_f64(BLOCK as f64 / f64::from(rate));
    let start = Instant::now();
    let mut frames = 0u64;
    while frames < total && !clock.stop.load(Ordering::Relaxed) {
        mixer.process(&mut l, &mut r);
        frames += BLOCK as u64;
        clock.frames.store(frames, Ordering::Release);
        let due = start + Duration::from_secs_f64(frames as f64 / f64::from(rate));
        let now = Instant::now();
        if now > due + block {
            clock.late.fetch_add(1, Ordering::Release);
            let _ = late_tx.push(Late {
                frame: frames,
                micros: (now - due).as_micros() as u64,
            });
        } else if let Some(wait) = due.checked_duration_since(now) {
            std::thread::sleep(wait);
        }
    }
}

/// Kindprozess: Sendemixer im Takt, Null-Ausgabe, Sendemix an `link`.
///
/// Zwei Threads wie in der Oberfläche: Der Takt-Thread rechnet nur den Mixer
/// ([`run_clock`]), dieser Thread steuert (Titel dekodieren, Playlist im Kreis, Titel an den
/// Feeder, Bericht). Bis 0.5.1 lief beides in einer Schleife; das Dekodieren beim Neustart
/// der Playlist (rund 250 ms für 3 min MP3) hielt den Takt an, daher die Bündel zu später
/// Blöcke im Dauertest vom 30.10.2026.
pub fn cmd_broadcast_ui(radio: &Path, link: &Path, rate: u32, seconds: f64) -> Result<(), String> {
    let show = RadioShow::load(radio)?;
    if show.playlist.is_empty() {
        return Err(format!("{}: Playlist ist leer", radio.display()));
    }
    let (mut c, mixer) = Controller::new(show, rate);
    c.measure_missing();
    // 2 s Puffer zum Feeder; er schickt alle paar Millisekunden.
    let (producer, consumer) = rtrb::RingBuffer::new(rate as usize * 4);
    c.set_stream_output(Some(producer));
    let feeder = Feeder::spawn(consumer, rate, link.to_path_buf())?;
    c.start();
    if let Some(m) = c.take_message() {
        return Err(m);
    }
    let total = (seconds.max(0.0) * f64::from(rate)) as u64;
    let clock = Arc::new(Clock::default());
    let (late_tx, mut late_rx) = rtrb::RingBuffer::<Late>::new(4096);
    let ticker = {
        let clock = Arc::clone(&clock);
        std::thread::Builder::new()
            .name("woa-takt".into())
            .spawn(move || run_clock(mixer, rate, total, &clock, late_tx))
            .map_err(|e| format!("Takt-Thread: {e}"))?
    };
    let start = Instant::now();
    let mut counts = UiCounts::default();
    let mut next_report = start + Duration::from_secs(5);
    let mut title = String::new();
    loop {
        c.pump();
        if c.finished() {
            // Playlist zu Ende: von vorn, damit die Sendung durchläuft.
            c.set_cue(0);
            c.start();
            if let Some((p, d)) = c.last_load() {
                eprintln!(
                    "ui-log t={:.3} playlist neu, {} in {} ms dekodiert",
                    start.elapsed().as_secs_f64(),
                    p.display(),
                    d.as_millis()
                );
            }
        }
        if let Some(m) = c.take_message() {
            eprintln!("ui-log t={:.3} {m}", start.elapsed().as_secs_f64());
        }
        if let Some(i) = c.current_index()
            && let Some(e) = c.show().playlist.get(i)
        {
            let t = e.title();
            if t != title {
                feeder.set_title(&t);
                title = t;
            }
        }
        while let Ok(x) = late_rx.pop() {
            eprintln!(
                "ui-log t={:.3} block zu spät um {:.1} ms (Sendezeit {:.3} s)",
                start.elapsed().as_secs_f64(),
                x.micros as f64 / 1000.0,
                x.frame as f64 / f64::from(rate)
            );
        }
        counts.frames = clock.frames.load(Ordering::Acquire);
        counts.late = clock.late.load(Ordering::Acquire);
        let done = ticker.is_finished();
        if done || Instant::now() >= next_report {
            next_report += Duration::from_secs(5);
            counts.dropped = c.status().stream_dropped.load(Ordering::Relaxed);
            println!("{}", counts.line());
        }
        if done {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    clock.stop.store(true, Ordering::Relaxed);
    if ticker.join().is_err() {
        return Err("Takt-Thread abgestürzt".into());
    }
    // Nur den Feeder lösen: Der Sendeprozess gehört dem Test.
    drop(feeder);
    Ok(())
}

/// Einstellungen von `broadcast-test`.
#[derive(Debug, Clone)]
pub struct TestPlan {
    pub radio: PathBuf,
    pub seconds: f64,
    /// Nach so vielen Sekunden die Oberfläche hart beenden (`None` = nie).
    pub kill_ui_at: Option<f64>,
    pub cfg: StreamConfig,
    /// Ordner für Link, Status und Log; Vorgabe ist ein Temp-Ordner.
    pub data: PathBuf,
    /// Bericht als JSON (optional).
    pub report: Option<PathBuf>,
}

/// Ergebnis des Dauertests.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct Report {
    pub seconds_planned: f64,
    pub seconds_run: f64,
    pub server: String,
    pub late_blocks: u64,
    pub stream_dropped: u64,
    pub live_underruns: u64,
    pub reconnects: u32,
    pub bytes_sent: u64,
    pub ui_killed: bool,
    pub failover_ms: Option<u64>,
    pub handback_ms: Option<u64>,
    pub problems: Vec<String>,
    pub passed: bool,
}

fn spawn_ui(
    plan: &TestPlan,
    link: &Path,
    seconds: f64,
    counts: Arc<Mutex<UiCounts>>,
    base: UiCounts,
) -> Result<Child, String> {
    let exe = std::env::current_exe().map_err(|e| format!("eigenes Programm: {e}"))?;
    let mut child = Command::new(exe)
        .arg("broadcast-ui")
        .arg(&plan.radio)
        .arg("--link")
        .arg(link)
        .arg("--rate")
        .arg(plan.cfg.sample_rate.to_string())
        .arg("--seconds")
        .arg(format!("{seconds:.3}"))
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| format!("broadcast-ui: {e}"))?;
    if let Some(out) = child.stdout.take() {
        std::thread::spawn(move || {
            for line in BufReader::new(out).lines().map_while(Result::ok) {
                if let Some(c) = UiCounts::parse(&line)
                    && let Ok(mut g) = counts.lock()
                {
                    *g = UiCounts {
                        late: base.late + c.late,
                        dropped: base.dropped + c.dropped,
                        frames: base.frames + c.frames,
                    };
                }
            }
        });
    }
    Ok(child)
}

fn wait_until(limit: Duration, mut ok: impl FnMut() -> bool) -> Option<Duration> {
    let t0 = Instant::now();
    while t0.elapsed() < limit {
        if ok() {
            return Some(t0.elapsed());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    None
}

/// Dauertest: Sendeprozess im Thread, Oberfläche als Kindprozess, einmal Absturz.
pub fn cmd_broadcast_test(plan: &TestPlan) -> Result<Report, String> {
    plan.cfg.validate()?;
    woa_stream::Kennwort::from_file(&plan.cfg.password_file)?;
    std::fs::create_dir_all(&plan.data).map_err(|e| format!("{}: {e}", plan.data.display()))?;
    let status_path = status::status_path(&plan.data);
    let read_status = || status::read(&status_path).unwrap_or_default();

    let stop = Arc::new(AtomicBool::new(false));
    let daemon = {
        let (cfg, data, stop) = (plan.cfg.clone(), plan.data.clone(), Arc::clone(&stop));
        std::thread::Builder::new()
            .name("woa-streamd".into())
            .spawn(move || daemon::run(cfg, &data, stop))
            .map_err(|e| e.to_string())?
    };
    let link = link::link_path(&plan.data);
    if wait_until(Duration::from_secs(5), || link::is_alive(&link)).is_none() {
        stop.store(true, Ordering::Relaxed);
        let err = match daemon.join() {
            Ok(Err(e)) => e,
            _ => "Sendeprozess startet nicht".into(),
        };
        return Err(err);
    }

    let mut report = Report {
        seconds_planned: plan.seconds,
        server: plan.cfg.label(),
        ..Report::default()
    };
    let counts = Arc::new(Mutex::new(UiCounts::default()));
    let start = Instant::now();
    let mut child = spawn_ui(
        plan,
        &link,
        plan.seconds,
        Arc::clone(&counts),
        UiCounts::default(),
    )?;
    let connected = wait_until(Duration::from_secs(10), || {
        let s = read_status();
        s.connected && s.ui_connected && !s.emergency
    });
    if connected.is_none() {
        let s = read_status();
        report.problems.push(format!(
            "Sendung kam nicht zustande: {}",
            s.last_error.unwrap_or_else(|| s.connection.clone())
        ));
    }

    let peak_reconnects = AtomicU64::new(0);
    let mut killed = plan.kill_ui_at.is_none();
    let mut next_line = Instant::now() + Duration::from_secs(60);
    loop {
        if let Ok(Some(_)) = child.try_wait() {
            break;
        }
        let elapsed = start.elapsed().as_secs_f64();
        if !killed && plan.kill_ui_at.is_some_and(|t| elapsed >= t) {
            killed = true;
            report.ui_killed = true;
            let base = counts.lock().map(|g| *g).unwrap_or_default();
            let _ = child.kill();
            let _ = child.wait();
            match wait_until(Duration::from_secs(5), || read_status().emergency) {
                Some(d) => {
                    report.failover_ms = Some(d.as_millis() as u64);
                    if d > FAILOVER_LIMIT {
                        report
                            .problems
                            .push(format!("Notfall-Playlist erst nach {} ms", d.as_millis()));
                    }
                }
                None => report
                    .problems
                    .push("Notfall-Playlist hat nicht übernommen".into()),
            }
            let rest = (plan.seconds - start.elapsed().as_secs_f64()).max(1.0);
            child = spawn_ui(plan, &link, rest, Arc::clone(&counts), base)?;
            match wait_until(Duration::from_secs(15), || {
                let s = read_status();
                s.ui_connected && !s.emergency
            }) {
                Some(d) => report.handback_ms = Some(d.as_millis() as u64),
                None => report
                    .problems
                    .push("Oberfläche hat nach dem Neustart nicht übernommen".into()),
            }
        }
        let s = read_status();
        peak_reconnects.fetch_max(u64::from(s.reconnects), Ordering::Relaxed);
        if Instant::now() >= next_line {
            next_line += Duration::from_secs(60);
            let c = counts.lock().map(|g| *g).unwrap_or_default();
            println!(
                "broadcast-test: {} von {} · {} · zu spät {} · verloren {} · Puffer leer {} · neu verbunden {}",
                woa_stream::status::clock(start.elapsed().as_secs()),
                woa_stream::status::clock(plan.seconds as u64),
                s.source,
                c.late,
                c.dropped,
                s.live_underruns,
                s.reconnects
            );
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    report.seconds_run = start.elapsed().as_secs_f64();
    // Ein letzter Status, dann Sendeprozess beenden.
    std::thread::sleep(Duration::from_millis(400));
    let s: StreamStatus = read_status();
    stop.store(true, Ordering::Relaxed);
    match daemon.join() {
        Ok(Ok(())) => {}
        Ok(Err(e)) => report.problems.push(format!("Sendeprozess: {e}")),
        Err(_) => report.problems.push("Sendeprozess abgestürzt".into()),
    }
    let c = counts.lock().map(|g| *g).unwrap_or_default();
    report.late_blocks = c.late;
    report.stream_dropped = c.dropped;
    report.live_underruns = u64::from(s.live_underruns);
    report.reconnects = s
        .reconnects
        .max(peak_reconnects.load(Ordering::Relaxed) as u32);
    report.bytes_sent = s.bytes_sent;
    if c.late > 0 {
        report
            .problems
            .push(format!("{} Blöcke zu spät (Aussetzer)", c.late));
    }
    if c.dropped > 0 {
        report
            .problems
            .push(format!("{} Samples nicht an den Sendeprozess", c.dropped));
    }
    if report.live_underruns > 0 {
        report.problems.push(format!(
            "Sendeprozess {}-mal ohne Sendemix im Puffer",
            report.live_underruns
        ));
    }
    if report.reconnects > 0 {
        report.problems.push(format!(
            "{} Verbindungsabbrüche zum Server",
            report.reconnects
        ));
    }
    report.passed = report.problems.is_empty();
    if let Some(p) = &plan.report {
        let text = serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?;
        std::fs::write(p, text).map_err(|e| format!("{}: {e}", p.display()))?;
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ui_lines() {
        let c = UiCounts {
            late: 2,
            dropped: 0,
            frames: 4096,
        };
        assert_eq!(UiCounts::parse(&c.line()), Some(c));
        assert_eq!(UiCounts::parse("sonst etwas"), None);
    }
}
