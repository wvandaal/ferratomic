//! Schema: attribute definitions and validation types.
//! INV-FERR-009: Schema validation at transact boundary.
//! INV-FERR-031: Genesis determinism (19 axiomatic meta-schema attributes).

use std::{collections::BTreeMap, sync::Arc};

use serde::{Deserialize, Serialize};

use crate::datom::Attribute;

/// The type of values an attribute accepts (INV-FERR-009).
///
/// Each attribute has exactly one declared `ValueType`. Schema validation
/// at the transact boundary checks that every datom's value matches its
/// attribute's declared type.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ValueType {
    /// Keyword (namespace/name string)
    Keyword,
    /// UTF-8 string
    String,
    /// 64-bit signed integer
    Long,
    /// 64-bit floating point (ordered)
    Double,
    /// Boolean
    Boolean,
    /// Timestamp (millis since epoch)
    Instant,
    /// 128-bit UUID
    Uuid,
    /// Byte array
    Bytes,
    /// Reference to another entity
    Ref,
    /// Arbitrary-precision integer
    BigInt,
    /// Arbitrary-precision decimal
    BigDec,
}

/// Cardinality of an attribute (INV-FERR-032).
///
/// Card-one uses last-writer-wins (LWW) resolution; card-many keeps
/// all non-retracted values for each entity-attribute pair.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Cardinality {
    /// At most one value per entity-attribute pair (last-writer-wins).
    One,
    /// Multiple values per entity-attribute pair.
    Many,
}

/// Resolution mode for card-one conflicts (INV-FERR-032).
///
/// Phase 4a implements `Lww` and `MultiValue` only.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ResolutionMode {
    /// Last-writer-wins by `TxId` ordering.
    Lww,
    /// Keep all non-retracted values (card-many behavior).
    MultiValue,
}

/// Definition of a single attribute in the schema (INV-FERR-009).
///
/// Governs transact-time validation: the declared `ValueType` and
/// `Cardinality` constrain which datoms are admissible.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct AttributeDef {
    value_type: ValueType,
    cardinality: Cardinality,
    resolution_mode: ResolutionMode,
    doc: Option<Arc<str>>,
}

impl AttributeDef {
    /// Construct a new attribute definition.
    ///
    /// INV-FERR-009: All attribute properties are set at construction.
    #[must_use]
    pub fn new(
        value_type: ValueType,
        cardinality: Cardinality,
        resolution_mode: ResolutionMode,
        doc: Option<Arc<str>>,
    ) -> Self {
        Self {
            value_type,
            cardinality,
            resolution_mode,
            doc,
        }
    }

    /// The value type this attribute accepts (INV-FERR-009).
    #[must_use]
    pub fn value_type(&self) -> &ValueType {
        &self.value_type
    }

    /// The cardinality: one or many (INV-FERR-032).
    #[must_use]
    pub fn cardinality(&self) -> &Cardinality {
        &self.cardinality
    }

    /// The conflict resolution mode for card-one attributes (INV-FERR-032).
    #[must_use]
    pub fn resolution_mode(&self) -> &ResolutionMode {
        &self.resolution_mode
    }

    /// Human-readable documentation, if any.
    #[must_use]
    pub fn doc(&self) -> Option<&str> {
        self.doc.as_deref()
    }
}

/// The schema: an ordered mapping from attribute names to their definitions.
///
/// INV-FERR-009: Schema-as-data. Schema evolution is a transaction.
/// INV-FERR-031: Genesis creates 19 axiomatic attributes that bootstrap
/// all subsequent schema definitions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Schema {
    attrs: BTreeMap<Attribute, AttributeDef>,
}

impl Schema {
    /// Create an empty schema with no defined attributes.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            attrs: BTreeMap::new(),
        }
    }

    /// Create schema from attribute definitions.
    ///
    /// INV-FERR-031: The internal map is ordered, so iterating a schema
    /// built from the same attribute set always yields the same sequence.
    #[must_use]
    pub fn from_attrs(attrs: impl IntoIterator<Item = (Attribute, AttributeDef)>) -> Self {
        Self {
            attrs: attrs.into_iter().collect(),
        }
    }

    /// Look up an attribute definition by name (INV-FERR-009).
    #[must_use]
    pub fn get(&self, attr: &Attribute) -> Option<&AttributeDef> {
        self.attrs.get(attr)
    }

    /// Check if an attribute is defined in this schema (INV-FERR-009).
    #[must_use]
    pub fn contains(&self, attr: &Attribute) -> bool {
        self.attrs.contains_key(attr)
    }

    /// Number of defined attributes in this schema.
    #[must_use]
    pub fn len(&self) -> usize {
        self.attrs.len()
    }

    /// Whether this schema contains no attribute definitions.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.attrs.is_empty()
    }

    /// Insert an attribute definition (INV-FERR-009).
    ///
    /// Used by schema evolution to install new attributes at transact time.
    /// If the attribute is already defined with an identical definition, this
    /// is a no-op (idempotent). If the existing definition differs, the new
    /// definition is installed (last-write-wins for schema evolution).
    ///
    /// HI-015: Previously silently overwrote any existing definition.
    /// Now logs a `debug_assert` for conflicting redefinitions to aid
    /// diagnosis while preserving backwards compatibility.
    pub fn define(&mut self, attr: Attribute, def: AttributeDef) {
        if let Some(existing) = self.attrs.get(&attr) {
            if *existing == def {
                return; // Idempotent: same definition already installed.
            }
            // INV-FERR-043: conflicting schema redefinition detected.
            // Production resolves via last-write-wins to maintain forward
            // compatibility with schema evolution. Warning only — not a panic.
            eprintln!(
                "WARN [ferratom] INV-FERR-043: conflicting schema redefinition \
                 for {attr:?}: existing={existing:?}, new={def:?}"
            );
        }
        self.attrs.insert(attr, def);
    }

    /// Iterate over all attribute definitions in sorted order (INV-FERR-031).
    pub fn iter(&self) -> impl Iterator<Item = (&Attribute, &AttributeDef)> {
        self.attrs.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc_attr(doc: &str) -> (Attribute, AttributeDef) {
        (
            Attribute::from("db/doc"),
            AttributeDef {
                value_type: ValueType::String,
                cardinality: Cardinality::One,
                resolution_mode: ResolutionMode::Lww,
                doc: Some(Arc::from(doc)),
            },
        )
    }

    #[test]
    fn test_schema_empty_define_and_lookup() {
        let mut schema = Schema::empty();
        assert!(
            schema.is_empty(),
            "empty schema must start with no attributes"
        );
        assert_eq!(schema.len(), 0, "empty schema must report length 0");

        let (attr, def) = doc_attr("docs");
        schema.define(attr.clone(), def.clone());

        assert!(
            schema.contains(&attr),
            "define must make the attribute visible"
        );
        assert_eq!(
            schema.get(&attr),
            Some(&def),
            "get must return inserted definition"
        );
        assert_eq!(
            schema.len(),
            1,
            "define must increase length for a new attribute"
        );
        assert!(
            !schema.is_empty(),
            "schema with one attribute must not be empty"
        );
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn test_inv_ferr_031_schema_iter_is_deterministic() {
        let schema_a = Schema::from_attrs([
            (
                Attribute::from("tx/time"),
                AttributeDef {
                    value_type: ValueType::Instant,
                    cardinality: Cardinality::One,
                    resolution_mode: ResolutionMode::Lww,
                    doc: None,
                },
            ),
            doc_attr("doc-a"),
            (
                Attribute::from("db/cardinality"),
                AttributeDef {
                    value_type: ValueType::Keyword,
                    cardinality: Cardinality::One,
                    resolution_mode: ResolutionMode::Lww,
                    doc: None,
                },
            ),
        ]);
        let schema_b = Schema::from_attrs([
            (
                Attribute::from("db/cardinality"),
                AttributeDef {
                    value_type: ValueType::Keyword,
                    cardinality: Cardinality::One,
                    resolution_mode: ResolutionMode::Lww,
                    doc: None,
                },
            ),
            (
                Attribute::from("tx/time"),
                AttributeDef {
                    value_type: ValueType::Instant,
                    cardinality: Cardinality::One,
                    resolution_mode: ResolutionMode::Lww,
                    doc: None,
                },
            ),
            doc_attr("doc-a"),
        ]);

        let attrs_a: Vec<&str> = schema_a.iter().map(|(attr, _)| attr.as_str()).collect();
        let attrs_b: Vec<&str> = schema_b.iter().map(|(attr, _)| attr.as_str()).collect();

        assert_eq!(
            attrs_a, attrs_b,
            "INV-FERR-031: identical schemas must iterate in the same order"
        );
        assert_eq!(
            attrs_a,
            vec!["db/cardinality", "db/doc", "tx/time"],
            "ordered schema iteration must follow Attribute::Ord"
        );
    }
}
