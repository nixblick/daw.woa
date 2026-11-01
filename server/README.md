# server

Endpunkte für Feedback und Telemetrie (M0). Konzept: `docs/konzept.md`.

## Feedback-Endpunkt

daw.woa baut keinen eigenen Ingest. Der Feedback-Button schickt an
`https://feedback.wironair.de/api/feedback`, die Telemetrie an `/api/logs/ingest` derselben Adresse.
Der Server läuft als Instanz einer Rolle in einem privaten Infrastruktur-Repo.

| Datei | Zweck |
|---|---|
| `feedback/nginx-feedback.wironair.de.conf` | vhost für den Edge-nginx, TLS per certbot-Webroot |

### Geheimnisse

Nur einer: der fine-grained Token (`nixblick/daw.woa`, Issues: Read and write). Er entsteht
und bleibt auf dem Server, nicht in diesem Repo. Vault-Ordner des Repos für Lokales:
`~/.secrets/daw.woa/` (bisher leer).
