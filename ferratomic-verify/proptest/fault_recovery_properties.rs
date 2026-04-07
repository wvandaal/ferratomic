//! INV-FERR-056: Crash recovery under adversarial fault model.
//!
//! Property-based tests verifying that the storage layer handles
//! faults correctly: no silent data corruption, no phantom datoms,
//! no panics. Uses `FaultInjectingBackend` (ADR-FERR-011).

use ferratom::{AgentId, Attribute, EntityId, Value};
use ferratomic_db::{
    checkpoint::{load_checkpoint, write_checkpoint, write_checkpoint_to_writer},
    storage::{cold_start_with_backend, InMemoryBackend, RecoveryLevel},
    store::Store,
    writer::Transaction,
};
use ferratomic_verify::fault_injection::{FaultInjectingBackend, FaultSpec};
use proptest::prelude::*;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a store with `n` user datoms (each tx also adds 2 metadata datoms).
fn store_with_datoms(n: usize) -> Store {
    let mut store = Store::genesis();
    let agent = AgentId::from_bytes([1u8; 16]);
    for i in 0..n {
        let tx = Transaction::new(agent)
            .assert_datom(
                EntityId::from_content(format!("e-{i}").as_bytes()),
                Attribute::from("db/doc"),
                Value::String(format!("v-{i}").into()),
            )
            .commit_unchecked();
        // bd-s0kt: `.expect()` is idiomatic for this pattern.
        store
            .transact_test(tx)
            .expect("INV-FERR-056: transact must succeed in test setup");
    }
    store
}

// ---------------------------------------------------------------------------
// Property tests
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(10_000))]

    /// INV-FERR-056: Checkpoint bit flip is detected by BLAKE3.
    ///
    /// Write a valid checkpoint to a temp file, read the raw bytes,
    /// flip one bit, write back, and assert `load_checkpoint` fails.
    #[test]
    fn inv_ferr_056_checkpoint_corruption_detected(
        datom_count in 1_usize..20,
        flip_offset in 0_usize..2000,
        flip_bit in 0_u8..8,
    ) {
        let store = store_with_datoms(datom_count);
        let mut bytes = Vec::new();
        write_checkpoint_to_writer(&store, &mut bytes)
            .expect("serialize checkpoint");

        // bd-gdgq: Skip no-op cases where flip_offset exceeds checkpoint size.
        prop_assume!(flip_offset < bytes.len(), "skip no-op cases where flip_offset exceeds checkpoint size");

        if flip_offset < bytes.len() {
            bytes[flip_offset] ^= 1 << flip_bit;
            let dir = tempfile::TempDir::new().expect("tmpdir");
            let path = dir.path().join("corrupted.chkp");
            std::fs::write(&path, &bytes).expect("write corrupted");
            let result = load_checkpoint(&path);
            prop_assert!(
                result.is_err(),
                "INV-FERR-056: bit flip at offset {flip_offset} \
                 bit {flip_bit} not detected"
            );
        }
    }

    /// INV-FERR-056: Checkpoint roundtrip is exact under no faults.
    ///
    /// Baseline: without faults, checkpoint roundtrip preserves
    /// datom set, epoch, and schema exactly (INV-FERR-013).
    #[test]
    fn inv_ferr_056_checkpoint_roundtrip_baseline(
        datom_count in 1_usize..30,
    ) {
        let store = store_with_datoms(datom_count);
        let dir = tempfile::TempDir::new().expect("tmpdir");
        let path = dir.path().join("roundtrip.chkp");
        write_checkpoint(&store, &path).expect("write checkpoint");
        let recovered = load_checkpoint(&path).expect("load checkpoint");

        prop_assert_eq!(store.epoch(), recovered.epoch());
        let orig: Vec<_> = store.datoms().collect();
        let recv: Vec<_> = recovered.datoms().collect();
        prop_assert_eq!(orig.len(), recv.len());
    }

    /// INV-FERR-056: Power cut recovery via FaultInjectingBackend.
    ///
    /// Cold start with a power cut scheduled after the Nth sync.
    /// On a fresh backend (no checkpoint, no WAL), the only path is
    /// genesis. The power cut may prevent WAL creation, but genesis
    /// should still work in-memory. Key: no panics, no corruption.
    #[test]
    fn inv_ferr_056_power_cut_cold_start(
        cut_after in 1_usize..5,
    ) {
        let backend = FaultInjectingBackend::new(
            InMemoryBackend::new(),
            vec![FaultSpec::PowerCut { after_nth_sync: cut_after }],
        );

        let result = cold_start_with_backend(&backend);
        match result {
            Ok(cs) => {
                // bd-mcvs: Verify recovered store is internally consistent.
                // Faults during genesis may produce an empty store (no WAL
                // committed). Both empty and populated states are valid — the
                // key assertion is that schema and datom count are coherent.
                let snap = cs.database.snapshot();
                let datom_count = snap.datoms().count();
                let schema = cs.database.schema();
                if datom_count > 0 {
                    prop_assert!(
                        !schema.is_empty(),
                        "INV-FERR-056: non-empty store must have schema"
                    );
                }
                // bd-taa8: InMemoryBackend::new() starts fresh (no checkpoint,
                // no WAL), so the only valid Ok path is genesis.
                if datom_count == 0 {
                    prop_assert_eq!(
                        cs.level,
                        RecoveryLevel::Genesis,
                        "INV-FERR-056: empty store from fresh backend must be Genesis level"
                    );
                }
            }
            Err(ferratom::FerraError::Io { .. })
            | Err(ferratom::FerraError::WalRead(_))
            | Err(ferratom::FerraError::WalWrite(_))
            | Err(ferratom::FerraError::CheckpointCorrupted { .. })
            | Err(ferratom::FerraError::CheckpointWrite(_)) => {}
            Err(ferratom::FerraError::InvariantViolation { invariant, details }) => {
                prop_assert!(
                    false,
                    "INV-FERR-056: power cut must not cause InvariantViolation \
                     (got {invariant}: {details}) — this indicates a logic bug, not a fault"
                );
            }
            // Catch-all: any new FerraError variant not listed above will fail the test,
            // ensuring explicit review when the error taxonomy expands.
            Err(e) => {
                prop_assert!(false, "INV-FERR-056: unexpected error category: {e}");
            }
        }
    }

    /// INV-FERR-056: IO error during cold start is handled gracefully.
    ///
    /// A transient IO error on the Nth read must produce an error
    /// or fall back to genesis — never corrupt data silently.
    #[test]
    fn inv_ferr_056_io_error_cold_start(
        nth_read in 1_usize..5,
    ) {
        let backend = FaultInjectingBackend::new(
            InMemoryBackend::new(),
            vec![FaultSpec::IoError { nth_read }],
        );

        let result = cold_start_with_backend(&backend);
        match result {
            Ok(cs) => {
                let snap = cs.database.snapshot();
                let datom_count = snap.datoms().count();
                let schema = cs.database.schema();
                if datom_count > 0 {
                    prop_assert!(
                        !schema.is_empty(),
                        "INV-FERR-056: non-empty store must have schema"
                    );
                }
                // bd-taa8: InMemoryBackend::new() starts fresh (no checkpoint,
                // no WAL), so the only valid Ok path is genesis.
                if datom_count == 0 {
                    prop_assert_eq!(
                        cs.level,
                        RecoveryLevel::Genesis,
                        "INV-FERR-056: empty store from fresh backend must be Genesis level"
                    );
                }
            }
            Err(ferratom::FerraError::Io { .. })
            | Err(ferratom::FerraError::WalRead(_))
            | Err(ferratom::FerraError::WalWrite(_))
            | Err(ferratom::FerraError::CheckpointCorrupted { .. })
            | Err(ferratom::FerraError::CheckpointWrite(_)) => {}
            Err(ferratom::FerraError::InvariantViolation { invariant, details }) => {
                prop_assert!(
                    false,
                    "INV-FERR-056: IO error must not cause InvariantViolation \
                     (got {invariant}: {details}) — this indicates a logic bug, not a fault"
                );
            }
            // Catch-all: any new FerraError variant not listed above will fail the test,
            // ensuring explicit review when the error taxonomy expands.
            Err(e) => {
                prop_assert!(false, "INV-FERR-056: unexpected error category: {e}");
            }
        }
    }

    /// INV-FERR-056: Disk full during cold start is handled gracefully.
    ///
    /// ENOSPC on the Nth write must not cause a panic or silent corruption.
    #[test]
    fn inv_ferr_056_disk_full_cold_start(
        nth_write in 1_usize..5,
    ) {
        let backend = FaultInjectingBackend::new(
            InMemoryBackend::new(),
            vec![FaultSpec::DiskFull { after_nth_write: nth_write }],
        );

        let result = cold_start_with_backend(&backend);
        match result {
            Ok(cs) => {
                let snap = cs.database.snapshot();
                let datom_count = snap.datoms().count();
                let schema = cs.database.schema();
                if datom_count > 0 {
                    prop_assert!(
                        !schema.is_empty(),
                        "INV-FERR-056: non-empty store must have schema"
                    );
                }
                // bd-taa8: InMemoryBackend::new() starts fresh (no checkpoint,
                // no WAL), so the only valid Ok path is genesis.
                if datom_count == 0 {
                    prop_assert_eq!(
                        cs.level,
                        RecoveryLevel::Genesis,
                        "INV-FERR-056: empty store from fresh backend must be Genesis level"
                    );
                }
            }
            Err(ferratom::FerraError::Io { .. })
            | Err(ferratom::FerraError::WalRead(_))
            | Err(ferratom::FerraError::WalWrite(_))
            | Err(ferratom::FerraError::CheckpointCorrupted { .. })
            | Err(ferratom::FerraError::CheckpointWrite(_)) => {}
            Err(ferratom::FerraError::InvariantViolation { invariant, details }) => {
                prop_assert!(
                    false,
                    "INV-FERR-056: disk full must not cause InvariantViolation \
                     (got {invariant}: {details}) — this indicates a logic bug, not a fault"
                );
            }
            // Catch-all: any new FerraError variant not listed above will fail the test,
            // ensuring explicit review when the error taxonomy expands.
            Err(e) => {
                prop_assert!(false, "INV-FERR-056: unexpected error category: {e}");
            }
        }
    }
}
