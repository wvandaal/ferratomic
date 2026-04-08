//! `SoA` columnar accessors for [`PositionalStore`] (INV-FERR-078, bd-574c).
//!
//! Extracts per-column views from the canonical datom array for
//! cache-optimal scans that avoid loading full `Datom` cache lines.

use bitvec::prelude::{BitVec, Lsb0};
use ferratom::{AttributeId, AttributeIntern, Datom, EntityId, Op, TxId};

use crate::store::PositionalStore;

impl PositionalStore {
    /// Entity column: `col_entities[p] = canonical[p].entity()` (INV-FERR-078).
    ///
    /// Lazily built on first access. Returns a contiguous `&[EntityId]` slice
    /// for cache-optimal entity-only scans. 32 bytes per datom, avoids loading
    /// full `Datom` cache lines when only the entity is needed.
    #[must_use]
    pub fn col_entities(&self) -> &[EntityId] {
        self.col_entities
            .get_or_init(|| self.canonical.iter().map(Datom::entity).collect())
    }

    /// Transaction column: `col_txids[p] = canonical[p].tx()`.
    ///
    /// Lazily built on first access. Returns a contiguous `&[TxId]` slice
    /// for cache-optimal transaction-order scans. 28 bytes per datom.
    #[must_use]
    pub fn col_txids(&self) -> &[TxId] {
        self.col_txids
            .get_or_init(|| self.canonical.iter().map(Datom::tx).collect())
    }

    /// Op column: `col_ops[p]` = `(canonical[p].op() == Op::Assert)`.
    ///
    /// Lazily built on first access. 1 bit per datom: `true` = Assert,
    /// `false` = Retract. Same `BitVec<u64, Lsb0>` representation as
    /// `live_bits` for consistency.
    #[must_use]
    pub fn col_ops(&self) -> &BitVec<u64, Lsb0> {
        self.col_ops.get_or_init(|| {
            self.canonical
                .iter()
                .map(|d| d.op() == Op::Assert)
                .collect()
        })
    }

    /// Build interned attribute column from an `AttributeIntern` table (ADR-FERR-030).
    ///
    /// Unlike `col_entities`/`col_txids`/`col_ops`, the attribute column
    /// cannot be lazily self-built because `PositionalStore` does not own an
    /// `AttributeIntern`. The caller provides the intern table and receives
    /// the column. 2 bytes per datom plus `Option` tag.
    ///
    /// Returns `Option<AttributeId>` per position to preserve positional
    /// correspondence with the canonical array. `None` means the attribute
    /// at that position is not present in the intern table. Callers that
    /// require a complete column should ensure the intern table covers all
    /// attributes in the store.
    #[must_use]
    pub fn build_col_attrs(&self, intern: &AttributeIntern) -> Vec<Option<AttributeId>> {
        self.canonical
            .iter()
            .map(|d| intern.id_of(d.attribute()))
            .collect()
    }
}
