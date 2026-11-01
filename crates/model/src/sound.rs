//! Klang einer MIDI-Spur aus der General-MIDI-SoundFont (Paket J1, Issue #261).
//!
//! Ohne [`TrackSound`] spielt die Spur den eingebauten Synth. Mit einem Klang spielt sie
//! den SoundFont-Player: ein GM-Programm (0..=127) oder, mit `drums`, ein Drumkit auf
//! MIDI-Kanal 10. Fehlt die SoundFont zur Laufzeit, bleibt es beim Synth.

use serde::{Deserialize, Serialize};

/// GM-Klang einer MIDI-Spur.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrackSound {
    /// GM-Programm 0..=127; bei `drums` die Kit-Nummer (0 = Standard).
    pub program: u8,
    /// `true`: Drumkit auf Kanal 10 (GM-Drumnoten, 36 = Kick, 38 = Snare, 42 = Hi-Hat).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub drums: bool,
    /// Zusatzbank (Paket J7); `None`: GeneralUser GS. Fehlt die Bank, spielt GeneralUser GS.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bank: Option<SoundBank>,
}

/// Zusatzbank, die beim ersten Gebrauch geladen wird (Paket J7, Issue #271). Der Katalog mit
/// URL, Prüfsumme und Lizenz steht in `docs/sounds/banks.tsv` (gelesen von `woa_engine::banks`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SoundBank {
    /// MuseScore General 0.2.0 (MIT), General MIDI.
    MusescoreGeneral,
    /// AVL Drumkits „Red Zeppelin 4pc“ (CC BY-SA 3.0), nur Drums.
    AvlRedZeppelin,
    /// Eine Bank, die dieser Bau nicht kennt; spielt GeneralUser GS.
    #[serde(other)]
    Unknown,
}

impl SoundBank {
    /// Alle bekannten Bänke, in Anzeigereihenfolge.
    pub const ALL: [SoundBank; 2] = [Self::MusescoreGeneral, Self::AvlRedZeppelin];

    /// Kennung in Projektdatei, Katalog und MCP.
    pub fn id(self) -> &'static str {
        match self {
            Self::MusescoreGeneral => "musescore-general",
            Self::AvlRedZeppelin => "avl-red-zeppelin",
            Self::Unknown => "unknown",
        }
    }

    /// Anzeigename.
    pub fn name(self) -> &'static str {
        match self {
            Self::MusescoreGeneral => "MuseScore General",
            Self::AvlRedZeppelin => "AVL Red Zeppelin",
            Self::Unknown => "Unbekannte Bank",
        }
    }

    /// `true`: die Bank hat nur ein Drumkit und gilt nur für Klänge mit `drums`.
    pub fn drums_only(self) -> bool {
        self == Self::AvlRedZeppelin
    }

    /// Bank zur Kennung (Gross-/Kleinschreibung egal).
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|b| b.id().eq_ignore_ascii_case(id))
    }
}

impl TrackSound {
    /// MIDI-Kanal (0-basiert): 9 für Drums, sonst 0.
    pub fn channel(&self) -> u8 {
        if self.drums { 9 } else { 0 }
    }
}

/// Rolle einer Klangauswahl (KISS: wenige sinnvolle Klänge je Rolle, Issue #261).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SoundRole {
    Lead,
    Bass,
    Pad,
    Kit,
}

impl SoundRole {
    /// Name für Oberfläche und MCP.
    pub fn name(self) -> &'static str {
        match self {
            Self::Lead => "lead",
            Self::Bass => "bass",
            Self::Pad => "pad",
            Self::Kit => "kit",
        }
    }
}

/// Ein Eintrag der Klangauswahl.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SoundPreset {
    pub role: SoundRole,
    /// Anzeigename.
    pub name: &'static str,
    pub sound: TrackSound,
}

const fn gm(role: SoundRole, name: &'static str, program: u8) -> SoundPreset {
    SoundPreset {
        role,
        name,
        sound: TrackSound {
            program,
            drums: false,
            bank: None,
        },
    }
}

const fn kit(name: &'static str, program: u8) -> SoundPreset {
    SoundPreset {
        role: SoundRole::Kit,
        name,
        sound: TrackSound {
            program,
            drums: true,
            bank: None,
        },
    }
}

/// Die Klangauswahl: je Rolle vier GM-Klänge (GeneralUser GS kennt alle).
pub const SOUND_PRESETS: &[SoundPreset] = &[
    gm(SoundRole::Lead, "Klavier", 0),
    gm(SoundRole::Lead, "E-Piano", 4),
    gm(SoundRole::Lead, "Glockenspiel", 9),
    gm(SoundRole::Lead, "Synth-Lead", 80),
    gm(SoundRole::Bass, "E-Bass", 33),
    gm(SoundRole::Bass, "Kontrabass", 32),
    gm(SoundRole::Bass, "Synth-Bass", 38),
    gm(SoundRole::Bass, "Fretless", 35),
    gm(SoundRole::Pad, "Streicher", 48),
    gm(SoundRole::Pad, "Warm Pad", 89),
    gm(SoundRole::Pad, "Orgel", 16),
    gm(SoundRole::Pad, "Chor", 52),
    kit("Standard-Kit", 0),
    kit("Room-Kit", 8),
    kit("Power-Kit", 16),
    kit("Elektro-Kit", 24),
];

/// Der Preset-Name zu einem Klang, falls er in der Auswahl steht (die Bank zählt nicht).
pub fn preset_name(sound: TrackSound) -> Option<&'static str> {
    SOUND_PRESETS
        .iter()
        .find(|p| p.sound.program == sound.program && p.sound.drums == sound.drums)
        .map(|p| p.name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_are_valid_and_unique() {
        for (i, p) in SOUND_PRESETS.iter().enumerate() {
            assert!(p.sound.program <= 127);
            assert_eq!(p.sound.drums, p.role == SoundRole::Kit);
            assert!(SOUND_PRESETS[..i].iter().all(|q| q.sound != p.sound));
        }
        assert_eq!(
            preset_name(TrackSound {
                program: 0,
                drums: true,
                bank: None,
            }),
            Some("Standard-Kit")
        );
        assert_eq!(
            TrackSound {
                program: 0,
                drums: true,
                bank: None,
            }
            .channel(),
            9
        );
    }
}
