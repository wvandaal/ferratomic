//! Schema creation and evolution helpers.
//!
//! INV-FERR-009: Schema validation at transact boundary.
//! INV-FERR-031: Genesis determinism — 19 axiomatic meta-schema attributes.
//!
//! This module contains:
//! - The genesis meta-schema (19 axiomatic attributes)
//! - Schema evolution logic (transact-time attribute installation)
//! - Value type and cardinality parsing from datom keywords

use std::collections::HashMap;
use std::sync::Arc;

use ferratom::{
    Attribute, AttributeDef, Cardinality, Datom, EntityId, FerraError, ResolutionMode, Schema,
    ValueType,
};

/// Build the deterministic genesis meta-schema with 19 axiomatic attributes.
///
/// INV-FERR-031: every call produces an identical schema. These 19
/// attributes are the ONLY hardcoded elements in the engine. Every
/// other attribute is defined by transacting datoms that reference
/// these 19. This is the schema-as-data bootstrap (C3, C7).
#[must_use]
#[allow(clippy::too_many_lines)] // 19 attribute definitions are inherently verbose
pub fn genesis_schema() -> Schema {
    let mut schema = Schema::empty();

    let lww_kw = |doc: &str| AttributeDef::new(
        ValueType::Keyword, Cardinality::One, ResolutionMode::Lww, Some(Arc::from(doc)),
    );
    let lww_str = |doc: &str| AttributeDef::new(
        ValueType::String, Cardinality::One, ResolutionMode::Lww, Some(Arc::from(doc)),
    );
    let lww_bool = |doc: &str| AttributeDef::new(
        ValueType::Boolean, Cardinality::One, ResolutionMode::Lww, Some(Arc::from(doc)),
    );
    let lww_ref = |doc: &str| AttributeDef::new(
        ValueType::Ref, Cardinality::One, ResolutionMode::Lww, Some(Arc::from(doc)),
    );
    let lww_instant = |doc: &str| AttributeDef::new(
        ValueType::Instant, Cardinality::One, ResolutionMode::Lww, Some(Arc::from(doc)),
    );

    // 1-9: db/* attributes (meta-schema)
    schema.define(Attribute::from("db/ident"), lww_kw("Attribute identity keyword"));
    schema.define(Attribute::from("db/valueType"), lww_kw("Declared value type"));
    schema.define(Attribute::from("db/cardinality"), lww_kw("Cardinality: one or many"));
    schema.define(Attribute::from("db/doc"), lww_str("Documentation string"));
    schema.define(Attribute::from("db/unique"), lww_kw("Uniqueness constraint"));
    schema.define(Attribute::from("db/isComponent"), lww_bool("Component ownership"));
    schema.define(Attribute::from("db/resolutionMode"), lww_kw("CRDT conflict resolution mode"));
    schema.define(Attribute::from("db/latticeOrder"), lww_ref("Reference to lattice definition"));
    schema.define(Attribute::from("db/lwwClock"), lww_kw("LWW clock source"));

    // 10-14: lattice/* attributes (lattice definitions)
    schema.define(Attribute::from("lattice/ident"), lww_kw("Lattice name"));
    schema.define(Attribute::from("lattice/elements"), lww_str("Ordered element list"));
    schema.define(Attribute::from("lattice/comparator"), lww_str("Comparison function"));
    schema.define(Attribute::from("lattice/bottom"), lww_kw("Least element"));
    schema.define(Attribute::from("lattice/top"), lww_kw("Greatest element"));

    // 15-19: tx/* attributes (transaction metadata)
    schema.define(Attribute::from("tx/time"), lww_instant("Transaction wall-clock time"));
    schema.define(Attribute::from("tx/agent"), lww_ref("Agent that created transaction"));
    schema.define(Attribute::from("tx/provenance"), lww_str("Provenance description"));
    schema.define(Attribute::from("tx/rationale"), lww_str("Why this transaction exists"));
    schema.define(Attribute::from("tx/coherence-override"), lww_str("Manual coherence exemption"));

    schema
}

/// Scan datoms for schema-defining patterns and install new attributes.
///
/// INV-FERR-009: schema evolution is a transaction. When a transaction
/// contains datoms with `db/ident`, `db/valueType`, and `db/cardinality`
/// all sharing the same entity, a new attribute is installed.
///
/// # Errors
///
/// Returns `FerraError::SchemaViolation` if a single entity carries
/// conflicting values for `db/ident`, `db/valueType`, or `db/cardinality`
/// within the same transaction (bd-ty5 / CR-037).
pub fn evolve_schema(schema: &mut Schema, datoms: &[Datom]) -> Result<(), FerraError> {
    let mut by_entity: HashMap<EntityId, Vec<&Datom>> = HashMap::new();
    for datom in datoms {
        by_entity.entry(datom.entity()).or_default().push(datom);
    }

    let db_ident = Attribute::from("db/ident");
    let db_value_type = Attribute::from("db/valueType");
    let db_cardinality = Attribute::from("db/cardinality");

    for entity_datoms in by_entity.values() {
        let mut ident: Option<&str> = None;
        let mut value_type: Option<ValueType> = None;
        let mut cardinality: Option<Cardinality> = None;

        for datom in entity_datoms {
            if datom.attribute() == &db_ident {
                if let ferratom::Value::Keyword(kw) = datom.value() {
                    let new_val = kw.as_ref();
                    if let Some(prev) = ident {
                        if prev != new_val {
                            return Err(FerraError::SchemaViolation {
                                attribute: "db/ident".to_string(),
                                expected: prev.to_string(),
                                got: new_val.to_string(),
                            });
                        }
                    }
                    ident = Some(new_val);
                }
            } else if datom.attribute() == &db_value_type {
                if let ferratom::Value::Keyword(kw) = datom.value() {
                    if let Some(new_vt) = parse_value_type(kw) {
                        if let Some(ref prev_vt) = value_type {
                            if *prev_vt != new_vt {
                                return Err(FerraError::SchemaViolation {
                                    attribute: "db/valueType".to_string(),
                                    expected: format!("{prev_vt:?}"),
                                    got: format!("{new_vt:?}"),
                                });
                            }
                        }
                        value_type = Some(new_vt);
                    }
                }
            } else if datom.attribute() == &db_cardinality {
                if let ferratom::Value::Keyword(kw) = datom.value() {
                    if let Some(new_card) = parse_cardinality(kw) {
                        if let Some(ref prev_card) = cardinality {
                            if *prev_card != new_card {
                                return Err(FerraError::SchemaViolation {
                                    attribute: "db/cardinality".to_string(),
                                    expected: format!("{prev_card:?}"),
                                    got: format!("{new_card:?}"),
                                });
                            }
                        }
                        cardinality = Some(new_card);
                    }
                }
            }
        }

        if let (Some(name), Some(vt), Some(card)) = (ident, value_type, cardinality) {
            schema.define(
                Attribute::from(name),
                AttributeDef::new(vt, card, ResolutionMode::Lww, None),
            );
        }
    }
    Ok(())
}

/// Parse a `db.type/*` keyword into a `ValueType`.
///
/// Returns `None` for unrecognized type keywords.
#[must_use]
pub fn parse_value_type(keyword: &str) -> Option<ValueType> {
    match keyword {
        "db.type/keyword" => Some(ValueType::Keyword),
        "db.type/string" => Some(ValueType::String),
        "db.type/long" => Some(ValueType::Long),
        "db.type/double" => Some(ValueType::Double),
        "db.type/boolean" => Some(ValueType::Boolean),
        "db.type/instant" => Some(ValueType::Instant),
        "db.type/uuid" => Some(ValueType::Uuid),
        "db.type/bytes" => Some(ValueType::Bytes),
        "db.type/ref" => Some(ValueType::Ref),
        "db.type/bigint" => Some(ValueType::BigInt),
        "db.type/bigdec" => Some(ValueType::BigDec),
        _ => None,
    }
}

/// Parse a `db.cardinality/*` keyword into a `Cardinality`.
///
/// Returns `None` for unrecognized cardinality keywords.
#[must_use]
pub fn parse_cardinality(keyword: &str) -> Option<Cardinality> {
    match keyword {
        "db.cardinality/one" => Some(Cardinality::One),
        "db.cardinality/many" => Some(Cardinality::Many),
        _ => None,
    }
}
