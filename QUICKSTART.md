# Ferratomic — Quick Orientation

**What**: Formally verified, distributed embedded datom database engine.
**Why**: General-purpose storage foundation for any system built on the datom model.
**Core property**: Store = (P(D), ∪) — G-Set CRDT semilattice. Writes never conflict.

**Canonical spec**: `spec/` is the canonical specification.

**Current state**: The workspace does NOT compile. Phase 3 (type definitions) will create
type stubs that make Phase 2 tests compilable. Phase 4 implementation makes them pass.

## Current Phase

Phase 1 (Lean proofs) is COMPLETE. Phase 2 (tests) is COMPLETE for MVP scope (INV-FERR-001..024).
Next: **Phase 3 (type definitions)**.

## Where to Start

1. Read `AGENTS.md` — guidelines, constraints, quality standards
2. Read `docs/prompts/session-001-bootstrap.md` — your execution guide
3. Read `spec/README.md` — spec module index (load only what you need)

Check project state: `br ready` (actionable tasks), `br list --status in_progress` (claimed work),
`bv --robot-next` (top pick).

## Crate Map

```
ferratom/           → Core types (Datom, EntityId, Value, Schema). ZERO deps.
ferratomic-core/    → Engine (Store, Database, WAL, snapshots). Depends on ferratom.
ferratomic-datalog/ → Query engine (Datalog). Depends on core.
ferratomic-verify/  → Proofs + tests (Lean 4, Stateright, Kani, proptest).
```

## Build

**CRITICAL**: Set `export CARGO_TARGET_DIR=/data/cargo-target` at session start.
This is NOT auto-configured. Omitting it uses /tmp (RAM-backed, will fill up).

```bash
export CARGO_TARGET_DIR=/data/cargo-target
cargo check --workspace
cargo test --workspace
cd ferratomic-verify/lean && lake build   # Lean proofs
```

## Phase Order (non-negotiable)

```
spec (DONE) → Lean proofs → tests (red) → types → implementation
```

No phase N+1 until phase N passes. No implementation until proofs + tests exist.
