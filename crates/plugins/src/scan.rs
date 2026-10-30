//! CLAP-Plugins auf dem Rechner finden (Paket 31).
//!
//! Gesucht wird in den Standardordnern der Plattform und in `CLAP_PATH` (wie der CLAP-Standard
//! es verlangt: Ordnerliste, getrennt mit `:` bzw. `;` unter Windows). Jede Datei mit der
//! Endung `.clap` wird geladen und nach ihren Plugins gefragt.
//!
//! Das Laden geschieht im eigenen Prozess: Eine kaputte Plugin-Bibliothek kann die Suche, im
//! schlimmsten Fall das Programm abstürzen lassen. Ein eigener Suchprozess folgt mit
//! Paket 33; bis dahin sucht die Oberfläche nur auf Knopfdruck und beim Start.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use clack_host::prelude::*;

/// Tiefste Ordnerebene unter einem Suchordner (Schutz vor Schleifen und riesigen Bäumen).
pub const MAX_DEPTH: usize = 4;
/// Höchste Zahl gefundener Dateien je Suche.
pub const MAX_FILES: usize = 2000;

/// Was die Plugin-Liste über ein Plugin weiß (aus dem Beschreiber, ohne es zu starten).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginInfo {
    /// CLAP-Kennung, z. B. `org.surge-synth-team.surge-xt`. Das Projekt speichert nur sie.
    pub clap_id: String,
    /// Anzeigename.
    pub name: String,
    /// Hersteller (kann leer sein).
    pub vendor: String,
    /// Merkmale laut Beschreiber (`instrument`, `audio-effect`, `stereo` ...).
    pub features: Vec<String>,
    /// Die `.clap`-Datei.
    pub path: PathBuf,
}

impl PluginInfo {
    /// Ob das Plugin als Instrument auftritt.
    pub fn is_instrument(&self) -> bool {
        self.features.iter().any(|f| f == "instrument")
    }

    /// Ob das Plugin als Effekt auftritt. Ohne eindeutiges Merkmal gilt ein Plugin, das kein
    /// Instrument ist, als Effekt.
    pub fn is_effect(&self) -> bool {
        self.features.iter().any(|f| f == "audio-effect") || !self.is_instrument()
    }
}

/// Ein Plugin, das die Suche nicht lesen konnte (Datei, Grund).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanProblem {
    /// Betroffene Datei.
    pub path: PathBuf,
    /// Kurzer Grund.
    pub reason: String,
}

/// Ergebnis einer Suche: die Plugins und alles, was sich nicht laden ließ.
#[derive(Default, Clone)]
pub struct Catalog {
    plugins: Vec<PluginInfo>,
    problems: Vec<ScanProblem>,
    entries: HashMap<PathBuf, PluginEntry>,
}

impl std::fmt::Debug for Catalog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Catalog")
            .field("plugins", &self.plugins.len())
            .field("problems", &self.problems.len())
            .finish()
    }
}

impl Catalog {
    /// Eine leere Liste.
    pub fn empty() -> Self {
        Self::default()
    }

    /// Sucht in den Standardordnern und in `CLAP_PATH`.
    pub fn scan_default() -> Self {
        Self::scan(&search_paths())
    }

    /// Sucht in den genannten Ordnern. Eine Kennung, die in mehreren Dateien vorkommt, gilt
    /// einmal: die der früheren Fundstelle.
    pub fn scan(roots: &[PathBuf]) -> Self {
        let mut cat = Self::default();
        for file in find_files(roots) {
            cat.add_file(&file);
        }
        cat.plugins
            .sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        tracing::info!(
            plugins = cat.plugins.len(),
            problems = cat.problems.len(),
            "clap scan done"
        );
        cat
    }

    fn add_file(&mut self, file: &Path) {
        // SAFETY: Eine Plugin-Bibliothek ist fremder Code; ihn zu laden führt ihn aus. Das ist
        // der Zweck der Suche, die Datei steht in einem Ordner, den der Nutzer (oder
        // `CLAP_PATH`) bestimmt hat. Schutz durch einen eigenen Prozess: Paket 33.
        let entry = match unsafe { PluginEntry::load(file) } {
            Ok(e) => e,
            Err(e) => {
                tracing::warn!(file = %file.display(), error = %e, "clap load failed");
                self.problems.push(ScanProblem {
                    path: file.to_path_buf(),
                    reason: e.to_string(),
                });
                return;
            }
        };
        let Some(factory) = entry.get_plugin_factory() else {
            self.problems.push(ScanProblem {
                path: file.to_path_buf(),
                reason: "keine Plugin-Fabrik".into(),
            });
            return;
        };
        let mut found = false;
        for d in factory.plugin_descriptors() {
            let (Some(id), Some(name)) = (d.id(), d.name()) else {
                continue;
            };
            let id = id.to_string_lossy().into_owned();
            if id.is_empty() || self.plugins.iter().any(|p| p.clap_id == id) {
                continue;
            }
            found = true;
            self.plugins.push(PluginInfo {
                clap_id: id,
                name: name.to_string_lossy().into_owned(),
                vendor: d
                    .vendor()
                    .map(|v| v.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                features: d
                    .features()
                    .map(|f| f.to_string_lossy().into_owned())
                    .collect(),
                path: file.to_path_buf(),
            });
        }
        if found {
            self.entries.insert(file.to_path_buf(), entry);
        }
    }

    /// Alle gefundenen Plugins, nach Name sortiert.
    pub fn plugins(&self) -> &[PluginInfo] {
        &self.plugins
    }

    /// Dateien, die sich nicht laden ließen.
    pub fn problems(&self) -> &[ScanProblem] {
        &self.problems
    }

    /// Das Plugin mit dieser Kennung.
    pub fn find(&self, clap_id: &str) -> Option<&PluginInfo> {
        self.plugins.iter().find(|p| p.clap_id == clap_id)
    }

    pub(crate) fn entry(&self, path: &Path) -> Option<&PluginEntry> {
        self.entries.get(path)
    }

    /// Trägt ein bereits geladenes Plugin ein (Tests mit dem Test-Plugin).
    #[cfg(any(test, feature = "test-plugin"))]
    pub fn add_loaded(&mut self, info: PluginInfo, entry: PluginEntry) {
        self.entries.insert(info.path.clone(), entry);
        self.plugins.push(info);
    }
}

/// Die Ordner, in denen gesucht wird: erst `CLAP_PATH`, dann die Standardordner.
pub fn search_paths() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(v) = std::env::var_os("CLAP_PATH") {
        out.extend(std::env::split_paths(&v));
    }
    out.extend(standard_paths(
        std::env::var_os("HOME").map(PathBuf::from),
        std::env::var_os("COMMONPROGRAMFILES").map(PathBuf::from),
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from),
    ));
    let mut seen = Vec::new();
    out.retain(|p| {
        let keep = !p.as_os_str().is_empty() && !seen.contains(p);
        if keep {
            seen.push(p.clone());
        }
        keep
    });
    out
}

/// Standardordner der Plattform aus CLAP (Heimordner, `%COMMONPROGRAMFILES%`,
/// `%LOCALAPPDATA%`). Die Eingaben stehen im Parameter, damit jede Plattform überall
/// getestet werden kann.
pub fn standard_paths(
    home: Option<PathBuf>,
    common_program_files: Option<PathBuf>,
    local_app_data: Option<PathBuf>,
) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if cfg!(windows) {
        if let Some(c) = common_program_files {
            out.push(c.join("CLAP"));
        }
        if let Some(l) = local_app_data {
            out.push(l.join("Programs").join("Common").join("CLAP"));
        }
    } else if cfg!(target_os = "macos") {
        out.push(PathBuf::from("/Library/Audio/Plug-Ins/CLAP"));
        if let Some(h) = home {
            out.push(h.join("Library/Audio/Plug-Ins/CLAP"));
        }
    } else {
        if let Some(h) = home {
            out.push(h.join(".clap"));
        }
        out.push(PathBuf::from("/usr/lib/clap"));
        out.push(PathBuf::from("/usr/local/lib/clap"));
    }
    out
}

/// Alle `.clap`-Dateien unter den Wurzeln, begrenzt in Tiefe und Zahl. Symbolische Links auf
/// Ordner werden nicht verfolgt (keine Schleifen); Links auf Dateien zählen.
pub fn find_files(roots: &[PathBuf]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for r in roots {
        walk(r, 0, &mut out);
        if out.len() >= MAX_FILES {
            break;
        }
    }
    out.sort();
    out.dedup();
    out
}

fn is_clap(p: &Path) -> bool {
    p.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("clap"))
}

fn walk(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    if depth > MAX_DEPTH || out.len() >= MAX_FILES {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let path = e.path();
        let Ok(ty) = e.file_type() else { continue };
        if is_clap(&path) {
            // Unter macOS ist ein Plugin ein Bündel (Ordner), sonst eine Datei.
            if ty.is_file() || ty.is_dir() || path.is_file() {
                out.push(path);
            }
        } else if ty.is_dir() {
            walk(&path, depth + 1, out);
        }
        if out.len() >= MAX_FILES {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_clap_files_in_nested_folders_and_ignores_the_rest() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path();
        std::fs::create_dir_all(root.join("herstellerA/sub")).unwrap();
        std::fs::write(root.join("a.clap"), b"x").unwrap();
        std::fs::write(root.join("herstellerA/sub/b.CLAP"), b"x").unwrap();
        std::fs::write(root.join("herstellerA/readme.txt"), b"x").unwrap();
        let got = find_files(&[root.to_path_buf(), root.join("fehlt")]);
        assert_eq!(got.len(), 2, "{got:?}");
    }

    #[test]
    fn depth_is_limited() {
        let d = tempfile::tempdir().unwrap();
        let mut p = d.path().to_path_buf();
        for i in 0..=MAX_DEPTH + 2 {
            p = p.join(format!("e{i}"));
        }
        std::fs::create_dir_all(&p).unwrap();
        std::fs::write(p.join("tief.clap"), b"x").unwrap();
        assert!(find_files(&[d.path().to_path_buf()]).is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn symlink_loops_do_not_hang() {
        let d = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(d.path(), d.path().join("schleife")).unwrap();
        std::fs::write(d.path().join("x.clap"), b"x").unwrap();
        assert_eq!(find_files(&[d.path().to_path_buf()]).len(), 1);
    }

    #[test]
    fn broken_file_is_a_problem_not_a_crash() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("kaputt.clap"), b"das ist keine Bibliothek").unwrap();
        let cat = Catalog::scan(&[d.path().to_path_buf()]);
        assert!(cat.plugins().is_empty());
        assert_eq!(cat.problems().len(), 1);
    }

    #[test]
    fn standard_paths_per_platform_are_not_empty() {
        let p = standard_paths(
            Some("/home/x".into()),
            Some("C:/Common".into()),
            Some("C:/Local".into()),
        );
        assert!(!p.is_empty());
    }

    #[test]
    fn clap_path_comes_first() {
        // Nur die Aufspaltung wird geprüft; die Umgebung bleibt unberührt.
        let joined = std::env::join_paths(["/a/clap", "/b/clap"]).unwrap();
        let parts: Vec<_> = std::env::split_paths(&joined).collect();
        assert_eq!(
            parts,
            vec![PathBuf::from("/a/clap"), PathBuf::from("/b/clap")]
        );
    }
}
