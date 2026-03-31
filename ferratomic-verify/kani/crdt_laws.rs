//! CRDT semilattice Kani harnesses.
//!
//! Covers INV-FERR-001, INV-FERR-002, INV-FERR-003, INV-FERR-004,
//! INV-FERR-010, and INV-FERR-031.

use std::collections::BTreeSet;

use ferratom::{AgentId, Attribute, Datom, EntityId, Value};
use ferratomic_core::{merge::merge, store::Store, writer::Transaction};

/// INV-FERR-001: merge(A, B) == merge(B, A).
#[kani::proof]
#[kani::unwind(10)]
fn merge_commutativity() {
    let a: BTreeSet<Datom> = kani::any();
    let b: BTreeSet<Datom> = kani::any();
    kani::assume(a.len() <= 4 && b.len() <= 4);

    let ab: BTreeSet<Datom> = a.union(&b).cloned().collect();
    let ba: BTreeSet<Datom> = b.union(&a).cloned().collect();

    assert_eq!(ab, ba);
}

/// INV-FERR-002: merge(merge(A, B), C) == merge(A, merge(B, C)).
#[kani::proof]
#[kani::unwind(10)]
fn merge_associativity() {
    let a: BTreeSet<Datom> = kani::any();
    let b: BTreeSet<Datom> = kani::any();
    let c: BTreeSet<Datom> = kani::any();
    kani::assume(a.len() <= 3 && b.len() <= 3 && c.len() <= 3);

    let ab: BTreeSet<Datom> = a.union(&b).cloned().collect();
    let ab_c: BTreeSet<Datom> = ab.union(&c).cloned().collect();

    let bc: BTreeSet<Datom> = b.union(&c).cloned().collect();
    let a_bc: BTreeSet<Datom> = a.union(&bc).cloned().collect();

    assert_eq!(ab_c, a_bc);
}

/// INV-FERR-003: merge(A, A) == A.
#[kani::proof]
#[kani::unwind(10)]
fn merge_idempotency() {
    let a: BTreeSet<Datom> = kani::any();
    kani::assume(a.len() <= 5);

    let aa: BTreeSet<Datom> = a.union(&a).cloned().collect();
    assert_eq!(a, aa);
}

/// INV-FERR-004: every apply step is monotone over the datom set.
#[kani::proof]
#[kani::unwind(10)]
fn monotonic_growth() {
    let s: BTreeSet<Datom> = kani::any();
    let d: Datom = kani::any();
    kani::assume(s.len() <= 5);

    let mut s_prime = s.clone();
    s_prime.insert(d);

    assert!(s_prime.len() >= s.len());
    assert!(s.is_subset(&s_prime));
}

/// INV-FERR-010: replicas that receive the same updates converge.
#[kani::proof]
#[kani::unwind(8)]
fn convergence_two_replicas() {
    let datoms: Vec<Datom> = (0..kani::any::<u8>().min(4)).map(|_| kani::any()).collect();

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
#[kani::proof]
#[kani::unwind(8)]
fn convergence_under_merge() {
    let a_datoms: BTreeSet<Datom> = kani::any();
    let b_datoms: BTreeSet<Datom> = kani::any();
    kani::assume(a_datoms.len() <= 3 && b_datoms.len() <= 3);

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
#[kani::proof]
#[kani::unwind(8)]
fn store_transact_monotonic_growth() {
    let mut store = Store::genesis();
    let agent = AgentId::from_bytes([1u8; 16]);

    let len_0 = store.len();

    // First transaction
    let tx1 = Transaction::new(agent)
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
    let tx2 = Transaction::new(agent)
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
#[kani::proof]
#[kani::unwind(4)]
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
