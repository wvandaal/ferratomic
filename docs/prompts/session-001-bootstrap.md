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
