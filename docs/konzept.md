# WOA – Wir on Air: Radio-DAW in Rust – Konzept

Oct 28, 2026 · @André

## Vision & Anspruch

WOA ist eine vollwertige, professionelle Digital Audio Workstation mit eingebautem Radio-Studio und Party-Player, gebaut in Rust, für Linux und Windows. Sie wird komplett umgesetzt, alle Meilensteine dieses Dokuments von Anfang bis Ende, egal ob das Wochen, Monate oder Jahre dauert. Es gibt kein abgespecktes MVP als Ziel; die Reihenfolge der Meilensteine dient nur dazu, dass jeder Schritt auf einem stabilen Fundament aufbaut.

**Leitprinzipien:**

- Profi-Qualität: stabil im Live-Betrieb, knackfreies Audio, saubere Übergänge, keine Abstürze
- Drei vollwertige Modi (Produzent, Radio, Party) auf einer gemeinsamen Engine
- Mensch und Agent gleichberechtigt: alles, was die Oberfläche kann, kann auch ein KI-Agent über MCP, und umgekehrt
- Von Anfang an messbar: umfassendes technisches Logging, Feedback-Button, automatische Tickets
- Offen: GPLv3, öffentliches GitHub-Repo, kostenloser Download über wironair.de

Name: **WOA – Wir on Air**, nach dem seit 20 Jahren laufenden Internetradio von [wironair.de](https://www.wironair.de).

## Die drei Modi

Ein Umschalter oben in der Leiste wechselt den Modus. Jeder Modus hat ein eigenes Layout (Workspace) mit eigenen Presets; Engine, Bibliothek und Projekt bleiben dieselben, Audio läuft beim Umschalten unterbrechungsfrei weiter.

| Modus | Layout | Presets |
| --- | --- | --- |
| Produzent | Arrangement-Timeline, Piano-Roll, Mixer, Instrumente, Effekte, Analyse-Fenster (Loudness, Spektrum, Stereo-Scope) | Studio-Monitoring, kein Limiter auf dem Master |
| Radio | Playlist, Jingle-/Cart-Wall, Mikro- und Gastkanäle, Stream-Status, Sendeplan, Uhr mit Restzeiten | Sendebus-Processing, Ducking an, Mitschnitt an |
| Party | Große Now-Playing-Anzeige, Playlist, wenige große Tasten, Wunschliste | Auto-Crossfade, Lautheitsangleich, Bedienung per Handy oder Tablet im WLAN |

Layouts sind frei anpassbar, als eigene Presets speicherbar und per Tastenkürzel, MIDI-Taste oder Agent-Tool `set_mode` umschaltbar.

## Funktionsumfang

### Produzent: klassische DAW

- Audio-, MIDI-, Bus- und Gruppenspuren; Arrangement-Timeline mit Clips, Schnitt, Fades, Crossfades, Zeitdehnung und Tonhöhenänderung
- Piano-Roll mit Velocity, Quantisierung, Akkord- und Skalenhilfen; Drum-Editor
- Audio-Aufnahme mehrspurig mit Latenzkompensation, Punch-in, Loop-Takes und Comping
- Mixer: Inserts, Sends, Busse, Sidechain, Volume/Pan, Automation für jeden Parameter
- Eingebaute Instrumente: Sampler (SFZ), Drum-Sampler, Synthesizer, SoundFont-Player (General MIDI)
- Eingebaute Effekte: EQ, Kompressor, Limiter, Gate, De-Esser, Reverb, Delay, Chorus, Sättigung, Rauschunterdrückung
- Fremd-Plugins: CLAP-Hosting (z. B. Surge XT, Vital, Dexed, Odin 2), später zusätzlich VST3
- Tempo- und Taktartwechsel, Metronom, Marker
- Analyse: LUFS (integriert, kurzzeitig, momentan), True Peak, Spektrum, Spektrogramm, Stereo-Scope, Korrelation
- Export: Mixdown, Stems, MIDI, DAWproject

### Radio: Sendestudio

- Shoutcast-Ausspielung (v2, eigener Server) und Icecast als Alternative; Encoding MP3 oder AAC, bei Icecast auch Opus; Titel-Metadaten automatisch
- Playlist-Player mit Crossfade, Auto-Gain auf gleiche Lautheit, Vorhören im Kopfhörer (Cue)
- Jingle-/Cart-Wall: frei belegbares Tastenraster für Jingles, Werbung, Sounds; per Maus, Tastatur oder MIDI-Controller
- Mikrofonkanal mit Gate, EQ, Kompressor, De-Esser und automatischem Ducking der Musik
- Gäste live zuschalten per Browser-Link (WebRTC), je Gast ein Mixer-Kanal mit Mix-Minus
- Sendebus-Processing: Kompressor und Limiter, Zielwert z. B. -16 LUFS
- Mitschnitt der ganzen Sendung als WAV/FLAC
- Sendeplan mit Zeitachse, Restzeit pro Titel und bis zum nächsten festen Punkt
- Fallback: fällt Mikro, Playlist oder Oberfläche aus, spielt automatisch eine Notfall-Playlist
- Cue-Punkte je Titel: Cue-In, Cue-Out, Ramp (Intro-Countdown für die Moderation), Hook, Fade-Out-Marker; Crossfade und Restzeit richten sich danach
- Vorhören (PFL) auf eigenem Ausgang: Kopfhörer auf Kanal 3/4, während Kanal 1/2 sendet
- Stundenuhr und AutoDJ: Rotation nach Kategorien (z. B. Hit, Neu, Jingle, Moderation), feste Zeit-Events (Nachrichten, Werbung, Stundenjingle), Sperrfristen gegen Wiederholung
- Voice-Tracking: Moderation vorab einsprechen, zwischen zwei Titeln mit Blick auf Outro und Intro einpassen
- Jingle- und Spot-Vorlagen (z. B. 10, 20, 30 s) mit Bett-Musik, Sprecherspur und Radio-Sprachkette; Export direkt in die Cart-Wall

### Party: Abspielmodus

- Große, gut lesbare Oberfläche, Auto-Crossfade, Lautheitsangleich
- Wunschliste und Fernbedienung über eine lokale Web-Oberfläche im WLAN

### Mitschnitt in allen Modi

In jedem Modus gibt es einen dauerhaft sichtbaren Aufnahme-Knopf, der den Gesamtausgang mitschneidet: die kreative Session im Produzenten-Modus, die Testsendung im Radio-Modus, das live gemischte DJ-Set im Party-Modus.

- Aufgenommen wird der fertige Mix inklusive Mikro, zugeschalteter Gäste, Jingles und Musik
- Formate wählbar: WAV, MP3 oder beides gleichzeitig
- Optional zusätzlich Einzelspuren (Mikro, jeder Gast, Musik, Jingles), um die Sendung später nachzubearbeiten
- Absturzsicher: Aufnahme schreibt fortlaufend auf die Platte und läuft im Stream-Prozess weiter; nach einem Absturz bleibt die Datei bis zur letzten Sekunde nutzbar
- Marker während der Aufnahme setzen (z. B. „Jingle“, „Gast 2“), Titelliste der gespielten Songs als Textdatei daneben
- Mitschnitte landen automatisch in der Bibliothek und lassen sich direkt im Produzenten-Modus öffnen

### Gemeinsam

- Bibliothek für Musik, Samples, Jingles: Suche, Tags, Vorhören, BPM- und Tonart-Erkennung, Lautheitsanalyse
- Eingabegeräte: USB-MIDI-Keyboards (z. B. Yamaha, class compliant, ohne Treiber), MIDI-Controller mit MIDI-Learn für Fader, Regler, Pads
- Formate lesen: WAV, AIFF, FLAC, MP3, AAC/M4A, Ogg Vorbis, Opus, ALAC; schreiben: WAV/FLAC bis 32 Bit Float, MP3, Opus, Ogg, AAC (über FFmpeg)
- Instrumente und Sounds aus freien Quellen (SFZ-Bibliotheken wie VSCO Community Edition, DrumGizmo-Kits, GeneralUser GS, Freesound), Lizenz je Quelle geprüft und im Repo dokumentiert

## Architektur

GUI und Agent schreiben nie direkt in die Engine. Beide schicken Commands an eine gemeinsame Schicht; daraus entstehen Undo, Projektdatei und der Audio-Graph.

&#91;embedded content: Architektur · 6 Schichten, GUI und Agent gleichberechtigt\]

Zusätzlich zum Bild: Der Stream-Encoder und die Notfall-Playlist laufen als eigener Prozess (`woa-streamd`), damit ein Absturz der Oberfläche die Sendung nicht beendet. Party-Fernbedienung und Gäste-Schalte hängen über einen lokalen Webserver an derselben Command-Schicht.

## Schnittstellen

| Schnittstelle | Zweck | Technik |
| --- | --- | --- |
| MCP-Server | KI-Agenten (Claude Code, Claude Desktop, Gemini CLI) bedienen WOA vollständig | `rmcp` |
| Shoutcast / Icecast | Live-Stream an eigenen Server | eigener Source-Client auf `tokio` |
| MIDI | Keyboards, Controller, MIDI-Learn, MIDI-Dateien | `midir`, `midly` |
| Audio-I/O | Interfaces, Mikro, Kopfhörer-Cue | `cpal`; PipeWire/JACK unter Linux, WASAPI/ASIO unter Windows |
| Plugins | Fremd-Instrumente und Effekte | CLAP via `clack`, später VST3 |
| Gäste-Schalte | Mikro von Freunden per Browser | WebRTC (`webrtc-rs`) |
| Party-Fernbedienung | Wünsche und Steuerung per Handy | lokaler Webserver (`axum`) |
| Feedback | Lob, Kritik, Bugs mit Screenshot aus der App | HTTPS an eigenen Server, der GitHub-Issues erzeugt |
| Telemetrie | technische Logs aller Installationen | HTTPS-Upload an eigenen Server |
| Auto-Updater | neue Versionen prüfen und installieren | signierte MSI- bzw. AppImage-Pakete |
| Austausch | Projekte mit anderen DAWs | DAWproject, Stems, MIDI |

### MCP-Tools für Agenten

Der Agent bekommt dieselben Commands wie die Oberfläche. Weil er nicht hört, misst er: Render plus Analyse ersetzen das Ohr.

| Bereich | Tools |
| --- | --- |
| Projekt | `get_project`, `create_track`, `delete_track`, `set_tempo`, `undo`, `redo` |
| Komposition | `write_notes`, `import_audio`, `insert_effect`, `load_instrument`, `set_param`, `add_automation` |
| Ergebnis | `transport`, `render`, `analyze` (LUFS, True Peak, Spektrum, Stereo), `spectrogram_png` |
| Radio | `build_playlist`, `make_jingle`, `normalize_library`, `run_order`, `stream_status` |
| Bedienung | `set_mode`, `get_library`, `search_library` |

Regeln: musikalische Einheiten (Beats, Takte, Notennamen) statt Samples; jeder Agent-Call ist ein Undo-Schritt mit Label „Agent“; `analyze` liefert Zahlen plus kurze Bewertung. Beispiel: „Mach einen 15-Sekunden-Jingle für Wir on Air“ → Agent legt Projekt an, komponiert, rendert, misst Länge und Lautheit, bessert nach; der Mensch sieht alles live und arbeitet danach selbst weiter.

## Tech-Stack

Alles Rust im Kern. Versionen und Pflegestand vor Start prüfen; die Liste ist nicht frisch recherchiert.

| Baustein | Wahl |
| --- | --- |
| Audio-I/O | `cpal` (+ `jack` für niedrige Latenz unter Linux, ASIO unter Windows) |
| MIDI | `midir`, `midly` |
| Echtzeit-Kommunikation | `rtrb`, `arc-swap`, `basedrop` |
| Decode / Encode | `symphonia`, `hound`, `flac-bound`, `mp3lame-encoder`, `opus`, FFmpeg für AAC |
| DSP / Analyse | eigene Module, `fundsp`, `rustfft`/`realfft`, `ebur128`, `nnnoiseless`, `rubato` (Resampling) |
| GUI | `egui`/eframe (Timeline, Meter, Wellenformen selbst gezeichnet) |
| Plugins | `clack` (CLAP-Host), `nih-plug` für eigene Effekte |
| Netzwerk | `tokio`, `axum`, `webrtc-rs`, `reqwest` |
| Agent-API | `rmcp` |
| Projekt & Daten | `serde` + JSON, SQLite (`rusqlite`) für die Bibliothek |
| Logging | `tracing`, `tracing-subscriber`, `tracing-appender` |
| Qualität | `assert_no_alloc`, `cargo-deny`, `clippy`, `insta` |
| Pakete | `cargo-wix` (MSI), `cargo-appimage`, Flatpak-Manifest |

## Echtzeit- und Stabilitätsregeln

Diese Regeln stehen wörtlich in der `CLAUDE.md` und gelten für jede Zeile Code.

1. Im Audio-Thread: keine Allokation, kein Mutex, kein I/O, kein Logging.
2. Kommunikation nur über lock-free Queues (`rtrb`).
3. Struktur-Änderungen baut der Control-Thread als neuen Graphen, atomarer Tausch (`arc-swap`), Freigabe außerhalb des Audio-Threads (`basedrop`).
4. Buffer vorab allokieren; Parameter glätten gegen Zipper-Noise.
5. Tests mit `assert_no_alloc` um jeden Process-Call.
6. Kein `unwrap()` außerhalb von Tests; jeder Fehler wird geloggt und sauber behandelt.
7. Autosave alle paar Sekunden, Wiederherstellung nach Absturz.
8. Stream-Prozess getrennt von der Oberfläche; Notfall-Playlist bei Ausfall.
9. Vor jedem Release ein Dauertest von mindestens 8 Stunden Radio-Betrieb ohne Underrun.

## Logging, Telemetrie & Datenschutz

Logging ist Kernbaustein ab der ersten Zeile. Claude Code wertet die Daten aller Installationen regelmäßig aus und findet Fehler, bevor sie jemand meldet.

- **Erfasst (nur technisch):** Abstürze mit Stacktrace, Errors, Warnings; Zeiten für Start, Laden von Instrumenten, Speichern, Rendern, Export; Buffer-Underruns, CPU-Last, Latenz; Stream-Abbrüche und Reconnects; Installations-ID (zufällige UUID), Betriebssystem, App-Version, Audio-Backend
- **Nie erfasst:** Inhalte, Dateinamen, Titel, Anzahl erstellter Dateien, Nutzungsverhalten, IP-Adressen
- **Technik:** `tracing` mit Spans für Zeitmessung, lokal rotierende JSON-Logs, gebündelter komprimierter Upload an den eigenen Server (Hetzner-VPS oder Goneo)
- **DSGVO:** Opt-in beim ersten Start, jederzeit abschaltbar, Datenschutzerklärung auf wironair.de, begrenzte Speicherfrist, Server in der EU
- **Auswertung:** regelmäßiger Claude-Code-Lauf über die gesammelten Logs, Ergebnis als GitHub-Issues

### Feedback-Button (Pflicht)

Wie in Bretteins und Mein Bilderschatz: Lob, Kritik oder Bug direkt aus der App, mit Screenshot. Das Feedback geht an einen Endpunkt auf dem Hetzner-VPS, der daraus ein GitHub-Issue erstellt; keine GitHub-Zugangsdaten in der App. Die Lösung wird aus den bestehenden Repos übernommen. Das ist der Haupt-Testweg: Screenshot machen, Hinweis schreiben, Ticket entsteht, KI-Agenten arbeiten es ab.

## Repository-Struktur

Repository-Name: **daw.woa** (DAW + Wir on Air), öffentlich auf GitHub.

```
daw.woa/
├── Cargo.toml              # Workspace
├── CLAUDE.md               # Regeln, Architektur, RT-Verbote, Arbeitsweise
├── LICENSE                 # GPLv3
├── crates/
│   ├── model/              # Projekt, Spuren, Clips, Commands, Undo
│   ├── engine/             # Audio-Graph, RT-Thread, Transport, Aufnahme
│   ├── dsp/                # Instrumente, Effekte, Analyse
│   ├── io/                 # Audio-/MIDI-Geräte, Datei-Import/-Export
│   ├── library/            # Bibliothek, Analyse, Suche (SQLite)
│   ├── plugins/            # CLAP-Host
│   ├── radio/              # Playlist, Cart-Wall, Sendeplan, Ducking
│   ├── stream/             # Encoder, Shoutcast/Icecast-Client
│   ├── guests/             # WebRTC-Gäste-Schalte
│   ├── remote/             # Party-Fernbedienung (Web)
│   ├── agent/              # MCP-Server
│   ├── telemetry/          # Logging, Upload, Feedback
│   ├── updater/            # Auto-Updater
│   └── gui/                # egui-Oberfläche, drei Layouts
├── apps/
│   ├── woa/                # Desktop-App
│   ├── woa-headless/       # Engine + MCP, für Agent und Tests
│   └── woa-streamd/        # Stream-Prozess mit Notfall-Playlist
├── server/                 # Endpunkte für Feedback und Telemetrie
├── packaging/              # MSI, AppImage, Flatpak
├── docs/                   # Router-/Stream-Anleitung, Lizenzen der Sounds
└── tests/golden/           # Referenz-Renders
```

## Umsetzungsplan

Alle Meilensteine werden umgesetzt. Jeder endet mit einem prüfbaren Abnahmekriterium; erst dann beginnt der nächste. Ab M3 ist WOA für die jährliche Sendung nutzbar, ab M6 als vollwertige DAW.

| Meilenstein | Inhalt | Abnahme |
| --- | --- | --- |
| M0 Fundament | Workspace, CLAUDE.md, CI, Logging, Feedback-Button, Projektmodell, Commands mit Undo, Speichern/Laden | Projekt-Roundtrip, Feedback erzeugt GitHub-Issue, Logs kommen auf dem Server an |
| M1 Engine | Audio-Graph, Transport, Geräteauswahl, Sampler, Synth, SoundFont-Player, Offline-Render | Golden-Render stimmt, keine Underruns bei 128 Samples |
| M2 Agent | MCP-Server mit allen Projekt-, Kompositions- und Analyse-Tools | Claude baut allein einen 8-Takt-Loop mit -14 LUFS |
| M3 Radio | Playlist-Player, Crossfade, Cart-Wall, Mikro mit Ducking, Encoder, Shoutcast/Icecast, Mitschnitt, Notfall-Playlist, Radio-Layout | 8 Stunden Testsendung ohne Abbruch |
| M4 MIDI & Eingabe | USB-Keyboard, MIDI-Aufnahme, Piano-Roll, MIDI-Learn für Controller; danach vorgezogen aus M7: Cue-Punkte je Titel und Vorhören (PFL) auf eigenem Ausgang (Paket 15, André 30.10.2026) | Yamaha-Keyboard spielt und nimmt auf, Controller steuert Mixer und Cart-Wall |
| M5 Mixer & Effekte | Busse, Sends, Sidechain, Automation, alle eingebauten Effekte, Analyse-Fenster | ganzer Jingle inklusive Sprachkette im Produzent-Layout |
| M6 Arrangement & Aufnahme | Timeline-Editor, Schnitt, Fades, Audio-Aufnahme mit Latenzkompensation, Comping, Time-Stretch | ganzer Song von Aufnahme bis Export |
| M7 Bibliothek & Formate | Bibliothek in SQLite (`crates/library`) mit Analyse (Cue-Punkte und PFL schon nach M4 vorgezogen, hier nur noch automatische Cue-Punkte aus der Analyse), alle Import-/Exportformate, Stems, DAWproject | 1.000 Titel importiert und lautheitsangeglichen |
| M8 Plugins | CLAP-Hosting mit Plugin-GUIs, Absturzschutz für Plugins | Surge XT und Vital laufen stabil im Projekt |
| M9 Gäste, Automation & Party | Stundenuhr und AutoDJ-Rotation, Voice-Tracking-Editor, WebRTC-Gäste-Schalte (`crates/guests`) mit Mix-Minus, Party-Layout, Web-Fernbedienung | Sendung mit zwei zugeschalteten Gästen; Party-Abend per Handy gesteuert |
| M10 Release | MSI und AppImage/Flatpak, Auto-Updater, Datenschutzerklärung, Doku, Download auf wironair.de | Installation auf frischem Windows und Linux, Update von Version zu Version |
| M11 Ausbau | VST3, macOS (wenn testbar), weitere Instrumente, Feinschliff aus Telemetrie und Feedback | laufend |

## Arbeitsweise mit Claude Code

1. Dieses Dokument liegt als `docs/konzept.md` im Repo; `CLAUDE.md` verweist darauf und enthält die Regeln.
2. Gearbeitet wird in Nutzer-Paketen, nicht in Einzelfunktionen: Claude baut ein Paket am Stück, André nimmt es einmal ab (`docs/abnahme.md`). Die Meilensteine bleiben die Landkarte, Pakete dürfen quer daraus ziehen (29.10.2026).
3. Kleine Commits, ein Feature pro Branch, Merge nur bei grüner CI (`clippy -D warnings`, `test`, `fmt --check`, `cargo-deny`).
4. Golden-File-Tests für den Klang, `assert_no_alloc` für Echtzeit, Snapshot-Tests für das Projektformat.
5. Testen per Feedback-Button: Screenshot plus Hinweis wird Issue, Agent arbeitet Issues ab.
6. Regelmäßige Telemetrie-Auswertung durch Claude Code; Auffälligkeiten werden Issues.
7. Ab und zu Architektur-Review per Claude, damit die Codebasis verständlich bleibt.

## Verteilung & Lizenz

- Lizenz: GPLv3, öffentliches GitHub-Repo; Copyright-Hinweis bleibt in jeder Weitergabe
- Plattformen: Windows nur als MSI, Linux als AppImage und Flatpak; macOS zurückgestellt
- Download über wironair.de, Auto-Updater in der App
- Lizenzen aller Crates per `cargo-deny`, Lizenzen aller mitgelieferten Sounds in `docs/`

## Stream und Rechtliches

WOA gibt selbst nie einen Link nach außen. Ein Knopf startet die Verbindung zum eigenen Shoutcast-Server; WOA zeigt die Hör-Adresse (IP, Port, Playlist-Link) zum Kopieren an. Die Freigabe nach außen (Router, DynDNS) macht der Nutzer selbst; eine Anleitung liegt in `docs/`.

Der Stream ist für Teststreams und geschlossene Runden gedacht: standardmäßig passwortgeschützt, keine Eintragung in öffentliche Verzeichnisse. Beim ersten Einrichten erscheint ein Hinweis: Wer fremde Musik öffentlich streamt, muss Urheberrechte und GEMA selbst klären; der Link darf nicht öffentlich geteilt werden. Intern gilt trotzdem der Qualitätsanspruch eines echten Radiosenders. Keine Rechtsberatung; vor dem Release juristisch gegenlesen lassen.

## Offene Entscheidungen

- Server für Telemetrie und Feedback: Hetzner-VPS oder Goneo
- GUI-Framework final bestätigen (`egui` als Vorschlag) nach einem kurzen Prototyp in M0
