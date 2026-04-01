# Ferratomic Execution Lifecycle Prompts

Optimized prompts for driving every phase of ferratomic development with AI agents.

## Prompt Index

| # | Prompt | Phase | DoF | When to use |
|---|--------|-------|-----|-------------|
| 01 | [Session Init](01-session-init.md) | Any | High | Starting a new session (cold start) |
| 02 | [Lean Proofs](02-lean-proofs.md) | 1 | Mixed | Writing Lean 4 theorems for INV-FERR |
| 03 | [Test Suite](03-test-suite.md) | 2 | Low | Writing tests before implementation (red phase) |
| 04 | [Type Definition](04-type-definition.md) | 3 | Low | Implementing ferratom crate types |
| 05 | [Implementation](05-implementation.md) | 4 | Low | Implementing ferratomic-core modules |
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
New prompt needed? → 15-prompt-forge.md
  ↓
Session end → 09-continuation.md
```

## Skill Loading

Each prompt specifies which `ms` skills to load. The general pattern:
- **Discovery/analysis**: `ms load spec-first-design -m --full`
- **Implementation**: `ms load rust-formal-engineering -m --full`
- **Prompt creation**: `ms load prompt-optimization -m --pack 2000`
- **Never stack** multiple full skills simultaneously (k* budget)
