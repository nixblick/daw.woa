# Telemetrie und Feedback (`crates/telemetry`, `woa-telemetry`)

Stand 28.10.2026. Grundsätze im Konzept, Abschnitt „Logging, Telemetrie & Datenschutz“.

## Lokales Log

- `woa_telemetry::init(log_dir, upload)` richtet `tracing` ein: JSON-Zeilen in
  `<log_dir>/woa.JJJJ-MM-TT.log`, eine Datei je Tag, höchstens 30 Dateien (`LOG_RETENTION_DAYS`).
- Filter per Umgebungsvariable `WOA_LOG` (Syntax von `EnvFilter`), Standard `info`.
- Panic-Hook: schreibt Ort, Nachricht und Backtrace als `error` ins lokale Log und erzeugt das
  Upload-Ereignis `art = "panic"` (ohne Nachricht). Der vorige Hook läuft danach weiter.
- Nie aus dem Audio-Thread loggen (CLAUDE.md, Regel 1).

## Upload-Senke (Opt-in)

- Aus, solange `UploadConfig.enabled == false` oder `url` leer ist (Standard). Dann kein Thread,
  kein Netz.
- Erfasst wird über den tracing-Layer: jedes Ereignis mit Feld `art` (plus optional `wert`) und
  jedes `warn`/`error` (dann `art = log.warn`/`log.error`). Direkt: `Uploader::record(stufe, modul, art, wert)`.
- **Nur Code, Stufe, Zahl.** Nachricht und alle anderen Felder bleiben lokal. `modul` (aus dem
  tracing-Target, `::` → `.`) und `art` müssen `^[a-z0-9_.-]+$` sein (60/80 Zeichen), kein `..`,
  kein führender Punkt, keine Dateiendung (`scrub.rs`). Sonst wird das Ereignis verworfen.
- Gebündelt: ab `batch_size` (200) Ereignissen oder nach `interval` (300 s). HTTP (`ureq`) läuft
  nur im Thread `woa-telemetry-upload`; Erfassen ist `try_send` auf eine begrenzte Queue.
- 2xx = gespeichert, Puffer leeren. Alles andere: Puffer behalten (höchstens `max_buffer`,
  älteste fallen weg) und später erneut. Beim Beenden ein letzter Versuch.

### Wire-Vertrag: `POST <url>` (bretteins `/ingest` bzw. `/api/logs/ingest`)

```json
{"install_id":"<uuid>","app_version":"0.1.0","os":"linux",
 "ereignisse":[{"zeit_ms":1761600000000,"stufe":"warn","modul":"woa_engine","art":"xrun","wert":1}]}
```

Geprüft gegen `bretteins.infra/ansible/roles/feedback/files/app.py` (`ingest_zeilen`, Route bei
`do_POST`), Stand 28.10.2026:

| Server erwartet | WOA schickt |
|---|---|
| `ereignisse` Liste, höchstens 2000 | ja, Puffer höchstens 2000 |
| `modul`/`art` Pflicht, `^[a-z0-9_.-]+$`, 60/80 Zeichen | Scrubber erzwingt dasselbe, strenger |
| `stufe` ≤ 20 Zeichen; `error`/`warn` zählen als Fehler/Warnung | `error`, `warn`, `info`, `debug` |
| `wert`, `zeit_ms` ganzzahlig, `zeit_ms` in ms-Epoche | `i64`, `u64` |
| `install_id` ≤ 64, `app_version` ≤ 32, `os` ≤ 32, optional | gesetzt, leer = weggelassen |
| Antwort 200 `{"accepted":n}`; 503 = nicht gespeichert | nur 2xx leert den Puffer |
| `Authorization: Bearer` nur, wenn der Server ein Token setzt | kein Token (die App trägt keine Geheimnisse) |

Folge: Der WOA-Server betreibt `/api/logs/ingest` ohne Token (wie bretteins mit leerem
`LOG_INGEST_TOKEN`); Schutz sind Whitelist für `modul`/`art` und Ratenbremse.

## Einstellungen und Opt-in (`apps/woa`, seit 0.1.15)

- `config.json` im Konfigurationsordner (`~/.config/woa/` bzw. `%APPDATA%\woa\`):
  `install_id` (zufällige UUID v4, beim ersten Start erzeugt, ohne Personenbezug),
  `telemetry_enabled` (`null` = noch nicht gefragt, `true`/`false`), optional `telemetry_url`
  (Standard `https://feedback.wironair.de/api/logs/ingest`) und `feedback_url`.
- Erster Start: Dialog „Fehlercodes senden?“. Ohne Ja bleibt die Senke aus (kein Thread, kein Netz).
  Später umschaltbar unter „Einstellungen“; gilt ab dem nächsten Start.
- `woa-headless log-test` schickt ohne `--url` an dieselbe Adresse.

## Datenschutz

Gesendet wird nur, wenn der Nutzer zugestimmt hat: Ereigniscode, Stufe, Zahl, Zeitpunkt, dazu
Installations-ID, App-Version und Betriebssystem. Keine Texte, Dateinamen, Pfade, Projektinhalte
oder Aufnahmen. Die Installations-ID ist Zufall und lässt sich durch Löschen von `install_id` in
`config.json` neu würfeln. Server: Instanz `daw.woa` der Rolle `feedback` (bretteins.infra) auf
der eigene Server, Datenbank `woa_stats`; Aufbewahrung nach der Retention der Rolle.

## Feedback

`submit_feedback(text, screenshot_png: Option<Vec<u8>>, &FeedbackMeta)` → `Result<Option<u64>>`
(Issue-Nummer, wenn der Server sie nennt). Blockierend, nie im Audio-/GUI-Thread aufrufen.

```json
{"kind":"feedback","title":"<erste Zeile, 80>","body":"<text>\n\nInstall: <id>","type":"bug",
 "trust":"L0","app":"0.1.0","appVersion":"0.1.0","os":"linux","installId":"<id>",
 "sentAt":1761600000000,"images":[{"name":"screenshot.png","content_b64":"…"}]}
```

Wie bretteins (`bretteins.desktop/app/core/src/feedback_req.rs`, Server `handle_feedback` und
`save_images`): `type` aus bug|idee|frage|lob (sonst leer → `feedback`), Text bis 20000 Zeichen,
Screenshot nur PNG bis 3 MB. Antwort 201/200 `{"ok":true,"issue":n}`; 429/502/503 → Fehler.

## Offen

- Server-Seite für Logs (DB `woa_stats`, Edge-Freigabe `/api/logs/ingest`): eigener PR in
  bretteins.infra, Ausrollen durch André. Bis dahin antwortet der Edge mit 404 und die App behält
  die Ereignisse im Puffer.

## Was ab 0.32.x zusätzlich gesendet wird (30.10.2026)

Alle 120 Sekunden statt alle 5 Minuten (André 30.10.). Weiter nur Bezeichner und Zahlen:

| `art` | `wert` | Woher |
|---|---|---|
| `log.warn.<datei>.<zeile>`, `log.error.<datei>.<zeile>` | 0 | jede Warnung/jeder Fehler ohne eigenes `art`; der Code-Ort statt der Nachricht |
| `span.<name>` | Dauer in ms | jede Aktion mit `#[tracing::instrument]`, die 20 ms oder länger dauert (z. B. `span.playlist_add`, `span.audio_load`, `span.show_load`, `span.dawproject_import`, `span.decode_file`, `span.ffmpeg_decode`, `span.export`, `span.render_stems`, `span.render_clips`, `span.load_wav`) |
| `ui.frame_slow` | längster Frame in ms | Oberfläche ruckelt (Frame ≥ 100 ms), höchstens einmal je Sekunde |
| Zusammenfassung alle 120 s | Zahl | `ui.frames`, `ui.fps`, `ui.frame_avg_ms`, `ui.frame_max_ms`, `ui.frames_slow`, `app.uptime_s`, `proc.rss_mb` und `proc.threads` (nur Linux), `project.tracks`, `project.clips`, `project.dirty`, `ui.zoom_pct`, `player.open`, `radio.underruns_total` |
| `ui.hang_open` | Dauer in ms | ein Frame hängt seit 2 s, danach alle 10 s erneut, auch wenn das Fenster nie mehr reagiert |

Auswertung auf dem Server: Datenbank `woa_stats`, Tabelle `log_lines` (Spalte `ms` = `wert`).
