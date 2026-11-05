# Auftrag Design Fassung 8: Ordnung und Ruhe (05.11.2026)

Grundlage: Screenshots vom laufenden WOA 0.41.x während einer Sendung (Radio, Einstellungen, Fragen, Abnahme).
Maßstab: `docs/design/woa-design-system/` (Fassung 8), Brand-README Abschnitt „Ordnung und Ruhe (Fassung 8)“, Soll-Bilder unter `docs/design/soll/fassung-8/` (dunkel und hell, 1920 × 1030).

Ein Paket, Abnahme am Stück. Je Punkt Prüfpunkte in `docs/abnahme.json`, Stand in `docs/design/umsetzung.md`.

## 0. Zuerst: Fehler aus dem Live-Betrieb

1. **Nur eine Instanz.** Ein zweiter Start von `woa.exe` hat am 05.11. der laufenden Sendung das USB-Audio-Gerät und die Verbindung zu `woa-streamd` weggenommen. Soll: Ein zweiter Start holt das vorhandene Fenster nach vorn und beendet sich, bevor Audio, MIDI oder Stream geöffnet werden (Windows: benannter Mutex plus Fenster-Aktivierung; Linux: Lock-Datei plus Socket).
   - Test: Ein zweiter Start ändert am Audio-Graph und an der Stream-Verbindung nichts.
2. **Schließen-X der lösbaren Fenster** (Fragen und To-dos, Abnahme) reagiert auf einen einfachen Klick ohne Mausbewegung nicht. Das Einstellungsfenster schließt richtig. Soll: Alle Fensterköpfe verhalten sich gleich (`Window`).

## 1. Pegelgruppe (`RadioLevels`)

- Ein Kanalbau für alle (`woa-ch8`): oben 24 px Stumm-Taste oder leerer Platz, in der Mitte Fader und LED-Kette gleich hoch, unten das Namensschild auf einer Linie.
- Gruppen Quellen (Playlist, Carts, Decks, Mikro, Gäste) · Summe (Master, 64 px, ohne Fader) · Ausgänge (Notebook, Stream mit M), getrennt durch 1-px-Linien. Gemeinsame dB-Skala links.
- LED nur grün, gelb, rot. **Kein Blau.**
- Die Karte füllt ihre Höhe, kein Leerraum. Die „M“-Kästen werden echte Tasten (`woa-key--rec`).

## 2. Meldungszeile (`StatusLine`)

- Fehler und Hinweise im Sende-Deck nicht mehr als roter Fließtext zwischen den Tasten, sondern eine Zeile (28 px) am Fuß der Karte: Warnsymbol, fetter Kurztext, Detail, passende Aktion („Neu lesen“).
- Mehrere Hinweise: Zähler-Chip und Liste per Klick.
- Gilt für Telefon- und Mikro-Ausfall, Stream-Neuverbindung und Lücken.

## 3. Radio 1920 × 1030 ohne Scrollen (`ScreenRadio`)

- **Links:** Sende-Deck mit Meldungszeile, darunter die Gäste-Karte (`GuestCard`), danach die Playlist.
  - Die Karten „Gäste & Mix-Minus“ und „Browser-Gäste“ werden zu dieser einen Karte.
  - Die Telefon-Taste entfällt.
- **Rechts:** Decks A–C kompakt, Cart-Wall 4 × 3 je Satz (Sätze 1–5 über Segmente), Pegel neben Processing in gleicher Höhe, Wunschtitel als eine Zeile.
- Leere Cart-Pads ohne das „-“-Kästchen.

## 4. Einstellungen (`SettingsAudio`, `ScreenSettings`)

Die Bereiche bleiben wie im Register (`settings.rs`, CLAUDE.md „Einstellungen einsortieren“).

- **Werte direkt in der Zeile** statt „Im Radio zeigen“ oder „Öffnen“, wo es nur ein Wert ist (Crossfade, Stopp blendet aus, Ducking, Bitrate …). Das Register-Feld bekommt dafür sein Bedienelement.
- **Ein Schaltertyp** (`woa-tog`): keine Häkchen, keine „an“-Chips als Schalter, keine Punkt-Optionen; Mehrfachwahl als Segmente.
- Aktiver Bereich in `surface` mit Kante `accent-2`, **nicht rot**.
- Fehlende Geräte zeigt der Bereich „Audio und Geräte“ mit Chip am Bereich (gelb, Anzahl).
- Das Fenster öffnet maximiert oder mit 1400 × 930.
- **Offen, erst nach OK von André:** Das Fenster „Audio-Geräte“ wird im Bereich „Audio und Geräte“ eingebettet. Es bleibt der eine Ort für Soundkarten, nur ohne Extrafenster; der Test `devices_are_listed_only_here` zieht mit um. Dazu die Übernehmen-Leiste mit dem Hinweis auf Sendung. Bis zum OK bleibt das Fenster.

## 5. Fragen und Abnahme (Nachtrag zu Fassung 7)

- Ticket-Chips und Filter neutral, nicht rot. Die Filter werden Segmente.
- Die Stand-Zeile rechts in den Fensterkopf.
- Der Abnahme-Zähler bekommt einen richtigen Balken (grün = geht, gelb = geht nicht).

## Abnahme

- Screenshots 1920 × 1030 von Radio (auf Sendung) und Einstellungen › Audio nach `docs/design/ist/`.
- Prüfpunkte in `docs/abnahme.json`, mindestens:
  - zweiter Start stört die Sendung nicht,
  - X schließt Fragen und Abnahme,
  - Pegel einheitlich,
  - Meldungszeile statt Fließtext,
  - Radio ohne Scrollen,
  - Audio-Geräte in den Einstellungen.
- PR mit grüner CI.
