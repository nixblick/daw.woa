//! Rückmeldung von Reglern, die der Nutzer in der Plugin-Oberfläche bewegt (Paket 32).
//!
//! Das Plugin meldet Änderungen als Ausgangs-Events des Audio-Blocks: `ParamGestureBegin`,
//! `ParamValue` (beliebig oft) und `ParamGestureEnd`. Der Audio-Thread legt sie unverändert in eine
//! lock-free Queue; hier, im Control-Thread, werden sie zu Bearbeitungen für das Projekt:
//!
//! - Eine Geste (Anfassen bis Loslassen) ergibt **eine** Bearbeitung mit dem Wert vom Anfang
//!   (`old`) und dem vom Ende (`new`), also einen Rückgängig-Schritt, egal wie lange gezogen wurde.
//! - Werte ohne Geste (MIDI-Zuweisung, Tastatur) werden je Regler zusammengefasst.
//! - Sind es viele auf einmal oder meldet das Plugin „Werte neu lesen“ (Preset gewechselt), ergibt das
//!   **eine** Bearbeitung [`ParamEdit::WholeState`]: der ganze Plugin-Zustand wird ins Projekt
//!   übernommen, nicht Hunderte Regler.
//!
//! Die Idee, Gesten zu einem Schritt zusammenzufassen (Anfassen/Loslassen), stammt aus Ardours
//! Automationsbehandlung (`start_touch`/`stop_touch`, GPLv2+); übernommen ist kein Code.

use std::collections::HashMap;

/// Ab so vielen verschiedenen Reglern ohne Geste gilt die Änderung als Preset-Wechsel.
pub const LOOSE_LIMIT: usize = 8;
/// Werte, die weniger abweichen, gelten als gleich (Rundung über `f32` im Mixer).
const EPS: f64 = 1e-5;

/// Art einer Meldung des Plugins.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    /// Der Nutzer fasst den Regler an.
    GestureBegin,
    /// Neuer Wert.
    Value,
    /// Der Nutzer lässt den Regler los.
    GestureEnd,
}

/// Eine Meldung aus dem Audio-Thread (klein, `Copy`, damit sie ohne Allokation durch die Queue geht).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParamChange {
    /// CLAP-Kennung des Reglers.
    pub id: u32,
    /// Was gemeldet wurde.
    pub kind: ChangeKind,
    /// Wert (nur bei [`ChangeKind::Value`]).
    pub value: f64,
}

/// Was ins Projekt übernommen werden soll.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ParamEdit {
    /// Ein Regler hat sich geändert (ein Rückgängig-Schritt).
    Param {
        /// Reglerkennung.
        id: u32,
        /// Wert vor der Änderung.
        old: f64,
        /// Wert danach.
        new: f64,
    },
    /// Viele Regler auf einmal (Preset): den ganzen Zustand übernehmen.
    WholeState,
}

/// Merkt sich die Werte der Regler und fasst Meldungen zu Bearbeitungen zusammen.
#[derive(Debug, Default)]
pub struct Feedback {
    mirror: HashMap<u32, f64>,
    /// Angefasste Regler: Wert beim Anfassen und letzter gemeldeter Wert.
    open: HashMap<u32, (f64, f64)>,
}

fn same(a: f64, b: f64) -> bool {
    (a - b).abs() <= EPS * a.abs().max(b.abs()).max(1.0)
}

impl Feedback {
    /// Leer.
    pub fn new() -> Self {
        Self::default()
    }

    /// Setzt den bekannten Wert eines Reglers (nach Laden, Zustand setzen, Wert von außen).
    pub fn set_known(&mut self, id: u32, value: f64) {
        self.mirror.insert(id, value);
    }

    /// Bekannter Wert eines Reglers.
    pub fn known(&self, id: u32) -> Option<f64> {
        self.mirror.get(&id).copied()
    }

    /// Verwirft alle Merkwerte und offenen Gesten (vor dem Neulesen aller Werte).
    pub fn clear(&mut self) {
        self.mirror.clear();
        self.open.clear();
    }

    /// Ob gerade ein Regler angefasst ist.
    pub fn gesture_open(&self) -> bool {
        !self.open.is_empty()
    }

    /// Verarbeitet einen Stapel Meldungen und hängt die abgeschlossenen Bearbeitungen an `out`.
    /// `rescan`: das Plugin hat „Werte neu lesen“ gemeldet. `emit`: `false` führt nur die
    /// Merkwerte nach (Fenster zu), ohne Bearbeitungen.
    pub fn feed(
        &mut self,
        batch: &[ParamChange],
        rescan: bool,
        emit: bool,
        out: &mut Vec<ParamEdit>,
    ) {
        let mut loose: Vec<(u32, f64)> = Vec::new();
        for c in batch {
            match c.kind {
                ChangeKind::GestureBegin => {
                    let start = self.mirror.get(&c.id).copied().unwrap_or(0.0);
                    self.open.entry(c.id).or_insert((start, start));
                }
                ChangeKind::Value => {
                    if !c.value.is_finite() {
                        continue;
                    }
                    if let Some(g) = self.open.get_mut(&c.id) {
                        g.1 = c.value;
                    } else if let Some(l) = loose.iter_mut().find(|l| l.0 == c.id) {
                        l.1 = c.value;
                    } else {
                        loose.push((c.id, c.value));
                    }
                }
                ChangeKind::GestureEnd => {
                    if let Some((old, new)) = self.open.remove(&c.id) {
                        self.mirror.insert(c.id, new);
                        if emit && !same(old, new) {
                            out.push(ParamEdit::Param { id: c.id, old, new });
                        }
                    }
                }
            }
        }
        if rescan || loose.len() > LOOSE_LIMIT {
            for (id, v) in loose {
                self.mirror.insert(id, v);
            }
            if emit {
                out.push(ParamEdit::WholeState);
            }
            return;
        }
        for (id, new) in loose {
            let old = self.mirror.insert(id, new).unwrap_or(new);
            if emit && !same(old, new) {
                out.push(ParamEdit::Param { id, old, new });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn begin(id: u32) -> ParamChange {
        ParamChange {
            id,
            kind: ChangeKind::GestureBegin,
            value: 0.0,
        }
    }
    fn value(id: u32, v: f64) -> ParamChange {
        ParamChange {
            id,
            kind: ChangeKind::Value,
            value: v,
        }
    }
    fn end(id: u32) -> ParamChange {
        ParamChange {
            id,
            kind: ChangeKind::GestureEnd,
            value: 0.0,
        }
    }

    #[test]
    fn a_whole_drag_is_one_edit_with_start_and_end_value() {
        let mut f = Feedback::new();
        f.set_known(1, 0.25);
        let mut out = Vec::new();
        f.feed(
            &[begin(1), value(1, 0.3), value(1, 0.5), value(1, 0.75)],
            false,
            true,
            &mut out,
        );
        assert!(out.is_empty(), "nothing before release");
        assert!(f.gesture_open());
        f.feed(&[end(1)], false, true, &mut out);
        assert_eq!(
            out,
            vec![ParamEdit::Param {
                id: 1,
                old: 0.25,
                new: 0.75
            }]
        );
        assert_eq!(f.known(1), Some(0.75));
        assert!(!f.gesture_open());
    }

    #[test]
    fn a_gesture_back_to_the_start_is_no_edit() {
        let mut f = Feedback::new();
        f.set_known(1, 0.5);
        let mut out = Vec::new();
        f.feed(
            &[begin(1), value(1, 0.9), value(1, 0.5), end(1)],
            false,
            true,
            &mut out,
        );
        assert!(out.is_empty());
    }

    #[test]
    fn loose_values_are_merged_per_param() {
        let mut f = Feedback::new();
        f.set_known(1, 0.0);
        f.set_known(2, 1.0);
        let mut out = Vec::new();
        f.feed(
            &[value(1, 0.1), value(1, 0.2), value(2, 0.5)],
            false,
            true,
            &mut out,
        );
        assert_eq!(
            out,
            vec![
                ParamEdit::Param {
                    id: 1,
                    old: 0.0,
                    new: 0.2
                },
                ParamEdit::Param {
                    id: 2,
                    old: 1.0,
                    new: 0.5
                },
            ]
        );
    }

    #[test]
    fn many_loose_values_become_one_whole_state_edit() {
        let mut f = Feedback::new();
        let mut out = Vec::new();
        let batch: Vec<ParamChange> = (0..=LOOSE_LIMIT as u32).map(|i| value(i, 0.5)).collect();
        f.feed(&batch, false, true, &mut out);
        assert_eq!(out, vec![ParamEdit::WholeState]);
    }

    #[test]
    fn a_rescan_is_one_whole_state_edit_even_for_one_value() {
        let mut f = Feedback::new();
        let mut out = Vec::new();
        f.feed(&[value(3, 0.5)], true, true, &mut out);
        assert_eq!(out, vec![ParamEdit::WholeState]);
    }

    #[test]
    fn values_the_host_set_itself_are_not_reported_back() {
        let mut f = Feedback::new();
        f.set_known(1, 0.123_456_789);
        let mut out = Vec::new();
        // Der Mixer rechnet in f32; die Echo-Meldung weicht nur um die Rundung ab.
        f.feed(
            &[value(1, f64::from(0.123_456_79_f32))],
            false,
            true,
            &mut out,
        );
        assert!(out.is_empty());
    }

    #[test]
    fn closed_window_only_tracks_values() {
        let mut f = Feedback::new();
        f.set_known(1, 0.0);
        let mut out = Vec::new();
        f.feed(
            &[begin(1), value(1, 0.7), end(1), value(2, 0.1)],
            true,
            false,
            &mut out,
        );
        assert!(out.is_empty());
        assert_eq!(f.known(1), Some(0.7));
    }

    #[test]
    fn non_finite_values_are_ignored() {
        let mut f = Feedback::new();
        let mut out = Vec::new();
        f.feed(
            &[value(1, f64::NAN), value(1, f64::INFINITY)],
            false,
            true,
            &mut out,
        );
        assert!(out.is_empty());
    }
}
