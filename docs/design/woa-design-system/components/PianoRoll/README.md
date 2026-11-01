# PianoRoll

Piano-Roll im Studio-Dock: Noten zeichnen und bearbeiten wie in einer Profi-DAW – übersichtlich, in Spurfarbe, mit sichtbarer Tonart.

**Leiste (eine Zeile, Gruppen durch Linien):**

1. Clip-Kennung: Spurfarbe, Name, Spur, Takte, Notenzahl, Auswahl.
2. Werkzeuge: Zeichnen (D) · Auswählen (V) · Löschen (E) als Segmentschalter mit Symbolen.
3. Raster:
   - Raster (1/16 …) und Triolen-Schalter „3“, für Shuffle.
   - Länge neuer Noten.
   - Quantisieren (Q).
4. Velocity der Auswahl als Zahlenfeld.
5. Tonart:
   - Auswahl (A Dur …).
   - Einrasten auf Tonart (Magnet).
   - „Nur benutzte Töne“ (Zusammenklappen leerer Tonhöhen).
6. Rechts: Probeton beim Zeichnen, Zoom −/+.

**Fläche:**

- **Lineal:** Taktnummern, Clip-Bereich als Balken in Spurfarbe; außerhalb des Clips schraffiert und abgedunkelt.
- **Klaviatur** wie ein echtes Klavier: weiße Tasten hell, schwarze als kurze dunkle Balken. C-Tasten beschriftet (C3, C4) und mit kräftigerer Linie; gespielte und gewählte Tonhöhen in Spurfarbe markiert.
- **Zeilen:** schwarze Tasten leicht abgesetzt, Töne der Tonart 8 % Spurfarbe. Taktlinien kräftig, Viertel mittel, Sechzehntel zart.
- **Noten** in Spurfarbe, Helligkeit nach Velocity, Notenname in der Note ab 40 px Breite, rechter Rand als Griff für die Länge. Auswahl mit `text`-Rahmen.
- **Velocity-Spur** unten (64 px): Stiel mit Kopf je Note in Spurfarbe, gewählte in `text`; Mittellinie gestrichelt.
- **Ohne Anleitungstext:** Die Bedienung steht in den Tooltips der Werkzeuge.
- Für Kit-Spuren öffnet sich das Drum-Raster.
