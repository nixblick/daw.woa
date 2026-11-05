//! CLAP-Host: Plugins finden ([`scan`]), laden und im Audio-Graphen spielen ([`host`]).

pub mod child;
mod feedback;
mod host;
mod instance;
mod rack;
pub mod remote;
pub mod safety;
mod scan;
pub mod shm;
// Nur fuer Tests (Feature `test-plugin`), darf daher bei Fehlern abbrechen.
#[cfg(any(test, feature = "test-plugin"))]
#[allow(clippy::unwrap_used, clippy::expect_used)]
pub mod testplugin;

pub use feedback::{ChangeKind, Feedback, LOOSE_LIMIT, ParamChange, ParamEdit};
pub use host::{ClapInstance, HostError, MAX_FRAMES, ParamDesc};
pub use instance::Instance;
pub use rack::{CRASHED, PluginRack, SlotStatus};
pub use remote::{HOST_FLAG, Health, HostConfig, Isolation, RemoteInstance};
pub use safety::{BlockEntry, Blocklist, PluginLogs};
pub use scan::{Catalog, PluginInfo, ScanProblem, search_paths};
