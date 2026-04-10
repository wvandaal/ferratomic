//! # ferratomic-store -- CRDT algebra core
//!
//! `Store = (P(D), union)` -- a G-Set CRDT semilattice where writes are
//! commutative, associative, and idempotent by construction. This crate
//! owns the mathematical heart of Ferratomic: the datom store, merge,
//! schema evolution, and query/snapshot operations.
//!
//! ## Algebraic properties
//!
//! - **INV-FERR-001**: merge is commutative (set union).
//! - **INV-FERR-002**: merge is associative (set union).
//! - **INV-FERR-003**: merge is idempotent (set union).
//! - **INV-FERR-004**: transact is strictly monotonic -- the store only grows.
//! - **INV-FERR-005**: secondary indexes are in bijection with the primary set.
//! - **INV-FERR-007**: epochs are strictly monotonically increasing.
//! - **INV-FERR-009**: schema evolution at transact boundary.
//! - **INV-FERR-031**: genesis produces a deterministic store.
//!
//! ## Module layout
//!
//! - [`store`] -- `Store`, `Snapshot`, `TxReceipt` (core types).
//! - `apply` -- transaction application, WAL replay, merge construction.
//! - [`merge`](mod@merge) -- CRDT merge (set union) and schema conflict resolution.
//! - `query` -- snapshot and LIVE-set query helpers.
//! - [`iter`] -- unified iteration over dual representation.
//! - `checkpoint` -- byte serialization convenience methods.
//! - `schema_evolution` -- genesis meta-schema and schema evolution.
//! - [`sketch`] -- `MinHash` sketch for O(delta) federation reconciliation.

#![forbid(unsafe_code)]
#![deny(clippy::all, missing_docs)]
#![warn(clippy::pedantic)]

mod apply;
mod checkpoint;
pub mod iter;
pub mod merge;
pub(crate) mod query;
pub(crate) mod repr;
pub(crate) mod schema_evolution;
pub mod sketch;
pub mod store;

#[cfg(test)]
mod tests;

// Re-exports for ergonomic access.
pub use self::{
    apply::TransactContext,
    checkpoint::{extract_checkpoint_data, store_from_checkpoint_data},
    iter::{DatomIter, DatomSetView, SnapshotDatoms},
    merge::{merge, selective_merge, MergeReceipt, SchemaConflict},
    sketch::{StoreSketch, DEFAULT_CAPACITY},
    store::{Snapshot, Store, TxReceipt},
};

// ---------------------------------------------------------------------------
// Trait implementations
// ---------------------------------------------------------------------------

/// INV-FERR-001..003: Store is a join-semilattice under set union.
/// The merge operation is commutative, associative, and idempotent.
impl ferratom::traits::Semilattice for Store {
    fn merge(&self, other: &Self) -> Result<Self, ferratom::FerraError> {
        Ok(Store::from_merge(self, other))
    }
}

// Note: ContentAddressed for Datom must be impl'd in ferratom crate
// (orphan rule). See ferratom/src/datom.rs -- Datom::content_hash()
// already provides the INV-FERR-012 contract.

/// Proof-friendly card-one LIVE selection hook.
///
/// Exposes the exact selection kernel used by `Store::live_resolve` without
/// requiring proof harnesses to construct a full store. This is a verification
/// boundary only; production callers use `Store::live_resolve`.
#[cfg(any(test, feature = "test-utils"))]
#[must_use]
pub fn select_latest_live_value_for_test(
    entries: &[(ferratom::Value, (ferratom::TxId, ferratom::Op))],
) -> Option<&ferratom::Value> {
    query::select_latest_live_value_for_test(entries)
}
