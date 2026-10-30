#!/usr/bin/env bash
# Schreibt die Metainfo mit Version (aus Cargo.toml) und Datum (erste Überschrift im CHANGELOG).
# Aufruf: packaging/linux/render-metainfo.sh <Ziel-Datei>; gibt die Version aus.
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/../.." && pwd)"
out="${1:?Aufruf: render-metainfo.sh <Ziel-Datei>}"
version="$(sed -n 's/^version = "\(.*\)"/\1/p' "$root/Cargo.toml" | head -n1)"
date="$(sed -n 's/^## [0-9][0-9.]* — \([0-9-]*\).*/\1/p' "$root/CHANGELOG.md" | head -n1)"
if [ -z "$version" ] || [ -z "$date" ]; then
  echo "Version oder Datum nicht gefunden" >&2
  exit 1
fi
mkdir -p "$(dirname "$out")"
sed -e "s/@VERSION@/$version/" -e "s/@DATE@/$date/" "$here/de.wironair.woa.metainfo.xml" > "$out"
echo "$version"
