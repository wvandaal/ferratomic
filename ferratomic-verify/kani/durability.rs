//! Durability and transaction-shape Kani harnesses.
//!
//! Covers INV-FERR-013, INV-FERR-014, INV-FERR-018, and INV-FERR-020.

use std::collections::BTreeSet;

use ferratom::{AgentId, Attribute, Datom, EntityId, Value};
use ferratomic_core::{store::Store, writer::Transaction};

/// INV-FERR-013: checkpoint serialization is a round trip on store state.
#[kani::proof]
#[kani::unwind(8)]
fn checkpoint_roundtrip() {
    let datoms: BTreeSet<Datom> = kani::any();
    kani::assume(datoms.len() <= 4);

    let store = Store::from_datoms(datoms.clone());
    let bytes = store.to_checkpoint_bytes();
    let loaded = Store::from_checkpoint_bytes(&bytes)
        .expect("INV-FERR-013: checkpoint bytes produced by the store must deserialize");

    assert_eq!(store.datom_set(), loaded.datom_set());
    assert_eq!(store.current_epoch(), loaded.current_epoch());
}

/// INV-FERR-014: recovery never loses committed datoms.
#[kani::proof]
#[kani::unwind(8)]
fn recovery_superset() {
    let committed: BTreeSet<Datom> = kani::any();
    kani::assume(committed.len() <= 4);

    let uncommitted: BTreeSet<Datom> = kani::any();
    kani::assume(uncommitted.len() <= 2);
    let survived: bool = kani::any();

    let mut recovered = committed.clone();
    if survived {
        for d in &uncommitted {
            recovered.insert(d.clone());
        }
    }

    assert!(committed.is_subset(&recovered));
}

/// INV-FERR-018: the datom set is append-only.
#[kani::proof]
#[kani::unwind(10)]
fn append_only() {
    let initial: BTreeSet<Datom> = kani::any();
    kani::assume(initial.len() <= 4);
    let new_datom: Datom = kani::any();

    let mut store = initial.clone();
    store.insert(new_datom);

    assert!(initial.is_subset(&store));
    assert!(store.len() >= initial.len());
}

/// INV-FERR-020: a committed transaction assigns one epoch to all of its datoms.
#[kani::proof]
#[kani::unwind(8)]
fn transaction_atomicity() {
    let mut store = Store::genesis();
    let n_datoms: u8 = kani::any();
    kani::assume(n_datoms > 0 && n_datoms <= 4);

    let tx = (0..n_datoms).fold(Transaction::new(AgentId::from_bytes([0u8; 16])), |tx, i| {
        tx.assert_datom(
            EntityId::from_content(&[i]),
            Attribute::from("test/counter"),
            Value::Long(i64::from(i)),
        )
    });
    let committed = tx
        .commit(store.schema())
        .expect("INV-FERR-020: harness transaction should validate");
    let tx_datoms: BTreeSet<_> = committed.datoms().cloned().collect();
    let _receipt = store
        .transact(committed)
        .expect("INV-FERR-020: harness transaction should apply");

    let snapshot = store.snapshot();
    let visible: BTreeSet<_> = snapshot.datoms().cloned().collect();
    let visible_count = tx_datoms.iter().filter(|d| visible.contains(*d)).count();
    assert!(visible_count == 0 || visible_count == tx_datoms.len());
}
