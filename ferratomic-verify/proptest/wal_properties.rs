//! WAL fsync ordering property tests.
//!
//! Tests INV-FERR-008: WAL entries are durable before snapshot publication.
//!
//! ALL TESTS MUST FAIL (red phase). Types are not yet implemented.

use ferratomic_core::wal::Wal;
use ferratomic_verify::generators::*;
use proptest::prelude::*;
use std::io::Write;
use tempfile::TempDir;

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

        for (i, tx) in txns.iter().enumerate() {
            wal.append(i as u64 + 1, tx)
                .expect("failed to append WAL entry");
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

        for (orig, recov) in txns.iter().zip(recovered.iter()) {
            let recovered_datoms: Vec<ferratom::Datom> =
                serde_json::from_slice(&recov.payload)
                    .expect("deserialize WAL payload");
            prop_assert_eq!(
                orig.datoms(),
                recovered_datoms.as_slice(),
                "INV-FERR-008 violated: WAL entry content differs after recovery"
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
            wal.append(i as u64 + 1, tx)
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
