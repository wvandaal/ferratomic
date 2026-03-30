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
#![deny(missing_docs)]

pub mod generators;

#[path = "../stateright/mod.rs"]
pub mod stateright_models;

// Kani-only module: cfg(kani) is set by the Kani verifier, not rustc.
#[allow(unexpected_cfgs)]
#[cfg(kani)]
#[path = "../kani/mod.rs"]
pub mod kani;
