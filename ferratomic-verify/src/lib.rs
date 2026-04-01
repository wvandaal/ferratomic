//! # ferratomic-verify — Formal verification suite
//!
//! Lean 4 proofs, Stateright model checking, Kani bounded verification,
//! proptest properties, integration tests.
//!
//! ## Development Order
//!
//! Written before ferratomic-core (Phase 2 red-phase TDD).
//! Phase 4a: all tests now passing against the implementation.

// INV-FERR-023: No unsafe code permitted. Compiler-enforced.
#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod confidence;
pub mod fault_injection;
pub mod generators;
pub mod invariant_catalog;

#[path = "../stateright/mod.rs"]
pub mod stateright_models;

// Kani-only module: cfg(kani) is set by the Kani verifier, not rustc.
#[allow(unexpected_cfgs)]
#[cfg(kani)]
#[path = "../kani/mod.rs"]
pub mod kani;
