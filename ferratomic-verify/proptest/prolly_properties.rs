//! Property-based tests for the prolly tree module.
//!
//! Tests INV-FERR-046 (history independence), INV-FERR-047 (diff correctness),
//! INV-FERR-049 (snapshot roundtrip) with random inputs at 10K proptest cases.

use std::collections::BTreeMap;

use ferratomic_db::prolly::{
    boundary::DEFAULT_PATTERN_WIDTH,
    build::build_prolly_tree,
    chunk::MemoryChunkStore,
    diff::{diff, diff_exact, DiffEntry},
    read::read_prolly_tree,
    snapshot::{create_manifest, resolve_manifest, RootSet},
    transfer::{ChunkTransfer, RecursiveTransfer},
};
use proptest::prelude::*;

/// Strategy: generate a BTreeMap<Vec<u8>, Vec<u8>> with 0..max_entries entries.
fn arb_kvs(max_entries: usize) -> impl Strategy<Value = BTreeMap<Vec<u8>, Vec<u8>>> {
    prop::collection::btree_map(
        prop::collection::vec(any::<u8>(), 1..32),
        prop::collection::vec(any::<u8>(), 1..64),
        0..max_entries,
    )
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(
        std::env::var("PROPTEST_CASES")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(10_000)
    ))]

    /// INV-FERR-049: build → read roundtrip is lossless.
    ///
    /// For any key-value set, `read_prolly_tree(build_prolly_tree(kvs)) == kvs`.
    #[test]
    fn prolly_build_read_roundtrip(kvs in arb_kvs(200)) {
        let store = MemoryChunkStore::new();
        let root = build_prolly_tree(&kvs, &store, DEFAULT_PATTERN_WIDTH)
            .expect("build must succeed");
        let recovered = read_prolly_tree(&root, &store)
            .expect("read must succeed");
        prop_assert_eq!(&recovered, &kvs,
            "INV-FERR-049: build→read roundtrip lost {} entries (built {}, read {})",
            kvs.len().abs_diff(recovered.len()), kvs.len(), recovered.len());
    }

    /// INV-FERR-046: history independence — same key-value set always
    /// produces the same root hash regardless of store instance.
    #[test]
    fn prolly_history_independence(kvs in arb_kvs(200)) {
        let store1 = MemoryChunkStore::new();
        let store2 = MemoryChunkStore::new();
        let root1 = build_prolly_tree(&kvs, &store1, DEFAULT_PATTERN_WIDTH)
            .expect("build1");
        let root2 = build_prolly_tree(&kvs, &store2, DEFAULT_PATTERN_WIDTH)
            .expect("build2");
        prop_assert_eq!(root1, root2,
            "INV-FERR-046: same kvs must produce identical root hashes");
    }

    /// INV-FERR-047: diff completeness — every actual change appears in
    /// the diff output (no false negatives).
    ///
    /// NOTE: The diff may produce EXTRA entries ("phantom diffs") when
    /// chunk boundaries shift between two trees. This is a known
    /// limitation of separator-based merge_join_children — entries that
    /// move between chunks appear as both LeftOnly and RightOnly even
    /// though their key-value is unchanged. Filed as bd-PHANTOM-DIFF
    /// for future fix (post-process deduplication or leaf-level merge).
    /// The completeness property (no missed changes) is verified here.
    #[test]
    fn prolly_diff_completeness(
        base in arb_kvs(100),
        changes in arb_kvs(30),
    ) {
        let store = MemoryChunkStore::new();

        let root1 = build_prolly_tree(&base, &store, DEFAULT_PATTERN_WIDTH)
            .expect("build base");

        // Apply changes to base
        let mut modified = base.clone();
        for (k, v) in &changes {
            modified.insert(k.clone(), v.clone());
        }
        let root2 = build_prolly_tree(&modified, &store, DEFAULT_PATTERN_WIDTH)
            .expect("build modified");

        // Collect diff
        let diff_entries: Vec<DiffEntry> = diff(&root1, &root2, &store)
            .collect::<Result<Vec<_>, _>>()
            .expect("diff must succeed");

        // Build a set of keys that appear in the diff
        let diff_keys: std::collections::BTreeSet<Vec<u8>> = diff_entries
            .iter()
            .map(|e| match e {
                DiffEntry::LeftOnly { key, .. }
                | DiffEntry::RightOnly { key, .. }
                | DiffEntry::Modified { key, .. } => key.clone(),
            })
            .collect();

        // Verify completeness: every actually-changed key appears in diff
        for (k, v) in &base {
            if let Some(mv) = modified.get(k) {
                if mv != v {
                    prop_assert!(diff_keys.contains(k),
                        "INV-FERR-047 completeness: modified key {:?} missing from diff", k);
                }
            }
            // No keys are removed from base in this test (only inserts)
        }
        for k in modified.keys() {
            if !base.contains_key(k) {
                prop_assert!(diff_keys.contains(k),
                    "INV-FERR-047 completeness: added key {:?} missing from diff", k);
            }
        }

        // Verify diff is non-empty when trees differ
        if root1 != root2 {
            prop_assert!(!diff_entries.is_empty(),
                "INV-FERR-047: different roots must produce non-empty diff");
        }
    }

    /// INV-FERR-047: diff_exact produces EXACTLY the symmetric difference.
    /// No phantom entries, no false positives.
    #[test]
    fn prolly_diff_exact_correctness(
        base in arb_kvs(100),
        changes in arb_kvs(30),
    ) {
        let store = MemoryChunkStore::new();
        let root1 = build_prolly_tree(&base, &store, DEFAULT_PATTERN_WIDTH)
            .expect("build base");

        let mut modified = base.clone();
        for (k, v) in &changes {
            modified.insert(k.clone(), v.clone());
        }
        let root2 = build_prolly_tree(&modified, &store, DEFAULT_PATTERN_WIDTH)
            .expect("build modified");

        let diff_entries = diff_exact(&root1, &root2, &store)
            .expect("diff_exact must succeed");

        // Compute expected symmetric difference
        let mut expected_count = 0usize;
        for (k, v) in &base {
            if let Some(mv) = modified.get(k) {
                if mv != v {
                    expected_count += 1; // Modified
                }
            }
        }
        for k in modified.keys() {
            if !base.contains_key(k) {
                expected_count += 1; // RightOnly
            }
        }

        prop_assert_eq!(diff_entries.len(), expected_count,
            "INV-FERR-047: diff_exact should produce exactly {} entries, got {}",
            expected_count, diff_entries.len());
    }

    /// INV-FERR-047: diff of identical trees is empty (O(1) fast path).
    #[test]
    fn prolly_diff_identical_is_empty(kvs in arb_kvs(200)) {
        let store = MemoryChunkStore::new();
        let root = build_prolly_tree(&kvs, &store, DEFAULT_PATTERN_WIDTH)
            .expect("build");
        let entries: Vec<DiffEntry> = diff(&root, &root, &store)
            .collect::<Result<Vec<_>, _>>()
            .expect("diff must succeed");
        prop_assert!(entries.is_empty(),
            "INV-FERR-047: diff(T, T) must be empty, got {} entries",
            entries.len());
    }

    /// INV-FERR-049: manifest roundtrip — create → resolve is lossless.
    #[test]
    fn prolly_manifest_roundtrip(
        primary in any::<[u8; 32]>(),
        eavt in any::<[u8; 32]>(),
        aevt in any::<[u8; 32]>(),
        vaet in any::<[u8; 32]>(),
        avet in any::<[u8; 32]>(),
    ) {
        let rs = RootSet { primary, eavt, aevt, vaet, avet };
        let store = MemoryChunkStore::new();
        let manifest = create_manifest(&rs, &store).expect("create manifest");
        let recovered = resolve_manifest(&manifest, &store).expect("resolve manifest");
        prop_assert_eq!(recovered, rs,
            "INV-FERR-049: manifest roundtrip must be lossless");
    }

    /// INV-FERR-048: transfer is complete — after transfer, read from
    /// dst produces the same data as read from src.
    #[test]
    fn prolly_transfer_complete(kvs in arb_kvs(100)) {
        let src = MemoryChunkStore::new();
        let dst = MemoryChunkStore::new();
        let root = build_prolly_tree(&kvs, &src, DEFAULT_PATTERN_WIDTH)
            .expect("build");

        let xfer = RecursiveTransfer;
        xfer.transfer(&src, &dst, &root).expect("transfer");

        let from_dst = read_prolly_tree(&root, &dst).expect("read from dst");
        prop_assert_eq!(&from_dst, &kvs,
            "INV-FERR-048: transfer must make full tree accessible from dst");
    }

    /// INV-FERR-048: transfer is idempotent — second transfer sends nothing.
    #[test]
    fn prolly_transfer_idempotent(kvs in arb_kvs(100)) {
        let src = MemoryChunkStore::new();
        let dst = MemoryChunkStore::new();
        let root = build_prolly_tree(&kvs, &src, DEFAULT_PATTERN_WIDTH)
            .expect("build");

        let xfer = RecursiveTransfer;
        xfer.transfer(&src, &dst, &root).expect("first transfer");
        let r2 = xfer.transfer(&src, &dst, &root).expect("second transfer");
        prop_assert_eq!(r2.chunks_transferred, 0,
            "INV-FERR-048: second transfer must send zero chunks");
    }
}
