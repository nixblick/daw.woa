# Runbook André: Nachtbau 29.10.2026 übernehmen

Reihenfolge = Ausführung. Kein Geheimniswert steht hier. Host steht über jedem Befehl.

Vorbelegt (einmal pro Terminal):

```bash
R=nixblick/daw.woa
cd ~/GitHub/nixblick/daw.woa
```

## 100 PRs mergen

Alle PRs gehen mit **Merge-Commit** (`--merge`), nicht Squash: #23–#25 bauen auf dem
Integrationszweig auf und tragen dieselben Commits; bei Squash gäbe es Konflikte.

### 101 · Host: Notebook
Stand prüfen, alle müssen grün sein:

```bash
gh pr list -R $R --json number,title,statusCheckRollup --jq '.[]|"\(.number) \([.statusCheckRollup[].conclusion]|unique|join(",")) \(.title)"'
```

Erwartet: bei jeder Zeile nur `SUCCESS`.

### 110 · Host: Notebook
Der Reihe nach mergen. Bricht einer mit „not mergeable“ ab: stehen lassen, Claude Bescheid geben.

```bash
for n in 21 17 14 15 18 19 23 24 25; do gh pr merge $n -R $R --merge || break; sleep 5; done
```

Erwartet: neun Zeilen `✓ Merged pull request …`.

### 120 · Host: Notebook
Integrationsnachweis schließen (nicht mergen) und CI auf main abwarten:

```bash
gh pr close 22 -R $R -c "Nachweis erfüllt, Inhalte über #21–#25 gemergt"
gh run watch -R $R $(gh run list -R $R -b main -L 1 --json databaseId --jq '.[0].databaseId') --exit-status
```

Erwartet: Lauf grün, rc 0.

### 130 · Host: Notebook
Lokal nachziehen und M0-Abnahme per CLI:

```bash
git switch main && git pull --ff-only
cargo run -q -p woa-headless -- roundtrip /tmp/abnahme.woa; echo "rc=$?"
```

Erwartet: `rc=0`.

## 200 Feedback-Server (Subdomain, Token, Instanz)

Steht vollständig in `docs/andre-handgriffe.md` (kommt mit #14 nach main), Blöcke:

| Block | Was | Host |
|---|---|---|
| 101–120 | DNS `feedback.wironair.de` A/AAAA auf den VPS | Notebook, DNS-Verwaltung |
| 201–230 | Fine-grained Token `daw-woa-feedback`, nur Issues:write auf daw.woa, liegt nur auf dem VPS | Browser, VPS |
| 301–330 | Zweite Instanz per Ansible, vhost, Zertifikat | Notebook, VPS |
| 401–420 | Gegenprobe per curl, Test-Issue schließen | Notebook |

Vor 301: der Patch aus `server/README.md` muss in bretteins.infra sein. Das ist ein Ticket
für Dave; Claude legt es an, sobald du „ja“ sagst.

### 290 · Host: Notebook
Danach die App gegen den Server prüfen:

```bash
cargo run -q -p woa-headless -- feedback-test --url https://feedback.wironair.de/api/feedback; echo "rc=$?"
cargo run -q -p woa-headless -- log-test --url https://feedback.wironair.de/ingest; echo "rc=$?"
```

Erwartet: zweimal `rc=0`, ein neues Issue in daw.woa. Damit ist #1 (M0) abnehmbar.

## 300 Entscheidungen (nur antworten, Claude setzt um)

| Nr | Frage | Vorschlag |
|---|---|---|
| 301 | Lizenz CDLA-Permissive-2.0 für `webpki-roots` erlauben? | ja (nur Zertifikatsliste, GPL-verträglich) |
| 302 | Pan-Gesetz | Constant-Power (-3 dB Mitte), so gebaut |
| 303 | SoundFont | GeneralUser GS (frei, Lizenz kommt nach `docs/`) |
| 304 | Merge-Erlaubnis für Claude in diesem Repo (siehe 600) behalten? | ja |

## 400 Audio am echten Gerät (M1-Abnahme)

### 401 · Host: Notebook (Kopfhörer an)

```bash
cargo run -q --release -p woa-headless --features device -- play --buffer 128 --seconds 60; echo "rc=$?"
```

Erwartet: 60 s sauberer Ton, am Ende `underruns=0`. Zahl an Claude, dann ist #2 abnehmbar.
Heißt der Unterbefehl anders: `cargo run -q -p woa-headless -- --help`.

## 500 Oberfläche ansehen

### 501 · Host: Notebook

```bash
cargo run -q --release -p woa
```

Spur anlegen, Undo, Speichern, Feedback-Knopf. Eindruck in einem Satz an Claude.

### 510 · Host: Windows-Rechner
CI baut Windows schon grün; ein Blick am echten Bildschirm genügt:

```bash
git clone https://github.com/nixblick/daw.woa && cd daw.woa && cargo run --release -p woa
```

## 600 Merge-Erlaubnis und Worktree-Regel (nur du darfst das)

Claude darf Rechte und `CLAUDE.md` nicht selbst ändern (Klassifizierer: „Self-Modification“).

### 601 · Host: Notebook
Merge-Erlaubnis für dieses Repo:

```bash
mkdir -p .claude && vi .claude/settings.json
```

Inhalt:

```json
{ "permissions": { "allow": ["Bash(gh pr merge:*)"] } }
```

Ob der Auto-Modus „Merge without review“ trotzdem sperrt, zeigt erst der nächste Lauf.

### 610 · Host: Notebook
`vi CLAUDE.md`, unter „Arbeitsweise“ nach Punkt 9 anhängen:

```
10. Agenten in Worktrees: höchstens zwei Rust-Builds gleichzeitig, je eigener `target/`, nach
    Abschluss `cargo clean`. Vorher `df -h /home`; unter 10 GB frei kein Start.
11. Merge durch Claude nur mit Merge-Commit (`gh pr merge --merge`) und nur bei grüner CI.
```

Ein gemeinsamer `CARGO_TARGET_DIR` (mein Vorschlag von heute Nacht) ist **falsch**: Er lieferte
am 29.10. in fotobrett fremde Test-Binaries. Deshalb je Worktree eigener Ordner plus Aufräumen.

### 620 · Host: Notebook

```bash
git add .claude/settings.json CLAUDE.md && git commit -m "chore: merge permission, worktree rule" && git push
```

## 700 Platte

### 701 · Host: Notebook

```bash
df -h /home; du -sh ~/GitHub/nixblick/*/target ~/GitHub/nixblick/*/app/target 2>/dev/null | sort -h | tail -8
```

Erwartet: Übersicht der größten Build-Ordner. `cargo clean` dort, wo du gerade nicht
baust, gibt sofort Platz frei (nur Zwischenstände, kein Code).
