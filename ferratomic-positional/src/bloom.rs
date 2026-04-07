//! Bloom filter for probabilistic entity existence checks (INV-FERR-027).
//!
//! Zero false negatives BY CONSTRUCTION: a set bit is never cleared
//! (monotonicity of bitwise OR). Insert is infallible -- no capacity
//! overflow, no eviction, no failure mode. False positive rate ~1%
//! at 10 bits per element with 7 hash functions.

use ferratom::EntityId;

/// Bloom filter for probabilistic entity existence checks (INV-FERR-027).
///
/// Zero false negatives BY CONSTRUCTION: a set bit is never cleared
/// (monotonicity of bitwise OR). Insert is infallible -- no capacity
/// overflow, no eviction, no failure mode. False positive rate ~1%
/// at 10 bits per element with 7 hash functions.
///
/// Uses BLAKE3 (already in the dependency tree) to derive k independent
/// hash functions from a single hash via double-hashing:
/// `h_i(x) = (h1(x) + i * h2(x)) mod m`.
#[derive(Clone, Debug)]
pub(crate) struct EntityBloom {
    /// Bit array. Total bits = `next_power_of_two(num_elements * BITS_PER_ELEMENT)`.
    bits: Vec<u64>,
    /// Total number of bits (`bits.len() * 64`).
    num_bits: u64,
}

/// Bits per element. 10 gives ~1% false positive rate with 7 hash functions.
const BITS_PER_ELEMENT: usize = 10;

/// Number of hash functions. Optimal k = (m/n) * ln(2) ~ 10 * 0.693 ~ 7.
const NUM_HASHES: u64 = 7;

impl EntityBloom {
    /// Build a Bloom filter from a set of entity IDs.
    ///
    /// Infallible: always succeeds regardless of input size.
    /// O(n) construction where n = number of entities.
    pub(crate) fn build(entity_ids: &[EntityId]) -> Self {
        let num_bits_raw = (entity_ids.len() * BITS_PER_ELEMENT).max(64);
        // Round up to next power of 2 so h2|1 guarantees gcd(h2, num_bits) = 1,
        // giving full period for the double-hashing probe sequence.
        let num_bits = (num_bits_raw as u64).next_power_of_two();
        let num_words = (num_bits / 64) as usize;
        let mut bits = vec![0u64; num_words];

        for eid in entity_ids {
            let (h1, h2) = Self::hash_pair(eid);
            for i in 0..NUM_HASHES {
                let bit_idx = (h1.wrapping_add(i.wrapping_mul(h2))) % num_bits;
                let word = (bit_idx / 64) as usize;
                bits[word] |= 1u64 << (bit_idx % 64);
            }
        }

        Self { bits, num_bits }
    }

    /// Check whether an entity ID MIGHT be present.
    ///
    /// Returns `false` -> entity is DEFINITELY absent (zero false negatives).
    /// Returns `true` -> entity is PROBABLY present (~1% false positive rate).
    pub(crate) fn maybe_contains(&self, eid: &EntityId) -> bool {
        let (h1, h2) = Self::hash_pair(eid);
        for i in 0..NUM_HASHES {
            let bit_idx = (h1.wrapping_add(i.wrapping_mul(h2))) % self.num_bits;
            let word = (bit_idx / 64) as usize;
            if self.bits[word] & (1u64 << (bit_idx % 64)) == 0 {
                return false; // definitive negative -- bit not set
            }
        }
        true
    }

    /// Derive two independent hash values from an `EntityId` via BLAKE3.
    ///
    /// Uses the first 8 bytes as h1 and the next 8 bytes as h2.
    /// `EntityId` is already a BLAKE3 hash (INV-FERR-012), so its bytes
    /// are uniformly distributed -- no additional hashing needed.
    ///
    /// The `h2 | 1` construction loses ~1 bit of entropy but increases FPR
    /// by <0.01%, well within the target 8% FPR bound.
    fn hash_pair(eid: &EntityId) -> (u64, u64) {
        let bytes = eid.as_bytes();
        let h1 = u64::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ]);
        // h2 must be odd so gcd(h2, num_bits) = 1 (num_bits is power of 2).
        let h2 = u64::from_le_bytes([
            bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15],
        ]) | 1;
        (h1, h2)
    }
}
