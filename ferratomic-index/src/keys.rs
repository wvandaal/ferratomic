//! Index key types with correct sort ordering (INV-FERR-005).
//!
//! | Index | Sort order                       | Access pattern              |
//! |-------|----------------------------------|-----------------------------|
//! | EAVT  | entity, attribute, value, tx, op | "all facts about entity E"  |
//! | AEVT  | attribute, entity, value, tx, op | "all entities with attr A"  |
//! | VAET  | value, attribute, entity, tx, op | "reverse ref: who points here?" |
//! | AVET  | attribute, value, entity, tx, op | "unique lookup by attr+val" |
//!
//! Field accessors are added on demand: `EavtKey::entity()` exists because
//! `positional.rs` needs it (INV-FERR-076). Other fields are accessed only
//! within this crate via `pub(crate)` visibility. Add accessors when
//! downstream consumers require them, not speculatively.

use ferratom::{Attribute, Datom, EntityId, Op, TxId, Value};

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

    /// The entity component of this key (INV-FERR-005, INV-FERR-076).
    #[must_use]
    pub fn entity(&self) -> EntityId {
        self.0
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

    /// Compare this key against a datom's AEVT fields without constructing
    /// an intermediate key. Zero Arc refcount operations (INV-FERR-027).
    #[inline]
    #[must_use]
    pub fn cmp_datom(&self, datom: &Datom) -> std::cmp::Ordering {
        self.0
            .cmp(datom.attribute())
            .then_with(|| self.1.cmp(&datom.entity()))
            .then_with(|| self.2.cmp(datom.value()))
            .then_with(|| self.3.cmp(&datom.tx()))
            .then_with(|| self.4.cmp(&datom.op()))
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

    /// Compare this key against a datom's VAET fields without constructing
    /// an intermediate key. Zero Arc refcount operations (INV-FERR-027).
    #[inline]
    #[must_use]
    pub fn cmp_datom(&self, datom: &Datom) -> std::cmp::Ordering {
        self.0
            .cmp(datom.value())
            .then_with(|| self.1.cmp(datom.attribute()))
            .then_with(|| self.2.cmp(&datom.entity()))
            .then_with(|| self.3.cmp(&datom.tx()))
            .then_with(|| self.4.cmp(&datom.op()))
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

    /// Compare this key against a datom's AVET fields without constructing
    /// an intermediate key. Zero Arc refcount operations (INV-FERR-027).
    #[inline]
    #[must_use]
    pub fn cmp_datom(&self, datom: &Datom) -> std::cmp::Ordering {
        self.0
            .cmp(datom.attribute())
            .then_with(|| self.1.cmp(datom.value()))
            .then_with(|| self.2.cmp(&datom.entity()))
            .then_with(|| self.3.cmp(&datom.tx()))
            .then_with(|| self.4.cmp(&datom.op()))
    }
}
