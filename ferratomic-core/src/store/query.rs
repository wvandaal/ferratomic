//! Snapshot and LIVE-set query helpers for [`Store`].
//!
//! INV-FERR-006: snapshots are immutable point-in-time views.
//! INV-FERR-029: LIVE values contain only non-retracted datoms.
//! INV-FERR-032: cardinality-one resolution selects the latest LIVE value.

use ferratom::{Attribute, Datom, EntityId, Op, TxId, Value};
use im::{OrdMap, OrdSet};

use super::{Snapshot, Store};

impl Snapshot {
    /// Iterate over all datoms visible in this snapshot.
    ///
    /// INV-FERR-006: the iterator yields exactly the datoms that
    /// were present when the snapshot was created -- no more, no fewer.
    pub fn datoms(&self) -> impl Iterator<Item = &Datom> {
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
        self.live_txids
            .get(&(entity, attribute.clone()))
            .and_then(select_latest_live_value)
    }

    /// Take an immutable point-in-time snapshot of the store.
    ///
    /// INV-FERR-006: the returned snapshot is frozen. Subsequent
    /// calls to `transact` or `insert` do not affect it.
    #[must_use]
    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            datoms: self.datoms.clone(),
            epoch: self.epoch,
        }
    }
}

/// Build a LIVE set from an iterator of datoms (full rebuild).
///
/// INV-FERR-029: Used during cold start, checkpoint load, and merge.
/// Processes datoms in iteration order (EAVT by `Datom::Ord`).
pub(super) fn build_live_set<'a>(
    datoms: impl Iterator<Item = &'a Datom>,
) -> OrdMap<(EntityId, Attribute), OrdSet<Value>> {
    let mut live: OrdMap<(EntityId, Attribute), OrdSet<Value>> = OrdMap::new();
    for datom in datoms {
        let key = (datom.entity(), datom.attribute().clone());
        match datom.op() {
            Op::Assert => {
                let vals = live.entry(key).or_default();
                vals.insert(datom.value().clone());
            }
            Op::Retract => {
                if let Some(vals) = live.get_mut(&key) {
                    vals.remove(datom.value());
                    if vals.is_empty() {
                        live.remove(&key);
                    }
                }
            }
        }
    }
    live
}

/// Build per-value LIVE `TxId` metadata from an iterator of datoms.
///
/// INV-FERR-032: surviving values retain the latest assert `TxId` that has not
/// been retracted, allowing card-one resolution by causal order.
pub(super) fn build_live_txids<'a>(
    datoms: impl Iterator<Item = &'a Datom>,
) -> OrdMap<(EntityId, Attribute), OrdMap<Value, TxId>> {
    let mut live: OrdMap<(EntityId, Attribute), OrdMap<Value, TxId>> = OrdMap::new();
    for datom in datoms {
        let key = (datom.entity(), datom.attribute().clone());
        match datom.op() {
            Op::Assert => {
                let txids = live.entry(key).or_default();
                txids.insert(datom.value().clone(), datom.tx());
            }
            Op::Retract => {
                if let Some(txids) = live.get_mut(&key) {
                    txids.remove(datom.value());
                    if txids.is_empty() {
                        live.remove(&key);
                    }
                }
            }
        }
    }
    live
}

fn select_latest_live_value(values: &OrdMap<Value, TxId>) -> Option<&Value> {
    values
        .iter()
        .max_by(|(left_value, left_tx), (right_value, right_tx)| {
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
