//! `GenericIndexes` — four secondary indexes over the datom set (INV-FERR-005).
//!
//! Each index uses a distinct sort ordering via its key type. The backend
//! is interchangeable via the [`IndexBackend`] trait (INV-FERR-025).
//! Runtime bijection enforcement via [`GenericIndexes::verify_bijection`].

use ferratom::Datom;
use im::OrdMap;

use crate::{AevtKey, AvetKey, EavtKey, IndexBackend, SortedVecBackend, VaetKey};

/// Secondary indexes over the datom set, each with a distinct sort order.
///
/// INV-FERR-005: every index is a bijection with the primary datom set.
/// After every mutation, all four maps have the same cardinality as the
/// primary set.
///
/// INV-FERR-025: the backend types are interchangeable. Each index uses
/// its own backend instance, parameterized by its key type. The default
/// is `im::OrdMap` (see [`Indexes`] type alias).
///
/// INV-FERR-027: correct per-index ordering enables O(log n + k) range
/// scans for different access patterns.
#[derive(Debug, Clone)]
pub struct GenericIndexes<BE, BA, BV, BAV>
where
    BE: IndexBackend<EavtKey, Datom>,
    BA: IndexBackend<AevtKey, Datom>,
    BV: IndexBackend<VaetKey, Datom>,
    BAV: IndexBackend<AvetKey, Datom>,
{
    /// Entity-Attribute-Value-Tx index.
    eavt: BE,
    /// Attribute-Entity-Value-Tx index.
    aevt: BA,
    /// Value-Attribute-Entity-Tx index (reverse references).
    vaet: BV,
    /// Attribute-Value-Entity-Tx index (unique/lookup).
    avet: BAV,
}

/// Default index type using `im::OrdMap` (ADR-FERR-001).
///
/// INV-FERR-025: type alias preserves backward compatibility — all
/// existing code that references `Indexes` continues to work without
/// changes.
pub type Indexes = GenericIndexes<
    OrdMap<EavtKey, Datom>,
    OrdMap<AevtKey, Datom>,
    OrdMap<VaetKey, Datom>,
    OrdMap<AvetKey, Datom>,
>;

/// Index type using [`SortedVecBackend`] for cache-optimal reads (INV-FERR-071).
///
/// INV-FERR-025: produces identical query results to [`Indexes`] (`OrdMap`
/// backend). Use for cold-start-loaded stores and read-heavy workloads.
/// Requires [`sort_all`](GenericIndexes::sort_all) after bulk insertion.
pub type SortedVecIndexes = GenericIndexes<
    SortedVecBackend<EavtKey, Datom>,
    SortedVecBackend<AevtKey, Datom>,
    SortedVecBackend<VaetKey, Datom>,
    SortedVecBackend<AvetKey, Datom>,
>;

impl
    GenericIndexes<
        SortedVecBackend<EavtKey, Datom>,
        SortedVecBackend<AevtKey, Datom>,
        SortedVecBackend<VaetKey, Datom>,
        SortedVecBackend<AvetKey, Datom>,
    >
{
    /// Sort all four index backends after bulk insertion (INV-FERR-071).
    ///
    /// Must be called after [`from_datoms`](GenericIndexes::from_datoms)
    /// to enable binary-search lookups. O(n log n) for n datoms.
    ///
    /// INV-FERR-005: after sorting, all four indexes are in their
    /// correct per-index order and binary search produces correct results.
    /// INV-FERR-025: behavioral equivalence with `im::OrdMap` is
    /// maintained after this call.
    pub fn sort_all(&mut self) {
        self.eavt.sort();
        self.aevt.sort();
        self.vaet.sort();
        self.avet.sort();
    }
}

impl<BE, BA, BV, BAV> GenericIndexes<BE, BA, BV, BAV>
where
    BE: IndexBackend<EavtKey, Datom>,
    BA: IndexBackend<AevtKey, Datom>,
    BV: IndexBackend<VaetKey, Datom>,
    BAV: IndexBackend<AvetKey, Datom>,
{
    /// Build indexes from a primary datom iterator.
    ///
    /// INV-FERR-005: all four indexes receive every datom from the primary
    /// set, ensuring bijection by construction.
    pub fn from_datoms<'a>(datoms: impl Iterator<Item = &'a Datom>) -> Self {
        let mut eavt = BE::default();
        let mut aevt = BA::default();
        let mut vaet = BV::default();
        let mut avet = BAV::default();

        for d in datoms {
            eavt.backend_insert(EavtKey::from_datom(d), d.clone());
            aevt.backend_insert(AevtKey::from_datom(d), d.clone());
            vaet.backend_insert(VaetKey::from_datom(d), d.clone());
            avet.backend_insert(AvetKey::from_datom(d), d.clone());
        }

        Self {
            eavt,
            aevt,
            vaet,
            avet,
        }
    }

    /// Insert a datom into all four indexes.
    ///
    /// INV-FERR-005: maintaining bijection requires every insert to
    /// touch all indexes.
    pub fn insert(&mut self, datom: &Datom) {
        self.eavt
            .backend_insert(EavtKey::from_datom(datom), datom.clone());
        self.aevt
            .backend_insert(AevtKey::from_datom(datom), datom.clone());
        self.vaet
            .backend_insert(VaetKey::from_datom(datom), datom.clone());
        self.avet
            .backend_insert(AvetKey::from_datom(datom), datom.clone());
    }

    /// Number of entries in the EAVT index (INV-FERR-005: same as all other indexes).
    #[must_use]
    pub fn len(&self) -> usize {
        self.eavt.backend_len()
    }

    /// Whether all indexes are empty (INV-FERR-005).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.eavt.backend_is_empty()
    }

    /// Access the EAVT index backend (INV-FERR-005, INV-FERR-027).
    #[must_use]
    pub fn eavt(&self) -> &BE {
        &self.eavt
    }

    /// Access the AEVT index backend (INV-FERR-005, INV-FERR-027).
    #[must_use]
    pub fn aevt(&self) -> &BA {
        &self.aevt
    }

    /// Access the VAET index backend (INV-FERR-005, INV-FERR-027).
    #[must_use]
    pub fn vaet(&self) -> &BV {
        &self.vaet
    }

    /// Access the AVET index backend (INV-FERR-005, INV-FERR-027).
    #[must_use]
    pub fn avet(&self) -> &BAV {
        &self.avet
    }

    /// Iterate EAVT datoms in index order (INV-FERR-027).
    pub fn eavt_datoms(&self) -> impl Iterator<Item = &Datom> {
        self.eavt.backend_values()
    }

    /// Iterate AEVT datoms in index order (INV-FERR-027).
    pub fn aevt_datoms(&self) -> impl Iterator<Item = &Datom> {
        self.aevt.backend_values()
    }

    /// Iterate VAET datoms in index order (INV-FERR-027).
    pub fn vaet_datoms(&self) -> impl Iterator<Item = &Datom> {
        self.vaet.backend_values()
    }

    /// Iterate AVET datoms in index order (INV-FERR-027).
    pub fn avet_datoms(&self) -> impl Iterator<Item = &Datom> {
        self.avet.backend_values()
    }

    /// Verify that all four indexes contain the same datom set (INV-FERR-005 bijection).
    ///
    /// INV-FERR-005: bijection implies both equal cardinality AND identical
    /// datom identity across all four indexes. Returns `true` if all four
    /// indexes agree on the count and the exact set of datom references.
    #[must_use]
    pub fn verify_bijection(&self) -> bool {
        let n = self.eavt.backend_len();
        if self.aevt.backend_len() != n
            || self.vaet.backend_len() != n
            || self.avet.backend_len() != n
        {
            return false;
        }
        // ME-003: Verify datom identity — not just cardinality. A bug
        // that inserts different datoms into different indexes would pass
        // the count-only check. O(n) but only called after transact, not
        // on the read hot path. Always-on (no cfg gate) per project rule:
        // "No #[cfg(...)] hiding code from the type checker."
        let eavt_datoms: std::collections::BTreeSet<_> = self.eavt.backend_values().collect();
        let aevt_datoms: std::collections::BTreeSet<_> = self.aevt.backend_values().collect();
        let vaet_datoms: std::collections::BTreeSet<_> = self.vaet.backend_values().collect();
        let avet_datoms: std::collections::BTreeSet<_> = self.avet.backend_values().collect();
        eavt_datoms == aevt_datoms && eavt_datoms == vaet_datoms && eavt_datoms == avet_datoms
    }
}
