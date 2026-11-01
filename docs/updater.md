# Auto-Update per MSI

WOA prüft beim Start (höchstens einmal am Tag), ob GitHub ein neueres, **veröffentlichtes**
Release hat. Gibt es eines, zeigt ein Streifen unter dem Menü „Update x.y.z verfügbar – Jetzt
installieren“ mit den Neuerungen. Unter Windows lädt ein Klick das MSI, prüft es, startet den
Installer und beendet die App; das MSI startet die neue Version nach der Installation. Unter
Linux und macOS gibt es nur den Hinweis mit Link zur Release-Seite.

Ausschalten: Einstellungen → „Beim Start nach Updates suchen“, oder `"auto_update": false` in
`config.json`. „Jetzt nach Updates suchen“ im selben Menü fragt sofort (ohne Tageslimit).

Code: `crates/updater` (Prüfung, Netz), `crates/gui/src/update.rs` und `update_*` in
`crates/gui/src/app.rs` (Anzeige), `packaging/windows/woa.wxs` (Neustart),
`.github/workflows/release.yml` (Bau, Signatur, Entwurf; Ablauf: `docs/release.md`).

## Quelle und Manifest

- `https://github.com/nixblick/daw.woa/releases/latest/download/latest.json`. „latest“ sind nur
  veröffentlichte Releases, keine Entwürfe und keine Vorabversionen. Ein Release wird also erst
  sichtbar, wenn es veröffentlicht ist.
- `latest.json`: `{version, notes, msi_url, msi_sha256, msi_sig_url}`. Ohne `msi_sig_url` lehnt die App ab.
- Die App lädt nur von `https://github.com/nixblick/daw.woa/releases/…`. Weiterleitungen folgt
  sie selbst und nur auf `objects.githubusercontent.com` und `release-assets.githubusercontent.com`.
  Alles andere (anderer Host, anderes Repo, `http`, `..` im Pfad) wird abgelehnt.
- Der Download läuft in einem Hintergrund-Thread mit Zeitlimit und Größengrenze, nie im
  UI-Thread.

## Signatur ist Pflicht

Die Datei `packaging/windows/updater.pub` mit dem öffentlichen minisign-Schlüssel wird zur
Bauzeit per `include_str!` eingebrannt. Ein Update wird nur installiert, wenn die minisign-Signatur
über die MSI-Bytes zu diesem Schlüssel passt. Fehlt die Signatur oder ist der Schlüssel kein
gültiger minisign-Schlüssel, wird abgelehnt (fail-closed); eine Prüfsumme allein genügt nie.

Zusätzlich gilt: nur strikt neuere Version (gleiche, ältere und unlesbare Versionen sind ein
Veto), nur HTTPS mit Zertifikatsprüfung, nur die festen GitHub-Hosts. Der signierte,
vertrauenswürdige Kommentar muss `woa <version>` heißen, und die Version muss der
Manifest-Version entsprechen. So kann niemand mit einer gefälschten Versionsangabe ein altes,
echt signiertes MSI unterschieben. Steht im Manifest zusätzlich eine SHA-256, muss auch sie stimmen.

## Bedrohungsmodell, ehrlich

Angreifer kann beliebige Bytes als Manifest und MSI liefern (DNS, WLAN, Proxy) oder die
Release-Dateien auf GitHub ändern.

- **Netzwerk-Angreifer (DNS, MITM, fremde WLANs):** In beiden Stufen abgewehrt. Ohne gültiges
  Zertifikat für `github.com` kommt nichts an, und andere Hosts nimmt die App nicht.
- **Beschädigter oder abgeschnittener Download:** In beiden Stufen abgefangen.
- **Zurückgespieltes altes Release:** Der Downgrade-Wächter lehnt es ab.
- **Kompromittierung der Release-Dateien auf GitHub:** abgewehrt. Der private Schlüssel liegt
  nur als GitHub-Secret in der CI und auf Andrés Rechner. Wer nur Release-Dateien ändern kann,
  kann keine gültige Signatur erzeugen. Restrisiko: Zugriff auf Secrets/Workflows des Repos.
- **Übergang:** Apps vor 0.35.4 wurden ohne Schlüssel gebaut und prüfen nur die SHA-256. Sie
  aktualisieren sich noch einmal auf ein signiertes Release; erst Versionen mit Schlüssel erzwingen
  Signaturen.
- Das MSI ist (noch) nicht mit einem Windows-Codesigning-Zertifikat signiert; SmartScreen kann
  bei einer Handinstallation nachfragen. Das ist unabhängig von der minisign-Signatur.

## Neustart nach der Installation

Gewählt: **eine WiX-Custom-Action im MSI** (`WoaNachUpdateStarten` in `woa.wxs`). Die App
startet `msiexec /i <datei> /passive /norestart RESTARTWOA=1` und beendet sich.

Begründung:

- Das MSI weiß als Einziges sicher, wann die Installation **erfolgreich** fertig ist, und
  kennt den Installationspfad aus der eigenen Dateitabelle. Ein `cmd /c`-Nachlauf müsste
  raten (Wartezeit) oder den Pfad und das Ende von `msiexec` selbst bestimmen, samt
  Anführungszeichen-Regeln von `cmd`.
- Sie hängt an der Eigenschaft `RESTARTWOA=1`: Ein Doppelklick auf das MSI oder eine
  Stille Installation startet nichts. Bei Deinstallation läuft sie nie.
- Sie läuft im Benutzerkontext des Aufrufers, die neue App läuft also **nicht** mit
  Administratorrechten.

Nachteil: Bricht der Nutzer die Installation ab (UAC abgelehnt), ist die App bereits beendet
und startet nicht von selbst neu. Dann einfach über das Startmenü starten; das Update wird beim
nächsten Start wieder angeboten.

Ungespeichertes Projekt: Die App fragt vor dem Herunterladen („Speichern und installieren“ oder
„Abbrechen“). Gelingt das Speichern nicht, wird nicht installiert.

## Release bauen

Ein Tag `v<version>` (Version = `Cargo.toml`, sonst bricht der Workflow ab) löst
`.github/workflows/release.yml` aus: Windows-Bau (derselbe wie `windows-build.yml`), danach in
einem Ubuntu-Job signieren, `latest.json` schreiben und ein Release als **Entwurf** anlegen
(MSI, ZIP, `.minisig`, `latest.json`). Die Neuerungen kommen aus dem `CHANGELOG.md`. Erst das
Veröffentlichen des Entwurfs macht das Update für die App sichtbar.

Ohne das Secret `WOA_UPDATER_KEY` bricht der Workflow bei einem Tag ab (im Probelauf nur eine
Warnung), ebenso wenn `updater.pub` kein minisign-Schlüssel ist: Keine App würde ein
unsigniertes Update annehmen.

## Erste Version mit Updater

Die erste Version, die den Updater enthält, muss einmal **von Hand** installiert werden (MSI aus
dem Windows-Bau oder dem Release doppelklicken). Erst von da an aktualisiert sie sich selbst.

## Schlüssel erzeugen (einmalig, erledigt)

Stand 01.11.2026: erledigt (#136), seit v0.35.4 sind Releases signiert; der Rückfall auf die
Prüfsumme ist mit 0.35.11 aus dem Code entfernt.

1. `minisign -G -W -p updater.pub -s updater.key` (`-W`: ohne Passwort, der Workflow signiert unbeaufsichtigt).
2. Inhalt von `updater.key` als Actions-Secret `WOA_UPDATER_KEY` setzen, `updater.key` danach in den Vault, nie ins Repo.
3. `updater.pub` nach `packaging/windows/updater.pub` kopieren, committen. Der Test
   `updater_pub_is_valid_key` meldet einen beschädigten Schlüssel.

## Tests

`cargo test -p woa-updater` (ohne Netz): Signatur gut, schlecht, fremder Schlüssel, Kommentar-Version
ungleich, geänderter Kommentar; Downgrade, gleiche und unlesbare Version; Manifest-Parsing;
Host-Regeln; Signatur Pflicht ohne Rückfall auf die Prüfsumme. Die Schlüssel- und
Signaturdateien in `tests/fixtures/updater/` sind nur für Tests; der zugehörige private Schlüssel
wurde nach dem Erzeugen gelöscht.

Nicht automatisch geprüft: der echte Lauf unter Windows (`msiexec`, Neustart durch das MSI,
Wiederherstellung bei abgebrochener Installation).
