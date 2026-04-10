> **Namespace**: FERR | **Wave**: 1 (Foundation) | **Stage**: 0
> **Shared definitions**: [00-preamble.md](00-preamble.md) (conventions, verification tags, constraints)

### External Traceability

This specification traces to two foundational documents:

- **SEED.md** — The foundational design document defining datoms, the harvest/seed
  lifecycle, and the bootstrap philosophy. Referenced as `SEED.md §N` throughout.
  The referenced axioms are:

  - **§1** (Foundational Epistemology): "Verifiable coherence" — knowledge must be
    traceable, integrity-verified, and relationally confirmed. Drives C2, C5,
    and the federation trust architecture.
  - **§4** (Core Abstraction): Datoms, identity, snapshots, schema-as-data, CRDT
    merge, substrate independence. Axiom 1 (append-only, C1), Axiom 2 (store =
    `(P(D), ∪)`, C4, L1-L5), Axiom 3 (snapshots, INV-FERR-006). Design
    commitments: content-addressed identity (C2), schema-as-data (C3),
    traceability (C5), substrate independence (C8), temporal ordering (HLC),
    "CRDT merge scales learning across organizations" (federation).
  - **§5** (Harvest/Seed Lifecycle): Durability, recovery, knowledge persistence
    across conversation boundaries. Drives WAL-before-snapshot (INV-FERR-008),
    crash recovery (INV-FERR-014), append-only guarantees.
  - **§9.2** (Central Finding): The memory architecture bottleneck in agentic
    systems — substrate divergence is the primary scaling obstacle. Drives
    INV-FERR-024 (Substrate Agnosticism).
  - **§10** (The Bootstrap): Self-hosting, genesis, self-verifying systems.
    Schema that defines datoms is itself stored as datoms (C7). Drives
    INV-FERR-013, INV-FERR-014, ADR-FERR-007.

- **Historical STORE algebra** — the precursor STORE namespace defined the five lattice
  laws preserved verbatim in Ferratomic. In this repository, that algebra now lives across
  [01-core-invariants.md](01-core-invariants.md), [02-concurrency.md](02-concurrency.md),
  and [03-performance.md](03-performance.md):
  - **L1**: Merge commutativity (`merge(A, B) = merge(B, A)`)
  - **L2**: Merge associativity (`merge(merge(A, B), C) = merge(A, merge(B, C))`)
  - **L3**: Merge idempotency (`merge(A, A) = A`)
  - **L4**: Monotonic growth (`S ⊆ apply(S, d)`)
  - **L5**: Strict growth for transactions (`|transact(S, T)| > |S|`)

### Traceability Conventions

Canonical trace targets in this repository are:
- `SEED.md §N` for external design axioms
- `INV-STORE-*` and `L1`..`L5` for historical STORE algebra
- `INV-FERR-*`, `ADR-FERR-*`, `NEG-FERR-*`, and `CI-FERR-*` for current Ferratomic elements

`Source` blocks may also cite local review artifacts and bead records, but they must do
so by explicit artifact name or file path, for example
`docs/reviews/2026-03-31-cleanroom-audit-phase4a.md (HI-004)`.

Deprecated shorthand such as `SR-*`, `PD-*`, `FD-*`, `AS-*`, `ADR-STORE-*`, and
`ADR-FOUNDATION-*` is not canonical and must not appear in edited spec content.

### Constraints

Referenced as `C1`, `C2`, etc. throughout the spec:

| ID | Name | Definition |
|----|------|-----------|
| C1 | Append-only store | Never delete or mutate datoms. Retractions are new datoms. |
| C2 | Content-addressed identity | EntityId = BLAKE3(content). |
| C3 | Schema-as-data | Schema is defined by datoms, not hardcoded. Schema evolution is a transaction. |
| C4 | CRDT merge = set union | Commutative, associative, idempotent. Merge is pure set union. |
| C5 | Causal traceability | Every datom records its provenance: who, when, why, and what was known. |
| C6 | *(Reserved — intentionally skipped. No constraint was assigned to C6.)* | |
| C7 | Self-bootstrap | Genesis is deterministic and self-describing. The schema describes itself. |
| C8 | Substrate independence | Ferratomic has no knowledge of application-layer concerns. **C8 Test**: "If someone used Ferratomic for a game engine, IoT sensors, or a financial ledger, would every engine-level name (type, attribute, function, genesis schema entry) make sense without knowing the intended use case? If any name assumes a specific domain (e.g., 'agent' assumes AI agents), it violates C8." Engine primitives use domain-neutral terms: NodeId (not AgentId), tx/origin (not tx/agent). Application-layer conventions (e.g., `:agent/*` namespace patterns) are permitted because they are optional patterns, not engine primitives. |
| C9 | Balanced workload | Workload is balanced/bursty: write bursts, read bursts, interleaved. NOT read-heavy 99:1. Performance architecture must optimize for both read and write throughput. |

## 23.0 Preamble

### Stage Definitions

- **Stage 0**: Core algebraic invariant. Required for MVP (Phase 4a).
- **Stage 1**: Extended invariant. Required for production (Phase 4b+).
- **Stage 2**: Future invariant. Designed now, implemented when needed.

### 23.0.1 Overview

Ferratomic is the embedded datom database engine that reifies the algebraic store `(P(D), ∪)`
specified across [01-core-invariants.md](01-core-invariants.md),
[02-concurrency.md](02-concurrency.md), and [03-performance.md](03-performance.md) as a
production-grade storage system. Where those modules define the mathematical object — the
G-Set CvRDT, the five lattice laws, the transaction
algebra — Ferratomic specifies the **engineering substrate** that makes those laws hold under
real-world conditions: concurrent writers, crash recovery, disk corruption, memory pressure,
and multi-process access.

The relationship between STORE and FERR is analogous to the relationship between a group axiom
and a concrete group representation: STORE says `MERGE(A, B) = MERGE(B, A)`; FERR says how
that commutativity is preserved when A and B are 50GB memory-mapped files being written by
independent OS processes that may crash at any byte boundary.

**Traces to**: SEED.md 4 (Core Abstraction: Datoms), SEED.md 5 (Harvest/Seed Lifecycle),
SEED.md 10 (The Bootstrap)

**Design principles**:

1. **Algebraic fidelity.** Every FERR invariant is a refinement of a STORE axiom. No FERR
   invariant may contradict or weaken any STORE law L1-L5. The refinement relation is
   formally: `INV-FERR-NNN refines INV-STORE-MMM` means that any system satisfying
   INV-FERR-NNN necessarily satisfies INV-STORE-MMM.

2. **Verification depth.** Every invariant carries six verification layers: algebraic law
   (Level 0), state invariant (Level 1), implementation contract (Level 2), falsification
   condition, proptest strategy, and Lean 4 theorem. The Lean theorems are mechanically
   checkable proofs that the algebraic laws hold for the `DatomStore := Finset Datom` model.

3. **Crash-safety first.** The WAL-before-snapshot discipline (INV-FERR-008) is the
   load-bearing durability guarantee. All other durability properties derive from it.

4. **Content-addressed everything.** Entity identity, transaction identity, and index entries
   are all derived from content hashes (BLAKE3). This eliminates allocation coordination
   across replicas and makes deduplication a structural tautology.

5. **Substrate independence (C8).** Ferratomic is a general-purpose embedded datom database.
   It has no knowledge of application-layer methodology, commands, or domain elements.
   It stores `[e, a, v, tx, op]` tuples and enforces schema constraints. Everything
   domain-specific enters through the schema layer, not the engine.

6. **Asupersync-first concurrency (ADR-FERR-002).** Asupersync is the primary async runtime.
   Structured concurrency via `Scope::spawn`, cancel-aware primitives (`&Cx`), two-phase
   reserve/commit channels, obligation tracking, and deterministic testing via DPOR/LabRuntime.
   Tokio is confined to explicit `asupersync-tokio-compat` adapter modules for tokio-only
   dependencies. Core domain code must not depend on tokio.

### 23.0.2 Crate Structure

```
ferratomic/                          -- workspace root
├── ferratom-clock/                  -- HLC, TxId, NodeId, Frontier
│   └── src/lib.rs                   -- Zero project-internal dependencies
├── ferratom/                        -- Primitive datom/schema/value/wire types
│   ├── src/lib.rs                   -- Re-export facade for clock/datom/schema types
│   ├── src/clock/                   -- Clock re-exports and leaf integration
│   ├── src/datom/                   -- Datom, EntityId, Value, Op
│   ├── src/error.rs                 -- Typed FerraError surface
│   ├── src/schema.rs                -- Schema and AttributeDef
│   ├── src/traits.rs                -- Shared trait boundaries
│   └── src/wire.rs                  -- Wire/core trust boundary
├── ferratomic-core/                 -- Storage engine: Store, WAL, recovery, observers
│   ├── src/lib.rs                   -- Public module surface
│   ├── src/db/                      -- Database, transact path, recovery constructors
│   ├── src/store/                   -- Store struct, apply/merge/query logic
│   ├── src/storage/                 -- Backend abstraction + cold start cascade
│   ├── src/wal/                     -- Write-ahead log with fsync ordering
│   ├── src/writer/                  -- Transaction builder + validation
│   ├── src/anti_entropy.rs          -- Anti-entropy trait boundary
│   ├── src/backpressure.rs          -- Write limiter
│   ├── src/checkpoint.rs            -- Durable snapshot format
│   ├── src/indexes.rs               -- Secondary index key types
│   ├── src/merge.rs                 -- CRDT merge facade
│   ├── src/observer.rs              -- Observer trait
│   ├── src/schema_evolution.rs      -- Genesis/schema evolution helpers
│   ├── src/snapshot.rs              -- Snapshot types
│   ├── src/topology.rs              -- Replica filter/topology surface
│   └── src/transport.rs             -- Federation transport boundary
├── ferratomic-datalog/              -- Query engine: Datalog dialect, semi-naive evaluation
│   ├── src/parser.rs                -- EDN-based Datalog parser
│   ├── src/planner.rs               -- Query plan generation with stratum classification
│   └── src/evaluator.rs             -- Semi-naive evaluation with CALM compliance
└── ferratomic-verify/               -- Verification harnesses: proptest, kani, stateright
    ├── src/generators.rs            -- Arbitrary instances for core types
    ├── kani/                        -- Bounded model checking proofs
    ├── proptest/                    -- Property tests and conformance checks
    └── stateright/                  -- Protocol model checking
```

**Dependency DAG** (acyclic, strict):
```
ferratom-clock  <--  ferratom  <--  ferratomic-core  <--  ferratomic-datalog
                         ^              ^
                         |              |
                         +--------------+-------  ferratomic-verify  (dev-dependency only)
```

`ferratom-clock` has zero project-internal dependencies. `ferratom` depends on
`ferratom-clock` plus external crates (`blake3`, `ordered-float`, `serde`).
`ferratomic-core` depends on `ferratom`, `im`, `arc-swap`, `blake3`, `bincode`,
`serde`, `asupersync`, and optional `tokio` adapter support. `ferratomic-datalog`
depends on `ferratom` and `ferratomic-core`. `ferratomic-verify` is a
dev-dependency workspace member that currently imports `ferratom` and
`ferratomic-core` directly for testing.

### 23.0.3 Relationship to the Historical STORE Algebra

The historical STORE namespace defines the algebraic specification: the datom type, the
store as `(P(D), ∪)`, the five lattice laws L1-L5, the transaction algebra, the value
domain, and the index invariants. In this repository, those preserved foundations are
distributed across [01-core-invariants.md](01-core-invariants.md),
[02-concurrency.md](02-concurrency.md), and [03-performance.md](03-performance.md).
The FERR namespace adds:

| STORE provides | FERR adds |
|----------------|-----------|
| L1-L3 (CRDT axioms) | Concrete merge implementation with crash-safety (INV-FERR-001 through INV-FERR-003) |
| L4-L5 (monotonicity, growth) | Monotonic growth with WAL durability (INV-FERR-004, INV-FERR-008) |
| Index invariants (EAVT, AEVT, VAET, AVET, LIVE) | Index bijection with crash-recovery (INV-FERR-005) |
| Transaction algebra | Snapshot isolation + write linearizability (INV-FERR-006, INV-FERR-007) |
| Schema-as-data | Schema validation at transact boundary (INV-FERR-009) |
| Content-addressed identity axiom | BLAKE3 content addressing (INV-FERR-012) |
| — | Merge convergence proof (INV-FERR-010) |
| — | Observer epoch monotonicity (INV-FERR-011) |

Every INV-FERR invariant traces to a STORE axiom or SEED.md section. No INV-FERR invariant
introduces a property not implied by the algebraic specification — it only specifies how
that property is maintained under real-world failure modes.

### 23.0.4 Lean 4 Foundation Model

The Lean 4 theorems throughout this specification operate on the following definitions:

```lean
import Mathlib.Data.Finset.Basic
import Mathlib.Data.Finset.Lattice
import Mathlib.Order.BooleanAlgebra

/-- A datom is an opaque five-tuple. For the algebraic model,
    we abstract over the concrete field types. -/
structure Datom where
  e  : Nat    -- entity (content-addressed, modeled as Nat for finiteness)
  a  : Nat    -- attribute
  v  : Nat    -- value (abstracted)
  tx : Nat    -- transaction
  op : Bool   -- true = assert, false = retract
  deriving DecidableEq, Repr

/-- A datom store is a finite set of datoms. -/
def DatomStore := Finset Datom

/-- Merge is set union. -/
def merge (a b : DatomStore) : DatomStore := a ∪ b

/-- Apply (transact) adds datoms to the store. -/
def apply_tx (s : DatomStore) (d : Datom) : DatomStore := s ∪ {d}

/-- Store cardinality (number of distinct datoms). -/
def store_size (s : DatomStore) : Nat := s.card

/-- Content-addressed identity: a datom's identity IS its content. -/
def datom_id (d : Datom) : Datom := d  -- identity function (tautological by construction)
```

#### Model Fidelity (ADR-FERR-007)

The Lean model is a **parallel abstraction**, not a code extraction. It proves properties
of the algebraic structure (`Finset Datom` with `Nat` fields), not of the Rust implementation
(`im::OrdMap` with BLAKE3 hashes). This is intentional per ADR-FERR-007 (parallel models).

The following aspects are **abstracted away** in Lean and verified by other layers:

| Aspect | Lean abstracts as | Verified instead by | INV-FERR |
|--------|------------------|--------------------|---------|
| Content-addressed hashing | Structural equality (`Datom.ext`) | proptest + Kani (BLAKE3 roundtrip) | 012 |
| Binary serialization | `toTuple`/`ofTuple` record destructuring | proptest + integration (byte-level) | 013 |
| Epoch-ordered LIVE resolution | `foldl apply_op` without epoch sorting | proptest + Kani (epoch-aware fold) | 029, 032 |
| Ed25519 signatures | Axiomatized `ed25519_correctness` | proptest + integration (real ed25519-dalek) | 051 |
| Merkle proof verification | Axiomatized `inclusion_proof_complete` | proptest + Kani (real prolly tree) | 052, 053 |
| VKC 3-part verification | Signature check only (context/calibration axiomatized) | proptest + integration | 055 |

#### Verification Layer Tags: Lean Abstraction Levels

| Tag | Scope | Meaning |
|-----|-------|---------|
| `V:LEAN-ABSTRACT` | `Finset Datom` (abstract model) | Proof operates on the set-theoretic model only. Proves algebraic identity at the set level. Does NOT verify concrete data structure implementation correctness. |
| `V:LEAN-CONCRETE` | Abstract model + concrete representation | Proof connects abstract model to concrete representation (e.g., sorted array to Finset bijection). Verifies that implementation-level operations preserve abstract properties. |

The **conformance bridge** (proptest matching Lean predictions) catches divergence between
the abstract model and concrete implementation. A property that holds in Lean but fails in
proptest indicates an implementation bug. A property that fails in Lean is an algebraic defect.

### 23.0.5 Stateright Foundation Model

The Stateright models throughout this specification operate on the following state machine:

```rust
use stateright::*;
use std::collections::BTreeSet;

/// A datom is a content-addressed five-tuple.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
struct Datom {
    e: u64,
    a: u64,
    v: u64,
    tx: u64,
    op: bool, // true = assert, false = retract
}

/// CRDT state: N nodes, each holding a G-Set of datoms, with in-flight merges.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct CrdtState {
    nodes: Vec<BTreeSet<Datom>>,
    in_flight: Vec<(usize, usize, BTreeSet<Datom>)>, // (from, to, payload)
}

/// Actions available to the model checker.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
enum CrdtAction {
    Write(usize, Datom),                  // node writes a datom
    InitMerge(usize, usize),             // node initiates merge to peer
    DeliverMerge(usize),                  // deliver in-flight merge at index
}
```

---
