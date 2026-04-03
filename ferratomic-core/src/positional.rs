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
use ferratom::{Datom, Op};

use crate::indexes::{AevtKey, AvetKey, EavtKey, VaetKey};

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

    /// EAVT lookup: O(log n) binary search on canonical array (INV-FERR-027).
    #[must_use]
    pub fn eavt_get(&self, key: &EavtKey) -> Option<&Datom> {
        self.canonical
            .binary_search_by(|d| EavtKey::from_datom(d).cmp(key))
            .ok()
            .map(|i| &self.canonical[i])
    }

    /// AEVT lookup: O(log n) binary search on permuted view (INV-FERR-027).
    ///
    /// Lazily builds the AEVT permutation on first access.
    #[must_use]
    pub fn aevt_get(&self, key: &AevtKey) -> Option<&Datom> {
        let perm = self
            .perm_aevt
            .get_or_init(|| build_permutation(&self.canonical, AevtKey::from_datom));
        permuted_binary_search(perm, &self.canonical, |d| AevtKey::from_datom(d).cmp(key))
    }

    /// VAET lookup: O(log n) binary search on permuted view (INV-FERR-027).
    ///
    /// Lazily builds the VAET permutation on first access.
    #[must_use]
    pub fn vaet_get(&self, key: &VaetKey) -> Option<&Datom> {
        let perm = self
            .perm_vaet
            .get_or_init(|| build_permutation(&self.canonical, VaetKey::from_datom));
        permuted_binary_search(perm, &self.canonical, |d| VaetKey::from_datom(d).cmp(key))
    }

    /// AVET lookup: O(log n) binary search on permuted view (INV-FERR-027).
    ///
    /// Lazily builds the AVET permutation on first access.
    #[must_use]
    pub fn avet_get(&self, key: &AvetKey) -> Option<&Datom> {
        let perm = self
            .perm_avet
            .get_or_init(|| build_permutation(&self.canonical, AvetKey::from_datom));
        permuted_binary_search(perm, &self.canonical, |d| AvetKey::from_datom(d).cmp(key))
    }

    /// Access the AEVT permutation array (INV-FERR-076).
    ///
    /// Lazily builds the permutation on first access.
    #[must_use]
    pub fn perm_aevt(&self) -> &[u32] {
        self.perm_aevt
            .get_or_init(|| build_permutation(&self.canonical, AevtKey::from_datom))
    }

    /// Access the VAET permutation array (INV-FERR-076).
    ///
    /// Lazily builds the permutation on first access.
    #[must_use]
    pub fn perm_vaet(&self) -> &[u32] {
        self.perm_vaet
            .get_or_init(|| build_permutation(&self.canonical, VaetKey::from_datom))
    }

    /// Access the AVET permutation array (INV-FERR-076).
    ///
    /// Lazily builds the permutation on first access.
    #[must_use]
    pub fn perm_avet(&self) -> &[u32] {
        self.perm_avet
            .get_or_init(|| build_permutation(&self.canonical, AvetKey::from_datom))
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
}

// ---------------------------------------------------------------------------
// Merge (INV-FERR-076 + INV-FERR-001)
// ---------------------------------------------------------------------------

/// CRDT merge via merge-sort on canonical arrays (INV-FERR-076).
///
/// INV-FERR-001: commutativity — `merge(a,b) = merge(b,a)` because
/// merge-sort of two sorted arrays is commutative on set semantics.
/// O(n + m) merge-sort + O(n) LIVE. Permutations are deferred (lazy).
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
    let n = canonical.len();
    let mut live = BitVec::repeat(false, n);
    let mut i = 0;
    while i < n {
        // Scan to end of this (entity, attribute, value) group.
        let entity = canonical[i].entity();
        let attribute = canonical[i].attribute();
        let value = canonical[i].value();
        let mut j = i + 1;
        while j < n
            && canonical[j].entity() == entity
            && *canonical[j].attribute() == *attribute
            && *canonical[j].value() == *value
        {
            j += 1;
        }
        // j-1 is the last datom in this (e,a,v) group (highest TxId).
        // Mark it live iff its Op is Assert.
        if canonical[j - 1].op() == Op::Assert {
            live.set(j - 1, true);
        }
        i = j;
    }
    live
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
fn merge_sort_dedup(a: &[Datom], b: &[Datom]) -> Vec<Datom> {
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
    // Drain remaining elements from whichever side isn't exhausted.
    for datom in &a[ia..] {
        result.push(datom.clone());
    }
    for datom in &b[ib..] {
        result.push(datom.clone());
    }
    result
}

// ---------------------------------------------------------------------------
// Internal: permuted binary search
// ---------------------------------------------------------------------------

/// Binary search on a permuted view of the canonical array.
///
/// The permutation array maps alternate-order positions to canonical
/// positions. The comparator operates on datoms at canonical positions.
fn permuted_binary_search<'a, F>(
    perm: &[u32],
    canonical: &'a [Datom],
    cmp_fn: F,
) -> Option<&'a Datom>
where
    F: Fn(&Datom) -> std::cmp::Ordering,
{
    perm.binary_search_by(|&pos| cmp_fn(&canonical[pos as usize]))
        .ok()
        .map(|i| &canonical[perm[i] as usize])
}
