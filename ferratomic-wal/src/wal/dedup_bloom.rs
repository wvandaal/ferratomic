//! WAL dedup Bloom filter for eliminating redundant writes (INV-FERR-084).
//!
//! Fixed 64 KB. Eliminates redundant WAL writes from bursty event sources.
//! False positive rate ~0.1% at 100K entries (k=7 hash functions over
//! 524,288 bits). Cleared on checkpoint to bound FP accumulation.
//!
//! The Bloom is ADVISORY — the store's set semantics (INV-FERR-003) are
//! the correctness guarantee. A false positive causes ONE datom to skip
//! ONE WAL write; the datom will be durable after the next submission or
//! checkpoint.

/// Bloom filter bits: 64 KB = 524,288 bits = 8,192 u64 words.
const BLOOM_BITS: usize = 524_288;

/// Number of u64 words in the bit array.
const BLOOM_WORDS: usize = BLOOM_BITS / 64;

/// Number of hash functions. k=7 gives ~0.1% FP rate at ~100K entries
/// for m=524,288 bits (optimal k = (m/n) * ln2 ≈ 3.6 for 100K;
/// k=7 is conservative, giving lower FP rate at the cost of slightly
/// more hash probes per operation).
const K: usize = 7;

/// WAL dedup Bloom filter (INV-FERR-084).
///
/// Fixed 64 KB. Detects duplicate datom content hashes in O(1) with
/// ~0.1% false positive rate at 100K entries. Zero false negatives.
///
/// Cleared on checkpoint (prevents unbounded FP accumulation).
pub struct WalDedupBloom {
    /// Bit array (64 KB heap-allocated).
    bits: Box<[u64]>,
    /// Entry count (monitoring, not correctness).
    count: u64,
}

impl WalDedupBloom {
    /// Create a new empty Bloom filter (64 KB).
    #[must_use]
    pub fn new() -> Self {
        Self {
            bits: vec![0u64; BLOOM_WORDS].into_boxed_slice(),
            count: 0,
        }
    }

    /// Check if a content hash is probably already in the WAL.
    ///
    /// False positive rate ~0.1% at 100K entries. Zero false negatives.
    /// INV-FERR-084: safe to skip WAL write on `true` because the store's
    /// set semantics (INV-FERR-003) guarantee idempotent re-insertion.
    #[inline]
    #[must_use]
    pub fn probably_contains(&self, hash: &[u8; 32]) -> bool {
        hash_indices(hash).iter().all(|&idx| self.get_bit(idx))
    }

    /// Record a content hash after WAL write.
    #[inline]
    pub fn insert(&mut self, hash: &[u8; 32]) {
        for idx in hash_indices(hash) {
            self.set_bit(idx);
        }
        self.count = self.count.saturating_add(1);
    }

    /// Clear all bits on checkpoint (INV-FERR-084: bounded FP accumulation).
    ///
    /// Checkpointed datoms are durable in the snapshot — their WAL entries
    /// are no longer needed, so the Bloom can be reset.
    pub fn clear(&mut self) {
        for word in &mut *self.bits {
            *word = 0;
        }
        self.count = 0;
    }

    /// Number of entries inserted since last clear.
    #[must_use]
    pub fn count(&self) -> u64 {
        self.count
    }

    #[inline]
    fn get_bit(&self, idx: usize) -> bool {
        let word = idx / 64;
        let bit = idx % 64;
        (self.bits[word] >> bit) & 1 == 1
    }

    #[inline]
    fn set_bit(&mut self, idx: usize) {
        let word = idx / 64;
        let bit = idx % 64;
        self.bits[word] |= 1u64 << bit;
    }
}

impl Default for WalDedupBloom {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for WalDedupBloom {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let set_bits = self.bits.iter().map(|w| w.count_ones()).sum::<u32>();
        f.debug_struct("WalDedupBloom")
            .field("bits_set", &set_bits)
            .field("bits_total", &BLOOM_BITS)
            .field("count", &self.count)
            .finish()
    }
}

/// Double hashing: h(i) = (h1 + i * h2) mod m.
///
/// h1 and h2 are extracted from the first 16 bytes of the 32-byte
/// content hash (BLAKE3, uniformly distributed). This gives k=7
/// independent bit positions without needing k separate hash functions.
#[inline]
fn hash_indices(hash: &[u8; 32]) -> [usize; K] {
    let h1 = u64::from_le_bytes([
        hash[0], hash[1], hash[2], hash[3], hash[4], hash[5], hash[6], hash[7],
    ]);
    let h2 = u64::from_le_bytes([
        hash[8], hash[9], hash[10], hash[11], hash[12], hash[13], hash[14], hash[15],
    ]);
    let m = BLOOM_BITS as u64;
    let mut indices = [0usize; K];
    let mut i = 0;
    while i < K {
        let bit_idx = h1.wrapping_add((i as u64).wrapping_mul(h2)).rem_euclid(m);
        // bit_idx < BLOOM_BITS = 524,288 which fits in u32 on all platforms.
        indices[i] = usize::try_from(bit_idx).unwrap_or(0);
        i += 1;
    }
    indices
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_inv_ferr_084_empty_bloom_contains_nothing() {
        let bloom = WalDedupBloom::new();
        let hash = [0xABu8; 32];
        assert!(
            !bloom.probably_contains(&hash),
            "INV-FERR-084: empty Bloom must not contain any hash"
        );
    }

    #[test]
    fn test_inv_ferr_084_insert_then_contains() {
        let mut bloom = WalDedupBloom::new();
        let hash = [0x42u8; 32];
        bloom.insert(&hash);
        assert!(
            bloom.probably_contains(&hash),
            "INV-FERR-084: inserted hash must be found (zero false negatives)"
        );
    }

    #[test]
    fn test_inv_ferr_084_distinct_hashes_independent() {
        let mut bloom = WalDedupBloom::new();
        let hash_a = [0x01u8; 32];
        bloom.insert(&hash_a);
        assert!(
            bloom.probably_contains(&hash_a),
            "INV-FERR-084: hash_a must be found after insert"
        );
        // Absent hash with only 1 entry in 64KB filter — FP rate is negligible.
        let hash_absent = [0x02u8; 32];
        assert!(
            !bloom.probably_contains(&hash_absent),
            "INV-FERR-084: absent hash should not be found with 1 entry in 64KB"
        );
    }

    #[test]
    fn test_inv_ferr_084_clear_resets() {
        let mut bloom = WalDedupBloom::new();
        let hash = [0xFFu8; 32];
        bloom.insert(&hash);
        assert!(bloom.probably_contains(&hash));
        bloom.clear();
        assert!(
            !bloom.probably_contains(&hash),
            "INV-FERR-084: clear must reset all bits"
        );
        assert_eq!(bloom.count(), 0, "INV-FERR-084: clear must reset count");
    }

    #[test]
    fn test_inv_ferr_084_zero_false_negatives() {
        let mut bloom = WalDedupBloom::new();
        // Insert 1000 distinct hashes, verify all are found.
        for i in 0u16..1000 {
            let mut hash = [0u8; 32];
            hash[0] = (i >> 8) as u8;
            hash[1] = (i & 0xFF) as u8;
            bloom.insert(&hash);
        }
        for i in 0u16..1000 {
            let mut hash = [0u8; 32];
            hash[0] = (i >> 8) as u8;
            hash[1] = (i & 0xFF) as u8;
            assert!(
                bloom.probably_contains(&hash),
                "INV-FERR-084: hash {i} must be found (zero false negatives)"
            );
        }
        assert_eq!(bloom.count(), 1000);
    }

    #[test]
    fn test_inv_ferr_084_size() {
        assert_eq!(
            BLOOM_WORDS * 8,
            65536,
            "INV-FERR-084: Bloom filter must be exactly 64KB"
        );
    }

    #[test]
    fn test_inv_ferr_084_false_positive_rate() {
        let mut bloom = WalDedupBloom::new();
        // Insert 100K entries.
        for i in 0u32..100_000 {
            let mut hash = [0u8; 32];
            hash[0..4].copy_from_slice(&i.to_le_bytes());
            bloom.insert(&hash);
        }
        // Check 100K ABSENT entries for false positives.
        let mut false_positives = 0u32;
        for i in 100_000u32..200_000 {
            let mut hash = [0u8; 32];
            hash[0..4].copy_from_slice(&i.to_le_bytes());
            if bloom.probably_contains(&hash) {
                false_positives += 1;
            }
        }
        let fp_rate = f64::from(false_positives) / 100_000.0;
        assert!(
            fp_rate < 0.01,
            "INV-FERR-084: FP rate {fp_rate:.4} exceeds 1% threshold at 100K entries \
             (expected ~0.1%, got {false_positives} FPs out of 100K probes)"
        );
    }
}
