//! Aufnahmequellen je Spur: Standard-Eingang über cpal (mit gemessener Latenz) und unter
//! Linux alles, was PipeWire anbietet (Eingänge, Systemklang, einzelne Programme).

use std::path::Path;
use std::time::Duration;

use woa_engine::record::{InputConfig, RecordStats, Recorder, start_recording};

/// Schlüssel des Standard-Eingangs (cpal). Jede Spur ohne Wahl nimmt ihn.
pub const DEFAULT_KEY: &str = "";

/// Eine Quelle in der Auswahlliste.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceEntry {
    pub key: String,
    pub label: String,
    /// Mithören möglich (nur Pulse-Quellen).
    pub can_monitor: bool,
}

/// Alle Quellen, Standard-Eingang zuerst. Fehler beim Abfragen von PipeWire landen im Log,
/// die Liste hat dann nur den Standard-Eingang.
pub fn list() -> Vec<SourceEntry> {
    #[cfg_attr(not(target_os = "linux"), allow(unused_mut))]
    let mut out = vec![SourceEntry {
        key: DEFAULT_KEY.to_string(),
        label: "Standard-Eingang (Mikrofon)".to_string(),
        can_monitor: false,
    }];
    #[cfg(target_os = "linux")]
    match woa_engine::capture::list_sources() {
        Ok(list) => out.extend(list.into_iter().map(|s| SourceEntry {
            label: match s.kind {
                woa_engine::capture::SourceKind::Input => format!("Eingang: {}", s.label),
                woa_engine::capture::SourceKind::System => s.label,
                woa_engine::capture::SourceKind::App => format!("Programm: {}", s.label),
            },
            key: s.key,
            can_monitor: true,
        })),
        Err(e) => tracing::warn!(error = %e, "listing pipewire sources failed"),
    }
    out
}

/// Kurzer Name eines Schlüssels für die Spur, ohne die Liste abzufragen.
pub fn short_label(key: &str) -> String {
    match key.split_once(':') {
        None => "Mikrofon".to_string(),
        Some(("sys", _)) => "Systemklang".to_string(),
        Some(("app", name)) => app_name(name),
        Some((_, name)) => format!("Eingang {}", name.rsplit('.').next().unwrap_or(name)),
    }
}

/// „PipeWire ALSA [vlc]“ → „vlc“, sonst unverändert.
pub fn app_name(name: &str) -> String {
    name.split_once('[')
        .and_then(|(_, r)| r.strip_suffix(']'))
        .unwrap_or(name)
        .to_string()
}

/// Eine laufende Aufnahme einer Spur.
pub enum Take {
    Device(Recorder),
    #[cfg(target_os = "linux")]
    Pulse(woa_engine::capture::CaptureRecorder, bool),
}

impl Take {
    /// Startet die Quelle `key` nach `path`.
    pub fn start(key: &str, path: &Path, monitor: bool) -> Result<Self, String> {
        if key == DEFAULT_KEY {
            return start_recording(&InputConfig::default(), path, |e| {
                tracing::warn!(error = %e, "input stream error");
            })
            .map(Take::Device)
            .map_err(|e| e.to_string());
        }
        #[cfg(target_os = "linux")]
        {
            use woa_engine::capture::{SourceKind, list_sources, start_capture};
            let source = list_sources()
                .map_err(|e| e.to_string())?
                .into_iter()
                .find(|s| s.key == key)
                .ok_or_else(|| match key.split_once(':') {
                    Some(("app", name)) => {
                        format!("{} spielt gerade keinen Ton", app_name(name))
                    }
                    _ => format!("Quelle {} nicht gefunden", short_label(key)),
                })?;
            let input = source.kind == SourceKind::Input;
            start_capture(&source, path, monitor)
                .map(|r| Take::Pulse(r, input))
                .map_err(|e| e.to_string())
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (path, monitor);
            Err(format!(
                "Quelle {} gibt es nur unter Linux",
                short_label(key)
            ))
        }
    }

    pub fn take_level(&self) -> f32 {
        match self {
            Take::Device(r) => r.take_level(),
            #[cfg(target_os = "linux")]
            Take::Pulse(r, _) => r.take_level(),
        }
    }

    pub fn elapsed(&self) -> Duration {
        match self {
            Take::Device(r) => r.elapsed(),
            #[cfg(target_os = "linux")]
            Take::Pulse(r, _) => r.elapsed(),
        }
    }

    pub fn path(&self) -> &Path {
        match self {
            Take::Device(r) => r.path(),
            #[cfg(target_os = "linux")]
            Take::Pulse(r, _) => r.path(),
        }
    }

    /// Beendet die Aufnahme. Liefert dazu die gemessene Latenz, um die der Clip nach vorn
    /// gehört: bei Eingängen Ein- plus Ausgangslatenz (Overdub), bei Systemklang und
    /// Programmen nichts, die laufen nicht gegen die Wiedergabe von WOA.
    pub fn stop(self) -> Result<(RecordStats, Duration), String> {
        let out = woa_engine::device::output_latency();
        match self {
            Take::Device(r) => r
                .stop()
                .map(|s| {
                    let l = s.input_latency + out;
                    (s, l)
                })
                .map_err(|e| e.to_string()),
            #[cfg(target_os = "linux")]
            Take::Pulse(r, input) => r
                .stop()
                .map(|s| (s, if input { out } else { Duration::ZERO }))
                .map_err(|e| e.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_are_short() {
        assert_eq!(short_label(""), "Mikrofon");
        assert_eq!(short_label("sys:alsa_output.pci.monitor"), "Systemklang");
        assert_eq!(short_label("app:PipeWire ALSA [vlc]"), "vlc");
        assert_eq!(short_label("app:Firefox"), "Firefox");
    }
}
