# Ferratomic Continuation -- Session 016

> Generated: 2026-04-07
> Last commit: `946cda2` "chore(beads): sync session 016 bead closures"
> Branch: main

## Read First

1. `QUICKSTART.md` -- project orientation
2. `AGENTS.md` -- guidelines and constraints
3. `spec/README.md` -- load only the spec modules you need

## Session Summary

### Completed

**11-crate decomposition (EPIC bd-cly9) -- COMPLETE:**
- bd-nt71: ferratomic-tx (369 LOC, 16 tests)
- bd-8fr9: ferratomic-storage (461 LOC, 4 tests)
- bd-bc41: ferratomic-wal (796 LOC, 11 tests) -- WAL decoupled from Transaction
- bd-nb12: ferratomic-index (609 LOC, 3-file split for Gate 8)
- bd-q0ys: ferratomic-positional (1,514 LOC, 8-module split, 21 tests)
- bd-bb9r: ferratomic-checkpoint (1,502 LOC, 21 tests) -- Store dependency broken via CheckpointData
- bd-ipln: ferratomic-store (2,256 LOC, 41 tests) -- CRDT algebra core
- bd-wrrg: ferratomic-core renamed to ferratomic-db (49 files updated)

**Performance EPIC (bd-4i6u) -- 4/20 beads closed:**
- bd-k4ex: Transaction::into_datoms() zero-clone
- bd-pb3b: Fix 4 threshold test failures (cold start + read latency)
- bd-wv6v: Borrow-based Eytzinger key comparison (cmp_datom on all 4 key types)
- bd-zwvb: WA measurement bincode baseline

**Cleanroom audits:** 5 audit rounds (10 subagent audits total), all findings resolved.

### Decisions Made
- ferratomic-core renamed to ferratomic-db (package name, directory unchanged)
- CheckpointData raw-data struct breaks Store circular dependency
- WAL append() removed; only append_raw() remains (frame-level purity)
- CI + pre-commit Gate 6 updated for 11 crates + ADR-FERR-020 deny exception
- Subagent cargo commands banned (caused competing threshold test processes)

### Bugs Found
- bd-pb3b: 4 threshold tests failed (store.indexes().unwrap() on Positional; epoch=0 from from_datoms)
- bd-zwvb: WA measurement used serde_json instead of bincode baseline

### Stopping Point
All 4 performance beads committed and pushed. All audit findings fixed. Session ending at ~65% context.

## Next Execution Scope

### Primary Task
**Continue performance EPIC (bd-4i6u).** 16 beads remain. Next high-impact picks:

```
bd-0zfw: Chunk fingerprint array (keystone, unblocks 2) — READY, P0
bd-fnod: Attribute interning u16 dictionary — READY, P1
bd-86ap: Checkpoint serialize from slice — READY, P1 (needs CheckpointData API change)
bd-t84f: Rank9/Select succinct LIVE bitvector — READY, P1
bd-iltk: SIMD XOR fingerprint — READY, P1
```

### Ready Queue
```bash
br ready          # Show unblocked issues
bv --robot-next   # Top pick with reasoning
```

### Dependency Context (performance)
```
Tier 1 (transact hot path):
  bd-0zfw (chunks) → bd-nq6v (inc. LIVE) → bd-886d (splice) → bd-ks5d (batch)

Tier 2 (alien data structures):
  bd-fnod (intern) → bd-574c (SoA) → bd-mdfq (entity RLE), bd-3ta0 (TxId perm)
  bd-t84f (rank/select), bd-iltk (SIMD fp) — independent

Tier 3 (information-theoretic):
  bd-wows (PinSketch), bd-m7te (entropy checkpoint) — depend on Tier 2
```

### Phase 4a gate path
```
bd-add (Phase 4a gate) blocked by:
  ✓ bd-cly9 (decomposition) — CLOSED this session
  ○ bd-7fub.22.10 (re-review 10.0/A+) — IN_PROGRESS, assessment only
  ○ bd-4i6u (performance 10.0) — 4/20 closed, 16 remaining
  ○ bd-y1w5 (tag gate) — procedural, after 22.10 + 4i6u
```

## Hard Constraints

- Safe callable surface: `#![forbid(unsafe_code)]` default; `#![deny(unsafe_code)]` for ferratomic-db and ferratomic-checkpoint (ADR-FERR-020 mmap)
- No `unwrap()` in production code (NEG-FERR-001)
- `CARGO_TARGET_DIR=/data/cargo-target`
- All 11 crates in acyclic DAG: clock → ferratom → {tx, storage, wal} → index → positional → checkpoint → store → db → datalog
- Subagents MUST NOT run cargo commands — orchestrator compiles once
- Performance weight 2.5x — profile every structural change
- Zero `#[allow(...)]` anywhere — fix root causes

## Stop Conditions

Stop and escalate to the user if:
- A performance bead requires changing the Store algebraic structure (INV-FERR-001-004)
- CheckpointData API changes break ferratomic-checkpoint ↔ ferratomic-store boundary
- Any bead's Pseudocode Contract doesn't match what the code actually needs
- Test failures beyond the known threshold bugs (all 4 now fixed)
- A crate dependency edge would create a cycle in the DAG
