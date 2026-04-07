//! # ferratomic-verify — Formal verification suite
//!
//! Lean 4 proofs, Stateright model checking, Kani bounded verification,
//! proptest properties, integration tests.
//!
//! ## Development Order
//!
//! Written before ferratomic-db (Phase 2 red-phase TDD).
//! Phase 4a: all tests now passing against the implementation.

// INV-FERR-023: No unsafe code permitted. Compiler-enforced.
#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod bench_helpers;
pub mod confidence;
pub mod fault_injection;
pub mod generators;
pub mod invariant_catalog;
pub mod isomorphism;

#[path = "../stateright/mod.rs"]
pub mod stateright_models;

// Kani harnesses are a [[test]] target (kani/mod.rs), not a library module.
// This ensures they compile under `cargo check --all-targets` and `cargo
// clippy --all-targets` without any allow-suppressions.
