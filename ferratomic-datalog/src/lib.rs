//! # ferratomic-datalog — Datalog query engine
//!
//! Parser, planner, evaluator, CALM classification, incremental view maintenance.
//!
//! ## Algebraic Role
//!
//! Facade crate. HOMOMORPHISMS — structure-preserving maps from Store to QueryResult.

// INV-FERR-023: No unsafe code permitted. Compiler-enforced.
#![forbid(unsafe_code)]
#![deny(clippy::all, missing_docs)]

pub mod parser;
pub mod planner;
pub mod evaluator;
// pub mod incremental;
// pub mod calm;
