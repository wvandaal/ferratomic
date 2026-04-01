//! Merge helpers for [`Store`] reconstruction.
//!
//! INV-FERR-001..003: merged stores are the set union of both inputs.
//! INV-FERR-007: merged stores preserve the maximum epoch.
//! INV-FERR-009: merged stores preserve the union of both schemas.
//! INV-FERR-010: merge convergence — SEC follows from 001+002+003.

use ferratom::Schema;

use super::Store;
use crate::indexes::Indexes;

impl Store {
    /// Construct a store from merging two stores: union datoms, union schemas,
    /// take max epoch.
    ///
    /// INV-FERR-001..003: datoms are the set union.
    /// INV-FERR-009: schema is the union of both schemas (all attributes from both).
    /// INV-FERR-007: epoch is `max(a.epoch, b.epoch)` -- the merged store is at
    /// least as current as either input.
    /// INV-FERR-010: this constructor is the SEC convergence mechanism.
    #[must_use]
    pub fn from_merge(a: &Store, b: &Store) -> Self {
        let datoms = a.datoms.clone().union(b.datoms.clone());
        let indexes = Indexes::from_datoms(datoms.iter());
        let schema = merge_schemas(&a.schema, &b.schema);
        let epoch = a.epoch.max(b.epoch);
        let genesis_agent = std::cmp::min(a.genesis_agent, b.genesis_agent);
        let live_set = super::query::build_live_set(datoms.iter());

        Self {
            datoms,
            indexes,
            schema,
            epoch,
            genesis_agent,
            live_set,
        }
    }
}

/// INV-FERR-043: Union two schemas with deterministic conflict resolution.
///
/// INV-FERR-001: schema merge must be commutative. When both schemas
/// define the same attribute with different definitions, keep the one
/// that sorts first by `Ord` (commutativity: `min(a,b) == min(b,a)`).
fn merge_schemas(a: &Schema, b: &Schema) -> Schema {
    let mut schema = Schema::empty();
    for (attr, def) in a.iter().chain(b.iter()) {
        match schema.get(attr) {
            None => {
                schema.define(attr.clone(), def.clone());
            }
            Some(existing) => {
                // INV-FERR-043: conflicting schema definitions resolved
                // deterministically by keeping the def that sorts first.
                // Commutativity preserved: min(a,b) == min(b,a).
                // This is expected in federation when two stores evolve
                // the same attribute differently.
                if def < existing {
                    schema.define(attr.clone(), def.clone());
                }
            }
        }
    }
    schema
}
