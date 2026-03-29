//! # ferratom — Core types for the Ferratomic datom database
//!
//! This is a **leaf crate** with minimal dependencies. All types are pure
//! algebraic definitions. No I/O, no concurrency, no side effects.
//!
//! ## Types as Propositions (Curry-Howard)
//!
//! Every type in this crate encodes an invariant:
//! - `Datom`: a 5-tuple atomic fact (INV-FERR-012: content-addressed identity)
//! - `EntityId`: BLAKE3 hash proving content-addressed identity (INV-FERR-012)
//! - `Attribute`: interned string proving O(1) comparison (INV-FERR-026)
//! - `Value`: sum type with exact cardinality for each variant (INV-FERR-018)
//! - `TxId`: HLC timestamp proving causal ordering (INV-FERR-015, INV-FERR-016)
//! - `Schema`: attribute definitions proving validation at transact (INV-FERR-009)
//!
//! ## Algebraic Role
//!
//! Leaf crate in the dependency DAG. FREE objects — no imposed structure.
//! `ferratom` depends on nothing project-internal. All other crates depend on it.

#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![deny(clippy::all)]
#![warn(clippy::pedantic)]

pub mod datom;
pub mod schema;
pub mod clock;
pub mod error;
pub mod traits;

pub use datom::{Datom, EntityId, Attribute, Value, Op};
pub use schema::{Schema, AttributeDef, ValueType, Cardinality};
pub use clock::{TxId, AgentId, HybridClock, Frontier};
pub use error::FerraError;
