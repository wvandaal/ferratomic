//! Commit-time validation and sealed transaction accessors.
//!
//! INV-FERR-009: schema validation rejects unknown attributes and wrong types.
//! INV-FERR-018: committed transactions are immutable.

use std::marker::PhantomData;

use ferratom::{Datom, Schema};

use crate::{validate, Building, Committed, Transaction};

/// Schema validation error produced by [`Transaction::commit`].
///
/// INV-FERR-009: Every datom in a transaction must reference a known attribute
/// with a value of the declared type. Violations are reported as typed errors
/// so callers can pattern-match on category.
#[derive(Debug, Clone)]
pub enum TxValidationError {
    /// The attribute is not defined in the schema (INV-FERR-009).
    UnknownAttribute(String),

    /// The value type does not match the attribute's declared `ValueType` (INV-FERR-009).
    SchemaViolation {
        /// The attribute where the violation occurred.
        attribute: String,
        /// The expected `ValueType` per schema.
        expected: String,
        /// The actual `Value` variant that was supplied.
        got: String,
    },

    /// Cardinality violation (INV-FERR-032, reserved for Phase 4b).
    CardinalityViolation {
        /// The attribute where the violation occurred.
        attribute: String,
    },
}

impl std::fmt::Display for TxValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownAttribute(attr) => write!(f, "Unknown attribute: {attr}"),
            Self::SchemaViolation {
                attribute,
                expected,
                got,
            } => write!(
                f,
                "Schema violation on {attribute}: expected {expected}, got {got}"
            ),
            Self::CardinalityViolation { attribute } => {
                write!(f, "Cardinality violation on {attribute}")
            }
        }
    }
}

impl std::error::Error for TxValidationError {}

/// Convert `TxValidationError` into `FerraError` for `?` propagation.
///
/// INV-FERR-019: typed errors propagate through the `?` operator without
/// losing semantic category. Callers pattern-match on `FerraError` variant,
/// not on message strings.
impl From<TxValidationError> for ferratom::FerraError {
    fn from(error: TxValidationError) -> Self {
        match error {
            TxValidationError::UnknownAttribute(attribute) => Self::UnknownAttribute { attribute },
            TxValidationError::SchemaViolation {
                attribute,
                expected,
                got,
            } => Self::SchemaViolation {
                attribute,
                expected,
                got,
            },
            TxValidationError::CardinalityViolation { attribute } => Self::SchemaViolation {
                attribute,
                expected: "correct cardinality".to_string(),
                got: "cardinality violation".to_string(),
            },
        }
    }
}

impl Transaction<Building> {
    /// Validate all datoms against the schema and seal the transaction.
    ///
    /// INV-FERR-009: Every datom's attribute must exist in the schema and
    /// its value must match the attribute's declared `ValueType`. If any
    /// datom fails validation, the entire transaction is rejected (INV-FERR-006).
    ///
    /// Schema-definition attributes (those in the `db/` namespace that define
    /// new attributes) are validated against the meta-schema — they are themselves
    /// datoms and must pass type checks.
    ///
    /// # Errors
    ///
    /// - [`TxValidationError::UnknownAttribute`] if any datom references an
    ///   attribute not in the schema.
    /// - [`TxValidationError::SchemaViolation`] if any datom's value type
    ///   does not match the attribute's declared type.
    pub fn commit(self, schema: &Schema) -> Result<Transaction<Committed>, TxValidationError> {
        validate::validate_datoms(&self.datoms, schema)?;

        Ok(Transaction {
            agent: self.agent,
            datoms: self.datoms,
            _state: PhantomData,
        })
    }

    /// Seal the transaction without schema validation. **Testing only.**
    ///
    /// Bypasses INV-FERR-009 so proptest generators can create committed
    /// transactions with arbitrary datoms. Production code must use
    /// [`commit`](Self::commit).
    ///
    /// HI-005: Gated behind `test` or `test-utils` feature to prevent
    /// production code from bypassing schema validation.
    #[cfg(any(test, feature = "test-utils"))]
    #[must_use]
    pub fn commit_unchecked(self) -> Transaction<Committed> {
        Transaction {
            agent: self.agent,
            datoms: self.datoms,
            _state: PhantomData,
        }
    }
}

impl Transaction<Committed> {
    /// The datoms in this committed transaction.
    ///
    /// INV-FERR-018: Returns a shared slice — committed transactions are
    /// immutable. The real `TxId` will be stamped by the `Store` at transact
    /// time; until then, datoms carry a placeholder `TxId(0, 0, 0)`.
    #[must_use]
    pub fn datoms(&self) -> &[Datom] {
        &self.datoms
    }

    /// Consume the transaction and yield the owned datom vector.
    ///
    /// INV-FERR-020: Ownership transfer prevents double-application.
    /// The caller must read [`agent()`](Self::agent) BEFORE calling this
    /// method, since `self` is consumed.
    #[must_use]
    pub fn into_datoms(self) -> Vec<Datom> {
        self.datoms
    }

    /// The agent who created this transaction.
    ///
    /// INV-FERR-015: The agent identity is used by `HybridClock::tick` to
    /// produce a unique `TxId` when the `Store` applies this transaction.
    #[must_use]
    pub fn agent(&self) -> ferratom::AgentId {
        self.agent
    }
}
