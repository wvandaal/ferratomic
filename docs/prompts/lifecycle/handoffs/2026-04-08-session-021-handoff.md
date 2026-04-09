# Ferratomic Continuation — Session 022

> Generated: 2026-04-08 (end of session 021)
> Last commit: `0feafe6` "docs: session 021 closeout — QUICKSTART roadmap pointer + concurrent doc updates"
> Branch: main (synced with master)
> Phase 4a: CLOSED at A+ 9.57 (`v0.4.0-gate` at commit `732c3aa`)

## Read First (in this exact order)

1. **`QUICKSTART.md`** — project orientation (already updated to point at the roadmap)
2. **`AGENTS.md`** — guidelines, hard constraints (C1-C8), CI gates, code discipline
3. **`docs/reviews/2026-04-08-phase-4a5-4b-audit.md`** §16-20 — **YOUR MARCHING ORDERS**:
   - §16: True North Roadmap (the 6-session execution plan from now to implementation)
   - §17: Bead topology + orphan taxonomy (7 hidden Phase 4b orphans you must add to scope)
   - §18.1: Pattern H DEFINITIVELY RESOLVED — author missing INV-FERR-045a + §23.9.0 in session 023
   - §18.2: Pattern F renumbering proposal (locked: spec/05 ADRs 031/032/033 → 034/035/036)
   - §19: Phase 3 batch script (ready to execute in session 025)
   - §20: Session 021 closeout summary
4. **`~/.claude/projects/-data-projects-ddis-ferratomic/memory/roadmap_audit_to_implementation.md`** — quick-reference index for the roadmap
5. **`docs/prompts/lifecycle/17-spec-audit.md`** — the spec audit protocol you'll execute
6. **`spec/01-core-invariants.md`** INV-FERR-001 — gold-standard calibration before auditing

## Session 021 Summary (what just happened)

### Completed (5 strategic moves + full bead audit)

**Audit work** (5 commits, 112 beads):
- 4a.5 cluster: 27/27 beads ✅
- 4b P0 cluster: 20/20 ✅
- 4b P1 cluster: 34/34 ✅
- 4b P2 cluster: 21/21 ✅
- 4b P3 cluster: 9/9 ✅
- 4b P4 cluster: 1/1 ✅
- **TOTAL: 112/112 beads at lab-grade depth, 170 findings, 8 cross-phase patterns**

**Strategic closeout** (3 commits):
- §16 True North Roadmap codified (6-session execution plan)
- §17 Bead topology mapped (7 hidden Phase 4b orphans found, Pattern D expanded to 3 hits)
- §18 Pattern H DEFINITIVELY RESOLVED + Pattern F renumbering proposal locked
- §19 Phase 3 batch script pre-computed (24 phantom edge removals + 14 dep edge additions + 6 orphan remediations + REWRITE list)
- §20 Closeout summary
- Memory file `roadmap_audit_to_implementation.md` created
- QUICKSTART.md updated to reflect new state

### Decisions Made (with explicit user authorization)

1. **6-session roadmap codified** (sessions 022-027+, see §16 + roadmap memory)
2. **Option Y for Pattern G** (REWRITE all 9 gvil sub-beads to lab-grade in session 025)
3. **Pattern H resolution: AUTHOR missing content** in session 023 (NOT bead-side rewrites)
4. **Pattern F renumbering**: spec/05 ADRs 031/032/033 → 034/035/036; ADR-FERR-020 keep spec/09:48, replace spec/04:507 with cross-reference
5. **bd-4vwk gets new ADR-FERR-037** in spec/09 (NOT amend existing 031)
6. **Audit scope expansion**: 7 hidden Phase 4b orphans added to scope (audit total 112 → 119)

### Bugs Found (cross-phase patterns)

| Pattern | Hits | Severity | Status |
|---------|------|----------|--------|
| A: bd-add (and bd-snnh) phantom edges | 24 | MINOR per-instance | Phase 3 batch script ready (§19) |
| B: stale paths from pre-decomp | 15+ | MAJOR | bd-0n9k owns; needs P1 elevation (§19) |
| C: internal numbering not bead-precise | pervasive | MINOR | Phase 3 cleanup |
| D: mismatched INV-029/032 + bd-l64y semantic ambiguity | 3 | MAJOR | Spec audit Section 6 |
| E: empty bodies (bdvf.13, bd-pdns, bd-a7i0, bd-q188) | 4 worst form | MAJOR | REWRITE in session 025 |
| F: ADR collision **QUADRUPLE** (FERR-020/031/032/033) | 4 | **CRITICAL** | Renumbering proposal locked (§18.2) |
| G: gvil family minimal-body skeleton (gvil.1-9) | 9 | MAJOR | REWRITE Option Y in session 025 |
| **H: fabricated INV-FERR-045a / S23.9.0** | 8 beads | **CRITICAL** | **DEFINITIVELY RESOLVED** — author missing content in session 023 |

### Stopping Point

**Strategic discovery is complete**. Session 021 ended at the point where the audit findings have been transformed into an executable multi-session plan. **No further strategic work is needed** — sessions 022-027+ execute mechanically against the codified roadmap.

The last action was committing `0feafe6` (QUICKSTART.md update + concurrent doc updates from another agent). Tree is clean. Working directory verified clean post-push.

## Next Execution Scope

### Primary Task

**Begin session 022 per the True North Roadmap (§16.3 row 2)**:

> **Session 022**: Spec audit Section 6 — `spec/05 §23.8.5` (lifecycle/17 deep mode) + ADD 7 hidden Phase 4b orphans to audit scope.

**Two phases of work in session 022**:

**Phase 022a** — Audit the 7 hidden Phase 4b orphans (~1-2 hours, do FIRST):

The original audit was scoped by `phase-4b` label. These 7 beads are clearly Phase 4b work but lack the label, so they were missed. They need lab-grade audits per `lifecycle/14` BEFORE the spec audit begins, because bd-l64y in particular identifies a real INV-FERR-029 ambiguity that affects spec/03 — and the spec audit needs to know about it.

| Order | Bead | Why hidden orphan |
|-------|------|-------------------|
| 1 | **bd-gvil** (CRITICAL) | Wavelet matrix MASTER EPIC, no phase label, stale title "(Phase 4c+)" — needs immediate relabel + title update per bd-4vwk reframe |
| 2 | bd-uyy9 | INV-FERR-003 self-merge fast path; Session 015 cleanroom finding |
| 3 | bd-fcta | WireEntityId trust boundary bug; Session 015 cleanroom finding |
| 4 | **bd-l64y** | merge_causal homomorphism INV-FERR-029 ambiguity — **3rd Pattern D hit**, informs spec audit |
| 5 | bd-wwia | Kani --unwind 8; minimal body (Pattern E candidate) |
| 6 | bd-8rvz | Two functions exceed 50 LOC limit; Session 015 cleanroom finding |
| 7 | bd-0wn5 | INV-FERR-057 soak test framework; Phase 4b verification work |

For each, run the standard `lifecycle/14` Phase 1 (4 checks) + Phase 2 (8 lenses) and append findings to audit doc Section 5 (under a new sub-section §5.6 "Hidden Phase 4b orphans (audited session 022)"). NO subagent delegation. Sequential. Read primary sources.

**Phase 022b** — Spec audit Section 6 (`spec/05 §23.8.5`) per `lifecycle/17` deep mode (~3-5 hours):

Scope (from session 020 mandate):
- INV-FERR-060 (Store Identity Persistence) — `spec/05:5491`
- INV-FERR-061 (Causal Predecessor Completeness) — `spec/05:5736`
- INV-FERR-062 (Merge Receipt Completeness) — `spec/05:5952`
- INV-FERR-063 (Provenance Lattice Total Order) — `spec/05:6143`
- INV-FERR-025b (Universal Index Algebra) — `spec/05:6343`
- INV-FERR-086 (Canonical Datom Format Determinism) — `spec/05:6722`
- ADR-FERR-021 (Signature Storage as Datoms) — `spec/05:4989`
- ADR-FERR-022 (Phase 4a.5 DatomFilter Scope) — `spec/05:5030`
- ADR-FERR-023 (Per-Transaction Signing) — `spec/05:5068`
- ADR-FERR-024 (Async Transport via std::future) — `spec/05:5108`
- ADR-FERR-025 (Transaction-Level Federation) — `spec/05:5144`
- ADR-FERR-026 (Causal Predecessors as Datoms) — `spec/05:5184`
- ADR-FERR-027 (Store Identity via Self-Signed Transaction) — `spec/05:5223`
- ADR-FERR-028 (ProvenanceType Lattice) — `spec/05:5265`
- **ADR-FERR-031 (Database-Layer Signing)** — `spec/05:5341` — **Pattern F resolution: renumber to ADR-FERR-034**
- **ADR-FERR-032 (TxId-Based Transaction Entity)** — `spec/05:5390` — **Pattern F resolution: renumber to ADR-FERR-035**
- **ADR-FERR-033 (Store Fingerprint in Signing Message)** — `spec/05:5437` — **Pattern F resolution: renumber to ADR-FERR-036**

Cross-reference resolution required:
- **Pattern D**: lookup spec/03 INV-FERR-029 (LIVE View Resolution) and INV-FERR-032 (LIVE Resolution Correctness) to determine canonical titles. Update bd-u5vi, bd-u2tx, and bd-l64y citations.
- INV-FERR-08x placeholder resolution: every "INV-FERR-08x" reference in the audited beads needs to be assigned a real number.

Apply 7 lenses per invariant per `lifecycle/17` Phase 3:
1. Algebraic Soundness
2. Level 0 ↔ Level 2 Consistency
3. Falsification Adequacy
4. proptest ↔ Falsification Correspondence
5. Lean ↔ Level 0 Correspondence
6. Stage ↔ Completeness Consistency
7. Internal Contradiction

**Acceptance for session 022**:
- 7 hidden orphans audited; findings appended to audit doc §5.6
- spec/05 §23.8.5 invariants + ADRs all audited per lifecycle/17 Phases 1-3
- Pattern F federation-side renumber executed in spec/05 (3 ADR renames + cross-reference updates in 4 cited beads)
- Pattern D INV-029/032 mismatch resolved in spec/03
- Findings appended to audit doc §6 + remediation log §11
- Commit + push

### Ready Queue

```bash
br ready          # 20 ready beads (P1 cluster)
bv --robot-next   # Top pick: bd-bdvf "Amend federation spec for Phase 4a.5 scope"
                  #   (PageRank 31%, blocks 4 — but this is the bead Section 6 of the spec audit
                  #    will resolve via the §23.8.5 audit pass)
```

**Note**: Do NOT claim a bead via `br update <id> --status in_progress`. Session 022 is a SPEC AUDIT session, not a bead implementation session. The audit doc IS your work tracker. Use beads only as reference data.

### Dependency Context

Sessions 022-024 are spec audits — they modify spec/* files, NOT bead state. Session 025 is when bead state changes happen (Phase 3 reconciliation per §19 batch script). Until then, the audit doc §10 findings register is the authoritative log.

The 6-session sequence is **strictly ordered** — you cannot reorder it without breaking dependencies:
- Session 022 depends on session 021's Pattern F resolution proposal (§18.2)
- Session 023 depends on session 022's Pattern D resolution (which informs INV-FERR-045a authoring)
- Session 024 depends on sessions 022+023 (Pattern F perf side resolution depends on the federation side being settled)
- Session 025 depends on all 3 spec audits (Phase 3 reconciles findings from all of them)
- Session 026 depends on session 025 (verification needs reconciliation done)

## Hard Constraints

- Safe callable surface: `#![forbid(unsafe_code)]` by default; internal unsafe permitted only when firewalled behind safe APIs, mission-critical, and ADR-documented (GOALS.md §6.2)
- No `unwrap()` in production code (NEG-FERR-001)
- `CARGO_TARGET_DIR=/data/cargo-target` (MUST set; default uses /tmp which fills up)
- Phase N+1 cannot start until Phase N passes isomorphism check
- Full defensive engineering standards: GOALS.md §6
- **NO subagent delegation for spec/bead auditing** (per `feedback_no_batch_bead_audit.md`)
- **NO batch updates** — sequential, one INV/bead at a time
- **NO rush** — multi-session pacing is intentional; correctness above throughput
- **Read primary sources every time** — every spec citation grep-verified, every file path ls-verified
- **Use bd-qguw / bd-hcns / bd-ena7 / bd-imwb as exemplar quality bars**
- **NO worktree isolation** for any subagent (corrupts .beads/ — `feedback_no_worktrees.md`)

## Stop Conditions

Stop and escalate to the user if:

- **You discover a NEW pattern** beyond A-H. The 8 patterns in §17/§18 cover everything found so far. A 9th pattern means the audit missed something structurally significant — flag for strategic discussion.
- **A spec audit finding REVERSES a session 021 decision**. The roadmap and resolution proposals were locked with explicit user authorization; reversing them needs a new authorization.
- **The spec content needed for Pattern H authoring** turns out to be unrecoverable from the 7 Bug Analysis sections (i.e., the bead descriptions are insufficient). This would mean session 023 needs design work, not just synthesis.
- **A bead currently in the audit reveals it should NOT be in Phase 4a.5 or 4b**. Scope creep or shrinkage requires user approval.
- **Any cargo gate fails** during the audit work (shouldn't happen since session 022 only edits spec/* and audit doc; no Rust changes).
- **You need to stash, revert, or destructively touch another agent's work**. Per CLAUDE.md, never disturb concurrent agents.
- **The `br ready` queue surfaces a bead in unexpected priority order**. The queue should be deterministic given the audit's known state.

## What NOT To Do

- Do NOT start Phase 3 reconciliation in session 022. That's session 025's job. The §19 batch script is pre-computed but waiting for spec audits to complete first.
- Do NOT skip the 7 hidden Phase 4b orphans. They are CRITICAL audit-scope corrections, not optional.
- Do NOT redo the bead audit. It's complete (112/112 beads). Just reference findings from §4 and §5.
- Do NOT modify the audit doc §16-20 sections. They are codified locked decisions. New findings go in §10 (findings register), §11 (remediation log), or new sub-sections under §6/§7/§8.
- Do NOT load multiple ms skills simultaneously. For session 022 spec audit, load `spec-first-design --pack 2000` (one skill).
- Do NOT batch-process invariants with subagents. Sequential, by orchestrator, primary-source-verified.
- Do NOT assume the spec content matches the bead descriptions. The bead audit found 8 fabricated citations (Pattern H) — the spec is the source of truth, not the beads.

## Session 022 success criterion

When session 022 ends:
- ✅ 7 hidden Phase 4b orphans audited and findings appended to audit doc §5.6
- ✅ spec/05 §23.8.5 fully audited per lifecycle/17 Phases 1-5
- ✅ Pattern F federation-side renumber executed in spec/05 (3 ADR renames + 4 bead citation updates)
- ✅ Pattern D INV-029/032 mismatch resolved (spec/03 read + bd-u5vi/u2tx/l64y citations updated)
- ✅ All session 022 findings recorded in audit doc §6 + §10 + §11
- ✅ spec/README.md ADR count updated if Pattern F renumber adds new ADRs
- ✅ Git committed and pushed (main + master)
- ✅ Session 023 handoff written with pickup point at "Spec audit Section 7 (spec/06 prolly tree) — author INV-FERR-045a + §23.9.0 from the 7 Bug Analysis sections"

## Final note from session 021

This session represents the strategic boundary of the audit work. **All future sessions are mechanical execution against the codified plan**. The compounding return on the strategic investment is now in motion: every defect caught at the spec/bead layer (the cheapest layer per §16.2) saves 10-1000× downstream. The user's mandate "iterate in planning space, not implementation space" has been honored to the maximum extent possible.

When session 027+ begins implementation, both Phase 4a.5 and Phase 4b will start from a provably clean work surface — no hidden defects, no fabricated citations, no stale paths, no empty bodies. The bead graph will be valid. The spec will be canonical. The implementation will be lab-grade by default.

The hardest thinking is done. What remains is the discipline to execute carefully.
