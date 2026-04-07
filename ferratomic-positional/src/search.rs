//! Interpolation search on EAVT canonical array (INV-FERR-077, contributes to INV-FERR-027).
//!
//! O(log log n) expected for inter-entity lookup (BLAKE3 uniformity).
//! Within a same-entity block (multiple datoms per entity), degrades to
//! O(log k) binary search where k = datoms sharing the entity prefix.

use ferratom::{Datom, EntityId};
use ferratomic_index::EavtKey;

use crate::{bloom::EntityBloom, mph::Mph, store::PositionalStore};

/// Interpolation search on EAVT-sorted canonical array (INV-FERR-077, contributes to INV-FERR-027).
///
/// O(log log n) expected for inter-entity lookup (BLAKE3 uniformity).
/// Within a same-entity block (multiple datoms per entity), degrades to
/// O(log k) binary search where k = datoms sharing the entity prefix.
/// Uses the first 8 bytes of `EntityId` as a u64 interpolation key.
/// Falls back to midpoint when all entities in the current range share
/// the same 8-byte prefix (same-entity block or division-by-zero guard).
///
/// # Overflow safety
///
/// The interpolation formula `lo + (key - lo_val) * (hi - lo) / (hi_val - lo_val)`
/// uses `u128` arithmetic to prevent overflow. `key_val`, `lo_val`, and `hi_val`
/// are `u64` values (8-byte entity prefixes), and `hi - lo` is at most
/// `u32::MAX` because INV-FERR-076 constrains the canonical array length
/// to `u32` (checked via `debug_assert` in all constructors). The widest
/// intermediate product is `u64 * u64 = u128`, which fits without overflow.
/// The final `usize::try_from(ratio).unwrap_or(hi)` safely degrades to a
/// midpoint-like fallback if the ratio exceeds `usize::MAX` (impossible in
/// practice given the `u32` length constraint, but defensive).
pub(crate) fn interpolation_search<'a>(canonical: &'a [Datom], key: &EavtKey) -> Option<&'a Datom> {
    if canonical.is_empty() {
        return None;
    }
    let mut lo: usize = 0;
    let mut hi: usize = canonical.len() - 1;

    while lo <= hi {
        let lo_val = entity_u64(&canonical[lo]);
        let hi_val = entity_u64(&canonical[hi]);
        let key_val = entity_key_u64(key);

        let pos = if hi_val == lo_val {
            lo + (hi - lo) / 2
        } else {
            let numerator =
                u128::from(key_val.saturating_sub(lo_val)) * u128::from((hi - lo) as u64);
            let denominator = u128::from(hi_val - lo_val);
            let ratio = numerator / denominator;
            // NEG-FERR-001: unwrap_or is panic-free -- falls back to hi on overflow
            let estimate = lo + usize::try_from(ratio).unwrap_or(hi);
            estimate.clamp(lo, hi)
        };

        match EavtKey::from_datom(&canonical[pos]).cmp(key) {
            std::cmp::Ordering::Equal => return Some(&canonical[pos]),
            std::cmp::Ordering::Less => lo = pos + 1,
            std::cmp::Ordering::Greater => {
                if pos == 0 {
                    return None;
                }
                hi = pos - 1;
            }
        }
    }
    None
}

/// Extract first 8 bytes of a `Datom`'s `EntityId` as big-endian u64.
fn entity_u64(datom: &Datom) -> u64 {
    let eid = datom.entity();
    let b = eid.as_bytes();
    u64::from_be_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]])
}

/// Extract first 8 bytes of an `EavtKey`'s entity as big-endian u64.
fn entity_key_u64(key: &EavtKey) -> u64 {
    let eid = key.entity();
    let b = eid.as_bytes();
    u64::from_be_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]])
}

// ---------------------------------------------------------------------------
// Entity lookup methods on PositionalStore (INV-FERR-027, INV-FERR-076)
// ---------------------------------------------------------------------------

impl PositionalStore {
    /// Sorted unique `EntityId`s from the canonical array (INV-FERR-076).
    ///
    /// Since canonical is EAVT-sorted, entities are grouped. A single O(n)
    /// pass extracts distinct entity IDs in sorted order. Shared by
    /// MPH (bd-wa5p) and Bloom filter (bd-218b).
    #[must_use]
    pub fn unique_entity_ids(&self) -> Vec<EntityId> {
        self.unique_entities_with_positions().0
    }

    /// Sorted unique `EntityId`s AND their first canonical positions (INV-FERR-076).
    ///
    /// Single O(n) pass. For each new entity encountered, records the entity
    /// ID and the canonical index of its first datom. Used by MPH construction
    /// to enable O(1) entity -> position lookup.
    #[must_use]
    fn unique_entities_with_positions(&self) -> (Vec<EntityId>, Vec<u32>) {
        let mut keys = Vec::new();
        let mut positions = Vec::new();
        let mut prev: Option<EntityId> = None;
        for (i, datom) in self.canonical.iter().enumerate() {
            let eid = datom.entity();
            if prev.as_ref() != Some(&eid) {
                keys.push(eid);
                positions.push(u32::try_from(i).unwrap_or(u32::MAX));
                prev = Some(eid);
            }
        }
        (keys, positions)
    }

    /// O(1) entity existence check + position lookup (INV-FERR-076, contributes to INV-FERR-027).
    ///
    /// Both positive and negative paths are O(1): MPH hash -> reverse lookup
    /// -> key verification -> position from precomputed `entity_first_pos`.
    ///
    /// The MPH is lazily built on first call. If build fails (extremely
    /// unlikely for BLAKE3 keys), falls back to binary search.
    #[must_use]
    pub fn entity_lookup(&self, eid: &EntityId) -> Option<u32> {
        let mph_opt = self.mph.get_or_init(|| {
            let (keys, positions) = self.unique_entities_with_positions();
            Mph::build(&keys, positions)
        });

        if let Some(mph) = mph_opt {
            return mph.entity_position(eid, &self.canonical);
        }

        // MPH build failed -> binary search fallback
        self.first_datom_position_for_entity(eid)
    }

    /// Find the position of the first datom for a given entity (O(log n)).
    ///
    /// Since canonical is EAVT-sorted, all datoms for an entity are
    /// contiguous. `partition_point` finds the first one.
    #[must_use]
    pub(crate) fn first_datom_position_for_entity(&self, eid: &EntityId) -> Option<u32> {
        let pos = self.canonical.partition_point(|d| d.entity() < *eid);
        if pos < self.canonical.len() && self.canonical[pos].entity() == *eid {
            u32::try_from(pos).ok()
        } else {
            None
        }
    }

    /// O(1) probabilistic entity existence check (INV-FERR-027, bd-218b).
    ///
    /// Bloom filter provides definitive negative answers: if the filter
    /// says "no", the entity is absent (zero false negatives by construction
    /// -- monotonicity of bitwise OR). Positive answers are verified by
    /// binary search (false positive rate ~1% at 10 bits/element).
    ///
    /// Construction is infallible -- no capacity overflow, no fallback path.
    #[must_use]
    pub fn entity_exists(&self, eid: &EntityId) -> bool {
        let bloom = self
            .bloom
            .get_or_init(|| EntityBloom::build(&self.unique_entity_ids()));
        if !bloom.maybe_contains(eid) {
            return false; // definitive negative -- entity absent
        }
        // Possible false positive -- verify with binary search.
        self.first_datom_position_for_entity(eid).is_some()
    }
}
