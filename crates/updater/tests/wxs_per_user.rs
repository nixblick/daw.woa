//! #452: Das MSI installiert pro Benutzer, ohne Admin-Abfrage. Prüft `packaging/windows/woa.wxs`
//! als Text (WiX selbst gibt es nur im Windows-Runner).
//!
//! Rotprobe vor dem Umbau (04.11.2026, Scope="perMachine"):
//!   thread 'msi_installs_per_user' panicked: Scope="perUser" fehlt
//!   thread 'msi_writes_no_machine_wide_state' panicked: ProgramFiles64Folder
//!   test result: FAILED. 1 passed; 2 failed

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
            .and_then(|s| s.split('"').next());
        assert!(
            matches!(guid, Some(g) if g.len() == 36 && g != "*"),
            "Komponente {id} ohne feste Guid"
        );
    }
}
