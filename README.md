# Ferratomic

**A formally verified, distributed embedded datom database engine.**

Ferratomic is the storage foundation for systems built on the datom model — an append-only, content-addressed, conflict-free knowledge substrate. It provides the algebraic guarantees of a CRDT with the performance characteristics of a modern embedded database and the trust properties of a cryptographically verified network.

## Core Properties

```
Store = (P(D), ∪)
```

The store is a grow-only set of datoms under set union. This single equation gives you:

- **Conflict-free merge.** Any two stores merge by set union. No conflicts, no consensus protocol, no coordination. The data structure IS the consistency mechanism.
- **Append-only history.** Every state the system has ever been in is recoverable. Nothing is deleted or mutated.
- **Content-addressed identity.** Same fact = same datom, regardless of who asserts it or when. Identity by BLAKE3 hash.
- **Causal traceability.** Every fact records its provenance: who, when, why, and what was known at the time.

## Architecture

```
┌─────────────────────────────────┐
│       Application Layer          │  ← your application
├─────────────────────────────────┤
│     ferratomic-datalog           │  ← Datalog query engine
├─────────────────────────────────┤
│     ferratomic-core              │  ← Storage + concurrency engine
│  ┌───────────┐ ┌──────────────┐ │
│  │ ArcSwap   │ │ WriterActor  │ │  ← Lock-free reads, single writer
│  │ snapshots │ │ group commit │ │
│  └───────────┘ └──────────────┘ │
│  ┌───────────┐ ┌──────────────┐ │
│  │ Prolly    │ │ WAL + check- │ │  ← O(d) diff, structural sharing
│  │ tree      │ │ point        │ │
│  └───────────┘ └──────────────┘ │
├─────────────────────────────────┤
│       ferratom                   │  ← Core types (zero dependencies)
└─────────────────────────────────┘
```

### Crates

| Crate | Role | Dependencies |
|-------|------|-------------|
| `ferratom` | Core types: Datom, EntityId, Value, Schema, HLC | blake3, ordered-float, serde |
| `ferratomic-core` | Engine: Store, MVCC snapshots, WAL, observers, federation | ferratom, im, arc-swap, asupersync |
| `ferratomic-datalog` | Query: Datalog parser, planner, evaluator, CALM classification | ferratom, ferratomic-core |
| `ferratomic-verify` | Proofs: Lean 4, Stateright, Kani, proptest | ferratom, ferratomic-core |

## Key Design Decisions

| Decision | Choice | Why |
|----------|--------|-----|
| Data structures | `im::OrdMap` (persistent) | O(1) snapshot clones via structural sharing |
| Concurrency | ArcSwap + single writer | Lock-free reads (~1ns), zero contention |
| Storage | Prolly tree block store | O(d) diff, chunk-based federation, on-disk structural sharing |
| Clock | Hybrid Logical Clock | Causal ordering without central coordination |
| Async | Asupersync (native) | Structured concurrency, DPOR testing, cancel-aware `&Cx` |
| Signing | Ed25519 per transaction | Trustless verification, 5µs sign / 2µs verify |
| Trust | Query-level predicate | `TrustPolicy::Calibrated(accuracy, samples)` |

## Verification

Every invariant is verified at three levels before implementation begins:

| Layer | Tool | What it proves |
|-------|------|---------------|
| **Algebraic proofs** | Lean 4 + mathlib | CRDT laws, index bijection, HLC causality |
| **Model checking** | Stateright | Snapshot isolation, merge convergence, federation |
| **Bounded verification** | Kani | Index consistency, WAL ordering, value safety |
| **Property testing** | proptest | Round-trip, monotonicity, Lean-Rust conformance |
| **Integration** | E2E tests | Lifecycle, recovery, observer delivery, federation |

**55 invariants. 9 ADRs. 5 negative cases.** [Full specification →](spec/README.md)

## Performance Targets

| Metric | Target | How |
|--------|--------|-----|
| Snapshot load | < 5ns | ArcSwap atomic pointer load |
| Point read | < 10µs | im::OrdMap at 100M datoms |
| Write throughput | 50-200K datoms/sec | Group commit, WAL fsync batching |
| Diff (d changes) | O(d × log N) | Prolly tree recursive descent |
| Federation transfer | O(\|Δ\|) chunks | Only missing chunks cross the network |
| Cold start | < 5s at 100M | Compressed checkpoint + lazy index |

## Quick Start

```bash
git clone https://github.com/wvandaal/ferratomic
cd ferratomic

# Build
CARGO_TARGET_DIR=/data/cargo-target cargo check --workspace

# Test
CARGO_TARGET_DIR=/data/cargo-target cargo test --workspace

# Lean proofs
cd ferratomic-verify/lean && lake build

# Project triage
br ready          # What's actionable
bv --robot-next   # Single top pick
```

## Development Methodology

**Spec-first TDD with Curry-Howard-Lambek correspondence.**

```
spec → Lean proofs → tests (red) → types → implementation
```

No phase N+1 until phase N passes its isomorphism check. A gap between spec, algebra, and tests is a defect — not technical debt. [Development guide →](docs/prompts/session-001-bootstrap.md)

## Specification

| Module | Invariants | Focus |
|--------|-----------|-------|
| [Core](spec/01-core-invariants.md) | INV-FERR-001..012 | CRDT semilattice, indexes, snapshots, identity |
| [Concurrency](spec/02-concurrency.md) | INV-FERR-013..024 | Checkpoint, recovery, HLC, atomicity, substrate |
| [Performance](spec/03-performance.md) | INV-FERR-025..032 | Write amplification, tail latency, LIVE resolution |
| [Decisions](spec/04-decisions-and-constraints.md) | INV-FERR-033..036 | ADRs, NEGs, cross-shard query, partition tolerance |
| [Federation + VKN](spec/05-federation.md) | INV-FERR-037..055 | Federated query, selective merge, cryptographic provenance |
| [Prolly Tree](spec/06-prolly-tree.md) | INV-FERR-045..050 | Chunk addressing, O(d) diff, block store |

## License

MIT OR Apache-2.0
