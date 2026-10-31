//! Titelinfos aus dem Datei-Kopf (Tags und Länge), ohne die Audiodaten zu dekodieren, und
//! Schreiben der Titelinfos (Titel, Interpret, Album, Jahr, Genre, Tracknummer, Kommentar,
//! Cover) für MP3 (ID3v2), FLAC, OGG/Opus, M4A und WAV.
//!
//! Der Ordner-Import liest nur das hier; dekodiert wird erst in der Hintergrundanalyse. Ein
//! 5-Minuten-MP3 braucht so Millisekunden statt einer halben Sekunde (Telemetrie 0.34.34).

use std::path::Path;

use lofty::config::{ParseOptions, WriteOptions};
use lofty::file::FileType;
use lofty::picture::{Picture, PictureType};
use lofty::prelude::*;
use lofty::probe::Probe;
use lofty::tag::{Tag, items::Timestamp};

/// Was der Kopf einer Datei verrät. Leere Felder fehlen in der Datei.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Header {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    /// Sekunden laut Kopf; 0, wenn die Datei keine Länge verrät (die Analyse trägt sie nach).
    pub duration: f64,
}

fn clean(s: Option<std::borrow::Cow<'_, str>>) -> Option<String> {
    s.map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

/// Liest Tags und Länge aus dem Kopf. Bilder werden übersprungen (schneller, weniger Speicher).
pub fn read_header(path: &Path) -> Result<Header, String> {
    let tagged = Probe::open(path)
        .map_err(|e| e.to_string())?
        .options(ParseOptions::new().read_cover_art(false))
        .read()
        .map_err(|e| e.to_string())?;
    let duration = tagged.properties().duration().as_secs_f64();
    let mut h = Header {
        duration,
        ..Header::default()
    };
    if let Some(tag) = tagged.primary_tag().or_else(|| tagged.first_tag()) {
        h.title = clean(tag.title());
        h.artist = clean(tag.artist());
        h.album = clean(tag.album());
    }
    Ok(h)
}

/// Bearbeitbare Titelinfos einer Datei. Leere Felder fehlen in der Datei.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Fields {
    pub title: String,
    pub artist: String,
    pub album: String,
    /// Jahr als Text (vier Ziffern oder leer).
    pub year: String,
    pub genre: String,
    /// Tracknummer als Text (Ziffern oder leer).
    pub track: String,
    pub comment: String,
}

/// Namen der Felder in der Reihenfolge der Oberfläche.
pub const FIELD_NAMES: [&str; 7] = [
    "Titel",
    "Interpret",
    "Album",
    "Jahr",
    "Genre",
    "Tracknummer",
    "Kommentar",
];

impl Fields {
    pub fn get(&self, i: usize) -> &str {
        match i {
            0 => &self.title,
            1 => &self.artist,
            2 => &self.album,
            3 => &self.year,
            4 => &self.genre,
            5 => &self.track,
            _ => &self.comment,
        }
    }

    pub fn get_mut(&mut self, i: usize) -> &mut String {
        match i {
            0 => &mut self.title,
            1 => &mut self.artist,
            2 => &mut self.album,
            3 => &mut self.year,
            4 => &mut self.genre,
            5 => &mut self.track,
            _ => &mut self.comment,
        }
    }

    /// Prüft Jahr und Tracknummer; Text ohne Pfad.
    pub fn check(&self) -> Result<(), String> {
        let y = self.year.trim();
        if !(y.is_empty() || y.len() == 4 && y.parse::<u16>().is_ok()) {
            return Err("Jahr: vier Ziffern, z. B. 1998".into());
        }
        let t = self.track.trim();
        if !t.is_empty() && t.parse::<u32>().map_or(true, |n| n == 0) {
            return Err("Tracknummer: eine Zahl ab 1".into());
        }
        Ok(())
    }
}

/// Ein Cover-Bild aus der Datei.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cover {
    pub data: Vec<u8>,
    /// z. B. `image/jpeg`.
    pub mime: String,
}

/// Was beim Speichern mit dem Cover geschieht.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum CoverEdit {
    #[default]
    Keep,
    /// Ersetzt alle Bilder durch dieses (als Vorderseite).
    Set(Vec<u8>),
    Remove,
}

/// Eine Änderung: `None` lässt ein Feld, wie es ist (Mehrfachauswahl mit verschiedenen
/// Werten); ein leerer Text löscht es.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Edit {
    pub fields: [Option<String>; 7],
    pub cover: CoverEdit,
}

impl Edit {
    /// Alle Felder und das Cover genau so setzen (Rückgängig).
    pub fn exact(f: &Fields, cover: Option<&Cover>) -> Self {
        let mut e = Self::default();
        for (i, slot) in e.fields.iter_mut().enumerate() {
            *slot = Some(f.get(i).to_string());
        }
        e.cover = cover.map_or(CoverEdit::Remove, |c| CoverEdit::Set(c.data.clone()));
        e
    }

    pub fn is_empty(&self) -> bool {
        self.fields.iter().all(Option::is_none) && self.cover == CoverEdit::Keep
    }
}

/// Warum eine Datei nicht geschrieben wurde.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteError {
    /// Datei oder Ordner schreibgeschützt.
    ReadOnly,
    /// Ein anderes Programm hält die Datei (Windows); später erneut versuchen.
    Locked,
    /// Ungültige Eingabe oder Format ohne Tags.
    Invalid(String),
    Other(String),
}

impl std::fmt::Display for WriteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ReadOnly => f.write_str("schreibgeschützt"),
            Self::Locked => f.write_str("gesperrt (ein anderes Programm nutzt die Datei)"),
            Self::Invalid(m) | Self::Other(m) => f.write_str(m),
        }
    }
}

fn io_error(e: &std::io::Error) -> WriteError {
    // Windows: 32 = Freigabeverletzung, 33 = Sperrverletzung (unter Unix andere Bedeutung).
    #[cfg(windows)]
    if matches!(e.raw_os_error(), Some(32 | 33)) {
        return WriteError::Locked;
    }
    match e.kind() {
        std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::ReadOnlyFilesystem => {
            WriteError::ReadOnly
        }
        _ => WriteError::Other(e.to_string()),
    }
}

/// Fehler beim letzten Schritt (Ersetzen): Die Kopie im selben Ordner ging schon, also ist
/// „Zugriff verweigert“ hier eine Sperre durch ein anderes Programm, nicht Schreibschutz.
fn rename_error(e: &std::io::Error) -> WriteError {
    match io_error(e) {
        WriteError::ReadOnly => WriteError::Locked,
        other => other,
    }
}

fn text(s: Option<std::borrow::Cow<'_, str>>) -> String {
    s.map(|v| v.trim().to_string()).unwrap_or_default()
}

fn read_tagged(path: &Path, covers: bool) -> Result<lofty::file::TaggedFile, String> {
    Probe::open(path)
        .map_err(|e| e.to_string())?
        .options(ParseOptions::new().read_cover_art(covers))
        .read()
        .map_err(|e| e.to_string())
}

/// Liest die bearbeitbaren Felder und das erste Bild (Vorderseite bevorzugt).
pub fn read_fields(path: &Path) -> Result<(Fields, Option<Cover>), String> {
    let tagged = read_tagged(path, true)?;
    let Some(tag) = tagged.primary_tag().or_else(|| tagged.first_tag()) else {
        return Ok((Fields::default(), None));
    };
    let f = Fields {
        title: text(tag.title()),
        artist: text(tag.artist()),
        album: text(tag.album()),
        year: tag.date().map(|d| d.year.to_string()).unwrap_or_default(),
        genre: text(tag.genre()),
        track: tag.track().map(|n| n.to_string()).unwrap_or_default(),
        comment: text(tag.comment()),
    };
    let pics = tag.pictures();
    let pic = pics
        .iter()
        .find(|p| p.pic_type() == PictureType::CoverFront)
        .or_else(|| pics.first());
    let cover = pic.map(|p| Cover {
        data: p.data().to_vec(),
        mime: p
            .mime_type()
            .map_or_else(|| "image/jpeg".into(), |m| m.as_str().to_string()),
    });
    Ok((f, cover))
}

fn apply(tag: &mut Tag, edit: &Edit) -> Result<(), WriteError> {
    let set = |tag: &mut Tag, i: usize, v: &str| -> Result<(), WriteError> {
        let v = v.trim();
        match (i, v.is_empty()) {
            (0, true) => tag.remove_title(),
            (0, false) => tag.set_title(v.into()),
            (1, true) => tag.remove_artist(),
            (1, false) => tag.set_artist(v.into()),
            (2, true) => tag.remove_album(),
            (2, false) => tag.set_album(v.into()),
            (3, true) => tag.remove_date(),
            (3, false) => {
                let year = v
                    .parse::<u16>()
                    .map_err(|_| WriteError::Invalid("Jahr: vier Ziffern".into()))?;
                tag.set_date(Timestamp {
                    year,
                    ..Timestamp::default()
                });
            }
            (4, true) => tag.remove_genre(),
            (4, false) => tag.set_genre(v.into()),
            (5, true) => tag.remove_track(),
            (5, false) => tag.set_track(
                v.parse()
                    .map_err(|_| WriteError::Invalid("Tracknummer: eine Zahl".into()))?,
            ),
            (_, true) => tag.remove_comment(),
            (_, false) => tag.set_comment(v.into()),
        }
        Ok(())
    };
    for (i, f) in edit.fields.iter().enumerate() {
        if let Some(v) = f {
            set(tag, i, v)?;
        }
    }
    match &edit.cover {
        CoverEdit::Keep => {}
        CoverEdit::Remove => {
            while !tag.pictures().is_empty() {
                tag.remove_picture(0);
            }
        }
        CoverEdit::Set(data) => {
            let mut pic = Picture::from_reader(&mut data.as_slice())
                .map_err(|_| WriteError::Invalid("Bild: nur JPEG oder PNG".into()))?;
            pic.set_pic_type(PictureType::CoverFront);
            while !tag.pictures().is_empty() {
                tag.remove_picture(0);
            }
            tag.push_picture(pic);
        }
    }
    Ok(())
}

/// Legt die Zwischendatei im selben Ordner neu an: zufälliger Name, `create_new` (folgt keinem
/// Symlink und überschreibt nichts), ohne Audio-Endung (Import und Formaterkennung nach
/// Endung übersehen sie). Gibt Pfad und offene Datei zurück.
fn create_temp(path: &Path) -> Result<(std::path::PathBuf, std::fs::File), WriteError> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let dir = path.parent().unwrap_or(Path::new("."));
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as u64);
    for _ in 0..8 {
        let n = N.fetch_add(1, Ordering::Relaxed);
        let mix = (nanos
            ^ n.wrapping_mul(0x9E37_79B9_7F4A_7C15)
            ^ u64::from(std::process::id()).rotate_left(32))
        .wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
        let tmp = dir.join(format!(".woa-tags-{mix:016x}.tmp"));
        match std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&tmp)
        {
            Ok(f) => return Ok((tmp, f)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(io_error(&e)),
        }
    }
    Err(WriteError::Other("keine Zwischendatei anlegbar".into()))
}

/// Löscht die eigene Zwischendatei, aber nur, wenn dort wirklich eine normale Datei liegt.
fn remove_temp(tmp: &Path) {
    if std::fs::symlink_metadata(tmp).is_ok_and(|m| m.file_type().is_file()) {
        let _ = std::fs::remove_file(tmp);
    }
}

/// Schreibt `edit` in die Datei, ohne sie je halb geschrieben zu hinterlassen: Kopie im selben
/// Ordner, Tags dort schreiben, auf die Platte bringen, dann atomar ersetzen. Die Audiodaten
/// werden nicht angefasst (lofty schreibt nur die Tag-Blöcke). Wiedergabe läuft aus dem
/// Speicher weiter; unter Windows scheitert das Ersetzen nur, wenn ein anderes Programm die
/// Datei ohne Löschfreigabe offen hält ([`WriteError::Locked`]).
///
/// Ist `path` ein Symlink, wird bewusst die Datei bearbeitet, auf die er zeigt (der Link
/// bleibt ein Link); die Zwischendatei liegt dann neben dieser Datei.
pub fn write_fields(path: &Path, edit: &Edit) -> Result<(), WriteError> {
    if edit.is_empty() {
        return Ok(());
    }
    let file_type = FileType::from_path(path)
        .ok_or_else(|| WriteError::Invalid("Format ohne Titelinfos".into()))?;
    let link = std::fs::symlink_metadata(path).map_err(|e| io_error(&e))?;
    let real = if link.file_type().is_symlink() {
        std::fs::canonicalize(path).map_err(|e| io_error(&e))?
    } else {
        path.to_path_buf()
    };
    let meta = std::fs::metadata(&real).map_err(|e| io_error(&e))?;
    if !meta.is_file() {
        return Err(WriteError::Invalid("keine normale Datei".into()));
    }
    if meta.permissions().readonly() {
        return Err(WriteError::ReadOnly);
    }
    let (tmp, file) = create_temp(&real)?;
    let r = write_via(&real, &tmp, file, file_type, edit);
    if r.is_err() {
        remove_temp(&tmp);
    }
    r
}

fn write_via(
    path: &Path,
    tmp: &Path,
    mut file: std::fs::File,
    file_type: FileType,
    edit: &Edit,
) -> Result<(), WriteError> {
    use std::io::Seek;
    let mut src = std::fs::File::open(path).map_err(|e| io_error(&e))?;
    std::io::copy(&mut src, &mut file).map_err(|e| io_error(&e))?;
    drop(src);
    // Rechte wie das Original (die neue Datei ersetzt es).
    if let Ok(m) = std::fs::metadata(path) {
        let _ = file.set_permissions(m.permissions());
    }
    file.rewind().map_err(|e| io_error(&e))?;
    let mut tagged = Probe::new(std::io::BufReader::new(&mut file))
        .set_file_type(file_type)
        .options(ParseOptions::new().read_cover_art(true))
        .read()
        .map_err(|e| WriteError::Other(e.to_string()))?;
    let tag_type = tagged.primary_tag_type();
    if tagged.tag(tag_type).is_none() {
        tagged.insert_tag(Tag::new(tag_type));
    }
    let Some(tag) = tagged.tag_mut(tag_type) else {
        return Err(WriteError::Invalid("Format ohne Titelinfos".into()));
    };
    apply(tag, edit)?;
    file.rewind().map_err(|e| io_error(&e))?;
    tagged
        .save_to(&mut file, WriteOptions::default())
        .map_err(|e| WriteError::Other(e.to_string()))?;
    file.sync_all().map_err(|e| io_error(&e))?;
    drop(file);
    std::fs::rename(tmp, path).map_err(|e| rename_error(&e))
}
#[cfg(test)]
mod tests {
    use super::*;

    const FORMATS: [&str; 6] = ["mp3", "flac", "ogg", "opus", "m4a", "wav"];

    fn data(name: &str) -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/data")
            .join(name)
    }

    fn copy(dir: &Path, ext: &str) -> std::path::PathBuf {
        let p = dir.join(format!("lied.{ext}"));
        std::fs::copy(data(&format!("ton.{ext}")), &p).unwrap();
        p
    }

    fn leftovers(dir: &Path) -> usize {
        std::fs::read_dir(dir)
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with(".woa-tags-"))
            .count()
    }

    fn full_edit(cover: &[u8]) -> Edit {
        let f = Fields {
            title: "Schöne Grüße".into(),
            artist: "Die Ärzte".into(),
            album: "Live".into(),
            year: "1998".into(),
            genre: "Rock".into(),
            track: "7".into(),
            comment: "Tippfehler behoben".into(),
        };
        let mut e = Edit::exact(&f, None);
        e.cover = CoverEdit::Set(cover.to_vec());
        e
    }

    #[test]
    fn all_fields_and_cover_round_trip_in_every_format() {
        let dir = tempfile::tempdir().unwrap();
        let png = std::fs::read(data("cover.png")).unwrap();
        for ext in FORMATS {
            let p = copy(dir.path(), ext);
            write_fields(&p, &full_edit(&png)).unwrap_or_else(|e| panic!("{ext}: {e}"));
            let (f, c) = read_fields(&p).unwrap();
            assert_eq!(f.title, "Schöne Grüße", "{ext}");
            assert_eq!(f.artist, "Die Ärzte", "{ext}");
            assert_eq!(f.album, "Live", "{ext}");
            assert_eq!(f.year, "1998", "{ext}");
            assert_eq!(f.genre, "Rock", "{ext}");
            assert_eq!(f.track, "7", "{ext}");
            assert_eq!(f.comment, "Tippfehler behoben", "{ext}");
            assert_eq!(c.expect("Cover").data, png, "{ext}");
            // Kopf lesen wie der Import.
            let h = read_header(&p).unwrap();
            assert_eq!(h.title.as_deref(), Some("Schöne Grüße"), "{ext}");
            // Keine Zwischendatei bleibt liegen.
            assert_eq!(leftovers(dir.path()), 0, "{ext}");
        }
    }

    #[test]
    fn cover_replace_and_remove_and_partial_edit() {
        let dir = tempfile::tempdir().unwrap();
        let png = std::fs::read(data("cover.png")).unwrap();
        let jpg = std::fs::read(data("cover.jpg")).unwrap();
        for ext in FORMATS {
            let p = copy(dir.path(), ext);
            write_fields(&p, &full_edit(&png)).unwrap();
            // Nur Titel und Cover ändern, der Rest bleibt (Mehrfachauswahl).
            let mut e = Edit::default();
            e.fields[0] = Some("Neu".into());
            e.cover = CoverEdit::Set(jpg.clone());
            write_fields(&p, &e).unwrap();
            let (f, c) = read_fields(&p).unwrap();
            assert_eq!(
                (f.title.as_str(), f.artist.as_str()),
                ("Neu", "Die Ärzte"),
                "{ext}"
            );
            assert_eq!(c.unwrap().data, jpg, "{ext}");
            // Leerer Text löscht, Cover entfernen.
            let mut e = Edit::default();
            e.fields[6] = Some(String::new());
            e.cover = CoverEdit::Remove;
            write_fields(&p, &e).unwrap();
            let (f, c) = read_fields(&p).unwrap();
            assert_eq!(f.comment, "", "{ext}");
            assert!(c.is_none(), "{ext}");
        }
    }

    /// Die Audiodaten bleiben bitgenau (dekodiert verglichen; Opus kann WOA nicht dekodieren).
    #[test]
    fn audio_samples_stay_bit_identical() {
        let dir = tempfile::tempdir().unwrap();
        let png = std::fs::read(data("cover.png")).unwrap();
        for ext in ["mp3", "flac", "ogg", "m4a", "wav"] {
            let p = copy(dir.path(), ext);
            let before = woa_engine::decode::decode_file(&p).unwrap();
            write_fields(&p, &full_edit(&png)).unwrap();
            let after = woa_engine::decode::decode_file(&p).unwrap();
            assert_eq!(before.rate, after.rate, "{ext}");
            assert_eq!(before.channels, after.channels, "{ext}");
            assert!(
                before
                    .samples
                    .iter()
                    .map(|v| v.to_bits())
                    .eq(after.samples.iter().map(|v| v.to_bits())),
                "{ext}: Audio verändert"
            );
            assert!(after.frames() > 0, "{ext}");
        }
    }

    #[test]
    fn bad_input_and_read_only_file_are_reported() {
        let dir = tempfile::tempdir().unwrap();
        let p = copy(dir.path(), "mp3");
        let original = std::fs::read(&p).unwrap();
        let mut e = Edit::default();
        e.fields[3] = Some("neunzehn".into());
        assert!(matches!(write_fields(&p, &e), Err(WriteError::Invalid(_))));
        let e = Edit {
            cover: CoverEdit::Set(b"kein Bild".to_vec()),
            ..Edit::default()
        };
        assert!(matches!(write_fields(&p, &e), Err(WriteError::Invalid(_))));
        assert_eq!(
            std::fs::read(&p).unwrap(),
            original,
            "nichts halb geschrieben"
        );
        assert_eq!(leftovers(dir.path()), 0);

        let mut perm = std::fs::metadata(&p).unwrap().permissions();
        perm.set_readonly(true);
        std::fs::set_permissions(&p, perm).unwrap();
        let mut e = Edit::default();
        e.fields[0] = Some("x".into());
        assert_eq!(write_fields(&p, &e), Err(WriteError::ReadOnly));
        assert_eq!(std::fs::read(&p).unwrap(), original);
    }

    /// WOA selbst hält die Datei offen (Dekoder, Vorhören): Speichern klappt trotzdem, auch
    /// unter Windows (Rust öffnet Dateien mit Lösch-Freigabe), und der offene Leser liest
    /// weiter den alten Stand.
    #[test]
    fn saving_works_while_the_file_is_open_for_reading() {
        let dir = tempfile::tempdir().unwrap();
        let p = copy(dir.path(), "flac");
        let original = std::fs::read(&p).unwrap();
        let mut reader = std::fs::File::open(&p).unwrap();
        let mut e = Edit::default();
        e.fields[0] = Some("Während der Wiedergabe".into());
        write_fields(&p, &e).unwrap();
        let mut old = Vec::new();
        std::io::Read::read_to_end(&mut reader, &mut old).unwrap();
        assert_eq!(old, original);
        assert_eq!(read_fields(&p).unwrap().0.title, "Während der Wiedergabe");
    }

    /// Bibliothek nach dem Schreiben: neuer Hash, Analyse bleibt gültig, Suche findet den Titel.
    #[test]
    fn library_follows_the_new_tags_without_reanalysis() {
        let dir = tempfile::tempdir().unwrap();
        let p = copy(dir.path(), "wav");
        let lib = crate::Library::open(&dir.path().join("l.sqlite3")).unwrap();
        let old = crate::file_hash(&p).unwrap();
        let crate::AddOutcome::Added(id) = lib
            .add_track(&crate::NewTrack {
                path: p.to_str().unwrap().into(),
                hash: old.clone(),
                title: "lied".into(),
                duration: 0.4,
                kind: crate::Kind::Music,
            })
            .unwrap()
        else {
            panic!("nicht neu")
        };
        lib.set_analysis(id, &old, &crate::Analysis::default())
            .unwrap();
        assert_eq!(lib.pending_analysis_count().unwrap(), 0);
        let mut e = Edit::default();
        e.fields[0] = Some("Sommerhit".into());
        e.fields[1] = Some("Band".into());
        write_fields(&p, &e).unwrap();
        let new = crate::file_hash(&p).unwrap();
        assert_ne!(new, old);
        let title = crate::import::display_title(&p, &read_header(&p).unwrap());
        assert_eq!(title, "Band - Sommerhit");
        lib.retagged(id, &old, &new, &title).unwrap();
        assert_eq!(
            lib.pending_analysis_count().unwrap(),
            0,
            "keine neue Messung"
        );
        let t = lib.get(id).unwrap().unwrap();
        assert_eq!(
            (t.title.as_str(), t.hash.as_str()),
            ("Band - Sommerhit", new.as_str())
        );
        assert!(t.analysis.is_some());
        let hit = lib
            .search(&crate::Query {
                text: "sommerhit".into(),
                ..crate::Query::default()
            })
            .unwrap();
        assert_eq!(hit.len(), 1);
    }

    #[test]
    fn os_errors_map_to_read_only_and_locked() {
        #[cfg(windows)]
        assert_eq!(
            io_error(&std::io::Error::from_raw_os_error(32)),
            WriteError::Locked
        );
        let denied = std::io::Error::from(std::io::ErrorKind::PermissionDenied);
        assert_eq!(io_error(&denied), WriteError::ReadOnly);
        // Beim Ersetzen heißt „verweigert“: ein anderes Programm hält die Datei.
        assert_eq!(rename_error(&denied), WriteError::Locked);
    }

    /// Ein Symlink mit einem vorhersagbaren Namen kann nichts überschreiben: die
    /// Zwischendatei hat einen zufälligen Namen und wird neu angelegt. Ein Symlink auf eine
    /// Musikdatei bearbeitet bewusst das Ziel, der Link bleibt.
    #[cfg(unix)]
    #[test]
    fn symlinks_are_never_followed_for_the_temp_file() {
        let dir = tempfile::tempdir().unwrap();
        let victim = dir.path().join("fremd.txt");
        std::fs::write(&victim, "nicht anfassen").unwrap();
        let music = dir.path().join("m");
        std::fs::create_dir_all(&music).unwrap();
        for name in [".lied.mp3.woa-tags.tmp", ".woa-tags-0000000000000000.tmp"] {
            std::os::unix::fs::symlink(&victim, music.join(name)).unwrap();
        }
        let p = copy(&music, "mp3");
        let mut e = Edit::default();
        e.fields[0] = Some("Sicher".into());
        write_fields(&p, &e).unwrap();
        assert_eq!(std::fs::read_to_string(&victim).unwrap(), "nicht anfassen");
        assert_eq!(read_fields(&p).unwrap().0.title, "Sicher");

        let link = music.join("link.mp3");
        std::os::unix::fs::symlink(&p, &link).unwrap();
        e.fields[0] = Some("Über den Link".into());
        write_fields(&link, &e).unwrap();
        assert!(
            std::fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(read_fields(&p).unwrap().0.title, "Über den Link");
    }

    #[test]
    fn year_and_track_are_checked() {
        let mut f = Fields::default();
        assert!(f.check().is_ok());
        f.year = "98".into();
        assert!(f.check().is_err());
        f.year = "1998".into();
        f.track = "0".into();
        assert!(f.check().is_err());
        f.track = "12".into();
        assert!(f.check().is_ok());
    }
}
