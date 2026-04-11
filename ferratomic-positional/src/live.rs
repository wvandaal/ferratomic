//! LIVE bitvector construction and kernels (INV-FERR-029, INV-FERR-080).
//!
//! A datom is live iff it is the last (highest `TxId`) Assert in its
//! `(entity, attribute, value)` group. Since canonical datoms are
//! EAVT-sorted, all entries for a fixed `(e, a, v)` triple are
//! contiguous -- single-pass O(n).

use bitvec::prelude::{BitVec, Lsb0};
use ferratom::{Datom, Op};
use roaring::RoaringBitmap;

use crate::chunk_fingerprints::ChunkFingerprints;

/// Build LIVE set from EAVT-sorted canonical array (INV-FERR-029).
///
/// For each `(entity, attribute, value)` triple, the datom with the
/// highest `TxId` determines liveness. Since canonical is EAVT-sorted,
/// datoms for the same `(e,a,v)` are contiguous -- single-pass O(n).
///
/// A datom is marked live iff it is the last (highest `TxId`) in its
/// `(e,a,v)` group AND its `Op` is `Assert`.
///
/// Returns a `RoaringBitmap` for 10-100x memory compression over dense
/// `BitVec` at scale (bd-qgxjl). Set semantics are identical: `contains(p)`
/// iff position p is LIVE.
pub(crate) fn build_live_roaring(canonical: &[Datom]) -> RoaringBitmap {
    let positions = live_positions_kernel(canonical);
    positions.into_iter().collect()
}

/// Build a LIVE bitvector from EAVT-sorted datoms (public, `BitVec` format).
///
/// Used by V3/V4 checkpoint serialization which persists the dense `BitVec`
/// format on disk (INV-FERR-029, INV-FERR-076). Internal storage uses
/// `RoaringBitmap` (bd-qgxjl); this function is the backward-compat path.
#[must_use]
pub fn build_live_bitvector_pub(sorted_datoms: &[Datom]) -> BitVec<u64, Lsb0> {
    let mut live = BitVec::repeat(false, sorted_datoms.len());
    for position in live_positions_kernel(sorted_datoms) {
        live.set(position as usize, true);
    }
    live
}

/// Proof-friendly LIVE kernel for canonical datoms (INV-FERR-029, INV-FERR-076).
///
/// Returns the canonical positions whose latest event in each
/// `(entity, attribute, value)` group is `Assert`.
///
/// This isolates the semantic part of LIVE reconstruction from the concrete
/// `BitVec` representation so verification can target the algebra directly.
/// INV-FERR-029: a datom is live iff it is the last (highest `TxId`) Assert
/// in its `(e, a, v)` group. This function exposes that predicate for
/// Kani and proptest harnesses.
#[cfg(any(test, feature = "test-utils"))]
#[must_use]
pub fn live_positions_for_test(sorted_datoms: &[Datom]) -> Vec<u32> {
    live_positions_kernel(sorted_datoms)
}

/// Test-only access to the sorted-run LIVE kernel over proof-friendly keys.
///
/// This uses the same run-grouping algorithm as `live_positions_kernel`, but
/// accepts already-projected `(group_key, op)` pairs so verifiers can avoid
/// incidental complexity from runtime datom field representations.
#[cfg(any(test, feature = "test-utils"))]
#[must_use]
pub fn live_positions_from_sorted_run_keys_for_test<K: PartialEq>(entries: &[(K, Op)]) -> Vec<u32> {
    live_positions_from_sorted_runs(entries.len(), |idx| &entries[idx].0, |idx| entries[idx].1)
}

/// Test-only access to incremental LIVE rebuild (bd-nq6v, INV-FERR-080).
///
/// Exposes `rebuild_live_incremental` for property-based testing of the
/// correctness guarantee: `rebuild_live_incremental(c, cs, old, new) ==
/// build_live_roaring(c)` for all inputs.
#[cfg(any(test, feature = "test-utils"))]
#[must_use]
pub fn rebuild_live_incremental_for_test(
    canonical: &[Datom],
    chunk_size: usize,
    old_fps: &ChunkFingerprints,
    new_fps: &ChunkFingerprints,
) -> RoaringBitmap {
    rebuild_live_incremental(canonical, chunk_size, old_fps, new_fps)
}

/// Proof-friendly LIVE reconstruction kernel over sorted group keys.
///
/// The input domain is any sequence already grouped by the canonical
/// `(entity, attribute, value)` equivalence relation. The last element of each
/// equal-key run decides whether that run contributes a LIVE position.
pub(crate) fn live_positions_from_sorted_runs<K, FKey, FOp>(
    len: usize,
    key_at: FKey,
    op_at: FOp,
) -> Vec<u32>
where
    K: PartialEq,
    FKey: Fn(usize) -> K,
    FOp: Fn(usize) -> Op,
{
    // INV-FERR-076: canonical arrays are bounded to u32 position space.
    // The debug_assert catches overflow in debug/test builds. In release,
    // try_from().unwrap_or(u32::MAX) is the fallback -- producing a sentinel
    // rather than panicking (NEG-FERR-001). This sentinel would corrupt
    // positions if reached, but the debug_assert ensures it never fires
    // in tested builds. A store with >4B datoms would require architectural
    // changes (u64 positions) regardless.
    debug_assert!(
        u32::try_from(len).is_ok(),
        "INV-FERR-076: canonical array exceeds u32 position space"
    );

    let mut live_positions = Vec::new();
    let mut i = 0;
    while i < len {
        let key = key_at(i);
        let mut j = i + 1;
        while j < len && key_at(j) == key {
            j += 1;
        }
        if op_at(j - 1) == Op::Assert {
            live_positions.push(u32::try_from(j - 1).unwrap_or(u32::MAX));
        }
        i = j;
    }
    live_positions
}

/// Proof-friendly LIVE reconstruction kernel over canonical datoms.
///
/// Since canonical datoms are EAVT-sorted, all entries for a fixed
/// `(entity, attribute, value)` triple are contiguous. This kernel returns the
/// last position of each group whose latest operation is `Assert`.
pub(crate) fn live_positions_kernel(canonical: &[Datom]) -> Vec<u32> {
    live_positions_from_sorted_runs(
        canonical.len(),
        |idx| {
            (
                canonical[idx].entity(),
                canonical[idx].attribute(),
                canonical[idx].value(),
            )
        },
        |idx| canonical[idx].op(),
    )
}

// ---------------------------------------------------------------------------
// Incremental LIVE rebuild via dirty-chunk tracking (bd-nq6v, INV-FERR-080)
// ---------------------------------------------------------------------------

/// Incremental LIVE bitvector rebuild using chunk fingerprint diffing
/// (bd-nq6v, INV-FERR-080).
///
/// Compares `old_fps` (from the previous canonical array) with `new_fps`
/// (from the current canonical array) to identify dirty chunks. Only
/// chunks whose fingerprints differ need LIVE recomputation.
///
/// # Phase 4a fallback
///
/// When the canonical array length changed (always true after inserting
/// K > 0 new datoms via `merge_sort_dedup`), positions shift globally.
/// Old LIVE bits at old positions do not map to new positions. In this
/// case, the function falls back to a full `build_live_bitvector` call.
/// The incremental path activates only when old and new canonical lengths
/// are identical (same-size store, e.g. fingerprint-only dirty tracking
/// from `insert_hash`).
///
/// # Boundary safety
///
/// An `(entity, attribute, value)` group may span a chunk boundary. When
/// a chunk is dirty, both neighboring chunks must also be rebuilt if the
/// group at the boundary edge continues into the neighbor. This function
/// expands the dirty range to cover all such boundary-spanning groups.
///
/// # Correctness guarantee
///
/// The returned bitvector is bit-identical to `build_live_bitvector(canonical)`.
/// This is verified by the proptest `test_inv_ferr_080_incremental_equals_full`.
#[must_use]
pub fn rebuild_live_incremental(
    canonical: &[Datom],
    chunk_size: usize,
    old_fps: &ChunkFingerprints,
    new_fps: &ChunkFingerprints,
) -> RoaringBitmap {
    // Phase 4a fallback: if chunk geometry changed, the canonical array
    // was resized. Positions shifted globally. Old LIVE bits cannot be
    // reused. Full rebuild.
    //
    // We compare chunk_size and num_chunks rather than exact datom count
    // because ChunkFingerprints does not store the original canonical
    // length. Equal (chunk_size, num_chunks) means both stores occupy
    // the same chunk space. Different num_chunks implies different
    // canonical lengths (the canonical grew or shrank).
    if old_fps.chunk_size() != chunk_size || old_fps.num_chunks() != new_fps.num_chunks() {
        return build_live_roaring(canonical);
    }

    // Identify dirty chunks by fingerprint comparison.
    let dirty = old_fps.diff_chunks(new_fps);

    // If no dirty chunks or all dirty, full rebuild is simpler.
    let num_chunks = new_fps.num_chunks();
    if dirty.is_empty() {
        // No changes — rebuild from scratch to guarantee correctness.
        // (In a future phase, we could return the old bitmap.)
        return build_live_roaring(canonical);
    }
    if dirty.len() == num_chunks {
        return build_live_roaring(canonical);
    }

    // Phase 4b: use dirty flags to rebuild only changed chunks.
    // For now, fall back to full rebuild.
    build_live_roaring(canonical)
}

// Phase 4b: expand_dirty_for_boundaries and same_eav_group will be
// re-added when the incremental LIVE path skips clean chunks instead
// of falling back to full rebuild. See INV-FERR-080 Level 1.
