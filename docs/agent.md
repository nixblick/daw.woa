# Agent: WOA per MCP bedienen

`woa-headless mcp <projektpfad>` startet einen MCP-Server über stdin/stdout (Crate `woa-agent`,
Bibliothek `rmcp` 3.8, Apache-2.0). Gibt es im Ordner noch kein `project.json`, beginnt ein leeres
Projekt; es wird beim ersten `save` geschrieben.

## In Claude Code eintragen

```sh
cargo build --release -p woa-headless
claude mcp add woa -- /pfad/zu/daw.woa/target/release/woa-headless mcp /pfad/zu/jingle.woa
claude mcp list
```

Mit `--scope project` landet der Eintrag in `.mcp.json` des aktuellen Repos, sonst nur lokal.
Claude Desktop: derselbe Befehl als `command`/`args` in `claude_desktop_config.json`.

## Über HTTP (ohne dass der Client einen Prozess startet)

Für Clients, die keinen Prozess starten können oder sollen: `woa-headless mcp <projekt> --http`.
Antwortet auf `http://127.0.0.1:8765/mcp` (Streamable HTTP). Das Zugriffstoken liest der Prozess
aus dem Schlüsselbund (Eintrag `woa / mcp-token`) oder aus der Umgebung `WOA_MCP_TOKEN`; jede
Anfrage braucht `Authorization: Bearer <Token>`. Optionen: `--port N`, `--bind ADRESSE` (Vorgabe
`127.0.0.1`; `0.0.0.0` öffnet den Server für das Netz, nur in einem vertrauten Netz).

Bequemer geht es in WOA unter **Einstellungen → KI & MCP**: Token erzeugen, Server im Hintergrund
an- und ausschalten (er startet nur mit „Speichern“, nie von selbst beim Start von WOA, und endet
mit WOA), fertige Einträge für Claude Code, Claude Desktop und andere Clients kopieren.

Alle Sitzungen eines Servers teilen sich ein Projekt. Läuft die WOA-App, reicht `woa-headless mcp`
(Stdio) an sie durch: Claude arbeitet live im offenen Fenster, jeder Schritt ist rückgängig zu machen,
und die Kopfleiste zeigt „KI bereit“ bzw. „KI verbunden“. Die App startet diesen Live-Server bei
jedem Start selbst, auch ohne erzeugtes Token (nur `127.0.0.1`, eigener Schlüssel je Start).
Der HTTP-Server `woa-headless mcp --http` dagegen arbeitet nur auf der Projektdatei im Ordner, nicht
live; gleichzeitiges Speichern in ihm und im Fenster überschreibt sich.

## Werkzeuge

| Tool | Parameter | Wirkung |
|---|---|---|
| `project_info` | – | Projekt als JSON, Pfad, `dirty`, Undo/Redo-Label |
| `add_track` | `name`, `kind` (`audio`/`midi`/`bus`, Standard `audio`) | neue Spur, liefert `track`-ID |
| `rename_track` | `track`, `name` | Spur umbenennen |
| `set_gain` | `track`, `gain_db` | Lautstärke in dB |
| `set_pan` | `track`, `pan` (-1..1) | Panorama |
| `set_mute` | `track`, `mute` | stumm schalten |
| `set_tempo` | `bpm` | Projekttempo |
| `set_solo` | `track`, `solo` | Solo; ist eine Spur auf Solo, klingen nur Solo-Spuren |
| `remove_track` | `track` | Spur samt Clips entfernen; Sends und Ausgänge darauf werden im selben Schritt getrennt |
| `list_effects` | `kind` (optional) | alle Effekte mit Reglern (Name, Einheit, Bereich, Vorgabe, Auswahlstufen) und Voreinstellungen |
| `insert_effect` | `track`, `kind` (`eq`/`compressor`/`limiter`/`gate`/`de_esser`/`reverb`/`delay`/`chorus`/`saturation`/`denoise`), `index`, `preset`, `params` (Name → Wert, über dem Preset) | Effekt in die Insert-Kette einer Spur oder eines Busses (vor dem Fader), liefert `insert`-ID |
| `insert_voice_chain` | `track` | Vorlage „Sprachkette“ (Hochpass-EQ, Gate, EQ, Kompressor, De-Esser) ans Ende der Kette, ein Undo-Schritt, liefert die fünf `inserts` |
| `voice_over` | `voice` | Stimme über das Bett (J5): Sprachkette auf die Stimme (falls noch keine), jede andere Audio- und MIDI-Spur ohne Sprachkette bekommt einen Kompressor mit der Stimme als Sidechain. Ein Undo-Schritt, nichts doppelt; liefert `chain` und je Bett-Spur `ducks` (`track`, `insert`) |
| `set_param` | `insert`, `param` (z. B. `bell_db`, `threshold_db`), `value` | Effekt-Regler setzen, Wert wird gegen den Bereich geprüft |
| `set_insert_bypass` | `insert`, `bypass` | Effekt knackfrei ein- oder ausschalten |
| `remove_insert` | `insert` | Effekt entfernen |
| `set_output` | `track`, `output` (Bus-ID, leer = Master) | Ausgang einer Spur; Schleifen werden abgelehnt |
| `set_send` | `track`, `target` (Bus), `level_db` (-96..12), `pre_fader` | Send anlegen oder ändern; Schleifen werden abgelehnt |
| `remove_send` | `track`, `target` | Send entfernen |
| `add_automation` | `track`, `target` (`gain`/`pan`/`send`/`param`), `send_target` (bei `send`), `insert` + `param` (bei `param`), `points` (`beat`, `value`), `mode` (`read`/`write`/`latch`) | Automationskurve eines Reglers setzen (ersetzt die ganze Kurve, ein Undo-Schritt); dazwischen gleitet der Wert, Lautstärke und Sends in dB; Wert wird gegen den Bereich geprüft |
| `remove_automation` | wie `add_automation` ohne `points` | Kurve entfernen |
| `set_sidechain` | `insert` (Kompressor oder Gate auf Audio-/MIDI-Spur), `source` (Spur-ID, leer = eigenes Signal) | Pegel von einer anderen Spur messen lassen (Ducking); Schleifen werden abgelehnt |
| `add_clip` | `track`, `name`, `start_beats`, `length_beats` | Platzhalter-Clip ohne Klang, liefert `clip`-ID |
| `generate_notes` | `track`, `length_beats`, `notes` (`beat`, `pitch` MIDI, `length_beats`, `velocity`), `waveform` (`sine`/`saw`/`square`), `start_beats`, `name` | rendert die Noten mit dem eingebauten Synth als WAV nach `aufnahmen/agent-<id>.wav` und legt einen Audio-Clip an |
| `generate_drums` | `track`, `kick`, `snare`, `hihat` (je Zeichen ein 16tel: `x` Schlag, `o` leise, `.` Pause), `start_beats`, `name`, `midi` | synthetische Drums (Kick: Sinus-Sweep, Snare: Ton + Rauschen, Hi-Hat: gefiltertes Rauschen) als Audio-Clip; Länge = längstes Muster; mit `midi: true` stattdessen GM-Drumnoten (36/38/42) als MIDI-Clip auf einer MIDI-Spur, die Spur bekommt das GM-Standard-Kit; mit `preset` (Pop, Four on the Floor, Rock, Hip-Hop, Nachrichten, Party, Halbes Tempo, Shuffle), `density` (-1/0/1) und `bars` dieselben Muster wie das Drum-Raster der App (immer MIDI) |
| `generate_fx` | `track`, `kind` (`riser`, `whoosh`, `downlifter`), `length_beats` (Vorgabe 4), `start_beats`, `name` | Jingle-Effekt aus Rauschen und Filter-Sweep als Audio-Clip, Länge im Projekttempo, Spitze −3 dBFS; Riser vor den Einsatz, Downlifter danach (gleicher Klang wie „Jingle-FX …“ in der App) |
| `search_samples` | `text`, `limit` (Vorgabe 15, höchstens 50) | Sucht bei Freesound, nur CC0 und CC BY; braucht den eigenen Freesound-Schlüssel des Nutzers (Einstellungen → KI), der nie beim Modell landet |
| `import_sample` | `id` (aus `search_samples`), `track`, `start_beats`, `name` | Lädt die Vorschau (MP3) nach `aufnahmen/`, legt den Clip an und schreibt Titel, Urheber, Lizenz und Link in `notizen.md` des Projekts |
| `track_sound` | `track`, optional `preset`, `program` (0–127), `drums`, `synth`, `bank` | liest oder setzt den Klang einer MIDI-Spur: GM-Programm aus der SoundFont oder Drumkit auf Kanal 10, `synth: true` zurück zum eingebauten Synth; `bank` wählt `musescore-general`, `avl-red-zeppelin` (nur Kits) oder `generaluser`. Eine fehlende Bank wird beim ersten Gebrauch im Hintergrund geladen (`bank_state`: `downloading`, dann `ready`), bis dahin und ohne Netz spielt GeneralUser GS. Liefert Klang, `soundfont_available`, die 16 Presets (je vier für lead, bass, pad, kit) und `banks` mit Zustand und Lizenz |
| `generate_chords` | `progression` (Katalog wie Fenster „Akkordfolge“, z. B. `I–V–vi–IV`, `-` geht für `–`), `key` (C … B, Vorgabe C), `bars` (Vorgabe 4), `start_beats` | legt drei MIDI-Spuren an: Akkorde (Streicher), Bass (E-Bass), Arpeggio (Klavier), je ein Clip, ein Akkord je Takt, alle Töne in der Tonart; ein Undo-Schritt; liefert Spuren und Clips |
| `transpose_notes` | `clips`, `semitones` (-24 … 24) | verschiebt alle Noten der MIDI-Clips (Tonartwechsel), ein Undo-Schritt |
| `import_audio` | `track`, `path` (WAV), `start_beats`, `name` | kopiert die Datei nach `aufnahmen/` und legt einen Clip in voller Länge an |
| `move_clip` | `clip`, `start_beats`, `track` (optional) | Clip verschieben, auch auf eine andere Spur |
| `trim_clip` | `clip`, `length_beats`, `offset_beats` (beide optional) | Länge und Versatz ins Audio |
| `set_fades` | `clip`, `fade_in_beats`, `fade_out_beats` | Ein- und Ausblenden |
| `split_clip` | `clip`, `at_beats` | Teilt am Punkt in zwei Clips, die wie das Original klingen; liefert die ID des rechten Teils |
| `set_fade_curves` | `clip`, `fade_in`, `fade_out` (`linear`, `equal_power`, `s_curve`; je optional) | Form der Fades |
| `set_clip_gain` | `clip`, `gain_db` (-96..24) | Clip-Gain vor dem Spurfader |
| `stretch_clip` | `clip`, `length_beats` | Zeitdehnung auf neue Länge, Tonhöhe bleibt (Faktor 0,25 bis 4) |
| `set_clip_pitch` | `clip`, `semitones` (-24..24) | Tonhöhe in Halbtönen, Länge bleibt |
| `set_clip_follow_tempo` | `clip`, `follow` | Audio folgt Tempoänderungen des Projekts (Dehnung statt Verschiebung) |
| `repeat_clip` | `clip`, `times` (1..256) | `times` Kopien direkt hintereinander (Loops), liefert die neuen IDs |
| `remove_clip` | `clip` | Clip entfernen (die Datei bleibt im Ordner) |
| `analyze` | `track` (optional) | rendert offline (48 kHz bis zum Ende des letzten Clips): integrierte LUFS, höchste momentane und kurzzeitige Lautheit, Sample-Peak und True Peak, Clipping, Länge, Spektrum (Spitze in Hz, Oktavbänder), Stereo (Korrelation, Balance, Seiten zu Mitte) und Hinweise; ohne `track` zusätzlich Peak je Audio-Spur, mit `track` nur diese Spur; ändert nichts |
| `spectrogram_png` | `path` (`.png`, relativ im Projektordner), `width` (16..4096, 800), `height` (16..2048, 400), `track` (optional) | Spektrogramm mit Logachse von 20 Hz bis zur halben Abtastrate als PNG |
| `export` | `path` (`.wav`/`.flac`/`.mp3`, nur relativ im Projektordner; absolut oder `..` wird abgelehnt), `target_lufs` (optional) | Mixdown wie in der GUI; mit `target_lufs` angeglichen, Spitzen bleiben unter −1 dBFS; liefert LUFS und Peak der geschriebenen Daten |
| `export_jingle` | `name` (nur Dateiname, kein Pfad), `target_lufs` (optional, Vorgabe: Ziel der Sendung aus `radio.json`), `cart` (optional, 1–16) | Fertiges Jingle (J6): Verstärkung auf die Ziel-Lautheit, Limiter auf −1 dBFS, WAV in den Jingle-Ordner der App (`WOA/Jingles` oder der eigene aus den Einstellungen), Eintrag als Jingle in der Bibliothek, auf Wunsch auf die Cart. Vorhandene Dateien bleiben, der nächste freie Name wird genommen; gleicher Inhalt gilt als schon vorhanden. Derselbe Weg wie „Exportieren → Als Jingle“ |
| `undo` / `redo` | – | letzten Schritt zurück / wieder her |
| `save` | – | atomar in den Projektordner schreiben |

Klang entsteht offline im Control-Bereich (`woa_dsp::generate`, nie im Audio-Thread) und liegt als
Datei im Projekt; die GUI spielt ihn wie jede Aufnahme. Gemessen wird nach ITU-R BS.1770
(`woa_engine::loudness`); True Peak misst `analyze` mit vierfacher Überabtastung (`woa_dsp::analysis`).

## Regeln

- Jedes Tool geht ausschließlich über `Session::apply` in `woa-model`; jeder Aufruf ist genau ein
  Undo-Schritt mit dem Label „Agent“. Mensch und Agent teilen dieselbe Undo-Geschichte.
- Ungültige Werte (z. B. Gain außerhalb des Bereichs, unbekannte Spur) kommen als Tool-Fehler
  (`isError: true`) zurück; am Projekt ändert sich dann nichts.
- Die Schemas der Parameter entstehen aus den serde-Typen (`schemars`).
- Der Server läuft im Control-Bereich, nie im Audio-Thread.
- stdout gehört dem Protokoll; Meldungen gehen nach stderr.

## Tests

`crates/agent/tests/mcp.rs` (rmcp-Client über In-Memory-Pipe),
`crates/agent/tests/loop.rs` (M2-Abnahme: 8-Takt-Loop nur über Tools, Export auf −14 LUFS, Datei
nachgemessen; Fehlerfälle aller neuen Tools) und
`apps/woa-headless/tests/mcp_stdio.rs` (rohes JSON-RPC über die Pipes des Prozesses).
