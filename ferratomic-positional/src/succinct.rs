//! Succinct bitvector with O(1) rank and O(log n) select (ADR-FERR-030
//! prerequisite).
//!
//! Wraps `BitVec<u64, Lsb0>` with a prefix-sum table for O(1) `rank1(i)`:
//! count of 1-bits in `[0, i)`. `select1(k)` uses binary search on the
//! prefix sums: O(log(n/64)).
//!
//! Space overhead: one `u64` per 64-bit word = 12.5% of the bitvector.
//! This is cheaper than Rank9 (25%) and sufficient for Phase 4a.

use bitvec::prelude::{BitVec, Lsb0};

/// Succinct bitvector with precomputed rank tables (ADR-FERR-030).
///
/// Supports O(1) `rank1` (count of set bits before a position) and
/// O(log n) `select1` (position of the k-th set bit). Built from a
/// `BitVec<u64, Lsb0>` in O(n/64) time.
#[derive(Clone, Debug)]
pub struct SuccinctBitVec {
    /// Raw bit storage.
    bits: BitVec<u64, Lsb0>,
    /// `prefix[i]` = cumulative popcount of words `0..i`.
    /// `prefix[0] = 0`, `prefix[w] = total 1-bits` where `w = words`.
    prefix: Vec<u64>,
    /// Total 1-bits (cached = `prefix[words]`).
    ones_count: usize,
}

impl SuccinctBitVec {
    /// Build rank tables from a bitvector. O(n/64).
    #[must_use]
    pub fn from_bitvec(bits: BitVec<u64, Lsb0>) -> Self {
        let raw = bits.as_raw_slice();
        let words = raw.len();
        let mut prefix = Vec::with_capacity(words + 1);
        prefix.push(0u64);
        let mut cumulative = 0u64;
        for &word in raw {
            cumulative = cumulative.saturating_add(u64::from(word.count_ones()));
            prefix.push(cumulative);
        }
        let ones_count = usize::try_from(cumulative).unwrap_or(usize::MAX);
        Self {
            bits,
            prefix,
            ones_count,
        }
    }

    /// Count of 1-bits in `[0, pos)`. O(1): one prefix lookup + one
    /// popcount of a partial word.
    ///
    /// Returns 0 if `pos == 0`. Returns `ones_count` if `pos >= len`.
    #[inline]
    #[must_use]
    pub fn rank1(&self, pos: usize) -> usize {
        if pos == 0 {
            return 0;
        }
        let len = self.bits.len();
        if pos >= len {
            return self.ones_count;
        }
        let word_idx = pos / 64;
        let bit_idx = pos % 64;
        // Cumulative count up to word_idx.
        let base = usize::try_from(self.prefix[word_idx]).unwrap_or(0);
        // Partial word: count bits in [0, bit_idx) of word_idx.
        let raw = self.bits.as_raw_slice();
        let partial = if bit_idx == 0 {
            0
        } else {
            (raw[word_idx] & ((1u64 << bit_idx) - 1)).count_ones() as usize
        };
        base + partial
    }

    /// Position of the k-th 1-bit (0-indexed). O(log(n/64)):
    /// binary search on prefix sums + scan within word.
    ///
    /// Returns `None` if `k >= ones_count`.
    #[must_use]
    pub fn select1(&self, k: usize) -> Option<usize> {
        if k >= self.ones_count {
            return None;
        }
        let target = k as u64 + 1; // prefix stores cumulative, we need >= target
                                   // Binary search: find first word_idx where prefix[word_idx+1] >= target.
        let words = self.prefix.len() - 1;
        let mut lo = 0usize;
        let mut hi = words;
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            if self.prefix[mid + 1] < target {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        // lo is the word containing the k-th 1-bit.
        let bits_before = usize::try_from(self.prefix[lo]).unwrap_or(0);
        let remaining = k - bits_before;
        // Find the (remaining)-th 1-bit within word lo.
        let raw = self.bits.as_raw_slice();
        let word = raw[lo];
        select_in_word(word, remaining).map(|bit| lo * 64 + bit)
    }

    /// Direct bit access (unchanged from `BitVec`).
    #[inline]
    #[must_use]
    pub fn get(&self, pos: usize) -> bool {
        self.bits.get(pos).as_deref() == Some(&true)
    }

    /// Total number of bits.
    #[must_use]
    pub fn len(&self) -> usize {
        self.bits.len()
    }

    /// Whether the bitvector is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.bits.is_empty()
    }

    /// Total number of 1-bits.
    #[must_use]
    pub fn ones_count(&self) -> usize {
        self.ones_count
    }

    /// Borrow the underlying `BitVec`.
    #[must_use]
    pub fn as_bitvec(&self) -> &BitVec<u64, Lsb0> {
        &self.bits
    }
}

/// Find the k-th (0-indexed) set bit within a single u64 word.
///
/// Uses iterative `tzcnt` (trailing zeros count) to skip to set bits.
/// Average case: O(k) with hardware `tzcnt`.
fn select_in_word(mut word: u64, k: usize) -> Option<usize> {
    for _ in 0..k {
        if word == 0 {
            return None;
        }
        word &= word - 1; // clear lowest set bit
    }
    if word == 0 {
        return None;
    }
    Some(word.trailing_zeros() as usize)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_bitvec(bits: &[bool]) -> BitVec<u64, Lsb0> {
        bits.iter().collect()
    }

    #[test]
    fn test_rank1_empty() {
        let sbv = SuccinctBitVec::from_bitvec(BitVec::<u64, Lsb0>::new());
        assert_eq!(sbv.rank1(0), 0);
        assert_eq!(sbv.ones_count(), 0);
    }

    #[test]
    fn test_rank1_all_ones() {
        let bits = make_bitvec(&[true; 128]);
        let sbv = SuccinctBitVec::from_bitvec(bits);
        assert_eq!(sbv.rank1(0), 0, "rank1(0) = 0 by definition");
        assert_eq!(sbv.rank1(1), 1);
        assert_eq!(sbv.rank1(64), 64);
        assert_eq!(sbv.rank1(128), 128);
        assert_eq!(sbv.ones_count(), 128);
    }

    #[test]
    fn test_rank1_alternating() {
        // [true, false, true, false, ...] for 128 bits
        let bits: Vec<bool> = (0..128).map(|i| i % 2 == 0).collect();
        let sbv = SuccinctBitVec::from_bitvec(make_bitvec(&bits));
        assert_eq!(sbv.rank1(0), 0);
        assert_eq!(sbv.rank1(1), 1); // bit 0 is set
        assert_eq!(sbv.rank1(2), 1); // bit 1 is clear
        assert_eq!(sbv.rank1(3), 2); // bit 2 is set
        assert_eq!(sbv.rank1(128), 64);
        assert_eq!(sbv.ones_count(), 64);
    }

    #[test]
    fn test_select1_basic() {
        let bits = make_bitvec(&[false, true, false, true, true, false, true]);
        let sbv = SuccinctBitVec::from_bitvec(bits);
        assert_eq!(sbv.select1(0), Some(1), "0th 1-bit at position 1");
        assert_eq!(sbv.select1(1), Some(3), "1st 1-bit at position 3");
        assert_eq!(sbv.select1(2), Some(4), "2nd 1-bit at position 4");
        assert_eq!(sbv.select1(3), Some(6), "3rd 1-bit at position 6");
        assert_eq!(sbv.select1(4), None, "only 4 ones, k=4 is out of range");
    }

    #[test]
    fn test_select1_across_words() {
        // 128 bits, one set bit per word at positions 32 and 96.
        let mut bits = vec![false; 128];
        bits[32] = true;
        bits[96] = true;
        let sbv = SuccinctBitVec::from_bitvec(make_bitvec(&bits));
        assert_eq!(sbv.select1(0), Some(32));
        assert_eq!(sbv.select1(1), Some(96));
        assert_eq!(sbv.select1(2), None);
    }

    #[test]
    fn test_rank_select_inverse() {
        // For any bitvector, select1(rank1(i)) >= i if bit i is set,
        // and rank1(select1(k)) == k for all k < ones_count.
        let bits: Vec<bool> = (0..256).map(|i| i % 3 == 0).collect();
        let sbv = SuccinctBitVec::from_bitvec(make_bitvec(&bits));

        for k in 0..sbv.ones_count() {
            let pos = sbv
                .select1(k)
                .unwrap_or_else(|| panic!("select1({k}) should exist"));
            assert_eq!(
                sbv.rank1(pos),
                k,
                "rank1(select1({k})) should equal {k}, got {}",
                sbv.rank1(pos)
            );
        }
    }

    #[test]
    fn test_get_bit() {
        let bits = make_bitvec(&[true, false, true]);
        let sbv = SuccinctBitVec::from_bitvec(bits);
        assert!(sbv.get(0));
        assert!(!sbv.get(1));
        assert!(sbv.get(2));
    }

    #[test]
    fn test_large_bitvector() {
        // 10K bits, every 7th bit set.
        let bits: Vec<bool> = (0..10_000).map(|i| i % 7 == 0).collect();
        let sbv = SuccinctBitVec::from_bitvec(make_bitvec(&bits));
        let expected_ones = (0..10_000).filter(|i| i % 7 == 0).count();
        assert_eq!(sbv.ones_count(), expected_ones);

        // Spot-check rank at a few positions.
        assert_eq!(sbv.rank1(7), 1);
        assert_eq!(sbv.rank1(14), 2);
        assert_eq!(sbv.rank1(10_000), expected_ones);

        // Spot-check select.
        assert_eq!(sbv.select1(0), Some(0));
        assert_eq!(sbv.select1(1), Some(7));
        assert_eq!(sbv.select1(2), Some(14));
    }
}
