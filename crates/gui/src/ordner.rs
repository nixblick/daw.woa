//! WOA-Ordner (#258, André 01.11.2026): eine feste Ablage im Musikordner des Benutzers,
//! `<Musik>/WOA/` mit einem Unterordner je Art. Beim Start legt die App fehlende Ordner still
//! an; jeder Speichern- und Öffnen-Dialog startet im passenden Ordner, Mitschnitte landen ohne
//! Dialog im Mitschnitt-Ordner. Bestehende Dateien werden nie verschoben oder gelöscht.
//!
//! In `config.json` steht nur, was vom Vorgabewert abweicht: ein eigener Basisordner und/oder
//! ein eigener Ordner je Art. Leer heißt `<Basis>/<Unterordner>`.

pub use woa_radio::ordner::{Art, Ordner, WOA_DIR, music_dir, music_dir_from};

use std::path::{Path, PathBuf};
use std::sync::RwLock;

/// Was gerade gilt (Einstellungen plus Musikordner), für die Dialoge in allen Fenstern.
struct Current {
    ordner: Ordner,
    music: PathBuf,
}

static CURRENT: RwLock<Option<Current>> = RwLock::new(None);

/// Übernimmt die Einstellungen (Start und nach jeder Änderung) und legt fehlende Ordner an.
pub fn apply(ordner: &Ordner) {
    let music = music_dir();
    ordner.ensure_in(&music);
    match CURRENT.write() {
        Ok(mut c) => {
            *c = Some(Current {
                ordner: ordner.clone(),
                music,
            })
        }
        Err(e) => tracing::warn!(error = %e, "folder settings lock"),
    }
}

/// Ordner einer Art nach den aktuellen Einstellungen.
pub fn dir(art: Art) -> PathBuf {
    match CURRENT.read().ok().as_ref().and_then(|c| c.as_ref()) {
        Some(c) => c.ordner.dir_in(art, &c.music),
        None => Ordner::default().dir_in(art, &music_dir()),
    }
}

/// Musikordner (Start für Dialoge, die Titel oder Musikordner öffnen).
pub fn musik() -> PathBuf {
    match CURRENT.read().ok().as_ref().and_then(|c| c.as_ref()) {
        Some(c) => c.music.clone(),
        None => music_dir(),
    }
}

/// Dateidialog, der in `dir` startet, sofern es den Ordner gibt.
pub fn dialog_in(dir: &Path) -> rfd::FileDialog {
    let d = rfd::FileDialog::new();
    if dir.is_dir() {
        d.set_directory(dir)
    } else {
        d
    }
}

/// Dateidialog im Ordner einer Art.
pub fn dialog(art: Art) -> rfd::FileDialog {
    dialog_in(&dir(art))
}

/// Dateidialog im Musikordner.
pub fn dialog_musik() -> rfd::FileDialog {
    dialog_in(&musik())
}

/// Fenster „Speicherorte …“ in den Einstellungen: eine Zeile je Art mit „Ändern …“.
/// Jede Änderung gilt sofort; „Vorgabe“ erscheint nur bei einem eigenen Ordner.
#[derive(Default)]
pub struct OrdnerUi {
    pub open: bool,
}

impl OrdnerUi {
    /// Zeigt das Fenster. `Some` = Einstellungen geändert (speichern und anwenden).
    pub fn show(&mut self, ctx: &eframe::egui::Context, saved: &Ordner) -> Option<Ordner> {
        use eframe::egui;
        if !self.open {
            return None;
        }
        let music = musik();
        let mut draft = saved.clone();
        let mut changed = false;
        let mut open = true;
        egui::Window::new("Speicherorte")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label(
                    egui::RichText::new(
                        "Hier legt WOA ab und hier starten die Dialoge. Fehlende Ordner legt WOA \
                         an; vorhandene Dateien bleiben, wo sie sind.",
                    )
                    .small()
                    .color(crate::theme::text_dim()),
                );
                ui.add_space(6.0);
                egui::Grid::new("ordner_grid")
                    .num_columns(3)
                    .spacing([10.0, 6.0])
                    .show(ui, |ui| {
                        let base = draft.base_in(&music);
                        if row(ui, "Basisordner", &base, draft.basis.is_some()) {
                            changed |= pick(&base).map(|p| draft.basis = Some(p)).is_some();
                        }
                        if reset_clicked(ui, draft.basis.is_some()) {
                            draft.basis = None;
                            changed = true;
                        }
                        ui.end_row();
                        for art in Art::ALL {
                            let dir = draft.dir_in(art, &music);
                            let own = draft.eigene.contains_key(&art);
                            if row(ui, art.folder(), &dir, own) {
                                changed |=
                                    pick(&dir).map(|p| draft.eigene.insert(art, p)).is_some();
                            }
                            if reset_clicked(ui, own) {
                                draft.eigene.remove(&art);
                                changed = true;
                            }
                            ui.end_row();
                        }
                    });
            });
        self.open = open;
        changed.then_some(draft)
    }
}

/// Name, Pfad und „Ändern …“. `true` = geklickt.
fn row(ui: &mut eframe::egui::Ui, name: &str, dir: &Path, own: bool) -> bool {
    use eframe::egui;
    ui.label(name);
    let text = egui::RichText::new(dir.display().to_string()).small();
    let text = if own {
        text
    } else {
        text.color(crate::theme::text_dim())
    };
    ui.horizontal(|ui| {
        ui.label(text);
        ui.button("Ändern …").clicked()
    })
    .inner
}

fn reset_clicked(ui: &mut eframe::egui::Ui, own: bool) -> bool {
    if own {
        ui.small_button("Vorgabe")
            .on_hover_text("Wieder den vorgegebenen Ordner nehmen")
            .clicked()
    } else {
        ui.label("");
        false
    }
}

fn pick(start: &Path) -> Option<PathBuf> {
    dialog_in(start).set_title("Ordner wählen").pick_folder()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_music_woa_subfolder() {
        let music = Path::new("/m/Musik");
        let o = Ordner::default();
        assert_eq!(o.dir_in(Art::Projekte, music), music.join("WOA/Projekte"));
        assert_eq!(o.dir_in(Art::Midi, music), music.join("WOA/MIDI"));
        assert_eq!(
            o.dir_in(Art::Mitschnitte, music),
            music.join("WOA/Mitschnitte")
        );
    }

    #[test]
    fn own_base_and_own_folder_per_kind() {
        let music = Path::new("/m/Musik");
        let mut o = Ordner {
            basis: Some(PathBuf::from("/b")),
            ..Ordner::default()
        };
        o.eigene.insert(Art::Mitschnitte, PathBuf::from("/rec"));
        o.eigene.insert(Art::Jingles, PathBuf::new());
        assert_eq!(o.dir_in(Art::Mitschnitte, music), PathBuf::from("/rec"));
        assert_eq!(o.dir_in(Art::Exporte, music), PathBuf::from("/b/Exporte"));
        // Leer = Basisordner/Unterordner.
        assert_eq!(o.dir_in(Art::Jingles, music), PathBuf::from("/b/Jingles"));
    }

    #[test]
    fn serde_roundtrip_and_missing_section() {
        let o: Ordner = serde_json::from_str("{}").expect("empty");
        assert!(o.is_default());
        let mut o = Ordner::default();
        o.eigene.insert(Art::Midi, PathBuf::from("/x"));
        let s = serde_json::to_string(&o).expect("ser");
        assert_eq!(s, r#"{"eigene":{"midi":"/x"}}"#);
        assert_eq!(serde_json::from_str::<Ordner>(&s).expect("de"), o);
    }

    #[test]
    fn music_dir_from_xdg_and_fallbacks() {
        let home = Path::new("/h");
        let ud =
            "# kommentar\nXDG_DESKTOP_DIR=\"$HOME/Schreibtisch\"\nXDG_MUSIC_DIR=\"$HOME/Töne\"\n";
        assert_eq!(music_dir_from(home, Some(ud), false), home.join("Töne"));
        let abs = "XDG_MUSIC_DIR=\"/daten/musik\"\n";
        assert_eq!(
            music_dir_from(home, Some(abs), false),
            PathBuf::from("/daten/musik")
        );
        let off = "XDG_MUSIC_DIR=\"$HOME/\"\n";
        assert_eq!(music_dir_from(home, Some(off), false), home.join("Musik"));
        assert_eq!(music_dir_from(home, None, false), home.join("Musik"));
        assert_eq!(music_dir_from(home, None, true), home.join("Music"));
    }

    #[test]
    fn ensure_is_idempotent_and_keeps_files() {
        let tmp = tempfile::tempdir().expect("tmp");
        let music = tmp.path();
        let o = Ordner::default();
        // Eine vorhandene Datei im Mitschnitt-Ordner bleibt unberührt.
        let rec = o.dir_in(Art::Mitschnitte, music);
        std::fs::create_dir_all(&rec).expect("rec");
        std::fs::write(rec.join("alt.wav"), b"alt").expect("write");
        let made = o.ensure_in(music);
        assert_eq!(made.len(), Art::ALL.len() - 1);
        for art in Art::ALL {
            assert!(o.dir_in(art, music).is_dir(), "{art:?}");
        }
        assert!(o.ensure_in(music).is_empty());
        assert_eq!(std::fs::read(rec.join("alt.wav")).expect("read"), b"alt");
    }

    #[test]
    fn ensure_does_not_replace_a_file_of_the_same_name() {
        let tmp = tempfile::tempdir().expect("tmp");
        let o = Ordner::default();
        let base = o.base_in(tmp.path());
        std::fs::create_dir_all(&base).expect("base");
        std::fs::write(base.join("MIDI"), b"datei").expect("write");
        o.ensure_in(tmp.path());
        assert_eq!(std::fs::read(base.join("MIDI")).expect("read"), b"datei");
        assert!(base.join("Projekte").is_dir());
    }
}
