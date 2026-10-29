# Changelog

## Unreleased

- Runbook `docs/runbook-morgen.md` (Merge-Reihenfolge, Server, Hardware-Test) samt Handgriffen für Merge-Erlaubnis und Worktree-Regel.

- Review M0 (#1): `.gitignore` hält jetzt auch `.claude/worktrees/`, `.claude/settings.local.json` und `*.tmp` (atomares Schreiben) aus dem Repo.

- Projektmodell `woa-model` (M0, Branch 2): `Project`/`Track`/`Clip` mit `u64`-IDs, `Command`-Enum (Spuren, Gain, Pan, Mute, Solo, Tempo, Clips) mit exakten Inversen, `Session::apply` mit Undo/Redo und Gruppen, Ordnerformat `<name>.woa/project.json` mit `format_version`, atomares Schreiben, Autosave-Hook `Session::autosave` ohne Zeitgeber. Tests: Roundtrip, insta-Snapshot, proptest apply+undo.

## 0.1.0 — 2026-10-28

- CI-Gate (M0, #1): fmt, clippy -D warnings, test auf Linux und Windows, cargo-deny mit GPL-kompatibler Lizenzliste. `.gitignore` hält lokale Projekte und Logs fern.

## 0.0.1 — 2026-10-28

- Repo angelegt: Konzept `docs/konzept.md` (aus Drive), Cargo-Workspace mit 14 Crates und 3 Apps als leere Gerüste, `CLAUDE.md` mit den Echtzeit- und Stabilitätsregeln, LICENSE GPLv3, Issues M0–M11.
