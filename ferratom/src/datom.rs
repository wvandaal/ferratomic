//! Datom: the atomic fact — a 5-tuple `[entity, attribute, value, tx, op]`.
//!
//! INV-FERR-012: Content-addressed identity. Two datoms with identical
//! 5-tuples are the same datom. Enforced by Eq/Hash/Ord on all five fields.
//!
//! INV-FERR-018: Append-only. Datoms are immutable after creation.
//! No `&mut` methods. Clone is the only way to "modify" (which creates a new datom).

use std::{fmt, sync::Arc};

use ordered_float::OrderedFloat;
use serde::{Deserialize, Serialize};

use crate::clock::TxId;

// ---------------------------------------------------------------------------
// EntityId
// ---------------------------------------------------------------------------

/// Content-addressed entity identifier: BLAKE3 hash of content bytes.
///
/// INV-FERR-012: `EntityId = BLAKE3(content)`. Two entities with identical
/// content produce identical identifiers. The inner field is private to
/// enforce construction only through `from_content` (production) or
/// `from_bytes` (testing).
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug, Serialize, Deserialize)]
pub struct EntityId([u8; 32]);

impl EntityId {
    /// Create an `EntityId` by BLAKE3-hashing arbitrary content bytes.
    ///
    /// This is the ONLY production constructor. Every entity in the store
    /// derives its identity from content, making the store a content-addressed
    /// structure (INV-FERR-012).
    #[must_use]
    pub fn from_content(content: &[u8]) -> Self {
        Self(*blake3::hash(content).as_bytes())
    }

    /// Create an `EntityId` from raw bytes. **Testing only.**
    ///
    /// Bypasses the BLAKE3 derivation. Used by proptest generators to cover
    /// the full 256-bit ID space without manufacturing content for each case.
    #[must_use]
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Borrow the underlying 32-byte array.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

// ---------------------------------------------------------------------------
// Attribute
// ---------------------------------------------------------------------------

/// Interned attribute name backed by `Arc<str>` for O(1) clone and
/// amortized O(1) equality (pointer comparison when the same allocation).
///
/// INV-FERR-026: Attribute names are interned strings. Comparison cost
/// is proportional to the shorter string length, and cloning is a
/// reference-count increment.
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Debug, Serialize, Deserialize)]
pub struct Attribute(Arc<str>);

impl Attribute {
    /// Borrow the attribute name as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for Attribute {
    fn from(s: &str) -> Self {
        Self(Arc::from(s))
    }
}

impl fmt::Display for Attribute {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

// ---------------------------------------------------------------------------
// Value
// ---------------------------------------------------------------------------

/// Sum type representing every value a datom can carry.
///
/// INV-FERR-018: Values are immutable. Heap-allocated variants use `Arc`
/// for cheap cloning without deep copies.
///
/// Derives `Eq`, `Ord`, and `Hash` because the `Double` variant wraps
/// `OrderedFloat<f64>`, which provides total ordering over IEEE 754
/// (NaN == NaN, -0 < +0).
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Debug, Serialize, Deserialize)]
pub enum Value {
    /// Namespaced keyword (e.g. `"db.type/string"`).
    Keyword(Arc<str>),
    /// UTF-8 string value.
    String(Arc<str>),
    /// 64-bit signed integer.
    Long(i64),
    /// 64-bit float with total ordering (via `OrderedFloat`).
    Double(OrderedFloat<f64>),
    /// Boolean value.
    Bool(bool),
    /// Milliseconds since Unix epoch (UTC).
    Instant(i64),
    /// 128-bit UUID stored as raw bytes.
    Uuid([u8; 16]),
    /// Opaque binary blob.
    Bytes(Arc<[u8]>),
    /// Reference to another entity (foreign key).
    Ref(EntityId),
    /// Arbitrary-precision integer (stored as i128).
    BigInt(i128),
    /// Arbitrary-precision decimal (stored as i128; scale defined by schema).
    BigDec(i128),
}

// ---------------------------------------------------------------------------
// Op
// ---------------------------------------------------------------------------

/// Transaction operation: assert a fact into the store, or retract it.
///
/// INV-FERR-018: Retractions are themselves datoms — the store is
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
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Debug, Serialize, Deserialize)]
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
            Value::Double(f) => hash_tagged_fixed(hasher, 3, &f.into_inner().to_le_bytes()),
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

    // -- EntityId ----------------------------------------------------------

    #[test]
    fn test_inv_ferr_012_entity_id_content_addressed() {
        // Same content produces same EntityId.
        let a = EntityId::from_content(b"same content");
        let b = EntityId::from_content(b"same content");
        assert_eq!(
            a, b,
            "INV-FERR-012: identical content must yield identical EntityId"
        );
    }

    #[test]
    fn test_inv_ferr_012_entity_id_different_content() {
        let a = EntityId::from_content(b"alpha");
        let b = EntityId::from_content(b"bravo");
        assert_ne!(
            a, b,
            "INV-FERR-012: distinct content must yield distinct EntityId"
        );
    }

    #[test]
    fn test_entity_id_from_bytes_roundtrip() {
        let bytes = [42u8; 32];
        let eid = EntityId::from_bytes(bytes);
        assert_eq!(*eid.as_bytes(), bytes);
    }

    // -- Attribute ---------------------------------------------------------

    #[test]
    fn test_inv_ferr_026_attribute_from_str() {
        let attr = Attribute::from("db/ident");
        assert_eq!(attr.as_str(), "db/ident");
    }

    #[test]
    fn test_attribute_display() {
        let attr = Attribute::from("user/name");
        assert_eq!(format!("{attr}"), "user/name");
    }

    #[test]
    fn test_attribute_clone_equality() {
        let a = Attribute::from("db/doc");
        let b = a.clone();
        assert_eq!(a, b, "INV-FERR-026: cloned attributes must be equal");
    }

    // -- Value -------------------------------------------------------------

    #[test]
    fn test_value_keyword() {
        let v = Value::Keyword(Arc::from("db.type/string"));
        if let Value::Keyword(k) = &v {
            assert_eq!(&**k, "db.type/string");
        } else {
            panic!("expected Keyword variant");
        }
    }

    #[test]
    fn test_value_string() {
        let v = Value::String(Arc::from("hello world"));
        if let Value::String(s) = &v {
            assert_eq!(&**s, "hello world");
        } else {
            panic!("expected String variant");
        }
    }

    #[test]
    fn test_value_double_total_ordering() {
        let a = Value::Double(OrderedFloat(1.0));
        let b = Value::Double(OrderedFloat(2.0));
        assert!(a < b, "Double must support total ordering via OrderedFloat");
    }

    #[test]
    fn test_value_bytes() {
        let data: Vec<u8> = vec![1, 2, 3, 4];
        let v = Value::Bytes(Arc::from(data));
        if let Value::Bytes(b) = &v {
            assert_eq!(&**b, &[1, 2, 3, 4]);
        } else {
            panic!("expected Bytes variant");
        }
    }

    #[test]
    fn test_value_ref() {
        let eid = EntityId::from_content(b"target");
        let v = Value::Ref(eid);
        if let Value::Ref(r) = &v {
            assert_eq!(*r, eid);
        } else {
            panic!("expected Ref variant");
        }
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

    #[test]
    fn test_inv_ferr_018_datom_clone_independence() {
        let a = sample_datom();
        let b = a.clone();
        // Clone produces an equal but independent datom.
        assert_eq!(a, b);
        // They share Arc-backed data but are logically independent values.
        assert_eq!(a.attribute(), b.attribute());
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
            Value::Double(OrderedFloat(0.0)),
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
