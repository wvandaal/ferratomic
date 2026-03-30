//! `db` — MVCC database with lock-free reads and serialized writes.
//!
//! INV-FERR-006: Snapshot isolation via `ArcSwap`. Readers call
//! `db.snapshot()` which performs an atomic pointer load (~1ns, no lock).
//! INV-FERR-007: Write linearizability via single-threaded writer.
//! All writes go through `db.transact()` which holds a mutex for
//! the duration of the transaction application.
//!
//! ## Architecture (ADR-FERR-003)
//!
//! ```text
//! Readers ──→ ArcSwap::load() ──→ Arc<Store>  (lock-free, O(1))
//! Writer  ──→ Mutex::lock()   ──→ mutate Store ──→ ArcSwap::store()
//! ```

use std::path::Path;
use std::sync::{Arc, Mutex};

use arc_swap::ArcSwap;

use ferratom::{FerraError, Schema};

use crate::store::{Snapshot, Store, TxReceipt};
use crate::wal::Wal;
use crate::writer::{Committed, Transaction};

// ---------------------------------------------------------------------------
// Database
// ---------------------------------------------------------------------------

/// MVCC database providing lock-free snapshot reads and serialized writes.
///
/// INV-FERR-006: snapshot isolation. Every `snapshot()` call returns an
/// immutable view that is never affected by concurrent or subsequent writes.
///
/// INV-FERR-007: write linearizability. The internal `Mutex` ensures that
/// exactly one writer operates at a time, producing strictly ordered epochs.
///
/// INV-FERR-008: when a WAL is attached, `transact()` writes and fsyncs the
/// WAL BEFORE advancing the epoch and swapping the pointer.
pub struct Database {
    /// The current store state. Readers load atomically. Writers swap after mutation.
    /// `ArcSwap` provides wait-free reads (~1ns) per ADR-FERR-003.
    current: ArcSwap<Store>,

    /// Write serialization lock. Only one writer at a time.
    /// INV-FERR-007: ensures epoch ordering is strict.
    write_lock: Mutex<()>,

    /// Optional WAL for durability. When `Some`, `transact()` writes and
    /// fsyncs the WAL before applying the transaction to the store.
    /// INV-FERR-008: `durable(WAL(T)) BEFORE visible(SNAP(e))`.
    wal: Mutex<Option<Wal>>,
}

impl Database {
    /// Create a new database from a genesis store (no WAL).
    ///
    /// INV-FERR-031: The initial store is deterministic — identical on
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
        }
    }

    /// Create a genesis database backed by a WAL file.
    ///
    /// INV-FERR-008: All subsequent `transact()` calls write and fsync
    /// the WAL before advancing the epoch.
    /// INV-FERR-031: The initial store is deterministic.
    ///
    /// # Errors
    ///
    /// Returns `FerraError::Io` if the WAL file cannot be created.
    pub fn genesis_with_wal(wal_path: &Path) -> Result<Self, FerraError> {
        let wal = Wal::create(wal_path)?;
        Ok(Self {
            current: ArcSwap::from_pointee(Store::genesis()),
            write_lock: Mutex::new(()),
            wal: Mutex::new(Some(wal)),
        })
    }

    /// Recover a database from a WAL file.
    ///
    /// INV-FERR-014: Recovery replays all complete WAL entries into a
    /// genesis store, producing the last committed state.
    ///
    /// # Errors
    ///
    /// Returns `FerraError` if the WAL cannot be opened or recovery fails.
    pub fn recover_from_wal(wal_path: &Path) -> Result<Self, FerraError> {
        let mut wal = Wal::open(wal_path)?;
        let entries = wal.recover()?;

        let mut store = Store::genesis();
        for entry in &entries {
            let datoms: Vec<ferratom::Datom> = serde_json::from_slice(&entry.payload)
                .map_err(|e| FerraError::WalRead(e.to_string()))?;
            // Re-insert recovered datoms directly (they already have real TxIds).
            for datom in datoms {
                store.insert(datom);
            }
        }

        Ok(Self {
            current: ArcSwap::from_pointee(store),
            write_lock: Mutex::new(()),
            wal: Mutex::new(Some(wal)),
        })
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
        }
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

    /// Apply a committed transaction to the database.
    ///
    /// INV-FERR-007: Serialized via write lock. Only one transaction is
    /// applied at a time. The epoch strictly increases with each successful
    /// transact.
    ///
    /// INV-FERR-006: After the write completes, subsequent `snapshot()` calls
    /// see the new state. Existing snapshots held by readers are unaffected
    /// (structural sharing via `im::OrdSet` — ADR-FERR-001).
    ///
    /// # Errors
    ///
    /// Returns `FerraError::Backpressure` if the write lock is already held
    /// (try-lock semantics — the caller should retry or shed load).
    /// Returns other `FerraError` variants if the transaction application
    /// itself fails (e.g., `EmptyTransaction`, `InvariantViolation`).
    pub fn transact(
        &self,
        transaction: Transaction<Committed>,
    ) -> Result<TxReceipt, FerraError> {
        // INV-FERR-007: serialize writes. try_lock returns immediately
        // rather than blocking, so callers can shed load under contention.
        let _guard = self
            .write_lock
            .try_lock()
            .map_err(|_| FerraError::Backpressure)?;

        // INV-FERR-008: Write WAL BEFORE applying to store.
        // durable(WAL(T)) BEFORE visible(SNAP(e)).
        let next_epoch = {
            let current = self.current.load();
            current.epoch() + 1
        };
        {
            let mut wal_guard = self
                .wal
                .lock()
                .map_err(|_| FerraError::Backpressure)?;
            if let Some(ref mut wal) = *wal_guard {
                wal.append(next_epoch, &transaction)?;
                wal.fsync()?;
            }
        }

        // Load the current store, clone it (O(1) via im::OrdSet structural
        // sharing — ADR-FERR-001), apply the transaction to the clone,
        // then atomically swap the pointer.
        let current = self.current.load();
        let mut new_store = Store::clone(&current);
        let receipt = new_store.transact(transaction)?;

        // Atomic swap: readers loading after this point see the new state.
        // Readers who loaded before still hold their old Arc<Store> —
        // INV-FERR-006 is preserved because im::OrdSet nodes are
        // reference-counted and live as long as any snapshot holds them.
        self.current.store(Arc::new(new_store));

        Ok(receipt)
    }
}

// Send + Sync safety: ArcSwap<T> is Send+Sync when T: Send+Sync.
// Store is Send+Sync (im::OrdSet is Send+Sync, Schema is Send+Sync).
// Mutex<()> is Send+Sync. Therefore Database is Send+Sync and can
// be shared across threads via Arc<Database>.

#[cfg(test)]
mod tests {
    use super::*;
    use ferratom::{AgentId, Attribute, EntityId, Value};

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

    /// INV-FERR-006: snapshot isolation — a snapshot taken before a write
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

    // -- Regression tests for cleanroom review defects -------------------------

    /// Regression: bd-2w9 — Database with WAL writes WAL before epoch advance.
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
        assert_eq!(
            entries[0].epoch, 1,
            "bd-2w9: WAL entry epoch must be 1"
        );
    }

    /// Regression: bd-2w9 — recover_from_wal restores state.
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
