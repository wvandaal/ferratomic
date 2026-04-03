//! Cache-oblivious permutation layout (INV-FERR-071).
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

// ---------------------------------------------------------------------------
// Tests (INV-FERR-071)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use ferratom::{Attribute, Datom, EntityId, Op, TxId, Value};

    use super::{layout_permutation, layout_search, layout_to_sorted};

    /// Helper: build a datom with a specific entity content for ordering.
    fn make_datom(content: &[u8]) -> Datom {
        Datom::new(
            EntityId::from_content(content),
            Attribute::from("db/doc"),
            Value::Bool(true),
            TxId::new(0, 1, 0),
            Op::Assert,
        )
    }

    /// Empty input produces a single-element sentinel array.
    /// Search on empty returns None.
    #[test]
    fn test_eytzinger_empty() {
        let result = layout_permutation(&[]);
        assert_eq!(
            result.len(),
            1,
            "INV-FERR-071: empty layout has sentinel only"
        );
        assert_eq!(result[0], u32::MAX, "INV-FERR-071: sentinel is u32::MAX");

        // Search on empty Eytzinger array returns None.
        let canonical: Vec<Datom> = Vec::new();
        let found = layout_search(&result, &canonical, |_d| std::cmp::Ordering::Equal);
        assert!(
            found.is_none(),
            "INV-FERR-071: search on empty returns None"
        );
    }

    /// Single element: [MAX, 0]. Search finds the element.
    #[test]
    fn test_eytzinger_single() {
        let result = layout_permutation(&[0]);
        assert_eq!(
            result,
            vec![u32::MAX, 0],
            "INV-FERR-071: single element layout"
        );

        // Search for the single element.
        let d = make_datom(b"alpha");
        let canonical = vec![d.clone()];
        let found = layout_search(&result, &canonical, |datom| datom.cmp(&d));
        assert!(found.is_some(), "INV-FERR-071: search finds single element");
    }

    /// Seven elements form a perfect binary tree of depth 3.
    ///
    /// Sorted: [0, 1, 2, 3, 4, 5, 6]
    /// BFS:    [MAX, 3, 1, 5, 0, 2, 4, 6]
    ///
    /// Tree:       3
    ///           /   \
    ///          1     5
    ///         / \   / \
    ///        0   2 4   6
    #[test]
    fn test_eytzinger_seven() {
        let sorted: Vec<u32> = (0..7).collect();
        let result = layout_permutation(&sorted);
        assert_eq!(
            result,
            vec![u32::MAX, 3, 1, 5, 0, 2, 4, 6],
            "INV-FERR-071: perfect binary tree BFS order"
        );
    }

    /// Round-trip: `layout_to_sorted(layout_permutation(sorted)) == sorted`.
    #[test]
    fn test_eytzinger_roundtrip() {
        for n in 0..=20 {
            let sorted: Vec<u32> = (0..n).collect();
            let bfs = layout_permutation(&sorted);
            let recovered = layout_to_sorted(&bfs);
            assert_eq!(
                recovered, sorted,
                "INV-FERR-071: round-trip failed for n={n}"
            );
        }
    }
}
