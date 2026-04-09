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
// AttributeId + AttributeIntern (ADR-FERR-030 prerequisite, bd-fnod)
// ---------------------------------------------------------------------------

/// Interned attribute identifier (ADR-FERR-030 prerequisite).
///
/// `Copy + Ord + Hash`. 2 bytes. Comparison is integer comparison (1 cycle).
/// IDs are assigned in lexicographic (sorted) order so that
/// `AttributeId::Ord` is isomorphic to `Attribute::Ord`.
///
/// The string name is recoverable via `AttributeIntern::resolve`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AttributeId(u16);

impl AttributeId {
    /// Raw numeric ID (for serialization or debugging).
    #[must_use]
    pub fn as_u16(self) -> u16 {
        self.0
    }
}

/// Bidirectional intern table for attribute names (ADR-FERR-030).
///
/// Assigns IDs in lexicographic order: if `a < b` as strings, then
/// `intern(a) < intern(b)` as `AttributeId`. This preserves the
/// `Attribute::Ord` isomorphism so that index key comparisons using
/// `AttributeId` produce the same ordering as string comparisons.
///
/// Append-only: attributes are never removed. Rebuilt on schema
/// evolution (rare) to maintain sorted ID assignment.
#[derive(Debug, Clone)]
pub struct AttributeIntern {
    /// name → id (O(log A) lookup).
    to_id: std::collections::BTreeMap<Attribute, AttributeId>,
    /// id → name (O(1) lookup by index).
    to_name: Vec<Arc<str>>,
}

impl AttributeIntern {
    /// Build an intern table from a set of attributes.
    ///
    /// Assigns IDs in sorted (lexicographic) order so that
    /// `AttributeId::Ord` is isomorphic to `Attribute::Ord`.
    ///
    /// # Errors
    ///
    /// Returns `FerraError::InvariantViolation` if more than 65,535
    /// distinct attributes are provided (u16 capacity exceeded).
    pub fn from_attributes(
        attrs: impl IntoIterator<Item = Attribute>,
    ) -> Result<Self, super::super::FerraError> {
        use std::collections::BTreeSet;
        let sorted: BTreeSet<Attribute> = attrs.into_iter().collect();
        // u16 holds 0..65535 = 65536 distinct IDs.
        if sorted.len() > usize::from(u16::MAX) + 1 {
            return Err(super::super::FerraError::InvariantViolation {
                invariant: "ADR-FERR-030".to_string(),
                details: format!(
                    "attribute count {} exceeds u16 capacity (max 65536)",
                    sorted.len(),
                ),
            });
        }
        let mut to_id = std::collections::BTreeMap::new();
        let mut to_name = Vec::with_capacity(sorted.len());
        for (i, attr) in sorted.into_iter().enumerate() {
            let id = AttributeId(u16::try_from(i).map_err(|_| {
                super::super::FerraError::InvariantViolation {
                    invariant: "ADR-FERR-030".to_string(),
                    details: "attribute index exceeds u16 range".to_string(),
                }
            })?);
            to_id.insert(attr.clone(), id);
            to_name.push(Arc::from(attr.as_str()));
        }
        Ok(Self { to_id, to_name })
    }

    /// Build from a `Schema` (convenience: extracts attribute names).
    ///
    /// # Errors
    ///
    /// Returns `FerraError::InvariantViolation` if the schema has more
    /// than 65,535 attributes (u16 capacity).
    pub fn from_schema(schema: &super::super::Schema) -> Result<Self, super::super::FerraError> {
        Self::from_attributes(schema.iter().map(|(a, _)| a.clone()))
    }

    /// Look up the ID for an attribute. Returns `None` if not interned.
    #[must_use]
    pub fn id_of(&self, attr: &Attribute) -> Option<AttributeId> {
        self.to_id.get(attr).copied()
    }

    /// Intern an attribute, assigning a new ID if not present.
    ///
    /// New IDs are appended at the END (not in sorted position),
    /// so the sorted-ordering invariant is violated until the table
    /// is rebuilt via `from_attributes`. Use this only for transient
    /// lookups where ordering does not matter.
    ///
    /// # Errors
    ///
    /// Returns `FerraError::InvariantViolation` if u16 capacity exceeded.
    pub fn intern(&mut self, attr: &Attribute) -> Result<AttributeId, super::super::FerraError> {
        if let Some(&id) = self.to_id.get(attr) {
            return Ok(id);
        }
        let id_raw = u16::try_from(self.to_name.len()).map_err(|_| {
            super::super::FerraError::InvariantViolation {
                invariant: "ADR-FERR-030".to_string(),
                details: "attribute intern table exceeded u16 capacity".to_string(),
            }
        })?;
        let id = AttributeId(id_raw);
        self.to_id.insert(attr.clone(), id);
        self.to_name.push(Arc::from(attr.as_str()));
        Ok(id)
    }

    /// Resolve an ID back to its string name. O(1).
    ///
    /// Returns `None` if the ID is out of range (should not happen
    /// for IDs obtained from this table).
    #[must_use]
    pub fn resolve(&self, id: AttributeId) -> Option<&str> {
        self.to_name.get(usize::from(id.0)).map(AsRef::as_ref)
    }

    /// Number of interned attributes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.to_name.len()
    }

    /// Whether the intern table is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.to_name.is_empty()
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

    // -----------------------------------------------------------------------
    // AttributeId + AttributeIntern tests (ADR-FERR-030)
    // -----------------------------------------------------------------------

    #[test]
    fn test_adr_ferr_030_attribute_id_is_copy() {
        let id = AttributeId(42);
        let copy = id;
        assert_eq!(id, copy, "ADR-FERR-030: AttributeId must be Copy");
    }

    #[test]
    fn test_adr_ferr_030_intern_roundtrip() {
        let attrs = vec![
            Attribute::from("db/doc"),
            Attribute::from("db/ident"),
            Attribute::from("tx/time"),
        ];
        let table = AttributeIntern::from_attributes(attrs.clone()).expect("intern must succeed");
        for attr in &attrs {
            let id = table.id_of(attr).expect("attribute must be interned");
            let resolved = table.resolve(id).expect("id must resolve");
            assert_eq!(resolved, attr.as_str(), "ADR-FERR-030: roundtrip failed");
        }
    }

    #[test]
    fn test_adr_ferr_030_sorted_id_assignment() {
        let attrs = vec![
            Attribute::from("z/last"),
            Attribute::from("a/first"),
            Attribute::from("m/middle"),
        ];
        let table = AttributeIntern::from_attributes(attrs).expect("intern must succeed");
        let id_a = table.id_of(&Attribute::from("a/first")).expect("a");
        let id_m = table.id_of(&Attribute::from("m/middle")).expect("m");
        let id_z = table.id_of(&Attribute::from("z/last")).expect("z");
        assert!(
            id_a < id_m && id_m < id_z,
            "ADR-FERR-030: IDs must be in sorted string order: a={id_a:?} m={id_m:?} z={id_z:?}"
        );
    }

    #[test]
    fn test_adr_ferr_030_intern_append() {
        let attrs = vec![Attribute::from("a"), Attribute::from("b")];
        let mut table = AttributeIntern::from_attributes(attrs).expect("intern");
        assert_eq!(table.len(), 2);
        let id_c = table.intern(&Attribute::from("c")).expect("intern c");
        assert_eq!(table.len(), 3);
        assert_eq!(
            table.resolve(id_c),
            Some("c"),
            "ADR-FERR-030: dynamically interned attribute must resolve"
        );
        // Re-interning returns same ID.
        let id_c2 = table.intern(&Attribute::from("c")).expect("re-intern c");
        assert_eq!(id_c, id_c2, "ADR-FERR-030: re-intern must return same ID");
    }

    #[test]
    fn test_adr_ferr_030_genesis_25_attributes() {
        // The genesis schema has exactly 25 attributes (19 original + 6 federation
        // metadata from Phase 4a.5). Verify the intern table can accommodate them
        // with deterministic IDs.
        let genesis_attrs: Vec<Attribute> = vec![
            "db/cardinality",
            "db/doc",
            "db/ident",
            "db/isComponent",
            "db/latticeOrder",
            "db/lwwClock",
            "db/resolutionMode",
            "db/unique",
            "db/valueType",
            "lattice/bottom",
            "lattice/comparator",
            "lattice/elements",
            "lattice/ident",
            "lattice/top",
            "tx/derivation-input",
            "tx/derivation-rule",
            "tx/derivation-source",
            "tx/origin",
            "tx/predecessor",
            "tx/provenance",
            "tx/rationale",
            "tx/signature",
            "tx/signer",
            "tx/time",
            "tx/validation-override",
        ]
        .into_iter()
        .map(Attribute::from)
        .collect();

        let table = AttributeIntern::from_attributes(genesis_attrs.clone())
            .expect("genesis intern must succeed");
        assert_eq!(table.len(), 25, "ADR-FERR-030: genesis has 25 attributes");

        // IDs must be 0..24 in sorted order.
        for (i, attr) in genesis_attrs.iter().enumerate() {
            let id = table
                .id_of(attr)
                .expect("genesis attribute must be interned");
            assert_eq!(
                id.as_u16(),
                u16::try_from(i).unwrap_or(u16::MAX),
                "ADR-FERR-030: genesis attribute '{attr}' should have ID {i}, got {}",
                id.as_u16()
            );
        }
    }

    #[test]
    fn test_adr_ferr_030_overflow_rejected() {
        // u16 max + 2 = 65537 attributes should be rejected
        let attrs = (0u32..65537).map(|i| Attribute::from(format!("attr/{i}").as_str()));
        let result = AttributeIntern::from_attributes(attrs);
        assert!(
            result.is_err(),
            "ADR-FERR-030: >65536 attributes must be rejected"
        );
    }

    #[test]
    fn test_adr_ferr_030_intern_breaks_sorted_order() {
        let attrs = vec![Attribute::from("a"), Attribute::from("c")];
        let mut table = AttributeIntern::from_attributes(attrs).expect("intern");
        let id_a = table.id_of(&Attribute::from("a")).expect("a");
        let id_c = table.id_of(&Attribute::from("c")).expect("c");
        // Dynamically intern "b" -- gets appended, NOT sorted between a and c
        let id_b = table.intern(&Attribute::from("b")).expect("b");
        assert!(
            id_b > id_c,
            "ADR-FERR-030: intern() appends -- b gets ID after c, breaking sort"
        );
        // But a < c still holds from original sorted construction
        assert!(
            id_a < id_c,
            "ADR-FERR-030: original sorted order preserved for a < c"
        );
    }

    use proptest::prelude::*;

    proptest! {
        #[test]
        fn test_adr_ferr_030_intern_roundtrip_proptest(
            names in prop::collection::hash_set("[a-z]{1,10}/[a-z]{1,10}", 1..100),
        ) {
            let attrs: Vec<Attribute> = names.iter().map(|s| Attribute::from(s.as_str())).collect();
            let table = AttributeIntern::from_attributes(attrs.clone()).expect("intern");
            for attr in &attrs {
                let id = table.id_of(attr).expect("must be interned");
                let resolved = table.resolve(id).expect("must resolve");
                prop_assert_eq!(resolved, attr.as_str());
            }
        }
    }
}
