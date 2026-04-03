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

// INV-FERR-023: No unsafe code permitted. Compiler-enforced.
#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![deny(clippy::all)]
// ME-016 / NEG-FERR-001: No panics in production code.
// unwrap_used / expect_used / panic enforced via CI:
//   cargo clippy --workspace --lib -- -D clippy::unwrap_used -D clippy::expect_used -D clippy::panic
// NOT as crate-level attributes, because those also block test code.
#![warn(clippy::pedantic)]

pub mod clock;
pub mod datom;
pub mod error;
pub mod schema;
pub mod traits;
pub mod wire;

pub use clock::{AgentId, ClockSource, Frontier, HybridClock, SystemClock, TxId};
pub use datom::{Attribute, Datom, EntityId, NonNanFloat, Op, Value};
pub use error::FerraError;
pub use schema::{AttributeDef, Cardinality, ResolutionMode, Schema, ValueType};
