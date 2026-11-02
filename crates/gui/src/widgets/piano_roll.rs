//! Piano-Roll (Paket 13, Design-Runde 3 nach `PianoRoll`/`ScreenPianoRoll`): Editor für einen
//! MIDI-Clip.
//!
//! Leiste in einer Zeile: Clip-Kennung, Werkzeuge Zeichnen/Auswählen/Löschen (D/V/E), Raster
//! mit Triolen „3“, Länge neuer Noten, Quantisieren (Q), Velocity, Tonart mit Magnet, „Nur
//! benutzte Töne“, Probeton beim Zeichnen, Zoom. Darunter Lineal, echte Klaviatur, Noten in
//! Spurfarbe und die Velocity-Spur. Kit-Spuren öffnen das Drum-Raster (`DrumGrid`). Jede
//! Änderung geht als ein `SetNotes` durch [`AppState`], so haben Oberfläche und Agent denselben
//! Weg und Strg+Z nimmt sie einzeln zurück.
//!
//! Vorhören und Probeton gehen als [`crate::probeton::Probe`] an die Spur des Clips; die App
//! reicht sie an die `rtrb`-Queue der Live-Spur weiter.

use eframe::egui;
use egui::{Color32, Pos2, Rect, Sense, Stroke, pos2, vec2};
use woa_model::harmony::Key;
use woa_model::{ClipId, MidiData, MidiNote, TrackId, grid_beats};

use crate::state::AppState;
use crate::theme;

const KEYS_W: f32 = 52.0;
const VEL_H: f32 = 64.0;
/// Lineal über den Noten: Taktnummern, Clip-Bereich und Akkordnamen (Design `PianoRoll`).
const RULER_H: f32 = 22.0;
const RESIZE_PX: f32 = 6.0;
const DIVISIONS: [u32; 4] = [4, 8, 16, 32];
/// Längen neuer Noten (Notenwert); `None` in [`PianoRoll::note_len`] heißt „wie das Raster“.
const LENGTHS: [u32; 6] = [1, 2, 4, 8, 16, 32];

/// Werkzeug der Notenfläche.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    /// Klick ins Leere zeichnet eine Note (Standard).
    Draw,
    /// Klick wählt nur, ins Leere ziehen zieht einen Rahmen.
    Select,
    /// Klick auf eine Note löscht sie.
    Erase,
}

impl Tool {
    const ALL: [(Self, &'static str, &'static str, &'static str); 3] = [
        (
            Self::Draw,
            egui_phosphor::regular::PENCIL_SIMPLE,
            "Zeichnen (D)",
            "Klick ins Leere: Note im Raster · Ziehen: verschieben · rechter Rand: Länge · \
             Alt: ohne Raster",
        ),
        (
            Self::Select,
            egui_phosphor::regular::CURSOR,
            "Auswählen (V)",
            "Klick: wählen · Strg/Umschalt+Klick: dazu · Rahmen ziehen · Strg+A: alle",
        ),
        (
            Self::Erase,
            egui_phosphor::regular::ERASER,
            "Löschen (E)",
            "Klick auf eine Note löscht sie · auch Doppelklick oder Entf",
        ),
    ];
}

/// What the pointer is doing in the note area.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Drag {
    /// Selected notes move by beats (snapped) and semitones.
    Move {
        d_beats: f64,
        d_pitch: i32,
        raw: egui::Vec2,
    },
    /// Selected notes get longer or shorter.
    Resize { d_beats: f64, raw: f32 },
    /// Rubber band from this point.
    Select { from: Pos2 },
    /// Velocity of the selection (or one note) follows the pointer.
    Velocity { note: usize, value: u8 },
}

/// Colours of the editor, from the current egui theme (hell oder dunkel).
pub(super) struct Palette {
    pub(super) bg: Color32,
    row_black: Color32,
    line: Color32,
    bar: Color32,
    note: Color32,
    note_sel: Color32,
    pub(super) text: Color32,
    key_white: Color32,
    key_black: Color32,
}

impl Palette {
    pub(super) fn of(ui: &egui::Ui, note: Color32) -> Self {
        if ui.visuals().dark_mode {
            Self {
                bg: theme::bg(),
                row_black: theme::mix(theme::bg(), Color32::BLACK, 0.06),
                line: theme::mix(theme::bg(), theme::grid(), 0.6),
                bar: theme::mix(theme::grid(), theme::text_dim(), 0.35),
                note,
                note_sel: theme::text(),
                text: theme::text_dim(),
                key_white: Color32::from_rgb(226, 230, 236),
                key_black: Color32::from_rgb(22, 24, 30),
            }
        } else {
            Self {
                bg: theme::panel(),
                row_black: theme::mix(theme::panel(), Color32::BLACK, 0.06),
                line: theme::grid(),
                bar: theme::border_strong(),
                note,
                note_sel: theme::text(),
                text: theme::text_dim(),
                key_white: Color32::WHITE,
                key_black: theme::text(),
            }
        }
    }
}

fn is_black(pitch: u8) -> bool {
    matches!(pitch % 12, 1 | 3 | 6 | 8 | 10)
}

/// Name like "C4" (60 = C4).
pub fn pitch_name(pitch: u8) -> String {
    const N: [&str; 12] = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "H",
    ];
    format!(
        "{}{}",
        N[usize::from(pitch % 12)],
        i32::from(pitch / 12) - 1
    )
}

/// Zeilen der Notenfläche von oben nach unten: alle 128 Töne oder nur die benutzten.
pub fn rows(notes: &[MidiNote], fold: bool) -> Vec<u8> {
    let mut used: Vec<u8> = notes.iter().map(|n| n.pitch).collect();
    used.sort_unstable_by(|a, b| b.cmp(a));
    used.dedup();
    if fold && !used.is_empty() {
        used
    } else {
        (0..=127u8).rev().collect()
    }
}

/// Zeile der Tonhöhe in `rows` (absteigend); fehlt sie, die nächsttiefere.
pub fn row_index(rows: &[u8], pitch: u8) -> usize {
    rows.iter()
        .position(|&r| r <= pitch)
        .unwrap_or(rows.len().saturating_sub(1))
}

/// Magnet: nächster Ton der Tonart (bei Gleichstand der tiefere).
pub fn snap_to_key(key: Option<Key>, pitch: u8) -> u8 {
    let Some(k) = key else {
        return pitch;
    };
    for d in 0..=6i16 {
        for p in [i16::from(pitch) - d, i16::from(pitch) + d] {
            if (0..=127).contains(&p) && k.contains(p as u8) {
                return p as u8;
            }
        }
    }
    pitch
}

/// Schraffur (außerhalb des Clips).
fn hatch(painter: &egui::Painter, r: Rect, c: Color32) {
    if r.width() <= 0.0 || r.height() <= 0.0 {
        return;
    }
    let p = painter.with_clip_rect(r.intersect(painter.clip_rect()));
    let step = 8.0;
    let mut x = r.left() - r.height();
    while x < r.right() {
        p.line_segment(
            [pos2(x, r.bottom()), pos2(x + r.height(), r.top())],
            Stroke::new(1.0, c),
        );
        x += step;
    }
}

/// Segmentschalter-Knopf; `true` bei Klick.
pub(super) fn seg(ui: &mut egui::Ui, on: bool, text: &str, tip: &str) -> bool {
    ui.add(egui::Button::selectable(on, text))
        .on_hover_text(tip)
        .clicked()
}

/// Symbol-Knopf mit Tooltip (und Namen für Screenreader und Tests).
pub(super) fn icon_button(
    ui: &mut egui::Ui,
    on: bool,
    icon: &str,
    name: &str,
    tip: &str,
) -> egui::Response {
    let r = ui
        .add(egui::Button::selectable(on, icon).min_size(vec2(26.0, 24.0)))
        .on_hover_text(tip);
    r.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, on, name));
    r
}

/// Editor state of one open MIDI clip.
pub struct PianoRoll {
    /// The clip being edited.
    pub clip: ClipId,
    /// Selected notes (indices into the clip's notes).
    pub sel: Vec<usize>,
    /// Grid as note value (4 = quarter … 32).
    pub division: u32,
    /// Triplet grid.
    pub triplet: bool,
    /// Horizontal zoom factor (1 = clip fits the window).
    pub zoom: f32,
    /// Werkzeug der Notenfläche.
    pub tool: Tool,
    /// Länge neuer Noten als Notenwert; `None`: ein Rasterschritt.
    pub note_len: Option<u32>,
    /// Gewählte Tonart; `None`: aus den Noten erkannt.
    pub key_pick: Option<Key>,
    /// Neue und verschobene Noten rasten auf die Tonart ein.
    pub magnet: bool,
    /// Nur benutzte Töne als Zeilen.
    pub fold: bool,
    /// Probeton beim Zeichnen.
    pub probe: bool,
    /// Abspielkopf in Beats (für das Lauflicht im Drum-Raster), von der App je Bild gesetzt.
    pub playhead: Option<f64>,
    /// First visible beat.
    x_off: f64,
    /// Row height in pixels (vertical zoom).
    row_h: f32,
    drag: Option<Drag>,
    /// Velocity for new notes (last one set).
    velocity: u8,
    /// Key held on the keyboard (preview).
    preview: Option<u8>,
    /// Scroll to the notes on the first frame.
    centre: bool,
    /// Kit-Spur: Raster (Standard) oder Noten.
    pub(super) drum_grid: Option<bool>,
    /// Dichte für Presets: -1 weniger, 0, +1 mehr.
    pub(super) density: i8,
    /// Zuletzt gewähltes Preset (Index in `DRUM_PRESETS`).
    pub(super) preset: Option<usize>,
    /// Zähler für den Würfel.
    pub(super) dice_seed: u64,
    /// Spurfarbe, Spur, erkannte Tonart und Clip-Anfang (Beat der Note 0), je Bild.
    pub(super) color: Color32,
    pub(super) track: Option<TrackId>,
    detected: Option<Key>,
    pub(super) origin: f64,
    chords: Vec<Option<woa_model::harmony::Chord>>,
    /// Zeilen der Notenfläche (Tonhöhen von oben nach unten).
    rows: Vec<u8>,
    /// Fenster füllt die App (Fensterkopf Fassung 7).
    pub maximized: bool,
}

impl PianoRoll {
    /// Opens the editor for `clip`.
    pub fn new(clip: ClipId) -> Self {
        Self {
            clip,
            sel: Vec::new(),
            division: 16,
            triplet: false,
            zoom: 1.0,
            tool: Tool::Draw,
            note_len: None,
            key_pick: None,
            magnet: false,
            fold: false,
            probe: true,
            playhead: None,
            x_off: 0.0,
            row_h: 12.0,
            drag: None,
            velocity: 100,
            preview: None,
            centre: true,
            drum_grid: None,
            density: 0,
            preset: None,
            dice_seed: 0,
            color: theme::track_color(0),
            track: None,
            detected: None,
            origin: 0.0,
            chords: Vec::new(),
            maximized: false,
            rows: rows(&[], false),
        }
    }

    /// Spurfarbe des Clips (Design: Noten in Spurfarbe, auch Velocity und Drum-Raster).
    pub fn note_color(state: &AppState, clip: ClipId) -> Color32 {
        let index = state
            .project()
            .tracks
            .iter()
            .position(|t| t.clips.iter().any(|c| c.id == clip));
        theme::track_color(index.unwrap_or(0))
    }

    fn grid(&self) -> f64 {
        grid_beats(self.division, self.triplet)
    }

    /// Länge einer neu gezeichneten Note in Beats.
    fn new_len(&self) -> f64 {
        self.note_len
            .map_or_else(|| self.grid(), |d| grid_beats(d, self.triplet))
    }

    /// Tonart für Zeilen-Tönung und Magnet: gewählt, sonst erkannt.
    pub fn key(&self) -> Option<Key> {
        self.key_pick.or(self.detected)
    }

    pub(super) fn notes(state: &AppState, clip: ClipId) -> Option<(MidiData, f64, String)> {
        let (_, c) = state.project().clip(clip)?;
        Some((c.midi.clone()?, c.length_beats, c.name.clone()))
    }

    /// Key shortcuts while the editor is open; consumed before the timeline sees them.
    /// Returns `false` when the editor should close (Escape).
    pub fn keys(&mut self, ctx: &egui::Context, state: &mut AppState) -> bool {
        if ctx.egui_wants_keyboard_input() {
            return true;
        }
        let n = egui::Modifiers::NONE;
        let (del, back, all, quant, esc, d, v, e) = ctx.input_mut(|i| {
            (
                i.consume_key(n, egui::Key::Delete),
                i.consume_key(n, egui::Key::Backspace),
                i.consume_key(egui::Modifiers::COMMAND, egui::Key::A),
                i.consume_key(n, egui::Key::Q),
                i.consume_key(n, egui::Key::Escape),
                i.consume_key(n, egui::Key::D),
                i.consume_key(n, egui::Key::V),
                i.consume_key(n, egui::Key::E),
            )
        });
        for (hit, t) in [(d, Tool::Draw), (v, Tool::Select), (e, Tool::Erase)] {
            if hit {
                self.tool = t;
            }
        }
        let Some((midi, _, _)) = Self::notes(state, self.clip) else {
            return false;
        };
        if del || back {
            self.delete(state, &midi);
        }
        if all {
            self.sel = (0..midi.notes.len()).collect();
        }
        if quant {
            self.quantize(state, &midi);
        }
        if esc {
            self.release(state);
        }
        !esc
    }

    /// Lässt gehaltene Probetöne los (Editor geschlossen).
    pub fn release(&mut self, state: &mut AppState) {
        self.preview = None;
        state.probeton.release_all();
    }

    fn delete(&mut self, state: &mut AppState, midi: &MidiData) {
        if !self.sel.is_empty() {
            state.set_notes(self.clip, midi.delete_notes(&self.sel));
            self.sel.clear();
        }
    }

    fn quantize(&mut self, state: &mut AppState, midi: &MidiData) {
        let (q, sel) = midi.quantize(&self.sel, self.grid(), false);
        state.set_notes(self.clip, q);
        self.sel = sel;
    }

    /// Draws the editor window. Returns `false` when it was closed or its clip is gone.
    pub fn show(&mut self, ctx: &egui::Context, state: &mut AppState) -> bool {
        let Some((midi, length, name)) = Self::notes(state, self.clip) else {
            return false;
        };
        self.sel.retain(|&i| i < midi.notes.len());
        let mut open = true;
        let screen = ctx.content_rect();
        let size = vec2(
            (screen.width() - 40.0).clamp(480.0, 1200.0),
            (screen.height() - 90.0).clamp(320.0, 700.0),
        );
        // Fassung 7: eigener Fensterkopf 44 px (Doppelklick maximiert, Esc schließt).
        let mut win = egui::Window::new(format!("Piano-Roll – {name}"))
            .id(egui::Id::new(("pianoroll", self.clip.0, self.maximized)))
            .title_bar(false)
            .collapsible(false)
            .constrain(true);
        win = if self.maximized {
            win.fixed_rect(screen.shrink(4.0)).resizable(false)
        } else {
            win.default_size(size)
                .default_pos(pos2(screen.left() + 20.0, screen.top() + 40.0))
                .min_size([480.0, 320.0])
                .resizable(true)
        };
        let mut max = self.maximized;
        win.show(ctx, |ui| {
            crate::fragen::kopf(
                ui,
                theme::ic::PIANO_KEYS,
                &format!("Piano-Roll – {name}"),
                "Noten zeichnen, verschieben, Tonart und Raster",
                &mut open,
                &mut max,
            );
            self.body(ui, state, &midi, length);
        });
        self.maximized = max;
        if !open {
            self.release(state);
        }
        open
    }

    /// Draws the Piano-Roll inline inside an existing panel (e.g. the docked bottom Studio panel).
    /// Returns `false` if the edited clip no longer exists.
    pub fn show_inline(&mut self, ui: &mut egui::Ui, state: &mut AppState) -> bool {
        let Some((midi, length, _)) = Self::notes(state, self.clip) else {
            self.release(state);
            return false;
        };
        self.sel.retain(|&i| i < midi.notes.len());
        self.body(ui, state, &midi, length);
        true
    }

    /// Ist der Clip auf einer Kit-Spur?
    fn is_kit(state: &AppState, clip: ClipId) -> bool {
        state
            .project()
            .clip(clip)
            .is_some_and(|(t, _)| t.sound.is_some_and(|s| s.drums))
    }

    /// Leiste plus Notenfläche, auf einer Kit-Spur das Drum-Raster.
    fn body(&mut self, ui: &mut egui::Ui, state: &mut AppState, midi: &MidiData, length: f64) {
        let kit = Self::is_kit(state, self.clip);
        let grid = kit && self.drum_grid.unwrap_or(true);
        self.color = Self::note_color(state, self.clip);
        self.track = state.project().clip(self.clip).map(|(t, _)| t.id);
        self.origin = state
            .project()
            .clip(self.clip)
            .map_or(0.0, |(_, c)| c.start_beats - c.offset_beats);
        let h = crate::timeline::harmony(state.project(), self.origin, 128);
        self.detected = h.key;
        self.chords = h.bars;
        if grid {
            self.drum_head(ui, state, midi, length);
            ui.add_space(4.0);
            self.drum_editor(ui, state, midi, length);
        } else {
            self.toolbar(ui, state, midi, length, kit);
            ui.add_space(2.0);
            self.editor(ui, state, midi, length);
        }
    }

    /// Clip-Kennung: Spurfarbe, Name, Spur, Takte, Notenzahl, Auswahl.
    fn clip_label(&self, ui: &mut egui::Ui, state: &AppState, midi: &MidiData, length: f64) {
        let (track, start) = state
            .project()
            .clip(self.clip)
            .map_or((String::new(), 0.0), |(t, c)| {
                (t.name.clone(), c.start_beats)
            });
        let name = state
            .project()
            .clip(self.clip)
            .map_or_else(String::new, |(_, c)| c.name.clone());
        let first = (start / 4.0).floor() as i64 + 1;
        let last = ((start + length) / 4.0).ceil() as i64;
        let sel = match self.sel.as_slice() {
            [] => String::new(),
            [i] => midi.notes.get(*i).map_or_else(String::new, |n| {
                format!(
                    " · {} · Takt {:.2} · Länge {:.3} · Vel {}",
                    pitch_name(n.pitch),
                    n.beat / 4.0 + 1.0,
                    n.length_beats,
                    n.velocity
                )
            }),
            s => format!(" · {} gewählt", s.len()),
        };
        egui::Frame::new()
            .fill(theme::mix(theme::surface(), self.color, 0.12))
            .corner_radius(theme::RADIUS_MD)
            .inner_margin(egui::Margin {
                left: 0,
                right: 8,
                top: 3,
                bottom: 3,
            })
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let (bar, _) = ui.allocate_exact_size(vec2(3.0, 18.0), Sense::hover());
                    ui.painter().rect_filled(bar, 1.0, self.color);
                    ui.label(egui::RichText::new(name).strong().color(theme::text()));
                    ui.label(
                        egui::RichText::new(format!(
                            "{track} · Takt {first}–{last} · {} Noten{sel}",
                            midi.notes.len()
                        ))
                        .monospace()
                        .size(11.0)
                        .color(theme::text_dim()),
                    );
                });
            });
    }

    /// Leiste in einer Zeile, Gruppen durch Linien (Design `PianoRoll`).
    fn toolbar(
        &mut self,
        ui: &mut egui::Ui,
        state: &mut AppState,
        midi: &MidiData,
        length: f64,
        kit: bool,
    ) {
        use egui_phosphor::regular as ic;
        use woa_model::chords::KEYS;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            if kit {
                self.kit_switch(ui, false);
                ui.separator();
            }
            self.clip_label(ui, state, midi, length);
            ui.separator();
            // Werkzeuge.
            ui.spacing_mut().item_spacing.x = 0.0;
            for (t, icon, name, tip) in Tool::ALL {
                if icon_button(ui, self.tool == t, icon, name, &format!("{name}: {tip}")).clicked()
                {
                    self.tool = t;
                }
            }
            ui.spacing_mut().item_spacing.x = 6.0;
            ui.separator();
            // Raster, Triolen, Länge neuer Noten, Quantisieren.
            egui::ComboBox::from_id_salt(("pr_grid", self.clip.0))
                .selected_text(format!("{} 1/{}", ic::GRID_FOUR, self.division))
                .width(74.0)
                .show_ui(ui, |ui| {
                    for d in DIVISIONS {
                        ui.selectable_value(&mut self.division, d, format!("1/{d}"));
                    }
                })
                .response
                .on_hover_text("Raster");
            if icon_button(
                ui,
                self.triplet,
                "3",
                "Triolen",
                "Triolen-Raster (für Shuffle)",
            )
            .clicked()
            {
                self.triplet = !self.triplet;
            }
            egui::ComboBox::from_id_salt(("pr_len", self.clip.0))
                .selected_text(match self.note_len {
                    None => format!("{} = Raster", ic::ARROWS_HORIZONTAL),
                    Some(d) => format!("{} 1/{d}", ic::ARROWS_HORIZONTAL),
                })
                .width(88.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.note_len, None, "= Raster");
                    for d in LENGTHS {
                        ui.selectable_value(&mut self.note_len, Some(d), format!("1/{d}"));
                    }
                })
                .response
                .on_hover_text("Länge neuer Noten");
            let what = if self.sel.is_empty() {
                "Alle quantisieren (Q)"
            } else {
                "Auswahl quantisieren (Q)"
            };
            if icon_button(ui, false, ic::MAGNET_STRAIGHT, what, what).clicked() {
                self.quantize(state, midi);
            }
            ui.separator();
            // Velocity der Auswahl (sonst neuer Noten).
            let mut v = self
                .sel
                .first()
                .and_then(|&i| midi.notes.get(i))
                .map_or(self.velocity, |n| n.velocity);
            ui.label(egui::RichText::new(ic::GAUGE).color(theme::text_dim()))
                .on_hover_text("Velocity");
            let r = ui
                .add(egui::DragValue::new(&mut v).range(1..=127))
                .on_hover_text("Velocity der Auswahl, sonst neuer Noten");
            if r.changed() {
                self.velocity = v;
            }
            if (r.drag_stopped() || (r.changed() && !r.dragged())) && !self.sel.is_empty() {
                state.set_notes(self.clip, midi.set_velocity(&self.sel, v));
            }
            ui.separator();
            // Tonart, Magnet, nur benutzte Töne.
            let shown = self.key();
            egui::ComboBox::from_id_salt(("pr_key", self.clip.0))
                .selected_text(shown.map_or_else(|| "Tonart".to_owned(), |k| k.name()))
                .width(96.0)
                .show_ui(ui, |ui| {
                    let auto = self.detected.map_or_else(
                        || "Erkannt: –".to_owned(),
                        |k| format!("Erkannt: {}", k.name()),
                    );
                    ui.selectable_value(&mut self.key_pick, None, auto);
                    for minor in [false, true] {
                        for tonic in 0..12u8 {
                            let k = Key { tonic, minor };
                            ui.selectable_value(&mut self.key_pick, Some(k), k.name());
                        }
                    }
                })
                .response
                .on_hover_text(format!(
                    "Tonart: Zeilen der Tonart sind getönt (12 Grundtöne: {})",
                    KEYS.join(" ")
                ));
            if icon_button(
                ui,
                self.magnet,
                ic::MAGNET,
                "Auf Tonart einrasten",
                "Auf Tonart einrasten: neue und verschobene Noten treffen nur Töne der Tonart",
            )
            .clicked()
            {
                self.magnet = !self.magnet;
            }
            if icon_button(
                ui,
                self.fold,
                ic::ARROWS_IN_LINE_VERTICAL,
                "Nur benutzte Töne",
                "Nur benutzte Töne: leere Tonhöhen ausblenden",
            )
            .clicked()
            {
                self.fold = !self.fold;
                self.centre = true;
            }
            // Rechts: Probeton, Zoom.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                if icon_button(
                    ui,
                    false,
                    ic::MAGNIFYING_GLASS_PLUS,
                    "Vergrößern",
                    "Vergrößern (Strg+Mausrad; Zeilenhöhe: Alt+Mausrad)",
                )
                .clicked()
                {
                    self.zoom = (self.zoom * 1.25).clamp(1.0, 16.0);
                }
                if icon_button(
                    ui,
                    false,
                    ic::MAGNIFYING_GLASS_MINUS,
                    "Verkleinern",
                    "Verkleinern (Strg+Mausrad; Zeilenhöhe: Alt+Mausrad)",
                )
                .clicked()
                {
                    self.zoom = (self.zoom / 1.25).clamp(1.0, 16.0);
                }
                ui.add_space(6.0);
                if icon_button(
                    ui,
                    self.probe,
                    ic::SPEAKER_HIGH,
                    "Probeton beim Zeichnen",
                    "Probeton beim Zeichnen: neue Noten erklingen kurz (Klaviatur klingt immer)",
                )
                .clicked()
                {
                    self.probe = !self.probe;
                }
            });
        });
    }

    /// x position of `beat` in a lane starting at `left`.
    fn x(&self, left: f32, px: f32, beat: f64) -> f32 {
        left + ((beat - self.x_off) as f32) * px
    }

    fn editor(&mut self, ui: &mut egui::Ui, state: &mut AppState, midi: &MidiData, length: f64) {
        let pal = Palette::of(ui, self.color);
        let avail = ui.available_size();
        let view_w = (avail.x - KEYS_W).max(100.0);
        let shown = length.max(midi.end_beats()).max(4.0) + 4.0;
        let px = view_w / shown as f32 * self.zoom;
        let visible = f64::from(view_w / px);
        self.x_off = self.x_off.clamp(0.0, (shown - visible).max(0.0));
        let grid = self.grid();
        let notes_h = (avail.y - VEL_H - 22.0 - RULER_H).max(120.0);
        let row_h = self.row_h;
        self.rows = rows(&midi.notes, self.fold);
        let full_h = self.rows.len() as f32 * row_h;
        let key = self.key();

        self.chord_ruler(ui, view_w, px, shown, length, &pal);

        // Notenfläche mit Klaviatur, senkrecht scrollbar.
        let mut scroll = egui::ScrollArea::vertical()
            .id_salt(("pr_scroll", self.clip.0))
            .max_height(notes_h)
            .auto_shrink([false, false]);
        if self.centre {
            // Auf die Noten (sonst C4) zentrieren.
            let mid = if midi.notes.is_empty() {
                60
            } else {
                let (lo, hi) = midi.notes.iter().fold((127u8, 0u8), |(lo, hi), n| {
                    (lo.min(n.pitch), hi.max(n.pitch))
                });
                ((u16::from(lo) + u16::from(hi)) / 2) as u8
            };
            let y = row_index(&self.rows, mid) as f32 * row_h;
            scroll = scroll.vertical_scroll_offset((y - notes_h / 2.0).max(0.0));
            self.centre = false;
        }
        let rows_now = self.rows.clone();
        scroll.show(ui, |ui| {
            let (rect, resp) =
                ui.allocate_exact_size(vec2(KEYS_W + view_w, full_h), Sense::click_and_drag());
            let painter = ui.painter_at(rect);
            let keys = Rect::from_min_size(rect.min, vec2(KEYS_W, full_h));
            let area = Rect::from_min_max(pos2(keys.right(), rect.top()), rect.max);
            let top = rect.top();
            let rws = rows_now.clone();
            let y_of = move |pitch: u8| top + row_index(&rws, pitch) as f32 * row_h;
            // Zeilen.
            for (i, &p) in rows_now.iter().enumerate() {
                let y = top + i as f32 * row_h;
                let r = Rect::from_x_y_ranges(area.x_range(), y..=y + row_h);
                let row = if is_black(p) { pal.row_black } else { pal.bg };
                // Töne der Tonart: 8 % Spurfarbe (Design `PianoRoll`).
                let row = match key {
                    Some(k) if k.contains(p) => theme::mix(row, pal.note, 0.08),
                    _ => row,
                };
                painter.rect_filled(r, 0.0, row);
                if p % 12 == 0 || self.fold {
                    painter.hline(
                        area.x_range(),
                        y + row_h,
                        Stroke::new(if p % 12 == 0 { 1.0 } else { 0.5 }, pal.bar),
                    );
                }
            }
            // Raster: Takte kräftig, Schläge mittel, Rasterschritte zart.
            self.grid_lines(&painter, area, px, grid, shown, &pal);
            // Außerhalb des Clips: abgedunkelt und schraffiert.
            let xe = self.x(area.left(), px, length);
            if xe < area.right() {
                let out = Rect::from_x_y_ranges(xe.max(area.left())..=area.right(), area.y_range());
                painter.rect_filled(out, 0.0, Color32::from_black_alpha(70));
                hatch(&painter, out, theme::mix(pal.bg, pal.bar, 0.45));
                painter.vline(xe, area.y_range(), Stroke::new(2.0, pal.bar));
            }
            // Klaviatur: weiße Tasten hell, schwarze als kurze dunkle Balken.
            let lit: Vec<u8> = self
                .sel
                .iter()
                .filter_map(|&i| midi.notes.get(i).map(|n| n.pitch))
                .chain(self.preview)
                .collect();
            painter.rect_filled(keys, 0.0, pal.key_white);
            for (i, &p) in rows_now.iter().enumerate() {
                let y = top + i as f32 * row_h;
                let r = Rect::from_x_y_ranges(keys.x_range(), y..=y + row_h);
                let on = lit.contains(&p);
                if is_black(p) {
                    let b = Rect::from_x_y_ranges(
                        keys.left()..=keys.left() + KEYS_W * 0.62,
                        y + 0.5..=y + row_h - 0.5,
                    );
                    painter.rect_filled(
                        b,
                        2.0,
                        if on {
                            theme::mix(pal.key_black, pal.note, 0.8)
                        } else {
                            pal.key_black
                        },
                    );
                } else if on {
                    painter.rect_filled(r.shrink2(vec2(0.0, 0.5)), 0.0, pal.note);
                }
                // Trennlinie zwischen weißen Tasten, an C kräftiger.
                let line = if p % 12 == 0 {
                    Stroke::new(1.2, Color32::from_gray(110))
                } else if !is_black(p) && !is_black(p.saturating_add(1)) || p % 12 == 4 {
                    Stroke::new(0.6, Color32::from_gray(170))
                } else {
                    Stroke::NONE
                };
                if line != Stroke::NONE {
                    painter.hline(keys.x_range(), y + row_h, line);
                }
                if p % 12 == 0 && row_h >= 8.0 {
                    painter.text(
                        pos2(keys.right() - 4.0, y + row_h / 2.0),
                        egui::Align2::RIGHT_CENTER,
                        pitch_name(p),
                        egui::FontId::new(
                            (row_h - 2.0).min(10.0),
                            egui::FontFamily::Name("ui-bold".into()),
                        ),
                        Color32::from_rgb(40, 44, 54),
                    );
                }
            }
            painter.vline(keys.right(), keys.y_range(), Stroke::new(1.0, pal.bar));
            // Noten (mit vorläufiger Verschiebung beim Ziehen).
            let (dm, dp, dl) = match self.drag {
                Some(Drag::Move {
                    d_beats, d_pitch, ..
                }) => (d_beats, d_pitch, 0.0),
                Some(Drag::Resize { d_beats, .. }) => (0.0, 0, d_beats),
                _ => (0.0, 0, 0.0),
            };
            let x_off = self.x_off;
            let xb = move |b: f64| area.left() + ((b - x_off) as f32) * px;
            let note_rect = move |n: &MidiNote, sel: bool| {
                let (b, p, l) = if sel {
                    (
                        (n.beat + dm).max(0.0),
                        (i32::from(n.pitch) + dp).clamp(0, 127) as u8,
                        (n.length_beats + dl).max(woa_model::MIN_NOTE_BEATS),
                    )
                } else {
                    (n.beat, n.pitch, n.length_beats)
                };
                let x0 = xb(b);
                let x1 = xb(b + l).max(x0 + 3.0);
                let y = y_of(p);
                Rect::from_x_y_ranges(x0..=x1, y + 1.0..=y + row_h - 1.0)
            };
            let band = match (self.drag, resp.interact_pointer_pos()) {
                (Some(Drag::Select { from }), Some(to)) => Some(Rect::from_two_pos(from, to)),
                _ => None,
            };
            for (i, n) in midi.notes.iter().enumerate() {
                let sel = self.sel.contains(&i);
                let r = note_rect(n, sel);
                if !r.intersects(area) {
                    continue;
                }
                let lit = sel || band.is_some_and(|b| b.intersects(r));
                // Spurfarbe, Velocity als Helligkeit; Auswahl mit `text`-Rahmen.
                let fill = theme::mix(
                    pal.bg,
                    pal.note,
                    0.45 + 0.55 * f32::from(n.velocity) / 127.0,
                );
                let r = r.intersect(area);
                painter.rect_filled(r, 2.0, fill);
                let edge = if lit {
                    Stroke::new(2.0, pal.note_sel)
                } else {
                    Stroke::new(1.0, pal.note)
                };
                painter.rect_stroke(r, 2.0, edge, egui::StrokeKind::Inside);
                // Griff für die Länge am rechten Rand.
                if r.width() >= 12.0 {
                    painter.vline(
                        r.right() - 3.0,
                        r.top() + 2.0..=r.bottom() - 2.0,
                        Stroke::new(1.0, theme::mix(fill, Color32::BLACK, 0.35)),
                    );
                }
                if r.width() >= 40.0 && row_h >= 10.0 {
                    painter.with_clip_rect(r).text(
                        pos2(r.left() + 4.0, r.center().y),
                        egui::Align2::LEFT_CENTER,
                        pitch_name(n.pitch),
                        egui::FontId::monospace((row_h - 3.0).min(10.0)),
                        theme::mix(pal.bg, Color32::BLACK, 0.4),
                    );
                }
            }
            // Abspielkopf in `accent`.
            if let Some(ph) = self.playhead {
                let x = self.x(area.left(), px, ph - self.origin);
                if area.x_range().contains(x) {
                    painter.vline(x, area.y_range(), Stroke::new(1.5, theme::accent()));
                }
            }
            if let Some(b) = band {
                painter.rect_stroke(
                    b,
                    0.0,
                    Stroke::new(1.0, pal.note_sel),
                    egui::StrokeKind::Inside,
                );
            }
            self.note_input(ui, &resp, keys, area, px, top, state, midi, &note_rect);
        });

        // Velocity-Spur unter der Notenfläche, gleiche Zeitachse.
        let (vrect, vresp) =
            ui.allocate_exact_size(vec2(KEYS_W + view_w, VEL_H), Sense::click_and_drag());
        self.velocity_lane(ui, &vresp, vrect, px, state, midi, &pal);

        // Waagerecht blättern.
        if visible < shown {
            let mut off = self.x_off;
            ui.horizontal(|ui| {
                ui.add_space(KEYS_W);
                ui.spacing_mut().slider_width = view_w - 20.0;
                ui.add(
                    egui::Slider::new(&mut off, 0.0..=shown - visible)
                        .show_value(false)
                        .trailing_fill(false),
                );
            });
            self.x_off = off;
        }
    }

    /// Lineal: Taktnummer, Clip-Bereich als Balken in Spurfarbe, außerhalb schraffiert, je Takt
    /// der Akkord aller MIDI-Spuren (dieselbe Erkennung wie die Akkordspur).
    fn chord_ruler(
        &self,
        ui: &mut egui::Ui,
        view_w: f32,
        px: f32,
        shown: f64,
        length: f64,
        pal: &Palette,
    ) {
        let (rect, _) = ui.allocate_exact_size(vec2(KEYS_W + view_w, RULER_H), Sense::hover());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, theme::well());
        let area = Rect::from_min_max(pos2(rect.left() + KEYS_W, rect.top()), rect.max);
        let bars = (shown / 4.0).ceil() as usize + 1;
        let xe = self.x(area.left(), px, length).min(area.right());
        let xs = self.x(area.left(), px, 0.0).max(area.left());
        if xe > xs {
            painter.rect_filled(
                Rect::from_x_y_ranges(xs..=xe, rect.bottom() - 3.0..=rect.bottom()),
                0.0,
                pal.note,
            );
        }
        if xe < area.right() {
            let out = Rect::from_x_y_ranges(xe.max(area.left())..=area.right(), rect.y_range());
            painter.rect_filled(out, 0.0, Color32::from_black_alpha(60));
            hatch(&painter, out, theme::mix(theme::well(), pal.bar, 0.5));
        }
        for bar in 0..bars {
            let x = self.x(area.left(), px, bar as f64 * 4.0);
            if x < area.left() - 1.0 || x > area.right() {
                continue;
            }
            painter.vline(x, rect.y_range(), Stroke::new(1.0, pal.bar));
            painter.text(
                pos2(x + 3.0, rect.center().y),
                egui::Align2::LEFT_CENTER,
                format!("{}", bar + 1),
                egui::FontId::monospace(10.0),
                pal.text,
            );
            if let Some(Some(ch)) = self.chords.get(bar) {
                painter.text(
                    pos2(x + 22.0, rect.center().y),
                    egui::Align2::LEFT_CENTER,
                    ch.name(),
                    egui::FontId::new(11.0, egui::FontFamily::Name("ui-bold".into())),
                    theme::text(),
                );
            }
        }
    }

    fn grid_lines(
        &self,
        painter: &egui::Painter,
        area: Rect,
        px: f32,
        grid: f64,
        shown: f64,
        pal: &Palette,
    ) {
        let step = if (grid as f32) * px >= 6.0 { grid } else { 1.0 };
        let mut b = (self.x_off / step).floor() * step;
        while b <= shown + 1e-9 {
            let x = self.x(area.left(), px, b);
            if x > area.right() {
                break;
            }
            if x >= area.left() {
                let is_bar = (b / 4.0 - (b / 4.0).round()).abs() < 1e-6;
                let is_beat = (b - b.round()).abs() < 1e-6;
                let (w, c) = if is_bar {
                    (1.5, pal.bar)
                } else if is_beat {
                    (1.0, theme::mix(pal.line, pal.bar, 0.4))
                } else {
                    (0.6, pal.line)
                };
                painter.vline(x, area.y_range(), Stroke::new(w, c));
            }
            b += step;
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn note_input(
        &mut self,
        ui: &egui::Ui,
        resp: &egui::Response,
        keys: Rect,
        area: Rect,
        px: f32,
        top: f32,
        state: &mut AppState,
        midi: &MidiData,
        note_rect: &dyn Fn(&MidiNote, bool) -> Rect,
    ) {
        let grid = self.grid();
        let key = self.key();
        let (mods, scroll) = ui.input(|i| (i.modifiers, i.smooth_scroll_delta));
        // Zoom mit Strg+Mausrad, Zeilenhöhe mit Alt+Mausrad, waagerecht mit Umschalt+Mausrad.
        if resp.hovered() {
            if mods.command && scroll.y != 0.0 {
                self.zoom = (self.zoom * (1.0 + scroll.y * 0.003)).clamp(1.0, 16.0);
            } else if mods.alt && scroll.y != 0.0 {
                self.row_h = (self.row_h + scroll.y * 0.02).clamp(6.0, 24.0);
            } else if mods.shift && (scroll.y != 0.0 || scroll.x != 0.0) {
                self.x_off -= f64::from((scroll.x + scroll.y) / px);
            }
        }
        let (x_off, row_h) = (self.x_off, self.row_h);
        let rows_now = self.rows.clone();
        let beat_at = move |x: f32| x_off + f64::from((x - area.left()) / px);
        let pitch_at = move |y: f32| {
            let i = ((y - top) / row_h).floor().max(0.0) as usize;
            rows_now
                .get(i.min(rows_now.len().saturating_sub(1)))
                .copied()
                .unwrap_or(60)
        };
        let hit = |pos: Pos2| -> Option<(usize, bool)> {
            midi.notes.iter().enumerate().rev().find_map(|(i, n)| {
                let r = note_rect(n, false);
                r.contains(pos)
                    .then(|| (i, pos.x >= r.right() - RESIZE_PX.min(r.width() / 3.0)))
            })
        };
        let press = resp.interact_pointer_pos();
        let track = self.track;

        // Klaviatur: Vorhören, solange gedrückt.
        let on_keys = press.is_some_and(|p| keys.contains(p)) && resp.is_pointer_button_down_on();
        let want = on_keys.then(|| press.map(|p| pitch_at(p.y))).flatten();
        if want != self.preview {
            if let (Some(p), Some(t)) = (self.preview.take(), track) {
                state.probeton.note_off(t, p);
            }
            if let (Some(p), Some(t)) = (want, track) {
                state.probeton.note_on(t, p, self.velocity);
                self.preview = Some(p);
            }
        }
        if let Some(pos) = resp.hover_pos() {
            if keys.contains(pos) {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            } else if let Some((_, true)) = hit(pos) {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
            }
        }

        if resp.double_clicked()
            && let Some(pos) = press.or(resp.hover_pos())
            && area.contains(pos)
            && let Some((i, _)) = hit(pos)
        {
            state.set_notes(self.clip, midi.delete_notes(&[i]));
            self.sel.clear();
            self.drag = None;
            return;
        }
        let origin = ui.input(|i| i.pointer.press_origin());
        if resp.drag_started()
            && let Some(origin) = origin
            && area.contains(origin)
        {
            self.drag = match (hit(origin), self.tool) {
                (Some(_), Tool::Erase) => None,
                (Some((i, edge)), _) => {
                    self.pick(i, mods);
                    Some(if edge {
                        Drag::Resize {
                            d_beats: 0.0,
                            raw: 0.0,
                        }
                    } else {
                        Drag::Move {
                            d_beats: 0.0,
                            d_pitch: 0,
                            raw: egui::Vec2::ZERO,
                        }
                    })
                }
                (None, _) => {
                    if !(mods.command || mods.shift) {
                        self.sel.clear();
                    }
                    Some(Drag::Select { from: origin })
                }
            };
        }
        if resp.dragged() {
            let delta = resp.drag_delta();
            self.drag = match self.drag {
                Some(Drag::Move { raw, .. }) => {
                    let raw = raw + delta;
                    let free = f64::from(raw.x / px);
                    let d_beats = if mods.alt {
                        free
                    } else {
                        (free / grid).round() * grid
                    };
                    // Tonhöhe über die Zeilen (auch bei „Nur benutzte Töne“), Magnet rastet ein.
                    let d_pitch = match origin {
                        Some(o) => {
                            let from = pitch_at(o.y);
                            let mut to = pitch_at(o.y + raw.y);
                            if self.magnet {
                                to = snap_to_key(key, to);
                            }
                            i32::from(to) - i32::from(from)
                        }
                        None => 0,
                    };
                    Some(Drag::Move {
                        d_beats,
                        d_pitch,
                        raw,
                    })
                }
                Some(Drag::Resize { raw, .. }) => {
                    let raw = raw + delta.x;
                    let free = f64::from(raw / px);
                    let d_beats = if mods.alt {
                        free
                    } else {
                        (free / grid).round() * grid
                    };
                    Some(Drag::Resize { d_beats, raw })
                }
                other => other,
            };
        }
        if resp.drag_stopped() {
            match self.drag.take() {
                Some(Drag::Move {
                    d_beats, d_pitch, ..
                }) => {
                    let (m, sel) = midi.move_notes(&self.sel, d_beats, d_pitch);
                    state.set_notes(self.clip, m);
                    self.sel = sel;
                }
                Some(Drag::Resize { d_beats, .. }) => {
                    let (m, sel) = midi.resize_notes(&self.sel, d_beats);
                    state.set_notes(self.clip, m);
                    self.sel = sel;
                }
                Some(Drag::Select { from }) => {
                    if let Some(to) = resp.interact_pointer_pos().or(resp.hover_pos()) {
                        let band = Rect::from_two_pos(from, to);
                        for (i, n) in midi.notes.iter().enumerate() {
                            if band.intersects(note_rect(n, false)) && !self.sel.contains(&i) {
                                self.sel.push(i);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        if resp.clicked()
            && let Some(pos) = press.or(resp.hover_pos())
            && area.contains(pos)
        {
            match (hit(pos), self.tool) {
                (Some((i, _)), Tool::Erase) => {
                    state.set_notes(self.clip, midi.delete_notes(&[i]));
                    self.sel.clear();
                }
                (Some((i, _)), _) => self.pick_click(i, mods),
                (None, _) if mods.command || mods.shift => {}
                (None, Tool::Draw) => {
                    // Klick ins Leere: Note im Raster zeichnen, Länge nach Leiste.
                    let beat = (beat_at(pos.x) / grid).floor() * grid;
                    let mut pitch = pitch_at(pos.y);
                    if self.magnet {
                        pitch = snap_to_key(key, pitch);
                    }
                    let (m, sel) = midi.add_note(MidiNote {
                        beat,
                        length_beats: self.new_len(),
                        pitch,
                        velocity: self.velocity,
                    });
                    if self.probe
                        && let Some(t) = track
                    {
                        state.probeton.tap(
                            t,
                            &[pitch],
                            self.velocity,
                            std::time::Instant::now(),
                            std::time::Duration::ZERO,
                        );
                    }
                    state.set_notes(self.clip, m);
                    self.sel = sel;
                }
                (None, _) => self.sel.clear(),
            }
        }
    }

    /// Pointer pressed on note `i` before a drag: keep a multi-selection, else select it.
    fn pick(&mut self, i: usize, mods: egui::Modifiers) {
        if mods.command || mods.shift {
            if !self.sel.contains(&i) {
                self.sel.push(i);
            }
        } else if !self.sel.contains(&i) {
            self.sel = vec![i];
        }
    }

    /// Click on note `i`: select it, Strg/Umschalt toggles it.
    fn pick_click(&mut self, i: usize, mods: egui::Modifiers) {
        if mods.command || mods.shift {
            if let Some(k) = self.sel.iter().position(|&x| x == i) {
                self.sel.remove(k);
            } else {
                self.sel.push(i);
            }
        } else {
            self.sel = vec![i];
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn velocity_lane(
        &mut self,
        ui: &egui::Ui,
        resp: &egui::Response,
        rect: Rect,
        px: f32,
        state: &mut AppState,
        midi: &MidiData,
        pal: &Palette,
    ) {
        let painter = ui.painter_at(rect);
        let lane = Rect::from_min_max(pos2(rect.left() + KEYS_W, rect.top()), rect.max);
        painter.rect_filled(rect, 0.0, theme::well());
        painter.rect_filled(lane, 0.0, pal.bg);
        painter.text(
            pos2(rect.left() + 4.0, rect.top() + 12.0),
            egui::Align2::LEFT_CENTER,
            "VELOCITY",
            egui::FontId::new(9.0, egui::FontFamily::Name("ui-bold".into())),
            pal.text,
        );
        let inner = lane.shrink2(vec2(0.0, 6.0));
        // Mittellinie gestrichelt (Velocity 64).
        let mid = inner.bottom() - inner.height() * 64.0 / 127.0;
        painter.extend(egui::Shape::dashed_line(
            &[pos2(lane.left(), mid), pos2(lane.right(), mid)],
            Stroke::new(1.0, pal.line),
            4.0,
            4.0,
        ));
        let value_at = |y: f32| {
            (((inner.bottom() - y) / inner.height()) * 127.0)
                .round()
                .clamp(1.0, 127.0) as u8
        };
        // Ziehen beginnt an der nächstliegenden Note (gewählte bevorzugt).
        if resp.drag_started()
            && let Some(o) = ui.input(|i| i.pointer.press_origin())
        {
            let beat = self.x_off + f64::from((o.x - lane.left()) / px);
            let near = |only_sel: bool| {
                midi.notes
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| !only_sel || self.sel.contains(i))
                    .min_by(|(_, a), (_, b)| {
                        (a.beat - beat).abs().total_cmp(&(b.beat - beat).abs())
                    })
                    .map(|(i, _)| i)
            };
            let tol = f64::from(12.0 / px);
            let pick = near(true)
                .filter(|&i| (midi.notes[i].beat - beat).abs() <= tol)
                .or_else(|| near(false));
            if let Some(i) = pick {
                if !self.sel.contains(&i) {
                    self.sel = vec![i];
                }
                self.drag = Some(Drag::Velocity {
                    note: i,
                    value: value_at(o.y),
                });
            }
        }
        if resp.dragged()
            && let Some(Drag::Velocity { note, .. }) = self.drag
            && let Some(p) = resp.interact_pointer_pos()
        {
            self.drag = Some(Drag::Velocity {
                note,
                value: value_at(p.y),
            });
        }
        if resp.drag_stopped()
            && let Some(Drag::Velocity { value, .. }) = self.drag.take()
        {
            state.set_notes(self.clip, midi.set_velocity(&self.sel, value));
            self.velocity = value;
        }
        let live = match self.drag {
            Some(Drag::Velocity { value, .. }) => Some(value),
            _ => None,
        };
        for (i, n) in midi.notes.iter().enumerate() {
            let x = self.x(lane.left(), px, n.beat);
            if x < lane.left() - 1.0 || x > lane.right() {
                continue;
            }
            let sel = self.sel.contains(&i);
            let v = match live {
                Some(v) if sel => v,
                _ => n.velocity,
            };
            let h = inner.height() * f32::from(v) / 127.0;
            let c = if sel { pal.note_sel } else { pal.note };
            painter.vline(
                x + 1.0,
                inner.bottom() - h..=inner.bottom(),
                Stroke::new(2.0, c),
            );
            painter.circle_filled(pos2(x + 1.0, inner.bottom() - h), 3.5, c);
        }
        resp.clone()
            .on_hover_text("Senkrecht ziehen: Velocity der Auswahl (sonst der nächsten Note)");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::kittest::Queryable;

    #[test]
    fn notes_take_the_colour_of_their_track() {
        let tmp = tempfile::tempdir().expect("tmp");
        let mut st = AppState::new(tmp.path().join("rec"), std::time::Instant::now());
        let p = woa_model::chords::progression("I–V–vi–IV").expect("pop");
        let made = st.add_chord_tracks(p, 0, 4, 0.0).expect("made");
        for (i, (_, clip)) in made.iter().enumerate() {
            assert_eq!(PianoRoll::note_color(&st, *clip), theme::track_color(i));
        }
    }

    #[test]
    fn pitch_names() {
        assert_eq!(pitch_name(60), "C4");
        assert_eq!(pitch_name(69), "A4");
        assert_eq!(pitch_name(0), "C-1");
        assert!(is_black(61) && !is_black(64));
    }

    fn note(pitch: u8) -> MidiNote {
        MidiNote {
            beat: 0.0,
            length_beats: 1.0,
            pitch,
            velocity: 100,
        }
    }

    #[test]
    fn only_used_pitches_fold_the_rows() {
        let notes = [note(64), note(60), note(64), note(67)];
        assert_eq!(rows(&notes, true), vec![67, 64, 60]);
        assert_eq!(rows(&notes, false).len(), 128);
        assert_eq!(rows(&[], true).len(), 128, "ohne Noten alle Zeilen");
        let r = rows(&notes, true);
        assert_eq!(row_index(&r, 64), 1);
        assert_eq!(
            row_index(&r, 62),
            2,
            "fehlende Tonhöhe: nächsttiefere Zeile"
        );
        assert_eq!(row_index(&rows(&[], false), 127), 0);
    }

    #[test]
    fn magnet_snaps_to_the_key() {
        let c = Some(Key {
            tonic: 0,
            minor: false,
        });
        assert_eq!(snap_to_key(c, 61), 60, "Cis → C");
        assert_eq!(snap_to_key(c, 64), 64);
        assert_eq!(snap_to_key(None, 61), 61);
        let a = Some(Key {
            tonic: 9,
            minor: false,
        });
        assert_eq!(snap_to_key(a, 60), 59, "A Dur: C → H");
    }

    struct Probe {
        state: AppState,
        pr: PianoRoll,
    }

    fn harness(kit: bool) -> (tempfile::TempDir, egui_kittest::Harness<'static, Probe>) {
        let tmp = tempfile::tempdir().expect("tmp");
        let mut state = AppState::new(tmp.path().join("rec"), std::time::Instant::now());
        let track = state.add_midi_track().expect("track");
        if kit {
            state.set_track_sound(
                track,
                Some(woa_model::TrackSound {
                    program: 0,
                    drums: true,
                    bank: None,
                }),
            );
        }
        let clip = state
            .add_midi_clip(Some(track), "Gitarre", 0.0, 16.0, MidiData::default())
            .expect("clip");
        let probe = Probe {
            state,
            pr: PianoRoll::new(clip),
        };
        let h = egui_kittest::Harness::builder()
            .with_size(vec2(1600.0, 600.0))
            .build_ui_state(
                |ui, p: &mut Probe| {
                    if crate::theme::install_fonts(ui.ctx()) {
                        return;
                    }
                    p.pr.show_inline(ui, &mut p.state);
                },
                probe,
            );
        (tmp, h)
    }

    #[test]
    fn toolbar_has_tools_and_no_instruction_line() {
        let (_tmp, mut h) = harness(false);
        h.run_steps(2);
        for name in [
            "Zeichnen (D)",
            "Auswählen (V)",
            "Löschen (E)",
            "Triolen",
            "Alle quantisieren (Q)",
            "Auf Tonart einrasten",
            "Nur benutzte Töne",
            "Probeton beim Zeichnen",
            "Vergrößern",
            "Verkleinern",
        ] {
            let _ = h.get_by_label(name);
        }
        assert!(h.query_by_label_contains("Klick: zeichnen").is_none());
        assert!(h.query_by_label("Drum-Raster").is_none());
        assert!(
            h.query_by_label("Raster").is_none(),
            "Noten | Raster nur auf Kits"
        );
        h.get_by_label("Löschen (E)").click();
        h.run_steps(2);
        assert_eq!(h.state().pr.tool, Tool::Erase);
        h.get_by_label("Probeton beim Zeichnen").click();
        h.run_steps(2);
        assert!(!h.state().pr.probe);
    }

    #[test]
    fn kit_tracks_open_the_drum_grid_and_preview_a_row() {
        let (_tmp, mut h) = harness(true);
        h.run_steps(2);
        let _ = h.get_by_label("Raster");
        let _ = h.get_by_label("Noten");
        h.get_by_label("Kick vorhören").click();
        h.run_steps(1);
        let p = h.state_mut();
        let got = p.state.probeton.take(std::time::Instant::now());
        assert!(
            matches!(
                got.first(),
                Some(crate::probeton::Probe::NoteOn { pitch: 36, .. })
            ),
            "Probeton Kick als Note-an-Command: {got:?}"
        );
        assert!(
            !p.state.probeton.is_idle(),
            "Note aus ist geplant, nichts bleibt hängen"
        );
    }
}
