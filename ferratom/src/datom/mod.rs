//! Datom: the atomic fact -- a 5-tuple `[entity, attribute, value, tx, op]`.
//!
//! INV-FERR-012: Content-addressed identity. Two datoms with identical
//! 5-tuples are the same datom. Enforced by Eq/Hash/Ord on all five fields.
//!
//! INV-FERR-018: Append-only. Datoms are immutable after creation.
//! No `&mut` methods. Clone is the only way to "modify" (which creates a new datom).

mod entity;
mod value;

pub use entity::EntityId;
use serde::{Deserialize, Serialize};
pub use value::{Attribute, NonNanFloat, Value};

use crate::clock::TxId;

// ---------------------------------------------------------------------------
// Op
// ---------------------------------------------------------------------------

/// Transaction operation: assert a fact into the store, or retract it.
///
/// INV-FERR-018: Retractions are themselves datoms -- the store is
/// append-only. A retraction does not delete; it records that a prior
/// assertion no longer holds as of a given transaction.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug, Serialize, Deserialize)]
pub enum Op {
    /// Assert: the datom holds as of this transaction.
    Assert,
    /// Retract: the datom no longer holds as of this transaction.
    Retract,
}

// ---------------------------------------------------------------------------
// Datom
// ---------------------------------------------------------------------------

/// The atomic fact: a 5-tuple `(entity, attribute, value, tx, op)`.
///
/// INV-FERR-012: Content-addressed identity. Equality and ordering are
/// defined over all five fields. Two datoms with identical tuples are
/// indistinguishable.
///
/// INV-FERR-018: Immutable after creation. All fields are private. There
/// are no `&mut self` methods. The only way to "modify" a datom is to
/// create a new one.
///
/// # Field order invariant (DEFECT-017)
///
/// `Ord` is derived, so comparison is lexicographic over fields in
/// **declaration order**: entity → attribute → value → tx → op. This
/// is EAVT order. `merge_sort_dedup` and `PositionalStore::canonical`
/// depend on `Datom::Ord` matching EAVT. **Do not reorder these fields.**
/// The regression test `test_defect_017_datom_ord_is_eavt` enforces this.
///
/// ADR-FERR-010: `Deserialize` is intentionally NOT derived. `Datom` contains
/// `EntityId`, so deserialization must go through `WireDatom` in the `wire`
/// module to enforce the trust boundary.
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Debug, Serialize)]
pub struct Datom {
    /// The entity this datom describes.
    entity: EntityId,
    /// The attribute (property name) being asserted or retracted.
    attribute: Attribute,
    /// The value of the attribute for this entity.
    value: Value,
    /// The transaction that produced this datom (HLC timestamp).
    tx: TxId,
    /// Whether this datom asserts or retracts the fact.
    op: Op,
}

impl Datom {
    /// Construct a new datom from its five components.
    ///
    /// INV-FERR-018: The returned datom is immutable. All fields are
    /// private and accessible only through read-only accessors.
    #[must_use]
    pub fn new(entity: EntityId, attribute: Attribute, value: Value, tx: TxId, op: Op) -> Self {
        Self {
            entity,
            attribute,
            value,
            tx,
            op,
        }
    }

    /// The entity this datom describes.
    ///
    /// Returns by value because `EntityId` is `Copy` (32 bytes, stack-allocated).
    #[must_use]
    pub fn entity(&self) -> EntityId {
        self.entity
    }

    /// The attribute (property name) of this datom.
    #[must_use]
    pub fn attribute(&self) -> &Attribute {
        &self.attribute
    }

    /// The value carried by this datom.
    #[must_use]
    pub fn value(&self) -> &Value {
        &self.value
    }

    /// The transaction that produced this datom (HLC timestamp).
    ///
    /// Returns by value because `TxId` is `Copy`.
    #[must_use]
    pub fn tx(&self) -> TxId {
        self.tx
    }

    /// Whether this datom asserts or retracts the fact.
    ///
    /// Returns by value because `Op` is `Copy`.
    #[must_use]
    pub fn op(&self) -> Op {
        self.op
    }

    /// BLAKE3 hash of all five fields, providing content-addressed identity.
    ///
    /// INV-FERR-012: Two datoms with identical 5-tuples produce the same
    /// content hash. This is the canonical identity function for
    /// deduplication and content-addressed storage.
    #[must_use]
    pub fn content_hash(&self) -> [u8; 32] {
        let mut hasher = blake3::Hasher::new();

        // Entity: 32 raw bytes
        hasher.update(self.entity.as_bytes());

        // Attribute: length-prefixed UTF-8
        let attr_bytes = self.attribute.as_str().as_bytes();
        hasher.update(&(attr_bytes.len() as u64).to_le_bytes());
        hasher.update(attr_bytes);

        // Value: discriminant tag + payload
        self.hash_value(&mut hasher);

        // Tx: physical, logical, agent
        hasher.update(&self.tx.physical().to_le_bytes());
        hasher.update(&self.tx.logical().to_le_bytes());
        hasher.update(self.tx.agent().as_bytes());

        // Op: single byte discriminant
        match self.op {
            Op::Assert => hasher.update(&[0]),
            Op::Retract => hasher.update(&[1]),
        };

        *hasher.finalize().as_bytes()
    }

    /// Hash a `Value` into the hasher with a discriminant tag prefix
    /// to prevent collisions between variants.
    fn hash_value(&self, hasher: &mut blake3::Hasher) {
        match &self.value {
            Value::Keyword(s) => hash_tagged_bytes(hasher, 0, s.as_bytes()),
            Value::String(s) => hash_tagged_bytes(hasher, 1, s.as_bytes()),
            Value::Long(n) => hash_tagged_fixed(hasher, 2, &n.to_le_bytes()),
            Value::Double(f) => {
                // INV-FERR-012: canonicalize -0.0 to +0.0 so Eq-equal floats
                // produce identical content hashes. OrderedFloat(-0.0) == OrderedFloat(0.0)
                // but (-0.0f64).to_le_bytes() != (0.0f64).to_le_bytes().
                let canonical = f.into_inner() + 0.0;
                hash_tagged_fixed(hasher, 3, &canonical.to_le_bytes());
            }
            Value::Bool(b) => hash_tagged_fixed(hasher, 4, &[u8::from(*b)]),
            Value::Instant(ms) => hash_tagged_fixed(hasher, 5, &ms.to_le_bytes()),
            Value::Uuid(bytes) => hash_tagged_fixed(hasher, 6, bytes),
            Value::Bytes(blob) => hash_tagged_bytes(hasher, 7, blob),
            Value::Ref(eid) => hash_tagged_fixed(hasher, 8, eid.as_bytes()),
            Value::BigInt(n) => hash_tagged_fixed(hasher, 9, &n.to_le_bytes()),
            Value::BigDec(n) => hash_tagged_fixed(hasher, 10, &n.to_le_bytes()),
        }
    }
}

/// INV-FERR-012: Datom identity is determined by content hash.
impl crate::traits::ContentAddressed for Datom {
    fn content_hash(&self) -> [u8; 32] {
        Datom::content_hash(self)
    }
}

/// Hash a discriminant tag followed by length-prefixed variable-length bytes.
fn hash_tagged_bytes(hasher: &mut blake3::Hasher, tag: u8, data: &[u8]) {
    hasher.update(&[tag]);
    hasher.update(&(data.len() as u64).to_le_bytes());
    hasher.update(data);
}

/// Hash a discriminant tag followed by fixed-length bytes (no length prefix).
fn hash_tagged_fixed(hasher: &mut blake3::Hasher, tag: u8, data: &[u8]) {
    hasher.update(&[tag]);
    hasher.update(data);
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::clock::TxId;

    /// Helper: build a minimal datom for testing.
    fn sample_datom() -> Datom {
        let entity = EntityId::from_content(b"test entity");
        let attribute = Attribute::from("db/ident");
        let value = Value::String(Arc::from("hello"));
        let tx = TxId::new(1_000_000, 0, 1);
        Datom::new(entity, attribute, value, tx, Op::Assert)
    }

    // -- Op ----------------------------------------------------------------

    #[test]
    fn test_op_assert_retract_distinct() {
        assert_ne!(Op::Assert, Op::Retract);
    }

    #[test]
    fn test_op_copy() {
        let op = Op::Assert;
        let copy = op;
        assert_eq!(op, copy, "Op must be Copy");
    }

    // -- Datom -------------------------------------------------------------

    #[test]
    fn test_inv_ferr_018_datom_accessors() {
        let d = sample_datom();
        // Accessors return correct values.
        assert_eq!(d.entity(), EntityId::from_content(b"test entity"));
        assert_eq!(d.attribute().as_str(), "db/ident");
        assert_eq!(d.op(), Op::Assert);
    }

    #[test]
    fn test_inv_ferr_012_datom_content_hash_deterministic() {
        let a = sample_datom();
        let b = sample_datom();
        assert_eq!(
            a.content_hash(),
            b.content_hash(),
            "INV-FERR-012: identical datoms must produce identical content hashes"
        );
    }

    #[test]
    fn test_inv_ferr_012_datom_content_hash_sensitive_to_value() {
        let entity = EntityId::from_content(b"e");
        let attr = Attribute::from("a");
        let tx = TxId::new(1, 0, 0);
        let d1 = Datom::new(entity, attr.clone(), Value::Long(1), tx, Op::Assert);
        let d2 = Datom::new(entity, attr, Value::Long(2), tx, Op::Assert);
        assert_ne!(
            d1.content_hash(),
            d2.content_hash(),
            "INV-FERR-012: different values must produce different content hashes"
        );
    }

    #[test]
    fn test_inv_ferr_012_datom_content_hash_sensitive_to_op() {
        let entity = EntityId::from_content(b"e");
        let attr = Attribute::from("a");
        let val = Value::Bool(true);
        let tx = TxId::new(1, 0, 0);
        let assert_datom = Datom::new(entity, attr.clone(), val.clone(), tx, Op::Assert);
        let retract_datom = Datom::new(entity, attr, val, tx, Op::Retract);
        assert_ne!(
            assert_datom.content_hash(),
            retract_datom.content_hash(),
            "INV-FERR-012: assert vs retract must produce different content hashes"
        );
    }

    #[test]
    fn test_inv_ferr_012_datom_equality() {
        let a = sample_datom();
        let b = sample_datom();
        assert_eq!(
            a, b,
            "INV-FERR-012: datoms with identical 5-tuples must be equal"
        );
    }

    /// DEFECT-017 regression: `Datom::Ord` (derived) MUST be EAVT order.
    ///
    /// `merge_sort_dedup` and `PositionalStore::canonical` assume that
    /// `Datom::cmp` sorts by entity first, then attribute, then value,
    /// then tx, then op — matching the struct field declaration order.
    /// If anyone reorders the fields, this test fails and merge breaks.
    #[test]
    fn test_defect_017_datom_ord_is_eavt() {
        let e1 = EntityId::from_content(b"aaa");
        let e2 = EntityId::from_content(b"zzz");
        let a1 = Attribute::from("a/first");
        let a2 = Attribute::from("z/last");
        let v1 = Value::Long(1);
        let v2 = Value::Long(2);
        let tx1 = TxId::new(1, 0, 0);
        let tx2 = TxId::new(2, 0, 0);

        // Entity is the primary sort key.
        // All other fields are identical — only entity differs.
        let (elo, ehi) = if e1 < e2 { (e1, e2) } else { (e2, e1) };
        let entity_lo = Datom::new(elo, a1.clone(), v1.clone(), tx1, Op::Assert);
        let entity_hi = Datom::new(ehi, a1.clone(), v1.clone(), tx1, Op::Assert);
        assert!(
            entity_lo < entity_hi,
            "entity must be the primary sort key (all other fields identical)"
        );

        // Attribute breaks entity ties.
        let attr_lo = Datom::new(e1, a1.clone(), v2.clone(), tx2, Op::Retract);
        let attr_hi = Datom::new(e1, a2.clone(), v1.clone(), tx1, Op::Assert);
        assert!(
            attr_lo < attr_hi,
            "attribute must be the secondary sort key (a/first < z/last)"
        );

        // Value breaks (entity, attribute) ties.
        let val_lo = Datom::new(e1, a1.clone(), v1.clone(), tx2, Op::Retract);
        let val_hi = Datom::new(e1, a1.clone(), v2.clone(), tx1, Op::Assert);
        assert!(
            val_lo < val_hi,
            "value must be the tertiary sort key (1 < 2)"
        );

        // Tx breaks (entity, attribute, value) ties.
        let tx_lo = Datom::new(e1, a1.clone(), v1.clone(), tx1, Op::Retract);
        let tx_hi = Datom::new(e1, a1.clone(), v1.clone(), tx2, Op::Assert);
        assert!(tx_lo < tx_hi, "tx must be the quaternary sort key");

        // Op breaks all other ties.
        let op_assert = Datom::new(e1, a1.clone(), v1.clone(), tx1, Op::Assert);
        let op_retract = Datom::new(e1, a1, v1, tx1, Op::Retract);
        assert!(
            op_assert < op_retract,
            "op must be the final sort key (Assert < Retract)"
        );
    }

    #[test]
    fn test_inv_ferr_018_datom_clone_independence() {
        let a = sample_datom();
        let b = a.clone();
        // Clone produces an equal but independent datom.
        assert_eq!(a, b);
        // They share Arc-backed data but are logically independent values.
        assert_eq!(a.attribute(), b.attribute());
    }

    /// INV-FERR-012: -0.0 and +0.0 are Eq-equal via `OrderedFloat`, so their
    /// content hashes MUST be identical. Regression test for the sign-bit
    /// canonicalization in `hash_value`.
    #[test]
    fn test_inv_ferr_012_content_hash_neg_zero_canonical() {
        let entity = EntityId::from_content(b"neg-zero-test");
        let attr = Attribute::from("test/float");
        let tx = TxId::new(1, 0, 0);
        let pos_zero = Datom::new(
            entity,
            attr.clone(),
            Value::Double(NonNanFloat::new(0.0).expect("0.0 is not NaN")),
            tx,
            Op::Assert,
        );
        let neg_zero = Datom::new(
            entity,
            attr,
            Value::Double(NonNanFloat::new(-0.0).expect("-0.0 is not NaN")),
            tx,
            Op::Assert,
        );
        // OrderedFloat treats -0.0 == +0.0.
        assert_eq!(
            pos_zero, neg_zero,
            "INV-FERR-012: -0.0 and +0.0 datoms must be Eq-equal"
        );
        // Content hashes must also be equal.
        assert_eq!(
            pos_zero.content_hash(),
            neg_zero.content_hash(),
            "INV-FERR-012: -0.0 and +0.0 must produce identical content hashes"
        );
    }

    #[test]
    fn test_all_value_variants_hash_distinctly() {
        // Ensure each variant's discriminant tag prevents cross-variant collisions.
        let entity = EntityId::from_content(b"e");
        let attr = Attribute::from("a");
        let tx = TxId::new(1, 0, 0);

        let values = vec![
            Value::Keyword(Arc::from("k")),
            Value::String(Arc::from("k")), // same payload as Keyword, different tag
            Value::Long(0),
            Value::Double(NonNanFloat::new(0.0).unwrap()),
            Value::Bool(false),
            Value::Instant(0),
            Value::Uuid([0; 16]),
            Value::Bytes(Arc::from(vec![0u8; 0])),
            Value::Ref(EntityId::from_bytes([0; 32])),
            Value::BigInt(0),
            Value::BigDec(0),
        ];

        let hashes: Vec<[u8; 32]> = values
            .into_iter()
            .map(|v| Datom::new(entity, attr.clone(), v, tx, Op::Assert).content_hash())
            .collect();

        // All content hashes must be unique.
        for i in 0..hashes.len() {
            for j in (i + 1)..hashes.len() {
                assert_ne!(
                    hashes[i], hashes[j],
                    "INV-FERR-012: variant {i} and {j} must produce distinct content hashes"
                );
            }
        }
    }
}
