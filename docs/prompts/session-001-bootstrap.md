# Ferratomic Session 001 — Bootstrap: Lean Proofs + Test Suite + Type Definitions

> **Scope**: Phases 1-3. Lean proofs, test suite (red phase), type definitions.
> **Mandate**: Cleanroom, lab-grade, zero-defect, NASA-grade Rust.
> **Method**: Spec-first TDD, Curry-Howard-Lambek, `ms` "rust-formal-engineering".
> **Prerequisite**: Spec complete (spec/23-ferratomic.md in braid — symlinked at docs/spec/).
> **Critical rule**: NO IMPLEMENTATION until Phases 1-2 are complete and isomorphic with spec.

---

## Phase 0: Context Recovery (do this FIRST)

1. Read `AGENTS.md` (this project's guidelines and hard constraints)
2. Read `docs/spec/23-ferratomic.md` — the formal specification (36 INV, 7 ADR, 5 NEG)
3. Read `docs/design/FERRATOMIC_ARCHITECTURE.md` — comprehensive architecture (17 sections)
4. Read `ferratomic-verify/lean/Ferratomic/Store.lean` — existing Lean proofs (CRDT foundation)
5. Run `ms load rust-formal-engineering -m --full`
6. Run `ms load spec-first-design -m --full`

**Checkpoint**: Before writing any code, verify:
- You understand the 36 INV-FERR invariants and which phase each belongs to
- You understand the Lean-Rust bridge methodology (parallel models + conformance tests)
- You understand the crate dependency DAG: ferratom → ferratomic-core → ferratomic-datalog
- `CARGO_TARGET_DIR=/data/cargo-target` (NOT /tmp)
- Git is clean on `main`

---

## Phase 1: Lean 4 Theorem Statements + Proofs

**Goal**: Every INV-FERR with `V:LEAN` tag gets a machine-checked theorem in Lean 4.

### Files to create/complete

| File | Theorems | INV-FERR |
|------|----------|----------|
| `Ferratomic/Store.lean` | merge_comm, merge_assoc, merge_idemp, merge_mono_left, merge_mono_right, transact_mono, merge_convergence | 001-004, 010, 018 |
| `Ferratomic/Datom.lean` | content_id (identity by content hash) | 012 |
| `Ferratomic/Index.lean` | index_bij (primary ↔ secondary bijection) | 005 |
| `Ferratomic/Clock.lean` | hlc_mono (monotonicity), hlc_causal (causality preservation) | 015, 016 |
| `Ferratomic/Schema.lean` | schema_valid (validation correctness), genesis_determinism | 009, 031 |
| `Ferratomic/Convergence.lean` | anti_entropy_convergence, shard_union_equiv | 017, 022 |

### Methodology

Store.lean is already started with the CRDT proofs (they're one-liners via mathlib's
`Finset.union_comm/assoc/self`). The remaining files model ferratomic's specific
structures in Lean and prove the invariant properties.

The Lean model is a PARALLEL MODEL — it doesn't extract to Rust. Instead:
1. Lean proves properties of the algebraic model
2. Lean exports test vectors (specific inputs → expected outputs)
3. Rust proptest (Phase 2) verifies the Rust implementation matches the Lean model

**Isomorphism check**: Every INV-FERR with `V:LEAN` has a theorem in Lean.
Run `lake build` — all theorems type-check.

**Acceptance**: `cd ferratomic-verify/lean && lake build` succeeds with zero errors.

---

## Phase 2: Test Suite (Red Phase)

**Goal**: Every INV-FERR has at least one executable test. ALL tests MUST FAIL initially.
This is the red phase of TDD — tests define the contract before implementation exists.

### Stateright models (`ferratomic-verify/stateright/`)

| Model | Properties Checked | INV-FERR |
|-------|-------------------|----------|
| `crdt_model.rs` | Merge commutativity, convergence under all message orderings | 001-003, 010 |
| `snapshot_model.rs` | Snapshot isolation, write linearizability, observer monotonicity | 006, 007, 011, 020 |
| `recovery_model.rs` | Crash recovery correctness, WAL ordering | 008, 014 |
| `federation_model.rs` | Gossip convergence, anti-entropy repair, partition tolerance | 022, 034-036 |
| `federated_query_model.rs` | Fan-out correctness, selective merge CRDT preservation, transport transparency, latency tolerance, live migration | 037-042 |

State type, action type, and property definitions are specified in
`spec/23-ferratomic.md` §23.0.5 and §23.2.

### Kani harnesses (`ferratomic-verify/kani/`)

| Harness | Bounds | INV-FERR |
|---------|--------|----------|
| `index_harness.rs` | ≤8 datoms, ≤4 entities, ≤3 attributes | 005, 018 |
| `wal_harness.rs` | ≤4 WAL frames | 008 |
| `value_harness.rs` | All Value variants | 009, 012, 019 |
| `clock_harness.rs` | ≤16 tick() calls | 015 |

### proptest properties (`ferratomic-verify/proptest/`)

| File | Properties | INV-FERR |
|------|-----------|----------|
| `generators.rs` | arb_datom, arb_store, arb_snapshot strategies | (foundation) |
| `algebraic.rs` | merge_comm, merge_assoc, merge_idemp, monotonic, index_consistency, content_identity, schema_validation, hlc_monotonic, hlc_causal, shard_union, no_shrink, checkpoint_roundtrip | 001-005, 009, 012, 013, 015-018 |
| `federation.rs` | federated_query_correct, transport_transparency, selective_merge_preserves_local, selective_merge_only_filtered, selective_merge_idempotent, selective_merge_all_equals_full, merge_preserves_all_txids, partial_result_is_subset_of_full, migration_preserves_datom_set | 037-042 |
| `conformance.rs` | Lean-Rust conformance via manifest | (bridge) |

### Integration tests (`ferratomic-verify/integration/`)

| File | Scenarios | INV-FERR |
|------|----------|----------|
| `lifecycle.rs` | open→write→read→merge→close, snapshot_isolation, write_ordering, txn_atomicity, backpressure, substrate_swap | 006, 007, 020, 021, 024 |
| `recovery.rs` | crash→recover→verify, wal_ordering | 008, 014 |
| `observer.rs` | subscribe→write→verify delivery, monotonic epochs | 011 |
| `federation.rs` | multi-node merge→convergence, anti_entropy | 010, 022 |
| `federated_query.rs` | federated_query→correctness, selective_merge→knowledge_transfer, transport_transparency, latency_tolerance, live_migration | 037-042 |

**Acceptance**: All test files compile. All tests FAIL (no implementation yet).
The failure messages document the expected behavior.

---

## Phase 3: Type Definitions (ferratom crate)

**Goal**: Implement the leaf crate types. Types ARE propositions — they encode
invariants at the type level so the compiler verifies them.

### Types to implement

| Type | File | Encodes | INV-FERR |
|------|------|---------|----------|
| `Datom` | datom.rs | 5-tuple, Eq/Hash/Ord on all fields, Clone (immutable) | 012, 018 |
| `EntityId` | datom.rs | `[u8; 32]`, Copy, content-addressed via BLAKE3 | 012 |
| `Attribute` | datom.rs | `Arc<str>`, interned for O(1) clone + comparison | 026 |
| `Value` | datom.rs | 11-variant enum with Arc sharing for String/Bytes | 009 |
| `Op` | datom.rs | `Assert | Retract`, Copy | 018 |
| `TxId` | clock.rs | HLC (wall_time: u64, logical: u32, agent: AgentId), Ord | 015, 016 |
| `AgentId` | clock.rs | `[u8; 16]`, Copy | — |
| `HybridClock` | clock.rs | tick() monotonic even under NTP regression | 015 |
| `Frontier` | clock.rs | `HashMap<AgentId, TxId>` vector clock | 016 |
| `Schema` | schema.rs | `im::HashMap<Attribute, AttributeDef>` | 009 |
| `AttributeDef` | schema.rs | ValueType, Cardinality, resolution mode | 009, 032 |
| `FerraError` | error.rs | Exhaustive enum, 5 categories, never panics | 019 |
| `Semilattice` | traits.rs | Trait encoding comm + assoc + idemp | 001-003 |
| `ContentAddressed` | traits.rs | Trait: id determined by content hash | 012 |

### Design principles

- **Minimal cardinality**: Every type admits exactly the valid states. Invalid states
  are unrepresentable. Every invalid state your type CAN represent is a proof
  obligation shifted from compiler to runtime.
- **`#![forbid(unsafe_code)]`**: Already in lib.rs. No exceptions.
- **No `unwrap()`, no `expect()`**: Every fallible operation returns `Result<T, FerraError>`.
- **Arc sharing**: `Value::String(Arc<str>)`, `Attribute(Arc<str>)` — O(1) clone,
  deduplication across indexes.

**Acceptance**:
- `cargo check -p ferratom` passes
- `cargo clippy -p ferratom -- -D warnings` zero warnings
- Some Phase 2 tests now pass (the ones testing type properties)
- `cargo test -p ferratom` passes (type-level tests)

---

## Phase 4a: Implementation (ferratomic-core) — ONLY AFTER Phases 1-3

**DO NOT START THIS until Phases 1-3 pass their isomorphism checks.**

Implementation order within Phase 4a:

1. **Store** (store.rs): im::OrdMap indexes, apply_datoms, merge
2. **Snapshot** (snapshot.rs): Arc<StoreInner> wrapper
3. **Writer** (writer.rs): mpsc channel, group commit logic
4. **WAL** (wal.rs): frame format, append, fsync, chain hash
5. **Database** (db.rs): ArcSwap + WriterActor + lifecycle
6. **Checkpoint** (checkpoint.rs): WAL → durable storage
7. **Storage** (storage.rs): disk layout, recovery
8. **Observer** (observer.rs): DatomObserver trait, broadcast
9. **Transport** (transport.rs): LocalTransport
10. **Schema validation** (integrated into Store::transact)

**Acceptance**: ALL Phase 2 tests pass. ALL Lean proofs check. Clippy clean.

---

## Phase 4b: Prolly Tree Block Store — ONLY AFTER Phase 4a

**DO NOT START THIS until Phase 4a is complete and all core tests pass.**

Phase 4b replaces the flat `store.bin` checkpoint from Phase 4a with a content-addressed
prolly tree block store. The in-memory `im::OrdMap` representation is UNCHANGED — Phase 4b
adds an on-disk format alongside it, not instead of it.

**Why Phase 4b exists**: Phase 4a's flat checkpoint serializes the entire store on every
checkpoint: O(n) write, O(n) diff, O(n) transfer. At 100M datoms (~20GB), this is untenable.
The prolly tree provides O(d) checkpoint, O(d) diff, and O(d) transfer where d is the number
of changed datoms. This is the prerequisite for efficient federation (Phase 4c).

**Spec reference**: spec/23-ferratomic.md section 23.9 (INV-FERR-045 through INV-FERR-050,
ADR-FERR-008). Read the full section before starting implementation.

### Prerequisites from Phase 4a

Phase 4b depends on these Phase 4a artifacts being complete and tested:

- `im::OrdMap` indexes (store.rs) — the in-memory representation that prolly tree checkpoints from
- `ArcSwap` + snapshot isolation (db.rs, snapshot.rs) — lock-free reads continue unchanged
- WAL (wal.rs) — the journal extends WAL concepts to the block store layer
- Schema validation — the prolly tree stores datoms that have already passed schema validation
- BLAKE3 hashing (datom.rs) — content-addressed entity IDs reused for chunk addressing

### Implementation Order within Phase 4b

1. **Chunk** (chunk.rs): `Chunk` struct, `ChunkStore` trait, `MemoryChunkStore` impl
   - INV-FERR-045: Content addressing (addr = BLAKE3(data))
   - INV-FERR-050: Substrate independence (trait abstraction)
   - This is the foundation — everything else depends on it
   - `#[derive(Debug, Clone, PartialEq, Eq)]`, `Arc<[u8]>` for zero-copy

2. **Prolly tree construction** (prolly.rs): `build_prolly_tree`, `is_boundary`, `rolling_hash`
   - INV-FERR-046: History independence (same KV set = same tree regardless of insertion order)
   - Rolling hash boundary function on **keys only** (Dolt's improvement over Noms)
   - CDF-bounded chunk sizes: min_size, max_size, pattern_width parameters
   - Single-pass O(n) construction from sorted iterator

3. **Prolly tree reads** (prolly.rs): `read_prolly_tree`, point lookup, range scan
   - Navigate root -> internal nodes -> leaf via binary search
   - O(log_k(n)) chunk reads per point lookup
   - Deserialize leaf chunks to reconstruct key-value pairs

4. **Diff** (diff.rs): `diff()`, `DiffIterator`, `DiffEntry`
   - INV-FERR-047: O(d * log_k(n)) complexity
   - Lazy iterator: yields DiffEntry without materializing full diff
   - Recursive descent comparing content-addressed hashes, skipping identical subtrees
   - O(1) fast path when root hashes are equal

5. **Copy-on-write update** (prolly.rs): `update_prolly_tree`
   - Modify leaf, recompute hashes up to root
   - Only the modified path (1 + log_k(n) chunks) is new
   - Siblings shared with previous version

6. **Transfer** (transfer.rs): `ChunkTransfer` trait, `RecursiveTransfer`
   - INV-FERR-048: Send only chunks receiver doesn't have
   - Recursive descent with `has_chunk` pruning
   - Idempotent, resumable, monotonic (never deletes from dst)

7. **Table files** (block_store/table.rs): on-disk chunk collection with prefix-map index
   - Immutable after creation (append-only at file level)
   - Index: 8-byte address prefix map + lengths array + suffix array
   - O(log n) lookup by chunk address

8. **Journal** (block_store/journal.rs): append-only log of chunk writes + root updates
   - Tagged records: ChunkWrite (0x01), RootUpdate (0x02), Checkpoint (0x03)
   - CRC32 per record for integrity
   - Recovery: replay from last Checkpoint record

9. **Manifest** (block_store/manifest.rs): tracks table files + current root hash
   - Compare-and-swap atomic update (write temp, rename)
   - The ONLY mutable file in the block store

10. **FileChunkStore** (block_store/file.rs): filesystem-backed chunk store
    - Files named by hex(address), content = raw bytes
    - Atomic write via temp + rename
    - Content-addressing verification on read (detect corruption)

11. **Checkpoint integration** (checkpoint.rs): im::OrdMap to prolly tree checkpoint
    - INV-FERR-049: Snapshot = root hash
    - First checkpoint: O(n) full build
    - Subsequent checkpoints: O(d) incremental (dirty tracking or diff)
    - Journal compaction: fold journal chunks into new table file

12. **Recovery integration** (storage.rs): prolly tree to im::OrdMap on startup
    - Read manifest -> resolve root hash -> walk tree -> build im::OrdMap
    - Replay journal entries after last Checkpoint
    - O(n) on startup (acceptable: happens once)

13. **Garbage collection** (gc.rs): mark-and-sweep over reachable chunks
    - Mark from retained root hashes, sweep unreachable table file chunks
    - Explicit operation (not automatic) — consistent with C1
    - Rewrite table files with < 50% reachable chunks

### Key Types

```rust
// chunk.rs
pub struct Chunk { addr: Hash, data: Arc<[u8]> }
pub trait ChunkStore: Send + Sync { put_chunk, get_chunk, has_chunk, all_addrs }
pub struct MemoryChunkStore { chunks: RwLock<BTreeMap<Hash, Arc<[u8]>>> }
pub struct FileChunkStore { root_dir: PathBuf }

// prolly.rs
pub fn is_boundary(key: &[u8], pattern_width: u32) -> bool
pub fn build_prolly_tree(kvs: &BTreeMap<Key, Value>, store: &dyn ChunkStore, pw: u32) -> Result<Hash>
pub fn read_prolly_tree(root: &Hash, store: &dyn ChunkStore) -> Result<BTreeMap<Key, Value>>

// diff.rs
pub enum DiffEntry { LeftOnly { key, value }, RightOnly { key, value }, Modified { key, left_value, right_value } }
pub fn diff(root1: &Hash, root2: &Hash, store: &dyn ChunkStore) -> Result<impl Iterator<Item = Result<DiffEntry>>>

// transfer.rs
pub struct TransferResult { chunks_transferred: u64, chunks_skipped: u64, bytes_transferred: u64, root: Hash }
pub trait ChunkTransfer { fn transfer(src, dst, root) -> Result<TransferResult> }
pub struct RecursiveTransfer;

// block_store/
pub struct Snapshot { root: Hash, tx: TxId }
pub struct VersionHistory { versions: Vec<Snapshot> }
```

### Verification Plan

#### Lean Theorems (ferratomic-verify/lean/)

| File | Theorems | INV-FERR |
|------|----------|----------|
| `Ferratomic/ChunkStore.lean` | chunk_content_identity, chunk_store_idempotent | 045 |
| `Ferratomic/ProllyTree.lean` | history_independence, prolly_merge_comm | 046 |
| `Ferratomic/Snapshot.lean` | snapshot_roundtrip, snapshot_deterministic | 049 |

#### Proptest Properties (ferratomic-verify/proptest/)

| File | Properties | INV-FERR |
|------|-----------|----------|
| `chunk_store.rs` | content_addressing, deduplication, substrate_independence | 045, 050 |
| `prolly_tree.rs` | history_independence, insert_delete_identity, construction_from_sorted | 046 |
| `diff.rs` | diff_correctness, diff_empty_identical, diff_symmetry, diff_complexity_bound | 047 |
| `transfer.rs` | transfer_correctness, transfer_minimality, transfer_idempotent, transfer_resumable | 048 |
| `snapshot.rs` | snapshot_roundtrip, snapshot_identity, snapshot_distinct_for_different_data | 049 |

#### Stateright Models (ferratomic-verify/stateright/)

| Model | Properties | INV-FERR |
|-------|-----------|----------|
| `prolly_model.rs` | History independence under concurrent inserts, convergence after diff+transfer | 046, 048 |
| `block_store_model.rs` | Journal recovery correctness, GC safety (no reachable chunk collected) | 045 |

#### Integration Tests (ferratomic-verify/integration/)

| File | Scenarios | INV-FERR |
|------|----------|----------|
| `prolly_lifecycle.rs` | build -> update -> diff -> transfer -> verify roundtrip | 045-049 |
| `checkpoint.rs` | im::OrdMap -> prolly checkpoint -> recovery -> verify identical | 045, 046, 049 |
| `block_store.rs` | journal write -> compaction -> table files -> manifest -> recovery | 045 |
| `substrate_swap.rs` | MemoryChunkStore vs FileChunkStore produce identical results | 050 |
| `federation_transfer.rs` | Two stores, shared history, apply changes to one, transfer delta | 048 |

#### Kani Harnesses (ferratomic-verify/kani/)

| Harness | Bounds | INV-FERR |
|---------|--------|----------|
| `chunk_harness.rs` | <= 4 chunks, <= 256 bytes each | 045 |
| `prolly_harness.rs` | <= 8 key-value pairs, <= 4 bytes per key | 046 |
| `diff_harness.rs` | Two trees with <= 4 keys each | 047 |

### Critical Design Decisions

1. **Rolling hash operates on keys only, not (key, value) pairs.** This is Dolt's improvement
   over Noms. Consequence: updating a value never changes chunk boundaries, so the structural
   impact of a point mutation is O(1) leaf chunks + O(log_k(n)) internal node updates. If we
   hashed (key, value), changing a value could cascade boundary changes across the entire tree.

2. **CDF-bounded chunk sizes.** Pure rolling hash gives a geometric distribution (high variance).
   The CDF approach enforces min_size and max_size bounds while preserving history independence.
   The `entries_since_last_boundary` counter is a function of the sorted key sequence, not
   insertion order.

3. **im::OrdMap remains the in-memory representation.** Phase 4b does NOT replace im::OrdMap
   with the prolly tree for in-memory queries. The prolly tree is the on-disk checkpoint
   format. This means:
   - Read latency is unchanged (O(log n) in-memory, not O(log_k(n)) chunk reads)
   - Memory usage is unchanged (full store in im::OrdMap)
   - Only the checkpoint path changes (O(d) prolly vs O(n) flat)

4. **The journal extends WAL concepts to chunk writes.** WAL (INV-FERR-008) guarantees
   durability via write-ahead logging. The journal applies the same principle to chunk
   writes: chunks are logged in the journal before being compacted into table files.
   Recovery replays the journal to reconstruct any chunks not yet in table files.

5. **Garbage collection is explicit, not automatic.** Consistent with C1 (append-only during
   normal operation). The application decides which root hashes to retain. GC is a separate
   lifecycle event that rewrites table files to reclaim space from unreachable chunks.

6. **S3ChunkStore uses asupersync-tokio-compat for HTTP.** Consistent with ADR-FERR-002
   (asupersync-first). Any tokio-only HTTP client is wrapped in an `asupersync-tokio-compat`
   adapter module. Core `ChunkStore` trait methods take `&Cx` for cancel-awareness.

### Performance Targets

| Metric | Target | Rationale |
|--------|--------|-----------|
| Checkpoint (100 changes, 100M store) | < 100ms | O(d * log_k(n)) = ~400 chunk writes at ~4KB each |
| Checkpoint (full initial build, 100M store) | < 60s | O(n) single pass, ~5M chunks at ~4KB each |
| Diff (100 changes, 100M store) | < 50ms | O(d * log_k(n)) = ~300 chunk reads |
| Transfer (100 changes, 100M store) | < 200ms LAN | ~300 chunks x ~4KB = ~1.2MB |
| GC (100M store, 10% unreachable) | < 5min | Rewrite affected table files |
| Recovery (100M store) | < 30s | Read all chunks, rebuild im::OrdMap |
| Chunk put (FileChunkStore) | < 1ms p99 | Single file write + rename |
| Chunk get (FileChunkStore) | < 0.5ms p99 | Single file read |

### File Layout

```
ferratomic-core/src/
  chunk.rs          # Chunk, ChunkStore trait, MemoryChunkStore
  prolly.rs         # Prolly tree construction, reads, updates, rolling hash
  diff.rs           # DiffIterator, DiffEntry, diff()
  transfer.rs       # ChunkTransfer trait, RecursiveTransfer
  block_store/
    mod.rs          # BlockStore (combines table files, journal, manifest)
    table.rs        # Table file format: data + index sections
    journal.rs      # Append-only log: ChunkWrite, RootUpdate, Checkpoint records
    manifest.rs     # Manifest: table file list + current root hash
    file.rs         # FileChunkStore implementation
    gc.rs           # Garbage collection: mark-and-sweep
  snapshot.rs       # Snapshot (root hash + TxId), VersionHistory
```

### Relationship to Phase 4c (Federation)

Phase 4b is the prerequisite for efficient federation (Phase 4c). Specifically:

- **Anti-entropy** (INV-FERR-022): Prolly tree diff IS Merkle anti-entropy. No separate
  data structure needed. `diff(root_local, root_remote)` identifies missing datoms in
  O(d * log_k(n)), and `transfer()` sends only missing chunks.

- **Selective merge** (INV-FERR-039): With attribute-prefixed prolly tree keys, selective
  merge can navigate directly to relevant subtrees instead of filtering after full transfer.

- **Transport** (INV-FERR-038): The `ChunkStore` trait + `ChunkTransfer` trait provide
  the data plane for federation transports. A `RemoteChunkStore` implementation wraps
  the transport layer, making remote chunk access transparent.

Phase 4c's federation implementation builds ON TOP of Phase 4b's block store. Without
prolly trees, federation requires O(n) full-store transfers. With prolly trees, federation
transfers are O(d) — proportional to changes, not store size. This is the difference
between "federation that works" and "federation that scales."

### Acceptance Criteria

- ALL Phase 4b test suites pass (proptest, stateright, kani, integration)
- ALL Phase 2 + Phase 4a tests still pass (no regressions)
- `build_prolly_tree(kvs)` followed by `read_prolly_tree(root)` roundtrips perfectly for all inputs
- `diff(root1, root2)` produces exactly the symmetric difference of the key-value sets
- `transfer(src, dst, root)` followed by `read_prolly_tree(root, dst)` produces identical results
- History independence: same KV set from different insertion orders = same root hash
- Substrate independence: MemoryChunkStore and FileChunkStore produce identical root hashes and chunks
- Checkpoint integration: im::OrdMap -> prolly -> recovery -> im::OrdMap roundtrip is lossless
- Lean theorems type-check (ChunkStore.lean, ProllyTree.lean, Snapshot.lean)
- Clippy clean, no `unwrap()`, `#![forbid(unsafe_code)]`

---

## Phase 4c: Federation Implementation — ONLY AFTER Phase 4a

**DO NOT START THIS until Phase 4a is complete and all core tests pass.**

Federation (spec §23.8, INV-FERR-037 through INV-FERR-044) builds on top of the
core store, snapshot, merge, and query infrastructure from Phase 4a.

Implementation order within Phase 4c:

1. **Transport trait** (transport.rs): `Transport` trait definition, `LocalTransport` impl
2. **Federation struct** (federation.rs): `Federation`, `StoreHandle`, `StoreId`, `FederationConfig`
3. **Federated query** (federation.rs): `federated_query` with CALM-correct fan-out for monotonic queries, materialization for non-monotonic
4. **DatomFilter** (filter.rs): `DatomFilter` enum with `All`, `AttributeNamespace`, `Entities`, `FromAgents`, `AfterEpoch`, `And`, `Or`, `Not`, `Custom`
5. **Selective merge** (federation.rs): `selective_merge` with schema compatibility check (INV-FERR-043)
6. **FederatedResult** (federation.rs): Per-store `StoreResponse` metadata, partial result handling (INV-FERR-041)
7. **Live migration** (migration.rs): `Migration` state machine — WAL streaming, catchup, atomic swap, drain, decommission (INV-FERR-042)
8. **Additional transports** (transport/): `UnixSocketTransport`, `TcpTransport` (QUIC and gRPC deferred to Stage 2)

**Acceptance**:
- ALL Phase 2 federation tests pass (proptest, stateright, integration)
- Transport transparency verified: same query, same store, `LocalTransport` vs `LoopbackTransport` produce identical results
- Selective merge preserves CRDT properties (monotonicity, idempotency, no invention)
- Merge provenance preserved through all paths (INV-FERR-040)
- Partial results correctly flagged when stores time out (INV-FERR-041)
- Clippy clean, no `unwrap()`, `#![forbid(unsafe_code)]`

---

## Execution Protocol

For each task: select highest-impact unblocked → implement → verify → observe.

**Quality standard**: `ms` "rust-formal-engineering". Every type encodes an invariant.
Every function proves a property. No `unwrap()`. No panics. Result everywhere.
`#![forbid(unsafe_code)]`. Zero clippy warnings.

**Build environment**: `CARGO_TARGET_DIR=/data/cargo-target` (real disk, not tmpfs).

**Subagent orchestration**: Parallel agents for disjoint crates/files.
Agents MUST NOT run cargo commands — orchestrator runs once after all agents complete.

---

## Success Criteria

1. `lake build` — all Lean theorems type-check
2. `cargo test --workspace` — all tests pass (Phase 2 red → green after Phase 4a)
3. `cargo clippy --workspace -- -D warnings` — zero warnings
4. Every INV-FERR has: Lean theorem + Stateright/Kani model + proptest + integration test
5. Conformance manifest: CI verifies spec ↔ algebra ↔ test isomorphism
6. `#![forbid(unsafe_code)]` in all 4 crates
7. No `unwrap()` or `expect()` in production code

---

## Hard Constraints

- **C1**: Append-only store. Never delete or mutate datoms.
- **C2**: Content-addressed identity. EntityId = BLAKE3(content).
- **C4**: CRDT merge = set union. Commutative, associative, idempotent.
- **INV-FERR-023**: `#![forbid(unsafe_code)]` in all crates.
- **NEG-FERR-001**: No panics in production code.
- **NEG-FERR-002**: No unsafe code.

---

## Stop Conditions

Stop and escalate to the user if:
- Lean proof doesn't type-check after reasonable effort (may indicate spec error)
- Stateright model state space explosion (may need bound reduction)
- im::OrdMap performance issue at any scale (trigger IndexBackend fallback analysis)
- Spec ambiguity in any INV-FERR (ask, don't assume)
- Any C1/C2/C4 violation in implementation
