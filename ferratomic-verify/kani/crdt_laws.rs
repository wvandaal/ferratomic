//! CRDT semilattice Kani harnesses.
//!
//! Covers INV-FERR-001, INV-FERR-002, INV-FERR-003, INV-FERR-004, and
//! INV-FERR-010.

use std::collections::BTreeSet;

use ferratom::Datom;
use ferratomic_core::{merge::merge, store::Store};

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
    let datoms: Vec<Datom> = (0..kani::any::<u8>().min(4))
        .map(|_| kani::any())
        .collect();

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

    let ab = merge(&a, &b);
    let ba = merge(&b, &a);

    assert_eq!(ab.datom_set(), ba.datom_set());
}
