//! Per-index key types, `IndexBackend` trait, and `Indexes` struct with
//! correct sort ordering.
//!
//! INV-FERR-005: four secondary indexes are maintained in bijection with
//! the primary datom set. Each index uses a distinct key type whose `Ord`
//! implementation arranges datom fields in the index-specific order:
//!
//! | Index | Sort order                       | Access pattern              |
//! |-------|----------------------------------|-----------------------------|
//! | EAVT  | entity, attribute, value, tx, op | "all facts about entity E"  |
//! | AEVT  | attribute, entity, value, tx, op | "all entities with attr A"  |
//! | VAET  | value, attribute, entity, tx, op | "reverse ref: who points here?" |
//! | AVET  | attribute, value, entity, tx, op | "unique lookup by attr+val" |
//!
//! INV-FERR-025: the index backend is interchangeable via the
//! [`IndexBackend`] trait. All backends produce identical query results
//! for the same sequence of operations — they differ only in performance
//! characteristics. The default backend is `im::OrdMap` (ADR-FERR-001).

use ferratom::{Attribute, Datom, EntityId, Op, TxId, Value};
use im::OrdMap;

// ---------------------------------------------------------------------------
// IndexBackend trait (INV-FERR-025)
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Index key types — Ord derives produce the correct sort order
// ---------------------------------------------------------------------------

/// EAVT key: sorted by (entity, attribute, value, tx, op) (INV-FERR-005).
///
/// Access pattern: "all facts about entity E".
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct EavtKey(
    pub(crate) EntityId,
    pub(crate) Attribute,
    pub(crate) Value,
    pub(crate) TxId,
    pub(crate) Op,
);

/// AEVT key: sorted by (attribute, entity, value, tx, op) (INV-FERR-005).
///
/// Access pattern: "all entities with attribute A".
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct AevtKey(
    pub(crate) Attribute,
    pub(crate) EntityId,
    pub(crate) Value,
    pub(crate) TxId,
    pub(crate) Op,
);

/// VAET key: sorted by (value, attribute, entity, tx, op) (INV-FERR-005).
///
/// Access pattern: "reverse reference -- who points to this entity?"
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct VaetKey(
    pub(crate) Value,
    pub(crate) Attribute,
    pub(crate) EntityId,
    pub(crate) TxId,
    pub(crate) Op,
);

/// AVET key: sorted by (attribute, value, entity, tx, op) (INV-FERR-005).
///
/// Access pattern: "unique lookup by attribute + value pair".
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct AvetKey(
    pub(crate) Attribute,
    pub(crate) Value,
    pub(crate) EntityId,
    pub(crate) TxId,
    pub(crate) Op,
);

impl EavtKey {
    /// Construct an EAVT key from a datom reference (INV-FERR-005).
    #[must_use]
    pub fn from_datom(d: &Datom) -> Self {
        Self(
            d.entity(),
            d.attribute().clone(),
            d.value().clone(),
            d.tx(),
            d.op(),
        )
    }
}

impl AevtKey {
    /// Construct an AEVT key from a datom reference (INV-FERR-005).
    #[must_use]
    pub fn from_datom(d: &Datom) -> Self {
        Self(
            d.attribute().clone(),
            d.entity(),
            d.value().clone(),
            d.tx(),
            d.op(),
        )
    }
}

impl VaetKey {
    /// Construct a VAET key from a datom reference (INV-FERR-005).
    #[must_use]
    pub fn from_datom(d: &Datom) -> Self {
        Self(
            d.value().clone(),
            d.attribute().clone(),
            d.entity(),
            d.tx(),
            d.op(),
        )
    }
}

impl AvetKey {
    /// Construct an AVET key from a datom reference (INV-FERR-005).
    #[must_use]
    pub fn from_datom(d: &Datom) -> Self {
        Self(
            d.attribute().clone(),
            d.value().clone(),
            d.entity(),
            d.tx(),
            d.op(),
        )
    }
}

// ---------------------------------------------------------------------------
// Indexes (generic over IndexBackend)
// ---------------------------------------------------------------------------

/// Secondary indexes over the datom set, each with a distinct sort order.
///
/// INV-FERR-005: every index is a bijection with the primary datom set.
/// After every mutation, all four maps have the same cardinality as the
/// primary set.
///
/// INV-FERR-025: the backend types are interchangeable. Each index uses
/// its own backend instance, parameterized by its key type. The default
/// is `im::OrdMap` (see [`Indexes`] type alias).
///
/// INV-FERR-027: correct per-index ordering enables O(log n + k) range
/// scans for different access patterns.
#[derive(Debug, Clone)]
pub struct GenericIndexes<BE, BA, BV, BAV>
where
    BE: IndexBackend<EavtKey, Datom>,
    BA: IndexBackend<AevtKey, Datom>,
    BV: IndexBackend<VaetKey, Datom>,
    BAV: IndexBackend<AvetKey, Datom>,
{
    /// Entity-Attribute-Value-Tx index.
    eavt: BE,
    /// Attribute-Entity-Value-Tx index.
    aevt: BA,
    /// Value-Attribute-Entity-Tx index (reverse references).
    vaet: BV,
    /// Attribute-Value-Entity-Tx index (unique/lookup).
    avet: BAV,
}

/// Default index type using `im::OrdMap` (ADR-FERR-001).
///
/// INV-FERR-025: type alias preserves backward compatibility — all
/// existing code that references `Indexes` continues to work without
/// changes.
pub type Indexes = GenericIndexes<
    OrdMap<EavtKey, Datom>,
    OrdMap<AevtKey, Datom>,
    OrdMap<VaetKey, Datom>,
    OrdMap<AvetKey, Datom>,
>;

/// Index type using [`SortedVecBackend`] for cache-optimal reads (INV-FERR-071).
///
/// INV-FERR-025: produces identical query results to [`Indexes`] (`OrdMap`
/// backend). Use for cold-start-loaded stores and read-heavy workloads.
/// Requires [`sort_all`](GenericIndexes::sort_all) after bulk insertion.
pub type SortedVecIndexes = GenericIndexes<
    SortedVecBackend<EavtKey, Datom>,
    SortedVecBackend<AevtKey, Datom>,
    SortedVecBackend<VaetKey, Datom>,
    SortedVecBackend<AvetKey, Datom>,
>;

impl
    GenericIndexes<
        SortedVecBackend<EavtKey, Datom>,
        SortedVecBackend<AevtKey, Datom>,
        SortedVecBackend<VaetKey, Datom>,
        SortedVecBackend<AvetKey, Datom>,
    >
{
    /// Sort all four index backends after bulk insertion (INV-FERR-071).
    ///
    /// Must be called after [`from_datoms`](GenericIndexes::from_datoms)
    /// to enable binary-search lookups. O(n log n) for n datoms.
    ///
    /// INV-FERR-005: after sorting, all four indexes are in their
    /// correct per-index order and binary search produces correct results.
    /// INV-FERR-025: behavioral equivalence with `im::OrdMap` is
    /// maintained after this call.
    pub fn sort_all(&mut self) {
        self.eavt.sort();
        self.aevt.sort();
        self.vaet.sort();
        self.avet.sort();
    }
}

impl<BE, BA, BV, BAV> GenericIndexes<BE, BA, BV, BAV>
where
    BE: IndexBackend<EavtKey, Datom>,
    BA: IndexBackend<AevtKey, Datom>,
    BV: IndexBackend<VaetKey, Datom>,
    BAV: IndexBackend<AvetKey, Datom>,
{
    /// Build indexes from a primary datom iterator.
    ///
    /// INV-FERR-005: all four indexes receive every datom from the primary
    /// set, ensuring bijection by construction.
    pub fn from_datoms<'a>(datoms: impl Iterator<Item = &'a Datom>) -> Self {
        let mut eavt = BE::default();
        let mut aevt = BA::default();
        let mut vaet = BV::default();
        let mut avet = BAV::default();

        for d in datoms {
            eavt.backend_insert(EavtKey::from_datom(d), d.clone());
            aevt.backend_insert(AevtKey::from_datom(d), d.clone());
            vaet.backend_insert(VaetKey::from_datom(d), d.clone());
            avet.backend_insert(AvetKey::from_datom(d), d.clone());
        }

        Self {
            eavt,
            aevt,
            vaet,
            avet,
        }
    }

    /// Insert a datom into all four indexes.
    ///
    /// INV-FERR-005: maintaining bijection requires every insert to
    /// touch all indexes.
    pub fn insert(&mut self, datom: &Datom) {
        self.eavt
            .backend_insert(EavtKey::from_datom(datom), datom.clone());
        self.aevt
            .backend_insert(AevtKey::from_datom(datom), datom.clone());
        self.vaet
            .backend_insert(VaetKey::from_datom(datom), datom.clone());
        self.avet
            .backend_insert(AvetKey::from_datom(datom), datom.clone());
    }

    /// Number of entries in the EAVT index (INV-FERR-005: same as all other indexes).
    #[must_use]
    pub fn len(&self) -> usize {
        self.eavt.backend_len()
    }

    /// Whether all indexes are empty (INV-FERR-005).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.eavt.backend_is_empty()
    }

    /// Access the EAVT index backend (INV-FERR-005, INV-FERR-027).
    #[must_use]
    pub fn eavt(&self) -> &BE {
        &self.eavt
    }

    /// Access the AEVT index backend (INV-FERR-005, INV-FERR-027).
    #[must_use]
    pub fn aevt(&self) -> &BA {
        &self.aevt
    }

    /// Access the VAET index backend (INV-FERR-005, INV-FERR-027).
    #[must_use]
    pub fn vaet(&self) -> &BV {
        &self.vaet
    }

    /// Access the AVET index backend (INV-FERR-005, INV-FERR-027).
    #[must_use]
    pub fn avet(&self) -> &BAV {
        &self.avet
    }

    /// Iterate EAVT datoms in index order (INV-FERR-027).
    pub fn eavt_datoms(&self) -> impl Iterator<Item = &Datom> {
        self.eavt.backend_values()
    }

    /// Iterate AEVT datoms in index order (INV-FERR-027).
    pub fn aevt_datoms(&self) -> impl Iterator<Item = &Datom> {
        self.aevt.backend_values()
    }

    /// Iterate VAET datoms in index order (INV-FERR-027).
    pub fn vaet_datoms(&self) -> impl Iterator<Item = &Datom> {
        self.vaet.backend_values()
    }

    /// Iterate AVET datoms in index order (INV-FERR-027).
    pub fn avet_datoms(&self) -> impl Iterator<Item = &Datom> {
        self.avet.backend_values()
    }

    /// Verify that all four indexes contain the same datom set (INV-FERR-005 bijection).
    ///
    /// INV-FERR-005: bijection implies both equal cardinality AND identical
    /// datom identity across all four indexes. Returns `true` if all four
    /// indexes agree on the count and the exact set of datom references.
    #[must_use]
    pub fn verify_bijection(&self) -> bool {
        let n = self.eavt.backend_len();
        if self.aevt.backend_len() != n
            || self.vaet.backend_len() != n
            || self.avet.backend_len() != n
        {
            return false;
        }
        // ME-003: Verify datom identity — not just cardinality. A bug
        // that inserts different datoms into different indexes would pass
        // the count-only check. O(n) but only called after transact, not
        // on the read hot path. Always-on (no cfg gate) per project rule:
        // "No #[cfg(...)] hiding code from the type checker."
        let eavt_datoms: std::collections::BTreeSet<_> = self.eavt.backend_values().collect();
        let aevt_datoms: std::collections::BTreeSet<_> = self.aevt.backend_values().collect();
        let vaet_datoms: std::collections::BTreeSet<_> = self.vaet.backend_values().collect();
        let avet_datoms: std::collections::BTreeSet<_> = self.avet.backend_values().collect();
        eavt_datoms == aevt_datoms && eavt_datoms == vaet_datoms && eavt_datoms == avet_datoms
    }
}
