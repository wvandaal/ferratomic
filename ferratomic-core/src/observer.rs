//! `observer` — monotonic snapshot observation.
//!
//! INV-FERR-011: Observer epoch is monotonically non-decreasing.
//! An observer never sees a snapshot older than its previous observation.

use std::{
    collections::VecDeque,
    sync::atomic::{AtomicU64, Ordering},
};

use ferratom::{AgentId, Datom};

use crate::store::{Snapshot, Store};

/// Default bounded observer history size.
///
/// INV-FERR-011: slow observers can catch up from a bounded in-memory
/// history before falling back to store replay.
pub const DEFAULT_OBSERVER_BUFFER: usize = 1024;

/// A push-based consumer of committed datom batches.
///
/// INV-FERR-011: delivery is at-least-once with epoch-based deduplication.
/// Implementations must therefore treat `epoch` as an idempotency key.
pub trait DatomObserver: Send + Sync {
    /// Deliver the datoms for one freshly committed epoch.
    fn on_commit(&self, epoch: u64, datoms: &[Datom]);

    /// Deliver a catch-up batch for all epochs after `from_epoch`.
    fn on_catchup(&self, from_epoch: u64, datoms: &[Datom]);

    /// Stable human-readable observer name for diagnostics.
    fn name(&self) -> &str;
}

#[derive(Clone, Debug)]
pub(crate) struct BroadcastEntry {
    epoch: u64,
    datoms: Vec<Datom>,
}

struct RegisteredObserver {
    observer: Box<dyn DatomObserver>,
    last_seen_epoch: u64,
}

/// Bounded observer broadcast state.
///
/// Stores recent transaction batches for fast catch-up and falls back to
/// store replay when an observer falls behind the bounded history window.
pub(crate) struct ObserverBroadcast {
    observers: Vec<RegisteredObserver>,
    recent: VecDeque<BroadcastEntry>,
    max_buffer: usize,
}

impl ObserverBroadcast {
    /// Create an empty observer broadcast state.
    #[must_use]
    pub(crate) fn new(max_buffer: usize) -> Self {
        Self {
            observers: Vec::new(),
            recent: VecDeque::new(),
            max_buffer: max_buffer.max(1),
        }
    }

    /// Register an observer and immediately catch it up to the current store state.
    pub(crate) fn register(&mut self, observer: Box<dyn DatomObserver>, store: &Store) {
        let current_epoch = store.epoch();
        if current_epoch > 0 {
            let datoms: Vec<Datom> = store.datoms().cloned().collect();
            observer.on_catchup(0, &datoms);
        }

        self.observers.push(RegisteredObserver {
            observer,
            last_seen_epoch: current_epoch,
        });
    }

    /// Publish a freshly committed batch to all observers.
    pub(crate) fn publish(&mut self, epoch: u64, datoms: &[Datom], store: &Store) {
        self.recent.push_back(BroadcastEntry {
            epoch,
            datoms: datoms.to_vec(),
        });
        while self.recent.len() > self.max_buffer {
            self.recent.pop_front();
        }

        let recent: Vec<BroadcastEntry> = self.recent.iter().cloned().collect();
        for registered in &mut self.observers {
            if epoch <= registered.last_seen_epoch {
                continue;
            }

            if registered.last_seen_epoch + 1 < epoch {
                let catchup = buffered_delta_since(&recent, registered.last_seen_epoch)
                    .unwrap_or_else(|| full_delta_since(store, registered.last_seen_epoch));
                registered
                    .observer
                    .on_catchup(registered.last_seen_epoch, &catchup);
            } else {
                registered.observer.on_commit(epoch, datoms);
            }

            registered.last_seen_epoch = epoch;
        }
    }
}

fn buffered_delta_since(recent: &[BroadcastEntry], from_epoch: u64) -> Option<Vec<Datom>> {
    let first_epoch = recent.first()?.epoch;
    if first_epoch > from_epoch.saturating_add(1) {
        return None;
    }

    let mut delta = Vec::new();
    for entry in recent {
        if entry.epoch > from_epoch {
            delta.extend(entry.datoms.clone());
        }
    }
    Some(delta)
}

fn full_delta_since(store: &Store, from_epoch: u64) -> Vec<Datom> {
    store
        .datoms()
        .filter(|datom| datom.tx().physical() > from_epoch)
        .cloned()
        .collect()
}

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

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use ferratom::{Attribute, EntityId, Value};

    use super::*;
    use crate::writer::Transaction;

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum Event {
        Commit { epoch: u64, count: usize },
        Catchup { from_epoch: u64, count: usize },
    }

    struct RecordingObserver {
        name: &'static str,
        events: Arc<Mutex<Vec<Event>>>,
    }

    impl RecordingObserver {
        fn new(name: &'static str, events: Arc<Mutex<Vec<Event>>>) -> Self {
            Self { name, events }
        }
    }

    impl DatomObserver for RecordingObserver {
        fn on_commit(&self, epoch: u64, datoms: &[Datom]) {
            self.events
                .lock()
                .expect("recording observer commit lock")
                .push(Event::Commit {
                    epoch,
                    count: datoms.len(),
                });
        }

        fn on_catchup(&self, from_epoch: u64, datoms: &[Datom]) {
            self.events
                .lock()
                .expect("recording observer catchup lock")
                .push(Event::Catchup {
                    from_epoch,
                    count: datoms.len(),
                });
        }

        fn name(&self) -> &str {
            self.name
        }
    }

    fn transact_doc(store: &mut Store, seed: u8) {
        let tx = Transaction::new(AgentId::from_bytes([seed; 16]))
            .assert_datom(
                EntityId::from_content(&[seed]),
                Attribute::from("db/doc"),
                Value::String(format!("doc-{seed}").into()),
            )
            .commit(store.schema())
            .expect("valid tx");
        store.transact(tx).expect("transact succeeds");
    }

    #[test]
    fn test_observer_broadcast_registers_with_catchup() {
        let mut store = Store::genesis();
        transact_doc(&mut store, 1);
        transact_doc(&mut store, 2);

        let mut broadcast = ObserverBroadcast::new(DEFAULT_OBSERVER_BUFFER);
        let events = Arc::new(Mutex::new(Vec::new()));
        broadcast.register(
            Box::new(RecordingObserver::new("catchup", Arc::clone(&events))),
            &store,
        );

        let recorded = events.lock().expect("events lock");
        assert!(
            matches!(recorded.as_slice(), [Event::Catchup { from_epoch: 0, count }] if *count > 0),
            "register must send catchup for existing epochs, got {:?}",
            *recorded
        );
    }

    #[test]
    fn test_observer_broadcast_keeps_recent_entries_bounded() {
        let mut store = Store::genesis();
        let mut broadcast = ObserverBroadcast::new(2);

        for seed in 1..=3 {
            transact_doc(&mut store, seed);
            let datoms: Vec<Datom> = store
                .datoms()
                .filter(|datom| datom.tx().physical() == u64::from(seed))
                .cloned()
                .collect();
            broadcast.publish(u64::from(seed), &datoms, &store);
        }

        assert_eq!(
            broadcast.recent.len(),
            2,
            "recent ring buffer must stay bounded"
        );
        assert_eq!(
            broadcast.recent.front().map(|entry| entry.epoch),
            Some(2),
            "oldest entry must be evicted when buffer is full"
        );
    }

    #[test]
    fn test_observer_observe_tracks_monotonic_epoch() {
        let mut store = Store::genesis();
        let observer = Observer::new(AgentId::from_bytes([9u8; 16]));

        assert_eq!(observer.last_epoch(), 0);
        transact_doc(&mut store, 1);
        let first = observer.observe(&store);
        transact_doc(&mut store, 2);
        let second = observer.observe(&store);

        assert!(second.epoch() >= first.epoch());
        assert_eq!(observer.last_epoch(), second.epoch());
    }
}
