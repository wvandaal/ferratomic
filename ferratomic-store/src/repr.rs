//! `StoreRepr` -- dual representation for the datom set (bd-h2fz).
//!
//! Cold-start-loaded stores begin as `Positional` (contiguous arrays,
//! cache-optimal, ~6x less memory). On first write, `Store::promote()`
//! converts to `OrdMap` (persistent tree, O(log n) insert).

use std::sync::Arc;

use ferratom::Datom;
use ferratomic_index::SortedVecIndexes;
use ferratomic_positional::PositionalStore;
use im::OrdSet;

// ---------------------------------------------------------------------------
// StoreRepr -- dual representation (bd-h2fz)
// ---------------------------------------------------------------------------

/// Internal representation of the datom set and indexes.
///
/// Cold-start-loaded stores begin as `Positional` (contiguous arrays,
/// cache-optimal, ~6x less memory). On first write, `Store::promote()`
/// converts to `OrdMap` (persistent tree, O(log n) insert).
///
/// INV-FERR-076: positional representation preserves all algebraic
/// properties. Promotion is semantics-preserving.
#[derive(Debug, Clone)]
pub(crate) enum StoreRepr {
    /// Cold-start representation: contiguous arrays with permutation indexes.
    /// Wrapped in `Arc` for O(1) clone (snapshot creation, merge input).
    Positional(Arc<PositionalStore>),
    /// Write-active representation: persistent balanced tree with `SortedVec` indexes.
    OrdMap {
        /// Primary datom set (ADR-FERR-001).
        datoms: OrdSet<Datom>,
        /// Secondary indexes maintained in bijection with primary set.
        indexes: SortedVecIndexes,
    },
}
