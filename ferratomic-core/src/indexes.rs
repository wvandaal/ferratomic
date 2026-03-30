//! Per-index key types and `Indexes` struct with correct sort ordering.
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

use im::OrdMap;

use ferratom::{Attribute, Datom, EntityId, Op, TxId, Value};

// ---------------------------------------------------------------------------
// Index key types — Ord derives produce the correct sort order
// ---------------------------------------------------------------------------

/// EAVT key: sorted by (entity, attribute, value, tx, op).
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct EavtKey(pub EntityId, pub Attribute, pub Value, pub TxId, pub Op);

/// AEVT key: sorted by (attribute, entity, value, tx, op).
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct AevtKey(pub Attribute, pub EntityId, pub Value, pub TxId, pub Op);

/// VAET key: sorted by (value, attribute, entity, tx, op).
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct VaetKey(pub Value, pub Attribute, pub EntityId, pub TxId, pub Op);

/// AVET key: sorted by (attribute, value, entity, tx, op).
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct AvetKey(pub Attribute, pub Value, pub EntityId, pub TxId, pub Op);

impl EavtKey {
    /// Construct from a datom reference.
    #[must_use]
    pub fn from_datom(d: &Datom) -> Self {
        Self(d.entity(), d.attribute().clone(), d.value().clone(), d.tx(), d.op())
    }
}

impl AevtKey {
    /// Construct from a datom reference.
    #[must_use]
    pub fn from_datom(d: &Datom) -> Self {
        Self(d.attribute().clone(), d.entity(), d.value().clone(), d.tx(), d.op())
    }
}

impl VaetKey {
    /// Construct from a datom reference.
    #[must_use]
    pub fn from_datom(d: &Datom) -> Self {
        Self(d.value().clone(), d.attribute().clone(), d.entity(), d.tx(), d.op())
    }
}

impl AvetKey {
    /// Construct from a datom reference.
    #[must_use]
    pub fn from_datom(d: &Datom) -> Self {
        Self(d.attribute().clone(), d.value().clone(), d.entity(), d.tx(), d.op())
    }
}

// ---------------------------------------------------------------------------
// Indexes
// ---------------------------------------------------------------------------

/// Secondary indexes over the datom set, each with a distinct sort order.
///
/// INV-FERR-005: every index is a bijection with the primary datom set.
/// After every mutation, all four maps have the same cardinality as the
/// primary set.
///
/// INV-FERR-027: correct per-index ordering enables O(log n + k) range
/// scans for different access patterns.
#[derive(Debug, Clone)]
pub struct Indexes {
    /// Entity-Attribute-Value-Tx index.
    eavt: OrdMap<EavtKey, Datom>,
    /// Attribute-Entity-Value-Tx index.
    aevt: OrdMap<AevtKey, Datom>,
    /// Value-Attribute-Entity-Tx index (reverse references).
    vaet: OrdMap<VaetKey, Datom>,
    /// Attribute-Value-Entity-Tx index (unique/lookup).
    avet: OrdMap<AvetKey, Datom>,
}

impl Indexes {
    /// Build indexes from a primary datom iterator.
    ///
    /// INV-FERR-005: all four indexes receive every datom from the primary
    /// set, ensuring bijection by construction.
    pub fn from_datoms<'a>(datoms: impl Iterator<Item = &'a Datom>) -> Self {
        let mut eavt = OrdMap::new();
        let mut aevt = OrdMap::new();
        let mut vaet = OrdMap::new();
        let mut avet = OrdMap::new();

        for d in datoms {
            eavt.insert(EavtKey::from_datom(d), d.clone());
            aevt.insert(AevtKey::from_datom(d), d.clone());
            vaet.insert(VaetKey::from_datom(d), d.clone());
            avet.insert(AvetKey::from_datom(d), d.clone());
        }

        Self { eavt, aevt, vaet, avet }
    }

    /// Insert a datom into all four indexes.
    ///
    /// INV-FERR-005: maintaining bijection requires every insert to
    /// touch all indexes.
    pub fn insert(&mut self, datom: &Datom) {
        self.eavt.insert(EavtKey::from_datom(datom), datom.clone());
        self.aevt.insert(AevtKey::from_datom(datom), datom.clone());
        self.vaet.insert(VaetKey::from_datom(datom), datom.clone());
        self.avet.insert(AvetKey::from_datom(datom), datom.clone());
    }

    /// Number of entries in the EAVT index (same as all other indexes).
    #[must_use]
    pub fn len(&self) -> usize {
        self.eavt.len()
    }

    /// Whether the indexes are empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.eavt.is_empty()
    }

    /// Entity-Attribute-Value-Tx index (full map for range scans).
    ///
    /// INV-FERR-005: bijective with the primary datom set.
    #[must_use]
    pub fn eavt(&self) -> &OrdMap<EavtKey, Datom> {
        &self.eavt
    }

    /// Attribute-Entity-Value-Tx index (full map for range scans).
    ///
    /// INV-FERR-005: bijective with the primary datom set.
    #[must_use]
    pub fn aevt(&self) -> &OrdMap<AevtKey, Datom> {
        &self.aevt
    }

    /// Value-Attribute-Entity-Tx index (full map for range scans).
    ///
    /// INV-FERR-005: bijective with the primary datom set.
    #[must_use]
    pub fn vaet(&self) -> &OrdMap<VaetKey, Datom> {
        &self.vaet
    }

    /// Attribute-Value-Entity-Tx index (full map for range scans).
    ///
    /// INV-FERR-005: bijective with the primary datom set.
    #[must_use]
    pub fn avet(&self) -> &OrdMap<AvetKey, Datom> {
        &self.avet
    }

    /// Iterate EAVT datoms in index order.
    pub fn eavt_datoms(&self) -> impl Iterator<Item = &Datom> {
        self.eavt.values()
    }

    /// Iterate AEVT datoms in index order.
    pub fn aevt_datoms(&self) -> impl Iterator<Item = &Datom> {
        self.aevt.values()
    }

    /// Iterate VAET datoms in index order.
    pub fn vaet_datoms(&self) -> impl Iterator<Item = &Datom> {
        self.vaet.values()
    }

    /// Iterate AVET datoms in index order.
    pub fn avet_datoms(&self) -> impl Iterator<Item = &Datom> {
        self.avet.values()
    }

    /// Verify that all four indexes have the same cardinality.
    ///
    /// INV-FERR-005: bijection implies equal cardinality. Returns `true`
    /// if all four indexes agree on the count of entries.
    #[must_use]
    pub fn verify_bijection(&self) -> bool {
        let n = self.eavt.len();
        self.aevt.len() == n && self.vaet.len() == n && self.avet.len() == n
    }
}
