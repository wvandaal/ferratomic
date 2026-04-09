//! Property tests for INV-FERR-018 (append-only).
//!
//! The store only grows. No datom is ever removed by any operation.
//! Retractions are new datoms with Op::Retract.

use std::{collections::BTreeSet, sync::Arc};

use ferratom::{Attribute, Datom, EntityId, NodeId, Op, TxId, Value};
use ferratomic_db::{store::Store, writer::Transaction};
use proptest::prelude::*;

/// Generate a schema-valid datom triple.
fn arb_genesis_triple() -> impl Strategy<Value = (EntityId, Attribute, Value)> {
    let entity = any::<[u8; 32]>().prop_map(EntityId::from_bytes);
    let value = ".*".prop_map(|s| Value::String(Arc::from(s.as_str())));
    (entity, Just(Attribute::from("db/doc")), value)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(10_000))]

    /// INV-FERR-018: after any sequence of transact operations,
    /// the store size never decreases.
    #[test]
    fn inv_ferr_018_transact_monotonic_growth(
        tx_count in 1usize..20,
        triples in prop::collection::vec(arb_genesis_triple(), 1..5),
    ) {
        let mut store = Store::genesis();
        let node = NodeId::from_bytes([1u8; 16]);
        let mut prev_len = store.len();

        for i in 0..tx_count {
            let idx = i % triples.len();
            let (entity, attr, val) = &triples[idx];
            let tx = Transaction::new(node)
                .assert_datom(*entity, attr.clone(), val.clone())
                .commit_unchecked();

            if let Ok(_receipt) = store.transact_test(tx) {
                let new_len = store.len();
                prop_assert!(
                    new_len >= prev_len,
                    "INV-FERR-018: store shrank from {} to {} after transact {}",
                    prev_len, new_len, i
                );
                prev_len = new_len;
            }
        }
    }

    /// INV-FERR-018: merge never removes datoms.
    #[test]
    fn inv_ferr_018_merge_monotonic(
        a_seeds in prop::collection::vec(any::<[u8; 4]>(), 0..10),
        b_seeds in prop::collection::vec(any::<[u8; 4]>(), 0..10),
    ) {
        let mut set_a = BTreeSet::new();
        for seed in &a_seeds {
            set_a.insert(Datom::new(
                EntityId::from_content(seed),
                Attribute::from("db/doc"),
                Value::String(Arc::from("a")),
                TxId::new(1, 0, 0),
                Op::Assert,
            ));
        }

        let mut set_b = BTreeSet::new();
        for seed in &b_seeds {
            set_b.insert(Datom::new(
                EntityId::from_content(seed),
                Attribute::from("db/doc"),
                Value::String(Arc::from("b")),
                TxId::new(2, 0, 0),
                Op::Assert,
            ));
        }

        let a = Store::from_datoms(set_a);
        let b = Store::from_datoms(set_b);
        let merged = Store::from_merge(&a, &b);

        // INV-FERR-004: merged size >= max of inputs.
        prop_assert!(
            merged.len() >= a.len().max(b.len()),
            "INV-FERR-004: merged store smaller than inputs: {} < max({}, {})",
            merged.len(), a.len(), b.len()
        );

        // INV-FERR-018: every datom from A is in merged.
        for d in a.datoms() {
            prop_assert!(
                merged.datom_set().contains(d),
                "INV-FERR-018: datom from A missing in merged store"
            );
        }

        // INV-FERR-018: every datom from B is in merged.
        for d in b.datoms() {
            prop_assert!(
                merged.datom_set().contains(d),
                "INV-FERR-018: datom from B missing in merged store"
            );
        }
    }

    /// INV-FERR-018: retractions are datoms, not deletions.
    /// After retracting a fact, the store is strictly larger (retract datom added).
    #[test]
    fn inv_ferr_018_retract_adds_datom(
        entity_seed in any::<[u8; 32]>(),
    ) {
        let mut store = Store::genesis();
        let node = NodeId::from_bytes([5u8; 16]);
        let entity = EntityId::from_bytes(entity_seed);

        // Assert a fact.
        let tx1 = Transaction::new(node)
            .assert_datom(entity, Attribute::from("db/doc"), Value::String(Arc::from("hello")))
            .commit_unchecked();
        store.transact_test(tx1).unwrap();
        let len_after_assert = store.len();

        // Retract the same fact.
        let tx2 = Transaction::new(node)
            .retract_datom(entity, Attribute::from("db/doc"), Value::String(Arc::from("hello")))
            .commit_unchecked();
        store.transact_test(tx2).unwrap();
        let len_after_retract = store.len();

        // Store must be strictly larger (retract datom + tx metadata added).
        prop_assert!(
            len_after_retract > len_after_assert,
            "INV-FERR-018: retraction must add datoms, not remove: {} <= {}",
            len_after_retract, len_after_assert
        );
    }
}
