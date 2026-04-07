//! CRDT merge and merge-sort for positional stores (INV-FERR-076 + INV-FERR-001).

use std::sync::OnceLock;

use ferratom::Datom;

use crate::{fingerprint::compute_fingerprint, live::build_live_bitvector, store::PositionalStore};

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
    let merged = merge_sort_dedup(a.datoms(), b.datoms());
    // Parallel O(n) passes (bd-a7s1).
    let (live_bits, fingerprint) = rayon::join(
        || build_live_bitvector(&merged),
        || compute_fingerprint(&merged),
    );
    PositionalStore {
        canonical: merged,
        live_bits,
        perm_aevt: OnceLock::new(),
        perm_vaet: OnceLock::new(),
        perm_avet: OnceLock::new(),
        fingerprint,
        mph: OnceLock::new(),
        bloom: OnceLock::new(),
    }
}

/// Merge two sorted, deduplicated slices into a single sorted, deduplicated
/// Vec (INV-FERR-001: set union via merge-sort).
///
/// O(n + m) time, sequential access on both inputs.
///
/// **Coupling (DEFECT-017)**: Both inputs MUST be sorted by `Datom::Ord`,
/// which is EAVT order (derived from struct field declaration order).
/// `PositionalStore::datoms()` and `OrdSet::iter()` both yield this order.
/// See `Datom` doc comment for the field-order invariant.
///
/// `pub` because `ferratomic-db` and `ferratomic-checkpoint` use this
/// for mixed-repr merge and LIVE-first deserialization.
#[must_use]
pub fn merge_sort_dedup(a: &[Datom], b: &[Datom]) -> Vec<Datom> {
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
