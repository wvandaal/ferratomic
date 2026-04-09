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

### S23.9.0: Canonical Datom Key Encoding

The prolly tree storage layer is generic over `(Key, Value)` pairs, but ferratomic stores
datoms — five-tuples `(entity, attribute, value, tx, op)` with multiple sort orderings
required for query routing (INV-FERR-005). This section defines how the four datom indexes
plus the canonical primary set are mapped onto five physically distinct prolly trees, what
each tree's key and value bytes encode, how a datom can be recovered from a tree entry,
and how the five tree roots compose into a single snapshot identity.

**Traces to**: INV-FERR-005 (Index Bijection), INV-FERR-012 (Content-Addressed Entities),
INV-FERR-049 (Snapshot = Root Hash), INV-FERR-086 (Canonical Datom Format Determinism),
C2 (Identity by Content), C8 (Substrate Independence)

**Why this section exists**: A naive reading of the prolly tree spec would conclude that
"key" and "value" are application-defined byte sequences that the tree treats opaquely.
That is true at the prolly tree layer, but the *application* (ferratomic-store) is not free
to choose arbitrary encodings without losing properties that downstream invariants require.
Round-trip recoverability (INV-FERR-049), index bijection (INV-FERR-005), and federation
transfer minimality (INV-FERR-048) all depend on the encoding being canonical, deterministic,
and reversible — properties that must be specified explicitly so independent implementations
agree on the bytes.

#### S23.9.0.1: Five-Tree Architecture

The store maintains **five physically distinct prolly trees**, one per index ordering from
INV-FERR-005:

| Tree | Sort key (lexicographic on bytes) | Purpose |
|------|------------------------------------|---------|
| `primary` | canonical datom bytes | Canonical content-addressed datom set; the source of truth for INV-FERR-012 entity identity |
| `eavt` | `(entity ‖ attribute ‖ value ‖ tx ‖ op)` | Entity-first scan: all datoms for one entity, in attribute order |
| `aevt` | `(attribute ‖ entity ‖ value ‖ tx ‖ op)` | Attribute-first scan: all datoms for one attribute |
| `vaet` | `(value ‖ attribute ‖ entity ‖ tx ‖ op)` | Reverse-reference scan: all datoms whose value is `Ref(target)` |
| `avet` | `(attribute ‖ value ‖ entity ‖ tx ‖ op)` | Attribute-value lookup: unique-attribute and equality-predicate queries |

Each tree's leaves contain `(key_bytes, value_bytes)` entries sorted lexicographically by
`key_bytes`. The five trees are content-addressed independently: each has its own root hash
and its own chunk set. Identical chunks across trees ARE physically deduplicated (by content
addressing, INV-FERR-045) but the tree roots are distinct because the keys are distinct.

**Why five and not four**: INV-FERR-005 names four secondary indexes (EAVT, AEVT, VAET,
AVET) and identifies EAVT as primary. The prolly tree storage layer separates the two
roles of EAVT — query routing (sorted-by-EAVT iteration) and content addressing (canonical
serialized identity) — into two physically distinct trees. The `primary` tree is keyed by
the canonical datom byte encoding from INV-FERR-086 (`canonical_bytes(d)`), which happens
to coincide with EAVT order for naturally-encoded datoms but is conceptually distinct: the
`primary` tree is the **content-addressed source of truth**, while the `eavt` tree is an
**index optimized for entity-prefix scans**. This separation allows future evolution where
the primary tree's encoding (e.g., attribute-interned form per INV-FERR-085) diverges from
the `eavt` tree's display-friendly form without losing index bijection.

#### S23.9.0.2: Key and Value Encodings

Every tree uses the same canonical building blocks defined in INV-FERR-086:

```
canonical_bytes(d)  : Datom → [u8; variable]   -- INV-FERR-086 §Level 2
content_hash(d)     : Datom → Hash             := BLAKE3(canonical_bytes(d))
```

The primary tree and the four secondary trees use identical encodings for individual
field components. The only difference is the *order* in which the components are
concatenated into the key.

**Primary tree entry**:

```
key   = canonical_bytes(d)              -- INV-FERR-086 layout, variable length
value = content_hash(d).as_bytes()      -- 32 bytes, fixed
```

**Secondary tree entries** (one of EAVT, AEVT, VAET, AVET):

```
key   = sort_prefix(d, ordering)        -- variable length, see below
value = content_hash(d).as_bytes()      -- 32 bytes, fixed
```

`sort_prefix(d, ordering)` writes the datom field components in the order required by the
index, using the same per-component encoding as `canonical_bytes` but with the field order
permuted. Because each component is self-delimiting (length-prefixed for variable-width
fields, fixed-width for entity / tx / op), the resulting byte string remains parseable
without an external schema and remains lexicographically ordered by the leading components.

**Why the value is `content_hash(d)`**: The 32-byte BLAKE3 of the canonical bytes is a
*structural cross-reference*, not a redundant copy. The value lets a query that hit a
secondary index resolve directly to the canonical datom identity in O(1) without rehashing
the key, and lets garbage collection (S23.9.2) walk the value-graph independently of the
key-graph. The fixed 32-byte width also stabilizes the leaf-chunk size distribution: leaf
chunks are bounded by `entry_count × (key_len + 32)` rather than by the variable-width
value sizes that would otherwise dominate.

#### S23.9.0.3: Round-Trip Semantics

> **Datom reconstruction comes from the KEY, not the value.**

Every tree's key contains the complete datom in serialized form. Decoding `key_bytes`
through the inverse of `sort_prefix` (or `canonical_bytes` for the primary tree) yields
the original five-tuple `(e, a, v, tx, op)`:

```
∀ d : Datom, ∀ tree ∈ {primary, eavt, aevt, vaet, avet}:
  let entry = tree.entry_for(d)
  decode_key(entry.key, tree.ordering) = d
```

The value field — `content_hash(d)` — is **not** a reconstruction source. It cannot be
inverted (BLAKE3 is one-way), and it would be insufficient even if it could be inverted
because a hash is a fixed-size summary of variable-size content. Implementations MUST NOT
attempt to reconstruct datoms by deserializing the value field. Implementations that need
to recover a datom from a tree entry MUST decode the key.

**Implementation contract**:

```rust
/// Decode a primary-tree key back into a Datom.
/// INV-FERR-086 + S23.9.0: key encoding is canonical and reversible.
pub fn decode_primary_key(key: &[u8]) -> Result<Datom, FerraError> {
    Datom::from_canonical_bytes(key)
}

/// Decode a secondary-tree key back into a Datom.
/// The ordering parameter selects which permutation to invert.
pub fn decode_index_key(key: &[u8], ordering: IndexOrdering) -> Result<Datom, FerraError> {
    Datom::from_sort_prefix(key, ordering)
}

/// The value field is for cross-reference only — never call this with intent to
/// reconstruct the datom. The signature deliberately returns Hash, not Datom.
pub fn decode_entry_value(value: &[u8]) -> Result<Hash, FerraError> {
    if value.len() != 32 {
        return Err(FerraError::InvalidValueLength { expected: 32, actual: value.len() });
    }
    Ok(Hash::from_bytes(value.try_into().expect("length checked above")))
}
```

A round-trip property follows immediately and is verified at the implementation level by
INV-FERR-049's snapshot proptest: building a prolly tree from `kvs`, extracting the root
hash, then resolving and decoding produces the original `kvs`.

#### S23.9.0.4: RootSet — Multi-Tree Snapshot Manifest

A complete store snapshot is identified by **five** root hashes (one per tree), composed
into a single fixed-size manifest:

```rust
/// Five tree roots that together identify a complete store snapshot.
/// Field order is FIXED for canonical serialization (S23.9.0.5).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RootSet {
    /// Primary tree root: canonical content-addressed datom set.
    pub primary: Hash,
    /// EAVT secondary index root.
    pub eavt: Hash,
    /// AEVT secondary index root.
    pub aevt: Hash,
    /// VAET secondary index root (reverse references).
    pub vaet: Hash,
    /// AVET secondary index root (attribute-value lookup).
    pub avet: Hash,
}
```

The `RootSet` is the bridge between the multi-tree physical layout and the
single-root-hash abstraction used by INV-FERR-049 (`Snapshot::root`). Snapshot comparison,
diff, and transfer all begin by comparing or operating on `RootSet`s, then descend into
individual tree pairs.

#### S23.9.0.5: RootSet Canonical Serialization

```
serialize(RootSet) : RootSet → [u8; 160]

Layout (fixed 160 bytes, no padding, no length prefixes):
  bytes 0..32    primary  : [u8; 32]
  bytes 32..64   eavt     : [u8; 32]
  bytes 64..96   aevt     : [u8; 32]
  bytes 96..128  vaet     : [u8; 32]
  bytes 128..160 avet     : [u8; 32]
```

```rust
/// Canonical RootSet serialization (S23.9.0.5).
/// Fixed 160 bytes. Field order: primary, eavt, aevt, vaet, avet.
impl RootSet {
    pub fn canonical_bytes(&self) -> [u8; 160] {
        let mut buf = [0u8; 160];
        buf[0..32].copy_from_slice(self.primary.as_bytes());
        buf[32..64].copy_from_slice(self.eavt.as_bytes());
        buf[64..96].copy_from_slice(self.aevt.as_bytes());
        buf[96..128].copy_from_slice(self.vaet.as_bytes());
        buf[128..160].copy_from_slice(self.avet.as_bytes());
        buf
    }

    pub fn from_canonical_bytes(buf: &[u8; 160]) -> Self {
        RootSet {
            primary: Hash::from_bytes(buf[0..32].try_into().expect("32 bytes")),
            eavt:    Hash::from_bytes(buf[32..64].try_into().expect("32 bytes")),
            aevt:    Hash::from_bytes(buf[64..96].try_into().expect("32 bytes")),
            vaet:    Hash::from_bytes(buf[96..128].try_into().expect("32 bytes")),
            avet:    Hash::from_bytes(buf[128..160].try_into().expect("32 bytes")),
        }
    }
}
```

#### S23.9.0.6: Snapshot Hash = BLAKE3(RootSet)

The `Snapshot::root` field of INV-FERR-049 is defined as:

```
snapshot_hash : RootSet → Hash
snapshot_hash(rs) = BLAKE3(serialize(rs))
```

This is the **manifest hash** that uniquely identifies a five-tree snapshot. It is what
gets stored in the journal `RootUpdate` records (S23.9.2), what gets compared for snapshot
identity, and what gets passed to `Snapshot::resolve`. Two stores with the same five tree
roots produce the same manifest hash; two stores that differ in any tree root produce
different manifest hashes.

**Resolve protocol** (used by `Snapshot::resolve` per INV-FERR-049 Level 2):

```
fn resolve(snapshot_root: Hash, store: &dyn ChunkStore) -> Result<RootSet, FerraError> {
    // 1. Load the manifest chunk addressed by snapshot_root.
    let manifest_chunk = store.get_chunk(&snapshot_root)?
        .ok_or(FerraError::ChunkNotFound(snapshot_root))?;

    // 2. The manifest chunk's content IS the canonical RootSet bytes (160 bytes).
    let buf: &[u8; 160] = manifest_chunk.data().try_into()
        .map_err(|_| FerraError::InvalidManifestSize)?;

    // 3. Deserialize the RootSet — five tree roots ready for tree-level access.
    Ok(RootSet::from_canonical_bytes(buf))
}
```

After resolution, callers descend into individual tree roots: `read_prolly_tree(rs.primary,
store)`, `diff(rs1.eavt, rs2.eavt, store)`, etc. The two-step indirection (manifest hash →
RootSet → tree roots) is the structural core of the multi-index store and the reason why
INV-FERR-049 can claim O(1) snapshot identity for a store that physically contains five
independent trees.

**Diff fast path**: `diff(rs1, rs2)` short-circuits at the manifest level: if `rs1 = rs2`
(equivalently, `snapshot_hash(rs1) = snapshot_hash(rs2)`), the trees are identical and no
further work is needed. Otherwise, the diff descends into the four (or five) tree pairs
where the roots differ. In the common case where only one index changed (e.g., a
new datom was inserted, affecting all five trees), this still bounds work to O(d × log_k n)
per affected tree.

#### S23.9.0.7: Encoding Stability and Versioning

The encodings defined in this section (canonical_bytes, sort_prefix permutations, RootSet
layout) are **format version 1**. Any change to the encoding — adding a field, changing
the field order, adopting a new value tag — is a breaking change that produces different
chunk addresses, different tree roots, and different snapshot hashes for the same logical
datom set. Version transitions are governed by the same migration discipline as
INV-FERR-086 and require an explicit ADR.

The `Chunk` discriminator byte specified in INV-FERR-045a includes a `format_version` field
that allows future encodings to coexist. Until that field is bumped, all implementations
MUST use the V1 encoding defined here.

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
/-- Chunk content addressing: address is BLAKE3 of content, and BLAKE3 is
    injective on practical inputs (collision-resistance assumption from
    00-preamble.md §23.0.4 axiomatizes blake3_injective). -/

def chunk_addr (data : List UInt8) : Hash := blake3 data

/-- Forward direction: same content → same address. This is `congrArg` applied
    to `blake3`, but we state it explicitly so downstream theorems can reference
    it by name. -/
theorem chunk_addr_deterministic (d1 d2 : List UInt8) (h : d1 = d2) :
    chunk_addr d1 = chunk_addr d2 :=
  congrArg blake3 h

/-- Substantive direction: same address → same content (with the BLAKE3
    collision-resistance assumption). This is the property that deduplication
    relies on: if two `put_chunk` calls report the same address, they were
    storing the same bytes. -/
theorem chunk_addr_content_recovery (d1 d2 : List UInt8)
    (h : chunk_addr d1 = chunk_addr d2) :
    d1 = d2 :=
  blake3_injective h

/-- Deduplication is structural: storing the same content twice is
    observationally equivalent to storing it once. The proof uses the
    canonical lattice law `s ∪ {x} ∪ {x} = s ∪ {x}` (Finset.union_idem). -/
theorem chunk_store_idempotent (s : Finset (Hash × List UInt8))
    (data : List UInt8) :
    let entry := (chunk_addr data, data)
    s ∪ {entry} ∪ {entry} = s ∪ {entry} := by
  intro entry
  rw [Finset.union_assoc]
  simp [Finset.union_idem]

/-- Two-chunk store consistency: if a store contains a chunk under address `a`,
    then any other put with the same content is a no-op. This is the operational
    consequence of address-content equivalence. -/
theorem chunk_store_dedup
    (s : Finset (Hash × List UInt8)) (data : List UInt8)
    (h_present : (chunk_addr data, data) ∈ s) :
    s ∪ {(chunk_addr data, data)} = s :=
  Finset.union_eq_left.mpr (Finset.singleton_subset_iff.mpr h_present)
```

---

### INV-FERR-045c: Leaf Chunk Codec Conformance

**Traces to**: INV-FERR-005 (Index Bijection), INV-FERR-025 (Index Backend
Interchangeability), INV-FERR-045 (Chunk Content Addressing), INV-FERR-046
(Prolly Tree History Independence), INV-FERR-074 (Homomorphic Store Fingerprint),
INV-FERR-086 (Canonical Datom Format Determinism), ADR-FERR-032 (Lean-Verified
Functor Composition for Representation Changes)
**Referenced by**: INV-FERR-045a (DatomPair reference codec implementation),
INV-FERR-047 (DiffIterator iterates leaf chunks via the codec interface),
INV-FERR-048 (ChunkTransfer materializes leaf chunks via the codec interface),
INV-FERR-049 (Snapshot resolution decodes leaf chunks via the codec interface),
`bd-gvil` epic (future WaveletMatrixCodec — spec authoring tracked under
sub-bead `bd-obo8`, implementation under `bd-o6io`)
**Verification**: `V:PROP`, `V:KANI`, `V:TYPE`, `V:LEAN`
**Stage**: 1

> INV-FERR-045 establishes that the chunk address is BLAKE3 of the chunk content;
> INV-FERR-045a establishes a specific V1 byte format for that content.
> INV-FERR-045c generalizes one step further: the on-disk leaf format is *one* of
> an open-ended family of conforming codecs, all of which preserve the algebraic
> properties on which the prolly tree depends. The trait `LeafChunkCodec` is the
> dock; INV-FERR-045a (DatomPair) is the reference implementation; a future
> WaveletMatrixCodec (`bd-gvil` Phase 4b epic) and a future Verkle/KZG
> commitment-based codec (currently exploratory in `docs/ideas/014`, not yet
> a filed bead) are example variants that plug in via spec evolution. Without
> INV-FERR-045c, every future codec is a one-off bolt-on. With it, every future
> codec is a one-line enum variant addition that compounds with all previous
> work — the load-bearing accretive lever (GOALS.md §7.2) for every alien
> artifact in `docs/ideas/014`. The five conformance theorems are non-negotiable:
> any codec that fails any one of them breaks history independence
> (INV-FERR-046), content addressing (INV-FERR-045), or homomorphic
> fingerprinting (INV-FERR-074).

#### Level 0 (Algebraic Law)
```
Let LeafChunkCodec be the abstract type of leaf chunk encoders, equipped with:

  encode       : Set(Datom)            → Bytes
  decode       : Bytes                 → Result<Set(Datom), FerraError>
  CODEC_TAG    : Byte                  -- registered in §23.9.8
  boundary_key : Bytes                 → Result<DatomKey, FerraError>   -- default

The trait deliberately omits a fingerprint method: chunk fingerprints are
computed by the framework at the datom level via INV-FERR-074 (XOR homomorphism)
and INV-FERR-086 (canonical_bytes), independent of the codec.

A codec C is CONFORMING iff it satisfies all five theorems below.

Theorem T1 (Round-trip):
  ∀ codec C : LeafChunkCodec, ∀ D : Set(Datom):
    C.decode(C.encode(D)) = Ok(D)

  Proof: T1 has no abstract proof at the trait level — it is the central
  per-codec discharge obligation. Each registered codec discharges T1 in its
  own spec entry by exhibiting `decode` as the structural inverse of `encode`
  against the codec's exact byte layout (e.g., INV-FERR-045a's Lean section
  discharges T1 for the DatomPair codec's V1 byte format). The trait-level
  statement of T1 is the universal quantification over those per-codec
  discharges; the trait's role is to enforce a uniform discharge interface so
  the conformance test harness in Level 2 can drive T1 mechanically for every
  registered codec via the per-codec proptest expansion of
  `codec_conformance_tests!`.

Theorem T2 (Determinism):
  ∀ codec C : LeafChunkCodec, ∀ D : Set(Datom):
    let bytes₁ = C.encode(D)
    let bytes₂ = C.encode(D)
    bytes₁ = bytes₂                              -- intra-process determinism

  AND

  ∀ implementations I₁, I₂ conforming to C, ∀ D : Set(Datom):
    encode_I₁(D) = encode_I₂(D)                  -- cross-implementation determinism

  Proof: encode is required to be a pure function — no hidden mutable state,
  no time-of-day intrinsics, no random number generation, no platform-dependent
  SIMD reduction trees that produce architecture-specific results. Cross-
  implementation determinism follows from each codec's spec entry specifying
  the exact byte layout precisely (e.g., INV-FERR-045a §Level 2 specifies the
  DatomPair V1 format down to the byte; the future WaveletMatrixCodec authored
  under `bd-obo8` will specify its layout the same way). Two conforming
  implementations therefore emit byte-identical output for byte-identical
  input by construction.

Theorem T3 (Injectivity):
  ∀ codec C : LeafChunkCodec, ∀ D₁ D₂ : Set(Datom):
    D₁ ≠ D₂  →  C.encode(D₁) ≠ C.encode(D₂)

  Proof: Injectivity is the contrapositive of: encode is a function on
  Set(Datom) AND decode is a function on Bytes that recovers the original (T1).
  Suppose for contradiction D₁ ≠ D₂ but C.encode(D₁) = C.encode(D₂). Then by
  T1:
    C.decode(C.encode(D₁)) = Ok(D₁)
    C.decode(C.encode(D₂)) = Ok(D₂)
  But the two encode results are equal, so decode receives the same input and
  must produce the same output (decode is a function), so Ok(D₁) = Ok(D₂),
  hence D₁ = D₂, contradicting D₁ ≠ D₂. Therefore encode is injective. ∎

  Note: T3 is structurally a CONSEQUENCE of T1, but it is listed explicitly
  in the conformance suite as defense in depth and as a faster-failing oracle
  during fuzzing — an injectivity counterexample is often easier to find than
  a round-trip counterexample for the same defect.

Theorem T4 (Fingerprint Homomorphism Compatibility):
  ∀ codec C : LeafChunkCodec, ∀ D : Set(Datom):
    chunk_fingerprint(D) = ⊕_{d ∈ D} BLAKE3(canonical_bytes(d))   -- INV-FERR-086

  AND

  ∀ codecs C₁ C₂ : LeafChunkCodec, ∀ D : Set(Datom):
    chunk_fingerprint_via(C₁, D) = chunk_fingerprint_via(C₂, D)   -- codec invariance

  where chunk_fingerprint_via(C, D) = chunk_fingerprint(C.decode(C.encode(D))).

  Consequence (the load-bearing one):
    The store fingerprint H : DatomStore → G defined by INV-FERR-074 satisfies
      H(merge(A, B)) = H(A) ⊕ H(B)        (for disjoint A, B)
    for ANY mix of codecs across the chunks of A and B. Replacing one chunk's
    codec with another does not change the store fingerprint, because the
    fingerprint is computed at the datom level (INV-FERR-074) and depends only
    on the LOGICAL DATOM SET, not on the codec's chosen byte representation.
    This is what makes session 023.5's "mixed-codec stores" property
    operationally well-defined.

  Proof: The trait deliberately excludes a `fingerprint` method that could
  depend on encoded bytes. Chunk fingerprints are computed by the framework
  (not the codec) as the XOR-sum of per-datom canonical-byte hashes per
  INV-FERR-086. By T1, the codec's encode/decode pair preserves the datom
  set exactly, so the framework can recover D from the encoded bytes and
  compute the canonical fingerprint independently of the codec. Mixed-codec
  stores compose because XOR is commutative and associative on the chunk
  multiset, regardless of which codec produced each chunk's on-disk bytes.
  Codec choice is a pure storage-layer decision; it never crosses into the
  fingerprint algebra.

Theorem T5 (Order Independence):
  ∀ codec C : LeafChunkCodec, ∀ list₁ list₂ : List(Datom):
    set(list₁) = set(list₂)  →  C.encode(set(list₁)) = C.encode(set(list₂))

  Equivalently: encode is a function on Set(Datom), not on List(Datom). Any
  codec that depends on insertion order, internal hash bucket layout,
  allocator state, or any other property not derivable from the set itself
  is non-conforming.

  Proof: encode's signature accepts Set(Datom). By the definition of a
  function, equal inputs produce equal outputs. Set equality is structural
  (two Set(Datom) values are equal iff they contain the same elements), not
  derivational (it does not matter how the set was constructed). Therefore
  C.encode(s) is uniquely determined by s as a set. Implementations using
  `BTreeSet<Datom>` iterate in canonical (sorted) order, making the
  order-independence operationally automatic; codecs using internal
  structures with non-deterministic iteration must canonicalize before
  encoding. The proptest strategy below verifies this by constructing the
  same logical set via two different insertion orders and asserting encode
  equality.
```

#### Level 1 (State Invariant)

For all leaf chunks reachable from any prolly tree root, the on-disk byte
representation is produced by SOME conforming codec C, and the chunk's logical
datom payload is recoverable via `C::decode`. The codec used for a particular
chunk is identified by its `CODEC_TAG` byte (the first byte on disk per
§23.9.8); the framework dispatches to the appropriate codec via pattern match
on the `LeafChunk` enum variant.

The trait `LeafChunkCodec` is **closed-world**: adding a new codec requires
spec evolution — a new INV-FERR-NNN authoring the codec's exact byte layout, a
`CODEC_TAG` reservation in §23.9.8, and discharge of all five conformance
theorems via the trait test harness. It is NOT a third-party plugin interface.
Ferratomic does not aim to be a codec marketplace, and the algebraic guarantees
depend on every codec being verified end-to-end before admission to the
`LeafChunk` enum.

**Why enum dispatch, not trait objects** (per ADR-FERR-032 and the precedent
set by the Phase 4a `AdaptiveIndexes` enum from INV-FERR-025): pattern-match
dispatch on the `LeafChunk` enum is monomorphized inside each variant,
producing zero vtable overhead and eliminating the type-erasure barrier that
would otherwise prevent Lean's static verification of per-codec properties.
**Mixed-codec stores are supported by construction** — each leaf chunk in the
prolly tree may use any registered codec independently, enabling gradual
migration (e.g., compacting old `DatomPair` chunks into `Wavelet` chunks) and
A/B benchmarking on the same data without forking the store. A trait object
alternative (`Box<dyn LeafChunkCodec>`) was rejected because (a) the vtable
indirection costs ~1-2 ns per call in the hot path, (b) `dyn` is incompatible
with `const CODEC_TAG`, (c) conformance testing cannot enumerate an open world
of codec implementations, and (d) ferratomic is not a plugin platform — codec
choice is a spec-level decision, not a runtime configuration. Static generic
dispatch (`Store<C: LeafChunkCodec>`) was also rejected: it would lock each
store to one codec at compile time, preventing mixed-codec stores entirely.

**Codec discriminator registry**: see §23.9.8 (Codec Discriminator Registry).
Spec-registered codecs occupy `CODEC_TAG` values `0x01..=0x7F`; experimental
codecs use `0x80..=0xFF`. The DatomPair reference codec (INV-FERR-045a) uses
`CODEC_TAG = 0x01`. Future allocations: wavelet matrix `0x02`, Verkle/KZG
`0x03`, BP+RMM internal chunks `0x04` (per future INV-FERR-045d).

**Failure mode without this invariant**: if the leaf chunk format were
hard-coded to a single representation, every future optimization that touches
leaf encoding (wavelet matrix density, prolly tree compaction, columnar
reorganization, learned indexes per column, post-quantum signature inclusion)
would require a one-off API surface, would break INV-FERR-046 (history
independence) for that codec, and would force a global re-shard of the store
to migrate. With the trait in place, each new codec is a one-line enum variant
addition plus a per-codec invariant authoring, and the conformance test
harness mechanically validates the algebraic properties before admission. This
is the structural reason `docs/ideas/014`'s 30 alien artifacts can be added
incrementally rather than requiring a from-scratch rewrite per technique.

#### Level 2 (Implementation Contract)
```rust
use std::collections::BTreeSet;
use ferratom::{Datom, DatomKey, FerraError, Hash};

// ==========================================================================
// LeafChunkCodec: the trait that every leaf encoding must satisfy
// ==========================================================================

/// A codec for leaf chunk payloads. Conforming codecs satisfy the five
/// conformance theorems of INV-FERR-045c (round-trip, determinism,
/// injectivity, fingerprint homomorphism compatibility, order independence).
///
/// The trait surface is intentionally narrow: `encode`, `decode`, and an
/// optional `boundary_key` fast path. The chunk fingerprint is computed at
/// the framework level via INV-FERR-074 + INV-FERR-086 (XOR of per-datom
/// canonical-byte hashes); the codec deliberately does NOT have a
/// `fingerprint` method that could depend on its encoded bytes. This is the
/// structural reason mixed-codec stores compose without breaking the
/// homomorphism (T4).
pub trait LeafChunkCodec {
    /// Codec discriminator byte (registered in §23.9.8 — Codec Discriminator
    /// Registry). Spec-registered codecs use `0x01..=0x7F`; experimental
    /// codecs use `0x80..=0xFF`. The discriminator is the FIRST byte on
    /// disk; all remaining bytes are the codec's payload.
    const CODEC_TAG: u8;

    /// Encode a finite set of datoms into the codec's canonical byte payload.
    /// The output does NOT include the `CODEC_TAG` byte (the framework
    /// prepends it via `LeafChunk::encode`).
    ///
    /// Must satisfy:
    /// - **T2 (determinism)**: same input → same bytes, intra- and
    ///   cross-implementation
    /// - **T3 (injectivity)**: distinct sets → distinct bytes
    /// - **T5 (order independence)**: output depends only on the set, not on
    ///   how it was constructed
    fn encode(datoms: &BTreeSet<Datom>) -> Vec<u8>;

    /// Decode a payload byte sequence (without the `CODEC_TAG` prefix) back
    /// into the datom set.
    ///
    /// Must satisfy:
    /// - **T1 (round-trip)**: `decode(encode(D)) == Ok(D)` for every
    ///   `D ∈ Set(Datom)`
    ///
    /// Returns `FerraError` on malformed input, on bytes that do not parse
    /// against the codec's grammar, or on bytes that decode to a non-canonical
    /// internal representation. **Defense in depth**: the codec must reject
    /// ALL non-canonical inputs at the deserialization boundary, not only at
    /// the type-level construction barrier.
    fn decode(bytes: &[u8]) -> Result<BTreeSet<Datom>, FerraError>;

    /// Return the smallest datom-key in this chunk (used by internal nodes to
    /// compute separator keys for routing). Default implementation: decode
    /// then take min. Codecs MAY override for efficiency, but the override
    /// MUST return the same `DatomKey` value as the default on every input
    /// (i.e., the override is a fast path, not an alternative semantics).
    ///
    /// Returns `FerraError::EmptyChunk` for empty payloads (empty leaves are
    /// syntactically valid but never appear in well-formed prolly trees).
    fn boundary_key(bytes: &[u8]) -> Result<DatomKey, FerraError> {
        let datoms = Self::decode(bytes)?;
        datoms
            .iter()
            .next()
            .map(|d| d.canonical_key())
            .ok_or(FerraError::EmptyChunk)
    }
}

// ==========================================================================
// LeafChunk: closed-world enum dispatch over registered codecs
// ==========================================================================

/// The closed-world enumeration of leaf chunk encodings. Adding a variant
/// requires:
///
///   1. Authoring a new `INV-FERR-NNN` that defines the codec's exact byte
///      layout, all six verification layers populated.
///   2. Reserving a `CODEC_TAG` value in §23.9.8.
///   3. Discharging all five conformance theorems of INV-FERR-045c via the
///      `codec_conformance_tests!` macro.
///
/// Mixed-codec stores are explicitly supported: each leaf chunk in a prolly
/// tree may use any registered codec independently. Dispatch is via pattern
/// match on the variant — zero vtable overhead, full monomorphization inside
/// each match arm.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LeafChunk {
    /// Reference codec: tagged length-value encoding (the V1 format authored
    /// in INV-FERR-045a, refactored in session 023.5 Phase 2 as the
    /// `DatomPairCodec` implementation of `LeafChunkCodec`).
    /// `CODEC_TAG = 0x01`.
    DatomPair(DatomPairChunk),
    // Future variants reserved by spec evolution. Each variant traces to its
    // authoring invariant and `CODEC_TAG` reservation in §23.9.8:
    //   Wavelet(WaveletMatrixChunk),  -- bd-gvil epic (spec authoring bd-obo8),
    //                                    CODEC_TAG = 0x02
    //   Verkle(VerkleChunk),          -- exploratory in docs/ideas/014 §4.1,
    //                                    not yet a filed bead, CODEC_TAG = 0x03
    //   ... etc.
}

impl LeafChunk {
    /// Encode this chunk to its on-disk byte representation, including the
    /// leading `CODEC_TAG` discriminator. The result is the input to BLAKE3
    /// for the chunk's content address (INV-FERR-045).
    pub fn encode(&self) -> Vec<u8> {
        match self {
            LeafChunk::DatomPair(chunk) => {
                let payload = DatomPairCodec::encode(chunk.datoms());
                let mut bytes = Vec::with_capacity(1 + payload.len());
                bytes.push(DatomPairCodec::CODEC_TAG);
                bytes.extend(payload);
                bytes
            }
        }
    }

    /// Decode an on-disk byte sequence by dispatching on the leading
    /// `CODEC_TAG`. Returns `FerraError::UnknownCodec` if the tag does not
    /// match any registered codec, or the codec's specific error on payload
    /// failure.
    pub fn decode(bytes: &[u8]) -> Result<Self, FerraError> {
        let (tag, payload) = bytes
            .split_first()
            .ok_or(FerraError::TruncatedChunk { needed: 1, got: 0 })?;
        match *tag {
            t if t == DatomPairCodec::CODEC_TAG => {
                let datoms = DatomPairCodec::decode(payload)?;
                Ok(LeafChunk::DatomPair(DatomPairChunk::new(datoms)?))
            }
            tag => Err(FerraError::UnknownCodec { tag }),
        }
    }

    /// Compute the chunk fingerprint per INV-FERR-074 + INV-FERR-086.
    ///
    /// The fingerprint depends ONLY on the logical datom set, NOT on the
    /// codec's encoded bytes. This is what makes mixed-codec stores
    /// composable under the XOR homomorphism (T4). The framework computes
    /// `⊕_{d ∈ datoms} BLAKE3(canonical_bytes(d))`, NEVER `BLAKE3(encoded
    /// bytes)`.
    pub fn fingerprint(&self) -> Hash {
        let datoms = match self {
            LeafChunk::DatomPair(chunk) => chunk.datoms(),
        };
        codec_conformance::framework_fingerprint(datoms)
    }
}

// ==========================================================================
// Conformance test harness — drives all five theorems for any codec
// ==========================================================================

/// Trait conformance test harness. Each `assert_*` function corresponds to
/// one of the five conformance theorems of INV-FERR-045c. The functions are
/// generic over the codec type so they can be invoked from per-codec test
/// modules via the `codec_conformance_tests!` macro.
pub mod codec_conformance {
    use super::*;

    /// **T1**: Round-trip. `decode(encode(D)) == Ok(D)` for every `D`.
    pub fn assert_round_trip<C: LeafChunkCodec>(d: &BTreeSet<Datom>) {
        let bytes = C::encode(d);
        let decoded = C::decode(&bytes)
            .expect("INV-FERR-045c T1: conforming codecs must decode their own output");
        assert_eq!(&decoded, d, "INV-FERR-045c T1: round-trip preserves the datom set");
    }

    /// **T2**: Determinism. `encode(D)` is a pure function.
    pub fn assert_deterministic<C: LeafChunkCodec>(d: &BTreeSet<Datom>) {
        let b1 = C::encode(d);
        let b2 = C::encode(d);
        assert_eq!(b1, b2, "INV-FERR-045c T2: encode must be deterministic");
    }

    /// **T3**: Injectivity. Different sets → different bytes.
    pub fn assert_injective<C: LeafChunkCodec>(
        d1: &BTreeSet<Datom>,
        d2: &BTreeSet<Datom>,
    ) {
        if d1 != d2 {
            assert_ne!(
                C::encode(d1),
                C::encode(d2),
                "INV-FERR-045c T3: distinct datom sets must encode to distinct bytes"
            );
        }
    }

    /// **T4**: Fingerprint homomorphism compatibility. The chunk fingerprint
    /// computed via the framework (per INV-FERR-074 + INV-FERR-086) does not
    /// depend on the codec's encoded bytes — only on the logical datom set
    /// recovered by `decode(encode(d))`.
    pub fn assert_fingerprint_codec_invariant<C: LeafChunkCodec>(d: &BTreeSet<Datom>) {
        let bytes = C::encode(d);
        let recovered = C::decode(&bytes)
            .expect("INV-FERR-045c T4: round-trip is a precondition of fingerprint compat");
        let fp_direct = framework_fingerprint(d);
        let fp_via_codec = framework_fingerprint(&recovered);
        assert_eq!(
            fp_direct, fp_via_codec,
            "INV-FERR-045c T4: fingerprint must depend only on the logical datom set"
        );
    }

    /// **T5**: Order independence. `encode` is a function on `Set(Datom)`,
    /// not on `List(Datom)`. Building the same logical set via different
    /// insertion orders must produce equal bytes.
    pub fn assert_order_independent<C: LeafChunkCodec>(d: &BTreeSet<Datom>) {
        // Build an equivalent set via reversed iteration order, then re-collect
        // into BTreeSet (which canonicalizes to sorted order). The two
        // BTreeSets are structurally equal; encode must agree.
        let v: Vec<Datom> = d.iter().cloned().collect();
        let shuffled: BTreeSet<Datom> = v.into_iter().rev().collect();
        assert_eq!(d, &shuffled, "BTreeSet collect must canonicalize order");
        assert_eq!(
            C::encode(d),
            C::encode(&shuffled),
            "INV-FERR-045c T5: encode must depend only on the set, not on construction order"
        );
    }

    /// Per-chunk canonical fingerprint, computed at the framework level via
    /// INV-FERR-074 (XOR homomorphism) and INV-FERR-086 (`canonical_bytes`).
    /// Codecs do NOT call this — it is the framework's authoritative
    /// definition of chunk fingerprint, deliberately kept outside the
    /// `LeafChunkCodec` trait surface so codecs cannot override it.
    pub fn framework_fingerprint(datoms: &BTreeSet<Datom>) -> Hash {
        let mut acc = [0u8; 32];
        for d in datoms.iter() {
            let h = blake3::hash(&d.canonical_bytes());
            for (a, b) in acc.iter_mut().zip(h.as_bytes().iter()) {
                *a ^= b;
            }
        }
        Hash::from_bytes(acc)
    }
}

/// Generates conformance test modules for a given codec type. Each codec
/// crate invokes this macro once with the codec's identifier; the macro
/// expands to a `proptest!` block that drives all five conformance theorems.
///
/// Per **D3** (decisions, session 023.5): codecs MAY add codec-specific
/// tests on top of the trait-level conformance suite, but the trait-level
/// suite is the MINIMUM required surface for admission to the `LeafChunk`
/// enum. This mirrors the INV-FERR-025 `IndexBackend` test pattern.
#[macro_export]
macro_rules! codec_conformance_tests {
    ($mod_name:ident, $codec:ty) => {
        #[cfg(test)]
        mod $mod_name {
            use super::*;
            use $crate::codec_conformance::*;
            use ferratomic_verify::generators::arb_datom_set;
            use proptest::prelude::*;

            proptest! {
                #[test]
                fn t1_round_trip(d in arb_datom_set(0..256)) {
                    assert_round_trip::<$codec>(&d);
                }

                #[test]
                fn t2_deterministic(d in arb_datom_set(0..256)) {
                    assert_deterministic::<$codec>(&d);
                }

                #[test]
                fn t3_injective(
                    d1 in arb_datom_set(1..128),
                    d2 in arb_datom_set(1..128),
                ) {
                    assert_injective::<$codec>(&d1, &d2);
                }

                #[test]
                fn t4_fingerprint_codec_invariant(d in arb_datom_set(0..256)) {
                    assert_fingerprint_codec_invariant::<$codec>(&d);
                }

                #[test]
                fn t5_order_independent(d in arb_datom_set(0..256)) {
                    assert_order_independent::<$codec>(&d);
                }
            }
        }
    };
}

// ==========================================================================
// Kani harness — bounded conformance for the reference codec
// ==========================================================================

/// Bounded conformance check: drive T1, T2, T3 against the DatomPair
/// reference codec on a 2-datom input. Per-codec Kani harnesses live in
/// each codec's spec entry; this top-level harness exists to assert that
/// AT LEAST ONE registered codec satisfies the trait under bounded model
/// checking.
#[kani::proof]
#[kani::unwind(4)]
fn datom_pair_codec_conformance_bounded() {
    let d1: Datom = kani::any();
    let d2: Datom = kani::any();
    kani::assume(d1 != d2);

    let mut set = BTreeSet::new();
    set.insert(d1.clone());
    set.insert(d2.clone());

    // T1: round-trip
    let bytes = DatomPairCodec::encode(&set);
    let decoded = DatomPairCodec::decode(&bytes)
        .expect("INV-FERR-045c T1: round-trip");
    assert_eq!(decoded, set);

    // T2: determinism (intra-process)
    let bytes2 = DatomPairCodec::encode(&set);
    assert_eq!(bytes, bytes2);

    // T3: injectivity (with a one-element set as the contrast witness)
    let mut singleton = BTreeSet::new();
    singleton.insert(d1.clone());
    assert_ne!(
        DatomPairCodec::encode(&set),
        DatomPairCodec::encode(&singleton)
    );
}
```

**Falsification**: Any one of the following witnesses falsifies INV-FERR-045c.
A conforming codec must rule out all five.

1. **T1 (round-trip) witness**: A codec `C` and datom set `D ∈ Set(Datom)`
   such that `C::decode(C::encode(D))` returns `Err(_)` or `Ok(D')` with
   `D' != D`. This indicates either (a) `encode` loses information (e.g.,
   truncating large values, dropping the `Op` field), or (b) `decode` is
   the wrong inverse (e.g., a length field is mis-parsed). Either makes the
   codec unusable for the prolly tree because INV-FERR-045 (content
   addressing) and INV-FERR-046 (history independence) both depend on
   recovering the exact logical content from the on-disk bytes.

2. **T2 (determinism) witness**: A codec `C` and datom set `D` such that two
   sequential invocations of `C::encode(&D)` produce different byte
   sequences. Sources include: hidden mutable state (a global counter, a
   per-instance sequence number), time-of-day intrinsics, ASLR-dependent
   ordering of an internal `HashMap`, non-deterministic SIMD reduction
   trees that produce architecture-dependent results, or thread-local
   random number generators used as a "compression seed." A second
   falsification class: two implementations conforming to the same spec
   entry that produce different bytes for the same input. This breaks
   cross-implementation chunk addresses and turns federated stores into
   per-implementation isolates.

3. **T3 (injectivity) witness**: A codec `C` and distinct datom sets
   `D₁ != D₂` such that `C::encode(&D₁) == C::encode(&D₂)`. Sources
   include: lossy compressors that map multiple inputs to the same
   compressed form, or internal hash-based representations whose collision
   rate exceeds zero on real datom inputs. Note that injectivity is a
   CONSEQUENCE of round-trip (T1) plus the pigeonhole principle on
   functions, so any T3 falsification is also a T1 falsification — the
   test is included separately as defense in depth and as a faster-failing
   oracle during fuzzing.

4. **T4 (fingerprint homomorphism compatibility) witness**: A codec `C`
   and datom set `D` such that, after slotting `C` into `LeafChunk`, the
   framework fingerprint computed via `LeafChunk::fingerprint()` (which
   goes through INV-FERR-074 + INV-FERR-086) does NOT equal the canonical
   fingerprint computed directly from `D`. Since the trait deliberately
   omits a `fingerprint` method, this witness type is structurally
   impossible — but the test harness asserts it explicitly to catch
   type-system violations (e.g., a refactor that introduces a codec-level
   fingerprint override). The deeper falsification class: a multi-codec
   store where replacing one chunk's codec changes the store fingerprint.
   This would indicate either T1 (round-trip) is broken or that the
   framework is calling a per-codec fingerprint instead of the canonical
   one.

5. **T5 (order independence) witness**: A codec `C` and two
   `BTreeSet<Datom>` instances `s1` and `s2` such that `s1 == s2`
   (structurally equal as sets) but `C::encode(&s1) != C::encode(&s2)`.
   Since `BTreeSet` iteration is canonical (sorted), this can only happen
   if the codec internally reconstructs the input via a non-deterministic
   structure (e.g., a `HashSet` with random hash seed, an
   allocator-dependent linked list). The witness is constructed by
   building two `BTreeSet`s from the same source `Vec<Datom>` via different
   insertion sequences (forward and reverse), then asserting `encode`
   produces equal bytes.

**proptest strategy**:
```rust
// All five properties are exercised by the conformance test harness macro
// defined in Level 2. Per-codec tests are generated via:
//
//     codec_conformance_tests!(datom_pair, DatomPairCodec);
//
// which expands to a `proptest!` block with one #[test] per conformance
// theorem. The standalone strategies below illustrate each property in
// isolation against the DatomPair reference codec; the macro is the
// production test mechanism for ALL registered codecs.

proptest! {
    /// T1: Round-trip — driven against the DatomPair reference codec.
    #[test]
    fn datom_pair_round_trip(
        datoms in arb_datom_set(0..256),
    ) {
        let bytes = DatomPairCodec::encode(&datoms);
        let decoded = DatomPairCodec::decode(&bytes)
            .expect("INV-FERR-045c T1: decode must succeed on valid input");
        prop_assert_eq!(decoded, datoms);
    }

    /// T2: Determinism — same input → same output bytes.
    #[test]
    fn datom_pair_deterministic(
        datoms in arb_datom_set(0..256),
    ) {
        let b1 = DatomPairCodec::encode(&datoms);
        let b2 = DatomPairCodec::encode(&datoms);
        prop_assert_eq!(b1, b2,
            "INV-FERR-045c T2: encode must be a pure function");
    }

    /// T3: Injectivity — distinct sets must encode to distinct bytes.
    #[test]
    fn datom_pair_injective(
        d1 in arb_datom_set(1..128),
        d2 in arb_datom_set(1..128),
    ) {
        prop_assume!(d1 != d2);
        prop_assert_ne!(
            DatomPairCodec::encode(&d1),
            DatomPairCodec::encode(&d2),
            "INV-FERR-045c T3: distinct sets must encode differently"
        );
    }

    /// T4: Fingerprint homomorphism compatibility — the chunk fingerprint
    /// computed by the framework depends only on the logical datom set,
    /// not on the codec's encoded bytes.
    #[test]
    fn datom_pair_fingerprint_codec_invariant(
        datoms in arb_datom_set(0..256),
    ) {
        let bytes = DatomPairCodec::encode(&datoms);
        let recovered = DatomPairCodec::decode(&bytes).unwrap();
        let fp_direct = codec_conformance::framework_fingerprint(&datoms);
        let fp_recovered = codec_conformance::framework_fingerprint(&recovered);
        prop_assert_eq!(fp_direct, fp_recovered,
            "INV-FERR-045c T4: fingerprint must depend only on the logical datom set");
    }

    /// T5: Order independence — equal sets via different insertion orders
    /// must encode equally.
    #[test]
    fn datom_pair_order_independent(
        raw in prop::collection::vec(arb_datom(), 0..200),
    ) {
        let s1: BTreeSet<Datom> = raw.iter().cloned().collect();
        let s2: BTreeSet<Datom> = raw.into_iter().rev().collect();
        prop_assert_eq!(&s1, &s2, "BTreeSet collect must canonicalize");
        prop_assert_eq!(
            DatomPairCodec::encode(&s1),
            DatomPairCodec::encode(&s2),
            "INV-FERR-045c T5: encode must not depend on construction order"
        );
    }
}
```

**Lean theorem**:
```lean
/-- INV-FERR-045c: Leaf chunk codec conformance theorems.

    Modeled at the trait level: a codec is a structure (encode, decode,
    boundary_key, codec_tag) where encode : Finset Datom → List UInt8 and
    decode : List UInt8 → Option (Finset Datom). The five conformance
    theorems formalize what it means for a codec to be well-behaved; any
    concrete codec slotted into LeafChunk must discharge them in its own
    Lean file (see INV-FERR-045a's Lean section for the DatomPair reference
    discharge). The byte-level concretization for specific codecs is
    deferred per the same pattern as INV-FERR-086 and INV-FERR-045a;
    tracked via the existing bd-aqg9h (045a Lean concretization) and the
    new follow-up bead filed during session 023.5 Phase 7. -/

structure LeafChunkCodec where
  encode      : Finset Datom → List UInt8
  decode      : List UInt8 → Option (Finset Datom)
  codecTag    : UInt8
  boundaryKey : List UInt8 → Option DatomKey

/-- T1: Round-trip — decode is the structural inverse of encode on every
    finite datom set in the codec's domain. This is a precondition of
    trait conformance; codecs that fail it do not conform. -/
def isRoundTrip (C : LeafChunkCodec) : Prop :=
  ∀ d : Finset Datom, C.decode (C.encode d) = some d

/-- T2: Determinism — encode is a pure function. In Lean's pure-functional
    model this is automatic (Lean has no notion of state or impure effects),
    so the theorem is `rfl`. The cross-implementation determinism clause is
    discharged at the per-codec spec entry level: each codec's spec defines
    the exact byte layout, and any conforming implementation produces those
    exact bytes by construction. -/
theorem encode_deterministic (C : LeafChunkCodec) (d : Finset Datom) :
    C.encode d = C.encode d := rfl

/-- T3: Injectivity — different inputs produce different outputs. -/
def isInjective (C : LeafChunkCodec) : Prop :=
  ∀ d₁ d₂ : Finset Datom, d₁ ≠ d₂ → C.encode d₁ ≠ C.encode d₂

/-- The structural theorem: round-trip implies injectivity. This is the
    proof referenced in the Level 0 algebraic law for T3. -/
theorem roundtrip_implies_injective (C : LeafChunkCodec)
    (h : isRoundTrip C) : isInjective C := by
  intro d₁ d₂ h_neq h_eq
  have r₁ : C.decode (C.encode d₁) = some d₁ := h d₁
  have r₂ : C.decode (C.encode d₂) = some d₂ := h d₂
  rw [h_eq] at r₁
  -- r₁ : C.decode (C.encode d₂) = some d₁
  -- r₂ : C.decode (C.encode d₂) = some d₂
  -- Functional equality of decode forces the somethings to agree.
  have h_some : some d₁ = some d₂ := by rw [← r₁, r₂]
  exact h_neq (Option.some.inj h_some)

/-- T4 part 1: framework fingerprint, computed at the datom level via
    INV-FERR-086's canonical_bytes XORed per INV-FERR-074. The actual
    byte-level XOR is axiomatized here; the per-bit reasoning lives in
    INV-FERR-074's Lean section. -/
axiom canonicalDatomBytes : Datom → List UInt8
axiom blake3Bytes         : List UInt8 → ByteVec 32
axiom xorByteVecs         : ByteVec 32 → ByteVec 32 → ByteVec 32

def frameworkFingerprint (d : Finset Datom) : ByteVec 32 :=
  d.fold
    (fun acc dt => xorByteVecs acc (blake3Bytes (canonicalDatomBytes dt)))
    (ByteVec.zero 32)

/-- T4 part 2: For a round-trip codec, the framework fingerprint computed
    directly from D equals the framework fingerprint computed from ANY
    `d'` returned by `decode(encode(D))`. The universal quantifier over
    `d'` is what captures codec invariance: regardless of what the codec
    chose to represent the chunk as on disk, the recovered datom set
    yields the same fingerprint as the original. This is what makes
    mixed-codec stores compose under INV-FERR-074's XOR homomorphism. -/
theorem fingerprint_codec_invariant
    (C : LeafChunkCodec) (h : isRoundTrip C) (d : Finset Datom) :
    ∀ d' : Finset Datom,
      C.decode (C.encode d) = some d' →
      frameworkFingerprint d = frameworkFingerprint d' := by
  intro d' h_dec
  -- Round-trip gives `C.decode (C.encode d) = some d`.
  -- Combined with `h_dec : C.decode (C.encode d) = some d'`, we get
  -- `some d = some d'`, hence `d = d'` by `Option.some.inj`.
  have r : some d = some d' := by rw [← h_dec]; exact h d
  have h_eq : d = d' := Option.some.inj r
  rw [h_eq]

/-- T5: Order independence — encode is a function on Finset, not on List.
    By Lean's type system, Finset has no notion of order, so encode applied
    to two equal Finsets returns equal results by definitional equality.
    Stated explicitly so the property is documented at the trait level. -/
theorem encode_order_independent (C : LeafChunkCodec)
    (l₁ l₂ : List Datom) (h : l₁.toFinset = l₂.toFinset) :
    C.encode l₁.toFinset = C.encode l₂.toFinset := by
  rw [h]

/-- Conformance bundle: a codec is conforming iff it satisfies T1 (the only
    propositional obligation requiring per-codec proof). T2 is `rfl`, T3
    follows from T1 via `roundtrip_implies_injective`, T4 follows from T1
    via `fingerprint_codec_invariant`, and T5 follows from Lean's
    Finset-equality being structural rather than derivational. -/
def isConforming (C : LeafChunkCodec) : Prop :=
  isRoundTrip C

/-- Conformance implies all five theorem statements. The structure of the
    proof makes the dependence visible: T1 is the only independent
    obligation; everything else is derived. This is why the trait is
    structurally minimal — the codec's only Lean discharge obligation is
    `isRoundTrip`. -/
theorem conforming_implies_all_five
    (C : LeafChunkCodec) (h : isConforming C) :
    isRoundTrip C ∧
    (∀ d, C.encode d = C.encode d) ∧                              -- T2
    isInjective C ∧                                               -- T3
    (∀ d d',                                                      -- T4
        C.decode (C.encode d) = some d' →
        frameworkFingerprint d = frameworkFingerprint d') ∧
    (∀ l₁ l₂ : List Datom, l₁.toFinset = l₂.toFinset →            -- T5
              C.encode l₁.toFinset = C.encode l₂.toFinset) := by
  refine ⟨h, ?_, ?_, ?_, ?_⟩
  · intro d; rfl
  · exact roundtrip_implies_injective C h
  · intro d d' h_dec; exact fingerprint_codec_invariant C h d d' h_dec
  · intro l₁ l₂ h_eq; rw [h_eq]

-- Per-codec discharges: each registered codec proves `isConforming` for
-- its own LeafChunkCodec instance in its dedicated Lean file. INV-FERR-045a
-- (DatomPair) is the reference discharge — see its Lean section for the
-- structural proof against the V1 byte format.
--
-- The byte-level concretization (proving `isRoundTrip` on the actual V1
-- bytes, not just abstract `Finset Datom → List UInt8`) is tracked for
-- each codec under the same Lean concretization beads as INV-FERR-086 and
-- INV-FERR-045a: bd-aqg9h (045a Lean concretization, already filed). A new
-- follow-up bead for the trait-level Lean concretization at the byte
-- boundary will be filed during session 023.5 Phase 7.
```

---

### INV-FERR-045a: Deterministic Chunk Serialization

**Traces to**: INV-FERR-045 (Chunk Content Addressing), INV-FERR-045c (Leaf Chunk
Codec Conformance — INV-FERR-045a is the DatomPair reference codec implementation
of the LeafChunkCodec trait; full refactor pending session 023.5 Phase 2),
INV-FERR-086 (Canonical Datom Format Determinism), S23.9.0 (Canonical Datom Key
Encoding), C2 (Identity by Content)
**Referenced by**: INV-FERR-046 (history independence relies on canonical leaf bytes),
INV-FERR-047 (DiffIterator deserializes chunk contents), INV-FERR-048 (transfer relies
on `decode_child_addrs` which deserializes internal chunks), INV-FERR-049 (snapshot
resolve deserializes the manifest chunk)
**Verification**: `V:PROP`, `V:KANI`, `V:TYPE`, `V:LEAN`
**Stage**: 1

> INV-FERR-045 establishes that *some* canonical byte representation produces the chunk
> address. INV-FERR-045a establishes *which* representation: the V1 format below, with
> validated constructors that prevent non-canonical chunks from existing. Without
> INV-FERR-045a, two implementations could compute different chunk addresses for the
> "same" chunk content and the structural sharing guarantees of INV-FERR-046 (history
> independence) and INV-FERR-022 (anti-entropy convergence) would degrade silently into
> per-implementation isolation.

#### Level 0 (Algebraic Law)
```
Let Chunk be the disjoint union LeafChunk ⊎ InternalChunk.
Let serialize_leaf     : LeafChunk     → Bytes
Let serialize_internal : InternalChunk → Bytes
Let serialize          : Chunk         → Bytes  := match c with
                                                    | Leaf l     → serialize_leaf l
                                                    | Internal i → serialize_internal i
Let deserialize        : Bytes → Result<Chunk, FerraError>

A LeafChunk L is canonical iff its entries are sorted strictly ascending by key bytes
  with no duplicate keys.
An InternalChunk I is canonical iff its children are sorted strictly ascending by
  separator-key bytes with no duplicate separators, and every child_addr is a 32-byte hash.

Let CanonicalLeafChunk     = { L : LeafChunk     | canonical(L) }
Let CanonicalInternalChunk = { I : InternalChunk | canonical(I) }
Let CanonicalChunk         = CanonicalLeafChunk ⊎ CanonicalInternalChunk

Theorem (round-trip):
  ∀ c ∈ CanonicalChunk:
    deserialize(serialize(c)) = Ok(c)

Theorem (canonicality / injectivity):
  ∀ c₁, c₂ ∈ CanonicalChunk:
    serialize(c₁) = serialize(c₂)  ⟺  c₁ = c₂

Theorem (cross-implementation determinism):
  ∀ implementations I₁, I₂ conforming to the V1 format,
  ∀ c ∈ CanonicalChunk:
    serialize_I₁(c) = serialize_I₂(c)

Proof:
  serialize_leaf and serialize_internal are total functions defined by a fixed,
  little-endian, length-prefixed byte layout (Level 2). Given identical canonical
  inputs they emit identical byte sequences by construction. The V1 format has no
  alignment padding, no implementation-defined choices, and no source of nondeterminism.

  Round-trip holds because every byte position in the V1 format encodes a single field
  with a unique tag-or-position, so deserialize is the structural inverse of serialize.
  Since the canonical predicate enforces sorted-strictly-ascending entries with no
  duplicates, the byte order of fields agrees with the byte order in the input, and
  deserialize reconstructs the same field values in the same order.

  Injectivity follows: if two canonical chunks serialize to the same bytes, then by
  round-trip they deserialize to identical chunks (deserialize is a function of the bytes,
  so equal bytes produce equal results), hence c₁ = c₂.

Corollary (content-addressing stability):
  ∀ c₁, c₂ ∈ CanonicalChunk:
    c₁ = c₂  ⟺  BLAKE3(serialize(c₁)) = BLAKE3(serialize(c₂))    (with negligible collision)

  This is the structural reason INV-FERR-045's content addressing is well-defined: the
  address depends only on the canonical chunk content, not on incidental serialization
  choices.
```

#### Level 1 (State Invariant)

For all chunks reachable from any prolly tree root, the on-disk byte representation is
the V1 canonical format:

- Leaf chunks contain a discriminator byte (`0x01`), a format version byte (`0x01`), an
  entry count, and a sequence of `(key_len, key, value_len, value)` records sorted strictly
  ascending by `key`. Two leaves containing the same logical key-value set produce
  byte-identical serializations and therefore byte-identical addresses.

- Internal chunks contain a discriminator byte (`0x02`), a format version byte (`0x01`),
  a tree-level byte, an entry count, and a sequence of `(separator_len, separator,
  child_addr)` records sorted strictly ascending by `separator`. Two internal nodes
  containing the same logical separator/child-address pairs at the same level produce
  byte-identical serializations.

- The canonical predicate is enforced **at construction**: the `LeafChunk` and
  `InternalChunk` types expose only constructors that validate the sorted-strictly-ascending
  invariant and return `FerraError::NonCanonicalChunk` on violation. Non-canonical chunks
  are unrepresentable in well-typed code: there is no public constructor that accepts
  unsorted or duplicate input.

- `serialize_leaf` and `serialize_internal` accept only the validated chunk types and
  therefore cannot fail on ordering grounds. They return `Vec<u8>` (infallible from the
  domain perspective; the only failure mode is OOM, which is a system-level concern).

- `deserialize_chunk` accepts arbitrary bytes and returns `Result<Chunk, FerraError>`.
  It rejects bytes that do not parse against the V1 grammar OR that decode to a chunk
  whose entries are not in canonical order. This double-check is the on-the-wire defense
  against an adversarial peer sending non-canonical bytes whose hash happens to collide
  with a legitimate chunk.

The "two layers of enforcement" — type-level construction barrier plus deserialize-time
validation — exist because chunks can enter the system from two sources: (1) construction
by ferratomic-store from an in-memory `im::OrdMap` (type-level enforcement is sufficient),
or (2) bytes received from a peer over the wire or read from a file written by a different
implementation (deserialize-time validation is required because the type system cannot
constrain bytes that haven't been parsed yet).

#### Level 2 (Implementation Contract)

```rust
// ==========================================================================
// V1 byte layout
// ==========================================================================
//
// Leaf chunk:
//   [0]      discriminator: u8 = LEAF_CHUNK_TAG (0x01)
//   [1]      format_version: u8 = 0x01
//   [2..6]   entry_count: u32-le
//   [6..]    entries[entry_count]:
//              key_len: u32-le
//              key: [u8; key_len]
//              value_len: u32-le
//              value: [u8; value_len]
//
// Internal chunk:
//   [0]      discriminator: u8 = INTERNAL_CHUNK_TAG (0x02)
//   [1]      format_version: u8 = 0x01
//   [2]      level: u8                 -- tree height level (>= 1; leaves are level 0)
//   [3..7]   entry_count: u32-le
//   [7..]    entries[entry_count]:
//              separator_len: u32-le
//              separator: [u8; separator_len]
//              child_addr: [u8; 32]    -- BLAKE3 hash of child chunk
//
// Cross-cutting:
//   - Multi-byte integers are little-endian (matches INV-FERR-086).
//   - No alignment padding anywhere; bytes are packed.
//   - Empty leaves (entry_count == 0) and empty internal nodes are syntactically valid
//     but never appear in well-formed prolly trees: the build path always splits chunks
//     at boundaries, never produces empty intermediate states. deserialize accepts them
//     for parser simplicity; downstream constructors reject them per canonical_predicate.

pub const LEAF_CHUNK_TAG: u8     = 0x01;
pub const INTERNAL_CHUNK_TAG: u8 = 0x02;
pub const CHUNK_FORMAT_VERSION: u8 = 0x01;

// ==========================================================================
// Validated chunk types
// ==========================================================================

/// A leaf chunk: a sorted, deduplicated sequence of (key, value) pairs.
/// Constructors validate the canonical predicate; non-canonical leaves are
/// unrepresentable in well-typed code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeafChunk {
    /// Entries in strict ascending key order. Field is private — the only way to
    /// populate it is through `LeafChunk::new` or `LeafChunk::from_sorted_unchecked`.
    entries: Vec<(Vec<u8>, Vec<u8>)>,
}

impl LeafChunk {
    /// Build a leaf chunk from arbitrary entries. Validates strict ascending order
    /// and duplicate-freedom; sorts internally if `entries` is unsorted.
    ///
    /// Returns `FerraError::NonCanonicalChunk` if duplicate keys are present.
    pub fn new(mut entries: Vec<(Vec<u8>, Vec<u8>)>) -> Result<Self, FerraError> {
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        for window in entries.windows(2) {
            if window[0].0 == window[1].0 {
                return Err(FerraError::NonCanonicalChunk {
                    reason: "duplicate key in leaf chunk",
                });
            }
        }
        Ok(LeafChunk { entries })
    }

    /// Build a leaf chunk from already-sorted, already-deduplicated entries.
    /// The caller asserts the canonical predicate; debug builds assert it.
    /// This is the hot path used by tree construction where the sort step has
    /// already happened upstream.
    pub fn from_sorted_unchecked(entries: Vec<(Vec<u8>, Vec<u8>)>) -> Self {
        debug_assert!(
            entries.windows(2).all(|w| w[0].0 < w[1].0),
            "from_sorted_unchecked called with non-canonical entries"
        );
        LeafChunk { entries }
    }

    pub fn entries(&self) -> &[(Vec<u8>, Vec<u8>)] { &self.entries }
    pub fn len(&self) -> usize { self.entries.len() }
    pub fn is_empty(&self) -> bool { self.entries.is_empty() }
}

/// An internal chunk: a sorted sequence of (separator_key, child_addr) pairs at a
/// specific tree level (>= 1). Constructors validate the canonical predicate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InternalChunk {
    /// Tree level. Leaves are level 0; the root has level == tree height.
    level: u8,
    /// Children in strict ascending separator-key order. Private field.
    children: Vec<(Vec<u8>, Hash)>,
}

impl InternalChunk {
    pub fn new(level: u8, mut children: Vec<(Vec<u8>, Hash)>) -> Result<Self, FerraError> {
        if level == 0 {
            return Err(FerraError::NonCanonicalChunk {
                reason: "internal chunk must have level >= 1",
            });
        }
        children.sort_by(|a, b| a.0.cmp(&b.0));
        for window in children.windows(2) {
            if window[0].0 == window[1].0 {
                return Err(FerraError::NonCanonicalChunk {
                    reason: "duplicate separator key in internal chunk",
                });
            }
        }
        Ok(InternalChunk { level, children })
    }

    pub fn from_sorted_unchecked(level: u8, children: Vec<(Vec<u8>, Hash)>) -> Self {
        debug_assert!(level >= 1);
        debug_assert!(
            children.windows(2).all(|w| w[0].0 < w[1].0),
            "from_sorted_unchecked called with non-canonical children"
        );
        InternalChunk { level, children }
    }

    pub fn level(&self) -> u8 { self.level }
    pub fn children(&self) -> &[(Vec<u8>, Hash)] { &self.children }
}

/// The full Chunk discriminated union. `serialize` accepts this type;
/// `deserialize_chunk` produces this type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProllyChunkBody {
    Leaf(LeafChunk),
    Internal(InternalChunk),
}

// ==========================================================================
// Serialization
// ==========================================================================

pub fn serialize_leaf(chunk: &LeafChunk) -> Vec<u8> {
    // Pre-compute capacity to avoid reallocs.
    let cap = 6 + chunk.entries.iter().map(|(k, v)| 8 + k.len() + v.len()).sum::<usize>();
    let mut buf = Vec::with_capacity(cap);

    buf.push(LEAF_CHUNK_TAG);
    buf.push(CHUNK_FORMAT_VERSION);
    buf.extend_from_slice(&(chunk.entries.len() as u32).to_le_bytes());

    for (k, v) in &chunk.entries {
        buf.extend_from_slice(&(k.len() as u32).to_le_bytes());
        buf.extend_from_slice(k);
        buf.extend_from_slice(&(v.len() as u32).to_le_bytes());
        buf.extend_from_slice(v);
    }
    buf
}

pub fn serialize_internal(chunk: &InternalChunk) -> Vec<u8> {
    let cap = 7 + chunk.children.iter().map(|(s, _)| 4 + s.len() + 32).sum::<usize>();
    let mut buf = Vec::with_capacity(cap);

    buf.push(INTERNAL_CHUNK_TAG);
    buf.push(CHUNK_FORMAT_VERSION);
    buf.push(chunk.level);
    buf.extend_from_slice(&(chunk.children.len() as u32).to_le_bytes());

    for (separator, child_addr) in &chunk.children {
        buf.extend_from_slice(&(separator.len() as u32).to_le_bytes());
        buf.extend_from_slice(separator);
        buf.extend_from_slice(child_addr.as_bytes()); // 32 bytes
    }
    buf
}

pub fn serialize_chunk(body: &ProllyChunkBody) -> Vec<u8> {
    match body {
        ProllyChunkBody::Leaf(l)     => serialize_leaf(l),
        ProllyChunkBody::Internal(i) => serialize_internal(i),
    }
}

// ==========================================================================
// Deserialization
// ==========================================================================

pub fn deserialize_chunk(bytes: &[u8]) -> Result<ProllyChunkBody, FerraError> {
    if bytes.is_empty() {
        return Err(FerraError::TruncatedChunk { needed: 1, got: 0 });
    }
    match bytes[0] {
        LEAF_CHUNK_TAG     => deserialize_leaf(bytes).map(ProllyChunkBody::Leaf),
        INTERNAL_CHUNK_TAG => deserialize_internal(bytes).map(ProllyChunkBody::Internal),
        tag => Err(FerraError::UnknownChunkTag { tag }),
    }
}

fn deserialize_leaf(bytes: &[u8]) -> Result<LeafChunk, FerraError> {
    let mut cur = Cursor::new(bytes);
    let tag = cur.read_u8()?;
    debug_assert_eq!(tag, LEAF_CHUNK_TAG);
    let version = cur.read_u8()?;
    if version != CHUNK_FORMAT_VERSION {
        return Err(FerraError::UnsupportedChunkVersion { version });
    }
    let entry_count = cur.read_u32_le()? as usize;
    let mut entries = Vec::with_capacity(entry_count);
    for _ in 0..entry_count {
        let key_len = cur.read_u32_le()? as usize;
        let key = cur.read_bytes(key_len)?.to_vec();
        let value_len = cur.read_u32_le()? as usize;
        let value = cur.read_bytes(value_len)?.to_vec();
        entries.push((key, value));
    }
    if !cur.is_empty() {
        return Err(FerraError::TrailingChunkBytes { extra: cur.remaining() });
    }
    // Defense in depth: revalidate the canonical predicate even though we constructed
    // entries in deserialize order. A non-canonical on-disk chunk is a corruption signal.
    LeafChunk::new(entries)
}

fn deserialize_internal(bytes: &[u8]) -> Result<InternalChunk, FerraError> {
    let mut cur = Cursor::new(bytes);
    let tag = cur.read_u8()?;
    debug_assert_eq!(tag, INTERNAL_CHUNK_TAG);
    let version = cur.read_u8()?;
    if version != CHUNK_FORMAT_VERSION {
        return Err(FerraError::UnsupportedChunkVersion { version });
    }
    let level = cur.read_u8()?;
    let entry_count = cur.read_u32_le()? as usize;
    let mut children = Vec::with_capacity(entry_count);
    for _ in 0..entry_count {
        let separator_len = cur.read_u32_le()? as usize;
        let separator = cur.read_bytes(separator_len)?.to_vec();
        let addr_bytes: [u8; 32] = cur.read_bytes(32)?.try_into()
            .expect("read_bytes(32) returns 32 bytes");
        children.push((separator, Hash::from_bytes(addr_bytes)));
    }
    if !cur.is_empty() {
        return Err(FerraError::TrailingChunkBytes { extra: cur.remaining() });
    }
    InternalChunk::new(level, children)
}

// ==========================================================================
// Helper used by INV-FERR-048 (federation transfer)
// ==========================================================================

/// Decode the child addresses from a chunk's bytes. Used by `RecursiveTransfer`
/// (INV-FERR-048) to walk the tree without materializing the full chunk content.
/// Returns an empty vec for leaf chunks (leaves have no children).
pub fn decode_child_addrs(chunk: &Chunk) -> Result<Vec<Hash>, FerraError> {
    match deserialize_chunk(chunk.data())? {
        ProllyChunkBody::Leaf(_)         => Ok(Vec::new()),
        ProllyChunkBody::Internal(inode) => Ok(
            inode.children().iter().map(|(_, addr)| addr.clone()).collect()
        ),
    }
}

// ==========================================================================
// Kani harness — bounded round-trip
// ==========================================================================

#[kani::proof]
#[kani::unwind(4)]
fn leaf_chunk_roundtrip_bounded() {
    // Two entries, small keys and values.
    let k1: [u8; 2] = kani::any();
    let v1: [u8; 2] = kani::any();
    let k2: [u8; 2] = kani::any();
    let v2: [u8; 2] = kani::any();
    kani::assume(k1 != k2);

    let entries = vec![(k1.to_vec(), v1.to_vec()), (k2.to_vec(), v2.to_vec())];
    let leaf = LeafChunk::new(entries).expect("distinct keys are canonical");

    let bytes = serialize_leaf(&leaf);
    let body = deserialize_chunk(&bytes).expect("V1 bytes must round-trip");
    match body {
        ProllyChunkBody::Leaf(decoded) => assert_eq!(decoded, leaf),
        ProllyChunkBody::Internal(_) => panic!("expected leaf"),
    }
}

#[kani::proof]
#[kani::unwind(4)]
fn internal_chunk_roundtrip_bounded() {
    let s1: [u8; 2] = kani::any();
    let h1: [u8; 32] = kani::any();
    let s2: [u8; 2] = kani::any();
    let h2: [u8; 32] = kani::any();
    kani::assume(s1 != s2);

    let children = vec![
        (s1.to_vec(), Hash::from_bytes(h1)),
        (s2.to_vec(), Hash::from_bytes(h2)),
    ];
    let inode = InternalChunk::new(1, children).expect("distinct separators are canonical");

    let bytes = serialize_internal(&inode);
    let body = deserialize_chunk(&bytes).expect("V1 bytes must round-trip");
    match body {
        ProllyChunkBody::Internal(decoded) => assert_eq!(decoded, inode),
        ProllyChunkBody::Leaf(_) => panic!("expected internal"),
    }
}
```

**Falsification**: Any one of the following witnesses falsifies INV-FERR-045a.

1. **Round-trip failure**: a canonical `LeafChunk` (or `InternalChunk`) `c` such that
   `deserialize_chunk(serialize(c)) != Ok(c)`. This indicates the V1 format encoding and
   the V1 format decoding are inconsistent.

2. **Canonicality / injectivity failure**: two canonical chunks `c₁ ≠ c₂` (different
   logical entries) such that `serialize(c₁) = serialize(c₂)`. This is a hash-collision-free
   way to demonstrate that the encoding is not injective on canonical inputs.

3. **Type-level escape**: a code path that constructs a `LeafChunk` or `InternalChunk` with
   non-canonical entries (unsorted, duplicate keys, or — for internal — `level == 0`)
   without going through `LeafChunk::new` / `InternalChunk::new`. The presence of such a
   path means the type-level enforcement claim of Level 1 is false. The only sanctioned
   bypass is `from_sorted_unchecked`, which is `debug_assert!`-checked and documented as
   a hot-path optimization that requires upstream sortedness.

4. **Deserialize accepts non-canonical bytes**: an on-disk byte sequence whose decoded
   entries are not in strict ascending key order, yet `deserialize_chunk` returns `Ok`.
   This violates the defense-in-depth requirement for bytes received from untrusted sources.

5. **Cross-implementation divergence**: two implementations conforming to this spec that
   produce different `serialize_leaf(c)` outputs for the same canonical input `c`.

**proptest strategy**:
```rust
proptest! {
    /// Round-trip property: serialize then deserialize produces the original chunk.
    /// Drives Falsification cases #1 and #4 (the latter implicitly: deserialize_chunk
    /// must validate the canonical predicate, and re-serializing the validated result
    /// must equal the original bytes).
    #[test]
    fn leaf_chunk_roundtrip(
        raw_entries in prop::collection::btree_map(
            prop::collection::vec(any::<u8>(), 1..32),  // keys
            prop::collection::vec(any::<u8>(), 0..256), // values
            0..200,
        ),
    ) {
        let entries: Vec<_> = raw_entries.into_iter().collect();
        let leaf = LeafChunk::new(entries).expect("BTreeMap iteration is canonical");
        let bytes = serialize_leaf(&leaf);

        let decoded = deserialize_chunk(&bytes).expect("V1 bytes must round-trip");
        match decoded {
            ProllyChunkBody::Leaf(d) => prop_assert_eq!(d, leaf),
            ProllyChunkBody::Internal(_) => prop_assert!(false, "expected leaf"),
        }

        // Re-serialize must produce identical bytes (canonicality of the format).
        let re_bytes = serialize_leaf(&leaf);
        prop_assert_eq!(bytes, re_bytes);
    }

    #[test]
    fn internal_chunk_roundtrip(
        raw_children in prop::collection::btree_map(
            prop::collection::vec(any::<u8>(), 1..32),
            any::<[u8; 32]>(),
            1..100,
        ),
        level in 1u8..8,
    ) {
        let children: Vec<_> = raw_children.into_iter()
            .map(|(s, h)| (s, Hash::from_bytes(h)))
            .collect();
        let inode = InternalChunk::new(level, children).expect("BTreeMap is canonical");
        let bytes = serialize_internal(&inode);

        let decoded = deserialize_chunk(&bytes).expect("V1 bytes must round-trip");
        match decoded {
            ProllyChunkBody::Internal(d) => prop_assert_eq!(d, inode),
            ProllyChunkBody::Leaf(_) => prop_assert!(false, "expected internal"),
        }
    }

    /// Canonicality / injectivity: two distinct canonical chunks must serialize to
    /// distinct byte sequences. Drives Falsification case #2.
    #[test]
    fn leaf_chunk_serialize_injective(
        entries1 in prop::collection::btree_map(
            prop::collection::vec(any::<u8>(), 1..16),
            prop::collection::vec(any::<u8>(), 0..32),
            1..40,
        ),
        entries2 in prop::collection::btree_map(
            prop::collection::vec(any::<u8>(), 1..16),
            prop::collection::vec(any::<u8>(), 0..32),
            1..40,
        ),
    ) {
        prop_assume!(entries1 != entries2);
        let l1 = LeafChunk::new(entries1.into_iter().collect()).unwrap();
        let l2 = LeafChunk::new(entries2.into_iter().collect()).unwrap();
        prop_assert_ne!(serialize_leaf(&l1), serialize_leaf(&l2),
            "INV-FERR-045a: distinct canonical leaves must serialize differently");
    }

    /// Defense-in-depth: deserialize must reject non-canonical input even if the bytes
    /// are syntactically valid. Drives Falsification case #4.
    #[test]
    fn deserialize_rejects_unsorted_leaf(
        k1 in prop::collection::vec(any::<u8>(), 1..16),
        v1 in prop::collection::vec(any::<u8>(), 0..32),
        k2 in prop::collection::vec(any::<u8>(), 1..16),
        v2 in prop::collection::vec(any::<u8>(), 0..32),
    ) {
        prop_assume!(k1 > k2);  // Force descending order in the wire bytes.
        // Hand-craft non-canonical leaf bytes by writing entries in the wrong order.
        let mut bytes = vec![LEAF_CHUNK_TAG, CHUNK_FORMAT_VERSION];
        bytes.extend_from_slice(&2u32.to_le_bytes());
        bytes.extend_from_slice(&(k1.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&k1);
        bytes.extend_from_slice(&(v1.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&v1);
        bytes.extend_from_slice(&(k2.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&k2);
        bytes.extend_from_slice(&(v2.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&v2);

        let result = deserialize_chunk(&bytes);
        prop_assert!(matches!(result, Err(FerraError::NonCanonicalChunk { .. })),
            "INV-FERR-045a: deserialize must reject non-canonical bytes");
    }

    /// Type-level enforcement: LeafChunk::new with duplicate keys must fail.
    /// Drives Falsification case #3.
    #[test]
    fn leaf_chunk_rejects_duplicate_keys(
        k in prop::collection::vec(any::<u8>(), 1..16),
        v1 in prop::collection::vec(any::<u8>(), 0..16),
        v2 in prop::collection::vec(any::<u8>(), 0..16),
    ) {
        let entries = vec![(k.clone(), v1), (k, v2)];
        let result = LeafChunk::new(entries);
        prop_assert!(matches!(result, Err(FerraError::NonCanonicalChunk { .. })),
            "INV-FERR-045a: LeafChunk::new must reject duplicate keys");
    }
}
```

**Lean theorem**:
```lean
/-- Deterministic chunk serialization (INV-FERR-045a).
    Modeled at the abstract level: serialize is a function on canonical chunks
    and is injective. The concrete byte layout is verified by proptest + Kani. -/

inductive ProllyChunkBody where
  | leaf     (entries  : List (List UInt8 × List UInt8))
  | internal (level    : Nat) (children : List (List UInt8 × Hash))

/-- A leaf chunk is canonical iff its entries are strictly ascending by key
    (which implies duplicate-free). -/
def canonicalLeaf (entries : List (List UInt8 × List UInt8)) : Prop :=
  entries.Pairwise (fun a b => a.1 < b.1)

def canonicalInternal (level : Nat) (children : List (List UInt8 × Hash)) : Prop :=
  level ≥ 1 ∧ children.Pairwise (fun a b => a.1 < b.1)

def canonicalChunk : ProllyChunkBody → Prop
  | .leaf entries     => canonicalLeaf entries
  | .internal lvl chs => canonicalInternal lvl chs

/-- Abstract serialization function. The concrete byte layout is given by
    `serialize_leaf` / `serialize_internal` in Level 2; here we treat it as
    an opaque function and prove the algebraic properties. -/
axiom serializeChunk : ProllyChunkBody → List UInt8

/-- Round-trip: deserializing a serialized canonical chunk recovers the original.
    Modeled as: there exists a deserialization function such that this holds. -/
axiom deserializeChunk : List UInt8 → Option ProllyChunkBody

axiom roundtrip_canonical (c : ProllyChunkBody) (h : canonicalChunk c) :
    deserializeChunk (serializeChunk c) = some c

/-- Injectivity on canonical chunks: distinct canonical chunks have distinct bytes. -/
theorem serialize_injective_canonical
    (c₁ c₂ : ProllyChunkBody)
    (h₁ : canonicalChunk c₁)
    (h₂ : canonicalChunk c₂)
    (h_eq : serializeChunk c₁ = serializeChunk c₂) :
    c₁ = c₂ := by
  have r₁ := roundtrip_canonical c₁ h₁
  have r₂ := roundtrip_canonical c₂ h₂
  rw [h_eq] at r₁
  -- Both r₁ and r₂ now state: deserializeChunk (serializeChunk c₂) = some <something>
  -- Functional equality of deserializeChunk forces the somethings to agree.
  have : some c₁ = some c₂ := by rw [← r₁, ← r₂]
  exact Option.some.inj this

/-- Content-addressing stability: distinct canonical chunks have distinct addresses
    (modulo BLAKE3 collision, which is treated as impossible in the abstract model). -/
theorem chunk_addr_injective_canonical
    (c₁ c₂ : ProllyChunkBody)
    (h₁ : canonicalChunk c₁)
    (h₂ : canonicalChunk c₂)
    (h_addr : blake3 (serializeChunk c₁) = blake3 (serializeChunk c₂)) :
    c₁ = c₂ := by
  -- BLAKE3 is injective on the practical inputs of interest (collision resistance).
  -- We axiomatize this in the foundation model; see 00-preamble.md §23.0.4.
  have h_bytes : serializeChunk c₁ = serializeChunk c₂ :=
    blake3_injective h_addr
  exact serialize_injective_canonical c₁ c₂ h₁ h₂ h_bytes

-- The two `axiom` declarations above are tracked for replacement with concrete
-- definitions when the V1 byte layout is formalized at the byte level. The current
-- form proves the algebraic properties needed by INV-FERR-046 (history independence)
-- and INV-FERR-049 (snapshot identity) without depending on a specific layout.
-- Tracked: bd-aqg9h (INV-FERR-045a Lean concretization).
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
    not the insertion history. The substantive proof: any two LISTS that determine
    the same SET produce the same sorted sequence and therefore the same tree. -/

/-- A boundary function that depends only on the key, not on insertion order. -/
def is_boundary (key : List UInt8) (pattern_width : Nat) : Bool :=
  (rolling_hash key) % (2 ^ pattern_width) == 0

/-- Chunk boundaries are determined by the sorted key list. The function is
    a pure function of the input list. -/
def chunk_boundaries (keys : List (List UInt8)) (pw : Nat) : List Nat :=
  keys.enum.filterMap fun (i, k) => if is_boundary k pw then some i else none

/-- The substantive history independence theorem: two lists with the same
    multiset of entries (i.e., same elements, possibly different order) produce
    the same prolly tree. The proof uses `List.Perm` (permutation equivalence)
    and the fact that `List.mergeSort` is a function from permutation classes
    to canonical sorted lists. -/
theorem history_independence_perm
    (xs ys : List (List UInt8 × List UInt8))
    (h : xs.Perm ys) (pw : Nat) :
    prolly_root (xs.mergeSort (fun a b => a.1 < b.1)) pw =
    prolly_root (ys.mergeSort (fun a b => a.1 < b.1)) pw := by
  -- mergeSort is permutation-invariant: equal multisets produce equal sorted lists.
  have h_sort : xs.mergeSort (fun a b => a.1 < b.1)
              = ys.mergeSort (fun a b => a.1 < b.1) :=
    List.mergeSort_eq_of_perm h
  rw [h_sort]

/-- Corollary: insertion in any order produces the same tree, because
    `List.insert` permutations of the same elements are `Perm`-equivalent. -/
theorem history_independence_set
    (kvs : Finset (List UInt8 × List UInt8)) (pw : Nat)
    (xs ys : List (List UInt8 × List UInt8))
    (hxs : xs.toFinset = kvs) (hys : ys.toFinset = kvs)
    (hxs_nodup : xs.Nodup) (hys_nodup : ys.Nodup) :
    prolly_root (xs.mergeSort (fun a b => a.1 < b.1)) pw =
    prolly_root (ys.mergeSort (fun a b => a.1 < b.1)) pw := by
  -- Two duplicate-free lists with the same toFinset are permutations of each other.
  have h_perm : xs.Perm ys :=
    List.perm_of_nodup_toFinset_eq hxs_nodup hys_nodup (hxs.trans hys.symm)
  exact history_independence_perm xs ys h_perm pw

/-- Merge commutativity extends to prolly trees via set commutativity.
    This was already substantive (uses `Finset.union_comm`); kept as the
    second pillar of history independence. -/
theorem prolly_merge_comm (a b : Finset (List UInt8 × List UInt8)) (pw : Nat) :
    prolly_root ((a ∪ b).sort (fun x y => x.1 < y.1)) pw =
    prolly_root ((b ∪ a).sort (fun x y => x.1 < y.1)) pw := by
  rw [Finset.union_comm]

-- Helper lemmas (axiomatized at this layer; concrete proofs tracked):
-- - List.mergeSort_eq_of_perm: permutation-equivalent lists sort to the same list
-- - List.perm_of_nodup_toFinset_eq: duplicate-free lists with the same Finset are
--   permutation-equivalent
-- These are standard mathlib results; the proofs above are mechanical applications.
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
/// # Root parameter scope (S23.9.0 disambiguation)
/// `root1` and `root2` are **tree roots** (the root chunk address of one prolly
/// tree), NOT manifest hashes. The `Snapshot` API of INV-FERR-049 stores manifest
/// hashes that resolve to a `RootSet` of five tree roots; callers that hold
/// `Snapshot`s must extract the appropriate tree root via
/// `Snapshot::resolve_root_set(...).primary` (or `.eavt`, `.aevt`, etc.) before
/// invoking `diff`. The `Snapshot::diff` method handles this two-step protocol
/// internally and is the preferred entry point for snapshot-level diffing.
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
    /// # Root parameter scope (S23.9.0 disambiguation)
    /// `root` is a **tree root** — the address of one prolly tree's root chunk
    /// (a leaf-or-internal chunk, parseable by `decode_child_addrs` per
    /// INV-FERR-045a). It is NOT a manifest hash; manifest chunks are 160 raw
    /// bytes (S23.9.0.5) without a `LEAF_CHUNK_TAG`/`INTERNAL_CHUNK_TAG`
    /// discriminator and therefore cannot be parsed as prolly tree internal
    /// nodes. Calling `transfer(_, _, manifest_hash)` would copy ONLY the
    /// 160-byte manifest chunk, not the trees it points to, because
    /// `decode_child_addrs` would fail on the manifest's first byte.
    ///
    /// To transfer a complete snapshot (all five trees plus the manifest), use
    /// `Snapshot::transfer_to_dst` (defined in INV-FERR-049 Level 2), which
    /// orchestrates the two-phase protocol:
    ///   1. `put_chunk(manifest_chunk)` into `dst` (160 bytes, idempotent).
    ///   2. For each tree root in the resolved `RootSet`, call
    ///      `transfer(src, dst, &tree_root)` — five tree transfers in total.
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

**Traces to**: INV-FERR-045 (Chunk Content Addressing), INV-FERR-045a (Deterministic
Chunk Serialization), S23.9.0 (Canonical Datom Key Encoding — RootSet manifest
structure), INV-FERR-006 (Snapshot Isolation), C2 (Identity by Content)
**Verification**: `V:PROP`, `V:LEAN`
**Stage**: 1

#### Level 0 (Algebraic Law)
```
Let ProllyTree be the type of prolly trees.
Let RootSet be the multi-tree snapshot manifest from S23.9.0.4 — a record of five
  tree roots: { primary, eavt, aevt, vaet, avet : Hash }.
Let Snapshot be the externally visible snapshot identifier: a single Hash that
  is the BLAKE3 of a serialized RootSet (S23.9.0.6).

Let tree_root_hash : ProllyTree -> Hash extract the root chunk address of one tree.
Let tree_resolve   : ChunkStore x Hash -> ProllyTree reconstruct one tree from its
                     root chunk address.
Let serialize_rs   : RootSet -> [u8; 160]               -- S23.9.0.5
Let snapshot_hash  : RootSet -> Hash := λ rs. BLAKE3(serialize_rs(rs))
Let resolve_rs     : ChunkStore x Hash -> RootSet := λ S, h.
                     RootSet::from_canonical_bytes(get_chunk(S, h))

Theorem (per-tree snapshot identity):
  ∀ T : ProllyTree, ∀ S : ChunkStore containing all chunks of T:
    tree_resolve(S, tree_root_hash(T)) = T

Proof: By structural induction on tree height.
  - Base case (leaf node): tree_root_hash(T) = BLAKE3(content(T)). tree_resolve
    fetches the chunk via INV-FERR-045 content-addressing and deserializes the
    leaf bytes via INV-FERR-045a's `deserialize_chunk`. The round-trip property
    of INV-FERR-045a guarantees the recovered LeafChunk equals T.
  - Inductive case (internal node): tree_root_hash(T) addresses an internal chunk
    whose deserialized form contains child hashes (INV-FERR-045a). tree_resolve
    invokes itself on each child hash; by induction each child resolves correctly,
    and reassembly produces T.

Theorem (snapshot identity / multi-tree extension):
  ∀ rs : RootSet, ∀ S : ChunkStore containing the manifest chunk and all chunks
                  of all five trees in rs:
    resolve_rs(S, snapshot_hash(rs)) = rs                                  -- (M1)
    ∀ field ∈ {primary, eavt, aevt, vaet, avet}:
      tree_resolve(S, rs.field) = the prolly tree built for that index      -- (M2)

Proof of (M1):
  snapshot_hash(rs) = BLAKE3(serialize_rs(rs)) is the address of a 160-byte chunk
  whose content is the canonical RootSet bytes. INV-FERR-045 guarantees that
  get_chunk(S, snapshot_hash(rs)) returns those exact 160 bytes (assuming S
  contains the manifest chunk). RootSet::from_canonical_bytes is the structural
  inverse of canonical_bytes (S23.9.0.5: fixed 160-byte layout, no padding,
  field order primary/eavt/aevt/vaet/avet). Therefore resolve_rs recovers rs
  exactly.

Proof of (M2): apply the per-tree snapshot identity theorem to each of the five
  fields of the resolved rs.

Corollary (snapshot cost):
  Creating a snapshot is O(1): serialize the current RootSet (160 bytes), put_chunk
  the result, record its address. The five trees and their chunks already exist in
  the chunk store (immutable, content-addressed). No tree data is copied; the
  manifest hash IS the externally visible snapshot identifier.

Corollary (version history):
  A sequence of manifest hashes [h1, h2, ..., hn] provides a complete version
  history. Each hi resolves to a RootSet, and through the RootSet to all five
  per-version tree states. Chunks are shared across versions by content-addressing,
  so the incremental storage cost of each new version is O(d) (the changed chunks
  in any of the five trees) plus 160 bytes for the new manifest, not O(n).
```

#### Level 1 (State Invariant)
For all reachable `(RootSet, ChunkStore)` pairs where the chunk store contains
the 160-byte manifest chunk addressed by `snapshot_hash(RootSet)` AND all chunks
reachable from each of the five tree roots:

- `Snapshot::resolve_root_set(store, snapshot_hash)` produces a `RootSet`
  identical to the original. The 160-byte manifest round-trip is lossless and
  is enforced by the fixed field layout in S23.9.0.5.
- For each of the five fields (`primary`, `eavt`, `aevt`, `vaet`, `avet`), the
  per-tree resolve protocol of INV-FERR-045/045a yields the original prolly tree.
  The round-trip from a key-value set through `build_prolly_tree` and back through
  `read_prolly_tree(rs.primary, store)` is lossless.
- The manifest hash uniquely identifies the snapshot state. Two snapshots with
  different `RootSet`s produce different manifest hashes (by injectivity of
  `serialize_rs` over fixed-layout 160-byte sequences and by BLAKE3 collision
  resistance — the same combinatorial argument as INV-FERR-045).
- The 32-byte manifest hash is a complete summary of the multi-tree store. The
  external "snapshot = root hash" abstraction (single Hash externally visible)
  is preserved even though the internal representation is five trees.
- Old manifest hashes remain valid as long as the manifest chunk and all five
  trees' chunks remain in the store. The chunk store is append-only during
  normal operation; garbage collection is a separate, explicit lifecycle event
  that the application controls (S23.9.2).

This invariant is the foundation for the journal format (section 23.9.2): the
journal stores `RootUpdate` records containing manifest hashes, and each manifest
hash resolves to a complete snapshot of all five trees at that point. Combined
with the O(d) diff per tree (INV-FERR-047), the journal enables efficient
time-travel queries: "diff the store between version V1 and V2" reduces to
loading both manifests and diffing the corresponding tree pairs.

#### Level 2 (Implementation Contract)
```rust
/// A snapshot is identified by a single MANIFEST hash that resolves to a `RootSet`
/// (per S23.9.0.4). The manifest hash is `BLAKE3(serialize(root_set))` per S23.9.0.6.
///
/// The single-Hash external interface preserves INV-FERR-049's "snapshot = root hash"
/// claim. The internal indirection through `RootSet` is what makes the multi-tree
/// store (5 prolly trees per S23.9.0.1) addressable by a single 32-byte identifier.
///
/// Creating a snapshot: serialize the current `RootSet`, store the manifest chunk,
///   record its address. O(1) chunk write (160 bytes).
/// Loading a snapshot: load the manifest chunk, deserialize the `RootSet`, descend
///   into individual tree roots. O(1) manifest load + O(n) tree traversal.
/// Diffing snapshots: load both manifests (O(1)), compare each `RootSet` field pair
///   (O(1)), and `diff()` only the trees whose roots differ. Common case: a single
///   transaction touches all five indexes, so all five tree pairs need O(d * log_k n)
///   diffing.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Snapshot {
    /// The MANIFEST hash. Per S23.9.0.6: `manifest_hash = BLAKE3(RootSet::canonical_bytes())`.
    /// To recover the five tree roots, load the chunk addressed by `manifest_hash`
    /// and deserialize its 160 bytes through `RootSet::from_canonical_bytes`.
    manifest: Hash,
    /// The transaction that produced this snapshot (for ordering).
    tx: TxId,
}

impl Snapshot {
    /// Create a snapshot from a `RootSet`. Stores the manifest chunk and records
    /// its content-addressed hash. O(1).
    pub fn create(
        root_set: &RootSet,
        tx: TxId,
        chunk_store: &dyn ChunkStore,
    ) -> Result<Self, FerraError> {
        let manifest_bytes = root_set.canonical_bytes();
        let manifest_chunk = Chunk::from_bytes(&manifest_bytes);
        let manifest = manifest_chunk.addr().clone();
        chunk_store.put_chunk(&manifest_chunk)?;
        Ok(Snapshot { manifest, tx })
    }

    /// Reconstruct a `Snapshot` from a known manifest hash (e.g., loaded from the
    /// journal). The chunk store is consulted lazily during `resolve_root_set`.
    pub fn from_manifest(manifest: Hash, tx: TxId) -> Self {
        Snapshot { manifest, tx }
    }

    /// The manifest hash. This IS the externally-visible snapshot identifier.
    pub fn manifest(&self) -> &Hash { &self.manifest }

    /// The transaction that produced this state.
    pub fn tx(&self) -> &TxId { &self.tx }

    /// Resolve this snapshot's manifest hash into the five tree roots.
    /// O(1) chunk load + O(1) deserialize. Per S23.9.0.6.
    pub fn resolve_root_set(
        &self,
        chunk_store: &dyn ChunkStore,
    ) -> Result<RootSet, FerraError> {
        let manifest_chunk = chunk_store.get_chunk(&self.manifest)?
            .ok_or_else(|| FerraError::ChunkNotFound(self.manifest.clone()))?;
        let buf: &[u8; 160] = manifest_chunk.data().try_into()
            .map_err(|_| FerraError::InvalidManifestSize {
                expected: 160,
                actual: manifest_chunk.data().len(),
            })?;
        Ok(RootSet::from_canonical_bytes(buf))
    }

    /// Resolve this snapshot to the full key-value set of the PRIMARY tree.
    /// Two-step protocol: (1) load manifest → RootSet, (2) descend the primary tree.
    /// O(1) manifest load + O(n) tree traversal where n = number of datoms.
    pub fn resolve(
        &self,
        chunk_store: &dyn ChunkStore,
    ) -> Result<BTreeMap<Key, Value>, FerraError> {
        let root_set = self.resolve_root_set(chunk_store)?;
        read_prolly_tree(&root_set.primary, chunk_store)
    }

    /// Diff this snapshot against another at the manifest level first, descending
    /// into individual tree pairs only where their roots differ.
    ///
    /// O(1) fast path when manifests are identical (no chunks loaded).
    /// O(1) per-tree fast path when individual tree roots are identical.
    /// O(d * log_k n) per tree where roots differ.
    ///
    /// The returned iterator yields entries from the PRIMARY tree only. Callers
    /// that need cross-index diffs must call `diff_index` for each ordering.
    pub fn diff<'a>(
        &self,
        other: &Snapshot,
        chunk_store: &'a dyn ChunkStore,
    ) -> Result<Box<dyn Iterator<Item = Result<DiffEntry, FerraError>> + 'a>, FerraError> {
        // Manifest fast path: identical manifest hash means identical RootSet,
        // which means identical trees by S23.9.0.6 + INV-FERR-045.
        if self.manifest == other.manifest {
            return Ok(Box::new(std::iter::empty()));
        }

        let rs_self  = self.resolve_root_set(chunk_store)?;
        let rs_other = other.resolve_root_set(chunk_store)?;

        // Tree-level fast path: identical primary roots → no primary diff.
        if rs_self.primary == rs_other.primary {
            return Ok(Box::new(std::iter::empty()));
        }
        Ok(Box::new(diff(&rs_self.primary, &rs_other.primary, chunk_store)?))
    }

    /// Transfer this snapshot (manifest + all five trees) from `src` to `dst`.
    ///
    /// Two-phase protocol because manifests are not parseable by INV-FERR-045a's
    /// `decode_child_addrs` (manifests have no chunk discriminator; they are 160
    /// raw bytes per S23.9.0.5):
    ///
    /// 1. **Manifest phase**: copy the 160-byte manifest chunk from `src` to `dst`
    ///    via `put_chunk`. Idempotent — re-runs are no-ops.
    /// 2. **Tree phase**: resolve the manifest into a `RootSet` by reading from
    ///    whichever store has it (preferentially `src`, fallback `dst` if the
    ///    manifest was already present), then call `ChunkTransfer::transfer`
    ///    for each of the five tree roots.
    ///
    /// After completion, `dst` contains every chunk reachable from this snapshot.
    /// `Snapshot::resolve_root_set` and `Snapshot::resolve` will succeed against
    /// `dst` without further chunk fetches.
    pub fn transfer_to_dst(
        &self,
        src: &dyn ChunkStore,
        dst: &dyn ChunkStore,
        chunk_transfer: &dyn ChunkTransfer,
    ) -> Result<(), FerraError> {
        // Phase 1: copy the manifest chunk if dst doesn't already have it.
        if !dst.has_chunk(&self.manifest)? {
            let manifest_chunk = src.get_chunk(&self.manifest)?
                .ok_or_else(|| FerraError::ChunkNotFound(self.manifest.clone()))?;
            dst.put_chunk(&manifest_chunk)?;
        }

        // Phase 2: resolve and transfer each tree root.
        let root_set = self.resolve_root_set(src)?;
        chunk_transfer.transfer(src, dst, &root_set.primary)?;
        chunk_transfer.transfer(src, dst, &root_set.eavt)?;
        chunk_transfer.transfer(src, dst, &root_set.aevt)?;
        chunk_transfer.transfer(src, dst, &root_set.vaet)?;
        chunk_transfer.transfer(src, dst, &root_set.avet)?;
        Ok(())
    }
}

/// A version history: sequence of snapshots (manifest hashes).
/// Storage cost: O(versions x 32 bytes) for the manifest list, plus O(d_total)
/// chunks across the version history (chunks shared by content addressing per
/// INV-FERR-045 + INV-FERR-046).
pub struct VersionHistory {
    /// Ordered list of `Snapshot { manifest, tx }` pairs.
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
    ) -> Result<Box<dyn Iterator<Item = Result<DiffEntry, FerraError>> + 'a>, FerraError> {
        let snap_from = self.at_version(from)
            .ok_or_else(|| FerraError::VersionNotFound(from.clone()))?;
        let snap_to = self.at_version(to)
            .ok_or_else(|| FerraError::VersionNotFound(to.clone()))?;
        snap_from.diff(snap_to, chunk_store)
    }
}
```

> **Manifest model rationale (§23.9.0.6)**: The original spec authored a single-tree
> Snapshot model where `Snapshot::root` was a direct prolly tree pointer. This was
> incompatible with the multi-tree (primary + EAVT/AEVT/VAET/AVET) physical layout
> that ferratomic-store actually requires per INV-FERR-005. The Pattern H authoring
> in session 023 (S23.9.0) made the manifest model canonical, and FINDING-226 of
> the session 023 spec audit identified this Level 2 as inconsistent with §23.9.0.6.
> The L2 contract above was updated to introduce the manifest hash as a content-addressed
> pointer to a `RootSet` chunk, preserving the "snapshot = root hash" external
> abstraction (INV-FERR-049 Level 0) while making the multi-tree resolve protocol
> explicit (INV-FERR-049 Level 2).

**Falsification**: Any one of the following witnesses falsifies INV-FERR-049.

1. **Per-tree round-trip failure**: A `tree_resolve(store, tree_root_hash(tree))` call
   that produces a key-value set different from the original tree's key-value set.
   Concretely: build a prolly tree from key-value set KV, extract `root_hash`, then
   `read_prolly_tree(root_hash, store)` and compare the result to KV.

2. **Manifest round-trip failure**: A `Snapshot::resolve_root_set(store)` call that
   produces a `RootSet` whose fields differ from the original `RootSet` used to
   construct the snapshot. This indicates the 160-byte canonical serialization
   from S23.9.0.5 is broken (wrong field order, wrong width, padding leak).

3. **Snapshot identity collision**: Two distinct `RootSet`s `rs1 != rs2` (different
   in at least one field) such that `snapshot_hash(rs1) = snapshot_hash(rs2)`.
   This would indicate either a BLAKE3 collision (negligible) or a serialization
   bug where two different `RootSet`s produce the same canonical bytes.

4. **Cross-tree leakage**: A `Snapshot::resolve` call that returns a key-value set
   containing keys from a different snapshot's primary tree. This would indicate
   the chunk store is non-deterministic or that the manifest hash is being misread.

**proptest strategy**:
```rust
proptest! {
    /// (1) Per-tree snapshot round-trip — builds one tree, resolves via the
    /// `Snapshot::resolve` two-step protocol, and compares against the original kvs.
    #[test]
    fn snapshot_primary_roundtrip(
        kvs in prop::collection::btree_map(
            prop::collection::vec(any::<u8>(), 1..32),
            prop::collection::vec(any::<u8>(), 1..256),
            0..500
        ),
        pattern_width in 4u32..12,
    ) {
        let store = MemoryChunkStore::new();
        let primary_root = build_prolly_tree(&kvs, &store, pattern_width).unwrap();
        // Single-tree test: populate only the primary field, leave others as
        // genesis sentinels. Real snapshots populate all five.
        let root_set = RootSet {
            primary: primary_root,
            eavt:    Hash::genesis(),
            aevt:    Hash::genesis(),
            vaet:    Hash::genesis(),
            avet:    Hash::genesis(),
        };
        let snapshot = Snapshot::create(&root_set, TxId::genesis(), &store).unwrap();
        let resolved = snapshot.resolve(&store).unwrap();

        prop_assert_eq!(resolved, kvs,
            "Snapshot primary roundtrip lost data: built from {} kvs, resolved {}",
            kvs.len(), resolved.len());
    }

    /// (2) Manifest round-trip — builds a `RootSet` with arbitrary tree roots,
    /// stores the manifest, then resolves and compares.
    #[test]
    fn snapshot_manifest_roundtrip(
        primary_bytes in any::<[u8; 32]>(),
        eavt_bytes in any::<[u8; 32]>(),
        aevt_bytes in any::<[u8; 32]>(),
        vaet_bytes in any::<[u8; 32]>(),
        avet_bytes in any::<[u8; 32]>(),
    ) {
        let root_set = RootSet {
            primary: Hash::from_bytes(primary_bytes),
            eavt:    Hash::from_bytes(eavt_bytes),
            aevt:    Hash::from_bytes(aevt_bytes),
            vaet:    Hash::from_bytes(vaet_bytes),
            avet:    Hash::from_bytes(avet_bytes),
        };
        let store = MemoryChunkStore::new();
        let snapshot = Snapshot::create(&root_set, TxId::genesis(), &store).unwrap();

        let resolved_rs = snapshot.resolve_root_set(&store).unwrap();
        prop_assert_eq!(resolved_rs, root_set,
            "Manifest roundtrip lost or reordered tree roots");

        // Verify the manifest chunk is exactly 160 bytes (S23.9.0.5).
        let manifest_chunk = store.get_chunk(snapshot.manifest()).unwrap().unwrap();
        prop_assert_eq!(manifest_chunk.data().len(), 160,
            "Manifest chunk must be exactly 160 bytes per S23.9.0.5");
    }

    /// (3) Snapshot identity — same `RootSet` always produces the same manifest hash.
    #[test]
    fn snapshot_identity(
        primary_bytes in any::<[u8; 32]>(),
    ) {
        let rs = RootSet {
            primary: Hash::from_bytes(primary_bytes),
            eavt:    Hash::genesis(),
            aevt:    Hash::genesis(),
            vaet:    Hash::genesis(),
            avet:    Hash::genesis(),
        };
        let store1 = MemoryChunkStore::new();
        let store2 = MemoryChunkStore::new();
        let s1 = Snapshot::create(&rs, TxId::genesis(), &store1).unwrap();
        let s2 = Snapshot::create(&rs, TxId::genesis(), &store2).unwrap();

        prop_assert_eq!(s1.manifest(), s2.manifest(),
            "Same RootSet must produce same manifest hash (snapshot identity)");
    }

    /// (4) Snapshot injectivity — distinct RootSets produce distinct manifest hashes.
    #[test]
    fn snapshot_distinct_for_different_data(
        rs1_primary in any::<[u8; 32]>(),
        rs2_primary in any::<[u8; 32]>(),
    ) {
        prop_assume!(rs1_primary != rs2_primary);
        let mk = |p: [u8; 32]| RootSet {
            primary: Hash::from_bytes(p),
            eavt:    Hash::genesis(),
            aevt:    Hash::genesis(),
            vaet:    Hash::genesis(),
            avet:    Hash::genesis(),
        };
        let store = MemoryChunkStore::new();
        let s1 = Snapshot::create(&mk(rs1_primary), TxId::genesis(), &store).unwrap();
        let s2 = Snapshot::create(&mk(rs2_primary), TxId::genesis(), &store).unwrap();

        prop_assert_ne!(s1.manifest(), s2.manifest(),
            "Different RootSets must produce different manifest hashes");
    }
}
```

**Lean theorem**:
```lean
/-- Snapshot identity at two layers:
    (a) per-tree: tree_resolve(store, tree_root_hash(tree)) = tree
    (b) multi-tree: resolve_rs(store, snapshot_hash(rs)) = rs
    Both are theorems in the Lean foundation model; (b) reduces (a) over the
    five RootSet fields. -/

/-- A per-tree snapshot is the content-addressed root hash of a single prolly tree. -/
def tree_snapshot (tree : ProllyTree) : Hash := tree_root_hash tree

/-- Per-tree round-trip: resolving a tree's root hash recovers the tree. -/
theorem tree_snapshot_roundtrip (tree : ProllyTree) (store : ChunkStore)
    (h : chunks_of tree ⊆ chunks_in store) :
    tree_resolve store (tree_snapshot tree) = tree := by
  induction tree with
  | leaf kvs =>
    simp [tree_snapshot, tree_root_hash, tree_resolve, key_values]
    exact chunk_retrieve_correct store (tree_root_hash (leaf kvs)) h
  | node children ih =>
    simp [tree_snapshot, tree_root_hash, tree_resolve, key_values]
    congr 1
    exact ih (chunks_subset_of_children h)

/-- Multi-tree snapshot: a RootSet is identified by BLAKE3 of its 160-byte
    canonical serialization (S23.9.0.5–.6). -/
structure RootSet where
  primary : Hash
  eavt    : Hash
  aevt    : Hash
  vaet    : Hash
  avet    : Hash

axiom serialize_rs : RootSet → List UInt8  -- 160 bytes per S23.9.0.5
axiom rs_serialize_injective : ∀ a b : RootSet,
  serialize_rs a = serialize_rs b → a = b
axiom rs_from_canonical_bytes : List UInt8 → Option RootSet
axiom rs_roundtrip : ∀ rs : RootSet,
  rs_from_canonical_bytes (serialize_rs rs) = some rs

def snapshot_hash (rs : RootSet) : Hash := blake3 (serialize_rs rs)

/-- Manifest round-trip: resolving the manifest hash through the chunk store
    yields the original RootSet. -/
theorem rs_snapshot_roundtrip (rs : RootSet) (store : ChunkStore)
    (h_manifest : (snapshot_hash rs, serialize_rs rs) ∈ chunks_in store) :
    rs_from_canonical_bytes
      (chunk_retrieve store (snapshot_hash rs)) = some rs := by
  -- The manifest chunk in store has content = serialize_rs rs (by content-addressing,
  -- INV-FERR-045). chunk_retrieve returns that content. rs_roundtrip then applies.
  rw [chunk_retrieve_eq_content store (snapshot_hash rs) (serialize_rs rs) h_manifest]
  exact rs_roundtrip rs

/-- Two RootSets with different fields have different manifest hashes
    (assuming BLAKE3 collision resistance). -/
theorem snapshot_hash_injective (a b : RootSet)
    (h : snapshot_hash a = snapshot_hash b) : a = b := by
  -- Manifest hash is BLAKE3 of canonical bytes; BLAKE3 is injective in the
  -- collision-resistance model (00-preamble.md §23.0.4).
  have h_bytes : serialize_rs a = serialize_rs b := blake3_injective h
  exact rs_serialize_injective a b h_bytes

/-- Two trees with the same key-value set have the same per-tree root hash
    (history independence INV-FERR-046 lifted into snapshot identity). -/
theorem tree_snapshot_deterministic (t1 t2 : ProllyTree)
    (h : key_values t1 = key_values t2) :
    tree_snapshot t1 = tree_snapshot t2 :=
  history_independence_root h

-- Tracked: bd-uhjj3 — replace `axiom serialize_rs` etc. with concrete
-- definitions over a 160-byte vector model and prove `rs_serialize_injective`
-- directly. The current axiomatic form proves the algebraic snapshot identity
-- needed by INV-FERR-049 without depending on a specific byte-level Lean
-- implementation.
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
