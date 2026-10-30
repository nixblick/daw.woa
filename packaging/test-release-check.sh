#!/usr/bin/env bash
# Tests für release-check.sh mit kleinen Beispiel-Repos in einem Temp-Ordner. Kein Netz, kein Ton.
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
check="$here/release-check.sh"
real_root="$(cd "$here/.." && pwd)"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
n=0

mkrepo() { # ordner version changelog-version
  mkdir -p "$1/packaging/windows"
  printf '[workspace.package]\nversion = "%s"\n' "$2" > "$1/Cargo.toml"
  printf '# Changelog\n\n## %s — 2026-10-30\n\n- etwas\n\n## 0.0.1 — 2026-01-01\n\n- alt\n' "$3" > "$1/CHANGELOG.md"
  cp "$real_root/packaging/windows/woa.wxs" "$1/packaging/windows/woa.wxs"
}
ok() { n=$((n+1)); echo "ok $n: $1"; }
# erwartet Erfolg
pass() { local what="$1"; shift; WOA_ROOT="$r" "$check" "$@" >/dev/null 2>&1 || { echo "FEHLER: $what sollte klappen" >&2; exit 1; }; ok "$what"; }
# erwartet Fehler
deny() { local what="$1"; shift; if WOA_ROOT="$r" "$check" "$@" >/dev/null 2>&1; then echo "FEHLER: $what sollte scheitern" >&2; exit 1; fi; ok "$what"; }

r="$tmp/gut"; mkrepo "$r" 1.2.3 1.2.3
pass "Tag passt" version v1.2.3
pass "ohne Tag (Probelauf)" version
[ "$(WOA_ROOT="$r" "$check" version v1.2.3)" = "1.2.3" ] && ok "gibt die Version aus"
deny "Tag zu alt" version v1.2.2
deny "Tag mit Zusatz" version v1.2.3-rc1
deny "Tag ohne v" version 1.2.3

r="$tmp/changelog"; mkrepo "$r" 1.2.3 1.2.2
deny "CHANGELOG hinkt hinterher" version v1.2.3

r="$tmp/leer"; mkrepo "$r" 1.2.3 1.2.3
printf '# Changelog\n\n## 1.2.3 — 2026-10-30\n\n## 0.0.1 — 2026-01-01\n\n- alt\n' > "$r/CHANGELOG.md"
deny "leerer CHANGELOG-Eintrag" version v1.2.3

r="$tmp/code"; mkrepo "$r" 1.2.3 1.2.3
sed -i 's/16647BDD-1E47-4AA9-AAED-B1D5D1D4A822/00000000-0000-0000-0000-000000000000/' "$r/packaging/windows/woa.wxs"
deny "geänderter UpgradeCode" version v1.2.3

r="$tmp/gut"
d="$tmp/dist"; mkdir "$d"
deny "leerer Ordner" dist "$d"
for f in woa-1.2.3-x64.msi woa-1.2.3-windows-x64.zip WOA-1.2.3-x86_64.AppImage woa-1.2.3.flatpak; do echo x > "$d/$f"; done
deny "latest.json fehlt" dist "$d"
echo '{}' > "$d/latest.json"
pass "alles da" dist "$d"
: > "$d/woa-1.2.3.flatpak"
deny "leere Datei" dist "$d"
echo y > "$d/woa-1.2.3.flatpak"

pass "SHA256SUMS schreiben" sums "$d"
[ "$(wc -l < "$d/SHA256SUMS")" = 5 ] && ok "5 Zeilen, ohne sich selbst"
( cd "$d" && sha256sum -c --quiet SHA256SUMS ) && ok "Gegenprobe neben den Dateien"
pass "zweiter Lauf gleich (SHA256SUMS zählt nicht mit)" sums "$d"
[ "$(wc -l < "$d/SHA256SUMS")" = 5 ] && ok "weiter 5 Zeilen"
echo z >> "$d/latest.json"
if ( cd "$d" && sha256sum -c --quiet SHA256SUMS ) >/dev/null 2>&1; then echo "FEHLER: Änderung unbemerkt" >&2; exit 1; fi
ok "geänderte Datei fällt auf"
echo "alle $n Tests bestanden"
