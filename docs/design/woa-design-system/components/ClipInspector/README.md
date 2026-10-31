# ClipInspector

Leiste über der Zeitleiste, die immer den gewählten Clip zeigt – nach dem Vorbild von Ableton Clip View, Bitwig Inspector und Logic Region Inspector, aber in einer Zeile.

Aufbau von links nach rechts, Gruppen durch 1-px-Linien getrennt:

1. **Kennung** – Spurfarbe als 4-px-Kante und 12-%-Fläche, Typ-Symbol (Wellenform = Audio, Klaviatur = MIDI, Stapel = mehrere), Name (Doppelklick = umbenennen).
2. **Fade** (Audio) – Fade-In, Fade-Out in ms. Bei MIDI stattdessen **Noten**: Raster, Quantisieren (Q), Velocity.
3. **Gain** – Clip-Gain in dB, Normalisieren (Spitze auf −1 dBFS).
4. **Zeit** – Start und Länge (Takt.Schlag.Teil), Tempo folgen (Schalter), Tonhöhe in st.
5. **Bearbeiten** – rechtsbündig: Teilen (S), Duplizieren (Strg+D), Umkehren (R) bzw. Piano-Roll (Enter), Loop (L), Stumm (0); ganz rechts mit Abstand Löschen (Entf) als Gefahr-Knopf.

Zustände:

- **Ein Clip:** alle Felder bedienbar.
- **Mehrere Clips („n Clips“):** gleiche Werte stehen da, unterschiedliche als „—“. Eintippen setzt alle auf den Wert; Ziehen an Gain verschiebt alle relativ. Nur Befehle, die für alle gelten, bleiben; Zusammenfügen (Strg+J) kommt dazu.
- **Nichts gewählt:** Leiste bleibt in voller Höhe (kein Springen), zeigt „Kein Clip gewählt“ und einen Hinweis.
- **Schmal (< 1040 px):** Gruppentitel und Feld-Präfixe fallen weg; die Tooltips tragen die Namen.

Regeln: Höhe 44 px. Knöpfe nur als Symbol (`IconButton`) mit Tooltip; Felder als `NumberField`. Ein Clip gewählt = seine Spur gewählt (Spurkopf, Kanalzug und Rack folgen).
