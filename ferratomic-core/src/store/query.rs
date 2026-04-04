//! Snapshot and LIVE-set query helpers for [`Store`].
//!
//! INV-FERR-006: snapshots are immutable point-in-time views.
//! INV-FERR-029: LIVE values contain only non-retracted datoms.
//! INV-FERR-032: cardinality-one resolution selects the latest LIVE value.

use ferratom::{Attribute, Datom, EntityId, Op, TxId, Value};
use im::{OrdMap, OrdSet};

use super::{iter::DatomIter, Snapshot, SnapshotDatoms, Store, StoreRepr};

/// Type alias for the causal OR-Set LIVE lattice.
///
/// Maps `(entity, attribute)` to `value` to `(TxId, Op)` where `TxId` is the
/// latest causal event for that `(e,a,v)` triple. Values with `Op::Assert` are
/// LIVE; values with `Op::Retract` are dead but causally tracked for merge
/// correctness.
///
/// This structure is a join-semilattice under per-key `max(TxId)`, making it
/// a lattice homomorphism over datom set union.
pub(crate) type LiveCausal = OrdMap<(EntityId, Attribute), OrdMap<Value, (TxId, Op)>>;

impl Snapshot {
    /// Iterate over all datoms visible in this snapshot.
    ///
    /// INV-FERR-006: the iterator yields exactly the datoms that
    /// were present when the snapshot was created -- no more, no fewer.
    #[must_use]
    pub fn datoms(&self) -> DatomIter<'_> {
        self.datoms.iter()
    }

    /// The epoch at which this snapshot was taken.
    ///
    /// INV-FERR-011: observer epochs derived from snapshots are
    /// monotonically non-decreasing.
    #[must_use]
    pub fn epoch(&self) -> u64 {
        self.epoch
    }
}

impl Store {
    /// Resolve the LIVE value(s) for an entity-attribute pair.
    ///
    /// INV-FERR-029: Returns the set of non-retracted values.
    /// INV-FERR-032: For card-one attributes, the caller should take the
    /// value with the highest `TxId` epoch (LWW). For card-many, all values
    /// are current. This method returns the raw set; callers apply
    /// cardinality resolution.
    #[must_use]
    pub fn live_values(&self, entity: EntityId, attribute: &Attribute) -> Option<&OrdSet<Value>> {
        self.live_set.get(&(entity, attribute.clone()))
    }

    /// Resolve a single LIVE value for a card-one entity-attribute pair.
    ///
    /// INV-FERR-032: For cardinality-one, returns the surviving value with the
    /// highest assert `TxId`, or `None` if fully retracted or never asserted.
    #[must_use]
    pub fn live_resolve(&self, entity: EntityId, attribute: &Attribute) -> Option<&Value> {
        self.live_causal
            .get(&(entity, attribute.clone()))
            .and_then(select_latest_live_value)
    }

    /// Take an immutable point-in-time snapshot of the store.
    ///
    /// INV-FERR-006: the returned snapshot is frozen. Subsequent
    /// calls to `transact` or `insert` do not affect it.
    ///
    /// bd-h2fz: `Positional` stores clone the `Arc` (O(1)).
    /// `OrdMap` stores clone the `OrdSet` (O(1) structural sharing).
    #[must_use]
    pub fn snapshot(&self) -> Snapshot {
        let datoms = match &self.repr {
            StoreRepr::Positional(ps) => SnapshotDatoms::Positional(ps.clone()),
            StoreRepr::OrdMap { datoms, .. } => SnapshotDatoms::OrdSet(datoms.clone()),
        };
        Snapshot {
            datoms,
            epoch: self.epoch,
        }
    }
}

/// Build a causal LIVE lattice from an iterator of datoms (full rebuild).
///
/// INV-FERR-029: Used during cold start, checkpoint load, and merge.
/// For each (entity, attribute, value) triple, retains the event with the
/// highest `TxId`. Dead values (`Op::Retract`) are tracked for merge correctness.
pub(super) fn build_live_causal<'a>(datoms: impl Iterator<Item = &'a Datom>) -> LiveCausal {
    let mut causal: LiveCausal = OrdMap::new();
    for datom in datoms {
        let key = (datom.entity(), datom.attribute().clone());
        let entries = causal.entry(key).or_default();
        let value = datom.value().clone();
        match entries.get(&value) {
            Some(&(existing_tx, _)) if existing_tx >= datom.tx() => {}
            _ => {
                entries.insert(value, (datom.tx(), datom.op()));
            }
        }
    }
    causal
}

/// Derive a materialized LIVE set from a causal lattice.
///
/// INV-FERR-029: Projects the causal map to only values with `Op::Assert`,
/// producing the set of non-retracted values per (entity, attribute).
pub(super) fn derive_live_set(causal: &LiveCausal) -> OrdMap<(EntityId, Attribute), OrdSet<Value>> {
    let mut live: OrdMap<(EntityId, Attribute), OrdSet<Value>> = OrdMap::new();
    for (key, entries) in causal {
        let mut values = OrdSet::new();
        for (value, &(_, op)) in entries {
            if op == Op::Assert {
                values.insert(value.clone());
            }
        }
        if !values.is_empty() {
            live.insert(key.clone(), values);
        }
    }
    live
}

/// Select the LIVE value with the highest assert `TxId` from a causal entry map.
///
/// INV-FERR-032: For card-one resolution, filters to `Op::Assert` entries and
/// picks the one with the highest `TxId` (LWW semantics).
fn select_latest_live_value(entries: &OrdMap<Value, (TxId, Op)>) -> Option<&Value> {
    select_latest_live_value_from_iter(entries.iter())
}

/// Proof-friendly card-one LIVE selection surface.
///
/// This exposes the exact selection kernel used by `Store::live_resolve`
/// without requiring harnesses to construct a full `Store`.
#[cfg(any(test, feature = "test-utils"))]
#[must_use]
pub fn select_latest_live_value_for_test(entries: &[(Value, (TxId, Op))]) -> Option<&Value> {
    select_latest_live_value_from_iter(entries.iter().map(|(value, meta)| (value, meta)))
}

fn select_latest_live_value_from_iter<'a>(
    entries: impl Iterator<Item = (&'a Value, &'a (TxId, Op))>,
) -> Option<&'a Value> {
    entries
        .filter(|(_, &(_, op))| op == Op::Assert)
        .max_by(|(left_value, (left_tx, _)), (right_value, (right_tx, _))| {
            left_tx
                .cmp(right_tx)
                .then_with(|| left_value.cmp(right_value))
        })
        .map(|(value, _)| value)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use ferratom::{AgentId, Attribute, Datom, EntityId, Op, TxId, Value};

    use super::Store;
    use crate::writer::Transaction;

    #[test]
    fn test_inv_ferr_032_lww_by_tx_id_not_value_ord() {
        let entity = EntityId::from_content(b"bd-ik91-lww");
        let attribute = Attribute::from("test/name");
        let older_value = Value::String("z-old".into());
        let newer_value = Value::String("a-new".into());

        let datoms = BTreeSet::from([
            Datom::new(
                entity,
                attribute.clone(),
                older_value.clone(),
                TxId::new(1, 0, 0),
                Op::Assert,
            ),
            Datom::new(
                entity,
                attribute.clone(),
                newer_value.clone(),
                TxId::new(2, 0, 0),
                Op::Assert,
            ),
        ]);
        let store = Store::from_datoms(datoms);

        assert_eq!(
            store.live_resolve(entity, &attribute),
            Some(&newer_value),
            "INV-FERR-032: LWW must choose the highest-TxId value, not the \
             lexicographically greatest Value"
        );
        let live_values = store
            .live_values(entity, &attribute)
            .expect("bd-ik91: both asserted values should remain live in the raw set");
        assert!(
            live_values.contains(&older_value) && live_values.contains(&newer_value),
            "bd-ik91: card-many-style raw LIVE set should retain all non-retracted values"
        );
    }

    #[test]
    fn test_inv_ferr_032_retraction_restores_next_latest_value() {
        let entity = EntityId::from_content(b"bd-ik91-retract");
        let attribute = Attribute::from("test/name");
        let older_value = Value::String("m-older".into());
        let newer_value = Value::String("a-newer".into());

        let datoms = BTreeSet::from([
            Datom::new(
                entity,
                attribute.clone(),
                older_value.clone(),
                TxId::new(1, 0, 0),
                Op::Assert,
            ),
            Datom::new(
                entity,
                attribute.clone(),
                newer_value.clone(),
                TxId::new(2, 0, 0),
                Op::Assert,
            ),
            Datom::new(
                entity,
                attribute.clone(),
                newer_value.clone(),
                TxId::new(3, 0, 0),
                Op::Retract,
            ),
        ]);
        let store = Store::from_datoms(datoms);

        assert_eq!(
            store.live_resolve(entity, &attribute),
            Some(&older_value),
            "INV-FERR-032: retracting the newest value must reveal the next-latest \
             surviving assertion"
        );
    }

    #[test]
    fn test_inv_ferr_032_transact_path_updates_live_metadata() {
        let mut store = Store::genesis();
        let agent = AgentId::from_bytes([0x91; 16]);
        let entity = EntityId::from_content(b"bd-ik91-transact");
        let attribute = Attribute::from("db/doc");
        let older_value = Value::String("z-old".into());
        let newer_value = Value::String("a-new".into());

        let tx1 = Transaction::new(agent)
            .assert_datom(entity, attribute.clone(), older_value.clone())
            .commit(store.schema())
            .expect("bd-ik91: first transact-path assertion must validate");
        store
            .transact_test(tx1)
            .expect("bd-ik91: first transact-path assertion must apply");

        let tx2 = Transaction::new(agent)
            .assert_datom(entity, attribute.clone(), newer_value.clone())
            .commit(store.schema())
            .expect("bd-ik91: second transact-path assertion must validate");
        store
            .transact_test(tx2)
            .expect("bd-ik91: second transact-path assertion must apply");

        assert_eq!(
            store.live_resolve(entity, &attribute),
            Some(&newer_value),
            "bd-ik91: transact path must keep LIVE LWW metadata in sync with primary datoms"
        );
        let live_values = store
            .live_values(entity, &attribute)
            .expect("bd-ik91: transact path must keep raw LIVE values in sync too");
        assert!(
            live_values.contains(&older_value) && live_values.contains(&newer_value),
            "bd-ik91: raw LIVE values should still expose all non-retracted assertions"
        );
    }
}
