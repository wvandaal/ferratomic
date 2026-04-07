//! WAL dedup Bloom filter for eliminating redundant writes (INV-FERR-084).
//!
//! Fixed 64 KB. Eliminates redundant WAL writes from bursty event sources.
//! False positive rate ~8% at 100K entries (k=4 hash functions over
//! 524,288 bits, optimal k = m/n * ln2 ≈ 3.6). Cleared on checkpoint
//! to bound FP accumulation.
//!
//! The Bloom is ADVISORY — the caller (Database layer) must verify store
//! membership before skipping a WAL write (INV-FERR-084 two-phase check).
//! A false positive triggers a store membership check, not an unconditional
//! skip.

/// Bloom filter bits: 64 KB = 524,288 bits = 8,192 u64 words.
const BLOOM_BITS: usize = 524_288;

/// Number of u64 words in the bit array.
const BLOOM_WORDS: usize = BLOOM_BITS / 64;

/// Number of hash functions. k=4 is near-optimal for m=524,288, n=100K
/// (optimal k = (m/n) * ln2 ≈ 3.63). Theoretical FP rate:
/// (1 - e^(-kn/m))^k ≈ 8.1% at 100K entries.
const K: usize = 4;

/// WAL dedup Bloom filter (INV-FERR-084).
///
/// Fixed 64 KB. Detects duplicate datom content hashes in O(1) with
/// ~8% false positive rate at 100K entries. Zero false negatives.
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
    /// False positive rate ~8% at 100K entries. Zero false negatives.
    /// INV-FERR-084: a `true` result triggers Phase 2 (store membership
    /// check). Only skip the WAL write if BOTH phases confirm the datom
    /// is already durable.
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
        // rem_euclid(m) guarantees bit_idx in [0, BLOOM_BITS). BLOOM_BITS = 524,288
        // fits in u32, so try_from is infallible on all platforms (usize >= u32).
        // unwrap_or(0) satisfies NEG-FERR-001 (no panic) as a fallback that can
        // never actually trigger.
        indices[i] = usize::try_from(bit_idx).unwrap_or(0);
        i += 1;
    }
    indices
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Generate a test hash with uniform entropy via `SplitMix64` PRNG.
    /// Produces 4 independent u64 values covering all 32 bytes, ensuring
    /// h1 (bytes 0-7) and h2 (bytes 8-15) are well-distributed.
    fn spread_hash(seed: u32) -> [u8; 32] {
        let mut state = u64::from(seed);
        let mut h = [0u8; 32];
        for chunk in h.chunks_exact_mut(8) {
            state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = state;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^= z >> 31;
            chunk.copy_from_slice(&z.to_le_bytes());
        }
        h
    }

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
        // Insert 1000 distinct hashes with entropy in both h1 and h2 regions.
        for i in 0u16..1000 {
            let hash = spread_hash(u32::from(i));
            bloom.insert(&hash);
        }
        for i in 0u16..1000 {
            let hash = spread_hash(u32::from(i));
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
        // Insert 100K entries with entropy spread across h1 AND h2 byte regions.
        for i in 0u32..100_000 {
            let hash = spread_hash(i);
            bloom.insert(&hash);
        }
        // Check 100K ABSENT entries for false positives.
        let mut false_positives = 0u32;
        for i in 100_000u32..200_000 {
            let hash = spread_hash(i);
            if bloom.probably_contains(&hash) {
                false_positives += 1;
            }
        }
        let fp_rate = f64::from(false_positives) / 100_000.0;
        // Theoretical FP rate for m=524288, k=4, n=100K: ~8.1%.
        // Allow up to 15% to account for variance.
        assert!(
            fp_rate < 0.15,
            "INV-FERR-084: FP rate {fp_rate:.4} exceeds 15% threshold at 100K entries \
             (expected ~8%, got {false_positives} FPs out of 100K probes)"
        );
    }
}
