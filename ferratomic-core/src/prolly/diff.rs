//! O(d) prolly tree diff via depth-first hash comparison.
//!
//! `INV-FERR-047`: `diff(root1, root2)` visits `O(d * log_k(n))` nodes
//! where `d` = number of changed key-value pairs, `k` = chunk fanout,
//! `n` = max store size. Identical subtrees are skipped in O(1) by
//! comparing chunk hashes before loading chunk contents.
//!
//! The diff algorithm is the core of federation efficiency (spec §23.8).
//! When a peer requests "what changed since version V?", the answer is
//! `diff(root_V, root_current)` — cost proportional to changes, not
//! store size.

use std::{cmp::Ordering, collections::VecDeque};

use ferratom::error::FerraError;

use crate::prolly::{
    build::{decode_child_addrs, decode_internal_children, deserialize_leaf_chunk},
    chunk::{Chunk, ChunkStore, Hash},
};

/// A single diff entry: a key-value pair that differs between two trees.
///
/// `INV-FERR-047`: The complete set of `DiffEntry` items from a diff
/// equals the symmetric difference `KV1 symmetric_diff KV2`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffEntry {
    /// Key-value exists in left tree only.
    LeftOnly {
        /// The key bytes.
        key: Vec<u8>,
        /// The value bytes.
        value: Vec<u8>,
    },
    /// Key-value exists in right tree only.
    RightOnly {
        /// The key bytes.
        key: Vec<u8>,
        /// The value bytes.
        value: Vec<u8>,
    },
    /// Key exists in both trees but values differ.
    Modified {
        /// The key bytes.
        key: Vec<u8>,
        /// The value in the left tree.
        left_value: Vec<u8>,
        /// The value in the right tree.
        right_value: Vec<u8>,
    },
}

/// Compute the diff between two prolly tree roots.
///
/// Returns a lazy iterator over [`DiffEntry`] items. If the roots are
/// equal, the iterator yields nothing (O(1) fast path).
///
/// `INV-FERR-047`: The diff produces exactly `KV1 symmetric_diff KV2`.
/// No changes are missed. No false changes are reported.
///
/// # Root parameter scope
///
/// `root1` and `root2` are **tree roots** (the root chunk address of
/// one prolly tree), NOT manifest hashes. See INV-FERR-049 for the
/// manifest → `RootSet` → tree root resolution protocol.
#[must_use]
pub fn diff<'a>(root1: &Hash, root2: &Hash, chunk_store: &'a dyn ChunkStore) -> DiffIterator<'a> {
    if root1 == root2 {
        DiffIterator {
            stack: Vec::new(),
            pending: VecDeque::new(),
            store: chunk_store,
        }
    } else {
        DiffIterator {
            stack: vec![DiffStackEntry::Compare(*root1, *root2)],
            pending: VecDeque::new(),
            store: chunk_store,
        }
    }
}

/// Stack entry for the depth-first diff walk.
///
/// Three variants handle one-sided subtrees (F01 fix from spec audit):
/// `LeftOnly`/`RightOnly` replace the original sentinel-hash approach
/// that caused `ChunkNotFound` errors.
enum DiffStackEntry {
    Compare(Hash, Hash),
    LeftOnly(Hash),
    RightOnly(Hash),
}

/// Which side of the diff a one-sided subtree belongs to.
#[derive(Clone, Copy)]
enum DiffSide {
    Left,
    Right,
}

/// Lazy iterator over diff entries between two prolly trees.
///
/// Maintains a stack of subtree comparisons and a buffer of pending
/// leaf-level diffs. Processes one stack entry per `advance()` call,
/// yielding buffered entries first.
pub struct DiffIterator<'a> {
    stack: Vec<DiffStackEntry>,
    pending: VecDeque<DiffEntry>,
    store: &'a dyn ChunkStore,
}

impl DiffIterator<'_> {
    /// Core iteration logic. Returns `Ok(Some(entry))` when a diff entry
    /// is ready, `Ok(None)` when the diff is exhausted, or `Err` on
    /// chunk load/deserialization failure.
    fn advance(&mut self) -> Result<Option<DiffEntry>, FerraError> {
        loop {
            if let Some(entry) = self.pending.pop_front() {
                return Ok(Some(entry));
            }

            let Some(stack_entry) = self.stack.pop() else {
                return Ok(None);
            };

            match stack_entry {
                DiffStackEntry::LeftOnly(hash) => {
                    self.enumerate_subtree(hash, DiffSide::Left)?;
                }
                DiffStackEntry::RightOnly(hash) => {
                    self.enumerate_subtree(hash, DiffSide::Right)?;
                }
                DiffStackEntry::Compare(left_hash, right_hash) => {
                    if left_hash == right_hash {
                        continue;
                    }
                    self.compare_chunks(left_hash, right_hash)?;
                }
            }
        }
    }

    /// Enumerate ALL entries in a subtree as one-sided diffs.
    ///
    /// Leaf chunks: deserialize and emit each entry as `LeftOnly`/`RightOnly`.
    /// Internal chunks: push children onto the stack for further descent
    /// (in reverse order for left-to-right DFS traversal).
    fn enumerate_subtree(&mut self, hash: Hash, side: DiffSide) -> Result<(), FerraError> {
        let chunk = load_chunk(&hash, self.store)?;
        let tag = chunk_tag(&chunk)?;

        if tag == 0x01 {
            let entries = deserialize_leaf_chunk(chunk.data())?;
            for (k, v) in entries {
                self.pending.push_back(match side {
                    DiffSide::Left => DiffEntry::LeftOnly { key: k, value: v },
                    DiffSide::Right => DiffEntry::RightOnly { key: k, value: v },
                });
            }
        } else {
            let children = decode_child_addrs(&chunk)?;
            for child in children.iter().rev() {
                self.stack.push(match side {
                    DiffSide::Left => DiffStackEntry::LeftOnly(*child),
                    DiffSide::Right => DiffStackEntry::RightOnly(*child),
                });
            }
        }
        Ok(())
    }

    /// Compare two chunks that have different hashes.
    ///
    /// Dispatches on chunk types:
    /// - Both leaves: sorted merge-diff of entries
    /// - Both internal: merge-join children by separator key
    /// - Cross-height (F02 fix): enumerate leaf as one-sided, push
    ///   internal children for further descent
    fn compare_chunks(&mut self, left_hash: Hash, right_hash: Hash) -> Result<(), FerraError> {
        let left_chunk = load_chunk(&left_hash, self.store)?;
        let right_chunk = load_chunk(&right_hash, self.store)?;

        let left_is_leaf = chunk_tag(&left_chunk)? == 0x01;
        let right_is_leaf = chunk_tag(&right_chunk)? == 0x01;

        match (left_is_leaf, right_is_leaf) {
            (true, true) => {
                let left_entries = deserialize_leaf_chunk(left_chunk.data())?;
                let right_entries = deserialize_leaf_chunk(right_chunk.data())?;
                diff_sorted_entries(&left_entries, &right_entries, &mut self.pending);
            }
            (false, false) => {
                let left_children = decode_internal_children(&left_chunk)?;
                let right_children = decode_internal_children(&right_chunk)?;
                merge_join_children(&left_children, &right_children, &mut self.stack);
            }
            (true, false) => {
                // Left is leaf, right is internal (F02 cross-height fix)
                let leaf_entries = deserialize_leaf_chunk(left_chunk.data())?;
                for (k, v) in leaf_entries {
                    self.pending
                        .push_back(DiffEntry::LeftOnly { key: k, value: v });
                }
                let children = decode_child_addrs(&right_chunk)?;
                for child in children.iter().rev() {
                    self.stack.push(DiffStackEntry::RightOnly(*child));
                }
            }
            (false, true) => {
                // Left is internal, right is leaf (F02 cross-height fix)
                let leaf_entries = deserialize_leaf_chunk(right_chunk.data())?;
                for (k, v) in leaf_entries {
                    self.pending
                        .push_back(DiffEntry::RightOnly { key: k, value: v });
                }
                let children = decode_child_addrs(&left_chunk)?;
                for child in children.iter().rev() {
                    self.stack.push(DiffStackEntry::LeftOnly(*child));
                }
            }
        }
        Ok(())
    }
}

impl Iterator for DiffIterator<'_> {
    type Item = Result<DiffEntry, FerraError>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.advance() {
            Ok(Some(entry)) => Some(Ok(entry)),
            Ok(None) => None,
            Err(e) => Some(Err(e)),
        }
    }
}

/// Load a chunk from the store, returning `InvariantViolation` if missing.
fn load_chunk(addr: &Hash, store: &dyn ChunkStore) -> Result<Chunk, FerraError> {
    store
        .get_chunk(addr)?
        .ok_or_else(|| FerraError::InvariantViolation {
            invariant: "INV-FERR-047".into(),
            details: format!(
                "chunk {:02x}{:02x}{:02x}{:02x}... not found during diff traversal",
                addr[0], addr[1], addr[2], addr[3],
            ),
        })
}

/// Validate and return the chunk type tag (0x01=leaf, 0x02=internal).
fn chunk_tag(chunk: &Chunk) -> Result<u8, FerraError> {
    let data = chunk.data();
    let tag = *data.first().ok_or(FerraError::EmptyChunk)?;
    if tag != 0x01 && tag != 0x02 {
        return Err(FerraError::UnknownCodecTag(tag));
    }
    Ok(tag)
}

/// Sorted merge-diff of two leaf entry lists.
///
/// Produces `DiffEntry` items for every entry that exists in only one
/// list or has different values. O(|left| + |right|) — single pass.
fn diff_sorted_entries(
    left: &[(Vec<u8>, Vec<u8>)],
    right: &[(Vec<u8>, Vec<u8>)],
    out: &mut VecDeque<DiffEntry>,
) {
    let mut l = left.iter().peekable();
    let mut r = right.iter().peekable();

    loop {
        match (l.peek(), r.peek()) {
            (Some((lk, lv)), Some((rk, rv))) => match lk.cmp(rk) {
                Ordering::Less => {
                    out.push_back(DiffEntry::LeftOnly {
                        key: (*lk).clone(),
                        value: (*lv).clone(),
                    });
                    l.next();
                }
                Ordering::Greater => {
                    out.push_back(DiffEntry::RightOnly {
                        key: (*rk).clone(),
                        value: (*rv).clone(),
                    });
                    r.next();
                }
                Ordering::Equal => {
                    if lv != rv {
                        out.push_back(DiffEntry::Modified {
                            key: (*lk).clone(),
                            left_value: (*lv).clone(),
                            right_value: (*rv).clone(),
                        });
                    }
                    l.next();
                    r.next();
                }
            },
            (Some((lk, lv)), None) => {
                out.push_back(DiffEntry::LeftOnly {
                    key: (*lk).clone(),
                    value: (*lv).clone(),
                });
                l.next();
            }
            (None, Some((rk, rv))) => {
                out.push_back(DiffEntry::RightOnly {
                    key: (*rk).clone(),
                    value: (*rv).clone(),
                });
                r.next();
            }
            (None, None) => break,
        }
    }
}

/// Merge-join two internal node children lists by separator key.
///
/// For each separator appearing in both sides with different hashes:
/// push a `Compare` entry. For separators in only one side: push
/// `LeftOnly`/`RightOnly` for recursive one-sided enumeration.
/// Identical hashes are skipped (O(1) structural sharing).
fn merge_join_children(
    left: &[(Vec<u8>, Hash)],
    right: &[(Vec<u8>, Hash)],
    stack: &mut Vec<DiffStackEntry>,
) {
    let mut l = left.iter().peekable();
    let mut r = right.iter().peekable();

    loop {
        match (l.peek(), r.peek()) {
            (Some((lk, lh)), Some((rk, rh))) => match lk.cmp(rk) {
                Ordering::Less => {
                    stack.push(DiffStackEntry::LeftOnly(*lh));
                    l.next();
                }
                Ordering::Greater => {
                    stack.push(DiffStackEntry::RightOnly(*rh));
                    r.next();
                }
                Ordering::Equal => {
                    if lh != rh {
                        stack.push(DiffStackEntry::Compare(*lh, *rh));
                    }
                    l.next();
                    r.next();
                }
            },
            (Some((_, lh)), None) => {
                stack.push(DiffStackEntry::LeftOnly(*lh));
                l.next();
            }
            (None, Some((_, rh))) => {
                stack.push(DiffStackEntry::RightOnly(*rh));
                r.next();
            }
            (None, None) => break,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::prolly::{
        boundary::DEFAULT_PATTERN_WIDTH, build::build_prolly_tree, chunk::MemoryChunkStore,
    };

    /// Collect all diff entries, panicking on any error.
    fn collect_diff(root1: &Hash, root2: &Hash, store: &dyn ChunkStore) -> Vec<DiffEntry> {
        let mut entries: Vec<DiffEntry> = diff(root1, root2, store)
            .collect::<Result<Vec<_>, _>>()
            .expect("diff must not fail");
        entries.sort_by(|a, b| diff_key(a).cmp(diff_key(b)));
        entries
    }

    fn diff_key(entry: &DiffEntry) -> &[u8] {
        match entry {
            DiffEntry::LeftOnly { key, .. }
            | DiffEntry::RightOnly { key, .. }
            | DiffEntry::Modified { key, .. } => key,
        }
    }

    /// Build a tree from a `BTreeMap`, returning its root hash.
    fn build(kvs: &BTreeMap<Vec<u8>, Vec<u8>>, store: &MemoryChunkStore) -> Hash {
        build_prolly_tree(kvs, store, DEFAULT_PATTERN_WIDTH).expect("build")
    }

    #[test]
    fn test_inv_ferr_047_identical_trees_empty_diff() {
        let store = MemoryChunkStore::new();
        let mut kvs = BTreeMap::new();
        for i in 0u32..50 {
            kvs.insert(i.to_be_bytes().to_vec(), vec![0u8; 4]);
        }
        let root = build(&kvs, &store);

        let entries = collect_diff(&root, &root, &store);
        assert!(
            entries.is_empty(),
            "INV-FERR-047: diff of identical roots must be empty (O(1) fast path)"
        );
    }

    #[test]
    fn test_inv_ferr_047_empty_vs_empty() {
        let store = MemoryChunkStore::new();
        let kvs: BTreeMap<Vec<u8>, Vec<u8>> = BTreeMap::new();
        let root = build(&kvs, &store);

        let entries = collect_diff(&root, &root, &store);
        assert!(entries.is_empty(), "empty vs empty = no diff");
    }

    #[test]
    fn test_inv_ferr_047_empty_vs_nonempty() {
        let store = MemoryChunkStore::new();
        let empty: BTreeMap<Vec<u8>, Vec<u8>> = BTreeMap::new();
        let root_empty = build(&empty, &store);

        let mut populated = BTreeMap::new();
        populated.insert(vec![1u8], vec![10u8]);
        populated.insert(vec![2u8], vec![20u8]);
        let root_pop = build(&populated, &store);

        let entries = collect_diff(&root_empty, &root_pop, &store);
        assert_eq!(entries.len(), 2, "empty vs 2 entries = 2 diffs");
        assert!(
            matches!(&entries[0], DiffEntry::RightOnly { key, value }
                if *key == vec![1u8] && *value == vec![10u8]),
            "first entry should be RightOnly(1)"
        );
        assert!(
            matches!(&entries[1], DiffEntry::RightOnly { key, value }
                if *key == vec![2u8] && *value == vec![20u8]),
            "second entry should be RightOnly(2)"
        );
    }

    #[test]
    fn test_inv_ferr_047_single_key_added() {
        let store = MemoryChunkStore::new();
        let mut kvs1 = BTreeMap::new();
        for i in 0u32..10 {
            kvs1.insert(i.to_be_bytes().to_vec(), vec![0u8; 4]);
        }
        let root1 = build(&kvs1, &store);

        let mut kvs2 = kvs1.clone();
        kvs2.insert(100u32.to_be_bytes().to_vec(), vec![99u8; 4]);
        let root2 = build(&kvs2, &store);

        let entries = collect_diff(&root1, &root2, &store);
        assert_eq!(entries.len(), 1, "one key added = one diff entry");
        assert!(
            matches!(&entries[0], DiffEntry::RightOnly { key, value }
                if *key == 100u32.to_be_bytes().to_vec() && *value == vec![99u8; 4]),
            "added key should appear as RightOnly"
        );
    }

    #[test]
    fn test_inv_ferr_047_single_key_removed() {
        let store = MemoryChunkStore::new();
        let mut kvs1 = BTreeMap::new();
        for i in 0u32..10 {
            kvs1.insert(i.to_be_bytes().to_vec(), vec![0u8; 4]);
        }
        let root1 = build(&kvs1, &store);

        let mut kvs2 = kvs1.clone();
        kvs2.remove(5u32.to_be_bytes().as_slice());
        let root2 = build(&kvs2, &store);

        let entries = collect_diff(&root1, &root2, &store);
        assert_eq!(entries.len(), 1, "one key removed = one diff entry");
        assert!(
            matches!(&entries[0], DiffEntry::LeftOnly { key, .. }
                if *key == 5u32.to_be_bytes().to_vec()),
            "removed key should appear as LeftOnly"
        );
    }

    #[test]
    fn test_inv_ferr_047_modified_value() {
        let store = MemoryChunkStore::new();
        let mut kvs1 = BTreeMap::new();
        for i in 0u32..10 {
            kvs1.insert(i.to_be_bytes().to_vec(), vec![0u8; 4]);
        }
        let root1 = build(&kvs1, &store);

        let mut kvs2 = kvs1.clone();
        kvs2.insert(3u32.to_be_bytes().to_vec(), vec![99u8; 4]);
        let root2 = build(&kvs2, &store);

        let entries = collect_diff(&root1, &root2, &store);
        assert_eq!(entries.len(), 1, "one value modified = one diff entry");
        assert!(
            matches!(&entries[0], DiffEntry::Modified { key, left_value, right_value }
                if *key == 3u32.to_be_bytes().to_vec()
                    && *left_value == vec![0u8; 4]
                    && *right_value == vec![99u8; 4]),
            "modified value should appear as Modified"
        );
    }

    #[test]
    fn test_inv_ferr_047_symmetric() {
        let store = MemoryChunkStore::new();
        let mut kvs1 = BTreeMap::new();
        for i in 0u32..20 {
            kvs1.insert(i.to_be_bytes().to_vec(), vec![1u8; 4]);
        }
        let root1 = build(&kvs1, &store);

        let mut kvs2 = BTreeMap::new();
        for i in 10u32..30 {
            kvs2.insert(i.to_be_bytes().to_vec(), vec![2u8; 4]);
        }
        let root2 = build(&kvs2, &store);

        let forward = collect_diff(&root1, &root2, &store);
        let backward = collect_diff(&root2, &root1, &store);

        // Count: symmetric difference check
        let fwd_left = forward
            .iter()
            .filter(|e| matches!(e, DiffEntry::LeftOnly { .. }))
            .count();
        let fwd_right = forward
            .iter()
            .filter(|e| matches!(e, DiffEntry::RightOnly { .. }))
            .count();
        let bwd_left = backward
            .iter()
            .filter(|e| matches!(e, DiffEntry::LeftOnly { .. }))
            .count();
        let bwd_right = backward
            .iter()
            .filter(|e| matches!(e, DiffEntry::RightOnly { .. }))
            .count();

        assert_eq!(
            fwd_left, bwd_right,
            "INV-FERR-047: LeftOnly in forward = RightOnly in backward"
        );
        assert_eq!(
            fwd_right, bwd_left,
            "INV-FERR-047: RightOnly in forward = LeftOnly in backward"
        );
    }

    #[test]
    fn test_inv_ferr_047_large_overlap_few_changes() {
        let store = MemoryChunkStore::new();
        let mut kvs1 = BTreeMap::new();
        for i in 0u32..200 {
            kvs1.insert(i.to_be_bytes().to_vec(), vec![0u8; 8]);
        }
        let root1 = build(&kvs1, &store);

        let mut kvs2 = kvs1.clone();
        // Modify 3 keys, add 1, remove 1
        kvs2.insert(10u32.to_be_bytes().to_vec(), vec![99u8; 8]);
        kvs2.insert(50u32.to_be_bytes().to_vec(), vec![88u8; 8]);
        kvs2.insert(100u32.to_be_bytes().to_vec(), vec![77u8; 8]);
        kvs2.insert(999u32.to_be_bytes().to_vec(), vec![66u8; 8]);
        kvs2.remove(150u32.to_be_bytes().as_slice());
        let root2 = build(&kvs2, &store);

        let entries = collect_diff(&root1, &root2, &store);
        assert_eq!(
            entries.len(),
            5,
            "3 modified + 1 added + 1 removed = 5 diff entries"
        );
    }

    #[test]
    fn test_inv_ferr_047_completely_disjoint() {
        let store = MemoryChunkStore::new();
        let mut kvs1 = BTreeMap::new();
        for i in 0u32..5 {
            kvs1.insert(i.to_be_bytes().to_vec(), vec![1u8]);
        }
        let root1 = build(&kvs1, &store);

        let mut kvs2 = BTreeMap::new();
        for i in 100u32..105 {
            kvs2.insert(i.to_be_bytes().to_vec(), vec![2u8]);
        }
        let root2 = build(&kvs2, &store);

        let entries = collect_diff(&root1, &root2, &store);
        assert_eq!(entries.len(), 10, "5 left + 5 right = 10 diff entries");

        let left_count = entries
            .iter()
            .filter(|e| matches!(e, DiffEntry::LeftOnly { .. }))
            .count();
        let right_count = entries
            .iter()
            .filter(|e| matches!(e, DiffEntry::RightOnly { .. }))
            .count();
        assert_eq!(left_count, 5);
        assert_eq!(right_count, 5);
    }

    #[test]
    fn test_diff_missing_chunk() {
        let store = MemoryChunkStore::new();
        let kvs: BTreeMap<Vec<u8>, Vec<u8>> = BTreeMap::new();
        let root = build(&kvs, &store);
        let bogus = [0xDEu8; 32];

        let result: Result<Vec<_>, _> = diff(&root, &bogus, &store).collect();
        assert!(result.is_err(), "diff with missing chunk must return error");
    }

    // DEFECT-003: multi-level tree diff exercising merge_join_children
    #[test]
    fn test_inv_ferr_047_multi_level_diff() {
        let store = MemoryChunkStore::new();
        // 500 entries with pw=8 (expected chunk ~256) forces multiple chunks
        // and at least one internal node, exercising merge_join_children.
        let mut kvs1 = BTreeMap::new();
        for i in 0u32..500 {
            kvs1.insert(i.to_be_bytes().to_vec(), vec![0u8; 16]);
        }
        let root1 = build(&kvs1, &store);

        // Modify 3 keys scattered across the key range
        let mut kvs2 = kvs1.clone();
        kvs2.insert(50u32.to_be_bytes().to_vec(), vec![0xAAu8; 16]);
        kvs2.insert(250u32.to_be_bytes().to_vec(), vec![0xBBu8; 16]);
        kvs2.insert(450u32.to_be_bytes().to_vec(), vec![0xCCu8; 16]);
        let root2 = build(&kvs2, &store);

        let entries = collect_diff(&root1, &root2, &store);
        assert_eq!(
            entries.len(),
            3,
            "INV-FERR-047: 3 modified keys in multi-level tree = 3 diff entries"
        );
        for entry in &entries {
            assert!(
                matches!(entry, DiffEntry::Modified { .. }),
                "all changes are value modifications, got: {entry:?}"
            );
        }

        // Verify symmetry on multi-level trees
        let backward = collect_diff(&root2, &root1, &store);
        assert_eq!(backward.len(), 3, "reverse diff must also have 3 entries");
    }
}
