//! Transaction application for `Database<Ready>`.
//!
//! INV-FERR-007: write linearizability via single-threaded writer.
//! INV-FERR-008: WAL write + fsync before epoch advance.
//! INV-FERR-020: transaction atomicity via full-batch swap.
//! INV-FERR-021: backpressure via `WriteLimiter` pre-check.

use std::sync::{atomic::Ordering, Arc};

use ferratom::{Datom, FerraError};

use super::{Database, Ready};
use crate::{
    store::{Store, TxReceipt},
    writer::{Committed, Transaction},
};

impl Database<Ready> {
    /// Apply a committed transaction to the database.
    ///
    /// INV-FERR-007: Serialized via write lock. Only one transaction is
    /// applied at a time. The epoch strictly increases with each successful
    /// transact.
    ///
    /// INV-FERR-006: After the write completes, subsequent `snapshot()` calls
    /// see the new state. Existing snapshots held by readers are unaffected
    /// (structural sharing via `im::OrdSet` -- ADR-FERR-001).
    ///
    /// INV-FERR-020: transaction atomicity. The write lock serializes all
    /// transactions so no interleaving is possible. `Store::transact` stamps
    /// every datom in the batch with the same epoch before any become visible.
    /// On success the full batch is published via `ArcSwap::store`; on failure
    /// the `Result::Err` path returns before the swap, leaving the store
    /// unchanged. Combined with WAL single-entry writes (INV-FERR-008), crash
    /// recovery replays or discards entire transactions -- never partial ones.
    ///
    /// # Errors
    ///
    /// Returns `FerraError::Backpressure` if the write lock is already held
    /// (try-lock semantics -- the caller should retry or shed load).
    /// Returns other `FerraError` variants if the transaction application
    /// itself fails (e.g., `EmptyTransaction`, `InvariantViolation`).
    pub fn transact(&self, transaction: Transaction<Committed>) -> Result<TxReceipt, FerraError> {
        // INV-FERR-021: pre-check concurrency limit before trying the lock.
        let write_slot = self
            .write_limiter
            .try_acquire()
            .ok_or(FerraError::Backpressure)?;

        // INV-FERR-007: serialize writes. try_lock returns immediately
        // rather than blocking, so callers can shed load under contention.
        // ME-001: Distinguish poisoned mutex (InvariantViolation) from
        // contention (Backpressure). A poisoned mutex is permanently broken.
        let guard = self.write_lock.try_lock().map_err(|e| match e {
            std::sync::TryLockError::Poisoned(_) => FerraError::InvariantViolation {
                invariant: "INV-FERR-007".to_string(),
                details: "write lock mutex poisoned (previous panic)".to_string(),
            },
            std::sync::TryLockError::WouldBlock => FerraError::Backpressure,
        })?;

        // HI-011: Tick the HLC under the write lock to produce a causally-
        // ordered TxId. INV-FERR-015: monotonicity guaranteed by HybridClock.
        let tx_id = {
            let mut clock = self
                .clock
                .lock()
                .map_err(|_| FerraError::InvariantViolation {
                    invariant: "INV-FERR-015".to_string(),
                    details: "HLC mutex poisoned (previous panic)".to_string(),
                })?;
            clock.tick()
        };

        // Step 1: Apply transaction to a cloned store with HLC-derived TxId.
        let current = self.current.load();
        let mut new_store = Store::clone(&current);
        let receipt = new_store.transact(transaction, tx_id)?;
        let new_datoms = receipt.datoms().to_vec();

        // Step 2: INV-FERR-008: WAL before publish.
        self.write_wal(receipt.epoch(), &new_datoms)?;

        // Step 3: Atomic swap + bijection canary.
        self.publish_and_check(new_store);

        // Release write lock and backpressure slot BEFORE observer delivery
        // (bd-jxi / CR-032). WAL + ArcSwap are already committed.
        drop(guard);
        drop(write_slot);

        // Step 4: Observer delivery (outside write lock scope).
        // HI-004: Observer delivery failure is advisory-only. The transaction
        // IS already committed (WAL fsynced + ArcSwap stored). Propagating
        // observer errors would mislead callers into retrying a committed tx.
        let _ = self.notify_observers(receipt.epoch(), &new_datoms);

        Ok(receipt)
    }

    /// Write stamped datoms to the WAL and fsync before publish.
    ///
    /// INV-FERR-008: `durable(WAL(T)) BEFORE visible(SNAP(e))`.
    /// The WAL contains post-stamp datoms so recovery produces identical state.
    /// No-op when no WAL is attached (in-memory-only mode).
    ///
    /// # Errors
    ///
    /// Returns `FerraError::WalWrite` on serialization or I/O failure.
    /// Returns `FerraError::Backpressure` if the WAL mutex is poisoned.
    fn write_wal(&self, epoch: u64, datoms: &[Datom]) -> Result<(), FerraError> {
        // ME-002: Poisoned WAL mutex is an invariant violation (permanent
        // failure from a prior panic), not backpressure (transient contention).
        let mut wal_guard = self
            .wal
            .lock()
            .map_err(|_| FerraError::InvariantViolation {
                invariant: "INV-FERR-008".to_string(),
                details: "WAL mutex poisoned (previous panic)".to_string(),
            })?;
        if let Some(ref mut wal) = *wal_guard {
            let payload =
                bincode::serialize(datoms).map_err(|e| FerraError::WalWrite(e.to_string()))?;
            wal.append_raw(epoch, &payload)?;
            wal.fsync()?;
        }
        Ok(())
    }

    /// Atomic-swap the new store and run the release-mode bijection canary.
    ///
    /// INV-FERR-006: readers loading after the swap see the new state.
    /// INV-FERR-005: every 100th transaction checks index bijection in
    /// release builds (when `release_bijection_check` feature is enabled).
    fn publish_and_check(&self, new_store: Store) {
        self.current.store(Arc::new(new_store));

        // ME-010: AcqRel ensures the counter increment is visible to
        // other threads checking the bijection canary.
        let count = self.transaction_count.fetch_add(1, Ordering::AcqRel) + 1;
        #[cfg(feature = "release_bijection_check")]
        {
            if count % 100 == 0 {
                let published_store = self.current.load();
                if !published_store.indexes().verify_bijection() {
                    eprintln!(
                        "WARN [ferratomic-core] INV-FERR-005 violation: \
                         index bijection check failed at transaction count {count}, \
                         epoch {}",
                        published_store.epoch(),
                    );
                }
            }
        }
        // Suppress unused-variable warning when the feature is not enabled.
        let _ = count;
    }

    /// Deliver commit notification to registered observers.
    ///
    /// INV-FERR-011: delivery is serialized by the observers mutex, not
    /// the write lock. Slow callbacks do not block concurrent `transact()`
    /// callers. Delivery is best-effort at-least-once.
    ///
    /// # Errors
    ///
    /// Returns `FerraError::InvariantViolation` if the observer mutex is
    /// poisoned.
    fn notify_observers(&self, epoch: u64, datoms: &[Datom]) -> Result<(), FerraError> {
        let published = self.current.load();
        let mut observers = self
            .observers
            .lock()
            .map_err(|_| FerraError::InvariantViolation {
                invariant: "INV-FERR-011".to_string(),
                details: "observer registry mutex poisoned during publish".to_string(),
            })?;
        observers.publish(epoch, datoms, published.as_ref());
        Ok(())
    }
}
