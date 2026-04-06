//! Merge helpers for [`Store`] reconstruction.
//!
//! INV-FERR-001..003: merged stores are the set union of both inputs.
//! INV-FERR-007: merged stores preserve the maximum epoch.
//! INV-FERR-009: merged stores preserve the union of both schemas.
//! INV-FERR-010: merge convergence — SEC follows from 001+002+003.

use std::sync::Arc;

use ferratom::{Attribute, AttributeDef, Datom, Schema};

use super::{Store, StoreRepr};
use crate::positional::{merge_positional, merge_sort_dedup, PositionalStore};

/// INV-FERR-043: A deterministic schema conflict discovered during merge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaConflict {
    /// The attribute whose definitions disagreed across replicas.
    pub attribute: Attribute,
    /// The deterministically selected definition (`Ord`-minimal).
    pub kept: AttributeDef,
    /// The losing definition retained only for diagnostics.
    pub discarded: AttributeDef,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SchemaMergeResult {
    schema: Schema,
    conflicts: Vec<SchemaConflict>,
}

impl Store {
    /// Construct a store from merging two stores: union datoms, union schemas,
    /// take max epoch.
    ///
    /// INV-FERR-001..003: datoms are the set union.
    /// INV-FERR-009: schema is the union of both schemas (all attributes from both).
    /// INV-FERR-007: epoch is `max(a.epoch, b.epoch)` -- the merged store is at
    /// least as current as either input.
    /// INV-FERR-010: this constructor is the SEC convergence mechanism.
    ///
    /// bd-h2fz: merge ALWAYS produces a Positional result. The 4-way match
    /// on (a.repr, b.repr) extracts datom slices/iterators and feeds them
    /// into `merge_positional` or `PositionalStore::from_datoms`.
    #[must_use]
    pub fn from_merge(a: &Store, b: &Store) -> Self {
        let positional = merge_repr(&a.repr, &b.repr);
        let schema_merge = merge_schemas(&a.schema, &b.schema);
        let epoch = a.epoch.max(b.epoch);
        let genesis_agent = std::cmp::min(a.genesis_agent, b.genesis_agent);
        // INV-FERR-029: merge causal LIVE lattices via per-key max(TxId).
        // O(min(|L_A|, |L_B|)) via im::OrdMap union — replaces the O(N) full
        // rebuild through build_live_causal(datoms.iter()).
        let live_causal = merge_causal(&a.live_causal, &b.live_causal);
        let live_set = super::query::derive_live_set(&live_causal);

        Self {
            repr: StoreRepr::Positional(Arc::new(positional)),
            schema: schema_merge.schema,
            epoch,
            genesis_agent,
            live_causal,
            live_set,
            schema_conflicts: schema_merge.conflicts,
        }
    }
}

/// bd-h2fz: 4-way match on repr variants for merge.
///
/// Both `Positional` → `merge_positional` (optimal: merge-sort on contiguous arrays).
/// Mixed or both `OrdMap` → O(n+m) merge-sort on sorted inputs.
/// The result is always `Positional`.
///
/// bd-9ecq: mixed-variant merge now uses `merge_sort_dedup` for O(n+m)
/// instead of the previous `from_datoms(chain)` which was O(n log n).
///
/// **Coupling (DEFECT-017)**: Both `PositionalStore::datoms()` and
/// `OrdSet::iter()` yield datoms in `Datom::Ord` order, which is EAVT
/// (entity → attribute → value → tx → op) because `Ord` is derived from
/// the struct field declaration order. `merge_sort_dedup` relies on this.
/// See `Datom` doc comment for the field-order invariant.
fn merge_repr(a: &StoreRepr, b: &StoreRepr) -> PositionalStore {
    match (a, b) {
        (StoreRepr::Positional(pa), StoreRepr::Positional(pb)) => merge_positional(pa, pb),
        (StoreRepr::Positional(pa), StoreRepr::OrdMap { datoms: db, .. }) => {
            let b_sorted: Vec<Datom> = db.iter().cloned().collect();
            let merged = merge_sort_dedup(pa.datoms(), &b_sorted);
            PositionalStore::from_sorted_canonical(merged)
        }
        (StoreRepr::OrdMap { datoms: da, .. }, StoreRepr::Positional(pb)) => {
            let a_sorted: Vec<Datom> = da.iter().cloned().collect();
            let merged = merge_sort_dedup(&a_sorted, pb.datoms());
            PositionalStore::from_sorted_canonical(merged)
        }
        (StoreRepr::OrdMap { datoms: da, .. }, StoreRepr::OrdMap { datoms: db, .. }) => {
            let a_sorted: Vec<Datom> = da.iter().cloned().collect();
            let b_sorted: Vec<Datom> = db.iter().cloned().collect();
            let merged = merge_sort_dedup(&a_sorted, &b_sorted);
            PositionalStore::from_sorted_canonical(merged)
        }
    }
}

/// INV-FERR-029: Merge two causal LIVE lattices by per-key `max(TxId)`.
///
/// For each `(entity, attribute, value)` triple present in either input,
/// retains the event with the highest `TxId`. This is a join-semilattice
/// operation (commutative, associative, idempotent) and a lattice
/// homomorphism over datom set union:
///
///   `merge_causal(LIVE(A), LIVE(B)) = LIVE(A ∪ B)`
///
/// Complexity: `O(m log n)` where `m = min(|L_A|, |L_B|)`, `n = max(|L_A|, |L_B|)`,
/// via `im::OrdMap::union_with` (iterates smaller map, inserts into larger).
fn merge_causal(
    a: &super::query::LiveCausal,
    b: &super::query::LiveCausal,
) -> super::query::LiveCausal {
    a.clone().union_with(b.clone(), |entries_a, entries_b| {
        entries_a.union_with(entries_b, std::cmp::max)
    })
}

/// INV-FERR-043: Union two schemas with deterministic conflict resolution.
///
/// INV-FERR-001: schema merge must be commutative. When both schemas
/// define the same attribute with different definitions, keep the one
/// that sorts first by `Ord` (commutativity: `min(a,b) == min(b,a)`).
fn merge_schemas(a: &Schema, b: &Schema) -> SchemaMergeResult {
    let mut schema = Schema::empty();
    let mut conflicts = Vec::new();
    for (attr, def) in a.iter().chain(b.iter()) {
        match schema.get(attr) {
            None => {
                schema.define(attr.clone(), def.clone());
            }
            Some(existing) => {
                if def == existing {
                    continue;
                }
                // INV-FERR-043: conflicting schema definitions resolved
                // deterministically by keeping the def that sorts first.
                // Commutativity preserved: min(a,b) == min(b,a).
                // Clone existing to release the immutable borrow on schema
                // before the potential mutable schema.define() call.
                let existing_owned = existing.clone();
                let (kept, discarded) = if def < &existing_owned {
                    schema.define(attr.clone(), def.clone());
                    (def.clone(), existing_owned)
                } else {
                    (existing_owned, def.clone())
                };
                conflicts.push(SchemaConflict {
                    attribute: attr.clone(),
                    kept,
                    discarded,
                });
            }
        }
    }
    SchemaMergeResult { schema, conflicts }
}

#[cfg(test)]
mod tests {
    use ferratom::{Attribute, Cardinality, ResolutionMode, ValueType};

    use super::*;

    fn lww_one(value_type: ValueType) -> AttributeDef {
        AttributeDef::new(value_type, Cardinality::One, ResolutionMode::Lww, None)
    }

    #[test]
    fn test_inv_ferr_043_merge_schema_conflict_audit_trail() {
        let attribute = Attribute::from("user/name");
        let string_def = lww_one(ValueType::String);
        let long_def = lww_one(ValueType::Long);
        let mut a = Store::genesis();
        let mut b = Store::genesis();

        a.schema.define(attribute.clone(), string_def.clone());
        b.schema.define(attribute.clone(), long_def.clone());

        let merged = Store::from_merge(&a, &b);
        let expected = if string_def < long_def {
            SchemaConflict {
                attribute: attribute.clone(),
                kept: string_def.clone(),
                discarded: long_def.clone(),
            }
        } else {
            SchemaConflict {
                attribute: attribute.clone(),
                kept: long_def.clone(),
                discarded: string_def.clone(),
            }
        };

        assert_eq!(
            merged.schema_conflicts(),
            std::slice::from_ref(&expected),
            "INV-FERR-043: merge must record each conflicting attribute definition"
        );
        assert_eq!(
            merged.schema().get(&attribute),
            Some(&expected.kept),
            "INV-FERR-043: merge must keep the Ord-minimal definition"
        );
    }

    #[test]
    fn test_inv_ferr_043_merge_schema_conflict_audit_trail_commutative() {
        let attribute = Attribute::from("user/name");
        let string_def = lww_one(ValueType::String);
        let long_def = lww_one(ValueType::Long);
        let mut a = Store::genesis();
        let mut b = Store::genesis();

        a.schema.define(attribute.clone(), string_def);
        b.schema.define(attribute, long_def);

        let ab = Store::from_merge(&a, &b);
        let ba = Store::from_merge(&b, &a);

        assert_eq!(
            ab.schema(),
            ba.schema(),
            "INV-FERR-001: merge schema must remain commutative"
        );
        assert_eq!(
            ab.schema_conflicts(),
            ba.schema_conflicts(),
            "INV-FERR-043: conflict audit trail must be identical regardless of merge order"
        );
    }
}
