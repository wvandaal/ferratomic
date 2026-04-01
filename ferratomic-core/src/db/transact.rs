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
        // INV-FERR-021: pre-check concurrency limit.
        let write_slot = self
            .write_limiter
            .try_acquire()
            .ok_or(FerraError::Backpressure)?;

        // INV-FERR-007: serialize writes + INV-FERR-015: tick HLC.
        let (guard, tx_id) = self.acquire_write_lock_and_tick()?;

        // Step 1: Apply transaction to a cloned store with HLC-derived TxId.
        let current = self.current.load();
        let mut new_store = Store::clone(&current);
        let receipt = new_store.transact(transaction, tx_id)?;
        let new_datoms = receipt.datoms().to_vec();

        // Step 2: INV-FERR-008: WAL before publish.
        self.write_wal(receipt.epoch(), &new_datoms)?;

        // Step 3: Atomic swap.
        self.publish_and_check(new_store);

        // INV-FERR-005: release-mode bijection canary.
        #[cfg(feature = "release_bijection_check")]
        self.verify_bijection_canary()?;

        // Release write lock and backpressure slot BEFORE observer delivery.
        // WAL + ArcSwap are already committed.
        drop(guard);
        drop(write_slot);

        // Step 4: HI-004: Observer delivery is advisory-only.
        let _ = self.notify_observers(receipt.epoch(), &new_datoms);

        Ok(receipt)
    }

    /// Acquire the write lock and tick the HLC under it.
    ///
    /// INV-FERR-007: Write serialization via `try_lock` (non-blocking).
    /// INV-FERR-015: HLC tick under the write lock ensures causal ordering.
    /// ME-001: Poisoned mutex → `InvariantViolation`, not `Backpressure`.
    fn acquire_write_lock_and_tick(
        &self,
    ) -> Result<(std::sync::MutexGuard<'_, ()>, ferratom::TxId), FerraError> {
        let guard = self.write_lock.try_lock().map_err(|e| match e {
            std::sync::TryLockError::Poisoned(_) => FerraError::InvariantViolation {
                invariant: "INV-FERR-007".to_string(),
                details: "write lock mutex poisoned (previous panic)".to_string(),
            },
            std::sync::TryLockError::WouldBlock => FerraError::Backpressure,
        })?;

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

        Ok((guard, tx_id))
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

    /// Atomic-swap the new store into the shared reference.
    ///
    /// INV-FERR-006: readers loading after the swap see the new state.
    fn publish_and_check(&self, new_store: Store) {
        self.current.store(Arc::new(new_store));

        // ME-010: AcqRel ensures the counter increment is visible to
        // other threads checking the bijection canary.
        self.transaction_count.fetch_add(1, Ordering::AcqRel);
    }

    /// INV-FERR-005: release-mode bijection canary.
    ///
    /// Every 100th transaction verifies that secondary indexes remain in
    /// bijection with the primary datom set. Only active when the
    /// `release_bijection_check` feature is enabled.
    ///
    /// # Errors
    ///
    /// Returns `FerraError::InvariantViolation` if the bijection check fails.
    #[cfg(feature = "release_bijection_check")]
    fn verify_bijection_canary(&self) -> Result<(), FerraError> {
        let count = self.transaction_count.load(Ordering::Acquire);
        if count % 100 == 0 {
            let published_store = self.current.load();
            if !published_store.indexes().verify_bijection() {
                return Err(FerraError::InvariantViolation {
                    invariant: "INV-FERR-005".to_string(),
                    details: format!(
                        "index bijection check failed at transaction count {count}, epoch {}",
                        published_store.epoch()
                    ),
                });
            }
        }
        Ok(())
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
