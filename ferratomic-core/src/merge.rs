//! CRDT merge: set union of two datom stores.
//!
//! INV-FERR-001 (commutativity), INV-FERR-002 (associativity),
//! INV-FERR-003 (idempotency), INV-FERR-004 (monotonic growth).
//! INV-FERR-010: merge convergence — strong eventual consistency (SEC)
//! follows from commutativity + associativity + idempotency. Any two
//! replicas that have received the same set of updates will converge
//! to identical state, regardless of delivery order.
//!
//! Merge is pure set union. No schema validation (C4).
//! No datoms are added or removed beyond the union.

use crate::store::Store;

/// Merge two stores by set union (INV-FERR-001, INV-FERR-002, INV-FERR-003).
///
/// The result contains exactly the union of both datom sets.
/// Commutative (INV-FERR-001), associative (INV-FERR-002), and
/// idempotent (INV-FERR-003). Both input stores are preserved
/// (INV-FERR-004: monotonic growth).
///
/// INV-FERR-010: this function is the mechanism by which strong eventual
/// consistency is achieved. Because merge is commutative, associative,
/// and idempotent, any two replicas that have received the same updates
/// converge to identical state regardless of delivery order.
///
/// INV-FERR-009: schemas are unioned (all attributes from both stores).
/// INV-FERR-007: epoch is `max(a.epoch, b.epoch)`.
/// HI-014: genesis agent is `min(a.genesis_agent, b.genesis_agent)`.
///
/// INV-FERR-043: conflicting schema definitions (same attribute, different
/// type/cardinality) are resolved deterministically by keeping the
/// definition that sorts first. This preserves commutativity. A debug
/// assertion fires to flag the conflict for diagnosis.
///
/// Currently infallible; returns `Result` for forward compatibility
/// with stricter schema conflict policies.
///
/// # Errors
///
/// Currently always returns `Ok`. Future versions may return
/// `FerraError::SchemaIncompatible` under stricter conflict policies.
pub fn merge(a: &Store, b: &Store) -> Result<Store, ferratom::FerraError> {
    Ok(Store::from_merge(a, b))
}
