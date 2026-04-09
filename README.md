# Ferratomic

<div align="center">

[![License: MIT OR Apache-2.0](https://img.shields.io/badge/License-MIT%20OR%20Apache--2.0-blue.svg)](https://opensource.org/licenses/MIT)
[![Rust](https://img.shields.io/badge/Rust-1.82+-orange.svg)](https://www.rust-lang.org)
[![Lean 4](https://img.shields.io/badge/Lean_4-proven-green.svg)](https://leanprover.github.io/)
[![Safe API surface](https://img.shields.io/badge/unsafe-contained-success.svg)](https://doc.rust-lang.org/nomicon/)

**A formally verified, distributed embedded datom database engine.**

</div>

Every agentic system decomposes into three components: an append-only event log, an opaque runtime, and a stateless policy function. This decomposition is algebraically necessary. The event log mediates between the policy (which needs epistemic state) and the runtime (which provides persistence). The bottleneck in agentic systems is not intelligence — it is memory architecture. Expert performance arises not from superior reasoning but from superior associative retrieval over a structured fact store.

**Ferratomic is that fact store.**

It reifies the algebraic store `(P(D), U)` — a grow-only set of datoms, merged by set union — as a production-grade embedded database. Append-only. Content-addressed. CRDT-mergeable without coordination. Temporally queryable. Horizontally scalable. It is the persistence substrate that makes durable knowledge accumulation, multi-agent federation, and self-evolving knowledge graphs possible.

Ferratomic is to agentic systems what the filesystem is to operating systems — the substrate that makes everything else possible.

```bash
git clone https://github.com/wvandaal/ferratomic && cd ferratomic
CARGO_TARGET_DIR=/data/cargo-target cargo test --workspace
```

---

## TL;DR

**The Problem**: Distributed agents that share knowledge need consensus protocols, conflict resolution strategies, and careful coordination. Every merge is a potential conflict. Every partition is a potential data loss event. Existing embedded databases (SQLite, RocksDB, LMDB) provide no algebraic guarantees about what happens when two replicas diverge and reconnect. Without a structured fact store, every capability — retrieval, coordination, verification, knowledge transfer — must be reinvented per-application, on ad-hoc substrates that cannot merge, cannot trace provenance, and cannot scale.

**The Solution**: Ferratomic eliminates conflicts by construction. The store is a grow-only set of immutable facts under set union. Two replicas that receive the same facts converge to the same state regardless of delivery order, timing, or network topology. No conflicts. No consensus protocol. No coordination. The data structure IS the consistency mechanism.

### Why Ferratomic?

| Property | What It Means |
|----------|---------------|
| **Conflict-free merge** | Any two stores merge by set union. Commutative, associative, idempotent. No coordination required. |
| **Append-only history** | Every state the system has ever been in is recoverable. Nothing is ever deleted or mutated. |
| **Content-addressed identity** | `EntityId = BLAKE3(content)`. Same fact = same datom, regardless of who asserts it or when. |
| **Causal traceability** | Every fact records who, when, and what was known at the time. Ed25519 signed transactions. |
| **Formally verified** | 86 invariants proven in Lean 4 (0 `sorry`), model-checked with Stateright, bounded-verified with Kani, property-tested with proptest at 10,000 cases per property. |
| **Safe callable surface** | `unsafe` permitted only when firewalled behind safe APIs, mission-critical for performance, and documented via ADR. Callers can never trigger UB. |

### What Ferratomic is NOT

These anti-goals prevent scope creep. Each rules out a direction that would compromise the project's purpose.

- **Not an application framework.** Zero knowledge of application-layer concerns — no domain entities, no workflow logic, no UI. Applications build ON Ferratomic, not IN it.
- **Not tied to any runtime or substrate.** No hard dependency on tokio, Linux syscalls, AWS APIs, or any specific compute platform. The core engine is portable across any environment where Rust compiles.
- **Not a consensus system.** CRDT merge means the data structure IS the consistency mechanism. Adding Raft, Paxos, or any coordination protocol for writes would contradict the algebraic foundation.
- **Not a retrieval heuristic.** Vector similarity finds "related" content. Ferratomic provides a verification substrate — structured, queryable, with provenance, temporal completeness, and formal consistency guarantees. Semantic search may be built on top; it is not a substitute for the substrate.

---

## The Core Equation

```
Store = (P(D), U)
```

The store is the powerset of datoms under set union. This single algebraic structure gives you Strong Eventual Consistency (SEC) as a theorem, not an implementation choice. The Lean proof is 4 lines:

```lean
theorem merge_comm (a b : DatomStore) : merge a b = merge b a :=
  Finset.union_comm a b

theorem merge_assoc (a b c : DatomStore) : merge (merge a b) c = merge a (merge b c) :=
  Finset.union_assoc a b c

theorem merge_idemp (a : DatomStore) : merge a a = a :=
  Finset.union_idempotent a
```

Commutativity + associativity + idempotency = join-semilattice. Convergence follows.

---

## Quick Example

```rust
use ferratom::{Attribute, Datom, EntityId, Value, Op, TxId, Schema};
use ferratomic_db::{Store, Database};

// 1. Create a database (deterministic 19-attribute genesis)
let db = Database::genesis();

// 2. Build a transaction
let mut tx = db.build_transaction();
tx = tx.assert_datom(
    EntityId::from_content(b"alice"),
    Attribute::from("user/name"),
    Value::String("Alice".into()),
);

// 3. Transact (WAL-durable before visible, lock-free reads after)
let receipt = db.transact(tx.commit(&db.schema())?)?;

// 4. Merge two divergent stores (conflict-free by construction)
let merged = Store::from_merge(&store_a, &store_b)?;
// merged == Store::from_merge(&store_b, &store_a)  -- always

// 5. Query LIVE state (latest non-retracted values)
let name = db.snapshot().live_resolve(
    EntityId::from_content(b"alice"),
    &Attribute::from("user/name"),
);
```

---

## Design Philosophy

### Value Hierarchy

When two good things conflict, this hierarchy resolves the conflict. Higher tiers win unconditionally.

| Tier | Priority | Examples | Tradeoff Rule |
|------|----------|----------|---------------|
| **1: Non-Negotiable** | Algebraic correctness, append-only durability, safety | CRDT laws hold under all conditions; safe callable surface; no panics | Never trade away |
| **2: Foundation** | Verification depth, architectural clarity, spec alignment | Prove 30 invariants at 6 layers rather than implement 55 with partial coverage | Trade only against Tier 1 |
| **3: Production** | Performance at scale, completeness, federation | <10ms p99 reads at 100M datoms; all phases implemented | Trade against Tier 2 with measured evidence |
| **4: Desirable** | API ergonomics, feature breadth | Simple interfaces; only spec-grounded features | Yield to higher tiers without resistance |

### 1. Types Are Propositions (Curry-Howard)

Every type admits exactly the valid states. Invalid states are unrepresentable at compile time.

- `EntityId([u8; 32])` not `Vec<u8>` -- exactly 256 bits, no more, no less
- `Transaction<Building>` -> `Transaction<Committed>` -- typestate prevents accessing datoms before validation
- `NonNanFloat` rejects NaN on construction -- total ordering guaranteed for all `Value` variants
- `Op::Assert | Op::Retract` -- two variants, no third option

### 2. Spec-First TDD with Formal Refinement

```
Phase 0: Specification (86 invariants, 32 ADRs, 7 NEGs)     DONE
Phase 1: Lean 4 proofs (0 sorry)                             DONE
Phase 2: Test suite (all fail -- red phase)                   DONE
Phase 3: Type definitions (types encode invariants)           DONE
Phase 4: Implementation (programs prove properties)           IN PROGRESS
Phase 5: Integration
```

No phase N+1 until phase N passes its isomorphism check. A gap between spec, algebra, and tests is a defect, not technical debt.

### 3. The Store IS the Consistency Mechanism

No consensus protocol. No conflict resolution logic. No vector clock comparison. The algebraic structure of set union under content-addressed identity handles all of it. Two agents independently create the same fact? Same EntityId (BLAKE3). Same datom. Set union deduplicates. No coordination needed.

### 4. Verification at Every Layer

| Layer | Tool | What It Proves | Coverage |
|-------|------|---------------|----------|
| Algebraic proofs | Lean 4 + mathlib | CRDT laws, index bijection, HLC causality, convergence | 0 sorry |
| Protocol model checking | Stateright | Snapshot isolation, crash recovery, merge convergence | Exhaustive state space |
| Bounded verification | Kani | Index consistency, WAL ordering, value safety, backpressure | Symbolic execution |
| Property testing | proptest (10K cases) | Round-trip, monotonicity, Lean-Rust conformance, isomorphism | Statistical confidence >99.97% |
| Fault injection | FaultInjectingBackend | TornWrite, PowerCut, IoError, DiskFull, BitFlip | Deterministic adversarial |
| MIRI | `cargo +nightly miri test` | Undefined behavior across unsafe boundaries | CI gate (nightly) |
| Fuzz testing | cargo-fuzz (5 targets) | Deserialization/WAL/checkpoint edge cases | CI smoke + pre-tag extended |
| Mutation testing | cargo-mutants | Weak test assertions (test strength, not coverage) | >80% kill rate target |
| Coverage | cargo-llvm-cov | Untested code paths | 90% line, 80% branch minimum |

### 5. Zero-Defect Cleanroom Standard

In agentic development, the codebase IS the training signal for every agent that touches it. Toxic patterns (unwrap, unsafe, unverified invariants, dead code, suppressed warnings) propagate through agent behavior. Clean patterns propagate too. The quality of the codebase determines the quality of all future work on it. Zero-defect is not a productivity target — it is a compound interest argument.

No `#[allow(clippy::...)]`. No `cfg` gating that hides code from the type checker. No `unwrap()` or `expect()` in production code. No lint escape hatches anywhere, including tests and verification code. If clippy flags it, fix the root cause. Every bug gets a regression test. Every fuzz crash gets a seed corpus entry. Coverage ratchet: thresholds only increase.

The full defensive engineering standard (11 CI gates, MIRI, ASan, mutation testing, coverage thresholds, supply chain audit, threat modeling, regression discipline) is specified in [GOALS.md §6](GOALS.md).

### 6. The Six-Dimension Decision Evaluation Framework

Every non-trivial design decision is scored across six orthogonal dimensions:

| Dimension | What it measures | Weight |
|-----------|------------------|--------|
| **Performance** | Latency, throughput, asymptotic complexity | High |
| **Efficiency** | Storage density, memory, bandwidth, energy | High |
| **Accretiveness** | Forward-looking — does this compound positively over future work? | High |
| **Correctness** | Internal consistency, edge cases, no contradictions | Critical (Tier 1) |
| **Quality** | Lab-grade adherence to lifecycle/16 + lifecycle/17 + INV-FERR-001 gold standard | High |
| **Optimality** | Best decision among options considered | Medium |

Each dimension is scored 1-10. Composite is the average. Literal 10.0 requires ALL six dimensions at 10.0 — any dimension below 10.0 must be documented with what would close the gap.

**Critical**: Accretiveness is **forward-looking** (does the choice compound positively?), NOT backward-looking. A correction that fixes a wrong design is HIGHLY accretive — it eliminates future debt. The framework distinguishes Performance from Efficiency because they are different concerns: an algorithm can be FAST but inefficient (extra space), or SLOW but efficient (in-place). For ferratomic, **storage efficiency** is a top priority — billion-scale single-machine deployment requires both axes.

Use the framework before authoring spec invariants, choosing implementations, scoring beads, gating phases, and reviewing PRs. The full framework, the 10.0 rule, the relationship to the value hierarchy, and a worked example are specified in [GOALS.md §7](GOALS.md).

---

## How Ferratomic Compares

| Feature | Ferratomic | SQLite | Datomic | CRDTs (Automerge) |
|---------|-----------|--------|---------|-------------------|
| Conflict-free merge | Set union (proven) | Manual conflict resolution | Peer-based with coordination | Document-level CRDT |
| Formal verification | Lean 4 + Kani + Stateright + proptest | Extensive testing, no proofs | No formal proofs | No formal proofs |
| Append-only history | By construction (C1) | WAL only, data mutable | Append-only | Append-only |
| Content-addressed ID | BLAKE3 per-datom | Integer rowid | Server-assigned | Lamport timestamps |
| Embedded (no server) | Yes | Yes | No (server required) | Yes |
| Query language | Datalog (Phase 4d) | SQL | Datalog | JavaScript API |
| Distribution | CRDT federation (Phase 4c) | Manual replication | Built-in clustering | Built-in sync |
| Signed transactions | Ed25519 (Phase 4c) | N/A | N/A | N/A |
| `unsafe` policy | Contained: safe API boundary, ADR-documented | Extensive C `unsafe` | JVM | JavaScript |

**When to use Ferratomic:**
- You need distributed agents to share knowledge without coordination
- You need cryptographic provenance (who asserted what, when, with what evidence)
- You need formal guarantees about merge correctness, not just "it usually works"
- You need append-only audit trails with conflict-free synchronization

**When Ferratomic might not be ideal:**
- You need SQL compatibility (use SQLite or Postgres)
- You need high-throughput OLAP workloads (Ferratomic optimizes for fact-level OLTP)
- You need mature production deployment today (Phase 4a approaching gate closure)

---

## Architecture

```
                        ┌─────────────────────────────┐
                        │     Your Application         │
                        └──────────────┬──────────────┘
                                       │
                        ┌──────────────▼──────────────┐
                        │   ferratomic-datalog         │  Datalog query engine
                        │   (Phase 4d: parser,         │  CALM monotonicity
                        │    planner, evaluator)        │  classification
                        └──────────────┬──────────────┘
                                       │
                        ┌──────────────▼──────────────┐
                        │   ferratomic-db            │  Storage + concurrency
                        │                              │
                        │  ┌────────┐  ┌────────────┐ │
                        │  │ArcSwap │  │  Database   │ │  Lock-free MVCC reads
                        │  │snapshot│  │  <Opening>  │ │  Typestate lifecycle
                        │  │  load  │  │  <Ready>   │ │
                        │  └────────┘  └────────────┘ │
                        │                              │
                        │  ┌────────────────────────┐  │
                        │  │ Store (dual-repr)       │  │
                        │  │ ┌──────────┐ ┌───────┐ │  │  Positional: read-optimal
                        │  │ │Positional│ │OrdMap │ │  │  OrdMap: write-capable
                        │  │ │ (arrays) │ │(tree) │ │  │  promote/demote per txn
                        │  │ └──────────┘ └───────┘ │  │
                        │  └────────────────────────┘  │
                        │                              │
                        │  ┌─────┐ ┌──────────┐       │
                        │  │ WAL │ │Checkpoint │       │  Durability: WAL fsync
                        │  │CRC32│ │ BLAKE3    │       │  before epoch advance
                        │  └─────┘ └──────────┘       │
                        └──────────────┬──────────────┘
                                       │
                        ┌──────────────▼──────────────┐
                        │   ferratom                   │  Core types facade
                        │   Datom, EntityId, Value,    │  Schema, FerraError
                        │   Attribute, Op, Wire types  │  ADR-FERR-010 trust
                        └──────────────┬──────────────┘
                                       │
                        ┌──────────────▼──────────────┐
                        │   ferratom-clock             │  ZERO project deps
                        │   HybridClock, TxId,         │  Clock leaf crate
                        │   AgentId, Frontier          │  (ADR-FERR-015)
                        └─────────────────────────────┘
```

### Crate Map

| Crate | Role | LOC | Dependencies |
|-------|------|-----|-------------|
| `ferratom-clock` | Clock primitives: HybridClock, TxId, AgentId, Frontier | ~350 | serde (zero project deps) |
| `ferratom` | Core types: Datom, EntityId, Value, Schema, Wire trust boundary | ~3,300 | ferratom-clock, blake3, ordered-float, serde |
| `ferratomic-db` | Store engine: MVCC, WAL, checkpoint, indexes, merge, LIVE resolution | ~8,800 | ferratom, im, arc-swap, bitvec, bincode, memmap2 |
| `ferratomic-datalog` | Query: Datalog parser, planner, evaluator, CALM classification | stubs | ferratom, ferratomic-db |
| `ferratomic-verify` | Proofs: Lean 4 (0 sorry), Stateright, Kani, proptest (10K cases) | ~12K+ | ferratom, ferratomic-db |

Dependency direction: `clock -> ferratom -> ferratomic-db -> ferratomic-datalog`. Acyclic. `ferratomic-verify` depends on both `ferratom` and `ferratomic-db`.

---

## Installation

### From source (requires Rust 1.82+)

```bash
git clone https://github.com/wvandaal/ferratomic
cd ferratomic

# CRITICAL: Set cargo target dir (default uses /tmp which is RAM-backed)
export CARGO_TARGET_DIR=/data/cargo-target

# Build
cargo check --workspace

# Test (1K proptest cases for fast iteration)
PROPTEST_CASES=1000 cargo test --workspace

# Full verification (10K cases, release mode, ~10 min)
cargo test --workspace --release

# Lean proofs
cd ferratomic-verify/lean && lake build
```

### Requirements

| Component | Version | Purpose |
|-----------|---------|---------|
| Rust | 1.82+ | Compiler (`rustup default stable`) |
| Lean 4 | latest | Formal proofs (`elan install`) |
| Lake | latest | Lean build system (bundled with Lean) |

---

## Quick Start

```bash
# 1. Clone and build
git clone https://github.com/wvandaal/ferratomic && cd ferratomic
export CARGO_TARGET_DIR=/data/cargo-target
cargo check --workspace

# 2. Run the test suite
PROPTEST_CASES=1000 cargo test --workspace

# 3. Verify Lean proofs (0 sorry)
cd ferratomic-verify/lean && lake build && cd ../..

# 4. Run clippy (zero warnings required)
cargo clippy --workspace --all-targets -- -D warnings

# 5. Check formatting
cargo fmt --all -- --check

# 6. Project triage (if beads tooling installed)
br ready          # Actionable tasks (no blockers)
bv --robot-next   # Top-priority pick
```

---

## Specification

The canonical specification lives in `spec/`. **86 invariants. 32 ADRs. 7 negative cases. 2 coupling invariants.** Every invariant has six verification layers: algebraic law (Level 0), state invariant (Level 1), Rust contract (Level 2), falsification condition, proptest strategy, and Lean theorem.

| Module | INV-FERR | Focus |
|--------|----------|-------|
| [Preamble](spec/00-preamble.md) | -- | Overview, crate structure, Lean foundation |
| [Core Invariants](spec/01-core-invariants.md) | 001-012 | CRDT semilattice, indexes, snapshots, WAL, schema, identity |
| [Concurrency](spec/02-concurrency.md) | 013-024 | Checkpoint, recovery, HLC, sharding, atomicity, backpressure |
| [Performance](spec/03-performance.md) | 025-032 | Index backend, write amplification, tail latency, LIVE resolution |
| [Decisions](spec/04-decisions-and-constraints.md) | 033-036 | ADRs, NEGs, cross-shard query, partition tolerance |
| [Federation](spec/05-federation.md) | 037-044, 051-055, 060-063, 025b | Federated query, selective merge, VKN, store identity, provenance |
| [Prolly Tree](spec/06-prolly-tree.md) | 045-050 | Chunk addressing, history independence, O(d) diff |
| [Refinement](spec/07-refinement.md) | CI-FERR-001-002 | Lean-Rust coupling invariant, refinement tower |
| [Verification Infra](spec/08-verification-infrastructure.md) | 056-059 | Fault injection, soak testing, metamorphic testing, self-monitoring convergence |
| [Performance Architecture](spec/09-performance-architecture.md) | 070-076, 079-080 | Zero-copy cold start, sorted-array backend, positional addressing, Eytzinger layout |

[Full specification index ->](spec/README.md)

---

## Key Design Decisions

| ADR | Decision | Why |
|-----|----------|-----|
| ADR-FERR-001 | `im::OrdMap` persistent data structures | O(1) snapshot clones via structural sharing |
| ADR-FERR-003 | ArcSwap + Mutex concurrency | Lock-free reads (~1ns), serialized writes |
| ADR-FERR-005 | Hybrid Logical Clock | Causal ordering without central coordination |
| ADR-FERR-008 | Prolly tree block store (Phase 4b) | O(d) diff, chunk-based federation, history independence |
| ADR-FERR-010 | Two-tier wire/core types | Trust boundary: WireEntityId -> EntityId via `into_trusted()` |
| ADR-FERR-015 | ferratom-clock extraction | Zero-dependency clock leaf crate |
| ADR-FERR-020 | Localized unsafe for mmap | Single function, BLAKE3 128-bit integrity guard |
| ADR-FERR-021 | Signatures as datoms | `:tx/signature` and `:tx/signer` metadata datoms |
| ADR-FERR-030 | Wavelet matrix target (Phase 4c+) | Information-theoretic convergence: ~5 bytes/datom vs ~130 current |

---

## Performance

### Current (Phase 4a, 200K datoms)

| Metric | Value | Mechanism |
|--------|-------|-----------|
| Snapshot load | ~1 ns | ArcSwap atomic pointer load |
| EAVT point read | ~20 ns | Interpolation search on BLAKE3-uniform EntityIds |
| Secondary index read | ~80 ns | Eytzinger (BFS) layout, cache-oblivious binary search |
| Cold start | ~5.8 s (bincode), ~8 ms (mmap) | Three-level recovery cascade |
| Memory per datom | ~130 bytes (Positional) | Contiguous sorted arrays + permutation indexes |
| Write (single txn) | ~100 us | Promote -> insert -> demote cycle |

### Targets (Phase 4b+)

| Metric | Target | Mechanism | Phase |
|--------|--------|-----------|-------|
| Write throughput | 50-200K datoms/sec | WriterActor + group commit | 4b |
| Diff (d changes) | O(d * log N) | Prolly tree recursive descent | 4b |
| Federation transfer | O(\|delta\|) chunks | Merkle diff + chunk exchange | 4c |
| Memory per datom | ~5 bytes | Wavelet matrix (ADR-FERR-030) | 4c+ |
| Cold start at 100M | < 5 s | mmap zero-copy + LIVE-first layout | 4b |

---

## Dual-Representation Store

Ferratomic's most distinctive architectural feature is the promote/demote cycle:

```
Cold start  ->  Positional (sorted arrays, ns reads, O(1) mmap)
                    |
First write ->  promote() to OrdMap (O(n log n) conversion)
                    |
Insert      ->  OrdMap + SortedVecIndexes (O(log n) insert)
                    |
After txn   ->  demote() back to Positional (O(n) rebuild)
                    |
Between txns -> Positional again (optimal for read-heavy workloads)
```

This works because reads vastly outnumber writes. The O(n) demote cost is amortized over many lock-free reads at ~1 ns each.

---

## Success Criteria

Three levels, each with testable predicates. Each level subsumes the previous.

**Level 1 — Foundation Complete**
- All development phases (0 through 4d) implemented
- All Stage 0 invariants verified across 6 layers (Lean proof, proptest, Kani, Stateright, integration test, type-level)
- Zero `sorry` in Lean proofs for Stage 0 invariants
- Performance targets met: <10ms p99 point read at 100M datoms, <5s cold start at 100M datoms
- Crate dependency DAG acyclic, LOC budgets respected

**Level 2 — Production Ready**
- All Stage 1 invariants fully implemented and verified
- Multi-node federation operational (INV-FERR-037..044)
- Prolly tree block store with O(d log n) diff (INV-FERR-047)
- Datalog query engine with CALM-compliant fan-out (INV-FERR-037)
- The bootstrap test: Ferratomic's own specification stored as datoms within itself

**Level 3 — Mission Accomplished**
- Adopted as persistence substrate for real agentic systems
- The harvest/seed lifecycle operational on Ferratomic (knowledge survives conversation boundaries via the datom store)
- Multi-agent federation across heterogeneous compute environments
- Self-authored knowledge graphs: agents write associations into the datom store, retrieval improves with use, expertise accumulates in the data rather than the model

Level 1 is fully within this project's control. Level 2 depends on integration with consuming systems. Level 3 depends on external adoption.

---

## Troubleshooting

| Problem | Cause | Fix |
|---------|-------|-----|
| `/tmp` fills up during build | Cargo target dir defaults to tmpfs | `export CARGO_TARGET_DIR=/data/cargo-target` |
| `lake build` fails | Lean/mathlib version mismatch | `cd ferratomic-verify/lean && lake update && lake build` |
| proptest takes >30 min in debug | 10K cases unoptimized | `PROPTEST_CASES=1000 cargo test` for iteration; use `--release` for full suite |
| `FerraError::Backpressure` | >64 concurrent transact attempts | Expected behavior (INV-FERR-021). Retry with backoff. |
| Clippy warns about line count | Function >50 LOC | Split by responsibility. Hard limit enforced by `clippy.toml`. |

---

## Limitations

- **Phase 4a (current phase)**: MVP core engine. No federation, no Datalog queries, no prolly tree yet.
- **No SQL**: Query language is Datalog (Phase 4d). If you need SQL, use SQLite or Postgres.
- **Single-writer**: Phase 4a uses Mutex-serialized writes. WriterActor with group commit is Phase 4b.
- **No production deployment yet**: Approaching Phase 4a gate closure. Not yet tagged for release.
- **Cold start at scale**: 5.8s at 200K datoms via bincode path. mmap path (~8ms) exists but not wired into default cold_start yet.
- **BigDec scale not in type**: `Value::BigDec(i128)` does not encode decimal scale. Schema context required to distinguish 1.00 from 100.

---

## FAQ

**Q: How is this different from just using a CRDT library?**
A: CRDT libraries give you data types (counters, sets, maps). Ferratomic gives you a complete database engine with indexes, snapshots, WAL durability, schema validation, and (planned) Datalog queries -- all preserving the CRDT algebraic properties. The G-Set semilattice is the storage engine, not a library type you compose manually.

**Q: What does "formally verified" actually mean here?**
A: Every invariant has a machine-checked Lean 4 proof of its algebraic law. Zero `sorry` (unproven assumptions). The Lean model and the Rust implementation are connected by a proptest conformance bridge (CI-FERR-001) that verifies the abstract model predicts the concrete implementation's behavior. Additionally, Kani provides bounded symbolic verification and Stateright provides exhaustive protocol model checking.

**Q: Why Lean 4 instead of Coq or Isabelle?**
A: Lean 4's `Finset` from mathlib maps directly to the G-Set abstraction. The proof style is tactic-based with good automation. ADR-FERR-007 documents this decision: parallel models (Lean for algebra, Rust for implementation) rather than code extraction (which would require Aeneas, still too immature for production use).

**Q: Can I use this as an embedded database today?**
A: The core engine works (Store, Database, WAL, checkpoint, indexes, LIVE resolution, merge). Phase 4a is approaching gate closure. However, there is no stable public API or published crate yet. Pin to a git commit if experimenting.

**Q: How does content-addressed identity work with mutable entities?**
A: Entities are not mutable. A "change" is a new datom with `Op::Retract` for the old value and `Op::Assert` for the new value. Both datoms are immutable facts in the append-only store. The LIVE resolution system (INV-FERR-029) computes the "current" state by folding assert/retract events in causal (HLC) order.

**Q: What's the relationship to Datomic?**
A: Ferratomic shares the datom model (entity, attribute, value, transaction, operation) and append-only philosophy. It differs in: (1) embedded, not client-server; (2) CRDT merge, not server-coordinated; (3) content-addressed identity, not server-assigned; (4) formally verified algebraic properties; (5) cryptographic provenance via Ed25519 signed transactions.

---

## About Contributions

Please don't take this the wrong way, but I do not accept outside contributions for any of my projects. I simply don't have the mental bandwidth to review anything, and it's my name on the thing, so I'm responsible for any problems it causes; thus, the risk-reward is highly asymmetric from my perspective. I'd also have to worry about other "stakeholders," which seems unwise for tools I mostly make for myself for free. Feel free to submit issues, and even PRs if you want to illustrate a proposed fix, but know I won't merge them directly. Instead, I'll have Claude or Codex review submissions via `gh` and independently decide whether and how to address them. Bug reports in particular are welcome. Sorry if this offends, but I want to avoid wasted time and hurt feelings. I understand this isn't in sync with the prevailing open-source ethos that seeks community contributions, but it's the only way I can move at this velocity and keep my sanity.

---

## License

MIT OR Apache-2.0
