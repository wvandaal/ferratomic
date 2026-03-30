//! # ferratomic-verify — Formal verification suite
//!
//! Lean 4 proofs, Stateright model checking, Kani bounded verification,
//! proptest properties, integration tests.
//!
//! ## Development Order
//!
//! This crate is written BEFORE ferratomic-core (Phase 2, red phase TDD).
//! All tests must FAIL initially. Implementation makes them pass.

#![forbid(unsafe_code)]

pub mod generators;

#[path = "../stateright/mod.rs"]
pub mod stateright_models;

#[cfg(kani)]
#[path = "../kani/mod.rs"]
pub mod kani;
