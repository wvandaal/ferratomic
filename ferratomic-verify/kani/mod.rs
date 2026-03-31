//! Kani bounded model checking harnesses for Ferratomic.
//!
//! These modules mirror the Level 2 Rust contracts in
//! `spec/01-core-invariants.md` and `spec/02-concurrency.md`.
//! They are red-phase verification artifacts: the referenced runtime APIs are
//! expected to arrive in later phases, so these harnesses intentionally lead the
//! implementation surface.

#![cfg(kani)]

pub mod backpressure_bounds;
pub mod clock;
pub mod crdt_laws;
pub mod durability;
pub mod error_exhaustiveness;
pub mod live_resolution;
pub mod schema_identity;
pub mod sharding;
pub mod store_views;
