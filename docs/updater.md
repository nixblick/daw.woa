# Auto-Updater (MSI, AppImage, Flatpak)

WOA prüft beim Start und danach im Betrieb **höchstens alle 12 Stunden**, ob GitHub ein
neueres, **veröffentlichtes** Release hat (nach einem Fehlversuch, etwa ohne Netz, frühestens
nach einer Stunde wieder). Gibt es eines, zeigt ein Streifen unter dem Menü „Update x.y.z
verfügbar“ mit den Neuerungen. Was ein Klick tut, hängt davon ab, wie WOA installiert ist
(„Hilfe → Über WOA“ nennt die Art):

| Art | Erkennung | Klick auf „Jetzt installieren“ |
|---|---|---|
| Windows (MSI) | Windows | lädt das MSI, prüft es, startet den Installer, beendet die App; das MSI startet die neue Version |
| AppImage | Umgebungsvariable `APPIMAGE` zeigt auf eine vorhandene Datei | lädt das neue AppImage neben die alte Datei, prüft es, ersetzt die Datei (`rename`, atomar), startet sie neu |
| Flatpak | `FLATPAK_ID` oder `/.flatpak-info` | **nur melden**: Streifen nennt `flatpak update de.wironair.woa`; Flatpak aktualisiert sich selbst |
| sonst (Quellbau, ZIP) | alles andere | nur melden, Link zur Release-Seite |

**Nie während Sendung oder Aufnahme:** Solange „Auf Sendung“ läuft oder eine Aufnahme (Audio
oder MIDI) offen ist, sucht WOA nicht von selbst, und „Jetzt installieren“ ist grau mit dem
Grund daneben. Läuft eine Sendung an, während das Paket geladen wird, wird nichts installiert;
das geladene Paket wird gelöscht und das Update bleibt wählbar.

**Erststart-Hinweis:** Vor der allerersten Suche erscheint einmal ein Fenster, das sagt, was
passiert (eine kleine Datei von GitHub, keine Daten über dich, nie von allein installieren),
mit „Verstanden“ und „Abschalten“. Bis zur Antwort wird nicht gesucht. Das gilt auch für
bestehende Installationen beim ersten Start mit dieser Version.

Ausschalten: Einstellungen → „Alle 12 Stunden nach Updates suchen“ (oder der Hinweis oben),
oder `"auto_update": false` in `config.json`. „Jetzt nach Updates suchen“ (Einstellungen und
„Über WOA“) fragt sofort, ohne die 12 Stunden abzuwarten.

Code: `crates/updater` (Prüfung, Netz), `crates/gui/src/update.rs` und `update_*` in
`crates/gui/src/app.rs` (Anzeige), `packaging/windows/woa.wxs` (Neustart),
`.github/workflows/release.yml` (Bau, Signatur, Entwurf; Ablauf: `docs/release.md`).

## Quelle und Manifest

- `https://github.com/nixblick/daw.woa/releases/latest/download/latest.json`. „latest“ sind nur
  veröffentlichte Releases, keine Entwürfe und keine Vorabversionen. Ein Release wird also erst
  sichtbar, wenn es veröffentlicht ist.
- `latest.json`: `{version, notes, msi_url, msi_sha256, msi_sig_url, appimage_url,
  appimage_sha256, appimage_sig_url}`. Die `*_sig_url` fehlen, wenn das Release nicht signiert
  wurde. Die `appimage_*`-Felder fehlen in Manifesten älterer Releases; dann meldet ein AppImage
  das Update nur und installiert nichts.
- MSI und AppImage laufen durch dieselbe Prüfung (Stufe A oder B unten), mit demselben
  signierten Kommentar `woa <version>`.
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

## Stufe B einschalten

Der Maintainer erzeugt ein minisign-Schlüsselpaar, hinterlegt den privaten Teil als Actions-Secret `WOA_UPDATER_KEY` und legt den öffentlichen Teil als `packaging/windows/updater.pub` ins Repo. Ab dem nächsten Release ist die Signatur Pflicht.

## Tests

`cargo test -p woa-updater` und `cargo test -p woa-gui --lib -- update` (ohne Internet, ohne Ton):

- **Fake-Server auf 127.0.0.1** (`crates/updater/src/net.rs`, `Policy::Local`, nur in Tests
  erreichbar): neuere, gleiche und ältere Version; kaputte Prüfsumme (nichts wird geschrieben);
  kein Netz (nichts lauscht); Serverfehler und Unsinn als Manifest; Weiterleitung auf fremden
  Host abgelehnt; Flatpak und Quellbau laden **kein Byte**; AppImage wird ausführbar abgelegt
  und ersetzt die alte Datei, bei Fehler bleibt die alte.
- **GUI ohne Fenster** (`crates/gui/src/app.rs`, Tests am Ende): gezeichneter Text der
  Update-Leiste je Paketart, graue Taste bei Aufnahme, Erststart-Hinweis, „Über WOA“.
- **12-Stunden-Regel** (`crates/gui/src/update.rs`, `auto_check_due`): Erfolg, Fehlversuch,
  zurückgestellte Uhr.

Dazu: Signatur gut, schlecht, fremder Schlüssel, Kommentar-Version
ungleich, geänderter Kommentar; Downgrade, gleiche und unlesbare Version; Manifest-Parsing;
Host-Regeln; Stufe A (Hash) und Stufe B (Signatur Pflicht, kein Rückfall). Die Schlüssel- und
Signaturdateien in `tests/fixtures/updater/` sind nur für Tests; der zugehörige private Schlüssel
wurde nach dem Erzeugen gelöscht.

Nicht automatisch geprüft: der echte Lauf unter Windows (`msiexec`, Neustart durch das MSI,
Wiederherstellung bei abgebrochener Installation); der Neustart eines echten AppImage nach dem
Ersetzen (die Tests ersetzen die Datei, starten sie aber nicht); AppImage-Ordner ohne
Schreibrecht (dann meldet WOA „Dateifehler“ und der Link zur Release-Seite bleibt).

Hinweis AppImage mit Schlüssel (Stufe B): Das AppImage muss dann ebenfalls signiert sein;
`release.yml` signiert es mit demselben Schlüssel wie das MSI.
