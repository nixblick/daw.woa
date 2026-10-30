//! CLAP-Host: Plugins finden ([`scan`]), laden und im Audio-Graphen spielen ([`host`]).

mod host;
mod rack;
mod scan;
#[cfg(any(test, feature = "test-plugin"))]
pub mod testplugin;

pub use host::{ClapInstance, HostError, MAX_FRAMES, ParamDesc};
pub use rack::{PluginRack, SlotStatus};
pub use scan::{Catalog, PluginInfo, ScanProblem, search_paths};
