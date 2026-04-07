//! [`PositionalStore`] — contiguous-array representation of the datom set.
//!
//! Replaces `OrdSet<Datom>` + 4x`OrdMap` with a sorted `Vec<Datom>` plus
//! lazy permutation arrays, LIVE bitvector, XOR fingerprint, Bloom filter,
//! and CHD perfect hash. Memory: ~26 MB at 200K datoms vs ~159 MB with
//! `im::OrdMap`. Construction via `from_datoms` (O(n log n) sort) or
//! `from_sorted_canonical` (O(n) for pre-sorted merge results).

use std::sync::OnceLock;

use bitvec::prelude::{BitVec, Lsb0};
use ferratom::Datom;
use ferratomic_index::{AevtKey, AvetKey, EavtKey, VaetKey};

use crate::{
    bloom::EntityBloom,
    fingerprint::compute_fingerprint,
    live::build_live_bitvector,
    mph::Mph,
    perm::{build_permutation, layout_permutation, layout_search, layout_to_sorted},
    search::interpolation_search,
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
/// - `fingerprint`: XOR of per-datom BLAKE3 content hashes (INV-FERR-074).
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
    pub(crate) canonical: Vec<Datom>,
    /// LIVE bitvector: `live_bits[p] = true` iff the datom at position
    /// p is the latest Assert for its `(entity, attribute, value)` triple
    /// (INV-FERR-029). 1 bit per datom -- 25 KB at 200K datoms.
    pub(crate) live_bits: BitVec<u64, Lsb0>,
    /// Permutation: AEVT-order index -> canonical position (INV-FERR-005).
    /// Lazily built on first access via `OnceLock`.
    pub(crate) perm_aevt: OnceLock<Vec<u32>>,
    /// Permutation: VAET-order index -> canonical position (INV-FERR-005).
    /// Lazily built on first access via `OnceLock`.
    pub(crate) perm_vaet: OnceLock<Vec<u32>>,
    /// Permutation: AVET-order index -> canonical position (INV-FERR-005).
    /// Lazily built on first access via `OnceLock`.
    pub(crate) perm_avet: OnceLock<Vec<u32>>,
    /// XOR homomorphic fingerprint: `H(S) = XOR_{d in S} content_hash(d)` (INV-FERR-074).
    pub(crate) fingerprint: [u8; 32],
    /// CHD perfect hash for O(1) entity existence checks (INV-FERR-076, contributes to INV-FERR-027).
    /// Lazily built on first `entity_lookup()` call via `OnceLock`.
    /// `None` if build fails (fallback to binary search).
    pub(crate) mph: OnceLock<Option<Mph>>,
    /// Bloom filter for O(1) probabilistic negative entity lookups (bd-218b).
    /// Lazily built on first `entity_exists()` call. ~1% false positive rate
    /// at 10 bits/element. Zero false negatives by construction.
    pub(crate) bloom: OnceLock<EntityBloom>,
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
            bloom: self.bloom.get().map_or_else(OnceLock::new, |v| {
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
            .field("bloom_init", &self.bloom.get().is_some())
            .finish()
    }
}

impl PositionalStore {
    /// Build from an unsorted datom iterator (INV-FERR-076).
    ///
    /// O(n log n) for sort + O(n) for LIVE scan + O(n) for fingerprint.
    /// After sort completes (needs `&mut`), the two O(n) passes run in
    /// parallel via `rayon::join` (bd-a7s1). Permutation arrays are
    /// deferred to first access via `OnceLock` (lazy construction).
    /// Uses `sort_unstable` -- O(1) auxiliary memory, matching the
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

        // After sort, canonical is immutable. Parallel O(n) passes (bd-a7s1).
        let (live_bits, fingerprint) = rayon::join(
            || build_live_bitvector(&canonical),
            || compute_fingerprint(&canonical),
        );

        Self {
            canonical,
            live_bits,
            perm_aevt: OnceLock::new(),
            perm_vaet: OnceLock::new(),
            perm_avet: OnceLock::new(),
            fingerprint,
            mph: OnceLock::new(),
            bloom: OnceLock::new(),
        }
    }

    /// Construct from a pre-sorted, deduplicated datom vector.
    ///
    /// INV-FERR-076: the caller guarantees `canonical` is EAVT-sorted and
    /// duplicate-free. Checked via `debug_assert` only -- release builds do
    /// not validate. Callers loading from untrusted sources must verify
    /// integrity independently (e.g., BLAKE3 per ADR-FERR-010).
    /// This is the O(n) construction path for merge results produced by
    /// `merge_sort_dedup`, which outputs sorted, deduplicated data.
    /// Skips the O(n log n) `sort_unstable()` call in `from_datoms`.
    #[must_use]
    pub fn from_sorted_canonical(canonical: Vec<Datom>) -> Self {
        debug_assert!(
            canonical.windows(2).all(|w| w[0] < w[1]),
            "INV-FERR-076: from_sorted_canonical requires strictly sorted input"
        );
        debug_assert!(
            u32::try_from(canonical.len()).is_ok(),
            "INV-FERR-076: canonical array exceeds u32 position space"
        );

        // Parallel O(n) passes (bd-a7s1).
        let (live_bits, fingerprint) = rayon::join(
            || build_live_bitvector(&canonical),
            || compute_fingerprint(&canonical),
        );

        Self {
            canonical,
            live_bits,
            perm_aevt: OnceLock::new(),
            perm_vaet: OnceLock::new(),
            perm_avet: OnceLock::new(),
            fingerprint,
            mph: OnceLock::new(),
            bloom: OnceLock::new(),
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
    /// Returns datoms where `live_bits[p] = true` -- the latest Assert
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
            layout_permutation(&sorted)
        });
        layout_search(perm, &self.canonical, |d| key.cmp_datom(d).reverse())
    }

    /// VAET lookup: O(log n) cache-oblivious search on Eytzinger layout (INV-FERR-027, INV-FERR-071).
    ///
    /// Lazily builds the VAET permutation in Eytzinger (BFS) order on first access.
    #[must_use]
    pub fn vaet_get(&self, key: &VaetKey) -> Option<&Datom> {
        let perm = self.perm_vaet.get_or_init(|| {
            let sorted = build_permutation(&self.canonical, VaetKey::from_datom);
            layout_permutation(&sorted)
        });
        layout_search(perm, &self.canonical, |d| key.cmp_datom(d).reverse())
    }

    /// AVET lookup: O(log n) cache-oblivious search on Eytzinger layout (INV-FERR-027, INV-FERR-071).
    ///
    /// Lazily builds the AVET permutation in Eytzinger (BFS) order on first access.
    #[must_use]
    pub fn avet_get(&self, key: &AvetKey) -> Option<&Datom> {
        let perm = self.perm_avet.get_or_init(|| {
            let sorted = build_permutation(&self.canonical, AvetKey::from_datom);
            layout_permutation(&sorted)
        });
        layout_search(perm, &self.canonical, |d| key.cmp_datom(d).reverse())
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

    /// XOR homomorphic store fingerprint (INV-FERR-074).
    ///
    /// `H(S) = XOR_{d in S} content_hash(d)`. Commutative and homomorphic
    /// over disjoint union: `H(A | B) = H(A) ^ H(B)` when `A & B = {}`.
    /// Empty stores have `[0; 32]` (XOR identity).
    #[must_use]
    pub fn fingerprint(&self) -> &[u8; 32] {
        &self.fingerprint
    }

    /// Clone the LIVE bitvector for checkpoint serialization (INV-FERR-076).
    ///
    /// V3 checkpoints persist the bitvector to skip recomputation on load.
    #[must_use]
    pub fn live_bits_clone(&self) -> BitVec<u64, Lsb0> {
        self.live_bits.clone()
    }

    /// Build from pre-sorted datoms and a pre-computed LIVE bitvector.
    ///
    /// INV-FERR-076: Zero-construction cold start for V3 checkpoint
    /// deserialization. The caller guarantees `canonical` is sorted and
    /// deduplicated (strictly increasing EAVT order, no duplicate datoms)
    /// and that `live_bits.len() == canonical.len()`. Runtime-validated
    /// in all builds (debug and release). Permutation arrays are deferred
    /// (`OnceLock::new()`).
    ///
    /// # Errors
    ///
    /// Returns `FerraError::InvariantViolation` (INV-FERR-076) if:
    /// - `live_bits.len() != canonical.len()`
    /// - `canonical` is not strictly sorted (EAVT order, no duplicates)
    /// - `canonical.len()` exceeds `u32::MAX` (position space overflow)
    pub fn from_sorted_with_live(
        canonical: Vec<Datom>,
        live_bits: BitVec<u64, Lsb0>,
    ) -> Result<Self, ferratom::FerraError> {
        if live_bits.len() != canonical.len() {
            return Err(ferratom::FerraError::InvariantViolation {
                invariant: "INV-FERR-076".to_string(),
                details: format!(
                    "live_bits length ({}) != canonical length ({})",
                    live_bits.len(),
                    canonical.len()
                ),
            });
        }
        if !canonical.windows(2).all(|w| w[0] < w[1]) {
            return Err(ferratom::FerraError::InvariantViolation {
                invariant: "INV-FERR-076".to_string(),
                details: "canonical datoms not strictly sorted (EAVT order, no duplicates)"
                    .to_string(),
            });
        }
        if u32::try_from(canonical.len()).is_err() {
            return Err(ferratom::FerraError::InvariantViolation {
                invariant: "INV-FERR-076".to_string(),
                details: "canonical array exceeds u32 position space".to_string(),
            });
        }
        // Single O(n) pass -- no rayon::join because live_bits is pre-computed.
        // The fingerprint computation is the only O(n) work here.
        let fingerprint = compute_fingerprint(&canonical);
        Ok(Self {
            canonical,
            live_bits,
            perm_aevt: OnceLock::new(),
            perm_vaet: OnceLock::new(),
            perm_avet: OnceLock::new(),
            fingerprint,
            mph: OnceLock::new(),
            bloom: OnceLock::new(),
        })
    }
}
