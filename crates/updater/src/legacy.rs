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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

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
