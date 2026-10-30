//! Effekte in der Insert-Kette einer Spur (Paket 16): welche Arten es gibt und welche
//! Regler sie haben. Das Modell kennt nur Namen, Bereiche und Werte; die Signalverarbeitung
//! steckt in `woa-dsp` und wird von `woa-engine` anhand der Reihenfolge der Regler gebaut.

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
    /// Hochpass und eine Glocke (Paket 17 baut ihn zum vollen EQ aus).
    Eq,
    /// Kompressor mit weichem Knie.
    Compressor,
    /// Spitzenbegrenzer.
    Limiter,
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
    }
}

// Die Reihenfolge ist die Reihenfolge der Regler-Indizes in `woa-dsp::fx`.
const EQ_PARAMS: [ParamSpec; 4] = [
    p("highpass_hz", "Hochpass", "Hz", 0.0, 1000.0, 0.0),
    p("bell_hz", "Glocke Frequenz", "Hz", 40.0, 16000.0, 1000.0),
    p("bell_db", "Glocke Pegel", "dB", -18.0, 18.0, 0.0),
    p("bell_q", "Glocke Güte", "", 0.1, 10.0, 1.0),
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

impl EffectKind {
    /// Alle Arten, in der Reihenfolge des Einfügen-Menüs.
    pub const ALL: [EffectKind; 3] = [EffectKind::Eq, EffectKind::Compressor, EffectKind::Limiter];

    /// Name für Menüs und Meldungen.
    pub fn label(self) -> &'static str {
        match self {
            EffectKind::Eq => "EQ",
            EffectKind::Compressor => "Kompressor",
            EffectKind::Limiter => "Limiter",
        }
    }

    /// Die Regler in fester Reihenfolge.
    pub fn params(self) -> &'static [ParamSpec] {
        match self {
            EffectKind::Eq => &EQ_PARAMS,
            EffectKind::Compressor => &COMPRESSOR_PARAMS,
            EffectKind::Limiter => &LIMITER_PARAMS,
        }
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
        if value.is_finite() && (spec.min..=spec.max).contains(&value) {
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
}
