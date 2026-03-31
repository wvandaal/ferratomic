# Ferratomic Continuation — Session 003: Bead Crystallization

> Generated: 2026-03-30
> Last commit: 188ebdb "fix: Phase 4a hardening — 20 cleanroom bugs + quality gates"
> Branch: main

## Read First

1. `AGENTS.md` — guidelines and constraints
2. `spec/README.md` — load only the spec modules you need
3. `docs/prompts/lifecycle/08-task-creation.md` — task format and dependency wiring
4. `docs/prompts/lifecycle/14-bead-audit.md` — the lab-grade bead standard (the bar)

## Session Summary

### Completed (Session 003)
- Three independent progress reviews conducted (Claude Opus 4.6, Codex GPT-5 x2)
- Cross-validated synthesis produced revised assessment: **C+ (6.1)**
- Phase gate beads created and wired: bd-add (4a), bd-7ij (4b), bd-fzn (4c), bd-lvq (4d)
- All phase-N+1 beads now depend on their gate bead — `br ready` shows only 4a work
- Revised review: `docs/reviews/2026-03-30-progress-review-phase4a.md`

### Key Findings from All Three Reviews
- Two correctness bugs found (recovery semantics, schema-conflict handling)
- Workspace is red (`cargo test --workspace` fails, `cargo clippy -- -D warnings` fails)
- 108 `unwrap()` in production code (63 in wal.rs alone)
- Durability tests assert wrong properties (existence, not exactness)
- Architecture docs overstate present implementation
- Phase gate verdict: **FAIL** (all three reviews agree)

### Current Bead State
- Open: 33 beads (but only 3 are ready — all Phase 4a)
- Ready: bd-n1i, bd-dsa, bd-wgl
- 4a gate (bd-add) depends on: bd-2qv, bd-3cn, bd-n1i, bd-2jx, bd-wgl, bd-dsa, bd-veg
- Phase gates operational: no 4b/4c/4d/5 work can become ready until 4a closes

## Primary Task: Define Exhaustive Phase 4a Closure Beads

### Objective

Create the complete set of lab-grade beads (per 14-bead-audit.md standard) that,
when ALL are closed, bring every scorecard vector to 10.0 / A+. Wire all new
beads as dependencies of bd-add (the 4a gate). When bd-add closes, Phase 4a is
complete to cleanroom standard.

**Do NOT write code.** This session produces beads only. Code happens in Session 005+.

### Method

1. Read the revised review at `docs/reviews/2026-03-30-progress-review-phase4a.md`
2. For each of the 10 scorecard vectors, enumerate every gap between current state
   and 10.0/A+
3. For each gap, create a lab-grade bead using the template from 14-bead-audit.md
4. Wire all new beads as deps of bd-add (or as deps of existing 4a beads)
5. Verify the final graph with `bv --robot-triage` and `bv --robot-insights`

### Gap Decomposition (Vector by Vector)

Use the review at `docs/reviews/2026-03-30-progress-review-phase4a.md` as the
primary source. The gap register, coverage matrix, and phase gate assessment
contain the specific findings. Below is the structured decomposition.

---

#### Vector 1: Correctness (7.5 -> 10.0, Weight 3x)

These are the highest-priority beads because correctness has the heaviest weight.

| Gap | INV-FERR | Current State | Required State | Priority |
|-----|----------|---------------|----------------|----------|
| Recovery replay restores datoms but not schema+epoch | 014, 007, 009 | `Store::insert` on recovered datoms skips schema evolution and epoch advancement | Dedicated replay helper restores exact `datoms + schema + epoch`; recovered store supports future transactions | P0 |
| Merge silently accepts schema conflicts | 043 | `debug_assert!(false)` branch in `Store::from_merge`; no `schema_compatible` function | Explicit `schema_compatible` gate returns `SchemaIncompatible` on conflict | P0 |
| Index bijection is debug_assert only | 005 | `verify_bijection()` is `debug_assert`; no proptest for simultaneous 4-index bijection | Runtime-enforced bijection check; proptest strategy for 4-index simultaneous verification | P1 |
| Convergence (010) has no implementation reference | 010 | Tests exist (Kani+proptest+Stateright) but no impl artifact cites INV-FERR-010 | Add explicit convergence documentation/markers in merge.rs or store.rs | P2 |

**Check existing beads**: bd-2jx ("commutativity test checks datom_set only, not full store") partially overlaps the recovery gap. Verify scope and split or absorb.

---

#### Vector 2: Completeness (4.2 -> 10.0, Weight 2x)

The coverage matrix (review Section "Coverage Matrix") shows which INV-FERR lack
impl+test. For each partial or unimplemented invariant in the 4a scope (001-032):

| Gap | INV-FERR | Status | Action |
|-----|----------|--------|--------|
| Shard equivalence — no implementation | 017 | Test-only (Kani) | Add shard partitioning stub/trait in ferratomic-core; wire to Kani harness |
| Error exhaustiveness — no test | 019 | Code-only (error.rs) | Add exhaustiveness test: every `FerraError` variant is constructible and pattern-matchable |
| Transaction atomicity — no impl reference | 020 | Test-only (Kani) | Add atomicity markers in db.rs transaction path |
| Backpressure safety — no test | 021 | Code-only (backpressure.rs) | Add proptest: backpressure semaphore correctly limits concurrent writes |
| Anti-entropy convergence | 022 | Unimplemented | Explicitly defer with rationale bead (phase-4c) OR implement stub |
| No unsafe code audit | 023 | Unimplemented (but `#![forbid(unsafe_code)]` present) | Add CI check bead; the guard exists, the trace is missing |
| Substrate agnosticism | 024 | Unimplemented | Explicitly defer (phase-4c) OR implement `StorageBackend` trait |
| Index backend interchangeability | 025 | Unimplemented | Explicitly defer (phase-4b) OR add `IndexBackend` trait skeleton |
| Write amplification — benchmark only | 026 | Type-level-only | Execute write_amplification benchmark; add proptest for threshold |
| Read latency — benchmark only | 027 | Type-level-only | Execute read_latency benchmark; add proptest for O(log N) verification |
| Cold start time — benchmark only | 028 | Type-level-only | Execute cold_start benchmark; verify against INV-FERR-028 target |
| LIVE view resolution — no test | 029 | Code-only (writer.rs) | Add proptest for LIVE resolution correctness |
| Read replica subset | 030 | Unimplemented | Explicitly defer (phase-4c) |
| LIVE resolution correctness — no test | 032 | Code-only (schema.rs) | Add proptest for LWW vs keep-all semantics |

**Decision point**: For unimplemented invariants that are genuinely phase-4c/4b
scoped (022, 024, 025, 030), create explicit deferral beads that document WHY
they're deferred and add them to the correct future-phase gate. Don't create
implementation beads for future-phase work in the 4a closure set.

---

#### Vector 3: Verification Depth (6.2 -> 10.0, Weight 2x)

The coverage matrix shows 15 zero-layer and 20 single-layer invariants. For A+:
every Phase 4a INV-FERR needs 4+ layers.

| Gap | Action |
|-----|--------|
| ferratomic-verify doesn't compile (missing_docs) | Fix the doc comment; this unblocks the entire verification pipeline |
| Benchmark suites scaffolded but never executed | Run all 6 Criterion suites; record baseline numbers; add to CI |
| INV-FERR-008 has no Kani harness | Add Kani bounded verification for WAL two-fsync ordering |
| INV-FERR-014 has no Stateright model | Add crash-recovery state machine to Stateright model |
| INV-FERR-005 has no integration test | Add integration test for 4-index bijection after transact |
| Many 4a invariants at 1-2 layers | Systematically add proptest or integration tests to raise each to 4+ |

**Primary source**: The full coverage matrix in the review. For each INV-FERR row
with < 4 populated columns, create a bead to fill the gap.

---

#### Vector 4: Code Quality (5.8 -> 10.0, Weight 1.5x)

| Gap | Current | Target | Priority |
|-----|---------|--------|----------|
| 8 clippy errors | `cargo clippy -- -D warnings` fails | 0 errors | P0 |
| ferratomic-verify missing_docs | `cargo test --workspace` fails | Compiles clean | P0 |
| 108 unwrap() in production code | NEG-FERR-001 violation | 0 unwrap (or explicit #[allow] with safety comment) | P1 |
| No `#[deny(clippy::unwrap_used)]` enforcement | Convention only | Compiler-enforced in ferratomic-core lib.rs | P1 |
| schema_evolution.rs function exceeds 50 LOC | 66 lines | Split into sub-functions under 50 LOC | P1 |

**Note**: Some of these overlap with Correctness (unwrap in wal.rs) and
Durability (unwrap in checkpoint.rs). Create beads that explicitly state the
quality dimension AND the correctness dimension they serve.

---

#### Vector 5: Architecture (6.8 -> 10.0, Weight 1.5x)

| Gap | Current | Target |
|-----|---------|--------|
| README/lib.rs describe future-phase architecture as current | Writer actors, group commit, prolly storage described | Phase-tagged; only 4a implementation described as current |
| Recovery replay bypasses core state invariants | `Store::insert` used for replay | Replay goes through full state machine (same as transact, or replay-specific path) |
| No module-level architecture doc for ferratomic-core | Ad hoc module structure | Each module's responsibility documented in lib.rs, traces to INV-FERR |

---

#### Vector 6: Performance (5.3 -> 10.0, Weight 1.5x)

| Gap | INV-FERR | Current | Target |
|-----|----------|---------|--------|
| Benchmarks not executed | 025-028 | Criterion scaffolding exists | Run all benchmarks, record baselines, compare to spec targets |
| Mutex-based write path | 007 | Single Mutex serialization | Document current throughput; this is a known 4a limitation |
| No O(log N) verification for index lookups | 027 | Claimed in spec | Benchmark proves O(log N) read latency at 1K/10K/100K scale |
| Write amplification not measured | 026 | Threshold in spec | Benchmark proves WAL overhead < 10x |

---

#### Vector 7: Durability (4.8 -> 10.0, Weight 2x)

| Gap | INV-FERR | Current | Target |
|-----|----------|---------|--------|
| Recovery doesn't restore schema+epoch | 014 | `Store::insert` on replay | Full state restoration (see Correctness) |
| Durability tests assert wrong properties | 014 | Datom set equality only | Exact state equality (datoms + schema + epoch) |
| Checkpoint+WAL allows off-by-one loss | 013 | Proptest allows it | Proptest requires exact roundtrip |
| Recovery test asserts non-emptiness only | 014 | test_recovery.rs | Assert exact state equality after recovery |
| 63 unwrap() in wal.rs | 008 | Panics on corrupt WAL | Returns FerraError::Io or FerraError::WalCorruption |
| 24 unwrap() in checkpoint.rs | 013 | Panics on corrupt checkpoint | Returns FerraError::CheckpointCorruption |
| 17 unwrap() in storage.rs | 014 | Panics on missing files | Returns FerraError::StorageCorruption |
| No crash-then-transact roundtrip test | 014 | Not tested | transact -> crash -> recover -> transact again succeeds |

---

#### Vector 8: Ergonomics (6.8 -> 10.0, Weight 0.5x)

| Gap | Current | Target |
|-----|---------|--------|
| No typestate for Transaction lifecycle | Plain struct | `Transaction<Building>` -> `Transaction<Committed>` |
| No typestate for Database lifecycle | Plain struct | `Database<Opening>` -> `Database<Ready>` |
| Merge API misleads about schema handling | Silently drops conflicts | Returns `Result` with `SchemaIncompatible` |
| Recovery API misleads about state completeness | Appears to restore full state | Either restores full state (fix) or documents limitation |

---

#### Vector 9: Axiological Alignment (7.8 -> 10.0, Weight 2x)

| Gap | Current | Target |
|-----|---------|--------|
| Docs present future architecture as current | README.md, lib.rs | Phase-tagged: "Phase 4a (current)" vs "Phase 4b (planned)" |
| Some impl code has no INV-FERR trace | Modules without citations | Every public function cites its governing INV-FERR |
| 3 test-only invariants lack impl references | 010, 017, 020 | Add explicit markers in implementing code |

---

#### Vector 10: Process Health (5.5 -> 10.0, Weight 1x)

| Gap | Current | Target |
|-----|---------|--------|
| Workspace is red | check passes, clippy/test fail | All three green |
| Phase gate was cultural | Gate beads now exist (DONE) | Maintain wiring for new beads |
| Worktree has 289 entries | .beads/.br_history churn | Clean up or .gitignore the history |
| Verify pipeline not running | ferratomic-verify doesn't compile | Full `cargo test --workspace` green |
| No CI enforcement | Quality gates are manual | Document CI expectations (even if CI isn't set up yet) |

---

### Bead Creation Protocol

For each gap above, create a bead following the full lab-grade template from
`docs/prompts/lifecycle/14-bead-audit.md`. Every bead must have:

- **Specification Reference**: Exact INV-FERR, Level, spec file
- **Preconditions**: Verifiable predicates (cite dep beads)
- **Postconditions**: Binary, verifiable, INV-traced
- **Frame Conditions**: What this bead must NOT touch
- **Refinement Sketch**: Abstract -> Concrete -> Coupling
- **Verification Plan**: Specific test names and commands
- **Files**: Exact paths
- **Dependencies**: Wired to bd-add or to other 4a beads

### Dependency Wiring Rules

1. All new beads that are Phase 4a work: `br dep add bd-add <new-bead>`
2. Ordering within 4a work: green-workspace beads first, then correctness fixes,
   then durability hardening, then quality sweep, then verification depth
3. New beads must not create cycles — verify with `bv --robot-insights` after each batch
4. Beads for future-phase deferrals (022, 024, 025, 030) get labeled `phase-4b` or
   `phase-4c` and wired behind bd-7ij or bd-fzn, NOT bd-add

### Deduplication Check

Before creating each bead, check existing open beads for overlap:

| Existing Bead | Topic | Check Against |
|---------------|-------|---------------|
| bd-n1i | Missing error-path tests | Durability test gaps |
| bd-2jx | Commutativity test checks datom_set only | Recovery/merge state completeness |
| bd-wgl | Value::Double NaN guard | Code quality |
| bd-dsa | No self-merge fast path | Merge correctness |
| bd-veg | Outdated red-phase header comments | Process health |
| bd-2qv | Phase 4a spec hardening epic | Completeness gaps |
| bd-3cn | Phase 4a impl completion epic | All implementation gaps |

If a new gap overlaps an existing bead, either:
- Strengthen the existing bead to lab-grade (add missing fields)
- Split the existing bead if scopes diverge
- Note the overlap in the new bead's dependencies

### Estimated Output

Expect approximately 25-35 new beads covering:
- ~5 correctness fixes (P0-P1)
- ~8 completeness gaps (P1-P2)
- ~5 verification depth additions (P2)
- ~5 code quality fixes (P0-P1)
- ~3 architecture alignment (P1-P2)
- ~3 durability hardening (P0-P1)
- ~3 ergonomics/process (P2-P3)
- ~3 explicit future-phase deferrals (routed to 4b/4c gates)

### Verification After All Beads Created

```bash
# Graph integrity
bv --robot-insights    # 0 cycles, healthy metrics
bv --robot-alerts      # 0 alerts
bv --robot-suggest     # Check for duplicates

# Ready queue sanity
br ready --limit 0     # Should show only 4a leaf tasks
bv --robot-triage      # Ranked recommendations should all be 4a work
bv --robot-plan        # Parallel execution tracks for swarm assignment

# Phase discipline
# Verify: no phase-4b/4c/4d/5 beads appear in ready queue
```

### Final Flush

```bash
br sync --flush-only   # Export to JSONL (no git operations)
# DO NOT commit yet — human will review the bead graph first
```

## Hard Constraints

- `#![forbid(unsafe_code)]` in all crates
- No `unwrap()` in production code (NEG-FERR-001)
- `CARGO_TARGET_DIR=/data/cargo-target` for all cargo commands
- Phase N+1 cannot start until Phase N gate bead closes
- Phase gate beads: bd-add (4a), bd-7ij (4b), bd-fzn (4c), bd-lvq (4d)
- New phase-4b beads must depend on bd-add
- This session creates BEADS ONLY — no code changes

## Stop Conditions

Stop and escalate to the user if:
- A gap requires a design decision not covered by existing spec or ADR
- Two existing beads contradict each other about the correct fix
- The dependency graph would require > 40 new beads (scope may need narrowing)
- An existing bead's scope overlaps but its approach conflicts with the review's recommendation
- You discover a correctness issue not identified in any of the three reviews
