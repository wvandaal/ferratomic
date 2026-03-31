//! Value types carried by datoms.
//!
//! INV-FERR-018: Values are immutable. Heap-allocated variants use `Arc`
//! for cheap cloning without deep copies.
//!
//! INV-FERR-026: Attribute names are interned strings with O(1) clone.

use std::{fmt, sync::Arc};

use ordered_float::OrderedFloat;
use serde::{Deserialize, Serialize};

use super::EntityId;

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
    /// Borrow the attribute name as a string slice (INV-FERR-026).
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
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug, Serialize, Deserialize)]
pub struct NonNanFloat(OrderedFloat<f64>);

impl NonNanFloat {
    /// Create a validated non-NaN float (INV-FERR-012).
    ///
    /// Returns `None` if the value is NaN. Parse, don't validate:
    /// once constructed, the inner `f64` is guaranteed non-NaN.
    #[must_use]
    pub fn new(f: f64) -> Option<Self> {
        if f.is_nan() { None } else { Some(Self(OrderedFloat(f))) }
    }

    /// The inner `f64` value, guaranteed non-NaN (INV-FERR-012).
    #[must_use]
    pub fn into_inner(self) -> f64 {
        self.0.into_inner()
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
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Debug, Serialize, Deserialize)]
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
    /// Arbitrary-precision integer (stored as i128).
    BigInt(i128),
    /// Arbitrary-precision decimal (stored as i128; scale defined by schema).
    BigDec(i128),
}

#[cfg(test)]
mod tests {
    use super::*;

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
