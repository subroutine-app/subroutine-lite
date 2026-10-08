use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyncDirection {
    Incoming,
    Outgoing,
    Status,
}

#[derive(Clone, Debug)]
pub struct SyncActivity {
    pub id: u64,
    pub at: DateTime<Utc>,
    pub direction: SyncDirection,
    pub summary: String,
}

#[derive(Default)]
pub(super) struct SyncActivityLog {
    entries: Vec<SyncActivity>,
    next_id: u64,
}

impl SyncActivityLog {
    const LIMIT: usize = 300;

    fn push(&mut self, direction: SyncDirection, summary: impl Into<String>) {
        if self.entries.len() == Self::LIMIT {
            self.entries.remove(0);
        }
        self.next_id += 1;
        self.entries.push(SyncActivity {
            id: self.next_id,
            at: Utc::now(),
            direction,
            summary: summary.into(),
        });
    }

    fn clear(&mut self) {
        self.entries.clear();
    }
}

fn resource_counts(counts: &[(&str, usize)]) -> String {
    let present = counts
        .iter()
        .filter(|(_, count)| *count > 0)
        .map(|(kind, count)| format!("{count} {kind}"))
        .collect::<Vec<_>>();
    if present.is_empty() {
        "no resource rows".into()
    } else {
        present.join(", ")
    }
}

fn snapshot_summary(data: &subroutine_core::AllData) -> String {
    let counts = resource_counts(&[
        ("actions", data.actions.len()),
        ("events", data.events.len()),
        ("routines", data.routines.len()),
        ("markers", data.markers.len()),
        ("signals", data.signals.len()),
        ("action templates", data.action_templates.len()),
        ("event templates", data.event_templates.len()),
        ("marker templates", data.marker_templates.len()),
        ("signal templates", data.signal_templates.len()),
    ]);
    format!(
        "Installed server snapshot at sequence {}: {counts}",
        data.seq
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct UndoTransaction(Uuid);

impl UndoTransaction {
    fn new() -> Self {
        Self(Uuid::now_v7())
    }
}

#[derive(Clone)]
pub(super) struct HistoryEntry<T> {
    transaction: UndoTransaction,
    value: T,
}

pub(super) struct UndoHistory<T> {
    undo: Vec<HistoryEntry<T>>,
    redo: Vec<HistoryEntry<T>>,
}

impl<T> Default for UndoHistory<T> {
    fn default() -> Self {
        Self {
            undo: Vec::new(),
            redo: Vec::new(),
        }
    }
}

impl<T> UndoHistory<T> {
    pub(super) fn record(&mut self, value: T) -> UndoTransaction {
        let transaction = UndoTransaction::new();
        self.redo.clear();
        self.undo.push(HistoryEntry { transaction, value });
        transaction
    }

    pub(super) fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub(super) fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub(super) fn next_undo_is(&self, transaction: UndoTransaction) -> bool {
        self.undo
            .last()
            .is_some_and(|entry| entry.transaction == transaction)
    }

    pub(super) fn pop_undo(&mut self) -> Option<HistoryEntry<T>> {
        self.undo.pop()
    }

    pub(super) fn pop_redo(&mut self) -> Option<HistoryEntry<T>> {
        self.redo.pop()
    }

    pub(super) fn push_undo(&mut self, entry: HistoryEntry<T>) {
        self.undo.push(entry);
    }

    pub(super) fn push_redo(&mut self, entry: HistoryEntry<T>) {
        self.redo.push(entry);
    }

    pub(super) fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }
}

mod http_store;
mod local_store;

pub use http_store::*;
