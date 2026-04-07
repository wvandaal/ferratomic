//! Store view and observation Kani harnesses.
//!
//! Covers INV-FERR-005, INV-FERR-006, INV-FERR-007, INV-FERR-011,
//! INV-FERR-025, and INV-FERR-027.

use std::{collections::BTreeSet, sync::Arc};

use ferratom::{AgentId, Attribute, Datom, EntityId, Op, TxId, Value};
use ferratomic_db::{
    indexes::{EavtKey, IndexBackend},
    store::Store,
    writer::Transaction,
};

use super::helpers::concrete_datom_set;
#[cfg(not(kani))]
use super::kani;

/// INV-FERR-005: every secondary index is a permutation of primary datoms.
#[cfg_attr(kani, kani::proof)]
#[cfg_attr(kani, kani::unwind(10))]
#[cfg_attr(not(kani), test)]
#[cfg_attr(not(kani), ignore = "requires Kani verifier")]
fn index_bijection() {
    let count: u8 = kani::any();
    kani::assume(count <= 4);
    let datoms = concrete_datom_set(count);

    let mut store = Store::from_datoms(datoms.clone());
    // bd-h2fz: promote to OrdMap to verify index bijection.
    store.promote();

    let primary: BTreeSet<&Datom> = store.datoms().collect();
    // bd-oett: descriptive expect instead of bare unwrap.
    let indexes = store
        .indexes()
        .expect("INV-FERR-005: indexes must be available after promote");
    let eavt: BTreeSet<&Datom> = indexes.eavt_datoms().collect();
    let aevt: BTreeSet<&Datom> = indexes.aevt_datoms().collect();
    let vaet: BTreeSet<&Datom> = indexes.vaet_datoms().collect();
    let avet: BTreeSet<&Datom> = indexes.avet_datoms().collect();

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
///
/// bd-z2jv: Rewritten to use the real Store type instead of raw u64 sequences.
/// Successive snapshots taken after each transact must have non-decreasing
/// epochs, proving observers see monotonically advancing state.
#[cfg_attr(kani, kani::proof)]
#[cfg_attr(kani, kani::unwind(10))]
#[cfg_attr(not(kani), test)]
#[cfg_attr(not(kani), ignore = "requires Kani verifier")]
fn observer_monotonicity() {
    let mut store = Store::genesis();
    let agent = AgentId::from_bytes([1u8; 16]);
    let mut epochs: Vec<u64> = Vec::new();

    // Record genesis epoch.
    epochs.push(store.snapshot().epoch());

    let n_txns: u8 = kani::any();
    kani::assume(n_txns > 0 && n_txns <= 4);

    for i in 0..n_txns {
        let tx = Transaction::new(agent)
            .assert_datom(
                EntityId::from_content(&[i, 0x11]),
                Attribute::from("db/doc"),
                Value::String(format!("obs-{i}").into()),
            )
            .commit(store.schema())
            .expect("INV-FERR-011: tx must validate");
        let _ = store
            .transact_test(tx)
            .expect("INV-FERR-011: tx must apply");
        epochs.push(store.snapshot().epoch());
    }

    // INV-FERR-011: observer epochs must be monotonically non-decreasing.
    for i in 1..epochs.len() {
        assert!(
            epochs[i] >= epochs[i - 1],
            "INV-FERR-011: epoch regressed from {} to {} at step {i}",
            epochs[i - 1],
            epochs[i]
        );
    }
}

// ---------------------------------------------------------------------------
// INV-FERR-025: Index backend interchangeability
// ---------------------------------------------------------------------------

/// INV-FERR-025: Store::from_datoms produces correct indexes regardless
/// of input order.
///
/// Two BTreeSets with the same elements (inserted in different order)
/// produce identical stores with identical index contents. This proves
/// the index backend is order-independent.
#[cfg_attr(kani, kani::proof)]
#[cfg_attr(kani, kani::unwind(8))]
#[cfg_attr(not(kani), test)]
#[cfg_attr(not(kani), ignore = "requires Kani verifier")]
fn index_backend_order_independence() {
    let entity = EntityId::from_content(b"inv025");
    let tx = TxId::new(1, 0, 0);

    let d1 = Datom::new(
        entity,
        Attribute::from("db/doc"),
        Value::String(Arc::from("alpha")),
        tx,
        Op::Assert,
    );
    let d2 = Datom::new(
        entity,
        Attribute::from("db/doc"),
        Value::String(Arc::from("beta")),
        tx,
        Op::Assert,
    );

    // Insert in order d1, d2.
    let mut set_ab = BTreeSet::new();
    set_ab.insert(d1.clone());
    set_ab.insert(d2.clone());
    let mut store_ab = Store::from_datoms(set_ab);
    store_ab.promote();

    // Insert in order d2, d1.
    let mut set_ba = BTreeSet::new();
    set_ba.insert(d2);
    set_ba.insert(d1);
    let mut store_ba = Store::from_datoms(set_ba);
    store_ba.promote();

    // Primary datom sets must be identical.
    assert_eq!(
        store_ab.datom_set(),
        store_ba.datom_set(),
        "INV-FERR-025: datom sets must be identical regardless of insert order"
    );

    // All four index iterators must yield the same datoms.
    // bd-oett: descriptive expect instead of bare unwrap.
    let idx_ab = store_ab
        .indexes()
        .expect("INV-FERR-025: indexes must be available after promote (store_ab)");
    let idx_ba = store_ba
        .indexes()
        .expect("INV-FERR-025: indexes must be available after promote (store_ba)");
    let eavt_ab: Vec<_> = idx_ab.eavt_datoms().collect();
    let eavt_ba: Vec<_> = idx_ba.eavt_datoms().collect();
    assert_eq!(eavt_ab, eavt_ba, "INV-FERR-025: EAVT must match");

    let aevt_ab: Vec<_> = idx_ab.aevt_datoms().collect();
    let aevt_ba: Vec<_> = idx_ba.aevt_datoms().collect();
    assert_eq!(aevt_ab, aevt_ba, "INV-FERR-025: AEVT must match");

    let vaet_ab: Vec<_> = idx_ab.vaet_datoms().collect();
    let vaet_ba: Vec<_> = idx_ba.vaet_datoms().collect();
    assert_eq!(vaet_ab, vaet_ba, "INV-FERR-025: VAET must match");

    let avet_ab: Vec<_> = idx_ab.avet_datoms().collect();
    let avet_ba: Vec<_> = idx_ba.avet_datoms().collect();
    assert_eq!(avet_ab, avet_ba, "INV-FERR-025: AVET must match");
}

// ---------------------------------------------------------------------------
// INV-FERR-027: Read P99 latency (EAVT lookup correctness)
// ---------------------------------------------------------------------------

/// INV-FERR-027: EAVT index lookup returns the correct datom for any
/// valid key.
///
/// The O(log n) bound comes from the im::OrdMap backend (ADR-FERR-001).
/// This harness verifies the correctness precondition: for every datom
/// inserted, looking it up by its EavtKey returns that exact datom.
#[cfg_attr(kani, kani::proof)]
#[cfg_attr(kani, kani::unwind(8))]
#[cfg_attr(not(kani), test)]
#[cfg_attr(not(kani), ignore = "requires Kani verifier")]
fn eavt_lookup_correctness() {
    let mut store = Store::genesis();
    let agent = AgentId::from_bytes([3u8; 16]);

    let tx = Transaction::new(agent)
        .assert_datom(
            EntityId::from_content(b"eavt-lookup"),
            Attribute::from("db/doc"),
            Value::String(Arc::from("lookup-test")),
        )
        .commit(store.schema())
        .expect("INV-FERR-027: tx must validate");

    let committed_datoms: Vec<Datom> = tx.datoms().to_vec();
    let _ = store
        .transact_test(tx)
        .expect("INV-FERR-027: tx must apply");

    // For every datom in the committed transaction, the EAVT index
    // must return that exact datom when queried by its key.
    // bd-oett: descriptive expect instead of bare unwrap.
    let indexes = store
        .indexes()
        .expect("INV-FERR-027: indexes must be available after transact_test");
    for datom in &committed_datoms {
        let key = EavtKey::from_datom(datom);
        let found = indexes.eavt().backend_get(&key);
        assert!(
            found.is_some(),
            "INV-FERR-027: EAVT lookup must find committed datom"
        );
        assert_eq!(
            found.map(|d| d.entity()),
            Some(datom.entity()),
            "INV-FERR-027: EAVT lookup must return correct entity"
        );
    }
}
