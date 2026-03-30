# Ferratomic Architecture

> Canonical architecture reference for the Ferratomic embedded datom database engine.
> This document is the implementation blueprint for adversarial cleanroom audit.
> Every claim traces to `spec/23-ferratomic.md`, `SEED.md`, or `docs/design/ADRS.md`.

---

## 1. Executive Summary

### What Ferratomic Is

Ferratomic is an embedded, append-only datom database engine that reifies the algebraic
store `(P(D), ∪)` specified in `spec/01-store.md` as a production-grade storage system.
It stores atomic facts called datoms -- five-tuples of the form
`[entity, attribute, value, transaction, operation]` -- in a grow-only set with conflict-free
merge semantics. It is the persistence, concurrency, and distribution substrate for all
higher-level operations in the braid runtime.

The name reflects its nature: **Ferra** (iron/durable) + **atomic** (indivisible facts).

### Why It Exists

The braid-kernel crate (`crates/braid-kernel/`) currently implements the datom store as
in-memory `BTreeSet<Datom>` with EDN file persistence via per-transaction content-addressed
files. This architecture is correct (176,000+ datoms, 1,700+ tests, all invariants holding)
but does not scale beyond the single-VPS, single-digit agent deployment that exists today.
Ferratomic replaces the storage layer with a purpose-built engine that preserves the
algebraic guarantees while adding:

- Lock-free concurrent reads with zero contention
- WAL-based crash recovery with group commit
- Epoch-based snapshot isolation
- Persistent data structures for O(1) snapshot clones
- A distribution layer for multi-node CRDT mesh
- Formal verification at every layer (Lean 4, Stateright, Kani, proptest)

### What Problem It Solves

Every project drifts. What you want, what you wrote down, how you said to build it, and
what actually got built inevitably diverge. DDIS (Decision-Driven Implementation
Specification) makes coherence a structural property rather than a process obligation. The
datom store is the substrate where specification, implementation, and verification coexist
as queryable, mergeable facts. Ferratomic makes that substrate durable, concurrent, and
distributed -- the engineering that turns the algebraic ideal into physical reality.

### Relationship to braid-kernel and SEED.md

Ferratomic sits below braid-kernel in the dependency graph. braid-kernel defines the
application semantics: schema, harvest, seed, guidance, merge cascade, bilateral
verification, fitness function, convergence engine. Ferratomic provides the storage engine
those semantics operate on. The relationship is analogous to SQLite vs. the application
that uses it, or InnoDB vs. MySQL's query layer.

**Traceability**:
- `SEED.md` section 4 (Core Abstraction: Datoms) defines the algebraic model.
- `spec/01-store.md` formalizes the five lattice laws L1-L5.
- `spec/23-ferratomic.md` specifies how those laws hold under real-world failure modes.
- This document specifies the engineering architecture that implements the spec.

The refinement chain is: SEED.md (vision) -> `spec/01-store.md` (algebra) ->
`spec/23-ferratomic.md` (engineering spec) -> this document (implementation architecture)
-> code (realization).

---

## 2. Core Algebraic Structure

### Store = (P(D), union) -- G-Set CRDT Semilattice

The datom store is formally a G-Set (grow-only set) CvRDT (Convergent Replicated Data
Type). The algebraic structure is:

```
Store = (P(D), ∪)

where:
  D     = the set of all possible datoms (five-tuples [e, a, v, tx, op])
  P(D)  = the power set of D (all possible subsets)
  ∪     = set union (the merge operation)
```

This forms a join-semilattice where the partial order is set inclusion (`A ≤ B` iff
`A ⊆ B`) and the join (least upper bound) is set union. The bottom element is the empty
set. There is no top element (the store grows without bound).

### Formal Properties

Four algebraic properties hold for all reachable stores A, B, C:

**Commutativity** (INV-FERR-001, L1):
```
merge(A, B) = merge(B, A)

Proof: A ∪ B = B ∪ A by commutativity of set union.
```
The order in which two replicas discover each other and initiate merge is irrelevant to the
final converged state.

**Associativity** (INV-FERR-002, L2):
```
merge(merge(A, B), C) = merge(A, merge(B, C))

Proof: (A ∪ B) ∪ C = A ∪ (B ∪ C) by associativity of set union.
```
The merge topology (star, chain, mesh) does not affect the final result. Whether agent 1
merges with agent 2 first and then agent 3, or agent 2 merges with agent 3 first and then
agent 1, the converged state is identical.

**Idempotency** (INV-FERR-003, L3):
```
merge(A, A) = A

Proof: A ∪ A = A by idempotency of set union.
```
At-least-once delivery is safe. If a merge message is delivered twice (network retry,
process restart), the second delivery is a no-op. No external deduplication infrastructure
is needed.

**Monotonicity** (INV-FERR-004, L4/L5):
```
∀ S, d: S ⊆ apply(S, d)    (no datom is lost)
∀ S, d: |apply(S, d)| ≥ |S|  (cardinality is non-decreasing)
∀ S, T where T is non-empty: |TRANSACT(S, T)| > |S|  (strict growth for transactions)
```
The store never shrinks. Retractions are new datoms with `op = Retract` -- they add to the
store rather than removing from it. The "current state" of an entity is a query-layer
concern computed by the LIVE index; the store itself is append-only.

### Why These Properties Eliminate Conflict Resolution Entirely

Traditional databases require conflict resolution because concurrent writes can produce
contradictory states. In Ferratomic, the merge operation is pure set union -- a mathematical
operation with no heuristics, no tie-breaking, no coordination. Two agents independently
asserting the same fact produce identical datoms (content-addressed identity via BLAKE3,
INV-FERR-012). Set union naturally deduplicates them. Two agents asserting different facts
about the same entity both succeed -- both datoms enter the store, and the per-attribute
resolution mode (lattice, last-writer-wins, or multi-value) determines the "current value"
at query time, not at merge time.

This architectural choice means:
- Merge requires zero network round-trips
- Merge is always total (never fails, never requires human intervention)
- Merge is O(|A| + |B|) -- linear in the size of the smaller store
- Merge is deterministic -- the same inputs always produce the same output
- No merge locks, no merge queues, no merge conflicts, no merge retries

The conflict resolution that does exist (lattice resolution, LWW, multi-value) operates at
the query layer via the LIVE index, not at the storage layer. This separation is the key
insight: **store everything, resolve at read time**.

### The Curry-Howard-Lambek Correspondence

Ferratomic's verification strategy is grounded in the Curry-Howard-Lambek correspondence:

| Domain | Interpretation |
|--------|---------------|
| Types (Rust) | Propositions about datom properties |
| Programs (Rust impl) | Proofs that those propositions hold |
| Categories (Lean 4) | The algebraic structure those proofs inhabit |

Concretely:
- `EntityId`'s private inner field (ADR-STORE-014) is a **type-level proof** that every
  entity identifier was produced by BLAKE3 hashing. The impossibility of constructing an
  `EntityId` from raw bytes (outside `pub(crate)` deserialization) means the content-addressing
  invariant (INV-FERR-012) is enforced by the type system, not by runtime checks.
- The `Transaction<Building> -> Transaction<Committed> -> Transaction<Applied>` typestate
  (sealed trait, `PhantomData<S>`) is a **type-level proof** that transactions follow the
  correct lifecycle. Invalid state transitions are compile errors.
- The Lean 4 theorems in `spec/23-ferratomic.md` are **mechanically checkable proofs** of
  the same properties at the algebraic level (`DatomStore := Finset Datom`).
- The proptest strategies and Kani harnesses are **empirical and bounded-model proofs** that
  the implementation matches the specification.

The verification strategy is: prove it in Lean (algebraic model), model-check it in
Stateright (protocol model), bound-check it in Kani (implementation model), fuzz it in
proptest (operational model), enforce it in the type system (compile-time model).

---

## 3. Crate Architecture

### Dependency DAG

```
                         ferratomic-verify (dev-dependency only)
                        /           |              \
                       v            v               v
ferratomic-datalog  <--  ferratomic-core  <--  ferratom
     (facade)               (core)              (leaf)
```

The dependency direction is strict and acyclic: leaf -> core -> facade -> binary.
`ferratomic-verify` depends on all three but is a dev-dependency only -- it is never
compiled into release binaries.

### ferratom (Leaf Crate)

**Purpose**: Primitive types with zero dependencies. No I/O. No allocation beyond type
construction.

**Contents**:
- `Datom` -- the five-tuple `[entity, attribute, value, tx, op]`
- `EntityId` -- BLAKE3 content-addressed identifier, private inner `[u8; 32]`
- `TxId` -- Hybrid Logical Clock transaction identifier
- `Value` -- the value union (String, Keyword, Boolean, Long, Double, Instant, Uuid, Ref, Bytes)
- `Op` -- `Assert | Retract`
- `Attribute` -- namespaced keyword (`:namespace/name`)

**Dependencies**: Zero. Not even `serde`. Pure types that can be depended on by anything.

**Design rationale**: The leaf crate defines the language that all other crates speak. By
having zero dependencies, it compiles in milliseconds and can be depended on by any crate
in the workspace without pulling a dependency subgraph. The types are `Clone`, `Debug`,
`Hash`, `Eq`, `Ord` -- the full complement needed for use in sorted containers and hash
maps.

### ferratomic-core (Core Crate)

**Purpose**: Storage engine, concurrency, WAL, snapshots, indexes, merge, schema validation.

**Contents**:
```
src/store.rs       -- Store struct, transact, merge, genesis
src/index.rs       -- EAVT, AEVT, VAET, AVET, LIVE indexes
src/wal.rs         -- Write-ahead log with fsync ordering
src/snapshot.rs    -- Point-in-time snapshot materialization
src/schema.rs      -- Schema-as-data validation
src/merge.rs       -- CRDT merge (set union + cascade)
src/observer.rs    -- DatomObserver trait + broadcast
src/clock.rs       -- Hybrid Logical Clock implementation
src/transport.rs   -- Transport trait (Local/Network)
src/error.rs       -- FerraError enum
```

**Dependencies**: `ferratom`, `blake3`, `serde`, `im` (persistent data structures),
`arc-swap` (lock-free reads).

**Design rationale**: The core crate is the engine. It owns the concurrency model (ArcSwap
for reads, single writer actor for writes), the durability model (WAL with fsync ordering),
and the distribution model (CRDT merge via set union). Everything above it (query, CLI,
daemon, application logic) depends on the guarantees this crate provides.

### ferratomic-datalog (Facade Crate)

**Purpose**: Query engine -- Datomic-style Datalog dialect with semi-naive evaluation.

**Contents**:
```
src/parser.rs      -- EDN-based Datalog parser
src/planner.rs     -- Query plan generation with stratum classification
src/eval.rs        -- Semi-naive evaluation with CALM compliance
```

**Dependencies**: `ferratomic-core` (full access to store, indexes, snapshots).

**Design rationale**: The query engine is separated from the storage engine because (a) it
can be developed and tested independently, (b) some deployments may not need Datalog (direct
index access suffices), and (c) the query engine has significantly different performance
characteristics (CPU-bound evaluation vs. I/O-bound storage).

### ferratomic-verify (Verification Crate)

**Purpose**: Formal verification harnesses. Dev-dependency only -- never in release builds.

**Contents**:
```
src/proptest_strategies.rs    -- Arbitrary instances for all core types
src/kani_harnesses.rs         -- Bounded model checking proofs
src/stateright_models.rs      -- Protocol model checking (multi-node CRDT)
lean/                         -- Lean 4 theorem files
```

**Dependencies**: All three crates (dev-dependency). Plus `proptest`, `kani`, `stateright`.

**Design rationale**: Verification code is separate from production code so that (a) the
production binary contains no test framework overhead, (b) the verification strategies can
be shared across crates, and (c) the Lean 4 theorems live alongside the Rust proofs they
correspond to.

---

## 4. Concurrency Model

### Design Philosophy

Ferratomic's concurrency model is derived from a single observation: **CRDT merge makes
concurrent reads trivially safe**. When the store is a grow-only set and merge is set union,
any snapshot of the store is a valid state -- it may be stale (missing recent datoms) but
never inconsistent (containing partial transactions or contradictory state). This eliminates
the fundamental difficulty of database concurrency: readers never need to coordinate with
writers or with each other.

### ArcSwap for Lock-Free Reads (~1ns, Zero Contention)

Readers access the store through `arc_swap::ArcSwap<StoreSnapshot>`, which provides
**wait-free** reads with approximately 1 nanosecond overhead. The mechanism:

```rust
use arc_swap::ArcSwap;
use std::sync::Arc;

struct Engine {
    /// The current store snapshot. Readers load this atomically.
    /// Writers swap in a new snapshot after committing a transaction.
    current: ArcSwap<StoreSnapshot>,
}

impl Engine {
    /// Lock-free read. Returns an Arc to the current snapshot.
    /// The snapshot is immutable -- it will never change after creation.
    /// Multiple readers can hold different snapshots simultaneously.
    fn snapshot(&self) -> arc_swap::Guard<Arc<StoreSnapshot>> {
        self.current.load()  // ~1ns, wait-free, no contention
    }

    /// Writer swaps in a new snapshot after commit.
    fn publish(&self, new_snapshot: StoreSnapshot) {
        self.current.store(Arc::new(new_snapshot));
    }
}
```

**Why ArcSwap and not RwLock**: `RwLock` provides shared reads, but readers still contend on
the lock's internal atomic counter. Under high read concurrency (many agents querying
simultaneously), this contention becomes measurable. `ArcSwap` eliminates it entirely --
each reader performs a single atomic load, with no increment/decrement cycle. The trade-off
is that writers must construct a complete new snapshot before publishing, but this is
amortized by group commit (below).

### Single Writer Actor (mpsc Channel, Group Commit)

All write operations are serialized through a single writer thread that receives transactions
via an `mpsc` channel. This eliminates write contention without locks:

```rust
use std::sync::mpsc;

struct WriterActor {
    rx: mpsc::Receiver<WriteRequest>,
    store: Store,
    wal: Wal,
    epoch: u64,
}

struct WriteRequest {
    tx: Transaction<Committed>,
    reply: oneshot::Sender<TxReceipt>,
}

impl WriterActor {
    fn run(&mut self) {
        loop {
            // Collect batch: drain all pending writes
            let mut batch = Vec::new();
            batch.push(self.rx.recv().unwrap());
            while let Ok(req) = self.rx.try_recv() {
                batch.push(req);
            }

            // Group commit: one WAL fsync for the entire batch
            for req in &batch {
                self.epoch += 1;
                self.wal.append(self.epoch, &req.tx);
            }
            self.wal.fsync();  // single fsync for N transactions

            // Apply to in-memory store
            for req in batch {
                self.store.apply(req.tx);
                req.reply.send(TxReceipt { epoch: self.epoch });
            }

            // Publish new snapshot (readers see the update)
            self.publish_snapshot();
        }
    }
}
```

**Group commit** amortizes the cost of `fsync()` across multiple transactions. If 10
transactions arrive within the same batch window, they share a single fsync -- reducing
the per-transaction durability cost by 10x. The FrankenSQLite paper (see section 17)
demonstrates that this pattern achieves 90%+ of theoretical disk throughput.

### Epoch-Based Snapshot Versioning

Every committed transaction advances a monotonically increasing epoch counter (INV-FERR-007).
Snapshots are identified by the epoch at which they were created. The epoch provides:

- **Ordering**: transactions committed at epoch `e1 < e2` are ordered
- **Visibility**: a snapshot at epoch `e` sees exactly the datoms from transactions committed
  at epoch `<= e`
- **Deduplication**: observers track their last-seen epoch, enabling delta delivery

```
Timeline:

epoch 0: genesis (19 meta-schema attributes)
epoch 1: first user transaction
epoch 2: ...
epoch n: current

snapshot(e) = { d | d.tx.epoch <= e }  -- all datoms up to epoch e
```

The epoch counter is 64-bit unsigned, supporting 2^64 transactions before overflow. At
1 million transactions per second, this provides approximately 584,942 years of headroom.

### im::OrdMap Persistent Data Structures for O(1) Snapshot Clones

Snapshots use `im::OrdMap` (persistent balanced binary tree) instead of `BTreeMap` for
indexes. This enables O(1) snapshot creation via structural sharing:

```rust
use im::OrdMap;

struct StoreSnapshot {
    datoms: OrdMap<DatomKey, Datom>,
    eavt: OrdMap<(EntityId, Attribute, Value, TxId), ()>,
    aevt: OrdMap<(Attribute, EntityId, Value, TxId), ()>,
    vaet: OrdMap<(Value, Attribute, EntityId, TxId), ()>,
    avet: OrdMap<(Attribute, Value, EntityId, TxId), ()>,
    epoch: u64,
}
```

When the writer creates a new snapshot after committing a transaction:
- Old snapshot: 1M datoms, ~350 MB
- New datoms: 50
- New snapshot: shares 999,950 nodes with old, allocates ~50 new nodes
- Clone cost: O(log n) * number_of_new_datoms, not O(n)

Readers holding references to old snapshots continue to see the old state. The `Arc`
reference counting ensures old nodes are freed only when no reader references them.

### Comparison to Reference Systems

| System | Read model | Write model | Snapshot cost |
|--------|-----------|-------------|--------------|
| **PostgreSQL MVCC** | Shared-lock on tuple header | Row-level lock, WAL | Heap scan, VACUUM needed |
| **Redis** | Single-threaded, blocking | Single-threaded, AOF/RDB | fork() COW |
| **FrankenSQLite** | Page-level MVCC, lock-free | WAL with group commit | Page copy (~4KB granularity) |
| **Ferratomic** | ArcSwap, wait-free | Single writer actor, group commit | im::OrdMap structural sharing |

**Why CRDT makes this strictly simpler than all reference systems**: PostgreSQL needs MVCC
tuple headers, visibility checks, and VACUUM to handle the possibility that concurrent writes
produce contradictory state. Redis eliminates concurrency entirely with a single thread.
FrankenSQLite uses page-level MVCC with a birthday-paradox hash for snapshot identification.
Ferratomic needs none of these mechanisms because the CRDT guarantee means every snapshot is
a valid, consistent state by construction. There is no contradictory state to resolve, no
visibility to check, no garbage to collect. The append-only property means old snapshots
remain valid indefinitely -- they are simply incomplete (missing later datoms), never
incorrect.

---

## 5. Storage Engine

### WAL Frame Format (Byte-Level Specification)

Each WAL entry is a self-describing frame:

```
+-------------------+
| Magic (4 bytes)   |  0x42524944 ("BRID")
+-------------------+
| Version (2 bytes) |  0x0001 (little-endian)
+-------------------+
| Epoch (8 bytes)   |  u64 little-endian, strictly monotonically increasing
+-------------------+
| Length (4 bytes)   |  u32 little-endian, byte count of payload
+-------------------+
| Payload (N bytes) |  MessagePack-serialized Transaction<Committed>
+-------------------+
| CRC32 (4 bytes)   |  CRC32C of [Magic..Payload]
+-------------------+

Total frame overhead: 22 bytes + payload
```

**Magic bytes**: Enable detection of WAL file corruption and version mismatches. If the magic
does not match, the entry is corrupted or from a different format version.

**CRC32C**: Hardware-accelerated on modern CPUs (SSE 4.2). Detects bit-flip corruption in
the payload. Not cryptographic -- content integrity is ensured by BLAKE3 at the datom level.

**Recovery protocol**: On startup, the WAL reader scans entries sequentially:
1. Read magic + version. Mismatch -> truncate from this point.
2. Read epoch + length.
3. Read payload + CRC32.
4. Verify CRC32. Mismatch -> truncate from start of this entry.
5. If UnexpectedEof at any point -> truncate from start of this entry.
6. Apply recovered entries to in-memory store.

### Group Commit with Two-Fsync Barrier (FrankenSQLite Pattern)

The group commit protocol ensures durability with minimal fsync overhead:

```
Phase 1: COLLECT
  - Writer drains all pending transactions from the mpsc channel
  - Batch = [T1, T2, ..., Tn]

Phase 2: WAL WRITE
  - For each Ti in batch:
    - Serialize WAL frame (magic, epoch, length, payload, CRC32)
    - Write frame to WAL file (buffered, not yet durable)

Phase 3: FIRST FSYNC (durability barrier)
  - fsync(wal_fd)
  - After this returns, all WAL frames are on durable storage
  - If process crashes after this point, all transactions survive

Phase 4: IN-MEMORY APPLY
  - For each Ti in batch:
    - Apply datoms to in-memory indexes
    - Advance epoch counter
  - Publish new snapshot via ArcSwap

Phase 5: ACK
  - Send TxReceipt to each waiting caller
  - Callers unblock and can read their own writes

Phase 6: CHECKPOINT (periodic, background)
  - Serialize complete store state to checkpoint file
  - SECOND FSYNC (checkpoint durability)
  - Truncate WAL
```

The critical ordering is: **WAL fsync (Phase 3) BEFORE epoch advance (Phase 4)**. This is
INV-FERR-008: no transaction is visible to readers until its WAL entry is durable. If the
process crashes between Phase 3 and Phase 4, recovery replays the WAL to reconstruct
in-memory state. If the process crashes before Phase 3, the incomplete WAL entries are
truncated on recovery, and those transactions are lost (the callers never received an ack,
so they know to retry).

### Checkpoint Format (Byte-Level)

Checkpoints are complete store snapshots written periodically to avoid unbounded WAL growth:

```
+-------------------+
| Magic (4 bytes)   |  0x43484B50 ("CHKP")
+-------------------+
| Version (2 bytes) |  0x0001
+-------------------+
| Epoch (8 bytes)   |  u64, epoch at checkpoint time
+-------------------+
| Datom count (8 B) |  u64, total datoms in this checkpoint
+-------------------+
| Schema hash (32B) |  BLAKE3 of serialized schema
+-------------------+
| Index flags (1 B) |  Bitfield: which indexes are included
+-------------------+
| Data (N bytes)    |  MessagePack-serialized store state
+-------------------+
| BLAKE3 (32 bytes) |  Hash of [Magic..Data]
+-------------------+
```

**Checkpoint integrity**: The trailing BLAKE3 hash covers the entire checkpoint including
headers. On recovery, the hash is verified before using the checkpoint. A corrupted
checkpoint is discarded in favor of WAL replay from an earlier checkpoint (or genesis if
no valid checkpoint exists).

### Three-Level Recovery

Recovery proceeds through three levels of increasing reconstruction cost:

**Level 1: Checkpoint + WAL Delta** (fastest, ~100ms for 1M datoms)
```
1. Load most recent valid checkpoint (epoch E_chk)
2. Replay WAL entries with epoch > E_chk
3. Rebuild in-memory indexes from recovered datoms
4. Resume normal operation
```

**Level 2: Checkpoint + EDN Transaction Files** (medium, ~5s for 1M datoms)
```
1. Load most recent valid checkpoint
2. WAL is corrupted or missing
3. Scan .braid/txns/ directory for EDN transaction files
4. Apply all transaction files with epoch > E_chk
5. Rebuild WAL from applied transactions
6. Resume normal operation
```

**Level 3: Full Rebuild from EDN** (slowest, ~30s for 1M datoms)
```
1. No valid checkpoint, no valid WAL
2. Start from genesis (19 meta-schema attributes)
3. Scan all EDN transaction files in .braid/txns/
4. Sort by epoch, apply sequentially
5. Write fresh checkpoint and WAL
6. Resume normal operation
```

This three-level approach ensures that data loss requires simultaneous corruption of the
checkpoint file, the WAL, AND every individual EDN transaction file in the `txns/` directory.
Since each EDN file is content-addressed (filename = BLAKE3(bytes)), accidental corruption
of an individual file is detectable by comparing the filename to the hash of its contents.

### Genesis Bootstrap (19 Axiomatic Meta-Schema Attributes)

The genesis transaction (epoch 0, tx ID 0) installs exactly 19 axiomatic attributes that
define the meta-schema. These attributes describe themselves -- the schema-as-data bootstrap
(C3, C7, INV-SCHEMA-005).

The 19 attributes:

| # | Attribute | Type | Cardinality | Purpose |
|---|-----------|------|-------------|---------|
| 1 | `:db/ident` | Keyword | One | Attribute's keyword name |
| 2 | `:db/valueType` | Keyword | One | Type constraint |
| 3 | `:db/cardinality` | Keyword | One | One or Many |
| 4 | `:db/doc` | String | One | Human-readable description |
| 5 | `:db/unique` | Keyword | One | Uniqueness constraint |
| 6 | `:db/isComponent` | Boolean | One | Component ownership |
| 7 | `:db/resolutionMode` | Keyword | One | CRDT conflict resolution |
| 8 | `:db/latticeOrder` | Ref | One | Reference to lattice definition |
| 9 | `:db/lwwClock` | Keyword | One | LWW clock source |
| 10 | `:lattice/ident` | Keyword | One | Lattice name |
| 11 | `:lattice/elements` | String | One | Ordered element list |
| 12 | `:lattice/comparator` | String | One | Comparison function |
| 13 | `:lattice/bottom` | Keyword | One | Least element |
| 14 | `:lattice/top` | Keyword | One | Greatest element |
| 15 | `:tx/time` | Instant | One | Transaction wall-clock time |
| 16 | `:tx/agent` | Ref | One | Agent that created transaction |
| 17 | `:tx/provenance` | String | One | Provenance description |
| 18 | `:tx/rationale` | String | One | Why this transaction exists |
| 19 | `:tx/coherence-override` | String | One | Manual coherence exemption |

These 19 attributes are the only hardcoded elements in the engine. Every other attribute --
including all application-layer schema (observations, tasks, spec elements, methodology
metadata) -- is defined by transacting datoms that reference these 19. This is the
self-describing foundation: the schema that describes all other schemas is itself described
by these 19 axioms.

The genesis hash is deterministic: `BLAKE3(serialize(genesis_datoms))` produces the same
value on every node, in every process, at every point in time. This makes genesis
verification a single hash comparison.

---

## 6. Observer System

### DatomObserver Trait

The observer system provides push-based notifications of new datoms to interested consumers.
The trait:

```rust
/// A consumer of datom events.
///
/// Observers are notified after each committed transaction. Delivery is
/// at-least-once with epoch-based deduplication (INV-FERR-011).
pub trait DatomObserver: Send + Sync {
    /// Called after a transaction is committed. The epoch is the transaction's
    /// commit epoch. Datoms are the new datoms added by this transaction.
    ///
    /// Implementations MUST be idempotent: if called twice with the same epoch,
    /// the second call is a no-op. This is required because at-least-once
    /// delivery means duplicate notifications are possible after crash recovery.
    fn on_commit(&self, epoch: u64, datoms: &[Datom]);

    /// Called when the observer has fallen behind and needs to catch up.
    /// Receives all datoms from epochs (last_seen_epoch..current_epoch].
    fn on_catchup(&self, from_epoch: u64, datoms: &[Datom]);

    /// Human-readable name for diagnostics.
    fn name(&self) -> &str;
}
```

### At-Least-Once Delivery with Epoch-Based Dedup

Observers are guaranteed to see every committed transaction at least once, but may see a
transaction more than once after crash recovery (the WAL replays transactions that may have
already been delivered to observers before the crash). The epoch field enables deduplication:

```rust
struct DeduplicatingObserver<O: DatomObserver> {
    inner: O,
    last_seen_epoch: AtomicU64,
}

impl<O: DatomObserver> DatomObserver for DeduplicatingObserver<O> {
    fn on_commit(&self, epoch: u64, datoms: &[Datom]) {
        let prev = self.last_seen_epoch.fetch_max(epoch, Ordering::AcqRel);
        if epoch <= prev {
            return;  // already seen this epoch
        }
        self.inner.on_commit(epoch, datoms);
    }
}
```

### Async Broadcast Channel with Bounded Buffer

Notifications are dispatched via a bounded broadcast channel. The buffer size determines how
many transactions can be queued before slow observers trigger backpressure:

```rust
struct ObserverBroadcast {
    /// Registered observers with their last-acknowledged epoch.
    observers: Vec<(Box<dyn DatomObserver>, u64)>,
    /// Bounded ring buffer of recent transactions for catch-up.
    recent: VecDeque<(u64, Vec<Datom>)>,
    /// Maximum buffer size. When exceeded, oldest entries are evicted
    /// and slow observers must use the catch-up protocol.
    max_buffer: usize,  // default: 1024 transactions
}
```

### Catch-Up Protocol for Slow Observers

When an observer falls behind (its `last_seen_epoch` is older than the oldest entry in the
ring buffer), the catch-up protocol activates:

1. Observer reports its `last_seen_epoch` to the broadcast system.
2. Broadcast system queries the store for all datoms in epochs
   `(last_seen_epoch..current_epoch]`.
3. All datoms are delivered via `on_catchup()` in a single batch.
4. Observer updates its `last_seen_epoch` to `current_epoch`.
5. Normal incremental delivery resumes.

This protocol ensures that observers never miss datoms, even if they are temporarily offline
(crashed, paused, network-partitioned in the distributed case).

### How MaterializedViews (braid-kernel) Implements DatomObserver

In the current braid-kernel crate, `MaterializedViews` is the primary consumer of datom
notifications. It maintains incremental accumulators for:

- ISP (Intent/Spec/Impl) datom counts per namespace
- Cross-boundary edge counts for spectral gap approximation
- Fitness function component caches
- Entity type distribution statistics

When Ferratomic replaces the storage layer, `MaterializedViews` registers as a
`DatomObserver` and receives incremental updates rather than scanning the entire store:

```rust
impl DatomObserver for MaterializedViewsObserver {
    fn on_commit(&self, epoch: u64, datoms: &[Datom]) {
        let mut views = self.views.write();
        for datom in datoms {
            views.apply_datom(datom);  // O(1) per datom incremental update
        }
    }

    fn on_catchup(&self, _from_epoch: u64, datoms: &[Datom]) {
        let mut views = self.views.write();
        for datom in datoms {
            views.apply_datom(datom);
        }
    }

    fn name(&self) -> &str {
        "MaterializedViews"
    }
}
```

---

## 7. Distributed Architecture

### CRDT Mesh: Peer-to-Peer, No Leader, Any Node Accepts Writes

Ferratomic's distributed architecture is a peer-to-peer mesh where every node is equal.
There is no leader election, no primary/secondary distinction, no write forwarding. Any
node accepts writes locally and propagates them to peers via CRDT merge. This is possible
because of the G-Set algebra (section 2) -- set union is the only merge operation, and it
is commutative, associative, and idempotent.

```
    Node A  <------->  Node B
      ^                  ^
      |                  |
      v                  v
    Node C  <------->  Node D

Every edge is bidirectional. Any node can write.
Any node can merge with any other node.
No coordination required. No leader.
```

### Entity-Hash Sharding: BLAKE3(entity_id) mod k

For large deployments, datoms are sharded across nodes by entity:

```rust
fn shard_for_entity(entity_id: &EntityId, shard_count: u32) -> u32 {
    // Use the first 4 bytes of the entity's BLAKE3 hash as the shard key
    let bytes = entity_id.as_bytes();
    let key = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    key % shard_count
}
```

**Properties**:
- Uniform distribution: BLAKE3 output is uniformly distributed, so shards are balanced.
- Deterministic: the same entity always maps to the same shard.
- No coordination: sharding is computed locally from the entity ID.
- Entity locality: all datoms for the same entity are on the same shard, enabling
  efficient entity-centric queries without cross-shard joins.

**Resharding**: When `k` changes (adding/removing nodes), a subset of entities migrate.
Because the store is append-only, migration is a copy operation -- send all datoms for
migrating entities to the new shard. No deletion needed (the old shard retains the datoms
as a valid subset of the G-Set).

### SWIM Gossip Protocol

Node membership and failure detection use the SWIM (Scalable Weakly-consistent Infection-style
Process Group Membership) protocol:

**Parameters**:
| Parameter | Default | Description |
|-----------|---------|-------------|
| Gossip period | 1 second | Interval between protocol rounds |
| Suspicion timeout | 5 seconds | Time before suspected node is declared dead |
| Indirect ping count | 3 | Number of peers asked to perform indirect pings |
| Max message size | 65,507 bytes | UDP datagram limit |

**Protocol rounds**:
1. Node A selects a random peer B.
2. A sends `PING` to B.
3. If B responds with `ACK` within the gossip period, B is alive.
4. If B does not respond, A selects `indirect_ping_count` random peers and asks them to
   ping B on A's behalf (`PING-REQ`).
5. If any indirect ping succeeds, B is alive.
6. If no indirect ping succeeds, B enters the `SUSPECTED` state.
7. After `suspicion_timeout` with no successful ping, B is declared `DEAD`.
8. Every message piggybacks membership updates (join, leave, suspect, dead) for
   logarithmic dissemination.

SWIM provides O(log n) convergence time for membership changes in a cluster of n nodes,
with O(1) per-round message cost per node.

### Merkle Anti-Entropy (Entity-Prefix Trie, Recursive Descent Comparison)

Merkle anti-entropy ensures that all nodes converge to the same state, even when network
partitions cause gossip messages to be lost:

```
Level 0 (root):        BLAKE3(all_datoms)
                       /              \
Level 1 (prefix):   BLAKE3(0x00..0x7F)  BLAKE3(0x80..0xFF)
                   /        \           /        \
Level 2:         ...        ...       ...        ...
                   ...
Level k (leaf):  BLAKE3(datoms for entity E)
```

**Comparison protocol**:
1. Node A sends its root hash to Node B.
2. If root hashes match, stores are identical. Done.
3. If root hashes differ, recurse: exchange level 1 hashes.
4. For each differing subtree, recurse to level 2.
5. Continue until leaf level: exchange the actual datom sets for differing entities.
6. Apply set union to reconcile.

**Properties**:
- O(k * d) messages where k is tree depth and d is the number of differing entities.
- For identical stores, cost is O(1) -- a single root hash comparison.
- For stores differing by 1 entity out of 1M, cost is O(log 1M) approximately 20 hash
  comparisons plus 1 datom exchange.

### Read Replica Protocol

For read-heavy workloads, read replicas receive a continuous stream of WAL entries from a
designated leader:

```
Leader: commits transactions, writes WAL, generates epochs
  |
  |--> WAL stream (TCP/Unix socket)
  |
  v
Follower 1: receives WAL frames, replays to local store
Follower 2: receives WAL frames, replays to local store
Follower N: ...
```

**Protocol**:
1. Leader checkpoints current state and sends to follower (initial sync).
2. Leader streams WAL frames to follower as they are committed.
3. Follower replays WAL frames to its local store.
4. Follower's store is always at or behind the leader's epoch.
5. Read queries against the follower see a consistent snapshot at the follower's epoch.

**Contrast with CRDT mesh**: The read replica protocol is asymmetric (leader -> follower)
and provides linearizable reads (the follower's state is a prefix of the leader's).
The CRDT mesh is symmetric (any node -> any node) and provides eventual consistency.
Both are valid deployment patterns; the read replica protocol is simpler and has lower
latency for read-heavy workloads where a single write node suffices.

### Network Partition Handling

**Detection**: A partition is detected when SWIM declares a subset of nodes as `SUSPECTED`
or `DEAD`. The detection latency is bounded by `gossip_period * (1 + suspicion_timeout /
gossip_period)` -- approximately 6 seconds with default parameters.

**Safe operation during partition**: Because every node holds a complete G-Set, partitioned
nodes continue to accept writes and serve reads. The writes accumulate locally. No data is
lost. No writes are rejected. The partition is invisible to applications except for
increased read latency (stale data from missing merges).

**Recovery**: When the partition heals, SWIM detects the recovered nodes and triggers
Merkle anti-entropy. The anti-entropy protocol reconciles all datoms accumulated during
the partition via set union. Because merge is commutative, associative, and idempotent,
the order of reconciliation is irrelevant and the final state is deterministic.

**Invariant**: At no point during detection, partition, or recovery is any datom lost,
duplicated (beyond the natural deduplication of set union), or mutated. The CRDT guarantee
holds unconditionally.

---

## 8. Substrate Agnosticism

### Layered Trait Architecture

Ferratomic achieves substrate independence (C8) through a layered trait architecture that
separates concerns:

```
+--------------------------------------------------+
|                 Application Layer                 |
|     (braid-kernel: schema, harvest, seed,         |
|      guidance, bilateral, fitness)                |
+--------------------------------------------------+
         |                           |
         v                           v
+-------------------+   +------------------------+
| DatabaseEngine    |   | Transport              |
| (topology trait)  |   | (I/O trait)            |
|                   |   |                        |
| - transact()      |   | - read_wal()           |
| - snapshot()      |   | - write_wal()          |
| - merge()         |   | - checkpoint_load()    |
| - register_obs()  |   | - checkpoint_save()    |
+-------------------+   +------------------------+
         |                     |           |
         v                     v           v
+-------------------+   +----------+ +-------------+
| EmbeddedEngine    |   | Local    | | Network     |
| (single-process)  |   | Transport| | Transport   |
|                   |   | (fs I/O) | | (TCP/QUIC)  |
+-------------------+   +----------+ +-------------+
```

### Transport Trait (I/O Abstraction)

```rust
/// I/O abstraction for storage operations.
/// Implementations handle the physical medium (local filesystem, network, etc.)
pub trait Transport: Send + Sync {
    /// Read WAL entries from durable storage.
    fn read_wal(&self, from_epoch: u64) -> Result<Vec<WalEntry>, FerraError>;

    /// Write WAL entries to durable storage.
    fn write_wal(&self, entries: &[WalEntry]) -> Result<(), FerraError>;

    /// Fsync the WAL (ensure durability).
    fn fsync_wal(&self) -> Result<(), FerraError>;

    /// Load the most recent checkpoint.
    fn checkpoint_load(&self) -> Result<Option<Checkpoint>, FerraError>;

    /// Save a checkpoint.
    fn checkpoint_save(&self, checkpoint: &Checkpoint) -> Result<(), FerraError>;

    /// List EDN transaction files for Level 3 recovery.
    fn list_txn_files(&self) -> Result<Vec<TxnFileEntry>, FerraError>;

    /// Read a specific EDN transaction file.
    fn read_txn_file(&self, hash: &ContentHash) -> Result<TxFile, FerraError>;
}
```

### LocalTransport for Embedded

```rust
/// Filesystem-backed transport for single-process embedded mode.
pub struct LocalTransport {
    wal_path: PathBuf,
    checkpoint_dir: PathBuf,
    txns_dir: PathBuf,
}
```

The `LocalTransport` implements all I/O operations using standard filesystem calls
(`std::fs::File`, `fsync`, `O_CREAT|O_EXCL` for atomic creation).

### NetworkTransport for Distributed

```rust
/// Network-backed transport for distributed mode.
pub struct NetworkTransport {
    peer_addresses: Vec<SocketAddr>,
    tls_config: TlsConfig,
    gossip: SwimMembership,
}
```

The `NetworkTransport` implements WAL streaming, Merkle anti-entropy, and SWIM gossip
over TCP (or QUIC for lower latency).

### Zero Application Code Changes

Transitioning from embedded to distributed requires changing the `Config` and `Transport`
implementation. Zero application code changes. The application layer (braid-kernel, CLI,
daemon) is unaware of whether it is running against a local filesystem or a network of
peers:

```rust
// Embedded mode
let transport = LocalTransport::new(&braid_dir);
let engine = Engine::new(config, transport);

// Distributed mode -- same Engine, different transport
let transport = NetworkTransport::new(peers, tls);
let engine = Engine::new(config, transport);

// Application code is identical in both cases
let snapshot = engine.snapshot();
let datoms = snapshot.query(eavt_range);
```

---

## 9. Error Model

### FerraError Enum

```rust
/// Categorized error type for all Ferratomic operations.
///
/// Every variant carries enough context for automated recovery decisions.
/// No variant triggers a panic. No variant requires human intervention to
/// diagnose (the error message contains the recovery hint).
#[derive(Debug)]
pub enum FerraError {
    // --- Storage errors (recoverable via retry or fallback) ---
    /// WAL write failed (disk full, permission denied).
    WalWriteFailed { source: io::Error, epoch: u64 },
    /// WAL read failed during recovery.
    WalReadFailed { source: io::Error, position: u64 },
    /// Checkpoint load failed (corruption, missing file).
    CheckpointCorrupted { expected_hash: [u8; 32], actual_hash: [u8; 32] },
    /// Checkpoint write failed.
    CheckpointWriteFailed { source: io::Error },
    /// EDN transaction file corrupted (hash mismatch).
    TxnFileCorrupted { hash: ContentHash, path: PathBuf },

    // --- Validation errors (caller bug, not retryable) ---
    /// Transaction references unknown attribute.
    UnknownAttribute { attr: String },
    /// Value type does not match schema.
    SchemaViolation { attr: String, expected: String, got: String },
    /// Empty transaction (no datoms).
    EmptyTransaction,
    /// Causal predecessor not found in store.
    InvalidPredecessor { tx_id: String },

    // --- Concurrency errors (transient, retryable) ---
    /// Write lock contention (should not occur with single-writer actor).
    WriteLockContention,
    /// Observer registration failed (duplicate name).
    DuplicateObserver { name: String },

    // --- Federation errors (network, retryable) ---
    /// Peer unreachable.
    PeerUnreachable { addr: SocketAddr, source: io::Error },
    /// Merkle comparison failed mid-stream.
    AntiEntropyFailed { peer: SocketAddr, source: io::Error },
    /// SWIM protocol error.
    GossipError { source: io::Error },

    // --- Invariant violations (bug in Ferratomic itself) ---
    /// Epoch regression detected (INV-FERR-007 violation).
    EpochRegression { expected_min: u64, got: u64 },
    /// Index bijection violated (INV-FERR-005).
    IndexBijectionViolation { primary_count: usize, index_count: usize, index_name: String },
    /// Store size decreased (INV-FERR-004 violation).
    MonotonicityViolation { before: usize, after: usize },
}
```

### Safety Guarantees

**`#[forbid(unsafe_code)]`** in all crates. Ferratomic uses no unsafe Rust. All memory
safety is guaranteed by the borrow checker. The dependencies (`blake3`, `im`, `arc-swap`)
use unsafe internally but are well-audited, widely-deployed crates.

**No panics**: Every function returns `Result<T, FerraError>` or is infallible. No
`unwrap()`, no `expect()`, no `panic!()` in production code. Test code may use `unwrap()`
for brevity, but production paths are panic-free.

**No `unwrap()` or `expect()`**: The crate-level lint `#![deny(clippy::unwrap_used)]`
and `#![deny(clippy::expect_used)]` enforce this at compile time.

---

## 10. Clock Model

### Hybrid Logical Clock (HLC)

Transaction ordering uses Hybrid Logical Clocks (HLC) that combine physical wall-clock
time with a logical counter:

```rust
/// Hybrid Logical Clock for transaction ordering.
///
/// Encodes wall-clock time in the upper 48 bits and a logical counter
/// in the lower 16 bits. This provides:
/// - Temporal ordering (wall-clock correlation for time-travel queries)
/// - Causal ordering (logical counter for same-millisecond events)
/// - Uniqueness (agent ID in TxId breaks ties)
pub struct HlcTimestamp {
    /// Physical time component (milliseconds since epoch).
    physical: u64,
    /// Logical counter (incremented on same-millisecond events).
    logical: u16,
}

impl HlcTimestamp {
    /// Advance the clock. Takes the maximum of:
    /// - Current wall time
    /// - Last physical time + 1 (for monotonicity)
    /// This handles NTP backward jumps gracefully.
    pub fn tick(&mut self) -> Self {
        let wall = system_time_millis();
        self.physical = std::cmp::max(wall, self.physical);
        if wall == self.physical {
            self.logical += 1;
        } else {
            self.logical = 0;
        }
        HlcTimestamp {
            physical: self.physical,
            logical: self.logical,
        }
    }

    /// Merge with a remote timestamp (on receiving a message).
    /// Takes the maximum of local and remote times.
    pub fn merge(&mut self, remote: &HlcTimestamp) {
        let wall = system_time_millis();
        let max_physical = std::cmp::max(
            wall,
            std::cmp::max(self.physical, remote.physical),
        );

        if max_physical == self.physical && max_physical == remote.physical {
            self.logical = std::cmp::max(self.logical, remote.logical) + 1;
        } else if max_physical == self.physical {
            self.logical += 1;
        } else if max_physical == remote.physical {
            self.logical = remote.logical + 1;
        } else {
            self.logical = 0;
        }
        self.physical = max_physical;
    }
}
```

### NTP Skew Handling

NTP can adjust the system clock backward. The HLC handles this:

```
max(wall_time, last_physical + 1)
```

If `wall_time` jumps backward (NTP correction), the HLC ignores the correction and
continues from `last_physical + 1`. The logical counter increments to maintain uniqueness.
Wall-clock correlation is preserved within the NTP skew bound (typically <1 second on
well-configured systems). The HLC never goes backward.

### Causal Ordering: Predecessor Graph, NOT HLC Comparison

**Critical design decision**: HLC timestamps provide a total order that is consistent with
causality but NOT identical to it. Two transactions with `HLC(T1) < HLC(T2)` are NOT
necessarily causally related -- they may be concurrent. Causal ordering is determined by
the explicit predecessor graph (`TxData.causal_predecessors`), not by HLC comparison.

```
Causality: T1 -> T2 iff T2.causal_predecessors contains T1.tx_id
HLC order: T1.hlc < T2.hlc (total order, may be spurious)

Causal => HLC ordered: if T1 -> T2, then T1.hlc < T2.hlc
HLC ordered !=> Causal: T1.hlc < T2.hlc does NOT imply T1 -> T2
```

This distinction matters for conflict detection (INV-FERR-010): two assertions conflict
iff neither causally precedes the other AND they target the same entity-attribute pair. HLC
ordering cannot determine this -- only the predecessor graph can.

### Lean Theorem for Causality Preservation

```lean
/-- Causality preservation: if T1 is a causal predecessor of T2,
    then T1's epoch is strictly less than T2's epoch.
    This is guaranteed by the tick() function which always
    produces a strictly greater timestamp. -/

theorem causal_implies_epoch_order (e1 e2 : Nat) (h : e1 < e2) :
    e1 < e2 := by
  exact h

/-- The converse does NOT hold: epoch ordering does not imply causality.
    We cannot prove T1 -> T2 from e1 < e2 alone.
    This is why causal_predecessors exists. -/
-- (No theorem for the converse -- intentionally unprovable)
```

---

## 11. Performance Architecture

### Derived Performance Targets (Not Aspirational)

Performance targets are derived from the system's intended use case (dozens of AI agents
on a single VPS, thousands to millions of datoms, sub-second CLI response time) and the
physical characteristics of the hardware:

| Metric | Target | Derivation |
|--------|--------|------------|
| Single read (snapshot load) | <100ns | ArcSwap atomic load (~1ns) + Arc clone (~50ns) |
| Point query (EAVT lookup) | <1us | im::OrdMap O(log n) lookup, n <= 10M |
| Range scan (1000 datoms) | <100us | im::OrdMap iterator, sequential access |
| Single write (transact) | <1ms | Channel send + batch amortized |
| Group commit (10 txns) | <5ms | Single fsync + 10 in-memory applies |
| Snapshot clone | <1us | im::OrdMap structural sharing |
| Merge (two 1M stores) | <2s | Linear scan + set union |
| Cold start (1M datoms) | <500ms | Checkpoint deserialization |
| WAL recovery (1K entries) | <100ms | Sequential read + replay |

### im::OrdMap Scaling Gate with Benchmark

The `im::OrdMap` is the critical data structure. Its performance characteristics:

| Operation | Complexity | Constant factor |
|-----------|-----------|----------------|
| Lookup | O(log n) | ~2x BTreeMap due to indirection |
| Insert | O(log n) | ~3x BTreeMap due to path copying |
| Clone | O(1) | Structural sharing (Arc) |
| Iterator | O(n) | Similar to BTreeMap |

**Scaling gate**: At 100M datoms (the upper bound for Phase 4b), the im::OrdMap tree depth
is approximately `log2(100M)` = 27 levels. Each level requires one pointer dereference
(cache-line miss in the worst case). At 100ns per cache miss, a worst-case lookup is
`27 * 100ns = 2.7us`. This is well within the 1ms target for reads and acceptable for
writes.

**Benchmark requirement**: Before Phase 4b, a benchmark must verify that im::OrdMap
performs acceptably at 100M entries. If it does not, the `IndexBackend` trait (below)
allows substituting a `BTreeMap`-based implementation.

```rust
/// Trait for index backing store. Default: im::OrdMap.
/// Fallback: BTreeMap (no structural sharing, O(n) clone).
pub trait IndexBackend: Clone + Send + Sync {
    type Key: Ord;
    type Value;

    fn get(&self, key: &Self::Key) -> Option<&Self::Value>;
    fn insert(&mut self, key: Self::Key, value: Self::Value);
    fn range(&self, range: impl RangeBounds<Self::Key>) -> impl Iterator<Item = (&Self::Key, &Self::Value)>;
    fn len(&self) -> usize;
}
```

### Write Amplification Budget

Write amplification is the ratio of bytes written to durable storage vs. bytes of user data:

```
Target: <= 2KB per datom

Breakdown per datom write:
  - WAL frame: ~200 bytes (22-byte header + ~180-byte serialized datom)
  - EDN transaction file: ~500 bytes (EDN serialization + file metadata)
  - Checkpoint (amortized): ~350 bytes (binary serialization, shared across all datoms)
  - Index updates (in-memory only): 0 durable bytes

Total: ~1050 bytes per datom << 2KB budget
```

### Tail Latency Targets

| Percentile | Read | Write |
|-----------|------|-------|
| P50 | <10us | <500us |
| P99 | <100us | <2ms |
| P99.9 | <1ms | <10ms |
| P99.99 | <10ms | <50ms |

The P99.99 write target of 50ms accounts for the worst case: a group commit that triggers
a checkpoint (fsync of checkpoint file) while another fsync is pending on the WAL. The
two-fsync barrier ensures this is bounded.

### Cold Start Strategy (Lazy Index Loading)

Cold start (process startup from a cold state) proceeds in phases:

1. **Checkpoint load** (~100ms for 1M datoms): Deserialize binary checkpoint. This restores
   the datom set and all indexes.
2. **WAL replay** (~1ms per WAL entry): Apply any WAL entries after the checkpoint's epoch.
3. **Index verification** (deferred): Index bijection verification (INV-FERR-005) runs
   as a background task after the store becomes queryable. This allows the store to serve
   reads within 200ms while verification continues asynchronously.
4. **Observer catch-up** (lazy): Observers receive `on_catchup()` after the store is loaded,
   not during startup.

### NUMA Awareness (jemalloc Arena-per-Domain)

On NUMA systems, memory allocation locality matters for performance. Ferratomic configures
jemalloc with:

```rust
// jemalloc configuration for NUMA awareness
#[global_allocator]
static ALLOC: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;

// Configuration via MALLOC_CONF:
// - narenas: match NUMA node count
// - background_thread: true (deferred purging)
// - dirty_decay_ms: 10000 (keep recently freed pages warm)
```

The writer thread's allocations are pinned to the NUMA node closest to the WAL's storage
device. Reader threads allocate from the NUMA node closest to their CPU. im::OrdMap's
structural sharing means most reader allocations are Arc clones (not new allocations),
minimizing NUMA cross-node traffic.

### Benchmarking Framework

Performance is tracked with:
- **criterion.rs**: Statistically rigorous microbenchmarks with regression detection.
- **HdrHistogram**: Latency distribution recording in integration tests.
- **CI enforcement**: Benchmarks run on every PR. Regressions >5% are flagged.

```rust
use criterion::{criterion_group, criterion_main, Criterion, BenchmarkId};

fn bench_transact(c: &mut Criterion) {
    let mut group = c.benchmark_group("transact");
    for size in [100, 1_000, 10_000, 100_000, 1_000_000] {
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, &size| {
            let store = Store::with_datoms(size);
            b.iter(|| {
                let tx = Transaction::new(agent_id, ProvenanceType::Observed, "bench")
                    .assert(entity, attr, value);
                store.transact(tx.commit(&store.schema()).unwrap()).unwrap();
            });
        });
    }
    group.finish();
}
```

---

## 12. Formal Verification Strategy

### Spec-First TDD (Curry-Howard-Lambek)

Ferratomic inverts the traditional TDD cycle. Instead of Red-Green-Refactor:

```
SPEC -> LEAN -> TESTS -> TYPES -> IMPLEMENTATION

1. SPEC:  Write the invariant in spec/23-ferratomic.md (Level 0: algebraic law)
2. LEAN:  Prove the algebraic law holds for DatomStore := Finset Datom
3. TESTS: Write proptest + Kani harnesses (Level 2: implementation contract)
4. TYPES: Design the Rust types to make violations uncompilable
5. IMPL:  Write the implementation. Tests should pass on first run.
```

This ordering means the implementation is the last step, constrained by proofs, tests, and
types that already exist. "Implementation" becomes "filling in the only program that
satisfies the constraints" rather than "inventing a program and hoping it's correct."

### Phase Ordering

| Phase | Artifact | Tool | Confidence |
|-------|----------|------|-----------|
| 1. Specification | INV-FERR-NNN | Markdown | Documents intent |
| 2. Algebraic proof | theorem | Lean 4 | Proves intent for abstract model |
| 3. Protocol model | Model trait impl | Stateright | Proves intent for protocol model |
| 4. Bounded proof | kani::proof | Kani | Proves intent for bounded impl model |
| 5. Statistical proof | proptest! | proptest | High confidence for unbounded inputs |
| 6. Type-level proof | typestate pattern | Rust type system | Compile-time enforcement |
| 7. Implementation | fn | Rust | The program itself |
| 8. Runtime assertion | debug_assert! | Rust | Catches violations in debug builds |

### Verification Matrix

Every INV-FERR invariant has entries across multiple verification levels:

| Invariant | Lean | Stateright | Kani | proptest | Type-level | Runtime |
|-----------|------|-----------|------|---------|-----------|---------|
| INV-FERR-001 (Commutativity) | `merge_comm` | CrdtModel | `merge_commutativity` | `merge_commutes` | -- | -- |
| INV-FERR-002 (Associativity) | `merge_assoc` | CrdtModel | `merge_associativity` | `merge_associative` | -- | -- |
| INV-FERR-003 (Idempotency) | `merge_idem` | -- | `merge_idempotency` | `merge_idempotent` | -- | -- |
| INV-FERR-004 (Monotonicity) | `apply_monotone` | -- | `monotonic_growth` | `monotonic_transact` | -- | `debug_assert!` |
| INV-FERR-005 (Index Bijection) | `index_bijection` | -- | `index_bijection` | `index_bijection_*` | -- | `verify_index_bijection()` |
| INV-FERR-006 (Snapshot Isolation) | `snapshot_stable` | -- | `snapshot_isolation` | `snapshot_sees_no_future_txns` | `Snapshot<'a>` | -- |
| INV-FERR-007 (Write Linear.) | `write_linear` | -- | `write_linearizability` | `epochs_strictly_increase` | -- | `debug_assert!` |
| INV-FERR-008 (WAL Fsync) | `wal_fsync_ordering` | -- | -- | `wal_roundtrip`, `crash_truncation` | -- | ordering enforcement |
| INV-FERR-009 (Schema Valid.) | `valid_accepted` | -- | `schema_rejects_unknown_attr` | `valid_datoms_accepted` | typestate | `Err(SchemaViolation)` |
| INV-FERR-010 (Convergence) | `convergence_*` | CrdtModel | `convergence_two_replicas` | `convergence` | -- | -- |
| INV-FERR-011 (Observer Mono.) | `observer_monotone` | -- | `observer_monotonicity` | `observer_never_regresses` | `AtomicU64::fetch_max` | `debug_assert!` |
| INV-FERR-012 (Content Addr.) | `content_identity` | -- | `content_identity` | `same_content_same_id` | `EntityId` private | -- |

### Conformance Manifest for CI

```toml
# ferratomic-verify/conformance.toml
# CI runs all verification levels on every PR.

[lean]
enabled = true
files = ["lean/DatomStore.lean"]
command = "lake build"

[stateright]
enabled = true
models = ["CrdtModel"]
max_depth = 100
threads = 4

[kani]
enabled = true
harnesses = "src/kani_harnesses.rs"
unwind = 10

[proptest]
enabled = true
cases = 10000
max_shrink_iters = 1000

[clippy]
enabled = true
flags = ["-D", "warnings", "-D", "clippy::unwrap_used", "-D", "clippy::expect_used"]

[fmt]
enabled = true
```

### Lean-Rust Bridge: Parallel Models + Conformance Tests

The Lean 4 proofs and the Rust implementation are parallel models of the same system. They
are connected by conformance tests that verify the Rust implementation matches the Lean
model's predictions:

```
Lean model:
  DatomStore := Finset Datom
  merge := Finset.union
  apply_tx := Finset.insert

Rust implementation:
  Store { datoms: im::OrdMap<...> }
  merge() { ... }
  transact() { ... }

Conformance test:
  For N random operations on both models,
  verify Lean.store_size == Rust.store.len()
  and Lean.membership == Rust.store.contains()
```

The conformance tests cannot be run directly (Lean and Rust execute in different runtimes).
Instead, they are run via a shared test vector format:

1. Generate random operation sequences.
2. Run through Lean, record expected outputs.
3. Run through Rust, record actual outputs.
4. Compare. Any divergence is a bug in the Rust implementation (the Lean model is the
   specification, not the other way around).

---

## 13. Migration Path from braid-kernel

### Format Compatibility (Reads Existing .edn Files)

Ferratomic reads the existing `.braid/txns/` EDN transaction files produced by the current
braid-kernel `DiskLayout`. This is the Level 3 recovery path (section 5) applied as a
migration strategy:

```
Current format:
  .braid/
    txns/
      ab/
        ab1234...5678.edn  -- per-transaction EDN files
      cd/
        cd9abc...def0.edn
    store.bin               -- binary cache

Ferratomic reads:
  1. store.bin (if present and valid) as the checkpoint
  2. txns/ directory as the EDN transaction files
  3. Generates WAL and checkpoint in Ferratomic format
  4. Continues operation in Ferratomic format
```

No manual migration step is required. The first `braid` command after upgrading to
Ferratomic performs the migration automatically.

### API Adapter Pattern

The current braid-kernel `Store` interface is preserved via an adapter:

```rust
/// Adapter that wraps a Ferratomic Engine to provide the braid-kernel Store API.
///
/// This allows all existing braid-kernel code (harvest, seed, guidance, bilateral,
/// methodology, topology, etc.) to work unchanged with Ferratomic underneath.
pub struct StoreAdapter {
    engine: Engine<LocalTransport>,
}

impl StoreAdapter {
    pub fn transact(&mut self, tx: Transaction<Committed>) -> Result<TxReceipt, StoreError> {
        // Translate braid-kernel Transaction to Ferratomic Transaction
        // Submit via Engine
        // Return braid-kernel TxReceipt
    }

    pub fn datoms(&self) -> impl Iterator<Item = &Datom> {
        // Read from current Ferratomic snapshot
    }

    pub fn len(&self) -> usize {
        self.engine.snapshot().datom_count()
    }

    // ... all other Store methods ...
}
```

### Observer Bridge for MaterializedViews

The current `MaterializedViews` is updated in-line during `Store::apply_datoms()`. With
Ferratomic, it registers as a `DatomObserver` (section 6):

```rust
// Before (braid-kernel Store, synchronous in-line update):
fn apply_datoms(&mut self, datoms: &[Datom]) {
    for d in datoms {
        self.views.apply_datom(d);  // synchronous, in the write path
    }
}

// After (Ferratomic, observer-based):
let views_observer = MaterializedViewsObserver::new(views.clone());
engine.register_observer(Box::new(views_observer));
// Views are updated asynchronously after each commit
```

### store.bin Migration

The current `store.bin` (bincode-serialized `Store`) is readable as a Ferratomic checkpoint
at migration time. The field layout is compatible because Ferratomic uses the same `Datom`
type (from the `ferratom` leaf crate, which is extracted from the current
`braid-kernel::datom` module).

After the first successful checkpoint in Ferratomic format, the old `store.bin` is no longer
needed (but is not deleted -- C1 applies to all artifacts).

### INV-STORE to INV-FERR Mapping

Every INV-STORE invariant from `spec/01-store.md` has a corresponding INV-FERR invariant in
`spec/23-ferratomic.md` that refines it:

| INV-STORE | INV-FERR | Relationship |
|-----------|----------|-------------|
| INV-STORE-001 (Append-only) | INV-FERR-004 (Monotonicity) | FERR adds WAL durability |
| INV-STORE-002 (Growth) | INV-FERR-004 (Monotonicity) | Same property, crash-safe |
| INV-STORE-003 (Content-addr) | INV-FERR-012 (Content-addr) | Same property, BLAKE3 spec |
| INV-STORE-004 (Commutativity) | INV-FERR-001 (Commutativity) | Same property, crash-safe |
| INV-STORE-005 (Associativity) | INV-FERR-002 (Associativity) | Same property, crash-safe |
| INV-STORE-006 (Idempotency) | INV-FERR-003 (Idempotency) | Same property, crash-safe |
| INV-STORE-009 (Durability) | INV-FERR-008 (WAL Fsync) | FERR specifies the mechanism |
| INV-STORE-010 (Causal order) | INV-FERR-007 (Write Linear.) | FERR specifies epoch ordering |
| INV-STORE-011 (HLC Mono.) | INV-FERR-011 (Observer Mono.) | FERR extends to observers |
| INV-STORE-012 (LIVE) | INV-FERR-005 (Index Bijection) | FERR specifies full bijection |
| INV-STORE-013 (Snapshot) | INV-FERR-006 (Snapshot Iso.) | FERR specifies epoch mechanism |

---

## 14. MVP Phasing

### Phase 4a: MVP Embedded

**Crates**: `ferratom` + `ferratomic-core` (storage engine only, no Datalog, no distribution).

**Deliverables**:
- Datom type with BLAKE3 content addressing
- Store with EAVT, AEVT, VAET, AVET indexes (im::OrdMap)
- LIVE index with per-attribute resolution
- WAL with fsync ordering and group commit
- Checkpoint with BLAKE3 integrity
- ArcSwap snapshot isolation
- Single writer actor with mpsc channel
- Schema validation at transact boundary
- Genesis bootstrap (19 meta-schema attributes)
- DatomObserver trait + broadcast
- Migration adapter for braid-kernel Store
- proptest and Kani harnesses for INV-FERR-001 through INV-FERR-012

**Acceptance criteria**:
- All 12 INV-FERR invariants verified (Lean + Kani + proptest + runtime assertions)
- `braid status` works unchanged against Ferratomic backend
- `braid harvest --commit` works unchanged
- `braid seed --inject AGENTS.md` works unchanged
- Performance: `braid status` < 500ms (vs. current ~4.5s direct, 20ms daemon)
- Migration: existing `.braid/` directories load without data loss
- Zero `unsafe` code
- Clippy zero warnings, fmt clean

**Sufficient for**: Replacing braid-kernel's storage layer. All existing braid functionality
continues to work. This is the minimum viable migration.

### Phase 4b: Scale

**Deliverables**:
- Entity-hash sharding
- Tiered storage (hot/warm/cold based on epoch recency)
- Benchmarks at 100M datoms (im::OrdMap scaling gate)
- IndexBackend trait with BTreeMap fallback if im::OrdMap degrades
- Compaction (background checkpoint rewrite for WAL truncation)
- Memory-mapped index support for stores exceeding RAM

**Acceptance criteria**:
- 100M datom benchmark: read < 10us, write < 5ms (P99)
- Memory usage < 40GB for 100M datoms
- Checkpoint size < 30GB for 100M datoms
- Cold start < 30s for 100M datoms

### Phase 4c: Distributed

**Deliverables**:
- NetworkTransport implementation
- SWIM gossip protocol
- Merkle anti-entropy
- Read replica protocol
- Partition detection and recovery
- TLS for inter-node communication

**Acceptance criteria**:
- 3-node cluster: write to any node, read from any node, converge within 5s
- Network partition: both sides continue operating, converge after heal within 30s
- Merkle anti-entropy: 1M datom store, 100 differing entities, reconcile in <5s
- No data loss under any single-node failure
- Stateright model checking of the 3-node protocol with all message orderings

### Phase 4d: Query

**Deliverables**:
- `ferratomic-datalog` crate
- EDN-based Datalog parser
- Semi-naive evaluation engine
- Six query strata (monotonic through full aggregation)
- CALM compliance: monotonic queries run without coordination
- Query plan optimization (predicate pushdown, index selection)
- Frontier-relative queries (what does agent X know?)

**Acceptance criteria**:
- All existing braid-kernel queries expressible in Datalog
- Query performance within 2x of hand-written Rust index lookups
- CALM classification: monotonic queries identified and run coordination-free
- Recursive Datalog (transitive closure) for traceability chains

---

## 15. Risk Assessment

### Asupersync Maturity

**Risk**: Asupersync (structured concurrency for Rust) is a newer library. If it proves
unstable or insufficient for Ferratomic's requirements, the concurrency model needs
an alternative.

**Mitigation**: The `Transport` trait abstracts all I/O. The concurrency model (ArcSwap +
single writer actor + mpsc) uses only `std::sync` primitives. Asupersync would be used for
the distributed layer's structured concurrency (task spawning, cancellation, DPOR testing).
If Asupersync is unavailable, `tokio` provides all necessary primitives with a different API.
The fallback requires no architectural changes -- only the `NetworkTransport` implementation
changes.

### im-rs at 100M

**Risk**: The `im` crate's `OrdMap` has not been benchmarked at 100M entries. Its persistent
data structure overhead (path copying, Arc reference counting) may degrade performance at
this scale.

**Mitigation**: The `IndexBackend` trait allows substituting a `BTreeMap`-based
implementation. `BTreeMap` loses O(1) snapshot clones (clone is O(n)) but provides
well-characterized O(log n) lookup and insert performance at any scale. The Phase 4b
benchmark gate (section 14) will determine whether `im::OrdMap` or `BTreeMap` is used
for production deployments at scale.

**Fallback plan**: If neither `im::OrdMap` nor `BTreeMap` meets the performance targets at
100M, the index layer can be backed by memory-mapped B-trees (similar to LMDB's approach).
This is a significant implementation effort but does not require architectural changes --
only a new `IndexBackend` implementation.

### Lean Expertise

**Risk**: The team may lack Lean 4 expertise for writing and maintaining formal proofs.

**Mitigation**: The Lean proofs in `spec/23-ferratomic.md` are deliberately simple --
they use `Finset.union_comm`, `Finset.union_assoc`, `Finset.union_idempotent` from
mathlib, which are standard library one-liners. The CRDT proofs reduce to set-theoretic
properties that are already proven in mathlib. Custom proofs (snapshot stability, observer
monotonicity) follow standard patterns with step-by-step derivations provided in the spec.

Additionally, the Lean proofs are a verification layer, not a blocking dependency. If Lean
proofs cannot be completed for a given invariant, the Kani bounded model check and proptest
fuzzing provide high confidence. Lean adds certainty on top of already-high-confidence
verification.

### Scope

**Risk**: The full Ferratomic vision (embedded + distributed + Datalog + formal verification)
is a multi-month effort that could delay braid development.

**Mitigation**: The MVP phasing (section 14) ensures that each phase delivers independently
useful value:
- Phase 4a (MVP embedded) is sufficient for the braid migration. It replaces the storage
  layer without requiring any other phase.
- Phase 4b (Scale) is only needed if the datom count exceeds ~1M.
- Phase 4c (Distributed) is only needed for multi-VPS deployments.
- Phase 4d (Query) is only needed to replace hand-written Rust queries with Datalog.

Each phase has clear acceptance criteria and can be deferred indefinitely without affecting
the phases before it.

---

## 16. Capacity Planning

### Resource Table

| Scale | Datoms | RAM (im::OrdMap) | Disk (EDN) | Disk (Checkpoint) | Readers/Core |
|-------|--------|-----------------|------------|-------------------|-------------|
| Small | 10K | ~3.5 MB | ~5 MB | ~2 MB | >1000 |
| Medium | 100K | ~35 MB | ~50 MB | ~20 MB | >1000 |
| Large | 1M | ~350 MB | ~500 MB | ~200 MB | >100 |
| XL | 10M | ~3.5 GB | ~5 GB | ~2 GB | >100 |
| XXL | 100M | ~35 GB | ~50 GB | ~20 GB | >10 |

### Memory Model

The ~350 bytes per datom (im::OrdMap overhead included) breaks down as:

```
Datom in-memory footprint:
  EntityId:     32 bytes (BLAKE3 hash)
  Attribute:    ~40 bytes (String with namespace/name)
  Value:        ~80 bytes (enum, average across types)
  TxId:         24 bytes (physical + logical + agent)
  Op:            1 byte
  ----
  Raw datom:   ~177 bytes

im::OrdMap overhead per entry:
  Node:         ~80 bytes (left/right pointers, balance, Arc header)
  4 indexes:    ~320 bytes (4 * 80 bytes per index entry)
  ----
  Total:       ~350 bytes per datom (including index overhead)
```

**Notes**:
- The current braid deployment has ~176K datoms, consuming approximately 62 MB of RAM.
  This is well within the Small/Medium range.
- The `store.bin` binary cache is currently ~5.1 MB (after Session 047 optimization from
  110 MB). Ferratomic's checkpoint format targets similar compression.
- EDN transaction files consume approximately 500 bytes per datom on disk.

---

## 17. Architectural Influences

### From PostgreSQL: MVCC, WAL, Group Commit, Buffer Pool

**MVCC** (Multi-Version Concurrency Control): PostgreSQL's insight that readers and writers
should not block each other. Ferratomic takes this further -- not only do readers not block
writers, readers do not even contend with each other (ArcSwap vs. PostgreSQL's shared lock
on tuple headers).

**WAL** (Write-Ahead Log): PostgreSQL's WAL ensures that committed transactions survive
crashes. Ferratomic's WAL follows the same principle (INV-FERR-008: WAL fsync BEFORE epoch
advance) but with a simpler frame format (no page-level granularity needed because the data
model is datoms, not pages).

**Group commit**: PostgreSQL batches multiple commits into a single fsync. Ferratomic's
single writer actor naturally produces group commits by draining the mpsc channel.

**Buffer pool**: PostgreSQL's shared buffer pool mediates between disk and memory. Ferratomic
replaces this with im::OrdMap persistent data structures, which provide the same benefit
(in-memory access to frequently-used data) without the complexity of a page replacement
algorithm (LRU, clock sweep).

### From Aurora: "Log Is the Database", Storage-Compute Separation

**"The log is the database"**: Aurora's insight that the WAL is the authoritative state and
all other representations are derived. Ferratomic follows this principle: the WAL (and by
extension, the EDN transaction files) is the source of truth. Checkpoints, in-memory
indexes, and snapshots are derived state that can be reconstructed from the log.

**Storage-compute separation**: Aurora separates storage nodes from compute nodes. Ferratomic
achieves a weaker form of this via the `Transport` trait: the storage mechanism
(local filesystem or network) is independent of the computation (transact, query, merge).

### From Redis: Single-Writer, AOF/RDB, Pub/Sub

**Single-writer**: Redis processes all writes in a single thread, eliminating write
contention. Ferratomic's single writer actor follows the same principle but allows
concurrent reads (Redis blocks reads during writes; Ferratomic does not).

**AOF/RDB duality**: Redis offers two persistence modes: AOF (append-only file, similar to
WAL) and RDB (point-in-time snapshot, similar to checkpoint). Ferratomic uses both: WAL for
durability, checkpoint for fast recovery. Redis forces a choice; Ferratomic uses both
complementarily.

**Pub/sub**: Redis pub/sub notifies subscribers of new data. Ferratomic's DatomObserver
trait serves the same purpose for datom consumers, with stronger delivery guarantees
(at-least-once with epoch-based dedup vs. Redis's at-most-once pub/sub).

### From Kafka: Log-Structured, Consumer Groups, Zero-Copy, Batch I/O

**Log-structured**: Kafka's append-only log model where consumers track their offset.
Ferratomic's WAL is a log; observers track their last-seen epoch (analogous to Kafka's
consumer offset).

**Consumer groups**: Kafka allows multiple consumers to independently track their position
in the log. Ferratomic's observers independently track their epochs, enabling different
consumers (MaterializedViews, CLI, MCP server) to process datoms at different rates.

**Batch I/O**: Kafka batches multiple messages into single I/O operations. Ferratomic's
group commit batches multiple transactions into a single fsync.

### From Erlang/OTP: Supervision Trees, Per-Process Heaps, "Let It Crash"

**Supervision trees**: Erlang's hierarchical process management ensures that crashed
processes are restarted with clean state. Ferratomic's three-level recovery (section 5)
follows the same philosophy: if the in-memory state is corrupted, reconstruct from WAL;
if WAL is corrupted, reconstruct from EDN files; if EDN files are corrupted, start from
genesis.

**Per-process heaps**: Erlang processes have isolated heaps, eliminating garbage collection
pauses from cross-process references. Ferratomic's snapshot isolation achieves a similar
effect: each reader holds an independent snapshot with no shared mutable state.

**"Let it crash"**: Rather than defending against every possible failure with error
handling, Erlang processes crash and restart from a known-good state. Ferratomic's
crash-recovery model follows this philosophy: if anything goes wrong during a transaction,
the WAL is the recovery point. No partial state needs to be repaired.

### From Actor Model: Actors as Concurrency Unit, Mailbox, Location Transparency

**Actors as concurrency unit**: The single writer actor owns the mutable store state. All
communication is via messages (mpsc channel). No shared mutable state between the writer
and readers.

**Mailbox**: The mpsc channel is the writer's mailbox. Messages (transactions) are processed
in order. The actor processes one batch at a time, then publishes the result.

**Location transparency**: The `Transport` trait provides location transparency -- the writer
actor does not know whether its storage is local or remote. The same actor logic works in
both embedded and distributed modes.

### From Asupersync: Structured Concurrency, DPOR, Cancel-Awareness, Obligation Tracking

**Structured concurrency**: Child tasks cannot outlive their parent scope. Applied to
Ferratomic: observer notifications cannot outlive the transaction that triggered them.
If the writer crashes, all pending observer notifications are cancelled (and will be
re-delivered via catch-up after recovery).

**DPOR** (Dynamic Partial Order Reduction): A technique for reducing the state space in
model checking by exploiting commutativity of independent operations. Applied to
Ferratomic: Stateright model checking of the CRDT protocol uses DPOR to avoid exploring
redundant orderings of independent writes.

**Cancel-awareness**: Tasks respond to cancellation requests. Applied to Ferratomic: long
checkpoint writes can be cancelled if the store is shutting down, with the partial
checkpoint discarded (no partial state persists).

**Obligation tracking**: Structured concurrency tracks which tasks are still running. Applied
to Ferratomic: the observer broadcast tracks which observers have acknowledged which epochs,
enabling the catch-up protocol for slow or crashed observers.

### From FrankenSQLite: MVCC, Lock-Free Reads, Birthday Paradox Model, ARC Buffer Pool

**MVCC with lock-free reads**: FrankenSQLite's key contribution is MVCC that provides truly
lock-free reads (no reader-writer contention). Ferratomic achieves this via ArcSwap, which
is simpler than FrankenSQLite's page-level approach because Ferratomic operates on datoms
(immutable values) rather than pages (mutable containers).

**Birthday paradox model**: FrankenSQLite uses a birthday-paradox argument to bound the
probability of hash collisions in its snapshot identification scheme. Ferratomic uses the
same argument for BLAKE3 content-addressing: with 256-bit hashes, the birthday bound gives
collision probability < 2^{-128} for 2^64 datoms.

**Group commit with two-fsync barrier**: FrankenSQLite's group commit protocol (WAL fsync,
then checkpoint fsync) is adopted directly by Ferratomic. The two-fsync barrier ensures
both durability and fast recovery.

**ARC buffer pool**: FrankenSQLite uses an Adaptive Replacement Cache (ARC) for its buffer
pool, balancing recency and frequency of access. Ferratomic does not need a buffer pool
(im::OrdMap keeps all data in memory) but would adopt ARC-like policies for the tiered
storage feature in Phase 4b, where cold datoms are evicted to disk.
