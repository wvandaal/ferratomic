//! Permutation accessor methods for [`PositionalStore`] (INV-FERR-071, INV-FERR-076).
//!
//! Lazily-built Eytzinger-layout permutation arrays for AEVT, VAET, AVET,
//! and `TxId` sort orders. Each permutation maps an alternate sort order
//! index to the canonical EAVT position.

use ferratomic_index::{AevtKey, AvetKey, VaetKey};

use crate::{
    perm::{build_permutation, layout_permutation, layout_to_sorted},
    store::PositionalStore,
};

impl PositionalStore {
    /// Access the AEVT permutation array in Eytzinger (BFS) order (INV-FERR-071, INV-FERR-076).
    ///
    /// Lazily builds the permutation on first access. The returned slice
    /// has `n + 1` elements: index 0 is a sentinel (`u32::MAX`), root at index 1.
    /// Use `perm_aevt_sorted()` for the original sorted permutation.
    #[must_use]
    pub fn perm_aevt(&self) -> &[u32] {
        self.perm_aevt.get_or_init(|| {
            let sorted = build_permutation(&self.canonical, AevtKey::from_datom);
            layout_permutation(&sorted)
        })
    }

    /// Access the VAET permutation array in Eytzinger (BFS) order (INV-FERR-071, INV-FERR-076).
    ///
    /// Lazily builds the permutation on first access. The returned slice
    /// has `n + 1` elements: index 0 is a sentinel (`u32::MAX`), root at index 1.
    /// Use `perm_vaet_sorted()` for the original sorted permutation.
    #[must_use]
    pub fn perm_vaet(&self) -> &[u32] {
        self.perm_vaet.get_or_init(|| {
            let sorted = build_permutation(&self.canonical, VaetKey::from_datom);
            layout_permutation(&sorted)
        })
    }

    /// Access the AVET permutation array in Eytzinger (BFS) order (INV-FERR-071, INV-FERR-076).
    ///
    /// Lazily builds the permutation on first access. The returned slice
    /// has `n + 1` elements: index 0 is a sentinel (`u32::MAX`), root at index 1.
    /// Use `perm_avet_sorted()` for the original sorted permutation.
    #[must_use]
    pub fn perm_avet(&self) -> &[u32] {
        self.perm_avet.get_or_init(|| {
            let sorted = build_permutation(&self.canonical, AvetKey::from_datom);
            layout_permutation(&sorted)
        })
    }

    /// Recover the sorted AEVT permutation from Eytzinger layout (INV-FERR-071).
    ///
    /// O(n) in-order traversal. Used for checkpoint serialization where
    /// the original sorted permutation order is required.
    #[must_use]
    pub fn perm_aevt_sorted(&self) -> Vec<u32> {
        layout_to_sorted(self.perm_aevt())
    }

    /// Recover the sorted VAET permutation from Eytzinger layout (INV-FERR-071).
    ///
    /// O(n) in-order traversal. Used for checkpoint serialization where
    /// the original sorted permutation order is required.
    #[must_use]
    pub fn perm_vaet_sorted(&self) -> Vec<u32> {
        layout_to_sorted(self.perm_vaet())
    }

    /// Recover the sorted AVET permutation from Eytzinger layout (INV-FERR-071).
    ///
    /// O(n) in-order traversal. Used for checkpoint serialization where
    /// the original sorted permutation order is required.
    #[must_use]
    pub fn perm_avet_sorted(&self) -> Vec<u32> {
        layout_to_sorted(self.perm_avet())
    }

    /// TxId-order permutation array in Eytzinger (BFS) layout (INV-FERR-081).
    ///
    /// Lazily builds the permutation on first access. The returned slice
    /// has `n + 1` elements: index 0 is a sentinel (`u32::MAX`), root at index 1.
    /// Use `perm_txid_sorted()` for the original sorted permutation.
    /// Enables O(log N) temporal range queries across all entities.
    ///
    /// Uses canonical position as a stable tiebreaker when two datoms share
    /// the same `TxId`, ensuring deterministic permutation order regardless
    /// of sort algorithm stability (INV-FERR-081).
    #[must_use]
    pub fn perm_txid(&self) -> &[u32] {
        self.perm_txid.get_or_init(|| {
            let mut indices: Vec<u32> =
                (0..u32::try_from(self.canonical.len()).unwrap_or(0)).collect();
            indices.sort_by(|&a, &b| {
                self.canonical[a as usize]
                    .tx()
                    .cmp(&self.canonical[b as usize].tx())
                    .then_with(|| a.cmp(&b))
            });
            layout_permutation(&indices)
        })
    }

    /// Recover the sorted `TxId` permutation from Eytzinger layout (INV-FERR-081).
    ///
    /// O(n) in-order traversal. Used for checkpoint serialization where
    /// the original sorted permutation order is required.
    #[must_use]
    pub fn perm_txid_sorted(&self) -> Vec<u32> {
        layout_to_sorted(self.perm_txid())
    }
}
