//! Topology and replica filter Kani harnesses.
//!
//! Covers INV-FERR-030: read replica subset property.

use std::sync::Arc;

use ferratom::{Attribute, Datom, EntityId, Op, TxId, Value};
use ferratomic_db::topology::{AcceptAll, ReplicaFilter};

#[cfg(not(kani))]
use super::kani;

/// Helper: build a datom with a given seed byte (INV-FERR-030).
fn make_datom(seed: u8) -> Datom {
    Datom::new(
        EntityId::from_content(&[seed]),
        Attribute::from("test/topology"),
        Value::String(Arc::from(format!("val-{seed}"))),
        TxId::new(u64::from(seed) + 1, 0, 0),
        Op::Assert,
    )
}

/// INV-FERR-030: AcceptAll.accepts() returns true for any datom.
///
/// The full-replica filter must never reject a datom. This is the
/// identity element for the replica filter composition.
#[cfg_attr(kani, kani::proof)]
#[cfg_attr(kani, kani::unwind(6))]
#[cfg_attr(not(kani), test)]
#[cfg_attr(not(kani), ignore = "requires Kani verifier")]
fn accept_all_never_rejects() {
    let filter = AcceptAll;

    for seed in 0..kani::any::<u8>().min(4) {
        let datom = make_datom(seed);
        assert!(
            filter.accepts(&datom),
            "INV-FERR-030: AcceptAll must accept datom with seed {}",
            seed
        );
    }
}

/// INV-FERR-030: AcceptAll accepts datoms with diverse value types.
///
/// Verifies the filter is value-type agnostic: String, Long, Bool,
/// and Ref values are all accepted.
#[cfg_attr(kani, kani::proof)]
#[cfg_attr(kani, kani::unwind(6))]
#[cfg_attr(not(kani), test)]
#[cfg_attr(not(kani), ignore = "requires Kani verifier")]
fn accept_all_diverse_values() {
    let filter = AcceptAll;
    let entity = EntityId::from_content(b"diverse");
    let tx = TxId::new(1, 0, 0);

    let string_datom = Datom::new(
        entity,
        Attribute::from("test/str"),
        Value::String(Arc::from("hello")),
        tx,
        Op::Assert,
    );
    let long_datom = Datom::new(
        entity,
        Attribute::from("test/long"),
        Value::Long(42),
        tx,
        Op::Assert,
    );
    let bool_datom = Datom::new(
        entity,
        Attribute::from("test/bool"),
        Value::Bool(true),
        tx,
        Op::Assert,
    );
    let ref_datom = Datom::new(
        entity,
        Attribute::from("test/ref"),
        Value::Ref(EntityId::from_content(b"target")),
        tx,
        Op::Assert,
    );

    assert!(
        filter.accepts(&string_datom),
        "INV-FERR-030: must accept String"
    );
    assert!(
        filter.accepts(&long_datom),
        "INV-FERR-030: must accept Long"
    );
    assert!(
        filter.accepts(&bool_datom),
        "INV-FERR-030: must accept Bool"
    );
    assert!(filter.accepts(&ref_datom), "INV-FERR-030: must accept Ref");
}

/// INV-FERR-030: a filter returning false excludes datoms from the replica.
///
/// Verifies the subset property: when AcceptAll is the identity, any
/// filtering logic that returns false for a datom means that datom is
/// NOT in the replica's subset. We model this by checking that a
/// datom which AcceptAll accepts can be conditionally excluded.
#[cfg_attr(kani, kani::proof)]
#[cfg_attr(kani, kani::unwind(6))]
#[cfg_attr(not(kani), test)]
#[cfg_attr(not(kani), ignore = "requires Kani verifier")]
fn filter_subset_property() {
    let filter = AcceptAll;
    let included = make_datom(1);
    let excluded = make_datom(2);

    // AcceptAll accepts both.
    assert!(
        filter.accepts(&included),
        "INV-FERR-030: AcceptAll must accept included"
    );
    assert!(
        filter.accepts(&excluded),
        "INV-FERR-030: AcceptAll must accept excluded"
    );

    // Model a subset: only datoms passing a predicate are in the replica.
    // This predicate excludes seed=2 datoms.
    let predicate = |d: &Datom| -> bool { d.entity() != EntityId::from_content(&[2u8]) };

    let included_in_replica = filter.accepts(&included) && predicate(&included);
    let excluded_from_replica = filter.accepts(&excluded) && predicate(&excluded);

    assert!(
        included_in_replica,
        "INV-FERR-030: included datom must pass both checks"
    );
    assert!(
        !excluded_from_replica,
        "INV-FERR-030: excluded datom must fail the predicate"
    );
}

/// INV-FERR-030: AcceptAll is Send + Sync.
///
/// Required by the ReplicaFilter trait bound. This harness verifies
/// the compile-time property by exercising the trait object.
#[cfg_attr(kani, kani::proof)]
#[cfg_attr(kani, kani::unwind(4))]
#[cfg_attr(not(kani), test)]
#[cfg_attr(not(kani), ignore = "requires Kani verifier")]
fn accept_all_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<AcceptAll>();

    // Also verify it works as a trait object.
    let filter: Box<dyn ReplicaFilter> = Box::new(AcceptAll);
    let datom = make_datom(0);
    assert!(
        filter.accepts(&datom),
        "INV-FERR-030: boxed AcceptAll must accept datom"
    );
}
