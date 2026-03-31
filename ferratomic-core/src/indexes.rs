//! Per-index key types, `IndexBackend` trait, and `Indexes` struct with
//! correct sort ordering.
//!
//! INV-FERR-005: four secondary indexes are maintained in bijection with
//! the primary datom set. Each index uses a distinct key type whose `Ord`
//! implementation arranges datom fields in the index-specific order:
//!
//! | Index | Sort order                       | Access pattern              |
//! |-------|----------------------------------|-----------------------------|
//! | EAVT  | entity, attribute, value, tx, op | "all facts about entity E"  |
//! | AEVT  | attribute, entity, value, tx, op | "all entities with attr A"  |
//! | VAET  | value, attribute, entity, tx, op | "reverse ref: who points here?" |
//! | AVET  | attribute, value, entity, tx, op | "unique lookup by attr+val" |
//!
//! INV-FERR-025: the index backend is interchangeable via the
//! [`IndexBackend`] trait. All backends produce identical query results
//! for the same sequence of operations — they differ only in performance
//! characteristics. The default backend is `im::OrdMap` (ADR-FERR-001).

use ferratom::{Attribute, Datom, EntityId, Op, TxId, Value};
use im::OrdMap;

// ---------------------------------------------------------------------------
// IndexBackend trait (INV-FERR-025)
// ---------------------------------------------------------------------------

/// Ordered-map abstraction for secondary index storage.
///
/// INV-FERR-025: all index backends are interchangeable. Switching
/// backends changes performance characteristics but not correctness.
/// Every implementation must provide ordered-map semantics: insert,
/// lookup, iteration in key order, and length.
///
/// `im::OrdMap` is the default backend (ADR-FERR-001), providing O(1)
/// clone via structural sharing. Alternative backends (B-tree, LSM,
/// `RocksDB`) can be substituted without changing store semantics.
pub trait IndexBackend<K: Ord, V>: Clone + Default + std::fmt::Debug {
    /// Insert a key-value pair into the map.
    ///
    /// For persistent data structures (like `im::OrdMap`), the receiver
    /// is mutated in place with structural sharing. For owned structures,
    /// this is a standard insert.
    fn backend_insert(&mut self, key: K, value: V);

    /// Look up a value by exact key.
    fn backend_get(&self, key: &K) -> Option<&V>;

    /// Number of entries in the map.
    fn backend_len(&self) -> usize;

    /// Whether the map contains no entries.
    fn backend_is_empty(&self) -> bool {
        self.backend_len() == 0
    }

    /// Iterate over all values in key order.
    fn backend_values(&self) -> Box<dyn Iterator<Item = &V> + '_>;
}

// ---------------------------------------------------------------------------
// im::OrdMap implementation (ADR-FERR-001)
// ---------------------------------------------------------------------------

/// INV-FERR-025: `im::OrdMap` backend — the default index backend.
///
/// Provides O(log n) insert/lookup with O(1) clone via structural
/// sharing, making it ideal for MVCC snapshot isolation (INV-FERR-006).
impl<K: Ord + Clone + std::fmt::Debug, V: Clone + std::fmt::Debug> IndexBackend<K, V>
    for OrdMap<K, V>
{
    fn backend_insert(&mut self, key: K, value: V) {
        self.insert(key, value);
    }

    fn backend_get(&self, key: &K) -> Option<&V> {
        self.get(key)
    }

    fn backend_len(&self) -> usize {
        self.len()
    }

    fn backend_is_empty(&self) -> bool {
        self.is_empty()
    }

    fn backend_values(&self) -> Box<dyn Iterator<Item = &V> + '_> {
        Box::new(self.values())
    }
}

// ---------------------------------------------------------------------------
// Index key types — Ord derives produce the correct sort order
// ---------------------------------------------------------------------------

/// EAVT key: sorted by (entity, attribute, value, tx, op) (INV-FERR-005).
///
/// Access pattern: "all facts about entity E".
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct EavtKey(
    pub(crate) EntityId,
    pub(crate) Attribute,
    pub(crate) Value,
    pub(crate) TxId,
    pub(crate) Op,
);

/// AEVT key: sorted by (attribute, entity, value, tx, op) (INV-FERR-005).
///
/// Access pattern: "all entities with attribute A".
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct AevtKey(
    pub(crate) Attribute,
    pub(crate) EntityId,
    pub(crate) Value,
    pub(crate) TxId,
    pub(crate) Op,
);

/// VAET key: sorted by (value, attribute, entity, tx, op) (INV-FERR-005).
///
/// Access pattern: "reverse reference -- who points to this entity?"
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct VaetKey(
    pub(crate) Value,
    pub(crate) Attribute,
    pub(crate) EntityId,
    pub(crate) TxId,
    pub(crate) Op,
);

/// AVET key: sorted by (attribute, value, entity, tx, op) (INV-FERR-005).
///
/// Access pattern: "unique lookup by attribute + value pair".
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct AvetKey(
    pub(crate) Attribute,
    pub(crate) Value,
    pub(crate) EntityId,
    pub(crate) TxId,
    pub(crate) Op,
);

impl EavtKey {
    /// Construct an EAVT key from a datom reference (INV-FERR-005).
    #[must_use]
    pub fn from_datom(d: &Datom) -> Self {
        Self(
            d.entity(),
            d.attribute().clone(),
            d.value().clone(),
            d.tx(),
            d.op(),
        )
    }
}

impl AevtKey {
    /// Construct an AEVT key from a datom reference (INV-FERR-005).
    #[must_use]
    pub fn from_datom(d: &Datom) -> Self {
        Self(
            d.attribute().clone(),
            d.entity(),
            d.value().clone(),
            d.tx(),
            d.op(),
        )
    }
}

impl VaetKey {
    /// Construct a VAET key from a datom reference (INV-FERR-005).
    #[must_use]
    pub fn from_datom(d: &Datom) -> Self {
        Self(
            d.value().clone(),
            d.attribute().clone(),
            d.entity(),
            d.tx(),
            d.op(),
        )
    }
}

impl AvetKey {
    /// Construct an AVET key from a datom reference (INV-FERR-005).
    #[must_use]
    pub fn from_datom(d: &Datom) -> Self {
        Self(
            d.attribute().clone(),
            d.value().clone(),
            d.entity(),
            d.tx(),
            d.op(),
        )
    }
}

// ---------------------------------------------------------------------------
// Indexes (generic over IndexBackend)
// ---------------------------------------------------------------------------

/// Secondary indexes over the datom set, each with a distinct sort order.
///
/// INV-FERR-005: every index is a bijection with the primary datom set.
/// After every mutation, all four maps have the same cardinality as the
/// primary set.
///
/// INV-FERR-025: the backend types are interchangeable. Each index uses
/// its own backend instance, parameterized by its key type. The default
/// is `im::OrdMap` (see [`Indexes`] type alias).
///
/// INV-FERR-027: correct per-index ordering enables O(log n + k) range
/// scans for different access patterns.
#[derive(Debug, Clone)]
pub struct GenericIndexes<BE, BA, BV, BAV>
where
    BE: IndexBackend<EavtKey, Datom>,
    BA: IndexBackend<AevtKey, Datom>,
    BV: IndexBackend<VaetKey, Datom>,
    BAV: IndexBackend<AvetKey, Datom>,
{
    /// Entity-Attribute-Value-Tx index.
    eavt: BE,
    /// Attribute-Entity-Value-Tx index.
    aevt: BA,
    /// Value-Attribute-Entity-Tx index (reverse references).
    vaet: BV,
    /// Attribute-Value-Entity-Tx index (unique/lookup).
    avet: BAV,
}

/// Default index type using `im::OrdMap` (ADR-FERR-001).
///
/// INV-FERR-025: type alias preserves backward compatibility — all
/// existing code that references `Indexes` continues to work without
/// changes.
pub type Indexes = GenericIndexes<
    OrdMap<EavtKey, Datom>,
    OrdMap<AevtKey, Datom>,
    OrdMap<VaetKey, Datom>,
    OrdMap<AvetKey, Datom>,
>;

impl<BE, BA, BV, BAV> GenericIndexes<BE, BA, BV, BAV>
where
    BE: IndexBackend<EavtKey, Datom>,
    BA: IndexBackend<AevtKey, Datom>,
    BV: IndexBackend<VaetKey, Datom>,
    BAV: IndexBackend<AvetKey, Datom>,
{
    /// Build indexes from a primary datom iterator.
    ///
    /// INV-FERR-005: all four indexes receive every datom from the primary
    /// set, ensuring bijection by construction.
    pub fn from_datoms<'a>(datoms: impl Iterator<Item = &'a Datom>) -> Self {
        let mut eavt = BE::default();
        let mut aevt = BA::default();
        let mut vaet = BV::default();
        let mut avet = BAV::default();

        for d in datoms {
            eavt.backend_insert(EavtKey::from_datom(d), d.clone());
            aevt.backend_insert(AevtKey::from_datom(d), d.clone());
            vaet.backend_insert(VaetKey::from_datom(d), d.clone());
            avet.backend_insert(AvetKey::from_datom(d), d.clone());
        }

        Self {
            eavt,
            aevt,
            vaet,
            avet,
        }
    }

    /// Insert a datom into all four indexes.
    ///
    /// INV-FERR-005: maintaining bijection requires every insert to
    /// touch all indexes.
    pub fn insert(&mut self, datom: &Datom) {
        self.eavt
            .backend_insert(EavtKey::from_datom(datom), datom.clone());
        self.aevt
            .backend_insert(AevtKey::from_datom(datom), datom.clone());
        self.vaet
            .backend_insert(VaetKey::from_datom(datom), datom.clone());
        self.avet
            .backend_insert(AvetKey::from_datom(datom), datom.clone());
    }

    /// Number of entries in the EAVT index (INV-FERR-005: same as all other indexes).
    #[must_use]
    pub fn len(&self) -> usize {
        self.eavt.backend_len()
    }

    /// Whether all indexes are empty (INV-FERR-005).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.eavt.backend_is_empty()
    }

    /// Access the EAVT index backend (INV-FERR-005, INV-FERR-027).
    #[must_use]
    pub fn eavt(&self) -> &BE {
        &self.eavt
    }

    /// Access the AEVT index backend (INV-FERR-005, INV-FERR-027).
    #[must_use]
    pub fn aevt(&self) -> &BA {
        &self.aevt
    }

    /// Access the VAET index backend (INV-FERR-005, INV-FERR-027).
    #[must_use]
    pub fn vaet(&self) -> &BV {
        &self.vaet
    }

    /// Access the AVET index backend (INV-FERR-005, INV-FERR-027).
    #[must_use]
    pub fn avet(&self) -> &BAV {
        &self.avet
    }

    /// Iterate EAVT datoms in index order (INV-FERR-027).
    pub fn eavt_datoms(&self) -> impl Iterator<Item = &Datom> {
        self.eavt.backend_values()
    }

    /// Iterate AEVT datoms in index order (INV-FERR-027).
    pub fn aevt_datoms(&self) -> impl Iterator<Item = &Datom> {
        self.aevt.backend_values()
    }

    /// Iterate VAET datoms in index order (INV-FERR-027).
    pub fn vaet_datoms(&self) -> impl Iterator<Item = &Datom> {
        self.vaet.backend_values()
    }

    /// Iterate AVET datoms in index order (INV-FERR-027).
    pub fn avet_datoms(&self) -> impl Iterator<Item = &Datom> {
        self.avet.backend_values()
    }

    /// Verify that all four indexes have the same cardinality.
    ///
    /// INV-FERR-005: bijection implies equal cardinality. Returns `true`
    /// if all four indexes agree on the count of entries.
    #[must_use]
    pub fn verify_bijection(&self) -> bool {
        let n = self.eavt.backend_len();
        if self.aevt.backend_len() != n
            || self.vaet.backend_len() != n
            || self.avet.backend_len() != n
        {
            return false;
        }
        // ME-003: In debug/test builds, also verify datom identity —
        // not just cardinality. A bug that inserts different datoms into
        // different indexes would pass the count-only check.
        #[cfg(any(test, debug_assertions))]
        {
            use std::collections::BTreeSet;
            let eavt_datoms: BTreeSet<_> = self.eavt.backend_values().collect();
            let aevt_datoms: BTreeSet<_> = self.aevt.backend_values().collect();
            if eavt_datoms != aevt_datoms {
                return false;
            }
        }
        true
    }
}
