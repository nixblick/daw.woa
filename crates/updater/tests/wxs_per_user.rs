//! #452: Das MSI installiert pro Benutzer, ohne Admin-Abfrage. Prüft `packaging/windows/woa.wxs`
//! als Text (WiX selbst gibt es nur im Windows-Runner).
//!
//! Rotprobe vor dem Umbau (04.11.2026, Scope="perMachine"):
//!   thread 'msi_installs_per_user' panicked: Scope="perUser" fehlt
//!   thread 'msi_writes_no_machine_wide_state' panicked: ProgramFiles64Folder
//!   test result: FAILED. 1 passed; 2 failed

#![allow(clippy::unwrap_used, clippy::expect_used)] // Tests duerfen abbrechen (Regel 6 gilt fuer Produktivcode).

const WXS: &str = include_str!("../../../packaging/windows/woa.wxs");

/// Der Teil ohne `<?if $(var.Legacy) ...?>`-Zweig (nur test-upgrade.ps1 baut die alte Form).
fn product() -> String {
    let mut out = String::new();
    let mut skip = false;
    for line in WXS.lines() {
        let t = line.trim();
        if t.starts_with("<?if $(var.Legacy)") {
            skip = true;
        } else if skip && t.starts_with("<?else?>") {
            skip = false;
        } else if !skip && !t.starts_with("<?endif?>") {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

#[test]
fn msi_installs_per_user() {
    let p = product();
    assert!(
        p.contains(r#"Scope="$(var.Scope)""#) || p.contains(r#"Scope="perUser""#),
        "Scope fehlt"
    );
    assert!(
        WXS.contains(r#"<?define Scope = "perUser" ?>"#),
        r#"Scope="perUser" fehlt"#
    );
    assert!(p.contains(r#"StandardDirectory Id="LocalAppDataFolder""#));
    assert!(p.contains(r#"Directory Id="PROGRAMSFOLDER" Name="Programs""#));
}

#[test]
fn msi_writes_no_machine_wide_state() {
    let p = product();
    for bad in [
        "HKLM",
        "ProgramFiles64Folder",
        "ProgramFilesFolder",
        "CommonAppDataFolder",
        "Impersonate=\"no\"",
    ] {
        assert!(!p.contains(bad), "{bad}");
    }
    // ICE38: jede Komponente im Benutzerprofil hat einen HKCU-Wert als KeyPath, keine Datei.
    for comp in p.split("<Component Id=").skip(1) {
        let comp = comp.split("</Component>").next().unwrap_or_default();
        assert!(
            comp.contains(r#"Root="HKCU""#),
            "Komponente ohne HKCU-KeyPath: {comp}"
        );
        assert!(
            !comp.contains(r#"<File"#)
                || !comp.split("<File").skip(1).any(|f| f
                    .split('>')
                    .next()
                    .unwrap_or_default()
                    .contains(r#"KeyPath="yes""#)),
            "Datei als KeyPath: {comp}"
        );
    }
}

#[test]
fn upgrade_code_unchanged() {
    assert!(WXS.contains(r#"UpgradeCode="16647BDD-1E47-4AA9-AAED-B1D5D1D4A822""#));
}

/// WIX0230 (Release-Lauf v0.41.26): Eine Komponente mit Dateien und HKCU-KeyPath bekommt keine
/// automatische GUID. Jede Komponente mit `<File` braucht eine feste `Guid="..."`.
/// Rotprobe (04.11.2026, ohne Guid): panicked: Komponente WoaExe ohne feste Guid
#[test]
fn file_components_have_fixed_guid() {
    let p = product();
    for teil in p.split("<Component ").skip(1) {
        let kopf = &teil[..teil.find('>').unwrap()];
        let rumpf = &teil[..teil.find("</Component>").unwrap()];
        if !rumpf.contains("<File") {
            continue;
        }
        let id = kopf
            .split("Id=\"")
            .nth(1)
            .and_then(|s| s.split('"').next())
            .unwrap_or("?");
        let guid = kopf
            .split("Guid=\"")
            .nth(1)
            .and_then(|s| s.split('"').next())
            .map(|g| resolve(&p, g));
        assert!(
            matches!(guid, Some(g) if g.len() == 36 && g != "*"),
            "Komponente {id} ohne feste Guid"
        );
    }
}

/// Die alte Form (`-d Legacy=1`, nur für test-upgrade.ps1) muss ebenfalls bauen. Release-Lauf v0.41.27:
/// WIX0094 `Directory:PROGRAMSFOLDER` fehlte dort. Jede `Directory="X"`-Referenz braucht in BEIDEN
/// Formen eine Definition `Id="X"`.
/// Rotprobe (04.11.2026, RemoveFolder ohne Legacy-Weiche): panicked: Legacy: PROGRAMSFOLDER nicht definiert
#[test]
fn directory_refs_defined_in_both_forms() {
    for (name, text) in [("perUser", product()), ("Legacy", legacy())] {
        for teil in text.split("Directory=\"").skip(1) {
            let id = teil.split('"').next().unwrap();
            let definiert = text.contains(&format!("<Directory Id=\"{id}\""))
                || text.contains(&format!("<StandardDirectory Id=\"{id}\""));
            assert!(definiert, "{name}: {id} nicht definiert");
        }
    }
}

/// Gegenstück zu `product()`: nur der Legacy-Zweig jeder Weiche.
fn legacy() -> String {
    let mut out = String::new();
    let mut skip = false;
    let mut in_if = false;
    for line in WXS.lines() {
        let t = line.trim();
        if t.starts_with("<?if $(var.Legacy)") {
            in_if = true;
        } else if in_if && t.starts_with("<?else?>") {
            skip = true;
        } else if in_if && t.starts_with("<?endif") {
            skip = false;
            in_if = false;
        } else if !skip {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

/// `$(var.Name)` aus den `<?define Name = "..." ?>` des jeweiligen Zweigs auflösen.
fn resolve(text: &str, value: &str) -> String {
    let Some(name) = value
        .strip_prefix("$(var.")
        .and_then(|v| v.strip_suffix(')'))
    else {
        return value.to_owned();
    };
    let kopf = format!("<?define {name} = \"");
    text.lines()
        .find_map(|l| l.trim().strip_prefix(kopf.as_str()))
        .and_then(|r| r.split('"').next())
        .unwrap_or("")
        .to_owned()
}

/// Release-Lauf v0.41.32: Mit denselben Komponenten-GUIDs in beiden Formen zählte Windows Installer
/// die Komponente doppelt; das Entfernen der alten Installation ließ `Program Files\WOA` stehen.
/// Rotprobe (05.11.2026, Legacy mit denselben GUIDs): GuidSounds gleich in beiden Formen
#[test]
fn legacy_form_has_its_own_component_guids() {
    for var in ["GuidExe", "GuidSounds"] {
        let neu = resolve(&product(), &format!("$(var.{var})"));
        let alt = resolve(&legacy(), &format!("$(var.{var})"));
        assert_eq!(neu.len(), 36, "{var} pro Benutzer");
        assert_eq!(alt.len(), 36, "{var} Legacy");
        assert_ne!(neu, alt, "{var} gleich in beiden Formen");
    }
}

/// #543: Eine Verknüpfung mit `Icon="…"` zeigt in den MSI-Zwischenspeicher
/// (`%APPDATA%\Microsoft\Installer\{ProductCode}\woa.ico`), der mit jedem Update wechselt;
/// angeheftete Kopien in der Taskleiste zeigen danach ein leeres Blatt. Ohne `Icon` nimmt
/// Windows das Symbol aus der `woa.exe` am festen Installordner.
#[test]
fn shortcuts_take_the_icon_from_the_exe() {
    for (i, part) in product().split("<Shortcut").enumerate().skip(1) {
        let tag = &part[..part.find("/>").or(part.find('>')).unwrap()];
        assert!(
            !tag.contains("Icon="),
            "Verknüpfung {i} bezieht ihr Symbol aus dem MSI-Cache"
        );
        assert!(
            tag.contains("woa.exe"),
            "Verknüpfung {i} zeigt nicht auf woa.exe"
        );
    }
}
