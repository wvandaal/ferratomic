//! CRDT merge: set union of two datom stores.
//!
//! INV-FERR-001 (commutativity), INV-FERR-002 (associativity),
//! INV-FERR-003 (idempotency), INV-FERR-004 (monotonic growth).
//!
//! Merge is pure set union. No schema validation (C4).
//! No datoms are added or removed beyond the union.

use crate::store::Store;

/// Merge two stores by set union.
///
/// The result contains exactly the union of both datom sets.
/// Commutative (INV-FERR-001), associative (INV-FERR-002),
/// idempotent (INV-FERR-003). Both input stores are preserved
/// (INV-FERR-004: monotonic growth).
///
/// Merge does NOT validate schema (C4). Datoms with unknown
/// attributes are preserved in the union.
#[must_use]
pub fn merge(a: &Store, b: &Store) -> Store {
    let mut datoms = a.datom_set().clone();
    for datom in b.datom_set() {
        datoms.insert(datom.clone());
    }
    Store::from_datoms(datoms)
}
