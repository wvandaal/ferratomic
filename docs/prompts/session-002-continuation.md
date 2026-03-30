# Ferratomic Continuation — Session 2

> Generated: 2026-03-30
> Last commit: 2867f5b "chore: sync beads state after session 2 completion"
> Branch: main

## Read First

1. `QUICKSTART.md` — project orientation
2. `AGENTS.md` — guidelines and constraints
3. `spec/README.md` — load only the spec modules you need

## Session Summary

### Completed
- **bd-85j.6**: Phase 3 types — all 14 ferratom types (1,404 LOC, 37 tests)
- **bd-85j.7**: Store + merge + Transaction (im::OrdSet per ADR-FERR-001, schema evolution, TxId stamping)
- **bd-85j.8**: Database (ArcSwap MVCC) + Observer (AtomicU64 monotonic epochs)
- **bd-85j.9**: WAL (FERR magic, CRC32 integrity, frame-based recovery)
- **bd-85j.11**: 19-attribute genesis schema + tx metadata datoms (strict growth)
- **bd-2pf**: Migrated Store from BTreeSet to im::OrdSet per ADR-FERR-001
- **bd-3gd + bd-22v**: Full workspace compilation — all integration tests fixed and passing
- **15 cleanroom defects** found and resolved across 3 reviews (bd-10p, bd-1n6, bd-10k, bd-kx4, bd-79n, bd-326, bd-3bg, bd-2x8, bd-1k2, bd-2sx, bd-154, bd-3n6, bd-2w9, bd-32t, bd-n59)
- **bd-thg, bd-3c7, bd-3ua**: Strict growth, snapshot O(1), genesis_with_wal — resolved

### Decisions Made
- Store uses `im::OrdSet<Datom>` not BTreeSet (ADR-FERR-001 compliance)
- `EntityId::from_bytes`, `AgentId::from_seed`, `TxId::new` gated behind `#[cfg(any(test, feature = "test-utils"))]` (INV-FERR-012)
- HLC `tick()` uses `checked_add` + busy-wait backpressure on logical overflow (INV-FERR-015)
- Database uses `std::sync::Mutex` not tokio (ADR-FERR-002)
- WAL writes POST-stamp datoms (after transact applies TxIds) but BEFORE ArcSwap publish — preserves INV-FERR-008 ordering while ensuring WAL recovery produces identical state
- Schema merge is symmetric with `debug_assert` on conflicts (INV-FERR-043)

### Bugs Found
- All 22 cleanroom defects filed as beads and resolved. Zero P0/P1 bugs remain in implemented code.
- Session 3 audit (separate session) identified Phase 4a spec gaps: 5 potentially missing invariants (INV-FERR-020..024), unspecified LIVE maintenance strategy, WriterActor vs Mutex ADR gap. These are tracked in bd-2qv epic.

### Stopping Point
All Phase 4a implementation beads from the original plan are closed (bd-85j.6 through bd-85j.11). The workspace compiles with 110 tests passing. Session 3 (parallel session) created two new Phase 4a epics:
- **bd-2qv** (spec hardening): 5+ tasks filling spec gaps
- **bd-3cn** (implementation completion): 6+ tasks for remaining code (checkpoint, storage, per-index OrdMaps, Observer broadcast, missing integration tests)

The session ended after the full workspace compilation milestone. No task is in-progress.

## Next Execution Scope

### Primary Task
**bd-1p3** [P0]: Verify or write INV-FERR-020 through 024 in spec/02-concurrency.md.

Session 3 audit found 5 invariants (Transaction Atomicity, Backpressure Safety, Anti-Entropy Convergence, No Unsafe Code, Substrate Agnosticism) that may be missing formal Level 0/1/2 definitions. The spec references them but their full contracts may be absent. This is P0 because the Curry-Howard methodology requires spec completeness before implementation acceptance.

**Acceptance**: Each of INV-FERR-020..024 has: Level 0 algebraic law, Level 1 state invariant, Level 2 implementation contract, falsification condition, proptest strategy. Or: confirmed they already exist and bd-1p3 is closed as "already present."

### Ready Queue
```bash
br ready          # ~20 actionable items
bv --robot-next   # Top pick with reasoning
```

### Dependency Context
```
bd-2qv (spec hardening) ──→ bd-3cn (impl completion) ──→ Phase 4a DONE
         │                           │
         ├─ bd-1p3 [P0] verify INV  ├─ bd-2kf [P1] Checkpoint
         ├─ bd-127 [P1] LIVE spec   ├─ bd-1z3 [P1] Per-index OrdMaps
         └─ (ADR amendments)        ├─ bd-85j.10 [P1] Observer broadcast
                                    ├─ bd-3v2 [P1] storage.rs cold_start
                                    ├─ bd-1i6 [P1] Integration tests 013..018
                                    └─ bd-20j [P2] Semilattice trait impl

bd-3gk (Phase 4b spec expansion) blocked by bd-2qv
```

Phase 4b cannot start until both bd-2qv AND bd-3cn are complete.

## Hard Constraints

- `#![forbid(unsafe_code)]` in all 4 crates
- No `unwrap()` or `expect()` in production code (verified by grep)
- `export CARGO_TARGET_DIR=/data/cargo-target` at session start (NOT auto-configured)
- Phase N+1 cannot start until Phase N passes isomorphism check
- Subagents must NEVER run cargo commands — orchestrator compiles once after all agents complete
- `EntityId::from_bytes` requires `test-utils` feature flag — not available in production builds
- Lean redundant theorem aliases are intentional for traceability (per feedback_lean_aliases.md)

## Stop Conditions

Stop and escalate to the user if:
- INV-FERR-020..024 are genuinely missing from spec and require design decisions to write
- Checkpoint format decisions conflict with the prolly tree spec (Phase 4b may supersede flat checkpoint)
- `im::OrdSet` performance degrades at scale (trigger IndexBackend fallback analysis per INV-FERR-025)
- Any test from the 110-test suite starts failing without an obvious cause
- store.rs exceeds 1,000 LOC (currently 1,022 — needs splitting per bd-3cn acceptance)
