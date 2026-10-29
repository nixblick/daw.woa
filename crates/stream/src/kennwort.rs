//! Das Quell-Kennwort: aus einer Datei gelesen, nie geloggt, nie in `Debug`.

use std::path::Path;

/// Kennwort im Speicher. `Debug` zeigt nur `***`.
#[derive(Clone, PartialEq, Eq)]
pub struct Kennwort(String);

impl Kennwort {
    /// Liest die erste Zeile von `path` (ohne Zeilenende). Die Fehlermeldung nennt nur
    /// den Pfad, nie den Inhalt.
    pub fn from_file(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("Kennwortdatei {} nicht lesbar: {e}", path.display()))?;
        let line = text.lines().next().unwrap_or("").trim_end_matches('\r');
        if line.is_empty() {
            return Err(format!("Kennwortdatei {} ist leer", path.display()));
        }
        Ok(Self(line.to_string()))
    }

    /// Aus einem Wert (Tests).
    pub fn from_value(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Der Wert, nur für das Protokoll.
    pub(crate) fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for Kennwort {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Kennwort(***)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_first_line_and_hides_it() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("k");
        std::fs::write(&p, "abc123x\r\nzweite\n").unwrap();
        let k = Kennwort::from_file(&p).unwrap();
        assert_eq!(k.expose(), "abc123x");
        assert!(!format!("{k:?}").contains("abc123x"));
        std::fs::write(&p, "\n").unwrap();
        assert!(Kennwort::from_file(&p).is_err());
        let e = Kennwort::from_file(&tmp.path().join("fehlt")).unwrap_err();
        assert!(e.contains("fehlt"));
    }
}
