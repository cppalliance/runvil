//! A list's records held aside while a compactor edits the list in place,
//! and the `compacting` flag set around that compactor. A snapshot shares
//! each record with the list and with every view read from it, so taking
//! one copies no record, and putting it back restores the list's records
//! exactly. Neither touches the list's last send or its rendered tool
//! list.

use std::sync::Arc;

use super::MessageList;
use crate::protocol::MessageRecord;

/// The records a compaction started from.
#[derive(Debug)]
pub(crate) struct Snapshot(Vec<Arc<MessageRecord>>);

impl Snapshot {
    /// A copy of the records in order, as [`MessageList::to_records`]
    /// copies a list's.
    pub(crate) fn to_records(&self) -> Vec<MessageRecord> {
        self.0
            .iter()
            .map(|record| MessageRecord::clone(record))
            .collect()
    }
}

impl MessageList {
    /// The list's records as they stand.
    pub(crate) fn snapshot(&self) -> Snapshot {
        Snapshot(self.state().records.clone())
    }

    /// Puts the records back; the send state and the rendered tool list stay.
    pub(crate) fn restore(&self, snapshot: Snapshot) {
        self.state().records = snapshot.0;
    }

    /// Removes every record; the send state and the rendered tool list stay.
    pub(crate) fn clear(&self) {
        self.state().records.clear();
    }

    /// Whether a compactor is running over the list.
    pub(crate) fn compacting(&self) -> bool {
        self.state().compacting
    }

    /// Sets or clears the flag [`MessageList::compacting`] reads.
    pub(crate) fn set_compacting(&self, compacting: bool) {
        self.state().compacting = compacting;
    }
}
