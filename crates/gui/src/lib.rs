//! egui-Oberfläche (M0-Prototyp): Spurliste, Undo/Redo, Datei, Autosave, Feedback.
//!
//! Die Logik steckt fensterunabhängig in [`state`] und [`feedback`] (testbar ohne
//! Display); [`app`] ist nur die dünne eframe-Schicht darüber.

pub mod app;
pub mod feedback;
pub mod state;

pub use app::WoaApp;
pub use feedback::GuiConfig;
pub use state::AppState;
