//! J6 Ausgabe (#261): ein fertiges Jingle aus dem Mix. Verstärkung auf die Ziel-Lautheit,
//! Limiter auf −1 dBFS, Datei im Jingle-Ordner, Eintrag als Jingle in der Bibliothek und auf
//! Wunsch auf eine Cart. Oberfläche und MCP (`export_jingle`) gehen beide durch [`publish`].

use std::path::{Path, PathBuf};

use crate::audio::{self, CEILING_DB};

/// Wohin und wie ein Jingle geht.
#[derive(Debug, Clone)]
pub struct Target {
    /// Jingle-Ordner (`WOA/Jingles` oder der eigene aus den Einstellungen).
    pub dir: PathBuf,
    /// Dateiname ohne Endung, kein Pfad.
    pub name: String,
    /// Ziel-Lautheit; `None` = die der Sendung (`target_lufs` in radio.json).
    pub target_lufs: Option<f64>,
    /// Cart 1 bis 16; `None` = keine.
    pub cart: Option<usize>,
    /// Bibliothek (`bibliothek.sqlite3`).
    pub db: PathBuf,
    /// Sendung (`radio.json`).
    pub radio: PathBuf,
}

impl Target {
    /// Die Vorgabe-Orte der App: Jingle-Ordner nach `config.json`, Bibliothek und radio.json
    /// im Datenordner.
    pub fn standard(name: impl Into<String>) -> Self {
        Self {
            dir: woa_radio::ordner::configured_dir(woa_radio::ordner::Art::Jingles),
            name: name.into(),
            target_lufs: None,
            cart: None,
            db: woa_radio::data_dir().join(woa_library::DB_FILE),
            radio: woa_radio::default_radio_path(),
        }
    }
}

/// Was entstanden ist.
#[derive(Debug, Clone, PartialEq)]
pub struct Published {
    pub path: PathBuf,
    pub target_lufs: f64,
    pub lufs: Option<f64>,
    pub peak_db: Option<f64>,
    /// Stärkste Absenkung durch den Limiter in dB.
    pub limiter_db: f32,
    pub library_id: i64,
    pub cart: Option<usize>,
}

/// Antwort des MCP-Werkzeugs `export_jingle`.
pub fn to_json(j: &Published) -> serde_json::Value {
    serde_json::json!({
        "path": j.path.to_string_lossy(),
        "target_lufs": j.target_lufs,
        "lufs": j.lufs.map(|v| audio::round(v, 2)),
        "sample_peak_dbfs": j.peak_db.map(|v| audio::round(v, 2)),
        "limiter_db": audio::round(f64::from(j.limiter_db), 2),
        "library_id": j.library_id,
        "cart": j.cart,
    })
}

/// Prüft den Namen: nicht leer, kein Pfad, keine Steuerzeichen.
pub fn check_name(name: &str) -> Result<&str, String> {
    let n = name.trim();
    let bad = n.is_empty()
        || n.starts_with('.')
        || n.chars().any(|c| {
            c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')
        });
    if bad {
        return Err(format!(
            "Name „{name}“ geht nicht: nur ein Dateiname ohne Pfad und ohne / \\ : * ? \" < > |"
        ));
    }
    Ok(n)
}

/// Freier Dateiname im Ordner: `name.wav`, sonst `name 2.wav`, `name 3.wav` … Nichts wird
/// überschrieben.
fn free_path(dir: &Path, name: &str) -> PathBuf {
    let first = dir.join(format!("{name}.wav"));
    if !first.exists() {
        return first;
    }
    (2..1000)
        .map(|n| dir.join(format!("{name} {n}.wav")))
        .find(|p| !p.exists())
        .unwrap_or(first)
}

/// Macht aus der Mischung ein Jingle nach `to` (siehe Moduldoku); `left`/`right` werden dabei
/// verändert. `rate` ist die Abtastrate der Mischung und der Datei.
pub fn publish(
    left: &mut [f32],
    right: &mut [f32],
    rate: u32,
    to: &Target,
) -> Result<Published, String> {
    let name = check_name(&to.name)?;
    if let Some(c) = to.cart
        && !(1..=woa_radio::CARTS).contains(&c)
    {
        return Err(format!(
            "Cart {c} gibt es nicht (1 bis {})",
            woa_radio::CARTS
        ));
    }
    let mut show = match to.cart {
        Some(_) => Some(woa_radio::RadioShow::load(&to.radio)?),
        None => None,
    };
    let target = match to.target_lufs {
        Some(t) => t,
        None => woa_radio::RadioShow::load(&to.radio)
            .map(|s| s.target_lufs)
            .unwrap_or(woa_radio::level::TARGET_LUFS),
    };
    if !(target.is_finite() && (-70.0..=0.0).contains(&target)) {
        return Err(format!(
            "Ziel-Lautheit muss zwischen -70 und 0 LUFS liegen, war {target}"
        ));
    }
    let m = woa_engine::loudness::master(left, right, rate, target, CEILING_DB)
        .ok_or("Mix ist still, kein Jingle")?;
    std::fs::create_dir_all(&to.dir).map_err(|e| format!("{}: {e}", to.dir.display()))?;
    let path = free_path(&to.dir, name);
    // Erst unter einem Zwischennamen schreiben, dann umbenennen: nie eine halbe Datei.
    let part = path.with_extension("wav.part");
    let spec = woa_engine::export::ExportSpec::new(woa_engine::export::ExportFormat::Wav);
    if let Err(e) = woa_engine::export::export_with(&part, &spec, left, right, rate)
        .map_err(|e| e.to_string())
        .and_then(|()| std::fs::rename(&part, &path).map_err(|e| e.to_string()))
    {
        let _ = std::fs::remove_file(&part);
        return Err(e);
    }

    let lib = woa_library::Library::open(&to.db).map_err(|e| e.to_string())?;
    let new = woa_library::import::probe(&path, woa_library::Kind::Jingle)?;
    let (id, duplicate) = match lib.add_track(&new).map_err(|e| e.to_string())? {
        woa_library::AddOutcome::Added(id) | woa_library::AddOutcome::Updated(id) => (id, false),
        woa_library::AddOutcome::Duplicate(id) => (id, true),
    };
    let track = lib
        .get(id)
        .map_err(|e| e.to_string())?
        .ok_or("Jingle nicht in der Bibliothek eingetragen")?;
    // Gleicher Inhalt liegt schon als Datei da: die eben geschriebene Kopie wieder weg, das
    // bekannte Jingle gilt.
    let path = if duplicate {
        let _ = std::fs::remove_file(&path);
        PathBuf::from(&track.path)
    } else {
        path
    };
    if track.kind != woa_library::Kind::Jingle {
        lib.set_kind(track.id, woa_library::Kind::Jingle)
            .map_err(|e| e.to_string())?;
    }

    if let (Some(show), Some(c)) = (show.as_mut(), to.cart) {
        show.carts.resize(woa_radio::CARTS, None);
        show.carts[c - 1] = Some(woa_radio::CartEntry::new(path.clone()));
        show.save(&to.radio)?;
    }
    Ok(Published {
        lufs: woa_engine::loudness::integrated_lufs(left, right, rate),
        peak_db: audio::peak_db(left, right),
        path,
        target_lufs: target,
        limiter_db: m.reduction_db,
        library_id: track.id,
        cart: to.cart,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Mix {
        left: Vec<f32>,
        right: Vec<f32>,
    }

    fn go(mut m: Mix, to: &Target) -> Result<Published, String> {
        publish(&mut m.left, &mut m.right, 48_000, to)
    }

    fn loud_mix() -> Mix {
        let mut left: Vec<f32> = (0..48_000 * 3)
            .map(|i| (i as f32 * 440.0 * std::f32::consts::TAU / 48_000.0).sin() * 0.05)
            .collect();
        for i in (0..left.len()).step_by(6_000) {
            left[i] = 0.95;
        }
        Mix {
            right: left.clone(),
            left,
        }
    }

    fn target(dir: &Path, cart: Option<usize>) -> Target {
        Target {
            dir: dir.join("Jingles"),
            name: "Station Ident".into(),
            target_lufs: Some(-16.0),
            cart,
            db: dir.join("daten").join(woa_library::DB_FILE),
            radio: dir.join("daten").join("radio.json"),
        }
    }

    #[test]
    fn jingle_lands_in_folder_library_and_cart() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("daten")).unwrap();
        let to = target(tmp.path(), Some(3));
        let p = go(loud_mix(), &to).unwrap();
        assert_eq!(p.path, tmp.path().join("Jingles/Station Ident.wav"));
        assert!(p.path.is_file());
        assert!((p.lufs.unwrap() + 16.0).abs() <= 0.5, "{:?}", p.lufs);
        assert!(p.peak_db.unwrap() <= CEILING_DB + 1e-3);
        assert!(p.limiter_db > 0.0);
        let lib = woa_library::Library::open(&to.db).unwrap();
        let t = lib.by_path(p.path.to_str().unwrap()).unwrap().unwrap();
        assert_eq!(t.kind, woa_library::Kind::Jingle);
        let show = woa_radio::RadioShow::load(&to.radio).unwrap();
        assert_eq!(show.carts[2].as_ref().unwrap().path, p.path);
        assert!(
            show.carts
                .iter()
                .enumerate()
                .all(|(i, c)| i == 2 || c.is_none())
        );

        // Gleicher Inhalt noch einmal: keine Kopie, das bekannte Jingle gilt.
        let same = go(loud_mix(), &target(tmp.path(), None)).unwrap();
        assert_eq!(same.path, p.path);
        assert!(!tmp.path().join("Jingles/Station Ident 2.wav").exists());
        // Geänderter Inhalt: nichts überschrieben, neue Datei, wieder ein Jingle.
        let mut other = loud_mix();
        other.left[100] = 0.5;
        let q = go(other, &target(tmp.path(), None)).unwrap();
        assert_eq!(q.path, tmp.path().join("Jingles/Station Ident 2.wav"));
        assert_eq!(
            lib.by_path(q.path.to_str().unwrap()).unwrap().unwrap().kind,
            woa_library::Kind::Jingle
        );
    }

    #[test]
    fn target_defaults_to_the_show_loudness() {
        let tmp = tempfile::tempdir().unwrap();
        let mut to = target(tmp.path(), None);
        to.target_lufs = None;
        let show = woa_radio::RadioShow {
            target_lufs: -20.0,
            ..Default::default()
        };
        show.save(&to.radio).unwrap();
        let p = go(loud_mix(), &to).unwrap();
        assert_eq!(p.target_lufs, -20.0);
        assert!((p.lufs.unwrap() + 20.0).abs() <= 0.5);
    }

    #[test]
    fn bad_names_and_carts_are_refused_before_writing() {
        let tmp = tempfile::tempdir().unwrap();
        for bad in ["", "../x", "a/b", "a\\b", ".hidden"] {
            let mut to = target(tmp.path(), None);
            to.name = bad.into();
            assert!(go(loud_mix(), &to).is_err(), "{bad}");
        }
        for c in [0, 17] {
            assert!(go(loud_mix(), &target(tmp.path(), Some(c))).is_err());
        }
        assert!(!tmp.path().join("Jingles").exists());
    }
}
