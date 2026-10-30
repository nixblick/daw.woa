//! Piano-Roll (Paket 13): Editor für einen MIDI-Clip.
//!
//! Noten zeichnen (Klick ins Leere), verschieben (ziehen), Länge ziehen (rechter Rand),
//! löschen (Doppelklick, Entf), Mehrfachauswahl (Strg/Umschalt+Klick, Rahmen ziehen,
//! Strg+A), Raster und Quantisieren (1/4 bis 1/32, Triolen), Velocity-Leiste, Klaviatur
//! mit Vorhören, Zoom. Jede Änderung geht als ein `SetNotes` durch [`AppState`], so
//! haben Oberfläche und Agent denselben Weg und Strg+Z nimmt sie einzeln zurück.
//!
//! Vorhören läuft über die `rtrb`-Queue der Live-Spur (dieselbe wie das Keyboard); der
//! Audio-Thread liest dort ohne Sperre. Ist kein Ausgang aktiv, bleibt es still.

use eframe::egui;
use egui::{Color32, Pos2, Rect, Sense, Stroke, pos2, vec2};
use woa_dsp::instrument::NoteEvent;
use woa_model::{ClipId, MidiData, MidiNote, grid_beats};

use crate::state::AppState;
use crate::theme;

const KEYS_W: f32 = 46.0;
const VEL_H: f32 = 70.0;
const RESIZE_PX: f32 = 6.0;
const DIVISIONS: [u32; 4] = [4, 8, 16, 32];

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
struct Palette {
    bg: Color32,
    row_black: Color32,
    line: Color32,
    bar: Color32,
    note: Color32,
    note_sel: Color32,
    text: Color32,
    key_white: Color32,
    key_black: Color32,
}

impl Palette {
    fn of(ui: &egui::Ui) -> Self {
        if ui.visuals().dark_mode {
            Self {
                bg: theme::bg(),
                row_black: theme::mix(theme::bg(), Color32::BLACK, 0.25),
                line: theme::mix(theme::bg(), theme::grid(), 0.6),
                bar: theme::mix(theme::grid(), theme::text_dim(), 0.35),
                note: Color32::from_rgb(82, 156, 255),
                note_sel: theme::accent(),
                text: theme::text_dim(),
                key_white: Color32::from_rgb(214, 218, 226),
                key_black: Color32::from_rgb(28, 30, 36),
            }
        } else {
            Self {
                bg: theme::panel(),
                row_black: theme::well(),
                line: theme::grid(),
                bar: theme::border_strong(),
                note: theme::track_color(0),
                note_sel: theme::accent(),
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
            x_off: 0.0,
            row_h: 12.0,
            drag: None,
            velocity: 100,
            preview: None,
            centre: true,
        }
    }

    fn grid(&self) -> f64 {
        grid_beats(self.division, self.triplet)
    }

    fn notes(state: &AppState, clip: ClipId) -> Option<(MidiData, f64, String)> {
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
        let (del, back, all, quant, esc) = ctx.input_mut(|i| {
            (
                i.consume_key(n, egui::Key::Delete),
                i.consume_key(n, egui::Key::Backspace),
                i.consume_key(egui::Modifiers::COMMAND, egui::Key::A),
                i.consume_key(n, egui::Key::Q),
                i.consume_key(n, egui::Key::Escape),
            )
        });
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
        !esc
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

    fn send(sink: Option<&woa_io::midi::EventSink>, ev: NoteEvent) {
        if let Some(sink) = sink
            && let Ok(mut g) = sink.lock()
            && let Some(p) = g.as_mut()
        {
            // Volle Queue: Vorhören fällt aus, nichts blockiert.
            let _ = p.push(ev);
        }
    }

    /// Draws the editor window. Returns `false` when it was closed or its clip is gone.
    /// `sink` is the live instrument queue of the clip's track (preview), if active.
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        state: &mut AppState,
        sink: Option<&woa_io::midi::EventSink>,
    ) -> bool {
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
        egui::Window::new(format!("Piano-Roll – {name}"))
            .id(egui::Id::new(("pianoroll", self.clip.0)))
            .open(&mut open)
            .default_size(size)
            .default_pos(pos2(screen.left() + 20.0, screen.top() + 40.0))
            .constrain(true)
            .min_size([480.0, 320.0])
            .resizable(true)
            .show(ctx, |ui| {
                self.toolbar(ui, state, &midi);
                ui.separator();
                self.editor(ui, state, &midi, length, sink);
            });
        if !open && let Some(p) = self.preview.take() {
            Self::send(sink, NoteEvent::note_off(p));
        }
        open
    }

    /// Draws the Piano-Roll inline inside an existing panel (e.g. the docked bottom Studio panel).
    /// Returns `false` if the edited clip no longer exists.
    pub fn show_inline(
        &mut self,
        ui: &mut egui::Ui,
        state: &mut AppState,
        sink: Option<&woa_io::midi::EventSink>,
    ) -> bool {
        let Some((midi, length, _)) = Self::notes(state, self.clip) else {
            return false;
        };
        self.sel.retain(|&i| i < midi.notes.len());
        self.toolbar(ui, state, &midi);
        ui.separator();
        self.editor(ui, state, &midi, length, sink);
        true
    }

    fn toolbar(&mut self, ui: &mut egui::Ui, state: &mut AppState, midi: &MidiData) {
        ui.horizontal_wrapped(|ui| {
            ui.label("Raster");
            egui::ComboBox::from_id_salt(("pr_grid", self.clip.0))
                .selected_text(format!("1/{}", self.division))
                .width(64.0)
                .show_ui(ui, |ui| {
                    for d in DIVISIONS {
                        ui.selectable_value(&mut self.division, d, format!("1/{d}"));
                    }
                });
            ui.checkbox(&mut self.triplet, "Triolen");
            let what = if self.sel.is_empty() {
                "Alle quantisieren (Q)"
            } else {
                "Auswahl quantisieren (Q)"
            };
            if ui.button(what).clicked() {
                self.quantize(state, midi);
            }
            if ui
                .add_enabled(!self.sel.is_empty(), egui::Button::new("Löschen (Entf)"))
                .clicked()
            {
                self.delete(state, midi);
            }
            ui.separator();
            ui.label("Zoom");
            ui.add(
                egui::Slider::new(&mut self.zoom, 1.0..=16.0)
                    .logarithmic(true)
                    .show_value(false),
            )
            .on_hover_text("Auch Strg+Mausrad über den Noten");
            ui.add(
                egui::Slider::new(&mut self.row_h, 6.0..=24.0)
                    .show_value(false)
                    .text("Höhe"),
            );
            ui.end_row();
            let info = match self.sel.as_slice() {
                [] => format!("{} Noten", midi.notes.len()),
                [i] => midi.notes.get(*i).map_or_else(String::new, |n| {
                    format!(
                        "{} · Takt {:.2} · Länge {:.3} · Velocity {}",
                        pitch_name(n.pitch),
                        n.beat / 4.0 + 1.0,
                        n.length_beats,
                        n.velocity
                    )
                }),
                s => format!("{} von {} Noten gewählt", s.len(), midi.notes.len()),
            };
            ui.label(egui::RichText::new(info).small());
            ui.label(
                egui::RichText::new(
                    "Klick: zeichnen · Ziehen: verschieben · rechter Rand: Länge · \
                     Doppelklick/Entf: löschen · Rahmen, Strg+Klick, Strg+A: Auswahl · \
                     Alt: ohne Raster · Esc: schließen",
                )
                .small()
                .weak(),
            );
        });
    }

    /// x position of `beat` in a lane starting at `left`.
    fn x(&self, left: f32, px: f32, beat: f64) -> f32 {
        left + ((beat - self.x_off) as f32) * px
    }

    fn editor(
        &mut self,
        ui: &mut egui::Ui,
        state: &mut AppState,
        midi: &MidiData,
        length: f64,
        sink: Option<&woa_io::midi::EventSink>,
    ) {
        let pal = Palette::of(ui);
        let avail = ui.available_size();
        let view_w = (avail.x - KEYS_W).max(100.0);
        let shown = length.max(midi.end_beats()).max(4.0);
        let px = view_w / shown as f32 * self.zoom;
        let visible = f64::from(view_w / px);
        self.x_off = self.x_off.clamp(0.0, (shown - visible).max(0.0));
        let grid = self.grid();
        let notes_h = (avail.y - VEL_H - 22.0).max(120.0);
        let row_h = self.row_h;
        let full_h = 128.0 * row_h;

        // Notenfläche mit Klaviatur, senkrecht scrollbar.
        let mut scroll = egui::ScrollArea::vertical()
            .id_salt(("pr_scroll", self.clip.0))
            .max_height(notes_h)
            .auto_shrink([false, false]);
        if self.centre {
            // Auf die Noten (sonst C4) zentrieren.
            let mid = if midi.notes.is_empty() {
                60.0
            } else {
                let (lo, hi) = midi.notes.iter().fold((127u8, 0u8), |(lo, hi), n| {
                    (lo.min(n.pitch), hi.max(n.pitch))
                });
                (f32::from(lo) + f32::from(hi)) / 2.0
            };
            scroll =
                scroll.vertical_scroll_offset(((127.0 - mid) * row_h - notes_h / 2.0).max(0.0));
            self.centre = false;
        }
        scroll.show(ui, |ui| {
            let (rect, resp) =
                ui.allocate_exact_size(vec2(KEYS_W + view_w, full_h), Sense::click_and_drag());
            let painter = ui.painter_at(rect);
            let keys = Rect::from_min_size(rect.min, vec2(KEYS_W, full_h));
            let area = Rect::from_min_max(pos2(keys.right(), rect.top()), rect.max);
            let y_of = |pitch: u8| rect.top() + (127.0 - f32::from(pitch)) * row_h;
            // Zeilen.
            for p in 0..=127u8 {
                let y = y_of(p);
                let r = Rect::from_x_y_ranges(area.x_range(), y..=y + row_h);
                painter.rect_filled(r, 0.0, if is_black(p) { pal.row_black } else { pal.bg });
                if p % 12 == 0 {
                    painter.hline(area.x_range(), y + row_h, Stroke::new(1.0, pal.bar));
                }
            }
            // Raster: Takte kräftig, Schläge und Rasterschritte fein.
            self.grid_lines(&painter, area, px, grid, shown, &pal);
            // Clip-Ende.
            let xe = self.x(area.left(), px, length);
            if xe < area.right() {
                painter.rect_filled(
                    Rect::from_x_y_ranges(xe..=area.right(), area.y_range()),
                    0.0,
                    pal.bg.gamma_multiply(0.6),
                );
                painter.vline(xe, area.y_range(), Stroke::new(2.0, pal.bar));
            }
            // Klaviatur.
            for p in 0..=127u8 {
                let y = y_of(p);
                let r = Rect::from_x_y_ranges(keys.x_range(), y..=y + row_h);
                let held = self.preview == Some(p);
                let fill = match (held, is_black(p)) {
                    (true, _) => pal.note_sel,
                    (false, true) => pal.key_black,
                    (false, false) => pal.key_white,
                };
                painter.rect_filled(r.shrink2(vec2(0.0, 0.5)), 1.0, fill);
                if p % 12 == 0 && row_h >= 8.0 {
                    painter.text(
                        pos2(keys.right() - 3.0, y + row_h / 2.0),
                        egui::Align2::RIGHT_CENTER,
                        pitch_name(p),
                        egui::FontId::proportional((row_h - 2.0).min(11.0)),
                        Color32::from_rgb(60, 64, 74),
                    );
                }
            }
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
                let base = if lit { pal.note_sel } else { pal.note };
                // Velocity als Helligkeit.
                let fill = theme::mix(pal.bg, base, 0.45 + 0.55 * f32::from(n.velocity) / 127.0);
                let r = r.intersect(area);
                painter.rect_filled(r, 2.0, fill);
                painter.rect_stroke(r, 2.0, Stroke::new(1.0, base), egui::StrokeKind::Inside);
            }
            if let Some(b) = band {
                painter.rect_stroke(
                    b,
                    0.0,
                    Stroke::new(1.0, pal.note_sel),
                    egui::StrokeKind::Inside,
                );
            }
            self.note_input(
                ui,
                &resp,
                keys,
                area,
                px,
                y_of(127),
                state,
                midi,
                sink,
                &note_rect,
            );
        });

        // Velocity-Leiste unter der Notenfläche, gleiche Zeitachse.
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
                    (1.0, pal.line)
                };
                painter.vline(x, area.y_range(), Stroke::new(w, c));
                if is_bar {
                    painter.text(
                        pos2(x + 3.0, area.top() + 2.0),
                        egui::Align2::LEFT_TOP,
                        format!("{}", (b / 4.0).round() as i64 + 1),
                        egui::FontId::proportional(10.0),
                        pal.text,
                    );
                }
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
        sink: Option<&woa_io::midi::EventSink>,
        note_rect: &dyn Fn(&MidiNote, bool) -> Rect,
    ) {
        let grid = self.grid();
        let (mods, scroll) = ui.input(|i| (i.modifiers, i.smooth_scroll_delta));
        // Zoom mit Strg+Mausrad, waagerecht blättern mit Umschalt+Mausrad.
        if resp.hovered() {
            if mods.command && scroll.y != 0.0 {
                self.zoom = (self.zoom * (1.0 + scroll.y * 0.003)).clamp(1.0, 16.0);
            } else if mods.shift && (scroll.y != 0.0 || scroll.x != 0.0) {
                self.x_off -= f64::from((scroll.x + scroll.y) / px);
            }
        }
        let (x_off, row_h) = (self.x_off, self.row_h);
        let beat_at = move |x: f32| x_off + f64::from((x - area.left()) / px);
        let pitch_at = move |y: f32| (127 - ((y - top) / row_h).floor() as i32).clamp(0, 127) as u8;
        let hit = |pos: Pos2| -> Option<(usize, bool)> {
            midi.notes.iter().enumerate().rev().find_map(|(i, n)| {
                let r = note_rect(n, false);
                r.contains(pos)
                    .then(|| (i, pos.x >= r.right() - RESIZE_PX.min(r.width() / 3.0)))
            })
        };
        let press = resp.interact_pointer_pos();

        // Klaviatur: Vorhören, solange gedrückt.
        let on_keys = press.is_some_and(|p| keys.contains(p)) && resp.is_pointer_button_down_on();
        let want = on_keys.then(|| press.map(|p| pitch_at(p.y))).flatten();
        if want != self.preview {
            if let Some(p) = self.preview.take() {
                Self::send(sink, NoteEvent::note_off(p));
            }
            if let Some(p) = want {
                Self::send(sink, NoteEvent::note_on(p, self.velocity));
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
        if resp.drag_started()
            && let Some(origin) = ui.input(|i| i.pointer.press_origin())
            && area.contains(origin)
        {
            self.drag = Some(match hit(origin) {
                Some((i, edge)) => {
                    self.pick(i, mods);
                    if edge {
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
                    }
                }
                None => {
                    if !(mods.command || mods.shift) {
                        self.sel.clear();
                    }
                    Drag::Select { from: origin }
                }
            });
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
                    Some(Drag::Move {
                        d_beats,
                        d_pitch: -(raw.y / self.row_h).round() as i32,
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
            match hit(pos) {
                Some((i, _)) => self.pick_click(i, mods),
                None if mods.command || mods.shift => {}
                None => {
                    // Klick ins Leere: Note im Raster zeichnen, einen Rasterschritt lang.
                    let beat = (beat_at(pos.x) / grid).floor() * grid;
                    let (m, sel) = midi.add_note(MidiNote {
                        beat,
                        length_beats: grid,
                        pitch: pitch_at(pos.y),
                        velocity: self.velocity,
                    });
                    Self::send(sink, NoteEvent::note_on(pitch_at(pos.y), self.velocity));
                    Self::send(sink, NoteEvent::note_off(pitch_at(pos.y)));
                    state.set_notes(self.clip, m);
                    self.sel = sel;
                }
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
        painter.rect_filled(rect, 0.0, pal.row_black);
        painter.rect_filled(lane, 0.0, pal.bg);
        painter.text(
            pos2(rect.left() + 4.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            "Velocity",
            egui::FontId::proportional(10.0),
            pal.text,
        );
        let inner = lane.shrink2(vec2(0.0, 4.0));
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
                Stroke::new(3.0, c),
            );
            painter.circle_filled(pos2(x + 1.0, inner.bottom() - h), 3.5, c);
        }
        resp.clone()
            .on_hover_text("Senkrecht ziehen: Velocity der Auswahl (sonst der nächsten Note)");
    }
}

/// Namen und Tastenfarben der Klaviatur.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pitch_names() {
        assert_eq!(pitch_name(60), "C4");
        assert_eq!(pitch_name(69), "A4");
        assert_eq!(pitch_name(0), "C-1");
        assert!(is_black(61) && !is_black(64));
    }
}
