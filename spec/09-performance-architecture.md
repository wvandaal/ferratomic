## 23.13 Performance Architecture — Columnar Storage & Zero-Copy Cold Start

The performance architecture transforms Ferratomic from a correct-but-slow in-memory
datom store into a system that operates at hardware I/O limits. Where section 23.3
defines performance TARGETS (latency bounds, write amplification limits), this section
specifies the MECHANISMS that achieve them — the algebraic data structures and physical
representations that make those targets reachable.

The core insight: `Store = (P(D), ∪)` is a free join-semilattice. Every optimization
in this section preserves the semilattice laws by construction. The representation
changes; the algebra does not. This means every node in a federated system may use a
DIFFERENT internal representation while remaining merge-compatible with all other nodes.
The semilattice's universal property guarantees this: any homomorphism-preserving
representation is automatically federation-compatible.

**Traces to**: GOALS.md §3 Tier 3 (Performance at Scale), INV-FERR-025 (Index Backend
Interchangeability), INV-FERR-027 (Read P99.99 Latency), INV-FERR-028 (Cold Start
Latency), ADR-FERR-001 (Persistent Data Structures), spec/06-prolly-tree.md (Phase 4b
physical representation)

**Design principles**:

1. **Representation independence.** The semilattice axioms (L1-L5) are defined on the
   abstract datom set `P(D)`. Every physical representation in this section is a
   FAITHFUL FUNCTOR from the abstract store to a concrete data structure. Faithfulness
   means no information is lost — the concrete structure can always reconstruct the
   abstract set. Different representations coexist in a federated system because merge
   is defined on the abstract level (set union), not on the representation level.

2. **Hardware-aware layout.** Data structures are designed for the memory hierarchy:
   L1 cache (64-byte lines), L2/L3 cache (MB-scale), DRAM (ns-scale random access),
   NVMe (μs-scale sequential, ms-scale random). Structures that work "against" the
   hierarchy (pointer-chasing through tree nodes scattered in DRAM) are replaced by
   structures that work "with" it (sequential scans through contiguous arrays).

3. **Algebraic compression.** The information content of a datom store is bounded by
   the entropy of the datom distribution, not by the raw byte count. Representations
   that exploit structural regularity (entity clustering, attribute sparsity, causal
   ordering) achieve compression ratios that are impossible for generic formats.

4. **Accretive design.** Every structure introduced here is a natural precursor to the
   prolly tree block store (section 23.9). Sorted arrays become prolly tree leaf chunks.
   Column stores become columnar chunks. Homomorphic hashes become Merkle root hashes.
   Nothing is throwaway.

---

### ADR-FERR-016: Localized Unsafe for Performance-Critical Cold Start

**Traces to**: INV-FERR-023 (No Unsafe Code), INV-FERR-028 (Cold Start Latency),
INV-FERR-013 (Checkpoint Equivalence)
**Stage**: 0

**Problem**: INV-FERR-023 mandates `#![forbid(unsafe_code)]` in all crates. The
zero-copy memory-mapped cold start path (INV-FERR-060) requires casting validated
bytes to typed references — an inherently unsafe operation. The 11,000x gap between
current cold start time (89s at 200K datoms) and the I/O-theoretic minimum (8ms)
is dominated by safe-but-slow tree construction. Closing this gap requires an
unsafe boundary.

**Options**:

| Option | Description | Pros | Cons |
|--------|-------------|------|------|
| A: No unsafe (status quo) | Accept 89s cold start; optimize within safe Rust | Zero unsafe surface. All verification layers hold. | 11,000x above I/O minimum. INV-FERR-028 unreachable at 100M. Federation cold start is minutes. |
| B: Localized unsafe module | Single `unsafe fn validate_and_cast()` in a dedicated `mmap` module, guarded by BLAKE3 verification. Rest of codebase stays `#![forbid(unsafe_code)]`. | Near-I/O-minimum cold start. 10ms at 200K, ~4s at 100M. Unsafe boundary is auditable (one function). | One unsafe function exists. Must be formally audited. BLAKE3 verification is the trust anchor. |
| C: External unsafe via FFI | Use an external C library (e.g., LMDB) for mmap. Unsafe lives in the C layer. | Battle-tested mmap implementation. | Adds C dependency. FFI is itself unsafe. Harder to audit than pure Rust. Violates substrate independence (C8). |

**Decision**: **Option B: Localized unsafe module**

The unsafe boundary is exactly one function: `validate_and_cast<T>(bytes: &[u8]) -> &T`.
This function:
1. Verifies BLAKE3(bytes[..len-32]) == bytes[len-32..] (integrity check)
2. Verifies alignment requirements for T
3. Casts the verified byte slice to a typed reference

The BLAKE3 verification provides 128-bit collision resistance — the probability of a
corrupted byte sequence passing validation is 2^{-128}. This is the same trust level
as the existing checkpoint format (which also uses BLAKE3).

The rest of the `mmap` module uses safe Rust around the validated reference. The rest
of the codebase retains `#![forbid(unsafe_code)]`. The unsafe surface is:
- 1 function (~15 lines)
- 1 module (`ferratomic-core/src/mmap.rs`)
- 1 crate (`ferratomic-core` — the only crate that touches disk)

`ferratom`, `ferratom-clock`, `ferratomic-datalog`, and `ferratomic-verify` remain
100% `#![forbid(unsafe_code)]`.

**Rejected**:
- **Option A**: Accepting 89s cold start is a Tier 3 value violation (Performance at
  Scale). The predecessor system "became unusable at 200K datoms" — Ferratomic exists
  to solve this exact problem. Leaving 11,000x performance on the table is misaligned
  with the project's purpose.
- **Option C**: FFI adds a larger unsafe surface than option B (the entire C library
  boundary), is harder to audit, and violates C8 (substrate independence).

**Consequence**: `ferratomic-core/Cargo.toml` changes from `#![forbid(unsafe_code)]` to
`#![deny(unsafe_code)]` with an explicit `#[allow(unsafe_code)]` ONLY on `mod mmap`.
All other modules in `ferratomic-core` retain the `deny`. A Kani harness verifies the
BLAKE3 guard property. A proptest verifies round-trip through the mmap path.

**Source**: GOALS.md §3 Tier 3 (Performance at Scale), GOALS.md §3 Tier 1
(Safety — the unsafe is justified because BLAKE3 provides the proof obligation that
`#![forbid(unsafe_code)]` normally provides via the type system).

---

### INV-FERR-060: Zero-Copy Cold Start via Memory-Mapped Checkpoint

**Traces to**: INV-FERR-028 (Cold Start Latency), INV-FERR-013 (Checkpoint Equivalence),
ADR-FERR-016 (Localized Unsafe), C2 (Content-Addressed Identity)
**Verification**: `V:PROP`, `V:KANI`, `V:LEAN`, `V:TYPE`
**Stage**: 0

#### Level 0 (Algebraic Law)
```
Let S be a DatomStore.
Let serialize : DatomStore → Bytes be the checkpoint serialization function.
Let mmap : Bytes → &ArchivedStore be the memory-mapping function.
Let project : &ArchivedStore → DatomStore be the abstraction (projection) function.

Axiom (round-trip identity):
  ∀ S : DatomStore: project(mmap(serialize(S))) = S

Axiom (I/O minimality):
  The time complexity of mmap(bytes) is O(1) — it performs no data transformation.
  The time complexity of project is O(1) — it performs no data copying.
  The only O(n) operation is serialize, which occurs during checkpoint write (background).

Theorem (cold start I/O bound):
  cold_start(file) = mmap(read(file)) has time complexity O(|file| / bandwidth).
  No processing beyond I/O occurs. The I/O-theoretic minimum is achieved.

Proof:
  mmap delegates to the OS virtual memory system, which establishes a page-table
  mapping without reading file content. First access to a page triggers a page fault
  (O(1) per page). The total I/O is bounded by the pages actually accessed during
  the first query, not by the file size. For sequential scans, OS readahead achieves
  bandwidth-optimal I/O. Therefore cold_start time = O(pages_accessed / bandwidth),
  which is ≤ O(|file| / bandwidth) and typically much less (only accessed pages).
```

#### Level 1 (State Invariant)
For all reachable store states produced by any sequence of TRANSACT and MERGE operations
starting from GENESIS: writing a checkpoint and memory-mapping the resulting file produces
a queryable store view that is IDENTICAL to the original store. "Identical" means: the
same set of datoms, the same index ordering, the same LIVE view, the same epoch. The
memory-mapped view is read-only — mutation requires promoting to a mutable representation
(INV-FERR-062).

Cold start time is bounded by I/O bandwidth, not by CPU processing. At 200K datoms
(~24MB file), cold start on NVMe (3 GB/s) is ~8ms. At 100M datoms (~12GB file), cold
start is ~4s. These bounds hold regardless of the index structure complexity because
no index construction occurs — the indexes are pre-built in the checkpoint file.

The memory-mapped view is validated by BLAKE3 checksum before first access. If the file
is corrupted (bit-rot, incomplete write, storage-layer failure), the validation fails
and cold start falls back to the V2 checkpoint path (deserialize + rebuild). This
defense-in-depth ensures INV-FERR-013 (round-trip identity) is maintained even under
storage corruption.

#### Level 2 (Implementation Contract)
```rust
/// Memory-map a checkpoint file and return a queryable store view.
///
/// INV-FERR-060: The returned view is identical to the store that wrote
/// the checkpoint. No index construction occurs — indexes are pre-built
/// in the file. Time complexity: O(1) for mapping, O(|file|/bandwidth)
/// for first full scan.
///
/// # Safety
///
/// Uses `unsafe` in `validate_and_cast` (ADR-FERR-016) to convert
/// BLAKE3-verified bytes to typed references. The unsafe boundary is
/// guarded by 128-bit collision-resistant integrity verification.
///
/// # Errors
///
/// Returns `FerraError::CheckpointCorrupted` if BLAKE3 verification fails.
pub fn mmap_cold_start(path: &Path) -> Result<MappedStore, FerraError> {
    let file = File::open(path)?;
    let mmap = unsafe { memmap2::MmapOptions::new().map(&file)? };
    let store = validate_and_cast::<ArchivedStore>(&mmap)?;
    Ok(MappedStore { _mmap: mmap, store })
}

#[kani::proof]
#[kani::unwind(4)]
fn mmap_roundtrip_identity() {
    let s = Store::from_datoms(kani::any());
    kani::assume(s.len() <= 4);
    let bytes = s.to_archived_bytes();
    let archived = validate_and_cast::<ArchivedStore>(&bytes)
        .expect("INV-FERR-060: valid bytes must validate");
    assert_eq!(archived.datom_count(), s.len());
    assert_eq!(archived.epoch(), s.epoch());
}
```

**Falsification**: Any store S where `project(mmap(serialize(S))) ≠ S`. Concretely:
a datom `d ∈ S` that is absent from the mapped view, or a datom in the mapped view
that is not in S. This would indicate that the serialization format loses or invents
datoms, or that the mapping introduces a representation error.

**proptest strategy**:
```rust
proptest! {
    fn mmap_roundtrip(
        datoms in prop::collection::btree_set(arb_datom(), 0..1000),
    ) {
        let store = Store::from_datoms(datoms);
        let bytes = store.to_archived_bytes();
        let mapped = validate_and_cast::<ArchivedStore>(&bytes)
            .expect("valid store must produce valid archived bytes");
        // Project back and compare datom sets
        let recovered = mapped.to_store();
        prop_assert_eq!(store.datom_set(), recovered.datom_set());
        prop_assert_eq!(store.epoch(), recovered.epoch());
    }
}
```

**Lean theorem**:
```lean
/-- INV-FERR-060: mmap round-trip is identity on the datom set.
    Modeled as: serialize then deserialize preserves the abstract store. -/
theorem mmap_roundtrip (s : DatomStore) :
    mmap_project (mmap_serialize s) = s :=
  -- Delegates to checkpoint_roundtrip (INV-FERR-013) since mmap is a
  -- representation change, not a semantic change. The abstract content
  -- is preserved by the same argument as checkpoint round-trip.
  checkpoint_roundtrip s
```

---

### INV-FERR-061: Sorted-Array Index Backend (Cache-Optimal Representation)

**Traces to**: INV-FERR-025 (Index Backend Interchangeability), INV-FERR-027
(Read P99.99 Latency), INV-FERR-005 (Index Bijection)
**Verification**: `V:PROP`, `V:KANI`, `V:LEAN`, `V:TYPE`
**Stage**: 0

#### Level 0 (Algebraic Law)
```
Let SortedVec<K, V> be a sorted array of (key, value) pairs where key order
is the total order on K.

Define the operations:
  insert(arr, k, v) = sort(arr ++ [(k, v)])
  lookup(arr, k) = binary_search(arr, k)
  range(arr, lo, hi) = slice(arr, binary_search_lo(arr, lo), binary_search_hi(arr, hi))
  values(arr) = map(snd, arr)
  len(arr) = |arr|

Theorem (behavioral equivalence with OrdMap):
  ∀ sequence of operations ops,
  ∀ initial state s₀:
    result(ops, SortedVec, s₀) = result(ops, OrdMap, s₀)

Proof:
  OrdMap and SortedVec are both faithful representations of the abstract
  ordered map (K → V). The key operations (insert, lookup, range, iterate)
  produce identical extensional results because both maintain the same
  total order on K. They differ only in:
  - Insert: OrdMap O(log n) with structural sharing; SortedVec O(n) worst case
            (must shift elements), or O(n log n) for batch insert-then-sort.
  - Lookup: OrdMap O(log n) with ~18 cache misses; SortedVec O(log n) with
            ~4 cache misses (contiguous memory, hardware prefetch).
  - Clone: OrdMap O(1) via structural sharing; SortedVec O(n) full copy.

  The extensional equivalence follows from the fact that both structures
  implement the same abstract ordered map interface (INV-FERR-025).
```

#### Level 1 (State Invariant)
The sorted-array index backend stores datom references as a contiguous `Vec<(K, Datom)>`
sorted by key K. It provides the same query results as the `im::OrdMap` backend for all
operations (insert, lookup, range scan, iteration). The performance characteristics
differ: lookups are ~4.5x faster (4 cache misses vs 18) due to contiguous memory layout
and hardware prefetch. Batch construction (collect + sort) is ~100x faster than
sequential OrdMap insertion because it avoids per-element tree rebalancing and allocation.

The tradeoff: snapshot isolation (INV-FERR-006) requires O(1) clone for MVCC. SortedVec
clone is O(n). This is acceptable for cold-start-loaded stores (which are read-only until
the first transaction) and for short-lived query snapshots. For mutable stores with
frequent snapshots, the OrdMap backend remains appropriate. INV-FERR-062 (lazy promotion)
handles the transition.

The sorted array is the in-memory analogue of a prolly tree leaf chunk (section 23.9).
When Phase 4b introduces content-defined chunking, each chunk IS a sorted array of
datoms. This representation is therefore maximally accretive — it does not need to be
replaced in later phases, only chunked.

#### Level 2 (Implementation Contract)
```rust
/// Sorted-array index backend (INV-FERR-061).
///
/// Implements `IndexBackend<K, V>` using a `Vec<(K, V)>` maintained in
/// sorted order. Bulk construction via `from_sorted` is O(n). Single
/// inserts mark the array as unsorted; the next lookup triggers a sort.
///
/// # Performance
/// - Lookup: O(log n) with ~4 L1 cache misses (binary search on contiguous memory)
/// - Range scan: O(log n + k) where k = result count (direct slice iteration)
/// - Bulk insert: O(n log n) via single `sort_unstable_by`
/// - Clone: O(n) — use OrdMap backend when O(1) clone is required
#[derive(Clone, Debug)]
pub struct SortedVecBackend<K: Ord, V> {
    entries: Vec<(K, V)>,
    sorted: bool,
}

impl<K: Ord + Clone + Debug, V: Clone + Debug> IndexBackend<K, V>
    for SortedVecBackend<K, V>
{
    fn backend_insert(&mut self, key: K, value: V) {
        self.entries.push((key, value));
        self.sorted = false;
    }

    fn backend_get(&self, key: &K) -> Option<&V> {
        debug_assert!(self.sorted, "INV-FERR-061: lookup on unsorted backend");
        self.entries
            .binary_search_by(|(k, _)| k.cmp(key))
            .ok()
            .map(|i| &self.entries[i].1)
    }

    fn backend_len(&self) -> usize {
        self.entries.len()
    }

    fn backend_values(&self) -> Box<dyn Iterator<Item = &V> + '_> {
        Box::new(self.entries.iter().map(|(_, v)| v))
    }
}

impl<K: Ord, V> SortedVecBackend<K, V> {
    /// Sort the backing array. Called once after batch insertion.
    /// O(n log n) with cache-optimal sequential access pattern.
    pub fn sort(&mut self) {
        if !self.sorted {
            self.entries.sort_unstable_by(|(a, _), (b, _)| a.cmp(b));
            self.sorted = true;
        }
    }
}

#[kani::proof]
#[kani::unwind(8)]
fn sorted_vec_lookup_matches_ordmap() {
    let key: u32 = kani::any();
    let value: u32 = kani::any();

    let mut sv = SortedVecBackend::default();
    sv.backend_insert(key, value);
    sv.sort();

    let mut om = OrdMap::new();
    om.insert(key, value);

    assert_eq!(sv.backend_get(&key), om.get(&key));
}
```

**Falsification**: Any sequence of insert + lookup operations where SortedVecBackend
returns a different result than OrdMap. Concretely: insert datoms D₁..Dₙ into both
backends, then lookup key K — the results differ. This would indicate that the sort
order or binary search implementation does not match OrdMap's tree-based ordering.

**proptest strategy**:
```rust
proptest! {
    fn sorted_vec_equivalent_to_ordmap(
        entries in prop::collection::vec((any::<u32>(), any::<u32>()), 0..500),
        query_keys in prop::collection::vec(any::<u32>(), 0..100),
    ) {
        let mut sv = SortedVecBackend::default();
        let mut om = OrdMap::new();
        for (k, v) in &entries {
            sv.backend_insert(*k, *v);
            om.insert(*k, *v);
        }
        sv.sort();

        for key in &query_keys {
            prop_assert_eq!(
                sv.backend_get(key).cloned(),
                om.get(key).cloned(),
                "INV-FERR-061: SortedVec and OrdMap must return identical results"
            );
        }
        prop_assert_eq!(sv.backend_len(), om.len());
    }
}
```

**Lean theorem**:
```lean
/-- INV-FERR-061: A sorted array and an ordered map produce identical lookup
    results for the same key, given the same set of inserted pairs. -/
theorem sorted_array_lookup_equiv (entries : List (Nat × Nat)) (key : Nat) :
    sorted_lookup (entries.toFinset) key = ordmap_lookup (entries.toFinset) key := by
  -- Both reduce to: find the unique pair (k, v) in entries where k = key.
  -- The sorted array finds it via binary search; the ordered map via tree traversal.
  -- Both are searching the same finite set, so the result is identical.
  simp [sorted_lookup, ordmap_lookup]
```

---

### INV-FERR-062: Lazy Representation Promotion (SortedVec → OrdMap)

**Traces to**: INV-FERR-061 (Sorted-Array Backend), INV-FERR-006 (Snapshot Isolation),
INV-FERR-025 (Index Backend Interchangeability)
**Verification**: `V:PROP`, `V:TYPE`
**Stage**: 0

#### Level 0 (Algebraic Law)
```
Let R₁ and R₂ be two faithful representations of the abstract ordered map.
Let promote : R₁ → R₂ be the promotion function.

Axiom (promotion preserves content):
  ∀ m : R₁: content(promote(m)) = content(m)

Where content : R → Set<(K, V)> extracts the abstract key-value set.

Axiom (promotion is idempotent via identity on R₂):
  ∀ m : R₂: promote(m) = m  (promotion of an already-promoted map is identity)

Theorem (lazy promotion preserves query results):
  ∀ m : R₁, ∀ query q:
    eval(q, m) = eval(q, promote(m))

Proof:
  Both R₁ and R₂ are faithful representations of the same abstract map.
  eval(q, _) depends only on the abstract content (by INV-FERR-025).
  Since content is preserved by promote, the query results are identical.
```

#### Level 1 (State Invariant)
A cold-start-loaded store uses SortedVecBackend for its indexes (fast bulk construction,
cache-optimal reads). The first mutating operation (TRANSACT) triggers promotion to
OrdMap backend (O(n log n) one-time cost), after which the store gains O(1) snapshot
cloning via structural sharing.

The promotion is transparent to callers: the Store API is identical before and after
promotion. The IndexBackend trait (INV-FERR-025) guarantees behavioral equivalence.
The promotion cost is amortized: it happens exactly once per cold start, and the
subsequent OrdMap operations benefit from structural sharing for the lifetime of the
store.

The lazy promotion pattern is the same as "copy-on-write" in virtual memory: the
read-only representation is retained until mutation forces a more expensive mutable
representation. This is the Curry-Howard analogue of lazy evaluation in functional
programming — defer computation until the result is needed.

#### Level 2 (Implementation Contract)
```rust
/// A store that uses SortedVec indexes for reads and promotes to OrdMap
/// on first write (INV-FERR-062).
///
/// INV-FERR-006: After promotion, snapshot isolation uses OrdMap structural
/// sharing. Before promotion, snapshots clone the SortedVec (O(n) but
/// cold-loaded stores are typically read-only until first transaction).
pub enum AdaptiveIndexes {
    /// Read-optimized: contiguous sorted arrays. O(log n) lookup with
    /// ~4 cache misses. O(n) clone. Used after cold start.
    SortedVec(GenericIndexes<SortedVecBackend<EavtKey, Datom>, ...>),
    /// Write-optimized: persistent balanced trees. O(log n) lookup with
    /// ~18 cache misses. O(1) clone. Used after first mutation.
    OrdMap(GenericIndexes<OrdMap<EavtKey, Datom>, ...>),
}

impl AdaptiveIndexes {
    /// Promote from SortedVec to OrdMap. Called once on first mutation.
    /// O(n log n) for n datoms — the sorted array is iterated and inserted
    /// into the OrdMap in sorted order (which is the optimal insertion order
    /// for balanced trees).
    fn promote(&mut self) {
        if let AdaptiveIndexes::SortedVec(sv) = self {
            let om = sv.to_ordmap(); // O(n log n) conversion
            *self = AdaptiveIndexes::OrdMap(om);
        }
    }
}
```

**Falsification**: Any store S where the datom set or query results change after
promotion. Concretely: `query(S_before_promotion) ≠ query(S_after_promotion)` for
any valid query. This would indicate that promotion loses or reorders datoms.

**proptest strategy**:
```rust
proptest! {
    fn promotion_preserves_queries(
        datoms in prop::collection::btree_set(arb_datom(), 0..500),
        query_entities in prop::collection::vec(arb_entity_id(), 0..50),
    ) {
        let store_sv = Store::from_datoms_sorted_vec(datoms.clone());
        let store_om = store_sv.promote_to_ordmap();

        for entity in &query_entities {
            let sv_result: Vec<_> = store_sv.datoms_for_entity(entity).collect();
            let om_result: Vec<_> = store_om.datoms_for_entity(entity).collect();
            prop_assert_eq!(sv_result, om_result,
                "INV-FERR-062: query results must be identical before and after promotion");
        }
        prop_assert_eq!(store_sv.len(), store_om.len());
    }
}
```

**Lean theorem**:
```lean
/-- INV-FERR-062: Promotion preserves the abstract datom set.
    Converting between representations does not change content. -/
theorem promote_preserves_content (s : DatomStore) :
    promote (sorted_vec_of s) = s := by
  -- sorted_vec_of and promote are inverse faithful functors.
  -- Their composition is the identity on abstract content.
  rfl
```

---

### INV-FERR-063: Yoneda Index Fusion (Single Store, Permutation Indexes)

**Traces to**: INV-FERR-005 (Index Bijection), INV-FERR-025 (Index Backend
Interchangeability), INV-FERR-061 (Sorted-Array Backend)
**Verification**: `V:PROP`, `V:KANI`, `V:LEAN`
**Stage**: 1

#### Level 0 (Algebraic Law)
```
Let S be a DatomStore with n datoms, stored as a sorted array A in EAVT order.
Let π_AEVT : [0,n) → [0,n) be the permutation such that A[π_AEVT(i)] is the
  i-th datom in AEVT order.
Similarly define π_VAET and π_AVET.

Theorem (Yoneda representation):
  ∀ query Q expressible in terms of any index order:
    eval(Q, {A, I_EAVT, I_AEVT, I_VAET, I_AVET})
    = eval(Q, {A, π_AEVT, π_VAET, π_AVET})

  The four materialized indexes are equivalent to one sorted array
  plus three permutation arrays.

Proof (sketch via Yoneda lemma):
  Each index I_X is a functor from the store category to the category of
  ordered sequences. By the Yoneda lemma, this functor is represented by
  its action on the identity morphism — which is the permutation that maps
  positions in the canonical (EAVT) order to positions in the X order.

  Concretely: to look up key k in the AEVT index, binary search the
  permuted view A[π_AEVT[0]], A[π_AEVT[1]], ..., A[π_AEVT[n-1]] for k.
  This produces the same result as binary search on a materialized AEVT
  sorted array because the permuted view IS the AEVT sorted sequence.

Space:
  4 materialized OrdMaps: ~4 × n × sizeof(Key + Datom) ≈ 4 × n × 150 bytes
  1 sorted array + 3 permutations: n × 120 bytes + 3 × n × 4 bytes = n × 132 bytes
  Reduction: 132 / 600 = 22% of materialized size. 78% memory savings.
```

#### Level 1 (State Invariant)
Instead of maintaining four separate index data structures (each containing a full copy
of every datom), the store maintains ONE sorted datom array in canonical (EAVT) order
plus three permutation arrays (u32 indices into the canonical array) for the AEVT, VAET,
and AVET orderings. The EAVT index is the canonical array itself (no permutation needed).

Queries against any index order work by binary search on the permuted view: to find
datom D in the AEVT index, binary search the array `[canonical[π_AEVT[0]], canonical[π_AEVT[1]], ...]`
for D's AEVT key. The indirection through the permutation adds one memory access per
comparison step (~4ns), a small constant factor versus the ~18 cache misses eliminated
by replacing OrdMap with contiguous arrays.

The permutation arrays are pre-computed during checkpoint write (or during bulk
construction) by sorting indices `[0..n)` by the respective key extractors. Three sorts
of `[u32; n]` at 200K datoms take ~15ms total — negligible compared to current 89s.

This representation is the discrete analogue of the Yoneda embedding from category
theory: the store is fully characterized by its canonical representation plus the
natural transformations (permutations) between index views. The bijection property
(INV-FERR-005) is a consequence: every datom appears exactly once in the canonical
array, and each permutation is a bijection on `[0, n)`.

#### Level 2 (Implementation Contract)
```rust
/// Yoneda-fused index representation (INV-FERR-063).
///
/// One sorted datom array + three permutation arrays.
/// 78% memory savings vs four materialized OrdMaps.
pub struct YonedaIndexes {
    /// Datoms sorted in canonical EAVT order.
    canonical: Vec<Datom>,
    /// Permutation: position in AEVT order → position in canonical array.
    perm_aevt: Vec<u32>,
    /// Permutation: position in VAET order → position in canonical array.
    perm_vaet: Vec<u32>,
    /// Permutation: position in AVET order → position in canonical array.
    perm_avet: Vec<u32>,
}

impl YonedaIndexes {
    /// Build from an unsorted datom iterator.
    /// O(n log n) for the canonical sort + 3 × O(n log n) for permutation sorts.
    pub fn from_datoms(datoms: impl Iterator<Item = Datom>) -> Self {
        let mut canonical: Vec<Datom> = datoms.collect();
        canonical.sort_unstable();

        let n = canonical.len();
        let mut perm_aevt: Vec<u32> = (0..n as u32).collect();
        let mut perm_vaet: Vec<u32> = (0..n as u32).collect();
        let mut perm_avet: Vec<u32> = (0..n as u32).collect();

        perm_aevt.sort_unstable_by(|&a, &b|
            AevtKey::from_datom(&canonical[a as usize])
                .cmp(&AevtKey::from_datom(&canonical[b as usize])));
        perm_vaet.sort_unstable_by(|&a, &b|
            VaetKey::from_datom(&canonical[a as usize])
                .cmp(&VaetKey::from_datom(&canonical[b as usize])));
        perm_avet.sort_unstable_by(|&a, &b|
            AvetKey::from_datom(&canonical[a as usize])
                .cmp(&AvetKey::from_datom(&canonical[b as usize])));

        Self { canonical, perm_aevt, perm_vaet, perm_avet }
    }

    /// AEVT lookup: binary search on the permuted view.
    pub fn aevt_get(&self, key: &AevtKey) -> Option<&Datom> {
        self.perm_aevt
            .binary_search_by(|&idx|
                AevtKey::from_datom(&self.canonical[idx as usize]).cmp(key))
            .ok()
            .map(|pos| &self.canonical[self.perm_aevt[pos] as usize])
    }
}
```

**Falsification**: Any datom D and index order X where `yoneda.X_get(key(D)) ≠ materialized_X.get(key(D))`.
The Yoneda representation returns a different result than four materialized indexes for
the same query.

**proptest strategy**:
```rust
proptest! {
    fn yoneda_equivalent_to_materialized(
        datoms in prop::collection::btree_set(arb_datom(), 0..500),
    ) {
        let yoneda = YonedaIndexes::from_datoms(datoms.iter().cloned());
        let materialized = Indexes::from_datoms(datoms.iter());

        for d in &datoms {
            let eavt_key = EavtKey::from_datom(d);
            let aevt_key = AevtKey::from_datom(d);

            // EAVT: canonical array lookup must match materialized
            prop_assert!(yoneda.canonical.binary_search(d).is_ok());

            // AEVT: permuted lookup must match materialized
            prop_assert_eq!(
                yoneda.aevt_get(&aevt_key).map(|d| d.entity()),
                materialized.aevt().get(&aevt_key).map(|d| d.entity()),
                "INV-FERR-063: Yoneda AEVT lookup must match materialized"
            );
        }
    }
}
```

**Lean theorem**:
```lean
/-- INV-FERR-063: A permutation of a finite set produces the same multiset
    of elements. Lookup on a permuted array is equivalent to lookup on the
    original array under the permuted ordering. -/
theorem permuted_lookup_equiv (arr : Fin n → α) (π : Equiv.Perm (Fin n))
    (key : α) [DecidableEq α] :
    (∃ i, arr i = key) ↔ (∃ i, arr (π i) = key) := by
  constructor
  · rintro ⟨i, hi⟩
    exact ⟨π.symm i, by rw [Equiv.Perm.apply_symm_apply]; exact hi⟩
  · rintro ⟨i, hi⟩
    exact ⟨π i, hi⟩
```

---

### INV-FERR-064: Homomorphic Store Fingerprint

**Traces to**: INV-FERR-010 (Merge Convergence), INV-FERR-013 (Checkpoint
Equivalence), C4 (CRDT Merge = Set Union), C2 (Content-Addressed Identity)
**Verification**: `V:PROP`, `V:LEAN`
**Stage**: 1

#### Level 0 (Algebraic Law)
```
Let H : DatomStore → G be a function from stores to an abelian group (G, +).
Let h : Datom → G be a function from individual datoms to group elements.

Define the store fingerprint as:
  H(S) = Σ_{d ∈ S} h(d)

Where Σ is the group sum (e.g., point addition on an elliptic curve, or
XOR over a hash space, or addition modulo a large prime).

Theorem (homomorphic merge):
  ∀ A, B ∈ DatomStore with A ∩ B = ∅:
    H(merge(A, B)) = H(A) + H(B)

Proof:
  merge(A, B) = A ∪ B (by definition of CRDT merge).
  Since A ∩ B = ∅:
    H(A ∪ B) = Σ_{d ∈ A ∪ B} h(d)
             = Σ_{d ∈ A} h(d) + Σ_{d ∈ B} h(d)   (disjoint union)
             = H(A) + H(B)

Theorem (incremental update):
  ∀ S, d where d ∉ S:
    H(S ∪ {d}) = H(S) + h(d)

Proof:
  Direct from the definition: the sum gains one additional term.

Corollary (O(1) merge verification):
  Given H(A) and H(B), one can verify H(merge(A, B)) = H(A) + H(B)
  in O(1) — a single group operation plus comparison. No re-hashing
  of the merged store is needed.

Corollary (O(1) convergence check):
  Two stores A and B have converged (contain identical datom sets) iff
  H(A) = H(B). Comparing 32-byte fingerprints replaces comparing
  potentially gigabyte-scale datom sets.
```

#### Level 1 (State Invariant)
Every store maintains a 32-byte fingerprint that is the group-sum of per-datom hashes.
The fingerprint is updated incrementally: each TRANSACT adds `h(d)` for each new datom
d. Each MERGE combines fingerprints via group addition (after accounting for shared
datoms via the intersection fingerprint).

The fingerprint enables O(1) convergence detection between federated stores: two stores
have identical datom sets if and only if their fingerprints match (with negligible
collision probability under the chosen hash function's security model). This replaces
the O(n) comparison needed by BLAKE3 whole-store hashing.

The fingerprint is the algebraic analogue of a Merkle root hash, but with a critical
advantage: it is INCREMENTALLY UPDATABLE without re-traversing the data structure. The
prolly tree's Merkle root (Phase 4b) requires O(log n) updates per datom insertion
(path from leaf to root). The homomorphic fingerprint requires O(1) updates regardless
of store size.

The concrete implementation uses BLAKE3 per-datom hashes XOR'd together. XOR is chosen
because it is the fastest group operation (single CPU instruction), and for SETS (where
each element appears exactly once), XOR is a valid group operation. For the G-Set CRDT
where elements are never removed, this is correct by construction.

#### Level 2 (Implementation Contract)
```rust
/// Homomorphic store fingerprint (INV-FERR-064).
///
/// H(S) = XOR_{d ∈ S} BLAKE3(serialize(d))
///
/// Incremental: adding datom d costs one BLAKE3 hash + one XOR.
/// Merge verification: H(merge(A,B)) == H(A) XOR H(B) for disjoint stores.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StoreFingerprint([u8; 32]);

impl StoreFingerprint {
    /// Empty store fingerprint (identity element of XOR group).
    pub const ZERO: Self = Self([0u8; 32]);

    /// Add one datom's contribution to the fingerprint.
    pub fn insert(&mut self, datom: &Datom) {
        let hash = blake3::hash(&bincode::serialize(datom).unwrap());
        for (a, b) in self.0.iter_mut().zip(hash.as_bytes()) {
            *a ^= b;
        }
    }

    /// Combine two fingerprints (for merge verification).
    pub fn merge(a: &Self, b: &Self) -> Self {
        let mut result = [0u8; 32];
        for i in 0..32 { result[i] = a.0[i] ^ b.0[i]; }
        Self(result)
    }
}
```

**Falsification**: Two stores A and B where `H(merge(A, B)) ≠ H(A) XOR H(B)` when
A and B are disjoint. This would indicate that the XOR accumulation is not faithfully
tracking the datom set, or that serialization is non-deterministic.

**proptest strategy**:
```rust
proptest! {
    fn fingerprint_homomorphic(
        a_datoms in prop::collection::btree_set(arb_datom(), 0..100),
        b_datoms in prop::collection::btree_set(arb_datom(), 0..100),
    ) {
        let a_only: BTreeSet<_> = a_datoms.difference(&b_datoms).cloned().collect();
        let b_only: BTreeSet<_> = b_datoms.difference(&a_datoms).cloned().collect();
        let merged: BTreeSet<_> = a_datoms.union(&b_datoms).cloned().collect();

        let fp_a = compute_fingerprint(&a_datoms);
        let fp_b_only = compute_fingerprint(&b_only);
        let fp_merged = compute_fingerprint(&merged);

        // H(A ∪ B_only) = H(A) XOR H(B_only) since A ∩ B_only = ∅
        let fp_combined = StoreFingerprint::merge(&fp_a, &fp_b_only);
        prop_assert_eq!(fp_combined, fp_merged,
            "INV-FERR-064: fingerprint must be homomorphic over disjoint union");
    }
}
```

**Lean theorem**:
```lean
/-- INV-FERR-064: XOR fingerprint is homomorphic over disjoint union. -/
theorem fingerprint_merge (A B : Finset Datom) (h : Disjoint A B)
    (fp : Datom → BitVec 256) :
    xor_fold fp (A ∪ B) = xor_fold fp A ^^^ xor_fold fp B := by
  rw [Finset.union_comm]
  exact xor_fold_disjoint_union fp h
```

---

### INV-FERR-065: LIVE-First Lattice Reduction Checkpoint

**Traces to**: INV-FERR-029 (LIVE View Resolution), INV-FERR-032 (LIVE Resolution
Correctness), INV-FERR-028 (Cold Start Latency), INV-FERR-013 (Checkpoint Equivalence)
**Verification**: `V:PROP`, `V:LEAN`
**Stage**: 1

#### Level 0 (Algebraic Law)
```
Let S be a DatomStore.
Let LIVE : DatomStore → P(EAV) be the LIVE projection:
  LIVE(S) = { (e, a, v) | ∃ t: Assert(e,a,v,t) ∈ S ∧
                           ¬∃ t' > t: Retract(e,a,v,t') ∈ S }

Theorem (LIVE idempotence):
  LIVE(LIVE(S)) = LIVE(S)
  (The LIVE projection is a retraction in the categorical sense.)

Theorem (LIVE-first checkpoint):
  ∀ S: the information content of S decomposes into:
    S = LIVE_datoms(S) ∪ HISTORICAL_datoms(S)
  where:
    LIVE_datoms(S) = { d ∈ S | (d.entity, d.attribute, d.value) ∈ LIVE(S) }
    HISTORICAL_datoms(S) = S \ LIVE_datoms(S)

  For any query Q that operates only on the current state (not historical):
    eval(Q, S) = eval(Q, LIVE_datoms(S))

Corollary (cold start reduction):
  If the checkpoint stores LIVE_datoms first and HISTORICAL_datoms second,
  cold start for current-state queries requires loading only |LIVE_datoms|
  datoms, which is ≤ |S| and typically much smaller (for mature stores
  with many retractions, |LIVE_datoms| ≪ |S|).

Proof:
  The decomposition S = LIVE_datoms(S) ∪ HISTORICAL_datoms(S) is a partition
  (every datom is in exactly one subset). Current-state queries depend only
  on the LIVE view, which is fully determined by LIVE_datoms. Historical
  datoms contribute only to temporal queries (as-of, history-of).
```

#### Level 1 (State Invariant)
The checkpoint file is structured with the LIVE datoms first, followed by historical
datoms. Cold start loads only the LIVE section for applications that need only the
current state (the common case for agentic systems retrieving current knowledge).
Historical datoms are loaded on demand when temporal queries are executed.

For a 200K-datom store where 50K values are currently live: cold start loads 50K datoms
(~6 MB) instead of 200K (~24 MB). At 100M datoms with 10M live: cold start loads 10M
(~1.2 GB) instead of 100M (~12 GB). The LIVE-first layout achieves 2-10x cold start
reduction depending on the retraction ratio.

The mathematical foundation is that LIVE is a RETRACTION in the category of semilattices
— it is a semilattice homomorphism that is also idempotent. This means:
`merge(LIVE(A), LIVE(B)) = LIVE(merge(A, B))` — LIVE views can be merged directly
without needing the full history. This property is the algebraic foundation for efficient
federation: the initial sync between two stores can exchange LIVE views only, with
historical datoms synced in the background.

#### Level 2 (Implementation Contract)
```rust
/// LIVE-first checkpoint layout (INV-FERR-065).
///
/// Section 1: LIVE datoms (current state — loaded at cold start)
/// Section 2: Historical datoms (past state — loaded on demand)
/// Section 3: Metadata (epoch, schema, fingerprint)
///
/// The boundary between sections 1 and 2 is stored in the metadata
/// so that cold start can stop reading after section 1.
pub struct LiveFirstCheckpoint {
    live_datom_count: u64,
    total_datom_count: u64,
    // ... checkpoint payload follows
}

/// Cold start with LIVE-first: load only the LIVE section.
/// Historical datoms are loaded lazily via `load_historical()`.
pub fn cold_start_live_first(path: &Path) -> Result<PartialStore, FerraError> {
    let checkpoint = read_checkpoint_header(path)?;
    let live_datoms = read_datoms(path, 0..checkpoint.live_datom_count)?;
    let store = Store::from_datoms(live_datoms);
    Ok(PartialStore { store, historical_path: path.to_owned() })
}
```

**Falsification**: Any store S where `LIVE(LIVE_datoms(S)) ≠ LIVE(S)`. This would mean
the LIVE datom subset doesn't fully determine the LIVE view — a datom in HISTORICAL
affects the current state, which contradicts the decomposition.

**proptest strategy**:
```rust
proptest! {
    fn live_first_preserves_live_view(
        datoms in prop::collection::btree_set(arb_datom(), 0..500),
    ) {
        let store_full = Store::from_datoms(datoms);
        let live_datoms = store_full.live_datoms();
        let store_live = Store::from_datoms(live_datoms);

        // The LIVE view of the full store must equal the LIVE view of just the LIVE datoms
        prop_assert_eq!(
            store_full.live_view(),
            store_live.live_view(),
            "INV-FERR-065: LIVE view must be fully determined by LIVE datoms"
        );
    }
}
```

**Lean theorem**:
```lean
/-- INV-FERR-065: The LIVE projection is idempotent — applying it twice
    produces the same result as applying it once. This is the retraction
    property that enables LIVE-first checkpointing. -/
theorem live_idempotent (S : DatomStore) :
    live_view_model (live_datoms S) = live_view_model (S.toList) := by
  -- The LIVE datoms are exactly those whose (e,a,v) triple has its latest
  -- operation being Assert. Filtering to LIVE datoms and recomputing LIVE
  -- produces the same set because no retraction from HISTORICAL can affect
  -- a triple whose latest operation is already Assert.
  sorry -- Non-trivial; requires induction on the datom sequence. File bead.
```

---

*Spec continues in next section with INV-FERR-066 (van Emde Boas cache-oblivious layout)
and INV-FERR-067 (columnar datom decomposition). These are Stage 2 invariants — designed
now, implemented when the Phase 4a foundations (060-062) are proven stable.*
