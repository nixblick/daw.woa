//! Aufnahmequellen je Spur: Standard-Eingang über cpal (mit gemessener Latenz) und unter
//! Linux alles, was PipeWire anbietet (Eingänge, Systemklang, einzelne Programme). Auf
//! Windows und macOS stehen alle Eingabegeräte des Systems in der Liste (#163).

use std::path::Path;
use std::time::Duration;

use woa_engine::record::{InputConfig, RecordStats, Recorder, start_recording};

/// Schlüssel des Standard-Eingangs (cpal). Jede Spur ohne Wahl nimmt ihn.
pub const DEFAULT_KEY: &str = "";

/// Präfix der Schlüssel für ein bestimmtes Eingabegerät (cpal), `dev:<Geräte-ID>`.
pub const DEVICE_PREFIX: &str = "dev:";

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
    // Ohne PipeWire kannte die Liste nur den Standard-Eingang (#163): alle Eingabegeräte
    // des Systems dazu. Unter Linux kommen sie schon über PipeWire.
    #[cfg(not(target_os = "linux"))]
    match woa_engine::record::list_input_devices() {
        Ok(list) => out.extend(device_entries(list)),
        Err(e) => tracing::warn!(error = %e, "listing input devices failed"),
    }
    out
}

/// Eingabegeräte als Einträge, ohne das Standardgerät (das steht schon oben).
#[cfg_attr(target_os = "linux", allow(dead_code))]
pub fn device_entries(
    devices: Vec<woa_engine::device::OutputDeviceInfo>,
) -> impl Iterator<Item = SourceEntry> {
    devices
        .into_iter()
        .filter(|d| !d.is_default)
        .map(|d| SourceEntry {
            key: format!("{DEVICE_PREFIX}{}", d.id),
            label: format!("Eingang: {}", d.name),
            can_monitor: false,
        })
}

/// Hinweis in der Auswahl, wenn kein Programm Ton abspielt.
pub const NO_APPS_HINT: &str = "Programm spielt gerade keinen Ton?";

/// Ob die Liste mindestens ein Programm enthält.
pub fn has_apps(list: &[SourceEntry]) -> bool {
    list.iter().any(|s| s.key.starts_with("app:"))
}

/// Kurzer Name eines Schlüssels für die Spur, ohne die Liste abzufragen.
pub fn short_label(key: &str) -> String {
    match key.split_once(':') {
        None => "Mikrofon".to_string(),
        Some(("dev", _)) => "Eingang".to_string(),
        Some(("sys", _)) => "Systemklang".to_string(),
        Some(("app", name)) => app_name(name),
        Some((_, name)) => format!("Eingang {}", name.rsplit('.').next().unwrap_or(name)),
    }
}

/// Name für die Spur: bei einem Eingabegerät der Gerätename aus `list`, sonst
/// [`short_label`].
pub fn track_label(key: &str, list: &[SourceEntry]) -> String {
    if key.starts_with(DEVICE_PREFIX)
        && let Some(s) = list.iter().find(|s| s.key == key)
    {
        return s.label.trim_start_matches("Eingang: ").to_string();
    }
    short_label(key)
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
        if let Some(id) = key.strip_prefix(DEVICE_PREFIX) {
            let config = InputConfig {
                device_id: Some(id.to_string()),
                ..InputConfig::default()
            };
            return start_recording(&config, path, |e| {
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

    /// Neue Wellenform-Punkte der laufenden Aufnahme an `out` anhängen (#43).
    pub fn drain_peaks(&mut self, out: &mut Vec<f32>) {
        match self {
            Take::Device(r) => r.drain_peaks(out),
            #[cfg(target_os = "linux")]
            Take::Pulse(r, _) => r.drain_peaks(out),
        }
    }

    /// Ob die Aufnahme vorzeitig abgebrochen ist (Schreibfehler); `stop` liefert dann den Fehler.
    pub fn failed(&self) -> bool {
        match self {
            Take::Device(r) => r.writer_failed(),
            #[cfg(target_os = "linux")]
            Take::Pulse(..) => false,
        }
    }

    /// Abtastrate der Aufnahme, Bezug für die Wellenform-Punkte.
    pub fn sample_rate(&self) -> u32 {
        match self {
            Take::Device(r) => r.sample_rate(),
            #[cfg(target_os = "linux")]
            Take::Pulse(..) => woa_engine::capture::RATE,
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

    /// #163: Auf Windows stand nur der Standard-Eingang in der Liste. Jedes weitere
    /// Eingabegerät wird ein eigener Eintrag mit Gerätekennung im Schlüssel.
    #[test]
    fn every_input_device_becomes_a_source() {
        use woa_engine::device::OutputDeviceInfo;
        let dev = |id: &str, name: &str, is_default| OutputDeviceInfo {
            id: id.into(),
            name: name.into(),
            is_default,
        };
        let list: Vec<_> = device_entries(vec![
            dev("wasapi:mic", "Mikrofon (USB)", true),
            dev("wasapi:line", "Line-In", false),
            dev("wasapi:mix", "Stereomix", false),
        ])
        .collect();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].key, "dev:wasapi:line");
        assert_eq!(list[0].label, "Eingang: Line-In");
        assert_eq!(list[1].label, "Eingang: Stereomix");
        assert_eq!(short_label(&list[0].key), "Eingang");
        assert_eq!(track_label(&list[0].key, &list), "Line-In");
        assert_eq!(track_label("", &list), "Mikrofon");
    }

    #[test]
    fn hint_only_without_apps() {
        let entry = |key: &str| SourceEntry {
            key: key.into(),
            label: key.into(),
            can_monitor: true,
        };
        assert!(!has_apps(&[entry(""), entry("sys:x.monitor")]));
        assert!(has_apps(&[entry(""), entry("app:Chromium")]));
    }
}
