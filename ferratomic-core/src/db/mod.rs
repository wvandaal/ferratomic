//! `db` -- MVCC database with lock-free reads and serialized writes.
//!
//! INV-FERR-006: Snapshot isolation via `ArcSwap`. Readers call
//! `db.snapshot()` which performs an atomic pointer load (~1ns, no lock).
//! INV-FERR-007: Write linearizability via single-threaded writer.
//! All writes go through `db.transact()` which holds a mutex for
//! the duration of the transaction application.
//!
//! ## Typestate
//!
//! `Database<Opening>` -- initialization in progress (conceptual).
//! `Database<Ready>` -- fully initialized; reads and writes available.
//!
//! All constructors return `Database<Ready>` directly. The default type
//! parameter is `Ready`, so bare `Database` equals `Database<Ready>`.
//! Methods `transact()`, `snapshot()`, `epoch()`, `schema()`, and
//! `register_observer()` are only available on `Database<Ready>`.
//!
//! ## Architecture (ADR-FERR-003)
//!
//! ```text
//! Readers --> ArcSwap::load() --> Arc<Store>  (lock-free, O(1))
//! Writer  --> Mutex::lock()   --> mutate Store --> ArcSwap::store()
//! ```
//!
//! ## Submodules
//!
//! - `transact`: transaction application (`Database::transact`).
//! - `recover`: WAL and checkpoint recovery constructors.

mod recover;
mod transact;

use std::{
    marker::PhantomData,
    sync::{
        atomic::AtomicU64,
        Mutex,
    },
};

use arc_swap::ArcSwap;
use ferratom::{FerraError, Schema};

use crate::{
    observer::{DatomObserver, ObserverBroadcast, DEFAULT_OBSERVER_BUFFER},
    store::{Snapshot, Store},
};

// ---------------------------------------------------------------------------
// Typestate markers
// ---------------------------------------------------------------------------

/// Marker: database is being initialized (genesis, WAL recovery, checkpoint
/// recovery). Not yet available for reads or writes.
///
/// This is a conceptual marker for the state space. In the current design all
/// constructors complete initialization in one step and return `Database<Ready>`
/// directly, so user code never holds a `Database<Opening>`. The marker exists
/// to make the two lifecycle phases explicit in the type system and to
/// support future phased-initialization flows (e.g., async recovery).
#[derive(Debug)]
#[allow(dead_code)] // Typestate marker — exists for the type system, not for construction.
pub(crate) struct Opening;

/// Marker: database initialization is complete. Reads (`snapshot`, `schema`,
/// `epoch`) and writes (`transact`, `register_observer`) are available.
///
/// All public constructors (`genesis`, `recover`, `from_store`, etc.) return
/// `Database<Ready>`.
#[derive(Debug)]
pub struct Ready;

// ---------------------------------------------------------------------------
// Database
// ---------------------------------------------------------------------------

/// MVCC database providing lock-free snapshot reads and serialized writes.
///
/// # Typestate
///
/// `Database<Opening>` -- initialization in progress (genesis, recovery).
/// `Database<Ready>` -- fully initialized; reads and writes are available.
///
/// All public constructors return `Database<Ready>` directly. The `Opening`
/// state exists in the type system for future phased-initialization flows
/// and to document the two lifecycle phases explicitly.
///
/// The default type parameter is `Ready`, so bare `Database` (without an
/// explicit parameter) is `Database<Ready>`. Existing code that does not
/// name the parameter continues to compile unchanged.
///
/// INV-FERR-006: snapshot isolation. Every `snapshot()` call returns an
/// immutable view that is never affected by concurrent or subsequent writes.
///
/// INV-FERR-007: write linearizability. The internal `Mutex` ensures that
/// exactly one writer operates at a time, producing strictly ordered epochs.
///
/// INV-FERR-008: when a WAL is attached, `transact()` writes and fsyncs the
/// WAL BEFORE advancing the epoch and swapping the pointer.
pub struct Database<S = Ready> {
    /// The current store state. Readers load atomically. Writers swap after mutation.
    /// `ArcSwap` provides wait-free reads (~1ns) per ADR-FERR-003.
    current: ArcSwap<Store>,

    /// Write serialization lock. Only one writer at a time.
    /// INV-FERR-007: ensures epoch ordering is strict.
    write_lock: Mutex<()>,

    /// Optional WAL for durability. When `Some`, `transact()` writes and
    /// fsyncs the WAL before applying the transaction to the store.
    /// INV-FERR-008: `durable(WAL(T)) BEFORE visible(SNAP(e))`.
    wal: Mutex<Option<crate::wal::Wal>>,

    /// Registered observers plus bounded history for catch-up.
    /// INV-FERR-011: delivery is at-least-once with bounded replay.
    observers: Mutex<ObserverBroadcast>,

    /// INV-FERR-021: Concurrency limiter for write backpressure.
    /// Pre-checks concurrent write attempts before `try_lock()` to
    /// prevent thundering herd on the write Mutex.
    write_limiter: crate::backpressure::WriteLimiter,

    /// Monotonic transaction counter for release-mode bijection canary.
    /// Incremented after every successful `transact()`. When the
    /// `release_bijection_check` feature is enabled, every 100th
    /// transaction triggers `SecondaryIndexes::verify_bijection()`.
    transaction_count: AtomicU64,

    /// Typestate marker. Zero-size, erased at compile time.
    _state: PhantomData<S>,
}

// ---------------------------------------------------------------------------
// In-memory constructors -- no WAL
// ---------------------------------------------------------------------------

impl Database<Ready> {
    /// Create a new database from a genesis store (no WAL).
    ///
    /// INV-FERR-031: The initial store is deterministic -- identical on
    /// every call. The genesis store contains the 19 axiomatic meta-schema
    /// attributes and no datoms.
    ///
    /// Without a WAL, transactions are not durable across crashes. Use
    /// [`genesis_with_wal`](Self::genesis_with_wal) for durability.
    #[must_use]
    pub fn genesis() -> Self {
        Self {
            current: ArcSwap::from_pointee(Store::genesis()),
            write_lock: Mutex::new(()),
            wal: Mutex::new(None),
            observers: Mutex::new(ObserverBroadcast::new(DEFAULT_OBSERVER_BUFFER)),
            write_limiter: crate::backpressure::WriteLimiter::new(&crate::backpressure::BackpressurePolicy::default()),
            transaction_count: AtomicU64::new(0),
            _state: PhantomData,
        }
    }

    /// Create a database from an existing store (no WAL).
    ///
    /// INV-FERR-006: the provided store becomes the initial snapshot state.
    /// INV-FERR-007: epoch ordering continues from the store's current epoch.
    #[must_use]
    pub fn from_store(store: Store) -> Self {
        Self {
            current: ArcSwap::from_pointee(store),
            write_lock: Mutex::new(()),
            wal: Mutex::new(None),
            observers: Mutex::new(ObserverBroadcast::new(DEFAULT_OBSERVER_BUFFER)),
            write_limiter: crate::backpressure::WriteLimiter::new(&crate::backpressure::BackpressurePolicy::default()),
            transaction_count: AtomicU64::new(0),
            _state: PhantomData,
        }
    }
}

// ---------------------------------------------------------------------------
// Operational API -- only available on Database<Ready>
// ---------------------------------------------------------------------------

impl Database<Ready> {
    /// Register a push-based datom observer.
    ///
    /// INV-FERR-011: the observer is caught up to the current store state
    /// before future commit notifications are delivered.
    ///
    /// # Errors
    ///
    /// Returns `FerraError::InvariantViolation` if the observer registry
    /// mutex is poisoned.
    pub fn register_observer(&self, observer: Box<dyn DatomObserver>) -> Result<(), FerraError> {
        let current = self.current.load();
        let mut observers = self
            .observers
            .lock()
            .map_err(|_| FerraError::InvariantViolation {
                invariant: "INV-FERR-011".to_string(),
                details: "observer registry mutex poisoned during register".to_string(),
            })?;
        observers.register(observer, current.as_ref());
        Ok(())
    }

    /// Take an immutable point-in-time snapshot.
    ///
    /// INV-FERR-006: The returned snapshot is frozen at the moment of the
    /// atomic pointer load. This call is lock-free (~1ns via `ArcSwap`).
    /// Multiple readers can hold different snapshots simultaneously
    /// without contention.
    #[must_use]
    pub fn snapshot(&self) -> Snapshot {
        let store = self.current.load();
        store.snapshot()
    }

    /// Access the current store's schema for transaction building.
    ///
    /// INV-FERR-009: the returned schema governs transact-time validation.
    /// Returns a clone of the schema. The schema is small (tens of
    /// attributes) so cloning is cheap relative to the transaction
    /// validation it enables.
    #[must_use]
    pub fn schema(&self) -> Schema {
        let store = self.current.load();
        store.schema().clone()
    }

    /// Access the current epoch.
    ///
    /// INV-FERR-007: the epoch is strictly monotonically increasing.
    /// The value returned reflects the epoch at the time of the atomic
    /// pointer load and may be stale by the time the caller uses it.
    #[must_use]
    pub fn epoch(&self) -> u64 {
        let store = self.current.load();
        store.epoch()
    }
}

// Send + Sync safety: ArcSwap<T> is Send+Sync when T: Send+Sync.
// Store is Send+Sync (im::OrdSet is Send+Sync, Schema is Send+Sync).
// Mutex<()> is Send+Sync. PhantomData<S> is Send+Sync for any S.
// Therefore Database<S> is Send+Sync and can be shared across
// threads via Arc<Database>.

#[cfg(test)]
mod tests {
    use std::sync::{Arc as StdArc, Mutex as StdMutex};

    use ferratom::{AgentId, Attribute, EntityId, Value};

    use super::*;
    use crate::observer::DatomObserver;
    use crate::wal::Wal;
    use crate::writer::Transaction;

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum ObserverEvent {
        Commit { epoch: u64, count: usize },
        Catchup { from_epoch: u64, count: usize },
    }

    struct RecordingObserver {
        name: &'static str,
        events: StdArc<StdMutex<Vec<ObserverEvent>>>,
    }

    impl RecordingObserver {
        fn new(name: &'static str, events: StdArc<StdMutex<Vec<ObserverEvent>>>) -> Self {
            Self { name, events }
        }
    }

    impl DatomObserver for RecordingObserver {
        fn on_commit(&self, epoch: u64, datoms: &[ferratom::Datom]) {
            self.events
                .lock()
                .expect("observer commit events lock")
                .push(ObserverEvent::Commit {
                    epoch,
                    count: datoms.len(),
                });
        }

        fn on_catchup(&self, from_epoch: u64, datoms: &[ferratom::Datom]) {
            self.events
                .lock()
                .expect("observer catchup events lock")
                .push(ObserverEvent::Catchup {
                    from_epoch,
                    count: datoms.len(),
                });
        }

        fn name(&self) -> &str {
            self.name
        }
    }

    /// INV-FERR-031: genesis produces a deterministic database.
    #[test]
    fn test_inv_ferr_031_genesis_determinism() {
        let db1 = Database::genesis();
        let db2 = Database::genesis();
        assert_eq!(
            db1.epoch(),
            db2.epoch(),
            "INV-FERR-031: genesis databases must have identical epochs"
        );
        let s1 = db1.snapshot();
        let s2 = db2.snapshot();
        assert_eq!(
            s1.epoch(),
            s2.epoch(),
            "INV-FERR-031: genesis snapshots must have identical epochs"
        );
    }

    /// INV-FERR-006: snapshot isolation -- a snapshot taken before a write
    /// does not see the write's effects.
    #[test]
    fn test_inv_ferr_006_snapshot_isolation() {
        let db = Database::genesis();
        let before = db.snapshot();

        let agent = AgentId::from_bytes([1u8; 16]);
        let schema = db.schema();
        let tx = Transaction::new(agent)
            .assert_datom(
                EntityId::from_content(b"e1"),
                Attribute::from("db/doc"),
                Value::String("hello".into()),
            )
            .commit(&schema);

        match tx {
            Ok(committed) => {
                let result = db.transact(committed);
                assert!(
                    result.is_ok(),
                    "INV-FERR-007: transact on genesis db must succeed"
                );

                let after = db.snapshot();

                // Before-snapshot must not see the new datom.
                assert_eq!(
                    before.epoch(),
                    0,
                    "INV-FERR-006: pre-write snapshot epoch must be 0"
                );
                // After-snapshot must see epoch advance.
                assert_eq!(
                    after.epoch(),
                    1,
                    "INV-FERR-007: post-write snapshot epoch must be 1"
                );
            }
            Err(e) => panic!("Transaction commit failed unexpectedly: {e}"),
        }
    }

    /// INV-FERR-007: epoch strictly increases with each transact.
    #[test]
    fn test_inv_ferr_007_epoch_monotonicity() {
        let db = Database::genesis();
        assert_eq!(db.epoch(), 0, "INV-FERR-031: genesis epoch is 0");

        let agent = AgentId::from_bytes([2u8; 16]);
        let schema = db.schema();

        for i in 1u64..=3 {
            let tx = Transaction::new(agent)
                .assert_datom(
                    EntityId::from_content(format!("e{i}").as_bytes()),
                    Attribute::from("db/doc"),
                    Value::String(format!("doc-{i}").into()),
                )
                .commit(&schema);

            match tx {
                Ok(committed) => {
                    let receipt = db.transact(committed);
                    match receipt {
                        Ok(r) => assert_eq!(
                            r.epoch(),
                            i,
                            "INV-FERR-007: epoch must equal {i} after transaction {i}"
                        ),
                        Err(e) => panic!("transact failed on iteration {i}: {e}"),
                    }
                }
                Err(e) => panic!("commit failed on iteration {i}: {e}"),
            }
        }

        assert_eq!(
            db.epoch(),
            3,
            "INV-FERR-007: final epoch must be 3 after 3 transactions"
        );
    }

    /// INV-FERR-006: from_store preserves the store's state.
    #[test]
    fn test_inv_ferr_006_from_store() {
        let store = Store::genesis();
        let epoch = store.epoch();
        let db = Database::from_store(store);
        assert_eq!(
            db.epoch(),
            epoch,
            "INV-FERR-006: from_store must preserve epoch"
        );
    }

    #[test]
    fn test_inv_ferr_011_register_observer_delivers_catchup() {
        let db = Database::genesis();
        let agent = AgentId::from_bytes([7u8; 16]);
        let schema = db.schema();

        for i in 0..2i64 {
            let tx = Transaction::new(agent)
                .assert_datom(
                    EntityId::from_content(format!("catchup-{i}").as_bytes()),
                    Attribute::from("db/doc"),
                    Value::String(format!("doc-{i}").into()),
                )
                .commit(&schema)
                .expect("valid tx");
            db.transact(tx).expect("transact succeeds");
        }

        let events = StdArc::new(StdMutex::new(Vec::new()));
        let observer = Box::new(RecordingObserver::new("catchup", StdArc::clone(&events)));
        db.register_observer(observer)
            .expect("observer registration succeeds");

        let recorded = events.lock().expect("events lock");
        assert!(
            matches!(recorded.as_slice(), [ObserverEvent::Catchup { from_epoch: 0, count }] if *count > 0),
            "register_observer must catch up existing state, got {:?}",
            *recorded
        );
    }

    #[test]
    fn test_inv_ferr_011_transact_notifies_registered_observer() {
        let db = Database::genesis();
        let events = StdArc::new(StdMutex::new(Vec::new()));
        let observer = Box::new(RecordingObserver::new("commit", StdArc::clone(&events)));
        db.register_observer(observer)
            .expect("observer registration succeeds");

        let schema = db.schema();
        let tx = Transaction::new(AgentId::from_bytes([8u8; 16]))
            .assert_datom(
                EntityId::from_content(b"observer-commit"),
                Attribute::from("db/doc"),
                Value::String("observed".into()),
            )
            .commit(&schema)
            .expect("valid tx");
        db.transact(tx).expect("transact succeeds");

        let recorded = events.lock().expect("events lock");
        assert!(
            recorded.iter().any(|event| {
                matches!(event, ObserverEvent::Commit { epoch: 1, count } if *count > 0)
            }),
            "registered observer must receive commit notification, got {:?}",
            *recorded
        );
    }

    // -- Regression tests for cleanroom review defects -------------------------

    /// Regression: bd-2w9 -- Database with WAL writes WAL before epoch advance.
    #[test]
    fn test_bug_bd_2w9_wal_written_on_transact() {
        let dir = tempfile::TempDir::new().unwrap();
        let wal_path = dir.path().join("test.wal");

        let db = Database::genesis_with_wal(&wal_path).unwrap();
        let agent = AgentId::from_bytes([1u8; 16]);
        let schema = db.schema();

        let tx = Transaction::new(agent)
            .assert_datom(
                EntityId::from_content(b"e1"),
                Attribute::from("db/doc"),
                Value::String("hello from wal".into()),
            )
            .commit(&schema)
            .expect("valid tx");

        db.transact(tx).expect("transact should succeed");

        // Verify WAL was written: open and recover
        let mut wal = Wal::open(&wal_path).expect("WAL must exist");
        let entries = wal.recover().expect("recovery must succeed");
        assert_eq!(
            entries.len(),
            1,
            "bd-2w9: WAL must contain 1 entry after 1 transact"
        );
        assert_eq!(entries[0].epoch, 1, "bd-2w9: WAL entry epoch must be 1");
    }

    /// Regression: bd-2w9 -- recover_from_wal restores state.
    #[test]
    fn test_bug_bd_2w9_recover_from_wal() {
        let dir = tempfile::TempDir::new().unwrap();
        let wal_path = dir.path().join("test.wal");

        // Write via WAL-backed database
        {
            let db = Database::genesis_with_wal(&wal_path).unwrap();
            let agent = AgentId::from_bytes([1u8; 16]);
            let schema = db.schema();

            for i in 0..3i64 {
                let tx = Transaction::new(agent)
                    .assert_datom(
                        EntityId::from_content(format!("e{i}").as_bytes()),
                        Attribute::from("db/doc"),
                        Value::String(format!("doc-{i}").into()),
                    )
                    .commit(&schema)
                    .expect("valid tx");
                db.transact(tx).expect("transact ok");
            }
        }

        // Recover from WAL alone (simulating crash + restart)
        let recovered = Database::recover_from_wal(&wal_path).expect("recovery must succeed");
        let snap = recovered.snapshot();

        // Must have datoms from all 3 transactions (user datoms + tx metadata)
        assert!(
            snap.datoms().count() > 0,
            "bd-2w9: recovered database must have datoms"
        );
    }
}
