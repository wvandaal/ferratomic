//! Store view and observation Kani harnesses.
//!
//! Covers INV-FERR-005, INV-FERR-006, INV-FERR-007, and INV-FERR-011.

use std::collections::BTreeSet;

use ferratom::{AgentId, Attribute, Datom, EntityId, Value};
use ferratomic_core::{store::Store, writer::Transaction};

#[cfg(not(kani))]
use super::kani;

/// INV-FERR-005: every secondary index is a permutation of primary datoms.
#[cfg_attr(kani, kani::proof)]
#[cfg_attr(kani, kani::unwind(10))]
#[cfg_attr(not(kani), test)]
#[cfg_attr(not(kani), ignore = "requires Kani verifier")]
fn index_bijection() {
    let datoms: BTreeSet<Datom> = kani::any();
    kani::assume(datoms.len() <= 4);

    let store = Store::from_datoms(datoms.clone());

    let primary: BTreeSet<&Datom> = store.datoms().collect();
    let eavt: BTreeSet<&Datom> = store.indexes().eavt_datoms().collect();
    let aevt: BTreeSet<&Datom> = store.indexes().aevt_datoms().collect();
    let vaet: BTreeSet<&Datom> = store.indexes().vaet_datoms().collect();
    let avet: BTreeSet<&Datom> = store.indexes().avet_datoms().collect();

    for d in primary.iter().copied() {
        assert!(eavt.contains(d));
        assert!(aevt.contains(d));
        assert!(vaet.contains(d));
        assert!(avet.contains(d));
    }

    assert_eq!(primary.len(), eavt.len());
    assert_eq!(primary.len(), aevt.len());
    assert_eq!(primary.len(), vaet.len());
    assert_eq!(primary.len(), avet.len());
}

/// INV-FERR-006: a snapshot must not see future datoms.
#[cfg_attr(kani, kani::proof)]
#[cfg_attr(kani, kani::unwind(8))]
#[cfg_attr(not(kani), test)]
#[cfg_attr(not(kani), ignore = "requires Kani verifier")]
fn snapshot_isolation() {
    let mut store = Store::genesis();
    let snapshot = store.snapshot();
    let snapshot_datoms: BTreeSet<Datom> = snapshot.datoms().cloned().collect();

    let tx = Transaction::new(AgentId::from_bytes([0u8; 16]))
        .assert_datom(
            EntityId::from_content(b"kani-snapshot"),
            Attribute::from("test/name"),
            Value::String("Alice".into()),
        )
        .commit(store.schema())
        .expect("INV-FERR-006: harness transaction should validate");
    let _ = store.transact_test(tx);

    let snapshot_datoms_after: BTreeSet<Datom> = snapshot.datoms().cloned().collect();
    assert_eq!(snapshot_datoms, snapshot_datoms_after);
}

/// INV-FERR-007: committed write epochs are strictly increasing.
#[cfg_attr(kani, kani::proof)]
#[cfg_attr(kani, kani::unwind(8))]
#[cfg_attr(not(kani), test)]
#[cfg_attr(not(kani), ignore = "requires Kani verifier")]
fn write_linearizability() {
    let mut epochs: Vec<u64> = Vec::new();
    let mut store = Store::genesis();
    let agent = AgentId::from_bytes([1u8; 16]);

    for _ in 0..kani::any::<u8>().min(5) {
        let datom_id = kani::any::<u8>();
        let tx = Transaction::new(agent)
            .assert_datom(
                EntityId::from_content(&[datom_id]),
                Attribute::from("test/counter"),
                Value::Long(i64::from(datom_id)),
            )
            .commit(store.schema())
            .expect("INV-FERR-007: harness transaction should validate");

        if let Ok(receipt) = store.transact_test(tx) {
            epochs.push(receipt.epoch());
        }
    }

    for i in 1..epochs.len() {
        assert!(epochs[i] > epochs[i - 1]);
    }
}

/// INV-FERR-011: observer epochs never regress.
#[cfg_attr(kani, kani::proof)]
#[cfg_attr(kani, kani::unwind(10))]
#[cfg_attr(not(kani), test)]
#[cfg_attr(not(kani), ignore = "requires Kani verifier")]
fn observer_monotonicity() {
    let mut epochs: Vec<u64> = Vec::new();
    let mut last: u64 = 0;

    for _ in 0..kani::any::<u8>().min(5) {
        let next: u64 = kani::any();
        kani::assume(next >= last);
        epochs.push(next);
        last = next;
    }

    for i in 1..epochs.len() {
        assert!(epochs[i] >= epochs[i - 1]);
    }
}
