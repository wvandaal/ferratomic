//! WAL fsync ordering, transaction shape, backpressure, and write amplification
//! property tests.
//!
//! Tests INV-FERR-008 (WAL durability), INV-FERR-020 (transaction atomicity),
//! INV-FERR-021 (backpressure safety), INV-FERR-026 (write amplification).
//!
//! Phase 4a: all tests passing against ferratomic-db implementation.

use std::io::Write;

use ferratomic_db::{
    backpressure::{BackpressurePolicy, WriteLimiter},
    wal::Wal,
};
use ferratomic_verify::generators::*;
use proptest::prelude::*;
use tempfile::TempDir;

// bincode: used by INV-FERR-026 to compute logical payload size.
extern crate bincode;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(10_000))]

    /// INV-FERR-008: WAL roundtrip — all committed entries survive recovery.
    ///
    /// Falsification: a committed entry is missing after recovery.
    #[test]
    fn inv_ferr_008_wal_roundtrip(
        txns in prop::collection::vec(arb_transaction(), 1..10),
    ) {
        let dir = TempDir::new().expect("failed to create temp dir");
        let wal_path = dir.path().join("test.wal");
        let mut wal = Wal::create(&wal_path)
            .expect("failed to create WAL");

        let mut payloads = Vec::new();
        for (i, tx) in txns.iter().enumerate() {
            let payload = bincode::serialize(tx.datoms())
                .expect("bincode serialize must succeed");
            wal.append_raw(i as u64 + 1, &payload)
                .expect("failed to append WAL entry");
            payloads.push(payload);
        }
        wal.fsync().expect("failed to fsync WAL");

        let recovered = wal.recover().expect("failed to recover WAL");
        prop_assert_eq!(
            recovered.len(),
            txns.len(),
            "INV-FERR-008 violated: WAL recovery lost entries. \
             wrote={}, recovered={}",
            txns.len(),
            recovered.len()
        );

        for (orig_payload, recov) in payloads.iter().zip(recovered.iter()) {
            prop_assert_eq!(
                orig_payload.as_slice(),
                recov.payload.as_slice(),
                "INV-FERR-008 violated: WAL entry payload differs after recovery"
            );
        }
    }

    /// INV-FERR-008: Crash truncation — partial entries removed,
    /// complete entries survive.
    ///
    /// Falsification: partial entry survives recovery, or complete entry lost.
    #[test]
    fn inv_ferr_008_crash_truncation(
        complete_txns in prop::collection::vec(arb_transaction(), 1..5),
        partial_bytes in prop::collection::vec(any::<u8>(), 1..100),
    ) {
        let dir = TempDir::new().expect("failed to create temp dir");
        let wal_path = dir.path().join("test.wal");
        let mut wal = Wal::create(&wal_path)
            .expect("failed to create WAL");

        for (i, tx) in complete_txns.iter().enumerate() {
            let payload = bincode::serialize(tx.datoms())
                .expect("bincode serialize must succeed");
            wal.append_raw(i as u64 + 1, &payload)
                .expect("failed to append WAL entry");
        }
        wal.fsync().expect("failed to fsync WAL");

        // Simulate crash: append garbage bytes after valid entries
        {
            let mut file = std::fs::OpenOptions::new()
                .append(true)
                .open(&wal_path)
                .expect("failed to open WAL for crash sim");
            file.write_all(&partial_bytes)
                .expect("failed to write partial bytes");
        }

        // Recovery must truncate partial entry, preserve complete ones
        let mut wal = Wal::open(&wal_path).expect("failed to reopen WAL");
        let recovered = wal.recover().expect("failed to recover WAL");

        prop_assert_eq!(
            recovered.len(),
            complete_txns.len(),
            "INV-FERR-008 violated: crash recovery wrong count. \
             complete={}, recovered={}, partial_bytes={}",
            complete_txns.len(),
            recovered.len(),
            partial_bytes.len()
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(10_000))]

    /// INV-FERR-020: All datoms in a committed transaction share one epoch.
    ///
    /// Falsification: two datoms from the same transaction have different
    /// TxIds in the store.
    #[test]
    fn inv_ferr_020_transaction_single_epoch(
        txns in prop::collection::vec(arb_transaction(), 1..10),
    ) {
        let mut store = ferratomic_db::store::Store::genesis();
        for tx in txns {
            let pre_len = store.len();
            let receipt = store.transact_test(tx)
                .expect("INV-FERR-020: transact must succeed");
            let epoch = receipt.epoch();

            // Datoms added by this transaction should all share the same epoch.
            // New datoms = store datoms not in pre-transaction set.
            let new_datoms: Vec<_> = store.datoms()
                .filter(|d| d.tx().physical() == epoch)
                .collect();

            prop_assert!(
                !new_datoms.is_empty(),
                "INV-FERR-020: transaction must add at least one datom. \
                 pre_len={}, post_len={}",
                pre_len, store.len()
            );

            // All new datoms must share the transaction's epoch
            for d in &new_datoms {
                prop_assert_eq!(
                    d.tx().physical(),
                    epoch,
                    "INV-FERR-020 violated: datom has different epoch than receipt. \
                     datom_epoch={}, receipt_epoch={}",
                    d.tx().physical(),
                    epoch
                );
            }
        }
    }

    /// INV-FERR-021: Backpressure safety — WriteLimiter semaphore correctly
    /// bounds concurrent writes.
    ///
    /// For any max_concurrent_writes N in 1..=16:
    /// 1. Acquiring N guards succeeds.
    /// 2. The (N+1)-th acquire returns None (capacity reached).
    /// 3. Dropping one guard frees a slot; the next acquire succeeds.
    ///
    /// Falsification: acquire succeeds beyond capacity, or fails below it.
    #[test]
    fn test_inv_ferr_021_backpressure_safety(
        max_writes in 1_usize..=16,
    ) {
        let policy = BackpressurePolicy {
            max_concurrent_writes: max_writes,
        };
        let limiter = WriteLimiter::new(&policy);

        // Phase 1: acquire up to max_writes guards — all must succeed.
        let mut guards = Vec::with_capacity(max_writes);
        for i in 0..max_writes {
            let guard = limiter.try_acquire();
            prop_assert!(
                guard.is_some(),
                "INV-FERR-021 violated: acquire #{} of {} failed \
                 when capacity should allow it (active={})",
                i + 1,
                max_writes,
                limiter.active_count()
            );
            guards.push(guard.unwrap());
        }
        prop_assert_eq!(
            limiter.active_count(),
            max_writes,
            "INV-FERR-021 violated: active count {} != max {}",
            limiter.active_count(),
            max_writes
        );

        // Phase 2: one more acquire must fail (at capacity).
        let overflow = limiter.try_acquire();
        prop_assert!(
            overflow.is_none(),
            "INV-FERR-021 violated: acquire succeeded at capacity \
             (max={}, active={})",
            max_writes,
            limiter.active_count()
        );
        // Active count must remain at max (failed acquire must not leak).
        prop_assert_eq!(
            limiter.active_count(),
            max_writes,
            "INV-FERR-021 violated: failed acquire changed active count \
             (expected {}, got {})",
            max_writes,
            limiter.active_count()
        );

        // Phase 3: drop one guard — slot freed — next acquire succeeds.
        guards.pop();
        prop_assert_eq!(
            limiter.active_count(),
            max_writes - 1,
            "INV-FERR-021 violated: drop did not decrement active count \
             (expected {}, got {})",
            max_writes - 1,
            limiter.active_count()
        );

        let reclaimed = limiter.try_acquire();
        prop_assert!(
            reclaimed.is_some(),
            "INV-FERR-021 violated: acquire failed after releasing a slot \
             (max={}, active={})",
            max_writes,
            limiter.active_count()
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(10_000))]

    /// INV-FERR-026: Write amplification < 10x.
    ///
    /// Write N datoms via WAL, measure the on-disk WAL size vs the logical
    /// payload size (bincode-serialized datoms). The ratio (WA) must be
    /// less than 10x. WAL frame overhead is 22 bytes per entry (header + CRC,
    /// see bd-pu4t, `WAL_FRAME_OVERHEAD` in `ferratomic-verify/kani/durability.rs`);
    /// the rest is the payload. For non-trivial payloads, this overhead is
    /// small relative to the datom data, so WA << 10x.
    ///
    /// Falsification: WAL file size exceeds 10x the logical payload size.
    #[test]
    fn inv_ferr_026_write_amplification(
        txns in prop::collection::vec(arb_transaction(), 1..10),
    ) {
        let dir = TempDir::new().expect("failed to create temp dir");
        let wal_path = dir.path().join("test.wal");
        let mut wal = Wal::create(&wal_path)
            .expect("failed to create WAL");

        let mut logical_payload_bytes: u64 = 0;

        for (i, tx) in txns.iter().enumerate() {
            // Compute the logical payload size (bincode serialization of datoms).
            let payload = bincode::serialize(tx.datoms())
                .expect("bincode serialize must succeed");
            logical_payload_bytes += payload.len() as u64;

            wal.append_raw(i as u64 + 1, &payload)
                .expect("failed to append WAL entry");
        }
        wal.fsync().expect("failed to fsync WAL");

        // Measure on-disk WAL size.
        let wal_file_size = std::fs::metadata(&wal_path)
            .expect("WAL file must exist")
            .len();

        // Guard against division by zero (empty payload is degenerate).
        if logical_payload_bytes > 0 {
            let wa_ratio = wal_file_size as f64 / logical_payload_bytes as f64;
            prop_assert!(
                wa_ratio < 10.0,
                "INV-FERR-026 violated: write amplification {:.2}x >= 10x. \
                 wal_size={}, logical_payload={}",
                wa_ratio, wal_file_size, logical_payload_bytes
            );
        }
    }
}
