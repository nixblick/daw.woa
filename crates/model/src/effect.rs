//! Effekte in der Insert-Kette einer Spur (Paket 16 und 17): welche Arten es gibt, welche
//! Regler sie haben, Voreinstellungen und die Vorlage „Sprachkette“. Das Modell kennt nur
//! Namen, Bereiche und Werte; die Signalverarbeitung steckt in `woa-dsp` und wird von
//! `woa-engine` anhand der Reihenfolge der Regler gebaut.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::error::{ModelError, Result};

/// Stabile Kennung eines Inserts. Teilt sich den Zahlenraum mit Spuren und Clips.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct InsertId(pub u64);

impl fmt::Display for InsertId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "I{}", self.0)
    }
}

/// Art eines eingebauten Effekts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectKind {
    /// Voller EQ: Hochpass, Tiefen- und Höhen-Kuhschwanz, zwei Glocken.
    Eq,
    /// Kompressor mit weichem Knie.
    Compressor,
    /// Spitzenbegrenzer.
    Limiter,
    /// Gate (Rauschtor).
    Gate,
    /// De-Esser: senkt nur das Zischband ab.
    DeEsser,
    /// Hall.
    Reverb,
    /// Echo, frei oder zum Tempo synchron.
    Delay,
    /// Chorus.
    Chorus,
    /// Sättigung (Röhre, Band, Übersteuerung).
    Saturation,
    /// Rauschunterdrückung (RNNoise, nur 48 kHz).
    Denoise,
}

/// Beschreibung eines Reglers: Name für den Agenten, Anzeige, Bereich, Vorgabe.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParamSpec {
    /// Kurzer Name ohne Leerzeichen (`threshold_db`), so spricht der Agent den Regler an.
    pub name: &'static str,
    /// Beschriftung in der Oberfläche.
    pub label: &'static str,
    /// Einheit, z. B. `dB`; leer ohne Einheit.
    pub unit: &'static str,
    /// Kleinster Wert.
    pub min: f64,
    /// Größter Wert.
    pub max: f64,
    /// Wert eines neuen Effekts.
    pub default: f64,
    /// Bei einem Auswahl-Regler die Namen der Stufen (Wert 0, 1, 2 …); sonst leer.
    pub choices: &'static [&'static str],
}

const fn p(
    name: &'static str,
    label: &'static str,
    unit: &'static str,
    min: f64,
    max: f64,
    default: f64,
) -> ParamSpec {
    ParamSpec {
        name,
        label,
        unit,
        min,
        max,
        default,
        choices: &[],
    }
}

/// Auswahl-Regler: ganze Werte `0 ..= choices.len() - 1`.
const fn pc(
    name: &'static str,
    label: &'static str,
    choices: &'static [&'static str],
    default: f64,
) -> ParamSpec {
    ParamSpec {
        name,
        label,
        unit: "",
        min: 0.0,
        max: (choices.len() - 1) as f64,
        default,
        choices,
    }
}

// Die Reihenfolge ist die Reihenfolge der Regler-Indizes in `woa-dsp::fx`. Neue Regler kommen
// nur hinten an, damit Projektdateien älterer Versionen lesbar bleiben (siehe `Insert::pad`).
const EQ_PARAMS: [ParamSpec; 11] = [
    p("highpass_hz", "Hochpass", "Hz", 0.0, 1000.0, 0.0),
    p("bell_hz", "Glocke Frequenz", "Hz", 40.0, 16000.0, 1000.0),
    p("bell_db", "Glocke Pegel", "dB", -18.0, 18.0, 0.0),
    p("bell_q", "Glocke Güte", "", 0.1, 10.0, 1.0),
    p("low_shelf_hz", "Tiefen Frequenz", "Hz", 20.0, 1000.0, 120.0),
    p("low_shelf_db", "Tiefen Pegel", "dB", -18.0, 18.0, 0.0),
    p(
        "high_shelf_hz",
        "Höhen Frequenz",
        "Hz",
        1000.0,
        16000.0,
        8000.0,
    ),
    p("high_shelf_db", "Höhen Pegel", "dB", -18.0, 18.0, 0.0),
    p("bell2_hz", "Glocke 2 Frequenz", "Hz", 40.0, 16000.0, 4000.0),
    p("bell2_db", "Glocke 2 Pegel", "dB", -18.0, 18.0, 0.0),
    p("bell2_q", "Glocke 2 Güte", "", 0.1, 10.0, 1.0),
];
const COMPRESSOR_PARAMS: [ParamSpec; 5] = [
    p("threshold_db", "Schwelle", "dB", -60.0, 0.0, -18.0),
    p("ratio", "Verhältnis", ": 1", 1.0, 20.0, 2.0),
    p("attack_ms", "Anstieg", "ms", 0.1, 200.0, 10.0),
    p("release_ms", "Rückstellung", "ms", 5.0, 2000.0, 120.0),
    p("makeup_db", "Ausgleich", "dB", 0.0, 24.0, 0.0),
];
const LIMITER_PARAMS: [ParamSpec; 2] = [
    p("ceiling_db", "Obergrenze", "dB", -24.0, 0.0, -1.0),
    p("release_ms", "Rückstellung", "ms", 5.0, 1000.0, 50.0),
];
const GATE_PARAMS: [ParamSpec; 5] = [
    p("threshold_db", "Schwelle", "dB", -80.0, 0.0, -45.0),
    p("range_db", "Absenkung", "dB", 0.0, 80.0, 40.0),
    p("attack_ms", "Anstieg", "ms", 0.1, 50.0, 1.0),
    p("hold_ms", "Halten", "ms", 0.0, 500.0, 100.0),
    p("release_ms", "Rückstellung", "ms", 5.0, 2000.0, 150.0),
];
const DEESSER_PARAMS: [ParamSpec; 4] = [
    p("freq_hz", "Frequenz", "Hz", 2000.0, 12000.0, 7000.0),
    p("threshold_db", "Schwelle", "dB", -60.0, 0.0, -30.0),
    p("ratio", "Verhältnis", ": 1", 1.0, 20.0, 4.0),
    p("max_cut_db", "Höchste Absenkung", "dB", 0.0, 24.0, 12.0),
];
const REVERB_PARAMS: [ParamSpec; 4] = [
    p("decay_s", "Nachhallzeit", "s", 0.1, 10.0, 1.8),
    p("predelay_ms", "Vorverzögerung", "ms", 0.0, 200.0, 15.0),
    p("damping_pct", "Dämpfung", "%", 0.0, 100.0, 40.0),
    p("mix_pct", "Mix", "%", 0.0, 100.0, 25.0),
];
/// Stufen des Delay-Reglers `sync`; Reihenfolge wie `woa_dsp::effects::delay::SYNC_BEATS`.
pub const DELAY_SYNC_CHOICES: [&str; 9] = [
    "Frei (ms)",
    "1/2",
    "1/4",
    "1/4 punktiert",
    "1/8",
    "1/8 punktiert",
    "1/16",
    "1/4 Triole",
    "1/8 Triole",
];
const DELAY_PARAMS: [ParamSpec; 6] = [
    p("time_ms", "Zeit", "ms", 1.0, 2000.0, 350.0),
    pc("sync", "Takt", &DELAY_SYNC_CHOICES, 0.0),
    p("feedback_pct", "Rückkopplung", "%", 0.0, 95.0, 35.0),
    p("tone_hz", "Klang (Tiefpass)", "Hz", 500.0, 20000.0, 6000.0),
    pc("pingpong", "Ping-Pong", &["Aus", "An"], 0.0),
    p("mix_pct", "Mix", "%", 0.0, 100.0, 30.0),
];
const CHORUS_PARAMS: [ParamSpec; 4] = [
    p("rate_hz", "Tempo", "Hz", 0.05, 5.0, 0.8),
    p("depth_ms", "Tiefe", "ms", 0.0, 10.0, 3.0),
    p("delay_ms", "Verzögerung", "ms", 5.0, 30.0, 15.0),
    p("mix_pct", "Mix", "%", 0.0, 100.0, 50.0),
];
const SATURATION_PARAMS: [ParamSpec; 4] = [
    p("drive_db", "Anzerren", "dB", 0.0, 36.0, 12.0),
    pc("shape", "Kennlinie", &["Weich", "Röhre", "Hart"], 0.0),
    p("output_db", "Ausgang", "dB", -24.0, 12.0, 0.0),
    p("mix_pct", "Mix", "%", 0.0, 100.0, 100.0),
];
const DENOISE_PARAMS: [ParamSpec; 1] = [p("amount_pct", "Stärke", "%", 0.0, 100.0, 100.0)];

/// Eine Voreinstellung: Name und die Regler, die sie von den Vorgaben abweichend setzt.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Preset {
    /// Name in der Oberfläche.
    pub name: &'static str,
    /// Reglername und Wert; alle anderen Regler bekommen ihre Vorgabe.
    pub values: &'static [(&'static str, f64)],
}

const fn pre(name: &'static str, values: &'static [(&'static str, f64)]) -> Preset {
    Preset { name, values }
}

const EQ_PRESETS: [Preset; 4] = [
    pre("Flach", &[]),
    pre(
        "Sprache klar",
        &[
            ("highpass_hz", 80.0),
            ("low_shelf_hz", 150.0),
            ("low_shelf_db", -2.0),
            ("bell_hz", 3000.0),
            ("bell_db", 3.0),
            ("high_shelf_hz", 10000.0),
            ("high_shelf_db", 2.0),
        ],
    ),
    pre(
        "Telefon",
        &[
            ("highpass_hz", 300.0),
            ("bell_hz", 1800.0),
            ("bell_db", 4.0),
            ("bell_q", 0.8),
            ("high_shelf_hz", 3400.0),
            ("high_shelf_db", -12.0),
        ],
    ),
    pre(
        "Wärme",
        &[
            ("low_shelf_hz", 200.0),
            ("low_shelf_db", 3.0),
            ("high_shelf_hz", 9000.0),
            ("high_shelf_db", -2.0),
        ],
    ),
];
const COMPRESSOR_PRESETS: [Preset; 3] = [
    pre(
        "Sprache",
        &[
            ("threshold_db", -20.0),
            ("ratio", 3.0),
            ("attack_ms", 8.0),
            ("release_ms", 120.0),
            ("makeup_db", 3.0),
        ],
    ),
    pre(
        "Musik sanft",
        &[
            ("threshold_db", -14.0),
            ("attack_ms", 30.0),
            ("release_ms", 200.0),
            ("makeup_db", 1.0),
        ],
    ),
    pre(
        "Kräftig",
        &[
            ("threshold_db", -26.0),
            ("ratio", 6.0),
            ("attack_ms", 5.0),
            ("release_ms", 100.0),
            ("makeup_db", 6.0),
        ],
    ),
];
const LIMITER_PRESETS: [Preset; 2] = [
    pre("Sendung", &[]),
    pre("Hart", &[("ceiling_db", -3.0), ("release_ms", 20.0)]),
];
const GATE_PRESETS: [Preset; 3] = [
    pre(
        "Sprache",
        &[
            ("threshold_db", -45.0),
            ("range_db", 15.0),
            ("hold_ms", 150.0),
            ("release_ms", 200.0),
        ],
    ),
    pre(
        "Streng",
        &[
            ("threshold_db", -35.0),
            ("attack_ms", 0.5),
            ("hold_ms", 60.0),
            ("release_ms", 80.0),
        ],
    ),
    pre(
        "Sanft",
        &[
            ("threshold_db", -55.0),
            ("range_db", 10.0),
            ("attack_ms", 5.0),
            ("hold_ms", 250.0),
            ("release_ms", 400.0),
        ],
    ),
];
const DEESSER_PRESETS: [Preset; 3] = [
    pre(
        "Sprecher",
        &[
            ("freq_hz", 7000.0),
            ("threshold_db", -28.0),
            ("max_cut_db", 10.0),
        ],
    ),
    pre(
        "Sprecherin",
        &[
            ("freq_hz", 8500.0),
            ("threshold_db", -30.0),
            ("max_cut_db", 10.0),
        ],
    ),
    pre(
        "Stark",
        &[
            ("threshold_db", -36.0),
            ("ratio", 8.0),
            ("max_cut_db", 18.0),
        ],
    ),
];
const REVERB_PRESETS: [Preset; 4] = [
    pre(
        "Raum",
        &[
            ("decay_s", 0.6),
            ("predelay_ms", 5.0),
            ("damping_pct", 50.0),
            ("mix_pct", 20.0),
        ],
    ),
    pre("Halle", &[]),
    pre(
        "Kathedrale",
        &[
            ("decay_s", 5.0),
            ("predelay_ms", 30.0),
            ("damping_pct", 30.0),
            ("mix_pct", 30.0),
        ],
    ),
    pre(
        "Platte",
        &[
            ("decay_s", 1.2),
            ("predelay_ms", 0.0),
            ("damping_pct", 15.0),
        ],
    ),
];
const DELAY_PRESETS: [Preset; 4] = [
    pre(
        "Slapback",
        &[
            ("time_ms", 90.0),
            ("feedback_pct", 15.0),
            ("tone_hz", 5000.0),
            ("mix_pct", 20.0),
        ],
    ),
    pre("Viertel", &[("sync", 2.0)]),
    pre(
        "Ping-Pong",
        &[("sync", 5.0), ("feedback_pct", 40.0), ("pingpong", 1.0)],
    ),
    pre(
        "Dub",
        &[
            ("sync", 4.0),
            ("feedback_pct", 65.0),
            ("tone_hz", 2500.0),
            ("mix_pct", 35.0),
        ],
    ),
];
const CHORUS_PRESETS: [Preset; 3] = [
    pre(
        "Dezent",
        &[
            ("rate_hz", 0.6),
            ("depth_ms", 2.0),
            ("delay_ms", 14.0),
            ("mix_pct", 35.0),
        ],
    ),
    pre(
        "Breit",
        &[("rate_hz", 0.9), ("depth_ms", 4.0), ("delay_ms", 18.0)],
    ),
    pre(
        "Tief",
        &[("rate_hz", 0.3), ("depth_ms", 6.0), ("delay_ms", 20.0)],
    ),
];
const SATURATION_PRESETS: [Preset; 3] = [
    pre("Wärme", &[("drive_db", 6.0), ("mix_pct", 60.0)]),
    pre(
        "Röhre",
        &[("drive_db", 12.0), ("shape", 1.0), ("output_db", -3.0)],
    ),
    pre(
        "Hart",
        &[("drive_db", 18.0), ("shape", 2.0), ("output_db", -6.0)],
    ),
];
const DENOISE_PRESETS: [Preset; 3] = [
    pre("Stark", &[]),
    pre("Mittel", &[("amount_pct", 70.0)]),
    pre("Sanft", &[("amount_pct", 40.0)]),
];

/// Die Vorlage „Sprachkette“ (Paket 17): Hochpass, Gate, EQ, Kompressor, De-Esser.
pub const VOICE_CHAIN: [(EffectKind, &[(&str, f64)]); 5] = [
    (EffectKind::Eq, &[("highpass_hz", 80.0)]),
    (
        EffectKind::Gate,
        &[
            ("threshold_db", -45.0),
            ("range_db", 15.0),
            ("hold_ms", 150.0),
            ("release_ms", 200.0),
        ],
    ),
    (
        EffectKind::Eq,
        &[
            ("low_shelf_hz", 150.0),
            ("low_shelf_db", -2.0),
            ("bell_hz", 3000.0),
            ("bell_db", 3.0),
            ("high_shelf_hz", 10000.0),
            ("high_shelf_db", 2.0),
        ],
    ),
    (
        EffectKind::Compressor,
        &[
            ("threshold_db", -20.0),
            ("ratio", 3.0),
            ("attack_ms", 8.0),
            ("release_ms", 120.0),
            ("makeup_db", 3.0),
        ],
    ),
    (
        EffectKind::DeEsser,
        &[
            ("freq_hz", 7000.0),
            ("threshold_db", -28.0),
            ("max_cut_db", 10.0),
        ],
    ),
];

impl EffectKind {
    /// Alle Arten, in der Reihenfolge des Einfügen-Menüs.
    pub const ALL: [EffectKind; 10] = [
        EffectKind::Eq,
        EffectKind::Compressor,
        EffectKind::Limiter,
        EffectKind::Gate,
        EffectKind::DeEsser,
        EffectKind::Reverb,
        EffectKind::Delay,
        EffectKind::Chorus,
        EffectKind::Saturation,
        EffectKind::Denoise,
    ];

    /// Name für Menüs und Meldungen.
    pub fn label(self) -> &'static str {
        match self {
            EffectKind::Eq => "EQ",
            EffectKind::Compressor => "Kompressor",
            EffectKind::Limiter => "Limiter",
            EffectKind::Gate => "Gate",
            EffectKind::DeEsser => "De-Esser",
            EffectKind::Reverb => "Reverb",
            EffectKind::Delay => "Delay",
            EffectKind::Chorus => "Chorus",
            EffectKind::Saturation => "Sättigung",
            EffectKind::Denoise => "Rauschunterdrückung",
        }
    }

    /// Die Regler in fester Reihenfolge.
    pub fn params(self) -> &'static [ParamSpec] {
        match self {
            EffectKind::Eq => &EQ_PARAMS,
            EffectKind::Compressor => &COMPRESSOR_PARAMS,
            EffectKind::Limiter => &LIMITER_PARAMS,
            EffectKind::Gate => &GATE_PARAMS,
            EffectKind::DeEsser => &DEESSER_PARAMS,
            EffectKind::Reverb => &REVERB_PARAMS,
            EffectKind::Delay => &DELAY_PARAMS,
            EffectKind::Chorus => &CHORUS_PARAMS,
            EffectKind::Saturation => &SATURATION_PARAMS,
            EffectKind::Denoise => &DENOISE_PARAMS,
        }
    }

    /// Die Voreinstellungen des Effekts.
    pub fn presets(self) -> &'static [Preset] {
        match self {
            EffectKind::Eq => &EQ_PRESETS,
            EffectKind::Compressor => &COMPRESSOR_PRESETS,
            EffectKind::Limiter => &LIMITER_PRESETS,
            EffectKind::Gate => &GATE_PRESETS,
            EffectKind::DeEsser => &DEESSER_PRESETS,
            EffectKind::Reverb => &REVERB_PRESETS,
            EffectKind::Delay => &DELAY_PRESETS,
            EffectKind::Chorus => &CHORUS_PRESETS,
            EffectKind::Saturation => &SATURATION_PRESETS,
            EffectKind::Denoise => &DENOISE_PRESETS,
        }
    }

    /// Die Regler-Indizes und Werte einer Voreinstellung (Vorgaben, dann die Abweichungen);
    /// `None` bei unbekanntem Namen.
    pub fn preset_values(self, preset: &str) -> Option<Vec<f64>> {
        let preset = self.presets().iter().find(|pr| pr.name == preset)?;
        let mut values = self.defaults();
        for (name, v) in preset.values {
            values[self.param_index(name)?] = *v;
        }
        Some(values)
    }

    /// Index des Reglers mit diesem Namen.
    pub fn param_index(self, name: &str) -> Option<usize> {
        self.params().iter().position(|s| s.name == name)
    }

    /// Vorgabewerte aller Regler.
    pub fn defaults(self) -> Vec<f64> {
        self.params().iter().map(|s| s.default).collect()
    }

    /// Prüft einen Wert gegen den Bereich des Reglers `index`.
    pub fn check_value(self, index: usize, value: f64) -> Result<()> {
        let spec = self
            .params()
            .get(index)
            .ok_or(ModelError::UnknownParam { index })?;
        let whole = spec.choices.is_empty() || value.fract() == 0.0;
        if value.is_finite() && whole && (spec.min..=spec.max).contains(&value) {
            Ok(())
        } else {
            Err(ModelError::InvalidValue {
                field: spec.name,
                value,
            })
        }
    }
}

/// Ein Effekt in der Insert-Kette einer Spur.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Insert {
    /// Stabile Kennung.
    pub id: InsertId,
    /// Art des Effekts.
    pub kind: EffectKind,
    /// Umgangen: das Signal läuft unverändert durch.
    #[serde(default, skip_serializing_if = "is_false")]
    pub bypass: bool,
    /// Werte der Regler, Reihenfolge wie [`EffectKind::params`].
    pub params: Vec<f64>,
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_false(v: &bool) -> bool {
    !*v
}

impl Insert {
    /// Ein neuer Effekt mit Vorgabewerten.
    pub fn new(id: InsertId, kind: EffectKind) -> Self {
        Self {
            id,
            kind,
            bypass: false,
            params: kind.defaults(),
        }
    }

    /// Ein neuer Effekt mit Vorgabewerten, davon die Regler `values` (nach Namen) gesetzt.
    /// Unbekannter Name oder Wert außerhalb des Bereichs ist ein Fehler.
    pub fn with_values(id: InsertId, kind: EffectKind, values: &[(&str, f64)]) -> Result<Self> {
        let mut insert = Self::new(id, kind);
        for (name, value) in values {
            let index = kind
                .param_index(name)
                .ok_or_else(|| ModelError::UnknownParamName((*name).to_string()))?;
            insert.params[index] = *value;
        }
        insert.check()?;
        Ok(insert)
    }

    /// Ein neuer Effekt mit den Werten der Voreinstellung `preset`.
    pub fn with_preset(id: InsertId, kind: EffectKind, preset: &str) -> Result<Self> {
        let params = kind
            .preset_values(preset)
            .ok_or_else(|| ModelError::UnknownParamName(format!("preset {preset}")))?;
        let mut insert = Self::new(id, kind);
        insert.params = params;
        insert.check()?;
        Ok(insert)
    }

    /// Wert des Reglers `name`.
    pub fn param(&self, name: &str) -> Option<f64> {
        self.params.get(self.kind.param_index(name)?).copied()
    }

    /// Ergänzt fehlende Regler mit Vorgabewerten (Datei einer älteren Version).
    pub(crate) fn pad(&mut self) {
        let specs = self.kind.params();
        while self.params.len() < specs.len() {
            self.params.push(specs[self.params.len()].default);
        }
    }

    /// Anzahl und Bereich aller Regler.
    pub(crate) fn check(&self) -> Result<()> {
        let n = self.kind.params().len();
        if self.params.len() != n {
            return Err(ModelError::UnknownParam {
                index: self.params.len(),
            });
        }
        for (i, v) in self.params.iter().enumerate() {
            self.kind.check_value(i, *v)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_inside_their_ranges() {
        for kind in EffectKind::ALL {
            let ins = Insert::new(InsertId(1), kind);
            assert!(ins.check().is_ok(), "{kind:?}");
            assert_eq!(ins.params.len(), kind.params().len());
        }
    }

    #[test]
    fn names_are_unique_and_found() {
        for kind in EffectKind::ALL {
            for (i, s) in kind.params().iter().enumerate() {
                assert_eq!(kind.param_index(s.name), Some(i));
            }
        }
        assert_eq!(EffectKind::Compressor.param_index("nope"), None);
    }

    #[test]
    fn out_of_range_and_nan_are_rejected() {
        let k = EffectKind::Compressor;
        assert!(k.check_value(0, -61.0).is_err());
        assert!(k.check_value(0, f64::NAN).is_err());
        assert!(k.check_value(9, 0.0).is_err());
        assert!(k.check_value(1, 4.0).is_ok());
    }

    #[test]
    fn short_parameter_list_is_padded() {
        let mut ins = Insert::new(InsertId(1), EffectKind::Eq);
        ins.params.truncate(1);
        ins.pad();
        assert_eq!(ins.params, EffectKind::Eq.defaults());
    }

    #[test]
    fn every_preset_names_real_params_inside_their_ranges() {
        for kind in EffectKind::ALL {
            assert!(!kind.presets().is_empty(), "{kind:?} has no presets");
            let mut names = std::collections::HashSet::new();
            for pr in kind.presets() {
                assert!(
                    names.insert(pr.name),
                    "{kind:?}: duplicate preset {}",
                    pr.name
                );
                let values = kind
                    .preset_values(pr.name)
                    .unwrap_or_else(|| panic!("{kind:?} {}: unknown parameter", pr.name));
                for (i, v) in values.iter().enumerate() {
                    kind.check_value(i, *v)
                        .unwrap_or_else(|e| panic!("{kind:?} {}: {e}", pr.name));
                }
            }
        }
        assert_eq!(EffectKind::Eq.preset_values("gibt es nicht"), None);
    }

    #[test]
    fn voice_chain_is_valid_and_in_the_promised_order() {
        let kinds: Vec<_> = VOICE_CHAIN.iter().map(|(k, _)| *k).collect();
        assert_eq!(
            kinds,
            [
                EffectKind::Eq,
                EffectKind::Gate,
                EffectKind::Eq,
                EffectKind::Compressor,
                EffectKind::DeEsser
            ]
        );
        for (i, (kind, values)) in VOICE_CHAIN.iter().enumerate() {
            Insert::with_values(InsertId(i as u64), *kind, values).expect("valid values");
        }
        assert!(Insert::with_values(InsertId(1), EffectKind::Gate, &[("nope", 1.0)]).is_err());
        assert!(
            Insert::with_values(InsertId(1), EffectKind::Gate, &[("threshold_db", 5.0)]).is_err()
        );
    }

    #[test]
    fn choice_params_take_whole_numbers_only() {
        let k = EffectKind::Delay;
        let sync = k.param_index("sync").expect("sync");
        assert!(k.check_value(sync, 4.0).is_ok());
        assert!(k.check_value(sync, 4.5).is_err());
        assert!(k.check_value(sync, 9.0).is_err());
        assert_eq!(k.params()[sync].choices.len(), DELAY_SYNC_CHOICES.len());
    }

    #[test]
    fn effect_kinds_keep_stable_file_names() {
        let names: Vec<String> = EffectKind::ALL
            .iter()
            .map(|k| serde_json::to_string(k).expect("json"))
            .collect();
        assert_eq!(
            names,
            [
                "\"eq\"",
                "\"compressor\"",
                "\"limiter\"",
                "\"gate\"",
                "\"de_esser\"",
                "\"reverb\"",
                "\"delay\"",
                "\"chorus\"",
                "\"saturation\"",
                "\"denoise\""
            ]
        );
    }
}
