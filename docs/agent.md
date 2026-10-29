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
| `add_clip` | `track`, `name`, `start_beats`, `length_beats` | Clip, liefert `clip`-ID |
| `undo` / `redo` | – | letzten Schritt zurück / wieder her |
| `save` | – | atomar in den Projektordner schreiben |

## Regeln

- Jedes Tool geht ausschließlich über `Session::apply` in `woa-model`; jeder Aufruf ist genau ein
  Undo-Schritt mit dem Label „Agent“. Mensch und Agent teilen dieselbe Undo-Geschichte.
- Ungültige Werte (z. B. Gain außerhalb des Bereichs, unbekannte Spur) kommen als Tool-Fehler
  (`isError: true`) zurück; am Projekt ändert sich dann nichts.
- Die Schemas der Parameter entstehen aus den serde-Typen (`schemars`).
- Der Server läuft im Control-Bereich, nie im Audio-Thread.
- stdout gehört dem Protokoll; Meldungen gehen nach stderr.

## Tests

`crates/agent/tests/mcp.rs` (rmcp-Client über In-Memory-Pipe) und
`apps/woa-headless/tests/mcp_stdio.rs` (rohes JSON-RPC über die Pipes des Prozesses).
