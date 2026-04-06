//! Schema validation for transaction commit.
//!
//! INV-FERR-009: Every datom in a transaction must reference a known attribute
//! with a value of the declared type.

use ferratom::{Datom, Schema, Value, ValueType};

use crate::TxValidationError;

// ---------------------------------------------------------------------------
// validate_datoms
// ---------------------------------------------------------------------------

/// Validate all datoms against the schema.
///
/// INV-FERR-009: Every datom's attribute must exist in the schema and its value
/// must match the attribute's declared `ValueType`. If any datom fails validation,
/// an error is returned immediately (INV-FERR-006: all-or-nothing atomicity).
///
/// Schema-definition attributes (those in the `db/` namespace that define new
/// attributes) are validated against the meta-schema -- they are themselves
/// datoms and must pass type checks.
pub(crate) fn validate_datoms(datoms: &[Datom], schema: &Schema) -> Result<(), TxValidationError> {
    for datom in datoms {
        let attr_name = datom.attribute();
        match schema.get(attr_name) {
            None => {
                return Err(TxValidationError::UnknownAttribute(
                    attr_name.as_str().to_owned(),
                ));
            }
            Some(attr_def) => {
                if !value_matches_type(datom.value(), attr_def.value_type()) {
                    return Err(TxValidationError::SchemaViolation {
                        attribute: attr_name.as_str().to_owned(),
                        expected: format!("{:?}", attr_def.value_type()),
                        got: format!("{:?}", datom.value()),
                    });
                }
            }
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// value_matches_type
// ---------------------------------------------------------------------------

/// Check whether a [`Value`] variant matches a declared [`ValueType`].
///
/// INV-FERR-009: This is the core type-checking predicate for schema
/// validation. Each `Value` variant maps to exactly one `ValueType`.
pub(crate) fn value_matches_type(value: &Value, expected: &ValueType) -> bool {
    matches!(
        (value, expected),
        (Value::Keyword(_), ValueType::Keyword)
            | (Value::String(_), ValueType::String)
            | (Value::Long(_), ValueType::Long)
            | (Value::Double(_), ValueType::Double)
            | (Value::Bool(_), ValueType::Boolean)
            | (Value::Instant(_), ValueType::Instant)
            | (Value::Uuid(_), ValueType::Uuid)
            | (Value::Bytes(_), ValueType::Bytes)
            | (Value::Ref(_), ValueType::Ref)
            | (Value::BigInt(_), ValueType::BigInt)
            | (Value::BigDec(_), ValueType::BigDec)
    )
}
