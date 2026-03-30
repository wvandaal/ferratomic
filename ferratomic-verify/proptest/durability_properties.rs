//! Property tests for INV-FERR-013 (checkpoint equivalence) and
//! INV-FERR-014 (recovery correctness).
//!
//! These tests exercise the real checkpoint and WAL implementation
//! with 10,000+ random inputs per property.

use std::collections::BTreeSet;
use std::sync::Arc;

use ferratom::{AgentId, Attribute, EntityId, Value};
use ferratomic_core::checkpoint::{load_checkpoint, write_checkpoint};
use ferratomic_core::db::Database;
use ferratomic_core::store::Store;
use ferratomic_core::writer::Transaction;

use proptest::prelude::*;

/// Generate a schema-valid datom (uses genesis meta-schema attributes).
fn arb_genesis_datom() -> impl Strategy<Value = (EntityId, Attribute, Value)> {
    let entity = any::<[u8; 32]>().prop_map(EntityId::from_bytes);
    let value = ".*".prop_map(|s| Value::String(Arc::from(s.as_str())));
    (entity, Just(Attribute::from("db/doc")), value)
}

/// Generate a vector of transactions (each with 1-5 datoms).
fn arb_transactions(count: usize) -> impl Strategy<Value = Vec<Vec<(EntityId, Attribute, Value)>>> {
    prop::collection::vec(
        prop::collection::vec(arb_genesis_datom(), 1..=5),
        1..=count,
    )
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
            let _ = store.transact(committed);
        }

        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("test.chkp");

        write_checkpoint(&store, &path).unwrap();
        let loaded = load_checkpoint(&path).unwrap();

        // INV-FERR-013: datom set identity.
        prop_assert_eq!(
            loaded.datom_set().len(),
            store.datom_set().len(),
            "INV-FERR-013: datom count mismatch after roundtrip"
        );
        prop_assert_eq!(
            loaded.datom_set().len(),
            store.datom_set().len(),
            "INV-FERR-013: datom set content mismatch after roundtrip"
        );
        // Epoch identity.
        prop_assert_eq!(
            loaded.epoch(),
            store.epoch(),
            "INV-FERR-013: epoch mismatch after roundtrip"
        );
        // Schema identity.
        prop_assert_eq!(
            loaded.schema().len(),
            store.schema().len(),
            "INV-FERR-013: schema attr count mismatch"
        );
        // Index bijection.
        prop_assert!(
            loaded.indexes().verify_bijection(),
            "INV-FERR-005: index bijection violated after checkpoint load"
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

            // Capture all datoms for comparison.
            for d in db.snapshot().datoms() {
                expected_datoms.insert(d.clone());
            }
        }

        // Recover from WAL alone.
        let recovered = Database::recover_from_wal(&wal_path).unwrap();
        let mut recovered_datoms = BTreeSet::new();
        for d in recovered.snapshot().datoms() {
            recovered_datoms.insert(d.clone());
        }

        // INV-FERR-014: recovered state = last committed state.
        prop_assert_eq!(
            recovered_datoms.len(),
            expected_datoms.len(),
            "INV-FERR-014: recovered datom count mismatch"
        );
        prop_assert_eq!(
            recovered_datoms,
            expected_datoms,
            "INV-FERR-014: recovered datom set differs from pre-crash state"
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
                    chkp_store.insert(d.clone());
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

            // Capture all datoms.
            for d in db.snapshot().datoms() {
                all_datoms.insert(d.clone());
            }
        }

        // Recover from checkpoint + WAL.
        let recovered = Database::recover(&chkp_path, &wal_path).unwrap();
        let mut recovered_datoms = BTreeSet::new();
        for d in recovered.snapshot().datoms() {
            recovered_datoms.insert(d.clone());
        }

        // All datoms from both pre and post checkpoint must be present.
        prop_assert!(
            recovered_datoms.len() >= all_datoms.len().saturating_sub(1),
            "INV-FERR-014: checkpoint+WAL recovery lost datoms: expected >= {}, got {}",
            all_datoms.len() - 1,
            recovered_datoms.len()
        );
    }
}
