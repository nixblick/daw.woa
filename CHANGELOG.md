# Changelog

## 0.3.4 — 2026-10-29

- **Oberfläche ohne Dauerlast (Fixes #56). Hauptursache ist die Ereignisschleife unter Wayland.** eframe 0.35 setzt bei jedem fälligen Repaint `ControlFlow::Poll`. Unter Wayland kommt das Neuzeichnen aber erst mit dem Frame-Callback des Compositors, und ein verdecktes Fenster (etwa hinter dem Browser) bekommt keinen. Dann dreht der UI-Thread leer in `epoll` und belegt einen ganzen Kern (egui#8326, behoben erst in eframe 0.37, das rustc 1.95 braucht; hier ist 1.92 installiert). Unter Linux nimmt WOA deshalb X11 (XWayland), solange `DISPLAY` gesetzt ist. `WOA_WAYLAND=1` erzwingt Wayland. Bei gebrochener Skalierung kann die Schrift unter XWayland etwas weicher aussehen. Gemessen am 29.10. im Radio-Modus, ohne Wiedergabe und ohne Sendung, je 20 s nach 8 s Anlauf: 0.3.2 86–102 %, 0.3.4 unter Wayland 24–91 % (je nach Fensterlage), 0.3.4 als Vorgabe (X11) 1 %. `WOA_REPAINT_DEBUG=1` schreibt alle 5 s Bilder je Sekunde und Repaint-Auslöser ins Log.
- **Cover-Überblendung ohne Dauer-Repaint.** Ein Titel ohne Cover-Bild startete die Überblendung des großen Covers in jedem Frame neu. Das löste `request_repaint()` ohne Pause aus, und der UI-Thread lief mit 100 % CPU, auch ohne Wiedergabe und ohne Sendung. Unter X11 gemessen, Radio-Modus, nichts abgespielt: 0.3.2 12 %, 0.3.4 1 %. Jetzt merkt sich die Überblendung das Ziel, auch wenn es kein Bild gibt. Sie fordert nur während der 0,4 s neue Bilder an, mit `request_repaint_after(16 ms)`. Der Radio-Modus zeichnet in Ruhe nur noch zweimal je Sekunde neu (vorher zehnmal), solange ein Titel läuft fünfmal.
- **Lokale Verbindung Oberfläche → woa-streamd bleibt stehen.** woa-streamd liest mit einer Frist von 500 ms. Lief sie mitten in einem Rahmen ab, ging dessen Rest verloren. Der nächste „Kopf“ bestand dann aus Audio-Bytes, und die Verbindung riss ab. Im Log stand alle 2–10 s „ui disconnected“, im Status `ui_connected:false`, und für diese Zeit lief die Notfall-Playlist. Neu ist `link::read_frame_patient`: Ein angefangener Rahmen wird zu Ende gelesen. Der Feeder versucht es mit Backoff von 1 s bis 10 s erneut statt fest jede Sekunde.
- **Sendeprozess versucht sichtbar weiter.** Ein Verbindungsversuch zum Server dauert höchstens 5 s für alle Adressen zusammen. Vorher waren es 5 s je Adresse, mit IPv6 und IPv4 also 10 s. Jeder fehlgeschlagene Versuch zählt in `reconnects`. Neu in `streamd-status.json` ist `retry_in_seconds`. Unter dem Sende-Knopf steht dann z. B. „Server nicht erreichbar (Port 8001 zu?), neuer Versuch in 30 s“. Je Versuch gibt es eine Logzeile, der Backoff geht bis 30 s. `connection:"beendet"` steht nur nach „Sendung beenden“; der Sendeprozess gab auch vorher nicht von selbst auf.
- **Stream-Server heißt jetzt `stream.wironair.de`.** Die Vorgabe für `host` ist `stream.wironair.de:8001` (Stream 1), gehört wird unter <https://stream.wironair.de/stream>. Ohne Quelle spielt der Server eine eigene Notfall-Schleife. Feedback und Telemetrie bleiben auf `feedback.wironair.de`. Angepasst: `docs/stream.md`, `docs/abnahme.md`.

## 0.3.3 — 2026-10-29

- **Aussetzer der Ausgabe protokollieren (#51).** Der Fehlerpfad des Stroms kann im Audio-Thread laufen. Er zählt Unterläufe jetzt nur noch, je Strom getrennt (Radio, Player), statt jede Meldung zu loggen; am 29.10. waren das 682 Zeilen. Im Log steht höchstens eine zusammengefasste Zeile je Minute (`output underruns (last minute)`). Sie nennt Anzahl, Gesamtzahl, Puffergröße, Samplerate und Gerätename. Beim Radio kommen dazu: Läuft der Mitschnitt? Läuft der Player-Strom gleichzeitig? Welcher Titel spielt, in welchem Format, und wie lange dauerte das letzte Dekodieren? Die Radio-Zeile kommt jede Minute, die Player-Zeile nur bei Aussetzern. Die Radio-Statuszeile zeigt dezent „Aussetzer: N“, sobald einer auftrat.
- **Zwei kleine Gegenmaßnahmen.** Im Radio-Modus ruht der Player-Strom (`pause_output`), damit nicht zwei Ströme auf demselben Gerät spielen. Beim Wechsel zurück läuft er weiter. Die Radio-Ausgabe nimmt die Puffergröße des Treibers statt fester 128 Frames. Einstellbar ist das in `config.json` mit `radio_buffer_frames` (fehlt oder 0 = Treiber) und bei `woa-headless radio-play --buffer`. `radio-play` meldet Aussetzer und verlorene Mitschnitt-Samples getrennt. Die eigentliche Ursache wird später behoben; die Vermutungen stehen in #51.

## 0.3.2 — 2026-10-29

- **Cover-Bild im Radio-Layout.** Neben der großen Uhr steht ein Quadrat (168 px) mit dem Cover des laufenden Titels (vor dem Start: des nächsten). Beim Titelwechsel blendet es in 0,4 s weich über. Ohne Bild steht dort eine dezente Note, nie ein leeres Loch. In der Playlist hat jede Zeile ein Vorschaubild (32 px).
- **Auslesen:** `woa_engine::cover::read_cover` holt das eingebettete Bild über die symphonia-Metadaten (ID3 APIC, MP4 `covr`, FLAC PICTURE, Vorbis `METADATA_BLOCK_PICTURE`), bevorzugt „Cover vorne“. Auslesen und Dekodieren laufen in einem eigenen Hintergrund-Thread (`woa-cover`), nie im Audio-Thread; die Oberfläche holt nur fertige Texturen ab.
- **Dekodieren:** über den `image`-Crate, der schon im Lockfile war, jetzt mit JPEG (`zune-jpeg`, ebenfalls schon im Lockfile) und PNG, ohne Standard-Features. Herunterskaliert auf 512 px (groß) und 64 px (Vorschau). Vorschaubilder bleiben im Cache, große Texturen nur für den laufenden und den vorigen Titel.
- **Speicherschutz:** Bilder über 10 MB oder über 8000 px Kante werden nicht angezeigt; symphonia liest größere Bilder gar nicht erst ein.
- Tests: Cover aus MP3 (selbst gebauter ID3-Tag), FLAC, M4A und OGG (per ffmpeg, ohne ffmpeg übersprungen); Datei ohne Bild, WAV und zu großes Bild ergeben kein Cover; PNG und JPEG werden dekodiert und skaliert, zu große und kaputte Bilder abgelehnt.

## 0.3.1 — 2026-10-29

- **MP3 mit großem Cover-Tag lesbar (Fixes #50).** Eine MP3 mit 1,2 MB großem ID3v2.3-Tag (APIC-Bild) scheiterte mit „no suitable format reader found“: symphonia lief ohne die Metadaten-Features und konnte den Tag nicht überspringen. Jetzt mit `id3v1`, `id3v2` und `ape`. Test mit selbst erzeugter MP3 und 1,2 MB APIC-Bild; Andrés Datei geht wieder (8 710 592 Frames, 44,1 kHz, Stereo).
- **Alle gängigen Formate lesen.** Neu über symphonia 0.6.1 (alle Teile MPL-2.0, `extended` MIT, ohne C-Abhängigkeit): OGG/Vorbis (`.ogg`, `.oga`), M4A/MP4 mit AAC oder ALAC (`.m4a`, `.mp4`), rohes AAC/ADTS (`.aac`), AIFF (`.aif`, `.aiff`, `.aifc`), CAF (`.caf`) und MP2 (`.mp2`). Opus geht weiter nicht: symphonia hat keinen Opus-Decoder.
- **Eine Liste für alle Endungen.** `woa_engine::decode::EXTENSIONS` (klein, Vergleich ohne Rücksicht auf die Schreibweise), `FILTER_NAME` und `dialog_extensions()` für die Dateidialoge der Radio-Playlist und der Cart-Wall; Drag-and-drop und Agent-Import prüfen über dieselbe Liste. Filtertext „Audio (MP3, FLAC, WAV, OGG, M4A, AAC, AIFF, CAF, MP2)“.
- Tests: Jedes neue Format wird mit ffmpeg aus dem Testton erzeugt, gelesen und auf Länge und Pegel je Kanal geprüft; 9 Kanäle in AIFF, CAF und M4A/ALAC werden abgelehnt (Speicherschutz aus #49). Ohne ffmpeg überspringen sich diese Tests mit Meldung.
- **Patent-Hinweis AAC:** Der AAC-Decoder ist reiner Rust-Code (MPL-2.0). AAC-Patente sind nach Kenntnis weitgehend ausgelaufen, das ist aber keine Rechtsauskunft; vor einem kommerziellen Vertrieb einmal prüfen.

## 0.3.0 — 2026-10-29

- **Sendeprozess `woa-streamd` (Paket 8).** Die Sendung geht an einen eigenen Shoutcast DNAS v2. `crates/stream` enthält:
  - den Quellclient im Shoutcast-1-Protokoll (Legacy): Kennwort-Zeile (ab Stream-ID 2 mit `:#sid`), `OK2`, `icy-*`-Kopfzeilen, dann MP3. Vorgabe ist der eigene Server `feedback.wironair.de:8001`, Stream 1 (bretteins.infra#150)
  - Titel per `admin.cgi?mode=updinfo` nur bei lokalem Server, weil dort das Kennwort im Klartext in der URL steht; gegen lfs001 vorerst keine Titel (v1 kann sie nicht in der Quellverbindung tragen). Hörerzahl aus `/7.html` nur mit `stream.admin_port`; der ist auf lfs001 von außen gesperrt. Die Quellanmeldung auf 8001 läuft im Klartext (bekanntes Risiko, `docs/stream.md`)
  - Wiederverbinden mit Backoff (1 s bis 30 s, nach abgelehntem Kennwort 60 s); während des Neuaufbaus werden höchstens 5 s gepuffert
  - MP3 CBR 128 kbit/s über LAME, 44,1 oder 48 kHz
  - die Notfall-Playlist: Nach 2 s ohne Ton von der Oberfläche spielt woa-streamd selbst Dateien oder Ordner aus der Konfiguration (WAV, MP3, FLAC über `woa_engine::decode`), endlos. Kommt die Oberfläche zurück, blendet er über 1 s zurück.

  Die Oberfläche schickt den Sendemix über einen eigenen `rtrb`-Ring des Mixers (der Audio-Callback schreibt nur hinein, Test unter `assert_no_alloc`) und einen Feeder-Thread an woa-streamd. Unter Unix läuft das über einen Unix-Socket in `<datenordner>/streamd/` (Modus 0700, also nur der eigene Benutzer), unter Windows über TCP auf 127.0.0.1. Dazu kommt ein zufälliges Token in `streamd.link` (Unix: Modus 0600). Die Verbindung ist abgesichert:
  - Die Anmeldung hat feste Größe und eine Frist von 200 ms.
  - Das Token wird in konstanter Zeit verglichen.
  - Höchstens 4 Verbindungen dürfen gleichzeitig unangemeldet sein; kommt eine weitere, fliegt die älteste wartende, damit stumme Verbindungen die Oberfläche nicht aussperren.
  - Es gibt nur eine aktive Oberfläche.
  - Rahmen sind auf 1 MiB begrenzt, und die Größe wird vor jeder Allokation geprüft.

  In der Oberfläche (Radio-Modus) gibt es den Knopf „Auf Sendung“ / „Sendung beenden“ und eine Zeile mit dem Zustand (verbunden, verbinde neu, Notfall), den Hörern, der Bitrate und der Dauer. woa-streamd wird abgelöst gestartet und überlebt einen Absturz der Oberfläche; nach dem Neustart hängt sie sich wieder an.

  Neu für Agent und Probe: das MCP-Tool `stream_status` (Zustand aus `streamd-status.json`) und `woa-headless stream-test` (30 s Testton an den Server, Exit 3 = Kennwort abgelehnt, 4 = nicht erreichbar). Anleitung und Fehlerbilder in `docs/stream.md`, Klickliste in `docs/abnahme.md`.

  Das Kennwort steht nie in der Konfiguration. Es wird aus einer Datei gelesen (Vorgabe `~/.secrets/daw.woa/shoutcast-source.pass`) und erscheint weder im Log noch in `Debug`.

- Neuere Clippy-Regeln der CI (Rust 1.99) im Sendeprozess erfüllt (`as_chunks`, keine unnötigen Borrows).

## 0.2.1 — 2026-10-29

- **MP3 und FLAC lesen.** Neues Modul `woa_engine::decode`: WAV weiter über `hound`, FLAC und MP3 über `symphonia` 0.6.1 (MPL-2.0, nur Features `mp3` und `flac`). Gilt überall, wo Audio gelesen wird: Clips im Projekt und Wellenform (`load_wav_mono`), Agent-Import (`import_audio` behält jetzt die Endung der Quelle), Playlist, Cart-Wall, LUFS-Messung und `radio-render` (`woa_radio::audio::load_stereo`). Dekodiert wird nur im Control- oder Hintergrund-Thread; andere Raten rechnet WOA wie bisher bei WAV linear auf die Projektrate um. Dateidialoge und Drag-and-drop im Radio-Modus nehmen `.mp3` und `.flac` an.
- **Speicherschutz beim Lesen.** Rate (8–384 kHz) und Kanalzahl (1–8) werden vor dem Dekodieren geprüft und für jeden dekodierten Puffer wiederholt; wechseln sie mitten in der Datei, wird sie abgelehnt. Feste Obergrenze aller Samples (`MAX_SAMPLES`, 6 h Stereo bei 48 kHz), unabhängig von Angaben aus der Datei, geprüft vor jedem Anhängen; WAV prüft die angegebene Länge vor dem Lesen. Tests mit Rate 0 im FLAC-Kopf, 9 Kanälen und einem WAV-Kopf, der fast 4 GB behauptet.
- **FLAC-Export repariert.** `flacenc` schrieb die Länge des kürzeren letzten Blocks als Mindest-Blockgröße in STREAMINFO. Strenge Leser (symphonia) fanden dann keinen einzigen Frame. Der Export setzt sie jetzt auf die feste Blockgröße, wie es die FLAC-Spezifikation verlangt.
- **Radio-Layout:** Crossfade und Ducking stehen je in einer eigenen Zeile, die Einheit steht am Wert (s, dB). Bei 900 px Fensterbreite wird die Beschriftung nicht mehr abgeschnitten.
- Tests: FLAC- und MP3-Datei per `flacenc`/LAME erzeugen, lesen und mit dem Original vergleichen (Länge, Pegel je Kanal, Mono-Mix in 48 kHz); unbekannte Endung und kaputte MP3 ergeben Fehler.

## 0.2.0 — 2026-10-29

- **Radio-Modus (Paket 7: Sendeablauf).** Umschalter Produzent/Radio oben rechts, gespeichert in `config.json`. Das Radio-Layout zeigt:
  - eine große Uhr mit der Restzeit des Titels und bis zum Playlist-Ende
  - Start/Weiter (Leertaste) und Stopp
  - die Playlist, gefüllt per Dialog oder Drag-and-drop, sortierbar und mit Entfernen
  - Regler für Crossfade und Ducking
  - die Cart-Wall 4×4 (Tasten 1–9, 0)
  - Mikro (Taste M) und Mitschnitt

  Die Sendung läuft in einem eigenen Steuer-Thread weiter, auch bei minimiertem Fenster. Änderungen an `radio.json` von außen (Agent) übernimmt die App. Neu: `woa-headless radio-play` als Probe am Gerät, `woa_engine::device::start_stereo_output` und `woa_engine::record::start_live_input`.
- **Agent-Werkzeuge für den Sendeablauf.** `build_playlist` (Dateiliste ersetzen oder anhängen, jeder Titel wird gemessen) und `run_order` (Reihenfolge, Gesamtlänge mit Überblendungen, LUFS und Auto-Gain je Titel). Beide arbeiten auf `radio.json`, Pfad als Parameter oder Vorgabe im Datenordner. Geschrieben wird nur in die Vorgabe oder relativ im Projektordner (Regeln wie beim Export: kein `..`, kein absoluter Pfad, kein Symlink als Ziel).
- **Sendeablauf-Kern (`crates/radio`).** Playlist mit Crossfade konstanter Leistung (Vorgabe 3 s), Auto-Gain auf −16 LUFS (gemessen und in `radio.json` gecacht), Cart-Wall mit 16 Tasten, Mikrofon mit geglättetem Ducking (−12 dB), Mitschnitt als WAV in `<datenordner>/mitschnitte/`. Der Mixer allokiert nicht (Tests unter `assert_no_alloc`), Dekodieren läuft im Control-Thread. Neu: `woa-headless radio-render <radio.json> <out.wav>`.
- **Export schreibt in das geöffnete Handle.** `woa_engine::export::export_stereo_to` schreibt in ein schon offenes `Write + Seek`; der Agent-Export öffnet die Temp-Datei mit `create_new` und schreibt genau in dieses Handle, danach `rename`. Geht der Export schief, entfernt er die Unterordner, die er selbst angelegt hat, wieder (nur leere). Die Endung wird geprüft, bevor ein Ordner entsteht. Neuer Test: nach einem Fehler bleibt kein leerer Ordner zurück.

## 0.1.17 — 2026-10-29

- **Export dicht gegen Symlinks.** `export` prüft jetzt den tiefsten vorhandenen Ordner im Pfad kanonisch gegen den Projektordner, legt fehlende Unterordner einzeln an und prüft jeden sofort nach. Ein Symlink weiter oben im Pfad (etwa `link/neu/x.wav` mit `link` → `/tmp`) wird abgelehnt, ebenso ein Ziel, das selbst ein Symlink ist. Geschrieben wird zuerst in eine neu angelegte Temp-Datei im geprüften Ordner, die danach per `rename` an den Zielnamen kommt; ein zwischendurch untergeschobener Symlink wird so ersetzt, nicht beschrieben. Neue Tests: Export in neuen Unterordner, Symlink-Vorfahr und Symlink-Ziel (nur Unix).

## 0.1.16 — 2026-10-29

- **Paket 6 Agent (M2).** Der MCP-Server kann jetzt allein einen hörbaren Loop bauen: `generate_notes` (Noten mit dem eingebauten Synth) und `generate_drums` (Kick, Snare, Hi-Hat als 16tel-Muster, `woa_dsp::generate`) erzeugen WAV-Dateien in `aufnahmen/` und legen Audio-Clips an. Dazu `import_audio`, `move_clip`, `trim_clip`, `set_fades`, `repeat_clip`, `remove_clip`, `remove_track`, `set_solo`. `analyze` misst LUFS, Spitze, Länge und Spitze je Spur, `export` schreibt WAV/FLAC/MP3 und gleicht auf Wunsch auf eine Ziel-Lautheit an. Jedes Tool ist ein Undo-Schritt „Agent“, Fehler lassen das Projekt unverändert. `export` schreibt nur relativ in den Projektordner (absolute Pfade und `..` werden abgelehnt). Abnahmetest: 8-Takt-Loop nur über Tools, exportiert und nachgemessen −14,0 LUFS, 16 s (ffmpeg bestätigt).

## 0.1.15 — 2026-10-29

- **M0 Logs, Client-Seite.** Beim ersten Start fragt WOA, ob anonyme Fehlercodes gesendet werden dürfen (Opt-in, nur Code, Stufe, Zahl). Mit Ja gehen sie ab dem nächsten Start an `https://feedback.wironair.de/api/logs/ingest`; umschaltbar unter „Einstellungen“. `config.json` bekommt `install_id` (zufällige UUID) und `telemetry_enabled`. `woa-headless log-test` nutzt dieselbe Adresse als Standard. Server-Seite folgt in bretteins.infra.
- `docs/abnahme.md`: Klickliste M0 Fundament; `docs/telemetrie.md`: Opt-in und Datenschutz.

## 0.1.14 — 2026-10-29

- **Feedback geht an den eigenen Server.** Ohne `config.json` schickt der Feedback-Knopf an `https://feedback.wironair.de/api/feedback`, und daraus wird ein Issue in daw.woa (Gegenprobe #40). Mit `"feedback_url": ""` in der `config.json` ist er abgeschaltet.

## 0.1.13 — 2026-10-29

- `docs/andre-handgriffe.md`: Andrés erledigte Schritte (DNS, Token, Ablauf) entfernt; Ausrollen macht Claude.

## 0.1.12 — 2026-10-29

- Windows-Build wieder grün (Clippy `unused_mut` in der Quellenliste).

## 0.1.11 — 2026-10-29

- **Paket 5 Quellen.** Jede Spur hat eine Quellenwahl: Standard-Mikrofon, alle Eingänge, „Systemklang“ (alles, was abgespielt wird) oder ein einzelnes Programm. Unter Linux wird direkt mit PipeWire/Pulse gesprochen (`engine/src/capture.rs`, Crate `pulseaudio`, reines Rust, MIT). Mehrere Spuren lassen sich scharf schalten (roter Punkt) und nehmen gleichzeitig ab demselben Punkt auf. Mit „hören“ ist die Quelle beim Aufnehmen hörbar.
- **Stopp am Ende** neben ▶ (Vorgabe an): Die Wiedergabe hält am Ende des letzten Clips an. Erneutes ▶ startet von vorn. Beim Aufnehmen läuft sie weiter.
- `woa-headless sources` listet die Quellen, `woa-headless capture <quelle> <datei>` nimmt eine davon auf.

## 0.1.10 — 2026-10-29

- **Design-Runde 1.** Dunkles Studio-Thema mit einer Akzentfarbe (Orange) und Rot für Aufnahme, sechs Spurfarben, abgerundete Knöpfe, größere Abstände (`gui/src/theme.rs`). Transportleiste mit großen Knöpfen und Zeitanzeige (Minuten:Sekunden, Takt, BPM), Aufnahme-Knopf in Rot, Zoom und Export-Lautheit rechts. Spurköpfe mit Farbstreifen, Aufnahmeziel-Punkt, Name, M, Pegel und Gain (Doppelklick: 0 dB) in fester Breite; Lineal mit Taktnummern und Kopf-Marke (Klick oder Ziehen setzt den Kopf). Zeitleiste mit Taktraster, Clips in Spurfarbe mit Kopfzeile und Wellenform. Statusleiste mit Version.
- `woa <projektordner>` öffnet ein Projekt beim Start. Fotomodus `WOA_SCREENSHOT=datei.png`: speichert ein Bild des Fensters und beendet (für Design-Prüfungen ohne Bildschirm-Werkzeug, auch unter Wayland).

## 0.1.9 — 2026-10-29

- **Paket 4 „Radio-tauglich“.** Export-Lautheit wählbar (−16 LUFS Web/Podcast, Vorgabe; −23 LUFS EBU R128; unverändert), Spitzen bleiben unter −1 dBFS; Messung nach ITU-R BS.1770 (`woa_engine::loudness`), mit ffmpeg `ebur128` gegengeprüft. `woa-headless render --lufs -16`.
- Latenzausgleich bei Aufnahmen: Eingangs- und Ausgangslatenz des Treibers werden gemessen und die Aufnahme um diese Zeit nach vorn gelegt (Audio-Versatz), dazu ein Wert von Hand in ms. `woa-headless record` meldet die Eingangslatenz.
- Mehrere Clips markieren (Strg/Umschalt+Klick), zusammen verschieben und löschen, je ein Undo-Schritt.

## 0.1.8 — 2026-10-29

- **Paket 3 „Arrangieren“.** Clips auf eine andere Spur ziehen, Clip-Ränder ziehen zum Kürzen (links verschiebt den Audio-Versatz mit, nie über Anfang oder Ende des Audios, Raster 1/4 Beat, ein Undo-Schritt), Zoom der Zeitleiste (Knöpfe − und +, Strg+Mausrad).
- Export als WAV, FLAC (24 bit, `flacenc`, Apache-2.0) oder MP3 (320 kbit/s, LAME über `mp3lame-encoder`, LGPL-3.0) nach Dateiendung, im Fenster (Strg+E) und in `woa-headless render`. Neu: `woa_engine::export`. `deny.toml`: Ausnahme für die veraltete Kennung „LGPL-3.0“ der zwei LAME-Crates.

## 0.1.7 — 2026-10-29

- **Paket 2 „Bearbeiten und Export“.** Clips per Ziehen verschieben (Raster 1/4 Beat), am roten Kopf schneiden (S oder Knopf, ein Undo-Schritt), Fade-in und Fade-out je Clip (Zeile über den Spuren, als Schatten im Clip sichtbar), Wellenform in den Clips, „Datei → Exportieren als WAV“ (Strg+E) bis zum Ende des letzten Clips.
- Modell: `Clip.offset_beats`, `fade_in_beats`, `fade_out_beats` (im JSON nur wenn nicht 0), Befehl `EditClip` mit Prüfung (Fades passen in den Clip) und Undo, `Clip::new`, `Clip::end_beats`.
- Engine: Fades in `ClipPlayer`, `clip::place_clip`, `clip_graph`, `render_clips`, `write_wav_stereo`, `peaks`; Fenster und `woa-headless` nutzen denselben Weg. `render` ohne `--seconds` rendert bis zum letzten Clip.

## 0.1.6 — 2026-10-29

- Pegelanzeige am Anfang jeder Spur (André): Spitzenpegel der Wiedergabe nach Gain, Mute und Solo, dB-Skala -60..0 dB, grün, ab -12 dB gelb, ab -3 dB rot, mit Abfall. Engine: `ClipPlayer::with_meter`, `clip::take_peak`, lock-frei per Atomic.

## 0.1.5 — 2026-10-29

- **Paket „Aufnehmen und Abspielen“** (erstes Abnahme-Paket, `docs/abnahme.md`). Fenster: Transportleiste (⏮, ▶/⏸, Zeit; Leertaste, Pos1), Zeitleiste je Spur mit Clips und rotem Abspielkopf, Klick setzt den Kopf, Clip anklicken und Entf löscht (mit Undo). Aufnahme startet am Abspielkopf auf der gewählten Spur (sonst neue Spur), die anderen Spuren laufen mit (Overdub), danach liegt der Take als Clip auf der Spur, Aufnahme samt neuer Spur ist ein Undo-Schritt. „Speichern unter“ kopiert die Takes in den Projektordner.
- Modell: `Clip.audio` (Pfad relativ zum Projekt, im JSON nur wenn gesetzt, alte Projekte laden unverändert).
- Engine: `clip::ClipPlayer` liest die Transportposition lock-frei und spielt Clips sample-genau, `clip::load_wav_mono` (Mono-Mix, lineares Resampling).
- `woa-headless render <projekt> <out.wav>` rendert offline, `play --project <projekt>` spielt ein Projekt am Gerät.
- Sicherheit: Clip-Pfade nur relativ im Projektordner (`is_safe_audio_path`, kein `..`, keine Symlinks), WAV-Laden nur 8–384 kHz und höchstens 6 h.
- Arbeitsweise: Abnahme in Paketen statt je Funktion (CLAUDE.md Regel 7, Konzept).

## 0.1.4 — 2026-10-29

- Aufnahme vorgezogen (Teil von M6): `woa-engine::record` nimmt den Eingang lock-frei über `rtrb` auf, ein Schreib-Thread schreibt 32-bit-Float-WAV und aktualisiert den Kopf alle 0,5 s. `woa-headless record <datei.wav> --seconds N` mit Pegelbalken, `devices --input`. Im Fenster: Knopf „● Aufnahme“ (Strg+R) mit Pegelanzeige, Takes landen als `aufnahme-NNN.wav` im Projektordner unter `aufnahmen/`, bei ungespeicherten Projekten unter `~/.local/share/woa/aufnahmen/`. CI installiert unter Linux `libasound2-dev`.

## 0.1.3 — 2026-10-29

- `woa-headless play`: Standard-Lautstärke 0.3 statt 0.1; mit 25 % Systemlautstärke war der Testton nicht zu hören (Abnahme M1 durch André).

## 0.1.2 — 2026-10-29

- M1-Abnahme am Gerät (#2): `woa-headless devices` listet die Ausgabegeräte, `woa-headless play` spielt einen Testton (Vorgabe 440 Hz, leise, Puffer 128, 10 s) über die Engine und meldet `underruns=N`, Exit 1 bei mindestens einem Underrun. Nur mit Feature `device` (Linux: `alsa-lib-devel`). `woa-engine`: `device::output_sample_rate`, Gerätewahl in einer Funktion.

## 0.1.1 — 2026-10-29

- Entscheidungen 29.10. (André) in `docs/entscheidungen.md`: CDLA-Permissive-2.0 nur für `webpki-roots`, Pan Constant-Power, SoundFont GeneralUser GS 2.0.3 (Lizenz in `docs/sounds/`), Feedback-Rolle mehrinstanzfähig per bretteins.infra#145.

- Runbook `docs/runbook-morgen.md` (Merge-Reihenfolge, Server, Hardware-Test) samt Handgriffen für Merge-Erlaubnis und Worktree-Regel.
- M1 (#2) Instrumente (`woa-dsp`): polyphoner Synth (Säge/Rechteck, ADSR, Voice-Stealing), Sampler (Loop, Tonhöhe per Rate, lineare Interpolation) und SoundFont-Player auf Basis von `rustysynth` (MIT); Note-Events sample-genau über `rtrb`-Queue (`InstrumentNode`). Tests unter `assert_no_alloc`, Golden-Renders für Synth, Sampler und SoundFont. Review: versehentlich geleerte CHANGELOG wiederhergestellt.
- M1 (#2) Transport, Geräteauswahl, Projekt-Render (`woa-engine`): Transport als Uhr (Play/Stop/Seek, Position in Samples als Atomic, Loop-Bereich samplegenau auch bei krummen Blöcken, Tempo aus `woa-model`, Beats↔Samples), Stereo-Ausgang im Graphen (`set_stereo_output`, `process_stereo`), `build_project_graph` baut aus einem Projekt im Control-Thread Quelle → Spur-Gain (dB, Mute/Solo) → Constant-Power-Pan → Summe L/R, Änderungen geglättet per `ProjectGraph::update`; `render_project` und Stereo-WAV. Feature `device`: Ausgabegeräte auflisten, Gerät per Id wählen, Puffergröße 128 als Vorgabe mit sauberem Fehler bei unpassendem Bereich, Stereo-Callback. Underrun-Zähler (Atomic, zählt `Xrun` im Fehlerpfad). Tests: Golden `tests/golden/project_mix_2tracks_05s.wav`, Pan, Solo, Transport, Loop, alles unter `assert_no_alloc`.
- MCP-Gerüst (Vorarbeit M2, #3): `woa-agent` mit rmcp 3.8 über stdio, Unterbefehl `woa-headless mcp <pfad>`. Tools `project_info`, `add_track`, `rename_track`, `set_gain`, `set_pan`, `set_mute`, `set_tempo`, `add_clip`, `undo`, `redo`, `save`, alle über `Session::apply`, je Aufruf ein Undo-Schritt „Agent“; Schemas aus serde/schemars. Tests mit rmcp-Client und rohem JSON-RPC über Pipes. Anleitung `docs/agent.md`.

- Härtung `woa-model` (Sicherheits-Review): IDs über 2^53 werden beim Laden und in Befehlen abgelehnt (`IdOutOfRange`), damit keine präparierte Datei die ID-Vergabe in eine Endlosschleife treibt. Atomares Schreiben über `tempfile` (exklusiv angelegt, zufälliger Name), folgt keinem untergeschobenen Symlink mehr.

- Review M0 (#1): `.gitignore` hält jetzt auch `.claude/worktrees/`, `.claude/settings.local.json` und `*.tmp` (atomares Schreiben) aus dem Repo.
- `woa-headless` (M0, Abnahmewerkzeug für #1): `new`, `info [--json]`, `add-track [--kind]`, `undo-demo`, `roundtrip` (Exit 1 bei Abweichung), `log-test` (`--url`/`WOA_TELEMETRY_URL`), `feedback-test` (`--url`/`WOA_FEEDBACK_URL`). Integrationstests mit assert_cmd, tempfile und lokalem Mock-Server. Pfad-Abhängigkeiten tragen eine Version, sonst wertet cargo-deny sie als Wildcard.

- egui-Prototyp (M0, Branch 5, `m0/gui`): `woa-gui` mit fensterunabhängigem `AppState` (Spur hinzufügen/umbenennen, Gain, Mute, Undo/Redo, Neu/Öffnen/Speichern, Autosave alle 10 s, ungespeichert in `recovery.woa`) und Feedback-Dialog (Text + Screenshot über `woa-telemetry`, leere `feedback_url` = „Server noch nicht eingerichtet“). `apps/woa` startet das Fenster (eframe 0.35, rfd). Bewertung in `docs/entscheidungen.md`. `deny.toml`: Lizenz-Ausnahme für die egui-Standardschriften (OFL-1.1, Ubuntu Font Licence), vier „unmaintained“-Hinweise transitiver Crates (ttf-parser, zbus-Teilcrates) bewusst ignoriert.
- Telemetrie (M0, `woa-telemetry`): `tracing` mit JSON-Logdatei je Tag (30 Tage), Panic-Hook, Opt-in-Upload-Senke (gebündelt, nur Code/Stufe/Zahl, Scrubber, eigener Thread, `ureq`) im Format der bretteins-`/ingest`-Route, Feedback-Client `submit_feedback` mit Screenshot. Tests gegen lokalen Mock-Server, Vertrag in `docs/telemetrie.md`. `deny.toml`: Ausnahme CDLA-Permissive-2.0 nur für `webpki-roots`.
- Projektmodell `woa-model` (M0, Branch 2): `Project`/`Track`/`Clip` mit `u64`-IDs, `Command`-Enum (Spuren, Gain, Pan, Mute, Solo, Tempo, Clips) mit exakten Inversen, `Session::apply` mit Undo/Redo und Gruppen, Ordnerformat `<name>.woa/project.json` mit `format_version`, atomares Schreiben, Autosave-Hook `Session::autosave` ohne Zeitgeber. Tests: Roundtrip, insta-Snapshot, proptest apply+undo.
- Feedback-Server vorbereitet (M0, Branch 4): `server/README.md` mit Anleitung für eine zweite Instanz der bretteins-Rolle `feedback` (feedback.wironair.de, Port 8081, `GITHUB_REPO=nixblick/daw.woa`), Prüfung der Mehrinstanzfähigkeit samt Patch-Vorschlag, Vorlagen unter `server/feedback/`, Runbook `docs/andre-handgriffe.md`. Nichts ausgerollt.
- Engine-Spike (Vorarbeit M1, #2): `woa-dsp` mit `Node`-Trait (allokationsfreies `process`), linearer Parameter-Glättung, Sinus-Oszillator und Gain. `woa-engine` mit Graph-Kompilierung im Control-Thread (Topologie, Puffer vorab, Zyklusprüfung), Kommandos über `rtrb`, Graph-Tausch im Audio-Thread und Freigabe über `basedrop`, Graph-Beschreibung per `arc-swap`, Offline-Render und WAV (hound). cpal-Ausgabe nur hinter Feature `device`. Tests: `assert_no_alloc` um jeden Process-Aufruf, Golden-Render `tests/golden/sine_440hz_gain05_1s.wav`, Glättung ohne Sprung.

## 0.1.0 — 2026-10-28

- CI-Gate (M0, #1): fmt, clippy -D warnings, test auf Linux und Windows, cargo-deny mit GPL-kompatibler Lizenzliste. `.gitignore` hält lokale Projekte und Logs fern.

## 0.0.1 — 2026-10-28

- Repo angelegt: Konzept `docs/konzept.md` (aus Drive), Cargo-Workspace mit 14 Crates und 3 Apps als leere Gerüste, `CLAUDE.md` mit den Echtzeit- und Stabilitätsregeln, LICENSE GPLv3, Issues M0–M11.
