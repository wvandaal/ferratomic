# Ferratomic — Quick Orientation

**What**: Formally verified, distributed embedded datom database engine.
**Core property**: `Store = (P(D), U)` — G-Set CRDT semilattice. Writes never conflict.
**Spec**: `spec/` — see `spec/README.md` for current invariant/ADR/NEG counts.

## Current Phase

Phases 0-3 COMPLETE. **Phase 4a CLOSED 2026-04-08 at composite 9.55-9.57 / A+** within Phase 4a's closed scope. Tag `v0.4.0-gate` at commit `732c3aa`. Closure document at `docs/reviews/2026-04-08-phase-4a-gate-closure.md`. Two independent verification layers: lifecycle/13 deep-mode review (prior session) + bd-snnh empirical 100M validation (today). The 0.43 gap from literal 10.0 is structurally Phase 4b/4c implementation work, not Phase 4a defects.

**Phase 4a.5 + Phase 4b begin in parallel (diamond topology).** Bead audit COMPLETE 2026-04-08 (session 021): all 112 beads (27 4a.5 + 85 4b) audited at lab-grade depth. 170 findings, 8 cross-phase patterns. Pattern H DEFINITIVELY RESOLVED via cass + git log + grep triple-confirmation. Roadmap codified in `docs/reviews/2026-04-08-phase-4a5-4b-audit.md` §16-20. **Next phase**: spec audits (sessions 022-024) per `lifecycle/17`, then Phase 3 reconciliation (session 025), then implementation in parallel diamond (session 027+). See roadmap memory `~/.claude/projects/-data-projects-ddis-ferratomic/memory/roadmap_audit_to_implementation.md`.

| Phase | Status |
|-------|--------|
| 0: Specification | DONE |
| 1: Lean proofs (0 sorry) | DONE |
| 2: Tests (red phase) | DONE |
| 3: Type definitions | DONE |
| **4a: Core implementation** | **CLOSED at A+ 2026-04-08 (v0.4.0-gate)** |
| **4a.5: Federation foundations** | **NEXT (diamond track 1)** |
| **4b: Performance + canonical spec form** | **NEXT (diamond track 2)** |
| 4c: Federation/transport | — |
| 4d: Datalog query engine | — |
| 5: Integration | — |

## Where to Start

1. Read `AGENTS.md` — build commands, hard constraints, quality gates, crate map
2. Read `GOALS.md` — value hierarchy, success criteria, defensive engineering standards (§6)
3. Read `spec/README.md` — spec module index (load only what you need)
4. Check project state:

```bash
export CARGO_TARGET_DIR=/data/cargo-target  # CRITICAL — omitting fills /tmp
br ready          # Actionable tasks (no blockers)
bv --robot-next   # Top-priority pick with claim command
```

## Key Documents

| Document | What It Contains |
|----------|-----------------|
| `AGENTS.md` | Build commands, hard constraints, CI gates, code discipline, agentic rules |
| `GOALS.md` | Purpose, value hierarchy, success criteria, defensive engineering standards (§6) |
| `spec/README.md` | Spec module index (canonical invariant/ADR/NEG counts) |
| `docs/prompts/lifecycle/` | One prompt per cognitive phase (17 prompts) |
| `docs/design/` | Migration path, architectural influences, refinement chains |
