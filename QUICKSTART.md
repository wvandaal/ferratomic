# Ferratomic — Quick Orientation

**What**: Formally verified, distributed embedded datom database engine.
**Core property**: `Store = (P(D), U)` — G-Set CRDT semilattice. Writes never conflict.
**Spec**: `spec/` — see `spec/README.md` for current invariant/ADR/NEG counts.

## Current Phase

Phases 0-3 COMPLETE. **Phase 4a (core implementation) — gate closure in progress.**
bd-add has 35 dependencies, 2 closed, 33 remaining. Key categories:
- **bd-4i6u** (perf EPIC, CLOSED) — 20/20 beads, 72 audit defects fixed
- **bd-cly9** (decomposition, CLOSED) — 11 crates
- **bd-7fub** (Path to 10.0 EPIC, OPEN) — 11 tier EPICs + ~120 children
- **bd-7fub.22.10** (re-review, IN_PROGRESS) — cleanroom re-review + 10.0/A+
- **bd-y1w5** (tag v0.4.0-gate, OPEN) — tag + gate closure document
- **28 standalone tasks** (OPEN) — testing, docs, bugs, code quality
Fastest path: close 28 standalones → quality EPICs → re-review → tag → gate.
Run `br show bd-add` for full list. See `docs/prompts/session-017-continuation.md`.

| Phase | Status |
|-------|--------|
| 0: Specification | DONE |
| 1: Lean proofs (0 sorry) | DONE |
| 2: Tests (red phase) | DONE |
| 3: Type definitions | DONE |
| **4: Implementation** | **IN PROGRESS** |
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
