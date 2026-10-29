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
| `remove_track` | `track` | Spur samt Clips entfernen |
| `add_clip` | `track`, `name`, `start_beats`, `length_beats` | Platzhalter-Clip ohne Klang, liefert `clip`-ID |
| `generate_notes` | `track`, `length_beats`, `notes` (`beat`, `pitch` MIDI, `length_beats`, `velocity`), `waveform` (`sine`/`saw`/`square`), `start_beats`, `name` | rendert die Noten mit dem eingebauten Synth als WAV nach `aufnahmen/agent-<id>.wav` und legt einen Audio-Clip an |
| `generate_drums` | `track`, `kick`, `snare`, `hihat` (je Zeichen ein 16tel: `x` Schlag, `o` leise, `.` Pause), `start_beats`, `name` | synthetische Drums (Kick: Sinus-Sweep, Snare: Ton + Rauschen, Hi-Hat: gefiltertes Rauschen) als Audio-Clip; Länge = längstes Muster |
| `import_audio` | `track`, `path` (WAV), `start_beats`, `name` | kopiert die Datei nach `aufnahmen/` und legt einen Clip in voller Länge an |
| `move_clip` | `clip`, `start_beats`, `track` (optional) | Clip verschieben, auch auf eine andere Spur |
| `trim_clip` | `clip`, `length_beats`, `offset_beats` (beide optional) | Länge und Versatz ins Audio |
| `set_fades` | `clip`, `fade_in_beats`, `fade_out_beats` | Ein- und Ausblenden |
| `repeat_clip` | `clip`, `times` (1..256) | `times` Kopien direkt hintereinander (Loops), liefert die neuen IDs |
| `remove_clip` | `clip` | Clip entfernen (die Datei bleibt im Ordner) |
| `analyze` | – | rendert offline (48 kHz bis zum Ende des letzten Clips): integrierte LUFS, Sample-Peak in dBFS, Clipping, Länge, Peak je Audio-Spur allein; ändert nichts |
| `export` | `path` (`.wav`/`.flac`/`.mp3`, relativ = im Projektordner), `target_lufs` (optional) | Mixdown wie in der GUI; mit `target_lufs` angeglichen, Spitzen bleiben unter −1 dBFS; liefert LUFS und Peak der geschriebenen Daten |
| `undo` / `redo` | – | letzten Schritt zurück / wieder her |
| `save` | – | atomar in den Projektordner schreiben |

Klang entsteht offline im Control-Bereich (`woa_dsp::generate`, nie im Audio-Thread) und liegt als
Datei im Projekt; die GUI spielt ihn wie jede Aufnahme. Gemessen wird nach ITU-R BS.1770
(`woa_engine::loudness`); True Peak (Überabtastung) misst `analyze` nicht, nur den Sample-Peak.

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
