# 13 Progress Review & Strategic Assessment

> **Purpose**: Holistic project assessment after a major body of work completes.
> Quantitative ratings, gap analysis, strategic recommendations, and agent retrospective.
> This is the "state of the union" — not a code review, not a bug hunt.
>
> **DoF**: Varies by phase. Low (measurement) → Structured (scoring) → High (analysis) → Very High (retrospective).
>
> **Cognitive mode**: Assessment. Output is a graded report, not code or tasks.
>
> **Model gate**: Opus 4.6 with /effort max or GPT 5.4 xhigh. Lower-capability models
> lack the sustained reasoning depth required for Phase 3-5.

---

## When to Use This Prompt

- A swarm has completed a phase or major epic
- You need to know "where are we?" before planning the next phase
- A phase gate decision is approaching (can Phase N+1 begin?)
- Velocity has stalled and you need to understand why
- Periodic check-in (e.g., weekly) on a long-running project

**This prompt does NOT produce code, tasks, or fixes.** It produces a graded assessment
document. Tasks and fixes are crystallized in follow-up sessions using
[07-bug-triage.md](07-bug-triage.md) and [08-task-creation.md](08-task-creation.md).

---

## Parameters

| Parameter | Default | Effect |
|-----------|---------|--------|
| `PHASE` | current phase | Which phase to assess: `4a`, `4b`, `all` |
| `DEPTH` | `standard` | `quick` (Phases 1+2, ~15 min), `standard` (all 5, ~45 min), `deep` (all 5 + full coverage matrix, ~90 min) |
| `SINCE` | last review or last tagged commit | Git log window for velocity metrics |
| `FOCUS` | none | Crate or spec section to examine more closely |

---

## Phase 0: Ground Yourself

Before measurement begins, internalize the project's formal identity.

```bash
# Orientation (skip if already loaded from 01-session-init)
cat AGENTS.md
cat spec/README.md

# What methodology applies?
ms load spec-first-design -m --pack 2000

# What's the current frontier?
bv --robot-triage
br list --status=open
```

**Checkpoint**: You must be able to state from memory:
- The core algebraic identity: `Store = (P(D), ∪)` — G-Set CRDT semilattice
- The phase ordering and current phase
- The hard constraints (C1, C2, C4, INV-FERR-023, NEG-FERR-001)
- The crate dependency DAG: `ferratom → ferratomic-core → ferratomic-datalog`

If you cannot, re-read until you can. The review is meaningless without this grounding.

---

## Phase 1: Measurement (Low DoF)

**Objective**: Collect raw data. No interpretation. No judgment. Numbers only.

Execute these commands and record the raw output. Every metric must be backed by
a command output or file read — no estimates, no "approximately."

### 1.1 Issue Graph State

```bash
bv --robot-triage                    # Ranked recommendations, health, velocity
bv --robot-insights                  # PageRank, betweenness, k-core, HITS, cycles
bv --robot-alerts                    # Stale issues, cascading blockers, mismatches
br list --status=open | wc -l        # Open count
br list --status=closed | wc -l      # Closed count
br ready | wc -l                     # Ready (unblocked) count
```

### 1.2 Git Velocity

```bash
git log --oneline --since="${SINCE}" | wc -l                      # Commit count
git log --oneline --since="${SINCE}" --format="%H" | \
  xargs -I{} git diff-tree --no-commit-id --name-only -r {} | \
  sort -u | wc -l                                                  # Unique files touched
git diff --stat $(git log --since="${SINCE}" --format="%H" | tail -1)..HEAD  # Net LOC delta
```

### 1.3 Build Health

```bash
CARGO_TARGET_DIR=/data/cargo-target cargo check --workspace 2>&1 | tail -1
CARGO_TARGET_DIR=/data/cargo-target cargo clippy --workspace -- -D warnings 2>&1 | tail -5
CARGO_TARGET_DIR=/data/cargo-target cargo test --workspace 2>&1 | grep -E "test result|running"
```

### 1.4 Codebase Size

```bash
# Per-crate LOC (excluding tests)
find ferratom/src -name '*.rs' | xargs wc -l
find ferratomic-core/src -name '*.rs' | xargs wc -l
find ferratomic-datalog/src -name '*.rs' | xargs wc -l
find ferratomic-verify/src -name '*.rs' | xargs wc -l
```

Compare against budgets from AGENTS.md:
- `ferratom`: < 2,000 LOC
- `ferratomic-core`: < 10,000 LOC
- `ferratomic-datalog`: < 5,000 LOC

### 1.5 Proof Health

```bash
# Lean sorry count (0 = all proofs discharged)
grep -r "sorry" ferratomic-verify/lean/ --include="*.lean" -c 2>/dev/null || echo "0"

# Test count
CARGO_TARGET_DIR=/data/cargo-target cargo test --workspace -- --list 2>&1 | grep -c ": test"
```

### 1.6 Spec-Implementation Drift (Manual)

Since no mechanical DDIS CLI index exists, assess drift by reading the spec and
cross-referencing against the implementation. For each INV-FERR in the assessed phase:

1. Read the spec invariant (Level 0 algebraic law, Level 1 state invariant, Level 2 Rust contract)
2. Search for the implementing code (`grep -r "INV-FERR-NNN" --include="*.rs"`)
3. Search for the test (`grep -r "inv_ferr_NNN\|INV.FERR.NNN" --include="*.rs"`)
4. Classify: **implemented** (code + test), **partial** (code or test, not both),
   **unimplemented** (neither), **contradicted** (code violates spec)

Compute drift score:
```
drift = |unimplemented| + |partial| + 2 × |contradicted|
```

### 1.7 Coverage Matrix (DEEP MODE ONLY)

In deep mode, construct the full matrix. Read `spec/README.md` for the complete
INV-FERR list, then for each invariant check 6 verification layers:

| INV-FERR | Lean | proptest | Kani | Stateright | Integration | Type-level |
|----------|------|----------|------|------------|-------------|------------|
| 001 | ✓/sorry/— | test_name/— | harness/— | model/— | test/— | enforced/— |
| ... | | | | | | |

Search locations:
- **Lean**: `ferratomic-verify/lean/Ferratomic/*.lean`
- **proptest**: `ferratomic-verify/tests/proptest/*.rs`
- **Kani**: `ferratomic-verify/src/kani/*.rs`
- **Stateright**: `ferratomic-verify/stateright/*.rs`
- **Integration**: `ferratomic-verify/tests/integration/*.rs`
- **Type-level**: `ferratom/src/*.rs` (look for typestate, newtype, `#![forbid(...)]`)

Empty cells are gaps. Cells with `sorry` or `todo!()` are partial.

### Output: METRICS Artifact

Record all raw data in a structured block. Tables, counts, percentages.
No prose. No judgment. This artifact feeds Phase 2.

---

## Phase 2: Scoring (Structured DoF)

**Objective**: Rate the project across 10 quality vectors. Every grade is anchored
to specific evidence from Phase 1.

### Scoring Rubric

Use a dual scale: letter grade (A through F) for qualitative gestalt, plus a
numeric score (1.0-10.0, one decimal) for weighted composite calculation.

| Grade | Numeric | Meaning |
|-------|---------|---------|
| A | 9.0-10.0 | Exemplary. Zero-defect cleanroom standard. No actionable gaps. |
| A- | 8.5-8.9 | Near-exemplary. Minor gaps, all non-critical. |
| B+ | 8.0-8.4 | Strong. Core properties verified. Secondary gaps exist. |
| B | 7.0-7.9 | Good. Solid foundation with identifiable improvement areas. |
| B- | 6.5-6.9 | Adequate-plus. Gaps are real but not structural. |
| C+ | 6.0-6.4 | Adequate. Meets minimum bar. Multiple improvement areas. |
| C | 5.0-5.9 | Functional. Works but significant quality concerns. |
| C- | 4.5-4.9 | Below adequate. Structural concerns emerging. |
| D | 3.0-4.4 | Deficient. Core properties questionable. Rework needed. |
| F | 1.0-2.9 | Failing. Fundamental issues. Stop and reassess. |

### The 10 Vectors

Score each vector independently. For each, write: the letter grade, the numeric
score, and 2-4 sentences of evidence citing specific INV-FERR IDs, file paths,
test names, or metrics from Phase 1.

#### 1. Correctness (Weight: 3×)

*Do the algebraic laws hold? Are the CRDT properties verified?*

- A: All CRDT laws (INV-FERR-001/002/003/010) proven in Lean with 0 sorry AND
  proptest 10K+ AND Stateright model. No known violations.
- C: Laws verified by proptest only. Some Lean sorry. No known violations.
- F: Known algebraic law violations, or laws not verified at all.

#### 2. Completeness (Weight: 2×)

*How much of the spec for the assessed phase is implemented?*

- A: >90% of INV-FERR in the assessed phase have code + tests.
- C: 60-90% coverage. Gaps are identified and tracked as issues.
- F: <60% coverage, or gaps in current phase are not tracked.

#### 3. Verification Depth (Weight: 2×)

*How many independent verification layers cover each invariant?*

- A: Current-phase INV-FERR have 4+ verification layers populated.
- C: Current-phase INV-FERR have 2-3 layers. Future phases have 1+.
- F: Single-layer verification only, or layers are non-functional.

#### 4. Code Quality (Weight: 1.5×)

*Does the code meet cleanroom standards from AGENTS.md?*

- A: All hard limits met (500 LOC/file, 50 LOC/fn, complexity 10, 5 params).
  `#![forbid(unsafe_code)]` in all crates. No `unwrap()` in production code.
  <5 open defects.
- C: Most limits met. <15 open defects. Minor violations tracked.
- F: Hard limit violations untracked. >20 open defects. `unsafe` present.

#### 5. Architecture (Weight: 1.5×)

*Dependency discipline, modularity, separation of concerns, single responsibility.*

The Braid lesson: poor architecture masks performance problems and compounds
technical debt. This vector catches structural issues before they cascade.

- A: Crate DAG acyclic. LOC budgets met. Every module has one concept.
  No God modules. Public API surface is minimal.
- C: DAG acyclic but some modules mix concerns. Within LOC budgets.
- F: Dependency cycles, God modules, or budget violations.

#### 6. Performance (Weight: 1.5×)

*Are spec performance targets met or credibly on track?*

The Braid lesson: performance issues discovered late indicate architectural
problems. Early measurement prevents late-stage rearchitecture.

- A: All measurable INV-FERR-025..028 targets benchmarked and met.
- C: Targets not yet benchmarked but no known violations. No O(n)
  operations hiding inside O(1) interfaces.
- F: Known target misses without filed issues, or benchmarks don't exist
  and the assessed phase is 4b+.

#### 7. Durability (Weight: 2×)

*Can the system recover from crashes without data loss?*

- A: WAL fsync ordering (INV-FERR-008) verified. Checkpoint round-trip
  (INV-FERR-013) proven. Recovery correctness (INV-FERR-014) tested.
  Cold start cascade (storage.rs) implemented.
- C: Recovery implemented and tested but not formally verified.
- F: Recovery path incomplete, untested, or known data-loss scenarios.

#### 8. Ergonomics (Weight: 0.5×)

*API design, error messages, developer experience.*

- A: Typestate enforced for all lifecycles. Errors are actionable
  (caller can match on category). API surface is minimal and intuitive.
- C: Reasonable API. Some rough edges. Errors could be more specific.
- F: Confusing API. Opaque errors. Invalid states constructible.

#### 9. Axiological Alignment (Weight: 2×)

*Does the work serve True North? Is every module traceable to a named principle?*

True North: Ferratomic provides the universal substrate — an append-only datom
store with content-addressed identity, CRDT merge, indexed random access, and
cloud-scale distribution.

- A: Every module traces to a named INV-FERR, ADR-FERR, or constraint.
  No speculative code. No features without spec grounding.
- C: Most work traces. Some code exists without clear spec backing.
- F: Significant work diverges from True North. Feature creep present.

#### 10. Process Health (Weight: 1×)

*Is the spec-first discipline maintained? Are defects tracked? Is velocity steady?*

- A: Phase gates respected (no Phase N+1 work before N passes isomorphism check).
  Defects tracked in beads with dependency edges. Cleanroom reviews performed.
  Steady commit velocity.
- C: Minor phase bleeding. Most defects tracked. Occasional missed reviews.
- F: Phases skipped. Defects accumulate untracked. No reviews performed.

### Composite Score

Calculate the weighted GPA:

```
composite = Σ(score_i × weight_i) / Σ(weight_i)

Weights: Correctness=3, Completeness=2, Verification=2, Quality=1.5,
         Architecture=1.5, Performance=1.5, Durability=2, Ergonomics=0.5,
         Axiological=2, Process=1
         Total weight = 17
```

Convert composite to letter grade using the same scale.

### Output: SCORECARD Artifact

A table of 10 rows (vector, grade, numeric, weight, evidence) plus composite GPA.

---

## Phase 3: Gap Analysis (High DoF)

**Objective**: Identify what's missing, broken, or weak. Structured discovery driven
by the metrics and scores from Phases 1-2.

### 3.1 Spec Coverage Gaps

From Phase 1.6 (or 1.7 in deep mode), classify each gap:

| Gap Type | Definition | Urgency |
|----------|-----------|---------|
| **Critical** | INV-FERR in current phase with code that contradicts spec | Fix before any new work |
| **Major** | INV-FERR in current phase with no implementation or no tests | Fix before phase gate |
| **Moderate** | INV-FERR in current phase with only 1-2 verification layers | Strengthen before next review |
| **Frontier** | INV-FERR in future phase with no implementation | Expected — quantify only |

### 3.2 Phase Gate Assessment

For each completed or in-progress phase, evaluate the isomorphism check.
A phase gate passes when all four correspondence checks hold:

| Boundary | Check | How to verify |
|----------|-------|--------------|
| Spec ↔ Lean | Lean theorem statements match spec Level 0 algebraic laws | Compare theorem names/statements against INV-FERR Level 0 |
| Lean ↔ Tests | Test names and strategies correspond to Lean theorem structure | Cross-reference test_inv_ferr_NNN names against lean theorem names |
| Tests ↔ Types | Types encode what tests assert (Curry-Howard) | Check that type cardinality matches valid state count |
| Types ↔ Impl | Implementation satisfies type contracts without `unsafe` escape | Run cargo check + clippy. No `unwrap()` in production paths |

Each boundary gets a verdict: **PASS**, **PARTIAL** (some correspondences hold),
or **FAIL** (structural mismatch). A single FAIL blocks the phase gate.

### 3.3 Risk Register

For each gap identified in 3.1 and each PARTIAL/FAIL in 3.2:

```markdown
### GAP-NNN: <one-line description>

**Type**: Critical | Major | Moderate | Frontier
**Traces to**: INV-FERR-NNN
**Severity**: Blocking | Degrading | Cosmetic
**Leverage**: High (fixes 3+ downstream) | Medium | Low
**Phase**: Which phase is affected
**Remediation effort**: S (<1 session) | M (1-3 sessions) | L (3+ sessions)
**Evidence**: <what the metrics show>
```

Order the register by severity × leverage (blocking+high first).

### Output: GAP REGISTER Artifact

Ordered list of gaps. Phase gate verdicts. This feeds Phase 4.

---

## Phase 4: Strategy (High DoF)

**Objective**: Convert gaps into prioritized action. Tactical and strategic.

### 4.1 Decision Matrix

For each unresolved design tradeoff surfaced by the gap analysis, beads history,
or working tree state:

| Decision | Option A | Option B | Correctness | Performance | Complexity | Spec Alignment | Recommendation |
|----------|----------|----------|-------------|-------------|------------|----------------|----------------|

Criteria columns are drawn from the project's own INV-FERR targets and ADR-FERR
constraints. Score each option per criterion as +/0/−. The recommendation must
cite the decisive criterion.

Only include decisions that are genuinely open. Settled ADR-FERR decisions are
not relitigated here — if a gap analysis finding suggests an ADR should be
reconsidered, flag it explicitly as "ADR-FERR-NNN reconsideration" with evidence.

### 4.2 Tactical Plan (Next 1-2 Sessions)

Top 5 actions ordered by severity × leverage from the gap register:

```markdown
1. **[ACTION]**: <what to do>
   - **Issue**: <beads ID or "needs filing">
   - **Files**: <which files change>
   - **Effort**: S/M/L
   - **Unblocks**: <what downstream work this enables>
   - **Prompt**: <which lifecycle prompt to use — 05, 06, 07, etc.>
```

Cross-reference against `bv --robot-next` and `bv --robot-plan` to ensure the
plan respects the dependency graph.

### 4.3 Strategic Plan (Next Phase Boundary)

**Phase gate checklist**: What must be true before Phase N+1 can begin?
Enumerate each remaining item as a concrete, verifiable predicate.

**Critical path**: Trace the longest dependency chain from "now" to "phase gate."
Name each node (beads ID or task description).

**Risk mitigation**: For the top 3 risks from the gap register, state the
contingency plan if the risk materializes.

**Swarm configuration** (if applicable): Recommend agent count, specializations,
and disjoint file set allocation for the next execution phase.

### Output: ROADMAP Artifact

Decision matrix + tactical plan + strategic plan.

---

## Phase 5: Agent Retrospective (Very High DoF)

**Objective**: Your honest, subjective assessment. What no metric captures.
Speak in first person. Be fully candid — process concerns, methodology criticism,
and trajectory warnings are not just permitted but expected.

### 5.1 What Is Going Well?

Name 3 specific strengths. For each: what it is, why it matters, what evidence
supports it, and whether it should be preserved, doubled-down on, or formalized.

### 5.2 What Is Going Poorly?

Name 3 specific weaknesses or concerns. Not code bugs (those are in the gap
register) — focus on process, methodology, trajectory, or structural concerns.
For each: what it is, what consequence it risks, and what would fix it.

### 5.3 What Surprised You?

During this review, what did you find that you didn't expect? Positive or negative.
What does it imply about the project's trajectory that wasn't visible from the
issue tracker or git log alone?

### 5.4 What Would You Change?

If you could change ONE thing about the project's structure, methodology, or
direction — what would it be and why? This is not a wishlist. It is the single
highest-leverage meta-intervention you can identify. Justify it from first principles.

### 5.5 Confidence Assessment

Rate your confidence (1-10) that this project will achieve its True North:
"Universal substrate — append-only datom store with content-addressed identity,
CRDT merge, indexed random access, cloud-scale distribution."

Separately rate confidence on three sub-dimensions:
- **Correctness confidence**: Will the algebraic guarantees hold under production load?
- **Completion confidence**: Will all phases (through 4d) be completed?
- **Architecture confidence**: Will the current architecture support cloud-scale distribution?

For each sub-rating, state what would increase your confidence by 1 point.

### Output: RETRO Artifact

Structured prose, ~500-800 words. This is the only section where the agent
speaks in first person and offers opinion rather than measurement.

---

## Final Assembly

Assemble all 5 artifacts into the review document:

```markdown
# Ferratomic Progress Review — YYYY-MM-DD

> **Reviewer**: <model name and version>
> **Scope**: <phase assessed, depth mode, SINCE date>
> **Duration**: <phases completed, approximate time>

---

## Executive Summary

<3-5 sentences. Composite grade. Top 3 strengths. Top 3 gaps.
Single most important next action.>

---

## Scorecard

<Phase 2 artifact. 10-vector table + composite GPA.>

---

## Metrics

<Phase 1 artifact. Structured data: drift, issues, velocity, build, LOC, proofs, tests.>

---

## Coverage Matrix (DEEP MODE ONLY)

<Phase 1.7 artifact. Full INV-FERR × verification layer table.>

---

## Gap Register

<Phase 3 artifact. Ordered gaps with severity, leverage, remediation.>

---

## Phase Gate Assessment

<Phase 3.2 artifact. Boundary verdicts: PASS / PARTIAL / FAIL.>

---

## Decision Matrix

<Phase 4.1 artifact. Open tradeoffs with scored options.>

---

## Tactical Plan

<Phase 4.2 artifact. Top 5 actions ordered by priority.>

---

## Strategic Plan

<Phase 4.3 artifact. Phase gate checklist, critical path, swarm config.>

---

## Retrospective

<Phase 5 artifact. Candid agent assessment.>

---

## Appendix: Raw Data

<Collapsed command outputs from Phase 1 for auditability.
Include git log, bv output, test results, LOC counts.>
```

---

## Demonstration: One Vector Scored

To illustrate the expected calibration, here is a complete scoring of one vector
at an intermediate project state:

```markdown
### Correctness: B+ (8.2)

**Evidence**:
- INV-FERR-001/002/003 (CRDT laws): Lean proofs complete (0 sorry for
  commutativity/associativity/idempotency). Proptest with 10K+ cases in
  ferratomic-verify/tests/proptest/semilattice_properties.rs. Stateright
  model verifies SEC convergence with non-vacuous write tracking. **Strong.**
- INV-FERR-010 (merge convergence): Stateright model explores all message
  delivery orderings. Non-vacuous SEC property (bd-272 fix). **Strong.**
- INV-FERR-013 (checkpoint equivalence): Proptest round-trip verified.
  BLAKE3 integrity check in checkpoint.rs. **Adequate.**
- INV-FERR-005 (index bijection): verify_bijection() is debug_assert only.
  No proptest strategy for simultaneous 4-index bijection. **Gap.**
- Kani harnesses reference non-existent APIs (Store::empty,
  to_checkpoint_bytes). These are non-functional. **Gap.**

**Why not A**: Kani harnesses non-functional. Index bijection debug-only.
**Why not B**: Core CRDT laws have triple-layer verification (Lean + proptest +
Stateright). The gaps are secondary paths, not the algebraic foundation.
```

---

## Integration with Other Prompts

| Review finding | Follow-up prompt |
|----------------|-----------------|
| Gap register contains CRITICAL items | [07-bug-triage.md](07-bug-triage.md) |
| Gap register contains spec gaps | [12-deep-analysis.md](12-deep-analysis.md) then [08-task-creation.md](08-task-creation.md) |
| Decision matrix has open tradeoffs | [12-deep-analysis.md](12-deep-analysis.md) |
| Tactical plan ready for execution | [05-implementation.md](05-implementation.md) or [02-lean-proofs.md](02-lean-proofs.md) |
| Phase gate verdict is FAIL | [06-cleanroom-review.md](06-cleanroom-review.md) |
| Review complete, session ending | [09-continuation.md](09-continuation.md) |

---

## Beads Integration

The review itself is a tracked task:

```bash
# Before review
br create --title "Progress review: Phase ${PHASE}" \
  --type task --priority 1 --label "phase-${PHASE}"
br update <id> --status in_progress

# After review (close with composite grade)
br close <id> --reason "Review complete: composite B+ (8.2). 3 critical gaps filed."

# File issues for CRITICAL and MAJOR gaps
br create --title "GAP-001: <description>" --type bug --priority 0
br create --title "GAP-002: <description>" --type bug --priority 1
```

---

## What NOT To Do

- Do not write code during a review. This is assessment, not implementation.
- Do not skip Phase 1. Scores without metrics are opinions without evidence.
- Do not inflate grades. A generous review is a useless review. If the index
  bijection is debug-only, that is a real gap even if the CRDT laws are perfect.
- Do not conflate phases. Measurement is not judgment. Judgment is not planning.
  Planning is not retrospection. Each phase has one cognitive mode.
- Do not suppress process concerns in the retrospective. If the methodology
  has diminishing returns or the spec is over-constraining implementation,
  say so. The review's value is proportional to its honesty.
- Do not relitigate settled ADR-FERR decisions unless the gap analysis produces
  specific evidence that the decision is causing structural harm. Even then,
  flag it — don't unilaterally reverse it.
- Do not produce a review longer than needed. Quick mode is 2 pages.
  Standard mode is 5-8 pages. Deep mode is 10-15 pages. Brevity is a virtue.
  The report is consumed by a human who has 15 other things to read.
