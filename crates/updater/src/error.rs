//! Fehler des Updaters.

/// Grund, aus dem eine Version nicht installiert werden darf.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Veto {
    #[error("Version nicht lesbar")]
    UnreadableVersion,
    #[error("gleiche Version, nichts zu tun")]
    SameVersion,
    #[error("ältere Version (Downgrade abgelehnt)")]
    Downgrade,
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum UpdateError {
    /// Kein öffentlicher Schlüssel eingebaut: der Updater ist aus.
    #[error("Updates nicht eingerichtet")]
    NotConfigured,
    #[error("{0}")]
    Veto(#[from] Veto),
    #[error("Netzwerk: {0}")]
    Net(String),
    #[error("Manifest ungültig: {0}")]
    Manifest(String),
    #[error("Signatur ungültig: {0}")]
    Signature(String),
    #[error("Dateifehler: {0}")]
    Io(String),
}
