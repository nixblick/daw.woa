# Klangqualität – Plan (Teil B, Stand 0.38.0)

Auftrag (André, 02.11.2026): „Ich will die qualitativ guten Sachen.“ Und: „Die besten Sound-Einstellungen
sollen der Standard sein beim Starten der App … die beste Stimme. Dass das alles einstellbar ist, gut.“

Teil A ist mit 0.38.0 gebaut: neue MIDI-Spuren spielen den Flügel aus GeneralUser GS, die Klangwahl ist im
Spurkopf, im Clip-Kopf und in sieben Kategorien zu finden. Dieses Dokument plant den Rest. Hier wird noch nichts gebaut.

## Stand heute

- **Klangbänke:** GeneralUser GS ist eingebaut. Beim ersten Gebrauch werden MuseScore General (MIT, 216 MB) und
  AVL Red Zeppelin (CC BY-SA 3.0, nur Drums, 26 MB) geladen. Der Lader (`woa_engine::banks`) nimmt nur eine einzelne
  `.sf2`-Datei mit fester SHA-256 über HTTPS an, keine Archive.
- **Vorgabe-Bank:** Sie bleibt GeneralUser GS. Für einen GM-Satz gilt sie als die stärkste frei lizenzierte Bank,
  und sie ist sofort da: kein Download, kein anderer Klang auf Rechnern ohne Netz. MuseScore General geht auf FluidR3
  zurück und ist kein klarer Gewinn. Ein deutlich besserer Flügel braucht Salamander (siehe B1).
- **Sprachausgabe:** WOA erzeugt heute **keine** Sprache. Die „Stimme“ im Jingle-Assistenten ist eine leere
  Audio-Spur zum Einsprechen. Es gibt also keine Stimme, die man auf „die beste“ stellen könnte. Deshalb steht B1 als
  erster Punkt hier.

## Punkte

### B1 Sprachausgabe mit Piper (zuerst)

| | |
|---|---|
| Was | Text-zu-Sprache für Jingles, Ansagen und Stationskennungen, offline. Als Bibliothek oder als eigener Prozess, das Ergebnis landet als Audio-Clip auf einer Spur. |
| Kandidat | Piper (Engine MIT, Laufzeit ONNX Runtime MIT). Stimme `de_DE-thorsten-high`: Thorsten-Datensatz CC0, Modell laut Modellkarte frei. Die Lizenz je Stimme steht in ihrer `MODEL_CARD` und wird vor dem Einbau einzeln geprüft. `thorsten_emotional` (medium) bringt Stimmungen, passt also zu Jingles. |
| Größe | Modell high etwa 110 MB, medium etwa 60 MB. Laufzeit plus Bibliothek etwa 20–30 MB. Wie die Bänke wird alles beim ersten Gebrauch geladen. |
| Rechenbedarf | CPU genügt: high erzeugt auf einem Notebook schneller als Echtzeit. Es läuft nie im Audio-Thread, sondern offline in eine WAV-Datei. |
| Aufwand | Mittel, etwa 3–5 Tage. Dazu gehören Prozessaufruf oder `ort`-Anbindung, Download mit Prüfsumme, Assistentenschritt „Text sprechen“ und MCP-Werkzeug. |
| Klanggewinn | Hoch für Jingles ohne Sprecher. Klingt aber hörbar synthetisch und ersetzt keinen Moderator. |
| Empfehlung | **Bauen, als erstes Paket nach 0.38.0.** Vorgabe: Thorsten high, einstellbar. Die Einstellungsseite kommt mit dem eigenen Einstellungs-Paket. |
| Weitere Kandidaten | Coqui XTTS-v2 (Coqui Public Model License, nicht kommerziell, **ausgeschlossen**). Kokoro-82M (Apache-2.0): bisher kaum Deutsch, später prüfen. F5-TTS (Code MIT, Gewichte CC BY-NC, **ausgeschlossen**). |

### B2 Besserer Flügel (Salamander Grand Piano)

| | |
|---|---|
| Was | Salamander Grand Piano V3 (Yamaha C5, Alexander Holm) als SF2. Quelle ist FreePats (`freepats.zenvoid.org`). |
| Lizenz | CC BY 3.0: Weitergabe und kommerzielle Nutzung sind erlaubt, Namensnennung ist Pflicht. Als Daten neben GPLv3 unproblematisch. |
| Größe | Archiv `SalamanderGrandPiano-SF2-V3+20200602.tar.xz` 310 MB, entpackt größer. |
| Hindernis | Die Datei gibt es nur als `.tar.xz`. Der Bank-Lader kann kein xz, und `xz`/`liblzma` wäre eine neue Abhängigkeit. |
| Wege | (a) Den Lader um tar.xz erweitern (eine neue Crate, kleiner Code). (b) Die entpackte `.sf2` als Release-Asset im eigenen Repo hosten, mit fester SHA-256 und Lizenzdatei. Das bedeutet 300+ MB je Asset. |
| Aufwand | Klein bis mittel, 1–2 Tage. |
| Klanggewinn | Hoch beim Klavier, sonst keiner. |
| Empfehlung | **Bauen, Weg (b), wenn André das Hosten freigibt, sonst (a).** Danach wird Salamander der Flügel neuer Spuren, sobald die Bank geladen ist. Bis dahin spielt GeneralUser GS. |
| Ebenso | Upright Piano KW (FreePats, CC BY 3.0, `.7z`, 29 MB) als zweites Klavier. Braucht dafür 7z, deshalb nur zusammen mit B2(a) oder als Release-Asset. |

### B3 Weitere SF2-Bänke

- **FluidR3_GM** (MIT, `fluid-soundfont.zip` 132 MB bei OSUOSL): Kaum Gewinn gegenüber MuseScore General, das daraus
  entstanden ist. Nicht aufnehmen.
- **Sonatina Symphonic Orchestra** (SF2 als zip): Lizenz CC Sampling Plus 1.0. Wie die kommerzielle Nutzung geregelt
  ist, ist unklar, daher **ausgeschlossen**.
- **Arachno SoundFont:** Lizenz „frei, aber nicht verkaufen“, nicht GPLv3-sauber, **ausgeschlossen**.
- **VSCO-2 CE, Virtual Playing Orchestra:** gute Orchester-Bibliotheken (CC0 bzw. frei), aber nur als SFZ, also erst mit B4.

### B4 SFZ-Unterstützung

| Weg | Aufwand | Lizenz | Bewertung |
|---|---|---|---|
| sfizz als CLAP-Plugin (vom Nutzer installiert) | 0 Tage, läuft schon über den CLAP-Host | BSD-2 | sofort möglich, aber der Nutzer muss es selbst installieren |
| sfizz als Bibliothek (C++, per FFI) | 5–8 Tage, Windows-Bau mit CMake | BSD-2 | voller Klang, aber schwerer Bau und C++ im Prozess |
| eigener SFZ-Sampler in Rust auf Basis von `woa_dsp::sampler` | 8–15 Tage für die gängigen Opcodes (Regionen, Velocity-Layer, Hüllkurve, Round-Robin) | eigener Code | passt zu den Echtzeitregeln, viel Arbeit |

**Empfehlung:** Zuerst sfizz-CLAP im Instrument-Browser empfehlen und vorschlagen (siehe B5). Den eigenen Sampler erst
bauen, wenn VSCO-2 CE oder Salamander-SFZ ausdrücklich gewünscht sind.

### B5 Freie CLAP-Instrumente für Windows

| Instrument | Lizenz | Art | Klanggewinn |
|---|---|---|---|
| Surge XT | GPLv3 | Hybrid-Synth, sehr viele Presets | hoch für Synth, Pads, Bässe |
| Vital (Vitalium-Fassung) | GPLv3 | Wavetable | hoch für moderne Synth-Klänge |
| Dexed | GPLv3 | FM (DX7), E-Pianos und Glocken | mittel bis hoch |
| sfizz | BSD-2 | SFZ-Player | hoch mit guten Bibliotheken |
| Odin 2 | GPLv3 | Analog-Synth | mittel |

**Wie WOA sie anbietet:** Im Instrument-Browser kommt ein Abschnitt „Empfohlen“ dazu. Er zeigt, was schon installiert
ist (CLAP-Suche), und verlinkt bei den anderen auf die offizielle Download-Seite. Mitgeliefert wird nichts: das hält das
Paket klein, und Updates kommen vom Hersteller. Aufwand etwa 1 Tag. **Empfehlung: bauen** (klein, großer Gewinn).

### B6 CC0-Effektpaket für Jingles

| | |
|---|---|
| Was | 20–40 Effekte: Feuerwerk, Whoosh, Riser, Impact, Swoosh, Glitzer, Schallplatten-Stopp. Sie erscheinen im Browser unter „Effekte“, im Jingle-Assistenten als Schritt „Akzent“. |
| Quellen | Nur Klänge unter CC0. Kandidaten: Sonniss-GDC-Pakete (eigene lizenzfreie Lizenz, Weitergabe als Paket **nicht** erlaubt, daher ausgeschlossen), Freesound **nur CC0-Filter**, Kenney-Audio (CC0). Jede Datei kommt mit Herkunft in `docs/sounds/`. |
| Größe | Etwa 10–20 MB als FLAC, gesammelt als ein Paket, das beim ersten Gebrauch geladen wird. |
| Aufwand | Etwa 2 Tage: Auswahl, Herkunftsliste, Katalog wie bei `banks.tsv`, Browser-Abschnitt. |
| Klanggewinn | Hoch für Jingles. |
| Empfehlung | **Bauen**, nach B1. |

## Reihenfolge

1. B1 Sprachausgabe (Piper, Thorsten high als Vorgabe)
2. B2 Salamander als Flügel neuer Spuren
3. B5 empfohlene CLAP-Instrumente
4. B6 Effektpaket
5. B4 eigener SFZ-Sampler, nur bei Bedarf

Alles bleibt einstellbar. Die Einstellungsseite für Vorgabe-Bank und Stimme kommt mit dem eigenen
Einstellungs-Paket.
