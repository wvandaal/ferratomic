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
pub(crate) const DEFAULT_OBSERVER_BUFFER: usize = 1024;

/// A push-based consumer that receives notifications when datom batches
/// are committed to the store.
///
/// INV-FERR-011: observer monotonicity — for any observer `alpha`,
/// `forall i, j where i < j: epoch_seq(alpha)[i] <= epoch_seq(alpha)[j]`.
/// The broadcast infrastructure delivers epochs in non-decreasing order.
/// Delivery is at-least-once: the same epoch may be delivered more than
/// once (e.g., after a catch-up replay), so implementations treat `epoch`
/// as an idempotency key and tolerate duplicate delivery.
///
/// The trait requires `Send + Sync` because the `ObserverBroadcast`
/// infrastructure invokes observer methods from the committing thread,
/// which may differ from the thread that registered the observer.
///
/// # Contract
///
/// Implementors guarantee:
/// - **Idempotency**: receiving the same epoch twice is a no-op.
///   `on_commit` and `on_catchup` use `epoch` as a deduplication key.
/// - **Thread safety**: methods may be called from any thread; internal
///   state must be `Send + Sync`-safe.
/// - **No panics**: observer callbacks must not panic. A panicking
///   observer would unwind through the broadcast infrastructure and
///   prevent delivery to other registered observers.
pub trait DatomObserver: Send + Sync {
    /// Receive the datoms for a single freshly committed epoch.
    ///
    /// INV-FERR-011: `epoch` is monotonically non-decreasing across
    /// successive calls on the same observer. The broadcast layer skips
    /// delivery when `epoch <= last_seen_epoch` for this observer,
    /// ensuring the monotonicity invariant. Implementations use `epoch`
    /// as an idempotency key — if a datom batch for epoch `e` has
    /// already been processed, the call is a no-op.
    fn on_commit(&self, epoch: u64, datoms: &[Datom]);

    /// Receive a catch-up batch containing datoms from all epochs after
    /// `from_epoch` up to the current store state.
    ///
    /// INV-FERR-011: called when an observer has fallen behind (either
    /// at registration time or when the gap between `last_seen_epoch`
    /// and the current epoch exceeds 1). The batch may originate from
    /// the bounded in-memory history or, if the observer has fallen
    /// beyond the buffer window, from a full store replay. Delivery
    /// is at-least-once — `datoms` may include datoms the observer
    /// has already seen. Implementors must be idempotent: receiving
    /// previously-processed datoms in a catchup batch is a no-op.
    fn on_catchup(&self, from_epoch: u64, datoms: &[Datom]);

    /// Return a stable, human-readable name for this observer.
    ///
    /// INV-FERR-011: the name is fixed at construction time and used
    /// for diagnostic logging. It does not affect delivery semantics.
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

        for registered in &mut self.observers {
            if epoch <= registered.last_seen_epoch {
                continue;
            }

            if registered.last_seen_epoch + 1 < epoch {
                let catchup = buffered_delta_since(&self.recent, registered.last_seen_epoch)
                    .unwrap_or_else(|| full_store_catchup(store));
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

fn buffered_delta_since(recent: &VecDeque<BroadcastEntry>, from_epoch: u64) -> Option<Vec<Datom>> {
    let first_epoch = recent.front()?.epoch;
    if first_epoch > from_epoch.saturating_add(1) {
        return None;
    }

    let mut delta = Vec::new();
    for entry in recent {
        if entry.epoch > from_epoch {
            delta.extend_from_slice(&entry.datoms);
        }
    }
    Some(delta)
}

/// Full-store catchup: return ALL datoms when the bounded buffer is exhausted.
///
/// INV-FERR-011: This is the fallback path when an observer has fallen so far
/// behind that the bounded `recent` buffer no longer covers the gap. The
/// observer receives the entire store state and resets its baseline.
///
/// HI-012: The previous implementation filtered by `datom.tx().physical() >
/// from_epoch`, which compared wall-clock milliseconds (from HLC) against an
/// epoch counter — always true for any real timestamp. With HLC wired in,
/// individual datoms do not carry epoch metadata (they carry `TxId` with HLC
/// physical time). Epoch-based delta filtering requires an epoch-indexed
/// structure (Phase 4b). For Phase 4a, full-store catchup is correct: the
/// observer contract is at-least-once delivery, and `on_catchup` semantics
/// handle receiving previously-seen datoms.
fn full_store_catchup(store: &Store) -> Vec<Datom> {
    store.datoms().cloned().collect()
}

/// An observer that tracks the latest epoch it has seen.
///
/// INV-FERR-011: `observe()` returns a snapshot at an epoch >= the
/// last observed epoch. Epochs never regress. This is enforced by
/// `AtomicU64::fetch_max` on every observation.
///
/// `Observer` is `Send + Sync` by construction: `AgentId` is `Copy`
/// and `AtomicU64` is the standard thread-safe counter.
///
/// # Visibility
///
/// `pub` because verification tests in `ferratomic-verify` exercise
/// observer monotonicity properties (INV-FERR-011 conformance testing).
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

        // INV-FERR-011: the store's epoch should never be less than what
        // this observer has already seen. A lower epoch indicates a bug
        // in the store or database layer (e.g., snapshot regression).
        // debug_assert catches this in test/debug builds without panicking
        // in production, where fetch_max safely ignores the lower value.
        debug_assert!(
            current_epoch >= self.last_epoch.load(Ordering::Acquire),
            "INV-FERR-011: store epoch ({}) < observer last_epoch ({}): \
             monotonicity violation — store snapshot regressed",
            current_epoch,
            self.last_epoch.load(Ordering::Acquire),
        );

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
        store.transact_test(tx).expect("transact succeeds");
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
