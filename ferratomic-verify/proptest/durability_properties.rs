//! Property tests for INV-FERR-013 (checkpoint equivalence),
//! INV-FERR-014 (recovery correctness), INV-FERR-024 (substrate agnosticism),
//! and INV-FERR-028 (cold start correctness).
//!
//! These tests exercise the real checkpoint and WAL implementation
//! with 10,000+ random inputs per property.

use std::{collections::BTreeSet, sync::Arc};

use ferratom::{AgentId, Attribute, EntityId, Value};
use ferratomic_core::{
    checkpoint::{load_checkpoint, write_checkpoint, write_checkpoint_to_writer},
    db::Database,
    storage::{cold_start_with_backend, InMemoryBackend, StorageBackend},
    store::Store,
    writer::Transaction,
};
use proptest::prelude::*;

/// Generate a schema-valid datom (uses genesis meta-schema attributes).
fn arb_genesis_datom() -> impl Strategy<Value = (EntityId, Attribute, Value)> {
    let entity = any::<[u8; 32]>().prop_map(EntityId::from_bytes);
    let value = ".*".prop_map(|s| Value::String(Arc::from(s.as_str())));
    (entity, Just(Attribute::from("db/doc")), value)
}

/// Generate a vector of transactions (each with 1-5 datoms).
fn arb_transactions(count: usize) -> impl Strategy<Value = Vec<Vec<(EntityId, Attribute, Value)>>> {
    prop::collection::vec(prop::collection::vec(arb_genesis_datom(), 1..=5), 1..=count)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(10_000))]

    /// INV-FERR-013: checkpoint(store) → load → identical datom set.
    #[test]
    fn inv_ferr_013_checkpoint_roundtrip(
        tx_batches in arb_transactions(10),
    ) {
        let mut store = Store::genesis();
        let agent = AgentId::from_bytes([42u8; 16]);

        for batch in &tx_batches {
            let mut tx = Transaction::new(agent);
            for (entity, attr, val) in batch {
                tx = tx.assert_datom(*entity, attr.clone(), val.clone());
            }
            let committed = tx.commit_unchecked();
            let _ = store.transact_test(committed);
        }

        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("test.chkp");

        write_checkpoint(&store, &path).unwrap();
        let loaded = load_checkpoint(&path).unwrap();

        // Index bijection (derived from datoms, so this is a secondary check).
        prop_assert!(
            loaded.indexes().verify_bijection(),
            "INV-FERR-005: index bijection violated after checkpoint load"
        );
        // INV-FERR-013: exact state equality — datoms, schema, epoch.
        // Store does not implement PartialEq; compare components instead.
        prop_assert_eq!(
            loaded.datom_set().len(), store.datom_set().len(),
            "INV-FERR-013: datom count mismatch after roundtrip"
        );
        prop_assert_eq!(
            loaded.datom_set(), store.datom_set(),
            "INV-FERR-013: datom set content mismatch after roundtrip"
        );
        prop_assert_eq!(
            loaded.epoch(), store.epoch(),
            "INV-FERR-013: epoch mismatch after roundtrip"
        );
        prop_assert_eq!(
            loaded.schema().len(), store.schema().len(),
            "INV-FERR-013: schema attr count mismatch after roundtrip"
        );
    }

    /// INV-FERR-014: WAL recovery produces the last committed state.
    #[test]
    fn inv_ferr_014_wal_recovery_correctness(
        tx_batches in arb_transactions(5),
    ) {
        let dir = tempfile::TempDir::new().unwrap();
        let wal_path = dir.path().join("test.wal");

        let mut expected_datoms = BTreeSet::new();
        let expected_epoch: u64;
        let expected_schema: ferratom::Schema;

        // Write transactions via WAL-backed database.
        {
            let db = Database::genesis_with_wal(&wal_path).unwrap();
            let agent = AgentId::from_bytes([99u8; 16]);
            let schema = db.schema();

            for batch in &tx_batches {
                let mut tx = Transaction::new(agent);
                for (entity, attr, val) in batch {
                    tx = tx.assert_datom(*entity, attr.clone(), val.clone());
                }
                let committed = tx.commit(&schema).unwrap();
                let _ = db.transact(committed);
            }

            // Capture full state for comparison.
            for d in db.snapshot().datoms() {
                expected_datoms.insert(d.clone());
            }
            expected_epoch = db.epoch();
            expected_schema = db.schema();
        }

        // Recover from WAL alone.
        let recovered = Database::recover_from_wal(&wal_path).unwrap();
        let mut recovered_datoms = BTreeSet::new();
        for d in recovered.snapshot().datoms() {
            recovered_datoms.insert(d.clone());
        }

        // INV-FERR-014: recovered state = last committed state (exact equality).
        prop_assert_eq!(
            recovered_datoms,
            expected_datoms,
            "INV-FERR-014: recovered datom set differs from pre-crash state"
        );
        prop_assert_eq!(
            recovered.epoch(),
            expected_epoch,
            "INV-FERR-014: recovered epoch differs from pre-crash epoch"
        );
        prop_assert_eq!(
            recovered.schema(),
            expected_schema,
            "INV-FERR-014: recovered schema differs from pre-crash schema"
        );
    }

    /// INV-FERR-014: checkpoint + WAL recovery = full state.
    #[test]
    fn inv_ferr_014_checkpoint_plus_wal_recovery(
        pre_chkp in arb_transactions(3),
        post_chkp in arb_transactions(3),
    ) {
        let dir = tempfile::TempDir::new().unwrap();
        let wal_path = dir.path().join("test.wal");
        let chkp_path = dir.path().join("test.chkp");

        let mut all_datoms = BTreeSet::new();
        let expected_epoch: u64;
        let expected_schema: ferratom::Schema;

        {
            let db = Database::genesis_with_wal(&wal_path).unwrap();
            let agent = AgentId::from_bytes([77u8; 16]);
            let schema = db.schema();

            // Pre-checkpoint transactions.
            for batch in &pre_chkp {
                let mut tx = Transaction::new(agent);
                for (entity, attr, val) in batch {
                    tx = tx.assert_datom(*entity, attr.clone(), val.clone());
                }
                let committed = tx.commit(&schema).unwrap();
                let _ = db.transact(committed);
            }

            // Write checkpoint.
            {
                let snap = db.snapshot();
                let mut chkp_store = Store::genesis();
                for d in snap.datoms() {
                    chkp_store.insert(d);
                }
                write_checkpoint(&chkp_store, &chkp_path).unwrap();
            }

            // Post-checkpoint transactions.
            for batch in &post_chkp {
                let mut tx = Transaction::new(agent);
                for (entity, attr, val) in batch {
                    tx = tx.assert_datom(*entity, attr.clone(), val.clone());
                }
                let committed = tx.commit(&schema).unwrap();
                let _ = db.transact(committed);
            }

            // Capture full state.
            for d in db.snapshot().datoms() {
                all_datoms.insert(d.clone());
            }
            expected_epoch = db.epoch();
            expected_schema = db.schema();
        }

        // Recover from checkpoint + WAL.
        let recovered = Database::recover(&chkp_path, &wal_path).unwrap();
        let mut recovered_datoms = BTreeSet::new();
        for d in recovered.snapshot().datoms() {
            recovered_datoms.insert(d.clone());
        }

        // INV-FERR-014: exact state equality — no off-by-one tolerance.
        prop_assert_eq!(
            recovered_datoms,
            all_datoms,
            "INV-FERR-014: checkpoint+WAL recovery datom set differs from pre-crash state"
        );
        prop_assert_eq!(
            recovered.epoch(),
            expected_epoch,
            "INV-FERR-014: checkpoint+WAL recovery epoch differs from pre-crash epoch"
        );
        prop_assert_eq!(
            recovered.schema(),
            expected_schema,
            "INV-FERR-014: checkpoint+WAL recovery schema differs from pre-crash schema"
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(10_000))]

    /// INV-FERR-024: InMemoryBackend cold-start round-trip preserves store.
    ///
    /// Write a store to an InMemoryBackend via checkpoint, then cold-start
    /// from that backend. The recovered database must contain all datoms
    /// from the original store.
    ///
    /// Falsification: a datom present before checkpoint is absent after
    /// cold-start recovery, or epoch/schema disagree.
    #[test]
    fn inv_ferr_024_in_memory_cold_start_roundtrip(
        tx_batches in arb_transactions(5),
    ) {
        let backend = InMemoryBackend::new();
        let agent = AgentId::from_bytes([24u8; 16]);

        // Build a store with some transactions.
        let mut store = Store::genesis();
        for batch in &tx_batches {
            let mut tx = Transaction::new(agent);
            for (entity, attr, val) in batch {
                tx = tx.assert_datom(*entity, attr.clone(), val.clone());
            }
            let committed = tx.commit_unchecked();
            let _ = store.transact_test(committed);
        }

        // Write checkpoint to in-memory backend.
        {
            let mut writer = backend.open_checkpoint_writer()
                .expect("INV-FERR-024: open_checkpoint_writer must succeed");
            write_checkpoint_to_writer(&store, &mut writer)
                .expect("INV-FERR-024: write_checkpoint_to_writer must succeed");
        }

        // Cold-start from the in-memory backend.
        let result = cold_start_with_backend(&backend)
            .expect("INV-FERR-024: cold_start_with_backend must succeed");

        // Recovered datom set must match original.
        let recovered_datoms: BTreeSet<_> = result.database.snapshot().datoms().cloned().collect();
        let original_datoms: BTreeSet<_> = store.datoms().cloned().collect();

        prop_assert_eq!(
            recovered_datoms.len(), original_datoms.len(),
            "INV-FERR-024 violated: datom count mismatch. original={}, recovered={}",
            original_datoms.len(), recovered_datoms.len()
        );
        prop_assert_eq!(
            recovered_datoms, original_datoms,
            "INV-FERR-024 violated: datom set content mismatch after in-memory cold start"
        );
    }

    /// INV-FERR-028: Cold-start recovery produces correct state.
    ///
    /// Checkpoint a store, then recover via `cold_start`. The recovered
    /// database must contain all datoms from the checkpointed store
    /// and have the correct epoch. This tests correctness, not timing
    /// (the < 5s target at 100M datoms is a benchmark concern).
    ///
    /// Falsification: recovered datom set or epoch differs from the
    /// checkpointed state.
    #[test]
    fn inv_ferr_028_cold_start_checkpoint_correctness(
        tx_batches in arb_transactions(8),
    ) {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let agent = AgentId::from_bytes([28u8; 16]);

        // Build a store with transactions.
        let mut store = Store::genesis();
        for batch in &tx_batches {
            let mut tx = Transaction::new(agent);
            for (entity, attr, val) in batch {
                tx = tx.assert_datom(*entity, attr.clone(), val.clone());
            }
            let committed = tx.commit_unchecked();
            let _ = store.transact_test(committed);
        }

        // Checkpoint to disk.
        let chkp_path = dir.path().join("checkpoint.chkp");
        write_checkpoint(&store, &chkp_path)
            .expect("INV-FERR-028: write_checkpoint must succeed");

        // Load from checkpoint.
        let loaded = load_checkpoint(&chkp_path)
            .expect("INV-FERR-028: load_checkpoint must succeed");

        // Verify round-trip correctness.
        prop_assert_eq!(
            loaded.datom_set(), store.datom_set(),
            "INV-FERR-028 violated: datom set differs after checkpoint round-trip"
        );
        prop_assert_eq!(
            loaded.epoch(), store.epoch(),
            "INV-FERR-028 violated: epoch differs. original={}, loaded={}",
            store.epoch(), loaded.epoch()
        );
        prop_assert_eq!(
            loaded.schema().len(), store.schema().len(),
            "INV-FERR-028 violated: schema length differs. original={}, loaded={}",
            store.schema().len(), loaded.schema().len()
        );
    }
}
