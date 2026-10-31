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
mod outputs;
mod outputs_ui;
pub mod party;
mod phone;
pub mod pianoroll;
pub mod player;
pub mod protokoll;
pub mod radio;
pub mod remote;
mod sources;
pub mod state;
pub mod theme;
pub mod timeline;
pub mod update;
pub mod voice_editor;
pub mod widgets;
mod wishes;
mod xrun;

pub use app::WoaApp;
pub use feedback::{GuiConfig, Mode, PflOutput};
pub use phone::PhoneInput;
pub use state::AppState;
