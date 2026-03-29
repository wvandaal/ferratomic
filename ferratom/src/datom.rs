//! Datom: the atomic fact — a 5-tuple `[entity, attribute, value, tx, op]`.
//!
//! INV-FERR-012: Content-addressed identity. Two datoms with identical
//! 5-tuples are the same datom. Enforced by Eq/Hash/Ord on all five fields.
//!
//! INV-FERR-018: Append-only. Datoms are immutable after creation.
//! No `&mut` methods. Clone is the only way to "modify" (which creates a new datom).

// TODO(Phase 3): Implement Datom, EntityId, Attribute, Value, Op
// Types must encode INV-FERR-012 (content identity) and INV-FERR-018 (immutability)
// See spec/23-ferratomic.md §23.1 INV-FERR-012 Level 2 for the Rust contract.
