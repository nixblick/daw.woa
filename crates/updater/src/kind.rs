//! Paketart: wie diese Installation auf den Rechner kam und ob sie sich selbst ersetzen darf.
//!
//! Idee wie bei Audacity (Prüfung und Hinweis überall, Austausch nur wo es sicher geht); die
//! Erkennung ist unsere eigene.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Windows, installiert per MSI: Installer starten.
    Msi,
    /// Linux-AppImage: die Datei selbst ersetzen.
    AppImage,
    /// Flatpak: nur melden, Aktualisieren macht `flatpak update`.
    Flatpak,
    /// Alles andere (Quellbau, ZIP, Paketverwaltung): nur melden.
    Other,
}

impl Kind {
    /// Erkennt die Art aus dem Betriebssystem und der Umgebung.
    pub fn detect() -> Self {
        let flatpak =
            std::env::var_os("FLATPAK_ID").is_some() || Path::new("/.flatpak-info").exists();
        Self::from_env(cfg!(windows), Self::appimage_path().as_deref(), flatpak)
    }

    /// Reine Regel hinter [`Kind::detect`]. Das AppImage setzt `APPIMAGE` auf seine Datei;
    /// zeigt die Variable ins Leere, gilt es nicht als AppImage.
    pub fn from_env(windows: bool, appimage: Option<&Path>, flatpak: bool) -> Self {
        if windows {
            Kind::Msi
        } else if flatpak {
            Kind::Flatpak
        } else if appimage.is_some_and(Path::is_file) {
            Kind::AppImage
        } else {
            Kind::Other
        }
    }

    /// Darf WOA sich in dieser Art selbst ersetzen?
    pub fn can_install(self) -> bool {
        matches!(self, Kind::Msi | Kind::AppImage)
    }

    /// Kurzname für „Über WOA“.
    pub fn label(self) -> &'static str {
        match self {
            Kind::Msi => "Windows-Installer (MSI)",
            Kind::AppImage => "AppImage",
            Kind::Flatpak => "Flatpak",
            Kind::Other => "ohne Paket (Quellbau oder Archiv)",
        }
    }

    /// Ort der laufenden AppImage-Datei laut Umgebung.
    pub fn appimage_path() -> Option<PathBuf> {
        std::env::var_os("APPIMAGE").map(PathBuf::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_follows_the_environment() {
        let dir = tempfile::tempdir().unwrap();
        let img = dir.path().join("WOA.AppImage");
        std::fs::write(&img, b"x").unwrap();
        assert_eq!(Kind::from_env(true, None, false), Kind::Msi);
        assert_eq!(Kind::from_env(false, Some(&img), false), Kind::AppImage);
        // Flatpak gewinnt, auch wenn jemand APPIMAGE gesetzt hat.
        assert_eq!(Kind::from_env(false, Some(&img), true), Kind::Flatpak);
        // Variable zeigt ins Leere: kein AppImage, nichts ersetzen.
        assert_eq!(
            Kind::from_env(false, Some(&dir.path().join("weg")), false),
            Kind::Other
        );
        assert_eq!(Kind::from_env(false, None, false), Kind::Other);
        assert!(Kind::Msi.can_install() && Kind::AppImage.can_install());
        assert!(!Kind::Flatpak.can_install() && !Kind::Other.can_install());
    }
}
