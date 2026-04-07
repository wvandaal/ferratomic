//! Chunk fingerprint array for O(delta) federation reconciliation (INV-FERR-079).
//!
//! Divides the canonical position space into fixed-size chunks and maintains
//! a 32-byte XOR fingerprint per chunk. The store-level fingerprint
//! (INV-FERR-074) equals the XOR of all chunk fingerprints (decomposition
//! theorem). Enables O(delta) federation sync and O(delta × C) incremental
//! LIVE rebuild (INV-FERR-080).
//!
//! Default chunk size: 1024 datoms (~120 KB per chunk at ~120 bytes/datom).
//! Array size at 100M datoms: ~100K entries × 32 bytes = ~3.2 MB.

use bitvec::prelude::{BitVec, Lsb0};
use ferratom::Datom;

/// Default chunk size: 1024 datoms.
pub const DEFAULT_CHUNK_SIZE: usize = 1024;

/// Chunk fingerprint array (INV-FERR-079).
///
/// Divides the canonical position space into fixed-size chunks and
/// maintains a 32-byte XOR fingerprint per chunk. Enables O(delta)
/// federation reconciliation and incremental LIVE maintenance.
///
/// The store-level fingerprint (INV-FERR-074) is the XOR of all chunk
/// fingerprints — the direct-sum decomposition of the homomorphism.
///
/// Dirty flags track which chunks were modified since the last LIVE
/// rebuild, enabling O(delta × C) incremental LIVE (INV-FERR-080).
#[derive(Clone, Debug)]
pub struct ChunkFingerprints {
    /// Per-chunk XOR fingerprints. `chunks[i]` = XOR of `content_hash(d)`
    /// for all datoms at canonical positions `[i*C, (i+1)*C)`.
    chunks: Vec<[u8; 32]>,
    /// Chunk size (number of datoms per chunk).
    chunk_size: usize,
    /// Dirty flags: `dirty[i]` is set if chunk `i` was modified since
    /// the last LIVE rebuild.
    dirty: BitVec<u64, Lsb0>,
}

impl ChunkFingerprints {
    /// Build from a canonical datom array. O(n) — one `content_hash` +
    /// one XOR per datom (INV-FERR-079).
    ///
    /// Uses `Datom::content_hash()` (INV-FERR-012) for consistency with
    /// the store-level fingerprint (INV-FERR-074).
    ///
    /// # Panics
    ///
    /// Panics if `chunk_size` is zero or not a power of two (INV-FERR-079
    /// Level 0: "C be the chunk size, a fixed power of 2").
    #[must_use]
    pub fn from_canonical(canonical: &[Datom], chunk_size: usize) -> Self {
        assert!(
            chunk_size > 0 && chunk_size.is_power_of_two(),
            "INV-FERR-079: chunk_size must be a positive power of 2, got {chunk_size}"
        );

        let num_chunks = if canonical.is_empty() {
            0
        } else {
            canonical.len().div_ceil(chunk_size)
        };
        let mut chunks = vec![[0u8; 32]; num_chunks];

        for (pos, datom) in canonical.iter().enumerate() {
            let chunk_idx = pos / chunk_size;
            let hash = datom.content_hash();
            for (a, b) in chunks[chunk_idx].iter_mut().zip(hash.iter()) {
                *a ^= b;
            }
        }

        let mut dirty = BitVec::<u64, Lsb0>::new();
        dirty.resize(num_chunks, false);

        Self {
            chunks,
            chunk_size,
            dirty,
        }
    }

    /// XOR a datom hash into the chunk at the given canonical position.
    /// O(1) — one XOR. Marks the chunk dirty (INV-FERR-079 incremental
    /// update theorem).
    ///
    /// Extends the chunk array if `position` falls beyond the current
    /// range. Used by incremental transact paths (bd-nq6v).
    pub fn insert_hash(&mut self, position: usize, datom_hash: &[u8; 32]) {
        let chunk_idx = position / self.chunk_size;
        if chunk_idx >= self.chunks.len() {
            self.chunks.resize(chunk_idx + 1, [0u8; 32]);
            self.dirty.resize(chunk_idx + 1, false);
        }
        for (a, b) in self.chunks[chunk_idx].iter_mut().zip(datom_hash.iter()) {
            *a ^= b;
        }
        self.dirty.set(chunk_idx, true);
    }

    /// Store-level fingerprint: XOR of all chunk fingerprints. O(K).
    ///
    /// Equals `compute_fingerprint(canonical)` (INV-FERR-074) by the
    /// decomposition theorem: H(S) = XOR_{i} CF(S)\[i\].
    #[must_use]
    pub fn store_fingerprint(&self) -> [u8; 32] {
        let mut result = [0u8; 32];
        for chunk in &self.chunks {
            for (a, b) in result.iter_mut().zip(chunk.iter()) {
                *a ^= b;
            }
        }
        result
    }

    /// Indices of chunks where fingerprints differ. O(K).
    ///
    /// For federation reconciliation: only differing chunks need datom
    /// transfer. Missing chunks on either side are treated as empty
    /// (fingerprint = `[0; 32]`, the XOR identity).
    #[must_use]
    pub fn diff_chunks(&self, other: &Self) -> Vec<usize> {
        let max_len = self.chunks.len().max(other.chunks.len());
        let mut differing = Vec::new();
        for i in 0..max_len {
            let a = self.chunk_at(i);
            let b = other.chunk_at(i);
            if a != b {
                differing.push(i);
            }
        }
        differing
    }

    /// Mark a chunk as dirty (modified since last LIVE rebuild).
    pub fn mark_dirty(&mut self, chunk_idx: usize) {
        if chunk_idx < self.dirty.len() {
            self.dirty.set(chunk_idx, true);
        }
    }

    /// Iterator over dirty chunk indices (INV-FERR-080 prerequisite).
    pub fn dirty_chunks(&self) -> impl Iterator<Item = usize> + '_ {
        self.dirty.iter_ones()
    }

    /// Clear all dirty flags after LIVE rebuild completes.
    pub fn clear_dirty(&mut self) {
        self.dirty.fill(false);
    }

    /// Number of chunks.
    #[must_use]
    pub fn num_chunks(&self) -> usize {
        self.chunks.len()
    }

    /// Chunk size (datoms per chunk).
    #[must_use]
    pub fn chunk_size(&self) -> usize {
        self.chunk_size
    }

    /// Fingerprint for a single chunk, or `[0; 32]` if out of range.
    #[must_use]
    fn chunk_at(&self, idx: usize) -> [u8; 32] {
        self.chunks.get(idx).copied().unwrap_or([0u8; 32])
    }
}
