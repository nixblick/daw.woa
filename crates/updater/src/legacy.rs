//! Umstieg von der Installation für alle Benutzer auf die pro Benutzer (#452).
//!
//! Bis 0.41.25 installierte das MSI für alle Benutzer (`C:\Program Files\WOA`, HKLM). Seither
//! installiert es pro Benutzer (`%LOCALAPPDATA%\Programs\WOA`, HKCU), ohne Admin-Abfrage.
//! Windows Installer ersetzt über diese Grenze hinweg nichts per MajorUpgrade: `FindRelatedProducts`
//! sucht nur im eigenen Kontext. Darum entfernt die neue App beim ersten Start die alte
//! Installation einmal selbst (`msiexec /x {ProductCode} /passive`); das ist die einzige
//! Admin-Abfrage beim Umstieg. Nutzerdaten (`%APPDATA%\woa`, `Music\WOA`) gehören keiner
//! MSI-Komponente und bleiben unberührt.

/// Name im Eintrag unter „Apps“ (`Package Name` in `woa.wxs`).
pub const PRODUCT_NAME: &str = "WOA - Wir on Air";

/// Merkt, dass der Umstieg für diese Version schon einmal angestoßen wurde (im Datenordner):
/// lehnt der Nutzer die Abfrage ab, fragt WOA erst mit der nächsten Version wieder.
pub const MARKER_FILE: &str = "altinstallation-entfernt.txt";

/// Liest die ProductCodes der Installation für alle Benutzer aus der Ausgabe von
/// `reg query HKLM\...\Uninstall /s /f "WOA - Wir on Air" /d /e`. Nur Schlüssel, deren
/// `DisplayName` genau [`PRODUCT_NAME`] ist und deren Name eine GUID in `{}` ist.
pub fn product_codes(reg_output: &str) -> Vec<String> {
    let mut codes = Vec::new();
    let mut key: Option<String> = None;
    for line in reg_output.lines() {
        let line = line.trim_end();
        if line.starts_with("HKEY_") {
            key = line
                .rsplit('\\')
                .next()
                .filter(|k| is_guid(k))
                .map(str::to_owned);
            continue;
        }
        let mut parts = line.split_whitespace();
        if parts.next() == Some("DisplayName") && parts.next() == Some("REG_SZ") {
            let value: Vec<&str> = parts.collect();
            if value.join(" ") == PRODUCT_NAME
                && let Some(k) = key.take()
                && !codes.contains(&k)
            {
                codes.push(k);
            }
        }
    }
    codes
}

/// Schlüssel, unter dem Windows Installer ein Produkt **für alle Benutzer** führt:
/// `HKLM\SOFTWARE\Classes\Installer\Products\<gepackte GUID>`. Der Uninstall-Zweig unter HKLM
/// listet auch Installationen pro Benutzer (Release-Lauf v0.41.30: zwei Treffer, die alte und die
/// neue); ohne diesen Filter entfernte die App sich selbst. Gepackt: die ersten drei Gruppen
/// rückwärts, danach je zwei Zeichen getauscht.
pub fn per_machine_key(code: &str) -> Option<String> {
    if !is_guid(code) {
        return None;
    }
    let hex: Vec<char> = code.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    let mut packed = String::with_capacity(32);
    for (start, len) in [(0, 8), (8, 4), (12, 4)] {
        packed.extend(hex[start..start + len].iter().rev());
    }
    for pair in hex[16..].chunks(2) {
        packed.push(pair[1]);
        packed.push(pair[0]);
    }
    Some(format!(
        r"HKLM\SOFTWARE\Classes\Installer\Products\{}",
        packed.to_ascii_uppercase()
    ))
}

fn is_guid(s: &str) -> bool {
    let Some(inner) = s.strip_prefix('{').and_then(|r| r.strip_suffix('}')) else {
        return false;
    };
    let groups: Vec<&str> = inner.split('-').collect();
    groups.iter().map(|g| g.len()).eq([8, 4, 4, 4, 12])
        && groups
            .iter()
            .all(|g| g.chars().all(|c| c.is_ascii_hexdigit()))
}

/// Ob die laufende App aus der Installation pro Benutzer stammt (nur dann wird die alte
/// abgelöst; ein Start aus `Program Files` oder aus dem ZIP lässt alles, wie es ist).
pub fn runs_per_user(exe: &std::path::Path, local_app_data: &std::path::Path) -> bool {
    exe.starts_with(local_app_data.join("Programs").join("WOA"))
}

/// Beim Start (nur Windows): läuft WOA pro Benutzer und gibt es noch die Installation für alle
/// Benutzer, wird sie einmal je Version per `msiexec /x` entfernt. Windows fragt dabei einmal
/// nach Admin-Rechten. Kehrt sofort zurück; Fehler werden nur geloggt.
#[cfg(windows)]
pub fn retire_per_machine(data_dir: &std::path::Path) {
    use std::process::Command;
    let (Ok(exe), Some(lad)) = (std::env::current_exe(), std::env::var_os("LOCALAPPDATA")) else {
        return;
    };
    if !runs_per_user(&exe, std::path::Path::new(&lad)) {
        return;
    }
    let marker = data_dir.join(MARKER_FILE);
    let version = env!("CARGO_PKG_VERSION");
    if std::fs::read_to_string(&marker).is_ok_and(|v| v.trim() == version) {
        return;
    }
    let out = match Command::new("reg")
        .args([
            "query",
            r"HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall",
            "/s",
            "/f",
            PRODUCT_NAME,
            "/d",
            "/e",
            "/reg:64",
        ])
        .output()
    {
        Ok(o) => o,
        Err(e) => {
            tracing::warn!(error = %e, "reg query failed");
            return;
        }
    };
    let codes: Vec<String> = product_codes(&String::from_utf8_lossy(&out.stdout))
        .into_iter()
        .filter(|code| {
            per_machine_key(code).is_some_and(|key| {
                Command::new("reg")
                    .args(["query", key.as_str(), "/reg:64"])
                    .output()
                    .is_ok_and(|o| o.status.success())
            })
        })
        .collect();
    if codes.is_empty() {
        return;
    }
    let _ = std::fs::create_dir_all(data_dir);
    if let Err(e) = std::fs::write(&marker, version) {
        tracing::warn!(error = %e, "marker not written");
    }
    for code in codes {
        tracing::info!(%code, "removing per-machine installation");
        if let Err(e) = Command::new("msiexec")
            .args(["/x", code.as_str(), "/passive", "/norestart"])
            .spawn()
        {
            tracing::warn!(error = %e, "msiexec /x not started");
        }
    }
}

/// Ob `command` die `woa-headless.exe` der alten Installation für alle Benutzer ist.
fn is_legacy_headless(command: &str) -> bool {
    let norm = command.replace('\\', "/").to_ascii_lowercase();
    norm.contains("/program files/woa/woa-headless")
        || norm.contains("/program files (x86)/woa/woa-headless")
}

fn collect_legacy_commands(v: &serde_json::Value, out: &mut Vec<String>) {
    match v {
        serde_json::Value::Object(map) => {
            if let Some(cmd) = map
                .get("mcpServers")
                .and_then(|s| s.get("woa"))
                .and_then(|w| w.get("command"))
                .and_then(|c| c.as_str())
                && is_legacy_headless(cmd)
                && !out.iter().any(|o| o == cmd)
            {
                out.push(cmd.to_owned());
            }
            map.values().for_each(|c| collect_legacy_commands(c, out));
        }
        serde_json::Value::Array(a) => a.iter().for_each(|c| collect_legacy_commands(c, out)),
        _ => {}
    }
}

/// Setzt in `text` (`.claude.json` bzw. `claude_desktop_config.json`) den `command` des Eintrags
/// `mcpServers.woa`, der noch auf `Program Files\WOA\woa-headless.exe` zeigt, auf `new_exe`.
/// Andere Einträge und die Formatierung bleiben unberührt (nur der Textwert wird ersetzt).
/// `None`: nichts zu tun oder kein gültiges JSON.
pub fn repoint_mcp_command(text: &str, new_exe: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(text).ok()?;
    let mut old = Vec::new();
    collect_legacy_commands(&v, &mut old);
    if old.is_empty() {
        return None;
    }
    let new = serde_json::to_string(new_exe).ok()?;
    let mut out = text.to_owned();
    for o in old {
        out = out.replace(&serde_json::to_string(&o).ok()?, &new);
    }
    (out != text).then_some(out)
}

/// Schreibt in den genannten Dateien die veraltete Registrierung um (Sicherung daneben:
/// `<datei>.bak-woa`). Gibt die geänderten Dateien zurück; fehlende Dateien sind kein Fehler.
pub fn repair_mcp_registrations(
    files: &[std::path::PathBuf],
    headless: &std::path::Path,
) -> Vec<std::path::PathBuf> {
    let new_exe = headless.to_string_lossy();
    let mut changed = Vec::new();
    for file in files {
        let Ok(text) = std::fs::read_to_string(file) else {
            continue;
        };
        let Some(fixed) = repoint_mcp_command(&text, &new_exe) else {
            continue;
        };
        let mut bak = file.as_os_str().to_owned();
        bak.push(".bak-woa");
        if let Err(e) = std::fs::write(&bak, &text) {
            tracing::warn!(error = %e, file = %file.display(), "mcp registration backup failed");
            continue;
        }
        match std::fs::write(file, fixed) {
            Ok(()) => {
                tracing::info!(file = %file.display(), command = %new_exe, "mcp registration updated");
                changed.push(file.clone());
            }
            Err(e) => {
                tracing::warn!(error = %e, file = %file.display(), "mcp registration not written");
            }
        }
    }
    changed
}

/// Beim Start (nur Windows): läuft WOA pro Benutzer, zeigt eine noch vorhandene Registrierung von
/// Claude (`~/.claude.json`, `claude_desktop_config.json`) aber auf `Program Files`, wird sie auf die
/// `woa-headless.exe` neben der laufenden App umgeschrieben. Gibt `true` zurück, wenn etwas geändert
/// wurde (Claude muss dann neu verbinden).
#[cfg(windows)]
pub fn repair_claude_registration() -> bool {
    use std::path::PathBuf;
    let (Ok(exe), Some(lad)) = (std::env::current_exe(), std::env::var_os("LOCALAPPDATA")) else {
        return false;
    };
    if !runs_per_user(&exe, std::path::Path::new(&lad)) {
        return false;
    }
    let Some(headless) = exe.parent().map(|d| d.join("woa-headless.exe")) else {
        return false;
    };
    if !headless.is_file() {
        return false;
    }
    let mut files = Vec::new();
    if let Some(p) = std::env::var_os("USERPROFILE") {
        files.push(PathBuf::from(p).join(".claude.json"));
    }
    if let Some(a) = std::env::var_os("APPDATA") {
        files.push(
            PathBuf::from(a)
                .join("Claude")
                .join("claude_desktop_config.json"),
        );
    }
    !repair_mcp_registrations(&files, &headless).is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    const OLD_CFG: &str = r#"{
  "numStartups": 3,
  "mcpServers": {
    "woa": { "type": "stdio", "command": "C:\\Program Files\\WOA\\woa-headless.exe", "args": ["mcp", "C:\\Users\\a\\Music\\Claude.woa"] },
    "other": { "command": "C:\\Program Files\\WOA\\woa-headless.exe" }
  }
}"#;
    const NEW_EXE: &str = r"C:\Users\a\AppData\Local\Programs\WOA\woa-headless.exe";

    #[test]
    fn registration_on_program_files_is_repointed() {
        let fixed = repoint_mcp_command(OLD_CFG, NEW_EXE).expect("must change");
        let v: serde_json::Value = serde_json::from_str(&fixed).unwrap();
        assert_eq!(v["mcpServers"]["woa"]["command"], NEW_EXE);
        assert_eq!(v["mcpServers"]["woa"]["args"][0], "mcp");
        assert_eq!(v["numStartups"], 3);
        // Wieder aufgerufen: nichts mehr zu tun.
        assert!(repoint_mcp_command(&fixed, NEW_EXE).is_none());
    }

    #[test]
    fn current_or_foreign_registrations_stay() {
        let cur = format!(
            r#"{{"mcpServers":{{"woa":{{"command":{}}}}}}}"#,
            serde_json::to_string(NEW_EXE).unwrap()
        );
        assert!(repoint_mcp_command(&cur, NEW_EXE).is_none());
        let foreign =
            r#"{"mcpServers":{"x":{"command":"C:\\Program Files\\WOA\\woa-headless.exe"}}}"#;
        assert!(repoint_mcp_command(foreign, NEW_EXE).is_none());
        assert!(repoint_mcp_command("kein json", NEW_EXE).is_none());
    }

    #[test]
    fn project_scoped_entries_are_found_too() {
        let t = r#"{"projects":{"C:/p":{"mcpServers":{"woa":{"command":"C:\\Program Files\\WOA\\woa-headless.exe"}}}}}"#;
        assert!(repoint_mcp_command(t, NEW_EXE).is_some());
    }

    #[test]
    fn files_are_rewritten_with_backup() {
        let dir = std::env::temp_dir().join(format!("woa-mcp-reg-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join(".claude.json");
        std::fs::write(&f, OLD_CFG).unwrap();
        let missing = dir.join("fehlt.json");
        let changed = repair_mcp_registrations(&[missing, f.clone()], Path::new(NEW_EXE));
        assert_eq!(changed, vec![f.clone()]);
        assert!(std::fs::read_to_string(&f).unwrap().contains("AppData"));
        assert_eq!(
            std::fs::read_to_string(dir.join(".claude.json.bak-woa")).unwrap(),
            OLD_CFG
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    const REG: &str = r"
HKEY_LOCAL_MACHINE\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\{0A1B2C3D-1111-2222-3333-444455556666}
    DisplayName    REG_SZ    WOA - Wir on Air

HKEY_LOCAL_MACHINE\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\WOA-Fremd
    DisplayName    REG_SZ    WOA - Wir on Air

HKEY_LOCAL_MACHINE\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\{FFFFFFFF-1111-2222-3333-444455556666}
    DisplayName    REG_SZ    WOA - Wir on Air Tools

End of search: 3 match(es) found.
";

    #[test]
    fn finds_only_exact_msi_entries() {
        assert_eq!(
            product_codes(REG),
            vec!["{0A1B2C3D-1111-2222-3333-444455556666}".to_string()]
        );
        assert!(product_codes("End of search: 0 match(es) found.").is_empty());
    }

    #[test]
    fn per_machine_key_packs_the_guid() {
        // Beispiel aus dem Release-Lauf v0.41.30.
        assert_eq!(
            per_machine_key("{2D9A1243-C50F-4ACA-BF27-273DCC970AA7}").as_deref(),
            Some(r"HKLM\SOFTWARE\Classes\Installer\Products\3421A9D2F05CACA4FB7272D3CC79A07A")
        );
        assert_eq!(per_machine_key("WOA-Fremd"), None);
    }

    #[test]
    fn per_user_only_from_local_app_data() {
        let lad = Path::new(r"C:\Users\a\AppData\Local");
        assert!(runs_per_user(
            &lad.join("Programs").join("WOA").join("woa.exe"),
            lad
        ));
        assert!(!runs_per_user(
            Path::new(r"C:\Program Files\WOA\woa.exe"),
            lad
        ));
        assert!(!runs_per_user(&lad.join("Temp").join("woa.exe"), lad));
    }
}
