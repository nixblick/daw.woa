//! Schnittstelle zwischen Engine und Fremd-Plugins (Paket 31). Die Engine kennt keine
//! Plugin-Bibliothek: Sie bekommt je Spur ein [`TrackPlugins`] mit Objekten, die
//! [`StereoProcessor`] können. `woa-plugins` liefert sie (CLAP), Tests liefern Attrappen.
//!
//! Echtzeitregel: `process` und `set_param` allokieren nie, sperren nie und machen kein I/O.

use crate::instrument::NoteEvent;

/// Reglerkennung, die ein Plugin-Knoten als „Umgehen“ (Wert 0 oder 1) versteht; CLAP-Kennungen
/// benutzen `u32::MAX` nie.
pub const BYPASS_PARAM: u32 = u32::MAX;

/// Ein Klangerzeuger oder Effekt mit zwei Kanälen, der Note-Events und Reglerwerte versteht.
pub trait StereoProcessor: Send {
    /// Setzt einen Regler (Kennung des Plugins, nicht Position); echtzeitsicher. Der Wert
    /// wirkt ab dem nächsten Block.
    fn set_param(&mut self, id: u32, value: f64);

    /// Rechnet einen Block. `events` sind nach `offset` sortiert, `offset` liegt im Block.
    /// Instrumente ignorieren den Eingang, Effekte die Events. Alle vier Puffer sind gleich
    /// lang. Echtzeitsicher; fällt das Plugin aus, schreibt der Aufruf Stille.
    fn process(
        &mut self,
        events: &[NoteEvent],
        in_left: &[f32],
        in_right: &[f32],
        out_left: &mut [f32],
        out_right: &mut [f32],
    );

    /// Alle Noten beenden und Hallfahnen leeren (nach Sprung oder Stopp); echtzeitsicher.
    fn reset(&mut self);
}

/// Ein Effekt-Plugin in der Kette einer Spur.
pub struct PluginEffect {
    /// Kennung des Eintrags im Projekt (`PluginSlot::id`).
    pub id: u64,
    /// Der Effekt.
    pub processor: Box<dyn StereoProcessor>,
    /// Umgangen: das Signal läuft unverändert durch.
    pub bypass: bool,
}

/// Alle Plugins einer Spur, so wie die Engine sie in den Graphen hängt.
#[derive(Default)]
pub struct TrackPlugins {
    /// Instrument einer MIDI-Spur; ersetzt den eingebauten Synth.
    pub instrument: Option<Box<dyn StereoProcessor>>,
    /// Effekte, erster zuerst; sie laufen nach den eingebauten Effekten der Spur.
    pub effects: Vec<PluginEffect>,
}

impl TrackPlugins {
    /// Ob die Spur gar kein Plugin hat.
    pub fn is_empty(&self) -> bool {
        self.instrument.is_none() && self.effects.is_empty()
    }
}
