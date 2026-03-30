//! Schema: attribute definitions and validation types.
//! INV-FERR-009: Schema validation at transact boundary.
//! INV-FERR-031: Genesis determinism (19 axiomatic meta-schema attributes).

use std::{collections::HashMap, sync::Arc};

use serde::{Deserialize, Serialize};

use crate::datom::Attribute;

/// The type of values an attribute accepts.
/// INV-FERR-009: Each attribute has exactly one declared `ValueType`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
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

/// Cardinality of an attribute.
/// INV-FERR-032: Card-one uses LWW, card-many keeps all non-retracted.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Cardinality {
    /// At most one value per entity-attribute pair (last-writer-wins).
    One,
    /// Multiple values per entity-attribute pair.
    Many,
}

/// Resolution mode for card-one conflicts.
/// Phase 4a: only `Lww` and `MultiValue` are implemented.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ResolutionMode {
    /// Last-writer-wins by `TxId` ordering.
    Lww,
    /// Keep all non-retracted values (card-many behavior).
    MultiValue,
}

/// Definition of a single attribute in the schema.
/// INV-FERR-009: Governs validation at transact boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttributeDef {
    /// What type of values this attribute accepts.
    pub value_type: ValueType,
    /// How many values per entity-attribute pair.
    pub cardinality: Cardinality,
    /// How to resolve conflicts (Phase 4a: `Lww` or `MultiValue` only).
    pub resolution_mode: ResolutionMode,
    /// Human-readable documentation.
    pub doc: Option<Arc<str>>,
}

/// The schema: a mapping from attribute names to their definitions.
/// INV-FERR-009: Schema-as-data. Schema evolution is a transaction.
/// INV-FERR-031: Genesis creates 19 axiomatic attributes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Schema {
    attrs: HashMap<Attribute, AttributeDef>,
}

impl Schema {
    /// Create an empty schema.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            attrs: HashMap::new(),
        }
    }

    /// Create schema from attribute definitions.
    #[must_use]
    pub fn from_attrs(attrs: HashMap<Attribute, AttributeDef>) -> Self {
        Self { attrs }
    }

    /// Look up an attribute definition.
    #[must_use]
    pub fn get(&self, attr: &Attribute) -> Option<&AttributeDef> {
        self.attrs.get(attr)
    }

    /// Check if an attribute is defined in the schema.
    #[must_use]
    pub fn contains(&self, attr: &Attribute) -> bool {
        self.attrs.contains_key(attr)
    }

    /// Number of defined attributes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.attrs.len()
    }

    /// Whether the schema is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.attrs.is_empty()
    }

    /// Insert or update an attribute definition.
    pub fn define(&mut self, attr: Attribute, def: AttributeDef) {
        self.attrs.insert(attr, def);
    }

    /// Iterate over all attribute definitions.
    pub fn iter(&self) -> impl Iterator<Item = (&Attribute, &AttributeDef)> {
        self.attrs.iter()
    }
}
