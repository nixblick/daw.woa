# Handgriffe für André: Feedback-Endpunkt daw.woa

Ziel: `https://feedback.wironair.de/api/feedback` legt Issues in `nixblick/daw.woa` an.

**Für André ist nichts mehr offen** (29.10.2026). Erledigt:

- 100 DNS: `feedback.wironair.de` und `feedback.wir-on-air.de` zeigen auf lfs001 (128.140.0.49).
- 200 Token: Der fine-grained Token (nur daw.woa, Issues: write, 90 Tage) liegt auf lfs001 unter
  `/etc/daw.woa/geheimnisse/feedback/GITHUB_TOKEN` (0640 root:999, nur der Wert).
- 230 Ablauf: `creavisions.stack/regeln/tokens.tsv` hat die Zeile `woa-feedback` (Ablauf 27.01.2027),
  der Kalendertermin liegt am 20.01.2027.

Ausrollen (Instanz `daw.woa`, Edge-vhost, Zertifikat) und die Gegenprobe per curl macht Claude
in bretteins.infra (Rolle `feedback`, Ticket bretteins.infra#145).
