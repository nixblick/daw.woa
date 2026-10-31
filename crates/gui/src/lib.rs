//! egui-Oberfläche (M0-Prototyp): Spurliste, Undo/Redo, Datei, Autosave, Feedback.
//!
//! Die Logik steckt fensterunabhängig in [`state`] und [`feedback`] (testbar ohne
//! Display); [`app`] ist nur die dünne eframe-Schicht darüber.

pub mod analysis;
pub mod app;
mod assistant_ui;
pub mod automation;
mod chat_ui;
pub mod clock_editor;
pub mod controls;
pub mod covers;
pub mod cue_editor;
pub mod event_editor;
pub mod export_dialog;
pub mod feedback;
mod ki_ui;
pub mod library;
pub mod manage;
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
mod stream_ui;
pub mod tag_editor;
pub mod theme;
pub mod timeline;
pub mod titel_tab;
pub mod update;
pub mod voice_editor;
pub mod widgets;
mod wishes;
mod xrun;

pub use app::WoaApp;
pub use feedback::{GuiConfig, Mode, PflOutput};
pub use phone::PhoneInput;
pub use state::AppState;
