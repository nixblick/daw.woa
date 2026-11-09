# Changelog

## 0.42.23 — 2026-11-09

- **Telemetrie-Schalter wirkt sofort:** Bisher startete das Hochladen der Fehlercodes nur beim Programmstart; wer den Schalter einschaltete, sah „an“, gesendet wurde aber erst nach einem Neustart. Die Senke läuft jetzt, sobald eine Server-Adresse eingetragen ist, und folgt dem Schalter ohne Neustart. Aus heißt weiter: nichts puffern, nichts senden.
- **Vorflug findet cargo-deny auch ohne `~/.cargo/bin` im PATH.**
- **Klarer benannt:** „Fehlercodes laufend im Hintergrund senden“; der Hinweis erklärt, dass das Protokoll im Feedback davon unabhängig ist (nur mit Haken im Feedback-Fenster).

## 0.42.22 — 2026-11-09

- **rmcp 3.9.0 (RUSTSEC-2026-0462):** Die wöchentliche deny-Prüfung war rot, weil die HTTP-Clients in rmcp 3.8.0 Antworten ohne Größengrenze lesen. WOA nutzt diese Clients nicht, das Update macht die Prüfung wieder grün.
- **Vorflug prüft Abhängigkeiten:** `scripts/vorflug.sh` ruft `cargo deny check` auf, wenn cargo-deny installiert ist.

## 0.42.21 — 2026-11-09

- **Denoise-Test ohne Wandzeit-Grenze (#575):** Der Test gab dem Arbeits-Thread je Block 10 ms und scheiterte auf einem langsamen Windows-Runner. Er wartet jetzt je Block auf den Arbeits-Thread (`wait_for_worker`).

## 0.42.20 — 2026-11-09

- **Master-LED folgt dem Regler (#569):** Die LED-Ketten der DJ-Decks hoben ihre Spitze nur an und setzten sie nie zurück, sie blieben auf dem lautesten Wert stehen; die Master-Kette zeigte dazu nur den lauteren Deck-Pegel vor dem Master-Regler. Jeder Schnappschuss holt die Spitze jetzt ab, die Master-Kette zeigt den Master nach dem Regler. Test `levels_follow_the_sound_and_the_master_control`.
- **Party-Tasten stauen nicht mehr (#568, #570, #572):** Jeder Übergang lud das freie Deck neu und dekodierte den ganzen Titel im Control-Thread; bei Video-Dateien vom Netzlaufwerk standen „→ Playlist“, hart/weich, Sync und Blende so lange still, wiederholte Klicks stauten sich. Liegt der Titel schon auf dem anderen Deck, wird dessen Puffer geteilt statt neu dekodiert. Test `loading_a_title_held_by_the_other_deck_does_not_decode_again`.
- **„Analyse fehlgeschlagen“ nennt den Grund (#571):** Ein Klick auf den Hinweis erklärt, warum kein Tempo gefunden wurde, was trotzdem geht, und bietet „Neu analysieren“. Das gesperrte SYNC sagt, dass es ein Tempo braucht (#572).

## 0.42.19 — 2026-11-07

- **Tempo auch bei dicht gemischter Musik:** Gemasterte Songs mit vollem Mix bekamen im DJ-Deck „Analyse fehlgeschlagen“, weil die Tempo-Erkennung nur harte Einsätze zählte (Pegel verdoppelt sich in 5 ms). Ein zweiter Weg zählt jetzt Einsätze relativ zur eigenen Einsatzkurve, sofern der Pegel nicht periodisch tief einbricht; Schwebungen und Tremolo täuschen weiter kein Tempo vor. Tests `dense_mix_has_a_tempo`, `beating_and_tremolo_have_no_tempo`; Prüfpunkt `tempo-dichter-mix` (Party DJ).

## 0.42.18 — 2026-11-07

- **Sende-Deck: Tasten in einer Reihe, ohne LED (#563):** Pause, Stopp, Fade, Blende, Mikro und die Sprung- und Spieltasten stehen jetzt in einer Reihe; ist das Fenster zu schmal, bricht sie um. Die LED neben MIKRO war der Telefon-Pegel und wirkte wie ein toter Mikro-Pegel; sie ist weg, der Telefon-Pegel steht weiter in der Gäste-Karte.

## 0.42.17 — 2026-11-07

- **„bis Playlist-Ende“ rechtsbündig (#563):** Die Anzeige im Sende-Deck springt nicht mehr zur Seite, wenn die RAMP-Anzeige erscheint oder verschwindet.
- **„Neuerungen“ zeigen alle übersprungenen Versionen (#561):** Wer mehrere Versionen auf einmal aktualisiert, sah im Update-Hinweis nur den Eintrag der neuesten. Das Manifest trägt jetzt die letzten 15 Changelog-Abschnitte, WOA zeigt davon alle, die neuer sind als die installierte, mit Überschrift je Version. Test `notes_cover_all_skipped_versions`.

## 0.42.16 — 2026-11-07

- **Keine „cover probe failed“-Warnung mehr bei WAV-Jingles (#559):** Das Cover-Lesen probte auch WAV und AIFF. Bei WAVE_FORMAT_EXTENSIBLE (Float32, so schreibt WOA und so liefert ElevenLabs) findet der Probe keinen Reader, und jeder Jingle brachte eine WARN- und zwei ERROR-Zeilen ins Protokoll, obwohl nichts schiefging. WAV und AIFF werden jetzt gar nicht erst geprobt; für MP3, FLAC, M4A und OGG bleibt die Warnung. Test `wav_and_aiff_are_not_probed`.

## 0.42.15 — 2026-11-07

- **Mikro-Anzeige auch bei geschlossenem Mikro (#556):** Die Pegelanzeige maß hinter MIKRO OFFEN. War das Mikro zu (nach jedem Neustart), stand sie still, und es sah aus, als käme kein Signal an. Jetzt misst sie vor dem Schalter: Man sieht sofort, ob das richtige Mikro gewählt ist. Auf Sendung und ins Ducking geht die Stimme weiter nur mit MIKRO OFFEN. Test `mic_meter_shows_signal_while_closed`.

## 0.42.14 — 2026-11-07

- **Update-Suche stündlich:** WOA sucht nicht mehr nur einmal am Tag beim Start nach einer neuen Version, sondern stündlich, auch während es läuft. Ein gescheiterter Versuch (offline) wartet ebenfalls eine Stunde. Abschalten weiter unter Allgemein › Updates.

## 0.42.13 — 2026-11-07

- **Strg+1/2/3 wechseln die Ansicht (#544):** Die Hinweise an PRODUZENT, RADIO und PARTY versprachen die Kürzel, geschrieben waren sie nie. Jetzt rufen sie `set_mode` auf (Protokoll: `mode` mit `from`/`to`). Nicht, solange ein Textfeld den Fokus hat; die Hot Cues auf 1–4 ohne Strg bleiben unberührt. Tests `ctrl_digits_switch_the_view`, `digits_without_ctrl_stay_hot_cues`.
- **Deck „Analyse fehlgeschlagen“ (#550):** Die Fremdmeldung „reached probe limit of 1048576 bytes“ im Protokoll kam vom Cover-Lesen: Titel mit viel Vorspann (großes Titelbild, lange Kennzeichnung) sprengten die 1-MiB-Suchgrenze, das Cover fehlte still. Das Cover-Lesen sucht jetzt wie das Dekodieren tiefer (bis 32 MiB) und schreibt bei Misserfolg eine eigene WARN-Zeile („cover probe failed“). Findet die Analyse kein Tempo, steht jetzt „deck analysis: no tempo found“ mit Dateiname im Protokoll, und der Hinweis am Chip sagt, dass kein klarer Taktschlag erkennbar war (Tonart und Wellenform bleiben gültig). Test `cover_survives_probe_limit_padding`.

## 0.42.12 — 2026-11-07

- **Notfall-Playlist bewusst an/aus, mit Zeitlimit:** Die Notfall-Playlist lief über Nacht, obwohl keine Sendung sein sollte: WOA war geschlossen worden, ohne die Sendung zu beenden, und woa-streamd sendete wie vorgesehen weiter. Neu unter Radio › Sendung › Erweitert: Schalter „Notfall-Playlist“ (Vorgabe an) und „höchstens … min“ (Vorgabe 30). Ist die Oberfläche länger weg, meldet sich woa-streamd selbst ab. Aus: Die Sendung endet sofort, wenn WOA geht. Stille bei laufender Oberfläche beendet nichts. Test `notfall_ends_after_limit_or_at_once_when_off`.

## 0.42.11 — 2026-11-07

- **Start ohne Stocken (Windows):** Das Ablösen der alten Installation für alle Benutzer und das Umstellen von Claudes MCP-Registrierung (`.claude.json` kann mehrere MB groß sein) laufen jetzt in einem Hintergrund-Thread statt im ersten Bild. Der Hinweis „Claude-Anbindung umgestellt“ erscheint, sobald der Thread fertig ist.

## 0.42.10 — 2026-11-06

Durchsicht der Änderungen seit 0.42.2 (Leistung, Stabilität):

- **Claude-Registrierung sicher umschreiben:** `.claude.json` und `claude_desktop_config.json` werden erst daneben geschrieben und dann umbenannt. Claude schreibt dieselbe Datei laufend; ein Abbruch mitten im Schreiben hätte seine ganze Konfiguration zerstört.
- **Mausrad am DJ-Regler:** Dreht das Rad einen Regler, rollt die Fläche darunter nicht mehr mit.

## 0.42.9 — 2026-11-06

- **Taskleisten-Logo bleibt nach Updates (#543):** Die Startmenü-Verknüpfung bezog ihr Symbol aus dem MSI-Zwischenspeicher (`%APPDATA%\Microsoft\Installer\{ProductCode}\woa.ico`), der mit jedem Release wechselt. Eine in die Taskleiste angeheftete Kopie zeigte danach ein leeres Blatt. Jetzt nimmt die Verknüpfung das Symbol aus der `woa.exe` im festen Installordner. Wer schon mit einer älteren Version angeheftet hat: einmal lösen und neu anheften. Test `shortcuts_take_the_icon_from_the_exe`.

## 0.42.8 — 2026-11-06

- **Fade-Taste an den Decks zählt herunter (#539):** Die quadratische Taste an Deck A–C zeigt beim Ausblenden die Restzeit („2,4“) statt des Symbols; der Tooltip nennt „noch 2,4 s“.
- **Test: eigene Stimme nicht im Kopfhörer (#537):** `own_voice_is_not_in_the_headphones_unless_asked` belegt, dass WOA das Mikro ab Werk nicht auf den Abhörausgang legt (nur mit Mithören an). Der Hall bei #537 kommt damit nicht aus WOA.
- **Leeres Deck zeigt LEER statt VORGELADEN (#540):** In der Party (Einfach) bekommt ein Deck ohne Titel das neutrale Badge LEER.

## 0.42.7 — 2026-11-06

- **Party › DJ: „LEER“ gefüllt** (#538). Das Badge eines leeren Decks war nur dunkel mit Rahmen, weil ein neutraler Chip auch mit `filled()` neutral blieb. Es ist jetzt voll in der Deckfarbe mit dunkler Schrift; „BEREIT · CUE“ ebenso (Farbe Cue). Test `empty_deck_badge_is_filled_in_deck_color`.

## 0.42.6 — 2026-11-06

- Fasst 0.42.3 und 0.42.4 (Claude-/MCP-Anbindung) mit 0.42.5 (Regler-Stil) in einem Release zusammen.

## 0.42.5 — 2026-11-06

- **Regler-Stil im DJ-Mixer wählbar** (Design-Fassung 8.1): Mini-Fader (Standard), Wertbalken, Zahlenfelder oder Drehknöpfe, für Trim, EQ, Filter, Master und Kopfhörer. Gewählt wird unter Einstellungen › Party › DJ › „Regler-Stil“ oder im Menü Ansicht › Regler-Stil (in der Party). Der Wechsel ändert keine Werte und wird pro Nutzer gespeichert. Für alle Stile gilt: Doppelklick setzt den Standardwert, Umschalt regelt fein, das Mausrad ändert, der Tooltip zeigt den Wert. Zahlenfelder nehmen per Klick auch getippte Werte („-3“, „kill“, „TP 40“). Neu sind KILL-Tasten je EQ-Band: Der erste Klick macht das Band stumm, der zweite stellt den Wert davor wieder her. Die Design-Vorlage liegt in `docs/design/woa-design-system/components/DjMixerStyles`.
- Versionsnummer 0.42.5, weil 0.42.3 und 0.42.4 auf einem eigenen Zweig liegen.

## 0.42.4 — 2026-11-05

- **MCP bleibt nach einem Neustart von WOA verbunden (#527):** `woa-headless mcp` hielt Port und Live-Schlüssel vom Start fest; nach einem Neustart der App (neuer Port, neuer Schlüssel) scheiterte jeder Aufruf mit „Verbindung verweigert“, bis Claude neu gestartet wurde. Jetzt liest die Weiterleitung bei einem Verbindungsfehler oder 401 `mcp-live.json` neu, meldet sich an der neuen App an und wiederholt den Aufruf einmal. Läuft keine WOA mehr, kommt „WOA läuft nicht: bitte die App starten …“ statt der Winsock-Meldung. Test `live_restart.rs`.

## 0.42.3 — 2026-11-05

- **Claude-Anbindung nach dem Umstieg auf das MSI pro Benutzer (#497):** Zeigte der Eintrag `mcpServers.woa` in `~/.claude.json` oder `claude_desktop_config.json` noch auf `C:\Program Files\WOA\woa-headless.exe`, verband Claude nicht mehr („Connection closed“). WOA schreibt beim Start (läuft es pro Benutzer) nur diesen Eintrag auf die `woa-headless.exe` neben der App um, legt `<datei>.bak-woa` an, loggt `mcp registration updated` und zeigt einen Hinweis, dass Claude neu starten muss. Andere Einträge und die Formatierung bleiben. Tests in `legacy.rs`, `test-upgrade.ps1` prüft toten alten und lebenden neuen Pfad.
- **Hinweis zu `ki.mcp_server.enabled = false` (gewollt):** Das ist der optionale HTTP-Server und startet seit #243 nur auf Wunsch; `app_started` nimmt den Haken beim Start zurück. Der stdio-MCP für Claude (`woa-headless mcp`) hängt nicht daran.

## 0.42.2 — 2026-11-05

- **Mikro einzeln, wie OBS:** Neuer Knopf in Audio-Geräte beim Pult-Modus. Er schaltet den Pult-Modus aus und macht das Gerät des Pult-Eingangs (etwa Maonocaster) zum Mikrofon. WOA mischt dann selbst und duckt die Musik an der Stimme, wie OBS mit dem Mikro als eigener Quelle. Am Gerät muss Loopback aus sein. Sind Mikrofon und Pult-Eingang im Pult-Modus dasselbe Gerät, warnt das Fenster: Dann steckt die Musik im Mikro, und das Ducking hält sie dauernd unten.

## 0.42.1 — 2026-11-05 (Rückmeldungen zu 0.42.0)

- **Knistern beim Fensterwechsel (Windows):** Die Audio-Threads melden sich jetzt als Echtzeit an (cpal-Feature `realtime`, unter Windows MMCSS „Pro Audio“). Beim Start nimmt sich WOA außerdem von der Energiespar-Drosselung aus (`SetProcessInformation`, Ausführungstempo und Zeitgeber-Auflösung). Lehnt das System die Echtzeit ab (unter Linux ohne Rechte üblich), läuft der Strom normal weiter, und das zählt nicht als Fehler. Ursache (Vermutung, noch nicht am Gerät gemessen): Windows 11 drosselt WOA, sobald ein anderes Fenster vorn ist.
- **Aussetzer auf allen Wegen in der Telemetrie:** Zu `radio.underruns_total` kommen `radio.link_faults_total` (zweite Ausgänge und Telefon: leer gelaufen, übersprungen, verworfen), `radio.pult_faults_total` und `radio.mic_faults_total`. Mikro- und Pult-Fehler zählt der Audio-Thread nur, ins Log kommt höchstens eine Sammelzeile je Minute (vorher über 80 Einzelzeilen „mic input“ in fünf Minuten).
- **Playlist immer sichtbar:** Auf kleinen Schirmen und bei Windows-Skalierung rollen Sende-Deck und Gäste in ihrem Bereich, die Playlist behält mindestens 260 px und füllt den Rest der Spalte (in 0.42.0 fiel sie unten heraus).
- **Processing-Rack schmal-fest:** Unter 330 px steht der Ziel-Chip unter der Überschrift und die Bezeichnung über den Feldern; die Ducking-Zeile stapelt unter 430 px. Bezeichnungen brechen um, nichts liegt mehr übereinander oder wird abgeschnitten. Test `playlist_and_rack_fit_common_sizes` für 1366 × 768, 1536 × 864 und 1920 × 1030; Ist-Bilder in `docs/design/ist/`.
- **Feedback wie bei fotobrett:** Der Klick auf FEEDBACK nimmt das Bildschirmfoto sofort auf, der Dialog zeigt die Vorschau und zwei gleichwertige Tasten „Mit Bild senden“ und „Ohne Bild senden“, statt eines Häkchens. Ein Satz erklärt, was das Bild zeigt. Esc und X schließen wie bisher; Strg+Enter sendet mit Bild.

## 0.42.0 — 2026-11-05 (Design-Fassung 8)

- **Befund behoben:** Die Zahlenfelder für Ducking-Rückkehr, Einblenden und Stopp-Ausblenden meldeten schon beim Zeichnen eine Änderung, solange der Wert außerhalb des Bereichs lag (etwa 0 s vor dem ersten Stand). Dann gingen die Mindestwerte (0,1 s, 0,1 s, 0,5 s) an den Mixer und nach `radio.json`. Gefunden beim Fotografieren für 0.42.0; das Verhalten steckt schon in älteren Versionen. Test `drawing_keeps_rack_values`.
- Radio: Die Telefon-Taste in der Tastenreihe entfällt (Auftrag Punkt 3). Geschaltet wird jetzt in der Gäste-Karte, der Pegel bleibt in der Reihe.
- Radio: Wunschtitel-Zeile 44 px hoch, damit sie bei 1920 × 1030 nicht abgeschnitten wird.

- **Nur ein WOA gleichzeitig:** Ein zweiter Start holt das laufende Fenster nach vorn und endet, bevor Audio, MIDI, Stream oder auch nur das Log geöffnet sind (am 05.11.2026 störte ein Doppelstart die Sendung). Sperre `woa.lock` im Datenordner (Betriebssystem-Sperre, fällt bei Absturz weg), Weckruf über einen lokalen Port (`woa.port`, nur 127.0.0.1). Integrationstest mit dem echten Binary. Der Weckruf läuft in einem eigenen Thread; eine hängende Verbindung kann die Oberfläche nicht blockieren.
- **X schließt Fragen und Abnahme mit einfachem Klick:** Alle Fensterköpfe (Einstellungen, Fragen, Abnahme, Hörer, Piano-Roll) teilen `fragen::kopf`; X und Maximieren reagieren jetzt auf jedes Loslassen über dem Knopf, auch wenn egui das Drücken der Kopffläche oder dem Fenster-Ziehen zugeordnet hatte. Im Test war der Fehler nicht nachzustellen (Zeiger steht still auf X, Tooltip sichtbar, einfacher Klick schließt dort schon); der Test `x_closes_on_plain_click` hält das Verhalten fest, normal und maximiert.
- **Pegel-Karte in einem Kanalbau:** Alle Kanäle (Playlist, Carts, Decks, Mikro, Telefon, Summe, Notebook, Stream) teilen Stumm-Platz oben (24 px), Fader und LED-Kette gleich hoch und das Schild auf einer Linie; Gruppen Quellen · Summe · Ausgänge mit 1-px-Linien und einer gemeinsamen dB-Skala links. Stumm als rote REC-Taste. Die LED-Kette kennt nur noch Grün, Gelb, Rot (kein Blau mehr für leise). Die Karte füllt ihre Höhe. Test `all_channels_share_one_build`.
- **Meldungszeile am Fuß des Sende-Decks** (`widgets::status_line`): 28 px, Warnsymbol, fetter Kurztext, Detail; mehrere Meldungen zeigen den Zähler-Chip „n Hinweise“, der die Liste aufklappt. Stille im Stream, Notfall-Playlist, Stream-Fehler, Lücken und Neuverbindungen, Telefon-Fehler, Ausgangs-Warnungen und ein Speicherfehler (wegklickbar) stehen dort statt als rote Fließtexte.
- **Radio ohne Rollen bei 1920 × 1030:** Links Sende-Deck, Gäste-Karte (Gäste, Telefon, Browser-Gäste), Playlist; rechts Decks, Cart-Wall, Pegel und Processing in gleicher Höhe, Wunschtitel als eine Zeile. Leere Cart-Pads ohne „–“-Kasten, Pads 46 px hoch. Die Cart-Wall bleibt 4 × 4 (16 Carts je Satz im Projektformat); auf kleinen Schirmen rollt die rechte Spalte weiter (#168). Wartezeit der Ziehen-Tests ereignisbasiert länger.
- **Einstellungen mit Werten in der Zeile:** Crossfade, Ducking samt Zeiten, Ducking bei Carts, Stopp blendet aus, Fade-Taste, Sendebus-Processing und Mikro-Kette sind dieselben Regler wie im Processing-Rack (`RadioDesk::rack_control`), statt „Im Radio zeigen“. Schalter sind überall `woa-tog` (Updates, Telemetrie, Piano-Roll-Fenster, Party-Felder), das Thema ist ein Segmentschalter. Der aktive Bereich links hat `surface` mit Kante `accent-2` statt Rot; „Audio und Geräte“ trägt einen gelben Chip „n fehlt“, wenn Ausgänge nicht offen sind. Startgröße 1400 × 930 (passt sich kleineren Schirmen an).
- **Fade-Tooltip** heißt jetzt „Ausblenden (3 s)“ mit der eingestellten Dauer.
- **Fragen und Abnahme:** Stand rechts im Fensterkopf als neutraler Chip („12 von 40 geprüft“, „3 von 9 beantwortet“); Ticket-Pillen neutral statt orange; der Abnahme-Balken zeigt Grün für „Geht“ und Gelb für „Geht nicht“/Anmerkung; „Erledigte zeigen“ als Schalter.

## 0.41.43 — 2026-11-05

- **`crates/gui/src/app.rs` aufgeteilt** (11 656 → rund 4 100 Zeilen): Die `impl WoaApp`-Blöcke liegen unverändert in `app/transport.rs`, `project_io.rs`, `menu.rs`, `tracks.rs`, `timeline.rs`, `mixer.rs` und `updates.rs`; verschobene Methoden sind `pub(super)`. Kein Verhaltenswechsel. Bauregel-Tests, die den Quelltext lesen (`dialogs_pause_the_watch`, `devices_are_listed_only_here`, die Nacharbeit-Tests), prüfen alle Dateien über `APP_SOURCES`. Workspace-Version 0.41.43.

## 0.41.42 — 2026-11-05

- **WOA startet auf frisch installiertem Windows:** `woa.exe` (MSI und ZIP) brach dort mit „VCRUNTIME140.dll nicht gefunden“ ab, weil die Visual-C++-Runtime fehlt (Rückmeldung eines Moderators nach Neuinstallation von Windows 11). Die C-Runtime wird jetzt statisch gelinkt (`.cargo/config.toml`, `+crt-static` für `x86_64-pc-windows-msvc`). Der Windows-Bau prüft, dass keine der drei exe `VCRUNTIME140.dll`, `MSVCP140.dll` oder `api-ms-win-crt-*` nachlädt; auf dem Runner ist die Runtime vorinstalliert, daher fiel es dort nie auf. Workspace-Version 0.41.42.
- **Wackliger Test behoben:** `unreachable_server_is_a_connect_error` (woa-stream) nahm einen festen Port neben dem Schein-Server und traf unter Windows gelegentlich den Server eines parallelen Tests; jetzt ein freier Port vom System.

## 0.41.40 — 2026-11-05

- **Regel 6 maschinell (kein `unwrap`/`expect` im Produktivcode):** Workspace-Lints `clippy::unwrap_used` und `clippy::expect_used` auf `deny`, jedes Crate erbt sie (`[lints] workspace = true`); `clippy.toml` erlaubt beides in Tests, Integrationstests (`tests/*.rs`) und im Test-Plugin per `#![allow]`. Vorher ergab ein Audit aller Audio-Callbacks (Ausgabe, Aufnahme, Player, Radio, Vorhören, Zweitgeräte) keinen Verstoß gegen die Echtzeit-Regeln; die Sperre hält das fest.
- **Fund:** `party/dj.rs` brach mit `expect` ab, falls die Übergangs-Taste nicht gezeichnet wurde; jetzt einfach kein Klick.
- **Fund:** `woa-headless` mit Feature `device` baute nicht mehr (`InputConfig` hatte das neue Feld `pair`). Behoben; der Windows-Job der CI prüft `woa-headless --features device` jetzt mit.

## 0.41.39 — 2026-11-05

- **Feedback bei Tageslimit wird später gesendet:** Antwortet der Feedback-Server mit 429 (`{"error":"daily limit reached"}`), zeigt der Dialog „Tageslimit erreicht, wird später gesendet“ statt „Senden fehlgeschlagen“. Der fertige Anfrage-Rumpf liegt als Datei im Datenordner (`feedback-ausstehend/`) und geht unverändert erneut, damit der Server Dubletten erkennt; auch nach einem Neustart. Geprüft wird jede Minute, gesendet erst nach Wartezeit (30 min, verdoppelt je Fehlversuch, höchstens 6 h), beim ersten 429 einer Runde hört sie auf; nach 40 Fehlversuchen wird die Meldung verworfen und geloggt. Neu im Telemetrie-Crate: `FeedbackError::DailyLimit`, `post_payload`. Tests `daily_limit_is_held_back_and_resent_unchanged` (Schein-Server 429, 429, 200), `outbox_backoff_doubles_and_is_capped`. Prüfpunkt `feedback-tageslimit`, Klickliste in `docs/abnahme.md`. Workspace-Version 0.41.39.

## 0.41.38 — 2026-11-05

Gäste und Telefon nach den Abstimmungen des Moderators unter Entwickler › Fragen. Prüfpunkte in Entwickler › Abnahme, Klickliste in `docs/abnahme.md`. Workspace-Version 0.41.38.

- **Höchstens 4 Gäste gleichzeitig** (Frage `gaeste-anzahl`): `MAX_GUESTS` ist 4 statt 8. Die Tasten „Testgast“ und „Datei als Gast“ sind ab dem vierten Gast aus, ein fünfter Browser-Gast wird abgewiesen; die Bank hat nur noch vier Kanäle.
- **Ein Link mit PIN für alle Gäste einer Sendung** (Frage `gaeste-einladung`): Statt einer Einladung je Gast gibt es einen Link (6 Stunden gültig) und eine sechsstellige PIN, die nicht im Link steht und groß auf der Gäste-Karte erscheint. Die Gast-Seite fragt Name und PIN. „Neuer Link“ macht Link und PIN ungültig, verbundene Gäste bleiben. Schutz (`gate.rs`): PIN-Vergleich in konstanter Zeit; 5 falsche PINs sperren die Adresse für 10 Minuten; 20 falsche PINs über alle Adressen verbrennen den Link; ein Erfolg setzt keinen Zähler zurück (Lehre aus #116); ist die Sendung mit 4 Gästen (wartend oder angenommen) voll, wird ein neuer Gast abgewiesen, nie ein vorhandener verdrängt. `/gast/check` prüft nur den Link und verrät nichts über die PIN. Tests: `one_link_for_several_guests`, `new_link_burns_the_old_but_keeps_guests`, `wrong_pin_locks_the_address_and_success_does_not_reset`, `many_wrong_pins_from_many_addresses_burn_the_link`, `full_show_refuses_new_guests_and_never_evicts`, dazu `join_state_leave_roundtrip` und der Loopback-Test mit falscher PIN.
- **Annehmen führt ins Vorgespräch, Ducking sobald ein Gast auf Sendung ist** (Frage `gaeste-vorgespraech`): Angenommene Browser-Gäste landen wie bisher zuerst im Vorgespräch (hören die Sendung, sind nicht zu hören); Karte und Gast-Seite heißen jetzt „Vorgespräch“ statt „Warteraum“. Neu: Ist mindestens ein Gast auf Sendung, duckt die Musik (Playlist, Decks, Carts) wie beim Telefon; im Vorgespräch und bei Stumm durch den Moderator nicht, beim Selbst-Stumm des Gastes weiter. Die Bank zählt Gäste auf Sendung (`GuestBank::any_on_air`, Befehl `GuestCmd::OnAir`), der Mixer prüft das je Block. Tests: `on_air_guest_is_reported_for_ducking_but_not_in_the_pre_talk`, `guest_ducks_the_music_only_once_on_air` (unter `assert_no_alloc`).
- **Telefon auf der Gäste-Karte und als eigener Kanal im Pult** (Frage `gaeste-telefon-taste`, beide Vorschläge bestätigt): Die Gäste-Karte hat einen eigenen Abschnitt TELEFON, getrennt von den Gästen, mit Taste „Anrufer auf Sendung“ und Pegel. In der Karte PEGEL steht neben dem Master ein Kanalzug „Telefon“ mit Fader (Doppelklick 0 dB) und LED-Kette; der Name leuchtet rot, solange das Telefon auf Sendung ist. Eine eigene Taste unter dem Kanalzug hätte die Karte erhöht und die Wunschzeile bei 1920 × 1030 unter den Rand geschoben (Test `everything_fits_1920x1030`), geschaltet wird deshalb auf der Gäste-Karte oder in der Tastenreihe. Der Fader wirkt, solange das Telefon auf Sendung ist (`Cmd::PhoneLevel`, `Controller::set_phone_level`); er wird wie die anderen Pegel nicht gespeichert. Die Telefon-Taste in der Tastenreihe bleibt. Test `phone_fader_sets_the_caller_level_on_air_and_off_is_silent`.
- **Gäste-Steuerung ohne Rückgängig** (Frage `gaeste-undo`): war schon so. Annehmen, Auf Sendung, Stumm, Lautstärke, Abhören und Auflegen gehen direkt an die Gäste und nie in einen Rückgängig-Stapel; im Radio gibt es Rückgängig nur für Cue-Punkte. Nichts geändert.

## 0.41.37 — 2026-11-05

Rückmeldungen des Moderators aus Entwickler › Fragen umgesetzt.

- **DJ-Modus: Deck-Badges gefüllt:** „DECK A/B“ und „LÄUFT“ im Kopf der DJ-Decks haben eine volle Fläche in ihrer Farbe mit dunkler Schrift, damit man sie im dunklen Club sieht. Außerhalb des DJ-Modus bleiben Badges nur umrandet. Neu `StateChip::filled`, Test `filled_chips_are_solid_only_when_asked`.
- **Vorhören-Knopf an jeder Playlist-Zeile und jeder Cart (#273):** Der Kopfhörer steht an jeder Zeile und jedem belegten Pad, nicht erst beim Überfahren mit der Maus; Rechtsklick › Vorhören bleibt. Test `preview_key_shows_on_every_occupied_row_and_cart`.
- **Fade-Dauer einstellbar (#320):** Die Fade-Tasten (Sende-Deck, Decks A–C, laufende Carts, Playlist-Zeile) blenden in der eingestellten Dauer aus, Vorgabe 3 s, wählbar 1–10 s im Rack „Processing & Übergänge“ bzw. unter Einstellungen › Radio › „Fade-Taste: Dauer“. Neues Feld `fade_seconds` in `radio.json`, `Controller::set_fade_seconds`. Eine laufende Blende behält ihre Dauer. Tests `a_running_fade_keeps_its_length_when_the_setting_changes`, `fade_runs_exactly_the_set_length`.
- **Sekunden bis Gesang (#299):** Ein laufendes Deck zeigt im Untertitel „Gesang in 12 s“ und zählt herunter; ein laufender Cart zeigt dasselbe links unten. Carts messen den Gesangseinsatz beim Laden auf dem Control-Thread. Die RAMP-Anzeige im Sende-Deck nimmt jetzt auch den geschätzten Einsatz, nicht nur die Ramp-Marke. Ohne erkannten Einsatz bleibt die Anzeige leer. Test `seconds_to_vocal_count_down_and_vanish`.
- **Sende-Deck bedient keine Decks mehr (nimmt #325 zurück):** „Stopp“ und die Fade-Taste im Sende-Deck gelten nur der Playlist; Decks A–C stoppen und blenden nur an ihren eigenen Tasten. Das Sende-Deck-Display zeigt laufende Decks als reine Anzeigezeile „DECK A Titel Restzeit“. Carts laufen beim Sende-Deck-Fade weiter (eigene Fade-Taste). „Fade alles“ bleibt für den Agenten. Test `send_stop_and_fade_leave_a_manually_started_deck_alone`.
- **Ducking nach Sprechpause (Moderator):** Offenes Mikro senkt die Musik nur, solange gesprochen wird (Pegel über etwa −45 dBFS). Nach 0,5 s Stille kommt die Musik zurück und blendet linear in 1 s ein; beide Zeiten als Felder im Rack („Ducking: Rückkehr nach“, „Ducking: Einblenden“, 0,1–5 s) und unter Einstellungen › Radio. Ducking wirkt jetzt auch auf Decks A–C und, bei Stimme oder Telefon, auf die Carts (ein Cart duckt sich nicht selbst). Die Einblendzeit gilt auch nach dem Cart-Ducking. Neue Felder `duck_hold_seconds`, `duck_release_seconds` in `radio.json`. Test `ducking_returns_after_half_a_second_of_silence_and_reaches_decks` unter `assert_no_alloc`.
- **MIDI-Learn im DJ-Modus:** Play/Pause und Cue je DJ-Deck sowie der Crossfader lassen sich lernen (Rechtsklick auf die Taste bzw. den Crossfader, oder im Controller-Fenster die Zeile „DJ:“). Neue Ziele `dj_play`, `dj_cue`, `dj_crossfader`; sie wirken nur bei offener DJ-Ansicht. Cue vom Controller ist ein Antippen (setzen bzw. zurück), Halten kennt der MIDI-Weg nicht. Test `dj_targets_are_play_cue_and_crossfader`.- **Prüfpunkte:** sieben neue bzw. geänderte Punkte in `docs/abnahme.json` (Version 0.41.37), Klickliste in `docs/abnahme.md`; Workspace-Version 0.41.37.
- **Rack:** Die beiden Ducking-Zeiten stehen in der Zeile „Musik-Ducking“ neben der Tiefe, damit bei 1920 × 1030 weiter alles ohne Rollen passt.

## 0.41.36 — 2026-11-05

- **Titel mit langem Vorspann werden gelesen (#347):** Dateien, bei denen mehr als 1 MiB Füllbytes oder Tag-Reste vor dem ersten Audio-Frame standen, scheiterten mit „reached probe limit“; das Laden ins Deck und die BPM-Analyse („Analyse fehlgeschlagen“) fielen aus. Scheitert die schnelle Formaterkennung, sucht ein zweiter Versuch bis 32 MiB tief (`decode.rs`, `wide_probe`). Test `mp3_with_more_than_one_mib_of_padding_before_first_frame_is_read`. Workspace-Version 0.41.36.

## 0.41.35 — 2026-11-05

- **Hörer-Seite: Rot nur noch für ON AIR (#451):** Titel, „Hören“-Taste, Lautstärke-Regler und Zahlenanzeigen sind jetzt im Orange des Logos (`#ff9838` dunkel, `#b35900` hell); rot bleibt die ON-AIR-Marke oben rechts. „Hörer jetzt“ leuchtet grün, sobald mindestens ein Hörer da ist (`server/live/live.css`, `live.js`). Workspace-Version 0.41.35.

## 0.41.34 — 2026-11-05

- **Sendezeile: Lampe, Taste und Status-Pille gleich hoch (#450):** Die kleine ON-AIR-Lampe war 24 px, die Taste „Sendung beenden“ 32 px und die Pille „verbunden · Sendemix“ 26 px hoch. Alle drei sind jetzt 32 px (`ON_AIR_ROW_H` in `radio.rs`). Test `on_air_row_widgets_share_one_height`. Workspace-Version 0.41.34.

## 0.41.33 — 2026-11-05

- **Upgrade-Probe: alte Form mit eigenen Komponenten-GUIDs (#452):** Der Release-Lauf v0.41.32 belegt den Filter aus 0.41.32: Von zwei Uninstall-Einträgen unter HKLM bleibt genau die alte Installation, sie wird entfernt. Rot war danach `Program Files\WOA leer`: Die nachgebaute alte Form (`-d Legacy=1`) nutzte dieselben festen Komponenten-GUIDs wie die neue. Windows Installer zählte die Komponente deshalb doppelt und ließ die Dateien liegen. Die echten MSI bis 0.41.25 hatten automatische GUIDs, betroffen war nur der Test. Die alte Form hat jetzt eigene GUIDs. Test `legacy_form_has_its_own_component_guids` (vorher rot). Workspace-Version 0.41.33.

## 0.41.32 — 2026-11-05

- **Umstieg entfernt nur die alte Installation, nie die neue (#452):** Der Release-Lauf v0.41.30 zeigte: Der Uninstall-Zweig unter HKLM listet auch die neue Installation pro Benutzer. `legacy.rs` hätte beim ersten Start beide per `msiexec /x` entfernt, also auch sich selbst. Jetzt bleibt nur ein ProductCode, den Windows Installer für alle Benutzer führt (`HKLM\SOFTWARE\Classes\Installer\Products\<gepackte GUID>`, neu `per_machine_key`). `test-upgrade.ps1` filtert genauso und prüft, dass genau die alte Installation übrig bleibt. Der Lauf belegt außerdem: Das MSI pro Benutzer installiert, aktualisiert und deinstalliert ohne Fehler. Test `per_machine_key_packs_the_guid`. Workspace-Version 0.41.32.

## 0.41.31 — 2026-11-05

- **Notfall ist sichtbar, leere Notfall-Playlist wird gemeldet (#400, Teil 2):** Sendet `woa-streamd` die Notfall-Playlist, steht neben der ON-AIR-Lampe der Kopfleiste und im Sendepult in Rot „NOTFALL: Stille vom Sendemix seit x s“ bzw. „NOTFALL: Oberfläche getrennt seit x s“ (neue Felder `emergency_reason`, `emergency_seconds`, `emergency_files` in `streamd-status.json`). Ist die Notfall-Playlist leer, steht das beim Gehen auf Sendung als Hinweis im Sendepult, und die NOTFALL-Zeile sagt, dass Stille gesendet wird. Offen: Knopf, der Titel aus der Bibliothek in die Notfall-Playlist legt. Tests: `emergency_is_named_with_reason`, `failover::emergency_takes_over_and_hands_back`. Workspace-Version 0.41.31.

## 0.41.30 — 2026-11-05

- **Upgrade-Probe findet die Installation pro Benutzer (#452):** Der Release-Lauf v0.41.28 brach in `test-upgrade.ps1` ab: Das MSI pro Benutzer installierte (msiexec 0), der Test fand aber keinen Eintrag, weil er unter `HKCU\…\Uninstall` suchte. Windows Installer führt Installationen pro Benutzer nicht dort. Der Test fragt jetzt Windows Installer selbst (`ProductsEx`, Kontext pro Benutzer bzw. für alle Benutzer). Die App selbst ist unverändert. Workspace-Version 0.41.30.

## 0.41.29 — 2026-11-04

- **Schnellerer Start: Grafik-Backend festgelegt (#279, Teil 1):** wgpu sucht nicht mehr über alle Backends, sondern unter Linux nur Vulkan, unter Windows DX12 und Vulkan (vorher auch OpenGL/EGL und Software-Treiber). Die Grafikkarte wird mit `LowPower` gewählt, damit auf Hybrid-Notebooks die schlafende Karte nicht geweckt wird. `WOA_WGPU_BACKEND=gl|vulkan|dx12|all` übersteuert, `WGPU_POWER_PREF` ebenso. Die Startzeile im Log nennt `t_wgpu_ms` und `t_fonts_ms`, eine zweite Zeile „woa app ready“ `t_app_ms` und `t_ready_ms`. Tests `backends_are_pinned_per_os_and_overridable`, `wgpu_configuration_prefers_low_power`. Noch offen: gestaffelter Start der Audio-/MIDI-/Plugin-/Bibliotheks-Init und die Messung am Gerät. Workspace-Version 0.41.29.

## 0.41.28 — 2026-11-04

- **Upgrade-Probe baut die alte Form wieder (#452):** Der Release-Lauf v0.41.27 brach in `test-upgrade.ps1` ab: Das MSI pro Benutzer baute, die nachgebaute alte Form für alle Benutzer (`-d Legacy=1`) nicht (`WIX0094`, `RemoveFolder` verwies auf `PROGRAMSFOLDER`, den es nur pro Benutzer gibt). Die Zeile steht jetzt nur in der Form pro Benutzer. Test `directory_refs_defined_in_both_forms` (vorher rot). Workspace-Version 0.41.28.

## 0.41.27 — 2026-11-04

- **Windows-MSI baut wieder (#452):** Der Release-Lauf v0.41.26 brach im WiX-Schritt ab (`WIX0230`): Komponenten mit Dateien und HKCU-KeyPath bekommen keine automatische GUID. `WoaExe` und `Sounds` haben jetzt feste GUIDs. Test `file_components_have_fixed_guid` (vorher rot). Workspace-Version 0.41.27.

## 0.41.26 — 2026-11-04

- **Windows: Installation und Update pro Benutzer, ohne Admin-Abfrage (#452):** Das MSI (`packaging/windows/woa.wxs`) installierte für alle Benutzer (`C:\Program Files\WOA`, HKLM); jede Installation und jedes Update verlangte die Abfrage der Benutzerkontensteuerung, auch still. Jetzt `Scope="perUser"`, Ziel `%LOCALAPPDATA%\Programs\WOA`, Registry nur HKCU (KeyPath je Komponente ein HKCU-Wert, `RemoveFolder` für die Ordner, Startmenü-Verknüpfung nicht mehr angekündigt), UpgradeCode unverändert. `woa-streamd` und die Audio-Ausgabe (cpal/WASAPI, kein ASIO) brauchen keine Admin-Rechte. Umstieg: MajorUpgrade sieht die alte Installation nicht (anderer Kontext); die neue App entfernt sie beim ersten Start aus dem Benutzerordner einmal per `msiexec /x {ProductCode} /passive` (`crates/updater/src/legacy.rs`, einmalige Admin-Abfrage, Merker `altinstallation-entfernt.txt`). Nutzerdaten bleiben. `test-upgrade.ps1` prüft zusätzlich den Umstieg (alte Form per `-d Legacy=1`). Tests (vorher rot): `msi_installs_per_user`, `msi_writes_no_machine_wide_state`; dazu `finds_only_exact_msi_entries`, `per_user_only_from_local_app_data`. Prüfpunkt `msi-pro-benutzer`. Workspace-Version 0.41.26.

## 0.41.25 — 2026-11-04

- **Esc schließt das oberste Fenster, auch Audio-Geräte (#441, Abnahme 0.41.24):** Ursache: Das Fenster „Audio-Geräte“ (`outputs_ui.rs`) war nicht bei der gemeinsamen Esc-Regel (`esc.rs`) angemeldet. Lagen Einstellungen und Audio-Geräte übereinander, kannte die Regel nur die Einstellungen und schloss sie; Audio-Geräte allein sah gar kein Esc. Jetzt meldet sich Audio-Geräte an (neu `esc::nach` für die Antwort von `Window::show`); „oberstes“ folgt der Ebenen-Reihenfolge von egui (zuletzt geöffnet bzw. angeklickt liegt oben). Ebenso nachgemeldet: Controller und MIDI-Learn, Speicherorte, Sendung: Stream-Server, Dienste und Konten, Entwickler-Modus, KI-Chat, Exportieren, Spur exportieren, Stundenuhr und AutoDJ, Zeit-Events, Verwalten, Protokoll, Produktionen, Titelinfos, Jingle bauen, Cue- und Voice-Editor; Feedback, „Playlist wechseln?“ und „Umbenennen“ als Fragen (Esc = Abbrechen). Tests (vorher rot): `escape_closes_audio_devices_above_settings`, `escape_closes_audio_devices_alone_after_click` (über den Knopf in den Einstellungen geöffnet, mit und ohne Klick ins Fenster). Prüfpunkt `esc-audio-geraete`. Workspace-Version 0.41.25.

- **DJ-Ansicht bei 1280 px ohne Abschneiden (#443, Abnahme 0.41.24):** Ursache: Die Decks hatten eine feste Mindestbreite von 300 px, die Tastenspalte mindestens 200 px und das Mischpult feste 390 px. Bei 1280 px (und erst recht bei 125 % Skalierung) war die Zeile breiter als das Fenster; egui weitete sie aus, Deck B und die rechte Spalte (Wunschliste, Fernbedienung) liefen rechts hinaus. In niedrigen Fenstern liefen die Inhalte unten über den Rand, bei 576 pt Höhe brach die Ansicht mit „Negative height“ ab. Jetzt: Mischpult 30 % der Breite (270–390 px), Decks teilen den Rest ohne Untergrenze, die Jog-Scheibe wird vor den Tasten kleiner (bis 64 px), Wellen- und Unterbereich passen sich der Höhe an; unter 600 pt rollt die Ansicht senkrecht statt abzuschneiden; im schmalen Deck heißt der Warn-Chip „Kein Tempo“ statt „Analyse fehlgeschlagen“ und die Chips brechen bei Bedarf um. Der Test wartet das Ende der Analyse ab (vorher unter Last zufällig rot). Test `party_dj_with_loaded_decks_fits_1280` mit zwei geladenen Titeln bei 1280×800 und 1280×720, je 100 % und 125 % (vorher rot), dazu `deck_body_fits_and_jog_shrinks_first`. Prüfpunkt `dj-1280`.

## 0.41.24 — 2026-11-04

- **Klickliste** „Abnahme-Nacharbeit 04.11.“ in `docs/abnahme.md` (#441–#444); Workspace-Version 0.41.24.

- **Abnahme 04.11.2026 (WOA 0.41.16, Windows 11) übernommen:** 32 bestandene Prüfpunkte in `docs/abnahme.json` mit `"geprueft": "abnahme-lauf 2026-11-04"` (das Wort „claude“ sperrt der Test `embedded_checklist_holds_nothing_internal`) markiert (entfernt werden sie erst nach Andrés Bestätigung).
- **#443 Nacharbeit, im Fotomodus bei 1280×720, 1366×768 und 1920×1030 (dunkel und hell) nachgemessen:** (1) Party DJ: Die Anschiebe-Tasten unter dem Tempo-Regler waren mit Polsterung 70 statt 56 px breit; Deck B und die rechte Spalte (Wunschliste, Fernbedienung) ragten dadurch 14 px aus dem Fenster, auch bei 1920 px. (2) Transport: In der umbrechenden Reihe lieferte `available_width()` die ganze Zeile statt des Rests, die rechte Gruppe lag per `with_layout` über der Pegel-LCD („Takte“/„Zeit“ auf den LEDs) und das ms-Feld rutschte unter den Latenz-Schalter. Jetzt zählt der Rest der Zeile, die rechte Gruppe ist ein eigenes Element, die Latenz bleibt einzeilig; bei 1920 px wieder eine Zeile. (3) Rack: Der Vorhören-Knopf war mit Polsterung 40 statt 26 px breit und schob Klangauswahl, Bank und Synth-Taste 28 px über den Rand; der Trennstrich vor dem Rack war nicht eingerechnet. (4) Audio-Geräte: passt seit 0.41.23, nur Fotomodus `WOA_AUDIO=1` neu. Tests: `party_dj_fits_the_window`, `sound_picker_fits_the_window`, `transport_never_overlaps` (AccessKit-Rechtecke, vorher rot).
- **Eine Esc-Regel für alle Fenster (#441):** Esc schließt das oberste offene Fenster; eine offene Frage („Projekt speichern?“, „Projekt überschreiben?“) geht vor und heißt Abbrechen. Hatte ein Textfeld den Fokus, nimmt das erste Esc nur den Fokus weg; bei offenem Menü schließt Esc nur das Menü. Neu dabei: Bibliothek und Analyse-Fenster. Einstellungen, Fragen und To-dos, Abnahme, Hörer-Statistik, Piano-Roll als Fenster und Über WOA laufen über dieselbe Regel (`crates/gui/src/esc.rs`); vorher schloss ein Esc alle offenen Fenster mit Kopfzeile zugleich, und die Textfokus-Sperre griff nie, weil egui den Fokus bei Esc schon vor dem Bild löscht. Tests: `escape_closes_library_in_party`, `escape_closes_analysis_window`, `escape_cancels_exit_question_in_every_view`, `escape_closes_settings_and_questions`, `escape_closes_only_the_top_window`, `escape_cancels_question_before_window`, `esc::tests::escape_in_text_field_only_drops_focus`, `escape_untouched_without_window`. Der Bibliothekstest `titelinfos` drückte zum Schließen des Menüs Esc und schloss damit nun die Bibliothek; der Schritt ist raus (das Menü schließt mit dem Klick).
- **Test für #442:** `click_hits_right_edge_when_scaled` klickt bei Skalierung 125 % und 150 % (1920 × 1030 Pixel) auf „Party“ oben rechts; der Klick kommt an. Den Windows-Fensterfehler selbst prüft nur das Gerät (Prüfpunkt `maximiert-klicks`).
- **Prüfpunkte angeglichen:** `radio-deck-menue` nennt die echten Einträge „Playlist ablösen, mit Blende/sofort“; `klang-neue-midi-spur` erwartet seit dem neuen Spurkopf das Flügel-Symbol statt des Worts.

## 0.41.23 — 2026-11-04

- **Bei etwa 1280 px Fensterbreite überlappt nichts mehr (#443):** (1) Party DJ: Die Tasten-Zeilen der Decks (Auto-Loop, Cue/Play/Sync) waren breiter als ihre Spalte und schoben das Jogwheel von Deck B aus dem Fenster; sie füllen die Spalte jetzt genau. Restzeit, BPM und Tonart rutschen bei schmalem Deck in eine zweite Kopfzeile statt auf die Chips. (2) Produzent, Transport: umbricht in eine zweite Zeile, wenn Gruppen nicht mehr passen; die Pegel-LEDs verschwinden nicht mehr. (3) Rack, Klangauswahl: Kategorie-Tasten zeigen nur den Namen, wenn Symbol und Name nicht passen („Streicher“ statt „Strei…“); Vorhören-Knopf mit Abstand zum Rand. (4) Audio-Geräte: öffnet oben, bleibt im Bild, die Rollfläche passt sich der Fensterhöhe an. Tests: `deck_key_rows_never_exceed_their_width`, `audio_device_window_fits_small_screens`. Wie es am Gerät aussieht, zeigt nur das Fenster: Prüfpunkt `fenster-1280`.

## 0.41.22 — 2026-11-04

- **Maximiertes Fenster: Klicks rechts gehen wieder (#442):** WOA maximierte sich schon beim Anlegen des Fensters (`with_maximized`). Unter Windows 11 mit Skalierung füllte das Fenster dann nur etwa zwei Drittel, und die Eingabe rechts kam nicht an. Jetzt wird mit Rahmen angelegt und nach dem ersten Bild per Befehl maximiert (wie F11 schon). Test: `maximize_is_sent_once_by_command_on_first_frame`. Ob es unter Windows wirkt, zeigt nur die echte Maus am Gerät: Prüfpunkt `maximiert-klicks`.

## 0.41.21 — 2026-11-04

- **„Über WOA“ schließt mit Esc (#441):** Wie Einstellungen und die übrigen Fenster schließt Esc jetzt auch das Fenster „Über WOA“ (nur ohne Textfokus). Test: `escape_closes_about_window`.

## 0.41.20 — 2026-11-04

- **Version:** Cargo-Version auf 0.41.20 gesetzt (Testversion mit den Fixes unten).
- **Pult-Modus schaltet live um (#399):** „Sendung kommt vom Pult“ während der Wiedergabe umgelegt, wirkt jetzt sofort im Mixer: aus = erst interne Mischung auf Stream und Mitschnitt, dann Eingang lösen; an = erst Eingang öffnen, dann umschalten (geht er nicht auf, bleibt die interne Mischung mit Meldung). Log: `pult mode on/off (live)`. Geänderte Ausgänge werden gemerkt, im Fenster „Audio-Geräte“ steht „gelten ab Ende der Wiedergabe“, und sie werden danach ohne Neustart angewendet. Vorher ging die Änderung bis zum Neustart verloren. Die Anzeige „PULT“ folgt dem wirklichen Zustand des Mixers.
- **Stummer Sendeweg wird sichtbar (#400, Teil):** Auf Sendung mit hörbarem Ausgang (> −40 dBFS), aber stillem Stream (< −60 dBFS) steht nach 5 s im Sendepult „Stream bekommt keinen Ton (Quelle: Pult-Eingang/Sendemix)“, mit Logzeile. `woa-streamd` schreibt beim Wechsel auf und von der Notfall-Playlist `emergency on reason=silence|ui disconnected` und `emergency off`. Offen: Hinweis bei leerer Notfall-Playlist beim Gehen auf Sendung.
- **Pult-Eingang: Aussetzer gezählt, Log gedrosselt (#401, Teil):** Unter-/Überläufe des Pult-Eingangs zählt WOA und zeigt sie neben dem PULT-Pegel; ins Log kommt höchstens eine Sammelzeile je Minute mit Anzahl statt einer je Ereignis. Puffergröße des Eingangs unverändert: ob der USB-Codec hörbar knackt, lässt sich nur an der echten Hardware messen.
- **Neu und Öffnen fragen mit „Nicht speichern“ (#408):** Bei ungespeicherten Änderungen erscheint vor Neu, Öffnen und DAWproject öffnen dieselbe Frage wie beim Beenden: Speichern · Nicht speichern · Abbrechen (Esc = Abbrechen). Bisher wurde Speichern erzwungen. Ein zweiter Klick öffnet keine zweite Frage. Tests: `new_with_unsaved_changes_offers_discard_and_asks_once`, `open_with_unsaved_changes_cancel_keeps_project`, `new_project_asks_before_dropping_unsaved_changes`.
- **Speichern nie mehr in einem fremden `.woa`-Paket (#409):** Wählt der Speichern-Dialog einen Ort innerhalb eines Projektordners, landet das Projekt daneben (`Projekte/X.woa` statt `Projekte/A.woa/X.woa`). Liegt am Ziel schon ein anderes Projekt, fragt WOA „Projekt überschreiben?“ und sichert das alte als `project.json.bak`. Tests: `save_as_never_nests_inside_a_project_package`, `save_as_over_another_project_keeps_a_backup`.
- **Öffnen eines normalen Ordners (#405):** Wer statt eines `.woa`-Ordners einen Ordner wie `Projekte/Robby` wählt, bekommt das eine Projekt darin geöffnet; liegen mehrere darin, nennt die Statuszeile sie, sonst den Hinweis, einen `.woa`-Ordner zu wählen (bisher: „Das System kann die angegebene Datei nicht finden“). Test: `open_finds_the_project_in_a_plain_folder_or_explains`. Code mit `cargo fmt` formatiert.
- **Party: am Ende der Playlist keine Schleife der letzten zwei Titel (#434):** Mit zwei Decks blieb nach dem letzten Titel der vorletzte (schon gespielte) auf der freien Seite liegen, der Auto-Übergang startete ihn, danach wieder den letzten, im Kreis. Jetzt wird die freie Seite geleert, wenn nichts folgt: bei „Wiederholen aus“ endet die Party nach dem letzten Titel, bei „Playlist wiederholen“ (Taste R) geht es mit dem ersten weiter. Die Notfall-Schleife von `woa-streamd` ist davon nicht berührt. Tests: `party_duo` (Attrappen, kein Ton).
- **Studio-Dock bleibt so klein, wie man es zieht (#396):** Nach dem Kleinerziehen wanderte das Dock von selbst wieder nach oben. Ursache: egui meldet die Höhe des Inhalts an das Panel zurück; war der Inhalt (Kanalzüge, Rack) höher als die gezogene Größe, wuchs das Panel wieder auf und speicherte diese Höhe. Jetzt bekommt der Inhalt genau die Fläche des Docks und wird unten beschnitten (Rack rollt wie bisher selbst). Die Vorgabehöhe steigt von 330 auf 400 px, damit „+ Effekt“ und Ausgang beim ersten Öffnen ohne Rollen sichtbar sind (bisher wuchs das Dock genau dorthin). Test: `app::nacharbeit_tests::studio_dock_keeps_the_dragged_size` (vorher rot: Kante sprang von 821 auf 714).
- **Cart-Wall: Vorhören-Symbol liegt nicht mehr über Taste, LED und Titel (#388):** Das Kopfhörer-Symbol (beim Hover) und das Fade-Symbol (laufend) lagen oben rechts über dem Tastenschild, der Status-LED und dem Titelende. Das Pad weiß jetzt, wie viele Symbole dort liegen (`CartPad::actions`): die Ecke bleibt frei, der Titel endet davor, Countdown bzw. Länge rücken 3 px tiefer. Test: `widgets::pad::tests::pad_leaves_the_corner_free_for_icons`.

## 0.41.19 — 2026-11-04

- **Hörer = eindeutige Hörer statt Verbindungen:** Ein iPhone (Safari) öffnet zwei Verbindungen zugleich; `/7.html` meldete `2,1,2,50,1,…` und überall standen 2 Hörer, obwohl nur einer zuhörte. Jetzt zählt überall Feld 4 (`eindeutig`/`unique`), Rückfall auf `aktuell` nur, wenn es fehlt. Sammler: `/api/jetzt` (`hoerer`, `spitze`) und `/api/hoerer` (`jetzt.aktuell`, Verlauf, Sendungsspitze und -schnitt) aus den gespeicherten eindeutigen Werten; die Shoutcast-Spitze (zählt Verbindungen) fällt als Rückfall weg; neues Feld `jetzt.verbindungen`. Felder bleiben kompatibel, Hörer-Seite und Startseite brauchen keine Änderung. `woa-streamd` meldet `listeners` eindeutig; App-Chip und Fenster „Hörer-Statistik“ ebenso (Kachel „Verbindungen“ statt „Eindeutig“). Tests: Attrappen mit `2,1,2,50,1`. Code mit `cargo fmt` formatiert.

## 0.41.18 — 2026-11-03

- **Sammler: CORS für die Startseite:** `GET /api/jetzt` und `GET /api/hoerer` senden `Access-Control-Allow-Origin` nur für genau `https://www.wir-on-air.de`, `https://wir-on-air.de`, `https://www.wironair.de` und `https://wironair.de` (Origin geprüft und zurückgegeben, `Vary: Origin`, keine Wildcard, keine Credentials). `OPTIONS` auf diesen Pfaden antwortet 204 (`GET`). Am nginx (bretteins.infra, Rolle edge) ist nichts zu ändern: er reicht die Köpfe durch; `limit_except GET` sperrt zwar `OPTIONS`, ein schlichter `fetch` ohne eigene Köpfe braucht aber keinen Vorabruf. Test: `test_cors_nur_fuer_wir_on_air` (Attrappe).

## 0.41.17 — 2026-11-03

- **README deutlich aktualisiert:** Stand 0.41.x mit Senden über `woa-streamd`, Titel über den eigenen Server, Hörer-Statistik und Hörer-Seite, Dienste und Konten (MCP, Chat-Anbieter, ElevenLabs), Audio-Geräte an einem Ort, Pult-Modus. Bilder aus `docs/design/ist/` eingebunden (hell/dunkel nach Systemthema), Download und Webseite www.wir-on-air.de verlinkt.

## 0.41.16 — 2026-11-03

- **Hörer-Seite ohne Design auf `/`:** Die Seite lud CSS, Skript und Bilder relativ; auf stream.wironair.de/ landeten diese Abrufe beim Stream statt bei den Dateien. Jetzt absolut unter `/live/`.

## 0.41.15 — 2026-11-03

- **Hörer-Seite `server/live/`:** eine statische Seite für Hörer im Browser (ohne Framework, Build, Tracking, Cookies und externe Quellen; Schriften selbst gehostet, SIL OFL). Großer Knopf „Hören“ (Start per Klick, Browser blocken Autoplay mit Ton; `preload="none"`, damit ein Seitenaufruf keinen Hörer zählt; Stopp schließt die Verbindung), „Läuft gerade“, Hörer jetzt und Spitze, zuletzt gespielte Titel, Status ON AIR / Notfall-Schleife / Sendepause und optional die nächste Sendung aus `naechste.json`. Farben aus den Themen `radio` und `radio-hell` (folgt dem System), mobil tauglich, Tastatur-Fokus sichtbar, Statuswechsel per `aria-live`. Titel kommen nur per `textContent` ins Dokument.
- **Sammler: `GET /api/jetzt`:** Titel aus `/currentsong`, die letzten zehn aus `/played?type=json` (im Docker-Netz, mit Browser-Kennung, Entities aufgelöst, Steuerzeichen raus, 200 Zeichen), Hörer und Spitze der laufenden Sendung aus SQLite. 10 s Zwischenspeicher (`Cache-Control: public, max-age=10`), keine IPs. Ist Shoutcast gesperrt oder weg, bleibt die Antwort 200 mit leeren Titeln. Tests: `JetztTests` in `server/hoerer/test_hoerer.py` (Attrappe, kein echter Server).

## 0.41.14 — 2026-11-03

- **Datenschutz Design-Bilder:** Drei Bilder unter `docs/design/ist/` zeigten unter WUNSCHTITEL einen Pfad mit einem lokalen Benutzernamen und sind neu im Fotomodus aufgenommen (`WOA_SCREENSHOT`, `WOA_WINDOW=1920x1030`, leerer Daten- und Konfigurationsordner unter `/tmp/woa-foto-0.41.14/`): `ist-f7-onair-dunkel-…` (`WOA_ONAIR=1`), `ist-f7-abnahme-dunkel-…` (`WOA_ABNAHME=1`), `ist-f7-fragen-dunkel-…` (`WOA_FRAGEN=1`); Dateinamen gleich, Verweise stimmen. Alle 94 PNGs unter `docs/` per Texterkennung geprüft, Wunschtitel-Leiste aller Ist-Bilder angesehen: sonst keine privaten Pfade. Die Git-Historie bleibt unverändert.

## 0.41.13 — 2026-11-03

- **Titel für Hörer auch bei entferntem Shoutcast:** Bisher sendete WOA gegen den eigenen Server keinen Titel, weil `admin.cgi?mode=updinfo` den Admin-Port (von außen zu) und das Kennwort im Klartext in der URL braucht. Jetzt geht der Titel per HTTPS an den Sendeserver (`POST https://<host>/api/titel`, abweichend per `title_url` in der Stream-Konfiguration; `http://` nur auf Loopback, keine Weiterleitungen). Die App weist sich mit dem Quellkennwort aus dem Schlüsselbund aus (`Authorization: Bearer`), ein neues Geheimnis gibt es nicht. Lokal bleibt alles wie bisher. Bei einer IP als Host ohne `title_url` bleibt der Titel aus (kein Zertifikat). Tests: `title_url_only_for_remote_shoutcast_names`, `title_body_is_short_and_one_line`, `remote_title_goes_to_server_endpoint`.
- **Sammler `server/hoerer/hoerer.py` nimmt `POST /api/titel` an:** vergleicht konstantzeitig mit der eingehängten Quellkennwort-Datei, säubert und kürzt den Titel (200 Zeichen, Rumpf höchstens 2 KB) und ruft `admin.cgi` im Docker-Netz mit dem Admin-Kennwort aus einer Datei auf; Kennwörter stehen in keinem Log und keiner Antwort. Ohne Kennwortdateien 503. nginx-Vorlage um `/api/titel` (nur POST, 30/min je IP) ergänzt. Tests: `TitelTests` in `server/hoerer/test_hoerer.py`.

## 0.41.12 — 2026-11-03

- **Hörer-Chip oben liest vom Statistik-Server:** „HÖRER x | y“ neben ON AIR nimmt `jetzt.aktuell` und die Spitze der laufenden Sendung aus `hoerer_url` (Vorgabe `https://feedback.wironair.de/api/hoerer`), höchstens alle 30 s im Hintergrund, „–“ bei Fehler. Bisher fragte woa-streamd `/7.html` am Admin-Port, der von außen zu ist, darum blieb der Chip leer. Fenster und Chip teilen sich jetzt den Abruf (`hoerer::Abruf`). Tests: `chip_numbers_come_from_the_server_answer`, `chip_stays_dash_without_test_server`.
- Der `/7.html`-Abruf in woa-streamd bleibt: Status-Zeile und Agent-Werkzeug (Sendestatus, `listeners`) nutzen ihn bei lokalem Server weiter.

## 0.41.11 — 2026-11-03

Hörerzahlen (Auftrag André 03.11., Ausnahme von der Fehlerphase).

- **Hörer oben in der Kopfleiste:** Auf Sendung steht neben ON AIR „HÖRER 12 | 18“ (jetzt | Spitze dieser Sendung) im Stil der REC-Laufzeit, grün statt rot, „–“ solange keine Zahl kommt; Klick öffnet die Statistik. Die Zahl liest woa-streamd wie bisher alle 15 s aus der öffentlichen Shoutcast-Kurzstatistik `/7.html` (ohne Kennwort), jetzt samt Spitze und eindeutigen Hörern (`listeners_peak`, `listeners_unique` in `streamd-status.json`). Tests: `seven_html_gives_current_peak_unique`, `chip_shows_current_and_session_peak_or_dash`, `session_peak_resets_with_each_broadcast`.
- **Fenster Hörer-Statistik (Ansicht › Hörer-Statistik):** Kacheln Jetzt, Spitze, Schnitt, Eindeutig, Linienverlauf der laufenden oder letzten Sendung, Liste der Sendungen der letzten 30 Tage. Die Daten kommen vom eigenen Server (`hoerer_url` in config.json, Vorgabe neben dem Feedback-Server); offline steht eine klare Meldung da. Tests mit Attrappe: `fetch_reads_the_server_answer`, `offline_or_broken_gives_a_clear_message`.
- **Sammler `server/hoerer/hoerer.py`:** läuft auf dem Sendeserver, liest alle 30 s `/7.html`, speichert Zeit, aktuell, Spitze, eindeutig und „Quelle verbunden“ in SQLite, auch wenn die App aus ist oder die Notfall-Playlist sendet; `GET /api/hoerer?von=&bis=` liefert nur aggregierte Zahlen (keine IPs, keine User-Agents). Zeitraum je Abfrage höchstens 90 Tage, `bis` vor `von` oder Nicht-Zahlen ergeben 400, höchstens 200 Sendungen je Antwort; jede Abfrage liest über eine eigene, nur lesende Verbindung (SQLite WAL) und blockiert den Sammler nicht; Endpunkt bindet an 127.0.0.1 (`HOERER_BIND`). nginx-Vorlage um `/api/hoerer` ergänzt. Tests: `python3 -m unittest discover -s server/hoerer`. Noch nicht ausgerollt.

## 0.41.10 — 2026-11-03

- **Dateidialoge zählen nicht mehr als Oberflächen-Hänger (#398):** Datei-, Ordner- und Speichern-unter-Dialoge blockieren den Frame, solange der Mensch wählt; der Wächter meldete das als `ui.hang_open` (ab 12 s) und `ui.frame_slow`. Sie laufen jetzt über `ordner::Modal` und halten die Messung an (`woa_telemetry::pause`); die Pause wird auch aus der Frame-Dauer herausgerechnet. Ursache der zwei Meldungen ist eine Vermutung (kein Kontext im Log); ein echter Hänger bleibt sichtbar und kommt wieder. Tests: `a_modal_dialog_is_not_a_hang`, `pause_time_is_taken_off_the_frame`, `dialogs_pause_the_watch`.

## 0.41.9 — 2026-11-03

Fehlerpaket aus Andrés Tickets vom 03.11. (0.41.8 ist das Radio-Paket daneben).

- **Einstellungen gehen nicht mehr verloren (#380):** Beim Schließen und beim Neuaufbau des Pults wird radio.json sofort geschrieben (vorher gingen Änderungen der letzten 0,7 s verloren). Eine nur berührte Datei mit unserem eigenen Inhalt gilt nicht mehr als „von außen geändert“, das Verwerfen lokaler Änderungen entfällt dann. config.json wird atomar geschrieben; eine kaputte Datei wird als `config.json.kaputt` aufgehoben statt beim nächsten Speichern alles auf Vorgabe zu setzen. Speicherfehler bleiben sichtbar. Tests: `radio::save_380`, `config_save_is_atomic_and_broken_file_is_kept`.
- **Einstellungen-Fenster breiter und gemerkt (#379), Zeile Klangbank je Spur lesbar (#378):** Start 1100×760, mindestens 720 breit, gezogene Größe in `settings_size`. Beschreibungen brechen um und laufen nicht mehr unter die Knöpfe. Test: `rows_never_run_under_their_buttons`.
- **ElevenLabs einrichten (#377):** „Einrichten“ rollt zum ElevenLabs-Block, der nennt die nötigen Rechte (Text to Speech, Sound Effects, Music Generation = Access, Voices = Read, User = Access nur fürs Guthaben, sonst No Access) und empfiehlt Credit-Limit und „Auto-disable if leaked“. Fehlt ein Recht, sagt die Meldung „Schlüssel gültig, aber Recht X fehlt“ statt „abgelehnt (HTTP 401)“; fehlt nur „User“, gilt der Schlüssel ohne Guthabenanzeige. Eingefügte Leerzeichen und Umbrüche werden entfernt. Karten: aktiv/eingerichtet grün, Fehler orange. Tests (Attrappe): `missing_permissions_are_named_and_user_read_is_optional`, `pasted_key_is_cleaned_and_rights_are_listed`.
- **Einrichten übernimmt den Anbieter (#376):** Ollama, OpenAI usw. sowie Claude Desktop/Code sind beim Öffnen vorgewählt (nur im Entwurf). Test: `setup_opens_with_the_clicked_provider`.
- **MIDI-Learn an Deck-Tasten (#371):** Der Text ist richtig (Deck-Tasten sind lernbar, 0.36.1); ohne gewählten Controller sagt die Leiste jetzt, dass erst ein Gerät zu wählen ist. Test: `learn_text_says_when_no_controller_is_chosen`.
## 0.41.8 — 2026-11-03

Fehlerbehebung aus Andrés Rückmeldungen vom 03.11. (Fehlerphase).

- **Cart-Wall: Beim Start von Jingle 3 fiel Jingle 4 aus dem Bild, auch maximiert (#369, #370).** Ursache: Das Fade-Symbol eines laufenden Carts (und das Vorhören-Symbol beim Hover) wurde mit `scope_builder` gezeichnet; im Raster belegte das eine eigene Zelle und schob alle folgenden Carts eine Stelle weiter. Jetzt liegen beide Symbole in einem Kind-Bereich ohne eigenen Platz. Test: `radio::tests::playing_or_hovered_cart_keeps_the_grid` (laufend und Hover, alle 16 in vier Reihen; scheitert mit dem alten Weg). Betrifft wohl auch #388.
- **ON AIR oben rechts leuchtete nicht trotz Sendung (#387).** Ursache: Im Pult-Modus war die Lampe „gedimmt“ (72 % Richtung dunkles Rotglas) und las sich wie aus. Das Wiederanhängen an woa-streamd war in Ordnung. Jetzt leuchtet sie auf Sendung immer voll; der Pult-Modus zeigt sich nur am kleineren Schein. Test: `widgets::lamp::tests::bar_lamp_is_off_on_or_dimmed_in_pult_mode`.
- **Vorhören bei den Decks ging nur bei stehendem Song (#373, #374).** Ursache: Eine neue Kopfhörer-Wahl wartete, bis nichts mehr lief, weil dafür die ganze Radio-Ausgabe neu geöffnet wurde. Ein eigenes Kopfhörer-Gerät ist aber ein eigener Strom: es wird jetzt sofort getauscht, die Sendung läuft weiter. Nur Kanal 3/4 des Sendegeräts wartet wie bisher. Test: `app::pfl_switch_tests::own_headphone_device_switches_live`.
- **Vorgehörtes Deck lief nicht sichtbar (#375).** Ein Deck, das im Kopfhörer läuft, leuchtet jetzt wie ein laufendes Deck, die Restzeit zählt ab der Vorhör-Stelle herunter und darunter steht „Vorhören im Kopfhörer“; Start/Stopp und Fade bleiben beim echten Abspielen. Neu in der Ansicht: `pfl_position`. Test: `radio::tests::previewed_deck_runs_visibly`.
- **Bibliothek in der Party ohne Rückmeldung, und aktiv trotz Feedback-Formular (#386).** Ursachen: Ansicht › Bibliothek setzte in der Party nur den Haken, das Fenster wurde dort nie gezeichnet. Die Party-Tasten (Leertaste, N, P …) liefen auch bei offenem Feedback-Formular weiter, und die Bibliothekssuche behielt den Tastaturfokus. Jetzt öffnet das Fenster auch in der Party, Party-Tasten ruhen unter einem Fenster wie dem Formular, und das Formular nimmt beim Öffnen den Fokus. Die Migration Schema 6 → 7 („library migration from=6 to=7“) ist kein Fehler: geprüft mit einer echten v6-Datenbank. Tests: `party_bibliothek::library_window_opens_in_party_mode`, `party::tests::party_keys_rest_behind_an_overlay`, `tests::migration_from_v6_keeps_tracks_searchable`.

## 0.41.7 — 2026-11-03

Fehlerbehebung und Übersicht, einzige neue Funktion: Mikrofon wählen (André 03.11.: „wo stell ich denn die mikro quelle ein? … doppelt gemoppelt, ich blicke da nicht durch“).

- **Ein Ort für alle Soundkarten: „Audio-Geräte“.** Das Fenster (Einstellungen › Audio und Geräte, Menü Einstellungen) ist gegliedert in Ausgänge (Gerät und Kanalpaar je Quelle der Sendung), Vorhören (PFL) und Eingänge (Mikrofon, Telefon, Pult-Eingang). Jede Zeile sagt in einem Satz, wofür sie ist. Vorher wählten das Untermenü „Vorhören (PFL)“, das Fenster „Ausgänge (Soundkarten)“ und das Fenster „Telefon“ Geräte getrennt. Diese Zeilen sind jetzt nur noch Verweise („In Audio-Geräte öffnen“), Meldungen zeigen auf „Einstellungen › Audio-Geräte“. Die Aufnahmequelle einer Spur im Produzenten bleibt an der Spur.
- **Mikrofon wählbar (neu):** Gerät und Kanalpaar, gespeichert (`mic` in `config.json`), sofort wirksam, auch bei offenem Mikro. Vorgabe Systemstandard wie bisher. Fehlt das Gerät, kommt eine klare Meldung, kein stiller Wechsel auf ein anderes Mikro. Ursache: das Radio öffnete immer das Standard-Eingabegerät. Der Live-Eingang hält jetzt auch das Kanalpaar ein.
- **Bestehende Einstellungen bleiben gültig:** Ausgänge, Vorhören, Telefon und Pult werden unverändert gelesen. Tests: `config_before_audio_devices_window_loads_unchanged`, `outputs_ui::tests` (Mikro-Vorgabe, Mikro-Wahl, ein Satz je Zeile, `devices_are_listed_only_here`), Suche „mikro“/„externes mikro“ findet Audio-Geräte, keine doppelten Menüeinträge (auch `kopfleiste::every_former_menu_entry_is_still_reachable`).

## 0.41.6 — 2026-11-03

- Verbesserungspaket, geplant als 0.41.3; 0.41.2, 0.41.4 und 0.41.5 kamen zuerst auf main, daher 0.41.6.
- **Party: Wunschliste und Fernbedienung ausblendbar (#361):** Schalter „WUNSCHLISTE“ und „FERNBEDIENUNG“ in der Party-Ansichtsleiste und im Menü Ansicht (nur in der Party), dazu Einstellungen › Party › Ansicht. Ausgeblendet nimmt die Bibliothek den Platz und zeigt so viele Treffer, wie passen (mindestens 8, höchstens 40); im DJ steht sie anstelle der Wunschliste. Eine laufende Fernbedienung bleibt an und zeigt den Chip „Fernbedienung an · ausgeblendet“. Gemerkt in `party_panels`. Test: `party::tests`.
- **Abhörlautstärke im Produzenten (#358):** kleiner waagerechter Regler mit Lautsprecher neben der PEGEL-LCD (Doppelklick: voll), auch unter Einstellungen › Produzent › Abhören. Wirkt nur am Lautsprecher nach dem Mix, gleitet je Sample (kein Knacken, keine Allokation); Pegel, Analyse und Export bleiben. Gemerkt in `monitor_volume`. Tests: `woa_dsp::monitor::tests`.
- **Gesangseinsatz je Titel (#299):** Die Analyse schätzt den ersten Gesang (Mitte-minus-Seite im Sprachband, erste 90 s, Hintergrund) und legt ihn in der Bibliothek ab (Schema 7, Spalte `vocal_start`) und an Playlist-Titeln/Carts. Playlist: Mikrofon und Rampzeit ab Cue-In links der Lautheit, geschätzt mit „≈“ in gedämpfter Farbe; Deck: „Gesang ab …“ und Strich auf dem Fortschrittsbalken. Von Hand: Cue-Editor › Rechtsklick in die Wellenform › „Gesang beginnt hier“ setzt die Ramp-Marke, die vor der Schätzung gilt; „Vorschlag einsetzen“ setzt die Ramp mit. Bereits gemessene Playlist-Titel werden einmal nachgemessen. Tests: `analysis::vocal::tests`, `show::tests`, `analysis::tests::vocal_start_is_stored_with_the_analysis`, `vocal_ui::tests`, `cue_editor::tests`.
- **Vorhören per Knopf in Playlist und Cart-Wall (#273):** Kopfhörer-Taste in der Playlist-Zeile (beim Hover/Auswahl, bleibt sichtbar, solange der Titel vorgehört wird) und oben rechts am belegten Cart (links neben dem Fade-Symbol); Klick startet, zweiter Klick stoppt, ohne PFL-Ausgang gesperrt mit Hinweis.
- **Kräftigere Pegel-LEDs (#287):** LED-Ketten der Karte PEGEL 16 statt 10 px breit, weniger und dickere LEDs mit Fuge, leuchtende mit hellem Kern und Schein. Fader und farbige Namensschilder unverändert. Test: `level_leds_are_fat`.
- Bilder 1920×1030 dunkel und hell: `docs/design/ist/ist-0.41.3-{party,produzent,radio}-{dunkel,hell}-1920x1030.png` (Party mit ausgeblendeten Karten, Produzent mit Abhörregler, Radio mit den neuen LEDs).

## 0.41.5 — 2026-11-03

- **Fehler: „Sendung beenden“ und gleich wieder „Auf Sendung“ ließ zwei woa-streamd laufen (#367).** Ja, es war ein Fehler. „Sendung beenden“ schickte `Stop` und kehrte sofort zurück; der nächste Start fand den alten woa-streamd noch am Leben und hängte sich an ihn, oder ein neuer startete, während der alte noch abräumte und dabei die Link-Datei und den Socket des neuen löschte. Ein `Stop` konnte zudem verloren gehen, wenn das Hello der nächsten Sendung die Verbindung vorher ablöste. Folge: Oberfläche „Verbindung zu woa-streamd unterbrochen, verbinde neu …“, der zweite woa-streamd bekam vom Server „Stream In Use“, zu hören war nur die Notfall-Playlist. Jetzt: „Sendung beenden“ wartet (höchstens 5 s), bis woa-streamd sich beim Server abgemeldet hat, und schickt `Stop` erneut, falls er nicht endet (`link::stop_daemon`); woa-streamd wertet `Stop` auch auf einer abgelösten Verbindung aus und löscht beim Beenden nur seine eigene Link- und Socket-Datei. Die Sendetaste ist entprellt (1,5 s), ein zweiter Klick aus einem hängenden Fenster startet oder beendet nichts mehr. Test: `failover::quick_restart_never_leaves_two_streamd` (start → ende → start sechsmal hintereinander gegen einen Attrappen-Server, der wie Shoutcast nur eine Quelle annimmt; scheitert auf 0.41.1).

## 0.41.4 — 2026-11-03

- **CI ohne Fehlalarm (André 03.11.: „erst gar nicht so viele Ereignisse mit Rot-Meldungen“):** rote Läufe nur noch bei echten Fehlern.
  - Wackeltests an der Ursache behoben: `scan::a_file_that_hangs_is_killed_and_blocked` startet das frisch geschriebene Skript über `/bin/sh` (vorher sporadisch ETXTBSY unter Linux, Problem „Suchprozess startet nicht“ statt „hängt“); `live::tests::forward_does_not_wait_…` wartet bis 10 s auf das DELETE statt 2 s; `webrtc_loopback` prüft am Pult und im Rückweg nur noch „Ton deutlich da“ (> -20 dB statt > -14 dB, gemessen -14,4 dB auf Windows); `radio::tests::many_rows_stay_fast` ist in der CI nur eine grobe Schranke (×5 + 20 ms), lokal bleibt ×2 + 2 ms.
  - CI: `concurrency` je PR, ein neuer Push bricht den alten Lauf ab (auf `main` läuft jeder Lauf zu Ende).
  - deny: `cargo-deny` direkt auf dem Runner statt im Container der Action (Toolchain-Meldung „musl is not installed“ weg). Lizenzen, Bans, Quellen laufen immer; Advisories/yanked in PRs nur, wenn sich ein fremdes Paket in `Cargo.lock` (checksum-/source-Zeile, nicht das Hochzählen der eigenen Version) oder `deny.toml` ändert. Voll und streng prüft `deny-woche.yml` montags und auf Knopfdruck.
  - `scripts/vorflug.sh` (fmt, clippy, test) vor jedem Push, optionaler Hook `scripts/hooks/pre-push` (nur fmt) per `git config core.hooksPath scripts/hooks`; Regel in `CLAUDE.md`.

## 0.41.2 — 2026-11-03

Fehlerbehebung, keine neuen Funktionen (André 03.11.).

- **Beenden fragt einheitlich (#365, #362):** Fenster schließen, Datei › Beenden (neu, unten, Strg+Q) und „Jetzt installieren“ gehen über eine Stelle (`exit::ExitGuard`). Bei ungespeichertem Projekt: „Speichern und beenden/installieren“, „Nicht speichern“, „Abbrechen“ (Esc). Abbrechen lässt WOA offen, das Update kommt später. Ursache: das Fenster schloss ohne Frage, das Update hatte nur Speichern, Beenden stand nur unter Entwickler. Tests: `exit::tests` (fünf), `closing_with_unsaved_changes_asks_save_discard_cancel`, `clean_project_quits_without_asking`.
- **Bibliothek → Playlist hängt hinten an (#360):** Ursache: der Knopf „→ Playlist“ fügte als „als Nächstes“ ein, bei stehender Wiedergabe also ganz oben. Jetzt ans Ende; Wünsche gehen weiter hinter den laufenden Titel, Ziehen an die Ablegestelle. Test: `library_title_is_appended_at_the_end`.
- **Von Hand gestartetes Deck stoppt aus dem Sendedeck (#325):** Ursache: „Stopp“ hielt bei stehender Playlist nur die Playlist an. Jetzt halten auch laufende Decks A–C an; „Fade“ blendete sie seit 0.41.0 schon aus. Test: `send_stop_and_fade_reach_a_manually_started_deck`.
- **Ziehen auf Cart-Pads (#323):** Ursache: Dateien aus dem Explorer und Playlist-Zeilen landeten auf einem Cart in der Playlist. Jetzt belegen sie das Pad (wie die Bibliothek). Decks nahmen Dateien schon seit 0.35.6 an. Tests: `explorer_file_dropped_on_a_cart_lands_there`, `dragged_row_lands_on_deck_row_or_nowhere`.
- **Nummernfeld der Playlist lässt sich verlassen (#322):** Ursache: das Feld holte sich in jedem Bild den Fokus zurück. Jetzt nur beim Öffnen; Esc, Tab und Klick daneben beenden ohne Verschieben, Enter verschiebt. Test: `playlist_number_field_can_be_left`.
- **Ducking hebt nach Carts früher an (#324):** Ursache: die Musik blieb unten, bis das Cart samt stiller Ausklingzeit zu Ende war. Jetzt zählt nur der hörbare Teil (bis −50 dBFS). Die Release-Werte (Mikro 80 ms, Voice-Track 0,8 s, einstellbar) bleiben. Test: `cart_duck::silent_cart_tail_does_not_hold_the_music_down`.
- **Lautstärke im Radio-Deck (#147):** Offline gemessen: die Verstärkung bleibt innerhalb eines Titels gleich (`constant_gain`, drei Tests); das Pumpen durch stille Cart-Enden ist mit #324 behoben. Kompressor und Spitzenbegrenzung wirken absichtlich.
- **Keine UI-Hänger beim Geräteausfall (#291):** Ursache: Prüfen des Standardgeräts, Schließen und Neuöffnen der Ausgänge liefen im GUI-Takt und dauern unter Windows nach „output lost“ mehrere Sekunden. Jetzt in einem eigenen Thread je Ausgabe (`outputs::OutputWatch`), auch „Ausgabe neu verbinden“. Test: `watch_tests::streams_live_in_their_own_thread`.

## 0.41.1 — 2026-11-03

- Version 0.41.1 statt 0.42.0: 0.42.0 bleibt der ersten Version für alle vorbehalten (André 03.11.).
- **Einstellungen neu gegliedert (André 03.11.2026):** ein Fenster (Strg+,) mit sieben festen Bereichen: Allgemein (Erscheinungsbild, Speicherorte, Updates, Datenschutz, Entwickler-Modus freischalten) · Audio und Geräte (Soundkarten, Vorhören, Telefon, MIDI-Controller) · Radio (Sendung/Shoutcast, Pult-Modus, Übergänge, Ducking, Processing, Wunschordner) · Party (DJ-Kurve) · Produzent (Klangbänke, Piano-Roll, Plugins) · Dienste und Konten · Entwickler (nur im Entwickler-Modus). Der Bereich „KI“ ist aufgelöst: „Dienste und Konten“ zeigt Karten nach Zweck (Assistent: Claude, ChatGPT, OpenRouter, Ollama, LM Studio, Werkzeuge · Stimmen und Klänge: ElevenLabs, Freesound · Steuerung von außen: Claude Desktop/Code, Hintergrund-Server · Sendung: Querverweis Shoutcast), je Karte Status-Chip, Technik-Marke „API“/„MCP“ und Aktion. Menü „Einstellungen“: oben „Alle Einstellungen (Strg+,)“, darunter die Abkürzungen je Bereich, die an die Stelle im Fenster springen und sie 2 s markieren. Zahnrad in der Kopfleiste öffnet das Fenster beim Bereich der aktuellen Ansicht. Ein Register (`settings::REGISTRY`) speist Fenster, Filter und Menü; Suchwörter mit Umgangssprache („11 labs“, „stimme“, „tts“, „shoutcast“, „mcp“). Das bisherige Fenster „KI“ heißt „Dienste und Konten“. Einsortier-Regel in `CLAUDE.md` und `docs/design/umsetzung.md`. Fotomodus `WOA_SETTINGS_AREA=<Bereich>`. Tests: `settings::tests` (Bereiche, Gruppen, Umgangssprache, jedes `GuiConfig`-Feld registriert, auch bei CRLF-Checkout unter Windows, Menü aus dem Register).

## 0.41.0 — 2026-11-02

- Alte Design-Zip (Fassung 6) entfernt, Fassung 7 liegt als Ordner in docs/design/woa-design-system/.
- Formatierung (cargo fmt) nachgezogen.
- **Fragen und To-dos, Abnahme (QuestionsWindow/AcceptanceWindow Fassung 7):** eigener Fensterkopf 44 px für Fragen, Abnahme, Einstellungen und Piano-Roll (Doppelklick maximiert, Esc schließt); Fragen mit Suchfeld 48 px, Filter Offen/Gesendet/Alle, Leuchtrahmen 1,5 s nach dem Senden und „Erneut senden“ nach Fehlern; Abnahme mit Tun/Soll nebeneinander, Tasten G/N, Zähler mit Balken. Einstellungen-Fenster: Treffer stehen untereinander (lagen nebeneinander), MIDI-Eingang und PFL als Aufklapper. Fotomodus `WOA_SETTINGS=<filter>` und `WOA_SOUNDPICKER`. Abnahmepunkte und Bilder in `docs/design/ist/ist-f7-*`.
- **ON AIR als Lampe (TopBar Fassung 7):** oben rechts `woa-onair--bar` (28 px, 3-px-Rahmen) mit drei Zuständen: aus, an (gleiche Glut wie im Sende-Deck), gedimmt im Pult-Modus (#301).
- **DJ-Kopf und -Mischer (DjHead/DjMixer Fassung 7):** Chips 28 px auf einer Mittellinie, Chip „Analyse“ (läuft, Kreisel) bzw. „Analyse fehlgeschlagen“ mit Knopf „Neu analysieren“; „Weiter · Übergang“ unter dem Crossfader neben weich|hart (36 px).
- **Design System Fassung 7 (Nachtrag 0.40):** `docs/design/woa-design-system/` ersetzt Fassung 6 (neue Bausteine Window, QuestionsWindow, QuestionStates, AcceptanceWindow, DjHead, FadeButton, ScreenSettings, SettingsFilter, AiSettings, JingleVoice, GuestCard), Zielbilder unter `docs/design/soll/fassung-7/`.
- **`woa-chip` als einziger kleiner Zustandsbaustein:** neuer Baustein `widgets::chip` (`StateChip`, `ChipTone` Neutral/Ok/Warn/Cue/Alt/Busy/Live): 26 px, im DJ-Kopf 28 px, Mono 12, Symbol 14 px oder LED, Fläche 14–16 % der Zustandsfarbe. `theme::status_pill` und `theme::badge_pill` zeichnen jetzt diesen Chip, damit sind alle kleinen Zustände (Radio, Party, Fernbedienung, Produzent, Fragen, Abnahme) einheitlich. „ÜBERSTEUERT“ ist `warn` statt Rot (Rot nur für live). Tests: `widgets::chip::tests` (Höhe 26/28 px, nur Live ist rot).
- **Spurkopf (TrackHeader Fassung 7):** Spuren bleiben 64 px. MIDI-Spuren zeigen in Zeile 2 links den Instrument-Knopf (Symbol der Klangkategorie in Spurfarbe, Name, Pfeil; öffnet die Klangauswahl im Rack), rechts den MIDI-Eingang als 104 px breites Feld (22 px hoch, Design 24, damit der Kopf eine freie Klickkante behält): Symbol leuchtet `cue`, solange Noten auf der spielenden Spur ankommen, Rahmen gestrichelt ohne Gerät; Klick öffnet die Gerätewahl. Neuer Baustein `widgets::track_head`. Test: `widgets::track_head::tests::midi_input_is_104_wide_and_both_fit_the_row`.
- **Klangauswahl (SoundPicker Fassung 7):** Suchfeld über alle Kategorien (Name oder Kategorie, ohne Groß/Klein), je Klang eine Vorhören-Taste (kurzer Probeton, der Spurklang bleibt), „Kein Klang gefunden“ bei leerem Treffer. Kategorien statt Rollen, Bank-Herkunft und „Synth (einfach)“ unten waren schon da. Test: `sound_search_tests`.
- **Einstellungen neu (ScreenSettings + SettingsFilter Fassung 7):** Fenster „Einstellungen“ (Menü Einstellungen › Alle Einstellungen, Strg+,) mit Bereichen links (Audio, Sendung, MIDI, KI, Speicherorte, Datenschutz, Entwickler) und Filter oben, der jede Einstellung über Name, Beschreibung und Suchwörter findet (Groß/Klein und Umlaute egal, „kopfhoerer“ findet „Kopfhörer“). Enter springt zum ersten Treffer und markiert ihn 2 s in `cue`. Jede Zeile wirkt wie der Menüeintrag. Eine Liste `settings::REGISTRY` speist Filter und Anzeige; ein Test sorgt dafür, dass jeder Menüeintrag unter Einstellungen darin steht. Tests: `settings::tests`.
- **KI (AiSettings + JingleVoice Fassung 7):** ElevenLabs-Zugang mit Kopf (Symbol, Zustandschip nicht eingerichtet · prüfe · gespeichert · verbunden `ok` · Schlüssel ungültig `warn`), Credits als LCD mit Balken, „von N · Abo“ und Erneuerung, „Entfernen“ als Symbol mit Rückfrage, Fehlermeldung in `warn` statt Rot, Datenschutz-Hinweis im `cue`-Rahmen. Jingle-Assistent: „Wer spricht? Frei (lokal) | ElevenLabs“ als Segmentschalter, neuer Regler Ähnlichkeit (geht als `similarity_boost` an ElevenLabs, bisher fest 0,75), Datenschutz fest sichtbar, Kostenkasten mit „Erzeugen“ in `ok` und Abbrechen, Fehler mit Grund und „Erneut“ (zeigt den Kostenkasten wieder, sendet nie von selbst). Tests: `elevenlabs_ui::tests` (Zustand, Credits-Balken, Erneut), `woa-agent` `language_code_only_for_v3` (Ähnlichkeit begrenzt). Kein echter Aufruf an ElevenLabs.
- **Fade-Taste fest 3 s (FadeButton, #320, #321, #274):** Sende-Deck „Fade“ nach „Stopp“ blendet alles Laufende aus (Playlist, Decks, Carts) und stoppt; je Deck A–C eine quadratische Fade-Taste neben Start/Stopp (MIDI-lernbar als „Deck X Fade“); Cart-Wall mit Fade-Symbol oben rechts am laufenden Cart und „Ausblenden (3 s)“ im Rechtsklick; in der laufenden Playlist-Zeile ein Fade-Symbol. „Blendet aus“ zeigt Rahmen und wachsende Füllung in `cue` plus Restzeit („1,1 s“); ein zweiter Druck stoppt sofort; läuft nichts, ist die Taste gesperrt. Die Einstellung „Stopp blendet aus“ (#227) bleibt unverändert. Neue Mixer-Befehle `FadeMusic`, `DeckFade`, `CartFade`, `FadeAll`, Controller `fade_music`, `fade_deck`, `fade_cart`, `fade_all`, `anything_running`, Konstante `woa_radio::FADE_SECONDS`. Fotomodus `WOA_FADE=1` (nur Anzeige, kein Befehl). Tests: `crates/radio/tests/fade.rs` (sechs, unter `assert_no_alloc`), `widgets::fade::tests` (drei), `controls::tests::decks_a_to_c_are_in_the_learn_list`.

## 0.40.2 — 2026-11-02

- **ON AIR oben rechts leuchtet (#353):** Auf Sendung zeigt die Kopfleiste ON AIR jetzt wie die Lampe im Sendepult: Fläche `on-air-fill`, weiße Schrift und LED, Schein `glow-on-air` außen (neuer `ChipLook::Live`, Farben aus `lamp_colors`, kein Hartcode). Nicht auf Sendung bleibt der ruhige Eintrag. Design-System: Ausnahme „ON AIR an“ von „In der Kopfleiste kein Glow“ in README und TopBar-README eingetragen. Fotomodus `WOA_ONAIR=1`. Test (vorher rot): `widgets::topbar::tests::live_on_air_chip_glows_like_the_lamp`.
- **Party DJ, keine BPM/Tonart nach dem Laden (#347):** Ursache: Das Deck las Tempo und Tonart nur aus der Bibliotheksanalyse. Titel, die per „Ordner laden“ in die Playlist kamen (nicht in der Bibliothek), oder noch nicht analysierte Titel bekamen nie Werte. Jetzt misst das Deck beim Laden selbst im Hintergrund (`waveform::deck_info` ruft `analysis::analyze_file`), legt das Ergebnis in der Bibliothek ab, wenn sie den Titel kennt, und gibt es an Deck und Playlist. Deck-Kopf: Restzeit- und BPM-LCD mit gleicher Ziffernhöhe, Camelot-Chip in derselben Höhe (Farben aus `theme::pill_colors`); in der einfachen Party Restzeit und BPM ebenso gleich groß. Fotomodus `WOA_DJ_LOAD=1` (Titel 1 und 2 auf Deck A und B). Tests (vorher rot): `waveform::tests::deck_info_measures_unanalysed_titles`; neu `party::dj::tests::deck_head_readouts_share_one_height`.

## 0.40.1 — 2026-11-02

- **Start maximiert statt Vollbild (Abnahme #343/#344).** André zu 0.39.0: „die app soll maximiert starten. nicht f11 vollbild.“ WOA öffnet jetzt maximiert im Fenster mit Rahmen und Knöpfen; der Vollbild-Befehl beim Start entfällt, F11 schaltet Vollbild weiter an und aus. Eine gespeicherte Einstellung „Start im Vollbild“ gab es nicht (nur `WOA_WINDOW` für feste Größen), daher nichts umzustellen. `StartWindow::Fullscreen` heißt `StartWindow::Maximized`.
- **Abnahme: Anmerkung jederzeit nachschicken.** André konnte nach „Geht“ keine Anmerkung mehr senden und musste dafür „Geht nicht“ klicken. Das Feld bleibt nach dem Urteil offen, daneben „Anmerkung senden“; gesendet als `[entwickler-abnahme] <id>: anmerkung` (Art allgemein, gleiche Quelle und Kennwort-Kopfzeile), das Urteil bleibt unverändert. Geht auch ohne Urteil. Im Fenster „Fragen und To-dos“ erscheint nach dem Senden „Anmerkung nachtragen“ (`[entwickler-fragen] <id>: anmerkung`). Prüfpunkt `vollbild-start` in `docs/abnahme.json` umgeschrieben (0.40.1).
- **Abgenommenes verschwindet.** Punkte, die auf diesem Gerät mit „Geht“ gesendet wurden, merkt sich WOA je ID und Version (`abnahme-erledigt.json` im Datenordner) und blendet sie aus; Schalter „Erledigte zeigen“. Bekommt ein Punkt eine neue Version, erscheint er wieder. Aus `docs/abnahme.json` entfernt (ok gemeldet): `radio-rot-nur-live` (#342), `fix-hot-cue-loeschen` (#345), `fix-weiter-uebergang` (#346); `vollbild-start` (#343) durch die maximiert-Fassung ersetzt. Regel in `CLAUDE.md`.
- **Feedback-Fenster:** Schließen-X oben rechts (rechts neben dem Titel, Fensterbreite unverändert); der Dank nach dem Senden steht 1,5 s statt 3 s.
- **Fehler behoben: Tests sendeten echte Meldungen.** Die neuen Fenster-Tests dieser Version liefen mit der Standard-Adresse und schickten vier Testmeldungen an den echten Server (#348, #349, #351, #352, leere Install-ID). Neu `feedback::may_send`: in Testläufen nur an 127.0.0.1/localhost; Feedback-, Fragen- und Abnahme-Versand prüfen das.
- Tests (vorher rot): `feedback::tests::{test_builds_never_reach_the_real_server, thanks_visible_one_and_a_half_seconds}`, `app::…::feedback_window_has_close_x`, `abnahme::tests::approved_points_hide_until_a_new_version`, `tests::app_starts_maximized_unless_woa_window_is_set`, `abnahme::tests::{note_payload_is_marked_anmerkung, note_can_be_sent_after_thumbs_up}`, `fragen::tests::note_can_be_added_after_sending`.

## 0.40.0 — 2026-11-02

- **ElevenLabs-Anbindung (Sprache, Soundeffekte, Musikbetten mit eigenem Schlüssel).** Wie beim Freesound-Zugang trägt man unter Einstellungen → KI einen eigenen ElevenLabs-API-Schlüssel ein; Speichern, Prüfen (Abo und verbleibende Credits aus `GET /v1/user/subscription`) und Entfernen. Der Schlüssel liegt nur im Schlüsselbund (`woa_secrets`, Eintrag `elevenlabs`), nie in `config.json`, Projekt, Log, Feedback oder MCP-Antworten. Ohne Schlüssel ändert sich nichts. Neues Modul `woa_agent::elevenlabs` mit blockierendem Client (`ureq`, nur https, nur im Hintergrund-Thread bzw. `spawn_blocking`): Text-to-Speech (`eleven_multilingual_v2` oder `eleven_v3`, Stimme aus der Kontoliste `GET /v2/voices`, Stabilität, Stil, `language_code` „de“ nur bei v3, wie die Doku es verlangt), Soundeffekte (`/v1/sound-generation`, 0,5–30 s) und Musik (`/v1/music`, instrumental, 3–600 s), Ausgabe immer `mp3_44100_128`. Fehler 401, 402/Kontingent, 429, 422 und Netz als klare deutsche Meldung. Schnittstelle in zwei Traits: `Sprecher` (Text → Sprache; später auch Piper lokal, `docs/klang-plan.md` B1) und `Klangerzeuger` (Effekt, Bett). Jingle-Assistent: im Schritt Stimme Sprecher „Selbst einsprechen“ oder „ElevenLabs“ (Text, Stimme, Modell, Regler); im Schritt Schluss „Effekt erzeugen …“ und „Bett erzeugen …“ (neue Spur, Bett so lang wie der Jingle). Vor jeder Erzeugung ein Kasten mit geschätzten Kosten (Sprache: Zeichen = Credits; Effekt/Musik: API-Preis in $) und Datenschutz-Hinweis, Abbrechen möglich. Ergebnisse als MP3 im Projekt (`aufnahmen/`), Kopie im Jingle-Ordner, Eintrag in der Bibliothek (Schlagworte `elevenlabs` und Art), Herkunft und Text in `<datei>.herkunft.json`. MCP-Werkzeuge `tts_generate`, `sfx_generate`, `music_generate` (`docs/agent.md`). Fotomodi `WOA_KI=1` (Einstellungen › KI) und `WOA_JINGLE=elevenlabs` (Schritt Stimme mit Beispielstimmen, kein Netz). Tests (ohne Netz, ohne Schlüsselbund, gegen lokalen Mock-Server): `crates/agent/tests/elevenlabs.rs` (sieben), `woa_agent::elevenlabs::tests` (sechs), `elevenlabs_ui::tests` (zwei), `widgets::wizard::tests::{elevenlabs_speaker_asks_before_spending_and_can_be_cancelled, without_key_the_voice_step_stays_as_before}`.

## 0.39.0 — 2026-11-02

- **Entwickler-Modus: „Fragen und To-dos“ schöner, Suche, neues Fenster „Abnahme“ (Andrés Feedback 02.11.):**
  - **Optik und Maximieren (#337):** Eigene Kopfzeile mit Symbol, Titel und Stand, rechts neben dem Schließen-Kreuz ein Maximieren-Knopf (füllt die App, zweiter Klick stellt die alte Größe wieder her). Jede Frage ist eine Karte (Design `card`), die Ticketnummer eine Pille mit Link-Symbol, der Status ein neutrales Badge; jeder Vorschlag liegt in einer Mulde mit Nummer, Dafür/Dagegen/anders als Segmente mit Symbol, „Senden“ in Akzent. Gesendete Vorschläge sind grün umrandet mit Haken, Fehler rot mit Warnsymbol.
  - **Kommentar wählt „anders“ (#338):** Sobald im Anmerkungsfeld Text steht, ist „anders“ gewählt.
  - **Suche:** Suchfeld ganz oben; filtert Titel, Erklärung, Vorschläge, Block, Status und Ticketnummer (jedes Wort muss vorkommen, Groß/klein egal). Blöcke ohne Treffer verschwinden, ohne Treffer steht „Nichts gefunden“.
  - **Neues Fenster „Abnahme“** im Menü Entwickler: Prüfpunkte aus `docs/abnahme.json` (eingebettet, beim Öffnen von GitHub nachgeladen wie die Fragen), gruppiert nach Ansicht (Radio, Party Einfach, Party DJ, Produzent, Piano-Roll, Jingle-Assistent, Einstellungen, Allgemein), je Punkt „Tun“ und „Soll“, Version und Ticket. „Geht“ (Daumen hoch) sendet sofort; „Geht nicht“ (Daumen runter) sendet sofort mit Text, ohne Text fragt es erst „Was ist passiert?“ und sendet beim zweiten Klick oder mit Enter. Ein Kommentar wählt hier nichts. Gesendet wird über den Feedback-Weg mit `[entwickler-abnahme] <id>: ok|fehler` im Titel, Quelle `entwickler-abnahme`, Art `lob` bzw. `bug`, Kennwort-Kopfzeile wie bei den Fragen. Kopfzeile zählt „x von n geprüft“. 40 Prüfpunkte aus den offenen Klicklisten 0.35.19 bis 0.39.0 (Klang-Paket #335, Fixes #336). Fotomodus `WOA_ABNAHME=1`.
  - Tests (vorher rot): `fragen::tests::{typing_a_comment_selects_anders, typing_in_the_window_turns_anders_on, window_has_maximize_next_to_close, search_matches_title_text_proposals_and_ticket, search_hides_everything_that_does_not_match}`, `abnahme::tests` (sieben: Datei gültig, nichts Internes, Ablehnen kaputter Fassungen, Kennzeichnung der Nutzlast, Daumen-runter-Logik, Suche, Fenster mit Suche/Maximieren ohne Abstimmen beim Tippen).

## 0.38.0 — 2026-11-02

- **Klangqualität: neue MIDI-Spuren spielen den Flügel, die Klangwahl ist sichtbar.** Eine neue MIDI-Spur (Menü, Keyboard-Aufnahme, leerer Clip für die Piano-Roll, `.mid`-Import auf neue Spur, MCP `add_track` mit `midi`) spielt jetzt den Flügel aus GeneralUser GS statt des eingebauten Synths; Piano-Roll-Vorhören und Keyboard spielen den Klang der Spur und klingen damit ebenso. Der eingebaute Synth heißt „Synth (einfach)“ und bleibt wählbar. Bestehende Projekte ändern ihren Klang nicht: Spuren ohne gespeicherten Klang behalten den Synth (neuer Command-Helfer `Command::add_midi_track`, nichts in `Track::new` oder der Projektdatei geändert). Der Spurkopf einer MIDI-Spur zeigt links das Instrument (z. B. „Flügel“), rechts den MIDI-Eingang; ein Klick auf das Instrument oder auf den Klangnamen im Clip-Kopf wählt die Spur und klappt im Rack die Klangauswahl auf. Neue leere Clips heißen nach dem Instrument statt „MIDI-Clip“. Die Klangauswahl ordnet nach Kategorien Klavier, Bass, Gitarre, Streicher, Bläser, Synth, Drums (31 Klänge, neu u. a. Gitarren, Trompete, Posaune, Saxophon, Flöte); die Rollen Lead/Bass/Pad/Kit steuern weiter Akkord- und Jingle-Bau. Das Preset „Klavier“ heißt „Flügel“, der alte Name wird im MCP weiter verstanden. Plan für SFZ, freie CLAP-Instrumente, Effektpaket und Sprachausgabe: `docs/klang-plan.md`. Tests (vorher rot): `crates/model/tests/default_sound.rs` (neue Spur = Flügel in einem Undo-Schritt, altes Projekt behält Synth, jede Kategorie belegt, „Klavier“ als Alias), `state::tests::new_midi_tracks_and_clips_play_the_grand_piano`; angepasst `timeline::tests::sound_caption_is_upper_case`, `crates/model/tests/sound.rs`, `crates/agent/tests/{track_bank,track_sound,j9_tools}.rs`, `crates/gui/tests/komponieren.rs`. MCP `list_instruments`/`track_sound` nennen je Preset jetzt auch `category`.

## 0.37.1 — 2026-11-02

- **Fixes aus Andrés Feedback am Windows-Gerät (0.36.2, Vollbild 1920 × 1080):**
  - **Produzent, Spuren halb so hoch (#334):** Mit offener Piano-Roll wurden die Spuren auf 36 px verkleinert (Design `ScreenPianoRoll`), der Spurkopf hat aber zwei Zeilen; die zweite (Eingang, Trim, Pan) war abgeschnitten. Die Spuren bleiben jetzt immer 64 px hoch. Test (vorher rot): `crates/gui/tests/spurkopf.rs::tracks_keep_their_height_with_the_piano_roll_open`.
  - **Party DJ, Mixer nicht mittig, rechts ein Rand (#332):** Deck A, Mixer und Deck B zogen ihren Rahmen doppelt von der Breite ab (einmal bei der Größe, einmal im Rahmen selbst); bei 1920 px fehlten so rechts gut 70 px und der Mixer stand links der Mitte. Jetzt bekommt jeder Teil seine äußere Größe, der Rahmen liegt innerhalb; die drei Teile füllen die Breite, der Mixer steht genau mittig. Test (vorher rot): `party::dj::tests::decks_and_mixer_fill_the_width_with_the_mixer_centered` (1280 bis 2560 px).
  - **Party DJ, Hot Cues löschen und „Weiter“ (#331):** Rechtsklick auf eine belegte Hot-Cue-Taste 1–4 löscht sie (Umschalt+Klick wie bisher). Neue Taste **„Weiter · Übergang“** unter dem Crossfader im DJ-Mixer: das andere Deck startet, der Crossfader fährt in der Crossfade-Länge der Sendung hinüber, danach hält das alte Deck an und bekommt den nächsten Titel; ist das andere Deck leer, kommt der nächste Titel erst dorthin; steht alles, startet das hörbare Deck. Ein Griff an den Crossfader bricht die Fahrt ab. Taste N und → tun in der DJ-Ansicht dasselbe. Neues Kommando `DjCommand::Transition`, für den Agenten `dj_mixer` mit `control: "transition"`. Tests: `crates/radio/tests/party_dj.rs::transition_fades_to_the_other_deck_and_reloads_the_old_one`, `party::dj::tests::right_click_clears_a_set_hot_cue`, erweitert `woa_agent::dj::tests::mixer_controls_map_to_commands`.
  - **Log-Warnung „Failed to find replacement characters '◻' or '?'“:** Die Schriftfamilie `icon-fill` bestand nur aus Phosphor Fill, das weder „◻“ noch „?“ hat; egui warnte beim ersten Gebrauch (Wechsel in Radio). Barlow steht jetzt als Rückfall dahinter. Test (vorher rot): `theme::tests::every_family_has_a_replacement_glyph`.

## 0.37.0 — 2026-11-02

- **Menüleiste aufgeräumt und Entwickler-Modus mit „Fragen und To-dos“:** Alle „…“ in den Menüeinträgen sind weg. Innerhalb jeder Gruppe (zwischen zwei Trennern) stehen die Einträge alphabetisch (Groß/klein egal, Umlaute wie der Grundbuchstabe); die Gruppen selbst bleiben (Audio, MIDI, …). Neu unter Einstellungen „Entwickler-Modus“: Mit einem Kennwort schaltet man das Menü „Entwickler“ direkt vor „Hilfe“ frei, mit „Entwickler-Modus beenden“, „Fragen und To-dos“ und „Nach Updates suchen“. Das Kennwort liegt im Schlüsselbund des Systems (`woa_secrets`, Eintrag `entwickler-kennwort`), `config.json` hält nur `entwickler: true`; beim Senden geht es als Kopfzeile `X-WOA-Entwickler` an den eigenen Server, nie im JSON. Das Fenster „Fragen und To-dos“ zeigt die inhaltlichen Entscheidungsfragen aus `docs/fragen.json`, nach Oberfläche gruppiert (Radio, Gäste/Telefon, Party/DJ, Produzent, Jingles, Allgemein), je Frage Erklärung, Status, Ticket-Link und Vorschläge. Je Vorschlag Daumen hoch, Daumen runter oder „anders“, ein Textfeld und „Senden“; die Wahl geht über den Feedback-Weg als Art „frage“, gekennzeichnet mit `[entwickler-fragen] <Fragen-ID>: <Wahl>` im Titel, `Quelle: entwickler-fragen` im Text und dem Feld `source`. Ein Vorschlag lässt sich nach erfolgreichem Senden nicht noch einmal schicken. Die Fragen sind eingebettet und werden beim Öffnen im Hintergrund von GitHub (`main`) nachgeladen; ohne Netz oder bei kaputter Datei bleibt die eingebettete Fassung. Fotomodus: `WOA_FRAGEN=1` öffnet das Fenster, `WOA_MENU=<Menü>` klappt ein Menü der Menüleiste auf. Tests (vorher rot): `app::nacharbeit_tests::{menu_groups_are_sorted_and_without_dots, menu_code_follows_the_sorted_groups, developer_menu_only_when_unlocked_and_before_help}`, `fragen::tests` (acht: Datei gültig und vollständig, nichts Internes, Rückfall bei Netzfehler und kaputter Datei, Nutzlast mit Quelle, Einmal-Senden), `telemetry`: `source_goes_as_field_and_dev_key_never_in_json_or_debug`, `mock_server::developer_feedback_carries_source_and_key_header`.

## 0.36.2 — 2026-11-02

- **Fix Vollbild-Start unter Windows:** 0.36.1 startete in etwa 2/3 Bildschirmgröße, ohne Fensterknöpfe, und ein Doppelklick auf die Titelleiste ging nicht. Ursache: `with_fullscreen` schon beim Anlegen des Fensters. Jetzt legt WOA das Fenster maximiert mit Rahmen an und schaltet danach per Befehl ins Vollbild; F11 führt zurück ins maximierte Fenster mit Knöpfen.

## 0.36.1 — 2026-11-02

- **Design-Runde 3b, Korrekturen nach Andrés Prüfung:** Party-Deck-Badges sind nie mehr gefüllt (Lime heißt nur „eingeschaltet“): BEREIT neutral (`well`, Rahmen `border`), VORGELADEN mit Lime-Rahmen und -Schrift, JETZT und LÄUFT MIT mit `accent`-Rahmen, PAUSE mit `warn`-Rahmen (neuer Baustein `theme::Badge`/`badge_pill`). Produzent: Das Insert-Rack zeigt „Ausgang / Pan“ bei 1920 × 1030 ganz; wird es zu lang, rollt nur das Rack. Transport-Pegel ist immer die horizontale LED-Kette mit dB-Zahl (Vorlage ScreenStudio „Pegel“); das analoge VU steht nur noch in der neuen Pegelansicht oben im Analyse-Fenster, umschaltbar Analog/Modern/Aus (gemerkt in `vu_view`). Die Piano-Roll öffnet im Dock; „Als Fenster lösen“ wird gemerkt (`piano_roll_window` in `config.json`, Vorgabe Dock). Akkord-Vorhören spielt über eine flüchtige Vorhör-Stimme mit dem Klang der Ausgabe (Akkorde = Streicher, Bass = E-Bass, Arpeggio = Lead), unabhängig von der gewählten Spur; die Zeilen Akkorde/Bass/Arpeggio klingen beim Klick. Die Stimme ist eine unsichtbare MIDI-Spur nur im Graphen des Players, nie im Projekt. MIDI-Learn kennt Start/Stopp, Cue und Vorhören je Deck A–C (`Target::Deck(n, DeckAction::Start|Cue|Pfl)`, erweiterbar für weitere Deck-Ziele, alte Zuordnungsdateien laden weiter), mit Rechtsklick „MIDI lernen“ an jeder Deck-Taste und einer Zeile „Decks:“ in der Controller-Liste. Neues MCP-Werkzeug `output_levels` (nur lesend, Live-Modus): Spitze je Radio-Ausgang Notebook, Stream, Vorhören in dBFS; dafür misst der PFL-Spieler seine Spitze lock-frei (`PflHandle::take_peak`, `View::pfl_peak`). Tests (vorher rot): `party::tests::deck_badges_are_never_filled`, `app::nacharbeit_tests::{rack_output_and_pan_fit_the_dock, transport_level_is_a_led_chain_and_vu_lives_in_analysis, piano_roll_opens_docked_and_window_choice_is_remembered}`, `feedback::tests::piano_roll_opens_in_dock_and_remembers_the_window_choice`, `akkorde::tests::chord_preview_uses_the_sound_of_each_output` (ersetzt `chord_row_preview_sends_note_on_to_a_midi_track`), `probeton::tests::voice_tap_plays_its_sound_then_goes_away`, `controller::tests::deck_targets_learn_and_old_maps_still_load`, `controls::tests::decks_a_to_c_are_in_the_learn_list`, `radio::tests::controller_starts_and_stops_a_deck`, `crates/agent/tests/live.rs::output_levels_are_read_only_and_listed`, erweitert `pfl::tests::plays_from_start_and_stops`.

## 0.36.0 — 2026-11-02

- **DJ-Paket (Design-System Fassung 5):** Neue Bausteine DjWaves, DjDeck, DjMixer, RemoteCard und der Entwurf ScreenPartyDJ samt Soll-Bildern `docs/design/soll/party-dj*.png`. Engine-Bausteine in `woa_dsp::dj`: Deck-Leser mit Tempo (Varispeed, auch rückwärts zum Scratchen) und Keylock (laufendes WSOLA im Audio-Thread, feste Puffer), Schleife; Dreiband-EQ als Linkwitz-Riley-Teilung mit echtem Kill, in Mittelstellung bitgenau umgangen; DJ-Filter (Tiefpass/Hochpass an einem Regler); Crossfader-Kurve weich/hart; Kopfhörer-Mix. Analyse: Beatgrid-Lage (`detect_beat_offset`) und Camelot-Schlüssel (`Key::camelot`, `camelot_of_name`, `camelot_compatible`). Golden Files `tests/golden/dj_*.wav`. Bibliothek **Schema 6**: Spalte `beat_offset` je Titel und Tabelle `dj_marks` (Hot Cues 1–4 und Schleife je Inhalts-Hash, Sekunden); Titel mit Tempo kommen einmal neu in die Analyse, damit sie ein Beatgrid bekommen. **Headless und GUI müssen gemeinsam ausgeliefert werden**, eine ältere Version öffnet die Bibliothek mit Schema 6 nicht. Engine: in der DJ-Ansicht liest jede Party-Seite über den Leser (Tempo ±8 % mit Anschieben, Keylock, Scratch, Schleife), dann Trim, EQ mit Kill, Filter, Kanalfader, Crossfader-Kurve weich/hart und Master; der Kopfhörer-Bus (Cue vor dem Fader, Mix Cue↔Master, Pegel) geht über eine eigene Taktverbindung nur in den Vorhör-Ausgang, nie auf Sendung, Mitschnitt oder Stream. Steuerung als Kommandos (`DjCommand`: Laden in A/B, Play, Cue setzen/halten, Sync mit Doppel-/Halbtempo, Hot Cues 1–4, Auto-Loop 1–16 Schläge, Tempo, Keylock, Mixer); in der DJ-Ansicht sind Auto-Übergang und Vorladen aus, die Einstellung der Sendung bleibt. MCP: `dj_state`, `dj_deck`, `dj_mixer` steuern die Decks der laufenden App (docs/agent.md). Oberfläche: Party oben umschaltbar „Einfach | DJ“; die DJ-Ansicht zeigt parallele Wellen mit Phase (Mausrad zoomt), Deck A · Mixer · Deck B (B gespiegelt) mit Camelot-Chip (grün, wenn passend), BPM- und Restzeit-LCD, Übersicht mit Hot Cues und Loop (Klick springt), Jogwheel (Rand anschieben, Platte scratchen), Hot Cues 1–4 (Umschalt löscht), Auto-Loop 1/2/4/8/16, Cue (halten/loslassen), Play/Pause, Sync, Keylock, Tempo ±8 % mit Anschieben; Mixer mit Trim, EQ mit Kill, Filter, Kopfhörer-Cue (ohne Vorhör-Ausgang gesperrt, Tooltip sagt wo), Kanalfader mit LED-Kette, Master, Kopfhörer Mix/Pegel, Crossfader mit Kurve weich/hart; Playlist mit BPM, Tonart und „A/B laden“. Tasten: Z/X/V Cue/Play/Sync A, Komma/Punkt/Schrägstrich für B, 1–4 und 7–0 Hot Cues (gegen RESERVED_KEYS geprüft). Hot Cues und Schleifen landen je Titel in der Bibliothek. RemoteCard: unter 900 px Fensterhöhe eine Zeile mit QR 64 px, Adresse · PIN, Handyzahl, „QR groß“ und „An“; Klick auf den QR, das Symbol oder Q zeigt ihn 300 px groß, Klick oder Esc schließt. Im Fotomodus stehen Platzhalter statt LAN-Adresse und PIN. In der DJ-Ansicht ist die Fernbedienung immer kompakt; Decks und Mixer ziehen ihren Rahmen von der Breite ab, damit Deck B auch bei 1366 × 768 ganz im Fenster bleibt. Fotomodus: `WOA_PARTY_VIEW=dj` öffnet die DJ-Ansicht, `WOA_PHOTO_FRAMES=n` wartet länger (Debug-Bau dekodiert langsam).

## 0.35.24 — 2026-11-02

- **Design-Runde 3, Abnahme:** Prüfliste `docs/design/umsetzung.md` (Baustein · Zustand · Ort · Stand) für alle Punkte aus `auftrag-runde-3.md` und `auftrag-nachbesserung.md`, mit Andrés Entscheidungen vom 02.11., vier offenen Fragen und den Abweichungen vom Entwurf. Neue Screenshots aller Modi dunkel und hell bei 1920 × 1030 (Radio und Produzent auch 1366 × 768) unter `docs/design/ist/`. Neuer Fotomodus `WOA_JINGLE=1` öffnet den Jingle-Assistenten. Klickliste „Design-Runde 3 gesamt“ in `docs/abnahme.md`.

## 0.35.23 — 2026-11-02

- **Design-Runde 3, Transport und Dock-Reiter:** Der ganze Transport des Produzenten kommt ohne `egui::Button` aus. Zurückspulen und Play sind Hardware-Tasten (`Key`) mit gefülltem Symbol, Play rastet beim Abspielen grün ein. Aufnahme ist eine `Key` mit Symbol und Text, die beim Aufnehmen rot (`on-air`) einrastet. Loop, Punch und „Stopp am Ende“ sind `Key` mit Symbol und Text, eingeschaltet in `accent-soft` mit Rand `accent` wie bisher. `Key` hat dafür zwei neue Varianten: `.icon(glyph)` für Symbol plus Text (oder Symbol allein) und `.soft(an)` für Schalter. Neuer Baustein `widgets::Segment` (Symbol plus Text, gewählt `accent-soft` mit Rand `accent`, sonst `well`) ersetzt die Hilfe `segment()` für die Dock-Reiter Studio-Mixer, Piano-Roll, Automation, KI-Chat und Analyse. Radio behält seine Segmente nach Design (`segments`, Mulde mit erhabenem gewähltem Knopf); die Party hat keine solchen Reiter. Tests: `app::nacharbeit_tests::whole_transport_has_no_egui_button`, `app::nacharbeit_tests::producer_segments_use_the_widget` (beide vorher rot), `widgets::segment::tests` (zwei).

## 0.35.22 — 2026-11-02

- **Design-Runde 3, Zusatz: Deck-Tasten, Ausgangspegel, Trim beim Aufnehmen:** Im Radio starten und stoppen die Tasten A, B, C das Deck A, B, C (wie die Start-Taste am Deck; nicht, solange ein Textfeld den Fokus hat oder eine Cart-Taste gelernt wird). Hinweis „Tasten A–C starten“ im Kopf der Decks, der Tooltip der Start-Taste nennt die Taste. A–C sind wie M und Leertaste reserviert und taugen nicht mehr als Cart-Taste; war ein Cart schon auf A, B oder C gelegt, bleibt der Eintrag in `radio.json`, die Taste wirkt aber nicht mehr, das Cart zeigt „–“ und ein einmaliger Hinweis nennt es zum Neubelegen. Decks sind weiterhin nicht per MIDI lernbar. Die Engine misst jetzt die Spitze je Ausgang hinter Regler und Stumm (`Status::take_output_peaks`: Notebook, Stream), lock-frei als Atomics wie die bestehenden Meter, ohne Allokation; Stream nur, wenn ein Encoder angeschlossen ist (sonst bleibt die LED-Kette dunkel, damit sie nicht „auf Sendung“ vortäuscht). In der Pegelkarte stehen neben den Fadern Notebook und Stream LED-Ketten wie bei den Quellen. Produzent: Aufnahme-Pegel (Spurkopf und Leiste) und Mithören laufen durch den Trim der Spur, auch wenn er sich während der Aufnahme ändert. **Die Datei wird roh geschrieben, der Trim wirkt beim Abspielen** (vor den Inserts, seit 0.35.18); so stimmen Mithören und Ergebnis überein, und der Trim zählt nicht doppelt. Die Übersteuert-Warnung bleibt am rohen Eingang. Tests: `crates/radio/tests/output_meters.rs` (Offline, `assert_no_alloc`), `input_trim::tests` (drei), in `radio.rs` `letter_keys_start_and_stop_their_deck`, `letter_key_in_a_text_field_starts_no_deck`, `deck_letters_are_no_cart_keys`, `old_cart_keys_on_deck_letters_are_reported`; erweitert `output_faders_have_level_and_mute`.

## 0.35.21 — 2026-11-02

- **Design-Runde 3, Nacharbeit Produzent:** WOA startet grundsätzlich im Vollbild; nur mit `WOA_WINDOW=BxH` (Screenshots) in fester Größe, ein unlesbarer Wert gibt 900×600. F11 schaltet in Produzent und Radio das Vollbild um (Party wie bisher F und F11), die Vollbild-Taste der Party zeigt den echten Zustand des Fensters. F11 ist für Cart-Tasten gesperrt wie M und Leertaste; eine früher auf F11 gelegte Cart-Taste gilt damit als frei. Neuer Baustein `widgets::IconButton` (24/28/32 px, Ruhe ohne Fläche, Hover mit Fläche, „an“ in `accent-soft`, Gefahr erst bei Hover rot). Clip-Inspektor: Fades auch bei Mehrfachauswahl in ms (gespeichert weiter in Schlägen), Werkzeuge Teilen, Duplizieren, Kopieren, Piano-Roll, Normalisieren, Löschen und „Tempo folgen“ als `IconButton` mit Tooltip; „Q“ (Quantisieren) als Text-Taste `Key`, weil der Buchstabe kein eindeutiges Symbol ist. Spurkopf-Löschen und Zoom −/+ im Transport nutzen denselben Baustein. Studio-Dock: „Lösen“ und „Einklappen“ nur noch als Symbol mit Tooltip. Transport: EXPORT, Latenz-Schalter und Takte/Zeit als Hardware-Tasten (`Key`) statt egui-Knöpfe. Piano-Roll, Drum-Raster und Jingle-Assistent sind eigene Bausteine unter `crates/gui/src/widgets/` (`piano_roll`, `drum_grid`, `wizard`); die alten Pfade `woa_gui::pianoroll` und `woa_gui::jingle_assistent` bleiben als Verweis, das Verhalten ist unverändert (Tests unverändert grün, `drum_clicks_follow_the_design` liegt jetzt in `widgets::drum_grid`). Browser › Akkorde: die Akkord-Chips liegen nicht mehr über dem Hinweis der nächsten Folge (der Vorhören-Knopf schob den Zeilen-Cursor zurück, `Ui::put` → `Ui::place`), Akkordname und Stufe in `lcd-text` auf der LCD-Fläche, also auch im hellen Thema lesbar. Fotomodus `WOA_SELECT=n` wählt die ersten n Clips für Screenshots des Clip-Inspektors. Tests: `akkorde::tests::chord_chips_are_readable_in_every_theme`, `akkorde::tests::chord_chips_sit_below_their_hint_without_overlap`, `app::nacharbeit_tests` (sieben: Fades in ms, Werkzeuge, Dock, Transport, F11 in Produzent und Radio, Party folgt dem Fenster, F11 gesperrt), `widgets::icon_button::tests` (vier), `app_starts_fullscreen_unless_woa_window_is_set` in `apps/woa` (alle vorher rot bzw. ohne Baustein nicht übersetzbar).

## 0.35.20 — 2026-11-02

- **Design-Runde 3: Piano-Roll, Drum-Raster, Jingle-Assistent und Probeton (PianoRoll, ScreenPianoRoll, DrumGrid, JingleWizard, SoundPicker; Refs #246, #261):** Piano-Roll neu: Leiste in einer Zeile mit Clip-Kennung (Spurfarbe, Name, Spur, Takte, Notenzahl, Auswahl), Werkzeugen Zeichnen · Auswählen · Löschen (D/V/E), Raster mit Triolen „3“, Länge neuer Noten, Quantisieren (Q), Velocity-Feld, Tonart mit Magnet (rastet auf Töne der Tonart ein), „Nur benutzte Töne“, Probeton beim Zeichnen und Zoom −/+. Die Anleitungszeile und die Überschrift „Drum-Raster“ entfallen, die Hilfe steht in den Tooltips. Echte Klaviatur (schwarze Tasten kurz, C je Oktave beschriftet, gespielte und gewählte Töne in Spurfarbe), Tonart-Zeilen 8 % Spurfarbe, Clip-Bereich als Balken im Lineal, außerhalb schraffiert, Abspielkopf in `accent`, Velocity-Spur mit Stiel, Kopf und gestrichelter Mitte. Zeilenhöhe jetzt per Alt+Mausrad statt Schieber. Solange die Piano-Roll offen ist, sind die Spuren 36 px hoch. Drum-Raster nur auf Kit-Spuren (Umschalter Noten | Raster): Kick oben, 16tel in Vierergruppen mit Zählzeiten, Zeilen mit Farbkante, Name und Vorhören, Muster aus dem Beat-Katalog, Länge in Takten, Dichte, Würfel, Lauflicht beim Abspielen; Klick laut (nochmal aus), Umschalt+Klick leise, Rechtsklick löschen. Jingle-Assistent mit Schrittleiste (Haken, aktueller Schritt in Akzent), Stimmungskarten mit Symbol, Länge als Segmentschalter, Zusammenfassung in Mono und Weiter in Akzent. Probeton: Note an/aus als Kommando auf eine Spur (`probeton.rs`), ohne Ton testbar; erklingt beim Wählen eines Klangs in der Instrument-Kachel, über „Vorhören“ je Akkordfolge im Browser, je Drum-Zeile, auf der Klaviatur und beim Zeichnen. Tests: `probeton::tests` (fünf), `pianoroll::tests` (fünf neue), `akkorde::tests::chord_row_preview_sends_note_on_to_a_midi_track`, `drums::set_step_sets_loud_soft_and_clears`; die Jingle-Tests laden jetzt die Schriften.

## 0.35.19 — 2026-11-02

- **Design-Runde 3: Radio (ScreenRadio, RadioDecks, RadioLevels):** Fehlerbanner als eine Zeile mit Symbol, voller Text im Tooltip und hinter „Details“. Die Tasten „Deck A/B/C“ in der Sendereihe entfallen; Ablösen der Playlist mit Blende oder hart steht jetzt im Rechtsklick-Menü des Decks. −10/+10 als Symboltasten. Rot nur noch für live: „Auf Sendung“ in Ruhe neutral, Auto-Weiter als LED-Taste, „+ Titel“ neutral, Cart-Sätze 1–5 als Segmentschalter. ON-AIR-Lampe links in der Stream-Zeile. Decks: Restzeit-LCD 132 px in 7-Segment mit Geistern „88:88“ und Fortschrittsbalken, Start/Stopp mit Text, Cue, Vorhören und „Zurück zur Playlist“ als Symboltasten mit Tooltip, „Leeren“ rot. Pegel wie der Kanalzug im Produzenten: LED-Kette sichtbar, Namensschild unten, dB-Wert nur im Tooltip (Soll-Bild), Kopf „Quellen · Master · Ausgänge“, breiteres Master-Meter. Processing als Zeilen „Bezeichnung · Zahlenfeld/Schalter“ statt Slider und Häkchen, Messzeile als LCD in `lcd-ink`. Bei 1920×1030 passt alles samt Wunschtitel ohne Scrollen. Tests: `design3_tests` in `crates/gui/src/radio.rs` (sieben, vorher rot); angepasst: `back_to_playlist_toggle_is_there_and_clicks` sucht „Zurück zur Playlist Deck A“, `deck_buttons_take_over_with_fade_or_hard_and_empty_is_off` entfällt (ersetzt durch `deck_take_over_menu_offers_fade_and_hard`).

## 0.35.18 — 2026-11-02

- **Design-Runde 3: Studio (Spurkopf, Transport, Clip-Inspektor, Dock, Trim, Beats):** Echter Eingangs-Trim je Spur (−24 bis +24 dB), getrennt vom Fader: sitzt im Klanggraph direkt nach der Quelle vor den Inserts, wirkt also auch bei Gain-Automation, eigener Undo-Schritt (`Command::SetTrim`), im Rack als Feld „Trim … dB“ (Doppelklick = 0 dB), für Busse ausgeblendet; neues MCP-Werkzeug `set_trim`. Projektdateien ohne Trim bleiben unverändert. Spurkopf ohne „MIDI“-Badge (überlappte den Namen). Transport: Latenzausgleich als Symbol-Schalter mit ms-Feld, Lineal Takte/Zeit als Segmentschalter, eigene Taste „EXPORT“ neben der Lautheitswahl. Clip-Inspektor: Fades in Millisekunden (gespeichert weiter in Schlägen), mehr Abstand vor Löschen. Studio-Dock: Reiter Studio-Mixer · Piano-Roll · Automation · KI-Chat · Analyse als Segmentschalter mit Symbol, Rack 340 px breit. Bei schmalen Fenstern (1366 px) weicht die Pegel-LCD im Transport, damit nichts überlappt. Browser › Beats: alle Drum-Presets mit Mini-Raster (6 Reihen × 16 Schritte), darunter die Kits; ein Klick legt eine Drum-Spur mit Kit und 4-Takt-Clip am Abspielkopf an (ein Undo-Schritt), der leere MIDI-Clip bleibt. Tests: `crates/model/tests/trim.rs` (drei), drei Trim-Tests in `crates/engine/tests/mixer.rs`, `set_trim_is_separate_from_gain_and_undoable` in `crates/agent/tests/mcp.rs`, `crates/gui/tests/design3_studio.rs` (fünf).

## 0.35.17 — 2026-11-02

- **Design-Runde 3: Party mit Lime, Wellenform und BPM (ScreenParty, Deck, §2/§4):** Zweite Akzentfarbe Lime (`accent-2`, dunkel #c6ff3a mit Text #0e1400, hell #3f6600 mit weißem Text) steht für „eingeschaltet“: Auto-Übergang, Zufall, Wiederholen, Stumm, Wunsch-Taste (Lime-Rand und -Schein), Fernbedienung „An“, die meistgewählte Stimmen-Pille, Kartentitel, „als Nächstes“, Marken VORGELADEN/BEREIT und die Beatgrid-Striche; nie für „läuft“ oder „verbunden“. Decks: Restzeit als 7-Segment immer MM:SS mit Geistersegmenten, daneben ein BPM-LCD, wenn die Analyse ein Tempo kennt; die Play-Taste pulsiert auf dem Schlag (ohne Analyse gleichmäßig). Statt des Fortschrittsbalkens eine Wellenform (52 px, gespielter Teil in Modusfarbe, Rest gedimmt, Abspielkopf, Beatgrid oben). Die Wellenform wird einmal im Hintergrund dekodiert (1000 Spitzen) und in der Bibliothek je Inhalts-Hash zwischengespeichert: neue Tabelle `waveforms`, Schema 5 — Headless und GUI müssen gemeinsam ausgeliefert werden, eine ältere Version öffnet die Bibliothek mit Schema 5 nicht. Suche und Wunsch-Eingabe als Eingabefeld (44 px, Symbol links). „Wdh. aus“ heißt „Wdh.“, der Zustand steht nur in der Füllung (Tooltip nennt ihn). Die Fernbedienung steht rechts fest unter Bibliothek und Wunschliste (die scrollen), damit QR, Adresse, PIN und Handyzahl sichtbar sind, sobald „An“ leuchtet. Fotomodus `WOA_REMOTE` schaltet die Fernbedienung für Screenshots ein. Tests: `waveform::tests` (drei, Migration von Schema 4), in `party.rs` `seg_clock_is_always_mm_ss_with_ghost`, `beatgrid_follows_bpm_and_length`, `wave_columns_take_peak_per_column`, `pulse_follows_beat_or_is_even`, `party_lime_tokens_dark_and_light`, `only_the_top_vote_is_lime`, `party_input_is_44_px`; angepasst: `party_pult_has_three_groups_of_one_key_size` (Taste „WDH.“).

## 0.35.16 — 2026-11-02

- **Design-Runde 3: Auftrag und Ist/Soll abgelegt:** `docs/auftrag-design.md`, `docs/auftrag-nachbesserung.md` (Prüfung vom 01.11.) und `docs/auftrag-runde-3.md` sind der Maßstab für Runde 3, zusammen mit dem Designsystem Fassung 4 (inhaltlich gleich mit `docs/design/woa-design-system/`, geprüft). `docs/design/soll/` enthält die Zielbilder Produzent, Piano-Roll, Radio und Party, `docs/design/ist/` die Ist-Bilder von 0.35.12/0.35.13. Bildschirmfotos mit privaten Inhalten (Benutzerpfad, Titelliste) liegen nur lokal unter `docs/intern/`.

## 0.35.15 — 2026-11-02

- **Design-Runde 2: Akkordspur, Klangwahl und Spurfarben im Produzenten (ChordTrack, SoundPicker, PianoRoll):** Unter dem Lineal eine Akkordspur (26 px) mit Kopf „Akkorde · C Dur“ und je Takt einem Chip mit Akkord und Stufe (C I, G V, Am vi, F IV). Tonart und Akkorde werden aus den Noten aller MIDI-Spuren (ohne Drums) erkannt, folgen also jedem Transponieren; die Spur erscheint nur, wenn es Töne gibt. Akkord-Clips zeigen unter dem Kopf je Takt den Akkord mit Trennlinien in Spurfarbe, MIDI-Clips rechts im Kopf den Klang in Versalien (STREICHER, E-BASS). Im Rack ein eigener Abschnitt „Instrument“ zwischen Eingang und Inserts: Kachel mit Symbol, Klang und „Rolle · Bank“, aufgeklappt Rollen Lead · Bass · Pad · Kit mit je vier Klängen, Klangbank-Wahl und „Eingebauter Synth“; ein CLAP-Instrument ersetzt die Kachel durch seinen Namen. MIDI-Spuren zeigen als Eingang das MIDI-Gerät („Kein MIDI-Gerät“) statt „Mikrofon“. Piano-Roll, Velocity-Spur und Drum-Raster malen Noten in der Spurfarbe, Auswahl mit hellem Rahmen, Zeilen der Tonart leicht in Spurfarbe getönt, neues Lineal mit Taktnummern und Akkordnamen, Notenname in breiten Noten. Akkord- und Tonarterkennung neu in `woa_model::harmony`, `chord_names` von der GUI ins Modell verschoben. Sicherheit: Notenlage und -länge sind auf `MAX_NOTE_BEATS` (2^20 Schläge) begrenzt, die Takt-Schleife der Akkorderkennung auf die Taktzahl. Tests: `crates/gui/tests/komponieren.rs` (drei Tests, vorher rot), `harmony::tests` (drei), `timeline::tests::harmony_reads_chords_and_key_from_midi_tracks`, `sound_caption_is_upper_case`, `pianoroll::tests::notes_take_the_colour_of_their_track`.

## 0.35.14 — 2026-11-01

- **Design-Runde 2: Produzent mit Browser links (ScreenStudio, Browser, Transport):** Die Knopfreihe unter den Spuren (Audio-Spur, MIDI-Spur, Audio importieren, Produktionen, MIDI-Clip, Akkordfolge, Jingle-FX, Jingle bauen) entfällt. Neu ist ein Browser links (280 px, volle Höhe, einklappbar über « bzw. Ansicht → Browser) mit sechs Reitern Klänge · Beats · Akkorde · FX · Samples · Jingles. Akkorde und FX erzeugen jetzt direkt im Browser statt in Fenstern: Folgen nach Dur und Moll mit Akkord-Chips in der gewählten Tonart, unten Stufen (I V vi IV) mit Akkordnamen, Tonart als 12 Tasten, Takte 2 · 4 · 8 (andere Längen 1–16 per Zahlenfeld), Ausgaben Akkorde · Bass · Arpeggio und die Hauptaktion „Einfügen · Takt n“ am Abspielkopf; der Tonartwechsel nach dem Einfügen transponiert wie bisher die drei Clips. FX als drei Formkarten mit Kurve (Riser, Whoosh, Downlifter), Länge 1 · 2 · 4 · 8 Schläge (1–32 per Zahlenfeld), Dauer in Sekunden, „Einfügen · Takt n“. Klänge: Audio importieren, Produktionen. Beats: leerer MIDI-Clip für das Drum-Raster. Samples: Freesound über den KI-Chat (öffnet den Chat-Reiter im Dock). Jingles: „Jingle bauen …“ öffnet den Assistenten. Neue Spur über „+“ im Spurenkopf (Audio-Spur, MIDI-Spur). Transportleiste: Loop · Punch · Stopp am Ende als eigene Gruppe neben den Transporttasten, eingeschaltet einheitlich `accent-soft` mit Rand `accent` statt Blau, Rot und Orange. Fotomodus `WOA_DEMO_SONG` legt für Screenshots ein Jingle-Gerüst mit Akkorden an. Tests: `crates/gui/tests/studio_browser.rs` (vier Tests, vorher rot) und `insert_label_names_the_bar_at_the_playhead` in `crates/gui/src/browser.rs`; angepasst: `button_opens_the_produktionen_window` klickt erst den Reiter „Klänge“.

## 0.35.13 — 2026-11-01

- **Design-Runde 2: Kopfleiste und Radio-Modus nach dem neuen Designsystem:** Kopfleiste rechts als ein eingelassener Block (`status_bar`) mit drei Gruppen: Modus (Produzent · Radio · Party, je in seiner Modusfarbe) | MIDI · ON AIR · REC · KI | Feedback. Alle Einträge gleich: LED, Wort in Versalien, an = Fläche `surface` mit Rand in der eigenen Farbe, aus = LED gedimmt. MIDI öffnet per Klick die Gerätewahl, ON AIR die Stream-Verbindung, KI die KI-Einstellungen; REC zeigt beim Mitschnitt die Laufzeit und blinkt. Radio: Tastenreihe Start · Stopp · Blende · Mikro · Telefon als 44-px-Tasten (Blende: steht die Playlist, startet sie, sonst Crossfade auf den nächsten Titel), darunter Sprung-, Wiederhol- und Zufallstasten als 32-px-Symboltasten und die Deck-Übernahme A/B/C. Das große Cover entfällt, vorn steht die Studio-Uhr. Decks: gleich hohe Zeilen mit Buchstabe im LCD-Kästchen und Restzeit in Sieben-Segment-Schrift, laufend Rahmen `accent`, leer gestrichelt mit „Titel hierher ziehen · Rechtsklick: Datei laden“, beim Ziehen einer Playlist-Zeile „Hier ablegen“ in `accent-2`, unter 10 s blinkt die Restzeit rot. Cart-Pads neutral auf `surface`, Farbe nur im Balken unten (Spurfarbe 35 %, laufend füllt sich der Fortschritt) und in der LED, laufend `accent-soft` mit Rand `accent`, leere Pads gestrichelt. Pegel als eigene Karte neben Processing: Kanalzüge Playlist, Carts, Decks, Mikro mit farbigem Namensschild, Master-Meter, Notebook und Stream mit M-Taste, Gruppen durch 1-px-Linien getrennt, Fader-Weg 130 px. Sende-Playlist mit den Spalten # · Titel · Lautheit · Dauer, Dauer neutral statt in Akzentfarbe, ON AIR / CUE als Badge; Hoch, Runter und Entfernen erscheinen beim Hover und an der gewählten Zeile (Platz bleibt reserviert, nichts springt) und stehen zusätzlich im Rechtsklick-Menü. Wunschtitel als eine Zeile, die Wünsche darunter nur, wenn es welche gibt. Designsystem unter `docs/design/woa-design-system/` (und die zip) durch die neue Fassung ersetzt. Erhalten: Satz-Reiter (#296), ganze Zeile anklickbar (#302), Ziehen in die Decks (#298), Pult-Meter (#301), Notebook- und Stream-Fader (#96). Test: `pads_are_neutral_with_colour_bar_and_titles_two_lines` in `crates/gui/src/widgets/pad.rs` (vorher rot), dazu angepasst statt `filled_pads_are_green_empty_grey_and_titles_two_lines`.

## 0.35.12 — 2026-11-01

- **Party nach Designsystem 2 (ScreenParty, PartyControls):** Party-Pult mit allen Befehlen als Großtasten in drei Gruppen mit Linie: Abspielen (Zurück · Play/Pause · Stopp · Weiter), Springen (Von vorn · −10 s · +10 s · Wiederholen), Lautstärke (Leiser · Anzeige · Lauter · Stumm). Eine Tastengröße, vier Spalten, 64 px hoch (48 px bei Fenstern unter 900 px Höhe). Die Lautstärke steht in einem LCD mit Balken und dB statt als Text unter den Tasten. Auto-Übergang und Zufall sind Schalter links neben dem Crossfader (Skala A | B darunter). Zustand nur über Füllung: Play läuft = Modusfarbe gefüllt und pulsiert ruhig (nicht bei reduzierter Bewegung), eingeschaltet (Auto-Übergang, Zufall, Stumm, Wiederholen, Vollbild) = `accent-2` gefüllt, sonst neutral; keine grünen Flächen mehr (Play-Taste, „läuft“ in der Playlist, Marke „JETZT“, „Fernbedienung an“). Decks: Plattenteller links, Deck-Name, Marke, Titel in Versalien, Restzeit als 7-Segment-LCD und Play/Pause als Großtaste, Fortschritt darunter. Unter den Decks links Schalter, Pult und Playlist (füllt die Höhe; Kopf mit Playlist-Wahl, Titelzahl, „Ordner laden“ und Vollbild), rechts in 520 px (bei 1366 px ein knappes Drittel) Bibliothek, Wunschliste (Zähler, Eingabe mit Wunsch-Taste, leerer Zustand mit Hinweis, Nebenaktionen als Symbol-Tasten mit Tooltip) und Fernbedienung (An als Schalter im Kopf, QR, Adresse und PIN, Handyzahl, OSC-Eingang). Alle bisherigen Funktionen bleiben: Auto-Übergang, Crossfader, Wunschtitel-Ordner, Wünsche suchen, einreihen, erledigen, entfernen, Ziehen in die Playlist, DJ-PIN, neue PINs, Tastenkürzel, Feedback in der Party-Ansicht (#288). Tests: `party_pult_has_three_groups_of_one_key_size` ersetzt `party_buttons_have_symbol_word_and_one_height` (Zufall, Vollbild und Ordner laden stehen nicht mehr im Pult), `key_state_is_fill_only_and_never_green`, `side_column_and_key_height_follow_window`; `crossfader_and_auto_switch` heißt `crossfader_position` (das Wort am Auto-Schalter wechselt nicht mehr, der Zustand steht in der Füllung).

## 0.35.11 — 2026-11-01

- **Updater: Signatur ist Pflicht (#136):** Der Pfad mit reiner SHA-256-Prüfung (Stufe A) ist aus dem Code entfernt. `verify_download` lehnt ab, wenn kein gültiger Schlüssel eingebaut ist (`NotConfigured`) oder die Signatur fehlt; eine passende Prüfsumme ersetzt sie nie. `verify_hash` entfällt, die Prüfsumme im Manifest bleibt als Zusatz. `download_and_verify` lädt immer erst die Signatur. `release.yml` bricht bei einem Tag ohne Secret `WOA_UPDATER_KEY` ab und prüft die Signatur gegen `updater.pub`. Tests: `no_valid_key_means_everything_is_refused_even_with_a_matching_hash`, `downgrade_guard_comes_first`, `updater_pub_is_valid_key` in `crates/updater/src/verify.rs`. Doku: `docs/updater.md`.

## 0.35.10 — 2026-11-01

- **Ganze Zeile anklickbar (#302):** In der Sende-Playlist und in den Decks ist die ganze Zeile eine Klickfläche, man muss nicht mehr den Titel treffen. Playlist: Klick wählt aus (Rahmen in Akzentfarbe), Doppelklick setzt „als Nächstes“, Rechtsklick öffnet überall das Titelmenü (Umbenennen, Titelinfos …), Ziehen wie bisher von jeder Stelle. Decks: Rechtsklick irgendwo auf der Deck-Zeile öffnet das Menü (Datei laden, Schneiden, Leeren). Nur Knöpfe und die Nummer (eintippen) behalten ihren eigenen Klick. Die Cart-Pads waren schon ganz klickbar. Technisch: neue Hilfsfunktion `row_area` (Klickfläche unter dem Inhalt per `UiBuilder::sense`), Beschriftungen in der Zeile sind nicht mehr markierbar, der lange Hover-Text wird erst beim Zeigen gebaut. Trägheit gemessen: 300 Zeilen etwa 15 ms je Bild mit Klickfläche gegen 13 ms ohne (Debug-Bau, Notebook), unter der `ui.frame_slow`-Schwelle von 50 ms; der Test vergleicht relativ, weil die CI langsamer ist; Wellenformen werden in den Zeilen nicht gezeichnet. Tests: `whole_row_selects_and_opens_the_menu_anywhere`, `many_rows_stay_fast` in `crates/gui/src/radio.rs`.

## 0.35.9 — 2026-11-01

- **Cart-Wall wieder sichtbar, Decks kompakter (#303):** Seit den Ausgangsreglern (#96, 0.35.x) stand in der Deck-Karte ein `separator()` zwischen Pegel- und Ausgangsreglern. In der rollenden rechten Spalte nahm dieser senkrechte Strich die ganze Höhe ein; die Deck-Karte wurde so hoch wie die Spalte und schob Wunschtitel und Cart-Wall aus dem Bild. Dazu waren Decks plus Regler breiter als die Spalte und wurden rechts abgeschnitten. Neu im Radio-Modus rechts: oben die Decks, je Deck eine Zeile (Name, Uhr, Start, Cue, Vorhören, Playlist, Titel), die Pegelregler daneben oder bei schmaler Spalte darunter (niedriger, 76 statt 150 px). Als Zweites die Cart-Wall mit den Satz-Reitern, die Reiter brechen um statt die Spalte zu verbreitern; der Bedienhinweis steht als Tooltip am Titel „CART-WALL“, die laufende Tastenwahl weiter sichtbar. Danach Wunschtitel, Rack und Gäste wie bisher. Alle 16 Pads sind bei 1366×768 und 1920×1080 ganz sichtbar. Test im egui-Harness (`all_cart_pads_fit_the_window`, vorher rot: Cart 1 lag unterhalb des Fensters). Fotomodus: `WOA_WINDOW=1366x768` setzt die Fenstergröße beim Start (zusammen mit `WOA_SCREENSHOT`).

## 0.35.8 — 2026-11-01

- **Pult-Modus: Sendung kommt vom externen Mischpult (#301):** Neuer Schalter „Sendung kommt vom Pult“ unter Einstellungen → Ausgänge, dazu Eingang vom Pult (Gerät und Kanalpaar). Ist er an, gehen Playlist, Carts und Decks wie bisher auf die gewählten Ausgänge (Richtung Pult), aber ohne Sendebus und ohne WOAs Mikro; Stream und Mitschnitt bekommen nur noch den Pult-Eingang (Taktverbindung wie beim Telefon), die Musik geht also nicht doppelt raus. Der Sendebus (Lautheitsregelung, Kompressor, Limiter) läuft mit seinem bisherigen Schalter auf dem Pult-Signal. WOAs Mikro ist im Pult-Modus aus (es hängt am Pult, sonst Rückkopplung); Telefon und Gäste gehen mit der Musik ins Pult. Vorhören auf Kanal 3/4 des Pult-Geräts wird abgelehnt (käme auf Sendung), ein eigenes Kopfhörer-Gerät bleibt. Pegel des Pult-Eingangs als LED-Meter neben dem Telefon. Fehlt der Eingang oder passt das Kanalpaar nicht, kommt die Sendung mit Meldung weiter aus WOA. `config.json` ohne Eintrag `pult` lädt unverändert, ohne Schalter ist die Sendung bit-gleich zu vorher. Noch nicht über MCP (die Ausgänge in `config.json` hat MCP bisher gar nicht). Tests: `crates/radio/tests/pult.rs` (nur Pult-Sinus auf Sendung, keine Playlist-Musik; Ausgang unbearbeitet; ohne Schalter bit-gleich; Pegel), `routing::pult_tests` (Geräte, Ausfall, Quelle auf fremdem Gerät, altes Format).

## 0.35.7 — 2026-11-01

- **Feedback geht auch in der Party-Ansicht (#288):** Die Party-Ansicht zeichnete nur ihr eigenes Pult und kehrte dann zurück, ohne die Fenster aus dem Menü zu zeichnen. Die Feedback-Taste öffnete das Fenster also unsichtbar, erst nach dem Wechsel zu Radio erschien es. Jetzt zeichnen alle drei Ansichten dieselben Fenster über `WoaApp::app_windows` (Feedback, Über WOA, Ausgabe, Telefon, Ordner, Stream, KI, Telemetrie, Export, Controller). Tests: `party_feedback_tests` in `crates/gui/src/app.rs` (Party war vor dem Fix rot, Radio als Gegenprobe).

## 0.35.6 — 2026-11-01

- **Playlist-Titel lassen sich wieder am Griff auf ein Deck ziehen (#298):** Seit dem Griff-Symbol aus #92 (0.34.74, Commit `1933917`) war der Griff am linken Zeilenrand ein markierbarer Text. egui gibt das Ziehen dann der Textauswahl statt der Zeile; die Zeile bewegte sich nicht, und auf dem Deck kam nichts an. Jetzt sind alle Texte einer Playlist-Zeile nicht markierbar, Ziehen am Griff wie am Titel lädt aufs Deck oder verschiebt in der Playlist. Klick, Doppelklick, Nummer eintippen und Kontextmenü bleiben. Bibliothek und Dateien aus dem Explorer waren nicht betroffen und sind jetzt mit abgedeckt. Tests im echten Pult-Layout ohne Audiogerät (`RadioDesk::headless`, der Mixer rechnet nur in einen Puffer): `deck_drop_tests` in `crates/gui/src/radio.rs` (Bibliothek, Explorer-Datei, Playlist am Griff, Playlist am Titel).

## 0.35.5 — 2026-11-01

- **Lautstärke innerhalb eines Titels konstant (#297, Refs #147):** Die Lautheitsregelung im Sendebus (`woa_radio::dynamics::BusProcessor`, seit Paket 9) führte die Verstärkung gleitend zum Ziel, mit bis zu 3 dB je Sekunde im Bereich ±12 dB. Damit hob sie leise Passagen eines Titels an und senkte laute ab, und nach jedem Ducking pumpte sie nach. Seit #149 (0.34.69) ducken auch die Carts, so fiel das öfter auf. Die Regelung regelt jetzt nicht mehr nach. Jeder Titel behält seine feste Verstärkung zum Ziel (Auto-Gain beim Bereitlegen); Kompressor und Limiter bleiben als Schutz unter „Sendebus“. Die Lautheitsanzeige am Ausgang bleibt, die Angabe „Regelung … dB“ entfällt. Die Decks A/B/C werden beim Laden jetzt gemessen und bekommen denselben Auto-Gain wie die Playlist (bisher 0 dB, Entscheidung André in #147). Erlaubt bleiben nur die Blenden am Anfang und Ende und das Ducking bei Mikro, Telefon oder Cart. Test: `crates/radio/tests/constant_gain.rs` (ein Titel mit leisem, lautem und leisem Teil über Playlist und Deck mit eingeschaltetem Sendebus, die Verstärkung schwankt um weniger als 0,5 dB, vorher 24 dB bzw. 21 dB; nach einem Cart kommt die Musik auf ihren alten Pegel zurück, vorher 7,8 dB daneben), alle Blöcke unter `assert_no_alloc`.

## 0.35.4 — 2026-11-01

- **WOA-Logo in der Windows-Taskleiste:** Die `woa.exe` trägt jetzt `packaging/windows/woa.ico` (16 bis 256 px) als Icon-Ressource, eingebettet per `apps/woa/build.rs` mit `winresource` (Build-Abhängigkeit nur unter Windows). Bisher zeigte die Taskleiste einen Platzhalter, weil das Fensterbild aus dem PNG nur als kleines Symbol ankommt und die Exe kein eigenes Icon hatte; aus der ZIP gestartet fehlte es ganz. Der Windows-Bau prüft das jetzt (Icon der `woa.exe` ungleich Standardsymbol). `woa-headless` und `woa-streamd` sind Konsolenprogramme ohne Fenster und bleiben ohne. Linux unverändert.

## 0.35.3 — 2026-11-01

- **PIN-Bremse nachgezogen (#294, drei Folgebefunde):** (1) Die globale Grenze (30 falsche PINs je Minute) sperrt nicht mehr alle aus, sondern verlangsamt: darüber wird nur noch ein Versuch je 2 s geprüft, die übrigen bekommen 429 ohne PIN-Vergleich; so kommt auch die richtige PIN durch. (2) Die Ausnahme hängt nicht mehr an der IP, sondern an einem gültigen DJ-Token (`Auth::login_with_token`, der Server liest es aus dem `Authorization`-Header); eine DJ-Adresse allein genügt nicht mehr. (3) Gezählt wird je volle Adresse statt je /64, ein Störer sperrt also nicht das ganze LAN; das Wechseln der Adresse fängt die globale Verlangsamung ab. Folge: Ein DJ ohne Token (neues Gerät) kann während eines Angriffs bis zu etwa 2 s warten müssen. Tests: `changing_ula_addresses_is_slowed_down_globally`, `real_dj_gets_in_during_a_guessing_attack`, `dj_address_alone_does_not_exempt_from_the_brake`, `ipv6_addresses_are_counted_individually`.

## 0.35.2 — 2026-11-01

- **Cart-Wall-Sätze 1–5, Cart-Dauer, Umbenennen (#292):** Über der Cart-Wall stehen fünf kleine Reiter, je ein Satz mit eigenem Namen (etwa Morgen, Mittag, Sondersendung) und eigener Belegung. Klick wechselt, Rechtsklick → „Umbenennen …“. Gespeichert in `radio.json` wie die Playlisten: der aktive Satz in `carts`, die anderen in `cart_sets`, der aktive in `cart_set`. Alte Dateien: die Belegung wird Satz 1. Tasten gelten je Platz. Ein laufender Cart läuft beim Wechsel weiter. Jedes Pad zeigt rechts unten seine Dauer (Start bis Ende des Schnitts), im Lauf wie bisher den Countdown; der Name ist größer (15 statt 13 px). Umbenennen überall per Rechtsklick → „Umbenennen …“: Cart, Satz, Playlist-Titel (neues Feld `name` am Eintrag, die Datei bleibt) und in der Bibliothek (öffnet die Titelinfos). Grund, warum André die Titel nicht fand: Die Playlist zeigte immer den Dateinamen, „Titelinfos bearbeiten“ schrieb nur die Tags der Datei, die Änderung war also unsichtbar. Leerer Name heißt zurück zum Dateinamen. MCP: neue Werkzeuge `select_cart_set` und `rename` (`cart_set`, `cart`, `title`), über dieselben Funktionen wie die App; Spuren weiter mit `rename_track`. Tests: `crates/radio/src/cart_sets.rs`, `crates/radio/tests/rt.rs` (laufender Cart über den Wechsel), `crates/gui/src/radio.rs` und `widgets/pad.rs` (Dauer am Pad), `crates/agent/src/lib.rs` (`cart_set_tests`).

## 0.35.1 — 2026-11-01

- **PIN-Bremse: wechselnde IPv6-Adressen im LAN umgehen sie nicht mehr (#294, zu #234):** Link-local- und ULA-Adressen zählen jetzt wie alle IPv6-Adressen je /64. Dazu gibt es wieder eine Obergrenze, die nicht von der Adresse abhängt: höchstens 30 falsche PINs je Minute insgesamt (`Limits::global_failures_per_minute`); ist sie erreicht, wird jede Anmeldung von Adressen ohne DJ-Sitzung abgelehnt, ohne die PIN zu prüfen (kein Treffer als Orakel). Die PIN wird nicht neu gewürfelt. Adressen, die schon eine DJ-Sitzung haben, sind von beiden Sperren ausgenommen, der angemeldete DJ kommt also weiter herein. Folge: Wer noch nicht angemeldet ist, kann während eines Angriffs bis zu einer Minute nicht anmelden. Tests: `changing_ula_addresses_cannot_bypass_the_brake`, `real_dj_gets_in_during_a_guessing_attack`.

## 0.35.0 — 2026-11-01

- **J10 Sample-Suche über Freesound (Paket aus #262, das letzte J-Paket):** Neue MCP-Werkzeuge `search_samples` (`text`, `limit`) und `import_sample` (`id`, `track`, `start_beats`, `name`). Gesucht wird nur nach CC0 und CC BY: der Filter steht in der Anfrage, und jede Antwort wird beim Lesen noch einmal geprüft, CC BY-NC, Sampling+ und Unbekanntes fallen raus, auch beim Import einer Nummer. Der Import lädt die Vorschau (MP3, das Original bräuchte OAuth2) nach `aufnahmen/`, legt den Clip an und schreibt die Herkunftszeile (Titel, Urheber, Lizenz mit Link, Quelle) in `notizen.md` im Projektordner. Der Schlüssel ist der eigene des Nutzers: neues Feld „Freesound-Schlüssel“ unter Einstellungen → KI, abgelegt im Schlüsselbund (Eintrag `freesound`), nie im Projekt, nie im Log, nie in einer Antwort ans Modell. Ohne Schlüssel sagen beide Werkzeuge, wo er hingehört. Nur https. Keine neue Abhängigkeit (ureq gab es schon). Tests: `crates/agent/src/freesound.rs`, `crates/agent/tests/j10_samples.rs` (lokaler Test-Freesound, kein Netz, kein Schlüsselbund), `crates/gui/src/ki_setup.rs`. Damit sind J1 bis J10 gebaut; Version 0.35.0.

## 0.34.98 — 2026-11-01

- **J4 Assistent „Jingle bauen“ (#289, Paket aus #261):** Neuer Knopf „Jingle bauen …“ neben „Akkordfolge …“ und „Jingle-FX …“. Sechs Schritte: Vorlage (Länge 5/10/30 s, Stimmung fröhlich, seriös, Nachrichten oder Party; Takte, Tempo, Tonart und Beat stehen klein darunter) → Beat (Preset, Würfeln, „Im Raster bearbeiten“ öffnet das Drum-Raster) → Akkorde (die Folgen der Stimmung zuerst) → Stimme („Stimme über Bett“, J5) → Schluss (Schluss-Stab, „Riser davor“ aus J8) → Fertig (öffnet den Export mit „Als Jingle“, J6). Der Assistent legt ein neues Projekt an (fragt vorher wie „Neues Projekt“ nach dem Speichern) mit normalen Spuren und MIDI-Clips: Drums mit GM-Kit, Akkorde, Bass, Arpeggio (J3) und eine leere Audio-Spur „Stimme“, und schaltet den Loop über das ganze Jingle. Längen-Rechnung: ganze Takte, das Tempo wird so gesetzt, dass die Länge genau stimmt (10 s Party: 5 Takte bei 120 BPM). Schluss-Stab: der letzte Takt wird ein einzelner Schlag (Grundakkord, Grundton, Becken und Kick). Jeder Schritt ist ein Undo-Schritt; nach Undo bis vor das Anlegen springt der Assistent auf Schritt 1. Vorlagen und Rechnung liegen als Daten in `woa_model::jingle`. Kein neues MCP-Werkzeug: Claude geht denselben Weg mit `set_tempo`, `generate_drums`, `generate_chords`, `voice_over`, `generate_fx` und `export`. Tests: `crates/model/src/jingle.rs`, `crates/gui/src/state.rs` (Anlegen, Stab, Speichern und Öffnen, Undo), `crates/gui/src/jingle_assistent.rs` (alle sechs Schritte per Klick bis zum Export).

## 0.34.97 — 2026-11-01

- **J5 Stimme und Ducking (#285, Paket aus #261):** Neue Funktion `Session::voice_over` im Modell: Die Stimmspur (Aufnahme oder Datei, beides gab es schon) bekommt die Sprachkette, falls sie noch keine hat, und jede andere Audio- und MIDI-Spur ohne Sprachkette einen Kompressor (Schwelle −40 dB, 8:1, Anstieg 5 ms, Rückstellung 300 ms), dessen Sidechain die Stimme ist. Busse können keine Sidechain haben, darum sitzt der Ducker auf den Bett-Spuren selbst. Ein Undo-Schritt, ein zweiter Aufruf legt nichts doppelt an. Ein Weg für Mensch und MCP: in der App „Stimme über Bett“ im Menü „+ Effekt“ neben „Vorlage Sprachkette“, über MCP das neue Werkzeug `voice_over` (`voice`). Messung mit `analyze` im Test: Mischung aus Sinus-Bett und Stimme −11,4 LUFS ohne, −14,7 LUFS mit Ducking (Sprachkette in beiden Fällen gleich). Tests: `crates/model/src/voice_over.rs`, `crates/agent/tests/j5_voice_over.rs`, `crates/gui/tests/voice_over.rs`.

## 0.34.96 — 2026-11-01

- **J8 Jingle-FX (#283, Paket aus #262):** Neue Klangerzeugung `woa_dsp::generate::render_fx`: Riser (schwillt an, Filter öffnet sich), Whoosh (auf und zu, laut in der Mitte) und Downlifter (setzt laut ein, Filter schließt sich), aus Rauschen durch einen wandernden Tiefpass (150 Hz bis 12 kHz), Spitze −3 dBFS, Anfang und Ende ohne Knacken. Ein Weg für Mensch und MCP: neues MCP-Werkzeug `generate_fx` (`track`, `kind`, `length_beats`, Vorgabe 4, `start_beats`, `name`) und in der App der Knopf „Jingle-FX …“ neben „Akkordfolge …“ (Art, Länge in Beats, Clip am Takt des Abspielkopfs auf der gewählten Audio-Spur, sonst neue Spur, ein Undo-Schritt). Beide rechnen über `woa_agent::audio::fx_buffer`; die Länge folgt dem Projekttempo. Keine neue Abhängigkeit. Tests: `crates/dsp/src/generate.rs` (Form je Art, Grenze, kein Knacken), `crates/agent/tests/j8_fx.rs` (Länge bei 100 BPM, `analyze` ohne Clipping, Fehlerwege), `crates/gui/tests/jingle_fx.rs`. Offen: die Kenney „Impact Sounds“ (CC0) als Bibliotheksordner sind nicht dabei, die Seite kenney.nl lehnte den Abruf ab (HTTP 403), Datei und Prüfsumme sind daher nicht geprüft.

## 0.34.95 — 2026-11-01

- **Fernbedienung (WAN, #162):** Falsche PINs würfeln die DJ-PIN nicht mehr neu, und es gibt keine globale Sperre, mit der ein Angreifer den echten DJ aussperren könnte. Fehlversuche zählen je Adresse; nur globale IPv6-Adressen zählen je /64-Netz, damit ein Internet-Anschluss nicht mit jeder neuen Adresse weiterraten kann. Link-local, ULA und IPv4 bleiben je Adresse, ein Gast im WLAN sperrt den DJ also nicht mit aus. PIN und Token werden nie geloggt. Tests für beide Angriffe dazu.
## 0.34.94 — 2026-11-01

- **Weiterleitung verliert unter Windows keine Aufrufe mehr (#245):** `woa-headless mcp` nutzte eine ruhende HTTP-Verbindung erneut, die die Gegenseite schon geschlossen hatte; unter Windows scheiterte der nächste Aufruf (und das abschließende DELETE) dann still. Die Weiterleitung öffnet jetzt je Anfrage eine neue lokale Verbindung, das DELETE gibt nach 2 s auf. Test `forward_does_not_wait_for_an_open_sse_stream…` prüft zusätzlich, dass der Werkzeugaufruf ein Ergebnis statt eines Fehlers liefert.
- **KI-Sitzung überlebt Pausen (#243):** Unter Windows bekam `woa-headless mcp` nach rund 5 Minuten ohne Aufruf von der App nur noch HTTP 404, weil rmcp eine Sitzung nach 300 s Leerlauf verwirft. Der Server auf 127.0.0.1 hält Sitzungen jetzt ohne Leerlauf-Grenze (vom Netz erreichbar bleibt sie). Zusätzlich meldet sich die stdio-Weiterleitung bei 404 still mit der gespeicherten Anmeldung neu an und wiederholt den Aufruf einmal; endet der Client, beendet sie ihre Sitzung. Test: `forward_reinitializes_after_the_app_dropped_the_session` in `crates/agent/src/live.rs`.

## 0.34.93 — 2026-11-01

- **J9 MCP-Ausbau (#277, Paket aus #262):** Neues MCP-Werkzeug `list_instruments`: GeneralUser GS und die Zusatzbänke mit Zustand, Grösse und Lizenz, dazu die Klang-Presets (derselbe Katalog wie `track_sound`). Mit `clap: true` sucht es die CLAP-Instrumente und -Effekte des Rechners, nur geschützt je Datei im eigenen Prozess (`woa-plugin-host`) mit der Sperrliste der App, nie im MCP-Prozess. Neues Werkzeug `list_samples`: sucht die Bibliothek nach Art (`sample`, `jingle`, `music`), Tag und Name, oder mit `folder` einen Ordner (z. B. Jingle-FX, bis 4 Ebenen) nach Audiodateien; nur lesend (`Library::open_read_only`, ohne WAL-Umstellung und Migration, fremde Schemaversion abgelehnt), eine fehlende Bibliothek wird nicht angelegt, `library` nur im WOA-Datenordner oder Projektordner. `insert_effect` nimmt statt `kind` auch `clap_id` und setzt einen CLAP-Effekt auf eine Spur (nicht auf Busse, ein Undo-Schritt); `set_insert_bypass` und `remove_insert` wirken auch auf ihn. Ein fehlendes Plugin reicht das Signal durch. Tests: `crates/agent/tests/j9_tools.rs`. Nebenbei: der Testserver in `live.rs` schickt `connection: close`, damit `forward_does_not_wait_for_an_open_sse_stream_and_keeps_the_session` nicht mehr wackelt.

## 0.34.92 — 2026-11-01

- **Updater Stufe B (#136):** `packaging/windows/updater.pub` enthält jetzt den echten öffentlichen minisign-Schlüssel. Ab dieser Version verlangt der Updater eine gültige Signatur über das MSI, ohne Rückfall auf die reine SHA-256-Prüfung. Das Secret `WOA_UPDATER_KEY` für den Release-Workflow ist gesetzt. Der Stufe-A-Pfad bleibt im Code, bis das erste signierte Release draußen ist. Doku `docs/updater.md` nachgezogen.

## 0.34.91 — 2026-11-01

- **MCP-HTTP-Server nur auf Wunsch (#243):** Der zweite Server `woa-headless mcp … --http` (Port 8765) startet nicht mehr beim Start der App, auch wenn er in einer früheren Sitzung eingeschaltet war. Er startet nur, wenn man unter KI & MCP den Haken „HTTP-Server im Hintergrund“ setzt und speichert, und endet mit WOA. Port, Reichweite und Projektordner bleiben gespeichert. Der Live-Server der App (Stdio-Weiterleitung) ist unverändert. Test `app_start_never_starts_http_server`.

## 0.34.89 — 2026-11-01

- **J6 Ausgabe (#275, Paket aus #261):** Im Fenster „Exportieren“ neuer Schalter „Als Jingle“ mit Name, Lautheit („wie die Sendung (Auto-Gain)“ oder eigene in LUFS) und Cart (keine, 1–16). „Als Jingle exportieren“ rendert den Mixdown wie der normale Export, bringt ihn auf die Ziel-Lautheit und begrenzt die Spitzen mit dem vorhandenen Limiter auf −1 dBFS (neu `woa_engine::loudness::master`; anders als der bisherige Angleich erreicht das auch spitzenreiche Mischungen, der bisherige Export bleibt unverändert). Die WAV landet im Jingle-Ordner (#260, auch ein eigener aus „Speicherorte“), steht sofort als Art Jingle in der Bibliothek und liegt auf Wunsch auf der Cart (über `radio.json`, die laufende Sendung übernimmt es nach wenigen Sekunden). Nichts wird überschrieben: bei gleichem Namen kommt „Name 2“, bei gleichem Inhalt gilt das vorhandene Jingle. Neues MCP-Werkzeug `export_jingle` (`name`, `target_lufs`, `cart`) geht durch dieselbe Funktion `woa_agent::jingle::publish`. Dafür liegt die Ordner-Logik ohne Oberfläche jetzt in `woa_radio::ordner` (liest den Abschnitt `ordner` aus `config.json`) und `DB_FILE` in `woa_library`; die Oberfläche nutzt beides unverändert weiter. Kein neues Projektformat. Tests: `crates/engine/src/loudness.rs` (Ziel trotz Spitzen erreicht), `crates/agent/src/jingle.rs` (Ordner, Bibliothek, Cart, Ziel der Sendung, kein Überschreiben, falsche Namen und Carts), `crates/agent/tests/jingle_export.rs` (MCP mit eigenem Jingle-Ordner aus `config.json`), `crates/radio/src/ordner.rs`, `crates/gui/src/export_dialog.rs`.

## 0.34.88 — 2026-11-01

- **J7 Bänke (#271, Paket aus #262):** Zwei Zusatzbänke neben GeneralUser GS, geladen beim ersten Gebrauch: MuseScore General 0.2.0 (MIT, SF2, 206 MB; SF3 lädt rustysynth nicht) und AVL Drumkits „Red Zeppelin 4pc“ (CC BY-SA 3.0, nur für Kits). Katalog mit Datei, Grösse, SHA-256, URL und Lizenz in `docs/sounds/banks.tsv`, gelesen von der App (`crates/engine/src/banks.rs`) und von `packaging/fetch-soundfont.sh <ordner> <bank>`. Download nur über https ohne Weiterleitungen in den Datenordner (`banks/`), erst als `.part`, Name erst nach stimmender Prüfsumme, Lizenz daneben. Bis die Bank da ist und ohne Netz spielt GeneralUser GS. Die Bank steht je Spur im Klang (Projektformat 6, nur wenn eine Bank gewählt ist; alte Projekte laden unverändert, unbekannte Bänke spielen GeneralUser GS). Wahl im Klang-Menü des Mixers unter „BANK“ mit Ladefortschritt und per MCP `track_sound` mit `bank` (Antwort mit `bank_state` und `banks`). Ein Klang- oder Bankwechsel baut jetzt sofort den Graphen neu. Dank im Fenster „Über WOA“. Die fertige Bank wird noch im Download-Thread eingelesen (kein Ruckeln der Oberfläche), ein hängender Download gibt nach einer Stunde auf.

## 0.34.87 — 2026-11-01

- **J3 Akkorde und Muster (#269, Paket aus #261):** Neuer Knopf „Akkordfolge …“ neben „+ MIDI-Clip“ öffnet ein kleines Fenster: Folge aus einem Katalog von elf (z. B. „I–V–vi–IV · hymnisch, der Pop-Klassiker“, „i–VI–III–VII · episch, groß“), Tonart (12 Grundtöne, Dur oder natürliches Moll je nach Folge), Takte (1–16), „Erzeugen“. Es entstehen drei MIDI-Spuren ab dem Takt am Abspielkopf: Akkorde (Dreiklänge, Klang Streicher), Bass (Grundton und Oktave, E-Bass), Arpeggio (Achtel aufwärts, Klavier), ein Akkord je Takt, alle Töne leitereigen, ein Undo-Schritt. Ein Tonartwechsel im Fenster danach transponiert alle drei Clips (ein Undo-Schritt). Es sind normale MIDI-Clips, Piano-Roll und Zeitleiste bleiben nutzbar. Der Katalog sind Daten in `crates/model/src/chords.rs` (mit Stimmung je Folge für J4). Neue MCP-Werkzeuge `generate_chords` (`progression`, `key`, `bars`, `start_beats`, derselbe Katalog) und `transpose_notes` (`clips`, `semitones`). Akkord-Pads (optional im Konzept) sind nicht gebaut. Tests: `crates/model/tests/chords.rs`, `crates/gui/tests/chords.rs`, `crates/agent/tests/chords.rs`.

## 0.34.86 — 2026-11-01

- **J2 Drum-Raster (#267, Paket aus #261):** Die Piano-Roll zeigt auf einer Spur mit GM-Kit ein 16-Schritt-Raster (Becken, Hi-Hat offen, Hi-Hat zu, Clap, Snare, Kick = GM-Noten 49/46/42/39/38/36); der Knopf „Drum-Raster“ schaltet zur normalen Piano-Roll. Klick schaltet einen Schritt an/aus, Rechtsklick macht ihn leise (Velocity 60). Acht Presets (Pop, Four on the Floor, Rock, Hip-Hop, Nachrichten, Party, Halbes Tempo, Shuffle), wiederholt über die ganze Clip-Länge, dazu Regler „weniger / mehr“ und „Würfeln“. Gespeichert werden normale MIDI-Noten über `SetNotes`, also ein Undo-Schritt je Klick; Noten außerhalb des Rasters bleiben erhalten. Die Logik liegt in `crates/model/src/drums.rs`; `generate_drums` nimmt dieselben Presets (`preset`, `density`, `bars`). Tests: `crates/model/tests/drums.rs`, `crates/gui/tests/drum_grid.rs`, `crates/agent/tests/drum_grid.rs`.

## 0.34.85 — 2026-11-01

- **SoundFont in allen Paketen (#265):** Die J1-Klänge (#263) fanden in der ausgelieferten App keine Bank und spielten immer den Synth. Jetzt bringt jeder Bau `GeneralUser-GS.sf2` (2.0.3, fester Commit, feste SHA-256) samt Lizenz mit, an den Pfad, den `crates/engine/src/soundfont.rs` sucht: MSI und Windows-ZIP `sounds\` neben `woa.exe`, AppImage und Flatpak `share/woa/sounds/`. Neues Skript `packaging/fetch-soundfont.sh` lädt und prüft (AppImage, Windows); Flatpak lädt als `sources`-Eintrag mit sha256 (Bau ohne Netz), ein CI-Schritt hält beide Werte gleich. Die `.sf2` bleibt aus dem Repo. CI prüft je Paket, dass die Datei am Suchpfad liegt (ZIP entpackt, MSI nach Installation in `test-upgrade.ps1`, AppImage entpackt, Flatpak installiert).

## 0.34.84 — 2026-11-01

- **J1 Klänge: SoundFont als Instrument der MIDI-Spur (#263, Paket aus #261):** Eine MIDI-Spur kann jetzt einen GM-Klang aus der SoundFont (GeneralUser GS) spielen statt des eingebauten Synths. Auswahl im Mixer-Streifen der Spur: „Synth (eingebaut)“ und je Rolle vier Klänge (Lead: Klavier, E-Piano, Glockenspiel, Synth-Lead · Bass: E-Bass, Kontrabass, Synth-Bass, Fretless · Pad: Streicher, Warm Pad, Orgel, Chor · Kit: Standard, Room, Power, Elektro auf MIDI-Kanal 10). Der Klang ist ein Undo-Schritt und steht in der Projektdatei (`sound` an der Spur, Format 5 nur wenn gesetzt; Projekte ohne Klang bleiben Format 4). Fehlt die `.sf2`, spielt der Synth wie bisher. Gesucht wird über `WOA_SOUNDFONT`, dann `sounds/GeneralUser-GS.sf2` neben der ausführbaren Datei, dann `../share/woa/sounds/` (`crates/engine/src/soundfont.rs`); geladen einmal je Prozess im Control-Thread. Befund: Die Paketskripte laden die `.sf2` entgegen `docs/sounds/README.md` noch nicht, die README sagt das jetzt. Neues MCP-Werkzeug `track_sound` (lesen, Preset, GM-Programm, zurück zum Synth); `generate_drums` mit `midi: true` schreibt GM-Drumnoten als MIDI-Clip und gibt der Spur das Standard-Kit, in einem Undo-Schritt. Tests: `crates/engine/tests/soundfont.rs` (Offline-Render mit winziger SF2 klingt anders als der Synth), `crates/model/tests/sound.rs`, `crates/agent/tests/track_sound.rs`.

## 0.34.83 — 2026-11-01

- **WOA-Ordner: feste Ablage im Musikordner (#258):** Beim Start legt WOA `<Musik>/WOA/` mit `Projekte`, `Aufnahmen`, `Mitschnitte`, `Exporte`, `Jingles`, `MIDI` und `Sendungen` an, falls sie fehlen (still, Fehler nur im Log, nichts wird verschoben oder überschrieben). Der Musikordner kommt unter Linux aus `user-dirs.dirs` (Rückfall `~/Musik`), unter Windows ist es `%USERPROFILE%\Music`; keine neue Abhängigkeit. Alle Speichern- und Öffnen-Dialoge starten im passenden Ordner (Projekt, Export, Stems, Spur-Export, DAWproject, MIDI, M3U, Carts, Events, Voice-Track, Gast-Datei); Titel- und Ordnerdialoge im Musikordner. Mitschnitte (REC) landen ohne Dialog in `Mitschnitte`, Dateiname wie bisher mit Datum und Uhrzeit; ältere Mitschnitte bleiben im Datenordner. Neues Fenster „Einstellungen → Speicherorte …“: eine Zeile je Art mit „Ändern …“, „Vorgabe“ nur bei eigenem Ordner. Gespeichert in `config.json` unter `ordner`, nur Abweichungen; alte Konfigurationen laden unverändert. Tests in `crates/gui/src/ordner.rs` und `old_config_without_folders_loads_and_custom_folder_survives`.

## 0.34.82 — 2026-11-01

- **Server-Doku auf das Nötigste gekürzt:** `server/README.md` ohne den längst umgesetzten Patch-Vorschlag, die überholte `server/feedback/daw-woa.vars.yml` ist entfernt, `docs/telemetrie.md` ohne „Offen“ und ohne Serverpfade. Testpfade in `crates/gui/src/feedback.rs` heißen jetzt `nutzer`.

## 0.34.81 — 2026-11-01

- **Drei wackelige Windows-Tests an der Ursache behoben (#256):**
  - `live::tests::stale_lock_is_not_a_running_app`: Der Test band einen Port, gab ihn frei und erwartete, dass dort niemand hört. Windows vergab den Port sofort an einen parallel laufenden Test (`Some(RunningApp { port: 51369 })`). Jetzt prüft er mit Port 0, auf dem nie jemand hört.
  - `after_token_change_the_old_token_gets_401` (os error 10053): Der MCP-Server beantwortete abgewiesene Anfragen (401, 404), ohne den Body zu lesen, und schloss die Verbindung. Ungelesene Bytes beim Schließen werden unter Windows zu einem RST, der Client sah dann den Abbruch statt der 401. Der Server liest den Body jetzt weg und verwirft ihn stückweise (höchstens 1 MiB, 2 s, 16 Anfragen gleichzeitig, nichts wird gepuffert) und antwortet mit `Connection: close`. Neuer Test `rejected_request_is_read_before_the_answer`.
  - `guest_tone_arrives_and_return_has_music_but_not_the_guest`: Browser und Bank laufen im Test im selben Thread, die Bank rechnete nach der Wanduhr. Hing der Thread auf dem Runner, holte die Bank die Pause in einem Zug nach, bevor die Pakete ankamen, und las Stille (-14,0 und -16,9 dB statt -10,4). Nachgestellt mit einer Pause von 600 ms: -14,4 dB. Die Bank läuft jetzt im Takt des Mikrofons, die Pause ist fest im Test, gemessen -10,4 dB. Die Schwelle steht wieder auf -14 dB (0.34.64 hatte sie auf -16 gelockert).

## 0.34.80 — 2026-11-01

- **Controller-Test misst das Cart-Ducking mit (#254):** `port::virtual_controller_drives_mixer_and_carts` in `crates/gui/tests/controller.rs` war lokal rot und in der CI nur scheinbar grün. Ursache: Der Test stammt aus 0.9.0 und erwartete beim Cart die volle Musik plus Cart. Seit #149 senkt ein Cart die Musik ab (Standard −12 dB), der gemessene Pegel (0,57) blieb unter der alten Schwelle. In der CI fehlt der ALSA-Sequencer, dort überspringt sich der Test selbst. Der Test lässt die Absenkung jetzt ausschwingen und prüft beides: Cart hörbar über der abgesenkten Musik, und die Musik unter dem Cart tatsächlich abgesenkt. Das Ducking bleibt unverändert.

## 0.34.79 — 2026-11-01

- **Lesbare Warnungen und Insert-Rack (#246):** Fehler- und Warntexte (Stream, Gäste, Plugins, Fernbedienung, KI, Feedback) nutzen im dunklen Thema `danger_text()`: `on-air` leicht aufgehellt, damit sie auf `panel` WCAG AA (mindestens 4,5:1 statt rund 4:1) erreichen. Im hellen Thema bleibt alles wie bisher, Lampe und ON-AIR-Schriftzug ebenso. Hinweise, Plugin-Fehler und Log im Insert-Rack stehen jetzt in 12 px statt 9–9,5 px. Die Gruppentitel des Racks („EINGANG“, „AUSGANG“ …) sind Modulköpfe wie überall sonst. Rein optisch, keine Bedienung geändert.

## 0.34.78 — 2026-11-01

- **Regler für die Ausgänge (#96):** Neben den Pegelreglern stehen zwei weitere senkrechte Fader im selben Stil, „Notebook“ (Wiedergabe Notebook, der Abhörausgang) und „Stream“ (Stream senden an woa-streamd), darunter je ein roter Stumm-Knopf „M“. Der Gain je Ausgang wird im Audio-Thread geglättet, lockfrei und ohne Allokation; der Mitschnitt bekommt weiter die volle Sendung. Die Einstellung steht in `radio.json` (`out_monitor`, `out_stream`); alte Dateien laden mit 0 dB, nicht stumm. Tests: `output_faders_scale_monitor_and_stream_separately` und `output_levels_default_to_full_and_round_trip` in `crates/radio/tests/rt.rs`, `output_faders_have_level_and_mute` in `crates/gui/src/radio.rs`.

## 0.34.77 — 2026-10-31

- **Einheitliche Modulköpfe (#246):** Alle Kartenköpfe in Radio, Party, Fernbedienung, Bibliothek, Analyse, Export, Automation und Uhren-Editor („SENDE-PLAYLIST“, „CART-WALL“, „WUNSCHLISTE“ …) stehen jetzt einheitlich als `panel-label` des Design-Systems: Barlow Condensed Bold, 12 px, leicht gesperrt, `text-dim`. Bisher mischten sich Barlow 11,5 px fett, 10 px und `small`. Das Pianoroll nimmt für Noten im Dunkeln die Spurfarbe 1 statt eines eigenen Blaus. Neu in `theme.rs`: `panel_label()` und `danger_text()` (Fehlertext mit WCAG-AA-Kontrast). Rein optisch, keine Bedienung geändert.

## 0.34.76 — 2026-10-31

- **`next_id` beim Rückgängig (#243):** geprüft und als gewollt dokumentiert. Der Zähler steigt nur: Eine neue Id wird schon vor dem Befehl vergeben, und Rückgängig senkt den Zähler nicht. So bekommt kein neues Objekt die Id einer rückgängig gemachten Spur, und ein Agent mit einer alten Id trifft nicht stillschweigend etwas anderes. Kommentar an `Project::next_id`, Test `undo_never_reuses_an_id`.

## 0.34.75 — 2026-10-31

- **Fenstertitel (#243):** Heißt das Projekt noch „Neues Projekt“ oder hat es keinen Namen, zeigt der Titel nach dem Speichern oder Öffnen den Dateinamen des Projekts (ohne `.woa`). Ein selbst vergebener Name bleibt wie bisher.

## 0.34.74 — 2026-10-31

- **Playlist: Griff und Nummer eintippen (#92):** Vor der Nummer jeder Zeile der Sende-Playlist steht ein Griff-Symbol (sechs Punkte); gezogen wird wie bisher die ganze Zeile (#166). Ein Klick auf die Nummer macht sie zum Eingabefeld: neue Nummer eintippen, Enter verschiebt den Titel dorthin (zu groß: ans Ende), Esc oder Wegklicken lässt alles, wie es ist. Pfeile, Entfernen, Doppelklick und Kontextmenü bleiben unverändert. Test: `typed_position` in `crates/gui/src/radio.rs`.

## 0.34.73 — 2026-10-31

- **BOM vor der ersten Zeile (#243):** Die stdio-Weiterleitung an die offene App (`woa-headless mcp` bei laufender App) verwirft ein führendes UTF-8-BOM, wie es etwa der StreamWriter von .NET schreibt; vorher ging `initialize` still verloren. Der Headless-stdio-Modus ohne App vertrug es schon, ein Test hält das jetzt fest (`crates/agent/tests/mcp.rs`).

## 0.34.72 — 2026-10-31

- **QR-Code zum Spenden (#231):** Im Fenster „Über WOA“ steht unter den Knöpfen PayPal und Ko-fi ein QR-Code, der den PayPal-Link kodiert; mit dem Handy scannen genügt. Lokal erzeugt mit demselben Baustein wie der Code der Fernbedienung, ohne neue Abhängigkeit.

## 0.34.71 — 2026-10-31

### Behoben

- **ON AIR nur bei Sendung (#225):** Die ON-AIR-Pille oben rechts leuchtete auch, wenn nur das Mikro offen war. Sie leuchtet jetzt nur noch bei „Auf Sendung“ (Stream läuft); das Mikro hat seine eigene Anzeige am Mikro-Knopf.

## 0.34.70 — 2026-10-31

- **Spur exportieren (#229):** Im Kontextmenü des Spurkopfs (Produzent) steht „Spur exportieren …“. Ein kleines Fenster fragt Dateiname (Vorgabe: Spurname), Format WAV oder MP3 sowie Titel, Interpret und Kommentar. Die Datei enthält nur diese Spur, genau wie ihr Stem (nach Fader und Pan, mit Plugins, ohne Master). Die Tags stehen in WAV als `LIST/INFO`, in MP3 als ID3v2.3; MP3 nutzt den eingebauten Encoder des Mixdown-Exports. Gerechnet und geschrieben wird in einem eigenen Thread, die Oberfläche bleibt bedienbar; die Statuszeile meldet das Ende. Eine stumme Spur meldet „kein Ton“.

## 0.34.69 — 2026-10-31

- **Ducking auch bei Carts (#149):** Spielt ein Cart, wird die Musik in den Playern auf die eingestellte Ducking-Tiefe abgesenkt und bleibt hörbar; nach dem Cart fährt sie sofort (80 ms Glättung) zurück. Abschaltbar mit dem Häkchen „auch bei Carts“ neben dem Ducking-Regler, Vorgabe an, gespeichert in `radio.json` (Feld `cart_duck`).

## 0.34.68 — 2026-10-31

- **Auto-Weiter abschaltbar (#148):** Im Kopf der Sende-Playlist steht neben „Gespielte ausblenden“ der Umschalter „Auto-Weiter“ (Vorgabe an, wie bisher). Aus: Nach jedem Titel hält die Playlist an, der nächste Titel steht als Cue bereit und startet mit „Play“; „Weiter“ springt sofort zum nächsten. Ausschalten mitten im Titel verwirft den schon bereitgelegten nächsten, der laufende spielt zu Ende. Gespeichert mit der Sendung (`auto_next`, alte Dateien laden unverändert). Decks, Party und Stopp-Verhalten bleiben unverändert. Test: `crates/radio/tests/auto_next.rs` unter `assert_no_alloc`.

## 0.34.67 — 2026-10-31

- **Stopp ist zuerst Pause (#95):** Im Radio-Modus hält der erste Druck auf „Stopp“ am Sende-Player den Titel an der Stelle fest wie „Pause“; „Play“ macht dort weiter. Der zweite Druck stoppt wie bisher. Mit „Stopp blendet aus“ blendet schon der erste Druck über die eingestellte Ausblenddauer aus. Gilt auch für die per MIDI gelernte Stopp-Taste; Decks, Party und Handy bleiben unverändert. Hinweis beim Darüberfahren. Test: `crates/radio/tests/stop_pauses.rs` unter `assert_no_alloc`.

## 0.34.66 — 2026-10-31

- **KI live ohne Token (#232):** Die App startet den Live-Server jetzt bei jedem Start selbst, auch wenn kein Zugriffstoken erzeugt wurde. Sie nutzt dann einen eigenen Schlüssel je Start (nur 127.0.0.1, Origin-Prüfung, Lock-Datei 0600, nie angezeigt). `woa-headless mcp` reicht so an das offene Fenster durch, statt daneben auf der Datei zu arbeiten. Kann der Server nicht starten, steht „KI aus“ in der Kopfleiste.
- **Kopierte Einträge:** kein Platzhalter `<Projektordner>` mehr; ohne gespeichertes Projekt kommt ein Hinweis. Claude Desktop bekommt nur den Eintrag `"woa": {…}` mit dem Hinweis, wohin er gehört; Claude Code nur `claude mcp add …`.
- **KI-Chat:** Ein Satz unter dem Chat sagt, dass er selbst die API des gewählten Anbieters aufruft, nicht Claude Code.
- **Weiterleitung an die App repariert:** `woa-headless mcp` wartete auf das Ende der SSE-Antwort; hält die App den Strom offen (gesehen unter Windows), bekam `initialize` nie eine Antwort. Jetzt liest die Weiterleitung den Strom zeilenweise und reicht die Antwort sofort durch; die Session-Id bleibt erhalten. Test mit offenem Strom dazu.
- **Ehrliche Hinweise:** Der HTTP-Server (fester Port) arbeitet nur auf der Datei, live geht es über Stdio; `docs/agent.md` und `docs/abnahme.md` (Anzeige in der Kopfleiste) nachgezogen.

## 0.34.65 — 2026-10-31

### Neu

- **Stopp mit Ausblenden (#227):** Im Radio-Modus steht bei Crossfade und Ducking der Schalter „Stopp blendet aus“ mit der „Ausblenddauer“ (0,5–10 s, Vorgabe 3 s). Ist er an, blenden Stopp am Sende-Player und Stopp an den Decks A/B/C über diese Dauer aus, etwa um mitten im Titel auszusteigen; aus bleibt der Stopp hart wie bisher. Kein neuer Knopf, Pause und Deck-Übergaben bleiben unverändert. Schalter und Dauer werden mit der Sendung gespeichert (Test unter `assert_no_alloc`).

## 0.34.64 — 2026-10-31

- **CI stabiler:** Drei Windows-Tests schlugen zufällig fehl (3 von 15 Läufen auf main). Die Titelinfos-Tests warten jetzt bis zu 10 s auf den Thread, der die Tags liest, statt fest 5 Bilder. Der WebRTC-Test lässt am Pult -16 statt -14 dB zu; gemessen wurden -14,02 dB.

## 0.34.63 — 2026-10-31

- **README neu:** zeigt, was WOA kann (Radio, Produzent, Party, KI über MCP), wo es den Download gibt und wer dahinter steht (André Hinz, creaVisions). Der veraltete Satz „noch nichts lauffähig“ ist raus.

## 0.34.62 — 2026-10-31

### Geändert

- **KI einfach einrichten (#217):** „Einstellungen → KI …“ öffnet vorn die Seite „KI einrichten“ mit einem Dropdown „Anbieter“: Claude Desktop / Claude Code (MCP), Anthropic API (eigener Schlüssel), Ollama lokal, OpenAI, OpenRouter, LM Studio; „Claude-Abo direkt im Chat“ steht ausgegraut als „kommt später“. Darunter nur die Felder, die der Anbieter braucht: Client, oder Modell (mit „Modelle holen“), Schlüssel und Adresse. Für MCP gibt es „JSON kopieren“ bzw. „Befehl kopieren“ mit drei Sätzen Anleitung, der Rohtext ist eingeklappt; das offene Projekt steht schon drin. Die Wahl im Dropdown wird sofort gespeichert. Die bisherigen Reiter stehen unter „Erweitert“.
- **KI-Chat im Studio-Dock:** Der Produzent hat neben Studio-Mixer, Piano-Roll und Automation den Reiter „KI-Chat“ mit dem gewählten Anbieter. Ohne eingerichtete KI steht dort ein Satz und der Link „Einstellungen → KI öffnen“; bei Claude Desktop/Code ein Hinweis, dass man dort spricht. Ändern sich Einstellungen oder Projekt, beginnt das Gespräch neu.
- **Schlüsselbund nicht mehr im UI-Thread:** Ob ein Schlüssel liegt, Speichern, Modelle holen und der Start des Chats lesen den Schlüsselbund im Hintergrund. Der Schlüssel erscheint weder im Fenster noch im Protokoll (Tests mit Fake-Speicher, ohne echte KI).

## 0.34.61 — 2026-10-31

### Geändert

- **Logs:** Eine kurze Startzeile mit Version, Build, Betriebssystem, Grafik-Backend, Adapter und Treiber; die lange wgpu-Adapterliste steht nur noch auf DEBUG (Test).
- **Logs:** Die Zwei-Minuten-Kennzahlen stehen in Datei und Protokoll als eine Zeile `ui summary`; der Upload bekommt sie weiter einzeln (Test).
- **Logs:** Die Unterlauf-Minutenzeile kommt nur noch bei Unterläufen; WASAPI-GUIDs in Gerätenamen sind gekürzt (Test).
- **Logs:** symphonia „using xing header“ fällt weg, „reached probe limit“ steht nicht mehr im Protokoll (das Laden meldet den Fehler selbst). Bei „clip/clap load failed“ und „plugin scan problem“ stehen Dateiname und Fehlertext nur noch in der lokalen Senke, im Ticket nur die Meldung (Test).
- **Logs:** Bedienaktionen als Info-Zeilen: Modus-Wechsel, Playlist und Deck A/B/C Start/Pause/Stop, Cartwall, PFL, Mitschnitt, Sendung Start und Ende. Datei- und Gerätenamen nur lokal, im Ticket nur Deck oder Slot (Test).
- **Ticket-Protokoll:** Zeitstempel mit Datum und Zeitzone (`2026-10-31 14:05:09 +01:00`); lokale Zeilen mit Dateinamen gehen nicht mehr ins Ticket (Test).

## 0.34.60 — 2026-10-31

### Behoben

- **Leere Kästchen statt Pfeilen (#218):** Hoch/Runter in „Verwalten“ sind jetzt Phosphor-Symbole statt „↑/↓“, die in Barlow fehlten. JetBrains Mono (liegt schon bei) dient als Rückfallschrift für Pfeile wie „→“ und „↩“ in allen Textfamilien. Vollbreiten-Zeichen in Titeln („｜“, „（“) werden in „Verwalten“ schmal angezeigt. Test: jedes Nicht-ASCII-Zeichen im GUI-Quelltext hat eine Glyphe.
- **Spurkopf ragte in die Zeitleiste (#212):** Die Eingangswahl bekommt nur noch den Platz neben Gain und Pan und kürzt lange Gerätenamen; was trotzdem zu breit ist, wird am Spurkopf abgeschnitten. Test: der Spurkopf endet vor Takt 1, auch mit langem Gerätenamen.

## 0.34.59 — 2026-10-31

### Neu

- **Gespielte ausblenden (#215):** Neben dem Titelzähler der Sende-Playlist schaltet ein kleines Auge-Symbol die gespielten Titel (alle vor dem laufenden bzw. nächsten) aus der Liste. Sie werden nur ausgeblendet, nicht gelöscht; der Zähler zeigt dann „offen, gespielt“. „bis Playlist-Ende“ zählt nur die offenen Titel. Die Einstellung gilt je Sendung und steht in radio.json (Vorgabe aus). Tests: Restzeit nur der offenen Titel, Speichern und Laden.
- **LED-Meter an den Fadern (#216):** Neben jedem der vier Fader (Playlist, Carts, Decks, Mikro) steht ein schmales senkrechtes LED-Meter nach dem Fader, dazu eine fünfte Spalte „Master“ ohne Fader: der Sendemix, der wirklich rausgeht. Grün bis -18 dBFS, Gelb bis -6, Rot darüber, Blau unter -40 („zu leise“); die Spitze hält 1,5 s. Der Audio-Thread schreibt nur drei weitere Spitzenwerte in Atomics, ohne Lock. Tests: Farbstufen, Spitze halten, fünf Meter vorhanden, Pegel je Zweig.

## 0.34.58 — 2026-10-31

### Behoben

- **Aussetzer im Sendestream:** Der Puffer für den Sendemix in woa-streamd hatte nur 0,1 s Vorlauf. Fiel er unter 20 ms, gab es 0,1 s Stille; stieg er über 0,5 s, wurden 0,4 s Ton weggeschnitten. Zittern der Lieferung (Feeder alle 20 ms, Treiberpuffer, alter Windows-Rechner) und die Uhrendifferenz zwischen Soundkarte und Systemuhr lösten das regelmäßig aus: Am 30.10. waren es 28 Unterläufe in 36 Minuten (`streamd-status.json`). Neu ist ein Jitter-Puffer mit 0,5 s Zielstand. Verlässt der Stand das Band von ±0,1 s, wird je 20-ms-Block ein einzelner Frame weggelassen oder wiederholt. Das gleicht bis etwa 1100 ppm Uhrendifferenz aus und kostet praktisch keine Rechenzeit. Hart gekürzt wird nur noch über 2 s. Tests: 10 Minuten Fake-Quelle mit ±1000 ppm, Häppchen von 20 bis 100 ms und 200-ms-Hängern ergeben keine Lücke und keinen Sprung; mit den alten Werten scheitern dieselben Tests.
- **Streamd:** Meldet sich die Oberfläche neu an, wird der Puffer nicht mehr geleert. Das Umrechnen auf die Stream-Rate läuft außerhalb der Sperre, die der Sende-Takt braucht. Unterläufe und Kürzungen stehen jetzt im Log („sendemix buffer gap“ mit Pufferstand, höchstens alle 10 s), und die Kürzungen zählt `live_trims` in `streamd-status.json`.
- **Abriss der Verbindung zu woa-streamd:** Der Feeder verwarf den Sendemix, solange keine Verbindung stand. Jetzt hält er bis zu 0,4 s zurück, versucht nach einem Schreibfehler sofort neu zu verbinden und schreibt „streamd link restored“ mit der Ausfallzeit ins Log. Unter „Auf Sendung“ steht „Verbindung zu woa-streamd unterbrochen, verbinde neu …“, danach, wie oft neu verbunden wurde und wie viele Lücken es gab (Test: echter Feeder unter Last mit Verbindungsabriss ergibt keinen Unterlauf).
- **Radio-Ausgabe:** Ohne Angabe in `config.json` läuft sie jetzt mit einem festen Puffer von 1024 Frames statt mit der Treibervorgabe. Lehnt das Gerät den Puffer ab, öffnet sie mit der Treibervorgabe, und eine Warnung steht im Log. Unter Windows entscheidet weiterhin der Treiber.

## 0.34.57 — 2026-10-31

- **Playlist umbenennen hielt fest:** Das Namensfeld im Fenster „Verwalten“ holte sich in jedem Bild den Fokus zurück, man kam nicht mehr heraus (André, 31.10.). Wie bei den Spuren (0.33.3) wird der Fokus nur beim Öffnen gesetzt; Enter oder ein Klick daneben übernimmt den Namen, Esc verwirft ihn. Test: Klick daneben schließt das Feld und benennt um.

## 0.34.56 — 2026-10-31

### Geändert

- **Kopfleiste aus einem Guss:** MIDI, ON AIR, REC, KI, Modus-Umschalter und Feedback sind jetzt gleich hohe Pillen (24 px) mit gleicher Schrift, Symbolgröße, Radius und Innenabstand (Design-System-Tokens). Abstände aus den Tokens: 16 px in der Gruppe, 24 px zwischen Status, Modus und Feedback. Rot nur für ON AIR, wenn live; der aktive Modus in der Akzentfarbe, auch während der Sendung. Unter 760 px freier Breite zeigen die Pillen nur ihr Symbol, das Wort steht im Tooltip (Tests bei 1280 und 900 px).
- **REC in der Kopfleiste:** Der Mitschnitt hat einen Knopf für alle Modi (Radio, Party, Produzent) und leuchtet rot, solange er läuft; der Tooltip nennt Dauer und Ziel. Ein Moduswechsel bricht ihn nicht ab. Der alte Mitschnitt-Knopf im Sendepult entfällt (Tests).
- **KI-Anzeige** („KI bereit“/„KI verbunden“) wandert aus der Statusleiste in die Status-Gruppe der Kopfleiste.

## 0.34.55 — 2026-10-31

### Geändert

- **Feedback (#211):** Der Haken „Protokoll mitsenden“ ist jetzt bei jedem Öffnen an (abwählbar). Nach Bildschirmfoto oder eigenem Bild erscheint eine kleine Vorschau (160 px) mit „× Entfernen“. Strg+Enter schickt mit den gesetzten Haken ab wie „Abschicken“ (Tooltip). Tests mit Fake-Server.

### Behoben

- **Vorhören (#205):** In Playlist, Cartwall und den Decks A/B/C ließ sich das Vorhören nicht über den Knopf beenden. Hört man dieselbe Quelle vor, heißt der Knopf jetzt „⏹ Vorhören stoppen“ (hervorgehoben) und stoppt beim Klick; eine andere Quelle wechselt das Vorhören dorthin (Test).

## 0.34.54 — 2026-10-31

### Behoben

- **Telemetrie:** Ohne Netz versuchte der Upload ab 100 gepufferten Ereignissen bei jedem neuen Ereignis eine HTTP-Anfrage. Nach einem Fehlschlag wartet er jetzt bis zum nächsten Intervall (Test).
- **Telemetrie:** Der volle Upload-Puffer verwarf das älteste Ereignis mit `Vec::remove(0)` (2000 Elemente verschieben); jetzt `VecDeque` (Test).
- **Telemetrie:** `proc.rss_mb` rechnete fest mit 4-KiB-Seiten; jetzt `VmRSS` aus `/proc/self/status` (Test).
- **Geheimnisse:** Fiel das Speichern auf die Ersatzdatei zurück, verdeckte ein alter Schlüsselbund-Eintrag beim Lesen den neuen Wert. Der alte Eintrag wird jetzt vorher entfernt (Test).

## 0.34.53 — 2026-10-31

### Behoben

- **Agent (MCP):** `analyze`, `spectrogram_png`, `export` und `build_playlist` renderten bzw. maßen synchron im einzigen Thread des MCP-Servers und hielten so alle anderen Anfragen an. Die Arbeit läuft jetzt im Blocking-Pool von tokio.
- **Assistent:** Scheiterte ein Chat-Zug nach ausgeführten Werkzeugen (z. B. LLM-Fehler in Runde 2), löschte er auch diese Aufrufe aus dem Verlauf, obwohl sie das Projekt schon geändert hatten. Die vollständigen Runden bleiben jetzt, dazu eine Abbruch-Notiz (Test).

## 0.34.52 — 2026-10-31

### Behoben

- **Stream-Uplink:** Hörerzahl- und Titelabfrage liefen synchron im Sende-Thread und hielten das Senden bis zu 15 s an. Beides läuft jetzt in einem eigenen Thread.
- **Streamd:** Das Umrechnen von UI- auf Stream-Rate begann je Häppchen neu (Drift, Knackser). Jetzt ein Resampler mit Zustand je Verbindung (Test: 60 s 48k→44,1k, Frame-Bilanz ±1).
- **Streamd:** Nach Standby oder Hänger wurde der ganze Rückstand auf einmal kodiert und gesendet. Das Aufholen ist jetzt auf 1 s begrenzt, ausgelassene Blöcke werden geloggt (Test).
- **Stream-Uplink:** Ein Pufferüberlauf schnitt den MP3-Strom an beliebiger Byte-Grenze; jetzt wird bis zum nächsten Sync-Wort verworfen (Test).

## 0.34.51 — 2026-10-31

- **Wunschtitel-Ordner:** Neue Dateien aus dem Wunschordner heißen in der Party-Wunschliste jetzt „Interpret – Titel“ aus den Tags. Fehlt Interpret oder Titel oder ist die Datei nicht lesbar, bleibt es beim Dateinamen ohne Endung. Die Tags werden nur gelesen, nicht dekodiert. Vorhandene Wünsche bleiben, wie sie sind.

## 0.34.50 — 2026-10-31

### Neu

- **KI live in der App:** Läuft WOA, arbeitet Claude auf dem Projekt, das in der App offen ist. Neue Spuren und Clips erscheinen sofort, und jeder Werkzeugaufruf steht als „Agent: <werkzeug>“ im Rückgängig-Verlauf (Bearbeiten → Rückgängig). Die App hostet dafür einen MCP-Server nur auf 127.0.0.1, mit dem vorhandenen Zugriffstoken und der Origin-Prüfung; der Port steht in `mcp-live.json` im Datenordner (ohne Token). `woa-headless mcp` (so startet Claude Desktop) erkennt die laufende App und reicht durch, sonst arbeitet es wie bisher headless. In Claude Desktop ist nichts umzustellen. Die Werkzeuge laufen zwischen zwei Bildern im GUI-Thread auf derselben Sitzung, die Wiedergabe läuft weiter. `save` speichert nur in den Ordner der App. Unten rechts steht „KI bereit“ bzw. „KI verbunden“. Sicherheit: Die Lock-Datei (Modus 0600) enthält statt des Tokens einen Live-Schlüssel, der je App-Start neu entsteht. Durchgereicht wird nur mit diesem Schlüssel und nur, wenn der Prozess der App noch lebt. Wird das Token im KI-Fenster neu erzeugt oder gelöscht, startet der Server neu bzw. endet, und das alte Token gilt nicht mehr.
- **Fenster „Produktionen“:** Knopf „Produktionen“ in der Clip-Leiste des Produzenten-Modus. Er zeigt alle Audio-Dateien des Projektordners (`aufnahmen/`, Exporte, von Claude erzeugte Dateien), die neuesten oben, mit Länge und Alter. Mit dem Kopfhörer-Knopf hörst du vor (PFL wie in der Bibliothek). Ziehst du eine Datei auf eine Spur, liegt sie an dieser Stelle. Gelesen wird beim Öffnen, auf „Neu einlesen“ und nach jedem Agent-Schritt, sonst nicht.

### Tests

- Test für den Lock-Weiterleiter liest die Anfrage vollständig, bevor er antwortet (unter Windows sonst Verbindungsabbruch statt Antwort). Formatierung.

## 0.34.49 — 2026-10-31

### Behoben

- **Rückgängig:** Der Verlauf war unbegrenzt, und jedes Rückgängig/Wiederholen spielt eine Kopie des ganzen Projekts nach. Die GUI-Sitzung hält jetzt höchstens 500 Schritte (Test).
- **Stundenuhr:** Das Speichern lief über einen festen Temp-Namen ohne fsync. Jetzt über `write_atomic` (eigene Temp-Datei, fsync, fsync des Ordners); eine untergeschobene Datei am alten Temp-Namen bleibt unberührt (Test).
- **Bibliothek:** Bei geänderter Datei am gleichen Pfad blieben Titel und Suchindex alt. Beides folgt jetzt den neuen Tags (Test).
- **AutoDJ:** Eine Kategorie wurde still bei 5000 Titeln (alphabetisch) abgeschnitten. Sie lädt jetzt alle Titel des Tags (Test).
- **Analyse:** Je Titel wurden Analyse und Dauer in zwei Autocommits geschrieben; jetzt in einer Transaktion.

## 0.34.48 — 2026-10-31

Party mit echter Zwei-Deck-Blende (André: „ja“ zu freier Blende von Hand mit Schalter „Auto-Übergang“ und zu Wunschtitel-Ordner → Party-Wunschliste).

### Neu

- **Echte Zwei-Deck-Wiedergabe:** Links und rechts sind zwei eigene Abspieler im Sendemixer (neben den Radio-Decks A bis C, die unberührt bleiben). Beide spielen gleichzeitig im Musikzweig, also auf Programm, Mitschnitt und Stream, und hören auf den Regler „Playlist“ (Lauter/Leiser in der Party). Der Crossfader steuert stufenlos eine Constant-Power-Kurve ohne Lautstärkeloch in der Mitte (beide −3 dB), mit exakten Endpunkten. Der Mixer fährt die Stellung sample-genau und linear nach (Handbewegung höchstens 10 ms für den ganzen Weg), darum knackt nichts. Keine Allokation im Audio-Thread.
- **Schalter „Auto-Übergang“** (Symbol und Wort, neben dem Crossfader, Vorgabe an, wird mit der Sendung gespeichert): An heißt, eine Crossfade-Länge vor dem Ende startet das andere Deck und der Fader fährt hinüber. Aus heißt, der DJ startet jedes Deck mit dessen eigenem „Play“ und blendet von Hand. Ein Deck, das endet, lädt den nächsten Titel vor und startet nicht.
- **Die Hand gewinnt:** Wer während der automatischen Fahrt den Crossfader anfasst, hält ihn; die Automatik fährt nicht dagegen.
- **Wunschordner → Party-Wunschliste (#134):** Neue, fertig kopierte Dateien im Wunschtitel-Ordner erscheinen als Wünsche und lassen sich direkt einreihen. Je Datei nur einmal; ein entfernter Wunsch kommt nicht wieder, und die Datei wird nie gelöscht. Es bleibt beim einen Ordner-Beobachter.

### Geändert

- **Crossfader über die Mitte ist kein „Weiter“ mehr:** Er blendet nur. „Weiter“ blendet sofort über die Crossfade-Länge zum anderen Deck.
- **Playlist bleibt das Modell:** „läuft“ ist das lautere Deck, „nächster“ das vorgeladene; Handy und Radio-Ansicht zeigen das. Beim Verlassen der Party läuft der hörbare Titel an derselben Stelle im Playlist-Abspieler weiter.
- Die alte, zeitgesteuerte Deck-Anzeige ist entfernt; Decks, Plattenteller und Fader lesen den Zustand des Mixers.

### Tests

- Neue Tests ohne Ton (`crates/radio/tests/party_duo.rs`, Fader in `level.rs`, Wunschordner in `party.rs` und der GUI): Constant Power mit exakten Enden, Glättung ohne Sprung, Auto an und aus, Hand schlägt Automatik, Mitte ist kein Weiter, Ende lädt vor ohne Start, Ein- und Austritt ohne Lücke, jeder Mixer-Block unter `assert_no_alloc`.

## 0.34.47 — 2026-10-31

### Behoben

- **Hall:** Auf einer Mono-Spur lief der Hall mono, selbst bei Mix 100 % (Korrelation 1). Die Kette einer Mono-Spur läuft jetzt ab dem ersten Hall in Stereo, die versetzten Kammfilter wirken auf beide Seiten (Test: Korrelation < 0,9). Gilt für GUI, Export und Agent, alle bauen denselben Graphen.
- **Agent:** `generate_notes` und `generate_drums` ließen ihre WAV in `aufnahmen/` liegen, wenn das Projekt nie gespeichert wurde. Beim Beenden entfernt der Agent jetzt die von ihm selbst erzeugten Takes, auf die weder das gespeicherte Projekt noch die Autosicherung verweist, und nur, wenn niemand sonst (GUI) die Sitzung hält. Während der Sitzung wird nichts gelöscht, Undo bleibt möglich (Tests).
- **Agent:** Pfade in den Antworten (`export`, `spectrogram_png`, `save` u. a.) tragen unter Windows nicht mehr die Vorsilbe `\\?\` (Test der Hilfsfunktion).
- **Agent:** `undo` und `redo` nennen jetzt den eben ausgeführten Schritt (`undone` bzw. `redone`) und beide nächsten Schritte (`undo`, `redo`); die bisherigen Felder bleiben (Test).
- **Agent:** `analyze` führt MIDI-Spuren in der Liste der Spurpegel (Test).

## 0.34.46 — 2026-10-31

Fehlerbehebung nach dem Test unter Windows (André: „bibliothek noch langsam. selbst bei 4 tracks“, „titelinfos ändert geht nicht. egal wo ich doppelt klicke“, „rechtsklick funktioniert auch nicht“).

- **Doppelklick öffnet die Titelinfos zuverlässig:** egui erkannte einen Doppelklick nur bei höchstens 0,3 s zwischen den Klicks (Windows erlaubt 0,5 s), und wer einen Titel erst anklickte und kurz darauf doppelklickte, erzeugte für egui einen Dreifachklick, der nichts tat. Jetzt gelten 0,5 s, und auch der Dreifachklick öffnet das Fenster. Getestet über die echte Oberfläche (Studio und Radio).
- **Überall gleich:** In der Bibliothek und im Reiter Verwalten → Titel öffnen ein Doppelklick und der Rechtsklick → „Titelinfos bearbeiten …“ das Fenster „Titelinfos“. Rechtsklick wählt dort zuerst den Titel. In der Radio-Playlist setzt ein Doppelklick schon „als Nächstes“, deshalb gibt es die Titelinfos dort nur im Kontextmenü. Liegt der Titel nicht in der Bibliothek, steht das als Hinweis in der Bibliothek.
- **Analyse etwa dreimal schneller:** Drei Viertel der Analysezeit gingen in die True-Peak-Messung. Sie rechnet das Filter jetzt nur dort, wo eine neue Spitze überhaupt möglich ist, und das Ergebnis bleibt exakt gleich. Gemessen im Release-Build (`examples/analyse_zeit.rs`): ein 3:17-Titel 0,9 s → 0,27 s, ein 5:21-Titel 1,5 s → 0,53 s.
- **Tests:** `crates/gui/tests/titelinfos.rs` klickt in der echten App (Doppelklick schnell, langsam, nach vorherigem Klick; Kontextmenü); `true_peak_skip_is_exact` vergleicht mit dem vollen Filter.
- **Analyse nur noch einen Titel nach dem anderen, unter 16 Threads:** Mit zwei gleichzeitig dauerte ein Dekodieren auf dem Notebook 1,3–2,8 s statt 0,3–0,4 s.

## 0.34.45 — 2026-10-31

Gäste im Browser: Moderator-Funktionen (André: „so das man die person auch einfach mal muten kann und vielleicht man ja auch mehrere hat“).

- **Warteraum:** Ein angenommener Browser-Gast kommt erst in den Warteraum. Er hört die Sendung (Mix-Minus), ist aber nicht zu hören, bis der Moderator ihn mit „Warteraum → Auf Sendung“ holt. Testgäste und Dateigäste sind wie bisher sofort auf Sendung.
- **Je Gast eine Zeile:** Name, Pegel vor dem Fader (zeigt auch im Warteraum und stumm, ob der Gast spricht), „Auf Sendung“/„Warteraum“, „Stumm“ (Symbol und Wort, hört weiter die Sendung), Lautstärke wie der Kanal-Gain (ziehen, Doppelklick = 0 dB), Rückweg abhören, „Trennen“ mit Bestätigung (zweiter Klick binnen 3 s). Darunter Leitung gut/mäßig/schlecht mit RTT und Verlust.
- **Mehrere Gäste:** bis zu 8 gleichzeitig (wie bisher), jeder mit eigenem Rückweg ohne sich selbst.
- **Browser-Seite des Gastes:** großes Schild „Auf Sendung“, „Stumm“ oder „Warteraum“, dazu „Mikro stumm schalten“. Selbst-Stumm schaltet das Mikrofon im Browser ab und meldet es dem Moderator („hat sich selbst stumm geschaltet“). Jede Seite steuert nur ihr eigenes Ticket; Einladung, PIN und Zugang sind unverändert.
- **Log ohne Gastnamen:** Gastnamen stehen nicht mehr im Log (nur Kanalnummern).
- **Tests:** Warteraum, Stumm, Lautstärke, Trennen, Mix-Minus mit zwei und vier Gästen, Selbst-Stumm nur mit eigenem angenommenem Ticket, Sendestand über HTTPS im Loopback-Test.

## 0.34.44 — 2026-10-31

Party mit zwei Decks (André: „als dj auf einer party die man streamt, fehlt bei party das zweite deck … und alle symbole die steuern einheitlich bitte mit symbol und wort“).

### Neu

- **Zwei Decks links und rechts:** Die Sende-Playlist bleibt das Modell. Jeder Titelwechsel legt den neuen Titel auf die andere Seite; auf der freien Seite steht der nächste Titel als „VORGELADEN“. Keine zweite Wiedergabe: es spielt weiter der Sendemixer mit Crossfade und Pause.
- **Crossfader:** Beim Übergang wandert er über die eingestellte Crossfade-Länge von einem Deck zum anderen. Von Hand über die Mitte gezogen heißt das „Weiter“ zum anderen Deck.
- **Plattenteller:** Er dreht sich nur am Deck, das gerade zu hören ist (beim Überblenden kurz beide). Pausiert oder vorgeladen steht er still.
- **Playlist in der Party:** Die aktive Playlist mit Auswahl, „läuft“ und „nächster“. Zeilen lassen sich zum Umordnen ziehen, Treffer der Bibliothekssuche lassen sich hineinziehen. Verwaltet wird weiter im Fenster „Verwalten“.
- **Bibliothekssuche in der Party:** Ein Suchfeld mit Treffern zum Ziehen oder „→ Playlist“ (hinter den laufenden Titel). Es nutzt dieselbe Suche wie das Fenster „Bibliothek“.
- **Wunschliste:** Neben der Playlist. „+ Wunsch“ nimmt freien Text an („Wer: Titel“). Je Wunsch gibt es „Suchen“ (füllt die Suche vor), „→ Playlist“, „Erledigt“ (bleibt durchgestrichen stehen) und „Entfernen“. Ein über „→ Playlist“ eingereihter Wunsch wird erledigt, sobald sein Titel läuft. Die Liste wird gespeichert (`party_wishes.json` im Datenordner). Wünsche vom Handy landen in derselben Liste; das Handy zeigt nur offene.
- **Einheitliche Steuerknöpfe:** Alle Party-Knöpfe haben jetzt Symbol und Wort, eine Höhe (44 px) und eine Mindestbreite (neuer Baustein `widgets::ctl_key`). Play leuchtet grün (`ok`), Pause bernstein (`warn`), in Hell und Dunkel.

## 0.34.43 — 2026-10-31

Decks und Playlist greifen ineinander, steuerbar und ohne Automatik (André: „nur wenn der aktiv ist, dann spielt mit fading die playlist weiter … denn das deck darf das nicht automatisch machen, falls man vorhören will“).

- **„↩ Playlist“ je Deck:** Umschalter am Deck, Vorgabe aus, wird mit der Sendung gespeichert (`deck_return` in radio.json). An: In den letzten Sekunden des Decks (Länge der eingestellten Crossfade-Blende) blendet das Deck aus und die Playlist ein. War die Playlist pausiert, geht es an der Pausenstelle weiter, sonst mit „Start“ ab dem Cue-Titel (nach Stopp der Titel, der zuletzt lief). Aus: Nach dem Deck bleibt es still, zum Reden oder Telefonieren. Läuft die Playlist schon, passiert nichts.
- **Deck-Knöpfe im Playlist-Player** („▶ Deck A/B/C“ neben Stopp): Ein Klick startet das Deck und löst die Playlist ab. Die Playlist blendet über den eingestellten Crossfade aus und pausiert, das Deck blendet ein. Umschalt+Klick oder Rechtsklick „Sofort (ohne Blende)“: harter Schnitt. Ein leeres Deck hat einen ausgegrauten Knopf, der Tooltip sagt warum.
- **Nichts startet von selbst:** Laden aufs Deck startet nie etwas, Vorhören bleibt getrennt. Start am Deck selbst läuft wie bisher parallel zur Playlist.
- **Ereignis „Deck fertig“:** Der Mixer zählt je Deck einen Zähler hoch, wenn es bis Cue-Out durchläuft (Atomic, keine Allokation im Audio-Thread). Der Controller schaltet daran und an der Restzeit die Rückkehr.
- **Senkrechte Pegelregler neben den Decks:** Playlist, Carts, Decks und Mikro als Fader-Gruppe, gleiches Widget wie der Kanal-Gain der Spuren (dB-Skala, Doppelklick = 0 dB, Rechtsklick = MIDI lernen). Die waagerechten Regler unter dem Rack sind weg. Neu ist der Regler „Decks“ (alle drei Decks zusammen, auch per MIDI als `decks` lernbar).
- **Cart-Restzeit:** Ein laufendes Cart-Pad zeigt groß rechts unten den Countdown („-12 s“, unter 10 s „-9,4 s“). Der Titel rückt dafür zusammen und bleibt zweizeilig.
- Tests ohne Ton (CI-fest formatiert): `radio/tests/deck_handover.rs` (Blende mit Pegelkurve, Stille ohne Umschalter, harter Schnitt, Laden startet nichts, Deck-Start bleibt parallel, leeres Deck), Oberflächentests für Deck-Knöpfe (Klick, Umschalt+Klick, leer), Umschalter, Fader (senkrecht, setzen, Doppelklick) und Cart-Countdown.

## 0.34.42 — 2026-10-31

### Behoben

- **Vorhören (PFL):** Die Wahl unter Einstellungen → Vorhören (PFL) wirkte nicht, solange etwas lief (auch nur die Playlist), und nichts sagte das. Jetzt merkt sich WOA die Wahl, zeigt im Menü „Wirkt, sobald nichts mehr läuft“ und öffnet die Ausgabe automatisch neu, sobald die Wiedergabe steht. Die laufende Sendung wird nie unterbrochen. Läuft nichts, greift die Wahl sofort.
- **Vorhören (PFL):** Fehler beim Öffnen des Kopfhörer-Ausgangs (zu wenige Kanäle, Gerät geht nicht, Kanalzahl nicht lesbar) stehen jetzt im Menü, im Protokoll und als Tooltip an den deaktivierten Vorhören-Knöpfen von Deck, Cart und Playlist. Ist das Vorhören aus, sagt der Tooltip „Vorhören ist aus: Einstellungen → Vorhören (PFL)“.

## 0.34.41 — 2026-10-31

### Behoben

- **Bibliothek:** Nach dem Wechsel des Datenordners blieb ein Fehler des alten Ordners stehen, die neue Datenbank wurde nie geöffnet. Der Fehler wird jetzt beim Wechsel zurückgesetzt (Test).
- **Stream-Test:** Solange der Verbindungstest lief, zeichnete das Fenster jeden Frame neu. Jetzt weckt nur der Test-Thread das Fenster, wenn er fertig ist.
- **Stream-Test:** Das gespeicherte Kennwort wurde im UI-Thread aus dem Schlüsselbund gelesen; das geschieht jetzt im Test-Thread.
- **KI-Fenster:** Ein Prüffehler beim Speichern im Reiter „Assistent“ war unsichtbar; er steht jetzt unter dem Speichern-Knopf.

## 0.34.40 — 2026-10-31

### Behoben

- **Neu und Öffnen:** Strg+N, Strg+O, „Datei → Neu“ und „DAWproject öffnen“ verwarfen ungespeicherte Änderungen ohne Rückfrage. Jetzt wird vorher gespeichert; bleibt das Projekt ungespeichert, bricht der Schritt mit einer Statusmeldung ab (Test mit Strg+N).
- **Radio:** Das Speichern von `radio.json` überschrieb eine kurz vorher von außen (Agenten-Werkzeug `build_playlist`) geschriebene Datei. Vor dem Speichern wird jetzt der Dateistand geprüft; bei fremder Änderung wird sie übernommen und der Konflikt gemeldet.
- **Tastenkürzel:** Strg+X, Strg+C/V, Strg+D, Leertaste, Pos1, S und Strg+E wirkten hinter offenen Dialogen (Über, Export, Feedback); nur Entf beachtete sie. Jetzt gilt die Prüfung für alle (kittest: Strg+X bei offenem Export).
- **Verwalten → Titel:** Die Liste zeichnete bis zu 500 Zeilen je Frame und suchte die Auswahl linear je Zeile und je Frame. Jetzt nur die sichtbaren Zeilen (`show_rows`), die Auswahl als Menge, das Bereinigen der Auswahl nur nach einer neuen Suche.
- **Verwalten → Titel:** Der Anker für Umschalt-Klick war ein Index und zeigte nach einer neuen Suche auf einen anderen Titel. Er ist jetzt die Track-ID (Test).
- **Protokoll:** Das Fenster holte das ganze Journal in jedem Frame und baute für jede sichtbare Zeile den Hover-Text. Jetzt höchstens einmal je Sekunde (oder bei Filterwechsel), der Hover-Text erst beim Hover.

## 0.34.39 — 2026-10-31

### Behoben

- **Mitschnitt:** Verlorene Samples (Warteschlange voll) wurden gezählt, aber nie gemeldet. Die Steuerung prüft den Zähler jetzt bei jedem Takt, schreibt eine Warnung ins Log und zeigt „Mitschnitt: n ms verloren“.
- **Sendung neu laden:** Stand dieselbe Datei mehrfach in der Playlist, bekamen alle Einträge dieselbe Laufzeit-Kennung. Jede alte Kennung wird jetzt höchstens einmal vergeben (Test mit doppeltem Pfad).
- **Sendung neu laden:** Jede Änderung an Decks oder Carts lud alle Decks und Carts neu und unterbrach ein laufendes Deck. Jetzt werden nur die Slots neu geladen, deren Eintrag sich geändert hat.

## 0.34.38 — 2026-10-31

### Behoben

- **Rauschunterdrückung:** Nach einem verspäteten Block des Arbeits-Threads blieb das Nasssignal dauerhaft einen Block hinter dem Trockensignal. Verspätete Ergebnisse werden jetzt verworfen, bis Lesen und Schreiben wieder im Gleichschritt sind (Test mit künstlich verzögertem Arbeits-Thread).
- **Aufnahme:** Ein Schreibfehler (z. B. Platte voll) wurde erst beim Stoppen gemeldet, bis dahin lief die Aufnahme scheinbar weiter. Der Recorder meldet ihn jetzt über `writer_failed()`, die Oberfläche beendet die Aufnahme sofort und zeigt den Fehler.

## 0.34.37 — 2026-10-31

Titelinfos bearbeiten, direkt in WOA (André: „Tippfehler und andere Feinheiten gleich in der App verbessern … möglichst ohne Wiedergabe zu unterbrechen“).

- **Radio → Verwalten → Titel:** links die Bibliothek mit Suche und Mehrfachauswahl (Klick, Strg-Klick, Umschalt-Klick), rechts die Titelinfos der Auswahl, „+“ legt weiter in die Playlist.
- **Bibliothek → Doppelklick auf einen Titel** oder Rechtsklick → „Titelinfos bearbeiten …“: Titel, Interpret, Album, Jahr, Genre, Tracknummer, Kommentar und das Cover (anzeigen, „Bild …“ ersetzt, „Entfernen“). Enter oder „Speichern“ schreibt in die Datei, „Rückgängig“ schreibt den Stand davor zurück.
- **Mehrere Titel auf einmal** (im Reiter „Titel“): Felder mit verschiedenen Werten stehen leer mit „(verschieden, bleibt)“ und werden nur gesetzt, wenn man sie ändert. Der Editor ist ein eigenes Bauteil (`tag_editor.rs`), Fenster und Reiter teilen ihn.
- **Formate:** MP3 (ID3v2), FLAC, OGG Vorbis, M4A und WAV, mit `lofty`. Opus-Dateien schreibt der Editor auch, WOA importiert sie aber noch nicht (kein Opus-Decoder).
- **Speichern ohne Unterbrechung:** WOA schreibt in eine neu angelegte Kopie mit zufälligem Namen im selben Ordner (folgt keinem fremden Symlink, überschreibt nichts; ein Symlink auf eine Musikdatei bearbeitet bewusst deren Ziel), bringt sie auf die Platte und ersetzt die Datei dann in einem Schritt; eine halb geschriebene Datei gibt es nie. Die Wiedergabe läuft aus dem Speicher weiter. Alles im Hintergrund, die Oberfläche wartet nicht.
- **Audiodaten bleiben bitgenau**, nur die Tag-Blöcke ändern sich. Die Bibliothek übernimmt den neuen Inhalt samt Analyse, ohne neu zu messen; Suche und Liste zeigen den neuen Titel sofort.
- **Fehler stehen im Fenster:** „schreibgeschützt“ (Datei oder Ordner), „gesperrt“ (ein anderes Programm hält die Datei unter Windows; auch „Zugriff verweigert“ beim Ersetzen zählt als Sperre). Gesperrte Dateien versucht WOA selbst alle 10 Sekunden wieder, auch bei geschlossenem Fenster, mit sichtbarem Hinweis. Ungültiges Jahr oder Tracknummer und Bilder, die kein JPEG/PNG sind, werden vor dem Schreiben abgelehnt.
- **Analyse auf alten Rechnern:** zwei Titel gleichzeitig erst ab acht Threads (ein Zweikerner mit Hyperthreading meldet vier und misst nun einen).
- **Grenze:** Die Sendeliste des Radios misst einen geänderten Titel einmal neu (sie erkennt Änderungen an Größe und Zeitstempel), im Hintergrund.
- **Tests:** alle Felder und Cover in allen sechs Formaten schreiben und lesen, Cover ersetzen und entfernen, Teiländerung, Audio bitgenau (dekodiert verglichen, außer Opus), schreibgeschützte Datei, Fehlernummern für gesperrt, Speichern bei offener Datei, Bibliothek ohne neue Analyse, zwei Titel gemeinsam setzen und Rückgängig, falsches Jahr wird nicht geschrieben, Symlink mit vorhersagbarem Namen bleibt unberührt. Kleine Testdateien liegen unter `crates/library/tests/data/`.

## 0.34.36 — 2026-10-31

Bibliothek: Ordner hinzufügen geht wieder schnell (André: „es dauert gerade wieder ewig“).

- **Ursache, gemessen:** Der Import dekodierte jede Datei ganz, nur um ihre Länge zu kennen, eine nach der anderen; eingetragen wurde erst je 50 Dateien. Danach dekodierte die Analyse jede Datei ein zweites Mal, und sie begann erst, wenn der Import fertig war. Telemetrie 0.34.34: `decode_file` im Median 0,5 s, höchstens 111,6 s. Am Notebook (Release-Bau, 5-Minuten-MP3): Kopf lesen 0,24 ms, ganz dekodieren 259 ms, Analyse 1,06 s.
- **Import liest nur noch den Datei-Kopf** (Tags und Länge, mit `lofty`, MIT/Apache-2.0). Titel heißen „Interpret - Titel“ aus den Tags, sonst wie bisher nach dem Dateinamen. Neue Titel stehen spätestens nach einer Viertelsekunde in der Liste, statt erst nach 50 dekodierten Dateien. 1000 Testdateien: 0,2 s.
- **Analyse (Lautheit, Cue-Punkte, Tempo, Tonart) startet schon während des Imports**, im Hintergrund, mit Fortschritt und Abbruch wie bisher. Sie misst höchstens zwei Titel gleichzeitig, auf Rechnern mit weniger als vier Kernen einen; jeder dekodierte Titel liegt ganz im Speicher. Sie trägt auch die genaue Länge nach, wenn der Kopf keine nennt.
- **Produzent:** Ein Titel aus der Bibliothek oder vom Desktop auf die Timeline gezogen wird nicht mehr zusätzlich im UI-Thread dekodiert, nur um seine Länge zu kennen. Das Laden des Clips für die Wiedergabe dekodiert weiterhin einmal im UI-Thread (offen).
- **Tests:** Import von 40 Dateien füllt die Liste ohne ein einziges Dekodieren (Zähler in `woa-engine`), Längen stimmen aus dem Kopf, Analyse kommt danach; die bisherigen Import- und Analyse-Tests laufen unverändert.

## 0.34.35 — 2026-10-31

Radio-Studio: mehrere Playlisten und ein Fenster „Verwalten“ (André: „playlisten verwalten, löschen, neu, anders … das muss einfach gehen“).

- **Mehrere benannte Playlisten je Sendung.** Im Kopf der Sende-Playlist steht nur eine Auswahl der aktiven Playlist und „⋯“ (Neue Playlist, Playlisten verwalten …).
- **Fenster „Verwalten“** mit zwei Reitern. **Playlisten:** links die Listen (Neu, M3U importieren; Rechtsklick oder „⋯“: Aktiv setzen, Umbenennen, Duplizieren, M3U exportieren, Leeren, Löschen; Doppelklick benennt um), rechts die Titel der gewählten Liste (↑/↓, ×, „+ Titel …“, Entf nimmt nur den markierten Titel heraus). **Titel:** zeigt die Bibliothek mit „+“ in die gewählte Playlist; hier hängt später der Tag-Editor ein (`crate::titel_tab::titel_tab`).
- **Löschen mit Rückgängig:** Hinweis unten 8 s mit ablaufendem Strich, wie beim Löschen einer Spur.
- **Auf Sendung:** Die laufende Playlist lässt sich nicht löschen (Knopf gesperrt mit Grund). Ein Wechsel fragt nach; der laufende Titel, die Decks und Carts spielen weiter, danach geht es mit der neuen Playlist weiter.
- **Speichern** in `radio.json` (`playlists`, `active`); alte Dateien werden verlustfrei zu „Playlist 1“. Die aktive Playlist steht wie bisher in `playlist`, AutoDJ, Voice-Tracking, Crossfade, Notfall-Playlist und die MCP-Werkzeuge arbeiten unverändert auf ihr. `read_playlist` nennt zusätzlich Namen der aktiven und aller Playlisten.
- **M3U/M3U8:** Import (Kommentare übersprungen, relative Pfade ab dem Ordner der Datei, keine Netzadressen) und Export als M3U8 mit `#EXTINF`.
- **Tests:** neu, umbenennen (leer und doppelt abgelehnt), duplizieren, leeren, löschen und Rückgängig (auch die aktive und die letzte), Migration alter Dateien, M3U hin und zurück, Wechsel und Löschen während der Sendung ohne Abbruch, Rückfrage beim Wechsel auf Sendung.

## 0.34.34 — 2026-10-31

KI & MCP, Teil 2: ein eigener Assistent in WOA mit frei wählbarem Anbieter und beliebigen MCP-Servern als Werkzeugen (André: „irgendein Server mit irgendeinem MCP bzw. LLM“). Nichts ist auf einen bestimmten Server festgelegt.

- **Neuer Reiter „Assistent“** im Fenster Einstellungen → „KI & MCP …“. Anbieter anlegen: Format (Anthropic oder OpenAI-kompatibel), Adresse, Modellname (frei eintippbar, „Modelle holen“ füllt eine Auswahlliste, wenn der Server eine nennt), Schlüssel. Voreinstellungen zum Antippen: Anthropic, OpenAI, OpenRouter, Ollama und LM Studio (lokal); „Eigener Server“ beginnt leer (vLLM und jeder andere OpenAI-kompatible Server). Die Adresse darf mit oder ohne `/v1` enden.
- **Schlüssel** liegen im Schlüsselbund des Systems (Eintrag `woa / ki-<Kennung>`), sonst in einer Datei mit Modus 0600; `config.json` kennt nur die Kennung. Im Feld sind sie maskiert, in Fehlermeldungen und im Log stehen sie nie. Lokale Server brauchen keinen.
- **Externe MCP-Server als Werkzeuge:** Liste beliebig vieler Server, je lokal gestartet (Befehl, Argumente, Umgebung) oder über Adresse (HTTP, Kopfzeilen). Eine geheime Kopfzeile (z. B. `Authorization`) oder Umgebungsvariable liegt ebenfalls im Schlüsselbund. Einzeln ein- und ausschaltbar.
- **„Verbindung testen“** je Anbieter und je Server, im Hintergrund, mit einer Meldung in einem Satz: verbunden, Modell nicht in der Liste, Schlüssel abgelehnt, Adresse nicht erreichbar, Server nicht gestartet.
- **Chat-Fenster:** Einstellungen → „Assistent (Chat) …“. Das Modell ruft WOAs eigene Werkzeuge (über `woa-headless mcp` auf dem offenen Projektordner) und die eingetragenen Server auf, bis es antwortet; höchstens 10 Runden je Eingabe. Gleichnamige Werkzeuge verschiedener Server bekommen den Servernamen vorangestellt. Läuft im Hintergrund-Thread, die Oberfläche steht nie.
- **Grenze:** Wie in Teil 1 arbeiten WOAs Werkzeuge auf der Projektdatei im Ordner, nicht auf dem offenen Fenster. Der Chat spricht nicht gestreamt, die Antwort erscheint auf einmal.
- **Sicherheit, Kopierbefehl für Claude Code:** Pfade standen in doppelten Anführungszeichen; ein `$(…)` oder Backtick im Pfad wäre beim Einfügen in bash/zsh ausgeführt worden. Jetzt einfache Anführungszeichen (`'` als `'\''`), unter Windows die PowerShell-Schreibweise (`'` verdoppelt). Ein Pfad mit Zeilenumbruch bekommt keinen Terminal-Befehl mehr, nur die JSON-Einträge.
- **Stream-Server ohne Vorgabe:** Das Feld „Server“ ist bei neuen Installationen leer (mit Platzhalter), es steht kein bestimmter Server mehr im Code; gespeicherte Einträge bleiben. `woa-headless stream-test` braucht jetzt `--host`.
- **Kennwort nicht gespeichert:** Scheitert die Ablage im Schlüsselbund, öffnet sich das Fenster „Sendung: Stream-Server“ wieder und nennt den Grund, statt es nur ins Log zu schreiben.
- **Tests (mit HTTP-Mock):** beide Formate mit einer Werkzeug-Runde (Anthropic `tool_use`/`tool_result`, OpenAI `tool_calls`/`tool`), Schlüssel als `x-api-key` bzw. `Bearer` bzw. gar nicht, Adress-Normalisierung, Texte des Testknopfs, Server ohne Modellliste, MCP-Client über HTTP (JSON und Ereignisstrom) und Stdio, Chat-Schleife von Modell über Werkzeug zurück zum Modell, Fehler lassen den Verlauf sauber, Fenster zeichnen.

## 0.34.33 — 2026-10-31

Studio-Uhr, Quellen unter Windows und Cart-Wall (#172, #163, #97).

- **Studio-Uhr (#172):** Die Digitalzeit hatte eine feste Größe und war in DSEG7 breiter als der Kreis der Stundenpunkte; bei 3 und 9 Uhr lagen Ziffern und Punkte übereinander. Die Schrift wird jetzt gemessen und so weit verkleinert, dass rundum Abstand zu den Punkten bleibt. Sekunden-LEDs, Stundenpunkte und „STUDIO-UHR“ bleiben wie sie waren.
- **Quellen je Spur (#163):** Unter Windows und macOS stand nur „Standard-Eingang (Mikrofon)“ in der Liste, weil weitere Quellen nur über PipeWire (Linux) gesucht wurden. Jetzt stehen dort alle Eingabegeräte des Systems („Eingang: …“), die Spur zeigt den Gerätenamen. Der Hinweis „Programm spielt gerade keinen Ton?“ erscheint nur noch unter Linux, wo es Programme als Quelle gibt. Linux unverändert.
- **Cart-Wall (#97):** Belegte Pads sind freundlich grün, leere grau, das laufende leuchtet wie bisher in der Modusfarbe. Die Nummer steht groß vorn (fast padhoch), der Titel daneben auf höchstens zwei Zeilen, Rest mit „…“. Tastenschild jetzt rechts oben neben der LED; Spurfarben-LED und -Balken, Rechtsklick-Menü und Tasten unverändert.
- **Tests:** Digitalzeit hält Abstand zu den Stundenpunkten (mit echter Schrift gemessen, alte Größe als Gegenprobe), jedes Eingabegerät wird ein Eintrag, Pad-Farben je Zustand und zweizeiliger Titel (`woa-gui`).

## 0.34.32 — 2026-10-31

Feedback: Bildschirmfoto erst auf Klick, eigenes Bild anhängen, Protokoll auf Wunsch mitsenden (#173, #174).

- **Bildschirmfoto auf Klick (#173):** WOA fotografiert beim Öffnen des Feedback-Fensters nicht mehr. Wer „Bildschirmfoto mitsenden“ anhakt, sieht das Fenster kurz verschwinden, dann wird ohne Fenster aufgenommen. Haken weg: das Bild ist verworfen. Vorgabe: aus.
- **Eigenes Bild (#174):** „Eigenes Bild anhängen …“ öffnet die Dateiauswahl (PNG oder JPEG). Das Bild wird als PNG geschickt und bei Bedarf verkleinert, damit es unter der Server-Grenze von 3 MB bleibt; „Entfernen“ nimmt es wieder ab. Höchstens zwei Bilder je Meldung.
- **Protokoll mitsenden:** neuer Haken, Vorgabe aus. Er hängt die Zeilen des Protokoll-Fensters (Hilfe → Protokoll) unter den Text. Der Heimordner (`/home/<name>`, `C:\Users\<name>`) wird zu `~` gekürzt. Der Text samt Protokoll bleibt unter 20 000 Zeichen; bei mehr gehen die neuesten Zeilen mit. Unter dem Haken lässt sich das Protokoll vorher ansehen.
- **Was mitgeht:** Über „Abschicken“ steht, was gesendet wird (Text, Bilder, Protokoll, Version und System). Es geht nur mit, was angehakt oder angehängt ist.
- **Sendeweg unverändert:** `POST https://feedback.wironair.de/api/feedback`, Bilder im Feld `images`. `woa-headless feedback-test` nutzt dieselbe Funktion.
- **Tests:** Bildschirmfoto und eigenes Bild setzen und entfernen, Pfadkürzung, ohne Haken kein Protokoll, Längengrenze (`woa-gui`); mehrere Bilder und Obergrenze (`woa-telemetry`).

## 0.34.31 — 2026-10-31

KI & MCP, Teil 1: WOA als MCP-Server für beliebige Clients (André: „nicht nur mein Server muss funktionieren, sondern irgendein Server mit irgendeinem MCP bzw. LLM“). Teil 2 (0.34.34): der eigene Assistent.

- **Neues Fenster:** Einstellungen → „KI & MCP …“. Dort steht der Hintergrund-Server (an/aus), der Projektordner, wer ihn erreicht (nur dieser Rechner oder das Netz), der Port und das Zugriffstoken.
- **Einträge zum Kopieren:** fertig für Claude Code, Claude Desktop und andere Clients, je für Stdio (der Client startet `woa-headless`) und für HTTP. Sie entstehen zur Laufzeit aus dem Pfad des laufenden Programms und der Einstellung; im Fenster ist das Token maskiert, erst „Kopieren“ setzt es ein.
- **HTTP-Transport (`woa-headless mcp <Projekt> --http [--bind ADRESSE] [--port N]`):** Streamable HTTP auf `/mcp`. Jede Anfrage braucht `Authorization: Bearer <Token>` (Vergleich in gleichbleibender Zeit), ohne gültiges Token kommt 401. Vorgabe ist `127.0.0.1`; eine andere Adresse ist eine ausdrückliche Wahl und wird im Fenster gewarnt. `Host` (Loopback) und `Origin` (nur lokale Seiten) werden gegen DNS-Rebinding geprüft. Ohne Token startet der Server nicht.
- **Token:** 256 Bit aus dem Zufallsgenerator des Systems, im Schlüsselbund (Eintrag `woa / mcp-token`), sonst Datei `mcp-token.pass` mit Modus 0600; nie in `config.json`, nie auf der Kommandozeile, nie im Log. Der Hintergrund-Prozess liest es selbst. In Containern geht auch `WOA_MCP_TOKEN`.
- **Neue Crates:** `woa-secrets` (die Schlüsselbund-Ablage aus dem Stream-Kennwort, jetzt für beliebig viele Einträge mit stabiler Kennung; `woa-stream` nutzt sie unverändert weiter) und `woa-assistant` (Einstellungen, Einträge, Hintergrund-Prozess).
- **Grenze:** Der Server arbeitet auf der Projektdatei im Ordner, nicht auf dem offenen Fenster. Das steht im Fenster und in `docs/agent.md`.
- Tests: HTTP ohne/falsches/richtiges Token, falscher `Origin`, fremder Pfad, leeres Token; Einträge sind gültiges JSON und zeigen das Token nie maskiert; Pfade mit Leerzeichen; Schlüsselbund-Ersatzdatei mit 0600 und Kennung als Dateiname; Fenster zeichnet.

## 0.34.30 — 2026-10-31

Bibliothek: Analyse hängt nicht mehr an langen Titeln, läuft im Hintergrund weiter, Titel lassen sich entfernen und Spalten sortieren (#122, #123, #124, #120).

- **Lautheitsmessung linear (#122, #120):** Die integrierte Lautheit wurde mit jedem 100-ms-Schritt über alle bisherigen Blöcke neu gegatet, die Rechenzeit wuchs mit dem Quadrat der Länge. Ein 30-Minuten-Titel brauchte so sehr lange; zwei Stunden Material dauerten im Test 104 s, jetzt 0,6 s. Das traf die Bibliotheks-Analyse, das Messen beim Einfügen in die Playlist und das Laden zum Abspielen.
- **Abspielen ohne doppeltes Messen (#120):** Beim Laden eines Titels wird nur noch gemessen, wenn er noch keine Messung hat. Bisher lief die Messung im Control-Thread jedes Mal und wurde verworfen.
- **Weniger Speicher bei der Analyse:** Die dekodierte Datei wird nach dem Aufteilen in links/rechts freigegeben.
- **Hintergrund (#123):** Import und Analyse laufen weiter, wenn das Bibliotheks-Fenster zu ist; liegen beim Start noch unanalysierte Titel vor, beginnt die Analyse sofort.
- **Entfernen (#124):** Rechtsklick auf einen Titel → „Aus der Bibliothek entfernen“, oder Titel anklicken und Entf. Die Datei auf der Platte bleibt. Entf wirkt nur auf den markierten Eintrag und nie in einem Textfeld.
- **Sortieren (#124):** Klick auf einen Spaltenkopf (Titel, BPM, Tonart, LUFS, Art, Dauer) sortiert, ein zweiter Klick dreht die Richtung; Pfeil zeigt die aktive Spalte. Sortiert wird in der Datenbank, nicht nur in den angezeigten 500 Zeilen; Titel ohne Messwert stehen am Ende.
- **Tests:** Lautheit über zwei Stunden in linearer Zeit (`woa-dsp`), Entfernen und Sortieren (`woa-library`), geschlossenes Fenster importiert und analysiert, Entfernen und Sortieren über den Fensterzustand (`woa-gui`).

## 0.34.29 — 2026-10-31

Einstellungen: Stream-Server (#121). Unter Einstellungen gibt es jetzt „Sendung: Stream-Server und Kennwort …“, damit man ohne Dateien von Hand den Server und das Kennwort einträgt.

- **Server:** Typ (Shoutcast v1, Shoutcast v2 mit Stream-ID, Icecast 2), Name oder IP, Port, bei Icecast Mountpoint und Benutzer, Sendername, Bitrate und Abtastrate. In das Feld „Server“ darf man eine ganze Adresse wie `http://1.2.3.4:8000/live` einfügen; sie wird in Server, Port und Mountpoint zerlegt.
- **Kennwort:** liegt im Schlüsselbund des Systems (Windows: Anmeldeinformationsverwaltung, Linux: Secret Service, macOS: Schlüsselbund), nie in `config.json`, nie im Log. Gibt es keinen Schlüsselbund, landet es in einer Datei `stream-source.pass` im Datenordner mit Modus 0600. Im Feld ist es maskiert, „Anzeigen“ blendet es ein. Leer lassen heißt: behalten.
- **Verbindung testen:** meldet sich einmal am Server an (ohne Ton) und sagt in einem Satz, was los ist: erreichbar, Kennwort oder Benutzer falsch, Server spricht ein anderes Protokoll, Mountpoint belegt, Port zu. Während der Sendung ist der Knopf aus.
- **Port:** In `config.json` steht jetzt der Port des Servers (wie im Server eingestellt); Shoutcast meldet die Quelle auf `port + 1` an. Alte Dateien werden beim Laden einmal umgerechnet (`schema` 2; 8001 wird 8000). Eine bisher eingetragene Kennwortdatei wird beim Start in den Schlüsselbund übernommen und ausgetragen; die Datei selbst bleibt liegen.
- Die Vorgabe `password_file` ist leer. woa-streamd liest das Kennwort aus dem Schlüsselbund; die Kommandozeile (`stream-test`, `broadcast-test`, `--pass-file`) funktioniert wie bisher mit dem Quell-Port.
- Tests: Laden und Umrechnen alter Konfigurationen, Zerlegen von Server-Eingaben und Adress-Bau (IPv6), Prüfung typischer Tippfehler, Kennwort-Ablage (Schlüsselbund, Ersatzdatei mit 0600, alte Datei einmal übernehmen), Verbindungstest gegen Test-Server (Shoutcast ok / falsches Kennwort / Webserver, Icecast ok / 401 / belegt / kein HTTP, nicht erreichbar), Fenster zeichnet für jeden Servertyp.

## 0.34.28 — 2026-10-31

Radio-Fehler aus dem Feedback (#168, #169, #166, dazu #95).

- **Crossfade wieder erreichbar (#168):** Die Regler für Crossfade-Blende und Musik-Ducking waren nie entfernt, sie lagen unter dem Fensterrand. Ursache: Im Deck-Kasten nahm der Fortschrittsbalken die ganze Zeilenbreite, der Text „leer: Datei hierher ziehen“ brach daneben Zeichen für Zeichen um, jedes Deck wurde über 400 px hoch. Zusammen mit der Wunschtitel-Karte schob das Rack und Gäste aus dem Bild, und die rechte Spalte rollte nicht. Jetzt hat der Balken eine feste Breite, der Titel kürzt ab, und die rechte Spalte rollt.
- **Pause pausiert (#169, #95):** Pause war intern ein Stopp. Der Mixer warf den Titel weg, Play lud die ganze Datei neu und sprang dann an die Stelle. Bei langen Titeln (30 min) dauerte das, und jeder Klick wartete dahinter. Jetzt blendet Pause aus und hält den Titel samt dem nächsten an der Stelle fest. Play macht sofort dort weiter, ohne Neuladen. Stopp nach Pause verwirft den Titel, Start beginnt neu.
- **Ziehen in der Playlist (#166):** Eine Playlist-Zeile lässt sich auf Deck A, B oder C ziehen (lädt den Titel mit seinen Cue-Punkten) oder an eine andere Stelle der Playlist (verschiebt ihn). Aufs Fenster gezogene Dateien gehen nicht mehr verloren, wenn gerade ein Textfeld den Fokus hat.
- **Tests:** `crates/radio/tests/pause.rs` (neu, scheitert mit dem alten Code), `party.rs` angepasst, Ablageziel beim Ziehen in `crates/gui/src/radio.rs`.

## 0.34.27 — 2026-10-31

Design System Fassung 2 (Claude Design schließt die Lücken aus `docs/design/luecken.md`).

- **Doku:** Neues Paket unter `docs/design/woa-design-system/` und als ZIP, Herkunft und Phosphor-Lizenz in `docs/herkunft.md`, alle Lücken in `docs/design/luecken.md` als geschlossen markiert.
- **Auswahl:** eine Farbe an drei Stellen (Spurkopf, Kanalzug, Rack-Kopf: 14 % Spurfarbe, 1 px Rand), Zeitleiste der gewählten Spur 6 % getönt.
- **Spurkopf:** 64 statt 92 px, zwei Zeilen; Mithören steckt im Quellmenü. Pan steht vorerst auch hier (`HEADER_PAN`).
- **Spur löschen:** Hinweis „Spur „X“ gelöscht · Rückgängig (Strg+Z)“ für 8 s; Strg+Z holt die Spur samt Routing zurück. Im Rack wandert „Spur löschen“ ins ⋯-Menü (rot, zuletzt).
- **Insert-Rack:** Kopf mit Art und Menü, Leer-Zustand ohne Spur, Gruppen EINGANG (Quelle, Trim) · INSERTS · SENDS · AUSGANG, Effekt-Slots mit Power-Bypass und aufklappbaren Reglern, „+ Effekt“ mit Suche.
- **Mixer:** „fx n“ am Kanalzug (rot bei fehlendem oder abgestürztem Plugin), „+ Bus“ als Platzhalter statt Namensfeld.
- **Clip-Inspektor:** 44 px, Zustände leer, Audio, MIDI (Raster, Quantisieren, Velocity) und „n Clips“ mit „—“ für ungleiche Werte; Start und Länge als Takt.Schlag.Sechzehntel. Tooltips nach 0,5 s.
- **Nicht gebaut** (Engine kann es nicht): Effekte umsortieren, Umkehren, Loop, Stumm, Zusammenfügen, „lädt …“, Stereo-Pegel.
- **Tests:** `crates/gui/tests/design_system_2.rs` (Rückgängig-Hinweis, Auswahlfarbe, Inspektor-Zustände, Rack-Leer-Zustände).

## 0.34.26 — 2026-10-31

Entf löscht eine Spur nur, wenn sie sichtbar markiert ist (André 31.10.: „Entf darf nur aktiv sein wenn es sinnvoll was zu löschen gibt“).

- **Ein Zustand:** Hervorhebung in Spurkopf und Kanalzug und das Ziel für Entf sind jetzt dieselbe markierte Spur. Bisher blieb eine Spur nach einem Klick auf einen Clip oder die Zeitleiste farbig hervorgehoben, ohne Ziel für Entf zu sein, und ohne Auswahl war die erste Spur hervorgehoben. Jetzt ist nach einem solchen Klick keine Spur mehr hervorgehoben, und ohne Auswahl ist keine markiert (das Insert-Rack zeigt weiter die erste Spur).
- **Entf ruht**, solange ein Menü, Popup, der Feedback-, Export- oder Über-WOA-Dialog offen ist oder ein Textfeld den Fokus hat.
- **Tests:** fünf neue Fälle in `crates/gui/tests/design_regressions.rs`.

## 0.34.25 — 2026-10-31

Protokoll-Fenster: Meldungen wie „MP3 beschädigt“ sind lokal sichtbar, nicht in der Telemetrie (André 31.10.: „lokal darf sie solche meldungen ruhig anzeigen“).

- **Hilfe → Protokoll …:** Fenster mit Info, Warnungen und Fehlern der laufenden Sitzung, je Zeile Zeit, Klartext und, wo vorhanden, Dateiname (im Tooltip). Filter je Stufe, Knöpfe „Kopieren“ und „Logordner öffnen“. Gespeist aus einem Ringpuffer im Speicher (höchstens 2000 Zeilen); gleiche Meldungen werden eine Zeile mit Zähler („×1084“).
- **Beschädigte Dateien:** Beim Lesen zählt WOA je Datei die fehlerhaften Stellen (Frame-Warnungen des MP3-Dekoders und verworfene Pakete). Statt tausender Rohwarnungen steht im Protokoll eine Zeile: „Datei „X.mp3“ ist beschädigt (n fehlerhafte Stellen), spielt aber. Tipp: neu exportieren.“ bzw. „… ist nicht lesbar (Grund)“.
- **Warnsymbol:** In Playlist und Bibliothek bekommt ein solcher Titel ein kleines Warnsymbol mit Tooltip. Kein Pop-up, das eine Sendung stört.
- **Nur lokal:** Diese Zeilen tragen das Ziel `woa_lokal` und gehen nie in den Upload (eigener Filter der Upload-Senke). Die Telemetrie bleibt bei Code, Stufe und Zahl, gedrosselt auf 20 je Minute.
- Tests: Ringpuffer (Zusammenfassen, Obergrenze), Fehlerzähler je Thread, beschädigte und unlesbare MP3 aus dem Tempordner (kein Ton), Brückentest ohne Dateinamen auf der Leitung.

## 0.34.24 — 2026-10-31

Sicherheit #136: Updater Stufe B vorbereitet, es fehlt nur noch der Schlüssel.

- **Updater:** Neuer Test wacht über `packaging/windows/updater.pub`: Die Datei ist entweder der Platzhalter oder ein gültiger minisign-Schlüssel. Ein beschädigt eingefügter Schlüssel würde sonst stillschweigend auf die schwächere Prüfsummen-Stufe zurückfallen; jetzt bricht der Test (und damit die CI) ab.
- **Doku:** `docs/updater.md` führt die Schritte zum Einschalten von Stufe B einzeln auf. Der Schlüssel entsteht mit `minisign -G -W` (ohne Passwort, weil der Workflow unbeaufsichtigt signiert).
- **Updater:** Der Platzhalter gilt nur noch, wenn die Datei mit `PLATZHALTER:` beginnt. Ein Schlüssel, dem die Kommentarzeile fehlt, fällt jetzt im Test auf.

Nicht erledigt: Der Schlüssel selbst (nur André), #136 bleibt offen.

## 0.34.23 — 2026-10-31

Eine Auswahl, ein ruhiger Clip-Inspektor (André 31.10.: „mehr buttons sind nie die lösung … KISS“).

- **Eine gewählte Spur:** Ein Klick irgendwo in den Spurkopf oder in den Kanalzug des Mixers wählt die Spur. Beide zeigen sie in ihrer Spurfarbe (Fläche und Rand), das Insert-Rack zeigt immer diese Spur mit Farbpunkt und Name im Kopf (ohne Auswahl die erste). Scharf geschaltet zeigt nur noch der Aufnahmepunkt.
- **Weniger Knöpfe:** Der Knopf „Effekte n“ je Kanalzug ist weg, die Zahl steht klein als „fx n“ im Namensstreifen. Der Papierkorb steht einmal je Spur im Spurkopf; im Kanalzug öffnet ein Rechtsklick auf den Namen das Spurmenü. Die Hinweise „Kanal wählen …“ sind entfallen.
- **Entf:** löscht den gewählten Clip (wie bisher), und ohne gewählten Clip die zuletzt angeklickte Spur. Ein Klick in die Zeitleiste oder auf einen Clip nimmt die Spur wieder aus diesem Griff, damit Entf nie versehentlich eine Spur löscht.
- **Clip-Inspektor in einer Zeile:** Gruppen Fade (Symbole für Ein- und Ausblenden, Länge, Kurve kurz Lin/Pow/S), Gain (dB, Normalisieren als Symbol), Zeit (Dehnung %, Tonhöhe st, Tempo folgen als Metronom-Taste; die Symbole leuchten, wenn der Clip bearbeitet ist) und Bearbeiten (Schneiden, Duplizieren, Kopieren, Löschen als Symbole). Erklärungen und Tastenkürzel stehen im Tooltip, auch der lange Hinweis ohne gewählten Clip.
- Tests (`crates/gui/tests/design_regressions.rs`): Auswahl über Spurkopf und Kanalzug mit Rack, Effekte für jede Spur per Klick auf den Kanalzug, Spur löschen per Rechtsklick im Kanalzug, ein Papierkorb je Spur, Entf für Clip und Spur, Papierkorb im Inspektor.
- Neu: `docs/design/luecken.md`, was die Gestaltungsvorlage noch nicht zeigt.

## 0.34.22 — 2026-10-31

Kopfleiste aufgeräumt, Über-WOA persönlicher, Feedback-Fenster einladender (André 31.10.).

- **Menüs links wie in üblichen DAWs:** Datei, Bearbeiten, Ansicht, Einstellungen, Hilfe. Import und Export stehen als Untermenüs in Datei, „Leeren MIDI-Clip anlegen“ in Bearbeiten. Bibliothek, Stundenuhr, Zeit-Events und Erscheinungsbild liegen unter Ansicht, Update-Suche und „Über WOA“ unter Hilfe. Einstellungen sind nach Audio und MIDI gegliedert.
- **Update-Info in der Mitte der Kopfleiste**, nur wenn ein Update da ist; der eigene Streifen darunter entfällt.
- **Rechts mehr Abstand.** Die ON-AIR-Lampe sitzt jetzt neben MIDI statt im Kopf des Radio-Decks und leuchtet wie bisher, wenn die Sendung läuft oder das Mikro offen ist.
- **Feedback** ist eine grüne Taste ganz rechts, ohne „…“.
- **Über WOA:** Logo, „WOA – built with love, coffee and AI“, Spenden über PayPal und Ko-fi (dieselben Links wie in bretteins und fotobrett) und „Danke an alle Lizenzgeber“ mit rotem Herz. Die Lizenzliste bleibt.
- **Feedback-Fenster:** 600 statt 420 px breit, mittig, freundlicher Titel und Hinweis, größeres Textfeld, „Bildschirmfoto mitsenden“, grüne Taste „Abschicken“.
- Tests über die echte Oberfläche: `crates/gui/tests/kopfleiste.rs`.

## 0.34.21 — 2026-10-31

Sicherheit #127: AppImage-Werkzeuge werden vor dem Ausführen per SHA-256 geprüft.

- **Bau:** `packaging/linux/build-appimage.sh` kennt für linuxdeploy und appimagetool (feste Versionen, kein `continuous`) je eine SHA-256 und prüft sie vor `chmod +x`, auch bei bereits zwischengespeicherten Dateien. Eine falsche Prüfsumme bricht den Bau ab.

Closes #127.

## 0.34.20 — 2026-10-31

Windows-Notebook mit schwacher CPU: Tausende Dekoder-Warnungen und ein Hänger beim Hinzufügen von Titeln.

- **Messen nacheinander:** Neue Titel werden in einem einzigen Hintergrund-Thread gemessen. Bisher bekam jede Datei einen eigenen Thread, und elf Titel auf einmal dekodierten elf Threads parallel. Auf einem alten Notebook blieb der Oberfläche dann keine CPU mehr.
- **MP3-Rauschen still:** Der MP3-Dekoder (symphonia) meldet bei beschädigten oder angeschnittenen MP3s für jeden Frame „invalid main_data_begin“ und spielt trotzdem weiter. Diese Warnung geht nicht mehr in Logdatei und Telemetrie, Fehler weiterhin schon.
- **Drosselung:** Gleiche Warnungen und Fehler ohne eigenen Code gehen höchstens 20-mal je Minute in die Telemetrie. Wie viele verworfen wurden, steht danach als `log.dropped`.
- **Herkunft:** Meldungen aus Abhängigkeiten (über das `log`-Crate) tragen in der Telemetrie jetzt den Crate-Namen (z. B. `symphonia_core`) statt `log`, dazu Dateiname und Zeile im Code-Ort. Text und Pfade bleiben weiterhin lokal.
- Tests: Drosselung, begrenzte Tabelle, Crate-Name statt Modulpfad, Brücke vom `log`-Crate mit 1084 Warnungen ohne Dateinamen auf der Leitung.

## 0.34.19 — 2026-10-31

Design-Nachbesserung: Was D1–D6 unerreichbar gemacht hatten, ist wieder da (André 31.10.: „Spur entfernen ist wichtig“, „Effekte sind weg im Studio-Mixer“).

- **Spur löschen per Rechtsklick:** Rechtsklick auf den Spurkopf (Hintergrund oder Name) öffnet das Spurmenü mit Umbenennen, Inhalt anlegen und Spur löschen, für Audio-, MIDI- und Bus-Spuren. Das Menü hing am Rahmen, der nur Hover meldet, und ging deshalb nie auf.
- **Sichtbarer Papierkorb:** im Spurkopf rechts und wieder in jedem Kanalzug des Studio-Mixers (vor dem Design war er dort, D5 hatte ihn ins Insert-Rack verschoben).
- **Effekte und Plugins:** stehen im Insert-Rack jetzt oben, direkt unter dem Kanalnamen, wie im Entwurf ScreenStudio. Vorher lagen „+ Effekt“ und „+ Plugin“ unter dem sichtbaren Rand des Docks. Ohne Auswahl zeigt das Rack den ersten Kanal, und jeder Kanalzug hat einen Knopf „Effekte n“, der seinen Kanal ins Rack holt.
- Bus-Spuren zeigen im Spurmenü kein „Audio-Datei importieren“ mehr.
- Tests über die echte Oberfläche ohne Fenster (`crates/gui/tests/design_regressions.rs`): Rechtsklick auf Spurkopf und Name → Spur löschen für alle drei Spurarten, Papierkorb im Spurkopf und im Kanalzug, „+ Effekt“ ohne Scrollen sichtbar und für jede Spur ein Effekt angelegt.

## 0.34.18 — 2026-10-31

Sicherheit #119: Gast-Rückweg wird geleert, Steuerzeichen im Gastnamen fliegen raus.

- **Rückweg:** Ein neuer Netz-Gast beginnt mit leerem Rückweg und hört nichts mehr vom vorherigen Gast (geleert beim Anlegen und beim Zurückgeben).
- **Gastname:** Steuerzeichen (Zeilenumbruch, ANSI-Sequenzen) werden vor dem Kürzen auf 24 Zeichen entfernt, damit sie weder im Log noch in der Anzeige landen. Bleibt nichts übrig, heißt der Gast "Gast N".
- Tests: neuer Gast beginnt mit leerem Rückweg, Name mit Zeilenumbruch und Escape-Zeichen kommt sauber an.

## 0.34.17 — 2026-10-31

Sicherheit #116: DJ-PIN-Sperre ließ sich per Gast-Login umgehen, Gäste konnten DJ-Sitzungen verdrängen.

- **Sperre:** nur noch eine erfolgreiche DJ-Anmeldung setzt den Fehlerzähler einer Adresse zurück. Eine Gast-Anmeldung tut es nicht mehr; die DJ-PIN lässt sich also nicht mehr zwischen Gast-Logins ohne Sperre durchprobieren.
- **Alle Adressen zusammen:** 50 falsche PINs innerhalb von 10 Minuten erneuern automatisch die DJ-PIN (Anzeige im Fenster zieht nach, laufende Sitzungen bleiben).
- **Längere DJ-PIN:** acht statt sechs Stellen. Die Gast-PIN im QR-Code bleibt sechsstellig.
- **Sitzungen:** Gäste und DJs haben je eigene Obergrenzen (256 Gäste, 32 DJs); verdrängt wird nur innerhalb derselben Rolle, ein Gast wirft nie einen DJ hinaus. Eine Adresse hält je Rolle höchstens 16 Sitzungen.
- Tests: Gast-Login setzt die DJ-Sperre nicht zurück, Gäste verdrängen keine DJ-Sitzung, Obergrenzen je Rolle und Adresse, globale Erneuerung der DJ-PIN.

Closes #116.

## 0.34.16 — 2026-10-31

Sicherheit #112: Plugin-Suche und gemeinsamer Speicher nur noch für den eigenen Nutzer.

- **Suchergebnis:** der Suchprozess schreibt in ein neu angelegtes privates Verzeichnis (Modus 0700, nicht vorhersehbarer Name) statt in eine feste Datei im gemeinsamen Temp-Ordner; das Verzeichnis wird danach gelöscht. Untergeschobene Dateien oder Symlinks sind damit wirkungslos.
- **Gemeinsamer Speicher:** die Datei zwischen Oberfläche und Plugin-Prozess wird unter Linux mit Modus 0600 angelegt; andere Nutzer können kein Audio mehr mitlesen.
- Tests belegen 0600 und 0700. Der Windows-Bau bleibt warnungsfrei.

Closes #112.

## 0.34.15 — 2026-10-31

Design D1–D6: gesammelte Abnahme in `docs/abnahme.md` (Thema Hell/Dunkel, Schriften, Icons, Bausteine, Mixer, Logo).

## 0.34.14 — 2026-10-31

Design D6: neues App-Logo.

- **App-Icon:** die Kachel aus dem Designsystem ersetzt das alte Icon (Linux: SVG und PNG für AppImage, Flatpak und `.DirIcon`; Windows: neue `woa.ico` für Startmenü-Verknüpfung und den Eintrag unter „Apps“ im MSI).
- **Fenstericon:** die App setzt das Icon selbst (Fensterleiste, Taskleiste, Alt-Tab).
- **Kopfleiste:** statt der Schrift „WOA“ steht die Bildmarke, die fünf Säulen in der Modusfarbe (Produzent, Radio, Party; Hell und Dunkel), die ON-AIR-Lampe immer rot.
- Noch offen: das Icon in der `woa.exe` selbst (Explorer, Datei-Symbol) braucht einen Build-Schritt mit zusätzlicher Abhängigkeit und ist nicht enthalten.

## 0.34.13 — 2026-10-31

Design D5: Kanalzug ohne Drehregler.

- **Kanalzug (Produzent, Mischpult):** nur noch Scribble-Strip mit dem Namen in Spurfarbe, die Tasten M, S und Aufnahme, die LED-Kette und der Fader. Pegel-Zahlenfeld, Panorama, Ausgang, Sends, Effektkette und Plugins sind aus dem Kanalzug heraus.
- **Insert-Rack:** neues Feld rechts neben den Kanalzügen für den gewählten Kanal (Klick auf den Namen im Kanalzug). Dort liegen Pegel (Gain), Panorama, Ausgang, Sends, Effektkette, Plugins und „Spur löschen“. Die Funktion ist unverändert, auch die Automation von Gain und Pan.
- Noch offen: eine PFL-Taste am Kanalzug (der Vorhörweg ist bisher nur als Ausgabe-Einstellung da) und ein eigener EQ-Regler (es gibt keinen eingebauten Kanal-EQ, er läuft als Effekt in der Kette).

## 0.34.12 — 2026-10-31

Design D4: Bausteine des Design Systems, eingesetzt in den drei Layouts.

- **Neue Bausteine** in `crates/gui/src/widgets/`: beleuchtete Taste (`Key`: erhaben, eingerastet, aktiv, voll an), Fader (senkrecht als Kanalfader, waagerecht für die Radio-Pegel, Doppelklick setzt zurück), VU-Meter, Studio-Uhr, ON-AIR-Lampe, Cart-Pad und Jogwheel.
- **Pegelansicht (Produzent):** Neue Einstellung „Pegelansicht“ unter „Erscheinungsbild“: Analog (VU-Zeiger mit hinterleuchtetem Skalenblatt), Modern (Balken mit Messwert) oder Aus. Gespeichert in `config.json` als `vu_view`, Vorgabe Analog. Gezeigt wird in der Transportleiste der höchste Pegel der sichtbaren Spuren (noch kein echter Summenpegel, kein Peak-Hold).
- **Radio:** Studio-Uhr mit 60 Sekunden-LEDs neben dem Cover, ON-AIR-Lampe in der Deck-Kopfzeile (leuchtet bei Sendung oder offenem Mikro), Cart-Wall mit den neuen Pads (das laufende Pad leuchtet in der Modusfarbe und blinkt), Pegel der Zweige als waagerechte Fader.
- **Produzent:** Kanaltasten M, S und Aufnahme als beleuchtete Tasten, dazu ein Kanalfader neben der LED-Kette (gleicher Gain wie das Zahlenfeld).
- **Party:** Jogwheel im Now-Playing-LCD, es dreht sich, solange ein Titel läuft; Großtasten als beleuchtete Tasten.
- **Bewegung:** Anzeigen stehen still, wenn die egui-Animationen aus sind (`animation_time` 0) oder `WOA_REDUCED_MOTION=1` gesetzt ist.
- **Für Entwickler:** LED-Schwellen (−12 und −3 dB) liegen nur noch in `widgets::vu::zone_for_db`; Zeigerwinkel, Zeigerträgheit, Uhr-LEDs, Jog-Drehzahl und Fader-Abbildung sind einzeln getestet.

## 0.34.11 — 2026-10-31

Design D3: Icons statt Emojis.

- **Phosphor-Icons:** `egui-phosphor` (MIT) ist eingebunden. Alle Emojis und Sonderzeichen-Symbole der Oberfläche (Transport, Radio-Konsole, Menüs, Knöpfe, Bibliothek, Studio-Dock, Spurliste) sind durch Phosphor-Icons ersetzt.
- **Transport im Fill-Stil:** Play, Pause, Stopp, Zurück, Weiter, Wiederholen, Zufall, Neustart und Aufnahme sind gefüllte Symbole, alle übrigen Icons im Regular-Stil.
- **Aufnahmeknopf:** Der selbst gezeichnete rote Punkt entfällt; das Aufnahme-Symbol gehört nun zur Beschriftung.
- **Für Entwickler:** `theme::ic` (Regular), `theme::icf` (Fill), `theme::fill_label` und `theme::fill_icon`; Fill liegt als eigene Schriftfamilie `icon-fill`.

## 0.34.10 — 2026-10-31

Design D2: Schriften des Design Systems (Barlow, Barlow Condensed, JetBrains Mono, DSEG7 Classic).

- **Eingebettete Schriften:** TTF-Dateien in `crates/gui/assets/fonts/` mit den Lizenztexten (SIL OFL 1.1), Quellen mit Commit in `docs/herkunft.md`. Barlow ist die Oberflächenschrift, JetBrains Mono die Festbreitenschrift; Barlow Condensed (`label`) und DSEG7 Classic (`seg`) sind eigene Familien (`theme::label_font`, `theme::seg_font`).
- **Typo-Skala:** Fließtext und Tasten 14 px, klein 12 px, Titel 22 px (bisher 13,5 / 11 / 16).
- **Sichtbar:** Uhrzeit und Restzeit im Radio-Modus als Leuchtziffern (DSEG7), Statuspillen in Barlow Condensed.
- **Neu:** Einstellungsleiste „Über WOA …“ mit Version, Lizenz und dem Hinweis auf die SIL Open Font License der Schriften.

## 0.34.9 — 2026-10-31

Silvestersendung Teil 2, Paket 4 von 4 (#134): Wunschtitel-Ordner. Damit ist #134 vollständig.

- **Wunschtitel:** Neue Karte „WUNSCHTITEL“ unter den Decks. Ein beobachteter Ordner (Vorgabe: `Wunschtitel` im Datenordner der Sendung, „Ordner wählen …“ für einen anderen, gespeichert in `config.json` als `wish_folder`). Neue Audiodateien stehen innerhalb einer Sekunde oben in der Liste, neue fett mit „neu“; eine Datei, die noch kopiert wird, ist gesperrt („wird kopiert …“).
- **Ein Klick:** „In die Playlist“ reiht den Titel nach dem laufenden ein, „A“, „B“, „C“ lädt ihn auf das Deck. Kein Spotify, nur Dateien; nichts wird verschoben oder gelöscht.
- Ein Faden liest den Ordner jede Sekunde (`read_dir`, keine neue Abhängigkeit, kaum Last auf dem alten Laptop). Tests offline (`crates/gui/src/wishes.rs`). Abnahme: `docs/abnahme.md`, „Silvester Teil 2, Paket 4“.

## 0.34.8 — 2026-10-31

Silvestersendung Teil 2, Paket 3 von 4 (#134): Telefon mit Rückweg.

- **Telefon-Eingang:** Einstellungen, „Telefon (Eingang für den Anrufer) …“: Gerät und Kanalpaar. Nie das Standardmikrofon; ohne Wahl gibt es kein Telefon. Fehlt das Gerät, steht der Name in der Meldung und die Sendung läuft weiter. Gespeichert in `config.json` (`phone`).
- **„Telefon auf Sendung“:** großer Knopf neben dem Mikro, Pegelbalken auch vor dem Auflegen auf die Sendung. Der Anrufer liegt dann im Programm (Aufnahme und Stream), die Musik wird wie beim Mikro abgesenkt.
- **Rückweg (Mix-Minus):** neue Quelle „Telefon-Rückweg“ unter „Ausgänge (Soundkarten)“, Gerät und Kanalpaar frei (Karte 1). Er trägt alles, was auf Sendung ist, nur nicht den Anrufer. Ohne Wahl gibt es keinen Rückweg und nie eine Ausgabe im Hauptausgang; ein Paar, das schon eine andere Quelle trägt, wird abgelehnt.
- **Zwei Karten, ein Takt:** Der Eingang kommt über dieselbe Taktverbindung wie die weiteren Ausgänge (`woa_engine::pairs::link`, leichter Hermite-Umrechner, 50 ms Polster): andere Rate und auseinanderlaufende Takte werden ausgeglichen. Neu in der Engine: `record::start_pair_input`, `probe_input`, `InputConfig.pair`.
- Tests ohne Ton und ohne Allokation: Rückweg ohne Anrufer-Anteil, ohne Anrufer-Signal gleich dem Programm, Pegel ohne Sendung, 44,1 kHz, kein Rückweg, Absenkung, Zuordnung. Abnahme: `docs/abnahme.md`, „Silvester Teil 2, Paket 3“.

## 0.34.7 — 2026-10-31

Design D1: zwei Themen, Hell und Dunkel, mit Modusfarbe (Vorgabe `docs/design/woa-design-system/`).

- **Thema wählbar:** Einstellungen, „Erscheinungsbild“: Dunkel (Vorgabe) oder Hell, gespeichert in der Konfiguration (`theme`). Alte Konfigurationen starten dunkel.
- **Modusfarbe folgt dem Layout:** Akzent, Zweitfarbe und Leuchtziffern wechseln mit Produzent (Amber), Radio (Rot) und Party (Magenta); sechs Paletten (3 Modi × 2 Themen) aus `tokens.json`.
- **Farben als Palette:** `crates/gui/src/theme.rs` hält alle Tokens (neu: `border_strong`, `lcd_bg`, `led_off`, `vu_face`, `vu_ink`, Abstände und Radien); die alten Konstanten (`ACCENT`, `WELL` …) sind Funktionen (`accent()`, `well()` …) der aktiven Palette. Ein Test liest `tokens.json` und prüft, dass alle Werte übereinstimmen.
- **LCDs und LED-Ketten bleiben auch im hellen Thema dunkel** (wie Hardware), Pegelanzeigen nutzen `lcd_bg` und die LED-Farben des Design Systems. Bedienbare Elemente haben die kräftigere Kontur `border_strong`.
- Die Probe `WOA_THEME=hell|dunkel` bleibt für den Fotomodus; sie schlägt die Einstellung.

## 0.34.6 — 2026-10-30

- **Design System im Repo:** `docs/design/woa-design-system.zip` (Original aus Claude Design) und entpackt unter `docs/design/woa-design-system/` (Brand-Book, Tokens für 6 Themen, Bausteine mit Vorschau, Screen-Entwürfe, Logos, Schriften). Übersicht in `docs/design/README.md`, Schriftlizenzen (SIL OFL 1.1) in `docs/herkunft.md`, `CLAUDE.md` nennt das Design System als verbindliche Vorgabe für `crates/gui`. Kein Code geändert.

## 0.34.5 — 2026-10-30

Fix #139: Kopfhörer unter Windows blieben stumm, nachdem Windows das Standard-Ausgabegerät umgeschaltet hatte.

- **Ausgabe verbindet sich selbst neu.** Player, Sendeausgänge (Hauptgerät und weitere Geräte) und Vorhören laufen jetzt über `ManagedOutput` (`crates/engine/src/device.rs`). Meldet der Strom `StreamInvalidated`, `DeviceNotAvailable` oder einen Treiberfehler, oder wechselt bei „Standard“ das Standardgerät (Prüfung alle 2 s), öffnet der GUI-Takt den Strom neu auf dem aktuellen Standardgerät, nie im Audio-Thread. Wartezeiten 1, 2, 4 … höchstens 16 s. Aussetzer und Umleitungen lösen nichts aus. Die Render-Funktion (Mixer) liegt außerhalb des Stroms und überlebt das Neuöffnen; der Audio-Thread nimmt sie nur mit `try_lock` (kein Warten). Grenze: Ändert sich dabei die Abtastrate, bleibt es still (Klasse `config`).
- **Telemetrie mit Fehlerklasse:** `audio.output.device_invalidated`, `device_not_available`, `device_busy`, `config`, `backend`, `other`; kein Freitext, kein Gerätename.
- **Einstellungen, „Ausgabe neu verbinden“** als Rettungsknopf.
- Tests mit simulierten Gerätefehlern (Klasse, Neuöffnen nur bei echten Fehlern, Wartezeiten, Render überlebt). Abnahme: `docs/abnahme.md`, „Kopfhörer unter Windows“.

## 0.34.4 — 2026-10-30

Silvestersendung Teil 2, Paket 2 von 4 (#134): drei Decks.

- **Decks A, B, C** im Radio-Fenster über der Cart-Wall: große Restzeit (rot unter 10 s), Start/Stopp, Cue (zurück an den Anfang) und Vorhören je Deck. Stopp blendet kurz aus und hält die Stelle, Start läuft von dort weiter; am Ende steht das Deck wieder am Anfang. Mehrere Decks laufen gleichzeitig, auch neben Playlist und Carts. Ohne Ausgänge-Zuordnung erscheinen sie im Hauptausgang.
- **Laden:** Datei aus dem Explorer oder aus der Bibliothek auf das Deck ziehen, Rechtsklick auf den Decktitel, „Datei laden …“, oder Rechtsklick auf einen Playlist-Titel, „Auf Deck laden“ (mit dessen Cue-In und Cue-Out). Die Decks bleiben in `radio.json` erhalten (`decks`, alte Dateien laden unverändert).
- **Schnitt und Vorhören:** Rechtsklick auf den Decktitel, „Start und Ende schneiden …“ wie bei den Carts; „Vorhören“ läuft über den Kopfhörer-Ausgang, nie auf Sendung.
- **Ausgänge:** Jedes Deck bekommt ein eigenes Gerät und Kanalpaar (Einstellungen, „Ausgänge (Soundkarten) …“). Neu: „Wie Playlist“ ist die Vorgabe für alle Quellen außer der Playlist. Bisher lief eine nicht eingestellte Cart-Wall auf dem Windows-Standardgerät, jetzt mit der Playlist.

## 0.34.3 — 2026-10-30

Ein Knopf für Play und Pause (Refs #137).

- **Überall genau ein Knopf:** ▶ wenn nichts läuft oder pausiert ist, ⏸ während es spielt, daneben Stopp. Party-Modus, Radio-Layout (statt „Start“/„Weiter“; die Leertaste auch) und Handy. Der Player hatte das schon.
- **Radio-Layout:** „Weiter“ (nächster Titel) ist als ⏭ in die Reihe der Transportknöpfe gewandert.

## 0.34.2 — 2026-10-30

Transportknöpfe (#137), Abnahme: `docs/abnahme.md`.

- **Normale Knöpfe in App und Fernbedienung:** Zurück, Play/Pause, Stopp, Weiter, Von vorn, 10 s zurück, 10 s vor, Stumm, Leiser, Lauter. Im Party-Modus in drei Reihen, am Handy (nur mit DJ-PIN) dieselben, im Radio-Modus neben „Stopp“. Alles läuft über eine Befehlsschicht (`PartyCommand` und `Effect` in `woa-radio`); die Tabelle der Befehlsnamen (`simple_command`) gilt für das Handy und für OSC (`/woa/party/<name>`). Neue Befehle sind in der Fernbedienung automatisch nur für den DJ, Anmeldung, Ratenbegrenzung und Eingabeprüfung sind unverändert.
- **Echtes Pause:** „Pause“ blendet aus und merkt sich die Stelle, „Play“ macht dort weiter (Sprung per `Seek`), auch wenn die Ausblendung noch läuft. „Stopp“ vergisst die Stelle. Der Zustand „PAUSE“ mit gemerkter Restzeit steht in App und Handy (`paused` im Schnappschuss).
- **Wiederholen wie bei Apple Music:** Ein Knopf, jeder Klick schaltet weiter: aus, 🔁 Playlist, 🔂 Titel, aus. „Titel“ beginnt den Titel am Ende neu, „Weiter“ springt trotzdem zum nächsten. „Playlist“ beginnt nach dem letzten Titel von vorn. **Zufall** ist ein eigener Umschalter (jeder Titel einmal je Runde, mit „Playlist“ endlos). Ist eine Funktion an, ist der Knopf orange, in der App und am Handy gleich (Zustand im Schnappschuss: `repeat`, `shuffle`, `muted`).
- **Mixer:** neues Kommando `Seek` (Sprung im laufenden Titel, begrenzt auf Cue-In bis Cue-Out) und ein Titelwechsel-Zähler im Status, damit auch derselbe Titel als neu begonnen erkannt wird. Tasten in der Party-Ansicht: P/Pfeil links, M, R, S.

## 0.34.1 — 2026-10-30

Paket 40 aus M10: Release-Workflow für beide Plattformen (Abnahme: `docs/abnahme.md`, Ablauf: `docs/release.md`).

- **`release.yml` auf Tag `v*`:** Erst „pruefen“ (Tag = Cargo-Version = oberster, nicht leerer CHANGELOG-Eintrag, fester MSI-UpgradeCode, Tag liegt in `main`), dann Windows- und Linux-Bau nebeneinander (`windows-build.yml`, `linux-build.yml` sind jetzt auch aufrufbar), dann ein Release als **Entwurf** mit MSI, ZIP, AppImage, Flatpak, `latest.json`, ggf. `.minisig` und neu **`SHA256SUMS`**. Veröffentlichen macht das Update sichtbar. Vorher hing der Versionsvergleich an einem Dateinamen; jetzt prüft ihn `packaging/release-check.sh` (getestet mit `packaging/test-release-check.sh`, 19 Fälle).
- **Probelauf ohne Tag:** `workflow_dispatch` (und Änderungen an `release.yml` im PR) laufen bis zum fertigen Ordner, legen aber kein Release an; der Ordner hängt als Artefakt `release-probe` am Lauf.
- **MSI-Upgrade-Probe im Windows-Bau:** `packaging/windows/test-upgrade.ps1` installiert eine 0.0.1 aus denselben Programmen, dann das neue MSI darüber (genau ein Eintrag, neue Version, alle Dateien), prüft die Downgrade-Sperre (Abbruch mit 1603) und die Deinstallation. Ohne Start der App. Das Test-MSI liegt im Temp-Ordner und gerät nicht ins Release-Artefakt.
- Neu: `docs/release.md`. Offen für André (im PR): Codesigning-Zertifikat, Signatur der Linux-Pakete, Download-Seite und Datenschutzerklärung auf wironair.de.

## 0.34.0 — 2026-10-30

Paket 39 aus M10: Linux-Pakete aus der CI, AppImage und Flatpak (Abnahme: `docs/abnahme.md`).

- **Workflow `linux-build.yml`:** Von Hand oder bei Änderungen an `packaging/linux/` startbar. Job „appimage“ (Ubuntu 22.04, ältere glibc) und Job „flatpak“. Beide laden ihr Paket als Artefakt (`WOA-<version>-x86_64.AppImage`, `woa-<version>.flatpak`), unsigniert.
- **AppImage:** `packaging/linux/build-appimage.sh` legt den AppDir an (Programme, Startmenü-Eintrag, Metainfo, Symbol), lässt `linuxdeploy` die Bibliotheken einsammeln und `appimagetool` das Paket schreiben (Ablauf nach LMMS). Beide Werkzeuge mit fester Version. Eigenes `AppRun`: ohne Zusatz startet `woa`, mit `WOA_APP=woa-headless` oder `woa-streamd` die anderen Programme.
- **Flatpak:** `packaging/linux/flatpak/de.wironair.woa.yaml` (Laufzeit freedesktop 25.08, Rust aus der SDK-Erweiterung). Zugriffsrechte nach Mixxx: Wayland/X11, Pulse/PipeWire, Geräte (Audio, MIDI), Netz, Musik, Dokumente, Wechseldatenträger. Alle Crates kommen als Quellen aus `Cargo.lock` (`flatpak-cargo-generator`), der Bau läuft ohne Netz.
- **Metainfo und Startmenü:** `de.wironair.woa.desktop`, `de.wironair.woa.metainfo.xml` (Version und Datum setzt `render-metainfo.sh` aus `Cargo.toml` und CHANGELOG), Symbol als SVG und PNG in den Farben der Oberfläche.
- **`woa --version` und `--help`:** Das Programm gibt Version bzw. Aufruf aus und beendet sich, ohne Fenster. Vorher hätte `--version` versucht, einen Projektordner dieses Namens zu öffnen.
- **Prüfungen im Lauf (ohne Ton, ohne Gerät):** `desktop-file-validate`, `appstreamcli validate`, `woa --version` stimmt mit der Version überein, Offline-Render eines neuen Projekts mit `woa-headless` (2 Sekunden Stille, Datei nicht leer) aus dem fertigen Paket; beim Flatpak im Lauf installiert und mit `flatpak run` gestartet (das Probeprojekt bekommt eine ausdrückliche Freigabe, die Sandbox sieht sonst nur ihre eigenen Ordner).
- **Test:** Einheitstests für `--version` und für den Projektordner als erstes Argument.
- **Review-Nachbesserung:** Lizenzdatei landet im AppImage unter `usr/share/doc/woa/LICENSE` (vorher falscher Pfad, Fehler verschluckt); verrutschte Überschrift im Eintrag 0.33.1 repariert.
- **Herkunft:** Ablauf aus LMMS (`LinuxDeploy.cmake`, GPL-2.0-or-later), Zugriffsrechte aus Mixxx (GPL-2.0-or-later); kein Code übernommen (`docs/herkunft.md`).
- **Nicht gebaut / nicht geprüft:** Signatur der Pakete; Veröffentlichung auf wironair.de oder Flathub; Start der Oberfläche im Paket (CI hat keinen Bildschirm); CLAP-Plugins im Flatpak (die Sandbox sieht die Plugins des Rechners nicht); Update-Prüfung (Paket 40).
- **Offen für André:** Code-Signing-Zertifikat und Signatur der Pakete (Kosten); Ort der Veröffentlichung (wironair.de, Flathub, GitHub-Releases); ob die App-Kennung `de.wironair.woa` so bleiben soll (setzt die Domain wironair.de voraus).

## 0.33.7 — 2026-10-30

Silvestersendung Teil 2, Paket 1 von 4 (#134): Ausgänge je Gerät.

- **Ausgänge (Soundkarten):** Einstellungen, „Ausgänge (Soundkarten) …“. Playlist und Cart-Wall bekommen je ein Gerät und ein Kanalpaar (1/2 bis 7/8). Mehrere Geräte laufen gleichzeitig, der Pegel wird am Pult gefahren. Gespeichert in `config.json` (`outputs`, alte Dateien laden unverändert). Ohne Zuordnung klingt alles wie bisher.
- **Fehlendes Gerät:** Fehlt das Gerät der Playlist, läuft die Sendung auf dem Standardgerät. Fehlt das einer anderen Quelle, läuft sie mit der Playlist. Beides steht als Meldung im Radio-Fenster und in den Ausgängen; wird ein Gerät mitten in der Sendung abgezogen, steht dort „Ausgang … ist ausgefallen“.
- **Takt:** Das Gerät der Playlist gibt den Takt vor, jedes weitere Gerät hat einen eigenen Strom und einen Ring dazwischen. Abweichende Abtastraten und Taktdrift gleicht ein nachgeführter Resampler aus (Polster etwa 50 ms), Zähler für Notfälle stehen in den Ausgängen.
- Engine: `woa_engine::pairs` (Kanalpaare, Ring mit Resampler), `start_pairs_output`; Radio: `routing` (Plan, Router), Mixer `process_multi`.

## 0.33.6 — 2026-10-30

- **Auto-Update per MSI** (André, für Tester unter Windows 10). Beim Start fragt WOA höchstens einmal am Tag bei GitHub nach dem neuesten **veröffentlichten** Release; ein Streifen zeigt „Update x.y.z verfügbar – Jetzt installieren“ mit den Neuerungen. Unter Windows lädt der Klick das MSI, prüft es, startet `msiexec /passive` und beendet die App (Projekt wird vorher gespeichert oder gefragt); das MSI startet die neue Version danach selbst (`RESTARTWOA=1`, `woa.wxs`). Linux und macOS zeigen nur den Link zur Release-Seite. Ausschalten: Einstellungen → „Beim Start nach Updates suchen“ (`auto_update` in `config.json`), sofort suchen im selben Menü. Netz nur in Hintergrund-Threads.
- **Zwei Stufen:** Stufe A (jetzt aktiv, solange `packaging/windows/updater.pub` ein Platzhalter ist): nur HTTPS auf `github.com/nixblick/daw.woa/releases/…` (Weiterleitungen nur auf `objects.githubusercontent.com` und `release-assets.githubusercontent.com`), SHA-256 aus `latest.json` gegen das MSI. Stufe B (echter minisign-Schlüssel in `updater.pub`): Signatur Pflicht, fail-closed, der signierte Kommentar muss `woa <version>` mit der Manifest-Version sein, kein Rückfall auf A. In beiden: Downgrade-Wächter (nur strikt neuere Version). Restrisiko von Stufe A und Einrichtung von Stufe B: `docs/updater.md`.
- **Release-Workflow** `release.yml` (Tag `v*`): Windows-Bau (wiederverwendet `windows-build.yml`), signieren mit `WOA_UPDATER_KEY` (fehlt das Secret: nur SHA-256 und Warnung), `latest.json`, Release als Entwurf. Veröffentlichen macht das Update sichtbar.
- Neuer Crate-Inhalt `woa-updater` (`minisign-verify`, `sha2`, `ureq`), Tests ohne Netz mit Fixtures in `tests/fixtures/updater/`.

## 0.33.5 — 2026-10-30

Silvester-Paket (#132), Abnahme: `docs/abnahme.md`.

- **Cart-Wall: Nummer ist die Taste:** Das Pad zeigt groß seine Nummer, die Taste startet das Cart, nochmal drücken hält es an (1–9, 0 für Cart 10 wie bisher). Die Taste steht klein in der Ecke und lässt sich je Cart ändern (Rechtsklick, „Taste ändern …“, Esc bricht ab, Entf nimmt die Taste weg). Jede Taste gehört genau einem Cart; M und Leertaste bleiben reserviert. Carts 11–16 haben anfangs keine Taste. Gespeichert in `radio.json` (`cart_keys`, alte Dateien laden unverändert).
- **Carts schneiden:** Rechtsklick, „Start und Ende schneiden …“: Cue-In und Cue-Out je Cart mit Wellenform und Vorhören. Der Mixer spielt nur das Stück dazwischen; das Vorhören eines Carts beginnt am Cue-In.
- **Uhrzeit:** Im Sende-Deck steht die Ortszeit groß über dem Countdown (Zone der Stundenuhr).
- **Jingle-Übergang:** Im Voice-Track-Editor stellt „Jingle-Übergang“ alles passend ein: Das Jingle beginnt zeitgenau mit dem nächsten Titel, der Titel startet 20 dB leiser darunter und wird nach dem Ende des Jingles in 1,5 s hochgefahren. Per Offline-Render gemessen.

## 0.33.4 — 2026-10-30

- **Windows: App beendete sich nicht, MSI-Installation brach ab** (André). Beim Schließen wartete die Telemetrie auf einen Kanal, den der globale Logger nie freigibt. Das Fenster ging zu, der Prozess lief weiter, und der Installer verlangte, die App zu beenden. Jetzt bekommt der Upload ein Ende-Signal und hat höchstens 3 s für den letzten Versand. Test: `drop_returns_while_a_layer_still_holds_a_sender`.
- **Windows: kein schwarzes Konsolenfenster mehr** neben der App (`windows_subsystem = "windows"`).

## 0.33.3 — 2026-10-30

- **Spur umbenennen hielt fest:** Das Namensfeld holte sich in jedem Bild den Fokus zurück, Enter und ein Klick daneben wirkten nicht, man kam nur per Neustart heraus (André, Windows 0.33.2). Der Fokus wird jetzt nur beim Öffnen gesetzt. Enter oder ein Klick daneben übernimmt den Namen, Esc verwirft ihn.

## 0.33.2 — 2026-10-30

- **Windows: Wiedergabe und Mikrofon gingen nicht** (▶ grau, „Der Threadmodus kann nicht nach dem Einstellen geändert werden“). Das Fenster richtet COM für Drag & Drop als STA ein, WASAPI braucht MTA. Jeder Gerätezugriff (Geräte auflisten, Ausgabe, Aufnahme, Live-Eingang) läuft unter Windows jetzt auf einem eigenen Thread. Außerdem bestimmt unter Windows der Treiber die Puffergröße statt fester 128 Frames. Ein grauer ▶ erklärt beim Darüberfahren, warum.

## 0.33.1 — 2026-10-30

- **Telemetrie, die etwas sagt:** Bisher kamen fast nur leere `log.warn`-Zeilen an, alle 5 Minuten. Jetzt alle 120 Sekunden, mit Code-Ort jeder Warnung und jedes Fehlers (`log.error.app.4711`), Dauer langsamer Aktionen (`span.playlist_add`, `span.audio_load`, `span.show_load`, `span.show_render`, `span.dawproject_import` u. a. ab 20 ms), langsamen Frames (`ui.frame_slow` ab 100 ms) und Hängern der Oberfläche (`ui.hang_open` ab 2 s, auch wenn das Fenster nie mehr reagiert). Dazu alle 120 s eine Zusammenfassung: Frames, fps, Durchschnitt und Maximum der Frame-Zeit, Laufzeit, Speicher und Threads (Linux), Spuren, Clips, ungespeichert, Zoom, Player offen, Radio-Aussetzer. Zeitmessung auch für Dekodieren, ffmpeg, Export, Stems und Clip-Rendern. Weiter nur Bezeichner und Zahlen, kein Text, keine Pfade. Übersicht in `docs/telemetrie.md`.

## 0.33.0 — 2026-10-30

Paket 38 aus M9: Browser-Gast per WebRTC, lokal (Abnahme: `docs/abnahme.md`).

- **Browser-Gäste:** In der Karte „Gäste & Mix-Minus“ gibt es den Abschnitt „BROWSER-GÄSTE“. „Browser-Zugang einschalten“ öffnet zwei Ports auf der LAN-Adresse (HTTPS 47601, UDP 47600, sonst vom System gewählte). „＋ Einladung“ erzeugt einen Link mit QR-Code, 30 Minuten gültig und einmal benutzbar; „Zurückziehen“ macht ihn ungültig. Der Gast öffnet den Link im Handy-Browser, gibt seinen Namen ein und erlaubt das Mikrofon. Er landet im Warteraum, dort steht „Annehmen“ oder „Ablehnen“. Nach „Annehmen“ belegt er einen Gast-Kanal (Fader, Stumm, Pegel wie bei den Testgästen); Auflegen durch den Gast oder „Entfernen“ gibt den Kanal frei. Bei jedem Programmstart ist der Zugang aus.
- **Audio:** Hinweg Mikrofon → Opus → Jitterpuffer → Gast-Kanal. Rückweg (Mix-Minus, ohne den Gast selbst) → Opus mit 64 kbit/s und eingebauter Verlustkorrektur → Browser. Ein UDP-Thread (`woa-guest-net`) treibt alle Gäste; die Audio-Seite bleibt frei von Locks und Allokation (Rückweg über `rtrb`, `assert_no_alloc` in den Tests). RTT und Paketverlust stehen im Log und in der Zeile des Gastes. Wird die Verbindung nach 20 Sekunden nicht fertig, gibt der Kanal sich frei.
- **Sicherheit der Anmeldung:** Signalisierung über HTTPS (Browser erlauben das Mikrofon nur auf gesicherten Seiten). Das Zertifikat ist selbst ausgestellt und entsteht bei jedem Start neu, ohne Schlüssel auf der Platte; das Handy zeigt darum eine Warnung. Einladungen sind 128 Bit zufällig und einmalig; höchstens 16 Einladungen und 8 Wartende; Namen höchstens 24 Zeichen ohne Steuerzeichen; Anfragen über 26 KiB und Köpfe über 8 KiB werden abgewiesen; Sicherheits-Köpfe und Content-Security-Policy auf jeder Antwort; Wartende ohne Lebenszeichen fliegen nach 20 Sekunden raus.
- **Abhängigkeiten:** `str0m` 0.24 (Sans-IO-WebRTC; Verschlüsselung über aws-lc-rs, weil die reine Rust-Variante `crypto-bigint` 0.5 mit RUSTSEC-2026-0387 mitbringt), `opus`, `rustls` und `rcgen` (Zertifikat), `httparse`, `getrandom`.
- **Neuer Code:** `crates/guests` mit `ret` (Rückweg-Ring), `resample`, `codec`, `net`, `gate` (Einladungen und Warteraum), `server` (HTTPS), `web` (Anbindung an den Hub) und `gast.html` (die Gast-Seite, dunkel wie die App); `GuestHub::web_*`, `Controller::guests_web_set`, Abschnitt in `crates/gui/src/radio.rs`.
- **Tests (ohne Ton, ohne Gerät, kein echtes Mikrofon):** (1) Zwei Teilnehmer im selben Prozess über Loopback: Gast-Sinus 440 Hz kommt am Pult mit −14 bis −8 dB an, Rückweg-Sinus 880 Hz beim Gast ebenso, der eigene Ton liegt unter −60 dB; mehr als 100 Pakete je Richtung. (2) Der ganze Weg über HTTPS: Einladung, Beitritt, Warteraum, Annehmen, fünf Sekunden Gespräch mit Messung, Auflegen, Kanal frei und wiederbenutzbar, Ablehnen, kaputtes Angebot, Ausschalten schließt die Ports. (3) Kopfloser Chromium mit falschem Mikrofon (`--use-fake-device-for-media-stream`, `--mute-audio`) gegen den Server: Seite kommt in den Warteraum, wird angenommen, verbindet (RTT 14 ms, 0 Promille Verlust, Rückweg-Pakete kommen im Browser an), „Auflegen“ durch WOA zeigt „ende“. Dieser Test läuft nur mit `WOA_CHROMIUM=<Pfad>` und ist in der CI aus. Dazu Einheitstests für Einladungen, Warteraum, Anfrage-Grenzen und Namen.
- **Herkunft:** Schleife und Aufbau nach dem Chat-Beispiel von str0m, Idee „Eingang mit Vorlauf“ aus Liquidsoap, kein Code übernommen außer dem Muster (`docs/herkunft.md`).
- **Nicht gebaut / nicht geprüft:** Gäste hinter fremden Routern (ohne TURN nur im selben Netz, offene Frage unten); ein echtes Handy, echte Kopfhörer und WLAN (nur Chromium am Notebook); Windows-Firewall-Abfrage; Zertifikat von einer echten Stelle statt der Warnung.
- **Offen für André:** TURN-Relay für Gäste von außen (laufende Kosten, Fremddienst oder eigener Server); echtes Zertifikat samt Domain; Firewall-Freigabe für UDP 47600 und TCP 47601.

## 0.32.0 — 2026-10-30

Paket 37 aus M9: Gast-Kanäle mit Mix-Minus, ohne Netz (Abnahme: `docs/abnahme.md`).

- **Gast-Kanäle:** Neue Karte „Gäste & Mix-Minus“ im Radio-Modus. „＋ Testgast“ legt einen Gast mit eigenem Sinuston an (Gast 1: 330 Hz, Gast 2: 440 Hz, danach in 110-Hz-Schritten), „＋ Datei als Gast“ spielt eine Audiodatei in Schleife. Bis zu acht Gäste. Je Gast: Pegelanzeige, Fader (−60 bis +12 dB, geglättet), Stumm, Rückweg abhören, Entfernen und eine Zeile mit Puffer in Millisekunden und Zählern. Die Gäste liegen in der Sendung, im Mitschnitt und im Stream.
- **Rückweg (Mix-Minus):** Je Gast die Summe aus Musik, Carts, Moderator und allen anderen Gästen, nur nicht er selbst. Die anderen Gäste werden einzeln addiert statt „Summe minus eigener Anteil“, damit von der eigenen Stimme auch kein Rundungsrest bleibt. Der Kopfhörer-Knopf (🎧) legt den Rückweg dieses Gastes auf den Abhörausgang des Sendemixers; nochmal drücken stellt den normalen Abhörmix zurück. Der Rückweg läuft durch dieselbe Lautheitsregelung und Begrenzung wie die Sendung.
- **Jitterpuffer:** Je Gast ein Ring mit 60 ms Vorlauf (Wiedergabe beginnt erst, wenn er gefüllt ist) und einer Obergrenze von 400 ms (was darüber ankommt, wird abgewiesen; was der Leser darüber findet, wird auf den Vorlauf abgeschnitten, damit der Gast sich nicht mit wachsender Verzögerung hört). Läuft der Puffer leer, kommt Stille mit kurzer Einblendung, und der Puffer füllt sich wieder bis zum Vorlauf. Jeder Aussetzer und jeder Überlauf wird gezählt, in der Anzeige gezeigt und mit `tracing` ins Log geschrieben (Gast, Anzahl, verworfene Frames).
- **Quellen:** Testgäste (Sinus oder Datei) schreiben in Echtzeit-Takt in den Puffer. Die spätere WebRTC-Seite schreibt über denselben `GuestFeed` (`GuestHub::add_external`); die Anbindung selbst ist Paket 38.
- **Neuer Code:** `crates/guests` (`jitter`, `bank` im Audio-Thread, `hub` im Control-Thread, `source`), Einbau in `woa_radio::Mixer` (Gast-Summe vor der Sendebus-Verarbeitung, Rückweg am Abhörausgang) und `Controller::guests()`.
- **Tests (ohne Ton, ohne Gerät):** Offline-Render mit zwei Sinus-Gästen (440 und 660 Hz) und Musik (880 Hz): Rückweg A enthält B und Musik, A liegt unter −90 dB (Goertzel), umgekehrt für B; mit nur einem Gast ist der Rückweg bitgleich die Musik; dasselbe durch den echten Sendemixer mit Umschalten des Abhörausgangs; Fader −20 dB und Stumm wirken in Sendung und Rückweg, Pegelanzeige folgt dem Fader; Vorlauf, Obergrenze, Abschneiden, genau ein Aussetzer je Leerlauf und Wiederanlauf erst nach Vorlauf; `assert_no_alloc` um Gast-Summe, Rückweg und den ganzen Mixer-Block; Kanal frei nach Entfernen, höchstens acht Gäste; Testquelle liefert in Echtzeit.
- **Lint:** Die Testquelle kommt ohne `chunks_exact_mut` aus (neuere Clippy-Version in der CI).
- **Herkunft:** Das Muster `buffer`/`max` aus dem Harbor-Eingang von Liquidsoap, nur die Idee (`docs/herkunft.md`).
- **Nicht gebaut / nicht geprüft:** WebRTC-Anbindung, Einladungslink und Browser-Mikrofon (Paket 38); Hören im echten Kopfhörer (nur Offline-Render); Rückweg als eigener Strom je Gast zum Netz (kommt mit WebRTC, die Bank liefert ihn bereits Frame für Frame über `return_bus`).

## 0.31.0 — 2026-10-30

Paket 36 aus M9: Web-Fernbedienung im WLAN (Abnahme: `docs/abnahme.md`).

- **Fernbedienung:** Im Party-Layout gibt es unter der Wunschliste die Karte „Fernbedienung“. „Fernbedienung an“ öffnet einen lokalen Webserver (nur auf der LAN-Adresse des Rechners, Port 8787, sonst ein freier) und zeigt einen QR-Code mit der Adresse samt Gast-PIN. Aus: Der Port ist zu, offene Handys bekommen ein Close und zeigen „getrennt“. Bei jedem Programmstart ist sie aus.
- **Zugang und Rollen:** Beim Einschalten entstehen zwei neue sechsstellige PINs (Zufall vom System). Die Gast-PIN steht im QR-Code, die DJ-PIN nur auf dem Bildschirm („DJ-PIN zeigen“, zum Eintippen). „Einmal“ heißt: pro Einschalten, denn viele Gäste scannen denselben Code; „Neue PINs“ trennt alle Handys. Gast: sieht die Anzeige, sucht Titel und reicht Wünsche ein (auch Freitext, höchstens 80 Zeichen). DJ: zusätzlich Play/Pause, Weiter, Lauter/Leiser, Wunsch als Nächstes oder verwerfen. Vollbild bleibt am Rechner.
- **Schutz:** Fünf falsche PINs je Adresse sperren sie 60 Sekunden (429); ein Gast darf alle 10 Sekunden einen Wunsch (je Adresse, ein neuer Login hilft nicht); zehn Befehle je Sekunde und Sitzung; höchstens 64 Verbindungen; Anfragen über 4 KiB und Köpfe über 8 KiB werden abgewiesen; die Handys sehen nie einen Dateipfad, nur kurze Kennungen. Alles wird mit `tracing` geloggt (Anmeldungen, falsche PINs, Sperren, Befehle).
- **Vollstand, dann Änderungen:** Beim Verbinden per WebSocket kommt der ganze Zustand, danach nur die geänderten Felder (Muster aus Ardours OSC-Oberfläche). Die Seite bleibt bei einem Verbindungsabbruch auf „getrennt“ und verbindet neu, wenn der Rechner wieder da ist.
- **Dieselbe Befehlsschicht:** Befehle vom Handy sind die `PartyCommand`s der Party-Schicht (`woa-radio`), laufen durch `PartyView::run` wie die großen Tasten und stehen im Befehlsprotokoll. Neuer Code: `crates/remote` (Server mit `std::net` und `tungstenite`, ohne Async-Stapel; Anmeldung und Grenzen; Befehle; QR mit `qrcode`; Brücke zur Party-Schicht), die Karte in `crates/gui/src/remote.rs`.
- **OSC (optional, aus):** Mit dem Haken „OSC-Eingang“ lauscht UDP-Port 9000 auf `/woa/party/play_pause`, `/next`, `/volume_up`, `/volume_down` (Tasten: nur Drücken zählt). OSC kennt keine Anmeldung und gibt darum jedem im Netz DJ-Rechte; nur mit Bedacht einschalten.
- **Tests (ohne Ton, ohne Gerät, gegen 127.0.0.1):** falsche PIN gibt 401; Gast bekommt 403 bei jedem Steuerbefehl, per HTTP und WebSocket; Wunsch aus der Suche landet in der Befehlsschicht und in der Wunschliste; Ratenbegrenzung für Wünsche, Befehle und PIN-Raten; Sperre und Freigabe; Vollstand und Änderungen, unveränderter Stand sendet nichts; 20 Handys gleichzeitig, danach Aus: alle bekommen Close, Port ist zu; „kein Port offen, wenn aus“; neue PINs trennen; Verbindungsgrenze; zu große Anfragen; OSC-Pfade; der ganze Weg Handy – Befehl – Wunschliste – Handy – DJ reiht ein.
- **Herkunft:** Ideen aus Ardour (OSC) und Mixxx (Controller), kein Code (`docs/herkunft.md`).
- **Nicht gebaut / nicht geprüft:** Die Seite im echten Handy-Browser und das Scannen des QR-Codes (nur Server, Protokoll und Seite als Text geprüft); Windows-Firewall-Abfrage beim ersten Start (nicht beobachtet); verschlüsselte Verbindung (HTTPS, siehe offene Fragen); Gäste-Schalte per WebRTC und Mix-Minus (Pakete 37 und 38).

## 0.30.0 — 2026-10-30

Paket 35 aus M9: Party-Layout (Abnahme: `docs/abnahme.md`).

- **Modus Party:** Dritter Modus neben Radio und Produzent (Taste „Party“ oben rechts, wird in `config.json` gespeichert). Er spielt über denselben Sendemixer wie das Radio, also mit AutoDJ, Crossfade und Lautheitsangleich aus `woa-radio`; der Projekt-Player ruht wie im Radio.
- **Anzeige:** Großer Titel mit Interpret (aus „Interpret - Titel“ im Dateinamen), große Restzeit (rot in den letzten Sekunden), Fortschrittsbalken, darunter der nächste Musiktitel (Voice-Tracks werden übersprungen).
- **Tasten:** Play/Pause, Weiter (Crossfade zum nächsten Titel), Leiser, Lauter (2-dB-Schritte auf dem Musikpegel), dazu Vollbild an/aus. Tastatur: Leertaste, N oder Pfeil rechts, Pfeil hoch/runter, F oder F11. „Ordner laden …“ hängt alle Audiodateien eines Ordners (bis drei Ebenen tief) hinten an die Playlist.
- **Wunschliste:** Wünsche werden nach Stimmen und dann nach Eingang sortiert; derselbe Titel (gleicher Name ohne Beachtung von Groß-/Kleinschreibung und Leerzeichen) zählt als Stimme, nicht als zweiter Eintrag. „Als Nächstes“ reiht den Wunsch hinter dem laufenden Titel ein, „✕“ verwirft ihn. Eingetragen wird bisher über „+ Wunsch aus Datei“; die Handy-Seite folgt mit Paket 36.
- **Befehlsschicht:** Zustand, Befehle und Layout stehen fensterunabhängig in `woa_radio::party` (`PartyCommand`, `PartyState::apply` mit Protokoll, `Wishlist`, `layout` mit Textform als Schnappschuss). Jede Taste ist ein Befehl mit Effekt auf die Sendung; Fernbedienung und MIDI sollen später dieselben Befehle nutzen. Die Idee „Überblenden jetzt“ getrennt von „Überspringen“ folgt Mixxx (`docs/herkunft.md`), Code ist nicht übernommen.
- **Tests:** Layout als Textschnappschuss ohne Fenster; Tasten lösen die erwarteten Effekte aus (Befehlsprotokoll); Wunschliste sortiert und dedupliziert; angezeigte Restzeit und Fortschritt gegen die Länge des Offline-Renders (Controller und Mixer in Blöcken, ohne Audiogerät); „Weiter“ blendet über und ein Wunsch läuft hinter dem laufenden Titel; Ordner-Suche; Modus wird gespeichert.
- **Nicht gebaut:** Cover im Party-Layout; Wünsche per Handy (Paket 36); Modus-Taste am Controller (`Layout::Party`); Streamen („Auf Sendung“) aus dem Party-Modus.

## 0.29.0 — 2026-10-30

Paket 34 aus M8: Abnahme „Surge XT und Vital stabil“ (Abnahme: `docs/abnahme.md`).

- **Sperrliste:** Eine Plugin-Datei, die beim Suchen abstürzt oder hängt, kommt auf die Sperrliste (`plugin-blocklist.tsv` im Konfigurationsordner, atomar gespeichert, kaputte Zeilen werden übergangen) und wird beim nächsten Suchen nicht mehr geladen. Wer den Suchprozess selbst nicht starten kann, sperrt keine Datei. Beim abgestürzten Platz gibt es „Sperren“ (gilt ab dem nächsten Suchen, nichts Laufendes wird angerührt). Unter „+ Plugin“ stehen die übersprungenen Dateien mit Grund, die gesperrten Dateien mit „Freigeben“ und „Alle freigeben“.
- **Neu-Scan:** „Neu suchen“ lässt gesperrte Dateien aus und trägt neue Fehler ein; Freigeben lädt die Datei beim nächsten Suchen wieder. Die Suche läuft weiter je Datei in einem eigenen Prozess (Frist 20 s).
- **Logs je Plugin:** Jedes Plugin im eigenen Prozess bekommt eine Datei `<Kennung>.log` im Datenordner (`logs/plugins/`): Start mit Prozessnummer, Ausfall mit Grund und Zahl verspäteter Blöcke, Neu laden, Ende, dazu alles, was der Plugin-Prozess auf `stderr` ausgibt. `scan.log` hält je Datei das Suchergebnis (in Ordnung, übersprungen, gesperrt, freigegeben). Im Audio-Thread wird nicht geschrieben. Eine Datei über 512 KiB wird beim nächsten Öffnen nach `.1` verschoben. Im Kanalzug zeigt „Log“ die letzten acht Zeilen.
- **Tests (ohne Ton, ohne Gerät):** Absturz und Hänger beim Suchen führen zur Sperre (mit Neustart der Liste, Freigabe, Suchlog); Log je Plugin mit Start, Ausfall, Neu laden, Ende in der richtigen Reihenfolge; **10 Minuten Musik offline durch zwei Plugin-Prozesse** (56 250 Blöcke je Plugin, unter `assert_no_alloc`): kein verspäteter Block, Klang kam heraus, Speicher von App und beiden Plugin-Prozessen nach der Einlaufzeit unverändert (Messung auf dem Notebook: App 4332 → 4336 KiB, Plugins 3944 → 3944 und 3704 → 3704 KiB; Grenze im Test 2 MiB je Prozess; nur unter Linux gemessen).
- **Herkunft:** Idee der Sperrliste und des Ergebnisses je Plugin aus dem Plugin-Verwalter von Ardour (`docs/herkunft.md`); kein Code übernommen.
- **Nicht gebaut / nicht geprüft:** Surge XT und Vital selbst (kein Ton am Gerät erlaubt, Probe durch André); Suche im Hintergrund statt im Bild; Sperre nach wiederholtem Absturz im Betrieb (nur von Hand); Rotation der Plugin-Logs, solange der Prozess läuft.

## 0.28.0 — 2026-10-30

Paket 33 aus M8: Absturzschutz per eigenem Prozess (Abnahme: `docs/abnahme.md`).

- **Plugin-Prozess:** Neues Programm `woa-plugin-host` (und derselbe Code über `woa --woa-plugin-host`) lädt ein CLAP-Plugin abseits der Oberfläche. Stürzt das Plugin ab oder hängt es, trifft es nur diesen Prozess.
- **Austausch:** Gemeinsamer Speicher (eine Datei, die beide abbilden) mit Atomen, dazu ein Briefkasten für Aufrufe (Info, Zustand, Fenster) und lock-freie Ringe für Reglerwerte und Rückmeldungen. Die Idee folgt dem RemotePlugin von LMMS; Code ist nicht übernommen (`docs/herkunft.md`).
- **Takt:** Jeder Block wird im Plugin-Prozess gerechnet, ohne zusätzliche Latenz. Kommt die Antwort nicht rechtzeitig (Echtzeit: etwa zwei Drittel der Blockdauer, mindestens 1 ms, höchstens 50 ms), gibt der Block Stille aus und zählt einen verspäteten Block. Im Audio-Thread wartet nur eine begrenzte Schleife; sie ist im Quelltext benannt.
- **Absturz und Hänger:** Ein beendeter Prozess oder ein Block, der länger als 1,5 s nicht fertig wird (dann wird der Prozess beendet), schaltet nur dieses Plugin stumm. Der Platz zeigt „Plugin abgestürzt – neu laden“, die Statuszeile nennt es ebenfalls; alles andere spielt weiter. „Neu laden“ startet den Prozess neu und stellt den letzten Zustand und die Regler wieder her; der Graph bleibt derselbe. Kein automatisches Neuladen.
- **Suche:** Jede Plugin-Datei wird in einem eigenen Suchprozess gelesen (20 s Frist); Absturz oder Zeitüberschreitung wird ein Suchproblem statt eines Absturzes der App.
- **Latenzausgleich:** Meldet ein Plugin Latenz (CLAP `latency`), verzögert die Engine alle anderen Spuren um die Differenz, damit alles zusammen ankommt; ein abgeschaltetes Effekt-Plugin behält seine Latenz. `StereoProcessor::latency()` (Standard 0).
- **Wächter:** Der Plugin-Prozess beendet sich selbst, wenn die App 10 s lang kein Lebenszeichen gibt.
- **Testmenü:** Mit `WOA_TESTMENU=1` erscheint im Plugin-Platz „Absturz simulieren“. `WOA_PLUGINS_IN_PROCESS=1` schaltet den Schutz aus (Fehlersuche).
- **Tests:** Selbstprüfung mit dem Test-Plugin, das absichtlich abstürzt oder hängt (Offline-Render und Echtzeit-Fenster, ohne Audiogerät): gleiche Ausgabe wie im Programm, gemessene Latenz (Impuls an Stelle 64), Absturz stumm nur dort, Hänger wird beendet, Neuladen, Zustand und Regler über die Prozessgrenze, ein Suchprozess, der stirbt; Engine: Ausgleich zwischen Spuren.
- **Nicht gebaut:** Plugin-Fenster im Schutzprozess unter Windows; Nachführen einer Latenz, die sich nach dem Laden ändert; Verpacken des zweiten Programms (AppImage, MSI, Flatpak) — bis dahin ruft sich die App selbst mit `--woa-plugin-host` auf.
- **Abhängigkeit:** `memmap2` in `woa-plugins`; `latency` in `clack-extensions`.
- **Windows:** Ungenutzter Import im Test behoben, der die Prüfung unter Windows brach.

## 0.27.1 — 2026-10-30

- **CI:** Neuer Workflow „Windows-Bau“ (`.github/workflows/windows-build.yml`), von Hand zu starten. Baut `woa.exe`, `woa-headless.exe` und `woa-streamd.exe` im Release-Profil und legt sie als MSI-Installer (WiX 5, `packaging/windows/woa.wxs`: Programme-Ordner, Startmenü-Eintrag, neuere Version ersetzt ältere) und als ZIP an den Lauf (30 Tage). Unsigniert. Produktname in der MSI mit einfachem Bindestrich (Codepage 1252).

## 0.27.0 — 2026-10-30

Paket 32 aus M8: Plugin-Fenster (Abnahme: `docs/abnahme.md`).

- **Fenster:** Im Kanalzug hat jedes geladene Plugin die Taste „Fenster“. Das Plugin öffnet sein eigenes, schwebendes Fenster (CLAP `gui` im schwebenden Modus). Schließen und wieder Öffnen baut nichts neu auf; beim Entfernen oder Beenden wird die Oberfläche vor dem Plugin abgebaut. Bietet ein Plugin kein schwebendes Fenster an, steht ein Hinweis in der Statuszeile.
- **Host:** Zeitgeber (`timer`), Dateikennungen (`posix-fd`, nur Unix), `request_callback` und Regler-Neuabgleich (`params`) werden bedient, jedes Bild der Oberfläche über `ClapInstance::pump`.
- **Reglerrückmeldung:** Was im Plugin-Fenster bewegt wird, kommt über eine lock-freie Queue (`rtrb`) aus dem Audio-Thread zurück und wird zu Rückgängig-Schritten: ein gezogener Regler ist ein Schritt, ein Preset-Wechsel (viele Regler auf einmal) ein Schritt mit dem ganzen Zustand. Werte, die der Host selbst setzt, werden nicht zurückgemeldet. Rückgängig und Wiederholen stellen die Regler im laufenden Plugin wieder her.
- **Tests:** Host- und Rack-Tests mit dem Test-Plugin (Fensterablauf, Zeitgeber, Regler ziehen, Preset-Flut ohne Allokation unter `assert_no_alloc`, Rückgängig/Wiederholen); kein echtes Fenster.
- **Nicht gebaut:** Automation von Plugin-Reglern, eingebettetes Fenster, Auslagern in einen eigenen Prozess (Paket 33).
- **Abhängigkeiten:** `rtrb` und `libc` (Unix) in `woa-plugins`; `gui`, `timer`, `posix-fd` in `clack-extensions`.

## 0.26.1 — 2026-10-30

Sicherheit (#101): Der DAWproject-Import begrenzt jede Audiodatei beim Lesen auf das Restbudget der Gesamtgrenze, nicht nur auf die Grenze je Datei. Eine präparierte Datei mit gefälschten ZIP-Größen kann die Platte nicht mehr vollschreiben. Test dazu.

## 0.26.0 — 2026-10-30

Paket 31 aus M8: CLAP-Plugins finden und laden (Abnahme: `docs/abnahme.md`).

- **Modell:** Plugin-Plätze auf Spuren (`PluginSlot`), fünf Befehle mit Rückgängig, Projektformat 4 nur bei Plugins.
- **Host (`woa-plugins`):** Suche in den Standardordnern und `CLAP_PATH`, Instanz über `clack-host`, Rechner ohne Sperre und ohne Speicheranforderung im Audio-Thread, Test-Plugin aus `clack-plugin` mit Selbstprüfung.
- **Engine:** Spuren mit Plugins laufen in Stereo. Ein Instrument-Plugin ersetzt den eingebauten Synth der MIDI-Spur (Clip-Noten im Takt des Transports, Live-Noten, Sprung und Stopp lassen nichts hängen), Effekt-Plugins folgen den eingebauten Effekten. Fehlt ein Instrument, schweigt die Spur; ein fehlender Effekt wird durchgereicht. Export und Stems rechnen mit Plugins (`render_clips_plugins`, `render_stems_plugins`).
- **Oberfläche:** Im Kanalzug „+ Plugin“ (Browser mit Instrumenten und Effekten, „Neu suchen“), Platz mit Ein/Aus und Entfernen, Regler als Zahlenfelder, „nicht installiert“ bei fehlendem Plugin. Die Suche startet erst beim ersten Bedarf. Der Plugin-Zustand wird beim Speichern, Autosave und Export ins Projekt übernommen; der Export rechnet mit eigenen Instanzen.
- **Tests:** Offline-Render mit dem Test-Plugin (Pegelvergleich, Bypass, fehlendes Plugin, Zustand speichern und laden, 200 Blöcke unter `assert_no_alloc`), Zustandstest der Oberfläche.
- **Herkunft:** Kein Code übernommen; `clack-host` und `clack-plugin` sind Abhängigkeiten. Ardour und tracktion sind noch nicht gelesen (Pakete 32/33).
- **Prüfung:** Kein Log mehr im Audio-Thread, wenn `start_processing` scheitert; der Platz meldet den Ausfall über seinen Status.
- **Abhängigkeiten:** `clack-host`, `clack-plugin` und `base64` (Lizenzen prüft `cargo deny` in der CI).
- **Nicht gebaut:** Plugin-Fenster (Paket 32), Auslagern in einen eigenen Prozess und Latenzausgleich (Paket 33), Anbindung in `woa-headless` und Agent.

## 0.25.0 — 2026-10-30

Paket 30 aus M9: Voice-Tracking-Editor.

- **Voice-Track (`woa_radio::voicetrack`):** Eine vorab aufgenommene oder importierte Moderation liegt als eigener Playlist-Eintrag (`voice`) zwischen zwei Titeln und verweist auf ihre Nachbarn. Zeitachse: Null ist der Beginn der Überblendung, die Stimme beginnt bei `offset` (negativ: im Ausklang von A). Werte: Beginn, Stimmenpegel, Absenkung der Musik (Vorgabe −12 dB), Rampe vor und nach der Stimme.
- **Ducking im Mixer:** Der Mixer startet die Stimme selbst, sample-genau relativ zum Übergang (keine Zeitsteuerung im Steuer-Thread). Die Absenkung ist eine Kosinus-Rampe aus einer einzigen reinen Funktion (`duck_gain`), die Mixer, Editor und Tests gemeinsam nutzen. Kommt der Übergang früher (Taste „Weiter“) oder später als geplant, läuft die Stimme immer vollständig von vorn, nie mittendrin. Die Stimme läuft über den Cart-Zweig; neuer Befehl `Cmd::VoiceOver`, kein Speicher und keine Sperre im Audio-Thread.
- **„Prüfen“:** Ändert sich ein Nachbar (anderer Titel, andere Länge, andere Marken), steht der Voice-Track auf „prüfen“ und geht nicht auf Sendung, bis er im Editor bestätigt wird. Nachträgliches Messen eines Nachbarn gilt nicht als Änderung. Die Meldung steht in der Sendungsmeldung und im Log.
- **Voice-Tracks sind keine Titel:** sie sind nie das laufende Deck, zählen nicht in Gesamtlänge und Restzeit, beginnen nie eine Sendung und werden bei harten Events nicht zwischen Stimme und Titel zerschnitten.
- **AutoDJ:** Auswahlen aus der Kategorie `moderation` werden als Voice-Track eingefügt, vor dem Titel, der dahinter kommt; sie zählen nicht zum Vorlauf.
- **Editor (Radio-Modus, Rechtsklick auf einen Titel):** „Voice-Track danach einfügen …“ wählt eine Sprachdatei (Messung, Vorschlag für den Beginn: die Stimme endet am Ramp-Punkt von B, sonst 2 s nach dem Beginn von B). „Voice-Track bearbeiten …“ zeigt die Wellenformen von A (Ausklang) und B (Anfang), die mit der Maus verschiebbare Stimme und darunter die Absenkungskurve. „Vorhören“ rendert A, Stimme, B offline durch den echten Mixer und spielt das Ergebnis nur im Kopfhörer (PFL), nie auf Sendung. In der Playlist trägt die Zeile „VOICE“ und „prüfen“.
- **Tests:** Offline-Render A, Stimme, B mit erzeugten Tönen durch den echten Mixer unter `assert_no_alloc` (Beginn der Stimme auf 0,06 s genau, Absenkung 12 dB auf 3 % genau, Rückkehr der Musik, keine Lücke), Stimme nach dem Übergang, „Weiter“ vorzeitig, Tausch eines Nachbarn setzt „prüfen“ und die Stimme wird nicht gesendet (nach Bestätigung schon), Speichern und Laden als Schnappschuss (`insta`), Einheitentests der Kurve, des Abgleichs und des Vorschlags, Editor als Zustandstest, Fenster zeichnet ohne Display.
- **Herkunft und Lizenz:** Idee von `smooth_add` in Liquidsoap, kein Code übernommen (`docs/herkunft.md`).
- **Nicht gebaut:** Aufnahme der Stimme im Editor (Mikrofon, braucht einen Gerätetest) und ein MCP-Werkzeug für Voice-Tracks.

## 0.24.0 — 2026-10-30

Paket 29 aus M9: Feste Zeit-Events und pünktliche volle Stunde.

- **Zeit-Events (`woa_radio::events`):** Nachrichten, Werbung oder Stundenjingle mit Uhrzeit oder Muster (Wochentage, Stunden, Minute, Sekunde), **hart** oder **weich**. Tabelle in `events.json` (atomar gespeichert, Grenzen geprüft, kaputte Datei wird gemeldet und nie überschrieben). Ein Prüftakt im Steuer-Thread löst aus; Vorbild ist die Eintragstabelle `cron.liq` in Liquidsoap (nur die Idee, kein Code).
- **Hart:** 30 s vorher wird die Datei vorgeladen, das Event wird in die Playlist eingefügt und beginnt zur genauen Zeit; der laufende Titel blendet in 250 ms ab, auch mitten in einer Überblendung (die wird beschleunigt beendet). **Weich:** das Event kommt direkt nach dem laufenden Titel. Zu spät (hart 20 s, weich 30 min) oder nicht auf Sendung: „verpasst“, mit Log, nie doppelt.
- **Sommerzeit automatisch:** Ortszeit mit EU-Regel ohne Zeitzonendatenbank (`woa_engine::localtime`), Häkchen „Sommerzeit“ in der Stundenuhr. Feste Uhrzeit in der doppelten Oktoberstunde: einmal; in der ausgefallenen Märzstunde: zum Umstellmoment; jede Stunde: jede echte Stunde. Die Stundenuhr nutzt dieselbe Zone.
- **AutoDJ plant rückwärts:** Vor einem harten Event wählt `rotation::plan_until` in den letzten 8 Minuten die Titel, die am besten in die Lücke passen, und zieht die Überblendung ab; Schnitt und Rest stehen im Log.
- **Übergänge nach Mixxx:** `woa_radio::transition` rechnet die Überblendung aus Outro- und Intro-Marken samt Randfällen. **Verhaltensänderung:** bei sehr kurzem Folgetitel (4 s) ist die Überblendung jetzt 1 s statt der Hälfte; `show::total_length` rechnet danach. `Cmd::Queue` trägt dafür `lead` und `hold`, neu sind `Cmd::StartAt` und `Cmd::ClearStartAt`.
- **Oberfläche:** Knopf „Zeit-Events“ (Liste, Muster, hart/weich, Datei, Entfernen); unter der Uhr „NÄCHSTES EVENT … in mm:ss“ und „ZULETZT … ±x,x s“.
- **Tests:** eine Stunde durch den echten Mixer (hart nach 0,12 s, mitten in Überblendung, weich nach dem Titel, keine Lücke, ohne Speicher im Audio-Thread), eine Woche mit simulierter Uhr über Mitternacht und Zeitumstellung, Mixxx-Randfälle, Planer, Editor, Fenster ohne Display.
- **Herkunft und Lizenz:** `docs/herkunft.md`.
- **CI:** Clippy-Hinweis (`explicit_counter_loop`) der neueren Toolchain in der Mustersuche behoben.
- **Nicht gebaut:** Nachrichtenquelle (externer Dienst); Events spielen eine lokale Datei.

## 0.23.0 — 2026-10-30

Paket 28 aus M9: Stundenuhr und AutoDJ-Rotation.

- **Stundenuhren (`woa_library::rotation`):** Eine Uhr ist eine Folge von Slots, jeder Slot hat eine Kategorie (ein Tag der Bibliothek, z. B. `hit`, `neu`, `jingle`) und optional eine Ersatzkategorie. Eine Wochentabelle (7 × 24 Stunden) ordnet jeder Stunde eine Uhr zu; innerhalb der Stunde laufen die Slots der Reihe nach und beginnen mit jeder neuen Stunde von vorn. Gespeichert wird in `stundenuhr.json` (atomar, Grenzen und Namen werden geprüft, kaputte Dateien werden gemeldet und nie überschrieben).
- **Auswahl:** Gewichtet nach der Zeit seit dem letzten Spiel (je länger her, desto wahrscheinlicher, höchstens eine Woche zählt), mit Sperrfristen für Titel (Vorgabe 180 min) und Interpret (60 min). Der Interpret kommt aus dem Titel nach dem Muster „Interpret - Titel“; ohne Trenner gilt nur die Titel-Sperre. Ist der Slot leer (keine Titel oder alle gesperrt), kommt die Ersatzkategorie, danach der Notfall-Tag (Vorgabe `notfall`; er ignoriert die Sperrfristen, bevor gar nichts spielt). Das Muster Slot, Ersatz, Notfall stammt von `fallback`/`rotate` in Liquidsoap, das Auffüllen einer Warteschlange vom AutoDJ in Mixxx; Code ist nicht übernommen.
- **Deterministisch:** Zufall ist SplitMix64 mit dem Startwert aus der Datei; gleicher Startwert, gleiche Bibliothek und Historie ergeben die gleiche Folge. Jede Wahl steht im Log (Uhr, Slot, gewünschte und gewählte Kategorie, Titel) und in der Liste „Letzte Auswahlen“.
- **Spielhistorie:** Neue Tabelle `plays` in der Bibliothek (Schema 4, Migration von 1 bis 3 getestet). Jeder Titel, der in der Sendung beginnt und in der Bibliothek steht, kommt mit Interpret, Kategorie und Zeit hinein; die Sperrfristen überleben damit einen Neustart, auch die Position in der Stundenuhr.
- **AutoDJ:** Schalter im Fenster. Er hält `Im Voraus` Titel hinter dem laufenden bereit (Vorgabe 3) und hängt nur hinten an: von Hand gesetzte Titel bleiben stehen, belegen aber ihre Zeit und gelten für die Sperrfristen (keine Doppel). Läuft im Oberflächen-Thread alle 3 Sekunden höchstens einmal, nie im Audio-Thread.
- **Oberfläche:** Neuer Knopf „Stundenuhr“ in der Menüleiste öffnet das Fenster „Stundenuhr und AutoDJ“: Uhren anlegen, umbenennen, löschen; Slots mit Kategorie und Ersatz (Auswahl aus den Tags der Bibliothek), verschieben, entfernen; Wochenraster mit Klick zum Zuordnen und Rechtsklick zum Leeren, die aktuelle Stunde ist umrandet; Sperrfristen, Zeitverschiebung gegenüber UTC, Notfall-Tag, AutoDJ-Schalter.
- **Zeitzone:** Die Ortszeit ist eine Einstellung in Minuten gegenüber UTC (Vorgabe 60); die Umstellung auf Sommerzeit muss von Hand erfolgen.
- **Nicht in diesem Paket:** feste Zeit-Events (Nachrichten, Werbung zur vollen Stunde) und der Voice-Tracking-Editor; beides gehört zu M9, steht aber nicht im Auftrag dieses Pakets.
- **Tests:** 24 Stunden mit simulierter Uhr gegen 500 erzeugte Titel (keine Sperrfrist verletzt, Slot-Folge je Stunde stimmt), Rückfall auf Ersatz und Notfall, nichts zu spielen ohne Absturz, gleicher und anderer Startwert, Historie nach Neustart der Datenbank (Sperren und Slot-Position), von Hand gesetzte Titel, Datei mit feindlichen Werten, Editor als Zustandstest (Anlegen, Umbenennen mit Wochenübernahme, Slots, Speichern und Laden), AutoDJ-Takt gegen eine nachgebildete Sendung, Fenster zeichnet ohne Display.

## 0.22.0 — 2026-10-30

Paket 27 aus M7: DAWproject-Import und -Export.

- **Neues Modul `woa_io::dawproject`:** Eine `.dawproject`-Datei (DAWproject 1.0, ZIP mit `project.xml`, `metadata.xml` und `audio/`) wird geschrieben und gelesen. Übertragen werden Spuren (Audio, MIDI, Bus), Clips mit Position, Länge, Versatz, Blenden und Dehnung, MIDI-Noten und Controller, Tempo, Lautstärke, Pan, Mute, Solo, Ausgänge, Sends, die eigenen Effekte (als `Device` mit der Kennung `woa:<art>` samt allen Reglern) und die Automation von Lautstärke, Pan, Sends und Effekt-Reglern.
- **Was das Format nicht kennt** (Tonhöhe, Clip-Verstärkung, Blendenform, „Tempo folgen“, die genaue Dehnung) steht als Notiz `woa: …` im Kommentar des Clips und kommt bei der Rückkehr nach WOA wieder heraus. Andere Programme zeigen die Notiz nur als Kommentar.
- **Was WOA nicht kennt** (fremde Plugins, Szenen, Marker, Tempo- und Taktart-Automation, Clip-Schleifen, Video, Warp-Punkte außer Anfang und Ende) wird nicht übernommen, aber im Bericht genannt und geloggt; die Statuszeile zeigt die Zahl der Hinweise.
- **Fremde Dateien sind Eingabe, nicht Vertrauen:** XML mit Tiefen- und Elementgrenze (Entitäten und DTDs werden nie aufgelöst), ZIP mit Grenzen für Einträge, Größe je Eintrag und gesamt (auch wenn die Größenangabe lügt), Audio landet nur unter selbst vergebenen Namen im Ordner `audio/` des neuen Projekts (nie ein Pfad aus der Datei), und das Ergebnis geht durch dieselbe Prüfung wie eine geladene `project.json`. Bei einem Fehler wird der angelegte Ordner wieder entfernt.
- **Konventionen nach den Referenztests von Bitwig:** Lautstärke linear, Pan normalisiert (0,5 = Mitte), Master als Track mit `Channel role="master"`, Audio über `Warps` mit Inhalt in Sekunden und Zeit in Beats, MIDI-Controller als `Points` mit `Target expression="channelController"`.
- **Oberfläche:** Menü Datei: „Als DAWproject exportieren …“ und „DAWproject öffnen …“ (legt `<Name>.woa` neben der Datei an und öffnet es). Rate und Länge der Audiodateien kommen aus dem WAV-Kopf, sonst vom Decoder.
- **Schema-Datei:** `crates/io/tests/schema/Project.xsd` stammt aus dem DAWproject-Repository (MIT, Copyright 2020 Bitwig), die Lizenz liegt daneben.
- **Abhängigkeiten:** `zip` 9.0 (nur Deflate) und `quick-xml` 0.42, beide MIT; `tempfile` jetzt auch für das atomare Schreiben. `zip` 8.6 fiel durch `cargo deny` (RUSTSEC-2026-0367 bis 0369, alle zu `extract()`, das wir nicht benutzen; behoben ab 9.0).
- **Tests:** Rundreise (Spuren, Mixer, Bus, Send, Effekte, Automation, MIDI, Clip-Werte), gedehnter Clip mit „Tempo folgen“, `project.xml` gegen das offizielle Schema geprüft (`xmllint`, übersprungen wo es fehlt), Schnappschuss des XML, handgebaute fremde Datei nach den Konventionen von Bitwig (mit fremdem Plugin im Bericht), feindliche Dateien (kein ZIP, ohne `project.xml`, zu tief, falsche Wurzel, Pfad-Trick im Audiopfad). Offizielle Beispieldateien: im DAWproject-Repository gibt es keine `.dawproject`-Dateien, nicht gefunden.

## 0.21.0 — 2026-10-30

Paket 26 aus M7: Alle Export-Formate und Stems.

- **Neue Formate:** Opus (`.opus`), Ogg Vorbis (`.ogg`) und AAC (`.aac`, ADTS) über ein externes `ffmpeg` (`woa_engine::ffmpeg`): als Prozess aufgerufen, nichts gelinkt, Argumente als Liste ohne Shell, Rohdaten über Pipes (Eingang in eigenem Thread, damit kein Pipe-Puffer verklemmt). Gesucht wird in `WOA_FFMPEG`, neben dem Programm und im `PATH`. Fehlt ffmpeg oder der Encoder, kommt eine klare Meldung und es bleibt keine halbe Datei zurück (erst kodieren, dann anlegen). Opus rechnet ffmpeg auf 48 kHz um.
- **Wortbreite:** WAV in 16 Bit, 24 Bit oder 32 Bit Float (Vorgabe wie bisher Float); FLAC in 16 oder 24 Bit (Vorgabe wie bisher 24). FLAC kennt kein Float, die Auswahl wird abgelehnt statt still etwas anderes zu bauen. `export_stereo` und `export_stereo_to` behalten ihr Verhalten; `export_with`/`export_with_to` nehmen einen `ExportSpec`.
- **Stems (`woa_engine::stems`):** Eine Datei je Spur und je Bus, alle gleich lang, dazu auf Wunsch der Mixdown (`00 Mixdown`). Gerechnet in einem Offline-Durchgang über die Abgriffe des Graphen, nach jedem Block geleert (ein Verlust wäre ein Fehler). Die Summe der obersten Stems (Ausgang Master) ist der Mixdown; Spuren in einem Bus stecken schon im Bus-Stem. Stumme Spuren bekommen keine Datei. Dateinamen mit laufender Nummer und bereinigt (Sonderzeichen, `..`, reservierte Windows-Namen); ein Symlink als Ziel wird abgelehnt; bei einem Fehler werden die neu angelegten Dateien entfernt. Der Lautheitsangleich gilt mit demselben Gain für alle Dateien.
- **Oberfläche:** Neues Export-Fenster (Strg+E, Menü Datei): Format (ausgegraut mit Hinweis, wenn ffmpeg oder der Encoder fehlt), Wortbreite oder Bitrate, Haken für Stems, Busse und Mixdown. Die Suche nach ffmpeg läuft einmal beim Öffnen, nie je Bild. Stems gehen in einen Unterordner „<Projekt> Stems“.
- **Kennung:** Das Lesen der ffmpeg-Ausgabe nutzt `as_chunks` (der neuere Clippy der CI verlangt es).
- **Agent:** `export` nimmt auch `.opus`, `.ogg`, `.aac`; die Fehlermeldung für eine unbekannte Endung nennt alle sechs.
- **Tests:** Summe der Stems gleich Mixdown (Projekt mit Bus, Send, Pan, Pegel, stummer Spur), Rücklesen aller Dateien, gleicher Gain, Wortbreiten, FLAC ohne Float, fehlendes ffmpeg für alle drei Formate, Opus/Ogg/AAC über das echte ffmpeg (übersprungen, wo es fehlt), Dateinamen, Symlink-Ziel, Export-Fenster als Zustandstest. Zwei bestehende Agent-Tests nutzten `.ogg` als „unbekannte Endung“ und nehmen jetzt `.xyz`.

## 0.20.1 — 2026-10-30

Öffentliches Repo aufgeräumt (André: „push nach nix hin was da nicht hin soll“).

- Interne Runbooks und die Mitbewerber-/UI-Analyse aus dem Repo genommen (`docs/andre-handgriffe.md`, `docs/runbook-morgen.md`, `docs/ui-analyse-und-ziele.md`); sie liegen lokal unter `docs/intern/`.
- Servername, Server-IP, Hosting-Anbieter, interne Token-Liste und Heimatpfade aus Code, Doku und Tests entfernt („eigener Server“).
- `.gitignore`: `docs/intern/`, `.refs/`, alles mit `mitbewerber` im Namen, `*.local.md`.
- `CLAUDE.md`: Regel „Das Repo ist öffentlich“.
- Prüfung: gitleaks über alle 136 Commits ohne Fund; kein Geheimnis war im Repo.## 0.20.0 — 2026-10-30

Paket 25 aus M7: Analyse mit Tempo, Tonart, Lautheit und automatischen Cue-Punkten.

- **Analyse (`woa_dsp::analysis::track`):** Tempo aus der Autokorrelation der Einsatzkurve (70 bis 180 BPM, `None` bei Sinus ohne Einsätze, Stille oder zu kurzem Material), Tonart aus einem Chroma-Vektor gegen die Krumhansl-Profile (Anzeige „C-Dur“, „a-Moll“), Stille vorn und hinten (Schwelle −50 dBFS), Lautheit und True Peak aus dem vorhandenen Code. Alles ohne neue Abhängigkeit, im Hintergrund-Thread, nie im Audio-Thread. Die Treffsicherheit an echter Musik ist eine Vermutung, bis André sie an bekannten Titeln gesehen hat.
- **Cue-Vorschläge:** Cue-In hinter der Stille am Anfang, Cue-Out vor der Stille am Ende, Fade-Out am Beginn des Ausklangs (höchstens 15 s). Kürzer als 2 s oder ohne Stille: kein Vorschlag. Im Radio-Modus setzt das Messen einer Playlist-Zeile die Vorschläge nur, wo noch keine Cue-Punkte stehen; von Hand gesetzte haben Vorrang. Im Cue-Editor zeigt eine blasse Linie den Vorschlag, „Vorschlag einsetzen“ übernimmt nur die fehlenden Marker.
- **Bibliothek:** Schema 3 (Spalten für Tempo, Tonart, Lautheit, True Peak, Stille und Cue-Vorschlag; `analyzed_hash` verknüpft die Werte mit dem Inhalt, eine geänderte Datei kommt wieder an die Reihe). Schema 2 wird beim Öffnen angehoben, die Titel bleiben. Die Analyse läuft nach dem Import im Hintergrund mit eigener Datenbankverbindung, Fortschritt und Abbruch über Atomics; eine nicht lesbare Datei wird einmal gezählt und nicht endlos wiederholt.
- **Oberfläche:** Neue Spalten BPM, TONART und LUFS in der Bibliothek („…“ bis der Titel dran war), die Detailzeile zeigt alle Messwerte und „Neu analysieren“. Fenster etwas breiter.
- **Lautheitsangleich:** Der Radio-Modus gleicht Titel schon auf −16 LUFS an (Spitze höchstens −1 dBFS); die Rechnung steht jetzt an einer Stelle (`gain_to_target_db`), Radio und Bibliothek nutzen dieselbe.
- **Tests:** Klicktrack 120 BPM (und andere Tempi und Abtastraten) gibt 120 ±1; Sinus-Akkorde geben ihre Tonart; Stille vorn und hinten; EBU-Tech-3341-Referenz (1 kHz bei −23 dBFS gibt −23,0 LUFS) auch über den Dateiweg; 1 000 erzeugte Titel offline analysiert, danach liegen alle mit dem Gain auf dem Ziel; Migration, kaputte Datei, geänderter Inhalt; Cue-Editor und Bibliothek-Fenster als Zustandstests.

## 0.19.0 — 2026-10-30

Paket 24 aus M7: Bibliothek in SQLite mit Suche und Tags.

- **`woa-library`:** Datenbank `bibliothek.sqlite3` im Datenordner. Schema 2 (Titel mit Pfad, Inhaltskennung, Dauer, Art Musik/Sample/Jingle; Tags; FTS5-Volltextindex über Titel, Tags und Pfad). Die Schemaversion steht in `PRAGMA user_version`; ältere Datenbanken werden beim Öffnen schrittweise angehoben (Test: Datenbank im Zustand von Schema 1 behält ihre Titel und bekommt den Suchindex), eine neuere wird abgelehnt statt verändert.
- **Suche:** Jedes Wort ist ein Präfix, alle Wörter müssen passen; Groß-/Kleinschreibung und Akzente sind egal („cafe“ findet „Café“). Der Suchtext wird zu einem sicheren FTS-Ausdruck umgeschrieben (Test mit Sonderzeichen und SQL-Fragmenten). Filter nach Art und Tag.
- **Tags:** getrimmt, klein geschrieben, höchstens 40 Zeichen und 50 je Titel; der Tag-Text steht auch im Suchindex.
- **Import im Hintergrund:** Ordner rekursiv (ohne Symlinks, höchstens 16 Ebenen und 200 000 Dateien), eine eigene Datenbankverbindung im Hintergrund-Thread, Fortschritt und Abbruch über Atomics, Eintragen in Stapeln von 50 je Transaktion. Doppelte werden am Inhalt erkannt (eigene 128-Bit-Kennung, kein kryptographischer Hash und keine neue Abhängigkeit dafür), eine verschobene Datei zieht ihren Eintrag nach, eine geänderte aktualisiert ihn. Kaputte Dateien werden gezählt und übersprungen.
- **Oberfläche:** Neuer Knopf „Bibliothek“ in der Menüleiste öffnet in beiden Layouts dasselbe Fenster: Suchfeld, Filter nach Art und Tag, „Ordner importieren …“ mit Fortschritt, Trefferliste, darunter Tags des gewählten Titels (anlegen und entfernen), „▶ Vorhören“ über den vorhandenen PFL-Weg (Radio-Modus mit Kopfhörer-Ausgang; nie auf Sendung). Titel lassen sich ziehen: in der Timeline auf die Spur unter dem Zeiger, im Radio auf die Playlist (hinten anhängen) oder auf ein Cart-Pad (belegen).
- **Kennung:** Die Inhaltskennung liest ihre 8-Byte-Wörter ohne `chunks_exact` (der neuere Clippy der CI verlangt dafür `as_chunks`).
- **Neue Abhängigkeit:** `rusqlite` 0.41 mit `bundled` (MIT; SQLite selbst ist gemeinfrei, wird mit eingebaut und braucht auf Linux und Windows einen C-Compiler im Bau). `cargo deny` läuft in der CI.
- **Tests:** 1 000 erzeugte Sinus-Dateien werden im Hintergrund importiert (Zeit im Testlauf unter 2 s im Debug-Bau), danach Suche nach Titel und Ordnername, zweiter Import erkennt alle 1 000 als Doppelte, Kopie unter anderem Namen wird erkannt, Abbruch, fehlender Ordner, Symlink-Schleife; Suche, Tags, Grenzen, Transaktion mit Rücksetzen, Migration; Fenster-Zustand (Import, Suche, Tag, Filter) und Zeichnen ohne Fenster.

## 0.18.1 — 2026-10-30

Feedback-Fenster (André: „mittig und in schön … nach 3 Sekunden automatisch schließt“).

- **Oberfläche:** Das Feedback-Fenster steht mittig über der abgedunkelten App, als Karte im Studio-Look mit Überschrift, breitem Textfeld und orangem Senden-Knopf.
- **Danke:** Nach erfolgreichem Senden zeigt es ein grünes Häkchen und „Danke!“ mit einem ablaufenden Balken und schließt sich nach 3 Sekunden von selbst. Fehlermeldungen bleiben stehen, bis man sie schließt. Test dazu.

## 0.18.0 — 2026-10-30

Paket 23 aus M6: Time-Stretch, Tonhöhe und die Abnahme „ganzer Song“.

- **Dehnung (`woa_dsp::stretch`):** Eigene Zeitdehnung nach WSOLA, rein in Rust und ohne neue Abhängigkeit (die Alternative `signalsmith-stretch` hätte eine C++-Bindung und einen Windows-CI-Check gebraucht, das bleibt eine spätere Option). Die Tonhöhe bleibt beim Dehnen erhalten; die Ausgabe hat genau `round(Länge × Faktor)` Samples. Die Tonhöhe ändert sich über Dehnen plus kubische Neuabtastung. Das läuft nur beim Bau des Graphen im Control-Thread, ein kleiner Zwischenspeicher verhindert, dass jeder Graphbau alles neu rechnet; der Audio-Thread spielt das fertige Ergebnis wie jeden anderen Clip.
- **Modell:** Jeder Clip hat `stretch` (Faktor, 0,25 bis 4), `pitch_semitones` (±24) und optional `follow_tempo_bpm`. Neue Befehle `SetClipStretch` (Länge, Faktor und Blenden zusammen), `SetClipPitch` und `SetClipFollowTempo`, je ein Undo-Schritt; ungültige Werte werden abgelehnt. Projektformat 3 wird nur geschrieben, wenn ein Clip eines dieser Felder nutzt; alle anderen Projekte bleiben Format 2. Der Versatz im Ton (`offset_beats`) zählt in Beats des Tons, die Länge in Beats der Zeitachse; Schneiden (Oberfläche und Agent) rechnet das um.
- **Tempo folgen:** Ein Clip mit „Tempo folgen“ merkt sich das Tempo, zu dem sein Ton gehört. Ändert sich das Projekttempo, wird der Ton gedehnt statt abgeschnitten oder verschoben: er bleibt auf dem Raster, die Tonhöhe bleibt. Ohne den Haken verhält sich der Clip wie bisher.
- **Oberfläche:** Alt gedrückt am linken oder rechten Clip-Rand ziehen dehnt die Zeit (ohne Alt kürzt der Rand wie bisher); die Wellenform zeigt die Dehnung schon beim Ziehen, das Raster rastet ein und der Faktor bleibt zwischen 0,25 und 4. In der Clip-Leiste eine neue Zeile „ZEIT“: Dehnung in Prozent, Tonhöhe in Halbtönen, Haken „Tempo folgen“; Doppelklick setzt zurück.
- **Agent:** neue Werkzeuge `stretch_clip`, `set_clip_pitch` und `set_clip_follow_tempo`, beschrieben in `docs/agent.md`.
- **Tests:** Sinus 440 Hz um 10 % gedehnt: Länge ±1 %, Frequenz bleibt (FFT-Spitze); Tonhöhe +2 Halbtöne: FFT-Spitze bei 493,9 Hz ±1 %, Länge gleich; Tempo 120 auf 100 mit Folgen: Länge und Frequenz stimmen; Undo bitgleich, 0 Halbtöne und gleiches Tempo lassen den Render bitgleich; Echtzeit unter `assert_no_alloc`. Modelltests (Undo, Redo, Fades, schlechte Werte, Format), Zustands- und Oberflächentest ohne Fenster und ohne Ton (Alt-Ziehen, ein Undo-Schritt, ohne Alt nur Kürzen), Agent über MCP. Dazu die M6-Abnahme als Test: ein ganzer Song mit geteilten, gedehnten, transponierten und überblendeten Clips und geändertem Tempo wird exportiert und wieder eingelesen (Länge, kein NaN, keine Knackser, Pegel, Frequenz).

## 0.17.2 — 2026-10-30

Sicherheit (#88): Take-Ordner aus einer präparierten Projektdatei können die Oberfläche nicht mehr lahmlegen.

- **Modell:** Ende und Versatz eines Take-Ordners sind auf `MAX_TAKE_BEATS` (1 000 000 Schläge, 24 h bei 400 BPM passen) begrenzt; größere Werte lehnt `check()` ab. Test dazu.
- **Oberfläche:** Die Wellenform im Comping-Bereich zeichnet nur noch die sichtbaren Spalten und hört am Ende der Peaks auf.

## 0.17.1 — 2026-10-30

Sicherheit: zwei Funde der automatischen Prüfung behoben.

- **Modell (#79):** Die Routing-Prüfung beim Laden erkennt Kreise jetzt in einem Durchgang über den ganzen Signalgraphen (O(V+E)) statt mit einer Suche je Verbindung; doppelte Sends und IDs prüfen Mengen statt Listen. Eine präparierte Projektdatei mit vielen Spuren und Sends legt das Laden nicht mehr lahm. Tests: 2000 Busse in Kette laden schnell, ein in die Datei geschmuggelter Kreis wird abgelehnt.
- **Agent (#83):** `write_png` schreibt wie der Export in eine neu angelegte Temp-Datei und setzt sie per `rename` an den Zielnamen; ein untergeschobener Symlink wird ersetzt, nicht beschrieben. Test dazu.

## 0.17.0 — 2026-10-30

Paket 22 aus M6: Aufnahme mit Latenzausgleich, Punch-in, Loop-Takes und Comping.

- **Modell:** Neuer Take-Ordner je Spur (`TakeFolder`): die Takes einer Strecke, dazu der Comp als Liste von Abschnitten. Der Comp wird als gewöhnliche Clips der Spur abgelegt, daher bleiben Engine, Export und Editoren unverändert. Ein Befehl `SetTakeFolder` mit Undo und Redo (ein Schritt je Auswahl); ungültige Takes, Bereiche und Pfade werden abgelehnt. Projektformat 3 wird auch geschrieben, wenn eine Spur einen Take-Ordner hat; alle anderen Projekte bleiben unverändert. `plan_recording` rechnet Latenz, Punch und Loop-Runden auf Samples genau.
- **Aufnahme:** Die Aufnahme wird ein Clip an der Abspielkopf-Position beim Start, die Gerätelatenz wird am Dateianfang übersprungen. Ein Punch-Bereich (in Takten) behält nur den Teil darin. Bei eingeschalteter Schleife entsteht je volle Runde ein Take (höchstens 64, eine angefangene Runde wird verworfen), alle als Ausschnitte einer einzigen Aufnahmedatei.
- **Oberfläche:** Schalter und Takt-Felder für Punch in der Transportleiste. Unter der Spur ein aufklappbarer Take-Ordner mit dem Comp-Streifen, aufgeklappt eine Zeile je Take mit Wellenform. Bereich ziehen oder Takt anklicken wählt den Take für diese Stelle; der Papierkorb löscht den Ordner (Strg+Z holt ihn zurück).
- **Tests:** Fake-Eingang mit künstlicher Latenz ohne Gerät und ohne Ton: Impuls liegt nach dem Ausgleich auf dem Sample, Punch, Runden am Null-Ausgang gleich Zahl der Takes, Comp-Render mit Undo; Modelltests (Undo, Redo, schlechte Eingaben, Format) und Zustandstests der Oberfläche.

## 0.16.0 — 2026-10-30

Paket 21 aus M6: Schnitt und Fades.

- **Modell:** Jeder Clip hat jetzt eine Kurve für Ein- und Ausblendung (Linear, Gleiche Leistung, S-Kurve) und eine Clip-Lautstärke in dB. Neue Befehle `SetClipFadeCurves` und `SetClipGain`, je ein Undo-Schritt; ungültige Werte werden abgelehnt. Projektformat 3 wird nur geschrieben, wenn ein Clip Gain oder eine andere Kurve als linear hat; alle anderen Projekte bleiben Format 2, ältere Dateien laden unverändert.
- **Engine:** Die Blenden folgen der gewählten Kurve, die Clip-Lautstärke wirkt vor der Spur. Überlappen sich zwei Clips auf einer Spur, blendet die Engine automatisch über (Gleiche Leistung über die Länge der Überlappung, wenn sie länger ist als die eigenen Blenden); der Offline-Render nutzt dieselbe Regel. Schneiden gibt bei Positionen auf ganzen Samples bitgleiches Audio; bei krummen Tempi kann ein Sample Versatz bleiben (getestet).
- **Oberfläche:** Der Griff am oberen Clip-Rand zieht die Ein- und Ausblendung (ein Undo-Schritt je Ziehen), die Blende wird mit ihrer Kurve gezeichnet. In der Clip-Leiste: Kurvenwahl je Seite, Clip-Lautstärke und „Normalisieren“ (Spitze des Clip-Teils auf −1 dBFS). Überlappungen zeigen die automatische Überblendung. Die Wellenform ist mit der Clip-Lautstärke skaliert.
- **Agent:** neue Werkzeuge `split_clip`, `set_fade_curves` und `set_clip_gain`, beschrieben in `docs/agent.md`.
- **Tests:** Nulltest nach dem Schneiden (zusammengesetzt gleich Original), Schneiden bei krummem Tempo, Mitte löschen; Kurventabellen und Monotonie; Überblendung gemessen (RMS bleibt bei Gleicher Leistung, bei linear sinkt sie); Klick-Erkennung; Clip-Gain; alles unter `assert_no_alloc`. Modell, Zustand, Agent über MCP und ein Oberflächentest ohne Fenster und ohne Ton (Blende ziehen, Strg+Z).

## 0.15.0 — 2026-10-30

Paket 20 aus M6: Timeline-Editor.

- **Oberfläche:** Die Arrangement-Ansicht hatte schon Lineal, Wellenformen, Zoom, Verschieben, Kürzen und Löschen; neu sind Raster, Kopieren und Einfügen, Verdoppeln, das Hineinziehen von Audiodateien und ein Lineal in Sekunden. Das Raster (Aus, Takt, 1/4, 1/8, 1/16) gilt für Verschieben, Kürzen und Einfügen. Strg+C, Strg+X und Strg+V kopieren, schneiden aus und fügen am Abspielkopf ein (auf die gewählte Spur, wenn der Spurtyp passt), Strg+D verdoppelt direkt hinter der Auswahl. Audiodateien lassen sich aufs Fenster ziehen: die erste landet auf der Spur unter dem Zeiger an der Zeigerposition, jede weitere auf einer neuen Spur an derselben Stelle. Der Schalter „Takte/Sekunden“ stellt die Lineal-Beschriftung um; die Teilung folgt dem Zoom.
- **Zustand:** `AppState::copy_clips`, `paste_clips` und `duplicate_clips`, jeweils ein Undo-Schritt, neue Ids, Spurart wird geprüft. `WoaApp::without_audio` baut die Oberfläche ohne Audiogerät für Tests.
- **Tests:** Modelltests für Verschieben, Trimmen, Spurwechsel, Kopieren, Einfügen und Verdoppeln mit Undo und Redo; Rundlauf Speichern und Laden; Oberflächentest ohne Fenster und ohne Ton (zwei Dateien ablegen, Clip ziehen, Rand ziehen, Strg+Z, Strg+Y, Strg+D, Speichern, neu öffnen); Offline-Render mit Impuls: Verschieben versetzt um genau die Beats in Samples, Kürzen am Anfang lässt das Audio an derselben Stelle, Undo stellt das Sample wieder her. Teilung des Lineals und Raster als Tabellentests.

## 0.14.0 — 2026-10-30

Paket 19 aus M5: Analyse-Fenster.

- **Oberfläche:** Der Knopf „📊 Analyse“ im unteren Dock öffnet ein schwebendes Fenster mit Lautheit (momentan, kurzzeitig, integriert), True Peak, Spektrum mit Halte-Marken, Spektrogramm, Stereo-Scope und Korrelationsanzeige. Die Quelle ist der Master oder eine einzelne Spur; „Zurücksetzen“ beginnt die integrierte Messung neu. Ist das Fenster zu, kostet es im Audio-Thread nur einen Atomic-Lesezugriff je Block.
- **Messung (`woa_dsp::analysis`):** Lautheit nach BS.1770/EBU R128 (K-Filter, Blöcke mit 75 % Überlappung, absolutes und relatives Gate), True Peak mit vierfacher Überabtastung, Spektrum mit Hann-Fenster und `realfft` (MIT/Apache), Oktavbänder, Spektrogramm, Korrelation, Balance und Seiten-zu-Mitte-Verhältnis. Der K-Filter der Engine liegt jetzt dort und wird von `woa_engine::loudness` nur noch weitergereicht.
- **Engine:** Neue Abgriffe (`woa_engine::tap`) am Master, an jedem Bus und an jeder Spur: lock-freier Ring (`rtrb`), Frames bleiben als Paar zusammen, ein voller Ring zählt Verluste statt zu warten. Das Signal bleibt bitgleich.
- **Agent:** `analyze` meldet zusätzlich True Peak, höchste momentane und kurzzeitige Lautheit, Spektrum-Spitze und Oktavbänder, Korrelation, Balance und Hinweise; mit `track` misst es eine Spur allein. Neues Werkzeug `spectrogram_png` schreibt ein Spektrogramm als PNG in den Projektordner.
- **Tests:** EBU-Referenzsignale (1 kHz bei −23 und −33 dBFS ergibt −23 und −33 LUFS), Sinus landet auf dem richtigen Bin mit richtigem Pegel, Korrelation +1 bei Mono und −1 bei Gegenphase, Snapshot des Spektrogramms (`insta`), Abgriffe unter `assert_no_alloc`, Agent über MCP einschließlich PNG-Prüfung.

## 0.13.0 — 2026-10-30

Paket 18 aus M5: Automation und Sidechain (Sidechain für Effekte auf Bussen und Live-Einfahren von Sends und Effekt-Reglern folgen später).

- **Modell:** Jede Spur kann je Ziel (Lautstärke, Pan, Send, Effekt-Regler) eine Automationskurve mit Betriebsart Lesen, Schreiben oder Latch haben (`Command::SetAutomation`). Kompressor und Gate nehmen ein Sidechain-Signal einer anderen Spur (`Command::SetInsertSidechain`); Kreise über Sidechains werden abgelehnt, Spur löschen trennt Sidechains im selben Undo-Schritt. Projektformat 3 wird nur geschrieben, wenn Automation oder Sidechain vorkommt; alle anderen Projekte bleiben Format 2, ältere Dateien laden unverändert.
- **Engine und DSP:** Kurven laufen im Audio-Thread sample-genau (Blockgröße ohne Einfluss, Loop-Sprung exakt, bei Stopp gilt der Wert am Cursor). Lautstärke, Pan (auch Balance am Bus) und Sends rechnen die Kurve pro Sample (dB-Interpolation), Effekt-Regler folgen in 64-Sample-Schritten. Nur wo eine Kurve spielt, tauscht der Graph die Knoten; Projekte ohne Automation klingen bit-gleich wie vorher. Kompressor und Gate einer Spur nehmen einen Sidechain-Schlüssel (Signal der Quelle nach Fader, vor Pan); jeder dynamische Effekt hat einen Absenkungsmesser (`ProjectGraph::reductions`). Kurven im Modus Schreiben oder Latch spielen nicht ab.
- **Agent:** neue Werkzeuge `add_automation` (Kurve für Lautstärke, Pan, Send oder Effekt-Regler, ein Undo-Schritt), `remove_automation` und `set_sidechain` (Kompressor oder Gate misst den Pegel einer anderen Spur; Schleifen werden abgelehnt). Beschrieben in `docs/agent.md`.
- **Oberfläche:** Neuer Reiter „Automation“ im Studio-Dock: Kurve einer Spur für Lautstärke, Pan, Sends und Effekt-Regler zeichnen (Klick setzt, Ziehen verschiebt, Rechtsklick löscht, ein Undo-Schritt je Änderung), Betriebsart Lesen, Schreiben oder Latch. Der Lautstärke-Regler folgt beim Abspielen der Kurve. Kompressor und Gate haben die Auswahl „Sidechain“ (nur Spuren) und eine Absenkungs-Anzeige. Controller-Fahrten (Fader und Pan, per MIDI-Lernen gebunden) werden in den Modi Schreiben und Latch aufgenommen und beim Stopp als eine Kurve eingetragen; währenddessen bleiben fester Reglerstand und Undo-Verlauf unberührt.
- **Tests:** Rampe mit drei Punkten gegen die dB-Rechnung Sample für Sample in Blöcken zu 1, 128 und 1000; Pan-Fahrt; Send-Kurve; Regler-Kurve (+12 dB); Loop-Sprung; Stopp am Cursor; Ducking per Sidechain gemessen (Musik mehr als 6 dB leiser, Absenkungsmesser schlägt aus); alles zusammen unter `assert_no_alloc`. Controller-Fahrt über einen virtuellen MIDI-Port wird zur Kurve, der Offline-Render folgt ihr.

## 0.12.0 — 2026-10-30

Paket 17 aus M5: Alle eingebauten Effekte.

- **Neue Effekte:** Gate, De-Esser, Hall, Echo (frei oder tempo-synchron, mit Ping-Pong), Chorus, Sättigung (weich, Röhre, hart) und Rauschunterdrückung (RNNoise über `nnnoiseless`, BSD-3, nur bei 48 kHz, 30 ms Verzögerung). Der EQ hat jetzt Tiefen- und Höhenregal und eine zweite Glocke; die neuen Bänder stehen flach, alte Projekte klingen unverändert.
- **Oberfläche:** „+ Effekt“ bietet alle zehn Arten und die Vorlage „Sprachkette“ (Hochpass-EQ, Gate, EQ, Kompressor, De-Esser) als einen Undo-Schritt. Jeder Effekt hat Voreinstellungen („Preset ▾“, ein Undo-Schritt) und Auswahllisten für Notenwert, Ping-Pong und Kennlinie.
- **Modell:** `EffectKind` mit zehn Arten, Auswahl-Regler (nur ganze Werte), Voreinstellungen je Effekt, Vorlage `VOICE_CHAIN`; `Insert::with_values` und `Insert::with_preset`. Projektformat bleibt 2, alte Dateien füllen neue Regler mit Vorgaben auf.
- **Engine:** Der Graph gibt Effekten das Projekttempo (synchrones Echo) und rechnet im Export (`render_project`, `render_clips`) die Rauschunterdrückung ohne Arbeits-Thread, mit demselben Ergebnis wie live. Live rechnet ein eigener Thread, damit der Audio-Thread nie allokiert; zählt Aussetzer statt zu warten.
- **Agent:** neue Werkzeuge `list_effects` und `insert_voice_chain`; `insert_effect` nimmt alle Arten und ein `preset`.
- **Fehler beim Messen behoben:** Die Allpässe des Halls hatten Verstärkung über 1 (Hall rund 11 dB zu laut); die Sättigung wurde bei der Röhrenkennlinie falsch normiert und konnte über Vollaussteuerung gehen.
- **Tests:** Messungen mit erzeugten Signalen für jeden Effekt, bitgenauer Bypass, `assert_no_alloc` um jeden Process-Aufruf, Golden-File `effects_p17_chain.wav`, Sprachkette im Graphen.

## 0.11.0 — 2026-10-30

Paket 16 aus M5: Mixer mit Bussen, Sends und Inserts.

- **Oberfläche:** Im Mixer-Dock hat jeder Zug Ausgangswahl, Sends (Pegel, vor/nach Fader) und eine Effektkette mit Bypass und Reglern; „+ Bus“ legt Gruppenspuren an, Bus-Züge haben Pegelanzeige. Der Player baut den Graphen neu, wenn sich Routing oder Effektkette ändern, und schickt Reglerwerte geglättet. Spuren löschen trennt Sends und Ausgänge im selben Undo-Schritt.

- **Modell:** Bus-Spuren sind jetzt verschaltbar. Jede Spur hat einen Ausgang (Master oder Bus), Sends (vor oder nach dem Fader) und eine Insert-Kette. Alles sind Commands mit genauem Gegenstück (Rückgängig/Wiederholen); ein Bus auf sich selbst oder im Kreis wird abgelehnt. Projektformat 2, Dateien der Version 1 laden unverändert.
- **Engine:** Der Graph kennt Stereo-Nodes (zwei Ein-, zwei Ausgänge). Daraus baut `build_project_graph` das Mischpult: Insert-Kette je Spur, Ausgang auf Master oder Bus, Sends vor oder nach dem Fader, Bus-Züge mit Inserts, Fader, Balance und Pegelmesser. Solo einer Spur hält die Busse hörbar, die sie speist. Änderungen an Reglern, Sends und Effekt-Parametern gehen geglättet ohne neuen Graphen. Der Golden-Render des alten Mixes bleibt bit-gleich innerhalb 1e-5, ein neuer Golden-Render (`mixer_bus_send_05s.wav`) hält Bus, Send und Effekte fest.
- **Agent:** Werkzeugliste in `docs/agent.md` ergänzt. Neue Werkzeuge `insert_effect`, `set_param`, `set_insert_bypass`, `remove_insert`, `set_output`, `set_send`, `remove_send`; `remove_track` trennt Sends und Ausgänge im selben Undo-Schritt.
- **DSP:** Neues Modul `fx` mit EQ (Hochpass und Glocke, Überblendung beim Umstellen), Kompressor und Limiter als Mono- und Stereo-Node (verkoppelt) samt geglättetem Bypass. Biquad-EQ, Kompressor, Limiter und Gate liegen jetzt in `woa-dsp` (vorher `woa-radio`); der Radio-Bus nutzt sie von dort.

## 0.10.0 — 2026-10-30

Paket 15: Cue-Punkte und Vorhören (PFL), aus M7 nach M4 vorgezogen (André, 30.10.2026).

- **Cue-Punkte je Titel:** Cue-In, Cue-Out, Ramp (Intro-Ende), Hook und Fade-Out-Marker, in Sekunden in `radio.json`. Der Mixer spielt von Cue-In bis Cue-Out; mit Fade-Out-Marker dauert die Überblendung von dort bis Cue-Out, sonst gilt die eingestellte Überblendung. Restzeit, Gesamtlänge und Offline-Render rechnen genauso.
- **Cue-Editor:** Rechtsklick auf einen Titel → „Cue-Punkte …“. Wellenform mit allen Markern, Klick setzt den gewählten, Zahlenfelder für den Feinschliff, Prüfung vor dem Übernehmen. „Übernehmen“ ist ein Rückgängig-Schritt (eigener Verlauf für Cue-Punkte, Rückgängig/Wiederholen im Editor).
- **Ramp-Countdown:** Im Sende-Deck läuft neben der Restzeit „RAMP n“ bis zum Intro-Ende herunter.
- **Vorhören (PFL):** Einstellungen → „Vorhören (PFL)“: aus, Kanal 3/4 des Sendegeräts oder ein zweites Gerät, gespeichert in `config.json`. Titel (ab Cue-In oder ab Hook), Carts und jeder Marker im Editor lassen sich vorhören. Der PFL-Spieler ist ein eigener Spieler mit eigener `rtrb`-Queue ohne Weg in Mixer, Mitschnitt oder Sende-Ring; im Audio-Thread ohne Allokation und Sperre, 5 ms Ein- und Ausblenden gegen Knacken.
- **Agent:** neues Werkzeug `set_cue_points` (prüft die Werte, gibt die alten zurück); `run_order` zeigt Cue-Punkte und gespielte Länge.
- Engine: `start_output_aux` schreibt einen zweiten Stereo-Callback auf Kanal 3/4 desselben Geräts.

## 0.9.1 — 2026-10-30

- Neues Studio- und Sende-Design (von Gemini gebaut, von André übernommen: „gefällt mir ganz gut“): Tiefenstaffelung und Hardware-Rahmen, Status-Pillen, 20-Segment-LED-Meter; Radio-Ansicht in Modulen (Sende-Deck mit Master-Uhr, Playlist mit LUFS, Cart-Wall im Drumpad-Stil, Processing-Rack).
- Studio: unteres Dock mit Mischpult (Kanalzüge mit M/S/Aufnahme/Löschen, LED-Meter, Gain, Pan) und angedockter Piano-Roll (lösbar als Fenster); Solo und Pan im Spurkopf, Kontextmenü (Umbenennen, leerer MIDI-Clip, Audio importieren, Spur löschen); Tempo im LCD editierbar, Loop mit Klammer im Lineal; Audio importieren, leere MIDI-Clips per Doppelklick.
- MIDI-Learn (Paket 14) bleibt an Start, Stopp, Mikro, Pegeln und Stumm erhalten.

## 0.9.0 — 2026-10-30

Paket 14 aus M4 (#5): Controller und MIDI-Learn.

- **Controller wählen:** Einstellungen → „Controller und MIDI-Learn …“. Ein zweiter MIDI-Eingang neben dem Keyboard, mit Hotplug wie dort (abziehen, wieder einstecken, verbindet nach höchstens 2 s neu).
- **MIDI-Learn per Rechtsklick:** Spur-Fader (dort auch Pan und Solo), Stumm-Knopf, Start/Weiter, Stopp, Mikro, die neuen Pegelregler und jede Cart-Taste haben „MIDI lernen“. Die nächste Bewegung oder das nächste Pad am Gerät wird gebunden; eine Quelle steuert genau ein Ziel. Das Layout (Produzent/Radio) lässt sich im Controller-Fenster auf einen Taster legen. Das Fenster zeigt die Liste (Quelle, Ziel) mit „Löschen“.
- **Gespeichert je Gerät** in `config.json` (nicht im Projekt), unter dem Gerätenamen ohne ALSA-Nummern: Nach Neustart oder Wieder-Einstecken an anderem USB-Port gilt dieselbe Zuordnung.
- **Takeover:** Ein Fader springt nicht. Er greift erst, wenn er den Wert auf dem Bildschirm erreicht oder überquert; stellt die Maus den Wert um, wird neu abgeholt. Spur-Fader und Pan gehen nach 300 ms Ruhe als ein Undo-Schritt ins Projekt, nicht je CC.
- **Pads lösen Carts aus** (zweites Drücken hält an, wie der Klick), Taster als CC lösen nur beim Drücken aus. **LEDs:** Hat das Gerät einen MIDI-Eingang, bekommen Pads Note-Out (127 an, 0 aus) für laufende Carts, offenes Mikro, laufende Sendung, stumme oder Solo-Spuren und das aktive Layout; gesendet wird nur bei Änderung.
- **Pegelregler im Radio-Layout:** Musik, Carts und Mikro von −60 dB (aus) bis +6 dB, im Mixer geglättet (kein Zipper beim Fader). Sie werden nicht gespeichert und starten bei 0 dB.
- **Echtzeit:** Die Controller-Meldungen gehen im MIDI-Thread in eine eigene `rtrb`-Queue (1024 Plätze), die Oberfläche leert sie je Bild und fasst mehrere Werte eines Faders zum letzten zusammen. Der MIDI-Thread weckt die Oberfläche sofort, ein Pad startet den Cart ohne auf das nächste Bild zu warten. Der Audio-Callback bekommt nur die vorhandenen Mixer-Befehle; neu ist dort `SetLevel`, ohne Sperre und Allokation.

## 0.8.0 — 2026-10-30

Paket 13 aus M4 (#5): Piano-Roll.

- **Editor für MIDI-Clips:** Doppelklick auf einen MIDI-Clip öffnet die Piano-Roll. Klick ins Leere zeichnet eine Note (einen Rasterschritt lang), Ziehen verschiebt in Zeit und Tonhöhe, der rechte Rand ändert die Länge, Doppelklick oder Entf löscht. Mehrfachauswahl mit Strg/Umschalt+Klick, Rahmen ziehen oder Strg+A. Alt beim Ziehen schaltet das Raster ab, Esc schließt.
- **Raster und Quantisieren:** 1/4 bis 1/32, auf Wunsch Triolen. „Quantisieren“ (oder Q) setzt die Einsätze der Auswahl, ohne Auswahl aller Noten, aufs Raster; die Längen bleiben, das Pedal bleibt, wo es war.
- **Velocity-Leiste** unter den Noten: senkrecht ziehen setzt die Velocity der Auswahl (sonst der nächsten Note). Die Helligkeit der Noten zeigt die Velocity.
- **Klaviatur** links mit Notennamen (C4 = 60). Klick hört die Taste vor, solange gedrückt, über das Live-Instrument der Spur; ohne aktiven Ausgang bleibt es still. Zoom waagerecht (Regler oder Strg+Mausrad), Zeilenhöhe, Umschalt+Mausrad blättert.
- **Alles über `SetNotes`:** Jede Bearbeitung ist ein Befehl und ein Undo-Schritt, Strg+Z/Strg+Y gehen sie einzeln zurück. Die Bearbeitungen selbst (`add_note`, `move_notes`, `resize_notes`, `delete_notes`, `set_velocity`, `quantize`, `grid_beats`) liegen in `woa_model::MidiData`, damit Oberfläche und Agent denselben Code nutzen.
- **Agent:** neue Werkzeuge `quantize_notes` (Raster, Triolen, Enden, nur bestimmte Noten) und `edit_notes` (Notenliste ersetzen: verschieben, Länge, Velocity, löschen; Pedal bleibt).
- Fotomodus: `WOA_PIANOROLL=1` öffnet den ersten MIDI-Clip, `WOA_THEME=hell` schaltet zur Probe auf das helle Thema (nur Fenster und Piano-Roll, die Zeitleiste bleibt dunkel).
- Tests: Bearbeitungsbefehle (Raster, Quantisieren mit und ohne Auswahl und Enden, Verschieben als Ganzes mit Grenzen, Auswahl folgt der neuen Reihenfolge, Länge, Löschen, Velocity), sechs Bearbeitungen als einzelne Undo-/Redo-Schritte, Offline-Render vor und nach dem Quantisieren (Einsätze vorher daneben, nachher höchstens 48 Samples am Raster), Agent-Loop über MCP mit Fehlerfällen und Undo.

## 0.7.2 — 2026-10-30

- Konzept: Lücken im Sendestudio aufgenommen – Cue-Punkte je Titel (Cue-In/-Out, Ramp, Hook, Fade-Out), Vorhören (PFL) auf eigenem Ausgang, Stundenuhr mit AutoDJ-Rotation und festen Zeit-Events, Voice-Tracking. Eingeplant in M7 (Bibliothek in SQLite, Cue-Punkte, PFL) und M9 (Automation, Voice-Tracking, Gäste).

## 0.7.1 — 2026-10-30

- MIDI-Import: Noten ohne „Aus“ zählen gegen die Obergrenze mit. Eine präparierte Datei nur aus Note-Ons konnte die Grenze umgehen und den Speicher füllen; das Auflösen ist jetzt FIFO mit `VecDeque` statt `remove(0)` (Fund der Sicherheitsprüfung).

## 0.7.0 — 2026-10-30

Paket 12 aus M4 (#5): MIDI aufnehmen und als Noten speichern.

- **MIDI-Clips im Projekt:** Ein Clip kann jetzt Noten (Beginn, Länge, Tonhöhe, Anschlag) und Controller (Haltepedal CC 64) tragen statt einer Audiodatei, gespeichert in `project.json` (Feld `midi`, alte Projekte laden unverändert). Neuer Befehl `SetNotes` ändert den Inhalt als ein Undo-Schritt; beim Laden und vor jedem Befehl werden die Noten geprüft (Tonhöhe, Anschlag, Länge, Obergrenze).
- **Aufnahme auf der scharfen MIDI-Spur:** Aufnahme-Knopf wie bei Audio. Jedes Event bekommt im MIDI-Thread die Zeit, eine Sample-Uhr des Audio-Threads (Seqlock, ohne Sperre und Allokation im Callback) rechnet sie auf das Sample genau in Transportposition um, unabhängig von der Blockgröße. Stopp legt einen Clip „MIDI-Take“ an (ein Undo-Schritt); ohne MIDI-Spur entsteht eine neue.
- **Abspielen und Mixdown:** Die MIDI-Spur spielt ihre Clips sampelgenau mit dem Spur-Synthesizer (16 Stimmen), zusammen mit dem Live-Keyboard. Springen und Stopp schicken „alle Noten aus“, nichts hängt. Der Export (WAV, FLAC, MP3) und der Agent-Export enthalten die MIDI-Spuren. Änderungen an Noten bauen den Graphen neu.
- **`.mid` importieren und exportieren:** Datei → „MIDI importieren (.mid) …“ legt den Inhalt als Clip am Kopf an, „MIDI-Clip exportieren (.mid) …“ schreibt den ausgewählten Clip. Eigener Leser und Schreiber (`woa_io::smf`) für Standard-MIDI-Dateien Format 0 und 1: Running Status, Note On mit Velocity 0, Tempo, Spurname, SysEx und Meta werden verstanden; geschrieben wird Format 0 mit 480 Ticks je Viertel. Kein `midly`, weil dessen Lizenz (Unlicense) in #5 noch offen ist.
- **Agent:** `generate_notes` mit `midi: true` legt auf einer MIDI-Spur einen bearbeitbaren MIDI-Clip an statt einer WAV-Datei. Neues Werkzeug `read_notes` liest Noten und Pedal eines MIDI-Clips zurück.
- `CHANGELOG.md` ohne die versehentlich mitgelieferten Konfliktmarken zwischen 0.6.2, 0.6.1, 0.6.0 und 0.5.2.
- Tests: Modell-Roundtrip, `SetNotes` mit Undo/Redo, ungültige Noten abgelehnt; Wiedergabe unter `assert_no_alloc` mit Einsatz höchstens 2 Samples daneben, Offline-Export mit Einsatzprüfung, Stopp und Springen ohne hängende Note; virtueller ALSA-Port spielt ein festes Muster mit Pedal, aufgenommen höchstens 1 ms daneben (gemessen: 0,2 ms); `.mid`-Roundtrip und eine von Hand nach der Spezifikation geschriebene Fremddatei (Format 1, Tempospur, Running Status); Agent-Loop über MCP (Noten schreiben, lesen, exportieren, Fehlerfälle, Undo).

## 0.6.2 — 2026-10-30

- CLAUDE.md: Lokal laufen nur clippy und die Tests der geänderten Crates, die volle Testsuite nur in der CI (Pflicht auf main). André: „ja, mach beides“.

## 0.6.1 — 2026-10-30

- Bauen ohne volle Debug-Infos: eigener Code mit `line-tables-only` (Backtraces mit Zeilen bleiben), Abhängigkeiten ganz ohne. Kleinerer Bauordner, schnelleres Linken, weniger RAM auf dem 7-GB-Notebook. André: „kann man das nicht effizienter???“

## 0.6.0 — 2026-10-30

Paket 11 aus M4 (#5): Keyboard spielt live.

- **MIDI-Eingang** (`woa_io::midi`, mit `midir` 0.11, MIT): Eingänge auflisten, verbinden, alle 2 s neu abfragen. Wird das Gerät abgezogen, trennt WOA die Verbindung, schickt „Pedal los“ und „alle Noten aus“ (nichts hängt) und verbindet von selbst wieder, sobald es zurück ist, auch mit neuer ALSA-Nummer. Log-Zeile je Auswahl, Verbindung, Trennung und Fehler (gleiche Fehler nur einmal).
- **Parser** für Note On/Off (Note On mit Velocity 0 = Note aus), Anschlagstärke, Haltepedal (CC 64), Pitchbend, Program Change und All Notes Off (CC 120/123); versteht Running Status, überspringt SysEx und Echtzeit-Bytes.
- **Scharfe MIDI-Spur spielt:** Neuer Knopf „+ MIDI-Spur“. Die erste scharf geschaltete MIDI-Spur (sonst die ausgewählte, wenn sie eine MIDI-Spur ist) bekommt ein Live-Instrument (Synthesizer, 16 Stimmen) und klingt durch Fader, Pan, Mute und Solo der Spur, auch ohne ▶. Die Events gehen über die `rtrb`-Queue in den Audio-Thread, dort ohne Sperre und ohne Allokation.
- **Synthesizer** versteht jetzt Haltepedal und Pitchbend (±2 Halbtöne), der SoundFont-Player reicht beides weiter. Der Sampler ignoriert beides noch.
- **Einstellungen → MIDI-Eingang:** Aus oder ein Gerät, gespeichert in `config.json` (`midi_input`). Ein gewähltes, nicht eingestecktes Gerät steht mit „(nicht verbunden)“ in der Liste.
- **MIDI-Lampe** rechts in der Kopfzeile: grau = aus oder verbunden in Ruhe, rot = gewählt, aber nicht da, grün bei jedem Event. Der Tooltip nennt Gerät und gespielte Spur.
- Tests: Parser mit Bytefolgen; virtueller ALSA-Port spielt eine leise und eine laute Note mit Pedal in die Instrument-Node (Offline-Render unter `assert_no_alloc`: nicht still, laut mehr als dreimal so laut wie leise, Pedal hält, Pedal los gibt frei), schließt den Port (Status „wartet“, keine hängende Note) und öffnet ihn wieder (verbunden, Events kommen an); Live-Instrument im Projekt-Graphen klingt ohne laufenden Transport.
## 0.5.2 — 2026-10-30

- **Dauertest: Bündel zu später Blöcke behoben.** Ursache: `broadcast-ui` rechnete den Mixer und dekodierte Titel in derselben Schleife. Läuft die Playlist im Kreis, wird der Titel beim Neustart neu dekodiert (rund 250 ms für 3 min MP3), und der Takt stand so lange still. Belegt mit Zeitstempeln: alle 25 späten Blöcke im Probelauf fielen auf t = 197,8 s, das Ende des 197,6-s-Titels, unmittelbar nach „decode_ms=255“; die Verspätung fiel von 296 ms in 11,6-ms-Schritten (ein Block) ab. Jetzt läuft der Mixer wie in der Oberfläche in einem eigenen Takt-Thread, der nur rechnet; Dekodieren, Titel und Bericht macht der Steuer-Thread. Jeder späte Block und jeder Neustart der Playlist steht als `ui-log` auf stderr, mit Unix-Zeit (Abgleich mit dem Journal), Prozess-ID (erste oder neu gestartete Oberfläche) und Sekunden seit Start; dazu ein Hinweis, wenn der Steuer-Thread über 100 ms still stand, und bei späten Blöcken Seitenfehler und Auslagerung des Prozesses. Probe nach dem Umbau: zwei Läufe à 10 min gegen lokalen Icecast, beide bestanden (0 zu spät, 0 verloren, 0 Puffer leer, 0 Neuverbindungen, Notfall nach 2009 und 2113 ms), obwohl der Steuer-Thread bei jedem Neustart der Playlist 315–382 ms stand.
- Nicht geklärt: Im ersten 10-min-Probelauf nach dem Umbau stand der ganze Rechner rund 5 s (Oberfläche und Prüfprozess gleichzeitig, Nutzer-Journal von 07:16:30 bis 07:18:32 leer, dann viele Timer auf einmal). Vermutung: Ruhezustand oder Speicherdruck des Notebooks, nicht der Mixer. Die neuen Logzeilen trennen das beim nächsten Mal.
- **CI-Test wackelt nicht mehr unter Windows (#66).** `broadcast_test_survives_ui_crash_against_icecast` prüft die Logik (Notfall übernimmt binnen 2,5 s, Oberfläche übernimmt wieder, keine verlorenen Samples, keine Neuverbindung, Bytes beim Server). Zu späte Blöcke und leere Sendepuffer auf dem geteilten Runner werden nur gemeldet; die Grenzen im Code und die strenge Zählung im echten Dauertest bleiben.

## 0.5.1 — 2026-10-30

- `cargo build -p woa-headless` ohne Feature `device` baut wieder. `RADIO_BUFFER_FRAMES` stand im Modul `device`, das ohne Feature fehlt; die Konstante liegt jetzt im Wurzelmodul von `woa-engine`, `device` reicht sie weiter. Gefunden beim Bau von Paket 10.

## 0.5.0 — 2026-10-29

Paket 10 aus M3 (#4): Dauertest der Sendung.

- **`woa-headless broadcast-test`**: Sendeprozess (`woa-streamd`-Kern) und Oberfläche als eigener Kindprozess (`broadcast-ui`). Die Oberfläche lässt den Sendemixer samt Sendebus-Processing im Takt der Uhr laufen, ohne Audiogerät; es wird nichts hörbar abgespielt. Die Playlist läuft im Kreis. Nach der Hälfte der Zeit (`--kill-ui-at`) wird die Oberfläche hart beendet. Der Test misst, wann die Notfall-Playlist übernimmt (Grenze 2,5 s), startet die Oberfläche neu und misst die Rückgabe. Gezählt werden zu spät gerechnete Blöcke, verlorene Bytes auf der lokalen Verbindung, leere Sendepuffer bei verbundener Oberfläche und neue Verbindungen zum Server. Bestanden nur, wenn alles null ist. Eine Fortschrittszeile je Minute, am Ende ein Bericht (`--report bericht.json`). Exit 0 = bestanden, 5 = Fehler gezählt, 3 = Kennwort abgelehnt. Vorgabe 8 Stunden.
- **Icecast als Server** (`protocol: "icecast"`, `mount: "/stream"` in `stream.json`): Quelle per HTTP `PUT` mit Basic-Auth (`source:<kennwort>`), das Kennwort steht nur im Anfragekopf. Titel und Hörerzahl gibt es bei Icecast noch nicht. Shoutcast bleibt Vorgabe.
- **`live_underruns`** in `streamd-status.json`: wie oft der Sendeprozess bei verbundener Oberfläche keinen Sendemix im Puffer hatte.
- **`tests/broadcast/icecast.sh start|stop`**: lokaler Icecast im Container (`libretime/icecast:2.4.4`, nur 127.0.0.1:8000) mit Wegwerf-Kennwörtern im Laufzeitordner.
- Tests: `broadcast-test` gegen einen Mini-Icecast im Test (14 s, Absturz der Oberfläche nach 5 s, Notfall unter 2,5 s, Rückgabe, keine Unterläufe, kein Neuverbinden) und mit falschem Kennwort; Base64 nach RFC 4648.

## 0.4.0 — 2026-10-29

Paket 9 aus M3 (#4): Sendebus-Processing.

- **Sendebus mit Lautheitsregelung, Kompressor und Limiter** (`woa_radio::dynamics::BusProcessor`). Die Regelung misst die Lautheit am Sendeausgang (K-Filter nach BS.1770, gleitend über etwa 3 s) und führt die Verstärkung langsam zum Ziel aus `radio.json` (`target_lufs`, Vorgabe −16 LUFS): höchstens 3 dB je Sekunde, Bereich ±12 dB. In Pausen unter −45 LUFS bleibt sie stehen und zieht kein Rauschen hoch. Danach folgt ein RMS-Kompressor (2,5 : 1, Schwelle 4 dB über dem Ziel, weiches Knie) und ein Limiter bei −1 dBFS mit sofortigem Angriff und harter Sicherung. Der Limiter hat keine Vorausschau und deshalb keine Latenz; Uhr, Mitschnitt und Sendung bleiben sample-genau. Sendung, Mitschnitt und Abhören bekommen denselben Faktor.
- **Mikrofon-Kette** (`MicChain`): Hochpass 80 Hz, Präsenz +3 dB bei 3 kHz, Gate ab −50 dBFS (schließt um 30 dB, 100 ms Haltezeit), Kompressor 4 : 1 ab −24 dBFS mit +6 dB Aufholverstärkung. Der De-Esser kommt in M5.
- **Schalter und Anzeige im Radio-Modus** unter den Reglern: „Sendebus“ und „Mikro-Kette“, beide an (gespeichert in `radio.json` als `bus_processing` und `mic_processing`). Daneben stehen die Kurzzeit-Lautheit am Ausgang, die Verstärkung der Regelung und die Absenkung durch Kompressor und Limiter, bei offenem Mikro „Gate offen/zu“. Das Agent-Werkzeug `build_playlist` meldet beide Schalter mit.
- Alles im Audio-Thread ohne Allokation (Tests unter `assert_no_alloc`). `radio-render` läuft mit Processing; die Spitze bleibt unter −1 dBFS.
- Tests: Limiter nie über der Decke bei +6 dBFS mit harten Sprüngen; leises und lautes Material landet bei −16 ± 1 LUFS; Regelung steht in Stille; Gate schließt bei Rauschen und öffnet bei Sprache; Hochpass nimmt Rumpeln, Kompressor verkleinert 20 dB Eingangsunterschied auf 3–12 dB; Sendemix mit lautem Titel, Cart und Mikro bleibt unter −1 dBFS und nach 10 s bei −16 ± 2 LUFS.

## 0.3.6 — 2026-10-29

- **Programme in der Quellenliste (Fixes #44).** Belegt am 29.10.: WOA listete seine eigenen Wiedergabe-Streams als „Programm: woa“, zweimal mit demselben Schlüssel. pipewire-alsa nennt sie „PipeWire ALSA [woa]“ und meldet keine Prozess-ID, deshalb griff der Filter über die eigene PID nicht. Chromium meldet sich laut WirePlumber als `application.name = Chromium` und wurde vom Namen her richtig erkannt. Warum es am 29.10. fehlte, ließ sich ohne laufenden Chromium-Stream nicht nachstellen (Wiedergabe auf dem Notebook ist tabu). Vermutung: Der Stream hatte keinen bekannten Ausgang (`sink_index`) und fiel stumm aus der Liste. Widerlegt sind: Die Liste wird nur beim Start gelesen (sie wurde jede Sekunde neu gelesen). Pausierte Streams werden ausgefiltert (es gab keinen Filter). Die Abfrage scheiterte (keine Warnung im Log).
- **Neu:** Ein Stream ohne bekannten Ausgang bleibt in der Liste und nimmt den ersten Ausgang. Eigene Streams erkennt WOA zusätzlich am Programmnamen. Jedes Programm steht einmal in der Liste, ein laufender Stream geht einem pausierten vor. Der Name kommt aus `application.name`, sonst aus dem Programmdatei-Namen, sonst aus dem Stream. Nichtssagende Medientitel („Playback“, „ALSA Playback“) werden weggelassen, echte Titel angehängt. Beim Öffnen fragt die Auswahl sofort neu ab. Ohne Programm steht unten „Programm spielt gerade keinen Ton?“. Die Zuordnung steckt jetzt in `capture::build_sources` und ist mit Beispieldaten aus `wpctl inspect` getestet.

## 0.3.5 — 2026-10-29

- **Man sieht, was man aufnimmt (Fixes #43).** Während der Aufnahme wächst auf jeder aufnehmenden Spur ab dem Startpunkt ein roter Clip „Aufnahme läuft“ mit, darin die laufende Wellenform. Nach dem Stopp tritt an seine Stelle der fertige Clip. Die Zeitleiste wird mitverlängert, wenn die Aufnahme über das bisherige Ende hinausläuft.
- **So kommen die Punkte an:** `woa_engine::live_peaks` sammelt im Schreib-Thread der Aufnahme (cpal) bzw. im Lese-Thread (PipeWire) je 256 Frames den lautesten Betrag über alle Kanäle, dieselbe Einheit wie die Wellenform fertiger Clips. Die Werte gehen über einen lock-freien Ring (`rtrb`) an die Oberfläche; der Audio-Callback bleibt unverändert. Läuft der Ring voll (etwa drei Minuten ohne Abholen), füllt die Oberfläche die fehlenden Stellen als Stille auf, damit die Zeitachse stimmt. Die Wellenform nutzt die Abtastrate der Aufnahme, nicht die der Wiedergabe.
- **Pegel:** Die Pegelanzeigen in der Aufnahmeleiste und an der Spur gab es schon. Neu ist die Übersteuerungslampe „ÜBERSTEUERT“ neben dem Pegel, sobald ein Sample 0 dBFS erreicht; sie bleibt bis zur nächsten Aufnahme an.
- **Keine Mehrlast:** Die Oberfläche zeichnet wie bisher nur während einer Aufnahme alle 40 ms neu, in Ruhe bleibt es bei 2 Bildern je Sekunde (#57). Gezeichnet werden nur sichtbare Spalten.
- Tests: Peak-Sammlung gegen die Wellenform der Mono-Datei (Stereo, zerstückelt angeliefert), schrittweises Abholen, voller Ring; `assert_no_alloc` um das Sammeln (`crates/engine/tests/live_peaks_rt.rs`).

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
  - Titel per `admin.cgi?mode=updinfo` nur bei lokalem Server, weil dort das Kennwort im Klartext in der URL steht; gegen den eigenen Server vorerst keine Titel (v1 kann sie nicht in der Quellverbindung tragen). Hörerzahl aus `/7.html` nur mit `stream.admin_port`; der ist auf dem eigenen Server von außen gesperrt. Die Quellanmeldung auf 8001 läuft im Klartext (bekanntes Risiko, `docs/stream.md`)
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
