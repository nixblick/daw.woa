//! egui-Oberfläche (M0-Prototyp): Spurliste, Undo/Redo, Datei, Autosave, Feedback.
//!
//! Die Logik steckt fensterunabhängig in [`state`] und [`feedback`] (testbar ohne
//! Display); [`app`] ist nur die dünne eframe-Schicht darüber.

pub mod analysis;
pub mod app;
pub mod automation;
pub mod clock_editor;
pub mod controls;
pub mod covers;
pub mod cue_editor;
pub mod event_editor;
pub mod export_dialog;
pub mod feedback;
pub mod library;
pub mod pianoroll;
pub mod player;
pub mod radio;
mod sources;
pub mod state;
mod theme;
pub mod timeline;
mod xrun;

pub use app::WoaApp;
pub use feedback::{GuiConfig, Mode, PflOutput};
pub use state::AppState;
