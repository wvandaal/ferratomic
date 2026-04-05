//! CHD perfect hash for O(1) entity rank computation (contributes to INV-FERR-027).
//!
//! Provides O(1) entity existence checking and monotone rank computation on
//! frozen `PositionalStore`s. The hash function itself is non-monotone and
//! non-minimal (3-hash CHD with 1.25x slot overprovisioning). The rank
//! result IS monotone: `lookup_key_index` returns the index in the sorted
//! unique entity array, which preserves key order by construction.
//!
//! Architecture: `MphBackend` trait defines the interface. `ChdBackend` is the
//! Phase 4a implementation. Phase 4c+ optimization target: swap to `PtrHash`
//! (Pibiri 2025, 2.0 bits/key, 8ns, `ptr_hash` crate) via the trait boundary.
//! ZERO caller changes -- callers use `Mph`, not the backend directly.
//!
//! Zero unsafe. Zero new dependencies. `EntityId` BLAKE3 bytes used directly
//! as h1/h2/h3 hash inputs (uniformly distributed by construction, INV-FERR-012).

use ferratom::EntityId;

// ---------------------------------------------------------------------------
// MphBackend trait (contributes to INV-FERR-027)
// ---------------------------------------------------------------------------

/// Backend trait for perfect hash implementations (contributes to INV-FERR-027).
///
/// Implementors map a known set of `EntityId`s to slots `[0..n)`.
/// The contract:
/// - `build()` succeeds for uniformly distributed BLAKE3 keys (with
///   overwhelming probability).
/// - For every key k in the construction set: `lookup(k)` returns a unique
///   slot in `[0..n)`.
/// - For keys NOT in the construction set: `lookup` returns an ARBITRARY
///   slot (caller must verify).
/// - `lookup` is O(1).
///
/// To swap backends: implement this trait for your crate's type, then change
/// `Mph::build()` to construct it instead of `ChdBackend`. Nothing else changes.
pub(crate) trait MphBackend: Clone + std::fmt::Debug {
    /// Build from sorted unique `EntityId`s. Returns `None` on failure.
    fn build(sorted_keys: &[EntityId]) -> Option<Self>
    where
        Self: Sized;

    /// O(1) lookup: key -> slot index.
    fn lookup(&self, key: &EntityId) -> usize;

    /// Reverse lookup: slot -> key index in the original sorted key array.
    ///
    /// Returns `None` if the slot is unoccupied (non-minimal hash overflow
    /// slot) or out of range.
    fn reverse_lookup(&self, slot: usize) -> Option<usize>;
}

// ---------------------------------------------------------------------------
// ChdBackend (Compress-Hash-Displace, Belazzougui et al. 2009)
// ---------------------------------------------------------------------------

/// Maximum displacement value to try per bucket before declaring failure.
const MAX_DISPLACEMENT: u32 = 1024;

/// Ratio of keys per bucket. Higher = fewer buckets = more collisions per
/// bucket but simpler displacement search. 4 is a good default for CHD.
const BUCKET_RATIO: usize = 4;

/// CHD backend: safe Rust. Zero unsafe. Zero external deps.
///
/// Uses `EntityId` BLAKE3 bytes directly: bytes `[0..8]` as h1,
/// bytes `[8..16]` as h2, bytes `[16..24]` as h3
/// (INV-FERR-012: content-addressed identity).
#[derive(Clone, Debug)]
pub(crate) struct ChdBackend {
    /// One displacement value per bucket.
    displacements: Vec<u32>,
    /// Reverse mapping: `slot_to_key_idx[slot]` = index in sorted keys array.
    /// `~1.25 × key count` slots (non-minimal overprovisioning).
    slot_to_key_idx: Vec<u32>,
}

/// Primary hash: first 8 bytes of `EntityId` as big-endian u64 (INV-FERR-012).
fn h1(eid: &EntityId) -> u64 {
    let b = eid.as_bytes();
    u64::from_be_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]])
}

/// Secondary hash: bytes 8..16 of `EntityId` as big-endian u64 (INV-FERR-012).
fn h2(eid: &EntityId) -> u64 {
    let b = eid.as_bytes();
    u64::from_be_bytes([b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15]])
}

/// Tertiary hash: bytes 16..24 of `EntityId` as big-endian u64 (INV-FERR-012).
///
/// Used as a multiplicative displacement factor: `slot = (h2 + d * h3) % n`.
/// Without h3, keys sharing `h2 mod n` collide for ALL displacement values d.
/// The multiplicative term breaks this fixed-offset degeneracy.
fn h3(eid: &EntityId) -> u64 {
    let b = eid.as_bytes();
    u64::from_be_bytes([b[16], b[17], b[18], b[19], b[20], b[21], b[22], b[23]])
}

/// Reduce a u64 hash value modulo a usize divisor.
///
/// Computes in u64 to avoid truncation on 32-bit targets.
/// Result < modulus <= `usize::MAX`, so `try_from` always succeeds.
/// The `unwrap_or(0)` is a NEG-FERR-001 safety net (no panics in production);
/// `debug_assert` catches the "impossible" path in test/debug builds.
fn reduce_hash(hash_val: u64, modulus: usize) -> usize {
    let result = hash_val % (modulus as u64);
    debug_assert!(
        usize::try_from(result).is_ok(),
        "reduce_hash: result {result} exceeds usize::MAX — modulus invariant violated"
    );
    usize::try_from(result).unwrap_or(0)
}

/// Assign keys to buckets by h1, sorted largest-first for greedy placement.
fn assign_buckets(sorted_keys: &[EntityId], num_buckets: usize) -> Vec<Vec<usize>> {
    let mut buckets: Vec<Vec<usize>> = vec![Vec::new(); num_buckets];
    for (i, key) in sorted_keys.iter().enumerate() {
        buckets[reduce_hash(h1(key), num_buckets)].push(i);
    }
    buckets
}

/// Compute slot for a key given displacement d: `(h2 + d * h3) % num_slots`.
///
/// 3-hash CHD: the multiplicative term `d * h3` ensures that keys sharing
/// the same `h2 mod num_slots` still produce different slot sequences as d
/// varies (provided their h3 values differ, which is overwhelming for BLAKE3).
fn slot_for_key(key: &EntityId, d: u32, num_slots: usize) -> usize {
    reduce_hash(
        h2(key).wrapping_add(u64::from(d).wrapping_mul(h3(key))),
        num_slots,
    )
}

/// Try to place all keys in one bucket using displacement `d`.
///
/// Returns the slot assignments if successful, or `None` on collision.
fn try_displacement(
    bucket: &[usize],
    sorted_keys: &[EntityId],
    d: u32,
    num_slots: usize,
    slot_occupied: &[bool],
) -> Option<Vec<usize>> {
    let mut slots_used = Vec::with_capacity(bucket.len());
    for &key_idx in bucket {
        let slot = slot_for_key(&sorted_keys[key_idx], d, num_slots);
        if slot_occupied[slot] || slots_used.contains(&slot) {
            return None;
        }
        slots_used.push(slot);
    }
    Some(slots_used)
}

impl MphBackend for ChdBackend {
    fn build(sorted_keys: &[EntityId]) -> Option<Self> {
        debug_assert!(
            sorted_keys.windows(2).all(|w| w[0] < w[1]),
            "ChdBackend::build requires strictly sorted, unique EntityIds"
        );
        debug_assert!(
            sorted_keys.len() < u32::MAX as usize,
            "ChdBackend::build: key count {} exceeds u32 position space",
            sorted_keys.len()
        );
        let n = sorted_keys.len();
        if n == 0 {
            return Some(Self {
                displacements: Vec::new(),
                slot_to_key_idx: Vec::new(),
            });
        }
        let num_buckets = n / BUCKET_RATIO + 1;
        // 1.25x overprovisioning: avoids within-bucket h2 collisions that
        // make placement impossible with num_slots = n (standard for 2-hash CHD).
        let num_slots = n + n / 4 + 1;
        let buckets = assign_buckets(sorted_keys, num_buckets);

        // Sort bucket indices by descending size for greedy placement.
        let mut bucket_order: Vec<usize> = (0..num_buckets).collect();
        bucket_order.sort_unstable_by(|&a, &b| buckets[b].len().cmp(&buckets[a].len()));

        let mut displacements = vec![0u32; num_buckets];
        let mut slot_occupied = vec![false; num_slots];
        let mut slot_to_key_idx = vec![u32::MAX; num_slots];

        for &bi in &bucket_order {
            if buckets[bi].is_empty() {
                continue;
            }
            let placed = (0..MAX_DISPLACEMENT).find_map(|d| {
                let slots =
                    try_displacement(&buckets[bi], sorted_keys, d, num_slots, &slot_occupied)?;
                Some((d, slots))
            });
            let (d, slots) = placed?; // None => build failure
            displacements[bi] = d;
            for (si, &slot) in slots.iter().enumerate() {
                slot_occupied[slot] = true;
                slot_to_key_idx[slot] = u32::try_from(buckets[bi][si]).unwrap_or(u32::MAX);
            }
        }

        Some(Self {
            displacements,
            slot_to_key_idx,
        })
    }

    fn lookup(&self, key: &EntityId) -> usize {
        if self.slot_to_key_idx.is_empty() {
            return 0;
        }
        let bucket = reduce_hash(h1(key), self.displacements.len());
        slot_for_key(key, self.displacements[bucket], self.slot_to_key_idx.len())
    }

    fn reverse_lookup(&self, slot: usize) -> Option<usize> {
        let raw = self.slot_to_key_idx.get(slot).copied().unwrap_or(u32::MAX);
        if raw == u32::MAX {
            None
        } else {
            Some(raw as usize)
        }
    }
}

// ---------------------------------------------------------------------------
// Mph wrapper (public API, backend-agnostic) (contributes to INV-FERR-027)
// ---------------------------------------------------------------------------

/// CHD perfect hash with sorted verification for `EntityId`s (contributes to INV-FERR-027).
///
/// Wraps an `MphBackend` implementation + the sorted key set for verification.
/// Callers use `Mph`, never the backend directly. Swapping backends changes
/// `Mph::build()` only -- all callers are unchanged.
///
/// The hash function (CHD) is non-monotone: slot assignments do not preserve
/// key order. However, `lookup_key_index` returns the index in the sorted
/// unique key array, which IS monotone (sorted keys -> sorted indices).
/// This provides the O(1) monotone rank computation required by ADR-FERR-030
/// (wavelet matrix `EntityId` symbol mapping).
///
/// # O(1) Entity Position Lookup
///
/// `entity_position(key, canonical)` returns the first canonical position
/// for `key` if present, `None` if absent. Zero false negatives for
/// construction keys. For absent keys, the backend returns an arbitrary
/// slot; the key comparison against canonical rejects it.
#[derive(Clone, Debug)]
pub(crate) struct Mph {
    /// The MPH backend (Phase 4a: `ChdBackend`).
    backend: ChdBackend,
    /// `entity_first_pos[i]` = canonical position of first datom for entity rank i.
    /// Verification uses `canonical[pos].entity()` — no separate keys storage.
    /// Length = number of unique entities.
    entity_first_pos: Vec<u32>,
}

impl Mph {
    /// Build from sorted unique `EntityId`s and their first canonical positions
    /// (contributes to INV-FERR-027).
    ///
    /// The `sorted_keys` are borrowed for backend construction only — NOT stored.
    /// Verification uses the canonical datom array (passed to `entity_position`).
    /// This saves 32 bytes per unique entity vs storing a keys copy.
    ///
    /// Phase 4a: constructs `ChdBackend`.
    /// To swap: change `ChdBackend::build` to `NewBackend::build` here.
    pub(crate) fn build(sorted_keys: &[EntityId], first_positions: Vec<u32>) -> Option<Self> {
        debug_assert_eq!(
            sorted_keys.len(),
            first_positions.len(),
            "Mph::build: keys and positions must have equal length"
        );
        let backend = ChdBackend::build(sorted_keys)?;
        Some(Self {
            backend,
            entity_first_pos: first_positions,
        })
    }

    /// O(1) entity -> first canonical position (contributes to INV-FERR-027).
    ///
    /// Combines backend hash -> reverse lookup -> key verification against
    /// canonical array -> position from `entity_first_pos`. Both positive
    /// and negative paths are O(1). No separate keys storage needed.
    pub(crate) fn entity_position(
        &self,
        key: &EntityId,
        canonical: &[ferratom::Datom],
    ) -> Option<u32> {
        if self.entity_first_pos.is_empty() {
            return None;
        }
        let slot = self.backend.lookup(key);
        let key_idx = self.backend.reverse_lookup(slot)?;
        // .get() handles bounds check — no redundant key_idx >= len() needed.
        let pos = *self.entity_first_pos.get(key_idx)?;
        // Verify against canonical array — zero-copy, no duplicate keys storage.
        if canonical.get(pos as usize).map(ferratom::Datom::entity) == Some(*key) {
            Some(pos)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use ferratom::{Attribute, Datom, EntityId, Op, TxId, Value};

    use crate::positional::PositionalStore;

    fn make_entity(seed: u8) -> EntityId {
        EntityId::from_content(&[seed])
    }

    /// Build a `PositionalStore` from entity seeds.
    /// Each seed produces one datom with a unique entity.
    fn build_positional(seeds: &[u8]) -> PositionalStore {
        let datoms = seeds.iter().map(|&s| {
            Datom::new(
                make_entity(s),
                Attribute::from("db/doc"),
                Value::String(std::sync::Arc::from("test")),
                TxId::new(0, 0, 0),
                Op::Assert,
            )
        });
        PositionalStore::from_datoms(datoms)
    }

    #[test]
    fn test_inv_ferr_027_mph_empty() {
        let ps = PositionalStore::from_datoms(std::iter::empty());
        assert_eq!(ps.entity_lookup(&make_entity(42)), None);
    }

    #[test]
    fn test_inv_ferr_027_mph_single() {
        let ps = build_positional(&[1]);
        let key = make_entity(1);
        let pos = ps.entity_lookup(&key);
        assert!(pos.is_some(), "present entity found");
        assert_eq!(
            ps.datom_at(pos.expect("asserted Some")).map(Datom::entity),
            Some(key)
        );
        assert_eq!(ps.entity_lookup(&make_entity(99)), None, "absent rejected");
    }

    #[test]
    fn test_inv_ferr_027_mph_completeness() {
        // All entities in the store must be found (zero false negatives).
        let seeds: Vec<u8> = (0..100).collect();
        let ps = build_positional(&seeds);
        for &seed in &seeds {
            let eid = make_entity(seed);
            assert!(
                ps.entity_lookup(&eid).is_some(),
                "false negative for entity with seed {seed}"
            );
        }
    }

    #[test]
    fn test_inv_ferr_027_mph_absent_rejected() {
        let seeds: Vec<u8> = (0..50).collect();
        let ps = build_positional(&seeds);
        for seed in 200..255u8 {
            let absent = make_entity(seed);
            assert_eq!(
                ps.entity_lookup(&absent),
                None,
                "absent entity {seed} should be rejected"
            );
        }
    }

    #[test]
    fn test_inv_ferr_027_mph_positions_bijective() {
        // Each entity maps to a unique first position.
        let seeds: Vec<u8> = (0..100).collect();
        let ps = build_positional(&seeds);
        let mut seen = std::collections::HashSet::new();
        for &seed in &seeds {
            let eid = make_entity(seed);
            let pos = ps.entity_lookup(&eid).expect("entity present");
            assert!(seen.insert(pos), "duplicate position {pos}");
        }
    }
}
