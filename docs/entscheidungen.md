# Entscheidungen

## egui als Oberfläche (M0, Branch `m0/gui`, Stand 29.10.2026)

Prototyp: `crates/gui` + `apps/woa` mit eframe/egui 0.35.0 (wgpu-Backend, Standard-Features),
Dateidialoge über `rfd` 0.17 (xdg-portal, kein GTK nötig). Spurliste, Umbenennen, Gain, Mute,
Undo/Redo (Strg+Z / Strg+Y bzw. Strg+Umschalt+Z), Neu/Öffnen/Speichern, Autosave alle 10 s,
Feedback-Dialog mit Screenshot (`ViewportCommand::Screenshot`).

**Version:** egui 0.36/0.37 verlangen rustc 1.95; lokal ist 1.92 installiert, daher 0.35.0. Die API
ist ab 0.35 schon die neue (`App::ui`/`App::logic`, `egui::Panel`), das Hochziehen ist ein
Einzeiler in zwei `Cargo.toml`, sobald die Toolchain neuer ist.

### Gemessen (Linux, Notebook, 12 Kerne, Rocky 9, Wayland)

| Messgröße | Wert |
|---|---|
| Release-Build `woa` kalt (ganzer Abhängigkeitsbaum) | 8 min 02 s |
| Debug-Build kalt (eframe + rfd neu) | ca. 14 min (parallel zu anderen Builds) |
| Binärgröße release | 27,5 MiB, gestrippt 22,5 MiB |
| Speicher (RSS) nach Start, leeres Projekt | ca. 76 MiB |
| CPU im Leerlauf (Repaint alle 500 ms für Autosave-Takt) | ca. 3 % eines Kerns |
| Start bis erstes wgpu-Log | ca. 10 s (debug und release gleich) |
| Absturztest: Start, 15–20 s laufen lassen, per Timeout beenden | kein Absturz, Log sauber |

Die 10 s Startzeit sind auffällig. Vermutung (nicht belegt): Adapter-Suche von wgpu über
Vulkan oder das Laden der Systemschriften (`system_fonts`). Gegenprobe: mit Feature `glow`
statt `wgpu` bzw. ohne `system_fonts` bauen und erneut messen.

### Nicht messbar ohne Bildschirm

- Bildrate und Bedienung beim Ziehen des Gain-Reglers, Schärfe der Schrift, HiDPI.
- Ob der Screenshot im Feedback-Dialog wirklich das Fenster zeigt.
- Windows-Lauf (Start, Dateidialog, Speicherpfad `%APPDATA%\woa`).

### Vorläufige Bewertung

Bauzeit und Größe sind für eine DAW vertretbar, die Architektur (Logik in `AppState` ohne
Fenster, UI dünn darüber) lässt sich gut testen. Bestätigt ist egui erst nach Andrés Blick am
Bildschirm unter Linux und Windows und nach Klärung der Startzeit.

## 29.10.2026: vier Entscheidungen nach dem Nachtbau (André)

- **Lizenz CDLA-Permissive-2.0 für `webpki-roots`:** erlaubt. Es ist nur Mozillas Zertifikatsliste,
  die Ausnahme in `deny.toml` gilt ausschließlich für dieses Crate.
- **Pan-Gesetz:** Constant-Power, -3 dB in der Mitte, so wie in `woa-engine` gebaut.
- **Standard-SoundFont:** GeneralUser GS 2.0.3, Lizenz und Bezug in `docs/sounds/`.
- **Feedback-Server:** Die Rolle `feedback` in bretteins.infra wird mehrinstanzfähig, Ticket an Dave
  nixblick/bretteins.infra#145. Danach rollt André die zweite Instanz nach `docs/intern/andre-handgriffe.md` (nur lokal) aus.
