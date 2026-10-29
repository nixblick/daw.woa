# Runbook André: Nachtbau 29.10.2026 übernehmen

Zum Durchziehen: Block für Block kopieren, Reihenfolge = Ausführung. Alles auf **Host: Notebook**,
ein Terminal. Kein Geheimniswert steht hier. Bricht ein Block ab: stehen lassen, Ausgabe an Claude.

## 100 Mergen (ca. 5 Min.)

### 101 · Host: Notebook
Vorbelegen und prüfen. Erwartet: jede Zeile nur `SUCCESS`.

```bash
R=nixblick/daw.woa
cd ~/GitHub/nixblick/daw.woa
gh pr list -R $R --json number,title,statusCheckRollup --jq '.[]|"\(.number) \([.statusCheckRollup[].conclusion]|unique|join(",")) \(.title)"'
```

### 110 · Host: Notebook
#21 ist schon gemergt. #17 hat danach einen Konflikt, deshalb geht der Rest über den
Integrationszweig #22: er enthält #14, #15, #17, #18, #19, #21 konfliktfrei (lokal geprüft,
Tests grün). GitHub markiert die Einzel-PRs danach selbst als gemergt.
Erwartet: `✓ Merged pull request nixblick/daw.woa#22`.

```bash
gh pr merge 22 -R $R --merge
```

### 120 · Host: Notebook
M1, M2 und Runbook nachziehen. Bei #24 und #25 ist der CHANGELOG-Konflikt schon gelöst.
Vorher kurz warten, bis die CI auf #24/#25 wieder grün ist (`gh pr checks 25 -R $R --watch`).
Erwartet: vier Zeilen `✓ Merged …`, am Ende `fertig`.

```bash
gh pr checks 25 -R $R --watch >/dev/null; for n in 23 24 25 26; do gh pr merge $n -R $R --merge || { echo "STOPP bei #$n"; break; }; sleep 5; done; echo fertig
```

### 130 · Host: Notebook
CI auf main abwarten (dauert einige Minuten). Erwartet: alle Jobs grün, am Ende `rc=0`.

```bash
sleep 20; gh run watch -R $R "$(gh run list -R $R -b main -L 1 --json databaseId --jq '.[0].databaseId')" --exit-status; echo "rc=$?"
```

## 200 Lokal prüfen (ca. 5 Min., erster Build dauert)

### 201 · Host: Notebook
main holen und M0 per CLI abnehmen: Projekt anlegen, zwei Spuren, Speichern/Laden vergleichen.
Erwartet: `Info` zeigt zwei Spuren, am Ende `rc=0`.

```bash
git switch main && git pull --ff-only && rm -rf /tmp/abnahme.woa && cargo run -q -p woa-headless -- new /tmp/abnahme.woa && cargo run -q -p woa-headless -- add-track /tmp/abnahme.woa Sprecher && cargo run -q -p woa-headless -- add-track /tmp/abnahme.woa Jingle --kind midi && cargo run -q -p woa-headless -- info /tmp/abnahme.woa && cargo run -q -p woa-headless -- roundtrip /tmp/abnahme.woa; echo "rc=$?"
```

### 210 · Host: Notebook
Oberfläche ansehen: Spur anlegen, Undo, Speichern, Feedback-Knopf. Eindruck in einem Satz an Claude.

```bash
cargo run -q --release -p woa
```

## 300 Antworten an Claude (ein Wort je Zeile genügt)

| Nr | Frage | Vorschlag |
|---|---|---|
| 301 | Lizenz CDLA-Permissive-2.0 für `webpki-roots` erlauben? | ja (nur Zertifikatsliste, GPL-verträglich) |
| 302 | Pan-Gesetz Constant-Power (-3 dB Mitte), so gebaut, ok? | ja |
| 303 | SoundFont GeneralUser GS (frei)? | ja |
| 304 | Ticket an Dave für den Feedback-Server-Patch in bretteins.infra? | ja |

## 400 Merge-Erlaubnis und Worktree-Regel (nur du darfst das, ca. 3 Min.)

Claude darf Rechte und `CLAUDE.md` nicht selbst ändern.

### 401 · Host: Notebook
Erlaubnis-Datei schreiben (ohne Editor):

```bash
mkdir -p .claude && printf '%s\n' '{ "permissions": { "allow": ["Bash(gh pr merge:*)"] } }' > .claude/settings.json && cat .claude/settings.json
```

### 410 · Host: Notebook
`vi CLAUDE.md`, unter „Arbeitsweise“ nach Punkt 9 anhängen:

```
10. Agenten in Worktrees: höchstens zwei Rust-Builds gleichzeitig, je eigener `target/`, nach
    Abschluss `cargo clean`. Vorher `df -h /home`; unter 10 GB frei kein Start.
11. Merge durch Claude nur mit Merge-Commit (`gh pr merge --merge`) und nur bei grüner CI.
```

### 420 · Host: Notebook

```bash
git add .claude/settings.json CLAUDE.md && git commit -m "chore: merge permission, worktree rule" && git push
```

## 500 Später, nicht heute früh

- **Feedback-Server** (Subdomain `feedback.wironair.de`, Token, zweite Instanz, Gegenprobe):
  steht nach dem Merge in `docs/andre-handgriffe.md`. Geht erst, wenn Dave den Patch in
  bretteins.infra hat (Antwort 304). Danach `woa-headless feedback-test` und `log-test`,
  damit ist #1 (M0) abnehmbar.
- **Audiotest am Gerät (M1, #2):** Einen Abspiel-Befehl gibt es noch nicht, der Underrun-Zähler
  steckt nur in der Engine. Claude baut `woa-headless play` als Nächstes, dann kommt hier ein Block.
- **Windows:** CI baut grün. Bei Gelegenheit am echten Bildschirm:
  `git clone https://github.com/nixblick/daw.woa && cd daw.woa && cargo run --release -p woa`
- **Platte:** `df -h /home`. `cargo clean` in Repos, in denen du gerade nicht baust, gibt Platz frei.
