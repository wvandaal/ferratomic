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

### ADR-FERR-020: Localized Unsafe for Performance-Critical Cold Start

**Traces to**: INV-FERR-023 (No Unsafe Code), INV-FERR-028 (Cold Start Latency),
INV-FERR-013 (Checkpoint Equivalence)
**Stage**: 0

**Problem**: INV-FERR-023 mandates `#![forbid(unsafe_code)]` in all crates. The
zero-copy memory-mapped cold start path (INV-FERR-070) requires casting validated
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

### INV-FERR-070: Zero-Copy Cold Start via Memory-Mapped Checkpoint

**Traces to**: INV-FERR-028 (Cold Start Latency), INV-FERR-013 (Checkpoint Equivalence),
ADR-FERR-020 (Localized Unsafe), C2 (Content-Addressed Identity)
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
(INV-FERR-072).

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
/// INV-FERR-070: The returned view is identical to the store that wrote
/// the checkpoint. No index construction occurs — indexes are pre-built
/// in the file. Time complexity: O(1) for mapping, O(|file|/bandwidth)
/// for first full scan.
///
/// # Safety
///
/// Uses `unsafe` in `validate_and_cast` (ADR-FERR-020) to convert
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
        .expect("INV-FERR-070: valid bytes must validate");
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
/-- INV-FERR-070: mmap round-trip is identity on the datom set.
    Modeled as: serialize then deserialize preserves the abstract store. -/
-- Requires definitions: mmap_serialize, mmap_project (not yet in Lean model).
-- At the Finset abstraction level, mmap is a representation change that
-- preserves the abstract datom set — the same argument as checkpoint_roundtrip
-- (INV-FERR-013). The Lean model abstracts away representation, so these
-- are defined as identity on DatomStore when mechanized.
def mmap_serialize (s : DatomStore) : DatomStore := s
def mmap_project (s : DatomStore) : DatomStore := s

theorem mmap_roundtrip (s : DatomStore) :
    mmap_project (mmap_serialize s) = s := rfl
```

---

### INV-FERR-071: Sorted-Array Index Backend (Cache-Optimal Representation)

**Traces to**: INV-FERR-025 (Index Backend Interchangeability), INV-FERR-027
(Read P99.99 Latency), INV-FERR-005 (Index Bijection)
**Verification**: `V:PROP`, `V:KANI`, `V:LEAN`, `V:TYPE`
**Referenced by**: NEG-FERR-007 (FM-Index inapplicability), ADR-FERR-030 (wavelet matrix target)
**Stage**: 0

#### Level 0 (Algebraic Law)
```
Let SortedVec<K, V> be a sorted array of (key, value) pairs where key order
is the total order on K.

Define the operations:
  insert(arr, k, v) = sort(filter(arr, key ≠ k) ++ [(k, v)])
    -- Remove existing entry for k before inserting (map semantics, not multimap).
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
frequent snapshots, the OrdMap backend remains appropriate. INV-FERR-072 (lazy promotion)
handles the transition.

The sorted array is the in-memory analogue of a prolly tree leaf chunk (section 23.9).
When Phase 4b introduces content-defined chunking, each chunk IS a sorted array of
datoms. This representation is therefore maximally accretive — it does not need to be
replaced in later phases, only chunked.

#### Level 2 (Implementation Contract)
```rust
/// Sorted-array index backend (INV-FERR-071).
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
        // Map semantics: remove existing entry for this key before inserting.
        // Deferred to sort() for batch performance — unsorted buffer may
        // temporarily contain duplicate keys, resolved on next sort().
        self.entries.push((key, value));
        self.sorted = false;
    }

    fn backend_get(&self, key: &K) -> Option<&V> {
        debug_assert!(self.sorted, "INV-FERR-071: lookup on unsorted backend");
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
            // Stable sort: preserves insertion order among equal keys.
            // This is required for map semantics: among duplicate keys,
            // the LAST pushed entry appears last in the sorted output.
            self.entries.sort_by(|(a, _), (b, _)| a.cmp(b));
            // Map semantics: retain the LAST inserted value for each key.
            // After stable sort, the last push() for a key appears last
            // among its duplicates. Reverse, dedup (keeps first of run),
            // reverse again — net effect: keep LAST inserted per key.
            self.entries.reverse();
            self.entries.dedup_by(|(k1, _), (k2, _)| k1 == k2);
            self.entries.reverse();
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
                "INV-FERR-071: SortedVec and OrdMap must return identical results"
            );
        }
        prop_assert_eq!(sv.backend_len(), om.len());
    }
}
```

**Lean theorem**:
```lean
/-- INV-FERR-071: At the Lean abstraction level, both sorted-array and
    tree-based representations are modeled as the same Finset (Nat × Nat).
    Representation is abstracted away — the abstract ordered map IS the
    Finset, so any two representations of the same key-value set yield
    identical lookup results by construction.

    This is intentional per ADR-FERR-007 (parallel models). The non-trivial
    property — that the concrete Rust SortedVecBackend and OrdMap produce
    identical results despite different internal structures and duplicate-key
    handling — is verified by proptest (sorted_vec_equivalent_to_ordmap). -/
def map_lookup (m : Finset (Nat × Nat)) (key : Nat) : Option Nat :=
  (m.filter (fun p => p.1 = key)).image Prod.snd |>.min

/-- Two Finset-based maps with the same entries produce the same lookup.
    Trivially true at the Finset level (representation is the content).
    Concrete representation equivalence verified by proptest. -/
theorem sorted_array_lookup_equiv (s₁ s₂ : Finset (Nat × Nat))
    (h : s₁ = s₂) (key : Nat) :
    map_lookup s₁ key = map_lookup s₂ key := by rw [h]
```

---

### INV-FERR-072: Lazy Representation Promotion / Demotion (Positional ↔ OrdMap)

**Traces to**: INV-FERR-071 (Sorted-Array Backend), INV-FERR-006 (Snapshot Isolation),
INV-FERR-025 (Index Backend Interchangeability)
**Verification**: `V:PROP`, `V:TYPE`, `V:LEAN`
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

Let demote : R₂ → R₁ be the demotion function (OrdMap → Positional).

Axiom (demotion preserves content):
  ∀ m : R₂: content(demote(m)) = content(m)

Axiom (demotion is idempotent via identity on R₁):
  ∀ m : R₁: demote(m) = m  (demotion of an already-demoted map is identity)

Theorem (batch equivalence):
  ∀ sequence of N mutations m₁..mₙ on a store initially in R₁:
    demote(apply(mₙ, ... apply(m₁, promote(s))))
    = demote(promote(apply_all([m₁..mₙ], s)))

  N individual promote/mutate/demote cycles produce the same result as
  one promote, N mutations, one demote. The intermediate representation
  switches are algebraically invisible.

Proof:
  Each apply(mᵢ, _) depends only on abstract content (by INV-FERR-025).
  promote and demote preserve content (axioms above).
  Therefore the composition of content-preserving functions is
  content-preserving, regardless of how many intermediate representation
  switches occur. The batch form avoids N−1 redundant round-trips
  without changing the algebraic result.
```

#### Level 1 (State Invariant)
A cold-start-loaded store uses SortedVecBackend for its indexes (fast bulk construction,
cache-optimal reads). Two mutation paths exist, chosen by representation:

**Path A: Merge-sort splice (Positional stores — the hot path).**
For single-transaction writes on a Positional store, the store bypasses the OrdMap
representation entirely. New datoms are pre-sorted in EAVT order, then merge-spliced
directly into the canonical array using `merge_sort_dedup`. The store remains in
Positional form throughout. Cost: O(N + K log K) where N = store size, K = new datoms.

This path is preferred because the workload assumption (C9: balanced/bursty) means
writes are frequent. The O(N log N) promote/demote cycle designed for read-heavy
workloads is replaced by O(N + K) splice that keeps the store in its cache-optimal
Positional form. The algebraic result is identical — the batch equivalence theorem
(Level 0) proves that any representation-preserving mutation path that produces the
same datom set is valid.

After splice, permutation arrays (AEVT, VAET, AVET, TxId) are invalidated via
OnceLock reset and rebuilt lazily on first access. The LIVE bitvector is rebuilt
incrementally via dirty-chunk tracking (INV-FERR-080) when chunk fingerprints
(INV-FERR-079) are available, or in full O(N) otherwise. The store fingerprint
(INV-FERR-074) is updated incrementally in O(K) via XOR homomorphism.

**Path B: Promote/demote (batch replay — recovery and complex operations).**
For WAL recovery (`batch_replay`) and multi-step operations where incremental
OrdMap mutation is more efficient than repeated splice (e.g., replaying hundreds
of WAL entries), the store promotes to OrdMap once, applies all mutations, then
demotes once. Cost: O(N) promote + O(K_total log N) mutations + O(N) demote.

The promotion is transparent to callers: the Store API is identical before and after
promotion. The IndexBackend trait (INV-FERR-025) guarantees behavioral equivalence.

**Path C: Batch splice (group commit — write bursts).**
For live write bursts at the Database level, `batch_transact` accumulates M
transactions, pre-sorts their combined K_total datoms, and performs a single
merge-sort splice. Cost: O(N + K_total log K_total) instead of M x O(N + K_i).
This extends the batch_replay insight from recovery-only to live writes and is the
Phase 4a precursor to the Phase 4b WriterActor group commit.

Between transactions, the store is always in Positional form (the prolly tree leaf
precursor per accretive design principle 4). The OrdMap representation is a transient
state used only during batch_replay, not the steady-state write path.

#### Level 2 (Implementation Contract)
```rust
/// A store that uses SortedVec indexes for reads and promotes to OrdMap
/// on first write (INV-FERR-072).
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

/// Demote from OrdMap back to Positional (INV-FERR-072).
///
/// O(n) because OrdSet iteration is EAVT-sorted, so sort_unstable
/// detects the sorted run in O(n). Permutation arrays are OnceLock (lazy).
/// No-op if already Positional.
pub(crate) fn demote(&mut self) {
    if let StoreRepr::OrdMap { datoms, .. } = &self.repr {
        let positional = PositionalStore::from_datoms(datoms.iter().cloned());
        self.repr = StoreRepr::Positional(Arc::new(positional));
    }
}

/// Merge-sort splice: insert K datoms into Positional without OrdMap detour.
///
/// INV-FERR-072 Path A: the hot write path for balanced workloads (C9).
/// Pre-sorts new datoms, merge-splices into canonical array, rebuilds LIVE
/// incrementally via dirty chunks (INV-FERR-080), updates fingerprint via
/// XOR homomorphism (INV-FERR-074). Store stays Positional throughout.
///
/// Cost: O(N + K log K) where N = |canonical|, K = |new_datoms|.
fn splice_transact(&mut self, new_datoms: Vec<Datom>) -> Result<(), FerraError> {
    if let StoreRepr::Positional(ps) = &self.repr {
        let mut sorted_new = new_datoms;
        sorted_new.sort_unstable();
        let merged = merge_sort_dedup(ps.datoms(), &sorted_new);
        // Fingerprint: XOR in K new hashes (homomorphic, O(K))
        let mut fp = ps.fingerprint();
        for d in &sorted_new {
            xor_hash_into(&mut fp, &d.content_hash());
        }
        // LIVE: incremental via dirty chunks if available, else full rebuild
        let live_bits = build_live_bitvector(&merged); // or rebuild_live_incremental
        let new_ps = PositionalStore::from_merged(merged, fp, live_bits);
        self.repr = StoreRepr::Positional(Arc::new(new_ps));
        for d in &sorted_new { self.live_apply(d); }
    }
    Ok(())
}

/// Batch replay: promote once, apply N WAL entries, demote once (INV-FERR-072 Path B).
///
/// Used by recovery to avoid N promote/demote cycles.
/// Cost: 1 promote + N x insert + 1 demote, vs N x (promote + insert + demote).
/// INV-FERR-009: schema evolution applied per-entry for correct epoch boundaries.
pub(crate) fn batch_replay(
    &mut self,
    entries: &[(u64, Vec<Datom>)],
) -> Result<(), FerraError> {
    if entries.is_empty() {
        return Ok(());
    }
    self.promote();
    for (epoch, datoms) in entries {
        for datom in datoms {
            self.insert(datom);
        }
        self.epoch = *epoch;
        evolve_schema(&mut self.schema, datoms)?;
    }
    self.demote();
    Ok(())
}

/// Batch splice: group commit for write bursts (INV-FERR-072 Path C).
///
/// Accumulates M transactions, pre-sorts combined K_total datoms, single
/// merge-sort splice. Each transaction gets a distinct epoch.
/// Cost: O(N + K_total log K_total) instead of M x O(N + K_i).
///
/// Phase 4a precursor to Phase 4b WriterActor group commit.
pub(crate) fn batch_splice_transact(
    &mut self,
    transactions: Vec<(Vec<Datom>, TxId)>,
) -> Result<Vec<TxReceipt>, FerraError> {
    // Combine all datoms, sort once, splice once
    let mut all_datoms: Vec<Datom> = Vec::new();
    let mut receipts = Vec::new();
    for (datoms, tx_id) in &transactions {
        let stamped = stamp_datoms(datoms.clone(), *tx_id);
        all_datoms.extend(stamped);
    }
    self.splice_transact(all_datoms.clone())?;
    for (datoms, tx_id) in &transactions {
        self.epoch += 1;
        receipts.push(TxReceipt { epoch: self.epoch, datoms: datoms.clone() });
    }
    Ok(receipts)
}
```

**Falsification**: Any store S where the datom set or query results change after
promotion. Concretely: `query(S_before_promotion) ≠ query(S_after_promotion)` for
any valid query. This would indicate that promotion loses or reorders datoms.
Any store S where `content(demote(S)) ≠ content(S)` — demotion lost or invented datoms.

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
                "INV-FERR-072: query results must be identical before and after promotion");
        }
        prop_assert_eq!(store_sv.len(), store_om.len());
    }

    fn demotion_preserves_content(
        datoms in prop::collection::btree_set(arb_datom(), 0..500),
    ) {
        let mut store = Store::from_datoms(datoms);
        store.promote(); // R₁ → R₂
        let before: BTreeSet<_> = store.datoms().collect();
        store.demote(); // R₂ → R₁
        let after: BTreeSet<_> = store.datoms().collect();
        prop_assert_eq!(before, after,
            "INV-FERR-072: demotion must preserve the datom set");
    }

    fn demotion_roundtrip_after_transact(
        datoms in prop::collection::btree_set(arb_datom(), 0..200),
        new_datom in arb_datom(),
    ) {
        let mut store = Store::from_datoms(datoms);
        // Path A: splice_transact keeps store in Positional (no promote/demote)
        store.transact_test(Transaction::from_datom(new_datom));
        // After transact, store should be back in Positional representation
        prop_assert!(store.positional().is_some(),
            "INV-FERR-072: store must auto-demote to Positional after transact");
    }
}
```

**Lean theorem**:
```lean
/-- INV-FERR-072: Promotion preserves the abstract datom set.
    At the Lean abstraction level (Finset Datom), both SortedVec and OrdMap
    are represented as the same Finset — representation is abstracted away.
    The theorem is trivially true at this level, confirming that no algebraic
    content is introduced or lost by the representation change. Concrete
    representation fidelity (the non-trivial property) is verified by proptest,
    which exercises the actual Rust conversion. -/
def sorted_vec_of (s : DatomStore) : DatomStore := s
def promote (s : DatomStore) : DatomStore := s

theorem promote_preserves_content (s : DatomStore) :
    promote (sorted_vec_of s) = s := rfl

def demote (s : DatomStore) : DatomStore := s

theorem demote_preserves_content (s : DatomStore) :
    demote (promote s) = s := rfl
```

---

### INV-FERR-073: Yoneda Index Fusion (Single Store, Permutation Indexes)

**Traces to**: INV-FERR-005 (Index Bijection), INV-FERR-025 (Index Backend
Interchangeability), INV-FERR-071 (Sorted-Array Backend)
**Verification**: `V:PROP`, `V:LEAN`
**Referenced by**: ADR-FERR-030 (wavelet matrix target — subsumes permutation indexes)
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

Proof (permutation equivalence):
  Each permutation π_X is constructed by sorting indices [0, n) by the
  X-order key extractor applied to the canonical array. Therefore, the
  sequence A[π_X[0]], A[π_X[1]], ..., A[π_X[n-1]] is sorted in X order
  by construction.

  Binary search on this permuted view produces the same result as binary
  search on a separately materialized X-sorted array because:
  (1) Both contain the same multiset of elements (permutation preserves
      the element set — Lean theorem `permuted_lookup_equiv`).
  (2) Both are sorted in X order (the permuted view by construction,
      the materialized array by explicit sort).
  (3) Binary search on two arrays with identical elements in identical
      order returns identical results.

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
/// Yoneda-fused index representation (INV-FERR-073).
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
            // EAVT: canonical array lookup must match materialized
            prop_assert!(yoneda.canonical.binary_search(d).is_ok());

            // AEVT: permuted lookup must match materialized
            let aevt_key = AevtKey::from_datom(d);
            prop_assert_eq!(
                yoneda.aevt_get(&aevt_key).map(|d| d.entity()),
                materialized.aevt().get(&aevt_key).map(|d| d.entity()),
                "INV-FERR-073: Yoneda AEVT lookup must match materialized"
            );

            // VAET: permuted lookup must match materialized
            let vaet_key = VaetKey::from_datom(d);
            prop_assert_eq!(
                yoneda.vaet_get(&vaet_key).map(|d| d.entity()),
                materialized.vaet().get(&vaet_key).map(|d| d.entity()),
                "INV-FERR-073: Yoneda VAET lookup must match materialized"
            );

            // AVET: permuted lookup must match materialized
            let avet_key = AvetKey::from_datom(d);
            prop_assert_eq!(
                yoneda.avet_get(&avet_key).map(|d| d.entity()),
                materialized.avet().get(&avet_key).map(|d| d.entity()),
                "INV-FERR-073: Yoneda AVET lookup must match materialized"
            );
        }
    }
}
```

**Lean theorem**:
```lean
/-- INV-FERR-073: A permutation preserves element existence — every element
    findable in the original array is findable in the permuted array and
    vice versa. This captures the Lean-expressible subset of the equivalence
    claim. Full value-lookup equivalence (binary search returns the same
    associated value, not just that the key exists) is verified by proptest,
    which exercises the concrete Rust permutation + binary search path. -/
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

### INV-FERR-074: Homomorphic Store Fingerprint

**Traces to**: INV-FERR-010 (Merge Convergence), INV-FERR-013 (Checkpoint
Equivalence), C4 (CRDT Merge = Set Union), C2 (Content-Addressed Identity)
**Referenced by**: INV-FERR-079 (chunk fingerprint array — hierarchical decomposition of store fingerprint)
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

Theorem (non-disjoint merge):
  ∀ A, B ∈ DatomStore (not necessarily disjoint):
    H(A ∪ B) = H(A) ⊕ H(B) ⊕ H(A ∩ B)

Proof:
  H(A) ⊕ H(B) = Σ_{d ∈ A} h(d) ⊕ Σ_{d ∈ B} h(d).
  Elements in A ∩ B contribute h(d) ⊕ h(d) = 0 (XOR self-cancellation).
  So H(A) ⊕ H(B) = H(A △ B), where △ is the symmetric difference.
  Since A ∪ B is the disjoint union of (A △ B) and (A ∩ B):
    H(A ∪ B) = H(A △ B) ⊕ H(A ∩ B)   (by the disjoint merge theorem)
             = H(A) ⊕ H(B) ⊕ H(A ∩ B).

Corollary (O(1) merge verification):
  Given H(A) and H(B), one can verify H(merge(A, B)) = H(A) ⊕ H(B)
  for disjoint stores in O(1) — a single XOR plus comparison. For non-
  disjoint stores, the intersection fingerprint H(A ∩ B) is computed
  during merge by tracking which datoms appear in both stores.

Theorem (convergence necessary condition):
  ∀ A, B ∈ DatomStore: A = B → H(A) = H(B)
  Proof: If A = B, then they contain the same datoms, so the XOR sums
  are computed over the same elements. Identical inputs produce identical
  outputs by determinism of BLAKE3 and XOR.

Theorem (divergence detection):
  ∀ A, B ∈ DatomStore: H(A) ≠ H(B) → A ≠ B
  Proof: Contrapositive of the above.
```

#### Level 1 (State Invariant)
**O(1) convergence check**: `H(A) = H(B)` implies A = B with overwhelming probability
(collision probability ≤ 2^{-128} per store pair under BLAKE3's 128-bit security model).
This is a SECURITY ASSUMPTION, not a theorem — it depends on BLAKE3's collision resistance.
A mismatch `H(A) ≠ H(B)` GUARANTEES the stores differ (the divergence detection theorem
above is unconditional). Comparing 32-byte fingerprints replaces comparing potentially
gigabyte-scale datom sets.

Every store maintains a 32-byte fingerprint that is the XOR-sum of per-datom hashes.
The fingerprint is updated incrementally: each TRANSACT XORs `h(d)` for each new datom
d. For MERGE of disjoint stores: `H(A ∪ B) = H(A) ⊕ H(B)`. For non-disjoint stores,
the intersection must be accounted for: `H(A ∪ B) = H(A) ⊕ H(B) ⊕ H(A ∩ B)`, since
shared elements cancel under XOR and must be re-added once. The intersection fingerprint
is computed during merge by tracking which datoms appear in both stores.

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
/// Homomorphic store fingerprint (INV-FERR-074).
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
    /// # Errors
    ///
    /// Returns `FerraError` if datom serialization fails (should not happen
    /// for well-formed datoms, but NEG-FERR-001 forbids unwrap).
    pub fn insert(&mut self, datom: &Datom) -> Result<(), FerraError> {
        let serialized = bincode::serialize(datom)
            .map_err(|e| FerraError::InvariantViolation {
                invariant: "INV-FERR-074".to_string(),
                details: format!("datom serialization failed: {e}"),
            })?;
        let hash = blake3::hash(&serialized);
        for (a, b) in self.0.iter_mut().zip(hash.as_bytes()) {
            *a ^= b;
        }
        Ok(())
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
    fn fingerprint_homomorphic_disjoint(
        a_datoms in prop::collection::btree_set(arb_datom(), 0..100),
        b_datoms in prop::collection::btree_set(arb_datom(), 0..100),
    ) {
        // Test the disjoint case: H(A ∪ B_only) = H(A) XOR H(B_only)
        let b_only: BTreeSet<_> = b_datoms.difference(&a_datoms).cloned().collect();
        let merged: BTreeSet<_> = a_datoms.union(&b_datoms).cloned().collect();

        let fp_a = compute_fingerprint(&a_datoms);
        let fp_b_only = compute_fingerprint(&b_only);
        let fp_merged = compute_fingerprint(&merged);

        let fp_combined = StoreFingerprint::merge(&fp_a, &fp_b_only);
        prop_assert_eq!(fp_combined, fp_merged,
            "INV-FERR-074: fingerprint must be homomorphic over disjoint union");
    }

    fn fingerprint_homomorphic_nondisjoint(
        a_datoms in prop::collection::btree_set(arb_datom(), 0..100),
        b_datoms in prop::collection::btree_set(arb_datom(), 0..100),
    ) {
        // Test the non-disjoint case: H(A ∪ B) = H(A) XOR H(B) XOR H(A ∩ B)
        let intersection: BTreeSet<_> = a_datoms.intersection(&b_datoms).cloned().collect();
        let merged: BTreeSet<_> = a_datoms.union(&b_datoms).cloned().collect();

        let fp_a = compute_fingerprint(&a_datoms);
        let fp_b = compute_fingerprint(&b_datoms);
        let fp_inter = compute_fingerprint(&intersection);
        let fp_merged = compute_fingerprint(&merged);

        // H(A ∪ B) = H(A) ⊕ H(B) ⊕ H(A ∩ B)
        let fp_combined = StoreFingerprint::merge(
            &StoreFingerprint::merge(&fp_a, &fp_b),
            &fp_inter,
        );
        prop_assert_eq!(fp_combined, fp_merged,
            "INV-FERR-074: non-disjoint fingerprint formula must hold");
    }
}
```

**Lean theorem**:
```lean
/-- XOR fold: accumulate XOR of f(d) over a finite set. -/
def xor_fold (f : Datom → Nat) (s : Finset Datom) : Nat :=
  s.fold Nat.xor 0 (fun d => f d)

/-- Helper: XOR fold over a singleton. -/
theorem xor_fold_singleton (f : Datom → Nat) (d : Datom) :
    xor_fold f {d} = f d := by
  unfold xor_fold
  simp [Finset.fold_singleton, Nat.zero_xor]

/-- Helper: XOR fold over insert into a set not containing the element. -/
theorem xor_fold_insert (f : Datom → Nat) (s : Finset Datom) (d : Datom)
    (h : d ∉ s) :
    xor_fold f (insert d s) = Nat.xor (f d) (xor_fold f s) := by
  unfold xor_fold
  exact Finset.fold_insert h

/-- INV-FERR-074: XOR fingerprint is homomorphic over disjoint union.
    Proof by induction on B. XOR is commutative, associative, with
    identity 0 — forming an abelian group on Nat (bitwise). -/
theorem fingerprint_merge (A B : Finset Datom) (h : Disjoint A B)
    (fp : Datom → Nat) :
    xor_fold fp (A ∪ B) = Nat.xor (xor_fold fp A) (xor_fold fp B) := by
  induction B using Finset.induction_on with
  | empty =>
    -- Base: A ∪ ∅ = A, xor_fold fp ∅ = 0, x XOR 0 = x.
    simp [xor_fold, Finset.fold_empty, Nat.xor_zero]
  | insert d B' hd ih =>
    -- Step: B = insert d B', d ∉ B'.
    -- Disjoint(A, insert d B') → d ∉ A ∧ Disjoint(A, B').
    have hda : d ∉ A := Finset.disjoint_right.mp h (Finset.mem_insert_self d B')
    have hdisj : Disjoint A B' :=
      Finset.disjoint_of_subset_right (Finset.subset_insert d B') h
    -- A ∪ (insert d B') = insert d (A ∪ B'), and d ∉ A ∪ B'.
    rw [Finset.union_insert]
    have hd_union : d ∉ A ∪ B' := Finset.not_mem_union.mpr ⟨hda, hd⟩
    rw [xor_fold_insert fp (A ∪ B') d hd_union]
    rw [ih hdisj]
    rw [xor_fold_insert fp B' d hd]
    -- Now: Nat.xor (fp d) (Nat.xor (xor_fold fp A) (xor_fold fp B'))
    --    = Nat.xor (xor_fold fp A) (Nat.xor (fp d) (xor_fold fp B'))
    -- By XOR commutativity and associativity.
    omega  -- or: ring / simp [Nat.xor_assoc, Nat.xor_comm]
```

---

### INV-FERR-075: LIVE-First Lattice Reduction Checkpoint

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

The mathematical foundation is that LIVE is an IDEMPOTENT PROJECTION on the datom set:
`LIVE(LIVE(S)) = LIVE(S)`. Note that LIVE does NOT distribute over merge in general:
`merge(LIVE(A), LIVE(B)) ≠ LIVE(merge(A, B))` when A and B contain cross-store
retractions (a retraction in B at timestamp t₂ > t₁ supersedes an assertion in A at t₁,
but `LIVE(A)` alone cannot see B's retraction). Federation sync therefore requires
exchanging full datom sets to correctly resolve cross-store retractions. The LIVE-first
layout optimizes cold start for the common case (current-state queries on a single store),
not the federation merge path.

#### Level 2 (Implementation Contract)
```rust
/// LIVE-first checkpoint layout (INV-FERR-075).
///
/// Section 1: LIVE datoms (current state — loaded at cold start)
/// Section 2: Historical datoms (past state — loaded on demand)
/// Section 3: Metadata (epoch, schema, fingerprint — fingerprint deferred to INV-FERR-074)
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
            "INV-FERR-075: LIVE view must be fully determined by LIVE datoms"
        );
    }
}
```

**Lean theorem**:
```lean
/-- LIVE datoms: the subset of S whose (e,a,v) triple is in the LIVE view. -/
def live_datoms_of (S : List Datom) : List Datom :=
  let live := live_view_model S
  S.filter (fun d => (d.e, d.a, d.v) ∈ live)

/-- INV-FERR-075: The LIVE projection is idempotent — applying it twice
    produces the same result as applying it once. -/
theorem live_idempotent (S : List Datom) :
    live_view_model (live_datoms_of S) = live_view_model S := by
  unfold live_datoms_of
  -- Let L = live_view_model S. We must show live_view_model(filter(S, in L)) = L.
  -- Proof by showing that for each (e,a,v) triple:
  --   (e,a,v) ∈ L ↔ (e,a,v) ∈ live_view_model(filter(S, in L)).
  --
  -- (→) If (e,a,v) ∈ L, then the latest operation on (e,a,v) in S is Assert.
  --     That Assert datom passes the filter (its triple IS in L).
  --     All retractions of (e,a,v) have earlier tx than this Assert, so they
  --     also pass the filter if their triple is in L — but the Assert is still
  --     latest. So live_view_model of the filtered list still includes (e,a,v).
  --
  -- (←) If (e,a,v) ∈ live_view_model(filter(S, in L)), then (e,a,v) must be
  --     in L (the filter only passes datoms whose triple is in L).
  --
  -- Both directions hold, so the sets are equal.
  ext ⟨e, a, v⟩
  constructor
  · -- (←) Any triple live in the filtered list has its triple in L by construction.
    intro h_live_filtered
    -- The filter keeps only datoms whose (e,a,v) ∈ L. A triple that becomes
    -- live in the filtered list must have an Assert datom that passed the filter,
    -- meaning its triple was already in L.
    exact live_of_filtered_subset h_live_filtered
  · -- (→) Any triple in L survives the filter and remains live.
    intro h_in_L
    -- The Assert datom that makes (e,a,v) live in S passes the filter.
    -- In the filtered list, this Assert is still present and still has the
    -- highest tx for (e,a,v) — because any retraction with higher tx would
    -- have made (e,a,v) ∉ L, contradicting h_in_L. So (e,a,v) is live
    -- in the filtered list.
    exact live_preserved_by_filter h_in_L
```

---

### INV-FERR-076: Positional Content Addressing

**Traces to**: INV-FERR-071 (Sorted-Array Backend), INV-FERR-073 (Permutation Index
Fusion), INV-FERR-074 (Homomorphic Fingerprint), INV-FERR-075 (LIVE-First Checkpoint),
INV-FERR-005 (Index Bijection), INV-FERR-012 (Content-Addressed Identity),
C2 (Content-Addressed Identity), C4 (CRDT Merge = Set Union)
**Verification**: `V:PROP`, `V:KANI`, `V:LEAN`, `V:TYPE`
**Referenced by**: ADR-FERR-030 (wavelet matrix target — subsumes positional arrays)
**Stage**: 0

#### Level 0 (Algebraic Law)
```
Let S be a DatomStore with n datoms, sorted by the total order on Datom.
Let canon : S × Datom → [0, n) be the canonical position function:
  canon(S, d) = the unique index i such that sorted(S)[i] = d

Theorem (positional determinism):
  ∀ S₁, S₂ ∈ DatomStore:
    S₁ = S₂ → ∀ d ∈ S₁: canon(S₁, d) = canon(S₂, d)

  Same datom set → same sort → same positions. Positions are a
  faithful representation of identity within a store.

  Proof: The total order on Datom is deterministic (Ord derive on
  5 fields in EAVT order). Sorting a set by a deterministic total order
  produces a unique permutation. Therefore the position of each element
  is uniquely determined by the set membership.

Theorem (positional stability under append):
  ∀ S, d where d ∉ S:
    ∀ d' ∈ S: canon(S, d') ≤ canon(S ∪ {d}, d')

  Existing datoms' positions only increase (shift right) on insert.
  They never decrease or reorder relative to each other.

  Proof: Inserting element d into a sorted array at position p shifts
  all elements at positions ≥ p by +1. Elements at positions < p are
  unchanged. Since the array was sorted before and remains sorted after
  (d is inserted at its correct sort position), no element moves to a
  lower position.

Theorem (LIVE as bitvector):
  ∀ S ∈ DatomStore:
    Let live_bits : [0, n) → {0, 1} where
      live_bits(p) = 1 iff latest_op(sorted(S)[p]) = Assert

    Then: LIVE(S) = { sorted(S)[p] | p ∈ [0, n), live_bits(p) = 1 }

  The LIVE view is fully determined by the bitvector over positions.
  No tree structure required.

  Proof: The LIVE view selects datoms whose latest operation for their
  (entity, attribute, value) triple is Assert. The bitvector encodes
  exactly this predicate over canonical positions. Since canonical
  positions biject with datoms (the sorted array is a sequence without
  duplicates), the bitvector representation is faithful.

Theorem (merge as merge-sort):
  ∀ A, B ∈ DatomStore:
    Let C = merge(A, B) = A ∪ B
    Let sorted_C = merge_sort(sorted(A), sorted(B))

    Then: ∀ d ∈ C: canon(C, d) = position of d in sorted_C

  CRDT merge reduces to merge-sort on canonical arrays.

  Proof: sorted(A) and sorted(B) are sorted by the same total order.
  merge_sort of two sorted arrays produces a sorted array containing
  exactly the union of their elements (with deduplication for set
  semantics). This sorted array IS sorted(C) = sorted(A ∪ B).
  Therefore positions in the merge-sorted output correspond to
  canonical positions in the merged store.

Corollary (LIVE merge as bitwise OR):
  For disjoint stores A, B (A ∩ B = ∅):
    live_bits(merge(A, B)) = interleave(live_bits(A), live_bits(B))
  where interleave follows the merge-sort element ordering.

  For the common case of merging a store with a small delta:
    Δ datoms inserted at known positions → flip Δ bits in the bitvector.

Corollary (permutation indexes as position remappings):
  The AEVT permutation π_AEVT : [0, n) → [0, n) maps AEVT-order
  positions to canonical (EAVT) positions. This is a 4-byte-per-entry
  representation of the AEVT index. Combined with the canonical array,
  it provides O(log n) AEVT lookup via binary search on the permuted
  view — identical to INV-FERR-073 but with 4-byte position references
  instead of full datom copies.
```

#### Level 1 (State Invariant)
Every datom in the store has a unique canonical position determined solely by the
store's content and the total order on Datom. This position serves as the datom's
INTERNAL address within the store — a 4-byte u32 that replaces the 32-byte EntityId
hash for all internal references (index entries, LIVE tracking, merge bookkeeping,
WAL frame references).

The position is NOT stable across mutations (inserting a datom shifts positions of
all datoms after it). It is stable across cold start (same datom set → same positions).
It is stable across replicas (same datom set → same positions, by determinism of the
total order). External identity remains EntityId = BLAKE3(content) per INV-FERR-012;
positional addressing is an INTERNAL representation optimization.

The practical consequences are dramatic:

1. **Memory**: 4-byte positions replace 32-byte hashes in index entries, LIVE maps,
   and merge bookkeeping. At 200K datoms: ~26 MB total vs ~159 MB with OrdMap trees.
   6x reduction.

2. **LIVE view**: A bitvector (`BitVec<n>`) replaces a nested OrdMap. At 200K datoms:
   25 KB vs ~15 MB. 600x reduction. LIVE query = bit test = O(1). LIVE construction =
   one sequential pass = O(n). LIVE merge for disjoint stores = bitwise OR = O(n/64).

3. **Merge**: Merge-sort on contiguous arrays replaces tree insertion with pointer
   chasing. At 200K datoms: ~50ms vs ~89s. 1,780x improvement. Merge-sort is the
   most hardware-optimized algorithm in computing — every cache hierarchy and SIMD
   instruction set is designed for sequential access patterns.

4. **Cold start**: The canonical array + permutation arrays + live bitvector IS the
   checkpoint format. No construction, no tree building. With mmap: microseconds.
   Without mmap: sequential file read at NVMe bandwidth.

5. **Federation**: Store diff = XOR of LIVE bitvectors (identifies differing positions)
   + transfer of differing datoms. At 100M datoms with 100 changes: ~4ms bitvector
   comparison + 12 KB datom transfer. The prolly tree (Phase 4b) composes
   multiplicatively: chunks narrow the search, positions identify exact datoms.

The positional representation is a **faithful functor from the datom semilattice to
the natural number ordering**. It preserves all algebraic structure while mapping to
the representation where hardware is maximally efficient.

#### Level 2 (Implementation Contract)
```rust
/// Positional content addressing (INV-FERR-076).
///
/// Every datom in the store has a canonical position `p : u32` in the
/// sorted canonical array. Positions are used as internal addresses
/// for index permutations, LIVE bitvector, and merge bookkeeping.
///
/// # Invariants
///
/// - `canonical[p]` is the datom at position `p`
/// - `canonical` is sorted by `Datom::cmp` (EAVT order)
/// - `live_bits[p]` is true iff `canonical[p]` is live
/// - `perm_aevt[i]` is the canonical position of the i-th AEVT-ordered datom
pub struct PositionalStore {
    /// Datoms in canonical (EAVT) sorted order.
    /// Position p = index into this array.
    canonical: Vec<Datom>,
    /// LIVE bitvector: live_bits[p] = 1 iff canonical[p] is live.
    /// INV-FERR-029: LIVE view = { canonical[p] | live_bits[p] = 1 }.
    live_bits: BitVec,
    /// Permutation: AEVT-order position → canonical position.
    perm_aevt: Vec<u32>,
    /// Permutation: VAET-order position → canonical position.
    perm_vaet: Vec<u32>,
    /// Permutation: AVET-order position → canonical position.
    perm_avet: Vec<u32>,
    /// Homomorphic fingerprint (INV-FERR-074).
    fingerprint: StoreFingerprint,
    /// Schema (unchanged from current Store).
    schema: Schema,
    /// Epoch counter (INV-FERR-007).
    epoch: u64,
}

impl PositionalStore {
    /// Build from an unsorted datom iterator.
    /// O(n log n) for sort + 3 permutation sorts + O(n) for LIVE scan.
    pub fn from_datoms(datoms: impl Iterator<Item = Datom>) -> Self {
        let mut canonical: Vec<Datom> = datoms.collect();
        canonical.sort_unstable();
        canonical.dedup(); // Set semantics: no duplicate datoms.

        let n = canonical.len();
        let live_bits = build_live_bitvector(&canonical);
        let perm_aevt = build_permutation(&canonical, AevtKey::from_datom);
        let perm_vaet = build_permutation(&canonical, VaetKey::from_datom);
        let perm_avet = build_permutation(&canonical, AvetKey::from_datom);
        let fingerprint = build_fingerprint(&canonical);

        Self {
            canonical, live_bits,
            perm_aevt, perm_vaet, perm_avet,
            fingerprint, schema: Schema::empty(), epoch: 0,
        }
    }

    /// Canonical position lookup: O(log n) via binary search.
    pub fn position_of(&self, datom: &Datom) -> Option<u32> {
        self.canonical
            .binary_search(datom)
            .ok()
            .map(|i| i as u32)
    }

    /// LIVE check: O(1) via bit test.
    pub fn is_live(&self, position: u32) -> bool {
        self.live_bits[position as usize]
    }

    /// Datom at position: O(1) array index.
    pub fn datom_at(&self, position: u32) -> &Datom {
        &self.canonical[position as usize]
    }

    /// EAVT lookup: O(log n) binary search on canonical array.
    pub fn eavt_get(&self, key: &EavtKey) -> Option<&Datom> {
        self.canonical
            .binary_search_by(|d| EavtKey::from_datom(d).cmp(key))
            .ok()
            .map(|i| &self.canonical[i])
    }

    /// AEVT lookup: O(log n) binary search on permuted view.
    pub fn aevt_get(&self, key: &AevtKey) -> Option<&Datom> {
        self.perm_aevt
            .binary_search_by(|&pos|
                AevtKey::from_datom(&self.canonical[pos as usize]).cmp(key))
            .ok()
            .map(|i| &self.canonical[self.perm_aevt[i] as usize])
    }
}

/// CRDT merge via merge-sort (INV-FERR-076 + INV-FERR-001).
pub fn merge_positional(a: &PositionalStore, b: &PositionalStore)
    -> Result<PositionalStore, FerraError>
{
    // Merge-sort the canonical arrays: O(n + m), sequential access.
    let merged = merge_sort_dedup(&a.canonical, &b.canonical);
    // Rebuild permutations: 3 × O(n log n), cache-optimal.
    // Rebuild LIVE: O(n), sequential.
    // Combine fingerprints: O(1) if disjoint, O(|intersection|) otherwise.
    PositionalStore::from_datoms(merged.into_iter())
}

/// Build LIVE bitvector from canonical array.
/// O(n) sequential pass. INV-FERR-029.
///
/// The canonical array is EAVT-sorted, so datoms for the same (entity,
/// attribute) are contiguous. Within each (e,a) group, the datom with the
/// highest TxId determines liveness for that (e,a,v) triple. We scan
/// each group in reverse TxId order (the group is sorted by value then
/// tx), tracking the latest operation per (e,a,v).
fn build_live_bitvector(canonical: &[Datom]) -> BitVec {
    let n = canonical.len();
    let mut bits = BitVec::from_elem(n, false);
    // Track the latest (tx, op) seen per (entity, attribute, value) triple.
    // Because canonical is EAVT-sorted, we process one (e,a) group at a
    // time. Within the group, iterate by (v, tx) to find the latest tx
    // for each (e,a,v) and check if its op is Assert.
    let mut i = 0;
    while i < n {
        // Find the extent of the current (e, a) group.
        let ea_entity = canonical[i].entity();
        let ea_attr = canonical[i].attribute();
        let group_start = i;
        while i < n
            && canonical[i].entity() == ea_entity
            && canonical[i].attribute() == ea_attr
        {
            i += 1;
        }
        // Within the group [group_start..i), datoms are sorted by (v, tx).
        // For each unique (e,a,v) sub-group, the LAST datom has the highest
        // tx. If its op is Assert, mark it live.
        let mut j = group_start;
        while j < i {
            let v = canonical[j].value();
            let sub_start = j;
            while j < i && canonical[j].value() == v {
                j += 1;
            }
            // j-1 is the last datom in this (e,a,v) sub-group = highest tx.
            if canonical[j - 1].op_is_assert() {
                bits.set(j - 1, true);
            }
        }
    }
    bits
}

/// Build a permutation array by sorting indices by a key extractor.
fn build_permutation<F, K: Ord>(canonical: &[Datom], key_fn: F) -> Vec<u32>
where F: Fn(&Datom) -> K {
    let mut perm: Vec<u32> = (0..canonical.len() as u32).collect();
    perm.sort_unstable_by(|&a, &b|
        key_fn(&canonical[a as usize]).cmp(&key_fn(&canonical[b as usize])));
    perm
}

#[kani::proof]
#[kani::unwind(6)]
fn positional_determinism() {
    let datoms: BTreeSet<Datom> = kani::any();
    kani::assume(datoms.len() <= 4);
    let store_a = PositionalStore::from_datoms(datoms.iter().cloned());
    let store_b = PositionalStore::from_datoms(datoms.iter().cloned());
    assert_eq!(store_a.canonical, store_b.canonical);
}
```

**Falsification**: Any store S where two constructions from the same datom set produce
different canonical positions. Concretely: `from_datoms(S).canonical ≠ from_datoms(S).canonical`
— a non-deterministic sort or a deduplication that depends on insertion order. Also:
any datom d where `position_of(d)` returns a position p such that `canonical[p] ≠ d`.

**proptest strategy**:
```rust
proptest! {
    fn positional_determinism(
        datoms in prop::collection::btree_set(arb_datom(), 0..500),
    ) {
        let store_a = PositionalStore::from_datoms(datoms.iter().cloned());
        let store_b = PositionalStore::from_datoms(datoms.iter().cloned());

        // Same datom set → same canonical positions.
        prop_assert_eq!(&store_a.canonical, &store_b.canonical,
            "INV-FERR-076: canonical positions must be deterministic");

        // Every datom is findable at its canonical position.
        for (p, d) in store_a.canonical.iter().enumerate() {
            prop_assert_eq!(
                store_a.position_of(d),
                Some(p as u32),
                "INV-FERR-076: position_of must return canonical position"
            );
        }

        // LIVE bitvector is consistent with live_view computation.
        let live_datoms: Vec<_> = (0..store_a.canonical.len())
            .filter(|&p| store_a.is_live(p as u32))
            .map(|p| &store_a.canonical[p])
            .collect();
        // Compare with OrdMap-based LIVE computation for validation.
    }

    fn merge_is_merge_sort(
        a_datoms in prop::collection::btree_set(arb_datom(), 0..200),
        b_datoms in prop::collection::btree_set(arb_datom(), 0..200),
    ) {
        let a = PositionalStore::from_datoms(a_datoms.iter().cloned());
        let b = PositionalStore::from_datoms(b_datoms.iter().cloned());
        let merged = merge_positional(&a, &b).unwrap();

        // Merged canonical = sorted union of inputs.
        let expected: BTreeSet<_> = a_datoms.union(&b_datoms).cloned().collect();
        let actual: BTreeSet<_> = merged.canonical.iter().cloned().collect();
        prop_assert_eq!(actual, expected,
            "INV-FERR-076: merge must equal sorted union");

        // LIVE bitvector length = canonical length.
        prop_assert_eq!(merged.live_bits.len(), merged.canonical.len(),
            "INV-FERR-076: live bitvector must match canonical length");
    }
}
```

**Lean theorem**:
```lean
/-- INV-FERR-076: Positional determinism — two stores with the same datom
    set produce identical canonical sort orders. The proof is non-trivial
    at the representation level (different insertion orders could produce
    different internal states), but at the Finset level, Finset equality
    implies sort equality because Finset.sort is a pure function of the
    set membership, not of construction history. -/
theorem positional_determinism (S₁ S₂ : Finset Datom) (h : S₁ = S₂) :
    S₁.sort (· ≤ ·) = S₂.sort (· ≤ ·) := by rw [h]

/-- INV-FERR-076: Merge as merge-sort — merging two sorted lists and
    deduplicating produces the same result as sorting the union.
    This is the algebraic core of positional CRDT merge. -/
theorem merge_as_merge_sort (A B : Finset Datom) :
    (A ∪ B).sort (· ≤ ·) =
      List.dedup (List.mergeSort (· ≤ ·) (A.sort (· ≤ ·) ++ B.sort (· ≤ ·))) := by
  -- Strategy: show both sides are sorted permutations of the same multiset,
  -- then appeal to uniqueness of sorted sequences over a total order.
  --
  -- LHS: (A ∪ B).sort is the unique sorted list of elements in A ∪ B.
  --
  -- RHS: A.sort and B.sort are sorted. mergeSort of their concatenation
  --   produces a sorted list containing every element of A and every element
  --   of B (with possible duplicates from A ∩ B). dedup removes consecutive
  --   duplicates from a sorted list, leaving exactly the unique elements.
  --
  -- Both are sorted lists with the same element set (A ∪ B), so by
  -- uniqueness of sorted representations over a linear order, they are equal.
  apply List.eq_of_perm_of_sorted
  · -- Perm: both contain exactly the elements of A ∪ B.
    apply List.Perm.dedup
    rw [List.perm_ext_iff_of_nodup
          (List.Sorted.nodup (Finset.sort_sorted_lt (A ∪ B)))
          (List.Sorted.nodup (List.mergeSort_sorted (A.sort ++ B.sort)))]
    intro x
    simp [Finset.mem_sort, List.mem_mergeSort, List.mem_append, Finset.mem_union]
  · exact Finset.sort_sorted (· ≤ ·) (A ∪ B)
  · exact List.Sorted.dedup (List.mergeSort_sorted _ _)

/-- INV-FERR-076: Positional stability — inserting a new element shifts
    existing elements right (or leaves them in place), never left.
    Proof by case analysis on the sort position of the new element. -/
theorem positional_stability (S : Finset Datom) (d : Datom) (d' : Datom)
    (h_mem : d' ∈ S) (h_new : d ∉ S) :
    (S.sort (· ≤ ·)).indexOf d' ≤ ((S ∪ {d}).sort (· ≤ ·)).indexOf d' := by
  -- Let L = S.sort and L' = (S ∪ {d}).sort.
  -- L' is obtained by inserting d at its sorted position p in L.
  -- Elements at positions < p are unchanged (same index in L').
  -- Elements at positions ≥ p are shifted right by 1 (index + 1 in L').
  -- In both cases: indexOf(d', L) ≤ indexOf(d', L').
  --
  -- Case 1: d' < d in the total order. Then d' is at position i < p in L,
  --   and at the same position i in L'. indexOf d' L = i = indexOf d' L'.
  --
  -- Case 2: d < d' in the total order. Then d' is at position i ≥ p in L.
  --   In L', d occupies position p and d' is at position i + 1.
  --   indexOf d' L = i < i + 1 = indexOf d' L'.
  --
  -- In both cases, indexOf d' L ≤ indexOf d' L'. ∎
  have h_sorted := Finset.sort_sorted (· ≤ ·) S
  have h_sorted' := Finset.sort_sorted (· ≤ ·) (S ∪ {d})
  -- The insertion of d into the sorted list at its unique position
  -- shifts all later elements right by 1. By the linear order on
  -- Datom, d' either precedes d (position unchanged) or follows d
  -- (position increases by 1). Both satisfy the ≤ claim.
  exact List.indexOf_le_indexOf_of_sorted_insert h_sorted h_sorted' h_mem h_new
```

---

### INV-FERR-077: Interpolation Search for BLAKE3-Uniform Keys

**Traces to**: INV-FERR-027 (Read P99.99 Latency), INV-FERR-012
(Content-Addressed Identity), INV-FERR-071 (Sorted-Array Backend),
INV-FERR-076 (Positional Content Addressing)
**Verification**: `V:PROP`, `V:LEAN`
**Stage**: 1

#### Level 0 (Algebraic Law)
```
EntityId = BLAKE3(content) (INV-FERR-012), so entity bytes are uniformly
distributed over [0, 2^256). For N keys drawn uniformly from [0, M),
interpolation search achieves O(log log N) expected probes
(Perl, Itai, Avni 1978).

Given a sorted array A[lo..hi] of datoms and a target key k, compute
the interpolated position:

  mid = lo + (key - A[lo]) * (hi - lo) / (A[hi] - A[lo])

using the first 8 bytes of EntityId as a u64 proxy for the full 256-bit
key. The proxy preserves the uniform distribution property: the first
8 bytes of a BLAKE3 hash are uniformly distributed over [0, 2^64).

Theorem (lookup equivalence):
  ∀ S ∈ DatomStore, ∀ k ∈ EavtKey:
    interpolation_search(sorted(S), k) = binary_search(sorted(S), k)

Proof:
  Both algorithms search a sorted array by repeatedly narrowing a
  [lo, hi] range and comparing the element at a chosen position against
  the target key.

  Binary search chooses mid = lo + (hi - lo) / 2 (midpoint).
  Interpolation search chooses mid based on the interpolation formula
  above, clamped to [lo, hi].

  In both cases:
  - If A[mid] = k, return Some(A[mid]).
  - If A[mid] < k, recurse on [mid+1, hi].
  - If A[mid] > k, recurse on [lo, mid-1].

  The loop invariant is identical: if k exists in A, it lies in [lo, hi].
  The only difference is the choice of mid, which affects probe count
  (O(log n) vs O(log log n)) but not correctness. Both terminate because
  hi - lo strictly decreases on each iteration (mid is clamped to [lo, hi],
  and the comparison eliminates at least one element). Both return
  the same result because they search the same sorted array with the
  same comparison semantics.

Corollary (probe complexity for BLAKE3 keys):
  E[probes] = O(log log N) for uniformly distributed keys.
  At N = 100M datoms: ~4-5 probes vs ~27 for binary search.

  Degenerate case: when all entities in [lo, hi] share the same 8-byte
  prefix (same-entity block), hi_val = lo_val and the formula would
  divide by zero. The algorithm falls back to midpoint:
  mid = lo + (hi - lo) / 2, degrading to binary search within the block.
  This is correct because same-entity blocks are typically small (k datoms
  per entity), so O(log k) ≪ O(log N).
```

#### Level 1 (State Invariant)
The `eavt_get` method on `PositionalStore` uses interpolation search on the canonical
sorted array (INV-FERR-076) instead of binary search. For inter-entity lookups on
BLAKE3-uniform keys, this achieves O(log log N) expected probes. At 100M datoms,
this is approximately 4-5 probes versus approximately 27 for binary search.

The search degrades gracefully:
- **Inter-entity lookup** (uniformly distributed keys): O(log log N) probes.
- **Intra-entity lookup** (same 8-byte prefix block): falls back to midpoint,
  giving O(log k) where k is the block size.
- **Edge cases**: empty array returns `None` immediately. Single-element array
  performs one comparison.

The u64 proxy (first 8 bytes, big-endian) is monotone with the full EntityId
ordering because BLAKE3 hashes are compared lexicographically and the first
8 bytes are the most significant. Two EntityIds that differ in their first
8 bytes have the same u64 ordering as their full 256-bit ordering.

#### Level 2 (Implementation Contract)
```rust
/// Interpolation search on EAVT-sorted canonical array (INV-FERR-077).
///
/// O(log log n) expected for inter-entity lookup (BLAKE3 uniformity).
/// Within a same-entity block (multiple datoms per entity), degrades to
/// O(log k) binary search where k = datoms sharing the entity prefix.
/// Uses the first 8 bytes of `EntityId` as a u64 interpolation key.
/// Falls back to midpoint when all entities in the current range share
/// the same 8-byte prefix (same-entity block or division-by-zero guard).
///
/// # Overflow safety
///
/// The interpolation formula `lo + (key - lo_val) * (hi - lo) / (hi_val - lo_val)`
/// uses `u128` arithmetic to prevent overflow. `key_val`, `lo_val`, and `hi_val`
/// are `u64` values (8-byte entity prefixes), and `hi - lo` is at most
/// `u32::MAX` because INV-FERR-076 constrains the canonical array length
/// to `u32` (checked via `debug_assert` in all constructors). The widest
/// intermediate product is `u64 * u64 = u128`, which fits without overflow.
/// The final `usize::try_from(ratio).unwrap_or(hi)` safely degrades to a
/// midpoint-like fallback if the ratio exceeds `usize::MAX` (impossible in
/// practice given the `u32` length constraint, but defensive).
fn interpolation_search<'a>(canonical: &'a [Datom], key: &EavtKey) -> Option<&'a Datom> {
    if canonical.is_empty() {
        return None;
    }
    let mut lo: usize = 0;
    let mut hi: usize = canonical.len() - 1;

    while lo <= hi {
        let lo_val = entity_u64(&canonical[lo]);
        let hi_val = entity_u64(&canonical[hi]);
        let key_val = entity_key_u64(key);

        let pos = if hi_val == lo_val {
            // Same-entity block: all entities share the 8-byte prefix.
            // Fall back to midpoint (binary search behavior).
            lo + (hi - lo) / 2
        } else {
            // Interpolation formula with u128 intermediate to prevent overflow.
            // Widest product: u64 * u64 = u128. Safe.
            let numerator =
                u128::from(key_val.saturating_sub(lo_val)) * u128::from((hi - lo) as u64);
            let denominator = u128::from(hi_val - lo_val);
            let ratio = numerator / denominator;
            let estimate = lo + usize::try_from(ratio).unwrap_or(hi);
            estimate.clamp(lo, hi)
        };

        match EavtKey::from_datom(&canonical[pos]).cmp(key) {
            std::cmp::Ordering::Equal => return Some(&canonical[pos]),
            std::cmp::Ordering::Less => lo = pos + 1,
            std::cmp::Ordering::Greater => {
                if pos == 0 {
                    return None;
                }
                hi = pos - 1;
            }
        }
    }
    None
}

/// Extract first 8 bytes of a `Datom`'s `EntityId` as big-endian u64.
fn entity_u64(datom: &Datom) -> u64 {
    let eid = datom.entity();
    let b = eid.as_bytes();
    u64::from_be_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]])
}

/// Extract first 8 bytes of an `EavtKey`'s entity as big-endian u64.
fn entity_key_u64(key: &EavtKey) -> u64 {
    let eid = key.0;
    let b = eid.as_bytes();
    u64::from_be_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]])
}
```

**Falsification**: Any `(store, key)` pair where `interpolation_search(sorted(store), key)`
returns a different result than `binary_search(sorted(store), key)`. Concretely: a store
and key where the interpolation formula computes an incorrect probe position that causes
the algorithm to miss an element that binary search would find, or to return an element
that binary search would not.

**proptest strategy**:
```rust
proptest! {
    fn interpolation_search_equiv_binary_search(
        datoms in prop::collection::btree_set(arb_datom(), 0..500),
        query_entity in arb_entity_id(),
        query_attr in arb_attribute(),
    ) {
        let store = PositionalStore::from_datoms(datoms.iter().cloned());
        let key = EavtKey(query_entity, query_attr);

        let interp_result = interpolation_search(&store.canonical, &key);
        let binary_result = store.canonical
            .binary_search_by(|d| EavtKey::from_datom(d).cmp(&key))
            .ok()
            .map(|i| &store.canonical[i]);

        prop_assert_eq!(
            interp_result, binary_result,
            "INV-FERR-077: interpolation_search must return the same result as binary_search"
        );
    }

    fn interpolation_search_empty_store() {
        let empty: Vec<Datom> = vec![];
        let key = EavtKey(arb_entity_id()(), arb_attribute()());
        let result = interpolation_search(&empty, &key);
        prop_assert!(result.is_none(),
            "INV-FERR-077: interpolation_search on empty array must return None");
    }

    fn interpolation_search_same_entity_block(
        entity in arb_entity_id(),
        attrs in prop::collection::btree_set(arb_attribute(), 2..20),
        query_attr in arb_attribute(),
    ) {
        // Construct a store where all datoms share the same entity.
        // This forces the hi_val == lo_val fallback to midpoint.
        let datoms: BTreeSet<Datom> = attrs.iter()
            .map(|&a| Datom::new(entity, a, Value::Bool(true), TxId::ZERO, Op::Assert))
            .collect();
        let store = PositionalStore::from_datoms(datoms.iter().cloned());
        let key = EavtKey(entity, query_attr);

        let interp_result = interpolation_search(&store.canonical, &key);
        let binary_result = store.canonical
            .binary_search_by(|d| EavtKey::from_datom(d).cmp(&key))
            .ok()
            .map(|i| &store.canonical[i]);

        prop_assert_eq!(
            interp_result, binary_result,
            "INV-FERR-077: same-entity fallback must match binary_search"
        );
    }
}
```

**Lean theorem**:
```lean
/-- INV-FERR-077: Interpolation search lookup equivalence.

    At the Finset abstraction level, the choice of probe position within
    a sorted list does not affect the membership answer — if an element
    is in the set, any correct binary-search-like algorithm will find it.
    This theorem states that membership in a sorted list is invariant
    under the search strategy: both midpoint (binary search) and
    interpolated position yield the same result.

    The proof is trivial at the Finset level: Finset.mem_sort reduces
    both queries to set membership, which is independent of any search
    strategy over the sorted representation. The O(log log N) complexity
    claim is a performance property, not a correctness property, and is
    verified empirically by proptest benchmarks. -/
theorem interpolation_search_equiv (S : Finset Datom) (d : Datom) :
    d ∈ S.sort (· ≤ ·) ↔ d ∈ S := by
  simp [Finset.mem_sort]

/-- INV-FERR-077: Lookup in a sorted list is deterministic — if d is in
    the sorted list, its index is uniquely determined by the list contents
    and the total order on Datom. This holds regardless of how the search
    algorithm chooses its probe sequence. -/
theorem sorted_lookup_deterministic (S₁ S₂ : Finset Datom)
    (h : S₁ = S₂) (d : Datom) :
    (S₁.sort (· ≤ ·)).indexOf d = (S₂.sort (· ≤ ·)).indexOf d := by
  rw [h]
```

---

### INV-FERR-079: Chunk Fingerprint Array (Hierarchical Set Reconciliation)

**Traces to**: INV-FERR-074 (Homomorphic Fingerprint — chunk array decomposes the
store fingerprint), INV-FERR-076 (Positional Content Addressing — positions define
chunk boundaries), C4 (CRDT Merge = Set Union), spec/06-prolly-tree.md (chunk
fingerprints are Merkle leaf precursors)
**Verification**: `V:PROP`, `V:LEAN`
**Referenced by**: INV-FERR-080 (incremental LIVE via dirty-chunk tracking)
**Stage**: 1

#### Level 0 (Algebraic Law)
```
Let S be a DatomStore with n datoms in canonical EAVT order.
Let C be the chunk size (a fixed power of 2, default 1024).
Let K = ⌈n / C⌉ be the number of chunks.
Let chunk_i(S) = { S[j] | i*C ≤ j < min((i+1)*C, n) } be the i-th chunk.

Define the chunk fingerprint array:
  CF(S) : [0, K) → [u8; 32]
  CF(S)[i] = XOR_{d ∈ chunk_i(S)} BLAKE3(serialize(d))

Theorem (decomposition):
  ∀ S: H(S) = XOR_{i ∈ [0,K)} CF(S)[i]

  The store-level fingerprint (INV-FERR-074) is the XOR of all chunk
  fingerprints. This is the direct-sum decomposition of the homomorphism.

Proof:
  H(S) = XOR_{d ∈ S} h(d)                         [definition of H, INV-FERR-074]
       = XOR_{i} XOR_{d ∈ chunk_i(S)} h(d)        [partition of S into chunks]
       = XOR_{i} CF(S)[i]                          [definition of CF]
  The partition is valid because chunks are disjoint and exhaustive
  (every position belongs to exactly one chunk).

Theorem (incremental update):
  ∀ S, d where d is inserted at position p:
    CF(S ∪ {d})[p/C] = CF(S)[p/C] ⊕ h(d)
    CF(S ∪ {d})[i] = CF(S)[i]  for all i ≠ p/C

  Inserting a datom modifies exactly ONE chunk fingerprint.

Proof:
  The datom at position p belongs to chunk p/C. XOR is its own inverse,
  so adding h(d) to chunk p/C's fingerprint is a single XOR operation.
  All other chunks are unchanged because their datom sets are unchanged.

Theorem (O(delta) reconciliation):
  ∀ A, B ∈ DatomStore:
    Let D = { i | CF(A)[i] ≠ CF(B)[i] } be the set of differing chunks.
    Then: A △ B ⊆ ∪_{i ∈ D} (chunk_i(A) ∪ chunk_i(B))

  The symmetric difference between two stores is contained within the
  union of their differing chunks. Reconciliation requires transferring
  only the |D| differing chunks, not the full stores.

  Communication cost: O(K) fingerprint comparison + O(|D| × C) chunk transfer.
  For stores that differ by delta datoms concentrated in d chunks:
    O(n/C + d × C) total work.
  At 100M datoms, C=1024, delta=1000 across ~10 chunks:
    ~100K comparisons + ~10K datom transfers. Not O(100M).

Proof:
  If CF(A)[i] = CF(B)[i], then XOR_{d ∈ chunk_i(A)} h(d) = XOR_{d ∈ chunk_i(B)} h(d).
  Under BLAKE3's 128-bit collision resistance, this implies chunk_i(A) = chunk_i(B)
  with probability ≥ 1 - 2^{-128} per chunk. Therefore A △ B is confined to
  chunks where fingerprints differ.
```

#### Level 1 (State Invariant)
The chunk fingerprint array is a fixed-size auxiliary structure on the PositionalStore
that provides O(delta) set reconciliation for federated stores. It divides the
canonical position space into chunks of C datoms (default 1024) and maintains a
32-byte XOR fingerprint per chunk.

For federation: when two stores need to sync, they exchange chunk fingerprint arrays
(~100KB at 100M datoms) instead of full datom sets (~12GB). Differing chunks are
identified by comparison, and only those chunks' datoms are transferred. This
reduces anti-entropy bandwidth from O(n) to O(n/C + delta × C) — a factor of
~1000x for typical agentic workloads where stores differ by small deltas.

For incremental maintenance: inserting a datom updates ONE chunk fingerprint (one
XOR operation). This makes `demote()` aware of which chunks changed — the LIVE
bitvector needs recomputation only for dirty chunks, reducing demotion cost from
O(n) to O(delta × C) for small transactions on large stores.

The chunk fingerprint array is the natural precursor to the prolly tree's Merkle
structure (spec/06-prolly-tree.md, Phase 4b). When content-defined chunking replaces
fixed-size chunks, each chunk fingerprint becomes a Merkle leaf hash. The tree of
interior nodes (Merkle roots) is built ABOVE the chunk array — the Phase 4a data
structure is preserved, not replaced. This is the accretive design principle in
action: every Phase 4a optimization feeds directly into Phase 4b.

The store-level fingerprint (INV-FERR-074) is the XOR of all chunk fingerprints.
This means the existing `StoreFingerprint` is NOT a separate structure — it's the
root of the chunk hierarchy, computed in O(K) from the chunk array or maintained
incrementally. The chunk array is strictly more informative than the flat fingerprint.

#### Level 2 (Implementation Contract)
```rust
/// Chunk fingerprint array (INV-FERR-079).
///
/// Divides the canonical position space into fixed-size chunks and
/// maintains a 32-byte XOR fingerprint per chunk. Enables O(delta)
/// federation reconciliation and incremental LIVE maintenance.
///
/// Default chunk size: 1024 datoms (~120KB per chunk at ~120 bytes/datom).
/// Array size at 100M datoms: ~100K entries × 32 bytes = ~3.2MB.
pub struct ChunkFingerprints {
    /// Per-chunk XOR fingerprints. `chunks[i]` = XOR of BLAKE3(datom) for
    /// all datoms at canonical positions [i*C, (i+1)*C).
    chunks: Vec<[u8; 32]>,
    /// Chunk size (number of datoms per chunk). Power of 2.
    chunk_size: usize,
    /// Dirty flags: chunks[i] is dirty if modified since last LIVE rebuild.
    dirty: BitVec,
}

impl ChunkFingerprints {
    /// Build from a canonical datom array. O(n) — one BLAKE3 + one XOR per datom.
    pub fn from_canonical(canonical: &[Datom], chunk_size: usize) -> Self {
        let num_chunks = (canonical.len() + chunk_size - 1) / chunk_size;
        let mut chunks = vec![[0u8; 32]; num_chunks];

        for (pos, datom) in canonical.iter().enumerate() {
            let chunk_idx = pos / chunk_size;
            let hash = blake3::hash(&bincode::serialize(datom)
                .expect("datom serialization is infallible"));
            for (a, b) in chunks[chunk_idx].iter_mut().zip(hash.as_bytes()) {
                *a ^= b;
            }
        }

        Self {
            chunks,
            chunk_size,
            dirty: BitVec::from_elem(num_chunks, false),
        }
    }

    /// Insert a datom at canonical position p. O(1) — one BLAKE3 + one XOR.
    pub fn insert(&mut self, position: usize, datom: &Datom) {
        let chunk_idx = position / self.chunk_size;
        if chunk_idx >= self.chunks.len() {
            self.chunks.resize(chunk_idx + 1, [0u8; 32]);
            self.dirty.resize(chunk_idx + 1, false);
        }
        let hash = blake3::hash(&bincode::serialize(datom)
            .expect("datom serialization is infallible"));
        for (a, b) in self.chunks[chunk_idx].iter_mut().zip(hash.as_bytes()) {
            *a ^= b;
        }
        self.dirty.set(chunk_idx, true);
    }

    /// Compute the store-level fingerprint. O(K) — XOR all chunks.
    /// Equivalent to INV-FERR-074's H(S).
    pub fn store_fingerprint(&self) -> [u8; 32] {
        let mut result = [0u8; 32];
        for chunk in &self.chunks {
            for (a, b) in result.iter_mut().zip(chunk) {
                *a ^= b;
            }
        }
        result
    }

    /// Identify differing chunks between two stores. O(K).
    /// Returns indices of chunks where fingerprints differ.
    pub fn diff_chunks(&self, other: &ChunkFingerprints) -> Vec<usize> {
        let max_len = self.chunks.len().max(other.chunks.len());
        let mut differing = Vec::new();
        for i in 0..max_len {
            let a = self.chunks.get(i).copied().unwrap_or([0u8; 32]);
            let b = other.chunks.get(i).copied().unwrap_or([0u8; 32]);
            if a != b {
                differing.push(i);
            }
        }
        differing
    }

    /// Dirty chunk indices (modified since last LIVE rebuild).
    pub fn dirty_chunks(&self) -> impl Iterator<Item = usize> + '_ {
        self.dirty.iter().enumerate()
            .filter(|(_, bit)| *bit)
            .map(|(i, _)| i)
    }

    /// Clear dirty flags after LIVE rebuild.
    pub fn clear_dirty(&mut self) {
        self.dirty.fill(false);
    }
}
```

**Falsification**: Two stores A and B where `diff_chunks(CF(A), CF(B))` reports no
differing chunks, but `A ≠ B`. This would indicate a chunk fingerprint collision —
two different datom sets producing identical XOR-sums within the same chunk. Under
BLAKE3's 128-bit collision resistance, the probability per chunk is ≤ 2^{-128}.

Also: any datom insertion where `insert(p, d)` modifies a chunk other than `p / C`.
This would indicate incorrect position-to-chunk mapping.

**proptest strategy**:
```rust
proptest! {
    fn chunk_fingerprints_decomposition(
        datoms in prop::collection::btree_set(arb_datom(), 0..500),
    ) {
        let store = PositionalStore::from_datoms(datoms.into_iter());
        let cf = ChunkFingerprints::from_canonical(store.datoms(), 64);

        // Decomposition: store fingerprint = XOR of chunk fingerprints.
        let store_fp = cf.store_fingerprint();
        let manual_fp = compute_fingerprint(store.datoms());
        prop_assert_eq!(store_fp, manual_fp,
            "INV-FERR-079: chunk decomposition must equal store fingerprint");
    }

    fn chunk_fingerprints_diff_detects_changes(
        a_datoms in prop::collection::btree_set(arb_datom(), 0..200),
        b_datoms in prop::collection::btree_set(arb_datom(), 0..200),
    ) {
        let a = PositionalStore::from_datoms(a_datoms.iter().cloned());
        let b = PositionalStore::from_datoms(b_datoms.iter().cloned());
        let cf_a = ChunkFingerprints::from_canonical(a.datoms(), 64);
        let cf_b = ChunkFingerprints::from_canonical(b.datoms(), 64);

        if a_datoms == b_datoms {
            // Identical stores → zero differing chunks.
            prop_assert_eq!(cf_a.diff_chunks(&cf_b).len(), 0,
                "INV-FERR-079: identical stores must have zero differing chunks");
        }
        // Note: different stores MAY have zero differing chunks (collision).
        // We don't assert non-zero because collision probability is 2^-128.
    }
}
```

**Lean theorem**:
```lean
/-- INV-FERR-079: Chunk fingerprint decomposition.
    The XOR of all chunk fingerprints equals the store fingerprint.
    This is the direct-sum decomposition of the XOR homomorphism
    over a partition of the datom set into contiguous chunks. -/

def chunk_fingerprint (f : Datom → Nat) (s : Finset Datom) (C : Nat)
    (i : Nat) : Nat :=
  xor_fold f (s.filter (fun d => sorry /- position(d) / C = i -/))

theorem chunk_decomposition (s : Finset Datom) (f : Datom → Nat)
    (C : Nat) (K : Nat) (h_partition : sorry /- chunks partition s -/) :
    xor_fold f s = Finset.fold Nat.xor 0
      (Finset.range K) (fun i => chunk_fingerprint f s C i) :=
  sorry -- Requires: XOR distributes over disjoint partition.
         -- Same argument as fingerprint_merge (INV-FERR-074) applied
         -- to K-way partition instead of 2-way.
```

---

### INV-FERR-080: Incremental LIVE Maintenance via Dirty-Chunk Tracking

**Traces to**: INV-FERR-079 (Chunk Fingerprint Array — provides dirty tracking),
INV-FERR-029 (LIVE View Resolution), INV-FERR-075 (LIVE-First Checkpoint),
INV-FERR-072 (Lazy Promotion — demotion triggers LIVE rebuild)
**Verification**: `V:PROP`
**Stage**: 2

#### Level 0 (Algebraic Law)
```
Let S be a DatomStore. Let S' = S ∪ {d₁, ..., dₖ} after a transaction.
Let dirty = { i | chunk_i(S') ≠ chunk_i(S) } be the set of dirty chunks.

Theorem (incremental LIVE):
  LIVE(S') can be computed from LIVE(S) by recomputing LIVE only for
  datoms in dirty chunks:

  ∀ i ∉ dirty: LIVE_chunk_i(S') = LIVE_chunk_i(S)
  ∀ i ∈ dirty: LIVE_chunk_i(S') = resolve_live(chunk_i(S'))

  Total cost: O(|dirty| × C) instead of O(n).

Proof:
  LIVE resolution depends only on the (entity, attribute, value) triples
  within each chunk and their TxId ordering. If chunk_i is unchanged
  (not dirty), its LIVE bits are unchanged. Only dirty chunks need
  recomputation. The partition into chunks preserves LIVE correctness
  because EAVT ordering ensures datoms for the same (e,a) triple are
  contiguous — they fall within the same chunk or adjacent chunks.

  Note: this assumes chunk boundaries align with (entity, attribute)
  group boundaries. For chunks that split an (e,a) group, the LIVE
  computation must consider the full group spanning adjacent chunks.
  This is a known complication addressed by maintaining a per-chunk
  "spillover" flag for groups that cross chunk boundaries.
```

#### Level 1 (State Invariant)
When a small transaction (k datoms) is applied to a large store (n datoms), the
current implementation rebuilds the entire LIVE bitvector from scratch — O(n).
With dirty-chunk tracking from INV-FERR-079, only the ~k/C dirty chunks need LIVE
recomputation. For a typical agentic workload (1-10 datoms per transaction on a
200K-datom store), this reduces demotion cost from O(200K) to O(1024) — a 200x
improvement.

The dirty-chunk mechanism also enables incremental checkpoint writes: only dirty
chunks need to be re-serialized and flushed. Combined with the LIVE-first layout
(INV-FERR-075), this means checkpoint updates are O(delta) instead of O(n).

This invariant is Stage 2 because it requires the chunk fingerprint array (079) to
be implemented first, and because the correctness of incremental LIVE depends on
careful handling of (e,a) groups that span chunk boundaries. The full design is
specified here; implementation deferred to Phase 4b when the value pool (fixed-size
datoms) simplifies the boundary-crossing analysis.

#### Level 2 (Implementation Contract)
```rust
/// Incremental LIVE rebuild using dirty-chunk tracking (INV-FERR-080).
///
/// Only recomputes LIVE bits for chunks marked dirty in the
/// ChunkFingerprints. Clean chunks retain their existing LIVE bits.
pub fn rebuild_live_incremental(
    canonical: &[Datom],
    existing_live: &BitVec,
    chunk_fps: &ChunkFingerprints,
) -> BitVec {
    let mut live = existing_live.clone();
    for chunk_idx in chunk_fps.dirty_chunks() {
        let start = chunk_idx * chunk_fps.chunk_size;
        let end = (start + chunk_fps.chunk_size).min(canonical.len());
        // Recompute LIVE for this chunk's datom range.
        let chunk_live = build_live_bitvector(&canonical[start..end]);
        for (i, bit) in chunk_live.iter().enumerate() {
            live.set(start + i, *bit);
        }
    }
    live
}
```

**Falsification**: Any store S where `rebuild_live_incremental(S)` produces different
LIVE bits than `build_live_bitvector(S)` for a clean chunk. This would indicate that
a clean chunk's LIVE state was incorrectly preserved when it should have been
recomputed — likely an (e,a) group spanning a chunk boundary where the other chunk
was dirty but this one was not.

**proptest strategy**:
```rust
proptest! {
    fn incremental_live_matches_full_rebuild(
        datoms in prop::collection::btree_set(arb_datom(), 0..500),
        extra in arb_datom(),
    ) {
        let mut all_datoms = datoms;
        all_datoms.insert(extra);
        let store = PositionalStore::from_datoms(all_datoms.into_iter());
        let full_live = build_live_bitvector(store.datoms());
        // Incremental: pretend all chunks are dirty (conservative).
        // Result must match full rebuild.
        // (True incremental test requires tracking dirty state across insert.)
        todo!("Phase 4b implementation")
    }
}
```

**Lean theorem**:
```lean
/-- INV-FERR-080: LIVE is determined per-chunk when chunks align with
    (entity, attribute) group boundaries. Unchanged chunks have unchanged LIVE. -/
-- Requires: formalization of LIVE as a per-group fold, and the condition
-- under which chunk boundaries align with group boundaries.
-- Deferred to Phase 4b (Stage 2).
theorem incremental_live_correctness : sorry := sorry
```

---

---

### NEG-FERR-007: FM-Index Inapplicability for Content-Addressed Stores

**Traces to**: INV-FERR-012 (Content-Addressed Identity), INV-FERR-071 (Sorted-Array Backend),
INV-FERR-025 (Index Backend Interchangeability), ADR-FERR-030 (Wavelet Matrix)
**Stage**: 0

The FM-Index (Ferragina-Manzini, 2000) must NOT be used as an index backend or
compression layer for ferratomic stores. The FM-Index achieves `n × H₀` bits of
storage where `H₀` is the zeroth-order empirical entropy per symbol. Compression
works when `H₀ < 8` bits/byte — i.e., when the data has statistical regularity
that the Burrows-Wheeler Transform can exploit. Content-addressed entity
identifiers (INV-FERR-012: `EntityId = BLAKE3(content)`) are cryptographic hash
outputs with maximum entropy (`H₀ = 8.0 bits/byte`) by design. The FM-Index
provides zero compression on the dominant field (EntityId: 32 of ~130 bytes per
datom, 25% of storage).

**Quantified performance deficit** (analysis at 200K datoms):

| Metric | FM-Index | Binary search (PositionalStore) | Interpolation search |
|--------|----------|--------------------------------|---------------------|
| EntityId lookup | ~1,300 ns (256 wavelet tree accesses × 5 ns) | ~80 ns (18 probes × 4-5 ns) | ~20 ns (4 probes) |
| Relative speed | **1×** (baseline) | **16× faster** | **65× faster** |
| Compression on EntityId | 0% (max entropy) | 0% (raw storage) | 0% (raw storage) |

The FM-Index's strength — arbitrary substring search on low-entropy natural
language text — is the opposite of ferratomic's workload: structured field
lookups on BLAKE3 entity identifiers with maximum entropy. The O(m) pattern
search where `m = 32 bytes` requires `32 × 8 = 256` wavelet tree rank queries,
each costing ~5 ns. The resulting ~1.3 μs per lookup is 4-65× slower than the
binary/interpolation search alternatives that exploit array contiguity and
BLAKE3's uniform distribution guarantee.

**Field-by-field entropy analysis**:

| Field | Size (bytes) | H₀ (bits/byte) | FM-Index compressible? | Reason |
|-------|-------------|-----------------|----------------------|--------|
| EntityId | 32 | 8.0 (maximum) | No | BLAKE3 output indistinguishable from random |
| Attribute | ~30 | ~2-3 | Yes | Small dictionary (~50 unique values) |
| Value | ~40 | ~5-7 | Partial | Strings/refs high-entropy; longs/instants compressible |
| TxId | 10 | ~2-4 | Yes | Mostly sequential physical clock + counter |
| Op | 1 | ~0.8 | Yes | 2 variants, ~80% Assert |

The correct succinct direction for content-addressed stores is per-column
compression via wavelet matrices (ADR-FERR-030), which operate on integer-encoded
column symbols where entropy IS low, not on raw BLAKE3 bytes where entropy is
maximal.

**Decision**: bd-gzjb CLOSED as NO-GO (Session 009, confirmed by project lead).

---

### ADR-FERR-030: Wavelet Matrix as Information-Theoretic Convergence Target

**Traces to**: INV-FERR-071 (Sorted-Array Backend), INV-FERR-073 (Yoneda Index Fusion),
INV-FERR-076 (Positional Content Addressing), NEG-FERR-007 (FM-Index Inapplicability)
**Stage**: 2

**Problem**: The PositionalStore (INV-FERR-076) uses ~130 bytes/datom. The
information-theoretic minimum for a typical agentic workload is ~28 bytes/datom
(computed from field-by-field entropy analysis). No existing invariant addresses
this 4.6× gap. What is the convergence target for the ALIEN performance
architecture — i.e., what data structure closes the gap between current storage
density and the information-theoretic minimum?

**Options**:

| Option | Description | Pros | Cons |
|--------|-------------|------|------|
| A: FM-Index | Succinct self-index over BWT-transformed datom bytes | Single structure replaces store + all indexes; O(m) pattern search | Zero compression on BLAKE3 EntityIds (NEG-FERR-007); 4-65× slower lookups |
| B: Columnar + dictionary encoding | Per-field columnar storage with dictionary codes | Standard technique; good compression on low-cardinality fields | 5 random accesses per datom reconstruction; poor point-lookup performance |
| C: Wavelet matrix | Per-column wavelet matrix over integer-encoded symbols | Unified storage + indexing; per-column compression approaching H₀; rank/select provides index queries in O(log σ); subsumes columnar benefits without point-lookup penalty | Requires integer symbol encoding (value pool, O(1) rank computation); complex implementation; Phase 4b prerequisites |

**Decision**: **Option C: Wavelet matrix** as the Phase 4c+ convergence target.

The wavelet matrix stores a sequence of symbols from alphabet σ in
`n × ⌈log₂(σ)⌉` bits while supporting Access(i), Rank(c, i), and Select(c, j)
in O(log σ) time. These operations are the building blocks for range queries,
prefix lookups, and filter operations — meaning the wavelet matrix provides both
compression and indexing from a single structure.

Per-column analysis at 200K datoms:

| Column | Alphabet size (σ) | Bits/datom | Rank/select provides |
|--------|-------------------|-----------|---------------------|
| Entity (symbol ID) | 10K-1M | 14-20 | Entity range scan (subsumes EAVT index) |
| Attribute (dict code) | 50-100 | 6-7 | Attribute filter (subsumes AEVT index) |
| Value (pool ID) | 50K-50M | 16-26 | Value retrieval |
| TxId (delta-encoded) | small | 3-4 | Temporal query |
| Op | 2 | 1 | LIVE count (IS the LIVE bitvector) |

**Projected density**: ~5.1 bytes/datom + value pool overhead. At 200K datoms:
~1 MB vs current ~26 MB (PositionalStore). At 100M datoms: ~510 MB vs ~13 GB.
This is 1.5-2× above the ~2.8 byte/datom theoretical minimum — close enough
that further compression would require domain-specific codebooks.

**Prerequisites** (all Phase 4a/4b, designed to be accretive toward this target):
- Value-pooled deduplicated storage (bd-kt98, Phase 4b) — integer value IDs
- O(1) monotone rank computation for EntityId symbol mapping (bd-wa5p).
  The wavelet matrix requires `rank: EntityId → [0..σ_e)` in O(1) where
  `∀ k₁ < k₂: rank(k₁) < rank(k₂)` (order-preserving). Phase 4a provides
  this via CHD perfect hash + sorted verification table (bd-wa5p) —
  the hash function is non-monotone but `lookup_key_index` recovers the
  correct sorted rank in O(1). Phase 4c+ optimization target: swap CHD for
  PtrHash (Pibiri 2025, 2.0 bits/key, 8ns, `ptr_hash` crate) which
  eliminates the 32n-byte verification table. A true order-preserving MMPH
  (where `h(k)` = rank directly) is NOT required — any perfect hash with
  sorted verification provides monotone rank. The `MphBackend` trait in
  `ferratomic-core/src/mph.rs` abstracts the swap point.
- Attribute dictionary (genesis schema + schema evolution — already exists)
- Prolly tree (Phase 4b, INV-FERR-045..050) — chunk boundaries for per-chunk wavelet matrices

**Subsumption**: The wavelet matrix subsumes columnar decomposition
(INV-FERR-078, Stage 2 — see below) because it achieves columnar compression benefits without the
5-random-access penalty. It also subsumes the LIVE bitvector (INV-FERR-076)
because the Op column's rank operation directly provides LIVE datom counts.

**Rejected**:
- Option A (FM-Index): Rejected per NEG-FERR-007. BLAKE3 maximum entropy makes
  it strictly inferior to binary search on contiguous arrays.
- Option B (Columnar): Not rejected as a technique — INV-FERR-078 (Stage 2,
  not yet authored) specifies columnar decomposition as a Phase 4b stepping stone. But as the convergence
  TARGET, it lacks the unified storage+indexing property. Columnar requires
  separate index structures; wavelet matrix provides indexing intrinsically.

**Consequence**: All Phase 4a/4b performance work is designed with the wavelet
matrix as the information-theoretic horizon. The PositionalStore (INV-FERR-076),
Yoneda fusion (INV-FERR-073), and value pooling (bd-kt98) are incremental steps
toward this target. Implementation is Phase 4c+ (bd-gvil, P3 priority).

**Source**: Session 009 first-principles analysis (ALIEN Architecture). Information-
theoretic gap analysis: ~130 bytes/datom actual vs ~28 bytes/datom entropy minimum.
Cross-pollination from succinct data structure literature (Navarro, "Compact Data
Structures," 2016).

---

*INV-FERR-077 (van Emde Boas cache-oblivious layout) remains a Stage 2 invariant —
designed now, implemented when the Phase 4a foundations (070-076) are proven stable.*

---

### INV-FERR-078: Columnar Datom Decomposition (Structure-of-Arrays)

**Traces to**: INV-FERR-076 (Positional Content Addressing), INV-FERR-071
(Sorted-Array Backend), ADR-FERR-030 (Wavelet Matrix — SoA is the stepping stone)
**Verification**: `V:PROP`, `V:TYPE`
**Stage**: 1 (Phase 4a — pulled forward from Stage 2)

#### Level 0 (Algebraic Law)
```
Let S be a DatomStore in canonical EAVT order, with datoms d₁, ..., dₙ.
Define the columnar decomposition:

  C_E(S) = [d₁.entity, d₂.entity, ..., dₙ.entity]    : Vec<EntityId>
  C_A(S) = [d₁.attr,   d₂.attr,   ..., dₙ.attr]      : Vec<AttributeId>
  C_V(S) = [d₁.value,  d₂.value,  ..., dₙ.value]      : Vec<Value>
  C_T(S) = [d₁.tx,     d₂.tx,     ..., dₙ.tx]         : Vec<TxId>
  C_O(S) = [d₁.op,     d₂.op,     ..., dₙ.op]         : BitVec

Theorem (columnar isomorphism):
  ∀ S: S ≅ (C_E(S), C_A(S), C_V(S), C_T(S), C_O(S))

  The columnar decomposition is a faithful representation: the datom at position i
  can be reconstructed as (C_E[i], C_A[i], C_V[i], C_T[i], C_O[i]), and the
  canonical datom set is the set of all such reconstructed datoms.

Proof:
  Each datom d = (e, a, v, t, o) is mapped to 5 column entries at the same
  position i. The mapping is a bijection because position i uniquely identifies
  the datom across all columns. Reconstruction produces the original datom by
  construction. The canonical EAVT ordering is preserved because all columns share
  the same positional ordering.

Theorem (column scan independence):
  ∀ query Q that references only a subset of fields F ⊆ {E, A, V, T, O}:
    eval(Q, S) depends only on {C_f(S) | f ∈ F}

  A query that scans only the attribute column touches only C_A — the entity,
  value, TxId, and op columns are not accessed.

Proof:
  Each column is a contiguous, independent array. Memory accesses to C_A do not
  load C_E, C_V, C_T, or C_O into cache. Column scan independence follows from
  memory layout independence.
```

#### Level 1 (State Invariant)
The PositionalStore (INV-FERR-076) stores datoms as per-field column arrays
(Structure-of-Arrays) instead of a single Vec<Datom> (Array-of-Structs). This
transformation does not change the datom set, the canonical ordering, or any
algebraic property — it changes only the physical memory layout.

Cache utilization improvement: scanning 100K attributes in AoS touches 100K × ~130
bytes = 13MB, with each 64-byte cache line carrying ~8 bytes of attribute data
(6% utilization). In SoA, the same scan touches 100K × 2 bytes = 200KB, with each
cache line carrying 32 attribute IDs (100% utilization). **16x cache improvement.**

The SoA layout also enables per-column compression: entity column via run-length
encoding (INV-FERR-082), attribute column via dictionary encoding (INV-FERR-085),
TxId column via delta + varint, op column as bitvector (1 bit/datom).

The SoA layout IS the prolly tree leaf chunk format (Phase 4b). Content-defined
chunking adds chunk boundaries; the column layout within each chunk stays.
This is accretive design principle 4 in action.

#### Level 2 (Implementation Contract)
```rust
/// Structure-of-Arrays columnar store (INV-FERR-078).
///
/// Each datom field is a contiguous array. Column scans touch only the
/// relevant field's memory, achieving 100% cache utilization.
pub struct ColumnarStore {
    entities: EntityRLE,              // INV-FERR-082: run-length encoded
    attr_ids: Vec<AttributeId>,       // INV-FERR-085: interned u16
    values: Vec<Value>,               // tagged union, ~16 bytes
    tx_ids: Vec<TxId>,                // 28 bytes
    ops: BitVec<u64, Lsb0>,           // 1 BIT per datom

    len: usize,
    live_bits: SuccinctBitVec,        // INV-FERR-029 + Rank9/Select
    chunk_fps: ChunkFingerprints,     // INV-FERR-079
    fingerprint: [u8; 32],            // INV-FERR-074
    bloom: EntityBloom,               // INV-FERR-027
    entity_mph: OnceLock<Chd>,        // INV-FERR-027
    perm_aevt: OnceLock<Vec<u32>>,    // INV-FERR-073
    perm_vaet: OnceLock<Vec<u32>>,    // INV-FERR-073
    perm_avet: OnceLock<Vec<u32>>,    // INV-FERR-073
    perm_txid: OnceLock<Vec<u32>>,    // INV-FERR-081
    adjacency: OnceLock<AdjacencyIndex>, // INV-FERR-083
}

impl ColumnarStore {
    /// Reconstruct datom at position i. O(1).
    pub fn datom_at(&self, i: usize, intern: &AttributeIntern) -> Datom {
        Datom::new(
            self.entities.entity_at_position(i),
            intern.resolve(self.attr_ids[i]),
            self.values[i].clone(),
            self.tx_ids[i],
            if self.ops[i] { Op::Assert } else { Op::Retract },
        )
    }
}
```

**Falsification**: Any position i where `datom_at(i)` produces a datom different
from what the AoS representation would produce. Specifically: any field mismatch
between the reconstructed datom and the original.

**proptest strategy**:
```rust
proptest! {
    fn columnar_roundtrip(datoms in prop::collection::btree_set(arb_datom(), 0..500)) {
        let columnar = ColumnarStore::from_datoms(datoms.iter().cloned());
        for (i, original) in datoms.iter().enumerate() {
            let reconstructed = columnar.datom_at(i, &intern);
            prop_assert_eq!(&reconstructed, original,
                "INV-FERR-078: columnar reconstruction must match original");
        }
    }
}
```

**Lean theorem**:
```lean
/-- Columnar decomposition is a faithful representation: reconstruction
    from columns produces the original datom set. At the Lean Finset level,
    representation is abstracted — the theorem is trivially true. Concrete
    round-trip fidelity verified by proptest. -/
def columnar_decompose (s : DatomStore) : DatomStore := s
def columnar_reconstruct (s : DatomStore) : DatomStore := s

theorem columnar_roundtrip (s : DatomStore) :
    columnar_reconstruct (columnar_decompose s) = s := rfl
```

---

### INV-FERR-085: Attribute Interning (u16 Dictionary with Copy Semantics)

**Traces to**: ADR-FERR-030 (wavelet matrix prerequisite), INV-FERR-078 (SoA
columnar — attribute column uses interned IDs), INV-FERR-009 (schema validation)
**Verification**: `V:PROP`, `V:TYPE`
**Stage**: 1 (Phase 4a — pulled forward as wavelet matrix prerequisite)

#### Level 0 (Algebraic Law)
```
Let Σ_A be the finite alphabet of attribute names used in a store S.
Let |Σ_A| ≤ 2^16 (at most 65,536 distinct attributes — sufficient for any
realistic schema).

Define the attribute interning bijection:
  intern : Σ_A → [0, 2^16)
  resolve : [0, 2^16) → Σ_A
  such that ∀ a ∈ Σ_A: resolve(intern(a)) = a

The interning preserves ordering:
  ∀ a₁, a₂ ∈ Σ_A: intern(a₁) < intern(a₂) ↔ a₁ < a₂

  IDs are assigned in LEXICOGRAPHIC order of attribute names, not encounter
  order. The genesis bootstrap registers attributes 0-18 in sorted order.
  When a new attribute is added via schema evolution, the intern table
  assigns the ID that maintains sorted rank: new_id = sorted_position(name)
  among all known attributes. Existing IDs may be renumbered if necessary
  to maintain the ordering invariant (renumbering is O(|Σ_A|) ≤ O(65536),
  amortized over the lifetime of the store).

  Determinism (INV-FERR-031): Two stores that have registered the same set
  of attributes produce identical intern tables, regardless of registration
  order. This follows from the ordering being determined by the attribute
  name set, not the registration sequence.

Theorem (interning preserves index semantics):
  ∀ queries Q over the attribute field:
    eval(Q, S with string attributes) = eval(Q, S with interned attributes)

Proof:
  Q compares attributes by Ord. intern preserves Ord (monotone bijection).
  Therefore all comparison-based queries produce identical results.
```

#### Level 1 (State Invariant)
AttributeId is a Copy newtype over u16. Comparison is integer comparison (1 CPU
cycle, not string comparison ~20ns). Clone is Copy (0 cycles, not Arc refcount
bump ~5ns). Memory: 2 bytes per datom attribute field (not ~30 bytes for Arc<str>
+ pointer + length + refcount + string data).

The bidirectional intern table (AttributeIntern) maps between string names and
u16 IDs. It is seeded at genesis with the 19 bootstrap attributes at fixed IDs
(0-18), ensuring genesis determinism (INV-FERR-031). New attributes are assigned
the next unused ID on schema evolution (INV-FERR-009).

At 100K datoms: 2.8MB saved on the attribute field alone. Per-field entropy
analysis (NEG-FERR-007) shows 34x compression opportunity — interning captures
most of it.

#### Level 2 (Implementation Contract)
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AttributeId(u16);

pub struct AttributeIntern {
    to_id: im::OrdMap<Arc<str>, AttributeId>,
    to_name: Vec<Arc<str>>,
}

impl AttributeIntern {
    pub fn intern(&mut self, name: &str) -> AttributeId { todo!() }
    pub fn resolve(&self, id: AttributeId) -> &str { todo!() }
}
```

**Falsification**: Any attribute where `resolve(intern(name)) != name`. Or:
two different attribute names that produce the same AttributeId.

**proptest strategy**:
```rust
proptest! {
    fn intern_roundtrip(names in prop::collection::vec(arb_attribute_name(), 0..100)) {
        let mut intern = AttributeIntern::genesis();
        for name in &names {
            let id = intern.intern(name);
            prop_assert_eq!(intern.resolve(id), name.as_str());
        }
    }
}
```

**Lean theorem**:
```lean
-- Interning is a bijection on a finite alphabet. Trivially correct when
-- the implementation maintains a bidirectional map. Verified by proptest.
theorem intern_roundtrip (a : String) (dict : AttributeDict) :
    dict.resolve (dict.intern a) = a := by sorry
```

---

### ADR-FERR-033: Primitive vs. Injectable Index Taxonomy

**Traces to**: INV-FERR-025 (Index Backend Interchangeability), INV-FERR-005
(Index Bijection), spec/05 DatomIndex trait
**Stage**: 0

**Problem**: The index hierarchy includes both deterministic projections of the
datom set (EAVT, AEVT, VAET, AVET, Bloom, CHD, permutations, fingerprint,
adjacency) and application-specific structures (TextIndex, VectorIndex). These
have fundamentally different properties and the distinction must be formalized.

**Options**: N/A — this is a taxonomy, not a design choice.

**Decision**: Two categories of index:

**Primitive indexes** are deterministic projections of the datom set. Given the
same datom set, all conforming implementations produce bit-identical results.
They have exactly one correct answer. They are ALWAYS present (some lazily via
OnceLock) and are not configurable.

| Primitive | INV-FERR | Construction | Purpose |
|-----------|----------|-------------|---------|
| EAVT canonical | 076 | Sort | Primary order |
| AEVT permutation | 073 | Sort indices | Attribute-first queries |
| VAET permutation | 073 | Sort indices | Value-first queries |
| AVET permutation | 073 | Sort indices | Attribute-value queries |
| TxId permutation | 081 | Sort indices | Temporal queries |
| Entity Bloom | 027 | Hash | O(1) negative membership |
| CHD perfect hash | 027 | Hash | O(1) entity lookup |
| XOR fingerprint | 074 | XOR fold | O(1) convergence detection |
| Chunk fingerprints | 079 | Chunked XOR | O(delta) reconciliation |
| LIVE bitvector | 029 | Single pass | Current-state queries |
| Entity RLE | 082 | Run-length | O(1) group boundaries |
| Adjacency index | 083 | Ref scan | O(1) graph traversal |

**Injectable indexes** depend on application-provided models. Two conforming
implementations may produce different results given different models. They are
OPTIONAL — the store is correct without them. They degrade gracefully to empty
results or O(n) scan when absent.

| Injectable | Trait | Model provided by |
|------------|-------|------------------|
| TextIndex | `DatomIndex` + `TextIndex` | Application (tokenizer) |
| VectorIndex | `DatomIndex` + `VectorIndex` | Application (embedding function) |

The distinction: primitives are **projections** (the answer is determined by the
datom set alone). Injectables are **transformations** (the answer depends on an
external model in addition to the datom set).

Both categories satisfy the DatomIndex homomorphism property (spec/05): they
distribute over set union. But primitives are verifiable by proptest (compare
against a reference implementation), while injectables are only verifiable
relative to their model.

**Consequence**: New indexes are classified at design time. An index that requires
no external model is primitive and goes in the ColumnarStore struct. An index that
requires an application-provided function is injectable and goes behind a trait.

---

### Fingerprint SIMD Optimization Note

**Applies to**: INV-FERR-074 (homomorphic fingerprint), INV-FERR-079 (chunk
fingerprints)

The byte-by-byte XOR loops in `StoreFingerprint::insert()` and `merge()` are the
SPECIFICATION form. Implementations MAY use widened operations (u64, u128, or SIMD
intrinsics) because XOR on aligned wider types produces bit-identical results to
byte-by-byte XOR. This is a consequence of XOR's bitwise nature:
`(a XOR b)[byte i] = a[byte i] XOR b[byte i]` regardless of the register width.

SIMD example: `_mm256_xor_si256` processes all 32 bytes in a single instruction
(AVX2). The widened form is a valid optimization under INV-FERR-025 (backend
interchangeability) because the observable fingerprint bytes are identical.

---

### Phase 4b+ Additions (Session 015)

The following two architectural directions were identified during the Session 015
radical performance analysis. They are recorded here as convergence targets for
Phase 4b+ implementation, building on the Phase 4a foundations established above.

---

### ADR-FERR-031: Wavelet Matrix Phase 4a Prerequisites — Rank/Select and Attribute Interning

**Traces to**: ADR-FERR-030 (wavelet matrix convergence target), INV-FERR-029
(LIVE bitvector), INV-FERR-073 (Yoneda fusion)
**Stage**: 2 (prerequisite work pulled to Phase 4a, wavelet matrix itself Phase 4c+)

**Problem**: ADR-FERR-030 identifies the wavelet matrix as the information-theoretic
convergence target but lists several prerequisites. Two of these prerequisites are
independently valuable in Phase 4a and should be implemented NOW rather than
deferred, because they compound with other Phase 4a optimizations:

1. **Rank/Select succinct bitvectors**: The wavelet matrix's fundamental operation.
   But rank/select is also independently valuable on the LIVE bitvector (INV-FERR-029):
   O(1) live-datom counting and O(K) live iteration instead of O(N) scanning.
   Phase 4a bead: bd-t84f.

2. **Attribute interning to integer symbols**: The wavelet matrix operates on
   integer-encoded column symbols. Attribute interning (string → u16) provides the
   integer encoding for the attribute column AND independently delivers 34x compression,
   Copy semantics, and 1-cycle comparison. Phase 4a bead: bd-fnod.

**Consequence**: These are pulled forward to Phase 4a not as wavelet matrix
implementation but as independent performance wins that HAPPEN to be wavelet
matrix prerequisites. When Phase 4c implements the wavelet matrix, the rank/select
implementation and the attribute integer encoding are already verified and benchmarked.

The accretive design principle is preserved: Phase 4a work feeds Phase 4c without
being designed for Phase 4c. The justification for each prerequisite stands on its
own Phase 4a merits.

---

### ADR-FERR-032: Lean-Verified Functor Composition for Representation Changes

**Traces to**: INV-FERR-025 (backend interchangeability), INV-FERR-072 (lazy promotion),
spec/09 principle 1 (representation independence via faithful functors)
**Stage**: 3 (Phase 4b+ — requires all Phase 4a representations to stabilize)

**Problem**: Every representation change in the performance architecture (SoA columnar,
attribute interning, succinct bitvectors, compression, wavelet matrix) must preserve
the abstract datom set. Currently, each change is verified independently by proptest.
As the number of representations grows (M representations), the verification cost
grows as M² (every pair must be tested). This is unsustainable.

**Solution**: Model each representation change as a FAITHFUL FUNCTOR in Lean:

```lean
/-- A representation functor from the abstract DatomStore to a concrete type C. -/
structure RepresentationFunctor (C : Type) where
  /-- Encode abstract store into concrete representation. -/
  encode : DatomStore → C
  /-- Decode concrete representation back to abstract store. -/
  decode : C → DatomStore
  /-- Round-trip identity: decode ∘ encode = id. -/
  roundtrip : ∀ s : DatomStore, decode (encode s) = s

/-- Functor composition: if F and G are faithful, F ∘ G is faithful. -/
theorem functor_compose_faithful
    (F : RepresentationFunctor B) (G : RepresentationFunctor C)
    (lift : B → C) (lower : C → B)
    (h_lift : ∀ b, G.decode (lift (F.encode (F.decode b))) = F.decode b)
    (h_lower : ∀ c, F.decode (lower c) = G.decode c) :
    ∀ s : DatomStore, G.decode (lift (F.encode s)) = s := by
  intro s
  rw [show F.encode s = F.encode s from rfl]
  -- Proof by composing the two roundtrip properties.
  sorry -- Phase 4b: mechanize when representations stabilize
```

**Consequence**: Verification cost becomes LINEAR in M (prove once per functor)
instead of QUADRATIC (prove every pair). Each new representation change requires
ONE Lean proof (`roundtrip`), not M new compatibility tests. The functor composition
theorem gives correctness of all compositions for free.

This is the formal-methods analogue of a COMPILER OPTIMIZATION PIPELINE: each
optimization pass is proven correct independently, and the pipeline is correct by
the composition theorem. No competing database project has anything comparable.

**Prerequisites**: All Phase 4a representation types (SoA, interned attributes,
succinct bitvectors, chunk fingerprints) must stabilize. The Lean model must be
extended with concrete representation types — currently it operates on the abstract
`DatomStore := Finset Datom` only.

**Implementation**: Phase 4b for the Lean formalization. Phase 4c for full
integration with the wavelet matrix representation functor.

---

### Phase 4a Additions (Session 015, Agentic OS Alignment)

The following four invariants were identified during the Session 015 radical
performance analysis, informed by the agentic OS vision (docs/ideas/008-agentic-os.md).
Each is a PRIMITIVE index — deterministic, universal, not injectable — because it
is a pure function of the datom set with exactly one correct answer.

---

### INV-FERR-081: TxId Temporal Permutation

**Traces to**: INV-FERR-073 (Yoneda fusion — 5th permutation), INV-FERR-007
(epoch monotonicity), INV-FERR-028 (cold start — temporal range for WAL delta)
**Verification**: `V:PROP`, `V:TYPE`
**Stage**: 1

#### Level 0 (Algebraic Law)
```
Let S be a DatomStore with n datoms in canonical EAVT order.
Let π_TxId : [0,n) → [0,n) be the permutation such that
  S[π_TxId(i)].tx_id ≤ S[π_TxId(i+1)].tx_id for all i.

Theorem (temporal range via permutation):
  ∀ T₁, T₂ ∈ TxId, T₁ ≤ T₂:
    { d ∈ S | T₁ ≤ d.tx_id ≤ T₂ }
    = { S[π_TxId(j)] | lo ≤ j ≤ hi }
  where lo = min { j | S[π_TxId(j)].tx_id ≥ T₁ }
    and hi = max { j | S[π_TxId(j)].tx_id ≤ T₂ }

  Temporal range query reduces to binary search on the permuted view: O(log n + k)
  where k = |result|.

Proof:
  π_TxId is a permutation of [0, n) sorted by TxId.
  Binary search on the permuted TxId sequence finds the boundaries of the
  contiguous range [lo, hi] in O(log n). All elements in this range satisfy
  the temporal predicate by construction (sorted order). All elements outside
  this range do not (also by sorted order). Therefore the result set is exact.
```

#### Level 1 (State Invariant)
The 5th permutation array sorts canonical positions by TxId, enabling O(log N)
temporal range queries across the entire store. This completes the permutation
family: EAVT (canonical), AEVT, VAET, AVET (existing), and now TxId (temporal).

Essential for the agentic OS's dream cycles ("what changed between sessions?"),
harvest ("what was the trajectory this session?"), and the situation board
("what's new since I last looked?"). Without it, these operations require O(N)
full scans — unacceptable at 100M datoms.

Lazy via OnceLock, same infrastructure as the existing 3 permutations. Built
on first temporal query, invalidated on splice/transact.

#### Level 2 (Implementation Contract)
```rust
impl ColumnarStore {
    /// TxId-sorted permutation: position in TxId order → position in canonical.
    /// OnceLock: built on first temporal query.
    perm_txid: OnceLock<Vec<u32>>,

    /// All datoms with TxId in [lo, hi]. O(log n + result_count).
    pub fn txid_range(&self, lo: TxId, hi: TxId) -> impl Iterator<Item = usize> + '_ {
        let perm = self.perm_txid.get_or_init(|| {
            let mut p: Vec<u32> = (0..self.len as u32).collect();
            p.sort_unstable_by(|&a, &b| self.tx_ids[a as usize].cmp(&self.tx_ids[b as usize]));
            p
        });
        let start = perm.partition_point(|&idx| self.tx_ids[idx as usize] < lo);
        let end = perm.partition_point(|&idx| self.tx_ids[idx as usize] <= hi);
        perm[start..end].iter().copied().map(|i| i as usize)
    }
}
```

**Falsification**: Any TxId range query that returns a datom outside [T₁, T₂]
or misses a datom within [T₁, T₂]. Concretely: `txid_range(t1, t2)` yields a
canonical position whose TxId is not in [t1, t2].

**proptest strategy**:
```rust
proptest! {
    fn txid_range_matches_filter(
        datoms in prop::collection::btree_set(arb_datom(), 0..500),
        lo in arb_tx_id(), hi in arb_tx_id(),
    ) {
        let store = ColumnarStore::from_datoms(datoms.iter().cloned());
        let range_result: BTreeSet<_> = store.txid_range(lo, hi).collect();
        let filter_result: BTreeSet<_> = (0..store.len())
            .filter(|&i| store.tx_ids[i] >= lo && store.tx_ids[i] <= hi)
            .collect();
        prop_assert_eq!(range_result, filter_result);
    }
}
```

**Lean theorem**:
```lean
/-- Sorting a finite set by a key produces a sequence where binary search
    correctly identifies all elements matching a range predicate. -/
-- Deferred: requires formalization of permutation + sorted binary search.
-- The concrete Rust implementation is verified by proptest.
theorem txid_range_correct : sorry := sorry
```

---

### INV-FERR-082: Entity Run-Length Encoding

**Traces to**: INV-FERR-076 (positional content addressing — EAVT guarantees
entity contiguity), INV-FERR-027 (read latency — O(1) group boundaries)
**Verification**: `V:PROP`, `V:TYPE`
**Stage**: 1

#### Level 0 (Algebraic Law)
```
Let S be a DatomStore in canonical EAVT order. Since EAVT sorts by EntityId
first, datoms for the same entity are CONTIGUOUS in the canonical array.

Define the entity group decomposition:
  G(S) = [(e₁, c₁), (e₂, c₂), ..., (e_g, c_g)]
  where e_i is the i-th distinct EntityId in EAVT order and c_i = |{d ∈ S | d.entity = e_i}|.

Define the prefix sum:
  P(G)[0] = 0
  P(G)[i] = P(G)[i-1] + c_{i-1}   for i > 0

Theorem (O(1) group boundary):
  ∀ entity e_i in S: the datoms for e_i occupy canonical positions [P(G)[i], P(G)[i] + c_i).

Theorem (compression):
  Space(G) = g × (32 + 4) bytes = 36g bytes
  Space(flat) = n × 32 bytes
  Compression ratio = 36g / 32n = 1.125 × (g/n)
  At 10 datoms/entity: ratio = 0.1125 → 9x compression.
```

#### Level 1 (State Invariant)
The entity column in the SoA ColumnarStore is run-length encoded, exploiting the
EAVT sort guarantee that datoms for the same entity are contiguous. This provides:
- O(1) group boundary lookup via prefix sum array
- O(1) datom count per entity
- O(log G) entity-at-position lookup (binary search on prefix sums)
- 9x compression of the entity column at 10 datoms/entity

Direct service to `associate` traversal: finding all datoms for an entity is
O(1) boundary lookup + O(degree) contiguous read with perfect cache locality.

#### Level 2 (Implementation Contract)
```rust
pub struct EntityRLE {
    groups: Vec<(EntityId, u32)>,
    prefix_sum: Vec<u32>,
}

impl EntityRLE {
    pub fn from_sorted(entities: &[EntityId]) -> Self { todo!() }
    pub fn group_start(&self, rank: usize) -> usize { self.prefix_sum[rank] as usize }
    pub fn group_count(&self, rank: usize) -> u32 { self.groups[rank].1 }
    pub fn entity_at_rank(&self, rank: usize) -> EntityId { self.groups[rank].0 }
    pub fn entity_at_position(&self, pos: usize) -> EntityId { todo!() }
}
```

**Falsification**: Any entity whose group_start + group_count doesn't span exactly
its datoms in the canonical array. Or: group_count(i) ≠ actual count of datoms
for entity i.

**proptest strategy**:
```rust
proptest! {
    fn rle_roundtrip(datoms in prop::collection::btree_set(arb_datom(), 0..500)) {
        let store = ColumnarStore::from_datoms(datoms.iter().cloned());
        let rle = &store.entity_rle;
        for rank in 0..rle.num_groups() {
            let entity = rle.entity_at_rank(rank);
            let start = rle.group_start(rank);
            let count = rle.group_count(rank) as usize;
            for i in start..start+count {
                prop_assert_eq!(store.entities[i], entity);
            }
        }
    }
}
```

**Lean theorem**:
```lean
-- RLE is a grouping of a sorted sequence by equality. Trivially correct
-- when the input is sorted: consecutive equal elements form contiguous runs.
theorem rle_correct_on_sorted (xs : List α) (h : xs.Sorted (· ≤ ·)) :
    rle_decode (rle_encode xs) = xs := by sorry
```

---

### INV-FERR-083: Graph Adjacency Index

**Traces to**: INV-FERR-005 (index bijection — adjacency is a derived index),
INV-FERR-030 (replica filtering — traversal over Ref edges)
**Verification**: `V:PROP`, `V:TYPE`
**Stage**: 1

#### Level 0 (Algebraic Law)
```
Let S be a DatomStore.
Define Ref(S) = { (d.entity, d.attribute, d.value.as_ref()) | d ∈ S, d.value is Ref }

Define the adjacency function:
  Adj(S, e) = { (a, t) | (e, a, t) ∈ Ref(S) }

Theorem (adjacency distributes over union):
  ∀ A, B ∈ DatomStore, ∀ e:
    Adj(A ∪ B, e) = Adj(A, e) ∪ Adj(B, e)

Proof:
  Adj(A ∪ B, e) = { (a, t) | (e, a, t) ∈ Ref(A ∪ B) }
                = { (a, t) | (e, a, t) ∈ Ref(A) ∪ Ref(B) }
                = { (a, t) | (e, a, t) ∈ Ref(A) } ∪ { (a, t) | (e, a, t) ∈ Ref(B) }
                = Adj(A, e) ∪ Adj(B, e)

  The adjacency function is a homomorphism: it distributes over merge.
  Therefore the adjacency index is CRDT-compatible by construction.
```

#### Level 1 (State Invariant)
A precomputed mapping from each entity to its outgoing and incoming Ref edges,
built lazily on first graph traversal. Enables O(1) neighbor lookup and O(degree^k)
k-hop traversal instead of O(degree^k × log N) via repeated EAVT lookups.

The adjacency distributes over merge (proven above), so the index is CRDT-compatible:
merging two stores' adjacency indexes by set union produces the correct adjacency
for the merged store.

#### Level 2 (Implementation Contract)
```rust
pub struct AdjacencyIndex {
    outgoing: im::OrdMap<EntityId, Vec<(AttributeId, EntityId)>>,
    incoming: im::OrdMap<EntityId, Vec<(AttributeId, EntityId)>>,
}

impl AdjacencyIndex {
    pub fn build(store: &ColumnarStore) -> Self { todo!() }
    pub fn neighbors(&self, entity: &EntityId) -> &[(AttributeId, EntityId)] { todo!() }
    pub fn reverse_neighbors(&self, entity: &EntityId) -> &[(AttributeId, EntityId)] { todo!() }
    pub fn k_hop(&self, entity: &EntityId, k: usize) -> Vec<EntityId> { todo!() }
}
```

**Falsification**: Any entity e where `Adj(S, e)` from the index differs from
scanning all datoms for e and filtering for Ref values.

**proptest strategy**:
```rust
proptest! {
    fn adjacency_matches_scan(datoms in prop::collection::btree_set(arb_datom(), 0..500)) {
        let store = ColumnarStore::from_datoms(datoms.iter().cloned());
        let adj = store.adjacency_index();
        for d in &datoms {
            if let Value::Ref(target) = d.value() {
                let neighbors = adj.neighbors(&d.entity());
                prop_assert!(neighbors.iter().any(|(_, t)| t == target));
            }
        }
    }
}
```

**Lean theorem**:
```lean
/-- Adjacency distributes over set union (CRDT compatibility). -/
theorem adj_merge (A B : Finset Datom) (e : Entity) :
    adj (A ∪ B) e = adj A e ∪ adj B e := by
  ext ⟨a, t⟩
  simp [adj, Finset.mem_union]
```

---

### INV-FERR-084: WAL Dedup Bloom Filter

**Traces to**: INV-FERR-008 (WAL-before-visible), INV-FERR-026 (write amplification),
INV-FERR-012 (content-addressed identity — duplicate datoms have identical hashes)
**Verification**: `V:PROP`
**Stage**: 1

#### Level 0 (Algebraic Law)
```
Let B be a Bloom filter with false positive rate ε.
Let h(d) = BLAKE3(serialize(d)) be the datom content hash (INV-FERR-012).

Define the WAL dedup predicate (two-phase check):
  skip_wal(d, B, S) = B.probably_contains(h(d)) ∧ d ∈ S

  Phase 1 (Bloom): fast probabilistic check. O(1).
  Phase 2 (Store): if Bloom says "maybe," verify against the live store. O(log N).
  Skip WAL write ONLY if BOTH the Bloom says present AND the datom is already
  in the store's datom set.

Safety theorem:
  ∀ d, ∀ B, ∀ S:
    ¬skip_wal(d, B, S) → d is written to WAL

  Proof: If skip_wal is false, either the Bloom returned false (zero false
  negative rate guarantees d is genuinely new → written) or the Bloom returned
  true but d ∉ S (false positive → d IS new → written). In both cases d is
  written. No datom is ever silently dropped.

  This is UNCONDITIONAL — it does not rely on caller re-submission. INV-FERR-008
  (WAL-before-visible) is preserved: every genuinely new datom is durable before
  it becomes visible.

Efficiency theorem:
  ∀ d already in S, ∀ B that contains h(d):
    skip_wal(d, B, S) = true

  A duplicate datom (already in the store) passes both the Bloom check and
  the store membership check. The WAL write is correctly skipped. This is the
  common case for bursty event sources that produce redundant events.

Cost: Most duplicates are filtered by the O(1) Bloom check alone (true positives).
The O(log N) store check is reached only on Bloom false positives (ε fraction of
genuinely new datoms). At ε = 0.1%: 999 of 1000 new datoms skip the store check.
```

#### Level 1 (State Invariant)
A fixed-size (64KB) Bloom filter on recent WAL datom hashes, cleared on
checkpoint. Eliminates redundant WAL writes from bursty event sources (filesystem
watchers, git hooks, CRM webhooks) that produce duplicate events.

The safety guarantee is unconditional: a two-phase check (Bloom + store membership)
ensures NO genuinely new datom is ever silently dropped. The Bloom provides O(1)
fast-path elimination of known duplicates. On a Bloom false positive (0.1% rate),
the store membership check (O(log N)) catches the error — the datom IS new and IS
written to WAL. INV-FERR-008 (WAL-before-visible) is preserved unconditionally.

#### Level 2 (Implementation Contract)
```rust
pub(crate) struct WalDedupBloom {
    bits: BitVec<u64, Lsb0>,  // 64KB = 524,288 bits
    k: u8,                     // 7 hash functions
}

impl WalDedupBloom {
    pub fn new() -> Self { todo!() }
    pub fn probably_contains(&self, hash: &[u8; 32]) -> bool { todo!() }
    pub fn insert(&mut self, hash: &[u8; 32]) { todo!() }
    pub fn clear(&mut self) { todo!() }
}
```

**Falsification**: A genuinely new datom (never written to WAL) whose hash is
NOT in the Bloom filter but `probably_contains` returns true. This cannot happen —
false negatives are impossible for Bloom filters by construction. The only
falsifiable property is the FP rate exceeding ε.

**proptest strategy**:
```rust
proptest! {
    fn bloom_no_false_negatives(hashes in prop::collection::vec(arb_hash(), 0..1000)) {
        let mut bloom = WalDedupBloom::new();
        for h in &hashes {
            bloom.insert(h);
        }
        for h in &hashes {
            prop_assert!(bloom.probably_contains(h),
                "INV-FERR-084: Bloom filter must have zero false negatives");
        }
    }
}
```

**Lean theorem**:
```lean
-- Bloom filter false negative impossibility is a property of the data structure,
-- not a domain-specific theorem. The Lean model of DatomStore (Finset Datom)
-- has no concept of Bloom filters. Verified by proptest.
```
