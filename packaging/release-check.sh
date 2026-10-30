#!/usr/bin/env bash
# Prüfungen für den Release-Workflow (release.yml), lokal und in der CI gleich.
#   release-check.sh version [TAG]   Tag = Cargo-Version = oberster CHANGELOG-Eintrag, MSI-UpgradeCode fest.
#                                    Ohne TAG wird "v<Cargo-Version>" angenommen (Probelauf). Gibt die Version aus.
#   release-check.sh dist ORDNER     Alle Release-Dateien da, Namen passen zur Version.
#   release-check.sh sums ORDNER     SHA256SUMS schreiben (alle Dateien außer ihr selbst) und gegenprüfen.
# WOA_ROOT überschreibt den Repo-Ordner (für Tests, packaging/test-release-check.sh).
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="${WOA_ROOT:-$(cd "$here/.." && pwd)}"
# Fester MSI-UpgradeCode (packaging/windows/woa.wxs): daran erkennt Windows ältere Versionen.
upgrade_code="16647BDD-1E47-4AA9-AAED-B1D5D1D4A822"

fail() { echo "FEHLER: $*" >&2; exit 1; }

cargo_version() {
  sed -n 's/^version = "\(.*\)"/\1/p' "$root/Cargo.toml" | head -n1
}

cmd_version() {
  local tag="${1:-}" cv top
  cv="$(cargo_version)"
  [[ "$cv" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || fail "Cargo-Version \"$cv\" ist keine Version x.y.z"
  [ -n "$tag" ] || tag="v$cv"
  [[ "$tag" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]] || fail "Tag \"$tag\" hat nicht die Form vX.Y.Z"
  [ "$tag" = "v$cv" ] || fail "Tag $tag passt nicht zur Cargo-Version $cv. Version anheben, Tag neu setzen."
  top="$(sed -n 's/^## \([0-9][0-9.]*\) .*/\1/p' "$root/CHANGELOG.md" | head -n1)"
  [ "$top" = "$cv" ] || fail "oberster CHANGELOG-Eintrag ist \"$top\", Cargo-Version ist $cv"
  # Der Abschnitt darf nicht leer sein: er wird zum Text des Release.
  awk -v v="## $cv " 'index($0, v) == 1 {on=1; next} /^## / {on=0} on && NF {n++} END {exit n ? 0 : 1}' \
    "$root/CHANGELOG.md" || fail "CHANGELOG-Eintrag $cv ist leer"
  grep -q "UpgradeCode=\"$upgrade_code\"" "$root/packaging/windows/woa.wxs" \
    || fail "UpgradeCode in woa.wxs ist nicht $upgrade_code (darf sich nie ändern, sonst ersetzt ein Update die alte Version nicht)"
  echo "$cv"
}

expected_files() { # version
  local v="$1"
  echo "woa-$v-x64.msi"
  echo "woa-$v-windows-x64.zip"
  echo "WOA-$v-x86_64.AppImage"
  echo "woa-$v.flatpak"
  echo "latest.json"
}

cmd_dist() {
  local dir="${1:?Ordner fehlt}" v f missing=0
  v="$(cmd_version "${WOA_TAG:-}")"
  while read -r f; do
    if [ ! -s "$dir/$f" ]; then echo "fehlt oder leer: $dir/$f" >&2; missing=1; fi
  done < <(expected_files "$v")
  [ "$missing" = 0 ] || fail "Release-Dateien unvollständig"
  echo "alle Release-Dateien für $v da"
}

cmd_sums() {
  local dir="${1:?Ordner fehlt}"
  [ -d "$dir" ] || fail "Ordner $dir fehlt"
  ( cd "$dir"
    rm -f SHA256SUMS
    # Nur Dateien, keine Unterordner; Namen ohne Pfad, damit "sha256sum -c" neben den Downloads läuft.
    # Erst sammeln, dann schreiben: sonst fände find die frisch angelegte, leere SHA256SUMS.
    sums="$(find . -maxdepth 1 -type f -printf '%f\n' | LC_ALL=C sort | xargs -d '\n' sha256sum)"
    printf '%s\n' "$sums" > SHA256SUMS
    [ -s SHA256SUMS ] || fail "keine Dateien in $dir"
    sha256sum -c --quiet SHA256SUMS
    cat SHA256SUMS )
}

case "${1:-}" in
  version) shift; cmd_version "$@" ;;
  dist) shift; cmd_dist "$@" ;;
  sums) shift; cmd_sums "$@" ;;
  *) echo "Aufruf: release-check.sh version [TAG] | dist ORDNER | sums ORDNER" >&2; exit 2 ;;
esac
