//! Erlaubte Adressen. Alles andere wird abgelehnt, auch über Weiterleitungen.
//!
//! Start: nur `https://github.com/nixblick/daw.woa/releases/…`. GitHub leitet Downloads auf
//! seine Asset-Hosts um; nur diese beiden sind als Ziel einer Weiterleitung erlaubt.

const START_HOST: &str = "github.com";
const START_PATH: &str = "/nixblick/daw.woa/releases/";
const REDIRECT_HOSTS: [&str; 2] = [
    "objects.githubusercontent.com",
    "release-assets.githubusercontent.com",
];

/// Zerlegt `https://host/pfad` streng. Kein Benutzerteil, kein Port, keine Leerzeichen.
fn split(url: &str) -> Option<(&str, &str)> {
    let rest = url.strip_prefix("https://")?;
    let (host, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    let host_ok = !host.is_empty()
        && host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-');
    let path_ok = !url.chars().any(|c| c.is_whitespace() || c.is_control())
        && !path.contains('\\')
        && !path
            .split(['/', '?', '#'])
            .any(|s| s == ".." || s.eq_ignore_ascii_case("%2e%2e"));
    (host_ok && path_ok).then_some((host, path))
}

/// Darf diese Adresse aus dem Manifest kommen (erster Schritt)?
pub fn start_allowed(url: &str) -> bool {
    matches!(split(url), Some((START_HOST, p)) if p.starts_with(START_PATH))
}

/// Darf eine Weiterleitung hierhin führen?
pub fn redirect_allowed(url: &str) -> bool {
    match split(url) {
        Some((h, _)) if REDIRECT_HOSTS.contains(&h) => true,
        _ => start_allowed(url),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_only_on_our_release_path() {
        assert!(start_allowed(
            "https://github.com/nixblick/daw.woa/releases/latest/download/latest.json"
        ));
        assert!(start_allowed(
            "https://github.com/nixblick/daw.woa/releases/download/v1.0.0/a.msi"
        ));
        for bad in [
            "http://github.com/nixblick/daw.woa/releases/x",
            "https://github.com/other/repo/releases/x",
            "https://github.com/nixblick/daw.woa/issues/1",
            "https://github.com.evil.org/nixblick/daw.woa/releases/x",
            "https://github.com@evil.org/nixblick/daw.woa/releases/x",
            "https://github.com:444/nixblick/daw.woa/releases/x",
            "https://evil.org/nixblick/daw.woa/releases/x",
            "https://github.com/nixblick/daw.woa/releases/../../evil/x",
            "https://objects.githubusercontent.com/x",
            "https://github.com/nixblick/daw.woa/releases/a b",
            "file:///etc/passwd",
            "",
        ] {
            assert!(!start_allowed(bad), "{bad}");
        }
    }

    #[test]
    fn redirects_only_to_github_asset_hosts() {
        assert!(redirect_allowed(
            "https://objects.githubusercontent.com/github-production/x?y=1"
        ));
        assert!(redirect_allowed(
            "https://release-assets.githubusercontent.com/a/b"
        ));
        assert!(redirect_allowed(
            "https://github.com/nixblick/daw.woa/releases/download/v1/a"
        ));
        for bad in [
            "https://raw.githubusercontent.com/x",
            "https://githubusercontent.com/x",
            "https://objects.githubusercontent.com.evil.org/x",
            "https://evil.org/objects.githubusercontent.com",
            "http://objects.githubusercontent.com/x",
        ] {
            assert!(!redirect_allowed(bad), "{bad}");
        }
    }
}
