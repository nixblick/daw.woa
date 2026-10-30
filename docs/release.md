# Release

Ein Release besteht aus fünf Dateien plus Prüfsummen, alle am selben GitHub-Release:

| Datei | Was | Wofür |
|---|---|---|
| `woa-<v>-x64.msi` | Installer für Windows | Installation und Auto-Update |
| `woa-<v>-windows-x64.zip` | Programme zum Auspacken | ohne Installation |
| `WOA-<v>-x86_64.AppImage` | Linux, eine Datei | auf den meisten Distributionen |
| `woa-<v>.flatpak` | Linux, Flatpak-Paket | Installation mit `flatpak install --user` |
| `latest.json` | Version, Neuerungen, MSI-Adresse und -Prüfsumme | die App fragt danach (`docs/updater.md`) |
| `SHA256SUMS` | SHA-256 aller Dateien oben | `sha256sum -c SHA256SUMS` neben den Downloads |

Bei einem signierten Release kommt `woa-<v>-x64.msi.minisig` dazu.

## Ablauf

1. Version in `Cargo.toml` anheben, `CHANGELOG.md` oben mit demselben Versionsabschnitt
   (`## x.y.z — Datum`, nicht leer), in `main` mergen.
2. Tag `vx.y.z` auf diesen Stand von `main` setzen und pushen: `git tag vx.y.z && git push origin vx.y.z`.
3. Der Workflow `.github/workflows/release.yml` läuft:
   - **pruefen:** Tag = Cargo-Version = oberster CHANGELOG-Eintrag, fester MSI-UpgradeCode, Tag liegt
     in `main` (`packaging/release-check.sh version`; Test: `packaging/test-release-check.sh`).
   - **windows:** `windows-build.yml`, baut MSI und ZIP und spielt ein Update durch (siehe unten).
   - **linux:** `linux-build.yml`, baut AppImage und Flatpak und startet sie ohne Fenster und Ton.
   - **paket:** signiert das MSI (wenn das Secret da ist), schreibt `latest.json` und `SHA256SUMS`,
     prüft, dass alle Dateien da sind, und legt ein Release als **Entwurf** an.
4. Der Entwurf ist nur für Maintainer sichtbar. Dateien ansehen, dann **Veröffentlichen**: Erst dann
   sieht die App das Update (`releases/latest` kennt keine Entwürfe).

Ist ein Tag falsch gesetzt, bricht der Lauf in „pruefen“ ab, bevor etwas gebaut wird. Dann Version oder
CHANGELOG korrigieren, mergen, den Tag auf dem neuen Stand neu setzen.

## Probelauf ohne Tag

Actions → Release → Run workflow (oder `gh workflow run release.yml`). Es läuft alles wie oben, aber es
wird kein Release angelegt; der fertige Ordner hängt als Artefakt `release-probe` am Lauf. Dasselbe
passiert bei Änderungen an `release.yml` und am Prüfskript im Pull Request.

## MSI-Update-Probe

`windows-build.yml` baut nach dem MSI aus denselben Programmen ein MSI mit Version 0.0.1 und führt auf
dem Windows-Runner `packaging/windows/test-upgrade.ps1` aus: alt installieren, neu darüber (genau ein
Eintrag, neue Version, alle Dateien da), Downgrade wird abgelehnt (Fehler 1638), Deinstallation räumt
auf. Die App wird dabei nicht gestartet. Der **UpgradeCode** in `packaging/windows/woa.wxs` ist fest und
darf sich nie ändern; `release-check.sh` prüft ihn bei jedem Tag.

## Datenschutzerklärung und Download-Seite

Die Datenschutzerklärung (Telemetrie nur mit Zustimmung, Inhalt: `docs/telemetrie.md`, Abschnitt
„Datenschutz“) und die Download-Seite stehen auf wironair.de. Beides ist Sache von André, siehe die
offenen Fragen im Pull Request: Der Workflow veröffentlicht nichts außerhalb von GitHub.

## Nicht eingebaut (braucht Andrés Entscheidung)

- Windows-Codesigning-Zertifikat (kostenpflichtig; ohne fragt SmartScreen bei der Handinstallation nach).
- Signatur von AppImage und Flatpak, Veröffentlichung auf Flathub, Download-Seite auf wironair.de.
- minisign-Schlüssel für Stufe B des Updaters (`docs/updater.md`).
