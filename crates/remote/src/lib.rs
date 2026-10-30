//! Party-Fernbedienung (Paket 36): lokaler Webserver im WLAN.
//!
//! - [`Remote`] öffnet den Port, bedient die Handy-Seite und schickt Befehle an die
//!   Oberfläche. Beim Ausschalten ist der Port zu.
//! - Zugang per QR-Code mit PIN, Rollen Gast (nur Wünsche) und DJ (steuern), siehe [`auth`].
//! - Ein Handy bekommt beim Verbinden den Vollstand, danach nur Änderungen ([`snapshot`]).
//! - Befehle sind die [`woa_radio::party::PartyCommand`] der Party-Schicht, dieselben wie bei
//!   Tasten und MIDI ([`command`]).
//! - Optional ein OSC-Eingang ([`osc`]), standardmäßig aus.
//!
//! Läuft nie im Audio-Thread und fasst die Engine nicht an.

pub mod auth;
pub mod bridge;
pub mod command;
pub mod osc;
pub mod qr;
mod server;
pub mod snapshot;

pub use auth::{Limits, Role};
pub use command::RemoteCommand;
pub use server::{Remote, RemoteConfig, RemoteError, lan_ip};
pub use snapshot::{CatalogItem, NowSnap, PartySnapshot, WishSnap};
