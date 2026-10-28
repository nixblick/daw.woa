# Changelog

## Unreleased

- Feedback-Server vorbereitet (M0, Branch 4): `server/README.md` mit Anleitung für eine zweite Instanz der bretteins-Rolle `feedback` (feedback.wironair.de, Port 8081, `GITHUB_REPO=nixblick/daw.woa`), Prüfung der Mehrinstanzfähigkeit samt Patch-Vorschlag, Vorlagen unter `server/feedback/`, Runbook `docs/andre-handgriffe.md`. Nichts ausgerollt.

## 0.1.0 — 2026-10-28

- CI-Gate (M0, #1): fmt, clippy -D warnings, test auf Linux und Windows, cargo-deny mit GPL-kompatibler Lizenzliste. `.gitignore` hält lokale Projekte und Logs fern.

## 0.0.1 — 2026-10-28

- Repo angelegt: Konzept `docs/konzept.md` (aus Drive), Cargo-Workspace mit 14 Crates und 3 Apps als leere Gerüste, `CLAUDE.md` mit den Echtzeit- und Stabilitätsregeln, LICENSE GPLv3, Issues M0–M11.
