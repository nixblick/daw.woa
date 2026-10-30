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
`.github/workflows/release.yml` (Bau, Signatur, Entwurf).

## Quelle und Manifest

- `https://github.com/nixblick/daw.woa/releases/latest/download/latest.json`. „latest“ sind nur
  veröffentlichte Releases, keine Entwürfe und keine Vorabversionen. Ein Release wird also erst
  sichtbar, wenn es veröffentlicht ist.
- `latest.json`: `{version, notes, msi_url, msi_sha256, msi_sig_url}`. `msi_sig_url` fehlt, wenn
  das Release nicht signiert wurde.
- Die App lädt nur von `https://github.com/nixblick/daw.woa/releases/…`. Weiterleitungen folgt
  sie selbst und nur auf `objects.githubusercontent.com` und `release-assets.githubusercontent.com`.
  Alles andere (anderer Host, anderes Repo, `http`, `..` im Pfad) wird abgelehnt.
- Der Download läuft in einem Hintergrund-Thread mit Zeitlimit und Größengrenze, nie im
  UI-Thread.

## Zwei Stufen, nie gemischt

Welche Stufe gilt, entscheidet die Datei `packaging/windows/updater.pub`, die zur Bauzeit per
`include_str!` eingebrannt wird.

| | Stufe A | Stufe B |
|---|---|---|
| Wann | `updater.pub` ist ein Platzhalter (Stand heute) | `updater.pub` ist ein echter minisign-Schlüssel |
| Prüfung | SHA-256 des MSI aus `latest.json` | minisign-Signatur über die MSI-Bytes, Pflicht |
| Fehlt der Prüfwert | abgelehnt | abgelehnt (kein Rückfall auf A) |
| Downgrade-Wächter | ja | ja |

In beiden Stufen gilt: nur strikt neuere Version (gleiche, ältere und unlesbare Versionen sind
ein Veto), nur HTTPS mit Zertifikatsprüfung, nur die festen GitHub-Hosts.

Stufe B zusätzlich: Der signierte, vertrauenswürdige Kommentar muss `woa <version>` heißen, und
die Version muss der Manifest-Version entsprechen. So kann niemand mit einer gefälschten
Versionsangabe ein altes, echt signiertes MSI unterschieben. Steht im Manifest zusätzlich eine
SHA-256, muss auch sie stimmen.

## Bedrohungsmodell, ehrlich

Angreifer kann beliebige Bytes als Manifest und MSI liefern (DNS, WLAN, Proxy) oder die
Release-Dateien auf GitHub ändern.

- **Netzwerk-Angreifer (DNS, MITM, fremde WLANs):** In beiden Stufen abgewehrt. Ohne gültiges
  Zertifikat für `github.com` kommt nichts an, und andere Hosts nimmt die App nicht.
- **Beschädigter oder abgeschnittener Download:** In beiden Stufen abgefangen.
- **Zurückgespieltes altes Release:** Der Downgrade-Wächter lehnt es ab.
- **Restrisiko Stufe A: Kompromittierung des GitHub-Kontos oder der Release-Assets.** Wer
  Release-Dateien ändern darf, tauscht MSI und `latest.json` gemeinsam aus; die SHA-256 passt
  dann zum Schadcode. Eine Prüfsumme neben dem Paket ist kein Schutz gegen den Betreiber des
  Ortes, an dem beides liegt. Das MSI läuft zudem mit Administratorrechten (UAC-Abfrage), ein
  untergeschobenes Paket wäre also volle Codeausführung. Dieses Risiko ist bewusst
  akzeptiert, solange es um wenige bekannte Tester geht.
- **Stufe B** schließt das: Der private Schlüssel liegt nur als GitHub-Secret in der CI und auf
  Andrés Rechner. Wer nur Release-Dateien ändern kann, kann keine gültige Signatur erzeugen.
  Restrisiko dort: Zugriff auf Secrets/Workflows des Repos.
- **Übergang A zu B:** Eine App, die noch mit Platzhalter gebaut wurde, prüft keine Signatur.
  Das erste Release mit echtem Schlüssel erreicht sie also noch über Stufe A. Erst Versionen, die
  den Schlüssel enthalten, erzwingen Signaturen.
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

Ohne das Secret `WOA_UPDATER_KEY` bricht der Workflow nicht ab: Er signiert nicht, schreibt nur
die SHA-256 in `latest.json` und warnt im Log (Stufe A). Ist die `updater.pub` aber schon echt
und das Secret fehlt, bricht er ab, weil kein Gerät ein unsigniertes Update annehmen würde.

## Erste Version mit Updater

Die erste Version, die den Updater enthält, muss einmal **von Hand** installiert werden (MSI aus
dem Windows-Bau oder dem Release doppelklicken). Erst von da an aktualisiert sie sich selbst.

## Einrichten (André, einmalig) – Stufe B

Optional und jederzeit nachholbar; ohne diese Schritte läuft Stufe A.

```
# 101  Notebook
mkdir -p ~/.secrets/daw.woa && chmod 700 ~/.secrets/daw.woa

# 110  Notebook – Schlüsselpaar ohne Passwort erzeugen (fragt nichts)
minisign -G -W -p ~/.secrets/daw.woa/updater.pub -s ~/.secrets/daw.woa/updater.key

# 120  Notebook
chmod 600 ~/.secrets/daw.woa/updater.key

# 130  Notebook – privaten Schlüssel als Secret ins Repo (Inhalt, kein Passwort nötig)
gh secret set WOA_UPDATER_KEY -R nixblick/daw.woa < ~/.secrets/daw.woa/updater.key

# 140  Notebook – öffentlichen Schlüssel ins Repo (er darf öffentlich sein)
git -C ~/GitHub/nixblick/daw.woa switch -c chore/updater-key
cp ~/.secrets/daw.woa/updater.pub ~/GitHub/nixblick/daw.woa/packaging/windows/updater.pub

# 150  Notebook – committen, pushen, Pull Request
git -C ~/GitHub/nixblick/daw.woa add packaging/windows/updater.pub
git -C ~/GitHub/nixblick/daw.woa commit -m "Updater: public key"
git -C ~/GitHub/nixblick/daw.woa push -u origin chore/updater-key
gh pr create -R nixblick/daw.woa --fill

# 160  Notebook – nach dem Merge ein Release taggen (Version aus Cargo.toml)
git -C ~/GitHub/nixblick/daw.woa switch main
git -C ~/GitHub/nixblick/daw.woa pull
V=$(sed -n 's/^version = "\(.*\)"/\1/p' ~/GitHub/nixblick/daw.woa/Cargo.toml | head -1)
git -C ~/GitHub/nixblick/daw.woa tag "v$V"
git -C ~/GitHub/nixblick/daw.woa push origin "v$V"

# 170  Notebook – auf den Lauf warten (grün = Entwurf liegt bereit)
gh run watch -R nixblick/daw.woa

# 180  Notebook – Entwurf ansehen: MSI, ZIP, .minisig, latest.json
gh release view "v$V" -R nixblick/daw.woa

# 190  Notebook – veröffentlichen; ab jetzt sehen Tester das Update
gh release edit "v$V" -R nixblick/daw.woa --draft=false
```

Der private Schlüssel `~/.secrets/daw.woa/updater.key` bleibt auf dem Notebook und im
GitHub-Secret. Geht er verloren oder wird er sichtbar: neues Paar erzeugen (Schritte 110 bis
150), neues Release taggen. Apps mit dem alten Schlüssel erreichen das neue Release dann nicht
mehr automatisch und müssen einmal von Hand installiert werden.

## Tests

`cargo test -p woa-updater` (ohne Netz): Signatur gut, schlecht, fremder Schlüssel, Kommentar-Version
ungleich, geänderter Kommentar; Downgrade, gleiche und unlesbare Version; Manifest-Parsing;
Host-Regeln; Stufe A (Hash) und Stufe B (Signatur Pflicht, kein Rückfall). Die Schlüssel- und
Signaturdateien in `tests/fixtures/updater/` sind nur für Tests; der zugehörige private Schlüssel
wurde nach dem Erzeugen gelöscht.

Nicht automatisch geprüft: der echte Lauf unter Windows (`msiexec`, Neustart durch das MSI,
Wiederherstellung bei abgebrochener Installation).
