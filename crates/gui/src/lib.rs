//! egui-Oberfläche (M0-Prototyp): Spurliste, Undo/Redo, Datei, Autosave, Feedback.
//!
//! Die Logik steckt fensterunabhängig in [`state`] und [`feedback`] (testbar ohne
//! Display); [`app`] ist nur die dünne eframe-Schicht darüber.

pub mod app;
pub mod covers;
pub mod feedback;
pub mod player;
pub mod radio;
mod sources;
pub mod state;
mod theme;

pub use app::WoaApp;
pub use feedback::{GuiConfig, Mode};
pub use state::AppState;
