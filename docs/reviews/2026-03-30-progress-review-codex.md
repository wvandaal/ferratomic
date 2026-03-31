# Ferratomic Progress Review — 2026-03-30

> **Reviewer**: Codex (GPT-5)
> **Scope**: Deep review, all phases visible, current frontier `4b`, metrics since `2026-03-29`
> **Prompt source**: `docs/prompts/lifecycle/13-progress-review.md`

---

## Executive Summary

Ferratomic is strongest where the system reduces cleanly to algebra: the semilattice core is well specified, Lean has `0` `sorry`, `INV-FERR-001/002/003/012` have explicit 7-layer trace coverage, and the issue graph is clean and acyclic. It is weakest at the operational edge: the workspace is not green, recovery does not currently restore full store semantics, and much of the 4b/4c frontier exists only in spec/Lean space.

Composite assessment: **C+ / 6.4**.

Single highest-leverage next action: **fix recovery so replay restores exact `datoms + schema + epoch`, then make the durability suite assert that exactness under a green workspace**.

---

## Top Gaps

### GAP-001: Recovery restores datoms but not store semantics

**Type**: Critical  
**Traces to**: `INV-FERR-014`, `INV-FERR-007`, `INV-FERR-009`  
**Severity**: Blocking  
**Leverage**: High  
**Phase**: 4a  
**Remediation effort**: M

**Evidence**:
- `Database::recover_from_wal` and `Database::recover` replay WAL payloads by calling `Store::insert` on recovered datoms.
- `Store::insert` updates the primary set and indexes only; it does not evolve schema or advance epoch.
- The spec explicitly requires recovered state to be fully functional, with correct epoch and working future transactions.

Files:
- `ferratomic-core/src/db.rs`
- `ferratomic-core/src/store.rs`
- `spec/02-concurrency.md`

### GAP-002: Merge contradicts the schema-compatibility contract

**Type**: Major  
**Traces to**: `INV-FERR-043`  
**Severity**: Degrading  
**Leverage**: High  
**Phase**: 4c (but already relevant in shared merge logic)  
**Remediation effort**: M

**Evidence**:
- The spec requires merge to be undefined on conflicting schemas and to return `SchemaIncompatible`.
- No Rust `schema_compatible` implementation exists.
- `Store::from_merge` silently keeps one conflicting definition after a `debug_assert!(false)` branch.

Files:
- `ferratomic-core/src/store.rs`
- `ferratomic-core/src/merge.rs`
- `spec/05-federation.md`

### GAP-003: The workspace is not phase-gateable

**Type**: Major  
**Traces to**: Cleanroom standards, `INV-FERR-023`, phase-gate “Types ↔ Impl” boundary  
**Severity**: Blocking  
**Leverage**: High  
**Phase**: 4a  
**Remediation effort**: S

**Evidence**:
- `cargo check --workspace` fails on missing docs in `ferratomic-verify/stateright/mod.rs`.
- `cargo test --workspace` fails on the same missing-docs error.
- `cargo clippy --workspace -- -D warnings` fails with 8 errors, including a 66-line production function in `schema_evolution.rs`.

Files:
- `ferratomic-verify/stateright/mod.rs`
- `ferratomic-verify/src/lib.rs`
- `ferratomic-core/src/schema_evolution.rs`
- `ferratomic-core/src/store.rs`
- `ferratomic-core/src/db.rs`
- `ferratomic-core/src/backpressure.rs`

### GAP-004: Durability tests miss the properties that are currently broken

**Type**: Major  
**Traces to**: `INV-FERR-014`  
**Severity**: Degrading  
**Leverage**: Medium  
**Phase**: 4a  
**Remediation effort**: S

**Evidence**:
- The WAL recovery property compares recovered datom sets but never checks recovered schema or epoch.
- The checkpoint+WAL property explicitly allows off-by-one datom loss.
- The core recovery regression test only asserts that the recovered snapshot is non-empty.

Files:
- `ferratomic-verify/proptest/durability_properties.rs`
- `ferratomic-core/src/db.rs`

### GAP-005: 4b/4c remain spec-forward and code-thin

**Type**: Frontier  
**Traces to**: `INV-FERR-038/041/042/047/048/050`  
**Severity**: Expected now, future-blocking later  
**Leverage**: Medium  
**Phase**: 4b/4c  
**Remediation effort**: L

**Evidence**:
- 4b implementation trace count is `0/6`.
- 4c implementation trace count is `1/8`, and that single hit is the shared merge path rather than a federation implementation.
- Several frontier invariants appear only in Lean or only in beads/spec.

---

## Scorecard

| Vector | Grade | Score | Weight | Evidence |
|---|---:|---:|---:|---|
| Correctness | B+ | 8.2 | 3.0 | Core CRDT laws are deeply covered; replay correctness and schema-conflict handling prevent an `A`. |
| Completeness | C+ | 6.2 | 2.0 | 4a has broad code presence, but the frontier is still mostly spec/proof surface. |
| Verification Depth | C | 5.5 | 2.0 | A few invariants are saturated; the long tail is thin. |
| Code Quality | C | 5.4 | 1.5 | `unsafe` discipline and crate budgets are good, but `check`/`clippy`/`test` are red. |
| Architecture | B | 7.6 | 1.5 | Crate DAG is clean and production file budgets pass; replay currently bypasses core state invariants. |
| Performance | C | 5.0 | 1.5 | Bench scaffolding exists, but `INV-FERR-025..030` lacks executed proof in code or measurements. |
| Durability | D | 4.2 | 2.0 | WAL/checkpoint machinery exists, but replay currently fails the stronger semantic contract. |
| Ergonomics | B- | 6.8 | 0.5 | Strong domain types and typed errors; replay and merge APIs still mislead about their guarantees. |
| Axiological Alignment | A- | 8.7 | 2.0 | The repo is highly spec-traced and True-North aligned, with little feature creep. |
| Process Health | C | 5.0 | 1.0 | The bead graph is healthy, but phase bleed and red builds keep the gate closed. |
| **Composite** | **C+** | **6.4** | **17.0** | Weighted GPA across the 10 vectors. |

---

## Metrics

### Issue Graph

| Metric | Value |
|---|---:|
| Total beads | 144 |
| Closed | 117 |
| Open | 26 |
| In progress | 1 |
| Ready | 21 |
| Blocked | 6 |
| Alerts | 0 |
| Cycles | 0 |

Key observations:
- `bv --robot-next` recommends `bd-3gk`.
- `bv --robot-plan` shows multiple parallel tracks, but that should not override the explicit 4a gate.

### Git Velocity

| Metric | Value |
|---|---:|
| Commits since `2026-03-29` | 45 |
| Unique non-`.beads` files touched | 96 |
| Non-`.beads` delta | `29,962` insertions, `141` deletions |

### Build Health

`cargo check --workspace`:

```text
error: missing documentation for a module
 --> ferratomic-verify/src/../stateright/mod.rs:7:1
```

`cargo clippy --workspace -- -D warnings`:

```text
error: this function has too many lines (66/50)
 --> ferratomic-core/src/schema_evolution.rs:85:1
error: could not compile `ferratomic-core` (lib) due to 8 previous errors
```

`cargo test --workspace -- --list`:

```text
error: missing documentation for a module
 --> ferratomic-verify/src/../stateright/mod.rs:7:1
```

### Codebase Size

| Crate | LOC |
|---|---:|
| `ferratom` | 1,559 |
| `ferratomic-core` | 4,425 |
| `ferratomic-datalog` | 25 |
| `ferratomic-verify/src` | 210 |

Source-budget result:
- No production-only Rust file exceeds 500 LOC before its `#[cfg(test)]` section.

### Proof Health

| Metric | Value |
|---|---:|
| Lean `sorry` count | 0 |
| Lean build | success |

### Coverage Highlights

Explicit layer counts by artifact family:

| Layer | Coverage |
|---|---:|
| Lean | 30 / 55 |
| proptest | 17 / 55 |
| Kani | 18 / 55 |
| Stateright | 5 / 55 |
| Integration | 12 / 55 |
| Type-level | 16 / 55 |
| Impl trace | 25 / 55 |

Strongest invariants:
- `001`, `002`, `003`, `012` with 7 explicit layers each.

Zero explicit-layer invariants:
- `022`, `023`, `024`, `025`, `030`, `034`, `036`, `038`, `041`, `042`, `047`, `048`, `050`

Important nuance:
- `INV-FERR-023` is a mechanical false-zero in this matrix because all four crates do enforce `#![forbid(unsafe_code)]`; the trace is missing, not the guard.

---

## Phase Gate Assessment

| Boundary | Verdict | Evidence |
|---|---|---|
| Spec ↔ Lean | PARTIAL | Lean covers major parts of the algebraic core and some frontier modules, but large regions remain absent or axiomatized only. |
| Lean ↔ Tests | PARTIAL | Core CRDT laws map well across Lean and Rust verification layers; much of the frontier does not. |
| Tests ↔ Types | PARTIAL | Newtypes and typestate are strong, but replay/schema semantics are not enforced by the type boundary. |
| Types ↔ Impl | FAIL | The workspace is red, and replay/merge semantics currently contradict named invariants. |

**Overall verdict**: `FAIL`

---

## Decision Matrix

| Decision | Option A | Option B | Correctness | Complexity | Spec Alignment | Recommendation |
|---|---|---|---|---|---|---|
| Recovery replay design | Raw `insert` of recovered datoms | Dedicated replay helper that restores schema + epoch | B wins | B slightly higher | B wins | **Option B** |
| Schema-conflict handling | Silent deterministic overwrite | Explicit compatibility gate returning `SchemaIncompatible` | B wins | B slightly higher | B wins | **Option B** |
| What to do next | Resume `bd-3gk` immediately | Close the 4a gate first | B wins | A lower short-term friction | B wins | **Option B** |

---

## Tactical Plan

1. Fix replay semantics in `ferratomic-core/src/db.rs` and `ferratomic-core/src/store.rs` so recovery restores exact `datoms + schema + epoch`.
2. Strengthen durability tests in `ferratomic-verify/proptest/durability_properties.rs`, `ferratomic-verify/integration/test_recovery.rs`, and `ferratomic-core/src/db.rs` so they assert exactness instead of existence.
3. Restore a green workspace by fixing missing docs and the current 8 clippy violations.
4. Implement `schema_compatible` and explicit `SchemaIncompatible` handling before merge.
5. After the 4a gate is truly green, resume `bd-3gk` and the 4b block-store work it unlocks.

---

## Strategic Plan

Phase 4a gate checklist:
- `cargo check --workspace` passes.
- `cargo clippy --workspace -- -D warnings` passes.
- `cargo test --workspace` passes.
- Recovery preserves exact `datoms + schema + epoch`.
- Merge rejects schema conflicts explicitly.
- Public docs stop presenting future-phase architecture as current implementation.

Critical path:
1. Green workspace
2. Replay fix
3. Durability test hardening
4. 4a gate close
5. `bd-3gk`

Top risks and contingencies:
- **Replay bug risk**: persistence claims are overstated. Contingency: block 4b and land replay/test fixes first.
- **Schema-conflict risk**: future federation work could inherit silent divergence. Contingency: add compatibility gate before transport/federation code expands.
- **Spec/proof lead risk**: prompt/spec surface outruns executable Rust. Contingency: require every new frontier invariant to land with at least one executable verification layer.

Recommended swarm shape for the next execution phase:
- Agent 1: green-workspace and lint/doc blockers.
- Agent 2: replay semantics and durability tests.
- Agent 3: schema-compatibility design and merge error surface.

---

## Retrospective

What is going well:
- The algebraic center of the project is real. This is not decorative formalism.
- The crate architecture is disciplined and still small enough to reason about.
- The bead graph is operationally useful, not just ceremonial.

What is going poorly:
- The project is psychologically ahead of itself on 4b while 4a is still red.
- Recovery semantics are weaker than the surrounding proof/test posture suggests.
- Public architecture language still blurs current implementation and intended future shape.

What surprised me:
- Lean is in better shape than the Rust workspace. That is unusual and important.

What I would change:
- I would turn the phase gate into an enforced control system rather than a cultural norm.

Confidence:
- True North: `6/10`
- Correctness: `7/10`
- Completion: `6/10`
- Architecture: `7/10`

What would raise each by one point:
- Correctness: exact replay fix plus green durability suite.
- Completion: explicit 4a closure before more 4b expansion.
- Architecture: concrete 4b/4c implementation slices replacing spec-only placeholders.
