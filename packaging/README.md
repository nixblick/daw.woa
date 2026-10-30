# packaging

MSI, AppImage, Flatpak (M10). Konzept: `docs/konzept.md`.

- `windows/` MSI (WiX), gebaut von `.github/workflows/windows-build.yml`.
- `linux/` AppImage und Flatpak, gebaut von `.github/workflows/linux-build.yml`:
  Startmenü-Eintrag, Metainfo (`render-metainfo.sh` setzt Version und Datum), Symbol,
  `build-appimage.sh`, `AppRun` und `flatpak/de.wironair.woa.yaml`.
  Lokal bauen: `cargo build --release -p woa -p woa-headless -p woa-streamd`, dann
  `packaging/linux/build-appimage.sh dist` (lädt `linuxdeploy` und `appimagetool` in `dist/tools`).
