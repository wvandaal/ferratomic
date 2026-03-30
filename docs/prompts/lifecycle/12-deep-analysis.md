# 12 Deep Analysis & Solution Synthesis

> **Purpose**: First-principles decomposition of complex problems. No implementation.
> **DoF**: Very high. Pure discovery and reasoning.
> **Cognitive mode**: Analysis only. Output is understanding, not code.

---

## When to Use This Prompt

- A review or audit surfaces findings that need decomposition
- A design decision has unclear consequences
- A test failure has a non-obvious root cause
- Multiple INV-FERR interact in ways that need untangling
- You need to understand a problem BEFORE deciding what to build

**This prompt produces analysis and proposals. It does NOT produce code.**
Implementation happens in a separate session with a separate prompt.

---

## Phase 1: Ground Yourself

Load context from all available sources. Do not start reasoning until you
have the full picture.

```bash
# What work exists?
br list --status=open
bv --robot-triage

# What happened recently?
cass search "<topic>" --robot --fields minimal --limit 10

# What methodology applies?
ms load spec-first-design -m --full

# What does the spec say?
# Read the relevant spec module(s) from spec/
```

**Checkpoint**: Before proceeding, you must be able to answer:
- What are the relevant INV-FERR invariants?
- What are the relevant ADR-FERR decisions?
- What has already been tried (from session history)?
- What is the dependency graph of the affected components?

---

## Phase 2: First-Principles Decomposition

For each problem or finding, work through these five steps:

### Step 1: Reduce to Axioms

What are the non-negotiable facts?

- Which INV-FERR constrain the solution space?
- Which ADR-FERR are settled decisions (do not relitigate)?
- Which NEG-FERR are failure modes to avoid?
- What are the algebraic properties that must hold? (commutativity, monotonicity, etc.)

### Step 2: Identify Algebraic Structure

What mathematical structure governs this problem?

- Is it a lattice? (partial order with join/meet)
- Is it a monoid? (associative operation with identity)
- Is it a functor? (structure-preserving map between categories)
- Is it a fixed point? (f(x) = x for some known f?)

Name the structure. If you can't name it, you don't understand it yet.

### Step 3: Locate in Dependency Graph

Where does this problem sit in the crate/module dependency DAG?

```
ferratom (types) -> ferratomic-core (engine) -> ferratomic-datalog (query)
                                              -> ferratomic-verify (proofs+tests)
```

- Is the problem in the leaf (types wrong)?
- Is the problem in the core (logic wrong)?
- Is the problem at an interface boundary (contract mismatch)?
- Does fixing it require changes that propagate up or down the DAG?

### Step 4: Assess Severity vs Leverage

Not all problems are equal. Classify:

| Severity | Definition |
|----------|-----------|
| **Blocking** | Prevents progress on downstream work |
| **Degrading** | Causes incorrect results or spec violations |
| **Cosmetic** | Suboptimal but functionally correct |

| Leverage | Definition |
|----------|-----------|
| **High** | Fix unblocks 3+ downstream tasks or fixes 3+ issues |
| **Medium** | Fix addresses the specific problem |
| **Low** | Fix improves one thing marginally |

**Priority = severity x leverage.** A blocking problem with high leverage is the
first thing to fix. A cosmetic problem with low leverage is last (or never).

### Step 5: Check for Hidden Coupling

The most dangerous problems are the ones that look local but aren't.

- Does fixing X break Y? (Check all callers of the affected function.)
- Does the fix change the algebraic properties? (e.g., does it break commutativity?)
- Does the fix violate any INV-FERR that isn't directly related?
- Is there a dependency cycle hiding in the solution?

---

## Phase 3: Solution Synthesis

For each problem identified in Phase 2, produce exactly ONE solution.
Do not propose alternatives -- pick the best one and justify it.

### Solution Format

For each solution, write:

```
### FINDING-N: <one-line problem statement>

**Root cause**: <why this happens, traced to specific code or spec element>

**Proposed fix**: <what to change, specifically>

**Verification**: <how to confirm the fix works>
  - Test: <specific test to write or existing test that should pass>
  - INV-FERR: <which invariant this restores or upholds>

**Risk**: <what could go wrong with this fix>
  - <specific risk 1>
  - Mitigation: <how to detect or prevent>

**Effort**: <S/M/L> — <one sentence justification>

**Depends on**: <other findings that must be resolved first, if any>
```

### Ordering the Solutions

After writing all solutions, order them by:

1. Blocking severity first (unblock downstream work)
2. Then by leverage (most downstream impact)
3. Then by dependency order (fix X before Y if Y depends on X)

This ordering becomes the execution plan for the implementation session.

---

## Output Checklist

Before declaring analysis complete, verify:

- [ ] Every finding traces to a specific INV-FERR, ADR-FERR, or NEG-FERR
- [ ] Every solution has a verification plan (not "it should work")
- [ ] No solution violates a settled ADR-FERR
- [ ] Solutions are ordered by severity x leverage
- [ ] Dependency edges between solutions are explicit
- [ ] Risks are specific, not generic ("might break something")
- [ ] Effort estimates are calibrated (S = < 1 hour, M = 1-4 hours, L = 4+ hours)
- [ ] The analysis is COMPLETE -- no "TBD" or "needs further investigation"

If you cannot complete the analysis, state exactly what information is missing
and what command or file read would provide it.

---

## What This Prompt Does NOT Do

- No code writing. Not even pseudocode (unless it clarifies algebraic structure).
- No task creation. Crystallize tasks in a follow-up session using [08-task-creation.md](08-task-creation.md).
- No optimization. That's [10-benchmarking.md](10-benchmarking.md).
- No implementation. That's [05-implementation.md](05-implementation.md) or a continuation prompt.

This is the "thinking" prompt. Its output is a document that a successor agent
(or human) can execute against without re-deriving the analysis.
