//! Undo/redo stacks with grouping.

use crate::command::Command;

/// One undo step: a label plus the commands it consists of.
///
/// `forward` holds the commands as applied (in order), `inverse` their inverses
/// (in the same order). Undo applies `inverse` back to front; redo applies `forward`
/// front to back.
#[derive(Debug, Clone, PartialEq)]
pub struct HistoryEntry {
    /// Label shown in the GUI ("Undo Set gain"), e.g. "Agent" for agent calls.
    pub label: String,
    /// Commands as they were applied.
    pub forward: Vec<Command>,
    /// Inverse of each forward command, same order.
    pub inverse: Vec<Command>,
}

/// Undo and redo stacks.
///
/// Usually used through [`crate::Session`]; exposed for inspection (e.g. a history
/// panel in the GUI).
#[derive(Debug, Clone, Default)]
pub struct History {
    pub(crate) undo: Vec<HistoryEntry>,
    pub(crate) redo: Vec<HistoryEntry>,
    pub(crate) open: Option<HistoryEntry>,
    pub(crate) depth: usize,
    pub(crate) limit: Option<usize>,
}

impl History {
    /// Empty history without size limit.
    pub fn new() -> Self {
        Self::default()
    }

    /// Keeps at most `limit` undo steps; older ones are dropped.
    pub fn with_limit(limit: usize) -> Self {
        Self {
            limit: Some(limit),
            ..Self::default()
        }
    }

    /// Undo steps, oldest first.
    pub fn undo_entries(&self) -> &[HistoryEntry] {
        &self.undo
    }

    /// Redo steps, the next one to redo is last.
    pub fn redo_entries(&self) -> &[HistoryEntry] {
        &self.redo
    }

    /// Label of the step [`crate::Session::undo`] would revert.
    pub fn undo_label(&self) -> Option<&str> {
        self.undo.last().map(|e| e.label.as_str())
    }

    /// Label of the step [`crate::Session::redo`] would repeat.
    pub fn redo_label(&self) -> Option<&str> {
        self.redo.last().map(|e| e.label.as_str())
    }

    /// True while a group is open.
    pub fn group_open(&self) -> bool {
        self.open.is_some()
    }

    pub(crate) fn record(&mut self, forward: Command, inverse: Command) {
        self.redo.clear();
        if let Some(group) = self.open.as_mut() {
            group.forward.push(forward);
            group.inverse.push(inverse);
        } else {
            let label = forward.label().to_owned();
            self.push(HistoryEntry {
                label,
                forward: vec![forward],
                inverse: vec![inverse],
            });
        }
    }

    pub(crate) fn push(&mut self, entry: HistoryEntry) {
        self.undo.push(entry);
        if let Some(limit) = self.limit
            && self.undo.len() > limit
        {
            let excess = self.undo.len() - limit;
            self.undo.drain(..excess);
        }
    }
}
