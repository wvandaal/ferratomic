//! Index key types, `IndexBackend` trait, and `GenericIndexes` struct.
//!
//! This crate defines the four canonical datom indexes and an abstract
//! backend trait, allowing the store layer to swap implementations
//! without changing query semantics.
//!
//! ## Dependency DAG
//!
//! Depends on `ferratom` (for `Datom`, `EntityId`, `Attribute`, `Value`)
//! and `im` (persistent collections for `SortedVecBackend`). Consumed by
//! `ferratomic-positional`, `ferratomic-checkpoint`, and `ferratomic-store`.
//!
//! ## Key Types
//!
//! - [`EavtKey`], [`AevtKey`], [`VaetKey`], [`AvetKey`] — index key
//!   newtypes. Each orders the four datom fields (entity, attribute,
//!   value, tx) in a different permutation for efficient range scans.
//! - [`IndexBackend`] — trait abstracting sorted-set operations (insert,
//!   range, contains). Implementations must be deterministic and total-order
//!   consistent with the key's `Ord`.
//! - [`SortedVecBackend`] — default backend backed by `im::OrdSet`.
//!   Persistent (structural sharing), O(log n) insert and lookup.
//! - [`GenericIndexes`] — bundles all four indexes with a single backend
//!   type parameter. [`Indexes`] and [`SortedVecIndexes`] are convenience
//!   aliases.
//!
//! ## Invariants
//!
//! - INV-FERR-005: four secondary indexes maintained in bijection with
//!   the primary datom set. Each uses a distinct key type whose `Ord`
//!   implementation arranges datom fields in index-specific order.
//! - INV-FERR-025: the index backend is interchangeable via the
//!   [`IndexBackend`] trait. All backends produce identical query results.

#![forbid(unsafe_code)]
#![deny(clippy::all, missing_docs)]
#![warn(clippy::pedantic)]

mod backend;
mod indexes;
mod keys;

pub use backend::{IndexBackend, SortedVecBackend};
pub use indexes::{GenericIndexes, Indexes, SortedVecIndexes};
pub use keys::{AevtKey, AvetKey, EavtKey, VaetKey};
