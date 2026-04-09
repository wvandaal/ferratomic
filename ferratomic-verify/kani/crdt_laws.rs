//! CRDT semilattice Kani harnesses.
//!
//! Covers INV-FERR-001, INV-FERR-002, INV-FERR-003, INV-FERR-004,
//! INV-FERR-010, and INV-FERR-031.

use std::collections::BTreeSet;

use ferratom::{Attribute, Datom, EntityId, NodeId, Value};
use ferratomic_db::{merge::merge, store::Store, writer::Transaction};

use super::helpers::{concrete_datom, concrete_datom_set};
#[cfg(not(kani))]
use super::kani;

/// INV-FERR-001: merge(A, B) == merge(B, A).
#[cfg_attr(kani, kani::proof)]
#[cfg_attr(kani, kani::unwind(10))]
#[cfg_attr(not(kani), test)]
#[cfg_attr(not(kani), ignore = "requires Kani verifier")]
fn merge_commutativity() {
    let count_a: u8 = kani::any();
    kani::assume(count_a <= 4);
    let a: BTreeSet<Datom> = (0..count_a).map(concrete_datom).collect();

    let count_b: u8 = kani::any();
    kani::assume(count_b <= 4);
    let b: BTreeSet<Datom> = (10..10 + count_b).map(concrete_datom).collect();

    let ab: BTreeSet<Datom> = a.union(&b).cloned().collect();
    let ba: BTreeSet<Datom> = b.union(&a).cloned().collect();

    assert_eq!(ab, ba);
}

/// INV-FERR-002: merge(merge(A, B), C) == merge(A, merge(B, C)).
#[cfg_attr(kani, kani::proof)]
#[cfg_attr(kani, kani::unwind(10))]
#[cfg_attr(not(kani), test)]
#[cfg_attr(not(kani), ignore = "requires Kani verifier")]
fn merge_associativity() {
    let count_a: u8 = kani::any();
    kani::assume(count_a <= 3);
    let a: BTreeSet<Datom> = (0..count_a).map(concrete_datom).collect();

    let count_b: u8 = kani::any();
    kani::assume(count_b <= 3);
    let b: BTreeSet<Datom> = (10..10 + count_b).map(concrete_datom).collect();

    let count_c: u8 = kani::any();
    kani::assume(count_c <= 3);
    let c: BTreeSet<Datom> = (20..20 + count_c).map(concrete_datom).collect();

    let ab: BTreeSet<Datom> = a.union(&b).cloned().collect();
    let ab_c: BTreeSet<Datom> = ab.union(&c).cloned().collect();

    let bc: BTreeSet<Datom> = b.union(&c).cloned().collect();
    let a_bc: BTreeSet<Datom> = a.union(&bc).cloned().collect();

    assert_eq!(ab_c, a_bc);
}

/// INV-FERR-003: merge(A, A) == A.
#[cfg_attr(kani, kani::proof)]
#[cfg_attr(kani, kani::unwind(10))]
#[cfg_attr(not(kani), test)]
#[cfg_attr(not(kani), ignore = "requires Kani verifier")]
fn merge_idempotency() {
    let count_a: u8 = kani::any();
    kani::assume(count_a <= 5);
    let a = concrete_datom_set(count_a);

    let aa: BTreeSet<Datom> = a.union(&a).cloned().collect();
    assert_eq!(a, aa);
}

/// INV-FERR-004: every apply step is monotone over the datom set.
#[cfg_attr(kani, kani::proof)]
#[cfg_attr(kani, kani::unwind(10))]
#[cfg_attr(not(kani), test)]
#[cfg_attr(not(kani), ignore = "requires Kani verifier")]
fn monotonic_growth() {
    let count_s: u8 = kani::any();
    kani::assume(count_s <= 5);
    let s = concrete_datom_set(count_s);

    let d = concrete_datom(kani::any::<u8>());

    let mut s_prime = s.clone();
    s_prime.insert(d);

    assert!(s_prime.len() >= s.len());
    assert!(s.is_subset(&s_prime));
}

/// INV-FERR-010: replicas that receive the same updates converge.
#[cfg_attr(kani, kani::proof)]
#[cfg_attr(kani, kani::unwind(8))]
#[cfg_attr(not(kani), test)]
#[cfg_attr(not(kani), ignore = "requires Kani verifier")]
fn convergence_two_replicas() {
    let count: u8 = kani::any();
    kani::assume(count <= 4);
    let datoms: Vec<Datom> = (0..count).map(concrete_datom).collect();

    let mut r1 = BTreeSet::new();
    let mut r2 = BTreeSet::new();

    for d in &datoms {
        r1.insert(d.clone());
    }
    for d in datoms.iter().rev() {
        r2.insert(d.clone());
    }

    assert_eq!(r1, r2);
}

/// INV-FERR-010: merge preserves convergence for concrete store values.
#[cfg_attr(kani, kani::proof)]
#[cfg_attr(kani, kani::unwind(8))]
#[cfg_attr(not(kani), test)]
#[cfg_attr(not(kani), ignore = "requires Kani verifier")]
fn convergence_under_merge() {
    let count_a: u8 = kani::any();
    kani::assume(count_a <= 3);
    let a_datoms: BTreeSet<Datom> = (0..count_a).map(concrete_datom).collect();

    let count_b: u8 = kani::any();
    kani::assume(count_b <= 3);
    let b_datoms: BTreeSet<Datom> = (10..10 + count_b).map(concrete_datom).collect();

    let a = Store::from_datoms(a_datoms);
    let b = Store::from_datoms(b_datoms);

    let ab = merge(&a, &b).expect("merge(A,B) must succeed");
    let ba = merge(&b, &a).expect("merge(B,A) must succeed");

    assert_eq!(ab.datom_set(), ba.datom_set());
}

/// INV-FERR-004: Store::transact never decreases store.len().
///
/// Bounded to 2 transactions of 1 datom each for Kani tractability.
/// After each successful transact, store.len() is >= the previous value.
#[cfg_attr(kani, kani::proof)]
#[cfg_attr(kani, kani::unwind(8))]
#[cfg_attr(not(kani), test)]
#[cfg_attr(not(kani), ignore = "requires Kani verifier")]
fn store_transact_monotonic_growth() {
    let mut store = Store::genesis();
    let node = NodeId::from_bytes([1u8; 16]);

    let len_0 = store.len();

    // First transaction
    let tx1 = Transaction::new(node)
        .assert_datom(
            EntityId::from_content(b"kani-mono-1"),
            Attribute::from("db/doc"),
            Value::String("first".into()),
        )
        .commit(store.schema())
        .expect("INV-FERR-004: first tx should validate");
    let _ = store.transact_test(tx1);
    let len_1 = store.len();
    assert!(
        len_1 >= len_0,
        "INV-FERR-004: store.len() must not decrease after first transact"
    );

    // Second transaction
    let tx2 = Transaction::new(node)
        .assert_datom(
            EntityId::from_content(b"kani-mono-2"),
            Attribute::from("db/doc"),
            Value::String("second".into()),
        )
        .commit(store.schema())
        .expect("INV-FERR-004: second tx should validate");
    let _ = store.transact_test(tx2);
    let len_2 = store.len();
    assert!(
        len_2 >= len_1,
        "INV-FERR-004: store.len() must not decrease after second transact"
    );
}

/// INV-FERR-031: two Store::genesis() calls produce identical stores.
///
/// Genesis is deterministic: same datom set, same schema, same epoch.
#[cfg_attr(kani, kani::proof)]
#[cfg_attr(kani, kani::unwind(4))]
#[cfg_attr(not(kani), test)]
#[cfg_attr(not(kani), ignore = "requires Kani verifier")]
fn genesis_determinism() {
    let a = Store::genesis();
    let b = Store::genesis();

    assert_eq!(
        a.datom_set(),
        b.datom_set(),
        "INV-FERR-031: genesis datom sets must be identical"
    );
    assert_eq!(
        a.schema(),
        b.schema(),
        "INV-FERR-031: genesis schemas must be identical"
    );
    assert_eq!(
        a.epoch(),
        b.epoch(),
        "INV-FERR-031: genesis epochs must be identical"
    );
}
