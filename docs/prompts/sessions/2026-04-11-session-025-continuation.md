# Ferratomic Continuation — Session 025

> Generated: 2026-04-11
> Last commit: `1c833c8` "feat: pre-commit LOC budgets + 6 audit reminders (tracked)"
> Branch: main (synced with master)

## Read First

1. `QUICKSTART.md` — Phase 4a.5 CLOSED this session
2. `AGENTS.md` — guidelines and constraints
3. `GOALS.md` §7 — Six-Dimension Decision Evaluation Framework
4. `spec/README.md` — 93 INV, 35 ADR, 7 NEG, 2 CI
5. This continuation prompt

## Session Summary

### Completed

**Phase 4a.5 CLOSED.** The largest single-session implementation push in project history.

**13 beads closed** (full dependency chain):
bd-b7pfg (Attribute u16) → bd-qguw (canonical bytes) → bd-6j0r (Ed25519 signing) → bd-3t63 (TransactContext + federation metadata) → bd-mklv (genesis_with_identity + transact_signed) → bd-sup6 (selective_merge) → bd-h51f (ReplicaFilter) → bd-1rcm (Transport trait) → bd-7dkk (filtered observers) → bd-lifv (LocalTransport) → bd-hlxr (integration tests) → bd-r7ht (bootstrap test) → bd-r3um (gate close)

**4 cleanroom reviews, 20 defects found and fixed:**
- 2 CRITICAL: signing pre-stamp datoms (DEFECT-025-003), selective_merge live_causal unfiltered (DEFECT-001)
- 5 MAJOR: Attribute Deserialize bypass, no genesis/signing tests, predecessor extraction, O(n*m) containment, group_into_bundles semantics
- 7 MINOR + STYLE: duplicate docs, weak assertions, unsafe noop_waker, WAL test doc

**4 performance optimizations:**
- signing_message: hash_signing_fields() streams directly into BLAKE3 (zero Vec alloc)
- selective_merge: merge_sort_dedup O(n+m) instead of sort+dedup O(n log n)
- observer: unfiltered publish passes slice directly (zero clone)
- signing predecessors: frontier entries → EntityIds (D19)

**3 ADRs authored:** ADR-FERR-037 (signing TxId exclusion), ADR-FERR-038 (two-tx identity bootstrap), ADR-FERR-039 (merge receipt as struct)

**Spec alignment:** INV-FERR-060/062/051 Level 1/2 contracts updated to match implementation. Zero spec-implementation drift for Phase 4a.5.

**Infrastructure:** Pre-commit hook upgraded with LOC budget enforcement (Gate 8) + 6 audit reminders mapped to lifecycle prompts. Tracked in `scripts/pre-commit`.

**Tests:** 878 total. 5 E2E federation tests, 6 integration tests, 1 bootstrap test, ~35 unit tests added.

### Decisions Made (locked)

- **D-025-1 / ADR-FERR-037**: Signing message excludes per-datom TxId. Covered separately via tx_id_canonical_bytes.
- **D-025-2 / ADR-FERR-038**: genesis_with_identity uses two transactions (schema def + value assertion). Required by commit() schema validation.
- **D-025-3 / ADR-FERR-039**: selective_merge returns (Store, MergeReceipt). Receipt datom emission deferred to Database layer (Phase 4c).
- **D-025-4**: selective_merge rebuilds live_causal from actual merged datoms (not unfiltered remote).
- **D-025-5**: Transport trait: Pin<Box<dyn Future + Send + 'a>>, zero async runtime deps.

### Bugs Found

- **DEFECT-025-003 (CRITICAL)**: Signing message hashed pre-stamp datoms with placeholder TxId. Fixed by hash_signing_fields() excluding TxId.
- **DEFECT-001 (CRITICAL)**: selective_merge merged live_causal from full remote, including filtered-out datoms. Fixed by rebuilding from actual merged set.
- **8 pre-existing bugs**: tx/provenance SchemaViolation (3 test files), broken doctests (2), prolly/build.rs FerraError::Validation (1), prolly/read.rs cast truncation (1), prolly/boundary.rs doc backticks (1).

### Stopping Point

**Exactly where I stopped**: All commits pushed. Working tree has unstaged changes from parallel agent (spec/06, spec/README.md, beads). Phase 4a.5 gate closed. Pre-commit hook installed and tested.

**The last thing I verified working**: All pre-commit gates pass. Audit reminders fire correctly (sorry count 3, spec drift 2). E2E + bootstrap + integration tests all green.

**The next thing to do**: Address Phase 4b cleanup (Lean sorry count, LOC budgets) then continue Phase 4b prolly tree implementation.

## Next Execution Scope

### Primary Task

**Phase 4b prolly tree** — parallel agent has foundation (bd-85j.13). Remaining: chunk boundary, history independence, diff algorithm. The prolly code needs the same cleanroom treatment Phase 4a.5 received.

### Before Phase 4b implementation

1. **Lean sorry count**: 3 (was 1). Parallel agent added 2 in prolly tree. Run lifecycle/18.
2. **LOC budget overages**: ferratom 6824/2000, ferratomic-core 9917/2000. Consider crate split or justified ADR.
3. **Pre-commit hook install**: New sessions must run `ln -sf ../../scripts/pre-commit .git/hooks/pre-commit`.

### Ready Queue

```bash
br ready
bv --robot-next
```

### Dependency Context

```
Phase 4a  ✅ (v0.4.0-gate)
Phase 4a.5 ✅ (session 025, 1c833c8)
     ↓              ↓
Phase 4b (active)  Phase 4c (unblocked, needs STRIDE threat model)
     ↓              ↓
     └──────┬───────┘
            ↓
        Phase 4d (Datalog)
```

## Hard Constraints

- `#![forbid(unsafe_code)]` by default. No `unwrap()` in production.
- `CARGO_TARGET_DIR=/data/cargo-target`
- **FROZEN**: signing message format (ADR-FERR-037), canonical_bytes layout (c681e80)
- **FROZEN**: content_hash IS BLAKE3(canonical_bytes layout)
- ed25519-dalek 2.2.0 workspace dependency
- TransactContext is the Database→Store transact interface
- Database::frontier() is live (Mutex<Frontier>, advances on each transact)
- Pre-commit hook: `scripts/pre-commit` — LOC budgets + 6 audit reminders

## Stop Conditions

Stop and escalate if:
- Signing message format change (FROZEN per ADR-FERR-037)
- canonical_bytes layout change (FROZEN per c681e80)
- Lean model-impl drift on Stage 0 invariant
- Prolly tree conflicts with federation metadata (tx/* datoms in chunk encoding)
- LOC budget hard violation in a leaf crate (pre-commit blocks)
