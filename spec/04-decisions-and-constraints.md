## 23.4 Architectural Decision Records

### ADR-FERR-001: Persistent Data Structures

**Traces to**: INV-FERR-006 (Snapshot Isolation), INV-FERR-027 (Read P99.99)
**Stage**: 0

**Problem**: Snapshot isolation requires readers to access consistent historical views
while writers mutate the store. How do we provide O(1) snapshot creation without
copying the entire store?

**Options**:

| Option | Description | Pros | Cons |
|--------|-------------|------|------|
| A: `im-rs` persistent collections | Structural sharing via HAMTs. `clone()` is O(1). | Proven Rust crate, excellent API, O(log n) ops. | ~2x memory overhead vs. BTreeMap. Write throughput ~50% of BTreeMap. |
| B: `BTreeMap` + CoW (copy-on-write) | `Arc<BTreeMap>` snapshots clone on write. | Zero overhead for read-only snapshots. Standard library. | Clone is O(n) — unacceptable at 100M datoms. Writer must clone before mutating. |
| C: Custom COW B-tree | Purpose-built persistent B-tree with page-level CoW. | Optimal performance. Page-level sharing. | Significant implementation effort (thousands of LoC). Correctness risk. |

**Decision**: **Option A: `im-rs`**

The 2x memory overhead is acceptable (100M datoms × 200 bytes × 2 = 40GB, within modern
server RAM). The ~50% write throughput reduction is acceptable because writes are
serialized anyway (INV-FERR-007). The main advantage is O(1) snapshot creation, which
enables INV-FERR-006 and INV-FERR-027 without any locking on the read path.

**Rejected**:
- Option B: O(n) clone is unacceptable at scale (copying 20GB per snapshot).
- Option C: The implementation and verification cost exceeds the benefit. `im-rs` is
  battle-tested with 10M+ downloads and property-based test coverage. A custom
  implementation would need equivalent verification effort.

**Consequence**: `Store` uses `im::OrdMap` and `im::OrdSet` instead of `std::BTreeMap`
and `std::BTreeSet`. All index structures use `im-rs`. Snapshot creation is
`store.clone()` which takes O(1) time. Writers pay a ~50% throughput penalty.

**Source**: SEED.md §4 Axiom 3 (Snapshots), ADR-STORE-003

---

### ADR-FERR-002: Async Runtime

**Traces to**: INV-FERR-024 (Substrate Agnosticism), INV-FERR-021 (Backpressure)
**Stage**: 0

**Problem**: Ferratomic needs concurrency for WAL writing, checkpoint creation, anti-entropy
protocol, read replica streaming, and federation fan-out. Which concurrency model should be used?

**Options**:

| Option | Description | Pros | Cons |
|--------|-------------|------|------|
| A: Asupersync (native) | Structured concurrency with `Scope::spawn`, `&Cx` cancel-awareness, two-phase reserve/commit channels, obligation tracking, DPOR/LabRuntime for deterministic testing. | Structured concurrency by construction. Cancel-aware primitives. Deterministic testing via DPOR. Two-phase effects eliminate partial-commit bugs. | Pre-release (author-controlled). Smaller ecosystem. |
| B: Tokio | Full async runtime with spawn, select, channels. | Ecosystem standard. Built-in timers, I/O, networking. Backpressure via bounded channels. | No DPOR. No structured concurrency (unscoped `tokio::spawn` leaks tasks). No cancel-aware two-phase effects. `JoinHandle` drops silently cancel tasks. |
| C: No async (`std::thread`) | OS threads for background tasks, channels for communication. | Simple, debuggable, no colored functions. Predictable latency. | Thread pool sizing is manual. No built-in backpressure. No structured cancellation. Poor fit for federation fan-out. |

**Decision**: **Option A: Asupersync (native greenfield)**

Ferratomic uses asupersync as its primary async runtime. The key properties:

- **Structured concurrency**: `Scope::spawn` ensures all spawned work is region-owned.
  No orphaned tasks. Parent scope cannot complete until children finish or cancel.
- **Cancel-aware primitives**: Every async function takes `&Cx` as first parameter.
  Cancellation propagates through the call tree. No silent task drops.
- **Two-phase reserve/commit channels**: `tx.reserve(cx).await?` is cancel-safe (nothing
  committed). `permit.send(value)` is infallible (committed). Eliminates the partial-send
  bugs inherent in `tokio::sync::mpsc::send`.
- **DPOR/LabRuntime**: Deterministic testing explores all interleavings. This is critical
  for verifying snapshot isolation and CRDT convergence properties under concurrency.
- **Obligation tracking**: Runtime statically verifies that all spawned work is awaited.

**Fallback**: The `asupersync-tokio-compat` boundary adapter confines tokio to explicit
adapter modules for any tokio-only dependency. Core domain code must NOT depend on tokio.
The `Transport` trait abstracts all I/O, enabling runtime-agnostic core if migration is
ever needed.

**Rejected**:
- Option B (Tokio): No DPOR for deterministic testing. No structured concurrency
  (`tokio::spawn` returns `JoinHandle` that silently cancels on drop). No cancel-aware
  two-phase effects. `tokio::sync::mpsc::send` is not cancel-safe at the application
  level (message may be consumed but reply channel dropped).
- Option C (std::thread): Insufficient for federation fan-out and live migration
  streaming. No built-in cancellation propagation.

**Consequence**: Ferratomic async APIs use `&Cx` as first parameter. Background tasks
use `Scope::spawn`. Channels use `asupersync::channel::mpsc` (two-phase reserve/commit)
and `asupersync::channel::broadcast`. Locks use `asupersync::sync::RwLock` (cancel-aware).
Tokio-only dependencies (if any) are wrapped in `asupersync-tokio-compat` adapter modules
that are never imported by core domain code.

**Source**: SEED.md §4, C8 (Substrate Independence)

---

### ADR-FERR-003: Concurrency Model

**Traces to**: INV-FERR-006 (Snapshot Isolation), INV-FERR-007 (Write Linearizability), INV-FERR-027 (Read P99.99)
**Stage**: 0

**Problem**: How do concurrent readers and writers access the store without lock contention
on the read path?

**Options**:

| Option | Description | Pros | Cons |
|--------|-------------|------|------|
| A: `ArcSwap` | Writers build a new snapshot and atomically swap the pointer. Readers load the pointer atomically (no lock). | Zero lock contention on reads. O(1) snapshot access. | Writers pay O(1) swap cost. Old snapshots held by readers prevent deallocation. |
| B: `RwLock` | Writers hold write lock, readers hold read lock. | Simple, standard. | Read lock is still a lock — contention under high reader load. Writer starvation possible. |
| C: Page-level MVCC | Per-page version numbers, copy-on-write at page granularity. | Fine-grained concurrency. Low memory overhead. | Complex implementation. Page-level conflicts. Harder to verify. |

**Decision**: **Option A: `ArcSwap`**

`ArcSwap` provides zero-cost read access: `store.load()` is an atomic pointer load with
no contention, no lock, no CAS loop. Combined with `im-rs` persistent data structures
(ADR-FERR-001), snapshot creation is O(1) and readers never block.

Writers build a new version of the store (using `im-rs` structural sharing), then
atomically swap the pointer. The old version remains accessible to any reader that
loaded it before the swap. When the last reader drops its reference, the old version
is deallocated.

**Rejected**:
- Option B: Even read locks introduce contention under high reader load (10K concurrent
  readers per INV-FERR-027). `pthread_rwlock` has measurable overhead at high reader
  counts.
- Option C: The complexity is not justified. Page-level MVCC is appropriate for disk-based
  databases (e.g., SQLite WAL mode), but Ferratomic's in-memory indexes do not benefit
  from page-level granularity.

**Consequence**: The `Store` is wrapped in `ArcSwap<Store>`. Readers call `store.load()`
(atomic, lock-free) to get a snapshot. Writers call `store.rcu(|old| { /* build new */ })`
to atomically update. Old snapshots are reference-counted and deallocated when no longer
referenced.

**Source**: INV-FERR-006, INV-FERR-027, ADR-STORE-006

**Phase 4a Amendment: Write Serialization via Mutex**

Phase 4a uses `std::sync::Mutex<()>` for write serialization instead of the
WriterActor/mpsc/group-commit pattern described in the architecture document
(FERRATOMIC_ARCHITECTURE.md §4). Both patterns satisfy INV-FERR-007 (write
linearizability) and INV-FERR-008 (WAL fsync ordering):

| Aspect | Mutex (Phase 4a) | WriterActor (Phase 4b+) |
|--------|-----------------|------------------------|
| Concurrency | `Mutex<()>` under `Database::transact()` | mpsc channel → single writer task |
| Fsync | One fsync per transaction | One fsync per batch (group commit) |
| Throughput | ~1-5K txn/s (fsync-bound) | ~50-200K datoms/s (batched fsync) |
| Complexity | Minimal — 20 LOC | Significant — channel, batch collection, timeout |
| Async runtime | None required (`std::sync::Mutex`) | Requires asupersync (ADR-FERR-002) |

**Why Mutex is correct for Phase 4a**: The Mutex pattern produces one WAL entry per
transaction with one fsync, satisfying the two-fsync barrier (INV-FERR-008). Every
invariant (INV-FERR-001..024) holds under Mutex serialization because it provides
strictly stronger ordering than the WriterActor (sequential execution vs. batched).
The throughput limitation (~1-5K txn/s) does not violate any Phase 4a invariant —
the 50-200K datoms/s target is a performance goal, not an invariant.

**When to upgrade to WriterActor**: When benchmarks (bd-85j.12) demonstrate that
single-fsync-per-transaction throughput is insufficient for the application workload.
The upgrade path is:
1. Replace `Mutex<()>` with an mpsc channel accepting `Transaction<Committed>`.
2. WriterActor task drains the channel, batches transactions, writes one WAL entry
   per batch, calls fsync once, then applies all batched transactions to the Store.
3. Partial batch failure: if one transaction in a batch fails schema validation, it
   is rejected individually; the remaining transactions in the batch proceed. Each
   transaction receives its own `Result<TxReceipt, TxApplyError>`. The batch shares
   a single WAL fsync but transactions are logically independent.
4. The WAL entry format does not change — each transaction is still a separate WAL
   frame. The group commit batches the fsync call, not the WAL content.

---

### ADR-FERR-004: Observer Delivery Semantics

**Traces to**: INV-FERR-003 (Merge Idempotency), INV-FERR-010 (Merge Convergence), ADRS PD-004
**Stage**: 0

**Problem**: When a store publishes events to observers (e.g., after a successful transact),
what delivery guarantees should be provided?

**Options**:

| Option | Description | Pros | Cons |
|--------|-------------|------|------|
| A: At-least-once | Events may be delivered more than once. Observers must be idempotent. | Simple. Crash-safe (retry on failure). Compatible with CRDT semantics (merge is idempotent). | Observer must handle duplicates. More network traffic on retries. |
| B: Exactly-once | Each event delivered exactly once, even across crashes. | Clean semantics. No duplicate handling needed. | Requires distributed consensus or transactional outbox. Significant complexity. |
| C: Best-effort | Events may be lost. No delivery guarantee. | Simplest implementation. Zero overhead. | Observers can miss events. Data inconsistency. |

**Decision**: **Option A: At-least-once**

At-least-once delivery aligns with the CRDT semantics of the store. Since merge is
idempotent (INV-FERR-003), receiving the same datoms twice is harmless — the second
delivery is a no-op. This makes observer delivery crash-safe without complex distributed
consensus: if a delivery fails, retry. If the retry delivers a duplicate, idempotency
absorbs it.

**Rejected**:
- Option B: Exactly-once requires either distributed consensus (Paxos/Raft) or a
  transactional outbox with deduplication. Both add significant complexity and latency.
  Since the underlying data model is CRDT (idempotent merge), exactly-once provides
  no additional correctness benefit.
- Option C: Best-effort delivery means observers can miss events permanently. This
  would require a separate synchronization mechanism (e.g., periodic full-state
  reconciliation), which is more expensive than at-least-once retry.

**Consequence**: Observer delivery uses a simple retry loop with exponential backoff.
Observers must be idempotent (which they are, since they process datoms via merge).
The anti-entropy protocol (INV-FERR-022) serves as a fallback: even if observer delivery
fails permanently, anti-entropy eventually synchronizes all nodes.

**Phase 4a Amendment: Advisory-Only Error Propagation**

Phase 4a observer delivery errors are advisory-only and do not propagate as transact
failures. The transaction is committed when WAL fsync completes (INV-FERR-008); observer
delivery is a post-commit side effect. If delivery fails, the error is logged but the
caller receives `Ok(TxReceipt)`, not `Err`. This prevents a committed-but-reported-as-failed
scenario where callers retry an already-committed transaction.

Rationale: the write path ordering is WAL fsync (step 2) → ArcSwap store (step 3) →
observer delivery (step 4). By step 4, the transaction is durable and visible. Observer
failure cannot un-commit a transaction, so propagating the error to the caller is
misleading. Anti-entropy (INV-FERR-022) is the convergence mechanism that ensures
observers eventually catch up, regardless of individual delivery failures.

For Phase 4c cross-process observers, the same principle applies: delivery is best-effort
with anti-entropy as the convergence guarantee. No synchronous retry loop blocks the
writer. Observer implementations that require guaranteed delivery should poll via
anti-entropy rather than relying on push delivery.

**Source**: INV-FERR-003, INV-FERR-010, PD-004, Cleanroom Audit HI-004

---

### ADR-FERR-005: Clock Model

**Traces to**: INV-FERR-015 (HLC Monotonicity), INV-FERR-016 (HLC Causality)
**Stage**: 0

**Problem**: Distributed datom stores need a clock model for causal ordering. Which clock
model provides the best tradeoff between accuracy, complexity, and availability?

**Options**:

| Option | Description | Pros | Cons |
|--------|-------------|------|------|
| A: Hybrid Logical Clock (HLC) | Physical time + logical counter + agent ID. Total order. | Captures causality. Tolerates clock skew. O(1) comparison. | Requires message piggyback. Logical overflow possible (handled by backpressure). |
| B: Lamport Clock | Logical counter only. Captures causality. | Simplest. No dependency on wall clock. | No connection to real time. Hard to debug ("when did this happen?"). |
| C: TrueTime (Google Spanner) | GPS + atomic clock for bounded uncertainty. | True calendar time with known error bounds. | Requires specialized hardware (GPS receivers, atomic clocks). Not available on commodity servers. |

**Decision**: **Option A: Hybrid Logical Clock (HLC)**

HLC provides the best balance: it captures causality (like Lamport clocks) and
approximates real time (like TrueTime), without requiring specialized hardware. The
physical component is useful for debugging ("this datom was created around 2026-03-29T10:00")
and for time-range queries. The logical component ensures strict ordering even when
physical clocks are skewed.

**Rejected**:
- Option B: Lamport clocks lose all connection to real time. A datom with Lamport
  timestamp 47 conveys no information about when it was created. This makes time-range
  queries impossible and debugging difficult.
- Option C: TrueTime is Google infrastructure. It requires GPS receivers and atomic
  clocks, which are not available on commodity servers or developer laptops. Ferratomic
  must run on any machine (C8).

**Consequence**: Every datom carries an HLC timestamp. The HLC is advanced on every
local event (tick) and on every message receipt (receive). Epoch ordering in the store
is derived from HLC values. Time-range queries use the physical component. Causal
ordering uses the full HLC.

**Source**: INV-FERR-015, INV-FERR-016, SR-004

---

### ADR-FERR-006: Sharding Strategy

**Traces to**: INV-FERR-017 (Shard Equivalence), INV-FERR-033 (Cross-Shard Query)
**Stage**: 0

**Problem**: When a store is too large for a single node, how should datoms be distributed
across shards?

**Options**:

| Option | Description | Pros | Cons |
|--------|-------------|------|------|
| A: Entity-hash | `shard(d) = hash(d.entity) % N`. All datoms for an entity on one shard. | Entity locality (single-entity queries hit one shard). Simple. Deterministic. | Hot entities (many datoms) can cause shard imbalance. Cross-entity queries require fan-out. |
| B: Attribute-namespace | `shard(d) = namespace(d.attribute)`. All schema datoms on one shard, all data datoms on another. | Namespace locality. Schema queries hit one shard. | Cross-namespace queries require fan-out. Schema shard may be tiny (waste). |
| C: Random | `shard(d) = hash(d.content_hash()) % N`. Uniformly distributed. | Perfect balance. No hot spots. | No locality. Every query requires fan-out to all shards. Entity-level operations require N round-trips. |

**Decision**: **Option A: Entity-hash**

Entity-hash sharding preserves entity locality: all datoms about entity E reside on
`shard(E) = hash(E) % N`. This means that single-entity operations (lookup all attributes
of E, resolve E's current state, retract a value on E) hit exactly one shard, with no
cross-shard coordination.

The hot-entity problem is mitigated by content-addressed entity IDs (INV-FERR-012):
entity IDs are BLAKE3 hashes, which are uniformly distributed, so datoms are
approximately uniformly distributed across shards. An entity with unusually many
attributes (hundreds of datoms) does create a minor imbalance, but this is bounded
and manageable.

**Rejected**:
- Option B: Attribute-namespace sharding creates highly unbalanced shards (the schema
  namespace has ~50 datoms; the data namespace has millions). It also breaks entity
  locality.
- Option C: Random sharding destroys all locality. Entity-level operations require
  fan-out to all N shards, increasing latency by N× and network traffic by N×.

**Consequence**: `shard_id(d) = u64::from_le_bytes(d.entity.as_bytes()[0..8]) % N`.
Entity-hash sharding is deterministic and content-addressed. Cross-entity queries
(e.g., "all entities with attribute A = V") require fan-out to all shards, which is
acceptable for OLAP workloads but not for OLTP. The AVET index on each shard
provides local optimization.

**Source**: INV-FERR-017, SEED.md §4

---

### ADR-FERR-007: Lean-Rust Bridge

**Traces to**: §23.0.4 (Lean 4 Foundation Model), all INV-FERR invariants with Lean theorems
**Stage**: 0

**Problem**: The Ferratomic specification includes Lean 4 theorems alongside Rust code.
How do we ensure that the Lean model and the Rust implementation remain in sync?

**Options**:

| Option | Description | Pros | Cons |
|--------|-------------|------|------|
| A: Parallel models | Lean model and Rust implementation are maintained independently. Consistency is verified by reviewing both side by side. | Simple. No tooling dependency. Each language is idiomatic. | Models can drift apart silently. No mechanical guarantee of consistency. Review-dependent. |
| B: Aeneas | Extract Lean types from Rust code (Charon frontend → Aeneas backend). Prove properties on the extracted Lean. | Mechanical extraction. Proofs apply to actual code. | Aeneas is research-grade. Limited Rust subset supported. Extraction can break on complex code. |
| C: Lean FFI | Call Rust from Lean (or vice versa) via C FFI. Run Lean proofs against Rust data structures. | Direct interop. Proofs on real data. | Complex build system. FFI boundary is unsafe. Performance overhead. |

**Decision**: **Option A: Parallel models (with mechanical consistency checks)**

The Lean model and Rust implementation are maintained as parallel codebases. The Lean
model captures the algebraic laws (Level 0) and key state invariants (Level 1). The
Rust implementation refines these into concrete data structures and algorithms (Level 2).

Consistency is maintained by:
1. **Structural correspondence**: every Lean definition (`DatomStore`, `merge`, `apply_tx`)
   has a direct Rust counterpart (`Store`, `merge()`, `transact()`).
2. **Property-based testing**: proptest strategies in Rust verify the same properties
   that Lean theorems prove. Any proptest failure indicates a model-implementation gap.
3. **Cleanroom review**: during specification review, Lean theorems and Rust implementations
   are compared side by side to verify that the Lean model accurately captures the Rust
   behavior.

**Rejected**:
- Option B: Aeneas is promising but immature. It supports only a subset of Rust (no
  async, limited trait support, no `BTreeSet`). The Ferratomic codebase would need to
  be rewritten to fit Aeneas's supported subset, which is unacceptable.
- Option C: FFI introduces `unsafe` code at the boundary, violating INV-FERR-023. The
  build system complexity (Lean toolchain + Rust toolchain + C FFI) is excessive.

**Consequence**: Lean theorems are maintained in the specification document alongside
the Rust code. They are not compiled or checked as part of `cargo build`. Lean
verification is a separate process (`lake build` in the Lean project) performed
during specification review. The Lean model is intentionally abstract (using `Finset`
rather than `BTreeSet`) to capture the algebraic properties without mirroring
implementation details.

**Source**: §23.0.4, SEED.md §10

---

### ADR-FERR-010: Deserialization Trust Boundary (Two-Tier Type System)

**Traces to**: INV-FERR-012 (Content-Addressed Identity), INV-FERR-054 (Trust Gradient Query), CI-FERR-002 (Type-Level Refinement)
**Stage**: 0

**Problem**: Core types (`EntityId`, `Value`, `Datom`) derive `Deserialize`, which accepts
arbitrary bytes as valid instances. For `EntityId`, this means any 32 bytes are accepted
as a "BLAKE3 hash" without proof — a `sorry` axiom in Curry-Howard terms. In Phase 4a
(single node, trusted storage), this is mitigated by CRC/BLAKE3 integrity checks on WAL
and checkpoint files. In Phase 4c (federation with adversarial peers), a Byzantine peer
can forge `EntityId` values that are not BLAKE3 of any content, poisoning the Merkle tree
and anti-entropy protocol.

**Options**:

| Option | Description | Pros | Cons |
|--------|-------------|------|------|
| A: Manual Deserialize impls | Custom `impl Deserialize` for `EntityId` and `NonNanFloat` | Targeted fix for the two known bypass types. | Does not generalize. `Datom` and `Value` still accept arbitrary `EntityId` via `Ref` variant. No structural guarantee. |
| B: Newtype with From | `TrustedEntityId(EntityId)` wrapper pattern | Adds a trust layer. | Every function taking `EntityId` must decide which newtype to accept. Viral signature changes. |
| C: Two-tier wire/core types | Separate `WireDatom`/`WireEntityId`/`WireValue` types for deserialization. Core types (`Datom`/`EntityId`/`Value`) have NO `Deserialize`. Trust boundary conversion via `into_trusted()`. | Complete structural guarantee: no path from bytes to core types without explicit trust decision. Zero performance cost (identity function). Forward-compatible with Phase 4c `into_verified()`. | 4 new types. ~10 deserialization callsite changes. |

**Decision**: **Option C: Two-Tier Type System (Architecture C)**

Architecture C provides the strongest Curry-Howard guarantee: every `EntityId` in the
system has known provenance. The type system enforces this structurally — there is no
code path that can accidentally construct an unverified `EntityId` from network bytes.

The design introduces a functor from the **Wire** category (types with `Deserialize`) to
the **Core** category (types with store operations). The functor `into_trusted()` is an
identity function on the bytes (zero cost) but a trust boundary in the type system.
`into_trusted()` is `pub(crate)` to `ferratomic-core` — the federation crate cannot call
it and must use `into_verified()` (which requires cryptographic proof).

**New types** (in `ferratom::wire` module):
- `WireEntityId([u8; 32])` — `Deserialize`, converts via `into_trusted()` or `into_verified(proof)`
- `WireValue` — 11 variants matching `Value`, with `WireEntityId` for `Ref`
- `WireDatom` — all wire-type fields
- `WireCheckpointPayload` — schema + genesis_agent + `Vec<WireDatom>`

**Modified core types**:
- `EntityId`: remove `Deserialize`, add `from_trusted_bytes(pub(crate))`
- `Value`: remove `Deserialize` (contains `EntityId` via `Ref`)
- `Datom`: remove `Deserialize` (contains `EntityId`)
- `NonNanFloat`: custom `Deserialize` impl that rejects NaN

**Unchanged types**: `TxId`, `AgentId`, `Op`, `Attribute` retain `Deserialize` (no
`EntityId` content, no invariants that deserialization could violate).

**Trust provenance model** (every EntityId has exactly one provenance):
- `from_content(bytes)` — I computed the BLAKE3 hash myself
- `into_trusted()` — I read it from my own integrity-verified storage (CRC/BLAKE3 checked)
- `into_verified(signature)` — A trusted agent signed it (Phase 4c, Ed25519)
- `into_merkle_verified(proof, root)` — It is included in a verified Merkle tree (Phase 4c)

This provenance model is the type-level encoding of INV-FERR-054 (trust gradient).

**Performance**: Zero overhead. `into_trusted()` is a field-by-field move. The optimizer
inlines and erases the conversion. Benchmarks show no regression.

**Rejected**:
- Option A: Fixes `EntityId` and `NonNanFloat` but leaves `Datom` and `Value` with
  derived `Deserialize` that can inject unverified `EntityId` via `Value::Ref`. Not
  structurally sound.
- Option B: Creates a viral type parameter problem. Every function signature must choose
  between `EntityId` and `TrustedEntityId`, and the boundary is never fully enforced.

**Source**: Cleanroom Audit 2026-03-31 (Section 14: Architecture C), DEFECT-P3-001, DEFECT-P3-002

---

## 23.5 Negative Cases

### NEG-FERR-001: No Panics in Production Code

**Traces to**: INV-FERR-019 (Error Exhaustiveness), ADRS FD-001
**Stage**: 0

**Statement**: No Ferratomic crate uses `unwrap()`, `expect()`, `panic!()`,
`unreachable!()`, `todo!()`, `unimplemented!()`, or any other panicking construct on
fallible operations in production code (non-test, non-bench).

**Rationale**: A panic in a database engine corrupts the caller's process. If the engine
panics during `transact()`, the host process crashes, the WAL may be left in an inconsistent
state, and all in-flight operations are lost. Database engines must return errors, not
abort.

**Enforcement**:
```rust
// In every Ferratomic crate's lib.rs:
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
#![deny(clippy::panic)]
#![deny(clippy::todo)]
#![deny(clippy::unimplemented)]
```

**Falsification**: A function in any Ferratomic crate that calls `unwrap()`, `expect()`,
`panic!()`, or any other panicking construct on a fallible operation, in non-test code.
Detection: `cargo clippy --all-targets -- -D clippy::unwrap_used -D clippy::expect_used
-D clippy::panic -D clippy::todo -D clippy::unimplemented` reports zero warnings for
non-test code.

**Exception**: `unreachable!()` is permitted in match arms that are provably unreachable
by the type system (e.g., after an exhaustive pattern match that the compiler cannot
verify is exhaustive due to cross-crate type boundaries). Each such usage must include
a comment explaining why the arm is unreachable.

---

### NEG-FERR-002: No Unsafe Code

**Traces to**: INV-FERR-023 (No Unsafe Code)
**Stage**: 0

**Statement**: No Ferratomic crate contains `unsafe` blocks, `unsafe fn`, `unsafe impl`,
or `unsafe trait` declarations.

**Rationale**: `unsafe` code bypasses the Rust borrow checker, enabling use-after-free,
data races, and buffer overflows. A database engine must not have these failure modes.
All performance-critical operations (hashing, serialization, index operations) have
safe implementations. The ~10% performance penalty of bounds checking is acceptable
for the correctness guarantee.

**Enforcement**: `#![forbid(unsafe_code)]` in every crate root. This is stronger than
`#![deny(unsafe_code)]` — it cannot be overridden by `#[allow(unsafe_code)]` on
individual items.

**Falsification**: Any Ferratomic crate compiles successfully while containing `unsafe`.
Detection: `#![forbid(unsafe_code)]` causes compilation failure. Additionally:
`grep -rn "unsafe" crates/ferratomic/ --include="*.rs"` should return zero results
(excluding comments and string literals).

**Exception**: None. Dependencies may use `unsafe` internally (e.g., `blake3` uses SIMD,
`crossbeam` uses atomics), but the Ferratomic crates themselves are pure safe Rust.

---

### NEG-FERR-003: No Data Loss on Crash

**Traces to**: INV-FERR-008 (WAL Fsync Ordering), INV-FERR-014 (Recovery Correctness), C1
**Stage**: 0

**Statement**: No committed transaction's datoms are lost after a crash. "Committed"
means `transact()` returned `Ok(receipt)`, which implies the WAL entry was fsynced
(INV-FERR-008).

**Rationale**: A database that loses committed data is worse than no database. The WAL
fsync ordering (INV-FERR-008) and recovery correctness (INV-FERR-014) together guarantee
this property. The WAL is the durable ground truth; everything else (indexes, snapshots,
caches) is derived state that can be rebuilt from the WAL.

**Falsification**: A transaction `T` where `transact(T)` returns `Ok(receipt)`, followed
by a crash, followed by recovery, and `T`'s datoms are absent from the recovered store.
Testing: simulate crashes at every point in the transact path (using `failpoints` or
`stateright` model checking) and verify that committed data survives.

**Exception**: None. This is an absolute guarantee. The only scenario where data is lost
is hardware failure (disk corruption beyond what BLAKE3 checksums can detect, or complete
disk failure without backup). Software crashes never cause data loss.

---

### NEG-FERR-004: No Stale Reads After Snapshot Publication

**Traces to**: INV-FERR-006 (Snapshot Isolation), INV-FERR-007 (Write Linearizability)
**Stage**: 0

**Statement**: Once a snapshot is published (epoch advanced, `ArcSwap` updated), any
new reader that calls `store.snapshot()` sees the new snapshot. No reader obtains a
snapshot from before the publication after the publication has completed.

**Rationale**: Stale reads can cause incorrect decisions. If agent A transacts a
retraction, and agent B (reading immediately after) still sees the old assertion,
agent B may act on stale data. The `ArcSwap` model (ADR-FERR-003) prevents this:
once `store.swap(new_snapshot)` returns, all subsequent `store.load()` calls return
the new snapshot.

**Clarification**: This invariant applies to new snapshot acquisitions, not to existing
snapshots. A reader that obtained a snapshot BEFORE the publication continues to see
the old data (this is snapshot isolation, INV-FERR-006, which is correct behavior).
Only readers that obtain a snapshot AFTER the publication must see the new data.

**Falsification**: A reader calls `store.snapshot()` after a writer's `transact()`
has returned `Ok`, and the reader does not see the transaction's datoms. This
indicates that the `ArcSwap` update was not visible to the reader — either the
swap was not performed, or the reader loaded a cached stale pointer.

**Exception**: None. This is a linearizability guarantee on the publication point.

---

### NEG-FERR-005: No Unbounded Memory Growth

**Traces to**: INV-FERR-021 (Backpressure Safety)
**Stage**: 0

**Statement**: The Ferratomic engine's memory usage is bounded. No operation causes
unbounded memory allocation. Specifically:
- The write queue depth is bounded (INV-FERR-021).
- Old snapshots held by readers are bounded by the number of concurrent readers and
  the snapshot retention policy.
- The WAL buffer is bounded (`wal_buffer_max` configuration).
- Index memory is proportional to the datom set size (no index bloat).

**Rationale**: An embedded database that grows without bound eventually OOMs the host
process. Since Ferratomic runs inside a long-lived host process (which runs indefinitely),
any unbounded growth is a time bomb.

**Falsification**: The Ferratomic engine's resident memory exceeds `expected_datom_memory
+ max_concurrent_snapshots × snapshot_overhead + wal_buffer_max + constant_overhead`.
Specific failure modes:
- **Snapshot leak**: a reader holds a snapshot reference indefinitely, preventing
  deallocation of the old store version. Memory grows with each new transaction.
- **WAL buffer leak**: the WAL flusher falls behind, and the buffer grows without bound.
- **Index bloat**: a secondary index grows faster than the primary store (e.g., due to
  a bug in index garbage collection, if one existed — but indexes do not have GC
  because the store is append-only).

**Detection**: Monitor `process_resident_memory_bytes` and `ferratomic_datom_count`.
The ratio `memory / datom_count` should be approximately constant (200-500 bytes/datom
depending on value sizes). A growing ratio indicates a memory leak.

**Exception**: None. Memory growth must be strictly proportional to datom count.

---

## 23.6 Cross-Shard Query Planning

### INV-FERR-033: Cross-Shard Query Correctness

**Traces to**: SEED.md §4, INV-FERR-017 (Shard Equivalence), ADR-FERR-006
**Verification**: `V:PROP`, `V:KANI`, `V:LEAN`
**Stage**: 0

#### Level 0 (Algebraic Law)
```
Let query : DatomStore → Result be a Datalog query.
Let S be a store sharded into N shards: S = ⋃ᵢ shard(S, i).

For monotonic queries Q (queries whose result can only grow as the
input grows — no negation, no aggregation, no set difference):

  query(S) = query(⋃ᵢ shard(S, i))
           = ⋃ᵢ query(shard(S, i))     (by monotonicity)

The result of querying the full store equals the union of querying
each shard independently. This is the CALM theorem applied to Datalog:
monotonic queries are coordination-free.

For non-monotonic queries Q' (negation, aggregation, set difference):
  query(S) ≠ ⋃ᵢ query(shard(S, i))    (in general)

Non-monotonic queries require either:
  1. Full materialization: collect all shards, then query locally.
  2. Multi-round coordination: exchange intermediate results.
  3. Explicit shard specification: the caller selects a single shard.
```

#### Level 1 (State Invariant)
The Datalog query evaluator classifies every query as monotonic or non-monotonic before
execution. For monotonic queries, the evaluator can execute the query independently on
each shard and merge the results. For non-monotonic queries, the evaluator requires the
full datom set before evaluation.

Classification is syntactic:
- **Monotonic**: conjunctive queries (joins), unions, projections, selections with
  monotonic predicates. No negation, no aggregation, no `not`.
- **Non-monotonic**: queries containing `not`, `count`, `sum`, `max`, `min`, `exists`,
  set difference, or any user-defined function not proven monotonic.

The query planner determines which shards to contact based on the query's variables:
- If the query specifies an entity ID, only the entity's shard is contacted.
- If the query is over an attribute range, all shards are contacted (fan-out).
- If the query is monotonic and full-fan-out, results are merged via set union.
- If the query is non-monotonic and full-fan-out, all shard data is materialized
  locally before evaluation.

#### Level 2 (Implementation Contract)
```rust
/// Query classification: monotonic vs non-monotonic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryMonotonicity {
    Monotonic,
    NonMonotonic,
}

/// Classify a Datalog query by monotonicity.
pub fn classify_query(query: &DatalogQuery) -> QueryMonotonicity {
    if query.has_negation() || query.has_aggregation() || query.has_set_difference() {
        QueryMonotonicity::NonMonotonic
    } else {
        QueryMonotonicity::Monotonic
    }
}

/// Execute a query across shards.
pub fn query_sharded(
    shards: &[Store],
    query: &DatalogQuery,
) -> Result<QueryResult, QueryError> {
    match classify_query(query) {
        QueryMonotonicity::Monotonic => {
            // Fan-out, merge results
            let results: Vec<QueryResult> = shards.iter()
                .map(|shard| eval_query(shard, query))
                .collect::<Result<_, _>>()?;
            Ok(merge_query_results(results))
        }
        QueryMonotonicity::NonMonotonic => {
            // Materialize all shards, then query
            let full_store = unshard(shards);
            eval_query(&full_store, query)
        }
    }
}

#[kani::proof]
#[kani::unwind(8)]
fn cross_shard_monotonic_correct() {
    let datoms: BTreeSet<Datom> = kani::any();
    kani::assume(datoms.len() <= 4);
    let shard_count: usize = kani::any();
    kani::assume(shard_count > 0 && shard_count <= 3);

    let store = Store::from_datoms(datoms.clone());
    let shards = shard(&store, shard_count);

    // For a simple monotonic query (attribute lookup):
    let attr: u64 = kani::any();

    // Full store result
    let full_result: BTreeSet<_> = store.datoms.iter()
        .filter(|d| d.a == attr)
        .cloned().collect();

    // Sharded result (union of per-shard results)
    let sharded_result: BTreeSet<_> = shards.iter()
        .flat_map(|s| s.datoms.iter().filter(|d| d.a == attr).cloned())
        .collect();

    assert_eq!(full_result, sharded_result);
}
```

**Falsification**: A monotonic query `Q` where `query(S) != ⋃ᵢ query(shard(S, i))`.
This would indicate either:
- The query classifier incorrectly classifies a non-monotonic query as monotonic.
- The shard function loses datoms (violates INV-FERR-017).
- The result merge function drops results.

**proptest strategy**:
```rust
proptest! {
    #[test]
    fn cross_shard_monotonic_correct(
        datoms in prop::collection::btree_set(arb_datom(), 0..200),
        shard_count in 1..8usize,
        query_attr in arb_attribute(),
    ) {
        let store = Store::from_datoms(datoms);
        let shards = shard(&store, shard_count);

        // Monotonic query: all datoms with attribute = query_attr
        let full_result: BTreeSet<_> = store.datoms.iter()
            .filter(|d| d.attribute == query_attr)
            .cloned().collect();

        let sharded_result: BTreeSet<_> = shards.iter()
            .flat_map(|s| s.datoms.iter()
                .filter(|d| d.attribute == query_attr)
                .cloned())
            .collect();

        prop_assert_eq!(full_result, sharded_result,
            "Cross-shard monotonic query gave different results");
    }

    #[test]
    fn monotonicity_classifier_sound(
        query in arb_datalog_query(),
    ) {
        let classification = classify_query(&query);

        if classification == QueryMonotonicity::Monotonic {
            // Verify: query has no negation, aggregation, or set difference
            prop_assert!(!query.has_negation(),
                "Monotonic classification but query has negation");
            prop_assert!(!query.has_aggregation(),
                "Monotonic classification but query has aggregation");
            prop_assert!(!query.has_set_difference(),
                "Monotonic classification but query has set difference");
        }
    }
}
```

**Lean theorem**:
```lean
/-- Cross-shard query correctness: for monotonic queries (modeled as
    filter predicates), querying the union equals the union of queries. -/

theorem filter_union_comm (a b : DatomStore) (p : Datom → Prop) [DecidablePred p] :
    (a ∪ b).filter p = a.filter p ∪ b.filter p := by
  exact Finset.filter_union a b p

/-- Generalized to N shards. -/
theorem filter_biUnion_comm (shards : Finset (Fin n)) (f : Fin n → DatomStore)
    (p : Datom → Prop) [DecidablePred p] :
    (shards.biUnion f).filter p = shards.biUnion (fun i => (f i).filter p) := by
  exact Finset.filter_biUnion shards f p
  sorry -- Finset.filter_biUnion may need explicit proof depending on Mathlib version
```

---

## 23.7 Partition Tolerance

### INV-FERR-034: Partition Detection

**Traces to**: SEED.md §4, INV-FERR-022 (Anti-Entropy Convergence)
**Verification**: `V:PROP`, `V:MODEL`
**Stage**: 0

#### Level 0 (Algebraic Law)
```
Let SWIM_PERIOD be the SWIM protocol failure detection period.
Let partition_event(t) be a network partition occurring at time t.

∀ partition_event(t):
  ∃ t_detect ≤ t + 2 × SWIM_PERIOD:
    partition_detected(t_detect)

Partitions are detected within two SWIM protocol periods. The SWIM
protocol (Scalable Weakly-consistent Infection-style Process Group
Membership Protocol) uses randomized probing and dissemination to
detect failures with bounded detection time and bounded false-positive
rate.
```

#### Level 1 (State Invariant)
When a network partition occurs between two subsets of nodes, the partition is detected
within `2 × SWIM_PERIOD` by at least one node on each side of the partition. Detection
means:
- The node emits a `PartitionDetected` event (incrementing the
  `ferratomic_partition_detected` counter).
- The node logs a warning with the list of unreachable peers.
- The node continues accepting local writes (CRDT safety: writes are always safe,
  per INV-FERR-035).
- The node notifies any registered observers of the partition event.

The `2 × SWIM_PERIOD` bound arises from the SWIM protocol mechanics:
- Each SWIM period, a node pings a random peer.
- If the peer does not respond, the node requests `k` other peers to probe the
  unresponsive peer (indirect probing).
- If all `k` indirect probes fail, the peer is marked as "suspected."
- After one more period without response, the peer is marked as "failed."
- Total detection time: ≤ 2 periods (one for initial failure, one for confirmation).

The false-positive rate is configurable: more indirect probes (`k`) reduce false
positives but increase network traffic. The default `k = 3` gives a false-positive
rate of < 0.01% for typical network conditions.

#### Level 2 (Implementation Contract)
```rust
/// SWIM-based partition detection.
pub struct PartitionDetector {
    swim_period: Duration,
    indirect_probes: usize,  // k
    peers: Vec<PeerInfo>,
    suspected: BTreeSet<PeerId>,
    failed: BTreeSet<PeerId>,
    metrics: PartitionMetrics,
}

pub struct PartitionMetrics {
    partition_detected: Counter,
    partition_duration_seconds: Histogram,
    anti_entropy_repair_datoms: Counter,
}

impl PartitionDetector {
    /// Run one SWIM protocol round.
    pub fn tick(&mut self) -> Vec<PartitionEvent> {
        let mut events = vec![];

        // Select random peer to probe
        let target = self.select_random_peer();
        let responded = self.direct_probe(&target);

        if !responded {
            // Indirect probing
            let indirect_ok = self.indirect_probe(&target, self.indirect_probes);
            if !indirect_ok {
                if self.suspected.contains(&target.id) {
                    // Previously suspected, now confirmed failed
                    self.suspected.remove(&target.id);
                    self.failed.insert(target.id.clone());
                    self.metrics.partition_detected.inc();
                    events.push(PartitionEvent::PeerFailed(target.id.clone()));

                    log::warn!(
                        "Partition detected: peer {} unreachable for 2 SWIM periods",
                        target.id
                    );
                } else {
                    // First failure: suspect
                    self.suspected.insert(target.id.clone());
                    events.push(PartitionEvent::PeerSuspected(target.id.clone()));
                }
            }
        } else {
            // Peer responded: clear suspicion
            self.suspected.remove(&target.id);
            if self.failed.remove(&target.id) {
                events.push(PartitionEvent::PeerRecovered(target.id.clone()));
            }
        }

        events
    }
}
```

**Falsification**: A network partition persists for more than `2 × SWIM_PERIOD` without
being detected by any node. Specific failure modes:
- **No probing**: the SWIM protocol tick is not invoked (timer not running, or the
  event loop is blocked).
- **All probes to non-partitioned peers**: the random peer selection never selects a
  partitioned peer (probability decreases with partition size, but possible for small
  partitions in large clusters).
- **False negative on indirect probe**: an indirect probe succeeds even though the
  target is partitioned (routing anomaly where the indirect prober can reach the
  target but the detector cannot — possible in complex network topologies).

**proptest strategy**:
```rust
proptest! {
    #[test]
    fn partition_detected_within_bound(
        peer_count in 3..20usize,
        partitioned_peers in prop::collection::btree_set(0..20usize, 1..10),
        rounds in 1..20usize,
    ) {
        let partitioned: BTreeSet<_> = partitioned_peers.into_iter()
            .filter(|&p| p < peer_count)
            .collect();

        if partitioned.is_empty() { return Ok(()); }

        let mut detector = PartitionDetector::new(peer_count, 3);

        // Simulate: partitioned peers never respond
        let mut detected = false;
        for round in 0..rounds {
            let events = detector.tick_with_partition(&partitioned);
            if events.iter().any(|e| matches!(e, PartitionEvent::PeerFailed(_))) {
                detected = true;
                // Must detect within 2 rounds per peer (amortized)
                prop_assert!(round <= 2 * peer_count,
                    "Detection took {} rounds (bound: {})", round, 2 * peer_count);
                break;
            }
        }

        if rounds >= 2 * peer_count {
            prop_assert!(detected,
                "Partition not detected after {} rounds", rounds);
        }
    }
}
```

**Lean theorem**:
```lean
/-- Partition detection bound: within 2 rounds, a failed peer is detected.
    We model this as: after 2 probe rounds targeting the failed peer,
    the peer is in the failed set. -/

structure SwimState where
  suspected : Finset Nat
  failed : Finset Nat

def probe_round (state : SwimState) (target : Nat) (responded : Bool) : SwimState :=
  if responded then
    { suspected := state.suspected.erase target, failed := state.failed.erase target }
  else if target ∈ state.suspected then
    { suspected := state.suspected.erase target, failed := state.failed ∪ {target} }
  else
    { suspected := state.suspected ∪ {target}, failed := state.failed }

theorem partition_detected_in_two_rounds (target : Nat) :
    let s0 : SwimState := { suspected := ∅, failed := ∅ }
    let s1 := probe_round s0 target false   -- round 1: suspect
    let s2 := probe_round s1 target false   -- round 2: confirm
    target ∈ s2.failed := by
  unfold probe_round
  simp [Finset.mem_empty, Finset.mem_union, Finset.mem_singleton,
        Finset.mem_erase, Finset.mem_insert]
  sorry -- straightforward case analysis
```

---

### INV-FERR-035: Partition-Safe Operation

**Traces to**: SEED.md §4, C4, INV-FERR-001 through INV-FERR-003 (CRDT laws)
**Verification**: `V:PROP`, `V:MODEL`
**Stage**: 0

#### Level 0 (Algebraic Law)
```
∀ partitions P dividing nodes into subsets {P₁, P₂, ..., Pₖ}:
  ∀ node n ∈ Pᵢ:
    TRANSACT(n, T) succeeds   (writes always accepted)

During a partition, every node continues to accept writes independently.
This is the AP (Availability + Partition tolerance) guarantee of the
CAP theorem. Consistency is eventual: after the partition heals,
anti-entropy (INV-FERR-022) converges all nodes.

Safety proof:
  - Writes are local (no coordination needed for TRANSACT).
  - The store is a G-Set CRDT (grow-only set).
  - G-Set write is always safe: adding a datom never conflicts with
    any other operation (set union is commutative, associative, idempotent).
  - After partition heals: merge(P₁, P₂) = P₁ ∪ P₂ (CRDT merge).
  - By L1-L3: the merged state is identical regardless of merge order.
```

#### Level 1 (State Invariant)
During a network partition, every node continues to accept writes. No node becomes
read-only, no node rejects transactions, no node requires quorum to commit. This is
possible because the store is a G-Set CRDT: every write is an addition to the set,
and additions never conflict with each other.

After the partition heals:
1. Anti-entropy (INV-FERR-022) detects the divergence via Merkle diff.
2. Datoms written during the partition are exchanged between the sides.
3. Both sides merge the received datoms (set union).
4. By INV-FERR-010 (merge convergence), both sides converge to the same state.

The only "conflict" that can arise is at the LIVE view level (INV-FERR-029): if two
agents on different sides of the partition assert different values for the same
cardinality-one attribute, the LIVE view must resolve the conflict. This resolution
is at the query layer, not the store layer: the store contains both assertions
(which is correct), and the LIVE view applies last-writer-wins (by epoch) or
escalates to deliberation (per the resolution policy).

#### Level 2 (Implementation Contract)
```rust
/// Partition-safe write: TRANSACT never fails due to partition.
/// It may fail for other reasons (schema validation, WAL I/O), but
/// never because other nodes are unreachable.
impl Store {
    pub fn transact(&mut self, tx: Transaction<Committed>) -> Result<TxReceipt, TxApplyError> {
        // This method has NO network calls.
        // It operates entirely on local state.
        // It succeeds even if every other node is unreachable.

        // 1. Validate schema (local)
        // 2. Write WAL (local disk)
        // 3. Apply datoms (local memory)
        // 4. Advance epoch (local counter)

        // No: quorum check, leader election, consensus round, remote RPC
        // ...
        Ok(receipt)
    }
}

// Stateright model: writes succeed on both sides of a partition
impl stateright::Model for PartitionModel {
    fn properties(&self) -> Vec<Property<Self>> {
        vec![
            Property::always("writes_always_succeed", |_, state: &PartitionState| {
                // Every pending write eventually succeeds (no rejection due to partition)
                state.pending_writes.iter().all(|w| {
                    state.committed.contains(w) || state.can_commit_locally(w)
                })
            }),
            Property::eventually("partition_convergence", |_, state: &PartitionState| {
                // After partition heals, all nodes converge
                if state.partition_healed {
                    state.nodes.windows(2).all(|w| w[0].datoms == w[1].datoms)
                } else {
                    false // not yet converged (expected)
                }
            }),
        ]
    }
}
```

**Falsification**: A node rejects a valid transaction (schema-valid, well-formed)
solely because it cannot reach other nodes. Specific failure modes:
- **Quorum requirement**: the transaction path includes a quorum check that fails
  when the majority of nodes are unreachable.
- **Leader requirement**: the node refuses to write because it is not the leader
  and cannot reach the leader.
- **Distributed lock**: the transaction path acquires a distributed lock that
  times out due to partition.

**proptest strategy**:
```rust
proptest! {
    #[test]
    fn writes_succeed_during_partition(
        txns in prop::collection::vec(arb_transaction(), 1..20),
    ) {
        // Single-node store: simulates one side of a partition
        let mut store = Store::genesis();

        for tx in txns {
            let result = store.transact(tx);
            // Must succeed (or fail for schema reasons, not partition reasons)
            match &result {
                Err(TxApplyError::Validation(_)) => {}, // schema error: OK
                Err(TxApplyError::WalWrite(_)) => {},    // local I/O error: OK
                Err(other) => {
                    prop_assert!(false,
                        "Unexpected error (possible partition-related): {:?}", other);
                }
                Ok(_) => {}, // success: expected
            }
        }
    }
}
```

**Lean theorem**:
```lean
/-- Partition-safe writes: TRANSACT is a local operation on the G-Set.
    It does not require coordination with any other node.
    This follows from the G-Set CRDT property: writes are always safe. -/

-- apply_tx is defined without any "network" or "quorum" parameter.
-- It operates on a single DatomStore. This IS the formal proof that
-- writes are partition-safe: the function's signature has no network dependency.

theorem partition_safe_write (s : DatomStore) (d : Datom) :
    ∃ s', s' = apply_tx s d := by
  exact ⟨apply_tx s d, rfl⟩

/-- After partition heals: merge restores full state. -/
theorem partition_recovery (side_a side_b : DatomStore) :
    let merged := merge side_a side_b
    side_a ⊆ merged ∧ side_b ⊆ merged := by
  constructor
  · exact merge_monotone_left side_a side_b
  · exact merge_monotone_right side_a side_b
```

---

### INV-FERR-036: Partition Recovery

**Traces to**: SEED.md §4, INV-FERR-022 (Anti-Entropy), INV-FERR-010 (Merge Convergence)
**Verification**: `V:PROP`, `V:MODEL`
**Stage**: 0

#### Level 0 (Algebraic Law)
```
Let Δ = |side_A \ side_B| + |side_B \ side_A| be the symmetric difference
  (number of datoms written during partition that the other side has not seen).
Let N = total number of nodes.

∀ partition recovery:
  anti_entropy_repair_time ∈ O(|Δ| × log N)

The repair time is proportional to the number of new datoms (|Δ|) times
the logarithmic factor for Merkle tree traversal (log N where N is the
number of datoms per node, not the number of nodes — the Merkle tree
depth is O(log N)).

After repair:
  state(side_A) = state(side_B) = state(side_A) ∪ state(side_B)
```

#### Level 1 (State Invariant)
When a partition heals, the anti-entropy protocol (INV-FERR-022) repairs the divergence.
The repair process:
1. **Detection**: both sides detect that previously-failed peers are now reachable
   (SWIM protocol, INV-FERR-034).
2. **Merkle comparison**: nodes exchange Merkle roots and walk the tree to identify
   differing subtrees. This takes `O(log N)` hash comparisons per differing datom.
3. **Datom exchange**: only the datoms in differing subtrees are transferred. This
   transfers exactly `|Δ|` datoms (the symmetric difference).
4. **Merge**: received datoms are merged via set union (INV-FERR-001 through
   INV-FERR-003). By CRDT properties, the merge is idempotent and order-independent.
5. **Convergence**: after one round of anti-entropy, both sides have the full state
   (INV-FERR-010). The Merkle roots match, confirming convergence.

The total repair time is dominated by datom transfer: `|Δ| × datom_size / bandwidth`.
For typical partitions (minutes to hours), `|Δ|` is in the thousands to millions.
At 200 bytes/datom and 100MB/s network, 1M datoms takes 2 seconds.

#### Level 2 (Implementation Contract)
```rust
/// Partition recovery: anti-entropy repair after partition heals.
/// Returns the number of datoms exchanged and the repair duration.
pub fn partition_repair(
    local: &mut Store,
    remote: &Store,
    metrics: &PartitionMetrics,
) -> RepairResult {
    let start = Instant::now();

    let exchanged = anti_entropy_full(local, remote);

    let duration = start.elapsed();
    metrics.partition_duration_seconds.observe(duration.as_secs_f64());
    metrics.anti_entropy_repair_datoms.inc_by(exchanged as u64);

    debug_assert_eq!(local.datom_set(), remote.datom_set(),
        "INV-FERR-036: stores did not converge after repair");

    RepairResult {
        datoms_exchanged: exchanged,
        duration,
    }
}

pub struct RepairResult {
    pub datoms_exchanged: usize,
    pub duration: Duration,
}
```

**Falsification**: After partition recovery, the two sides have different datom sets
(convergence failure). Or: the repair time is not `O(|Δ| log N)` — it takes time
proportional to the full store size rather than the delta. Specific failure modes:
- **Full re-sync**: the Merkle comparison identifies the root as different and falls
  back to transferring the entire store (instead of walking the tree to find the delta).
- **Non-convergent merge**: merge has order-dependent behavior (violates INV-FERR-001
  through INV-FERR-003), causing the two sides to diverge further instead of converging.
- **Incomplete exchange**: the anti-entropy protocol exchanges datoms in only one
  direction (A sends to B but B does not send to A), leaving one side behind.

**proptest strategy**:
```rust
proptest! {
    #[test]
    fn partition_repair_converges(
        shared_datoms in prop::collection::btree_set(arb_datom(), 0..100),
        a_only in prop::collection::btree_set(arb_datom(), 0..50),
        b_only in prop::collection::btree_set(arb_datom(), 0..50),
    ) {
        let mut a_datoms = shared_datoms.clone();
        a_datoms.extend(a_only.clone());
        let mut b_datoms = shared_datoms;
        b_datoms.extend(b_only.clone());

        let mut store_a = Store::from_datoms(a_datoms);
        let mut store_b = Store::from_datoms(b_datoms);

        // Repair
        let metrics = PartitionMetrics::default();
        let result = partition_repair(&mut store_a, &mut store_b, &metrics);

        // Converged
        prop_assert_eq!(store_a.datom_set(), store_b.datom_set(),
            "Stores did not converge after partition repair");

        // All datoms from both sides present
        for d in &a_only {
            prop_assert!(store_b.datom_set().contains(d),
                "A-only datom missing from B after repair");
        }
        for d in &b_only {
            prop_assert!(store_a.datom_set().contains(d),
                "B-only datom missing from A after repair");
        }
    }

    #[test]
    fn repair_time_proportional_to_delta(
        shared_datoms in prop::collection::btree_set(arb_datom(), 50..200),
        delta_datoms in prop::collection::btree_set(arb_datom(), 1..50),
    ) {
        let mut a = Store::from_datoms(shared_datoms.clone());
        let mut b_datoms = shared_datoms;
        b_datoms.extend(delta_datoms.iter().cloned());
        let b = Store::from_datoms(b_datoms);

        let metrics = PartitionMetrics::default();
        let result = partition_repair(&mut a, &b, &metrics);

        // Datoms exchanged should be approximately |delta|
        // (may be slightly more due to Merkle tree granularity)
        prop_assert!(result.datoms_exchanged >= delta_datoms.len(),
            "Exchanged {} datoms but delta is {}",
            result.datoms_exchanged, delta_datoms.len());
        prop_assert!(result.datoms_exchanged <= delta_datoms.len() * 2,
            "Exchanged {} datoms but delta is only {} (too much overhead)",
            result.datoms_exchanged, delta_datoms.len());
    }
}
```

**Lean theorem**:
```lean
/-- Partition recovery: after merging, both sides have the union of all datoms. -/

theorem partition_recovery_complete (shared a_only b_only : DatomStore) :
    let side_a := shared ∪ a_only
    let side_b := shared ∪ b_only
    let merged := merge side_a side_b
    merged = shared ∪ a_only ∪ b_only := by
  unfold merge
  simp [Finset.union_assoc, Finset.union_comm]
  sorry -- Finset union associativity/commutativity rearrangement

theorem partition_recovery_symmetric (a b : DatomStore) :
    merge a b = merge b a := by
  exact merge_comm a b
```

### Operational Monitoring

The following metrics and alerts support partition tolerance in production:

| Metric | Type | Description |
|--------|------|-------------|
| `ferratomic_partition_detected` | Counter | Number of partition events detected. Increment on each `PeerFailed` event. |
| `ferratomic_partition_duration_seconds` | Histogram | Duration of each partition (from detection to recovery). Buckets: 1s, 5s, 30s, 60s, 300s, 600s, 3600s. |
| `ferratomic_anti_entropy_repair_datoms` | Counter | Total datoms exchanged during anti-entropy repair. High values indicate large partitions or frequent splits. |
| `ferratomic_swim_probe_failures` | Counter | Number of failed SWIM probes (before confirmation). Rising rate indicates network instability. |
| `ferratomic_merkle_diff_datoms` | Histogram | Number of differing datoms per Merkle comparison. Monitors convergence rate. |

| Alert | Condition | Action |
|-------|-----------|--------|
| Partition detected | `ferratomic_partition_detected` increments | Log warning. Notify registered observers. Continue accepting local writes. |
| Long partition | `ferratomic_partition_duration_seconds` > 300s | Escalate to operator. Consider manual intervention (network repair). |
| Large repair | `ferratomic_anti_entropy_repair_datoms` > 1M in single repair | Log warning. Monitor for performance impact during repair. |
| Convergence failure | Two nodes with same update set have different Merkle roots | **Critical**: indicates CRDT invariant violation (INV-FERR-001 through INV-FERR-003). Halt and investigate. |

---

