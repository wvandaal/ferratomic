//! Unified iteration and view types for `Store`'s dual representation.
//!
//! The `AdaptiveStore` pattern (bd-h2fz) uses two internal representations:
//! `PositionalStore` (contiguous arrays) for cold-start-loaded stores and
//! `OrdSet<Datom>` (persistent tree) for stores that have received writes.
//!
//! These types abstract over that distinction so callers get a uniform API
//! regardless of which representation is active.

use std::fmt;

use ferratom::Datom;
use ferratomic_positional::PositionalStore;
use im::OrdSet;

// ---------------------------------------------------------------------------
// DatomIter -- unified iterator over both representations
// ---------------------------------------------------------------------------

/// Iterator over datoms in a `Store`, dispatching to the active representation.
///
/// INV-FERR-004: yields every datom ever inserted, regardless of repr.
/// Variants are internal -- callers should use the `Iterator` impl, not match.
#[non_exhaustive]
pub enum DatomIter<'a> {
    /// Iterating over a `PositionalStore`'s contiguous canonical slice.
    Slice(std::slice::Iter<'a, Datom>),
    /// Iterating over an `im::OrdSet<Datom>`'s persistent tree.
    OrdSet(im::ordset::Iter<'a, Datom>),
}

impl<'a> Iterator for DatomIter<'a> {
    type Item = &'a Datom;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            DatomIter::Slice(it) => it.next(),
            DatomIter::OrdSet(it) => it.next(),
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        match self {
            DatomIter::Slice(it) => it.size_hint(),
            DatomIter::OrdSet(it) => it.size_hint(),
        }
    }
}

impl ExactSizeIterator for DatomIter<'_> {
    fn len(&self) -> usize {
        match self {
            DatomIter::Slice(it) => it.len(),
            DatomIter::OrdSet(it) => it.len(),
        }
    }
}

// ---------------------------------------------------------------------------
// DatomSetView -- unified "set reference" for callers of datom_set()
// ---------------------------------------------------------------------------

/// A read-only view of the datom set, dispatching to the active representation.
///
/// Provides `contains`, `len`, `is_empty`, `iter`, and equality comparison
/// so callers do not need to know which representation is active.
/// Variants are internal -- callers should use the method API, not match.
#[non_exhaustive]
pub enum DatomSetView<'a> {
    /// View into a `PositionalStore`'s canonical slice.
    Slice(&'a [Datom]),
    /// View into an `im::OrdSet<Datom>`.
    OrdSet(&'a OrdSet<Datom>),
}

impl<'a> DatomSetView<'a> {
    /// Whether the set contains the given datom.
    ///
    /// Slice variant: O(log n) binary search. `OrdSet` variant: O(log n) tree lookup.
    #[must_use]
    pub fn contains(&self, datom: &Datom) -> bool {
        match self {
            DatomSetView::Slice(slice) => slice.binary_search(datom).is_ok(),
            DatomSetView::OrdSet(set) => set.contains(datom),
        }
    }

    /// Number of datoms in the set.
    #[must_use]
    pub fn len(&self) -> usize {
        match self {
            DatomSetView::Slice(slice) => slice.len(),
            DatomSetView::OrdSet(set) => set.len(),
        }
    }

    /// Whether the set is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        match self {
            DatomSetView::Slice(slice) => slice.is_empty(),
            DatomSetView::OrdSet(set) => set.is_empty(),
        }
    }

    /// Iterate over all datoms in sorted order.
    ///
    /// Both representations are sorted (canonical EAVT for Slice, `Ord` for `OrdSet`),
    /// so the iteration order is identical.
    #[must_use]
    pub fn iter(&self) -> DatomIter<'a> {
        match self {
            DatomSetView::Slice(slice) => DatomIter::Slice(slice.iter()),
            DatomSetView::OrdSet(set) => DatomIter::OrdSet(set.iter()),
        }
    }
}

impl<'a> IntoIterator for &'a DatomSetView<'a> {
    type Item = &'a Datom;
    type IntoIter = DatomIter<'a>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl PartialEq for DatomSetView<'_> {
    /// Element-wise equality via sorted iteration.
    ///
    /// Both representations yield datoms in the same sorted order, so
    /// `iter().eq()` is correct regardless of which variants are compared.
    fn eq(&self, other: &Self) -> bool {
        self.len() == other.len() && self.iter().eq(other.iter())
    }
}

impl Eq for DatomSetView<'_> {}

impl fmt::Debug for DatomSetView<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DatomSetView::Slice(slice) => {
                f.debug_tuple("DatomSetView::Slice").field(slice).finish()
            }
            DatomSetView::OrdSet(set) => f.debug_tuple("DatomSetView::OrdSet").field(set).finish(),
        }
    }
}

// ---------------------------------------------------------------------------
// SnapshotDatoms -- owned datom set for Snapshot
// ---------------------------------------------------------------------------

/// Owned datom set stored inside a `Snapshot`, dispatching on representation.
///
/// INV-FERR-006: snapshots are frozen at creation time. `PositionalStore` is
/// wrapped in `Arc` for O(1) clone; `OrdSet` uses structural sharing.
/// Variants are internal -- callers should use the method API, not match.
#[non_exhaustive]
#[derive(Clone, Debug)]
pub enum SnapshotDatoms {
    /// Snapshot backed by a shared `PositionalStore`.
    Positional(std::sync::Arc<PositionalStore>),
    /// Snapshot backed by a cloned `OrdSet<Datom>`.
    OrdSet(OrdSet<Datom>),
}

impl SnapshotDatoms {
    /// Iterate over all datoms in the snapshot.
    #[must_use]
    pub fn iter(&self) -> DatomIter<'_> {
        match self {
            SnapshotDatoms::Positional(ps) => DatomIter::Slice(ps.datoms().iter()),
            SnapshotDatoms::OrdSet(set) => DatomIter::OrdSet(set.iter()),
        }
    }
}

impl<'a> IntoIterator for &'a SnapshotDatoms {
    type Item = &'a Datom;
    type IntoIter = DatomIter<'a>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}
