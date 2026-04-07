//! Store sketch for O(delta) federation reconciliation (bd-wows).
//!
//! Two stores compute `MinHash` sketches of their datom content-hash sets.
//! Comparing sketches estimates the Jaccard similarity and symmetric
//! difference size, enabling O(delta)-bandwidth federation sync: only
//! datoms in the estimated difference need to be exchanged.
//!
//! ## Algorithm
//!
//! `MinHash`: for each datom, compute `h = content_hash(datom)` (BLAKE3,
//! 32 bytes). Keep the K smallest hashes (the "signature"). Two stores
//! exchange signatures (~8 KB each for K=256). Jaccard similarity =
//! |intersection of min-hashes| / |union of min-hashes|. Estimated
//! symmetric difference = `(count_a + count_b) * (1 - J) / (1 + J)`.
//! Integer arithmetic avoids floating-point precision issues.
//!
//! ## Phase roadmap
//!
//! Phase 4a uses `MinHash` (simpler, probabilistic). Phase 4b upgrades
//! to `PinSketch` (exact, information-theoretically optimal over GF(2^b)).

use std::collections::BinaryHeap;

use ferratom::{Datom, FerraError};

/// Default signature size (number of min-hashes retained).
///
/// K=256 gives ~8 KB signatures and sufficient precision for
/// federation diff estimation with stores up to ~10M datoms.
pub const DEFAULT_CAPACITY: usize = 256;

/// `MinHash` sketch for O(delta) federation reconciliation.
///
/// Stores the K smallest content hashes from a datom set, enabling
/// probabilistic estimation of Jaccard similarity and symmetric
/// difference between two stores without exchanging the full sets.
///
/// INV-FERR-001..003: sketch comparison is consistent with CRDT
/// merge semantics -- identical stores produce identical sketches,
/// and the estimated diff converges to 0 as stores converge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreSketch {
    /// K smallest content hashes (sorted ascending). Sorted order is
    /// maintained as an invariant for O(K) intersection computation.
    min_hashes: Vec<[u8; 32]>,
    /// Total datom count in the source store (for set size estimation).
    count: usize,
    /// Signature capacity (K). Both sketches must use the same K for
    /// meaningful comparison.
    capacity: usize,
}

// ---------------------------------------------------------------------------
// Internal: sorted-array set operations
// ---------------------------------------------------------------------------

/// Result of comparing two sorted `[u8; 32]` arrays in lockstep.
struct SetCounts {
    /// Number of elements present in both arrays.
    intersection: u32,
    /// Number of distinct elements across both arrays.
    union: u32,
}

/// Walk two sorted `[u8; 32]` slices and count intersection / union.
///
/// Both slices must be sorted ascending (an invariant of `StoreSketch`).
/// Returns counts as `u32` since both slices are bounded by sketch
/// capacity (at most `DEFAULT_CAPACITY` = 256 elements each).
fn sorted_set_counts(a: &[[u8; 32]], b: &[[u8; 32]]) -> SetCounts {
    let (mut i, mut j) = (0, 0);
    let mut intersection = 0_u32;
    let mut union = 0_u32;

    while i < a.len() && j < b.len() {
        union += 1;
        match a[i].cmp(&b[j]) {
            std::cmp::Ordering::Equal => {
                intersection += 1;
                i += 1;
                j += 1;
            }
            std::cmp::Ordering::Less => {
                i += 1;
            }
            std::cmp::Ordering::Greater => {
                j += 1;
            }
        }
    }
    // Remaining elements from whichever side is not exhausted.
    // Both (a.len() - i) and (b.len() - j) are <= capacity, which
    // is bounded by sketch construction. Sum fits in u32 for any
    // realistic capacity.
    let remaining = a.len() - i + b.len() - j;
    // Saturate rather than panic for defensive correctness.
    union += u32::try_from(remaining).unwrap_or(u32::MAX);

    SetCounts {
        intersection,
        union,
    }
}

impl StoreSketch {
    /// Compute a `MinHash` sketch from a datom iterator. O(N log K).
    ///
    /// `capacity` is the signature size K. Larger K gives more precise
    /// estimates at the cost of larger signatures. `DEFAULT_CAPACITY`
    /// (256) is recommended for most use cases.
    ///
    /// # Errors
    ///
    /// Returns `FerraError::InvariantViolation` if `capacity` is 0.
    pub fn compute<'a>(
        datoms: impl Iterator<Item = &'a Datom>,
        capacity: usize,
    ) -> Result<Self, FerraError> {
        if capacity == 0 {
            return Err(FerraError::InvariantViolation {
                invariant: "bd-wows".into(),
                details: "sketch capacity must be > 0".into(),
            });
        }

        // Use a max-heap to track the K smallest hashes efficiently.
        // We push each hash and pop the largest when we exceed capacity,
        // so the heap always contains the K smallest seen so far.
        let mut heap: BinaryHeap<[u8; 32]> = BinaryHeap::with_capacity(capacity + 1);
        let mut count: usize = 0;

        for datom in datoms {
            count += 1;
            let hash = datom.content_hash();
            if heap.len() < capacity {
                heap.push(hash);
            } else if let Some(max) = heap.peek() {
                if hash < *max {
                    heap.pop();
                    heap.push(hash);
                }
            }
        }

        // Drain heap into a sorted Vec for O(K) intersection later.
        let mut min_hashes: Vec<[u8; 32]> = heap.into_vec();
        min_hashes.sort_unstable();

        Ok(Self {
            min_hashes,
            count,
            capacity,
        })
    }

    /// Estimate Jaccard similarity with another sketch.
    ///
    /// Both sketches must have been computed with the same capacity K.
    ///
    /// # Errors
    ///
    /// Returns `FerraError::InvariantViolation` if capacities differ.
    ///
    /// `J = |intersection(sig_a, sig_b)| / |union(sig_a, sig_b)|`
    ///
    /// For sketches with fewer than K elements (small stores), we use
    /// the actual element counts rather than K as the denominator.
    pub fn jaccard_similarity(&self, other: &Self) -> Result<f64, FerraError> {
        if self.capacity != other.capacity {
            return Err(FerraError::InvariantViolation {
                invariant: "bd-wows".into(),
                details: format!(
                    "sketch capacity mismatch: {} vs {}",
                    self.capacity, other.capacity
                ),
            });
        }

        // Empty sketches: both empty => identical (J=1), one empty => disjoint (J=0).
        if self.min_hashes.is_empty() && other.min_hashes.is_empty() {
            return Ok(1.0);
        }
        if self.min_hashes.is_empty() || other.min_hashes.is_empty() {
            return Ok(0.0);
        }

        let counts = sorted_set_counts(&self.min_hashes, &other.min_hashes);

        if counts.union == 0 {
            return Ok(1.0);
        }

        // Both intersection and union are u32 (bounded by 2*capacity).
        // f64::from(u32) is lossless -- no clippy pedantic issues.
        Ok(f64::from(counts.intersection) / f64::from(counts.union))
    }

    /// Estimate the symmetric difference size between two stores.
    ///
    /// Uses integer arithmetic to avoid floating-point precision issues:
    /// `diff = (count_a + count_b) * (1 - J) / (1 + J)`
    ///
    /// In integer form: `total * (union - intersection) / (union + intersection)`.
    /// This is the standard `MinHash`-based symmetric difference estimator.
    ///
    /// For identical stores (J=1): returns 0. For completely disjoint
    /// stores (J=0): returns `count_a + count_b` (the full symmetric diff).
    ///
    /// # Errors
    ///
    /// Returns `FerraError::InvariantViolation` if capacities differ.
    pub fn estimated_diff_size(&self, other: &Self) -> Result<usize, FerraError> {
        if self.capacity != other.capacity {
            return Err(FerraError::InvariantViolation {
                invariant: "bd-wows".into(),
                details: format!(
                    "sketch capacity mismatch: {} vs {}",
                    self.capacity, other.capacity
                ),
            });
        }

        // Both empty => identical => zero diff.
        if self.min_hashes.is_empty() && other.min_hashes.is_empty() {
            return Ok(0);
        }
        // One empty, one not => fully disjoint => diff = other's count.
        if self.min_hashes.is_empty() {
            return Ok(other.count);
        }
        if other.min_hashes.is_empty() {
            return Ok(self.count);
        }

        let counts = sorted_set_counts(&self.min_hashes, &other.min_hashes);

        // Identical sketches => zero diff.
        if counts.intersection == counts.union {
            return Ok(0);
        }

        if counts.union == 0 {
            return Ok(0);
        }

        // Integer arithmetic: diff = total * (1 - J) / (1 + J)
        //   = total * (union - intersection) / (union + intersection)
        //
        // Use u128 to avoid overflow for large stores.
        let count_a = self.count as u64;
        let count_b = other.count as u64;
        let total = u128::from(count_a) + u128::from(count_b);
        let diff_numerator = u128::from(counts.union - counts.intersection);
        let denominator = u128::from(counts.union) + u128::from(counts.intersection);

        if denominator == 0 {
            return Ok(0);
        }

        // Integer division with rounding: (a + b/2) / b
        let result = (total * diff_numerator + denominator / 2) / denominator;

        Ok(usize::try_from(result).unwrap_or(usize::MAX))
    }

    /// Merge two sketches (for distributed sketch computation).
    ///
    /// The merged sketch contains the K smallest hashes from the union
    /// of both input signatures, with `count = count_a + count_b`.
    /// This enables computing a sketch incrementally across partitions.
    ///
    /// Both sketches must have been computed with the same capacity K.
    ///
    /// # Errors
    ///
    /// Returns `FerraError::InvariantViolation` if capacities differ.
    pub fn merge(&self, other: &Self) -> Result<Self, FerraError> {
        if self.capacity != other.capacity {
            return Err(FerraError::InvariantViolation {
                invariant: "bd-wows".into(),
                details: format!(
                    "sketch capacity mismatch: {} vs {}",
                    self.capacity, other.capacity
                ),
            });
        }

        // Merge-sort both sorted arrays, deduplicate, take first K.
        let mut merged = Vec::with_capacity(self.min_hashes.len() + other.min_hashes.len());
        let (mut i, mut j) = (0, 0);
        let a = &self.min_hashes;
        let b = &other.min_hashes;

        while i < a.len() && j < b.len() {
            match a[i].cmp(&b[j]) {
                std::cmp::Ordering::Equal => {
                    merged.push(a[i]);
                    i += 1;
                    j += 1;
                }
                std::cmp::Ordering::Less => {
                    merged.push(a[i]);
                    i += 1;
                }
                std::cmp::Ordering::Greater => {
                    merged.push(b[j]);
                    j += 1;
                }
            }
        }
        // Append remaining from whichever side is not exhausted.
        if i < a.len() {
            merged.extend_from_slice(&a[i..]);
        }
        if j < b.len() {
            merged.extend_from_slice(&b[j..]);
        }

        // Truncate to capacity (take K smallest).
        merged.truncate(self.capacity);

        Ok(Self {
            min_hashes: merged,
            count: self.count + other.count,
            capacity: self.capacity,
        })
    }

    /// The min-hash signature (K smallest content hashes, sorted ascending).
    #[must_use]
    pub fn min_hashes(&self) -> &[[u8; 32]] {
        &self.min_hashes
    }

    /// Total datom count in the source store.
    #[must_use]
    pub fn count(&self) -> usize {
        self.count
    }

    /// Signature capacity K.
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.capacity
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use ferratom::{Attribute, EntityId, Op, TxId, Value};

    use super::*;

    /// Build a datom with a unique entity derived from `seed`.
    fn make_datom(seed: &str) -> Datom {
        Datom::new(
            EntityId::from_content(seed.as_bytes()),
            Attribute::from("test/attr"),
            Value::String(Arc::from(seed)),
            TxId::new(1, 0, 0),
            Op::Assert,
        )
    }

    /// Build a Vec of N distinct datoms.
    fn make_datoms(n: usize) -> Vec<Datom> {
        (0..n).map(|i| make_datom(&format!("datom-{i}"))).collect()
    }

    // -- Test 1: Empty store -> empty sketch ----------------------------------

    #[test]
    fn test_empty_store_produces_empty_sketch() {
        let empty: Vec<Datom> = Vec::new();
        let sketch = StoreSketch::compute(empty.iter(), DEFAULT_CAPACITY)
            .expect("compute must succeed for empty input");
        assert_eq!(sketch.min_hashes().len(), 0);
        assert_eq!(sketch.count(), 0);
    }

    // -- Test 2: Identical stores -> jaccard = 1.0, diff = 0 ------------------

    #[test]
    fn test_identical_stores_jaccard_one_diff_zero() {
        let datoms = make_datoms(100);
        let s1 =
            StoreSketch::compute(datoms.iter(), DEFAULT_CAPACITY).expect("compute must succeed");
        let s2 =
            StoreSketch::compute(datoms.iter(), DEFAULT_CAPACITY).expect("compute must succeed");

        let j = s1.jaccard_similarity(&s2).expect("jaccard must succeed");
        assert!(
            (j - 1.0).abs() < f64::EPSILON,
            "identical stores must have Jaccard = 1.0, got {j}"
        );

        let diff = s1.estimated_diff_size(&s2).expect("diff must succeed");
        assert_eq!(diff, 0, "identical stores must have diff = 0");
    }

    // -- Test 3: Disjoint stores -> jaccard near 0.0 --------------------------

    #[test]
    fn test_disjoint_stores_jaccard_near_zero() {
        // Two completely disjoint sets of 500 datoms each.
        let set_a: Vec<Datom> = (0..500)
            .map(|i| make_datom(&format!("alpha-{i}")))
            .collect();
        let set_b: Vec<Datom> = (0..500)
            .map(|i| make_datom(&format!("bravo-{i}")))
            .collect();

        let sa =
            StoreSketch::compute(set_a.iter(), DEFAULT_CAPACITY).expect("compute must succeed");
        let sb =
            StoreSketch::compute(set_b.iter(), DEFAULT_CAPACITY).expect("compute must succeed");

        let j = sa.jaccard_similarity(&sb).expect("jaccard must succeed");
        // With 500 datoms per set and K=256, disjoint sets should have
        // near-zero Jaccard (very unlikely for 256 of 500 random hashes
        // to collide). We allow a generous threshold.
        assert!(
            j < 0.05,
            "disjoint stores must have Jaccard near 0.0, got {j}"
        );
    }

    // -- Test 4: Sketch merge is commutative ----------------------------------

    #[test]
    fn test_sketch_merge_commutative() {
        let set_a = make_datoms(50);
        let set_b: Vec<Datom> = (50..100)
            .map(|i| make_datom(&format!("datom-{i}")))
            .collect();

        let sa = StoreSketch::compute(set_a.iter(), 32).expect("compute must succeed");
        let sb = StoreSketch::compute(set_b.iter(), 32).expect("compute must succeed");

        let ab = sa.merge(&sb).expect("merge must succeed");
        let ba = sb.merge(&sa).expect("merge must succeed");

        assert_eq!(
            ab.min_hashes(),
            ba.min_hashes(),
            "sketch merge must be commutative: min_hashes must be identical"
        );
        assert_eq!(
            ab.count(),
            ba.count(),
            "sketch merge must be commutative: counts must be identical"
        );
    }

    // -- Test 5: Diff estimate is reasonable ----------------------------------

    #[test]
    fn test_diff_estimate_reasonable() {
        // Two stores with 200 datoms each, sharing 100.
        let shared: Vec<Datom> = (0..100)
            .map(|i| make_datom(&format!("shared-{i}")))
            .collect();
        let only_a: Vec<Datom> = (0..100)
            .map(|i| make_datom(&format!("only-a-{i}")))
            .collect();
        let only_b: Vec<Datom> = (0..100)
            .map(|i| make_datom(&format!("only-b-{i}")))
            .collect();

        let store_a: Vec<Datom> = shared.iter().chain(only_a.iter()).cloned().collect();
        let store_b: Vec<Datom> = shared.iter().chain(only_b.iter()).cloned().collect();

        let sa =
            StoreSketch::compute(store_a.iter(), DEFAULT_CAPACITY).expect("compute must succeed");
        let sb =
            StoreSketch::compute(store_b.iter(), DEFAULT_CAPACITY).expect("compute must succeed");

        let diff = sa.estimated_diff_size(&sb).expect("diff must succeed");
        // Actual symmetric difference = 200 (100 only-a + 100 only-b).
        // We allow the estimate to be within 2x of the actual.
        let actual_diff = 200_usize;
        assert!(
            diff <= actual_diff * 2 && diff >= actual_diff / 3,
            "diff estimate {diff} should be within reasonable range of actual {actual_diff}"
        );
    }

    // -- Edge cases -----------------------------------------------------------

    #[test]
    fn test_zero_capacity_returns_error() {
        let datoms = make_datoms(10);
        let result = StoreSketch::compute(datoms.iter(), 0);
        assert!(result.is_err(), "capacity 0 must return error");
    }

    #[test]
    fn test_capacity_mismatch_returns_error() {
        let datoms = make_datoms(10);
        let s1 = StoreSketch::compute(datoms.iter(), 8).expect("compute must succeed");
        let s2 = StoreSketch::compute(datoms.iter(), 16).expect("compute must succeed");

        assert!(
            s1.jaccard_similarity(&s2).is_err(),
            "capacity mismatch must return error from jaccard"
        );
        assert!(
            s1.estimated_diff_size(&s2).is_err(),
            "capacity mismatch must return error from diff"
        );
        assert!(
            s1.merge(&s2).is_err(),
            "capacity mismatch must return error from merge"
        );
    }

    #[test]
    fn test_store_smaller_than_capacity() {
        // 5 datoms with K=256 -- sketch should contain all 5 hashes.
        let datoms = make_datoms(5);
        let sketch =
            StoreSketch::compute(datoms.iter(), DEFAULT_CAPACITY).expect("compute must succeed");
        assert_eq!(sketch.min_hashes().len(), 5);
        assert_eq!(sketch.count(), 5);
    }

    #[test]
    fn test_empty_sketches_are_identical() {
        let empty: Vec<Datom> = Vec::new();
        let s1 =
            StoreSketch::compute(empty.iter(), DEFAULT_CAPACITY).expect("compute must succeed");
        let s2 =
            StoreSketch::compute(empty.iter(), DEFAULT_CAPACITY).expect("compute must succeed");

        let j = s1.jaccard_similarity(&s2).expect("jaccard must succeed");
        assert!(
            (j - 1.0).abs() < f64::EPSILON,
            "two empty sketches should be identical (J=1.0), got {j}"
        );

        let diff = s1.estimated_diff_size(&s2).expect("diff must succeed");
        assert_eq!(diff, 0, "two empty sketches should have diff = 0");
    }

    #[test]
    fn test_merge_preserves_capacity() {
        let set_a = make_datoms(20);
        let set_b: Vec<Datom> = (20..40)
            .map(|i| make_datom(&format!("datom-{i}")))
            .collect();

        let sa = StoreSketch::compute(set_a.iter(), 8).expect("compute must succeed");
        let sb = StoreSketch::compute(set_b.iter(), 8).expect("compute must succeed");

        let merged = sa.merge(&sb).expect("merge must succeed");
        assert!(
            merged.min_hashes().len() <= 8,
            "merged sketch must not exceed capacity K=8"
        );
        assert_eq!(merged.capacity(), 8);
        assert_eq!(merged.count(), 40, "merged count = 20 + 20");
    }

    #[test]
    fn test_single_datom_sketch() {
        let datoms = make_datoms(1);
        let s1 =
            StoreSketch::compute(datoms.iter(), DEFAULT_CAPACITY).expect("compute must succeed");
        let s2 =
            StoreSketch::compute(datoms.iter(), DEFAULT_CAPACITY).expect("compute must succeed");

        assert_eq!(s1.min_hashes().len(), 1);
        assert_eq!(s1, s2, "identical single-datom sketches must be equal");

        let j = s1.jaccard_similarity(&s2).expect("jaccard must succeed");
        assert!(
            (j - 1.0).abs() < f64::EPSILON,
            "identical single-datom sketches must have J=1.0"
        );
    }
}
