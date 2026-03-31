//! Transaction application for `Database<Ready>`.
//!
//! INV-FERR-007: write linearizability via single-threaded writer.
//! INV-FERR-008: WAL write + fsync before epoch advance.
//! INV-FERR-020: transaction atomicity via full-batch swap.
//! INV-FERR-021: backpressure via `WriteLimiter` pre-check.

use std::sync::{atomic::Ordering, Arc};

use ferratom::FerraError;

use crate::{
    store::{Store, TxReceipt},
    writer::{Committed, Transaction},
};

use super::{Database, Ready};

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
        let guard = self
            .write_lock
            .try_lock()
            .map_err(|_| FerraError::Backpressure)?;

        // Step 1: Apply transaction to a cloned store (stamps TxIds, creates
        // tx metadata datoms). The clone is NOT yet published.
        // TxReceipt carries the inserted datoms directly -- no O(n) set
        // difference needed. This is O(m) where m = transaction datom count.
        let current = self.current.load();
        let mut new_store = Store::clone(&current);
        let receipt = new_store.transact(transaction)?;
        let new_datoms = receipt.datoms().to_vec();

        // Step 2: INV-FERR-008: Write WAL with STAMPED datoms BEFORE publishing.
        // durable(WAL(T)) BEFORE visible(SNAP(e)).
        // The WAL contains post-stamp datoms so recovery produces identical state.
        {
            let mut wal_guard = self.wal.lock().map_err(|_| FerraError::Backpressure)?;
            if let Some(ref mut wal) = *wal_guard {
                let payload = bincode::serialize(&new_datoms)
                    .map_err(|e| FerraError::WalWrite(e.to_string()))?;
                wal.append_raw(receipt.epoch(), &payload)?;
                wal.fsync()?;
            }
        }

        // Step 3: Atomic swap -- readers loading after this point see the new state.
        // INV-FERR-006 preserved: im::OrdSet nodes are reference-counted.
        self.current.store(Arc::new(new_store));

        // Step 3b: Increment transaction counter and run release-mode bijection
        // canary every 100 transactions. INV-FERR-005: index bijection check.
        // In debug builds, Store::transact already asserts bijection via
        // debug_assert!. This canary provides sampling-based coverage in
        // release builds when the `release_bijection_check` feature is enabled.
        let count = self.transaction_count.fetch_add(1, Ordering::Relaxed) + 1;
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

        // Release write lock and backpressure slot BEFORE observer delivery
        // (bd-jxi / CR-032). Slow observer on_commit callbacks no longer
        // block concurrent transact() callers. WAL + ArcSwap are already
        // committed; observer delivery is best-effort at-least-once.
        drop(guard);
        drop(write_slot);

        // Step 4: Observer delivery (outside write lock scope).
        // INV-FERR-011: delivery serialized by observers mutex, not write lock.
        let published = self.current.load();
        let mut observers = self
            .observers
            .lock()
            .map_err(|_| FerraError::InvariantViolation {
                invariant: "INV-FERR-011".to_string(),
                details: "observer registry mutex poisoned during publish".to_string(),
            })?;
        observers.publish(receipt.epoch(), &new_datoms, published.as_ref());

        Ok(receipt)
    }
}
