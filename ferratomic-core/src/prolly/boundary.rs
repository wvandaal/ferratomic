//! Gear hash and chunk boundary detection.
//!
//! `INV-FERR-046a`: The rolling hash algorithm (Gear hash with a fixed
//! BLAKE3-derived 256-entry table) and boundary predicate are fully
//! specified. Any conforming implementation produces identical chunk
//! boundaries for the same sorted key sequence.
//!
//! The `GEAR_TABLE` seed is `b"ferratomic-gear-hash-table"` — changing it
//! is a breaking change that invalidates all existing chunk stores.

use std::sync::LazyLock;

/// Fixed random table for Gear hash.
///
/// Generated deterministically from BLAKE3:
/// `GEAR_TABLE[i] = u64::from_le_bytes(BLAKE3::derive_key(..., &[i])[0..8])`
///
/// `INV-FERR-046a`: This table is part of the specification. Changing it
/// changes all chunk boundaries in all stores.
static GEAR_TABLE: LazyLock<[u64; 256]> = LazyLock::new(|| {
    let mut table = [0u64; 256];
    for byte_val in u8::MIN..=u8::MAX {
        let key = blake3::derive_key("ferratomic-gear-hash-table", &[byte_val]);
        let bytes: [u8; 8] = [
            key[0], key[1], key[2], key[3], key[4], key[5], key[6], key[7],
        ];
        table[usize::from(byte_val)] = u64::from_le_bytes(bytes);
    }
    table
});

/// Minimum chunk size in entries.
///
/// Chunks smaller than this are never split regardless of hash value.
/// `INV-FERR-046a`: spec constant — changing invalidates chunk stores.
pub const MIN_CHUNK_SIZE: usize = 32;

/// Maximum chunk size in entries.
///
/// Chunks reaching this size are always split regardless of hash value.
/// `INV-FERR-046a`: spec constant — changing invalidates chunk stores.
pub const MAX_CHUNK_SIZE: usize = 1024;

/// Default pattern width. Expected chunk size = `2^PATTERN_WIDTH` entries.
///
/// `INV-FERR-046a`: `pattern_width` is a store-wide constant determined at
/// creation. Changing it after creation is a breaking change.
pub const DEFAULT_PATTERN_WIDTH: u32 = 8;

/// Gear hash: content-defined chunking hash function.
///
/// `INV-FERR-046a` T1/T2: deterministic, pure. For each byte `b` in the input,
/// the accumulator is rotated left by 1 bit and XOR-combined with `GEAR_TABLE[b]`.
#[must_use]
pub fn gear_hash(key: &[u8]) -> u64 {
    let table = &*GEAR_TABLE;
    let mut hash: u64 = 0;
    for &byte in key {
        hash = hash.rotate_left(1) ^ table[byte as usize];
    }
    hash
}

/// Chunk boundary predicate with CDF bounds.
///
/// `INV-FERR-046a` T3: deterministic. A key is a boundary if:
/// 1. `entries_since_last >= MIN_CHUNK_SIZE`, AND
/// 2. `gear_hash(key) & mask == mask` OR `entries_since_last >= MAX_CHUNK_SIZE`
///
/// The `entries_since_last` parameter is the count of entries since the
/// last boundary in the sorted key sequence — a pure function of position,
/// NOT mutable state carried across builds.
#[must_use]
pub fn is_boundary(key: &[u8], pattern_width: u32, entries_since_last: usize) -> bool {
    if entries_since_last < MIN_CHUNK_SIZE {
        return false;
    }
    if entries_since_last >= MAX_CHUNK_SIZE {
        return true;
    }
    let hash = gear_hash(key);
    let mask = (1u64 << pattern_width) - 1;
    (hash & mask) == mask
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_inv_ferr_046a_gear_hash_deterministic() {
        let key = b"test key for gear hash";
        let h1 = gear_hash(key);
        let h2 = gear_hash(key);
        assert_eq!(h1, h2, "INV-FERR-046a T1: gear_hash must be deterministic");
    }

    #[test]
    fn test_inv_ferr_046a_gear_hash_varies() {
        let h1 = gear_hash(b"key one");
        let h2 = gear_hash(b"key two");
        assert_ne!(
            h1, h2,
            "gear_hash should produce different hashes for different inputs"
        );
    }

    #[test]
    fn test_inv_ferr_046a_gear_hash_empty() {
        let h = gear_hash(b"");
        assert_eq!(
            h, 0,
            "gear_hash of empty input is 0 (identity of rotate-XOR)"
        );
    }

    #[test]
    fn test_inv_ferr_046a_boundary_min_chunk() {
        let key = b"any key at all";
        for entries in 0..MIN_CHUNK_SIZE {
            assert!(
                !is_boundary(key, DEFAULT_PATTERN_WIDTH, entries),
                "entries {entries} < MIN must not be a boundary",
            );
        }
    }

    #[test]
    fn test_inv_ferr_046a_boundary_max_chunk() {
        let key = b"any key at all";
        assert!(
            is_boundary(key, DEFAULT_PATTERN_WIDTH, MAX_CHUNK_SIZE),
            "entries >= MAX must always be a boundary"
        );
        assert!(
            is_boundary(key, DEFAULT_PATTERN_WIDTH, MAX_CHUNK_SIZE + 500),
            "entries >> MAX must always be a boundary"
        );
    }

    #[test]
    fn test_inv_ferr_046a_boundary_deterministic() {
        let key = b"boundary test key";
        let pw = DEFAULT_PATTERN_WIDTH;
        let entries = 100;
        let b1 = is_boundary(key, pw, entries);
        let b2 = is_boundary(key, pw, entries);
        assert_eq!(
            b1, b2,
            "INV-FERR-046a T3: is_boundary must be deterministic"
        );
    }

    #[test]
    fn test_inv_ferr_046a_gear_table_deterministic() {
        let table = &*GEAR_TABLE;
        let key = blake3::derive_key("ferratomic-gear-hash-table", &[0u8]);
        let bytes: [u8; 8] = [
            key[0], key[1], key[2], key[3], key[4], key[5], key[6], key[7],
        ];
        let expected = u64::from_le_bytes(bytes);
        assert_eq!(
            table[0], expected,
            "GEAR_TABLE[0] must match BLAKE3-derived value from spec seed"
        );
    }

    #[test]
    fn test_inv_ferr_046a_boundary_expected_rate() {
        // Statistical test: 100K trials for tighter confidence bounds
        let pw = 8u32;
        let expected_rate = 1.0 / f64::from(1u32 << pw); // ~0.0039
        let mut boundaries = 0u32;
        let trials = 100_000u32;
        for i in 0..trials {
            let key = i.to_le_bytes();
            if is_boundary(&key, pw, MIN_CHUNK_SIZE) {
                boundaries += 1;
            }
        }
        let actual_rate = f64::from(boundaries) / f64::from(trials);
        // With 100K trials and expected rate ~0.39%, we expect ~390 boundaries.
        // Tolerance: 0.5x to 2.0x (tighter than before, justified by 10x more trials)
        assert!(
            actual_rate > expected_rate * 0.5 && actual_rate < expected_rate * 2.0,
            "boundary rate {actual_rate:.6} should be within 0.5x-2.0x of {expected_rate:.6} (1/2^{pw})",
        );
    }
}
