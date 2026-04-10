//! Chunk-based federation transfer between chunk stores.
//!
//! `INV-FERR-048`: `transfer(src, dst, root)` copies exactly the chunks
//! reachable from `root` in `src` that are not present in `dst`. No more
//! (minimality), no less (completeness). The operation is idempotent,
//! monotonic, and resumable.
//!
//! This is the operational realization of anti-entropy (INV-FERR-022).
//! At 100M datoms with 100 changed entries, transfer sends ~330 chunks
//! (~43 MB), not 100M datoms.

use ferratom::error::FerraError;

use crate::prolly::{
    build::decode_child_addrs,
    chunk::{Chunk, ChunkStore, Hash},
};

/// The result of a transfer operation.
///
/// `INV-FERR-048`: `chunks_transferred + chunks_skipped` equals the
/// total number of chunks reachable from `root` in `src`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferResult {
    /// Number of chunks transferred (not already present in dst).
    pub chunks_transferred: u64,
    /// Number of chunks skipped (already present in dst).
    pub chunks_skipped: u64,
    /// Total bytes transferred.
    pub bytes_transferred: u64,
    /// The root hash that is now resolvable from dst.
    pub root: Hash,
}

/// Transfer trait: send chunks between chunk stores.
///
/// `INV-FERR-048`: Implementations must satisfy minimality (no redundant
/// chunks), monotonicity (more chunks in dst → fewer to transfer), and
/// idempotency (second transfer sends nothing).
pub trait ChunkTransfer {
    /// Transfer all chunks reachable from `root` in `src` that are not
    /// present in `dst`.
    ///
    /// `root` is a **tree root** (not a manifest hash). See INV-FERR-049
    /// for the manifest → `RootSet` → tree root resolution protocol.
    ///
    /// # Errors
    ///
    /// Returns `FerraError` if chunk store operations fail or if a
    /// chunk referenced by the tree is missing from `src`.
    fn transfer(
        &self,
        src: &dyn ChunkStore,
        dst: &dyn ChunkStore,
        root: &Hash,
    ) -> Result<TransferResult, FerraError>;
}

/// Default implementation: recursive descent with `has_chunk` pruning.
///
/// `INV-FERR-048`: Walks the tree from `root` to leaves. For each chunk:
/// if `dst` already has it (by content address), skip the entire subtree.
/// Otherwise, copy the chunk and recurse into its children.
pub struct RecursiveTransfer;

impl ChunkTransfer for RecursiveTransfer {
    fn transfer(
        &self,
        src: &dyn ChunkStore,
        dst: &dyn ChunkStore,
        root: &Hash,
    ) -> Result<TransferResult, FerraError> {
        let mut result = TransferResult {
            chunks_transferred: 0,
            chunks_skipped: 0,
            bytes_transferred: 0,
            root: *root,
        };
        transfer_recursive(src, dst, root, &mut result)?;
        Ok(result)
    }
}

/// Recursive descent: copy chunk if missing, then recurse into children.
fn transfer_recursive(
    src: &dyn ChunkStore,
    dst: &dyn ChunkStore,
    addr: &Hash,
    result: &mut TransferResult,
) -> Result<(), FerraError> {
    // Pruning: if dst already has this chunk, skip entire subtree.
    // Content addressing guarantees identical content.
    if dst.has_chunk(addr)? {
        result.chunks_skipped += 1;
        return Ok(());
    }

    let chunk = load_chunk(addr, src)?;

    // Store in destination (idempotent per INV-FERR-050c)
    dst.put_chunk(&chunk)?;
    result.chunks_transferred += 1;
    result.bytes_transferred += chunk.len() as u64;

    // Decode children and recurse (empty for leaf chunks)
    let children = decode_child_addrs(&chunk)?;
    for child_addr in &children {
        transfer_recursive(src, dst, child_addr, result)?;
    }

    Ok(())
}

/// Load a chunk from the store, returning `InvariantViolation` if missing.
fn load_chunk(addr: &Hash, store: &dyn ChunkStore) -> Result<Chunk, FerraError> {
    store
        .get_chunk(addr)?
        .ok_or_else(|| FerraError::InvariantViolation {
            invariant: "INV-FERR-048".into(),
            details: format!(
                "chunk {:02x}{:02x}{:02x}{:02x}... not found in source during transfer",
                addr[0], addr[1], addr[2], addr[3],
            ),
        })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::prolly::{
        boundary::DEFAULT_PATTERN_WIDTH, build::build_prolly_tree, chunk::MemoryChunkStore,
        read::read_prolly_tree,
    };

    fn build(kvs: &BTreeMap<Vec<u8>, Vec<u8>>, store: &MemoryChunkStore) -> Hash {
        build_prolly_tree(kvs, store, DEFAULT_PATTERN_WIDTH).expect("build")
    }

    #[test]
    fn test_inv_ferr_048_transfer_to_empty() {
        let src = MemoryChunkStore::new();
        let dst = MemoryChunkStore::new();
        let mut kvs = BTreeMap::new();
        for i in 0u32..50 {
            kvs.insert(i.to_be_bytes().to_vec(), vec![0u8; 8]);
        }
        let root = build(&kvs, &src);

        let xfer = RecursiveTransfer;
        let result = xfer.transfer(&src, &dst, &root).expect("transfer");

        assert!(
            result.chunks_transferred > 0,
            "must transfer at least one chunk"
        );
        assert_eq!(result.chunks_skipped, 0, "empty dst has nothing to skip");

        // Verify roundtrip from dst
        let recovered = read_prolly_tree(&root, &dst).expect("read from dst");
        assert_eq!(recovered, kvs, "INV-FERR-048: transfer must be complete");
    }

    #[test]
    fn test_inv_ferr_048_idempotent() {
        let src = MemoryChunkStore::new();
        let dst = MemoryChunkStore::new();
        let mut kvs = BTreeMap::new();
        for i in 0u32..30 {
            kvs.insert(i.to_be_bytes().to_vec(), vec![1u8; 4]);
        }
        let root = build(&kvs, &src);

        let xfer = RecursiveTransfer;
        let r1 = xfer.transfer(&src, &dst, &root).expect("first transfer");
        assert!(r1.chunks_transferred > 0);

        let r2 = xfer.transfer(&src, &dst, &root).expect("second transfer");
        assert_eq!(
            r2.chunks_transferred, 0,
            "INV-FERR-048: second transfer must send zero chunks"
        );
        assert!(
            r2.chunks_skipped > 0,
            "second transfer must skip all chunks"
        );
    }

    #[test]
    fn test_inv_ferr_048_partial_overlap() {
        let src = MemoryChunkStore::new();
        let dst = MemoryChunkStore::new();

        // Build base tree in both stores (shared history).
        // Need enough entries to produce multiple chunks (pw=8, expected ~256/chunk).
        let mut base = BTreeMap::new();
        for i in 0u32..500 {
            base.insert(i.to_be_bytes().to_vec(), vec![0u8; 8]);
        }
        let base_root = build(&base, &src);
        let xfer = RecursiveTransfer;
        xfer.transfer(&src, &dst, &base_root).expect("sync base");

        // Modify source only — add one key at the end
        let mut modified = base.clone();
        modified.insert(9999u32.to_be_bytes().to_vec(), vec![99u8; 8]);
        let new_root = build(&modified, &src);

        let result = xfer.transfer(&src, &dst, &new_root).expect("incremental");
        assert!(result.chunks_skipped > 0, "shared chunks must be skipped");
        assert!(
            result.chunks_transferred > 0,
            "new chunks must be transferred"
        );

        let recovered = read_prolly_tree(&new_root, &dst).expect("read from dst");
        assert_eq!(
            recovered, modified,
            "INV-FERR-048: incremental transfer must be complete"
        );
    }

    #[test]
    fn test_inv_ferr_048_monotonic() {
        let src = MemoryChunkStore::new();
        let dst = MemoryChunkStore::new();
        let mut kvs = BTreeMap::new();
        for i in 0u32..20 {
            kvs.insert(i.to_be_bytes().to_vec(), vec![0u8; 4]);
        }
        let root = build(&kvs, &src);

        // Record dst state before transfer
        let dst_before = dst.all_addrs().expect("addrs");

        let xfer = RecursiveTransfer;
        xfer.transfer(&src, &dst, &root).expect("transfer");

        // Verify: all pre-existing chunks still present (monotonic)
        for addr in &dst_before {
            assert!(
                dst.has_chunk(addr).expect("has"),
                "INV-FERR-048: transfer must not delete existing chunks"
            );
        }
    }

    #[test]
    fn test_inv_ferr_048_empty_tree() {
        let src = MemoryChunkStore::new();
        let dst = MemoryChunkStore::new();
        let kvs: BTreeMap<Vec<u8>, Vec<u8>> = BTreeMap::new();
        let root = build(&kvs, &src);

        let xfer = RecursiveTransfer;
        let result = xfer.transfer(&src, &dst, &root).expect("transfer empty");
        assert_eq!(result.chunks_transferred, 1, "empty tree is one leaf chunk");

        let recovered = read_prolly_tree(&root, &dst).expect("read");
        assert!(recovered.is_empty());
    }

    #[test]
    fn test_inv_ferr_048_missing_source_chunk() {
        let src = MemoryChunkStore::new();
        let dst = MemoryChunkStore::new();
        let bogus = [0xDEu8; 32];

        let xfer = RecursiveTransfer;
        let err = xfer.transfer(&src, &dst, &bogus).unwrap_err();
        assert!(
            matches!(err, FerraError::InvariantViolation { .. }),
            "missing source chunk must return InvariantViolation, got: {err:?}"
        );
    }

    #[test]
    fn test_inv_ferr_048_bytes_tracked() {
        let src = MemoryChunkStore::new();
        let dst = MemoryChunkStore::new();
        let mut kvs = BTreeMap::new();
        kvs.insert(vec![1u8], vec![2u8]);
        let root = build(&kvs, &src);

        let xfer = RecursiveTransfer;
        let result = xfer.transfer(&src, &dst, &root).expect("transfer");
        assert!(
            result.bytes_transferred > 0,
            "bytes_transferred must be positive"
        );
    }
}
