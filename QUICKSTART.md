# Ferratomic — Quick Orientation

**What**: Formally verified, distributed embedded datom database engine.
**Core property**: `Store = (P(D), U)` — G-Set CRDT semilattice. Writes never conflict.
**Spec**: `spec/` — see `spec/README.md` for current invariant/ADR/NEG counts.

## Current Phase

Phases 0-3 COMPLETE. **Phase 4a CLOSED 2026-04-08 at composite 9.55-9.57 / A+** within Phase 4a's closed scope. Tag `v0.4.0-gate` at commit `732c3aa`. Closure document at `docs/reviews/2026-04-08-phase-4a-gate-closure.md`. Two independent verification layers: lifecycle/13 deep-mode review (prior session) + bd-snnh empirical 100M validation (today). The 0.43 gap from literal 10.0 is structurally Phase 4b/4c implementation work, not Phase 4a defects.

**Phase 4a.5 + Phase 4b begin in parallel (diamond topology).** Bead audit COMPLETE 2026-04-08 (session 021): all 112 beads (27 4a.5 + 85 4b) audited at lab-grade depth. 170 findings, 8 cross-phase patterns. Roadmap codified in `docs/reviews/2026-04-08-phase-4a5-4b-audit.md` §16-20. **Sessions 022 + 022.5 (2026-04-08/09)**: 7 hidden Phase 4b orphans audited (Pattern I 9th NEW pattern); spec/05 §23.8.5 spec audit complete (26 findings 200-225); Pattern F federation-side renumber EXECUTED (spec/05 ADR-031/032/033 → 034/035/036); spec/09 perf ADRs preserved; all 5 §23.8.5 CRITICAL findings resolved. **Session 023 (2026-04-09)**: **Pattern H DEFINITIVELY RESOLVED at the spec layer** — §23.9.0 "Canonical Datom Key Encoding" (267 lines, 7 sub-sections, 5-tree architecture + RootSet manifest model) and INV-FERR-045a "Deterministic Chunk Serialization" (637 lines, full 6-layer Stage 1 invariant) AUTHORED in spec/06. Spec audit Section 7 Phases 1-5 complete: 7 findings (1 CRITICAL, 3 MAJOR, 3 MINOR); FINDING-226 (INV-FERR-049 multi-tree manifest L2 rewrite) + FINDING-227/228 (Lean tautology fixes for INV-FERR-045/046) executed inline; 5 follow-up beads filed (bd-aqg9h, bd-uhjj3, bd-dhv31, bd-4o8uv, bd-e2gu3). spec/06: 2043 → 3295 lines (+61%). **INV count: 86 → 87 (incl. 045a)**. All 24 cross-references resolve. All 8 Pattern H victim beads now have valid spec citations. **Next phase**: session 024 = spec audit Section 8 (spec/09 perf architecture, INV-FERR-070..085) + Pattern F perf-side cross-reference disambiguation. Then session 025 Phase 3 reconciliation, session 027+ implementation in parallel diamond. See roadmap memory `~/.claude/projects/-data-projects-ddis-ferratomic/memory/roadmap_audit_to_implementation.md` and session 024 handoff at `docs/prompts/sessions/2026-04-09-session-023-continuation.md`.

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
