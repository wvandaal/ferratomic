//! Value types carried by datoms.
//!
//! Values are immutable (private fields, no mutation methods).
//! Heap-allocated variants use `Arc` for cheap cloning without deep copies.
//!
//! Attribute names are interned strings with O(1) clone.

use std::{fmt, sync::Arc};

use ordered_float::OrderedFloat;
use serde::{
    de::{Deserializer, Unexpected},
    Deserialize, Serialize,
};

use super::EntityId;

// ---------------------------------------------------------------------------
// Attribute
// ---------------------------------------------------------------------------

/// Interned attribute name backed by `Arc<str>` for O(1) clone.
///
/// Cloning is a reference-count increment. Equality comparison is O(n) in the
/// string length (derived `PartialEq` compares by content, not pointer).
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
// NonNanFloat
// ---------------------------------------------------------------------------

/// A non-NaN 64-bit float with total ordering.
///
/// INV-FERR-012: content-addressed identity requires deterministic hashing.
/// NaN has multiple bit representations, so accepting NaN would break hash
/// determinism. Construction rejects NaN at the boundary (parse, don't validate).
///
/// Wraps `OrderedFloat<f64>` for total ordering (-0 < +0).
/// Serde-transparent: serializes identically to `OrderedFloat<f64>`.
/// CR-003: Manual `Deserialize` impl rejects NaN. Derived `Deserialize`
/// delegated to `OrderedFloat<f64>` which accepts NaN, bypassing the
/// `new()` constructor gate. INV-FERR-012 requires deterministic hashing;
/// NaN has multiple bit representations that break this.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug, Serialize)]
pub struct NonNanFloat(OrderedFloat<f64>);

impl NonNanFloat {
    /// Create a validated non-NaN float (INV-FERR-012).
    ///
    /// Returns `None` if the value is NaN. Parse, don't validate:
    /// once constructed, the inner `f64` is guaranteed non-NaN.
    #[must_use]
    pub fn new(f: f64) -> Option<Self> {
        if f.is_nan() {
            None
        } else {
            Some(Self(OrderedFloat(f)))
        }
    }

    /// The inner `f64` value, guaranteed non-NaN (INV-FERR-012).
    #[must_use]
    pub fn into_inner(self) -> f64 {
        self.0.into_inner()
    }
}

/// CR-003: Custom `Deserialize` that rejects NaN during deserialization.
///
/// INV-FERR-012: Content-addressed identity requires deterministic hashing.
/// `OrderedFloat<f64>::deserialize` accepts NaN, bypassing the `new()`
/// constructor gate. This impl deserializes the f64, then validates.
impl<'de> Deserialize<'de> for NonNanFloat {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let f = OrderedFloat::<f64>::deserialize(deserializer)?;
        if f.into_inner().is_nan() {
            Err(serde::de::Error::invalid_value(
                Unexpected::Float(f64::NAN),
                &"a finite float (NaN rejected per INV-FERR-012)",
            ))
        } else {
            Ok(NonNanFloat(f))
        }
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
/// The `Double` variant wraps [`NonNanFloat`], which rejects NaN at
/// construction and provides total ordering via `OrderedFloat<f64>`.
/// ADR-FERR-010: `Deserialize` is intentionally NOT derived. `Value` contains
/// `EntityId` via the `Ref` variant, so deserialization must go through
/// `WireValue` in the `wire` module to enforce the trust boundary.
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Debug, Serialize)]
pub enum Value {
    /// Namespaced keyword (e.g. `"db.type/string"`).
    Keyword(Arc<str>),
    /// UTF-8 string value.
    String(Arc<str>),
    /// 64-bit signed integer.
    Long(i64),
    /// 64-bit float with total ordering, NaN rejected at construction.
    Double(NonNanFloat),
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
    /// Large integer stored as i128 (ME-015: not truly arbitrary precision;
    /// covers ±170 undecillion, sufficient for Phase 4a).
    BigInt(i128),
    /// Large decimal stored as i128 with scale defined by schema (MI-003:
    /// scale is NOT encoded in the type — two `BigDec` values with the same
    /// i128 but different schema-defined scales compare as equal).
    BigDec(i128),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_attribute_from_str() {
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
        assert_eq!(a, b, "cloned attributes must remain equal");
    }

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
        let a = Value::Double(NonNanFloat::new(1.0).unwrap());
        let b = Value::Double(NonNanFloat::new(2.0).unwrap());
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
}
