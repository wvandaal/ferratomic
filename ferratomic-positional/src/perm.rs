//! Cache-oblivious permutation layout (INV-FERR-071) and permutation construction (INV-FERR-073/076).
//!
//! Phase 4a: Eytzinger (BFS) layout. Future: vEB swap (change this file only).
//!
//! Eytzinger layout stores a sorted sequence in BFS (breadth-first) order of
//! an implicit binary search tree. This eliminates pointer chasing and enables
//! cache-oblivious search: the memory access pattern is a root-to-leaf path
//! in a complete binary tree, which is optimal for any cache line size.
//!
//! Convention: 1-based indexing. `bfs[0]` is a sentinel (`u32::MAX`).
//! Root at index 1, left child at `2*i`, right child at `2*i + 1`.

use ferratom::Datom;

/// Build a permutation array by sorting indices by a key extractor.
///
/// `perm[i]` = canonical position of the i-th element in alternate order.
/// O(n log n) sort on u32 indices -- cache-optimal.
pub(crate) fn build_permutation<F, K: Ord>(canonical: &[Datom], key_fn: F) -> Vec<u32>
where
    F: Fn(&Datom) -> K,
{
    let mut perm: Vec<u32> = (0..canonical.len())
        .map(|i| u32::try_from(i).unwrap_or(u32::MAX))
        .collect();
    perm.sort_unstable_by(|&a, &b| {
        let da = &canonical[a as usize];
        let db = &canonical[b as usize];
        key_fn(da).cmp(&key_fn(db))
    });
    perm
}

/// Rearrange a sorted `u32` permutation into Eytzinger (BFS) order.
///
/// Output has `n + 1` elements: index 0 = `u32::MAX` sentinel.
/// O(n) time via in-order traversal of the implicit binary tree.
///
/// INV-FERR-071: cache-oblivious layout for permutation arrays.
#[must_use]
pub(crate) fn layout_permutation(sorted: &[u32]) -> Vec<u32> {
    let n = sorted.len();
    let mut bfs = vec![u32::MAX; n + 1]; // index 0 = sentinel
    let mut src = 0usize;
    fill_bfs(sorted, &mut bfs, &mut src, 1);
    debug_assert_eq!(src, n, "eytzinger: must consume all {n} elements");
    bfs
}

/// Recursive in-order fill of the BFS array.
///
/// Visits the implicit binary tree in-order (left, root, right),
/// assigning sorted elements to BFS positions.
fn fill_bfs(sorted: &[u32], bfs: &mut [u32], src: &mut usize, node: usize) {
    if node >= bfs.len() {
        return;
    }
    fill_bfs(sorted, bfs, src, 2 * node); // left subtree
    if *src < sorted.len() {
        bfs[node] = sorted[*src];
        *src += 1;
    }
    fill_bfs(sorted, bfs, src, 2 * node + 1); // right subtree
}

/// Search an Eytzinger-laid-out permutation array.
///
/// Returns the datom at the found canonical position, or `None`.
/// O(log n) with cache-oblivious access pattern (INV-FERR-071).
#[must_use]
pub(crate) fn layout_search<'a, F>(
    bfs_perm: &[u32],
    canonical: &'a [Datom],
    cmp_fn: F,
) -> Option<&'a Datom>
where
    F: Fn(&Datom) -> std::cmp::Ordering,
{
    let n = bfs_perm.len().saturating_sub(1);
    if n == 0 {
        return None;
    }
    let mut node = 1usize;
    while node <= n {
        let pos = bfs_perm[node] as usize;
        match cmp_fn(&canonical[pos]) {
            std::cmp::Ordering::Equal => return Some(&canonical[pos]),
            std::cmp::Ordering::Less => node = 2 * node + 1,
            std::cmp::Ordering::Greater => node *= 2,
        }
    }
    None
}

/// Recover sorted order from Eytzinger layout via in-order traversal.
///
/// O(n) time. Used for checkpoint serialization, which requires the
/// original sorted permutation (INV-FERR-071).
#[must_use]
pub(crate) fn layout_to_sorted(bfs_perm: &[u32]) -> Vec<u32> {
    let n = bfs_perm.len().saturating_sub(1);
    let mut sorted = Vec::with_capacity(n);
    inorder_collect(bfs_perm, &mut sorted, 1);
    sorted
}

/// Recursive in-order collection from the BFS array.
fn inorder_collect(bfs: &[u32], out: &mut Vec<u32>, node: usize) {
    if node >= bfs.len() {
        return;
    }
    inorder_collect(bfs, out, 2 * node); // left
    out.push(bfs[node]);
    inorder_collect(bfs, out, 2 * node + 1); // right
}
