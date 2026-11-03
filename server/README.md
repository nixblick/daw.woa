# server

Endpunkte für Feedback und Telemetrie (M0). Konzept: `docs/konzept.md`.

## Feedback-Endpunkt

daw.woa baut keinen eigenen Ingest. Der Feedback-Button schickt an
`https://feedback.wironair.de/api/feedback`, die Telemetrie an `/api/logs/ingest` derselben Adresse.
Der Server läuft als Instanz einer Rolle in einem privaten Infrastruktur-Repo.

| Datei | Zweck |
|---|---|
| `feedback/nginx-feedback.wironair.de.conf` | vhost für den Edge-nginx, TLS per certbot-Webroot |

## Hörer-Statistik (`hoerer/`)

`hoerer/hoerer.py` (nur Python-Standardbibliothek) läuft auf dem Sendeserver neben Shoutcast,
liest alle 30 s die öffentliche Kurzstatistik `/7.html` (ohne Kennwort) und speichert Zeit,
aktuelle Hörer, Spitze, eindeutige Hörer und „Quelle verbunden“ in SQLite. Das läuft auch, wenn
die App aus ist oder die Notfall-Playlist sendet. `GET /api/hoerer?von=&bis=` (Unix-Sekunden,
Vorgabe 30 Tage) liefert `jetzt`, den `verlauf` der neuesten Sendung und die `sendungen`
(Start, Ende, Dauer, Spitze, Durchschnitt), keine IPs oder User-Agents. Einstellungen per
Umgebung, siehe Kopf der Datei. Tests: `python3 -m unittest discover -s server/hoerer`.

**Titel für Hörer (`POST /api/titel`):** Bei entferntem Shoutcast schickt die App den Titel
per HTTPS an den Sendeserver (`Authorization: Bearer <Quellkennwort>`, Rumpf
`{"titel": "..."}`, höchstens 200 Zeichen). Der Dienst vergleicht konstantzeitig mit der
Quellkennwort-Datei und ruft `admin.cgi?mode=updinfo` im Docker-Netz mit dem Admin-Kennwort
auf. Beide Dateien entstehen auf dem Server (Rolle `shoutcast` in bretteins.infra) und werden
nur lesend eingehängt (`HOERER_TITEL_QUELL_KW`, `HOERER_TITEL_ADMIN_KW`); fehlt eine, antwortet
der Endpunkt 503. Antworten: 204 gesetzt, 400 Titel fehlt, 401 Kennwort falsch, 413 zu groß,
502 Shoutcast lehnt ab. nginx bremst je IP (30/min).

### Geheimnisse

Nur einer: der fine-grained Token (`nixblick/daw.woa`, Issues: Read and write). Er entsteht
und bleibt auf dem Server, nicht in diesem Repo. Vault-Ordner des Repos für Lokales:
`~/.secrets/daw.woa/` (bisher leer).
