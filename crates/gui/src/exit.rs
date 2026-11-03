//! Eine Stelle für „WOA wird beendet“ (#365, #362): Beenden im Menü, Fenster schließen und
//! Update installieren fragen bei ungespeichertem Projekt dasselbe — Speichern, Nicht
//! speichern oder Abbrechen. Abbrechen heißt: WOA bleibt offen, das Update kommt später.

/// Warum WOA beendet werden soll.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExitReason {
    /// Datei › Beenden, Strg+Q oder Fenster schließen.
    Quit,
    /// „Jetzt installieren“: laden, prüfen, Installer starten, WOA beenden.
    Update,
}

/// Antwort in der Frage „Projekt speichern?“.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    Save,
    Discard,
    Cancel,
}

/// Zustand der Frage. Ohne Fenster testbar.
#[derive(Debug, Default)]
pub struct ExitGuard {
    asking: Option<ExitReason>,
    /// Der Nutzer hat entschieden (gespeichert oder verworfen): nicht noch einmal fragen.
    confirmed: Option<ExitReason>,
}

impl ExitGuard {
    /// WOA soll beendet werden. `true`: sofort weiter. `false`: erst die Frage zeigen.
    pub fn request(&mut self, reason: ExitReason, dirty: bool) -> bool {
        if self.confirmed.is_some() || !dirty {
            return true;
        }
        self.asking = Some(reason);
        false
    }

    /// Die offene Frage, falls eine da ist.
    pub fn asking(&self) -> Option<ExitReason> {
        self.asking
    }

    /// Antwort verarbeiten. `clean` sagt, ob das Projekt danach gespeichert ist (nach einem
    /// Speichern-Versuch). `Some(grund)`: jetzt beenden bzw. installieren.
    pub fn answer(&mut self, choice: Choice, clean: bool) -> Option<ExitReason> {
        let reason = self.asking?;
        match choice {
            Choice::Cancel => {
                self.asking = None;
                None
            }
            // Speichern-Dialog abgebrochen: die Frage bleibt, man kann noch verwerfen.
            Choice::Save if !clean => None,
            Choice::Save | Choice::Discard => {
                self.asking = None;
                self.confirmed = Some(reason);
                Some(reason)
            }
        }
    }

    /// Entschieden, das Beenden läuft (z. B. das Update lädt noch).
    pub fn confirmed(&self) -> bool {
        self.confirmed.is_some()
    }

    /// Wofür entschieden wurde.
    pub fn confirmed_for(&self) -> Option<ExitReason> {
        self.confirmed
    }

    /// Ohne Frage freigeben (Fotomodus, Installer läuft schon).
    pub fn allow(&mut self, reason: ExitReason) {
        self.asking = None;
        self.confirmed = Some(reason);
    }

    /// Das Beenden kam nicht zustande (Download oder Installer fehlgeschlagen): beim
    /// nächsten Mal wieder fragen.
    pub fn reset(&mut self) {
        self.confirmed = None;
        self.asking = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_project_exits_without_asking() {
        let mut g = ExitGuard::default();
        assert!(g.request(ExitReason::Quit, false));
        assert!(g.asking().is_none());
    }

    #[test]
    fn cancel_keeps_woa_open_and_asks_again_next_time() {
        let mut g = ExitGuard::default();
        assert!(!g.request(ExitReason::Update, true));
        assert_eq!(g.asking(), Some(ExitReason::Update));
        assert_eq!(g.answer(Choice::Cancel, false), None);
        assert!(g.asking().is_none());
        assert!(!g.confirmed());
        assert!(!g.request(ExitReason::Update, true), "fragt erneut");
    }

    #[test]
    fn discard_exits_without_saving() {
        let mut g = ExitGuard::default();
        g.request(ExitReason::Update, true);
        assert_eq!(g.answer(Choice::Discard, false), Some(ExitReason::Update));
        assert!(g.confirmed());
        assert!(
            g.request(ExitReason::Update, true),
            "nicht noch einmal fragen"
        );
    }

    #[test]
    fn save_cancelled_in_the_file_dialog_keeps_the_question_open() {
        let mut g = ExitGuard::default();
        g.request(ExitReason::Quit, true);
        assert_eq!(g.answer(Choice::Save, false), None);
        assert_eq!(g.asking(), Some(ExitReason::Quit));
        assert_eq!(g.answer(Choice::Save, true), Some(ExitReason::Quit));
    }

    #[test]
    fn reset_after_failed_update_asks_again() {
        let mut g = ExitGuard::default();
        g.request(ExitReason::Update, true);
        g.answer(Choice::Discard, false);
        g.reset();
        assert!(!g.request(ExitReason::Quit, true));
    }
}
