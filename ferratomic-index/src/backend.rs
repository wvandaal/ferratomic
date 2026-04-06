//! `IndexBackend` trait and implementations (INV-FERR-025).
//!
//! All index backends are interchangeable — switching backends changes
//! performance characteristics but not correctness. The default is
//! `im::OrdMap` (ADR-FERR-001).

use im::OrdMap;

/// Ordered-map abstraction for secondary index storage.
///
/// INV-FERR-025: all index backends are interchangeable. Switching
/// backends changes performance characteristics but not correctness.
/// Every implementation provides ordered-map semantics: insert,
/// lookup, iteration in key order, and length. Inserting the same
/// datom set in any order produces identical index state -- the
/// resulting ordered map is determined solely by the set of
/// key-value pairs, not by insertion sequence.
///
/// INV-FERR-005: the store maintains four secondary indexes in
/// bijection with the primary datom set. Each `IndexBackend`
/// instance backs one of those indexes (EAVT, AEVT, VAET, AVET).
/// Correct bijection requires that every datom inserted into the
/// primary set is also inserted into every index backend, and that
/// no index backend contains entries absent from the primary set.
///
/// `im::OrdMap` is the default backend (ADR-FERR-001), providing O(1)
/// clone via structural sharing. Alternative backends (B-tree, LSM,
/// `RocksDB`) can be substituted without changing store semantics.
///
/// # Contract
///
/// Implementors guarantee:
/// - **Determinism**: `backend_get` returns the most recently inserted
///   value for a given key. After sorting (if applicable), iteration
///   order is uniquely determined by key `Ord`.
/// - **Totality**: `backend_len` reflects the exact count of distinct
///   keys. `backend_values` yields exactly `backend_len` items.
/// - **Order independence** (INV-FERR-025): two instances that receive
///   the same set of `(K, V)` pairs (in any order) compare as
///   equivalent after any deferred sorting.
pub trait IndexBackend<K: Ord, V>: Clone + Default + std::fmt::Debug {
    /// Insert a key-value pair into the map.
    ///
    /// For persistent data structures (like `im::OrdMap`), the receiver
    /// is mutated in place with structural sharing. For owned structures,
    /// this is a standard insert.
    ///
    /// INV-FERR-005: callers insert into all four index backends for
    /// every datom added to the primary set, maintaining bijection.
    ///
    /// # Postcondition
    ///
    /// After `backend_insert(k, v)`, `backend_get(&k)` returns `Some(&v)`
    /// (immediately for sorted backends; after `sort()` for deferred-sort
    /// backends like [`SortedVecBackend`]).
    fn backend_insert(&mut self, key: K, value: V);

    /// Look up a value by exact key match.
    ///
    /// Returns `Some(&V)` if the key exists, `None` otherwise. For
    /// deferred-sort backends, the backing array must be sorted before
    /// this method produces correct results (see [`SortedVecBackend::sort`]).
    ///
    /// INV-FERR-025: all backends provide O(log n) lookup.
    /// INV-FERR-027: this is the interface through which read latency
    /// bounds are upheld.
    fn backend_get(&self, key: &K) -> Option<&V>;

    /// Number of entries in the map.
    ///
    /// INV-FERR-005: after a well-formed bulk load or sequence of
    /// inserts, all four index backends report the same `backend_len`
    /// as the primary datom set's cardinality. A mismatch indicates
    /// a bijection violation.
    fn backend_len(&self) -> usize;

    /// Whether the map contains no entries.
    ///
    /// Equivalent to `self.backend_len() == 0`. Provided as a default
    /// implementation; backends may override for efficiency.
    fn backend_is_empty(&self) -> bool {
        self.backend_len() == 0
    }

    /// Iterate over all values in key order.
    ///
    /// Yields exactly `backend_len` items. Iteration order follows the
    /// `Ord` implementation of `K`, which encodes the index-specific
    /// sort order (EAVT, AEVT, VAET, or AVET per INV-FERR-005).
    ///
    /// INV-FERR-027: ordered iteration enables O(log n + k) range
    /// scans for each index's access pattern.
    fn backend_values(&self) -> Box<dyn Iterator<Item = &V> + '_>;
}

// ---------------------------------------------------------------------------
// im::OrdMap implementation (ADR-FERR-001)
// ---------------------------------------------------------------------------

/// INV-FERR-025: `im::OrdMap` backend — the default index backend.
///
/// Provides O(log n) insert/lookup with O(1) clone via structural
/// sharing, making it ideal for MVCC snapshot isolation (INV-FERR-006).
impl<K: Ord + Clone + std::fmt::Debug, V: Clone + std::fmt::Debug> IndexBackend<K, V>
    for OrdMap<K, V>
{
    fn backend_insert(&mut self, key: K, value: V) {
        self.insert(key, value);
    }

    fn backend_get(&self, key: &K) -> Option<&V> {
        self.get(key)
    }

    fn backend_len(&self) -> usize {
        self.len()
    }

    fn backend_is_empty(&self) -> bool {
        self.is_empty()
    }

    fn backend_values(&self) -> Box<dyn Iterator<Item = &V> + '_> {
        Box::new(self.values())
    }
}

// ---------------------------------------------------------------------------
// SortedVecBackend (INV-FERR-071)
// ---------------------------------------------------------------------------

/// Sorted-array index backend (INV-FERR-071).
///
/// `Vec<(K, V)>` with deferred sort. Binary search on contiguous memory
/// achieves ~4 L1 cache misses per lookup vs ~18 for tree-based backends.
///
/// INV-FERR-025: behavioral equivalence with `im::OrdMap` for all operations.
/// INV-FERR-027: O(log n) lookups with cache-optimal memory layout.
///
/// # Visibility
///
/// `pub` because verification tests in `ferratomic-verify` exercise
/// backend equivalence properties between `SortedVecBackend` and
/// `OrdMap` (INV-FERR-025 conformance testing).
#[derive(Clone, Debug)]
pub struct SortedVecBackend<K: Ord, V> {
    entries: Vec<(K, V)>,
    sorted: bool,
}

impl<K: Ord, V> Default for SortedVecBackend<K, V> {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            sorted: true,
        }
    }
}

impl<K: Ord + Clone + std::fmt::Debug, V: Clone + std::fmt::Debug> IndexBackend<K, V>
    for SortedVecBackend<K, V>
{
    fn backend_insert(&mut self, key: K, value: V) {
        self.entries.push((key, value));
        self.sorted = false;
    }

    fn backend_get(&self, key: &K) -> Option<&V> {
        // Production guard: binary search on unsorted data returns wrong results.
        // In debug builds this fires as a debug_assert; in release, returns None
        // (safe degradation per NEG-FERR-001).
        if !self.sorted {
            debug_assert!(false, "INV-FERR-071: lookup on unsorted backend");
            return None;
        }
        // NOTE: `binary_search_by` returns an arbitrary match when duplicate
        // keys exist. This is safe because `sort()` calls `dedup_by` after
        // sorting (INV-FERR-071), guaranteeing keys are unique in the sorted
        // state. Index keys contain all 5 datom fields, so key equality
        // implies datom equality (INV-FERR-012). Therefore the "arbitrary"
        // match IS the unique match in any correctly constructed backend.
        self.entries
            .binary_search_by(|(k, _)| k.cmp(key))
            .ok()
            .map(|i| &self.entries[i].1)
    }

    fn backend_len(&self) -> usize {
        self.entries.len()
    }

    fn backend_values(&self) -> Box<dyn Iterator<Item = &V> + '_> {
        debug_assert!(
            self.sorted,
            "INV-FERR-071: iteration on unsorted SortedVecBackend — \
             call sort() or ensure_indexes_sorted() before querying"
        );
        Box::new(self.entries.iter().map(|(_, v)| v))
    }
}

impl<K: Ord, V> SortedVecBackend<K, V> {
    /// Construct from a pre-sorted, deduplicated vector (INV-FERR-071).
    ///
    /// The caller guarantees entries are sorted by key with no duplicate
    /// keys. This is the O(1) construction path for checkpoint loading.
    #[must_use]
    pub fn from_sorted(entries: Vec<(K, V)>) -> Self {
        debug_assert!(
            entries.windows(2).all(|w| w[0].0 < w[1].0),
            "INV-FERR-071: from_sorted requires sorted, deduplicated input"
        );
        Self {
            entries,
            sorted: true,
        }
    }

    /// Sort the backing array into key order (INV-FERR-071).
    ///
    /// Callers MUST NOT insert duplicate keys. Index keys contain all 5
    /// datom fields — two datoms with the same key ARE the same datom
    /// (INV-FERR-012) — so the primary datom set's uniqueness guarantee
    /// propagates to all index backends. A defensive `dedup_by` after
    /// sort removes any duplicates that would violate INV-FERR-071.
    ///
    /// Uses `sort_unstable_by` — O(n log n) time, O(1) auxiliary memory.
    /// Stable sort would allocate O(n) auxiliary (~20GB at 100M datoms),
    /// directly undermining positional content addressing targets
    /// (INV-FERR-076). This matches `PositionalStore::from_datoms` which
    /// uses `sort_unstable()` for the same reason.
    pub fn sort(&mut self) {
        if !self.sorted {
            self.entries.sort_unstable_by(|(a, _), (b, _)| a.cmp(b));
            // Dedup after sort: removes duplicates that would violate
            // INV-FERR-071 (index keys derived from deduplicated datom set).
            // O(n) but sort() is called at most once per store lifecycle
            // (during promote), not on the read hot path.
            self.entries.dedup_by(|(a, _), (b, _)| a == b);
            self.sorted = true;
        }
    }

    /// Whether the array is in sorted, deduplicated order (INV-FERR-071).
    #[must_use]
    pub fn is_sorted(&self) -> bool {
        self.sorted
    }
}
