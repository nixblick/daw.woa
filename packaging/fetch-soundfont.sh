#!/usr/bin/env bash
# Legt die Standard-SoundFont (GeneralUser GS 2.0.3, docs/sounds/README.md) samt Lizenz in einen Ordner.
# Die .sf2 liegt nicht im Repo; sie kommt beim Bau mit festem Commit und fester SHA-256 (#265).
# Aufruf: packaging/fetch-soundfont.sh <ordner>   -> <ordner>/GeneralUser-GS.sf2, GeneralUser-GS-LICENSE.txt
#         packaging/fetch-soundfont.sh <ordner> <bank>   -> Zusatzbank aus docs/sounds/banks.tsv (Paket J7),
#         z. B. musescore-general; dieselben Werte nutzt die App beim ersten Gebrauch.
# Neue Version: Commit, SHA-256 hier UND in packaging/linux/flatpak/de.wironair.woa.yaml gemeinsam erneuern.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/.." && pwd)"
dest="${1:?Aufruf: fetch-soundfont.sh <ordner>}"
commit="684543d5e5efaef08d02be50dcda8d552478fa60"
url="https://raw.githubusercontent.com/mrbumpy409/GeneralUser-GS/$commit/GeneralUser-GS.sf2"
sha256="9575028c7a1f589f5770fccc8cff2734566af40cd26ed836944e9a5152688cfe"

bank="${2:-}"
if [ -n "$bank" ]; then
  line="$(grep -v '^#' "$root/docs/sounds/banks.tsv" | awk -F'\t' -v id="$bank" '$1 == id')"
  [ -n "$line" ] || { echo "Bank unbekannt: $bank" >&2; exit 1; }
  IFS=$'\t' read -r _ file _ sha256 url license <<<"$line"
  case "$url" in https://*) ;; *) echo "Nur https: $url" >&2; exit 1 ;; esac
fi

mkdir -p "$dest"
if [ -n "$bank" ]; then
  out="$dest/$file"
  [ -f "$out" ] || curl -fsSL --proto '=https' --retry 3 "$url" -o "$out.part"
  [ -f "$out" ] || mv "$out.part" "$out"
  echo "$sha256  $out" | sha256sum -c --quiet - \
    || { echo "Prüfsumme falsch: $out (erwartet $sha256)" >&2; rm -f "$out"; exit 1; }
  cp "$root/docs/sounds/$license" "$dest/$license"
  echo "$out"
  exit 0
fi
sf2="$dest/GeneralUser-GS.sf2"
[ -f "$sf2" ] || curl -fsSL --retry 3 "$url" -o "$sf2"
echo "$sha256  $sf2" | sha256sum -c --quiet - \
  || { echo "Prüfsumme falsch: $sf2 (erwartet $sha256)" >&2; rm -f "$sf2"; exit 1; }
cp "$root/docs/sounds/GeneralUser-GS-LICENSE.txt" "$dest/GeneralUser-GS-LICENSE.txt"
echo "$sf2"
