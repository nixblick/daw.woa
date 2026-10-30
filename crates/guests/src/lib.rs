//! # woa-guests: Gast-Kanäle mit Mix-Minus (Paket 37)
//!
//! Je Gast ein Mixer-Kanal mit Fader und Pegelanzeige und ein eigener Rückweg:
//! die Summe der Sendung ohne die eigene Stimme des Gastes (Mix-Minus), damit der Gast sich
//! nicht selbst mit Verzögerung hört.
//!
//! Aufteilung nach den Echtzeitregeln (CLAUDE.md):
//! - [`bank::GuestBank`] lebt im Audio-Thread (im Sendemixer). Keine Allokation, kein Lock,
//!   kein I/O, kein Logging. Befehle kommen über eine `rtrb`-Queue, Audio über je einen
//!   `rtrb`-Ring pro Kanal, Messwerte gehen über Atomics zurück.
//! - [`hub::GuestHub`] lebt im Control-Thread: legt Gäste an und ab, startet Testquellen,
//!   zeigt Pegel und Zähler an und schreibt Aussetzer ins Log.
//! - [`jitter`]: Jitterpuffer mit Vorlauf und Obergrenze (Muster `buffer`/`max` aus dem
//!   Harbor-Eingang von Liquidsoap, nur die Idee).
//! - [`source`]: Testquellen (Sinus, Datei in Schleife). Die Netzquelle (WebRTC) hängt später
//!   am selben [`jitter::GuestFeed`].

pub mod bank;
pub mod hub;
pub mod jitter;
pub mod source;

pub use bank::{GuestBank, GuestCmd};
pub use hub::{GuestHub, GuestView, channel};
pub use jitter::{GuestFeed, SlotStats};
pub use source::TestSignal;

/// Höchstzahl gleichzeitiger Gäste.
pub const MAX_GUESTS: usize = 8;
