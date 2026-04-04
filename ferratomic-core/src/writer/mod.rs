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
//! ```rust
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

mod commit;
mod validate;

use std::marker::PhantomData;

pub use commit::TxValidationError;
use ferratom::{AgentId, Attribute, Datom, EntityId, Op, TxId, Value};

#[cfg(test)]
mod tests;

// ---------------------------------------------------------------------------
// Typestate markers
// ---------------------------------------------------------------------------

/// Marker: transaction is being assembled (INV-FERR-018).
///
/// In this state, datoms can be added via [`Transaction::assert_datom`]
/// and [`Transaction::retract_datom`]. Transition to [`Committed`] via
/// [`Transaction::commit`] (with schema validation) or
/// [`Transaction::commit_unchecked`] (testing only).
///
/// `pub` because callers must name this type in generic bounds and
/// function signatures that accept `Transaction<Building>`.
#[derive(Debug)]
pub struct Building;

/// Marker: transaction has been validated and sealed (INV-FERR-018).
///
/// In this state, the transaction is read-only. The datom list is
/// accessible via [`Transaction::datoms`]. No further modifications
/// are possible -- invalid state transitions are compile errors.
///
/// `pub` because callers must name this type in function signatures
/// that accept `Transaction<Committed>`, including `Database::transact`
/// and WAL append.
#[derive(Debug)]
pub struct Committed;

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
}
