# Ferratomic — Quick Orientation

**What**: Formally verified, distributed embedded datom database engine.
**Why**: Foundation for braid and any system built on the datom model.
**Core property**: Store = (P(D), ∪) — G-Set CRDT semilattice. Writes never conflict.

## Current Phase

Phase 0 (specification) is COMPLETE. Next: **Phase 1 (Lean proofs)**.

## Where to Start

1. Read `AGENTS.md` — guidelines, constraints, quality standards
2. Read `docs/prompts/session-001-bootstrap.md` — your execution guide
3. Read `spec/README.md` — spec module index (load only what you need)

## Crate Map

```
ferratom/           → Core types (Datom, EntityId, Value, Schema). ZERO deps.
ferratomic-core/    → Engine (Store, Database, WAL, snapshots). Depends on ferratom.
ferratomic-datalog/ → Query engine (Datalog). Depends on core.
ferratomic-verify/  → Proofs + tests (Lean 4, Stateright, Kani, proptest).
```

## Build

```bash
CARGO_TARGET_DIR=/data/cargo-target cargo check --workspace
CARGO_TARGET_DIR=/data/cargo-target cargo test --workspace
cd ferratomic-verify/lean && lake build   # Lean proofs
```

## Phase Order (non-negotiable)

```
spec (DONE) → Lean proofs → tests (red) → types → implementation
```

No phase N+1 until phase N passes. No implementation until proofs + tests exist.
