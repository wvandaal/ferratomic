//! Schema creation and evolution helpers.
//!
//! INV-FERR-009: Schema validation at transact boundary.
//! INV-FERR-031: Genesis determinism -- 25 axiomatic meta-schema attributes
//! (9 db/*, 5 lattice/*, 11 tx/*).
//!
//! This module contains:
//! - The genesis meta-schema (25 axiomatic attributes: 9 db/*, 5 lattice/*, 11 tx/*)
//! - Schema evolution logic (transact-time attribute installation)
//! - Value type and cardinality parsing from datom keywords
//!
//! # Examples
//!
//! Schema evolution happens automatically at transact time when datoms
//! define new attributes via the `db/ident`, `db/valueType`, and
//! `db/cardinality` meta-attributes.
//!
//! ```rust,no_run
//! use std::sync::Arc;
//! use ferratom::{NodeId, Attribute, EntityId, Value};
//! use ferratomic_db::db::Database;
//! use ferratomic_db::writer::Transaction;
//!
//! let db = Database::genesis();
//! let node = NodeId::from_bytes([1u8; 16]);
//!
//! // The genesis schema contains 25 axiomatic attributes (INV-FERR-031).
//! // "db/doc" is one of them -- it accepts String values.
//! let schema = db.schema();
//! assert!(schema.get(&Attribute::from("db/doc")).is_some());
//!
//! // Define a new attribute by transacting meta-schema datoms.
//! // All three meta-attributes must share the same entity.
//! let attr_entity = EntityId::from_content(b"user/email-def");
//! let tx = Transaction::new(node)
//!     .assert_datom(
//!         attr_entity,
//!         Attribute::from("db/ident"),
//!         Value::Keyword(Arc::from("user/email")),
//!     )
//!     .assert_datom(
//!         attr_entity,
//!         Attribute::from("db/valueType"),
//!         Value::Keyword(Arc::from("db.type/string")),
//!     )
//!     .assert_datom(
//!         attr_entity,
//!         Attribute::from("db/cardinality"),
//!         Value::Keyword(Arc::from("db.cardinality/one")),
//!     )
//!     .commit(&schema)
//!     .unwrap();
//!
//! db.transact(tx).unwrap();
//!
//! // After transact, the new attribute is installed in the schema.
//! let updated_schema = db.schema();
//! assert!(updated_schema.get(&Attribute::from("user/email")).is_some());
//! ```

use std::{collections::HashMap, sync::Arc};

use ferratom::{
    Attribute, AttributeDef, Cardinality, Datom, EntityId, FerraError, ResolutionMode, Schema,
    ValueType,
};

/// Build the deterministic genesis meta-schema with 25 axiomatic attributes.
///
/// Helper: LWW keyword attribute definition for genesis schema.
fn lww_kw(doc: &str) -> AttributeDef {
    AttributeDef::new(
        ValueType::Keyword,
        Cardinality::One,
        ResolutionMode::Lww,
        Some(Arc::from(doc)),
    )
}

/// Helper: LWW string attribute definition for genesis schema.
fn lww_str(doc: &str) -> AttributeDef {
    AttributeDef::new(
        ValueType::String,
        Cardinality::One,
        ResolutionMode::Lww,
        Some(Arc::from(doc)),
    )
}

/// Helper: LWW boolean attribute definition for genesis schema.
fn lww_bool(doc: &str) -> AttributeDef {
    AttributeDef::new(
        ValueType::Boolean,
        Cardinality::One,
        ResolutionMode::Lww,
        Some(Arc::from(doc)),
    )
}

/// Helper: LWW ref attribute definition for genesis schema.
fn lww_ref(doc: &str) -> AttributeDef {
    AttributeDef::new(
        ValueType::Ref,
        Cardinality::One,
        ResolutionMode::Lww,
        Some(Arc::from(doc)),
    )
}

/// Helper: LWW instant attribute definition for genesis schema.
fn lww_instant(doc: &str) -> AttributeDef {
    AttributeDef::new(
        ValueType::Instant,
        Cardinality::One,
        ResolutionMode::Lww,
        Some(Arc::from(doc)),
    )
}

/// Helper: LWW bytes attribute definition for genesis schema.
fn lww_bytes(doc: &str) -> AttributeDef {
    AttributeDef::new(
        ValueType::Bytes,
        Cardinality::One,
        ResolutionMode::Lww,
        Some(Arc::from(doc)),
    )
}

/// Helper: `MultiValue` card-many ref attribute definition for genesis schema.
fn mv_ref_many(doc: &str) -> AttributeDef {
    AttributeDef::new(
        ValueType::Ref,
        Cardinality::Many,
        ResolutionMode::MultiValue,
        Some(Arc::from(doc)),
    )
}

/// Build the deterministic genesis meta-schema with 25 axiomatic attributes.
///
/// INV-FERR-031: every call produces an identical schema. These 25
/// attributes are the ONLY hardcoded elements in the engine. Every
/// other attribute is defined by transacting datoms that reference
/// these 25. This is the schema-as-data bootstrap (C3, C7).
#[must_use]
pub(crate) fn genesis_schema() -> Schema {
    let mut schema = Schema::empty();
    define_meta_schema(&mut schema);
    define_tx_schema(&mut schema);
    schema
}

/// Attributes 1-14: db/* (meta-schema) and lattice/* definitions.
fn define_meta_schema(schema: &mut Schema) {
    // 1-9: db/* attributes (meta-schema)
    schema.define(
        Attribute::from("db/ident"),
        lww_kw("Attribute identity keyword"),
    );
    schema.define(
        Attribute::from("db/valueType"),
        lww_kw("Declared value type"),
    );
    schema.define(
        Attribute::from("db/cardinality"),
        lww_kw("Cardinality: one or many"),
    );
    schema.define(Attribute::from("db/doc"), lww_str("Documentation string"));
    schema.define(
        Attribute::from("db/unique"),
        lww_kw("Uniqueness constraint"),
    );
    schema.define(
        Attribute::from("db/isComponent"),
        lww_bool("Component ownership"),
    );
    schema.define(
        Attribute::from("db/resolutionMode"),
        lww_kw("CRDT conflict resolution mode"),
    );
    schema.define(
        Attribute::from("db/latticeOrder"),
        lww_ref("Reference to lattice definition"),
    );
    schema.define(Attribute::from("db/lwwClock"), lww_kw("LWW clock source"));

    // 10-14: lattice/* attributes (lattice definitions)
    schema.define(Attribute::from("lattice/ident"), lww_kw("Lattice name"));
    schema.define(
        Attribute::from("lattice/elements"),
        lww_str("Ordered element list"),
    );
    schema.define(
        Attribute::from("lattice/comparator"),
        lww_str("Comparison function"),
    );
    schema.define(Attribute::from("lattice/bottom"), lww_kw("Least element"));
    schema.define(Attribute::from("lattice/top"), lww_kw("Greatest element"));
}

/// Attributes 15-25: tx/* transaction metadata.
///
/// Phase 4a.5 federation metadata attributes added per INV-FERR-051,
/// INV-FERR-061, INV-FERR-063, and design decision D20.
fn define_tx_schema(schema: &mut Schema) {
    // --- derivation attributes (D20: derived datom provenance, Phase 4d) ---
    schema.define(
        Attribute::from("tx/derivation-input"),
        mv_ref_many("Input datoms for derivation (D20, Phase 4d)"),
    );
    schema.define(
        Attribute::from("tx/derivation-rule"),
        lww_kw("Rule that produced derivation (D20, Phase 4d)"),
    );
    schema.define(
        Attribute::from("tx/derivation-source"),
        lww_kw("Source of derived datoms (D20)"),
    );

    // --- existing + federation attributes ---
    schema.define(
        Attribute::from("tx/origin"),
        lww_ref("Node that originated transaction"),
    );
    schema.define(
        Attribute::from("tx/predecessor"),
        mv_ref_many("Causal predecessor entity refs (INV-FERR-061)"),
    );
    schema.define(
        Attribute::from("tx/provenance"),
        // ADR-FERR-028: changed from String to Keyword (INV-FERR-063).
        lww_kw("Epistemic confidence level (INV-FERR-063)"),
    );
    schema.define(
        Attribute::from("tx/rationale"),
        lww_str("Why this transaction exists"),
    );
    schema.define(
        Attribute::from("tx/signature"),
        lww_bytes("Ed25519 signature bytes (INV-FERR-051)"),
    );
    schema.define(
        Attribute::from("tx/signer"),
        lww_bytes("Ed25519 verifying key bytes (INV-FERR-051)"),
    );
    schema.define(
        Attribute::from("tx/time"),
        lww_instant("Transaction wall-clock time"),
    );
    schema.define(
        Attribute::from("tx/validation-override"),
        lww_str("Manual validation exemption"),
    );
}

/// Scan datoms for schema-defining patterns and install new attributes (INV-FERR-009).
///
/// When a transaction contains datoms with `db/ident`, `db/valueType`, and
/// `db/cardinality` all sharing the same entity, a new attribute is installed
/// into the schema. This is the schema-as-data bootstrap mechanism (C3, C7).
///
/// # Errors
///
/// Returns `FerraError::SchemaViolation` if a single entity carries
/// conflicting values for `db/ident`, `db/valueType`, or `db/cardinality`
/// within the same transaction (bd-ty5 / CR-037).
pub(crate) fn evolve_schema(schema: &mut Schema, datoms: &[Datom]) -> Result<(), FerraError> {
    let mut by_entity: HashMap<EntityId, Vec<&Datom>> = HashMap::new();
    for datom in datoms {
        by_entity.entry(datom.entity()).or_default().push(datom);
    }

    let db_ident = Attribute::from("db/ident");
    let db_value_type = Attribute::from("db/valueType");
    let db_cardinality = Attribute::from("db/cardinality");

    for entity_datoms in by_entity.values() {
        if let Some((attr, def)) =
            extract_attribute_def(entity_datoms, &db_ident, &db_value_type, &db_cardinality)?
        {
            schema.define(attr, def);
        }
    }
    Ok(())
}

/// Extract a keyword string from a [`Value`], or `None` for non-keyword variants.
///
/// INV-FERR-009: exhaustive match ensures a new `Value` variant triggers a
/// compile error here, forcing explicit handling rather than silent skip.
fn as_keyword(value: &ferratom::Value) -> Option<&str> {
    match value {
        ferratom::Value::Keyword(kw) => Some(kw.as_ref()),
        ferratom::Value::String(_)
        | ferratom::Value::Long(_)
        | ferratom::Value::Double(_)
        | ferratom::Value::Bool(_)
        | ferratom::Value::Instant(_)
        | ferratom::Value::Uuid(_)
        | ferratom::Value::Bytes(_)
        | ferratom::Value::Ref(_)
        | ferratom::Value::BigInt(_)
        | ferratom::Value::BigDec(_) => None,
    }
}

/// Extract a schema attribute definition from a group of entity datoms.
///
/// Returns `Some((attribute, definition))` when the entity carries all three
/// required meta-attributes (`db/ident`, `db/valueType`, `db/cardinality`).
/// Returns `Err` if the same entity carries conflicting values for any
/// meta-attribute (bd-ty5 / CR-037).
fn extract_attribute_def(
    datoms: &[&Datom],
    db_ident: &Attribute,
    db_value_type: &Attribute,
    db_cardinality: &Attribute,
) -> Result<Option<(Attribute, AttributeDef)>, FerraError> {
    let (mut ident, mut vtype, mut card): (Option<&str>, Option<ValueType>, Option<Cardinality>) =
        (None, None, None);

    for d in datoms {
        let Some(kw) = as_keyword(d.value()) else {
            continue;
        };
        let attr = d.attribute();
        if attr == db_ident {
            if let Some(prev) = ident {
                if prev != kw {
                    return Err(schema_violation("db/ident", prev, kw));
                }
            }
            ident = Some(kw);
        } else if attr == db_value_type {
            let vt = parse_value_type(kw).ok_or_else(|| {
                schema_violation("db/valueType", "recognized db.type/* keyword", kw)
            })?;
            if let Some(ref prev) = vtype {
                if *prev != vt {
                    return Err(schema_violation(
                        "db/valueType",
                        &format!("{prev:?}"),
                        &format!("{vt:?}"),
                    ));
                }
            }
            vtype = Some(vt);
        } else if attr == db_cardinality {
            let c = parse_cardinality(kw).ok_or_else(|| {
                schema_violation("db/cardinality", "recognized db.cardinality/* keyword", kw)
            })?;
            if let Some(ref prev) = card {
                if *prev != c {
                    return Err(schema_violation(
                        "db/cardinality",
                        &format!("{prev:?}"),
                        &format!("{c:?}"),
                    ));
                }
            }
            card = Some(c);
        }
    }

    match (ident, vtype, card) {
        (Some(name), Some(vt), Some(c)) => Ok(Some((
            Attribute::from(name),
            AttributeDef::new(vt, c, ResolutionMode::Lww, None),
        ))),
        _ => Ok(None),
    }
}

/// Build a `SchemaViolation` error for conflicting meta-attribute values.
fn schema_violation(field: &str, expected: &str, got: &str) -> FerraError {
    FerraError::SchemaViolation {
        attribute: field.to_string(),
        expected: expected.to_string(),
        got: got.to_string(),
    }
}

/// Parse a `db.type/*` keyword into a `ValueType`.
///
/// INV-FERR-009: used during schema evolution to interpret the
/// `db/valueType` keyword into the typed `ValueType` enum that
/// governs transact-time validation.
///
/// Returns `None` for unrecognized type keywords; callers decide whether to
/// treat that as absence or a schema violation.
#[must_use]
pub(crate) fn parse_value_type(keyword: &str) -> Option<ValueType> {
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
/// INV-FERR-009: used during schema evolution to interpret the
/// `db/cardinality` keyword. Cardinality governs whether an
/// entity-attribute pair holds one value or many.
///
/// Returns `None` for unrecognized cardinality keywords; callers decide whether
/// to treat that as absence or a schema violation.
#[must_use]
pub(crate) fn parse_cardinality(keyword: &str) -> Option<Cardinality> {
    match keyword {
        "db.cardinality/one" => Some(Cardinality::One),
        "db.cardinality/many" => Some(Cardinality::Many),
        _ => None,
    }
}
