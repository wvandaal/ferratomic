//! Transaction typestate builder for Ferratomic.
//!
//! INV-FERR-009: Schema validation at the transact boundary.
//! INV-FERR-006: Transaction atomicity — all datoms commit or none do.
//! INV-FERR-018: Committed transactions are immutable (enforced by typestate).
//!
//! # Typestate Pattern
//!
//! `Transaction<Building>` accepts new datoms via `assert_datom`. Calling
//! `commit` (with schema validation) or `commit_unchecked` (testing only)
//! produces `Transaction<Committed>`, which is read-only.
//!
//! Invalid state transitions are compile errors:
//! - Cannot call `assert_datom` on `Transaction<Committed>`.
//! - Cannot call `datoms()` on `Transaction<Building>`.
//!
//! # Example
//!
//! ```rust,no_run
//! use ferratom::{AgentId, Attribute, EntityId, Value};
//! use ferratomic_core::writer::Transaction;
//!
//! let agent = AgentId::from_bytes([0u8; 16]);
//! let tx = Transaction::new(agent)
//!     .assert_datom(
//!         EntityId::from_content(b"e1"),
//!         Attribute::from("db/doc"),
//!         Value::String("hello".into()),
//!     );
//! // tx is Transaction<Building>; call .commit(schema) to validate and seal.
//! ```

mod validate;

use std::marker::PhantomData;

use ferratom::{AgentId, Attribute, Datom, EntityId, Op, Schema, TxId, Value};

// ---------------------------------------------------------------------------
// Typestate markers
// ---------------------------------------------------------------------------

/// Marker: transaction is being assembled (INV-FERR-018).
///
/// In this state, datoms can be added via [`Transaction::assert_datom`]
/// and [`Transaction::retract_datom`]. Transition to [`Committed`] via
/// [`Transaction::commit`] (with schema validation) or
/// [`Transaction::commit_unchecked`] (testing only).
#[derive(Debug)]
pub struct Building;

/// Marker: transaction has been validated and sealed (INV-FERR-018).
///
/// In this state, the transaction is read-only. The datom list is
/// accessible via [`Transaction::datoms`]. No further modifications
/// are possible -- invalid state transitions are compile errors.
#[derive(Debug)]
pub struct Committed;

// ---------------------------------------------------------------------------
// TxValidationError
// ---------------------------------------------------------------------------

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

/// Convert `TxValidationError` into `FerraError` for `?` propagation (INV-FERR-019).
impl From<TxValidationError> for ferratom::FerraError {
    fn from(e: TxValidationError) -> Self {
        match e {
            TxValidationError::UnknownAttribute(attr) => Self::UnknownAttribute { attribute: attr },
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

// ---------------------------------------------------------------------------
// Transaction<S>
// ---------------------------------------------------------------------------

/// A transaction assembling or holding a batch of datoms.
///
/// INV-FERR-006: Transactions are atomic — all datoms succeed or none do.
/// INV-FERR-018: After `commit`, the transaction is sealed and immutable.
///
/// The typestate parameter `S` is either [`Building`] (mutable) or
/// [`Committed`] (sealed). The phantom data ensures zero runtime cost.
#[derive(Debug)]
pub struct Transaction<S> {
    /// The agent that authored this transaction.
    agent: AgentId,
    /// The datoms accumulated in this transaction.
    datoms: Vec<Datom>,
    /// Zero-size typestate marker.
    _state: PhantomData<S>,
}

// ---------------------------------------------------------------------------
// Transaction<Building>
// ---------------------------------------------------------------------------

impl Transaction<Building> {
    /// Create a new transaction for the given agent.
    ///
    /// The transaction starts empty. Add datoms via [`assert_datom`](Self::assert_datom),
    /// then seal with [`commit`](Self::commit) or [`commit_unchecked`](Self::commit_unchecked).
    #[must_use]
    pub fn new(agent: AgentId) -> Self {
        Self {
            agent,
            datoms: Vec::new(),
            _state: PhantomData,
        }
    }

    /// Add an assert datom to this transaction.
    ///
    /// INV-FERR-018: Creates a `Datom` with `Op::Assert` and a placeholder
    /// `TxId(0, 0, 0)`. The real `TxId` is assigned by the `Store` at
    /// transact time (INV-FERR-015).
    ///
    /// Consumes and returns `self` for builder-style chaining.
    #[must_use]
    pub fn assert_datom(mut self, entity: EntityId, attribute: Attribute, value: Value) -> Self {
        let placeholder_tx = TxId::with_agent(0, 0, AgentId::from_bytes([0u8; 16]));
        let datom = Datom::new(entity, attribute, value, placeholder_tx, Op::Assert);
        self.datoms.push(datom);
        self
    }

    /// Add a retract datom to this transaction.
    ///
    /// INV-FERR-018: Retractions are new datoms with `Op::Retract`. The
    /// store is append-only — a retraction does not delete, it records that
    /// a prior assertion no longer holds as of this transaction.
    ///
    /// INV-FERR-029: The LIVE view uses assert/retract pairs to compute
    /// the current state of each entity-attribute pair.
    ///
    /// Consumes and returns `self` for builder-style chaining.
    #[must_use]
    pub fn retract_datom(mut self, entity: EntityId, attribute: Attribute, value: Value) -> Self {
        let placeholder_tx = TxId::with_agent(0, 0, AgentId::from_bytes([0u8; 16]));
        let datom = Datom::new(entity, attribute, value, placeholder_tx, Op::Retract);
        self.datoms.push(datom);
        self
    }

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

// ---------------------------------------------------------------------------
// Transaction<Committed>
// ---------------------------------------------------------------------------

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

    /// The agent who created this transaction.
    ///
    /// INV-FERR-015: The agent identity is used by `HybridClock::tick` to
    /// produce a unique `TxId` when the `Store` applies this transaction.
    #[must_use]
    pub fn agent(&self) -> AgentId {
        self.agent
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use validate::value_matches_type;

    use super::*;

    /// Helper: build a minimal schema with one String attribute.
    fn test_schema() -> Schema {
        use std::collections::HashMap;

        use ferratom::{AttributeDef, Cardinality, ResolutionMode};

        let mut attrs = HashMap::new();
        attrs.insert(
            Attribute::from("user/name"),
            AttributeDef::new(
                ferratom::ValueType::String,
                Cardinality::One,
                ResolutionMode::Lww,
                None,
            ),
        );
        attrs.insert(
            Attribute::from("user/age"),
            AttributeDef::new(
                ferratom::ValueType::Long,
                Cardinality::One,
                ResolutionMode::Lww,
                None,
            ),
        );
        Schema::from_attrs(attrs)
    }

    #[test]
    fn test_transaction_new_is_empty() {
        let agent = AgentId::from_bytes([0u8; 16]);
        let tx = Transaction::new(agent);
        // Building state: no public datoms() accessor, but commit_unchecked reveals it.
        let committed = tx.commit_unchecked();
        assert!(
            committed.datoms().is_empty(),
            "new transaction should have no datoms"
        );
    }

    #[test]
    fn test_transaction_assert_datom_accumulates() {
        let agent = AgentId::from_bytes([1u8; 16]);
        let entity = EntityId::from_content(b"test");
        let tx = Transaction::new(agent)
            .assert_datom(
                entity,
                Attribute::from("user/name"),
                Value::String("Alice".into()),
            )
            .assert_datom(entity, Attribute::from("user/age"), Value::Long(30));

        let committed = tx.commit_unchecked();
        assert_eq!(committed.datoms().len(), 2, "should have 2 datoms");
    }

    #[test]
    fn test_transaction_agent_preserved() {
        let agent = AgentId::from_bytes([42u8; 16]);
        let tx = Transaction::new(agent).commit_unchecked();
        assert_eq!(
            tx.agent(),
            agent,
            "committed transaction should preserve agent"
        );
    }

    #[test]
    fn test_inv_ferr_009_commit_valid() {
        let schema = test_schema();
        let agent = AgentId::from_bytes([0u8; 16]);
        let entity = EntityId::from_content(b"e1");

        let tx = Transaction::new(agent).assert_datom(
            entity,
            Attribute::from("user/name"),
            Value::String("Bob".into()),
        );

        let result = tx.commit(&schema);
        assert!(
            result.is_ok(),
            "INV-FERR-009: valid datom should be accepted"
        );
    }

    #[test]
    fn test_inv_ferr_009_commit_unknown_attribute() {
        let schema = test_schema();
        let agent = AgentId::from_bytes([0u8; 16]);
        let entity = EntityId::from_content(b"e1");

        let tx = Transaction::new(agent).assert_datom(
            entity,
            Attribute::from("nonexistent/attr"),
            Value::String("test".into()),
        );

        let result = tx.commit(&schema);
        assert!(
            matches!(result, Err(TxValidationError::UnknownAttribute(_))),
            "INV-FERR-009: unknown attribute should be rejected, got {result:?}"
        );
    }

    #[test]
    fn test_inv_ferr_009_commit_wrong_type() {
        let schema = test_schema();
        let agent = AgentId::from_bytes([0u8; 16]);
        let entity = EntityId::from_content(b"e1");

        // user/name expects String, but we pass Long
        let tx = Transaction::new(agent).assert_datom(
            entity,
            Attribute::from("user/name"),
            Value::Long(42),
        );

        let result = tx.commit(&schema);
        assert!(
            matches!(result, Err(TxValidationError::SchemaViolation { .. })),
            "INV-FERR-009: wrong value type should be rejected, got {result:?}"
        );
    }

    #[test]
    fn test_inv_ferr_006_atomic_rejection() {
        let schema = test_schema();
        let agent = AgentId::from_bytes([0u8; 16]);

        // First datom is valid, second has unknown attribute.
        // The entire transaction must be rejected.
        let tx = Transaction::new(agent)
            .assert_datom(
                EntityId::from_content(b"e1"),
                Attribute::from("user/name"),
                Value::String("valid".into()),
            )
            .assert_datom(
                EntityId::from_content(b"e2"),
                Attribute::from("bogus/attr"),
                Value::Long(0),
            );

        let result = tx.commit(&schema);
        assert!(
            result.is_err(),
            "INV-FERR-006: transaction with one invalid datom must be fully rejected"
        );
    }

    #[test]
    fn test_datoms_have_placeholder_tx_id() {
        let agent = AgentId::from_bytes([0u8; 16]);
        let entity = EntityId::from_content(b"e1");

        let committed = Transaction::new(agent)
            .assert_datom(
                entity,
                Attribute::from("user/name"),
                Value::String("x".into()),
            )
            .commit_unchecked();

        let datom = &committed.datoms()[0];
        let placeholder = TxId::new(0, 0, 0);
        assert_eq!(
            datom.tx(),
            placeholder,
            "datoms should carry placeholder TxId before store assigns real one"
        );
    }

    #[test]
    fn test_datoms_have_op_assert() {
        let agent = AgentId::from_bytes([0u8; 16]);
        let entity = EntityId::from_content(b"e1");

        let committed = Transaction::new(agent)
            .assert_datom(
                entity,
                Attribute::from("user/name"),
                Value::String("x".into()),
            )
            .commit_unchecked();

        assert_eq!(
            committed.datoms()[0].op(),
            Op::Assert,
            "assert_datom should produce Op::Assert"
        );
    }

    #[test]
    fn test_value_matches_type_all_variants() {
        // Every (Value, ValueType) pair must match its own variant.
        // Note: Value::Double wraps OrderedFloat<f64>; we use From<f64> via
        // ordered_float's Into impl which is re-exported through the enum.
        let pairs: Vec<(Value, ferratom::ValueType)> = vec![
            (Value::Keyword(Arc::from("k")), ferratom::ValueType::Keyword),
            (Value::String(Arc::from("s")), ferratom::ValueType::String),
            (Value::Long(0), ferratom::ValueType::Long),
            (
                Value::Double(ferratom::NonNanFloat::new(1.0).unwrap()),
                ferratom::ValueType::Double,
            ),
            (Value::Bool(true), ferratom::ValueType::Boolean),
            (Value::Instant(0), ferratom::ValueType::Instant),
            (Value::Uuid([0; 16]), ferratom::ValueType::Uuid),
            (
                Value::Bytes(Arc::from(vec![0u8])),
                ferratom::ValueType::Bytes,
            ),
            (
                Value::Ref(EntityId::from_bytes([0; 32])),
                ferratom::ValueType::Ref,
            ),
            (Value::BigInt(0), ferratom::ValueType::BigInt),
            (Value::BigDec(0), ferratom::ValueType::BigDec),
        ];

        for (value, vtype) in &pairs {
            assert!(
                value_matches_type(value, vtype),
                "value_matches_type should return true for matching pair: {value:?} vs {vtype:?}"
            );
        }

        // All 11 variants covered (bd-326 regression: Double was previously omitted).
    }

    #[test]
    fn test_value_matches_type_rejects_mismatches() {
        // String value should NOT match Long type.
        assert!(
            !value_matches_type(&Value::String(Arc::from("s")), &ferratom::ValueType::Long),
            "String should not match Long"
        );
        // Long value should NOT match String type.
        assert!(
            !value_matches_type(&Value::Long(42), &ferratom::ValueType::String),
            "Long should not match String"
        );
    }

    #[test]
    fn test_commit_unchecked_accepts_anything() {
        let agent = AgentId::from_bytes([0u8; 16]);

        // Datom with a completely arbitrary attribute — commit_unchecked must not fail.
        let committed = Transaction::new(agent)
            .assert_datom(
                EntityId::from_content(b"e"),
                Attribute::from("totally/made-up"),
                Value::Long(999),
            )
            .commit_unchecked();

        assert_eq!(committed.datoms().len(), 1);
    }

    #[test]
    fn test_builder_chaining() {
        let agent = AgentId::from_bytes([0u8; 16]);
        let e = EntityId::from_content(b"e");

        // Exercise the builder pattern from generators.rs
        let mut tx = Transaction::new(agent);
        for i in 0..5 {
            tx = tx.assert_datom(e, Attribute::from("user/name"), Value::Long(i));
        }
        let committed = tx.commit_unchecked();
        assert_eq!(committed.datoms().len(), 5);
    }

    // -- Regression tests for cleanroom review defects -------------------------

    /// Regression: bd-79n — `retract_datom` creates `Op::Retract` datoms.
    #[test]
    fn test_bug_bd_79n_retract_datom() {
        let agent = AgentId::from_bytes([0u8; 16]);
        let entity = EntityId::from_content(b"e1");

        let committed = Transaction::new(agent)
            .retract_datom(
                entity,
                Attribute::from("user/name"),
                Value::String("old_value".into()),
            )
            .commit_unchecked();

        assert_eq!(committed.datoms().len(), 1, "should have 1 datom");
        assert_eq!(
            committed.datoms()[0].op(),
            Op::Retract,
            "bd-79n: retract_datom must produce Op::Retract"
        );
    }

    /// Regression: bd-79n — mixed assert and retract transaction.
    #[test]
    fn test_bug_bd_79n_mixed_assert_retract() {
        let agent = AgentId::from_bytes([0u8; 16]);
        let entity = EntityId::from_content(b"e1");

        let committed = Transaction::new(agent)
            .assert_datom(
                entity,
                Attribute::from("user/name"),
                Value::String("new_value".into()),
            )
            .retract_datom(
                entity,
                Attribute::from("user/name"),
                Value::String("old_value".into()),
            )
            .commit_unchecked();

        assert_eq!(committed.datoms().len(), 2, "should have assert + retract");
        assert_eq!(committed.datoms()[0].op(), Op::Assert);
        assert_eq!(committed.datoms()[1].op(), Op::Retract);
    }
}
