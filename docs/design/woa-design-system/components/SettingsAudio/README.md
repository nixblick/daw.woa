# SettingsAudio

Einstellungen (Fassung 8). Bereiche nach dem Register in `settings.rs`: **Allgemein · Audio und Geräte · Radio · Party · Produzent · Dienste und Konten** (Entwickler nur bei aktivem Entwickler-Modus).

- **Werte direkt in der Zeile**, wo es nur ein Wert ist (Crossfade, Ducking, Stopp-Dauer, Bitrate …), statt „Im Radio zeigen“. Verweise nur dort, wo wirklich ein eigenes Werkzeug steht.
- **Audio und Geräte = der eine Ort für Soundkarten** (Regel 03.11.2026). Vorschlag Fassung 8: Der Inhalt des Fensters „Audio-Geräte“ steht eingebettet im Bereich, statt nur „Öffnen“ – es bleibt ein Ort, nur ohne Extrafenster. **Erst nach OK von André.**
  - Ausgänge je Quelle (Playlist, Carts, Decks A–C): Gerät + Kanalpaar, „Wie Playlist“ gedimmt.
  - Vorhören (PFL), Eingänge (Mikrofon mit Pegel, Telefon), Pult-Modus.
  - Fehlt ein Gerät: Zeile in `warn`, Chip „abgezogen“, Auswahl gelb umrandet, Anleitung im Untertitel.
  - **Übernehmen-Leiste** unten, sobald Geräte geändert wurden; auf Sendung mit Hinweis auf die kurze Unterbrechung.
- **Ein Schaltertyp:** `woa-tog`. Keine Häkchen, keine „an“-Chips als Schalter, keine Punkt-Optionen; Mehrfachwahl als Segmente.
- Aktiver Bereich: `surface` + Kante `accent-2`, **nicht rot**. Chips am Bereich zeigen Hinweise (Audio „1“) oder Anzahl eingerichteter Dienste.
- Fenster: maximiert oder 1400 × 930 (siehe `Window`).
