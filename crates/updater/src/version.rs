//! Versionsvergleich und Downgrade-Wächter.
//!
//! Nur streng `X.Y.Z` (ganze Zahlen) ist lesbar. Alles andere (Vorabversion, `v`-Präfix,
//! Leerstring) ist ein Veto: im Zweifel nie installieren.

use crate::error::Veto;

/// Liest `X.Y.Z`; alles andere ist `None`.
pub fn parse(s: &str) -> Option<(u64, u64, u64)> {
    let mut it = s.split('.');
    let mut next = || {
        let part = it.next()?;
        if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        part.parse::<u64>().ok()
    };
    let v = (next()?, next()?, next()?);
    it.next().is_none().then_some(v)
}

/// Erlaubt nur eine strikt neuere Version. Gleich, älter oder unlesbar ist ein Veto.
///
/// Das ergänzt die Signaturprüfung: sie sagt „diese Bytes sind von uns“, der Wächter sagt
/// „und sie sind neuer“ (ein altes, echt signiertes Release darf nicht zurückgespielt werden).
pub fn ensure_newer(candidate: &str, current: &str) -> Result<(), Veto> {
    let (Some(c), Some(i)) = (parse(candidate), parse(current)) else {
        return Err(Veto::UnreadableVersion);
    };
    match c.cmp(&i) {
        std::cmp::Ordering::Greater => Ok(()),
        std::cmp::Ordering::Equal => Err(Veto::SameVersion),
        std::cmp::Ordering::Less => Err(Veto::Downgrade),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_strictly_newer_passes() {
        assert_eq!(ensure_newer("0.34.0", "0.33.4"), Ok(()));
        assert_eq!(ensure_newer("1.0.0", "0.99.99"), Ok(()));
        assert_eq!(ensure_newer("0.33.4", "0.33.4"), Err(Veto::SameVersion));
        assert_eq!(ensure_newer("0.33.3", "0.33.4"), Err(Veto::Downgrade));
        assert_eq!(ensure_newer("0.9.0", "0.10.0"), Err(Veto::Downgrade));
    }

    #[test]
    fn order_is_numeric_not_textual() {
        assert_eq!(ensure_newer("0.10.0", "0.9.0"), Ok(()));
        assert_eq!(ensure_newer("0.33.10", "0.33.9"), Ok(()));
    }

    #[test]
    fn unreadable_versions_are_vetoed() {
        for bad in [
            "",
            "1",
            "1.2",
            "1.2.3.4",
            "v1.2.3",
            "1.2.3-beta",
            "1.2.x",
            " 1.2.3",
            "-1.0.0",
        ] {
            assert_eq!(
                ensure_newer(bad, "0.1.0"),
                Err(Veto::UnreadableVersion),
                "{bad}"
            );
            assert_eq!(
                ensure_newer("9.9.9", bad),
                Err(Veto::UnreadableVersion),
                "{bad}"
            );
        }
    }
}
