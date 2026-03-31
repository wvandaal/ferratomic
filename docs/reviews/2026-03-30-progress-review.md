# Ferratomic Progress Review — 2026-03-30

> **Reviewer**: Codex (GPT-5)
> **Scope**: Phase 4a gate readiness, deep mode, `SINCE=2026-03-29` (repo inception; no tags or prior review artifact found)
> **Current frontier**: The bead graph points at Phase 4b (`bd-3gk`), but project methodology still requires a 4a gate pass before 4b starts

---

## Executive Summary

Ferratomic has a real Phase 4a foundation: the core CRDT laws are proven in Lean, `lake build` succeeds with zero `sorry`, the issue graph is acyclic, the crate DAG and LOC budgets are healthy, and durability/recovery work is materially implemented. The project is not ready to declare Phase 4a complete, though, because the Rust workspace is currently not green: `cargo check`, `cargo test`, and `cargo clippy -- -D warnings` all fail, and the current-phase invariant coverage is still incomplete.

Composite score: **C+ / 6.4**. The strongest areas are correctness, durability, and axiological alignment. The weakest areas are completeness, verification breadth across all current-phase invariants, and process health around the 4a→4b gate.

Single most important next action: **restore a green 4a workspace, then close the explicit 4a spec/verification gaps before allowing any Phase 4b execution to proceed**.

---

## Scorecard

| Vector | Grade | Score | Weight | Evidence |
|---|---:|---:|---:|---|
| Correctness | B+ | 8.3 | 3.0 | Core CRDT invariants `001/002/003/010` are covered in Lean (`Store.lean`), proptest (`crdt_properties.rs`), Kani (`crdt_laws.rs`), Stateright (`crdt_model.rs`), and integration (`test_crdt.rs`). `lake build` succeeds and no Lean `sorry` was found. |
| Completeness | D | 3.6 | 2.0 | For the 4a gate, only **17/32** current-phase invariants have both implementation/type anchors and tests, **10/32** are partial, and **5/32** are absent by explicit trace. The unresolved 4a set still includes `010, 017, 019-030, 032` at varying depths. |
| Verification Depth | C | 5.7 | 2.0 | Deep-mode layer counts are uneven: across all 55 invariants, the distribution is `{0:15, 1:20, 2:3, 3:5, 4:7, 5:1, 6:4}`. The implemented 4a core is deep, but many 4a performance/concurrency invariants still sit at 0-1 layers. |
| Code Quality | C | 5.8 | 1.5 | `#![forbid(unsafe_code)]` is present in all four crates, production `unwrap()/expect()` sites were not found outside test blocks, and production-only file sizes stay under the 500 LOC limit. But `cargo check` fails on `ferratomic-verify/stateright/mod.rs`, and `cargo clippy -- -D warnings` reports 8 cleanroom violations including `schema_evolution.rs:85`. |
| Architecture | B- | 6.8 | 1.5 | The crate DAG is clean (`ferratom → core → datalog`), production-only file budgets hold, and there are no dependency cycles in beads. The main drag is architecture/documentation drift: `README.md` and `ferratomic-core/src/lib.rs` still describe writer-actor/prolly capabilities that are not the current 4a implementation shape. |
| Performance | C | 5.3 | 1.5 | Benchmark scaffolding exists (`ferratomic-verify/benches/*.rs`), and some performance invariants (`029/031/032`) have Lean coverage, but the review found no executed benchmark evidence for the published targets. The current Mutex-based write path also means the advertised group-commit throughput story is not yet substantiated. |
| Durability | B | 7.6 | 2.0 | `INV-FERR-013/014` have meaningful code in `checkpoint.rs`, `wal.rs`, `storage.rs`, and `db.rs`, plus proptest (`durability_properties.rs`), Kani (`durability.rs`), and integration (`test_recovery.rs`). This is one of the best-covered areas in the codebase, even though the workspace is currently non-green. |
| Ergonomics | B | 7.0 | 0.5 | The type discipline is strong: newtypes in `ferratom`, typed `FerraError`, and transaction typestate in `writer.rs`. Rough edges remain around TODO public modules and the inability to enumerate tests because the workspace does not currently compile cleanly. |
| Axiological Alignment | B+ | 7.8 | 2.0 | The repo is strongly spec-traced: most implemented modules cite `INV-FERR-*`, the True North algebra is explicit in `README.md` and `spec/README.md`, and there is little evidence of speculative product creep. The main misalignment is representational: public docs overstate future-phase architecture as if it were already present. |
| Process Health | C- | 4.6 | 1.0 | Beads discipline is materially useful: `117` closed, `26` open, `22` ready, `6` blocked, no cycles, and no active alerts. But the 4a gate is still open, `bd-3gk` is queued despite session docs explicitly blocking 4b behind `bd-2qv` and `bd-3cn`, and the worktree is noisy (`289` entries, `207` untracked, mostly bead-history churn). |
| **Composite** | **C+** | **6.4** | **17.0** | Weighted GPA across the 10 vectors. |

---

## Metrics

### Issue Graph State

| Metric | Value |
|---|---:|
| Total beads | 144 |
| Closed | 117 |
| Open | 26 |
| In progress | 1 |
| Ready | 22 |
| Blocked | 6 |
| Alerts | 0 |
| Graph cycles | 0 |
| Highest-impact next bead | `bd-3gk` |

Key graph facts:
- `bv --robot-next` recommends `bd-3gk` because it unblocks `bd-85j.13` and `bd-aii`.
- `bv --robot-plan` shows `21` actionable items across parallel tracks.
- `bv --robot-insights` identifies `bd-3gk` and `bd-85j.13` as structural cut points, but the session docs still block 4b on 4a completion.

### Git Velocity

| Metric | Value |
|---|---:|
| Commits since `2026-03-29` | 46 |
| Unique files touched | 163 |
| Net repo delta from empty tree | 34,802 insertions across 167 files |
| Worktree entries | 289 |
| Untracked entries | 207 |

Recent headline commits:
- `188ebdb` `fix: Phase 4a hardening — 20 cleanroom bugs + quality gates`
- `624061e` `fix: non-vacuous SEC property + wire WriteLimiter`
- `d7f5369` `fix: INV-FERR-010 Stateright model — SEC safety + liveness`
- `19be289` `feat: backpressure module + Semilattice trait on Store`

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
error: item in documentation is missing backticks
 --> ferratomic-core/src/store.rs:47:68
error: item in documentation is missing backticks
 --> ferratomic-core/src/store.rs:65:9
error: this argument is passed by value, but not consumed in the function body
 --> ferratomic-core/src/store.rs:300:37
error: used underscore-prefixed binding
 --> ferratomic-core/src/db.rs:329:14
error: used underscore-prefixed binding
 --> ferratomic-core/src/db.rs:330:14
error: item in documentation is missing backticks
 --> ferratomic-core/src/backpressure.rs:32:63
error: the following explicit lifetimes could be elided: 'a
 --> ferratomic-core/src/backpressure.rs:58:6
```

`cargo test --workspace`:

```text
error: missing documentation for a module
 --> ferratomic-verify/src/../stateright/mod.rs:7:1
warning: build failed, waiting for other jobs to finish...
```

`cargo test --workspace -- --list`:

```text
error: missing documentation for a module
 --> ferratomic-verify/src/../stateright/mod.rs:7:1
```

Lean:

```text
lake build
Build completed successfully (760 jobs).
```

### Codebase Size

| Crate | LOC |
|---|---:|
| `ferratom` | 1,559 |
| `ferratomic-core` | 4,425 |
| `ferratomic-datalog` | 25 |
| `ferratomic-verify/src` | 210 |

Complexity budget checks:
- Largest production-only Rust file: `ferratomic-core/src/store.rs` at `472` lines.
- Largest total Rust file including inline tests: `ferratomic-core/src/store.rs` at `866` lines.
- Production files over 500 LOC: `0`
- Total files over 1,500 LOC: `0`

### Proof and Test Health

| Metric | Value |
|---|---:|
| Lean `sorry` count | 0 |
| Lean build | success |
| Invariants with Lean references | 30 / 55 |
| Invariants with proptest references | 17 / 55 |
| Invariants with Kani references | 18 / 55 |
| Invariants with Stateright references | 5 / 55 |
| Invariants with integration references | 12 / 55 |
| Invariants with type-level references | 16 / 55 |

Source-level test inventory:
- `#[test]` attributes found: `166`
- `proptest!` blocks found: `8`
- `#[kani::proof]` harnesses found: `20`
- Integration test files: `4`
- Stateright model files: `2`

### Spec-Implementation Drift

Current-phase (4a) trace classification:

| Status | Count |
|---|---:|
| Implemented (impl/type + tests) | 17 |
| Partial | 10 |
| Unimplemented | 5 |
| Contradicted | 0 explicit invariant contradictions found |
| **Drift score** | **15** |

Full-project trace classification:

| Status | Count |
|---|---:|
| Implemented (impl/type + tests) | 17 |
| Partial | 11 |
| Unimplemented | 27 |
| Contradicted | 0 explicit invariant contradictions found |
| **Drift score** | **38** |

Current-phase 4a gaps by explicit trace:
- Partial: `010`, `017`, `019`, `020`, `021`, `026`, `027`, `028`, `029`, `032`
- Unimplemented: `022`, `023`, `024`, `025`, `030`

Phase-level trace totals:

| Phase | Implemented | Partial | Unimplemented |
|---|---:|---:|---:|
| 4a (`001-032`) | 17 | 10 | 5 |
| 4b (`045-050`) | 0 | 0 | 6 |
| 4c (`037-044`, `051-055`) | 0 | 1 | 12 |
| 4d (`033-036`) | 0 | 0 | 4 |

---

## Coverage Matrix

| INV-FERR | Lean | proptest | Kani | Stateright | Integration | Type-level |
|---|---|---|---|---|---|---|
| 001 | Store.lean | crdt_properties.rs | crdt_laws.rs | crdt_model.rs | test_crdt.rs | traits.rs |
| 002 | Store.lean | crdt_properties.rs | crdt_laws.rs | crdt_model.rs | test_crdt.rs | traits.rs |
| 003 | Store.lean | crdt_properties.rs | crdt_laws.rs | crdt_model.rs | test_crdt.rs | traits.rs |
| 004 | Refinement.lean, Store.lean | append_only_properties.rs, crdt_properties.rs | crdt_laws.rs | — | test_crdt.rs | — |
| 005 | — | durability_properties.rs, index_properties.rs | store_views.rs | — | — | error.rs |
| 006 | — | index_properties.rs | store_views.rs | — | test_snapshot.rs | — |
| 007 | Refinement.lean | index_properties.rs | store_views.rs | — | test_snapshot.rs | — |
| 008 | — | wal_properties.rs | — | — | test_recovery.rs | error.rs |
| 009 | — | crdt_properties.rs, schema_properties.rs | schema_identity.rs | — | test_crdt.rs, test_schema.rs | error.rs, lib.rs, schema.rs |
| 010 | Store.lean | crdt_properties.rs | crdt_laws.rs | crdt_model.rs, mod.rs | test_crdt.rs | — |
| 011 | — | schema_properties.rs | store_views.rs | — | test_snapshot.rs | — |
| 012 | Store.lean | crdt_properties.rs | schema_identity.rs | crdt_model.rs | test_crdt.rs | datom.rs, lib.rs, traits.rs |
| 013 | Concurrency.lean | durability_properties.rs | durability.rs | — | — | error.rs |
| 014 | — | durability_properties.rs | durability.rs | — | — | — |
| 015 | Concurrency.lean | clock_properties.rs | clock.rs | — | — | clock.rs, lib.rs |
| 016 | Concurrency.lean | clock_properties.rs | clock.rs | — | — | clock.rs, lib.rs |
| 017 | Concurrency.lean | — | sharding.rs | — | — | — |
| 018 | Store.lean | append_only_properties.rs | durability.rs | — | — | datom.rs, lib.rs |
| 019 | — | — | — | — | — | error.rs |
| 020 | — | — | durability.rs | — | — | — |
| 021 | — | — | — | — | — | clock.rs, error.rs |
| 022 | — | — | — | — | — | — |
| 023 | — | — | — | — | — | — |
| 024 | — | — | — | — | — | — |
| 025 | — | — | — | — | — | — |
| 026 | — | — | — | — | — | datom.rs, lib.rs |
| 027 | — | — | — | — | — | — |
| 028 | — | — | — | — | — | — |
| 029 | Performance.lean | — | — | — | — | — |
| 030 | — | — | — | — | — | — |
| 031 | Performance.lean | — | — | — | test_schema.rs | schema.rs |
| 032 | Performance.lean | — | — | — | — | schema.rs |
| 033 | Decisions.lean, Federation.lean | — | — | — | — | — |
| 034 | — | — | — | — | — | — |
| 035 | Decisions.lean | — | — | — | — | — |
| 036 | — | — | — | — | — | — |
| 037 | Federation.lean | — | — | — | — | — |
| 038 | — | — | — | — | — | — |
| 039 | Federation.lean | — | — | — | — | — |
| 040 | Federation.lean | — | — | — | — | — |
| 041 | — | — | — | — | — | — |
| 042 | — | — | — | — | — | — |
| 043 | Federation.lean | — | — | — | — | — |
| 044 | Federation.lean | — | — | — | — | — |
| 045 | ProllyTree.lean | — | — | — | — | — |
| 046 | ProllyTree.lean | — | — | — | — | — |
| 047 | — | — | — | — | — | — |
| 048 | — | — | — | — | — | — |
| 049 | ProllyTree.lean | — | — | — | — | — |
| 050 | — | — | — | — | — | — |
| 051 | VKN.lean | — | — | — | — | — |
| 052 | VKN.lean | — | — | — | — | — |
| 053 | VKN.lean | — | — | — | — | — |
| 054 | VKN.lean | — | — | — | — | — |
| 055 | VKN.lean | — | — | — | — | — |

---

## Gap Register

### Phase Gate Verdicts

| Boundary | Verdict | Evidence |
|---|---|---|
| Spec ↔ Lean | PARTIAL | 4a has substantive Lean coverage for `001/002/003/004/010/012/013/015/016/017/018/029/031/032`, but large parts of `019-028` and `030` still have no Lean theorem trace. |
| Lean ↔ Tests | PARTIAL | The core CRDT laws line up cleanly across Lean, proptest, Kani, Stateright, and integration; many 4a performance/durability/ergonomics invariants do not yet show theorem-to-test symmetry. |
| Tests ↔ Types | PARTIAL | Newtypes, typestate, and typed errors encode several 4a properties, but many invariants are only tested procedurally or only hinted in types. |
| Types ↔ Impl | FAIL | `cargo check`, `cargo test`, and `cargo clippy -- -D warnings` do not pass, so the implementation does not currently satisfy the cleanroom/tooling contract. |

**Overall Phase 4a gate verdict: FAIL**

### GAP-001: Green-workspace gate is currently broken

**Type**: Major  
**Traces to**: INV-FERR-023, cleanroom standards, phase-gate “Types ↔ Impl” boundary  
**Severity**: Blocking  
**Leverage**: High  
**Phase**: 4a  
**Remediation effort**: S  
**Evidence**: `cargo check` and `cargo test` both fail on missing docs for `ferratomic-verify/stateright/mod.rs`; `cargo clippy -- -D warnings` fails on 8 issues including `schema_evolution.rs`, `store.rs`, `db.rs`, and `backpressure.rs`.

### GAP-002: Phase 4a invariant closure is incomplete

**Type**: Major  
**Traces to**: INV-FERR-010, 017, 019-030, 032  
**Severity**: Blocking  
**Leverage**: High  
**Phase**: 4a  
**Remediation effort**: M  
**Evidence**: Current-phase trace totals are `17 implemented / 10 partial / 5 unimplemented`, and the session continuation docs explicitly state that `bd-2qv` and `bd-3cn` remain prerequisites for “Phase 4a DONE.” |

### GAP-003: Public architecture docs overstate present implementation reality

**Type**: Moderate  
**Traces to**: ADR-FERR-003, 4a/4b phase boundary  
**Severity**: Degrading  
**Leverage**: High  
**Phase**: 4a→4b  
**Remediation effort**: S  
**Evidence**: `README.md` and `ferratomic-core/src/lib.rs` still describe writer actors, group commit, and prolly storage as if they are current, while `ferratomic-core/src/db.rs` is explicitly a Mutex-serialized writer and `snapshot.rs`, `transport.rs`, and `topology.rs` are TODO stubs.

### GAP-004: Phase 4b is proof/spec-forward but code-empty

**Type**: Frontier  
**Traces to**: INV-FERR-045..050  
**Severity**: Degrading if started early; expected if 4a still open  
**Leverage**: Medium  
**Phase**: 4b  
**Remediation effort**: M-L  
**Evidence**: `045`, `046`, and `049` have Lean coverage, but the entire 4b set is still `0 implemented / 0 partial / 6 unimplemented` by code+test trace.

### GAP-005: Federation and VKN remain mostly design artifacts

**Type**: Frontier  
**Traces to**: INV-FERR-033..055 outside the 4a core  
**Severity**: Cosmetic now, future-blocking later  
**Leverage**: Medium  
**Phase**: 4c/4d  
**Remediation effort**: L  
**Evidence**: 4c is `0 implemented / 1 partial / 12 unimplemented`; 4d is `0 / 0 / 4`. Most of this area is Lean-only or spec-only, which is acceptable today but should not be confused with delivered capability.

### GAP-006: Worktree hygiene is noisy enough to obscure real signal

**Type**: Moderate  
**Traces to**: Process discipline  
**Severity**: Degrading  
**Leverage**: Medium  
**Phase**: Cross-phase  
**Remediation effort**: S  
**Evidence**: `git status --short` reports `289` entries, overwhelmingly generated `.beads/.br_history` churn. This does not look like a logic defect, but it makes review and reproducibility harder than necessary.

---

## Roadmap

### Decision Matrix

| Decision | Option A | Option B | Correctness | Performance | Complexity | Spec Alignment | Recommendation |
|---|---|---|---|---|---|---|---|
| What to do next | Continue `bd-3gk` Phase 4b expansion because `bv` ranks it highest | Close the 4a gate first (`build green` + `bd-2qv` + `bd-3cn`) | A: 0, B: + | A: + later, B: 0 now | A: − | A: −, B: + | **Option B**. The decisive criterion is spec alignment: the session docs explicitly block 4b until 4a is done. |
| How to handle architecture drift | Keep README/lib docs aspirational | Rewrite docs to match the actual 4a Mutex-based implementation | A: 0, B: + | A: 0, B: 0 | A: + short-term, B: + medium-term | A: −, B: + | **Option B**. The decisive criterion is trustworthiness of project state. |
| How to interpret current test inventory | Count the source tests as “good enough” | Treat buildable test enumeration as required and fix the workspace first | A: − | A: 0 | A: + short-term | A: −, B: + | **Option B**. A test suite you cannot run is not a closed verification loop. |

### Tactical Plan (Next 1-2 Sessions)

1. **Restore a green workspace**
   - **Issue**: needs filing, or absorb into the current cleanroom-fix bead set
   - **Files**: `ferratomic-verify/stateright/mod.rs`, `ferratomic-verify/src/lib.rs`, `ferratomic-core/src/schema_evolution.rs`, `ferratomic-core/src/store.rs`, `ferratomic-core/src/db.rs`, `ferratomic-core/src/backpressure.rs`
   - **Effort**: S
   - **Unblocks**: `cargo check`, `cargo test`, `cargo clippy`, phase-gate “Types ↔ Impl”
   - **Prompt**: `07-bug-triage.md` → `05-implementation.md` → `06-cleanroom-review.md`

2. **Close the explicit Phase 4a spec hardening backlog**
   - **Issue**: `bd-2qv`, starting with `bd-1p3`
   - **Files**: `spec/02-concurrency.md`, possibly `spec/03-performance.md`, supporting proof/test files
   - **Effort**: M
   - **Unblocks**: formal closure of `INV-FERR-020..024` and the 4a gate
   - **Prompt**: `08-task-creation.md` → `02-lean-proofs.md` → `03-test-suite.md`

3. **Close the 4a implementation-completion backlog**
   - **Issue**: `bd-3cn`
   - **Files**: `ferratomic-core/src/*`, `ferratomic-verify/integration/*`
   - **Effort**: M
   - **Unblocks**: “Phase 4a DONE” dependency chain and realistic 4b entry
   - **Prompt**: `05-implementation.md` → `06-cleanroom-review.md`

4. **Align public architecture docs with the actual phase**
   - **Issue**: needs filing
   - **Files**: `README.md`, `ferratomic-core/src/lib.rs`, possibly `docs/design/FERRATOMIC_ARCHITECTURE.md`
   - **Effort**: S
   - **Unblocks**: truthful status communication, lower reviewer confusion, better next-session initialization
   - **Prompt**: `05-implementation.md`

5. **Only after 4a passes, resume Phase 4b spec/impl work**
   - **Issue**: `bd-3gk`, then `bd-85j.13`
   - **Files**: `spec/06-prolly-tree.md`, `ferratomic-verify/lean/Ferratomic/ProllyTree.lean`, future core block-store files
   - **Effort**: M-L
   - **Unblocks**: real 4b execution and later 4c federation work
   - **Prompt**: `02-lean-proofs.md` → `03-test-suite.md` → `05-implementation.md`

### Strategic Plan (Next Phase Boundary)

Phase 4a gate checklist:
- `cargo check --workspace` passes.
- `cargo clippy --workspace -- -D warnings` passes.
- `cargo test --workspace` passes.
- `INV-FERR-020..024` have complete Level 0/1/2 contracts and corresponding proof/test traces.
- The current-phase partial/unimplemented set is either closed or explicitly demoted out of the 4a gate.
- Public docs no longer describe future-phase architecture as if it is already delivered.

Critical path:
1. Green workspace fix
2. `bd-2qv`
3. `bd-3cn`
4. Phase 4a DONE
5. `bd-3gk`
6. `bd-85j.13`

Risk mitigation:
- **Build/lint regressions**: Make `cargo check`, `cargo clippy -- -D warnings`, and `cargo test` the non-negotiable close-out trio for each execution slice.
- **Phase bleed**: Reject any 4b implementation work until the 4a gate checklist is fully satisfied.
- **Architecture/status drift**: Phase-tag all public architecture claims so aspirational 4b/4c sections cannot be mistaken for present implementation.

Recommended swarm configuration after this review:
- **Agent 1**: cleanroom/build restore on `ferratomic-verify` + core lint blockers
- **Agent 2**: `bd-2qv` spec/proof/test closure in `spec/` and `ferratomic-verify/`
- **Agent 3**: documentation/status alignment in `README.md`, `docs/design/`, and crate docs

Disjoint file sets:
- Agent 1: `ferratomic-verify/stateright/*`, `ferratomic-verify/src/lib.rs`, core lint-hit files
- Agent 2: `spec/*`, `ferratomic-verify/lean/*`, `ferratomic-verify/proptest/*`, `ferratomic-verify/kani/*`
- Agent 3: `README.md`, `docs/design/*`, crate-level `lib.rs` docs

---

## Retrospective

### What Is Going Well?

The first thing I would preserve is the project’s algebraic spine. Ferratomic is not waving vaguely at formal methods; it actually has machine-checked CRDT laws in Lean, concrete Kani harnesses, a Stateright model, and a real property-test layer. The important nuance is that these are not toy placeholders. The core proofs I inspected are substantive, `lake build` is green, and the implemented 4a durability and CRDT paths are genuinely anchored in named invariants. That is worth protecting because it is the difference between “spec-inspired Rust” and an actual Curry-Howard workflow.

The second strength is structural discipline. The crate DAG is clean, production modules stay within the stated file-size budgets, `unsafe` is forbidden, and the main type surfaces are sensibly narrow. `ferratom` still looks like a leaf crate rather than a dumping ground, and `writer.rs` is a good example of the intended style: newtypes, typestate, and error categories doing real work. I would preserve that because structural quality is much easier to lose than to regain.

The third strength is that the bead graph is useful, not ceremonial. The graph is acyclic, there are no active alerts, the next-work recommendations are coherent, and the project has clearly been using review findings to generate new work. That matters because once the codebase gets larger, the team will need the task graph to prevent phase bleed and local optimization.

### What Is Going Poorly?

The biggest concern is phase-boundary honesty. The repo’s own session docs say 4b cannot start until 4a is done, but the live frontier has already psychologically moved to 4b because `bv` says `bd-3gk` is the highest-impact next bead. That is rational from a dependency-graph perspective and still wrong from a methodology perspective. If that continues, the project will slowly normalize “mostly done” gates, which is exactly the kind of slippage formal methods are supposed to prevent.

The second concern is status drift between the public story and the current implementation. The README and crate docs still describe writer actors, group commit, and prolly storage as if they are current architecture, while the actual 4a system is a Mutex-serialized writer with several public TODO modules. I don’t think this is dishonest; I think it is a symptom of forward-looking design text not being phase-tagged aggressively enough. The risk is that reviewers and future agents start reasoning from the aspirational architecture rather than the code that actually exists.

The third concern is that the workspace is not green at a moment when the project most needs cleanroom certainty. The present failures are small and fixable, which is exactly why they matter. A project like this cannot afford to blur “almost compiling” with “phase closed.” If the tooling contract is soft, the proof/test/type contract will also soften over time.

### What Surprised Me?

I expected either shallow formalism or broken Lean. Instead, I found the opposite: the Lean side is better than the Rust tooling surface right now. Zero `sorry`, successful `lake build`, and several genuinely meaningful theorem families are already in place. The surprise is that the project’s strongest signal is not implementation breadth but proof quality in the areas it has chosen to tackle.

That has an implication I did not expect going in: Ferratomic’s main risk is not “can the team do hard formal work?” The evidence says yes. The risk is execution sequencing. The project is more likely to stumble by advancing phases out of order or letting status drift accumulate than by failing to produce proofs.

### What Would I Change?

If I could change one thing, I would make the phase gate operational instead of cultural. Concretely: no bead labeled `phase-4b` should become actionable while the 4a gate checklist is red, and all public docs should carry explicit phase tags for architectural claims. That single intervention has the highest leverage because it converts the methodology from a document into a control system. Right now the project knows what the rule is; it does not yet consistently enforce it.

### Confidence Assessment

My confidence that this project can reach True North is **7/10**.

- **Correctness confidence: 8/10**. The formal core is real, not decorative. A fully green Rust workspace plus broader current-phase invariant closure would raise this to 9.
- **Completion confidence: 5/10**. The scope from 4a to 4d is still very large, and most of 4b/4c/4d remains spec- or Lean-only. A passed 4a gate and one delivered 4b slice would raise this to 6.
- **Architecture confidence: 6/10**. The present structure can support the embedded 4a system well, but the future distributed/prolly/federation story is still largely design-level. A phase-tagged architecture doc plus one real 4b substrate implementation would raise this to 7.
