# Sound-Lizenzen

## GeneralUser GS (Standard-SoundFont)

- **Quelle:** https://github.com/mrbumpy409/GeneralUser-GS (S. Christian Collins), Version 2.0.3
  (Commit `684543d`), Datei `GeneralUser-GS.sf2`, rund 32 MB.
- **Lizenz:** GeneralUser GS License v2.0, Volltext in
  [`GeneralUser-GS-LICENSE.txt`](GeneralUser-GS-LICENSE.txt). Freie Nutzung privat und kommerziell,
  auch in Software, Änderungen erlaubt. Bitte nicht direkt auf die Downloads des Autors verlinken,
  sondern eine eigene Kopie ausliefern.
- **Verhältnis zur GPLv3:** Die Bank ist Daten, kein Code. Sie wird neben dem Programm
  ausgeliefert, nicht hineingelinkt.
- **Im Repo:** nur Lizenz und dieser Verweis, die `.sf2` nicht (Größe). Vorgesehen ist, dass die
  Pakete (`packaging/`) sie beim Bau aus der genannten Quelle mit festem Commit laden. **Stand
  01.11.2026: Das tut noch kein Paketskript** (grep nach `sf2` in `packaging/` ohne Treffer).
- **Wo die App sie sucht** (`crates/engine/src/soundfont.rs`), die erste lesbare Datei gilt:
  1. Umgebungsvariable `WOA_SOUNDFONT` (voller Pfad),
  2. `sounds/GeneralUser-GS.sf2` neben der ausführbaren Datei (Windows, AppImage),
  3. `../share/woa/sounds/GeneralUser-GS.sf2` relativ zur ausführbaren Datei (Linux-Pakete).
  Fehlt sie, spielen MIDI-Spuren mit GM-Klang den eingebauten Synth.

Entschieden von André am 29.10.2026.

## Zusatzbänke, beim ersten Gebrauch geladen (Paket J7, #271)

Katalog: [`banks.tsv`](banks.tsv) (Datei, Grösse, SHA-256, URL, Lizenz). Die App
(`crates/engine/src/banks.rs`) und `packaging/fetch-soundfont.sh <ordner> <bank>` lesen dieselbe
Datei. Geladen wird nur über https, ohne Weiterleitungen, in den Datenordner der App
(`banks/` unter `%APPDATA%\woa` bzw. `~/.local/share/woa`), zuerst als `.part`; erst bei stimmender
Grösse und Prüfsumme bekommt die Datei ihren Namen, die Lizenz wird daneben gelegt. Fehlt die Bank
(kein Netz, falsche Prüfsumme), spielt die Spur mit GeneralUser GS weiter.

### MuseScore General 0.2.0

- **Quelle:** https://ftp.osuosl.org/pub/musescore/soundfont/MuseScore_General/ (`VERSION`: 0.2.0),
  Datei `MuseScore_General.sf2`, 215 614 036 Bytes.
- **SHA-256:** `ee51d2c4b1525e70f19a45909c4fd7a2e26d91d115fa89dbf5a6bc413d8b9bf3`. Zweite Quelle:
  dieselbe Prüfsumme im Projekt muscriptor (`muscriptor/soundfonts.py`, Commit `f72be95`).
- **Warum SF2 und nicht SF3 (38 MB):** rustysynth 1.3.8 (neueste Version) lädt SF3 nicht. Probe am
  01.11.2026 mit der echten `MuseScore_General.sf3`: „SoundFont3 is not yet supported“; rustysynth
  prüft das selbst im Test `test_load_reject_sf3`.
- **Lizenz:** MIT, Wortlaut in [`MuseScore_General-LICENSE.md`](MuseScore_General-LICENSE.md)
  (`MuseScore_General_License.md` von der Quelle). Die Hinweise auf Urheber müssen mitgehen; die
  App legt die Datei neben die Bank.

### AVL Drumkits, Red Zeppelin 4pc

- **Quelle:** AVL Drumkits von Glen MacArthur (https://www.bandshed.net/avldrumkits/), SF2-Fassung
  aus https://github.com/x42/avldrums.lv2, Commit `9389f16f139410d9a55f9006976bd54cbd8b7806`,
  Datei `sf2/Red_Zeppelin_4_LV2.sf2`, 26 470 546 Bytes. Gleiche Bytes wie
  `https://www.bandshed.net/sounds/AVLDrumkits_SF2/Red_Zeppelin_4_LV2.sf2`.
- **SHA-256:** `289fc3e30a4859eb74ae14eb08ddef453e8bf8eab3e501772dcf96fd8328a71a`. Zweite Quelle:
  der git-Blob-Hash `06663ffad1f8ccb02bbb56f7a5cf8d75319017d0` laut GitHub-API stimmt mit der
  geladenen Datei überein.
- **Lizenz:** CC BY-SA 3.0 mit Ausnahme für damit gemachte Musik, Wortlaut in
  [`AVL-Drumkits-LICENSE.txt`](AVL-Drumkits-LICENSE.txt) (`sf2/README` im genannten Commit), dazu
  http://creativecommons.org/licenses/by-sa/3.0/. Namensnennung im Fenster „Über WOA“.
- **Nur Drums:** Die Bank hat ein einziges Preset (0:0). WOA nutzt sie nur für Spuren mit Kit;
  GM-Noten 36, 38 und 42 klingen (Offline-Probe `real_banks_match_the_catalog_and_sound`).

