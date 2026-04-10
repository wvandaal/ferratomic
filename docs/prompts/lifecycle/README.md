# Ferratomic Execution Lifecycle Prompts

Optimized prompts for driving every phase of ferratomic development with AI agents.

## Prompt Index

| # | Prompt | Phase | DoF | When to use |
|---|--------|-------|-----|-------------|
| 01 | [Session Init](01-session-init.md) | Any | High | Starting a new session (cold start) |
| 02 | [Lean Proofs](02-lean-proofs.md) | 1 | Mixed | Writing Lean 4 theorems for INV-FERR |
| 03 | [Test Suite](03-test-suite.md) | 2 | Low | Writing tests before implementation (red phase) |
| 04 | [Type Definition](04-type-definition.md) | 3 | Low | Implementing ferratom crate types |
| 05 | [Implementation](05-implementation.md) | 4 | Low | Implementing workspace crate modules |
| 06 | [Cleanroom Review](06-cleanroom-review.md) | Any | High | Post-implementation adversarial audit |
| 07 | [Bug Triage](07-bug-triage.md) | Any | Mixed | When a defect is discovered |
| 08 | [Task Creation](08-task-creation.md) | Any | Low | Creating new beads issues |
| 09 | [Continuation](09-continuation.md) | Any | Low | End of session — handoff to successor |
| 10 | [Benchmarking](10-benchmarking.md) | 4b | Mixed | Performance measurement and optimization |
| 11 | [Federation Testing](11-federation-testing.md) | 4c | Mixed | Cross-store and distributed verification |
| 12 | [Deep Analysis](12-deep-analysis.md) | Any | Very High | First-principles problem decomposition |
| 13 | [Progress Review](13-progress-review.md) | Any | Varies | Holistic project assessment with scored vectors |
| 14 | [Bead Audit](14-bead-audit.md) | Any | Varies | Audit and harden all open beads to lab-grade |
| 15 | [Prompt Forge](15-prompt-forge.md) | Any | Varies | Design new lifecycle prompts from first principles |
| 16 | [Spec Authoring](16-spec-authoring.md) | 0 | High→Low | Writing new INV-FERR, ADR-FERR, NEG-FERR, spec sections |
| 17 | [Spec Audit](17-spec-audit.md) | Any | High→Low | Audit and harden spec sections to lab-grade |
| 18 | [Verification Audit](18-verification-audit.md) | Any | High→Low | Audit Lean proofs, Kani harnesses, Stateright models for drift |
| 19 | [Test Suite Audit](19-test-suite-audit.md) | Any | High→Low | Audit tests for false confidence, weak assertions, coverage gaps |

## Usage Pattern

```
New phase → 16-spec-authoring.md (formalize) → 17-spec-audit.md (verify)
  ↓
Session start → 01-session-init.md
  ↓
Phase work → 02/03/04/05 (depending on current phase)
  ↓
Review → 06-cleanroom-review.md
  ↓
Issues found? → 07-bug-triage.md + 08-task-creation.md
  ↓
Post-swarm? → 13-progress-review.md (assess) → 14-bead-audit.md (harden)
  ↓
Verification drift? → 18-verification-audit.md (Lean/Kani/Stateright)
  ↓
Test confidence? → 19-test-suite-audit.md (assertions/coverage/fuzz/catalog)
  ↓
New prompt needed? → 15-prompt-forge.md
  ↓
Session end → 09-continuation.md
```

## Defensive Engineering Standards

All prompts that produce code reference **GOALS.md §6** — the canonical defensive
engineering standard. Key requirements embedded across prompts:

| Standard | Where Enforced | Prompts |
|----------|---------------|---------|
| 11 CI gates (GOALS.md §6.8) | Quality Gates sections | 04, 05, 07, 09, 13 |
| MIRI UB detection | Dynamic analysis | 03, 05, 06, 08, 11, 13, 14 |
| Fuzz testing (5 targets) | Deserialization/WAL/checkpoint paths | 03, 06, 07, 08, 11, 14 |
| Mutation testing (>80% kill rate) | Test strength verification | 03, 06, 08, 13, 14 |
| Coverage thresholds (90%/80%) | Coverage ratchet | 03, 06, 13 |
| Unsafe containment (§6.2) | Hard constraints, review phases | 05, 06, 09, 13, 14 |
| Supply chain (cargo-deny) | CI gates | 05, 06, 07, 09, 13 |
| Regression discipline (§6.9) | Bug triage, review | 06, 07 |
| Verification tags (V:MIRI/FUZZ/MUTANT/FAULT) | Bead and spec authoring | 08, 14, 16, 17 |

When in doubt, GOALS.md §6 is the source of truth. These prompts are the
operational application of that standard.

## Skill Loading

Each prompt specifies which `ms` skills to load. The general pattern:
- **Discovery/analysis**: `ms load spec-first-design -m --full`
- **Implementation**: `ms load rust-formal-engineering -m --full`
- **Prompt creation**: `ms load prompt-optimization -m --pack 2000`
- **Never stack** multiple full skills simultaneously (k* budget)
