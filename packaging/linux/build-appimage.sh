#!/usr/bin/env bash
# Baut WOA-<version>-x86_64.AppImage aus den fertigen Release-Programmen (cargo build --release).
# Muster nach LMMS (cmake/linux/LinuxDeploy.cmake, GPL-2.0-or-later): AppDir -> linuxdeploy -> appimagetool.
# Nur die Schritte, kein Code übernommen (docs/herkunft.md).
# Aufruf aus dem Repo-Wurzelordner: packaging/linux/build-appimage.sh [Ausgabeordner]
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/../.." && pwd)"
out="$(realpath -m "${1:-$root/dist}")"
bin="${CARGO_TARGET_DIR:-$root/target}/release"
tools="${WOA_TOOLS_DIR:-$out/tools}"
arch="x86_64"
# Feste Versionen, damit ein Bau nicht von "continuous" abhängt.
linuxdeploy_url="https://github.com/linuxdeploy/linuxdeploy/releases/download/1-alpha-20250213-2/linuxdeploy-$arch.AppImage"
appimagetool_url="https://github.com/AppImage/appimagetool/releases/download/1.9.1/appimagetool-$arch.AppImage"

mkdir -p "$out" "$tools"
for f in woa woa-headless woa-streamd; do
  [ -x "$bin/$f" ] || { echo "fehlt: $bin/$f (erst cargo build --release)" >&2; exit 1; }
done

fetch() { # url datei
  [ -x "$2" ] || { curl -fsSL "$1" -o "$2" && chmod +x "$2"; }
}
fetch "$linuxdeploy_url" "$tools/linuxdeploy"
fetch "$appimagetool_url" "$tools/appimagetool"

appdir="$out/WOA.AppDir"
rm -rf "$appdir"
mkdir -p "$appdir/usr/bin" "$appdir/usr/share/applications" "$appdir/usr/share/metainfo" \
  "$appdir/usr/share/icons/hicolor/256x256/apps" "$appdir/usr/share/icons/hicolor/scalable/apps"
install -m755 "$bin/woa" "$bin/woa-headless" "$bin/woa-streamd" "$appdir/usr/bin/"
install -m644 "$here/de.wironair.woa.desktop" "$appdir/usr/share/applications/"
install -m644 "$here/icons/de.wironair.woa.png" "$appdir/usr/share/icons/hicolor/256x256/apps/"
install -m644 "$here/icons/de.wironair.woa.svg" "$appdir/usr/share/icons/hicolor/scalable/apps/"
version="$("$here/render-metainfo.sh" "$appdir/usr/share/metainfo/de.wironair.woa.appdata.xml")"
install -m644 "$root/LICENSE" "$appdir/usr/share/doc-LICENSE" 2>/dev/null || true
echo "X-AppImage-Version=$version" >> "$appdir/usr/share/applications/de.wironair.woa.desktop"

# Die AppImage-Werkzeuge sind selbst AppImages; ohne FUSE (CI) entpackt sie sich selbst.
export APPIMAGE_EXTRACT_AND_RUN=1
export DISABLE_COPYRIGHT_FILES_DEPLOYMENT=1
export ARCH="$arch"

# linuxdeploy sammelt die Bibliotheken von woa und woa-headless (libasound u. a.) in den AppDir.
# Die Programme laden OpenGL/Wayland/X11 des Systems zur Laufzeit selbst.
"$tools/linuxdeploy" --appdir "$appdir" \
  --executable "$appdir/usr/bin/woa" \
  --executable "$appdir/usr/bin/woa-headless" \
  --executable "$appdir/usr/bin/woa-streamd" \
  --desktop-file "$appdir/usr/share/applications/de.wironair.woa.desktop" \
  --icon-file "$here/icons/de.wironair.woa.png" \
  --custom-apprun "$here/AppRun"

# Harte Kopie als .DirIcon statt Verweis (LMMS: Cinnamon mag keine Links).
cp "$here/icons/de.wironair.woa.png" "$appdir/.DirIcon"

target="$out/WOA-$version-$arch.AppImage"
rm -f "$target"
"$tools/appimagetool" "$appdir" "$target"
echo "$target"
