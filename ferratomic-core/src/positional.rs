//! Positional content addressing (INV-FERR-076).
//!
//! Every datom in the store has a canonical position `p : u32` in the
//! sorted canonical array. Positions serve as internal addresses for
//! index permutations, LIVE bitvector, and merge bookkeeping.
//!
//! This is a faithful functor from the datom semilattice to the natural
//! number ordering: same datom set → same sort → same positions.
//!
//! INV-FERR-076: positional determinism, stability under append,
//! LIVE as bitvector, merge as merge-sort.

use std::sync::OnceLock;

use bitvec::prelude::{BitVec, Lsb0};
use ferratom::{Datom, EntityId, Op};

use crate::{
    indexes::{AevtKey, AvetKey, EavtKey, VaetKey},
    mph::Mph,
};

// ---------------------------------------------------------------------------
// PositionalStore (INV-FERR-076)
// ---------------------------------------------------------------------------

/// Positional content addressing store (INV-FERR-076).
///
/// Replaces `OrdSet<Datom>` + 4x`OrdMap` with contiguous arrays:
/// - `canonical`: sorted `Vec<Datom>` (EAVT order). Position = index.
/// - `live_bits`: `BitVec` where `live_bits[p]` = datom p is live.
/// - `perm_aevt/vaet/avet`: lazily-built permutation arrays mapping alternate
///   sort orders to canonical positions (`OnceLock` for deferred construction).
/// - `fingerprint`: XOR-sum placeholder for INV-FERR-074 (Stage 1).
///
/// Memory at 200K datoms: ~26 MB vs ~159 MB with `im::OrdMap`.
/// Cold start: O(n log n) sort vs O(n) tree insertions with pointer chasing.
/// Permutations are built on first access, not at construction time.
///
/// # Visibility
///
/// `pub` because verification tests in `ferratomic-verify` exercise
/// positional store properties (INV-FERR-076 determinism, INV-FERR-029
/// LIVE bitvector correctness, merge commutativity on positional stores).
/// Also used by `Store::from_positional` for internal representation
/// selection.
pub struct PositionalStore {
    /// Datoms in canonical (EAVT) sorted order (INV-FERR-076).
    canonical: Vec<Datom>,
    /// LIVE bitvector: `live_bits[p] = true` iff the datom at position
    /// p is the latest Assert for its `(entity, attribute, value)` triple
    /// (INV-FERR-029). 1 bit per datom — 25 KB at 200K datoms.
    live_bits: BitVec<u64, Lsb0>,
    /// Permutation: AEVT-order index → canonical position (INV-FERR-005).
    /// Lazily built on first access via `OnceLock`.
    perm_aevt: OnceLock<Vec<u32>>,
    /// Permutation: VAET-order index → canonical position (INV-FERR-005).
    /// Lazily built on first access via `OnceLock`.
    perm_vaet: OnceLock<Vec<u32>>,
    /// Permutation: AVET-order index → canonical position (INV-FERR-005).
    /// Lazily built on first access via `OnceLock`.
    perm_avet: OnceLock<Vec<u32>>,
    /// Homomorphic fingerprint placeholder (INV-FERR-074, Stage 1).
    fingerprint: [u8; 32],
    /// CHD perfect hash for O(1) entity existence checks (INV-FERR-076, contributes to INV-FERR-027).
    /// Lazily built on first `entity_lookup()` call via `OnceLock`.
    /// `None` if build fails (fallback to binary search).
    mph: OnceLock<Option<Mph>>,
}

impl Clone for PositionalStore {
    fn clone(&self) -> Self {
        Self {
            canonical: self.canonical.clone(),
            live_bits: self.live_bits.clone(),
            perm_aevt: self.perm_aevt.get().map_or_else(OnceLock::new, |v| {
                let lock = OnceLock::new();
                let _ = lock.set(v.clone());
                lock
            }),
            perm_vaet: self.perm_vaet.get().map_or_else(OnceLock::new, |v| {
                let lock = OnceLock::new();
                let _ = lock.set(v.clone());
                lock
            }),
            perm_avet: self.perm_avet.get().map_or_else(OnceLock::new, |v| {
                let lock = OnceLock::new();
                let _ = lock.set(v.clone());
                lock
            }),
            fingerprint: self.fingerprint,
            mph: self.mph.get().map_or_else(OnceLock::new, |v| {
                let lock = OnceLock::new();
                let _ = lock.set(v.clone());
                lock
            }),
        }
    }
}

impl std::fmt::Debug for PositionalStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PositionalStore")
            .field("canonical_len", &self.canonical.len())
            .field("live_bits_len", &self.live_bits.len())
            .field("perm_aevt_init", &self.perm_aevt.get().is_some())
            .field("perm_vaet_init", &self.perm_vaet.get().is_some())
            .field("perm_avet_init", &self.perm_avet.get().is_some())
            .field("fingerprint", &self.fingerprint)
            .field("mph_init", &self.mph.get().is_some())
            .finish()
    }
}

impl PositionalStore {
    /// Build from an unsorted datom iterator (INV-FERR-076).
    ///
    /// O(n log n) for sort + O(n) for LIVE scan. Permutation arrays are
    /// deferred to first access via `OnceLock` (lazy construction).
    /// Uses `sort_unstable` — O(1) auxiliary memory, matching the
    /// performance architecture targets (INV-FERR-076).
    #[must_use]
    pub fn from_datoms(datoms: impl Iterator<Item = Datom>) -> Self {
        let mut canonical: Vec<Datom> = datoms.collect();
        canonical.sort_unstable();
        canonical.dedup();
        debug_assert!(
            u32::try_from(canonical.len()).is_ok(),
            "INV-FERR-076: canonical array exceeds u32 position space"
        );

        let live_bits = build_live_bitvector(&canonical);

        Self {
            canonical,
            live_bits,
            perm_aevt: OnceLock::new(),
            perm_vaet: OnceLock::new(),
            perm_avet: OnceLock::new(),
            fingerprint: [0u8; 32],
            mph: OnceLock::new(),
        }
    }

    /// Construct from a pre-sorted, deduplicated datom vector.
    ///
    /// INV-FERR-076: the caller guarantees `canonical` is EAVT-sorted and
    /// duplicate-free. Checked via `debug_assert` only — release builds do
    /// not validate. Callers loading from untrusted sources must verify
    /// integrity independently (e.g., BLAKE3 per ADR-FERR-010).
    /// This is the O(n) construction path for merge results produced by
    /// `merge_sort_dedup`, which outputs sorted, deduplicated data.
    /// Skips the O(n log n) `sort_unstable()` call in `from_datoms`.
    #[must_use]
    pub(crate) fn from_sorted_canonical(canonical: Vec<Datom>) -> Self {
        debug_assert!(
            canonical.windows(2).all(|w| w[0] < w[1]),
            "INV-FERR-076: from_sorted_canonical requires strictly sorted input"
        );
        debug_assert!(
            u32::try_from(canonical.len()).is_ok(),
            "INV-FERR-076: canonical array exceeds u32 position space"
        );

        let live_bits = build_live_bitvector(&canonical);

        Self {
            canonical,
            live_bits,
            perm_aevt: OnceLock::new(),
            perm_vaet: OnceLock::new(),
            perm_avet: OnceLock::new(),
            fingerprint: [0u8; 32],
            mph: OnceLock::new(),
        }
    }

    /// Number of datoms in the canonical array (INV-FERR-076).
    #[must_use]
    pub fn len(&self) -> usize {
        self.canonical.len()
    }

    /// Whether the store is empty (INV-FERR-076).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.canonical.is_empty()
    }

    /// Canonical position lookup: O(log n) binary search (INV-FERR-076).
    #[must_use]
    pub fn position_of(&self, datom: &Datom) -> Option<u32> {
        self.canonical
            .binary_search(datom)
            .ok()
            .and_then(|i| u32::try_from(i).ok())
    }

    /// LIVE check: O(1) bit test (INV-FERR-029, INV-FERR-076).
    #[must_use]
    pub fn is_live(&self, position: u32) -> bool {
        let pos = position as usize;
        pos < self.live_bits.len() && self.live_bits[pos]
    }

    /// Datom at canonical position: O(1) array index (INV-FERR-076).
    #[must_use]
    pub fn datom_at(&self, position: u32) -> Option<&Datom> {
        self.canonical.get(position as usize)
    }

    /// Slice of all datoms in canonical EAVT order (INV-FERR-076).
    #[must_use]
    pub fn datoms(&self) -> &[Datom] {
        &self.canonical
    }

    /// Iterator over live datoms only (INV-FERR-029, INV-FERR-076).
    ///
    /// Returns datoms where `live_bits[p] = true` — the latest Assert
    /// for each `(entity, attribute, value)` triple.
    pub fn live_datoms(&self) -> impl Iterator<Item = &Datom> + '_ {
        self.canonical
            .iter()
            .zip(self.live_bits.iter())
            .filter_map(|(d, live)| if *live { Some(d) } else { None })
    }

    /// EAVT lookup: O(log log n) interpolation search on canonical array (INV-FERR-027).
    ///
    /// `EntityId` is BLAKE3 (uniformly distributed), so interpolation search
    /// achieves O(log log n) expected complexity. Falls back to midpoint
    /// when the range has identical entity prefixes.
    #[must_use]
    pub fn eavt_get(&self, key: &EavtKey) -> Option<&Datom> {
        interpolation_search(&self.canonical, key)
    }

    /// AEVT lookup: O(log n) cache-oblivious search on Eytzinger layout (INV-FERR-027, INV-FERR-071).
    ///
    /// Lazily builds the AEVT permutation in Eytzinger (BFS) order on first access.
    #[must_use]
    pub fn aevt_get(&self, key: &AevtKey) -> Option<&Datom> {
        let perm = self.perm_aevt.get_or_init(|| {
            let sorted = build_permutation(&self.canonical, AevtKey::from_datom);
            crate::perm_layout::layout_permutation(&sorted)
        });
        crate::perm_layout::layout_search(perm, &self.canonical, |d| {
            AevtKey::from_datom(d).cmp(key)
        })
    }

    /// VAET lookup: O(log n) cache-oblivious search on Eytzinger layout (INV-FERR-027, INV-FERR-071).
    ///
    /// Lazily builds the VAET permutation in Eytzinger (BFS) order on first access.
    #[must_use]
    pub fn vaet_get(&self, key: &VaetKey) -> Option<&Datom> {
        let perm = self.perm_vaet.get_or_init(|| {
            let sorted = build_permutation(&self.canonical, VaetKey::from_datom);
            crate::perm_layout::layout_permutation(&sorted)
        });
        crate::perm_layout::layout_search(perm, &self.canonical, |d| {
            VaetKey::from_datom(d).cmp(key)
        })
    }

    /// AVET lookup: O(log n) cache-oblivious search on Eytzinger layout (INV-FERR-027, INV-FERR-071).
    ///
    /// Lazily builds the AVET permutation in Eytzinger (BFS) order on first access.
    #[must_use]
    pub fn avet_get(&self, key: &AvetKey) -> Option<&Datom> {
        let perm = self.perm_avet.get_or_init(|| {
            let sorted = build_permutation(&self.canonical, AvetKey::from_datom);
            crate::perm_layout::layout_permutation(&sorted)
        });
        crate::perm_layout::layout_search(perm, &self.canonical, |d| {
            AvetKey::from_datom(d).cmp(key)
        })
    }

    /// Access the AEVT permutation array in Eytzinger (BFS) order (INV-FERR-071, INV-FERR-076).
    ///
    /// Lazily builds the permutation on first access. The returned slice
    /// has `n + 1` elements: index 0 is a sentinel (`u32::MAX`), root at index 1.
    /// Use `perm_aevt_sorted()` for the original sorted permutation.
    #[must_use]
    pub fn perm_aevt(&self) -> &[u32] {
        self.perm_aevt.get_or_init(|| {
            let sorted = build_permutation(&self.canonical, AevtKey::from_datom);
            crate::perm_layout::layout_permutation(&sorted)
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
            crate::perm_layout::layout_permutation(&sorted)
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
            crate::perm_layout::layout_permutation(&sorted)
        })
    }

    /// Recover the sorted AEVT permutation from Eytzinger layout (INV-FERR-071).
    ///
    /// O(n) in-order traversal. Used for checkpoint serialization where
    /// the original sorted permutation order is required.
    #[must_use]
    pub fn perm_aevt_sorted(&self) -> Vec<u32> {
        crate::perm_layout::layout_to_sorted(self.perm_aevt())
    }

    /// Recover the sorted VAET permutation from Eytzinger layout (INV-FERR-071).
    ///
    /// O(n) in-order traversal. Used for checkpoint serialization where
    /// the original sorted permutation order is required.
    #[must_use]
    pub fn perm_vaet_sorted(&self) -> Vec<u32> {
        crate::perm_layout::layout_to_sorted(self.perm_vaet())
    }

    /// Recover the sorted AVET permutation from Eytzinger layout (INV-FERR-071).
    ///
    /// O(n) in-order traversal. Used for checkpoint serialization where
    /// the original sorted permutation order is required.
    #[must_use]
    pub fn perm_avet_sorted(&self) -> Vec<u32> {
        crate::perm_layout::layout_to_sorted(self.perm_avet())
    }

    /// Length of the LIVE bitvector (INV-FERR-076: equals `len()`).
    #[must_use]
    pub fn live_bits_len(&self) -> usize {
        self.live_bits.len()
    }

    /// Number of live datoms (INV-FERR-029).
    #[must_use]
    pub fn live_count(&self) -> usize {
        self.live_bits.count_ones()
    }

    /// Access the fingerprint (INV-FERR-074 placeholder).
    #[must_use]
    pub fn fingerprint(&self) -> &[u8; 32] {
        &self.fingerprint
    }

    /// Clone the LIVE bitvector for checkpoint serialization (INV-FERR-076).
    ///
    /// V3 checkpoints persist the bitvector to skip recomputation on load.
    #[must_use]
    pub(crate) fn live_bits_clone(&self) -> BitVec<u64, Lsb0> {
        self.live_bits.clone()
    }

    /// Build from pre-sorted datoms and a pre-computed LIVE bitvector.
    ///
    /// INV-FERR-076: Zero-construction cold start for V3 checkpoint
    /// deserialization. The caller guarantees `canonical` is sorted and
    /// `live_bits.len() == canonical.len()`. Checked via `debug_assert`
    /// only — release builds do not validate. Callers loading from
    /// untrusted sources must verify integrity independently (e.g.,
    /// BLAKE3 per ADR-FERR-010). Permutation arrays are deferred
    /// (`OnceLock::new()`).
    #[must_use]
    pub(crate) fn from_sorted_with_live(
        canonical: Vec<Datom>,
        live_bits: BitVec<u64, Lsb0>,
    ) -> Self {
        debug_assert!(
            live_bits.len() == canonical.len(),
            "INV-FERR-076: live_bits length ({}) must equal canonical length ({})",
            live_bits.len(),
            canonical.len(),
        );
        debug_assert!(
            canonical.windows(2).all(|w| w[0] < w[1]),
            "INV-FERR-076: canonical datoms must be strictly sorted (EAVT order, no duplicates)",
        );
        debug_assert!(
            u32::try_from(canonical.len()).is_ok(),
            "INV-FERR-076: canonical array exceeds u32 position space"
        );
        Self {
            canonical,
            live_bits,
            perm_aevt: OnceLock::new(),
            perm_vaet: OnceLock::new(),
            perm_avet: OnceLock::new(),
            fingerprint: [0u8; 32],
            mph: OnceLock::new(),
        }
    }

    /// Sorted unique `EntityId`s from the canonical array (INV-FERR-076).
    ///
    /// Since canonical is EAVT-sorted, entities are grouped. A single O(n)
    /// pass extracts distinct entity IDs in sorted order. Shared by
    /// MPH (bd-wa5p) and cuckoo filter (bd-218b).
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
    fn first_datom_position_for_entity(&self, eid: &EntityId) -> Option<u32> {
        let pos = self.canonical.partition_point(|d| d.entity() < *eid);
        if pos < self.canonical.len() && self.canonical[pos].entity() == *eid {
            u32::try_from(pos).ok()
        } else {
            None
        }
    }
}

/// Build a LIVE bitvector from EAVT-sorted datoms (public within crate).
///
/// Delegates to `build_live_bitvector`. Used by V3 checkpoint deserialization
/// when loading from non-Positional stores (INV-FERR-029, INV-FERR-076).
pub(crate) fn build_live_bitvector_pub(sorted_datoms: &[Datom]) -> BitVec<u64, Lsb0> {
    build_live_bitvector(sorted_datoms)
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

// ---------------------------------------------------------------------------
// Merge (INV-FERR-076 + INV-FERR-001)
// ---------------------------------------------------------------------------

/// CRDT merge via merge-sort on canonical arrays (INV-FERR-076).
///
/// INV-FERR-001: commutativity -- `merge(a,b) = merge(b,a)` because
/// merge-sort of two sorted arrays is commutative on set semantics.
/// INV-FERR-002: associativity -- `merge(a, merge(b, c)) = merge(merge(a, b), c)`.
/// INV-FERR-003: idempotency -- `merge(a, a) = a`.
/// INV-FERR-004: monotonic growth -- `|merge(a,b)| >= max(|a|, |b|)`.
///
/// O(n + m) merge-sort + O(n) LIVE rebuild. Permutations are deferred (lazy).
///
/// # Visibility
///
/// `pub` because verification tests in `ferratomic-verify` exercise
/// positional merge commutativity and idempotency properties directly.
#[must_use]
pub fn merge_positional(a: &PositionalStore, b: &PositionalStore) -> PositionalStore {
    let merged = merge_sort_dedup(&a.canonical, &b.canonical);
    let live_bits = build_live_bitvector(&merged);
    PositionalStore {
        canonical: merged,
        live_bits,
        perm_aevt: OnceLock::new(),
        perm_vaet: OnceLock::new(),
        perm_avet: OnceLock::new(),
        fingerprint: [0u8; 32],
        mph: OnceLock::new(),
    }
}

// ---------------------------------------------------------------------------
// Internal: LIVE bitvector construction (INV-FERR-029)
// ---------------------------------------------------------------------------

/// Build LIVE bitvector from EAVT-sorted canonical array (INV-FERR-029).
///
/// For each `(entity, attribute, value)` triple, the datom with the
/// highest `TxId` determines liveness. Since canonical is EAVT-sorted,
/// datoms for the same `(e,a,v)` are contiguous — single-pass O(n).
///
/// A datom is marked live iff it is the last (highest `TxId`) in its
/// `(e,a,v)` group AND its `Op` is `Assert`.
fn build_live_bitvector(canonical: &[Datom]) -> BitVec<u64, Lsb0> {
    let mut live = BitVec::repeat(false, canonical.len());
    for position in live_positions_kernel(canonical) {
        live.set(position as usize, true);
    }
    live
}

/// Proof-friendly LIVE reconstruction kernel over sorted group keys.
///
/// The input domain is any sequence already grouped by the canonical
/// `(entity, attribute, value)` equivalence relation. The last element of each
/// equal-key run decides whether that run contributes a LIVE position.
fn live_positions_from_sorted_runs<K, FKey, FOp>(len: usize, key_at: FKey, op_at: FOp) -> Vec<u32>
where
    K: PartialEq,
    FKey: Fn(usize) -> K,
    FOp: Fn(usize) -> Op,
{
    // INV-FERR-076: canonical arrays are bounded to u32 position space.
    // The debug_assert catches overflow in debug/test builds. In release,
    // try_from().unwrap_or(u32::MAX) is the fallback — producing a sentinel
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
fn live_positions_kernel(canonical: &[Datom]) -> Vec<u32> {
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
// Internal: permutation construction (INV-FERR-073/076)
// ---------------------------------------------------------------------------

/// Build a permutation array by sorting indices by a key extractor.
///
/// `perm[i]` = canonical position of the i-th element in alternate order.
/// O(n log n) sort on u32 indices — cache-optimal.
fn build_permutation<F, K: Ord>(canonical: &[Datom], key_fn: F) -> Vec<u32>
where
    F: Fn(&Datom) -> K,
{
    let mut perm: Vec<u32> = (0..canonical.len())
        .map(|i| u32::try_from(i).unwrap_or(u32::MAX))
        .collect();
    perm.sort_unstable_by(|&a, &b| {
        let da = &canonical[a as usize];
        let db = &canonical[b as usize];
        key_fn(da).cmp(&key_fn(db))
    });
    perm
}

// ---------------------------------------------------------------------------
// Internal: merge-sort with dedup (INV-FERR-001/076)
// ---------------------------------------------------------------------------

/// Merge two sorted, deduplicated slices into a single sorted, deduplicated
/// Vec (INV-FERR-001: set union via merge-sort).
///
/// O(n + m) time, sequential access on both inputs.
///
/// `pub(crate)` because `store::merge` uses this for mixed-repr merge
/// (Positional + `OrdMap`) to avoid the O(n log n) sort in `from_datoms`.
pub(crate) fn merge_sort_dedup(a: &[Datom], b: &[Datom]) -> Vec<Datom> {
    debug_assert!(
        a.windows(2).all(|w| w[0] < w[1]),
        "INV-FERR-001: merge_sort_dedup requires sorted, deduplicated input (a)"
    );
    debug_assert!(
        b.windows(2).all(|w| w[0] < w[1]),
        "INV-FERR-001: merge_sort_dedup requires sorted, deduplicated input (b)"
    );
    let mut result = Vec::with_capacity(a.len() + b.len());
    let mut ia = 0;
    let mut ib = 0;
    while ia < a.len() && ib < b.len() {
        match a[ia].cmp(&b[ib]) {
            std::cmp::Ordering::Less => {
                result.push(a[ia].clone());
                ia += 1;
            }
            std::cmp::Ordering::Greater => {
                result.push(b[ib].clone());
                ib += 1;
            }
            std::cmp::Ordering::Equal => {
                result.push(a[ia].clone());
                ia += 1;
                ib += 1; // dedup: skip duplicate
            }
        }
    }
    for datom in &a[ia..] {
        result.push(datom.clone());
    }
    for datom in &b[ib..] {
        result.push(datom.clone());
    }
    result
}

// ---------------------------------------------------------------------------
// Internal: interpolation search on EAVT canonical array (INV-FERR-027)
// ---------------------------------------------------------------------------

/// Interpolation search on EAVT-sorted canonical array (contributes to INV-FERR-027).
///
/// O(log log n) expected for inter-entity lookup (BLAKE3 uniformity).
/// Within a same-entity block (multiple datoms per entity), degrades to
/// O(log k) binary search where k = datoms sharing the entity prefix.
/// Uses the first 8 bytes of `EntityId` as a u64 interpolation key.
/// Falls back to midpoint when all entities in the current range share
/// the same 8-byte prefix (same-entity block or division-by-zero guard).
fn interpolation_search<'a>(canonical: &'a [Datom], key: &EavtKey) -> Option<&'a Datom> {
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
    let eid = key.0;
    let b = eid.as_bytes();
    u64::from_be_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]])
}

// NOTE: permuted_binary_search was removed — replaced by
// crate::perm_layout::layout_search (INV-FERR-071, Eytzinger layout).

#[cfg(test)]
mod tests {
    use ferratom::{Attribute, Datom, EntityId, Op, TxId, Value};

    use super::{
        build_live_bitvector, live_positions_for_test,
        live_positions_from_sorted_run_keys_for_test, live_positions_kernel,
    };

    fn proof_entity_id(id: u8) -> EntityId {
        let mut bytes = [0u8; 32];
        bytes[31] = id;
        EntityId::from_bytes(bytes)
    }

    fn canonical_positions(bits: &bitvec::prelude::BitVec<u64, bitvec::prelude::Lsb0>) -> Vec<u32> {
        bits.iter_ones()
            .map(|position| u32::try_from(position).unwrap_or(u32::MAX))
            .collect()
    }

    #[test]
    fn test_inv_ferr_029_live_bitvector_matches_kernel_positions() {
        let entity = proof_entity_id(0x29);
        let attr = Attribute::from("a");
        let canonical = vec![
            Datom::new(
                entity,
                attr.clone(),
                Value::Long(1),
                TxId::new(1, 0, 0),
                Op::Assert,
            ),
            Datom::new(
                entity,
                attr.clone(),
                Value::Long(1),
                TxId::new(2, 0, 0),
                Op::Retract,
            ),
            Datom::new(
                entity,
                attr.clone(),
                Value::Long(2),
                TxId::new(1, 0, 0),
                Op::Assert,
            ),
            Datom::new(entity, attr, Value::Long(2), TxId::new(2, 0, 0), Op::Assert),
        ];

        assert_eq!(
            canonical_positions(&build_live_bitvector(&canonical)),
            live_positions_kernel(&canonical),
            "INV-FERR-029: bitvector LIVE representation must reflect kernel live positions"
        );
    }

    #[test]
    fn test_inv_ferr_029_live_bitvector_respects_triple_boundaries() {
        let attr = Attribute::from("a");
        let canonical = vec![
            Datom::new(
                proof_entity_id(0x31),
                attr.clone(),
                Value::Long(1),
                TxId::new(1, 0, 0),
                Op::Assert,
            ),
            Datom::new(
                proof_entity_id(0x31),
                attr.clone(),
                Value::Long(1),
                TxId::new(2, 0, 0),
                Op::Retract,
            ),
            Datom::new(
                proof_entity_id(0x32),
                attr.clone(),
                Value::Long(1),
                TxId::new(1, 0, 0),
                Op::Assert,
            ),
            Datom::new(
                proof_entity_id(0x32),
                attr,
                Value::Long(1),
                TxId::new(2, 0, 0),
                Op::Assert,
            ),
        ];

        assert_eq!(
            canonical_positions(&build_live_bitvector(&canonical)),
            live_positions_kernel(&canonical),
            "INV-FERR-029: bitvector LIVE representation must track each triple independently"
        );
    }

    #[test]
    fn test_inv_ferr_029_datom_wrapper_matches_group_key_kernel() {
        let attr = Attribute::from("a");
        let canonical = vec![
            Datom::new(
                proof_entity_id(0x41),
                attr.clone(),
                Value::Long(1),
                TxId::new(1, 0, 0),
                Op::Assert,
            ),
            Datom::new(
                proof_entity_id(0x41),
                attr.clone(),
                Value::Long(1),
                TxId::new(2, 0, 0),
                Op::Retract,
            ),
            Datom::new(
                proof_entity_id(0x41),
                attr.clone(),
                Value::Long(2),
                TxId::new(1, 0, 0),
                Op::Assert,
            ),
            Datom::new(
                proof_entity_id(0x41),
                attr,
                Value::Long(2),
                TxId::new(2, 0, 0),
                Op::Assert,
            ),
        ];
        let proof_entries = [
            ((0x41_u8, 0_u8, 1_i64), Op::Assert),
            ((0x41_u8, 0_u8, 1_i64), Op::Retract),
            ((0x41_u8, 0_u8, 2_i64), Op::Assert),
            ((0x41_u8, 0_u8, 2_i64), Op::Assert),
        ];

        assert_eq!(
            live_positions_for_test(&canonical),
            live_positions_from_sorted_run_keys_for_test(&proof_entries),
            "INV-FERR-029: datom LIVE wrapper must agree with the sorted-run kernel"
        );
        assert_eq!(
            live_positions_kernel(&canonical),
            live_positions_from_sorted_run_keys_for_test(&proof_entries),
            "INV-FERR-029: canonical datom grouping must preserve the proof-kernel result"
        );
    }

    /// INV-FERR-027: binary search fallback path produces correct results.
    ///
    /// Tests `first_datom_position_for_entity` which is the fallback when
    /// MPH build fails. Verifies it returns the same positions as
    /// `entity_lookup` (which uses the MPH path).
    #[test]
    fn test_inv_ferr_027_binary_search_fallback() {
        use std::sync::Arc;

        use super::PositionalStore;

        let attr = Attribute::from("db/doc");
        let datoms = vec![
            Datom::new(
                EntityId::from_content(b"a"),
                attr.clone(),
                Value::String(Arc::from("v1")),
                TxId::new(0, 0, 0),
                Op::Assert,
            ),
            Datom::new(
                EntityId::from_content(b"b"),
                attr.clone(),
                Value::String(Arc::from("v2")),
                TxId::new(0, 0, 0),
                Op::Assert,
            ),
            Datom::new(
                EntityId::from_content(b"c"),
                attr,
                Value::String(Arc::from("v3")),
                TxId::new(0, 0, 0),
                Op::Assert,
            ),
        ];
        let ps = PositionalStore::from_datoms(datoms.into_iter());

        // Test the binary search fallback directly.
        let eid_a = EntityId::from_content(b"a");
        let eid_b = EntityId::from_content(b"b");
        let eid_absent = EntityId::from_content(b"absent");

        let pos_a = ps.first_datom_position_for_entity(&eid_a);
        let pos_b = ps.first_datom_position_for_entity(&eid_b);
        let pos_absent = ps.first_datom_position_for_entity(&eid_absent);

        assert!(pos_a.is_some(), "entity a must be found by binary search");
        assert!(pos_b.is_some(), "entity b must be found by binary search");
        assert_eq!(pos_absent, None, "absent entity must return None");

        // Binary search must agree with MPH path.
        assert_eq!(pos_a, ps.entity_lookup(&eid_a));
        assert_eq!(pos_b, ps.entity_lookup(&eid_b));
        assert_eq!(pos_absent, ps.entity_lookup(&eid_absent));
    }
}
