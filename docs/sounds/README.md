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
