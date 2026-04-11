//! Prolly tree read — walk from root hash to leaves.
//!
//! `INV-FERR-049`: `read_prolly_tree(S, root_hash(T)) = T` — reconstructing
//! a prolly tree from its root hash and a `ChunkStore` containing all its
//! chunks yields the original key-value set.
//!
//! This module implements the inverse of [`build_prolly_tree`]: given a root
//! hash and a store, recursively descend through internal nodes to leaves,
//! deserialize all leaf entries, and collect them into a sorted `BTreeMap`.
//!
//! [`build_prolly_tree`]: super::build::build_prolly_tree

use std::collections::BTreeMap;

use ferratom::error::FerraError;

use crate::prolly::{
    build::{decode_child_addrs, deserialize_leaf_chunk},
    chunk::{Chunk, ChunkStore, Hash},
};

/// Read all key-value pairs from a prolly tree rooted at `root`.
///
/// `INV-FERR-049` (Snapshot = Root Hash): This is the inverse of
/// [`build_prolly_tree`](super::build::build_prolly_tree). For any
/// key-value set `kvs`:
///
/// ```text
/// read_prolly_tree(store, build_prolly_tree(kvs, store, pw)) = kvs
/// ```
///
/// The function recursively descends from the root chunk to leaf chunks,
/// deserializes each leaf via [`deserialize_leaf_chunk`], and collects
/// all entries into a `BTreeMap` sorted by key.
///
/// # Errors
///
/// Returns `FerraError` if:
/// - A chunk referenced by the tree is missing from the store
///   (`InvariantViolation` — the store must contain all chunks per
///   INV-FERR-049 precondition)
/// - A chunk fails deserialization (`TruncatedChunk`, `UnknownCodecTag`, etc.)
/// - The underlying store operation fails
pub fn read_prolly_tree(
    root: &Hash,
    chunk_store: &dyn ChunkStore,
) -> Result<BTreeMap<Vec<u8>, Vec<u8>>, FerraError> {
    let mut result = BTreeMap::new();
    read_subtree(root, chunk_store, &mut result)?;
    Ok(result)
}

/// Key-value entry type for the Vec-based read path.
type KvEntry = (Vec<u8>, Vec<u8>);

/// Read all key-value pairs into a sorted `Vec` — O(n) collection.
///
/// Like [`read_prolly_tree`] but avoids the O(n log n) `BTreeMap` insertion
/// cost. The prolly tree's DFS traversal visits leaves in sorted key order
/// (because the tree was built from sorted input and leaf chunks are
/// ordered by the Gear hash boundary function). The output Vec is already
/// sorted — no re-sorting needed.
///
/// At 10M entries: ~3s (Vec) vs ~26s (`BTreeMap`). The difference is
/// allocation + tree balancing overhead in `BTreeMap`.
///
/// # Errors
///
/// Same as [`read_prolly_tree`].
pub fn read_prolly_tree_vec(
    root: &Hash,
    chunk_store: &dyn ChunkStore,
) -> Result<Vec<KvEntry>, FerraError> {
    let mut result = Vec::new();
    read_subtree_vec(root, chunk_store, &mut result)?;
    Ok(result)
}

/// Recursively read a subtree into a Vec (O(n) append).
fn read_subtree_vec(
    addr: &Hash,
    chunk_store: &dyn ChunkStore,
    result: &mut Vec<KvEntry>,
) -> Result<(), FerraError> {
    let chunk = load_chunk(addr, chunk_store)?;
    let data = chunk.data();

    if data.is_empty() {
        return Err(FerraError::EmptyChunk);
    }

    match data[0] {
        0x01 => {
            let entries = deserialize_leaf_chunk(data)?;
            result.extend(entries);
            Ok(())
        }
        0x02 => {
            let children = decode_child_addrs(&chunk)?;
            for child_addr in &children {
                read_subtree_vec(child_addr, chunk_store, result)?;
            }
            Ok(())
        }
        tag => Err(FerraError::UnknownCodecTag(tag)),
    }
}

/// Load a chunk from the store, returning `InvariantViolation` if missing.
///
/// `INV-FERR-049` precondition: the store must contain all chunks
/// reachable from the root hash. A missing chunk indicates corruption
/// or an incomplete transfer.
fn load_chunk(addr: &Hash, store: &dyn ChunkStore) -> Result<Chunk, FerraError> {
    store
        .get_chunk(addr)?
        .ok_or_else(|| FerraError::InvariantViolation {
            invariant: "INV-FERR-049".into(),
            details: format!(
                "chunk {:02x}{:02x}{:02x}{:02x}... not found during tree traversal",
                addr[0], addr[1], addr[2], addr[3],
            ),
        })
}

/// Recursively read a subtree rooted at `addr` into `result`.
fn read_subtree(
    addr: &Hash,
    chunk_store: &dyn ChunkStore,
    result: &mut BTreeMap<Vec<u8>, Vec<u8>>,
) -> Result<(), FerraError> {
    let chunk = load_chunk(addr, chunk_store)?;
    let data = chunk.data();

    if data.is_empty() {
        return Err(FerraError::EmptyChunk);
    }

    match data[0] {
        // Leaf chunk: deserialize and collect entries
        0x01 => {
            let entries = deserialize_leaf_chunk(data)?;
            for (key, value) in entries {
                result.insert(key, value);
            }
            Ok(())
        }
        // Internal chunk: decode child addresses and recurse
        0x02 => {
            let children = decode_child_addrs(&chunk)?;
            for child_addr in &children {
                read_subtree(child_addr, chunk_store, result)?;
            }
            Ok(())
        }
        tag => Err(FerraError::UnknownCodecTag(tag)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prolly::{
        boundary::DEFAULT_PATTERN_WIDTH,
        build::build_prolly_tree,
        chunk::{Chunk, MemoryChunkStore},
    };

    #[test]
    fn test_inv_ferr_049_empty_roundtrip() {
        let store = MemoryChunkStore::new();
        let kvs: BTreeMap<Vec<u8>, Vec<u8>> = BTreeMap::new();
        let root =
            build_prolly_tree(&kvs, &store, DEFAULT_PATTERN_WIDTH).expect("build empty tree");

        let result = read_prolly_tree(&root, &store).expect("read empty tree");
        assert_eq!(result, kvs, "INV-FERR-049: empty tree roundtrip");
    }

    #[test]
    fn test_inv_ferr_049_single_entry_roundtrip() {
        let store = MemoryChunkStore::new();
        let mut kvs = BTreeMap::new();
        kvs.insert(vec![1u8, 2, 3], vec![10u8, 20]);

        let root =
            build_prolly_tree(&kvs, &store, DEFAULT_PATTERN_WIDTH).expect("build single entry");

        let result = read_prolly_tree(&root, &store).expect("read single entry");
        assert_eq!(result, kvs, "INV-FERR-049: single entry roundtrip");
    }

    #[test]
    fn test_inv_ferr_049_multi_entry_roundtrip() {
        let store = MemoryChunkStore::new();
        let mut kvs = BTreeMap::new();
        for i in 0u32..100 {
            kvs.insert(i.to_be_bytes().to_vec(), vec![(i & 0xFF) as u8; 4]);
        }

        let root =
            build_prolly_tree(&kvs, &store, DEFAULT_PATTERN_WIDTH).expect("build 100 entries");

        let result = read_prolly_tree(&root, &store).expect("read 100 entries");
        assert_eq!(result, kvs, "INV-FERR-049: 100-entry roundtrip");
    }

    #[test]
    fn test_inv_ferr_049_large_tree_roundtrip() {
        let store = MemoryChunkStore::new();
        let mut kvs = BTreeMap::new();
        for i in 0u32..500 {
            kvs.insert(i.to_be_bytes().to_vec(), format!("value-{i}").into_bytes());
        }

        let root =
            build_prolly_tree(&kvs, &store, DEFAULT_PATTERN_WIDTH).expect("build 500 entries");

        let result = read_prolly_tree(&root, &store).expect("read 500 entries");
        assert_eq!(
            result.len(),
            kvs.len(),
            "INV-FERR-049: all 500 entries recovered"
        );
        assert_eq!(
            result, kvs,
            "INV-FERR-049: large tree roundtrip exact match"
        );
    }

    #[test]
    fn test_inv_ferr_049_read_deterministic() {
        let store = MemoryChunkStore::new();
        let mut kvs = BTreeMap::new();
        for i in 0u32..50 {
            kvs.insert(i.to_be_bytes().to_vec(), vec![0u8; 8]);
        }

        let root = build_prolly_tree(&kvs, &store, DEFAULT_PATTERN_WIDTH).expect("build");

        let r1 = read_prolly_tree(&root, &store).expect("read 1");
        let r2 = read_prolly_tree(&root, &store).expect("read 2");
        assert_eq!(r1, r2, "INV-FERR-049: reads must be deterministic");
    }

    #[test]
    fn test_read_missing_chunk() {
        let store = MemoryChunkStore::new();
        let bogus_hash = [0xDEu8; 32];

        let err = read_prolly_tree(&bogus_hash, &store).unwrap_err();
        assert!(
            matches!(err, FerraError::InvariantViolation { .. }),
            "missing chunk must return InvariantViolation, got: {err:?}"
        );
    }

    #[test]
    fn test_read_corrupted_tag() {
        let store = MemoryChunkStore::new();
        let bad_data = vec![0xFF, 0, 0, 0, 0];
        let chunk = Chunk::from_bytes(&bad_data);
        let addr = *chunk.addr();
        store.put_chunk(&chunk).expect("put");

        let err = read_prolly_tree(&addr, &store).unwrap_err();
        assert!(
            matches!(err, FerraError::UnknownCodecTag(0xFF)),
            "corrupted chunk must return UnknownCodecTag, got: {err:?}"
        );
    }

    // ── Vec-based read tests ──────────────────────────────────────────

    #[test]
    fn test_inv_ferr_049_vec_matches_btreemap() {
        let store = MemoryChunkStore::new();
        let mut kvs = BTreeMap::new();
        for i in 0u32..500 {
            kvs.insert(i.to_be_bytes().to_vec(), format!("val-{i}").into_bytes());
        }
        let root = build_prolly_tree(&kvs, &store, DEFAULT_PATTERN_WIDTH).expect("build");

        let btree_result = read_prolly_tree(&root, &store).expect("btree read");
        let vec_result = read_prolly_tree_vec(&root, &store).expect("vec read");

        // Vec should be sorted (DFS visits leaves in order)
        for window in vec_result.windows(2) {
            assert!(
                window[0].0 < window[1].0,
                "Vec read must produce sorted output"
            );
        }

        // Vec entries should match BTreeMap entries exactly
        let btree_vec: Vec<(Vec<u8>, Vec<u8>)> = btree_result.into_iter().collect();
        assert_eq!(
            vec_result, btree_vec,
            "INV-FERR-049: Vec read must match BTreeMap read"
        );
    }

    #[test]
    fn test_inv_ferr_049_vec_empty() {
        let store = MemoryChunkStore::new();
        let kvs: BTreeMap<Vec<u8>, Vec<u8>> = BTreeMap::new();
        let root = build_prolly_tree(&kvs, &store, DEFAULT_PATTERN_WIDTH).expect("build");

        let result = read_prolly_tree_vec(&root, &store).expect("vec read");
        assert!(result.is_empty(), "empty tree vec read");
    }
}
