//! Testquellen für Gäste: Sinus oder eine Datei in Schleife, in Echtzeit-Takt in den
//! Jitterpuffer geschrieben (kein Netz, kein Mikrofon). Später schreibt die WebRTC-Seite am
//! selben [`GuestFeed`].

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::jitter::GuestFeed;

/// Pegel des Testsinus (linear, etwa -10 dBFS).
pub const SINE_AMPLITUDE: f32 = 0.3;
/// Takt der Quelle.
const TICK: Duration = Duration::from_millis(5);
/// Größter Block, den die Quelle auf einmal schreibt (Frames).
const MAX_CHUNK: usize = 960;

/// Was ein Testgast „spricht“.
#[derive(Debug, Clone, PartialEq)]
pub enum TestSignal {
    /// Sinus in Hz.
    Sine { freq: f32 },
    /// Datei (WAV, FLAC, MP3) in Schleife.
    File(PathBuf),
}

impl TestSignal {
    /// Kurzname für die Anzeige.
    pub fn label(&self) -> String {
        match self {
            TestSignal::Sine { freq } => format!("Sinus {freq:.0} Hz"),
            TestSignal::File(p) => p
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "Datei".into()),
        }
    }
}

/// Sinus-Erzeuger (Phase bleibt über Blöcke stetig).
#[derive(Debug, Clone)]
pub struct SineGen {
    phase: f64,
    step: f64,
    amp: f32,
}

impl SineGen {
    pub fn new(freq: f32, rate: u32, amp: f32) -> Self {
        Self {
            phase: 0.0,
            step: std::f64::consts::TAU * f64::from(freq) / f64::from(rate.max(1)),
            amp,
        }
    }

    /// Füllt `out` Stereo verschachtelt (beide Seiten gleich).
    pub fn fill(&mut self, out: &mut [f32]) {
        for f in out.chunks_exact_mut(2) {
            let v = self.phase.sin() as f32 * self.amp;
            f[0] = v;
            f[1] = v;
            self.phase += self.step;
            if self.phase > std::f64::consts::TAU {
                self.phase -= std::f64::consts::TAU;
            }
        }
    }
}

enum Gen {
    Sine(SineGen),
    Loop { data: Vec<f32>, pos: usize },
}

impl Gen {
    fn fill(&mut self, out: &mut [f32]) {
        match self {
            Gen::Sine(s) => s.fill(out),
            Gen::Loop { data, pos } => {
                for v in out.iter_mut() {
                    *v = data.get(*pos).copied().unwrap_or(0.0);
                    *pos += 1;
                    if *pos >= data.len() {
                        *pos = 0;
                    }
                }
            }
        }
    }
}

fn build(signal: &TestSignal, rate: u32) -> Result<Gen, String> {
    match signal {
        TestSignal::Sine { freq } => Ok(Gen::Sine(SineGen::new(*freq, rate, SINE_AMPLITUDE))),
        TestSignal::File(p) => {
            let d =
                woa_engine::decode::decode_file(p).map_err(|e| format!("{}: {e}", p.display()))?;
            let (l, r) = d.stereo();
            let (l, r) = (
                woa_engine::decode::resample_linear(&l, d.rate, rate),
                woa_engine::decode::resample_linear(&r, d.rate, rate),
            );
            let n = l.len().min(r.len());
            if n == 0 {
                return Err(format!("{}: leer", p.display()));
            }
            let mut data = Vec::with_capacity(n * 2);
            for i in 0..n {
                data.push(l[i]);
                data.push(r[i]);
            }
            Ok(Gen::Loop { data, pos: 0 })
        }
    }
}

/// Laufende Testquelle (eigener Thread). [`TestSource::stop`] gibt den Feed zurück, damit der
/// Kanal wieder frei ist.
pub struct TestSource {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<GuestFeed>>,
}

impl TestSource {
    /// Startet die Quelle. Fehler (Datei nicht lesbar) kommen hier zurück, nicht im Thread.
    /// Eine Datei wird im Aufrufer-Thread dekodiert (Control-Thread, nie Audio).
    ///
    /// Bei einem Fehler vor dem Thread kommt der Feed zurück; scheitert nur der Thread-Start
    /// (Betriebssystem ohne Ressourcen), ist der Feed weg und der Kanal bleibt zu.
    pub fn spawn(
        signal: &TestSignal,
        mut feed: GuestFeed,
        rate: u32,
    ) -> Result<Self, (String, Option<GuestFeed>)> {
        let mut gen_ = match build(signal, rate) {
            Ok(g) => g,
            Err(e) => return Err((e, Some(feed))),
        };
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let thread = std::thread::Builder::new()
            .name("woa-guest-test".into())
            .spawn(move || {
                let start = Instant::now();
                let mut produced: u64 = 0;
                let mut buf = vec![0.0f32; MAX_CHUNK * 2];
                while !flag.load(Ordering::Relaxed) {
                    let due = (start.elapsed().as_secs_f64() * f64::from(rate)) as u64;
                    while produced < due && !flag.load(Ordering::Relaxed) {
                        let n = ((due - produced) as usize).min(MAX_CHUNK);
                        gen_.fill(&mut buf[..n * 2]);
                        feed.push(&buf[..n * 2]);
                        produced += n as u64;
                    }
                    std::thread::sleep(TICK);
                }
                feed
            })
            .map_err(|e| (e.to_string(), None))?;
        Ok(Self {
            stop,
            thread: Some(thread),
        })
    }

    /// Hält die Quelle an und gibt den Feed zurück.
    pub fn stop(mut self) -> Option<GuestFeed> {
        self.stop.store(true, Ordering::Relaxed);
        self.thread.take().and_then(|t| t.join().ok())
    }
}

impl Drop for TestSource {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sine_has_the_right_level_and_is_continuous() {
        let mut g = SineGen::new(1000.0, 48_000, 0.5);
        let mut a = vec![0.0; 480];
        let mut b = vec![0.0; 480];
        g.fill(&mut a);
        g.fill(&mut b);
        let peak = a.iter().chain(&b).fold(0.0f32, |m, v| m.max(v.abs()));
        assert!((peak - 0.5).abs() < 0.01, "{peak}");
        // 1 kHz bei 48 kHz: 48 Frames je Periode, Phase läuft über die Blockgrenze weiter.
        assert!((a[0] - 0.0).abs() < 1e-6 && (b[0] - 0.0).abs() < 0.02);
        assert!(a.chunks(2).all(|f| f[0] == f[1]));
    }

    #[test]
    fn missing_file_is_an_error_not_a_thread() {
        let (feed, _r) = crate::jitter::pair(
            48_000,
            std::sync::Arc::new(crate::jitter::SlotStats::default()),
        );
        let e = TestSource::spawn(
            &TestSignal::File(PathBuf::from("/nicht/da.wav")),
            feed,
            48_000,
        );
        assert!(matches!(e, Err((_, Some(_)))));
    }
}
