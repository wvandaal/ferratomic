//! Live resolution Kani harnesses.
//!
//! Covers INV-FERR-029 (retraction removes from live view) and
//! INV-FERR-032 (live query returns only non-retracted datoms).
//!
//! The Store does not yet expose a `live_view()` method. These harnesses
//! model the live view computation inline using the spec definition:
//!
//! ```text
//! live(store) = { (e, a, v) | exists Assert(e, a, v, t) in store
//!                              AND NOT exists Retract(e, a, v, t') in store
//!                              where t' > t }
//! ```
//!
//! These harnesses use fixed fixture `EntityId`s rather than calling
//! `EntityId::from_content`. That keeps the proof target on LIVE semantics;
//! INV-FERR-012 content-addressing remains a separate proof obligation.
//!
//! For Kani tractability, the model is simplified: for each (entity, attribute,
//! value) triple, if there exists ANY retraction in the datom set, that
//! triple is removed from the live view. This is sound for the bounded
//! harnesses below because each triple is asserted and retracted at most once.

use std::collections::{BTreeMap, BTreeSet};

use ferratom::{Attribute, Datom, EntityId, Op, TxId, Value};
use ferratomic_core::store::select_latest_live_value_for_test;

use super::helpers::proof_entity_id;
#[cfg(not(kani))]
use super::kani;

/// Compute the live view from a set of datoms.
///
/// For each (entity, attribute, value) triple, the live view contains
/// the triple if and only if the latest operation (by TxId) is Assert.
///
/// INV-FERR-029: a retraction with a later TxId than any assertion
/// removes the triple from the live set.
/// INV-FERR-032: only non-retracted triples appear in the result.
fn live_view(datoms: &BTreeSet<Datom>) -> BTreeSet<(EntityId, Attribute, Value)> {
    // Group by (entity, attribute, value), keep the latest (TxId, Op).
    let mut latest: BTreeMap<(EntityId, Attribute, Value), (TxId, Op)> = BTreeMap::new();

    for d in datoms {
        let key = (d.entity(), d.attribute().clone(), d.value().clone());
        let entry = latest.entry(key).or_insert((d.tx(), d.op()));
        if d.tx() > entry.0 {
            *entry = (d.tx(), d.op());
        }
    }

    latest
        .into_iter()
        .filter(|(_, (_, op))| *op == Op::Assert)
        .map(|(key, _)| key)
        .collect()
}

/// INV-FERR-029: a retraction removes a datom from the live view.
///
/// Assert a datom, then retract it at a later TxId. The live view
/// must not contain the retracted triple.
#[cfg_attr(kani, kani::proof)]
#[cfg_attr(kani, kani::unwind(6))]
#[cfg_attr(not(kani), test)]
#[cfg_attr(not(kani), ignore = "requires Kani verifier")]
fn retraction_removes_from_live_view() {
    let entity = proof_entity_id(0x29);
    let attr = Attribute::from("a");
    let val = Value::Long(1);

    let assert_tx = TxId::new(1, 0, 0);
    let retract_tx = TxId::new(2, 0, 0);

    let assert_datom = Datom::new(entity, attr.clone(), val.clone(), assert_tx, Op::Assert);
    let retract_datom = Datom::new(entity, attr.clone(), val.clone(), retract_tx, Op::Retract);

    let mut datoms = BTreeSet::new();
    datoms.insert(assert_datom);
    datoms.insert(retract_datom);

    let live = live_view(&datoms);

    // The retracted triple must NOT be in the live view.
    assert!(
        !live.contains(&(entity, attr, val)),
        "INV-FERR-029: retracted datom must not appear in live view"
    );
    assert!(
        live.is_empty(),
        "INV-FERR-029: live view must be empty after retraction"
    );
}

/// INV-FERR-029: retraction of one triple does not affect others.
///
/// Assert two triples, retract one. The other must remain live.
#[cfg_attr(kani, kani::proof)]
#[cfg_attr(kani, kani::unwind(8))]
#[cfg_attr(not(kani), test)]
#[cfg_attr(not(kani), ignore = "requires Kani verifier")]
fn retraction_is_targeted() {
    let e1 = proof_entity_id(0x31);
    let e2 = proof_entity_id(0x32);
    let attr = Attribute::from("a");
    let val1 = Value::Long(1);
    let val2 = Value::Long(2);

    let t1 = TxId::new(1, 0, 0);
    let t2 = TxId::new(2, 0, 0);

    let mut datoms = BTreeSet::new();
    // Assert both
    datoms.insert(Datom::new(e1, attr.clone(), val1.clone(), t1, Op::Assert));
    datoms.insert(Datom::new(e2, attr.clone(), val2.clone(), t1, Op::Assert));
    // Retract only e1
    datoms.insert(Datom::new(e1, attr.clone(), val1.clone(), t2, Op::Retract));

    let live = live_view(&datoms);

    assert!(
        !live.contains(&(e1, attr.clone(), val1)),
        "INV-FERR-029: retracted triple must be absent from live view"
    );
    assert!(
        live.contains(&(e2, attr, val2)),
        "INV-FERR-029: non-retracted triple must remain in live view"
    );
}

/// INV-FERR-032: live query returns only non-retracted datoms.
///
/// Symbolic: for a bounded set of datoms, every triple in the live view
/// must have its latest operation be Assert (not Retract).
#[cfg_attr(kani, kani::proof)]
#[cfg_attr(kani, kani::unwind(6))]
#[cfg_attr(not(kani), test)]
#[cfg_attr(not(kani), ignore = "requires Kani verifier")]
fn live_view_contains_only_asserted() {
    let entity = proof_entity_id(0x33);
    let attr = Attribute::from("a");

    // Symbolic choice: the operation at t=1 and t=2.
    let op1_raw: bool = kani::any();
    let op2_raw: bool = kani::any();
    let op1 = if op1_raw { Op::Assert } else { Op::Retract };
    let op2 = if op2_raw { Op::Assert } else { Op::Retract };

    let val = Value::Long(42);
    let t1 = TxId::new(1, 0, 0);
    let t2 = TxId::new(2, 0, 0);

    let mut datoms = BTreeSet::new();
    datoms.insert(Datom::new(entity, attr.clone(), val.clone(), t1, op1));
    datoms.insert(Datom::new(entity, attr.clone(), val.clone(), t2, op2));

    let live = live_view(&datoms);
    let triple = (entity, attr, val);

    // INV-FERR-032: the triple is in the live set if and only if
    // the latest operation (t2, since t2 > t1) is Assert.
    if op2 == Op::Assert {
        assert!(
            live.contains(&triple),
            "INV-FERR-032: triple with latest Assert must be in live view"
        );
    } else {
        assert!(
            !live.contains(&triple),
            "INV-FERR-032: triple with latest Retract must not be in live view"
        );
    }
}

/// INV-FERR-032: card-one resolution picks the highest surviving `TxId`.
///
/// This harness targets the exact selection kernel used by
/// `Store::live_resolve`. First, the newest asserted value must win. Then,
/// after retracting that newest value, resolution must fall back to the
/// next-highest surviving assertion.
#[cfg_attr(kani, kani::proof)]
#[cfg_attr(kani, kani::unwind(6))]
#[cfg_attr(not(kani), test)]
#[cfg_attr(not(kani), ignore = "requires Kani verifier")]
fn select_latest_live_value_lww_semantics() {
    let older_value = Value::Long(1);
    let newer_value = Value::Long(2);

    assert_eq!(
        select_latest_live_value_for_test(&[
            (older_value.clone(), (TxId::new(1, 0, 0), Op::Assert)),
            (newer_value.clone(), (TxId::new(2, 0, 0), Op::Assert)),
        ]),
        Some(&newer_value),
        "INV-FERR-032: highest surviving TxId must win for card-one resolution"
    );

    assert_eq!(
        select_latest_live_value_for_test(&[
            (older_value.clone(), (TxId::new(1, 0, 0), Op::Assert)),
            (newer_value.clone(), (TxId::new(2, 0, 0), Op::Retract)),
        ]),
        Some(&older_value),
        "INV-FERR-032: retracting the head value must reveal the next-highest surviving assert"
    );
}
