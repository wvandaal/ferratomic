//! Datom: the atomic fact -- a 5-tuple `[entity, attribute, value, tx, op]`.
//!
//! INV-FERR-012: Content-addressed identity. Two datoms with identical
//! 5-tuples are the same datom. Enforced by Eq/Hash/Ord on all five fields.
//!
//! INV-FERR-018: Append-only. Datoms are immutable after creation.
//! No `&mut` methods. Clone is the only way to "modify" (which creates a new datom).

mod entity;
mod value;

use std::sync::Arc;

pub use entity::EntityId;
use serde::{Deserialize, Serialize};
pub use value::{Attribute, AttributeId, AttributeIntern, NonNanFloat, Value};

use crate::{
    clock::{NodeId, TxId},
    error::FerraError,
};

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

        // Tx: physical, logical, node
        hasher.update(&self.tx.physical().to_le_bytes());
        hasher.update(&self.tx.logical().to_le_bytes());
        hasher.update(self.tx.node().as_bytes());

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

    /// Canonical byte serialization per `INV-FERR-086`.
    ///
    /// Deterministic, self-delimiting, cross-implementation-reproducible.
    /// This is the byte representation that chunk codecs store as the
    /// key in `DatomPairChunk` entries (per `S23.9.0.2`).
    ///
    /// Layout:
    /// - entity: `[u8; 32]`
    /// - attribute: `u16-le` length + UTF-8
    /// - value: `u8` tag + payload (see `INV-FERR-086` value tags)
    /// - tx: `u64-le` physical + `u32-le` logical + `[u8; 16]` node
    /// - op: `u8` (`0x00` = Assert, `0x01` = Retract)
    #[must_use]
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(128);
        buf.extend_from_slice(self.entity.as_bytes());
        canonical_attr_bytes(&self.attribute, &mut buf);
        canonical_value_bytes(&self.value, &mut buf);
        canonical_tx_bytes(&self.tx, &mut buf);
        buf.push(match self.op {
            Op::Assert => 0x00,
            Op::Retract => 0x01,
        });
        buf
    }

    /// Reconstruct a `Datom` from its canonical byte representation.
    ///
    /// Inverse of [`Datom::canonical_bytes`]. Returns `FerraError` on
    /// truncated or malformed input.
    ///
    /// # Errors
    ///
    /// Returns [`FerraError::TruncatedChunk`] if the bytes are too short
    /// to contain a valid datom.
    pub fn from_canonical_bytes(bytes: &[u8]) -> Result<Self, FerraError> {
        let mut offset = 0;
        let entity = parse_entity(bytes, &mut offset)?;
        let attribute = parse_attribute(bytes, &mut offset)?;
        let (value, vlen) = parse_canonical_value(&bytes[offset..])?;
        offset += vlen;
        let tx = parse_tx(bytes, &mut offset)?;
        let op = parse_op(bytes, &mut offset)?;
        Ok(Self {
            entity,
            attribute,
            value,
            tx,
            op,
        })
    }
}

/// INV-FERR-012: Datom identity is determined by content hash.
impl crate::traits::ContentAddressed for Datom {
    fn content_hash(&self) -> [u8; 32] {
        Datom::content_hash(self)
    }
}

/// Parse entity (32 bytes) from canonical bytes.
fn parse_entity(bytes: &[u8], offset: &mut usize) -> Result<EntityId, FerraError> {
    if *offset + 32 > bytes.len() {
        return Err(FerraError::TruncatedChunk);
    }
    let mut b = [0u8; 32];
    b.copy_from_slice(&bytes[*offset..*offset + 32]);
    *offset += 32;
    Ok(EntityId::from_trusted_bytes(b))
}

/// Parse attribute (u16-le length + UTF-8) from canonical bytes.
fn parse_attribute(bytes: &[u8], offset: &mut usize) -> Result<Attribute, FerraError> {
    if *offset + 2 > bytes.len() {
        return Err(FerraError::TruncatedChunk);
    }
    let len = u16::from_le_bytes(
        bytes[*offset..*offset + 2]
            .try_into()
            .map_err(|_| FerraError::TruncatedChunk)?,
    ) as usize;
    *offset += 2;
    if *offset + len > bytes.len() {
        return Err(FerraError::TruncatedChunk);
    }
    let Ok(s) = core::str::from_utf8(&bytes[*offset..*offset + len]) else {
        return Err(FerraError::NonCanonicalChunk);
    };
    *offset += len;
    Ok(Attribute::from(s))
}

/// Parse `TxId` (u64-le + u32-le + [u8; 16] = 28 bytes) from canonical bytes.
fn parse_tx(bytes: &[u8], offset: &mut usize) -> Result<TxId, FerraError> {
    if *offset + 28 > bytes.len() {
        return Err(FerraError::TruncatedChunk);
    }
    let physical = u64::from_le_bytes(
        bytes[*offset..*offset + 8]
            .try_into()
            .map_err(|_| FerraError::TruncatedChunk)?,
    );
    *offset += 8;
    let logical = u32::from_le_bytes(
        bytes[*offset..*offset + 4]
            .try_into()
            .map_err(|_| FerraError::TruncatedChunk)?,
    );
    *offset += 4;
    let mut node = [0u8; 16];
    node.copy_from_slice(&bytes[*offset..*offset + 16]);
    *offset += 16;
    Ok(TxId::with_node(physical, logical, NodeId::from_bytes(node)))
}

/// Parse Op (1 byte) from canonical bytes.
fn parse_op(bytes: &[u8], offset: &mut usize) -> Result<Op, FerraError> {
    if *offset >= bytes.len() {
        return Err(FerraError::TruncatedChunk);
    }
    let op = match bytes[*offset] {
        0x00 => Op::Assert,
        0x01 => Op::Retract,
        _ => return Err(FerraError::NonCanonicalChunk),
    };
    *offset += 1;
    Ok(op)
}

/// Serialize an attribute to canonical bytes: u16-le length + UTF-8.
fn canonical_attr_bytes(attr: &Attribute, buf: &mut Vec<u8>) {
    let b = attr.as_str().as_bytes();
    let len = u16::try_from(b.len()).unwrap_or(u16::MAX);
    buf.extend_from_slice(&len.to_le_bytes());
    buf.extend_from_slice(b);
}

/// Serialize a `TxId` to canonical bytes: u64-le + u32-le + [u8; 16].
fn canonical_tx_bytes(tx: &TxId, buf: &mut Vec<u8>) {
    buf.extend_from_slice(&tx.physical().to_le_bytes());
    buf.extend_from_slice(&tx.logical().to_le_bytes());
    buf.extend_from_slice(tx.node().as_bytes());
}

/// Serialize a `Value` to canonical bytes per `INV-FERR-086` value tag table.
fn canonical_value_bytes(value: &Value, buf: &mut Vec<u8>) {
    match value {
        Value::Keyword(s) => push_tag_u16_str(buf, 0x01, s.as_bytes()),
        Value::String(s) => push_tag_u32_bytes(buf, 0x02, s.as_bytes()),
        Value::Long(n) => {
            buf.push(0x03);
            buf.extend_from_slice(&n.to_le_bytes());
        }
        Value::Double(f) => {
            buf.push(0x04);
            buf.extend_from_slice(&(f.into_inner() + 0.0).to_le_bytes());
        }
        Value::Bool(b) => {
            buf.push(0x05);
            buf.push(u8::from(*b));
        }
        Value::Instant(ms) => {
            buf.push(0x06);
            buf.extend_from_slice(&ms.to_le_bytes());
        }
        Value::Uuid(bytes) => {
            buf.push(0x07);
            buf.extend_from_slice(bytes);
        }
        Value::Bytes(blob) => push_tag_u32_bytes(buf, 0x08, blob),
        Value::Ref(eid) => {
            buf.push(0x09);
            buf.extend_from_slice(eid.as_bytes());
        }
        Value::BigInt(n) => {
            buf.push(0x0A);
            buf.extend_from_slice(&n.to_le_bytes());
        }
        Value::BigDec(n) => {
            buf.push(0x0B);
            buf.extend_from_slice(&n.to_le_bytes());
        }
    }
}

fn push_tag_u16_str(buf: &mut Vec<u8>, tag: u8, data: &[u8]) {
    buf.push(tag);
    let len = u16::try_from(data.len()).unwrap_or(u16::MAX);
    buf.extend_from_slice(&len.to_le_bytes());
    buf.extend_from_slice(data);
}

fn push_tag_u32_bytes(buf: &mut Vec<u8>, tag: u8, data: &[u8]) {
    buf.push(tag);
    let len = u32::try_from(data.len()).unwrap_or(u32::MAX);
    buf.extend_from_slice(&len.to_le_bytes());
    buf.extend_from_slice(data);
}

/// Parse a `Value` from canonical bytes. Returns the value and bytes consumed.
fn parse_canonical_value(bytes: &[u8]) -> Result<(Value, usize), FerraError> {
    if bytes.is_empty() {
        return Err(FerraError::TruncatedChunk);
    }
    let tag = bytes[0];
    let rest = &bytes[1..];
    match tag {
        0x01 | 0x02 | 0x08 => parse_varlen_value(tag, rest),
        0x03..=0x07 | 0x09..=0x0B => parse_fixed_value(tag, rest),
        _ => Err(FerraError::NonCanonicalChunk),
    }
}

/// Parse variable-length value variants (Keyword u16, String u32, Bytes u32).
fn parse_varlen_value(tag: u8, rest: &[u8]) -> Result<(Value, usize), FerraError> {
    match tag {
        0x01 => {
            if rest.len() < 2 {
                return Err(FerraError::TruncatedChunk);
            }
            let len = u16::from_le_bytes(
                rest[..2]
                    .try_into()
                    .map_err(|_| FerraError::TruncatedChunk)?,
            ) as usize;
            if rest.len() < 2 + len {
                return Err(FerraError::TruncatedChunk);
            }
            let s = core::str::from_utf8(&rest[2..2 + len])
                .map_err(|_| FerraError::NonCanonicalChunk)?;
            Ok((Value::Keyword(Arc::from(s)), 1 + 2 + len))
        }
        0x02 => {
            if rest.len() < 4 {
                return Err(FerraError::TruncatedChunk);
            }
            let len = u32::from_le_bytes(
                rest[..4]
                    .try_into()
                    .map_err(|_| FerraError::TruncatedChunk)?,
            ) as usize;
            if rest.len() < 4 + len {
                return Err(FerraError::TruncatedChunk);
            }
            let s = core::str::from_utf8(&rest[4..4 + len])
                .map_err(|_| FerraError::NonCanonicalChunk)?;
            Ok((Value::String(Arc::from(s)), 1 + 4 + len))
        }
        0x08 => {
            if rest.len() < 4 {
                return Err(FerraError::TruncatedChunk);
            }
            let len = u32::from_le_bytes(
                rest[..4]
                    .try_into()
                    .map_err(|_| FerraError::TruncatedChunk)?,
            ) as usize;
            if rest.len() < 4 + len {
                return Err(FerraError::TruncatedChunk);
            }
            Ok((Value::Bytes(Arc::from(&rest[4..4 + len])), 1 + 4 + len))
        }
        _ => Err(FerraError::NonCanonicalChunk),
    }
}

/// Parse fixed-size value variants (Long, Double, Bool, Instant, Uuid, Ref, `BigInt`, `BigDec`).
fn parse_fixed_value(tag: u8, rest: &[u8]) -> Result<(Value, usize), FerraError> {
    match tag {
        0x03 => read_i64(rest).map(|n| (Value::Long(n), 1 + 8)),
        0x04 => {
            let f = read_f64(rest)?;
            let nnf = NonNanFloat::new(f).ok_or(FerraError::NonCanonicalChunk)?;
            Ok((Value::Double(nnf), 1 + 8))
        }
        0x05 => {
            if rest.is_empty() {
                return Err(FerraError::TruncatedChunk);
            }
            Ok((Value::Bool(rest[0] != 0), 1 + 1))
        }
        0x06 => read_i64(rest).map(|n| (Value::Instant(n), 1 + 8)),
        0x07 => {
            if rest.len() < 16 {
                return Err(FerraError::TruncatedChunk);
            }
            let mut uuid = [0u8; 16];
            uuid.copy_from_slice(&rest[..16]);
            Ok((Value::Uuid(uuid), 1 + 16))
        }
        0x09 => {
            if rest.len() < 32 {
                return Err(FerraError::TruncatedChunk);
            }
            let mut eid = [0u8; 32];
            eid.copy_from_slice(&rest[..32]);
            Ok((Value::Ref(EntityId::from_trusted_bytes(eid)), 1 + 32))
        }
        0x0A => read_i128(rest).map(|n| (Value::BigInt(n), 1 + 16)),
        0x0B => read_i128(rest).map(|n| (Value::BigDec(n), 1 + 16)),
        _ => Err(FerraError::NonCanonicalChunk),
    }
}

fn read_i64(rest: &[u8]) -> Result<i64, FerraError> {
    if rest.len() < 8 {
        return Err(FerraError::TruncatedChunk);
    }
    Ok(i64::from_le_bytes(
        rest[..8]
            .try_into()
            .map_err(|_| FerraError::TruncatedChunk)?,
    ))
}

fn read_f64(rest: &[u8]) -> Result<f64, FerraError> {
    if rest.len() < 8 {
        return Err(FerraError::TruncatedChunk);
    }
    Ok(f64::from_le_bytes(
        rest[..8]
            .try_into()
            .map_err(|_| FerraError::TruncatedChunk)?,
    ))
}

fn read_i128(rest: &[u8]) -> Result<i128, FerraError> {
    if rest.len() < 16 {
        return Err(FerraError::TruncatedChunk);
    }
    Ok(i128::from_le_bytes(
        rest[..16]
            .try_into()
            .map_err(|_| FerraError::TruncatedChunk)?,
    ))
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
    fn test_inv_ferr_012_content_hash_sensitive_to_entity() {
        let a1 = Attribute::from("test/a");
        let v = Value::Long(1);
        let tx = TxId::new(1, 0, 0);
        let d1 = Datom::new(
            EntityId::from_content(b"entity-A"),
            a1.clone(),
            v.clone(),
            tx,
            Op::Assert,
        );
        let d2 = Datom::new(EntityId::from_content(b"entity-B"), a1, v, tx, Op::Assert);
        assert_ne!(
            d1.content_hash(),
            d2.content_hash(),
            "INV-FERR-012: different entities must produce different content hashes"
        );
    }

    #[test]
    fn test_inv_ferr_012_content_hash_sensitive_to_attribute() {
        let e = EntityId::from_content(b"e");
        let v = Value::Long(1);
        let tx = TxId::new(1, 0, 0);
        let d1 = Datom::new(e, Attribute::from("attr/one"), v.clone(), tx, Op::Assert);
        let d2 = Datom::new(e, Attribute::from("attr/two"), v, tx, Op::Assert);
        assert_ne!(
            d1.content_hash(),
            d2.content_hash(),
            "INV-FERR-012: different attributes must produce different content hashes"
        );
    }

    #[test]
    fn test_inv_ferr_012_content_hash_sensitive_to_tx() {
        let e = EntityId::from_content(b"e");
        let a = Attribute::from("a");
        let v = Value::Long(1);
        let d1 = Datom::new(e, a.clone(), v.clone(), TxId::new(1, 0, 0), Op::Assert);
        let d2 = Datom::new(e, a, v, TxId::new(2, 0, 0), Op::Assert);
        assert_ne!(
            d1.content_hash(),
            d2.content_hash(),
            "INV-FERR-012: different TxIds must produce different content hashes"
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

    // -- INV-FERR-086: canonical_bytes round-trip ----------------------------

    /// Helper: build a datom with a specific Value variant for round-trip testing.
    fn datom_with_value(value: Value) -> Datom {
        Datom::new(
            EntityId::from_content(b"test"),
            Attribute::from("test/attr"),
            value,
            TxId::new(100, 1, 42),
            Op::Assert,
        )
    }

    /// `INV-FERR-086`: `canonical_bytes` round-trip for all 11 `Value` variants.
    /// Each variant must survive serialize then deserialize without loss.
    #[test]
    fn test_inv_ferr_086_canonical_bytes_round_trip_all_variants() {
        let variants = vec![
            datom_with_value(Value::Keyword(Arc::from("test/kw"))),
            datom_with_value(Value::String(Arc::from("hello world"))),
            datom_with_value(Value::Long(42)),
            datom_with_value(Value::Long(-1)),
            datom_with_value(Value::Long(i64::MAX)),
            datom_with_value(Value::Double(
                NonNanFloat::new(1.234_567_89).expect("not NaN"),
            )),
            datom_with_value(Value::Double(NonNanFloat::new(0.0).expect("not NaN"))),
            datom_with_value(Value::Bool(true)),
            datom_with_value(Value::Bool(false)),
            datom_with_value(Value::Instant(1_700_000_000_000)),
            datom_with_value(Value::Uuid([0xAB; 16])),
            datom_with_value(Value::Bytes(Arc::from(vec![1u8, 2, 3, 4].as_slice()))),
            datom_with_value(Value::Bytes(Arc::from(Vec::<u8>::new().as_slice()))),
            datom_with_value(Value::Ref(EntityId::from_content(b"ref target"))),
            datom_with_value(Value::BigInt(i128::MAX)),
            datom_with_value(Value::BigInt(i128::MIN)),
            datom_with_value(Value::BigDec(123_456_789_012_345)),
        ];

        for (i, d) in variants.iter().enumerate() {
            let bytes = d.canonical_bytes();
            let recovered = Datom::from_canonical_bytes(&bytes).unwrap_or_else(|e| {
                panic!(
                    "INV-FERR-086: variant {i} ({:?}) failed to parse: {e}",
                    d.value()
                )
            });
            assert_eq!(
                &recovered,
                d,
                "INV-FERR-086: variant {i} round-trip failed for {:?}",
                d.value()
            );
        }
    }

    /// INV-FERR-086: Assert vs Retract round-trips correctly.
    #[test]
    fn test_inv_ferr_086_canonical_bytes_op_round_trip() {
        let assert_datom = datom_with_value(Value::Long(1));
        let retract_datom = Datom::new(
            assert_datom.entity(),
            assert_datom.attribute().clone(),
            assert_datom.value().clone(),
            assert_datom.tx(),
            Op::Retract,
        );

        let a_bytes = assert_datom.canonical_bytes();
        let r_bytes = retract_datom.canonical_bytes();
        assert_ne!(
            a_bytes, r_bytes,
            "Assert and Retract must produce different bytes"
        );

        let a_recovered = Datom::from_canonical_bytes(&a_bytes).expect("assert round-trip");
        let r_recovered = Datom::from_canonical_bytes(&r_bytes).expect("retract round-trip");
        assert_eq!(a_recovered.op(), Op::Assert);
        assert_eq!(r_recovered.op(), Op::Retract);
    }

    /// `INV-FERR-086`: `canonical_bytes` is deterministic.
    #[test]
    fn test_inv_ferr_086_canonical_bytes_deterministic() {
        let d = sample_datom();
        assert_eq!(d.canonical_bytes(), d.canonical_bytes());
    }

    /// INV-FERR-086: distinct datoms produce distinct canonical bytes (injectivity).
    #[test]
    fn test_inv_ferr_086_canonical_bytes_injective() {
        let d1 = datom_with_value(Value::Long(1));
        let d2 = datom_with_value(Value::Long(2));
        assert_ne!(
            d1.canonical_bytes(),
            d2.canonical_bytes(),
            "INV-FERR-086: distinct datoms must have distinct canonical bytes"
        );
    }

    /// INV-FERR-086: truncated canonical bytes are rejected.
    #[test]
    fn test_inv_ferr_086_from_canonical_bytes_rejects_truncated() {
        let d = sample_datom();
        let bytes = d.canonical_bytes();
        // Truncate at various points — all must fail
        for len in [0, 1, 31, 32, 33, bytes.len() - 1] {
            assert!(
                Datom::from_canonical_bytes(&bytes[..len]).is_err(),
                "INV-FERR-086: truncation at {len} bytes must be rejected"
            );
        }
    }
}
