## 23.9 Prolly Tree Storage Engine

The Prolly Tree Block Store is the Phase 4b storage layer that replaces flat serialization
(the current `store.bin` checkpoint model) with a content-addressed, structurally-shared,
diffable on-disk format. Where sections 23.1 through 23.8 define the algebraic and operational
semantics of the datom store, section 23.9 specifies the **physical representation** that makes
those semantics hold efficiently across versions, across machines, and across organizational
boundaries.

The prolly tree is the bridge between the in-memory `im::OrdMap` world (Phase 4a) and the
distributed federation world (section 23.8). In-memory, structural sharing is provided by
`im-rs`'s persistent HAMTs. On-disk, structural sharing is provided by the prolly tree's
content-addressed chunks. The same datom set, the same merge semantics, the same CRDT
guarantees — different physical substrates for different operational regimes.

**Traces to**: SEED.md section 4 (Content-Addressed Identity, Design Commitment #2), SEED.md
section 10 (The Bootstrap), INV-FERR-012 (Content-Addressed Entities), INV-FERR-010 (Merge
Convergence), INV-FERR-022 (Anti-Entropy Convergence), ADR-FERR-001 (Persistent Data
Structures), section 23.8 (Federation & Federated Query)

**Design principles**:

1. **Content-addressed everything.** Every storage chunk has address = BLAKE3(content).
   Identical content is stored exactly once. Deduplication is a structural tautology, not
   a background process. This is INV-FERR-012 (content-addressed entities) extended to the
   storage layer itself: not just datoms, but the blocks that hold datoms are content-addressed.

2. **History independence.** Given the same set of key-value pairs, the prolly tree produces
   the same structure regardless of insertion order. This is the on-disk analogue of the
   algebraic commutativity guarantee (INV-FERR-001): just as `merge(A, B) = merge(B, A)`,
   `tree(inserts_in_order_1) = tree(inserts_in_order_2)` when the final key sets are equal.

3. **O(d) operations.** Diffing, checkpointing, and transferring data are all proportional
   to the number of changed datoms `d`, not the total store size `n`. This is the property
   that makes federation (section 23.8) practical at scale: transferring 100 changed datoms
   from a 100M-datom store sends O(100) chunks, not O(100M).

4. **Substrate independence (C8).** The `ChunkStore` trait abstracts the physical storage
   backend. The same prolly tree structure works identically on local filesystem, in-memory
   (for testing), or cloud object storage (S3). Application code does not change when the
   storage substrate changes. This extends ADR-FERR-002 (no async runtime lock-in) to
   storage: no filesystem lock-in either.

5. **Algebraic fidelity.** The prolly tree is a faithful functor from the algebraic store
   `(P(D), union)` to on-disk blocks. Every STORE axiom L1-L5 holds through the serialization
   round-trip. The prolly tree adds no semantic content — it is a pure representation concern.

---

### ADR-FERR-008: Storage Engine — Prolly Tree Block Store

**Traces to**: SEED.md section 4 (Content-Addressed Identity), INV-FERR-012 (Content-Addressed
Entities), INV-FERR-022 (Anti-Entropy Convergence), INV-FERR-010 (Merge Convergence),
ADR-FERR-001 (Persistent Data Structures)
**Stage**: 1

**Problem**: How to achieve on-disk structural sharing, O(d) diffing, and chunk-based
federation transfer. The current model (flat `store.bin` checkpoint) serializes the entire
store on every checkpoint: O(n) write, O(n) diff (byte comparison), O(n) transfer (full copy).
At 100M datoms (~20GB), this is untenable for frequent checkpoints and prohibitive for
federation transfers where only a handful of datoms differ.

**Options**:

| Option | Description | Pros | Cons |
|--------|-------------|------|------|
| A: Flat serialization (`store.bin`) | Flat checkpoint model. Full serialize on checkpoint. | Simple. No dependencies. | O(n) checkpoint. O(n) diff. O(n) transfer. No structural sharing. No version history. |
| B: Log-structured merge tree (LSM) | RocksDB/LevelDB model. Append-only write, background compaction. | Excellent write throughput. Battle-tested (RocksDB). | No structural sharing across versions. Complex compaction. Background I/O unpredictable. Hard to diff two versions. No content-addressing. |
| C: Prolly tree block store | Dolt/Noms model. Content-addressed chunks, probabilistic B-tree, rolling hash boundaries. | O(d) diff, O(d) checkpoint (only changed chunks), O(d) transfer, structural sharing across versions, content-addressed deduplication, version history via root hashes. | Write overhead (1+k/w) x log_k(n) vs B-tree log_k(n). Rolling hash computation per key. More complex than flat serialization. |

**Decision**: **Option C: Prolly tree block store**

Content-addressed chunks give three properties that are essential for federation at scale:

1. **Structural sharing**: Identical data is stored once across all versions. Two snapshots
   that share 99.99% of their data physically share 99.99% of their chunks on disk. This
   enables O(1) snapshot creation (store the new root hash) with O(d) storage cost
   (only the modified path from leaf to root is new).

2. **O(d) diffing**: Comparing two versions starts at the root hashes. If equal, the trees
   are identical (done in O(1)). If different, recurse into children where hashes differ.
   Only the O(d) changed subtrees are visited. This is the Merkle property applied to
   the B-tree structure.

3. **Chunk-based federation transfer**: Sending data between stores is reduced to "send
   the chunks the receiver doesn't have." The sender walks the prolly tree from the root,
   the receiver reports which chunk addresses it already has (by hash), and only missing
   chunks are transferred. This IS the Merkle anti-entropy protocol from INV-FERR-022,
   naturally implemented by the prolly tree's content-addressed structure.

The write overhead ((1+k/w) x log_k(n) vs B-tree log_k(n)) is acceptable because:
- `k` (fanout) is typically 64-256, making the factor small
- Write serialization (INV-FERR-007) means writes are not throughput-critical
- The O(d) checkpoint and transfer benefits far outweigh the write penalty

**Rejected**:
- **Option A**: O(n) checkpoint is unacceptable at scale. A 100M-datom store takes minutes
  to serialize. Checkpointing after every transaction (required for durability) becomes
  the bottleneck. No structural sharing means no version history and no efficient
  federation transfer. The current model is suitable only for the Phase 4a MVP.
- **Option B**: LSM trees are optimized for write-heavy workloads with eventual read
  amplification. They do not provide structural sharing across versions (old SST files
  are compacted away). Diffing two LSM states requires full scans. No content-addressing
  means no deduplication and no chunk-based transfer. RocksDB also brings `unsafe` code
  via C FFI, violating INV-FERR-023.

**Consequence**: Write path becomes copy-on-write along the modified prolly tree path.
Read path can serve directly from chunk store (mmap) or load into `im::OrdMap`. Rolling
hash computation adds latency to key insertion but is amortized across batch writes.
The chunk store abstraction (`ChunkStore` trait) becomes the physical storage interface,
replacing direct file I/O.

**Source**: Dolt (dolthub.com) — production MySQL-compatible database built on prolly trees.
Noms database (attic-labs/noms) — original prolly tree implementation. SEED.md section 4
(content-addressed identity).

---

### INV-FERR-045: Chunk Content Addressing

**Traces to**: SEED.md section 4 (Content-Addressed Identity), INV-FERR-012
(Content-Addressed Entities), C2 (Identity by Content)
**Verification**: `V:PROP`, `V:KANI`, `V:LEAN`
**Stage**: 1

#### Level 0 (Algebraic Law)
```
Let Chunk be the type of storage chunks (byte sequences).
Let addr : Chunk -> Hash be the addressing function, defined as addr(c) = BLAKE3(content(c)).
Let content : Chunk -> Bytes extract the raw content of a chunk.

Axiom (determinism):
  forall c : Chunk: addr(c) = BLAKE3(content(c))

Theorem (content-addressed identity):
  forall c1 c2 : Chunk: content(c1) = content(c2) -> addr(c1) = addr(c2)

Proof:
  If content(c1) = content(c2), then BLAKE3(content(c1)) = BLAKE3(content(c2))
  by referential transparency of BLAKE3. Therefore addr(c1) = addr(c2).

Corollary (deduplication):
  forall c1 c2 : Chunk: content(c1) = content(c2) -> store(c1) and store(c2) are
  the same physical storage operation (idempotent write to the same address).

Corollary (collision resistance):
  Under BLAKE3's collision resistance assumption (2^128 security level):
  forall c1 c2 : Chunk: addr(c1) = addr(c2) -> content(c1) = content(c2)
  with negligible probability of violation.
```

#### Level 1 (State Invariant)
For all reachable `ChunkStore` states produced by any sequence of `put_chunk` operations:
- Every chunk stored under address `a` has content `c` such that `BLAKE3(c) = a`.
- Two `put_chunk` calls with identical content produce the same address and do not
  increase physical storage (deduplication is structural, not a background process).
- `get_chunk(addr)` returns the same bytes that were passed to `put_chunk` when the
  chunk with that address was first stored.
- The chunk store never contains two chunks with the same address but different content.
  This is enforced by the addressing function, not by runtime checks.

Deduplication is the load-bearing property for structural sharing: two prolly tree versions
that share subtrees physically share the chunks of those subtrees. The chunk store does not
need to know about prolly trees — it merely observes that the same bytes are being stored
and returns the same address.

#### Level 2 (Implementation Contract)
```rust
/// A content-addressed chunk: address = BLAKE3(data).
/// Chunks are immutable after creation (C1 extended to storage layer).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chunk {
    /// The content-addressed hash. addr = BLAKE3(data).
    addr: Hash,
    /// The raw bytes of this chunk.
    data: Arc<[u8]>,
}

impl Chunk {
    /// Create a chunk from raw bytes. Address is computed deterministically.
    pub fn from_bytes(data: &[u8]) -> Self {
        let addr = Hash::from(blake3::hash(data));
        Chunk {
            addr,
            data: Arc::from(data),
        }
    }

    /// The content-addressed hash of this chunk.
    pub fn addr(&self) -> &Hash { &self.addr }

    /// The raw bytes.
    pub fn data(&self) -> &[u8] { &self.data }
}

/// The chunk store trait. Abstracts physical storage.
/// Implementations: FileChunkStore, MemoryChunkStore, S3ChunkStore.
pub trait ChunkStore: Send + Sync {
    /// Store a chunk. Returns the content-addressed hash.
    /// If a chunk with this address already exists, this is a no-op (idempotent).
    fn put_chunk(&self, chunk: &Chunk) -> Result<Hash, FerraError>;

    /// Retrieve a chunk by its content-addressed hash.
    /// Returns None if the chunk is not present in this store.
    fn get_chunk(&self, addr: &Hash) -> Result<Option<Chunk>, FerraError>;

    /// Check whether a chunk with the given address exists without loading it.
    fn has_chunk(&self, addr: &Hash) -> Result<bool, FerraError>;

    /// Return the set of all chunk addresses in this store.
    /// Used for anti-entropy diff computation (section 23.8, INV-FERR-022).
    fn all_addrs(&self) -> Result<BTreeSet<Hash>, FerraError>;
}

#[kani::proof]
#[kani::unwind(5)]
fn chunk_content_addressing() {
    let data: [u8; 4] = kani::any();
    let c1 = Chunk::from_bytes(&data);
    let c2 = Chunk::from_bytes(&data);
    assert_eq!(c1.addr(), c2.addr(), "Same content must produce same address");
    assert_eq!(c1.data(), c2.data());
}
```

**Falsification**: Any `ChunkStore` state where `get_chunk(a)` returns bytes `b` such that
`BLAKE3(b) != a`. Concretely: the chunk store contains a chunk whose address does not
match the BLAKE3 hash of its content. This would indicate either (a) the address was
computed using a different hash function, (b) the content was modified after storage
(violating immutability), or (c) a hash collision occurred (vanishingly improbable with
BLAKE3's 256-bit output).

**proptest strategy**:
```rust
proptest! {
    #[test]
    fn chunk_content_addressing_prop(
        data in prop::collection::vec(any::<u8>(), 0..8192),
    ) {
        let c1 = Chunk::from_bytes(&data);
        let c2 = Chunk::from_bytes(&data);
        prop_assert_eq!(c1.addr(), c2.addr(),
            "Content-addressed identity violated: same bytes, different address");

        let expected_addr = Hash::from(blake3::hash(&data));
        prop_assert_eq!(*c1.addr(), expected_addr,
            "Address does not match BLAKE3 of content");
    }

    #[test]
    fn chunk_store_deduplication(
        data in prop::collection::vec(any::<u8>(), 1..4096),
    ) {
        let store = MemoryChunkStore::new();
        let chunk = Chunk::from_bytes(&data);

        let addr1 = store.put_chunk(&chunk).unwrap();
        let addr2 = store.put_chunk(&chunk).unwrap();
        prop_assert_eq!(addr1, addr2, "Idempotent put returned different addresses");

        // Store size should not increase on duplicate put
        let all = store.all_addrs().unwrap();
        prop_assert_eq!(all.len(), 1, "Duplicate chunk increased store size");
    }
}
```

**Lean theorem**:
```lean
/-- Chunk content addressing: identical content produces identical address.
    This is the storage-layer extension of INV-FERR-012. -/

def chunk_addr (data : List UInt8) : Hash := blake3 data

theorem chunk_content_identity (d1 d2 : List UInt8) (h : d1 = d2) :
    chunk_addr d1 = chunk_addr d2 := by
  subst h; rfl

/-- Deduplication is structural: storing the same content twice
    is observationally equivalent to storing it once. -/
theorem chunk_store_idempotent (s : Finset (Hash x List UInt8))
    (data : List UInt8) :
    let entry := (chunk_addr data, data)
    s ∪ {entry} ∪ {entry} = s ∪ {entry} := by
  simp [Finset.union_self_of_subset (Finset.subset_union_right)]
```

---

### INV-FERR-046: Prolly Tree History Independence

**Traces to**: INV-FERR-001 (Merge Commutativity), INV-FERR-045 (Chunk Content Addressing),
SEED.md section 4 (Content-Addressed Identity)
**Verification**: `V:PROP`, `V:KANI`, `V:LEAN`
**Stage**: 1

#### Level 0 (Algebraic Law)
```
Let KV = {(k1,v1), (k2,v2), ..., (kn,vn)} be a finite set of key-value pairs
  with k1 < k2 < ... < kn (keys are totally ordered).
Let insert_seq : List (Key x Value) -> ProllyTree be the function that builds a
  prolly tree by inserting key-value pairs in the given order.
Let perm : Set (Key x Value) -> List (List (Key x Value)) enumerate all permutations.

Theorem (history independence):
  forall pi1 pi2 in perm(KV):
    insert_seq(pi1) = insert_seq(pi2)

Proof:
  The prolly tree structure is determined by two factors:
  1. The sorted sequence of keys (determines leaf ordering)
  2. The rolling hash boundary function applied to keys (determines chunk boundaries)

  Both factors depend ONLY on the final set of keys, not on the order they were inserted.

  Step 1: After all insertions, the sorted key sequence is the same for any permutation
    (sorting is a function of the set, not the history).
  Step 2: Rolling hash boundaries are computed on the sorted key sequence. The boundary
    function B(k) = (rolling_hash(k) % (1 << pattern_width)) == 0 depends only on
    each key and its neighbors in sorted order, not on insertion history.
  Step 3: Once the leaf chunks are determined (by boundaries), the internal node chunks
    are determined recursively (content-addressed hashes of children).
  Step 4: Content-addressing (INV-FERR-045) ensures that identical chunk content produces
    identical chunk addresses at every level.

  Therefore the root hash — and the entire tree structure — is a function of the
  key-value set alone. History is erased. QED.

Corollary (merge commutativity):
  Since merge(A, B) produces the union of key-value sets, and the tree structure depends
  only on the set, merge(A, B) and merge(B, A) produce identical prolly trees with
  identical root hashes. This is INV-FERR-001 extended to the storage layer.
```

#### Level 1 (State Invariant)
For all reachable prolly tree states produced by any sequence of insert and delete operations:
- The tree structure (chunk boundaries, internal nodes, root hash) depends only on the
  current set of key-value pairs, not on the order or timing of operations that produced them.
- Two prolly trees built from the same key-value set have identical root hashes, identical
  chunk sets, and identical chunk addresses at every level.
- Deleting a key and re-inserting it produces the same tree as if the delete never happened
  (the tree has no memory of transient states).
- The rolling hash boundary function `B(k)` is a pure function of the key bytes and
  a fixed pattern width. It does not depend on the current tree state, the insertion
  index, or any mutable counter.

History independence is the property that makes prolly trees suitable for CRDT-based
systems: two replicas that arrive at the same key-value set through different operation
sequences produce identical on-disk representations. Diff and transfer algorithms can
rely on hash comparison without worrying about divergent tree structures from divergent
histories.

#### Level 2 (Implementation Contract)
```rust
/// Determine whether a key is a chunk boundary using a rolling hash.
/// The boundary function depends ONLY on the key bytes and the pattern width.
/// It does NOT depend on insertion order, tree state, or any mutable state.
///
/// boundary(k) = true iff (rolling_hash(k) & mask) == mask
/// where mask = (1 << pattern_width) - 1
///
/// The pattern_width controls expected chunk size:
///   pattern_width = 12 -> expected 4096 items per chunk
///   pattern_width = 10 -> expected 1024 items per chunk
///   pattern_width = 8  -> expected 256 items per chunk
pub fn is_boundary(key: &[u8], pattern_width: u32) -> bool {
    let hash = rolling_hash(key);
    let mask = (1u64 << pattern_width) - 1;
    (hash & mask) == mask
}

/// Build a prolly tree from a set of key-value pairs.
/// The result is independent of the order of iteration over the input.
///
/// # Guarantees
/// - Same input set -> same root hash (history independence, INV-FERR-046)
/// - Same input set -> same chunk set (structural identity)
/// - O(n) construction time (single sorted pass + boundary computation)
pub fn build_prolly_tree(
    kvs: &BTreeMap<Key, Value>,
    chunk_store: &dyn ChunkStore,
    pattern_width: u32,
) -> Result<Hash, FerraError> {
    // BTreeMap iteration is sorted by key -- deterministic ordering
    let sorted_kvs: Vec<(&Key, &Value)> = kvs.iter().collect();

    // Phase 1: Split into leaf chunks at boundary keys
    let leaf_chunks = split_at_boundaries(&sorted_kvs, pattern_width);

    // Phase 2: Serialize leaf chunks, compute addresses, store
    let leaf_addrs: Vec<Hash> = leaf_chunks
        .iter()
        .map(|chunk_kvs| {
            let serialized = serialize_leaf_chunk(chunk_kvs);
            let chunk = Chunk::from_bytes(&serialized);
            let addr = chunk.addr().clone();
            chunk_store.put_chunk(&chunk)?;
            Ok(addr)
        })
        .collect::<Result<Vec<_>, _>>()?;

    // Phase 3: Build internal nodes recursively until single root
    build_internal_nodes(&leaf_addrs, chunk_store, pattern_width)
}

#[kani::proof]
#[kani::unwind(5)]
fn history_independence_bounded() {
    let n: usize = kani::any();
    kani::assume(n <= 3);

    // Generate n key-value pairs
    let mut kvs = BTreeMap::new();
    for i in 0..n {
        let k: [u8; 4] = kani::any();
        let v: [u8; 4] = kani::any();
        kvs.insert(Key::from(k), Value::from(v));
    }

    let store1 = MemoryChunkStore::new();
    let store2 = MemoryChunkStore::new();

    let root1 = build_prolly_tree(&kvs, &store1, 2).unwrap();
    let root2 = build_prolly_tree(&kvs, &store2, 2).unwrap();

    assert_eq!(root1, root2, "Same key-value set must produce same root hash");
}
```

**Falsification**: Any two prolly trees built from the same key-value set that produce
different root hashes. Concretely: `build_prolly_tree(kvs_1, ...) != build_prolly_tree(kvs_2, ...)`
where `kvs_1` and `kvs_2` contain the same key-value pairs (possibly inserted in different
order). This would indicate that the boundary function, serialization, or hashing depends
on mutable state (e.g., an insertion counter, a random salt, or the current tree shape).

**proptest strategy**:
```rust
proptest! {
    #[test]
    fn prolly_tree_history_independence(
        kvs in prop::collection::btree_map(
            prop::collection::vec(any::<u8>(), 1..32),  // keys
            prop::collection::vec(any::<u8>(), 1..256), // values
            1..200
        ),
        pattern_width in 4u32..12,
    ) {
        let store1 = MemoryChunkStore::new();
        let store2 = MemoryChunkStore::new();

        // Build from BTreeMap (sorted iteration)
        let root1 = build_prolly_tree(&kvs, &store1, pattern_width).unwrap();

        // Build from reversed insertion order (same final BTreeMap)
        let mut kvs_reversed = BTreeMap::new();
        for (k, v) in kvs.iter().rev() {
            kvs_reversed.insert(k.clone(), v.clone());
        }
        let root2 = build_prolly_tree(&kvs_reversed, &store2, pattern_width).unwrap();

        prop_assert_eq!(root1, root2,
            "History independence violated: different root from different insertion order");

        // Verify chunk sets are identical
        let addrs1 = store1.all_addrs().unwrap();
        let addrs2 = store2.all_addrs().unwrap();
        prop_assert_eq!(addrs1, addrs2,
            "History independence violated: different chunk sets from different insertion order");
    }

    #[test]
    fn prolly_tree_insert_delete_identity(
        kvs in prop::collection::btree_map(
            prop::collection::vec(any::<u8>(), 1..32),
            prop::collection::vec(any::<u8>(), 1..256),
            2..100
        ),
        pattern_width in 4u32..12,
    ) {
        let store1 = MemoryChunkStore::new();
        let store2 = MemoryChunkStore::new();

        // Build directly from kvs
        let root1 = build_prolly_tree(&kvs, &store1, pattern_width).unwrap();

        // Build from kvs, remove first key, re-add first key
        let first_key = kvs.keys().next().unwrap().clone();
        let first_val = kvs[&first_key].clone();
        let mut kvs_modified = kvs.clone();
        kvs_modified.remove(&first_key);
        kvs_modified.insert(first_key, first_val);
        let root2 = build_prolly_tree(&kvs_modified, &store2, pattern_width).unwrap();

        prop_assert_eq!(root1, root2,
            "Delete+re-insert of same key-value should produce identical tree");
    }
}
```

**Lean theorem**:
```lean
/-- History independence: prolly tree structure is a function of the key-value set,
    not the insertion history. Modeled as: sorted list determines the tree. -/

/-- A boundary function that depends only on the key, not on insertion order. -/
def is_boundary (key : List UInt8) (pattern_width : Nat) : Bool :=
  (rolling_hash key) % (2 ^ pattern_width) == 0

/-- Chunk boundaries are determined by the sorted key list. -/
def chunk_boundaries (keys : List (List UInt8)) (pw : Nat) : List Nat :=
  keys.enum.filterMap fun (i, k) => if is_boundary k pw then some i else none

/-- Any permutation of inserts produces the same sorted key list,
    therefore the same chunk boundaries, therefore the same tree. -/
theorem history_independence (kvs1 kvs2 : Finset (List UInt8 x List UInt8))
    (h : kvs1 = kvs2) (pw : Nat) :
    prolly_root (kvs1.sort (fun a b => a.1 < b.1)) pw =
    prolly_root (kvs2.sort (fun a b => a.1 < b.1)) pw := by
  subst h; rfl

/-- Merge commutativity extends to prolly trees via history independence. -/
theorem prolly_merge_comm (a b : Finset (List UInt8 x List UInt8)) (pw : Nat) :
    prolly_root ((a ∪ b).sort (fun x y => x.1 < y.1)) pw =
    prolly_root ((b ∪ a).sort (fun x y => x.1 < y.1)) pw := by
  rw [Finset.union_comm]
```

---

### INV-FERR-047: O(d) Diff Complexity

**Traces to**: INV-FERR-045 (Chunk Content Addressing), INV-FERR-046 (History Independence),
INV-FERR-022 (Anti-Entropy Convergence), section 23.8 (Federation)
**Verification**: `V:PROP`, `V:KANI`
**Stage**: 1

#### Level 0 (Algebraic Law)
```
Let T1, T2 be two prolly trees over key-value stores KV1, KV2.
Let d = |KV1 symmetric_diff KV2| be the symmetric difference (number of changed key-value pairs).
Let n = max(|KV1|, |KV2|) be the total store size.
Let k be the expected chunk fanout (branching factor).

Theorem (diff complexity):
  diff(root(T1), root(T2)) visits O(d * log_k(n)) nodes and produces exactly the set
  of changed key-value pairs KV1 symmetric_diff KV2.

Proof:
  The diff algorithm proceeds by recursive descent from the roots:

  1. Compare root hashes. If equal, the trees are identical: return empty diff.
     (This is the O(1) fast path for unchanged stores.)

  2. If root hashes differ, compare child hashes pairwise. For each child pair:
     a. If hashes match, skip the subtree (structural sharing guarantees identity).
     b. If hashes differ, recurse into the differing children.

  3. At leaf level, compare key-value pairs directly and emit differences.

  Analysis: Each changed key-value pair affects at most one leaf chunk and
  at most log_k(n) internal nodes on the path from leaf to root. The diff
  algorithm visits each affected path exactly once. Unchanged subtrees are
  skipped in O(1) by hash comparison.

  Total nodes visited: d * log_k(n) (one path per changed key-value pair).
  Total comparisons: O(k * d * log_k(n)) (comparing k children at each level
  on each affected path). Since k is constant, this simplifies to O(d * log_k(n)).

  When d = 0: O(1) (compare root hashes only).
  When d = n: O(n * log_k(n)) ~ O(n log n) (every path is visited).
  Typical case (d << n): O(d * log_k(n)) << O(n).

Corollary (efficiency ratio):
  The speedup over linear diff is n / (d * log_k(n)). For d = 100 changes
  in a 100M-datom store with k = 256: speedup = 10^8 / (100 * 3.3) ~ 300,000x.
```

#### Level 1 (State Invariant)
For all reachable prolly tree pairs `(T1, T2)`:
- `diff(T1, T2)` produces exactly the key-value pairs in the symmetric difference
  `KV1 symmetric_diff KV2`. No changes are missed, no false changes are reported.
- `diff(T1, T2)` visits at most `O(d * log_k(n))` internal nodes, where `d` is the
  number of changed key-value pairs and `n` is the total store size.
- `diff(T, T)` returns an empty iterator in O(1) time (root hash comparison).
- `diff(T1, T2)` = `diff(T2, T1)` with insertions and deletions swapped
  (symmetric: what's added in one direction is deleted in the other).
- The diff iterator is lazy: it yields key-value changes one at a time without
  materializing the full diff set in memory. This enables streaming diffs
  for large `d`.

The diff algorithm is the core of federation efficiency (section 23.8). When a federation
peer requests "what changed since version V?", the answer is `diff(root_V, root_current)`.
The cost is proportional to the number of changes, not the store size. This is what
makes federation practical at 100M+ datoms.

#### Level 2 (Implementation Contract)
```rust
/// A single diff entry: a key-value pair that exists in one tree but not the other.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffEntry {
    /// Key-value exists in left tree only (added in left, or deleted in right).
    LeftOnly { key: Key, value: Value },
    /// Key-value exists in right tree only (added in right, or deleted in left).
    RightOnly { key: Key, value: Value },
    /// Key exists in both but values differ.
    Modified { key: Key, left_value: Value, right_value: Value },
}

/// Compute the diff between two prolly tree roots.
/// Returns a lazy iterator over DiffEntry items.
///
/// # Complexity
/// - O(1) if roots are equal (identical trees)
/// - O(d * log_k(n)) where d = number of changed key-value pairs
/// - Memory: O(log_k(n)) stack depth (one frame per tree level)
///
/// # Correctness
/// The diff produces exactly KV1 symmetric_diff KV2 (symmetric difference of key-value sets).
/// No changes are missed. No false changes are reported.
pub fn diff<'a>(
    root1: &Hash,
    root2: &Hash,
    chunk_store: &'a dyn ChunkStore,
) -> Result<impl Iterator<Item = Result<DiffEntry, FerraError>> + 'a, FerraError> {
    if root1 == root2 {
        return Ok(Box::new(std::iter::empty()) as Box<dyn Iterator<Item = _>>);
    }
    Ok(Box::new(DiffIterator::new(root1.clone(), root2.clone(), chunk_store)))
}

/// Internal diff iterator state. Maintains a stack of node pairs to compare.
struct DiffIterator<'a> {
    /// Stack of (left_hash, right_hash) pairs to compare.
    /// Starts with the root pair, recurses into differing children.
    stack: Vec<(Hash, Hash)>,
    /// Buffered leaf-level diffs ready to yield.
    pending: VecDeque<DiffEntry>,
    /// The chunk store for resolving hashes to chunks.
    store: &'a dyn ChunkStore,
}

#[kani::proof]
#[kani::unwind(5)]
fn diff_identical_roots_is_empty() {
    let root: [u8; 32] = kani::any();
    let root_hash = Hash::from(root);

    // Diff of identical roots must produce zero entries
    let store = MemoryChunkStore::new();
    let entries: Vec<_> = diff(&root_hash, &root_hash, &store)
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert!(entries.is_empty(), "Diff of identical roots must be empty");
}
```

**Falsification**: A `diff(T1, T2)` call that either (a) visits O(n) nodes when only
O(d) keys differ (d << n), indicating that the algorithm does not skip identical
subtrees; (b) misses a changed key-value pair that exists in `KV1 symmetric_diff KV2`; or
(c) reports a change for a key-value pair that is identical in both trees.

Concretely: construct two 10,000-key prolly trees that differ in exactly 1 key.
Count the number of `get_chunk` calls made by `diff()`. If more than `2 * log_k(10000)`
chunks are loaded (factor of 2 for left and right paths), the O(d) claim is violated.

**proptest strategy**:
```rust
proptest! {
    #[test]
    fn diff_correctness(
        base_kvs in prop::collection::btree_map(
            prop::collection::vec(any::<u8>(), 1..32),
            prop::collection::vec(any::<u8>(), 1..128),
            10..500
        ),
        changes in prop::collection::btree_map(
            prop::collection::vec(any::<u8>(), 1..32),
            prop::collection::vec(any::<u8>(), 1..128),
            0..50
        ),
        pattern_width in 4u32..10,
    ) {
        let store = MemoryChunkStore::new();

        // Build base tree
        let root1 = build_prolly_tree(&base_kvs, &store, pattern_width).unwrap();

        // Apply changes
        let mut modified_kvs = base_kvs.clone();
        for (k, v) in &changes {
            modified_kvs.insert(k.clone(), v.clone());
        }
        let root2 = build_prolly_tree(&modified_kvs, &store, pattern_width).unwrap();

        // Compute diff
        let diff_entries: Vec<_> = diff(&root1, &root2, &store)
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        // Compute expected symmetric difference
        let expected_diff = symmetric_difference(&base_kvs, &modified_kvs);

        // Verify: every expected change appears in diff
        for (k, change) in &expected_diff {
            prop_assert!(
                diff_entries.iter().any(|e| e.key() == k),
                "Missing change for key {:?}", k
            );
        }

        // Verify: no spurious changes in diff
        for entry in &diff_entries {
            prop_assert!(
                expected_diff.contains_key(entry.key()),
                "Spurious diff entry for key {:?}", entry.key()
            );
        }
    }

    #[test]
    fn diff_empty_when_identical(
        kvs in prop::collection::btree_map(
            prop::collection::vec(any::<u8>(), 1..32),
            prop::collection::vec(any::<u8>(), 1..128),
            1..200
        ),
        pattern_width in 4u32..10,
    ) {
        let store = MemoryChunkStore::new();
        let root = build_prolly_tree(&kvs, &store, pattern_width).unwrap();

        let diff_entries: Vec<_> = diff(&root, &root, &store)
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        prop_assert!(diff_entries.is_empty(),
            "Diff of identical trees must be empty, got {} entries", diff_entries.len());
    }

    #[test]
    fn diff_symmetry(
        kvs1 in prop::collection::btree_map(
            prop::collection::vec(any::<u8>(), 1..32),
            prop::collection::vec(any::<u8>(), 1..128),
            5..200
        ),
        kvs2 in prop::collection::btree_map(
            prop::collection::vec(any::<u8>(), 1..32),
            prop::collection::vec(any::<u8>(), 1..128),
            5..200
        ),
        pattern_width in 4u32..10,
    ) {
        let store = MemoryChunkStore::new();
        let root1 = build_prolly_tree(&kvs1, &store, pattern_width).unwrap();
        let root2 = build_prolly_tree(&kvs2, &store, pattern_width).unwrap();

        let diff_fwd: BTreeSet<_> = diff(&root1, &root2, &store)
            .unwrap()
            .collect::<Result<BTreeSet<_>, _>>()
            .unwrap();
        let diff_rev: BTreeSet<_> = diff(&root2, &root1, &store)
            .unwrap()
            .collect::<Result<BTreeSet<_>, _>>()
            .unwrap();

        // Forward and reverse diffs have the same keys
        let fwd_keys: BTreeSet<_> = diff_fwd.iter().map(|e| e.key().clone()).collect();
        let rev_keys: BTreeSet<_> = diff_rev.iter().map(|e| e.key().clone()).collect();
        prop_assert_eq!(fwd_keys, rev_keys,
            "Forward and reverse diff must cover the same keys");
    }
}
```

---

### INV-FERR-048: Chunk-Based Federation Transfer

**Traces to**: INV-FERR-022 (Anti-Entropy Convergence), INV-FERR-047 (O(d) Diff),
INV-FERR-045 (Chunk Content Addressing), section 23.8 (Federation & Federated Query),
INV-FERR-037 (Federated Query Correctness)
**Verification**: `V:PROP`, `V:KANI`
**Stage**: 1

#### Level 0 (Algebraic Law)
```
Let S_src, S_dst be two chunk stores.
Let chunks(S) = {c | c stored in S} and addrs(S) = {addr(c) | c in chunks(S)}.

Definition (transfer set):
  transfer(S_src, S_dst) = {c in chunks(S_src) | addr(c) not_in addrs(S_dst)}

Theorem (transfer minimality):
  transfer(S_src, S_dst) is the minimal set of chunks that, when added to S_dst,
  makes S_dst contain all chunks reachable from root(S_src).

Proof:
  1. Every chunk in transfer(S_src, S_dst) is needed: by definition, addr(c) not_in addrs(S_dst),
     so S_dst does not have this chunk.
  2. No chunk outside transfer(S_src, S_dst) is needed: if addr(c) in addrs(S_dst),
     then S_dst already has a chunk with the same content (by content-addressing,
     INV-FERR-045). Sending it again would be redundant.
  3. After transferring all chunks in transfer(S_src, S_dst), every chunk reachable
     from root(S_src) is present in S_dst: either it was already there (addr in addrs(S_dst))
     or it was transferred.

Theorem (transfer monotonicity):
  transfer(S_src, S_dst) superset_of transfer(S_src, S_dst')
  when addrs(S_dst) subset_of addrs(S_dst').
  (More chunks in dst -> fewer chunks to transfer.)

Theorem (transfer idempotency):
  After executing transfer(S_src, S_dst):
    transfer(S_src, S_dst_new) = empty_set
  (A second transfer sends nothing.)

Corollary (structural sharing efficiency):
  If S_src and S_dst share a prolly tree root R_old, and S_src has a new root R_new
  where diff(R_old, R_new) = d key-value pairs, then:
    |transfer(S_src, S_dst)| = O(d * log_k(n))
  because only the chunks on the changed paths from leaf to root are new.
```

#### Level 1 (State Invariant)
For all reachable `(ChunkStore_src, ChunkStore_dst)` pairs:
- `transfer(src, dst)` sends exactly the chunks that are reachable from `root(src)`
  and not present in `dst`. No more (no redundant transfers), no less (no missing chunks).
- After transfer completes, `resolve(dst, root(src))` succeeds: the entire prolly tree
  rooted at `root(src)` is navigable from `dst`'s chunk store.
- Transfer does not modify `src`. The source store is read-only during transfer.
- Transfer does not delete or modify any chunk in `dst`. Transfer is monotonic:
  `addrs(dst_after) superset_of addrs(dst_before)`.
- Two concurrent transfers from different sources to the same destination are safe:
  chunk stores are append-only (content-addressed puts are idempotent), so concurrent
  writes to the same address are harmless.
- The transfer protocol is resumable: if interrupted, re-running transfer picks up
  where it left off (already-transferred chunks are detected by `has_chunk` and skipped).

The transfer protocol is the operational realization of anti-entropy (INV-FERR-022).
Where INV-FERR-022 guarantees eventual convergence of datom sets, INV-FERR-048
specifies the mechanism: chunk-level diff and transfer. The chunk granularity means
that even in a 100M-datom store, transferring 100 changed datoms sends ~300 chunks
(100 leaf changes x ~3 levels), not 100M datoms.

#### Level 2 (Implementation Contract)
```rust
/// The result of a transfer operation.
#[derive(Debug)]
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
pub trait ChunkTransfer {
    /// Transfer all chunks reachable from `root` in `src` that are not present in `dst`.
    ///
    /// # Algorithm
    /// 1. Start at `root`. Check if `dst.has_chunk(root)`.
    /// 2. If yes, the entire subtree is already present (content-addressing). Done.
    /// 3. If no, fetch the chunk from `src`, put it in `dst`, decode its children,
    ///    and recurse on each child.
    ///
    /// # Complexity
    /// O(|transfer_set| * log_k(n)) chunk reads + writes.
    /// When src and dst share most chunks (common case for federation),
    /// this is O(d * log_k(n)) where d = number of changed key-value pairs.
    ///
    /// # Resumability
    /// If interrupted and re-invoked, chunks already in dst are skipped.
    /// The operation is idempotent.
    fn transfer(
        &self,
        src: &dyn ChunkStore,
        dst: &dyn ChunkStore,
        root: &Hash,
    ) -> Result<TransferResult, FerraError>;
}

/// Default implementation: recursive descent with has_chunk pruning.
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
            root: root.clone(),
        };
        self.transfer_recursive(src, dst, root, &mut result)?;
        Ok(result)
    }
}

impl RecursiveTransfer {
    fn transfer_recursive(
        &self,
        src: &dyn ChunkStore,
        dst: &dyn ChunkStore,
        addr: &Hash,
        result: &mut TransferResult,
    ) -> Result<(), FerraError> {
        // Pruning: if dst already has this chunk, skip entire subtree
        if dst.has_chunk(addr)? {
            result.chunks_skipped += 1;
            return Ok(());
        }

        // Fetch from source
        let chunk = src.get_chunk(addr)?
            .ok_or(FerraError::ChunkNotFound(addr.clone()))?;

        // Store in destination (idempotent)
        dst.put_chunk(&chunk)?;
        result.chunks_transferred += 1;
        result.bytes_transferred += chunk.data().len() as u64;

        // Decode children and recurse
        let children = decode_child_addrs(&chunk)?;
        for child_addr in &children {
            self.transfer_recursive(src, dst, child_addr, result)?;
        }

        Ok(())
    }
}
```

**Falsification**: A transfer operation that either (a) fails to send a chunk that is
reachable from `root(src)` and not present in `dst`, leaving `resolve(dst, root(src))`
unable to navigate the full tree; (b) sends a chunk that is already present in `dst`
(redundant transfer); or (c) modifies or deletes a chunk already in `dst` (non-monotonic).

Concretely: after `transfer(src, dst, root)`, call `resolve(dst, root)` and verify that
every key-value pair in the source prolly tree is accessible. Then verify that
`chunks_transferred + chunks_skipped` equals the total number of chunks reachable
from `root` in `src`. Then verify that `chunks_transferred` equals exactly the number
of chunks in `src` that were not in `dst` before the transfer.

**proptest strategy**:
```rust
proptest! {
    #[test]
    fn transfer_correctness(
        base_kvs in prop::collection::btree_map(
            prop::collection::vec(any::<u8>(), 1..32),
            prop::collection::vec(any::<u8>(), 1..128),
            10..300
        ),
        changes in prop::collection::btree_map(
            prop::collection::vec(any::<u8>(), 1..32),
            prop::collection::vec(any::<u8>(), 1..128),
            1..50
        ),
        pattern_width in 4u32..10,
    ) {
        let src_store = MemoryChunkStore::new();
        let dst_store = MemoryChunkStore::new();

        // Build base tree in both stores (simulate shared history)
        let base_root = build_prolly_tree(&base_kvs, &src_store, pattern_width).unwrap();
        // Copy all base chunks to dst
        for addr in src_store.all_addrs().unwrap() {
            let chunk = src_store.get_chunk(&addr).unwrap().unwrap();
            dst_store.put_chunk(&chunk).unwrap();
        }

        // Apply changes to source only
        let mut modified_kvs = base_kvs.clone();
        for (k, v) in &changes {
            modified_kvs.insert(k.clone(), v.clone());
        }
        let new_root = build_prolly_tree(&modified_kvs, &src_store, pattern_width).unwrap();

        // Record dst state before transfer
        let dst_before = dst_store.all_addrs().unwrap();

        // Transfer
        let transfer = RecursiveTransfer;
        let result = transfer.transfer(&src_store, &dst_store, &new_root).unwrap();

        // Verify: new root is resolvable from dst
        let dst_kvs = read_prolly_tree(&new_root, &dst_store).unwrap();
        prop_assert_eq!(dst_kvs, modified_kvs,
            "Transfer did not make full tree accessible from dst");

        // Verify: no chunks deleted from dst
        for addr in &dst_before {
            prop_assert!(dst_store.has_chunk(addr).unwrap(),
                "Transfer deleted a pre-existing chunk from dst");
        }

        // Verify: transfer is idempotent (re-run sends nothing)
        let result2 = transfer.transfer(&src_store, &dst_store, &new_root).unwrap();
        prop_assert_eq!(result2.chunks_transferred, 0,
            "Second transfer should send zero chunks");
    }

    #[test]
    fn transfer_minimality(
        kvs in prop::collection::btree_map(
            prop::collection::vec(any::<u8>(), 1..32),
            prop::collection::vec(any::<u8>(), 1..128),
            50..500
        ),
        pattern_width in 4u32..10,
    ) {
        let src_store = MemoryChunkStore::new();
        let dst_store = MemoryChunkStore::new();

        let root = build_prolly_tree(&kvs, &src_store, pattern_width).unwrap();

        // Transfer to empty dst: should send all chunks
        let transfer = RecursiveTransfer;
        let result = transfer.transfer(&src_store, &dst_store, &root).unwrap();

        let src_addrs = src_store.all_addrs().unwrap();
        // All reachable chunks from root should now be in dst
        let dst_addrs = dst_store.all_addrs().unwrap();
        let reachable = reachable_addrs(&root, &src_store).unwrap();
        for addr in &reachable {
            prop_assert!(dst_addrs.contains(addr),
                "Reachable chunk {:?} missing from dst after transfer", addr);
        }

        prop_assert_eq!(result.chunks_transferred as usize, reachable.len(),
            "Transfer count should equal reachable chunk count");
    }
}
```

---

### INV-FERR-049: Snapshot = Root Hash

**Traces to**: INV-FERR-045 (Chunk Content Addressing), INV-FERR-006 (Snapshot Isolation),
C2 (Identity by Content)
**Verification**: `V:PROP`, `V:LEAN`
**Stage**: 1

#### Level 0 (Algebraic Law)
```
Let ProllyTree be the type of prolly trees.
Let root_hash : ProllyTree -> Hash extract the root's content address.
Let resolve : ChunkStore x Hash -> ProllyTree reconstruct a tree from its root hash.

Theorem (snapshot identity):
  forall T : ProllyTree, forall S : ChunkStore containing all chunks of T:
    resolve(S, root_hash(T)) = T

Proof:
  By structural induction on tree height:
  - Base case (leaf node): root_hash(T) is BLAKE3(content(T)). resolve(S, root_hash(T))
    retrieves the chunk, deserializes the leaf content, producing T. Content-addressing
    (INV-FERR-045) guarantees the retrieved chunk has the right content.
  - Inductive case (internal node): root_hash(T) addresses a chunk containing child hashes.
    resolve deserializes the child hashes and recursively resolves each child.
    By induction, each child resolves correctly. The reassembled tree = T.

Corollary (snapshot cost):
  Creating a snapshot costs O(1): store the root hash.
  The entire tree is already in the chunk store (immutable, content-addressed).
  No data is copied; the root hash IS the snapshot.

Corollary (version history):
  Storing a sequence of root hashes [h1, h2, ..., hn] provides a complete
  version history. Each hi resolves to the full store state at version i.
  Old versions remain accessible as long as their chunks are not garbage-collected.
  Since chunks are shared across versions, the incremental storage cost of each
  new version is O(d) (only the changed chunks), not O(n).
```

#### Level 1 (State Invariant)
For all reachable `(ProllyTree, ChunkStore)` pairs where the chunk store contains
all chunks reachable from the tree's root:
- `resolve(store, root_hash(tree))` produces a key-value set identical to the tree's
  key-value set. The round-trip is lossless.
- The root hash uniquely identifies the store state. Two stores with different key-value
  sets produce different root hashes (by history independence INV-FERR-046 and
  content-addressing INV-FERR-045, assuming collision resistance).
- Storing the root hash is sufficient to reconstruct the full store. The root hash is
  a O(32-byte) summary of an arbitrarily large store.
- Old root hashes remain valid as long as their chunks exist. The chunk store is append-only
  (chunks are never modified or deleted during normal operation). Garbage collection is an
  explicit, separate operation that the application controls.

This invariant is the foundation for the journal format (section 23.9.2): the journal stores
root hash updates, and each root hash is a complete snapshot of the store at that point.
Combined with the O(d) diff (INV-FERR-047), the journal enables efficient time-travel
queries: "diff the store between version V1 and V2."

#### Level 2 (Implementation Contract)
```rust
/// A snapshot is identified by a single root hash.
/// The root hash is a content-addressed pointer to an immutable prolly tree.
///
/// Creating a snapshot: store the root hash (O(1)).
/// Loading a snapshot: resolve the root hash through the chunk store.
/// Diffing snapshots: diff(root1, root2) using INV-FERR-047.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Snapshot {
    /// The root hash of the prolly tree at this point in time.
    root: Hash,
    /// The transaction that produced this snapshot (for ordering).
    tx: TxId,
}

impl Snapshot {
    /// Create a snapshot from a root hash and transaction.
    pub fn new(root: Hash, tx: TxId) -> Self {
        Snapshot { root, tx }
    }

    /// The root hash. This IS the snapshot.
    pub fn root(&self) -> &Hash { &self.root }

    /// The transaction that produced this state.
    pub fn tx(&self) -> &TxId { &self.tx }

    /// Resolve this snapshot to a full key-value set.
    /// O(n) where n = number of key-value pairs in the tree.
    pub fn resolve(
        &self,
        chunk_store: &dyn ChunkStore,
    ) -> Result<BTreeMap<Key, Value>, FerraError> {
        read_prolly_tree(&self.root, chunk_store)
    }

    /// Diff this snapshot against another.
    /// O(d * log_k(n)) where d = number of changed key-value pairs.
    pub fn diff<'a>(
        &self,
        other: &Snapshot,
        chunk_store: &'a dyn ChunkStore,
    ) -> Result<impl Iterator<Item = Result<DiffEntry, FerraError>> + 'a, FerraError> {
        diff(&self.root, &other.root, chunk_store)
    }
}

/// A version history: sequence of snapshots (root hashes).
/// Storage cost: O(versions x 32 bytes) for the hash list,
/// plus O(d_total) chunks where d_total is the cumulative changes across versions.
pub struct VersionHistory {
    /// Ordered list of (TxId, root_hash) pairs.
    versions: Vec<Snapshot>,
}

impl VersionHistory {
    /// Get the snapshot at a specific version.
    pub fn at_version(&self, tx: &TxId) -> Option<&Snapshot> {
        self.versions.iter().find(|s| s.tx() == tx)
    }

    /// Get the latest snapshot.
    pub fn latest(&self) -> Option<&Snapshot> {
        self.versions.last()
    }

    /// Diff between two versions.
    pub fn diff_versions<'a>(
        &self,
        from: &TxId,
        to: &TxId,
        chunk_store: &'a dyn ChunkStore,
    ) -> Result<impl Iterator<Item = Result<DiffEntry, FerraError>> + 'a, FerraError> {
        let snap_from = self.at_version(from)
            .ok_or(FerraError::VersionNotFound(from.clone()))?;
        let snap_to = self.at_version(to)
            .ok_or(FerraError::VersionNotFound(to.clone()))?;
        snap_from.diff(snap_to, chunk_store)
    }
}
```

**Falsification**: A `resolve(store, root_hash(tree))` call that produces a key-value set
different from the original tree's key-value set. Concretely: build a prolly tree from
key-value set KV, extract `root_hash`, then `resolve(store, root_hash)` and compare the
result to KV. Any difference — a missing key, a wrong value, an extra key — constitutes
a violation.

**proptest strategy**:
```rust
proptest! {
    #[test]
    fn snapshot_roundtrip(
        kvs in prop::collection::btree_map(
            prop::collection::vec(any::<u8>(), 1..32),
            prop::collection::vec(any::<u8>(), 1..256),
            0..500
        ),
        pattern_width in 4u32..12,
    ) {
        let store = MemoryChunkStore::new();
        let root = build_prolly_tree(&kvs, &store, pattern_width).unwrap();

        let snapshot = Snapshot::new(root, TxId::genesis());
        let resolved = snapshot.resolve(&store).unwrap();

        prop_assert_eq!(resolved, kvs,
            "Snapshot roundtrip lost data: built from {:?}, resolved to {:?}",
            kvs.len(), resolved.len());
    }

    #[test]
    fn snapshot_identity(
        kvs in prop::collection::btree_map(
            prop::collection::vec(any::<u8>(), 1..32),
            prop::collection::vec(any::<u8>(), 1..256),
            1..200
        ),
        pattern_width in 4u32..12,
    ) {
        let store1 = MemoryChunkStore::new();
        let store2 = MemoryChunkStore::new();

        let root1 = build_prolly_tree(&kvs, &store1, pattern_width).unwrap();
        let root2 = build_prolly_tree(&kvs, &store2, pattern_width).unwrap();

        prop_assert_eq!(root1, root2,
            "Same key-value set must produce same root hash (snapshot identity)");
    }

    #[test]
    fn snapshot_distinct_for_different_data(
        kvs1 in prop::collection::btree_map(
            prop::collection::vec(any::<u8>(), 1..32),
            prop::collection::vec(any::<u8>(), 1..256),
            1..200
        ),
        kvs2 in prop::collection::btree_map(
            prop::collection::vec(any::<u8>(), 1..32),
            prop::collection::vec(any::<u8>(), 1..256),
            1..200
        ),
        pattern_width in 4u32..12,
    ) {
        prop_assume!(kvs1 != kvs2);

        let store = MemoryChunkStore::new();
        let root1 = build_prolly_tree(&kvs1, &store, pattern_width).unwrap();
        let root2 = build_prolly_tree(&kvs2, &store, pattern_width).unwrap();

        prop_assert_ne!(root1, root2,
            "Different key-value sets must produce different root hashes");
    }
}
```

**Lean theorem**:
```lean
/-- Snapshot = root hash: resolve(store, root_hash(tree)) = tree.
    Modeled as: the root hash is a faithful representation of the key-value set. -/

/-- A snapshot is a root hash. Creating it is O(1). -/
def snapshot (tree : ProllyTree) : Hash := root_hash tree

/-- Resolving a snapshot recovers the original key-value set. -/
theorem snapshot_roundtrip (tree : ProllyTree) (store : ChunkStore)
    (h : chunks_of tree ⊆ chunks_in store) :
    resolve store (snapshot tree) = key_values tree := by
  induction tree with
  | leaf kvs =>
    simp [snapshot, root_hash, resolve, key_values]
    exact chunk_retrieve_correct store (root_hash (leaf kvs)) h
  | node children ih =>
    simp [snapshot, root_hash, resolve, key_values]
    congr 1
    exact ih (chunks_subset_of_children h)

/-- Two trees with the same key-value set have the same snapshot. -/
theorem snapshot_deterministic (t1 t2 : ProllyTree)
    (h : key_values t1 = key_values t2) :
    snapshot t1 = snapshot t2 := by
  exact history_independence_root h
```

---

### INV-FERR-050: Block Store Substrate Independence

**Traces to**: C8 (Substrate Independence), INV-FERR-024 (Substrate Agnosticism),
ADR-FERR-008 (Storage Engine), INV-FERR-045 (Chunk Content Addressing)
**Verification**: `V:PROP`
**Stage**: 1

#### Level 0 (Algebraic Law)
```
Let ChunkStore be the typeclass of chunk stores, with operations:
  put_chunk : ChunkStore -> Chunk -> Hash
  get_chunk : ChunkStore -> Hash -> Option Chunk
  has_chunk : ChunkStore -> Hash -> Bool

Let FileChunkStore, MemoryChunkStore, S3ChunkStore be instances of ChunkStore.

Theorem (substrate independence):
  forall ops : List ChunkOp, forall S1 S2 : ChunkStore instances:
    apply(ops, S1) ~ apply(ops, S2)
  where ~ denotes observational equivalence: the same sequence of get_chunk calls
  returns the same results.

Proof:
  Each ChunkStore instance implements the same interface. Content-addressing
  (INV-FERR-045) guarantees that put_chunk(chunk) stores under addr = BLAKE3(content).
  The storage medium (file, memory, S3) is invisible to the caller. Two stores
  that have received the same put_chunk calls produce the same get_chunk results.
  The algebraic content is independent of the physical substrate. QED.

Corollary (migration):
  For any ChunkStore instances S_old, S_new:
    transfer(S_old, S_new, root) followed by using S_new is observationally
    equivalent to continuing to use S_old. The prolly tree structure, root hash,
    and all query results are identical.
```

#### Level 1 (State Invariant)
For all `ChunkStore` implementations:
- The same sequence of `put_chunk` and `get_chunk` operations produces the same
  observable results regardless of the implementation. A `MemoryChunkStore` used
  in tests is observationally equivalent to a `FileChunkStore` used in production,
  which is observationally equivalent to an `S3ChunkStore` used in cloud deployments.
- Application code (prolly tree construction, diff, transfer) uses only the `ChunkStore`
  trait. It never imports or references a specific implementation. Swapping the storage
  backend requires changing one line of initialization code, not the application logic.
- The `ChunkStore` trait is the ONLY interface between application code and physical storage.
  There are no backdoors (direct file I/O, hardcoded paths, platform-specific APIs)
  in the application layer.

Substrate independence extends C8 from the logical layer (no DDIS-specific logic in kernel)
to the physical layer (no filesystem-specific logic in the application). The same prolly
tree, the same root hash, the same chunks, the same diff algorithm — different atoms
(filesystem blocks, RAM cells, S3 objects) underneath.

#### Level 2 (Implementation Contract)
```rust
/// File-based chunk store. Chunks stored as individual files in a directory.
/// File name = hex(address). Content = raw chunk bytes.
/// Suitable for local development and single-machine deployments.
pub struct FileChunkStore {
    /// Root directory for chunk files.
    root_dir: PathBuf,
}

impl ChunkStore for FileChunkStore {
    fn put_chunk(&self, chunk: &Chunk) -> Result<Hash, FerraError> {
        let path = self.chunk_path(chunk.addr());
        if path.exists() {
            return Ok(chunk.addr().clone()); // Idempotent: already stored
        }
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, chunk.data())?;
        std::fs::rename(&tmp, &path)?; // Atomic on POSIX
        Ok(chunk.addr().clone())
    }

    fn get_chunk(&self, addr: &Hash) -> Result<Option<Chunk>, FerraError> {
        let path = self.chunk_path(addr);
        if !path.exists() {
            return Ok(None);
        }
        let data = std::fs::read(&path)?;
        let chunk = Chunk::from_bytes(&data);
        // Verify content-addressing invariant
        if chunk.addr() != addr {
            return Err(FerraError::ChunkCorruption {
                expected: addr.clone(),
                actual: chunk.addr().clone(),
            });
        }
        Ok(Some(chunk))
    }

    fn has_chunk(&self, addr: &Hash) -> Result<bool, FerraError> {
        Ok(self.chunk_path(addr).exists())
    }

    fn all_addrs(&self) -> Result<BTreeSet<Hash>, FerraError> {
        let mut addrs = BTreeSet::new();
        for entry in std::fs::read_dir(&self.root_dir)? {
            let entry = entry?;
            if let Some(name) = entry.file_name().to_str() {
                if let Ok(hash) = Hash::from_hex(name) {
                    addrs.insert(hash);
                }
            }
        }
        Ok(addrs)
    }
}

/// In-memory chunk store. For testing and ephemeral use.
/// No persistence, no I/O, no filesystem.
pub struct MemoryChunkStore {
    chunks: RwLock<BTreeMap<Hash, Arc<[u8]>>>,
}

impl ChunkStore for MemoryChunkStore {
    fn put_chunk(&self, chunk: &Chunk) -> Result<Hash, FerraError> {
        let mut map = self.chunks.write().map_err(|_| FerraError::LockPoisoned)?;
        map.entry(chunk.addr().clone())
            .or_insert_with(|| chunk.data().into());
        Ok(chunk.addr().clone())
    }

    fn get_chunk(&self, addr: &Hash) -> Result<Option<Chunk>, FerraError> {
        let map = self.chunks.read().map_err(|_| FerraError::LockPoisoned)?;
        match map.get(addr) {
            Some(data) => Ok(Some(Chunk::from_bytes(data))),
            None => Ok(None),
        }
    }

    fn has_chunk(&self, addr: &Hash) -> Result<bool, FerraError> {
        let map = self.chunks.read().map_err(|_| FerraError::LockPoisoned)?;
        Ok(map.contains_key(addr))
    }

    fn all_addrs(&self) -> Result<BTreeSet<Hash>, FerraError> {
        let map = self.chunks.read().map_err(|_| FerraError::LockPoisoned)?;
        Ok(map.keys().cloned().collect())
    }
}

/// S3-backed chunk store. Chunks stored as objects in an S3 bucket.
/// Object key = hex(address). Object body = raw chunk bytes.
/// Suitable for distributed and cloud deployments.
///
/// Note: S3ChunkStore uses synchronous HTTP calls internally
/// (consistent with ADR-FERR-002: no async in ferratomic).
/// The caller can wrap in spawn_blocking if needed.
pub struct S3ChunkStore {
    /// S3 bucket name.
    bucket: String,
    /// Key prefix for chunk objects (e.g., "chunks/").
    prefix: String,
    /// S3 client (synchronous).
    client: S3Client,
}
// Implementation follows the same trait contract as FileChunkStore and MemoryChunkStore.
```

**Falsification**: Application code that uses `FileChunkStore`-specific methods,
filesystem paths, or I/O operations instead of the `ChunkStore` trait. Concretely:
any `use crate::FileChunkStore` import in application-layer code (prolly tree, diff,
transfer) outside of initialization/configuration. Also: a test suite that passes
with `MemoryChunkStore` but fails with `FileChunkStore` (or vice versa) on the same
operations, indicating implementation-dependent behavior that violates the trait contract.

**proptest strategy**:
```rust
proptest! {
    #[test]
    fn substrate_independence_memory_vs_file(
        kvs in prop::collection::btree_map(
            prop::collection::vec(any::<u8>(), 1..32),
            prop::collection::vec(any::<u8>(), 1..128),
            1..200
        ),
        pattern_width in 4u32..10,
    ) {
        let mem_store = MemoryChunkStore::new();
        let tmp_dir = tempfile::tempdir().unwrap();
        let file_store = FileChunkStore::new(tmp_dir.path());

        let root_mem = build_prolly_tree(&kvs, &mem_store, pattern_width).unwrap();
        let root_file = build_prolly_tree(&kvs, &file_store, pattern_width).unwrap();

        // Same data -> same root hash regardless of backend
        prop_assert_eq!(root_mem, root_file,
            "Root hash differs between MemoryChunkStore and FileChunkStore");

        // Same data retrievable from both
        let resolved_mem = read_prolly_tree(&root_mem, &mem_store).unwrap();
        let resolved_file = read_prolly_tree(&root_file, &file_store).unwrap();
        prop_assert_eq!(resolved_mem, resolved_file,
            "Resolved data differs between substrates");
        prop_assert_eq!(resolved_mem, kvs,
            "Resolved data differs from original");

        // Same chunk addresses in both stores
        let addrs_mem = mem_store.all_addrs().unwrap();
        let addrs_file = file_store.all_addrs().unwrap();
        prop_assert_eq!(addrs_mem, addrs_file,
            "Chunk address sets differ between substrates");
    }
}
```

---

### S23.9.1: Prolly Tree Architecture

The prolly tree (probabilistic B-tree) is a content-addressed, history-independent
sorted key-value structure. It combines the logarithmic access guarantees of B-trees
with the structural sharing and diffability of Merkle trees, using a rolling hash
to determine chunk boundaries probabilistically rather than by fixed-capacity pages.

#### Leaf Layer

Key-value pairs are sorted by key (total ordering on byte sequences) and packed
into leaf chunks. Chunk boundaries are determined by the rolling hash boundary
function (INV-FERR-046): a key `k` is a boundary if `rolling_hash(k) & mask == mask`
where `mask = (1 << pattern_width) - 1`.

The boundary function operates on **keys only**, not values. This is Dolt's improvement
over the original Noms design, which hashed (key, value) pairs. Key-only boundaries
mean that updating a value never changes the chunk structure — only the chunk containing
that key is rewritten. The structural impact of a point mutation is O(1) chunks at the
leaf level plus O(log_k(n)) internal node updates on the path to root.

```
Leaf chunk format:
  [entry_count: u32]
  [entries: [(key_len: u32, key: [u8], value_len: u32, value: [u8])] x entry_count]

Chunk address: BLAKE3(serialized leaf chunk)
```

Each leaf chunk contains between 1 and `2 x expected_chunk_size` entries, where
`expected_chunk_size = 2^pattern_width`. The distribution follows a geometric distribution
with parameter `p = 2^(-pattern_width)`, so the expected chunk size is `1/p = 2^pattern_width`.

#### Chunk Size Distribution

The rolling hash boundary creates a geometric distribution of chunk sizes. With
`pattern_width = 12`, the expected chunk size is 4096 entries. However, the variance
is high (standard deviation = expected size). To achieve tighter chunk size distribution,
Dolt's CDF (cumulative distribution function) approach is used:

```
CDF boundary function:
  boundary(key, min_size, max_size, pattern_width) =
    if entries_since_last_boundary < min_size: false  (never split too small)
    if entries_since_last_boundary >= max_size: true   (always split at max)
    if entries_since_last_boundary >= min_size:
      rolling_hash(key) & mask == mask                 (probabilistic split)

Typical parameters:
  min_size = expected / 4 = 1024  (minimum 1KB chunk)
  max_size = expected x 4 = 16384 (maximum 16KB chunk)
  expected = 4096                 (target ~4KB chunks)
```

The CDF approach bounds chunk sizes to `[min_size, max_size]` while preserving
history independence: the boundary function depends only on the key sequence and
the position within the current run, not on insertion history.

**Important**: The `entries_since_last_boundary` counter resets at each boundary.
Since boundaries are determined by key content (not insertion order), the counter
value at any position is a function of the sorted key sequence up to that point.
History independence is preserved.

#### Content-Addressed Hashing

Every chunk — leaf and internal — has address `BLAKE3(serialized_content)`.

BLAKE3 is chosen over SHA-256 for three reasons:
1. **Performance**: BLAKE3 is ~5x faster than SHA-256 on modern hardware (vectorized,
   tree-structured internally).
2. **Security**: 256-bit output provides 128-bit collision resistance, sufficient for
   content addressing.
3. **Consistency**: The rest of ferratomic already uses BLAKE3 for entity IDs (INV-FERR-012).

#### Internal Node Layer

Internal nodes are constructed recursively from the layer below:

```
Level 0 (leaves):  [chunk_a] [chunk_b] [chunk_c] [chunk_d] ...
Level 1 (internal): [node_X: (last_key_a, addr_a), (last_key_b, addr_b), ...]
Level 2 (internal): [node_Y: (last_key_X, addr_X), ...]
...
Level h (root):     [root_node: ...]
```

Each internal node entry is `(last_key_of_child, child_chunk_address)`. Internal nodes
use the same rolling hash boundary function on the `last_key` values to determine
internal chunk boundaries. This recursive application of the same boundary function at
every level ensures history independence at every level (INV-FERR-046 is structural, not
level-specific).

```
Internal chunk format:
  [entry_count: u32]
  [level: u8]
  [entries: [(last_key_len: u32, last_key: [u8], child_addr: [u8; 32])] x entry_count]

Chunk address: BLAKE3(serialized internal chunk)
```

The tree height is `h = ceil(log_k(n))` where `k` is the effective fanout
(expected chunk size) and `n` is the number of key-value pairs. For k=4096 and
n=100M: h = ceil(log_4096(10^8)) = ceil(2.2) = 3 levels.

#### Write Path: Copy-on-Write

Inserting or updating a key-value pair:
1. Navigate from root to the leaf containing the key (or where the key would be inserted).
2. Modify the leaf chunk (insert/update/delete the key-value pair).
3. Recompute the leaf chunk's hash (the chunk is now a NEW chunk with a new address).
4. Update the parent internal node to reference the new leaf chunk address.
5. Recompute the parent's hash. Repeat up to the root.
6. The new root hash is the snapshot of the updated store.

Only the chunks on the path from the modified leaf to the root are new. All sibling
chunks are unchanged and shared with the previous version. This is copy-on-write at
the chunk level, not the page level: the granularity of sharing is determined by the
rolling hash, not by a fixed page size.

```
Cost of a single-key write:
  - 1 leaf chunk rewritten
  - log_k(n) internal chunks rewritten
  - Total: (1 + log_k(n)) new chunks
  - For k=4096, n=100M: 1 + 3 = 4 new chunks

Cost of a batch write (w keys in the same leaf):
  - Same as single write if all keys fall in the same leaf chunk
  - Cost: (1 + log_k(n)) new chunks
  - Amortized per key: (1 + log_k(n)) / w

Cost of a batch write (w keys across different leaves):
  - w leaf chunks rewritten
  - At most w * log_k(n) internal chunks rewritten (often fewer due to shared paths)
  - Total: O(w * log_k(n)) new chunks
```

#### Read Path

Reading a key-value pair:
1. Start at the root chunk (known root hash).
2. Binary search the internal node entries for the key (O(log k) per level).
3. Descend to the child chunk containing the key.
4. Repeat until leaf level.
5. Binary search the leaf entries for the key.

```
Cost: O(log_k(n)) chunk reads, each with O(log k) binary search.
Total: O(log_k(n) * log(k)) = O(log(n)) comparisons.
With mmap: O(log(n)) memory accesses (zero-copy, no deserialization).
```

#### Tree Reconstruction

Building a prolly tree from a sorted key-value iterator is a single-pass O(n) operation:

1. Iterate over sorted key-value pairs.
2. Accumulate entries into the current leaf chunk.
3. When a key triggers a boundary (rolling hash), finalize the current chunk:
   serialize, hash, store, record (last_key, address) for the parent level.
4. After all entries: finalize the last chunk (no boundary trigger needed).
5. Repeat steps 2-4 for the internal node entries, building the next level.
6. Continue until a single root chunk remains.

This O(n) construction is used for the initial checkpoint (Phase 4a to Phase 4b migration)
and for rebuilding after corruption. Incremental updates use the copy-on-write path instead.

---

### S23.9.2: Block Store Format

The on-disk format is inspired by Dolt's Noms Block Store (NBS) with simplifications
appropriate for an embedded database rather than a MySQL-compatible server.

#### Table Files

A table file is a collection of chunks packed into a single file with an index.
Table files are immutable after creation (append-only at the chunk level translates
to immutable files at the filesystem level). New chunks go into a new table file
or the active journal.

```
Table file layout:
  [data section: chunks packed sequentially]
  [index section:]
    [chunk_count: u64]
    [prefix_map: [(addr_prefix: [u8; 8], ordinal: u32)] x chunk_count]
    [lengths: [chunk_len: u32] x chunk_count]
    [suffixes: [addr_suffix: [u8; 24]] x chunk_count]
  [footer:]
    [index_offset: u64]
    [chunk_count: u64]
    [magic: [u8; 4] = "FBLK"]
    [version: u8 = 1]
```

**Index structure**: The prefix map enables O(log n) lookup by chunk address:
1. Binary search the prefix map for the 8-byte prefix of the target address.
2. Multiple prefixes may match (prefix collisions). For each matching ordinal:
3. Check the full address: prefix_map[ordinal].prefix ++ suffixes[ordinal].
4. If full address matches: the chunk data starts at `sum(lengths[0..ordinal])`
   and has length `lengths[ordinal]`.

The 8-byte prefix provides a 2^64 address space for the prefix map, making collisions
extremely rare (expected collision rate < 1 at 4 billion chunks per table file).

#### Manifest

The manifest tracks the current set of table files and the current root hash.
It is the ONLY mutable file in the block store (updated atomically on checkpoint).

```
Manifest format (JSON for readability, binary for production):
  {
    "version": 1,
    "root": "<hex-encoded root hash>",
    "tables": [
      { "name": "table_001.fblk", "chunk_count": 4096, "size_bytes": 16777216 },
      { "name": "table_002.fblk", "chunk_count": 2048, "size_bytes": 8388608 },
      ...
    ],
    "lock": "<hex-encoded lock hash for atomic update>"
  }
```

Manifest updates use the compare-and-swap pattern:
1. Read current manifest, record its hash.
2. Compute new manifest (add table files, update root).
3. Write new manifest to a temp file.
4. Atomic rename temp file to manifest file.
5. If another writer has updated the manifest since step 1 (detected by lock hash
   mismatch), retry from step 1.

This is the same pattern used by Dolt for concurrent manifest updates. For ferratomic's
single-writer model (INV-FERR-007), the CAS is a safety check, not a performance concern.

#### Journal

The journal is an append-only log of chunk writes and root hash updates. It serves
the same role as the WAL (INV-FERR-008) extended to the block store layer:
writes go to the journal first, and periodic compaction folds journal entries into
new table files.

```
Journal format:
  [record]*

  Record types:
    ChunkWrite:
      [tag: u8 = 0x01]
      [addr: [u8; 32]]
      [data_len: u32]
      [data: [u8; data_len]]
      [crc32: u32]   // CRC32 of tag + addr + data_len + data

    RootUpdate:
      [tag: u8 = 0x02]
      [root: [u8; 32]]
      [tx_id: [u8; 20]]  // HLC timestamp
      [crc32: u32]

    Checkpoint:
      [tag: u8 = 0x03]
      [table_name_len: u16]
      [table_name: [u8; table_name_len]]
      [chunk_count: u64]
      [crc32: u32]
```

**Recovery**: On startup, the block store:
1. Reads the manifest to discover table files and the last committed root hash.
2. Reads the journal from the position after the last Checkpoint record.
3. Replays ChunkWrite records into an in-memory chunk index.
4. The current root is the last RootUpdate in the journal (or manifest if no journal entries).
5. Journal chunks are served alongside table file chunks until the next compaction.

**Compaction**: Periodically (or on explicit checkpoint), journal chunks are written
into a new table file, a new manifest is written, and the journal is truncated:
1. Collect all ChunkWrite records since last Checkpoint.
2. Build a new table file containing those chunks.
3. Update manifest to include the new table file and the latest root hash.
4. Write a Checkpoint record to the journal referencing the new table file.
5. Truncate journal records before the Checkpoint (or start a new journal file).

#### Garbage Collection

Chunks become unreachable when no root hash in the version history references them
(directly or transitively). Garbage collection reclaims their storage:

1. **Mark phase**: Starting from all retained root hashes (current + any historical
   snapshots the application wants to keep), traverse the prolly trees and mark all
   reachable chunk addresses.
2. **Sweep phase**: For each table file, check which chunks are marked. If a table
   file has < 50% reachable chunks, rewrite it containing only reachable chunks.
   If 100% reachable, keep the table file as-is.
3. **Update manifest**: Remove fully-swept table files, add rewritten table files.

Garbage collection is an explicit operation, not automatic. The application controls
which root hashes to retain (e.g., keep the last 10 snapshots, or keep all snapshots
from the last 7 days). This is consistent with C1 (append-only during normal operation):
GC is a separate, explicit lifecycle event, not an automatic background process.

---

### S23.9.3: Integration with im::OrdMap

The in-memory representation (`im::OrdMap` from Phase 4a) and the on-disk representation
(prolly tree from Phase 4b) coexist as complementary substrates for the same logical
store. Neither replaces the other in Phase 4b; they serve different operational needs.

#### Phase 4a: im::OrdMap Only

In Phase 4a, the store exists only in memory with flat checkpoints:
- **In-memory**: `im::OrdMap<Key, Value>` with `ArcSwap` for lock-free snapshots.
- **On-disk**: Flat `store.bin` checkpoint (full serialization via EDN or bincode).
- **Snapshot**: `Arc::clone()` of the `im::OrdMap` — O(1), structural sharing in RAM.
- **Checkpoint**: Serialize entire `im::OrdMap` to disk — O(n).
- **Recovery**: Deserialize `store.bin` + replay WAL — O(n).

This is simple, correct, and sufficient for single-machine stores up to ~10M datoms.

#### Phase 4b: Prolly Tree Block Store

Phase 4b adds the prolly tree as the durable, diffable, transferable on-disk format:
- **In-memory**: `im::OrdMap<Key, Value>` (unchanged from 4a).
- **On-disk**: Prolly tree chunks in block store (table files + journal).
- **Snapshot (in-memory)**: `Arc::clone()` of `im::OrdMap` — O(1), unchanged.
- **Snapshot (on-disk)**: Store root hash — O(1).
- **Checkpoint**: Diff the current `im::OrdMap` against the last checkpointed prolly tree,
  write only changed chunks — O(d) where d = datoms changed since last checkpoint.
- **Recovery**: Resolve the latest root hash from the block store, reconstruct `im::OrdMap` — O(n).
- **Diff (for federation)**: Compare two root hashes — O(d).
- **Transfer (for federation)**: Send changed chunks — O(d).

#### Checkpoint: im::OrdMap to Prolly Tree

```
1. Get current im::OrdMap state (lock-free read via ArcSwap)
2. If first checkpoint: build_prolly_tree(ordmap, chunk_store, pattern_width) -- O(n)
3. If subsequent checkpoint:
   a. Identify changed keys since last checkpoint (dirty tracking or full diff)
   b. For each changed key, update the prolly tree via copy-on-write -- O(d * log_k(n))
   c. Store the new root hash in the journal (RootUpdate record)
4. Periodically compact the journal into table files
```

The checkpoint is incremental after the first: only changed chunks are written.
The cost is proportional to the number of changed datoms, not the total store size.

**Dirty tracking**: Rather than diffing the entire `im::OrdMap` against the prolly tree
on every checkpoint, the transact path can maintain a dirty set of changed keys since
the last checkpoint. The checkpoint then processes only the dirty set. This reduces
checkpoint cost from O(n) (full diff) to O(d) (dirty set size) even without comparing
against the prolly tree.

#### Load: Prolly Tree to im::OrdMap

```
1. Read the latest root hash from the manifest (or journal)
2. Resolve the root hash through the chunk store:
   a. Load root chunk, decode internal node entries
   b. For each child, load chunk and recurse
   c. At leaf level, decode key-value pairs
3. Insert all key-value pairs into a new im::OrdMap
4. Publish the im::OrdMap via ArcSwap
```

Load is O(n) — every key-value pair must be read from the chunk store and inserted
into the `im::OrdMap`. This is acceptable because load happens once at startup (or
on recovery after crash).

#### Future: Prolly Tree as Primary Index

In a potential future phase (not specified here), the prolly tree could serve as the
primary query index, eliminating the need for `im::OrdMap` entirely:

- **Read path**: Navigate the prolly tree chunks directly (mmap for zero-copy access).
- **Write path**: Copy-on-write on the prolly tree, publish new root hash via ArcSwap.
- **Snapshot**: Store root hash (already O(1)).
- **Memory**: Only hot chunks in the OS page cache, not the entire store in application memory.

This would reduce memory usage from O(n) (full `im::OrdMap`) to O(working_set) (mmap'd
chunks), enabling stores far larger than available RAM. However, point read latency
increases from O(log n) (in-memory HAMT) to O(log_k(n)) chunk reads (potentially from disk).

This future direction is documented but not specified. Phase 4b's scope is limited to
adding the prolly tree as a durable, diffable checkpoint format alongside the existing
`im::OrdMap` in-memory representation.

---

### S23.9.4: Performance Characteristics

| Operation | Prolly Tree (Phase 4b) | im::OrdMap (Phase 4a) | Flat store.bin (current) |
|-----------|------------------------|----------------------|--------------------------|
| Point read | O(log_k n) chunk reads | O(log n) in-memory | N/A (deserialize first) |
| Point write | (1+k/w) * log_k(n) chunks | O(log n) in-memory | N/A |
| Range scan | O(log_k n + r) where r = result count | O(log n + r) | N/A |
| Diff (d changes) | O(d * log_k n) | O(n) (full comparison) | O(n) (byte comparison) |
| Snapshot creation | O(1) (store root hash) | O(1) (Arc clone) | O(n) (serialize all) |
| Checkpoint to disk | O(d * log_k n) (only changed chunks) | N/A (im::OrdMap is in-memory only) | O(n) (full serialize) |
| Transfer (d changes) | O(d) chunks | N/A | O(n) (full copy) |
| Structural sharing (on-disk) | Yes (content-addressed chunks) | N/A (no disk representation) | No |
| Structural sharing (in-memory) | No (chunks are independent) | Yes (HAMT nodes) | No |
| History / version keeping | Yes (old roots reference immutable chunks) | No (old snapshots GC'd by Arc) | No |
| Memory usage | O(working_set) with mmap, O(n) if loaded to im::OrdMap | O(n) always in memory | O(n) on load |
| Build from sorted iterator | O(n) single pass | O(n log n) insertions | O(n) serialization |
| Recovery from crash | O(journal_size) replay + O(n) optional rebuild | O(n) deserialize store.bin + WAL replay | O(n) |

**Key insight**: The prolly tree and `im::OrdMap` have complementary strengths:
- `im::OrdMap` is optimal for in-memory operations (O(log n) point access, O(1) snapshots via Arc).
- Prolly tree is optimal for on-disk operations (O(d) checkpoint, O(d) diff, O(d) transfer).

Phase 4b uses both: `im::OrdMap` for the hot path (in-memory queries), prolly tree for
the cold path (persistence, federation, version history). Phase 4a's flat checkpoint is
replaced by the incremental prolly tree checkpoint. The `im::OrdMap` in-memory representation
is unchanged.

**Concrete numbers** (estimates for k=4096, n=100M datoms, ~200 bytes/datom):

| Metric | Value |
|--------|-------|
| Store size on disk | ~20GB (raw), ~18GB with chunk deduplication |
| Tree height | 3 levels (leaf, one internal, root) |
| Chunk size (expected) | ~4KB (~20 datoms/chunk) |
| Total chunks | ~5M leaf + ~1,300 internal + 1 root |
| Point read | 3 chunk reads (~12KB) |
| Point write | 4 new chunks (~16KB) |
| Diff (100 changes) | ~300 chunk reads |
| Checkpoint (100 changes) | ~400 chunk writes (~1.6MB) vs 20GB flat serialize |
| Transfer (100 changes) | ~300 chunks (~1.2MB) vs 20GB full copy |
| Snapshot (version history) | 32 bytes per version (root hash only) |

---

### S23.9.5: Substrate Migration with Prolly Trees

The `ChunkStore` trait (INV-FERR-050) enables transparent migration between storage
substrates. The prolly tree structure is independent of where chunks physically reside.

#### Migration Protocol

Moving a store from local filesystem to cloud:

1. **Source verification**: Verify the source `FileChunkStore` is consistent:
   resolve `root_hash`, walk the full tree, verify all chunks are accessible
   and content-addressing holds (BLAKE3(content) == address for each chunk).

2. **Chunk transfer**: Use `ChunkTransfer` (INV-FERR-048) to copy all chunks
   from `FileChunkStore` to `S3ChunkStore`:
   ```rust
   let transfer = RecursiveTransfer;
   let result = transfer.transfer(&file_store, &s3_store, &current_root)?;
   // result.chunks_transferred == total reachable chunks
   // result.chunks_skipped == 0 (S3 store was empty)
   ```

3. **Destination verification**: Resolve the same `root_hash` from the `S3ChunkStore`
   and verify the full tree is accessible. The resolved key-value set must be identical
   to the source (proptest: `resolved_src == resolved_dst`).

4. **Manifest update**: Update the manifest to reference the S3 backend instead of
   the file backend. The root hash, chunk addresses, and prolly tree structure are
   unchanged — only the physical location of chunks changes.

5. **Cache layer** (optional): Configure the local `FileChunkStore` as a read-through
   cache for the `S3ChunkStore`. Hot chunks are served from local disk; cold chunks
   are fetched from S3 on first access and cached locally.
   ```rust
   let cached_store = CachedChunkStore::new(
       file_store,  // local cache
       s3_store,    // remote source
   );
   ```

6. **Cutover**: All new writes go to the S3 backend (via the cached store or directly).
   The local file store is either retained as cache or decommissioned.

**Zero application code changes**: The application uses `&dyn ChunkStore` throughout.
Swapping `FileChunkStore` for `S3ChunkStore` (or `CachedChunkStore`) requires changing
one initialization line. The prolly tree, diff, transfer, and all query algorithms are
unchanged. This is INV-FERR-050 (substrate independence) in practice.

**Bidirectional migration**: The same protocol works for cloud-to-local migration (e.g.,
downloading a store for local analysis). The `ChunkTransfer` trait is symmetric: any
`ChunkStore` can be source or destination.

---

### S23.9.6: Relationship to Federation

Prolly trees make federation (section 23.8) dramatically more efficient by providing the
structural foundation for three federation operations:

#### Anti-Entropy (INV-FERR-022)

The anti-entropy convergence protocol from INV-FERR-022 requires peers to identify
and exchange missing datoms. Without prolly trees, this requires exchanging full datom
sets (O(n)) or maintaining separate change logs. With prolly trees:

1. **Compare root hashes**: If equal, stores are identical. Done in O(1).
2. **Diff the prolly trees**: `diff(root_local, root_remote)` produces exactly the
   changed key-value pairs in O(d * log_k(n)) time.
3. **Transfer missing chunks**: `transfer(remote_store, local_store, root_remote)`
   sends only the O(d) chunks the local store doesn't have.

This IS the Merkle anti-entropy protocol, naturally implemented by the prolly tree's
content-addressed structure. No separate anti-entropy data structure is needed — the
prolly tree IS the anti-entropy index.

#### Selective Merge (INV-FERR-039)

Selective merge with attribute-namespace filters is more efficient with prolly trees
when the index keys include the attribute as a prefix:

1. **Diff the remote prolly tree** against the local: get changed datoms.
2. **Filter by attribute namespace**: discard datoms outside the desired namespaces.
3. **Transfer only matching chunks**: or, if the attribute is the key prefix, navigate
   directly to the relevant subtree and transfer only those chunks.

Without prolly trees, selective merge requires full-store transfer followed by local
filtering — O(n) transfer, O(n) filter. With prolly trees and attribute-prefixed keys:
O(d_namespace) transfer where d_namespace is the diff within the target namespace.

#### Bandwidth

| Federation Operation | Without Prolly Trees | With Prolly Trees |
|---------------------|---------------------|-------------------|
| Anti-entropy sync | O(n) datoms transferred | O(d) chunks transferred |
| Detect differences | O(n) comparison | O(d * log_k n) hash comparisons |
| Selective merge | O(n) transfer + O(n) filter | O(d_ns) chunks |
| Version comparison | Not possible (no history) | O(1) root hash comparison |
| Incremental backup | O(n) full copy | O(d) changed chunks |

For a 100M-datom store with 100 changed datoms since last sync:
- Without prolly trees: ~20GB transferred
- With prolly trees: ~1.2MB transferred (300 chunks x ~4KB)
- Speedup: ~17,000x
