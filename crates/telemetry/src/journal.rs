//! Protokoll für das Fenster in der App: ein Ringpuffer im Speicher, gespeist als
//! `tracing`-Layer. Nur lokal, nichts davon geht in den Upload.
//!
//! - [`JournalLayer`] nimmt Info, Warnungen und Fehler auf, höchstens [`MAX_ENTRIES`]
//!   Zeilen. Gleiche Meldungen (Stufe, Text, Datei) werden zu einer Zeile mit Zähler.
//! - [`FaultCounter`] zählt je Thread die Frame-Warnungen des MP3-Dekoders, damit der
//!   Dekoder je Datei melden kann, wie viele Stellen fehlerhaft waren
//!   ([`take_decoder_faults`]).
//! - [`LOCAL_TARGET`]: Meldungen mit Pfad oder Dateiname; die Upload-Senke sieht sie nie.

use std::cell::Cell;
use std::collections::VecDeque;
use std::fmt::Write as _;
use std::sync::{Mutex, OnceLock};
use std::time::SystemTime;

use tracing::field::{Field, Visit};
use tracing_log::NormalizeEvent;
use tracing_subscriber::Layer;
use tracing_subscriber::layer::Context;

/// Höchstzahl der Zeilen im Protokoll.
pub const MAX_ENTRIES: usize = 2000;

/// Ziel für Meldungen, die nur lokal bleiben (Dateinamen, Pfade).
pub const LOCAL_TARGET: &str = "woa_lokal";

/// Stufe einer Protokollzeile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Stufe {
    Info,
    Warnung,
    Fehler,
}

impl Stufe {
    fn from_tracing(l: &tracing::Level) -> Option<Self> {
        match *l {
            tracing::Level::ERROR => Some(Self::Fehler),
            tracing::Level::WARN => Some(Self::Warnung),
            tracing::Level::INFO => Some(Self::Info),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Info => "Info",
            Self::Warnung => "Warnung",
            Self::Fehler => "Fehler",
        }
    }
}

/// Eine Zeile im Protokoll.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    /// Zeit der letzten gleichen Meldung.
    pub zeit: SystemTime,
    pub stufe: Stufe,
    /// Herkunft (Modulpfad), z. B. `woa_engine::decode`.
    pub ziel: String,
    pub text: String,
    /// Dateiname, wenn die Meldung das Feld `datei` trägt.
    pub datei: Option<String>,
    /// Wie oft die Meldung kam.
    pub anzahl: u64,
}

/// Der Ringpuffer.
#[derive(Debug, Default)]
pub struct Journal {
    entries: Mutex<VecDeque<Entry>>,
}

impl Journal {
    /// Nimmt eine Meldung auf; eine gleiche wird hochgezählt und ans Ende gesetzt.
    pub fn push(&self, stufe: Stufe, ziel: &str, text: String, datei: Option<String>) {
        let Ok(mut e) = self.entries.lock() else {
            return;
        };
        let now = SystemTime::now();
        if let Some(i) = e
            .iter()
            .rposition(|x| x.stufe == stufe && x.text == text && x.datei == datei)
        {
            if let Some(mut x) = e.remove(i) {
                x.anzahl += 1;
                x.zeit = now;
                e.push_back(x);
            }
            return;
        }
        if e.len() >= MAX_ENTRIES {
            e.pop_front();
        }
        e.push_back(Entry {
            zeit: now,
            stufe,
            ziel: ziel.to_string(),
            text,
            datei,
            anzahl: 1,
        });
    }

    /// Kopie aller Zeilen, älteste zuerst.
    pub fn snapshot(&self) -> Vec<Entry> {
        self.entries
            .lock()
            .map(|e| e.iter().cloned().collect())
            .unwrap_or_default()
    }

    pub fn len(&self) -> usize {
        self.entries.lock().map(|e| e.len()).unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn clear(&self) {
        if let Ok(mut e) = self.entries.lock() {
            e.clear();
        }
    }
}

/// Das Protokoll der laufenden App.
pub fn journal() -> &'static Journal {
    static J: OnceLock<Journal> = OnceLock::new();
    J.get_or_init(Journal::default)
}

/// Layer, der in einen [`Journal`] schreibt (Standard: [`journal`]).
pub struct JournalLayer {
    target: &'static Journal,
}

impl JournalLayer {
    pub fn new(target: &'static Journal) -> Self {
        Self { target }
    }
}

impl Default for JournalLayer {
    fn default() -> Self {
        Self::new(journal())
    }
}

#[derive(Default)]
struct Fields {
    message: String,
    datei: Option<String>,
    rest: String,
}

impl Visit for Fields {
    fn record_str(&mut self, field: &Field, value: &str) {
        match field.name() {
            "message" => self.message = value.to_string(),
            "datei" => self.datei = Some(value.to_string()),
            n if n.starts_with("log.") => {}
            n => {
                let _ = write!(self.rest, " {n}={value}");
            }
        }
    }

    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        match field.name() {
            "message" => self.message = format!("{value:?}"),
            "datei" => self.datei = Some(format!("{value:?}").trim_matches('"').to_string()),
            n if n.starts_with("log.") => {}
            n => {
                let _ = write!(self.rest, " {n}={value:?}");
            }
        }
    }
}

impl<S: tracing::Subscriber> Layer<S> for JournalLayer {
    fn on_event(&self, event: &tracing::Event<'_>, _ctx: Context<'_, S>) {
        let normalized = event.normalized_metadata();
        let meta = normalized.as_ref().unwrap_or_else(|| event.metadata());
        let Some(stufe) = Stufe::from_tracing(meta.level()) else {
            return;
        };
        let mut f = Fields::default();
        event.record(&mut f);
        let text = format!("{}{}", f.message, f.rest);
        self.target.push(stufe, meta.target(), text, f.datei);
    }
}

thread_local! {
    static FAULTS: Cell<u64> = const { Cell::new(0) };
}

/// Frame-Warnungen des Dekoders in diesem Thread seit dem letzten Aufruf; setzt auf 0.
pub fn take_decoder_faults() -> u64 {
    FAULTS.with(|c| c.replace(0))
}

/// Layer, der die Frame-Warnungen des MP3-Dekoders je Thread zählt. Läuft vor dem
/// [`crate::noise_filter`], der sie für Datei und Upload verwirft.
#[derive(Debug, Default, Clone, Copy)]
pub struct FaultCounter;

impl FaultCounter {
    /// Filter für den Zähler: nur die Warnungen des MP3-Dekoders. Ein `filter_fn`, kein
    /// `Targets`: Meldungen aus dem `log`-Crate teilen sich eine Callsite mit Ziel `log`,
    /// die `Targets` sonst für immer abschaltet.
    pub fn filter() -> tracing_subscriber::filter::FilterFn<impl Fn(&tracing::Metadata<'_>) -> bool>
    {
        tracing_subscriber::filter::filter_fn(|m| {
            *m.level() == tracing::Level::WARN && m.target() == "symphonia_bundle_mp3::layer3"
        })
    }
}

impl<S: tracing::Subscriber> Layer<S> for FaultCounter {
    fn on_event(&self, event: &tracing::Event<'_>, _ctx: Context<'_, S>) {
        let normalized = event.normalized_metadata();
        let meta = normalized.as_ref().unwrap_or_else(|| event.metadata());
        if *meta.level() == tracing::Level::WARN {
            FAULTS.with(|c| c.set(c.get() + 1));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tracing_subscriber::layer::SubscriberExt;

    fn leak() -> &'static Journal {
        Box::leak(Box::default())
    }

    #[test]
    fn equal_messages_merge_and_buffer_is_bounded() {
        let j = leak();
        for _ in 0..1084 {
            j.push(Stufe::Warnung, "x", "kaputt".into(), Some("a.mp3".into()));
        }
        j.push(Stufe::Warnung, "x", "kaputt".into(), Some("b.mp3".into()));
        let s = j.snapshot();
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].anzahl, 1084);
        assert_eq!(s[1].datei.as_deref(), Some("b.mp3"));
        for i in 0..MAX_ENTRIES + 10 {
            j.push(Stufe::Info, "x", format!("m{i}"), None);
        }
        assert_eq!(j.len(), MAX_ENTRIES);
        assert_eq!(
            j.snapshot().last().unwrap().text,
            format!("m{}", MAX_ENTRIES + 9)
        );
    }

    #[test]
    fn layer_takes_info_and_up_with_file_field() {
        let j = leak();
        let sub = tracing_subscriber::registry().with(JournalLayer::new(j));
        tracing::subscriber::with_default(sub, || {
            tracing::debug!("leise");
            tracing::info!("gestartet");
            tracing::warn!(target: LOCAL_TARGET, datei = "x.mp3", "Datei „x.mp3“ ist beschädigt");
            tracing::error!(code = 3, "geht nicht");
        });
        let s = j.snapshot();
        assert_eq!(s.len(), 3);
        assert_eq!(s[0].stufe, Stufe::Info);
        assert_eq!(s[1].datei.as_deref(), Some("x.mp3"));
        assert_eq!(s[1].ziel, LOCAL_TARGET);
        assert_eq!(s[2].text, "geht nicht code=3");
    }

    #[test]
    fn fault_counter_counts_per_thread() {
        use tracing_subscriber::Layer as _;
        let sub =
            tracing_subscriber::registry().with(FaultCounter.with_filter(FaultCounter::filter()));
        tracing::subscriber::with_default(sub, || {
            take_decoder_faults();
            for _ in 0..5 {
                tracing::warn!(target: "symphonia_bundle_mp3::layer3", "underflow");
            }
            tracing::warn!(target: "symphonia_bundle_mp3::demuxer", "anderes");
            assert_eq!(take_decoder_faults(), 5);
            assert_eq!(take_decoder_faults(), 0);
        });
    }
}
