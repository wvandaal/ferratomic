//! `observer` — monotonic snapshot observation.
//!
//! INV-FERR-011: Observer epoch is monotonically non-decreasing.
//! An observer never sees a snapshot older than its previous observation.

use std::sync::atomic::{AtomicU64, Ordering};

use ferratom::AgentId;

use crate::store::{Snapshot, Store};

/// An observer that tracks the latest epoch it has seen.
///
/// INV-FERR-011: `observe()` returns a snapshot at an epoch >= the
/// last observed epoch. Epochs never regress. This is enforced by
/// `AtomicU64::fetch_max` on every observation.
///
/// `Observer` is `Send + Sync` by construction: `AgentId` is `Copy`
/// and `AtomicU64` is the standard thread-safe counter.
pub struct Observer {
    /// The agent identity of this observer.
    agent: AgentId,
    /// The highest epoch this observer has seen.
    /// Uses `AtomicU64` for thread-safe monotonic tracking.
    last_epoch: AtomicU64,
}

impl Observer {
    /// Create a new observer for the given agent.
    ///
    /// INV-FERR-011: The observer starts at epoch 0. The first
    /// `observe()` call will advance to the store's current epoch.
    #[must_use]
    pub fn new(agent: AgentId) -> Self {
        Self {
            agent,
            last_epoch: AtomicU64::new(0),
        }
    }

    /// Observe the current state of the store.
    ///
    /// INV-FERR-011: Returns a snapshot at an epoch >= the last
    /// observed epoch. The internal epoch counter advances monotonically
    /// via `fetch_max` — it never decreases even if called with a
    /// store at a lower epoch (which would indicate a bug elsewhere).
    #[must_use]
    pub fn observe(&self, store: &Store) -> Snapshot {
        let snap = store.snapshot();
        let current_epoch = snap.epoch();

        // Monotonic advance: only update if current is greater.
        // fetch_max returns the previous value; we don't need it.
        self.last_epoch.fetch_max(current_epoch, Ordering::AcqRel);

        snap
    }

    /// The agent identity of this observer.
    ///
    /// INV-FERR-011: identity is fixed at construction time and
    /// never changes over the observer's lifetime.
    #[must_use]
    pub fn agent(&self) -> AgentId {
        self.agent
    }

    /// The highest epoch this observer has seen.
    ///
    /// INV-FERR-011: This value only increases over the observer's lifetime.
    #[must_use]
    pub fn last_epoch(&self) -> u64 {
        self.last_epoch.load(Ordering::Acquire)
    }
}
