# WOA – Wir on Air

**Radio-Studio, DJ-Pult und Musikproduktion in einer App. Frei, offen, für Linux und Windows.**

WOA ist eine Digital Audio Workstation mit eingebautem Sendestudio. Du produzierst deinen Jingle,
legst ihn auf die Cartwall und gehst damit live – ohne zwischen drei Programmen zu wechseln.
Benannt nach dem Internetradio [wironair.de](https://www.wironair.de), dessen Moderatoren WOA im
Alltag testen.

[**⬇ Neueste Version herunterladen**](https://github.com/nixblick/daw.woa/releases/latest) ·
Windows (MSI/ZIP) · Linux (AppImage/Flatpak) · eingebauter Updater

---

## Drei Modi, ein Programm

### 🎙 Radio – senden wie ein Profi
- **Sende-Deck mit Master-Uhr**, Playlist mit Lautheit je Titel und Restzeit bis Playlist-Ende
- **Decks A, B, C** und eine **Cartwall mit 16 Tasten** für Jingles und Sounds
- **Vorhören (PFL)** auf einem zweiten Gerät, ein Klick startet, der zweite stoppt
- **Automatische Lautheit** auf -16 LUFS, Crossfade, **Musik-Ducking** beim Mikro
- **LED-Pegel** an jedem Fader und für das, was wirklich rausgeht
- **Stream direkt aus der App** zu deinem Shoutcast-Server – über einen eigenen Sendedienst,
  der weitersendet, selbst wenn die Oberfläche abstürzt
- **Gäste per Browser-Link** ins Studio holen, mit Warteraum und Stummschalten
- **REC** schneidet die Sendung mit, egal in welchem Modus

### 🎛 Produzent – Musik machen
- Audio- und MIDI-Spuren, Piano-Roll, Studio-Mixer, Automation, Busse
- CLAP-Plugins, Aufnahme mit Latenzausgleich, Loop und Punch
- Export als Mixdown oder Stems, auf Wunsch gleich auf Sendelautheit

### 🪩 Party – auflegen
- Zwei Decks mit echtem Crossfader und **Auto-Übergang**
- **Wunschliste**: Gäste legen Titel in einen Ordner, WOA zeigt Interpret und Titel aus den Tags

## KI, die mitarbeitet

WOA spricht **MCP**. Verbinde Claude Desktop oder Claude Code mit der laufenden App, und die KI
baut Beats, legt Spuren an und mischt – live im offenen Projekt. Alternativ chattest du direkt im
Produzenten-Modus mit Anthropic, OpenAI, OpenRouter oder lokal mit Ollama bzw. LM Studio.
Einrichtung: *Einstellungen → KI*, Anbieter wählen, „JSON kopieren“, fertig.

## Gebaut mit Sorgfalt

- **Rust** von vorn bis hinten, Audio-Pfad ohne Locks und ohne Speicherzuteilung
- Läuft auch auf **alten Laptops** – getestet auf einem rund 20 Jahre alten Windows-Rechner
- Jede Änderung mit Tests, jede Version mit [Changelog](CHANGELOG.md)
- **Feedback direkt aus der App**, mit Screenshot und Protokoll – Wünsche landen oft am selben Tag
  in einer neuen Version

## Wer dahinter steht

WOA wird von **André Hinz** entwickelt, IT-Freelancer und Gründer von
[**creaVisions**](https://www.creavisions.de). Er baut Software, die Menschen wirklich benutzen:
neben WOA die Schach-App [bretteins](https://www.bretteins.de) und die Foto-Software
[Main BilderSchatz](https://www.mainbilderschatz.de). Entwickelt wird mit einem Team aus
KI-Mitarbeitern – schnell, getestet und offen.

Du betreibst ein Webradio, brauchst eine Anpassung oder willst WOA in deinem Studio einsetzen?
Schreib über [creavisions.de](https://www.creavisions.de).

## Mitmachen

- Fehler und Wünsche: am liebsten über **Feedback** in der App, sonst als
  [Issue](https://github.com/nixblick/daw.woa/issues)
- Konzept: [`docs/konzept.md`](docs/konzept.md) · Senden: [`docs/stream.md`](docs/stream.md) ·
  KI/MCP: [`docs/agent.md`](docs/agent.md)
- Lizenz: **GPLv3**, siehe [`LICENSE`](LICENSE)

## Geheimnisse

Bisher keine. Vault: `~/.secrets/daw.woa/`.
