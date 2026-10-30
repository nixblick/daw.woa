# Analyse & Zielkatalog: Konzept-Abgleich und Studio-Optik (mAirList + Ableton)

Stand: 30.10.2026 · Branch: `feat/ui-studio-redesign`

Dieses Dokument hält die Bestandsaufnahme von **WOA – Wir on Air** gegenüber dem Gesamtkonzept (`docs/konzept.md`) sowie die konkreten **Optik- und Layout-Ziele** fest, anhand derer wir den Umbau zu einer vollwertigen Open-Source-Alternative für **mAirList** (Radio) und **Ableton Live** (Produzent) abnehmen können.

---

## Teil 1: Standortbestimmung – Wie nah sind wir an mAirList & Ableton?

### 1.1 Gesamtfortschritt gegenüber `docs/konzept.md` (~35 %)
* **Substanziell ausprogrammiert (~20.000 LOC Rust + ~3.600 LOC Tests):**
  `crates/gui`, `crates/engine`, `crates/radio`, `crates/stream`, `crates/agent`, `crates/dsp`, `crates/model`, `crates/telemetry`, `crates/io` (MIDI-Live & SMF) sowie `apps/woa`, `apps/woa-headless`, `apps/woa-streamd`.
* **Aktuell noch 1-Zeilen-Platzhalter (Stubs für M7–M10):**
  `crates/library` (SQLite), `crates/plugins` (CLAP), `crates/guests` (WebRTC), `crates/remote` (Party-Web), `crates/updater`.
* **Hinweis zum fehlenden Konzept-Screenshot:**
  In `docs/konzept.md` (Zeile 86) befindet sich der Platzhalter `[embedded content: Architektur · 6 Schichten, GUI und Agent gleichberechtigt]`, der beim Übertragen aus dem ursprünglichen Chat/Post ohne Bilddatei übernommen wurde.

### 1.2 Reifegrad als mAirList-Alternative (Radio-Sendestudio) → ~55 %
- [x] **2-Deck-Playout & Crossfade:** Constant-Power-Crossfade auf eigenem Clock-Thread (`woa_radio`).
- [x] **Cart-Wall:** 12 Slots für Jingles/Beds, triggerbar per Maus oder Tasten `1`–`9`, `0`.
- [x] **Mikrofon-Kette & Auto-Ducking:** 80-Hz-HPF, 3-kHz-Präsenz, Gate, Kompressor und automatische Musikabsenkung (`duck_db`).
- [x] **Sendebus-Processing:** 3-stufige Broadcast-Kette (BS.1770 AGC auf Ziel-LUFS, Kompressor, Brickwall-Limiter bei −1 dBFS).
- [x] **Absturzsicherer Stream-Daemon (`woa-streamd`):** Eigener Prozess für Shoutcast v1/v2 & Icecast 2 (MP3) inkl. 2-Sekunden-Notfall-Playlist-Failover.
- [x] **Sendungsmitschnitt:** Durchgehende WAV-Aufnahme im Hintergrund.
- [ ] **Offen für 100 % mAirList-Parität:**
  - [ ] Track-spezifische Audio-Cue-Marker (`Cue In`, `Ramp / Intro-Countdown`, `Hook`, `Fade Out`, `Cue Out`).
  - [ ] Separater Vorhör-Ausgang (`PFL / Cue` auf zweitem Stereo-Ausgang / Kopfhörer).
  - [ ] SQLite-Musikbibliothek (`crates/library`) mit Tag-Suche, Kategorien und Stundenuhr/Rotation.
  - [ ] Voice-Tracking-Editor & WebRTC-Gästeschalte (`crates/guests`).

### 1.3 Reifegrad als Ableton-Live-Alternative (Produzent / DAW) → ~20 %
- [x] **Lineare Arrangement-Timeline:** Mehrspur-Audio & MIDI-Clips, Verschieben, Trimmen, Fades, Schneiden (`S`), Loop-Playback, LUFS-normalisierter Export (WAV/FLAC/MP3).
- [x] **Live-Recording:** Mehrspur-Overdub mit mitwachsender Wellenform, Latenzkompensation und PipeWire-App-Capture.
- [x] **MIDI-Fundament (M4 Paket 11 & 12):** Live-MIDI-Keyboard-Eingang (`midir`), MIDI-Aufnahme, `.mid`-Import/Export und Echtzeit-Klangerzeuger (`PolySynth`, Sampler, `rustysynth` SF2).
- [x] **Alleinstellungsmerkmal KI-Agent (`crates/agent`):** 22 MCP-Tools mit gleichen Rechten wie die GUI (inkl. `render` & `analyze`).
- [ ] **Offen für 100 % Ableton-Parität:**
  - [ ] Interaktiver Piano-Roll- und Drum-Editor zum Bearbeiten einzelner Noten/Velocities (M4 Paket 13).
  - [ ] MIDI-Learn für Fader, Regler und Cart-Wall (M4 Paket 14).
  - [ ] Kanal-Insert-Effekte (EQ, Reverb, Delay, Sidechain), Busse/Sends und Parameter-Automation (M5).
  - [ ] Time-Stretching / Warping & Pitch-Shifting für Audio-Clips (M6).
  - [ ] CLAP-/VST3-Plugin-Hosting (`crates/plugins`, M8).

---

## Teil 2: Optik- & Layout-Ziele (Studio- & Broadcast-Redesign)

Grundsatz: **Alle bisherigen Bedienelemente, Anzeigen und Tastenkürzel bleiben sichtbar und unverändert bedienbar**, erhalten aber die visuelle Struktur, Tiefe und Haptik echter Studio-Hardware (Ableton Live / Bitwig / mAirList / DHD-Sendepult).

### Checkliste 1: Farbwelt, Kontrast & Bauteile (`crates/gui/src/theme.rs`)
- [x] **3-stufige Tiefenhierarchie statt Einheitsgrau:**
  - `WELL` (`rgb(12, 14, 18)`): Tiefschwarze Vertiefung für LCD-Displays, Pegelschächte und leere Zeitleisten.
  - `BG` (`rgb(18, 20, 26)`) & `PANEL` (`rgb(27, 30, 38)`): Abgegrenzte Modul-Karten mit `1px`-Hardware-Bevel (`BORDER`).
  - `SURFACE` (`rgb(41, 46, 58)`): Griffige Hardware-Taster mit feiner Kontur.
- [x] **Klare Signalfarben:**
  - `ACCENT` (Warm-Amber) für Playhead, aktive Modi und Haupt-LCD.
  - `ON_AIR` (Broadcast-Rot) für Aufnahme, offenes Mikrofon, On-Air-Warnung (< 10 s) und Clipping.
  - `OK_GREEN` (Signal-Grün) für laufende Decks, offenes Gate und aktive Stream-Verbindung.
  - `CUE_CYAN` (Studio-Cyan) für Cue-Vorauswahl und MIDI-Spuren.
- [x] **Wiederverwendbare Studio-Komponenten:**
  - `card_frame()` für eingefasste Studio-Module.
  - `lcd_frame()` für tiefschwarze Digital-Anzeigen.
  - `status_pill()` für beleuchtete LED-Status-Badges (`DECK LÄUFT`, `MIKRO OFFEN`, `REC LÄUFT`, `GATE OFFEN`).
  - `led_meter()` für mehrgliedrige 20-Segment-LED-Pegelketten (−60 bis 0 dB, Grün → Gelb → Rot).

### Checkliste 2: Gemeinsame Kopfleiste (`crates/gui/src/app.rs`)
- [x] **Marken-Badge & Menüs:** Prägnantes `WOA`-Studio-Badge links vor `Datei`, `Bearbeiten`, `Einstellungen`, `Feedback …`.
- [x] **Segmentierter Modus-Umschalter:** Eingefasster Hardware-Umschalter (`🎚 Produzent` | `📻 Radio`) rechts oben, der bei laufender Sendung rot (`● 📻 Radio`) leuchtet.
- [x] **MIDI-Aktivitäts-Pille:** Beleuchtetes `MIDI`-Status-Badge mit LED-Punkt direkt neben dem Modus-Umschalter.

### Checkliste 3: Radio-Modus als „Broadcast Cockpit“ (`crates/gui/src/radio.rs`)
- [x] **4 klar eingefasste Studio-Karten statt loser Spalten:**
  1. **Oben Links – `SENDE-DECK & MASTER-UHR`:**
     - [x] Status-Zeile mit beleuchteten LED-Badges (`DECK LÄUFT` / `BEREIT / CUE`, `MIKRO OFFEN`, `REC LÄUFT`).
     - [x] Eingefasstes Album-Cover (`152×152 px`) neben tiefschwarzem **LCD-Master-Display**.
     - [x] LCD zeigt Status (`AKTUELL ON AIR` / `NÄCHSTER START`), LUFS-Wert, großen Titelnamen, **54px-Restzeituhr**, **Restzeit bis Playlist-Ende** sowie einen **visuellen Titel-Fortschrittsbalken** (färbt sich unter 10 s rot).
     - [x] Beleuchtete Broadcast-Tastenreihe (`▶ Start / Weiter`, `■ Stopp`, `🎙 Mikro (M)`, `● Mitschnitt`) + eigenes **Stream-LCD-Modul (`📡 Auf Sendung`)** mit Live-Status-Pille.
  2. **Unten Links – `SENDE-PLAYLIST`:**
     - [x] Modul-Header mit Titelanzahl, Lautheits-Mess-Spinner und hervorgehobenem `+ Titel …`-Button.
     - [x] Tabellen-Spaltenkopf (`#`, `TITEL & LAUTHEIT`, `AKTION · DAUER`).
     - [x] Zeilen im Zebra-Look mit **farbigem 4px-Indikatorstreifen links** (Rot für `ON AIR`, Cyan für `CUE`), Cover-Thumbnail, sichtbarem **LUFS-/Auto-Gain-Wert**, Monospace-Länge und rechtsbündigen Steuerknöpfen (`^`, `v`, `x`).
  3. **Oben Rechts – `CART-WALL (JINGLES & SOUNDS)`:**
     - [x] 4×3-Matrix im Hardware-Drumpad-Stil: Jedes Pad mit Tasten-Nummer (`1`–`9`, `0`), Jingle-Titel, **Status-LED oben rechts** und **Spurfarben-Balken am unteren Pad-Rand** (leuchtet bei Wiedergabe).
  4. **Unten Rechts – `BROADCAST-PROCESSING & ÜBERGÄNGE` (19"-Rack-Optik):**
     - [x] Eigenes Modul unter der Cart-Wall (statt loser Regler unter der Uhr) mit `ZIEL -16 LUFS`-Badge.
     - [x] Eingefasstes Übergangs-Panel für `Crossfade-Blende (s)` und `Musik-Ducking (dB)`.
     - [x] Eingefasstes Processing-Panel für `Sendebus` & `Mikro-Kette` inkl. `Gate offen / Gate zu`-Pille und digitaler Messzeile (`LUFS · Regelung dB · Begrenzer dB`).

### Checkliste 4: Produzent-Modus als „DAW Studio“ (`crates/gui/src/app.rs`)
- [x] **Aufgeräumte Transport-Konsole:**
  - [x] Eingefasste Hardware-Transportgruppe (`⏮`, `▶/⏸` in Signalgrün, `● Aufnahme` in Rot, `⇥ Stopp am Ende`).
  - [x] Tiefschwarzes **LCD-Zeit- & Takt-Display** (`00:00.0`, `Takt 1.1 · 120 BPM`).
  - [x] 20-Segment-LED-Aufnahmepegel + `ÜBERSTEUERT`-Pille bei laufender Aufnahme.
  - [x] Sauber gruppierte Latenz-Kapsel sowie rechtsbündige Zoom- und Mastering-Export-Steuerung (`-16 LUFS` / `-23 LUFS`).
- [x] **Clip-Inspektor-Leiste (`clipbar`):**
  - [x] Immer erkennbares `CLIP-INSPEKTOR`- bzw. `CLIP`-Badge mit Fade-In/Out-Reglern, `✂ Schneiden (S)` und `🗑 Löschen (Entf)`.
- [x] **Spurköpfe wie echte Mischpult-Kanäle:**
  - [x] Kräftiger `5px`-Farbstreifen links + farbiger Rahmen bei Auswahl/Scharfschaltung.
  - [x] Beleuchteter Record-Arm-Taster (`●`), Spurname, `MIDI`-Badge auf MIDI-Spuren und kantiger `M`-(Mute)-Taster.
  - [x] **Mehrgliedriges LED-Peak-Meter (`led_meter`)** neben dem dB-Fader sowie Aufnahmequellen-Dropdown + `hören`-Monitor.
- [x] **Pro-Timeline & Clips:**
  - [x] Dunkles Takt-Lineal mit Haupt- und Unterteilungen + `SPUREN & MIXER`-Kopf links.
  - [x] Zeitleiste mit abgedunkeltem `WELL`-Hintergrund und Zebra-Taktschattierung.
  - [x] **Audio-Clips** mit farbigem Titelbalken, zweistufiger Wellenform (Körper + Spitzenkontur) und **sichtbaren diagonalen Fade-Linien**.
  - [x] **MIDI-Clips** mit automatischer **Mini-Piano-Roll-Notenvorschau** direkt im Clip auf der Zeitleiste.
