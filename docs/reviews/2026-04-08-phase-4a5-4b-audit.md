# Phase 4a.5 + Phase 4b Audit (Beads + Spec)

> **Started**: 2026-04-08
> **Status**: IN PROGRESS — multi-session audit
> **Mandate**: Session 020 handoff — full lab-grade audits of the entire Phase 4a.5
> and Phase 4b work surface BEFORE any Phase 4b implementation begins.
> **Protocol**: `docs/prompts/lifecycle/14-bead-audit.md` (beads) +
> `docs/prompts/lifecycle/17-spec-audit.md` (spec).
> **Discipline**: NO subagent delegation. NO batch updates. NO rush. Sequential,
> orchestrator-only, lab-grade per item. Quality over throughput.

---

## 1. Scope

| Track | Source | Item count |
|-------|--------|-----------|
| Bead audit — Phase 4a.5 | `br list --status=open --label=phase-4a5` | 27 (1 EPIC + 26 children) |
| Bead audit — Phase 4b   | `br list --status=open --label=phase-4b`  | 85 |
| Spec audit — `spec/05 §23.8.5` | Phase 4a.5 federation invariants | INV-FERR-060..063, 025b, 086, ADR-FERR-021..029, 031, 032, 033 |
| Spec audit — `spec/06`         | Prolly tree | INV-FERR-045..050 + ADRs/NEGs |
| Spec audit — `spec/09`         | Performance architecture | INV-FERR-070..085, NEG-FERR-007, ADR-FERR-030..033 |

Note: Phase 4a.5 / Phase 4b labels overlap on a few cross-phase beads. The audit
processes each bead once; cross-listing is recorded in the per-bead finding.

---

## 2. BEFORE metrics (baseline 2026-04-08T22:30Z)

| Metric | Value | Source |
|--------|-------|--------|
| Total beads | 998 | `bv --robot-triage` |
| Open beads | 179 | `bv --robot-triage` |
| Phase-4a5 open | 27 | `br list --label=phase-4a5` |
| Phase-4b open | 85 | `br list --label=phase-4b --limit 0` |
| In progress | 2 | `bv --robot-triage` |
| Cycles | 0 | `bv --robot-insights` |
| Articulation points | present (bd-m8ym noted) | `bv --robot-insights` |
| Alerts | 3 (1 warning: bd-85j stale 10d; 2 info: bd-bdvf, bd-obo8 cascades) | `bv --robot-alerts` |
| Priority misalignments | 10 (mostly bv suggesting decrease) | `bv --robot-priority` |
| Suggestions | bd-7fub.* duplicate noise (most already closed) | `bv --robot-suggest` |

`spec/README.md` canonical counts: **86 invariants** (incl. 025b, 086) · **32 ADRs**
· **7 negative cases** · **2 coupling invariants**.

---

## 3. Calibration

Gold standard re-read prior to audit start:
- `spec/01-core-invariants.md` INV-FERR-001 (Merge Commutativity) — all 6
  layers populated, Lean proof complete (`Finset.union_comm`), Kani harness
  bounded, proptest with 0..100 generator, falsification specific.

Methodology skill loaded (one only): `spec-first-design --pack 2000` (overview,
429 tokens) with `prompt-optimization` dependency. Cognitive mode: adversarial
verification, then surgical editing.

---

## 4. Bead Audit — Phase 4a.5 (Track 1)

**Order**: P0 → P1 → P2 → P3, then EPIC. Sequential, one bead at a time.
Per-bead protocol: Phase 1 verification (4 checks) → Phase 2 quality
assessment (8 lenses) → verdict.

### 4.1 P0 beads (1)

#### bd-r7ht — Bootstrap test: store Phase 4a.5 spec as signed datoms

**Phase 1 verification**

| Check | Result |
|-------|--------|
| Code | CONFIRMED — `ferratomic-verify/src/lib.rs` exists; `bootstrap_test.rs` correctly absent (NEW file slot valid in current `src/` layout: `bench_helpers.rs`, `confidence.rs`, `fault_injection.rs`, `generators.rs`, `invariant_catalog.rs`, `isomorphism.rs`, `lib.rs`, `bin/`). |
| Spec | CONFIRMED — all 8 cited INV-FERR resolve: 031 (`spec/03:811` Genesis Determinism), 039 (`spec/05:546` Selective Merge), 051 (`spec/05:2355` Signed Transactions), 060 (`spec/05:5491` Store Identity Persistence), 061 (`spec/05:5736` Causal Predecessor Completeness), 062 (`spec/05:5952` Merge Receipt Completeness), 063 (`spec/05:6143` Provenance Lattice Total Order), 025b (`spec/05:6343` Universal Index Algebra). Both ADRs resolve: 013 (`spec/08:1199` Machine-Readable Invariant Catalog), 028 (`spec/05:5265` ProvenanceType Lattice). GOALS.md L2 success criterion at line 213 ("The bootstrap test: Ferratomic's own specification stored as datoms within itself"). |
| Dependencies | DISCREPANCY — graph has 8 hard `blocked-by` edges (bd-bdvf, bd-hlxr, bd-mklv, bd-lifv, bd-sup6, bd-7dkk, bd-3t63, bd-h51f) + 1 EPIC parent (bd-oiqr). Bead prose `## Dependencies` enumerates only `bd-hlxr` by ID; the other 7 are covered by an umbrella phrase ("all Phase 4a.5 types, signing, transport, selective merge, identity") but not bead-precise. Graph itself is correct; the prose is incomplete. |
| Duplicates | UNIQUE — bd-5bvd (federation bootstrap research) and bd-4pna (schema bootstrap ordering) differ in scope. |

**Phase 2 lenses (8)**

| Lens | Result | Notes |
|------|--------|-------|
| L0 Epistemic Fit | PASS | Integration test bead; correct V:TYPE + runtime composition method. No Lean/Kani prescribed (correct — composition test is not a Finset-expressible algebraic property). |
| L1 Structural | PASS-minor | All 13 template fields present. Dependencies prose underspecified (1 of 8 deps). |
| L2 Traceability | PASS | Chain `Bead → 8 INVs + 2 ADRs → spec/05, /03, /08 → algebraic laws` resolves in <3 hops for every reference. |
| L3 Postcondition Strength | PASS-minor | 8/10 postconditions are strong (binary, INV-traced, mechanical verify). #8 (`:verification/*` schema attrs) has no INV trace. #9 ("Gate closure expressible as predicate") doesn't specify the predicate body — agent must invent it. |
| L4 Scope Atomicity | PASS | 2 files, 1 test fn, ≤1 session of focused work after the 8 deps land. |
| L5 Frame Adequacy | PASS-minor | Frame condition #1 references `B01-B16` (internal Phase 4a.5 bead numbering scheme) instead of br IDs. An agent reading the bead in isolation cannot resolve B01..B16 → (bd-…) without an external translation table. |
| L6 Compiler Test | PASS | Correctly N/A — bead consumes existing types, introduces none. |
| L7 Axiological | PASS | Directly realizes GOALS.md L2 success criterion #6. Keystone composition test for the entire Phase 4a.5 scope. |

**Verdict**: SOUND with 4 MINOR findings → action **EDIT** (no rewrite).

**Findings raised**:
- [FINDING-001] Dependencies prose enumerates 1 of 8 graph deps.
- [FINDING-002] Postcondition #8 lacks INV-FERR trace.
- [FINDING-003] Postcondition #9 lacks the concrete predicate body.
- [FINDING-004] Frame condition #1 references internal `B01-B16` numbering instead of br IDs.

### 4.2 P1 beads (9)

_Order_: bd-oiqr (EPIC, gives context) → bd-bdvf (highest centrality, blocks 4) → bd-bdvf.13 (audit gate child) → bd-r3um (gate) → bd-qguw (canonical datom format) → bd-k5bv (C8 rename) → bd-4pna (schema bootstrap) → bd-u5vi (LIVE retraction) → bd-0lk8 (Ed25519 fail-fast).

#### bd-oiqr — EPIC: Phase 4a.5 — Federation foundations

**Phase 1 verification**

| Check | Result |
|-------|--------|
| Code | N/A — epic, no files |
| Spec | CONFIRMED — all 8 cited INVs resolve: 038 (`spec/05:358` Substrate Transparency), 039 (`spec/05:546` Selective Merge), 044 (`spec/05:1729` Namespace Isolation; also re-cited at `spec/05:2155`), 051 (`spec/05:2355` Signed Transactions), 060 (`spec/05:5491` Store Identity), 061 (`spec/05:5736` Causal Predecessors), 062 (`spec/05:5952` Merge Receipts), 025b (`spec/05:6343` Universal Index Algebra). |
| Dependencies | PHANTOM — `bd-oiqr` is blocked by `bd-add`, but `bd-add` was CLOSED 2026-04-08 ("PHASE 4A GATE CLOSED — composite 9.55-9.57/A+", commit `732c3aa`, tag `v0.4.0-gate`). The edge is now satisfied; per `lifecycle/14` Phase 3 Step 3, phantom edges to closed satisfied beads are removed during reconciliation. 23 child parent-child edges resolve correctly. |
| Duplicates | UNIQUE — only Phase 4a.5 EPIC. |

**Phase 2 lenses (8)**

| Lens | Result | Notes |
|------|--------|-------|
| L0 Epistemic Fit | PASS | Epics do not prescribe verification methods; method belongs to children. |
| L1 Structural | **NEEDS WORK** | Epic template (`14-bead-audit.md` §Epic-Specific Fields) requires `## Child Beads` enumeration AND `## Progress` (N/M children closed + current bottleneck) in the bead body. bd-oiqr has neither — children are visible only via `br show`'s graph metadata, not in the description body. |
| L2 Traceability | PASS | 8 INVs trace to `spec/05`, all resolve. |
| L3 Postcondition Strength | N/A | Epic — children own postconditions. |
| L4 Scope Atomicity | PASS | Scope = "all federation foundations work" is the entire Phase 4a.5; appropriate epic granularity. |
| L5 Frame Adequacy | N/A | Epic — children own frame conditions. |
| L6 Compiler Test | N/A | Epic, no types. |
| L7 Axiological | PASS | Federation foundations are the gateway to multi-agent cognition; direct True North alignment ("federation-native ... designed from day one for agents spanning heterogeneous compute environments" — GOALS.md §2). |

**Verdict**: NEEDS WORK with 3 MINOR findings → action **EDIT** (add 2 epic template sections + remove 1 phantom edge in Phase 3).

**Findings raised**:
- [FINDING-005] bd-oiqr missing `## Child Beads` enumeration in body.
- [FINDING-006] bd-oiqr missing `## Progress` (N/M closed + bottleneck).
- [FINDING-007] bd-oiqr → bd-add is a PHANTOM dependency edge (bd-add closed 2026-04-08).

#### bd-bdvf — Amend federation spec for Phase 4a.5 scope and invariants

**Phase 1 verification**

| Check | Result |
|-------|--------|
| Code | N/A — docs/spec-authoring bead. |
| Spec | CONFIRMED — `spec/05-federation.md:4951` `## 23.8.5 Phase 4a.5: Federation Foundations` exists. The "lines 4951-end" reference resolves. |
| Dependencies | DISCREPANCY — outgoing graph: bd-oiqr (parent-child) + bd-add (PHANTOM, closed 2026-04-08). 13 child parent-child edges (bdvf.1 through bdvf.13) all valid (12 closed, bdvf.13 open). 10 incoming `blocks` edges (r7ht, qguw, 4pna, u5vi, r3um, u2tx, hcns, 1zxn, tck2, 8f4r). bd-add edge is phantom and must be removed. **Type mismatch**: bd-bdvf has 13 children but is `type=task`; per lab-grade template, beads with children must be `type=epic`. |
| Duplicates | NOT a duplicate of bdvf.13 — parent-child relationship is correct hierarchy, not duplication. The audit work of bd-bdvf is to be performed by bdvf.13; the parent represents the goal, the child represents the action. |

**Phase 2 lenses (8)**

| Lens | Result | Notes |
|------|--------|-------|
| L0 Epistemic Fit | PASS | Five-lens convergence review is the correct method for spec audit (lifecycle/16/17 protocol). |
| L1 Structural | **NEEDS WORK** | Type mismatch: 13 children present, type=task. Should be epic. Body has compact What/Why/Acceptance but lacks epic-template Child Beads list and Progress section. |
| L2 Traceability | PASS | Direct reference to `spec/05 §23.8.5 (lines 4951-end)`. |
| L3 Postcondition Strength | WEAK | Acceptance: "bdvf.13 closed cleanly with audit notes." This is a forward-reference to a child bead whose body is itself empty (see bdvf.13 audit below). No INV trace, no enumerated postconditions, no verification command. The substance has been delegated to a child that doesn't carry it. |
| L4 Scope Atomicity | PASS | Convergence review is an atomic activity once preconditions land. |
| L5 Frame Adequacy | MISSING | No frame conditions stated. For a docs/spec bead this is typically "no code modifications, only spec/README.md and spec/05 edits" — but it must be explicit. |
| L6 Compiler Test | N/A | Docs bead. |
| L7 Axiological | PASS | Five-lens convergence is the cleanroom gate quality check on Phase 4a.5 spec content. |

**Verdict**: NEEDS WORK with 4 findings (1 MAJOR, 3 MINOR) → action **EDIT + RECLASSIFY type** (`task` → `epic`).

**Findings raised**:
- [FINDING-008] bd-bdvf type=task but has 13 children — should be type=epic. MAJOR.
- [FINDING-009] bd-bdvf → bd-add is a PHANTOM edge (closed). MINOR.
- [FINDING-010] bd-bdvf has weak Acceptance — forward-references bdvf.13 which is itself empty. MAJOR (cascades from FINDING-011).
- [FINDING-011] bd-bdvf missing Frame Conditions section. MINOR.

#### bd-bdvf.13 — bdvf.13: Five-lens convergence review (audit gate child)

**Phase 1 verification**

| Check | Result |
|-------|--------|
| Code | N/A — docs/audit bead. |
| Spec | CANNOT VERIFY — bead body is **empty**. Title implies it audits `spec/05 §23.8.5` per the parent (bd-bdvf) context, but the child bead does not state this. |
| Dependencies | bd-bdvf.12 (parent-child sibling, closed) listed as `blocks` (incorrect relation type — siblings should not block each other unless ordering is required). bd-bdvf parent-child link valid. 2 incoming blocks (bd-y1rs, bd-m8ym). |
| Duplicates | The bead's *intent* overlaps with the current audit session's mandate. The current audit is a precursor; bdvf.13 is the *final* convergence pass after remediation lands. Not a duplicate but functionally adjacent. |

**Phase 2 lenses (8)**

| Lens | Result | Notes |
|------|--------|-------|
| L0 Epistemic Fit | UNCLEAR | The five-lens method is correct in principle, but the bead doesn't specify which spec sections to apply it to. |
| L1 Structural | **CRITICAL FAIL** | The bead body is **empty**. Zero of the 13 lab-grade template fields are present: no Specification Reference, no Preconditions, no Postconditions, no Frame Conditions, no Refinement Sketch, no Pseudocode Contract (not even an explicit N/A), no Verification Plan, no Files, no Dependencies prose. The title is the entire content. |
| L2 Traceability | FAIL | No spec reference, no INV trace. |
| L3 Postcondition Strength | FAIL | No postconditions exist. |
| L4 Scope Atomicity | PASS (probably) | Title implies a single convergence review pass. |
| L5 Frame Adequacy | FAIL | No frame conditions. |
| L6 Compiler Test | PASS-trivial | Docs bead, no types. |
| L7 Axiological | PASS | Audit gate quality check serves zero-defect cleanroom standard. |

**Verdict**: **CRITICAL FAIL on L1/L2/L3/L5** → action **REWRITE** (the entire body must be authored from the lab-grade template). This is the most defective bead found so far.

**Findings raised**:
- [FINDING-012] bd-bdvf.13 body is **empty** — only the title carries content. All 13 lab-grade template fields absent. Severity: **MAJOR** (would be CRITICAL if it weren't a docs/audit bead with no algebraic content; the absence is structural, not semantic).
- [FINDING-013] bd-bdvf.13 → bd-bdvf.12 edge has relation type `blocks` between sibling-children of the same parent. The bdvf.12 sibling is closed, so the edge is also PHANTOM. Sibling ordering should be expressed via parent-child, not blocks. MINOR.

#### bd-r3um — Close Phase 4a.5 gate before Phase 4c

**Phase 1 verification**

| Check | Result |
|-------|--------|
| Code | N/A — routing bead, no files (correctly stated). |
| Spec | CONFIRMED — references AGENTS.md "phase gate methodology" (resolvable). No INV-FERR cited (correct for a gate; gates have no algebraic content). |
| Dependencies | DISCREPANCY — graph has 23 outgoing `blocks` edges, including bd-add (**PHANTOM** — closed 2026-04-08). The other 22 are correct child dependencies. Also: 3 incoming `blocks` (bd-fzn Phase 4c gate, bd-q2c9 content-addressed export/import, bd-12d2 TextIndex trait). |
| Duplicates | UNIQUE — only Phase 4a.5 gate. |

**Phase 2 lenses (8)**

| Lens | Result | Notes |
|------|--------|-------|
| L0 Epistemic Fit | PASS | Gate beads use compositional verification: build + test + child closure. Correct method for a gate. |
| L1 Structural | PASS-minor | Uses single `Acceptance:` section as shorthand for combined Postconditions + Verification Plan. Non-template but justifiable for routing beads. References `V01-V03` internal numbering (same pattern as FINDING-004's `B01-B16`). |
| L2 Traceability | PASS | AGENTS.md phase gate methodology + `lifecycle/14`. |
| L3 Postcondition Strength | PASS | 7 acceptance items: 3 mechanical (`cargo check/clippy/test`), 3 graph constraints (children closed), 1 functional (bootstrap test passes). All binary. |
| L4 Scope Atomicity | PASS | Gates are inherently atomic close-criteria. |
| L5 Frame Adequacy | MISSING | No `## Frame Conditions` section. For a routing bead, frame should be: "1. No file modifications. 2. No code touched. 3. Closure is a state change only." |
| L6 Compiler Test | PASS | Routing bead, no types. |
| L7 Axiological | PASS | Phase ordering is the methodology backbone — every Phase N+1 begins only after Phase N's gate closes. |

**Verdict**: SOUND with 4 MINOR findings → action **EDIT**.

**Findings raised**:
- [FINDING-014] bd-r3um uses non-template `Acceptance` shorthand; lab-grade prefers split `## Postconditions` + `## Verification Plan`. MINOR.
- [FINDING-015] bd-r3um references "V01-V03" internal verification bead numbering (same pattern as FINDING-004's `B01-B16`). MINOR.
- [FINDING-016] bd-r3um → bd-add is also PHANTOM (pattern: FINDING-007/009/016). MINOR.
- [FINDING-017] bd-r3um missing `## Frame Conditions` section. MINOR.

#### bd-qguw — Define canonical datom byte format (INV-FERR-086)

**Note**: This bead is the **exemplar** of the 4a.5 audit so far. All 13 lab-grade
template fields are present and substantive. The Pseudocode Contract includes a
fixed-layout encoding table with all 11 Value tags enumerated. Findings below are
minor polish issues only.

**Phase 1 verification**

| Check | Result |
|-------|--------|
| Code | CONFIRMED — `ferratom/src/lib.rs` exists; `canonical.rs` correctly absent (NEW). Current `ferratom/src/`: clock/, datom/, error.rs, lib.rs, schema.rs, traits.rs, wire.rs. New module slot is valid. |
| Spec | CONFIRMED — INV-FERR-086 at `spec/05:6722`, INV-FERR-074 at `spec/09:919`, INV-FERR-012 at `spec/01:1452`, INV-FERR-051 at `spec/05:2355`. C2 and C8 are constraint codes resolvable from `spec/00-preamble.md`. |
| Dependencies | VALID — bd-k5bv (C8 rename), bd-bdvf (spec amendment), bd-oiqr (parent EPIC). Outgoing blocks: bd-r3um, bd-6j0r, bd-3t63 — all valid. **No phantom edge to bd-add** (this bead was authored after the phantom-edge pattern arose; it correctly does not chain to bd-add). |
| Duplicates | UNIQUE — bd-m8ym ("Canonical spec form") and bd-ipzu ("Flywheel demo via canonical spec store") are about spec serialization, not Datom byte serialization. Different concerns. |

**Phase 2 lenses (8)**

| Lens | Result | Notes |
|------|--------|-------|
| L0 Epistemic Fit | PASS | Determinism + injectivity verified by proptest 10K (statistical) + structural assertions. Correct fit — these are concrete-implementation properties, not algebraic identities, so proptest is right (would be wrong to use Lean). |
| L1 Structural | PASS | All 13 lab-grade fields present and substantive. |
| L2 Traceability | PASS | Chain `Bead → INV-FERR-086 → spec/05:6722 → algebraic property` resolves in 2 hops. Supporting INVs (012, 074, 051) all confirmed. |
| L3 Postcondition Strength | PASS-minor | 6/7 postconditions strong. #3 ("Format already in spec/05-federation.md. Verify: grep INV-FERR-086.") is a **precondition** phrased as a postcondition — it asserts a state that already holds before this bead's work begins. |
| L4 Scope Atomicity | PASS | 2 files, 1 concept (canonical bytes), 7 binary postconditions. Atomic. |
| L5 Frame Adequacy | PASS | 3 explicit frame conditions stated. |
| L6 Compiler Test | **PASS — EXEMPLARY** | Sub-checks 6a-6f all pass. The Pseudocode Contract includes the full layout table with 11 Value tag bytes (0x01..0x0B) enumerated, fixed-size byte counts, little-endian convention, and explicit module wiring (`pub mod canonical; pub use canonical::tx_id_canonical_bytes`). An agent can write the `.rs` file with `todo!()` bodies from this contract alone. |
| L7 Axiological | PASS | Canonical byte format is the foundation for content-addressed identity, signing, fingerprinting, and the consensus-free model. Direct True North alignment ("federation-native ... merge by set union" — GOALS.md §2). |

**Verdict**: **SOUND, near-exemplar quality** with 2 MINOR findings → action **EDIT** (light polish).

**Findings raised**:
- [FINDING-018] bd-qguw postcondition #3 is a precondition phrased as a postcondition. MINOR.
- [FINDING-019] bd-qguw references internal `D16/D17/D19/D21` design decision numbering and `B01/B08/B09` bead numbering. Same pattern as FINDING-004/015. MINOR.

#### bd-k5bv — Rename AgentId to NodeId and tx/agent to tx/origin (C8 compliance)

**Phase 1 verification**

| Check | Result |
|-------|--------|
| Code | **STALE FILE PATHS** — `ferratom-clock/src/{txid,lib,frontier}.rs` exist ✓; `ferratom/src/lib.rs` exists ✓; `ferratomic-core/src/{db/mod,db/transact,mmap}.rs` exist ✓. **But**: `ferratomic-core/src/store/{mod,apply,merge,tests}.rs` and `ferratomic-core/src/writer/mod.rs` **DO NOT EXIST** in those locations. Per the 11-crate decomposition (bd-cly9), `store/*` moved to `ferratomic-store/src/{store,apply,merge,tests}.rs` and writer/transaction logic moved to `ferratomic-tx/src/{commit,validate,lib}.rs`. The bead's Files section is partly accurate (clock + db + mmap) and partly stale (store + writer). Code state for the rename itself: ferratom-clock/src/txid.rs still contains 11 occurrences of `AgentId`/`tx/agent`/`genesis_agent` — confirming the rename has NOT been done (bead is OPEN, code matches expected pre-state). |
| Spec | CONFIRMED — INV-FERR-015 at `spec/02:500`, INV-FERR-016 at `spec/02:736`, INV-FERR-051 at `spec/05:2355`. C8 referenced in `spec/00-preamble.md` at lines 19, 67, 116 (substrate independence). The bead's postcondition #12 ("spec/00-preamble.md constraint table gains C8 TEST definition") asks for a *structured* C8 TEST framing to be added — this is forward work, not yet present in the file. PASS (forward-looking postcondition). |
| Dependencies | VALID — bd-oiqr parent-child only. No `blocked-by` (correct: this is a leaf task that must run FIRST). 5 incoming `blocks` edges (bd-qguw, bd-r3um, bd-3t63, bd-1zxn, bd-tck2) — all valid downstream. |
| Duplicates | UNIQUE — only C8 rename bead. |

**Phase 2 lenses (8)**

| Lens | Result | Notes |
|------|--------|-------|
| L0 Epistemic Fit | PASS | Rename verification = grep + build + test. Correct method (V:GREP + V:BUILD + V:TEST). |
| L1 Structural | PASS — substantive | All 13 lab-grade fields present + a 17-row Rename Map table. Comprehensive. |
| L2 Traceability | PASS | C8 → preamble §C8 → GOALS.md §2 (substrate independence). Chain resolves in 2 hops. |
| L3 Postcondition Strength | PASS | 14 binary, grep-verifiable postconditions. Each cites C8 + concrete file/symbol/grep target. |
| L4 Scope Atomicity | PASS | ~20 files but single concept (mechanical rename). Atomic at the concept level. |
| L5 Frame Adequacy | PASS | 4 explicit frame conditions, including the important one ("the `:agent/*` namespace convention is UNCHANGED — that is application-layer, not engine"). |
| L6 Compiler Test | PASS — correctly N/A | "N/A — pure rename, no new types or signatures." Correct. |
| L7 Axiological | PASS — high alignment | C8 substrate independence is a foundational constraint per GOALS.md §2 ("not tied to any runtime or substrate"). The C8 TEST framing the bead introduces ("would every name make sense for a financial ledger or IoT sensors?") is exactly the kind of cross-domain validation the project needs. |

**Verdict**: NEEDS WORK with 1 MAJOR finding (stale file paths) + 1 cross-cutting issue (overlap with bd-0n9k mandate) → action **EDIT** (update Files section).

**Findings raised**:
- [FINDING-020] bd-k5bv `## Files` section has STALE paths from pre-11-crate-decomp era. MAJOR.
- [FINDING-021] bd-k5bv overlaps with bd-0n9k's mandate ("Update Phase 4a.5 bead file paths for 11-crate decomposition"). bd-0n9k should be a precondition of bd-k5bv, OR bd-0n9k's scope should explicitly cover bd-k5bv's Files list. Currently neither is true. MINOR.

#### bd-4pna — Verify schema bootstrap ordering in WAL recovery and checkpoint

**Phase 1 verification**

| Check | Result |
|-------|--------|
| Code | **STALE PATHS (pattern: same as FINDING-020)**. `ferratomic-core/src/db/recover.rs` exists ✓. But: `ferratomic-core/src/writer/commit.rs` (no `writer/` in ferratomic-core; logic moved to `ferratomic-tx/src/{commit,validate}.rs`); `ferratomic-core/src/store/apply.rs` and `ferratomic-core/src/store/tests.rs` (moved to `ferratomic-store/src/{apply,tests}.rs`). 3 of 4 file paths stale. |
| Spec | CONFIRMED — INV-FERR-009 at `spec/01:941` (Schema Validation), INV-FERR-014 at `spec/02:274` (Recovery Correctness). Sources cited (Braid sessions) are external context, not verifiable in this repo. |
| Dependencies | VALID — bd-bdvf, bd-1zxn, bd-oiqr (parent). bd-r3um incoming (gate). |
| Duplicates | UNIQUE in name; conceptually adjacent to bd-u5vi (LIVE retraction) which is also a verification bug-bead in the same area. Not a duplicate. |

**Phase 2 lenses (8)**

| Lens | Result | Notes |
|------|--------|-------|
| L0 Epistemic Fit | PASS | Bug bead, V:TEST. Regression tests + new fix tests are correct method. |
| L1 Structural | PASS | Bug-template fields (Observed/Expected/Root Cause/Fix) all present. The Audit Notes block from Session 013 (which refocused the bead from recovery to commit path) is preserved as continuity context — not in template but useful. |
| L2 Traceability | PASS | INV-FERR-009 + INV-FERR-014 both resolve. |
| L3 Postcondition Strength | PASS | "Expected" has 3 binary items; Verification Plan has 5 specific test names. |
| L4 Scope Atomicity | PASS | 4 files, focused on `validate_datoms()` ordering. |
| L5 Frame Adequacy | PASS | 3 frame conditions, including the critical one ("recovery path must NOT be modified — already correct"). |
| L6 Compiler Test | **PASS-minor** | "N/A — verification/fix bead, type changes depend on which fix option is chosen." Borderline — deferring the contract behind an undecided fix option is exactly what the template tries to prevent. The bead's own analysis concludes "Option A is more principled"; commitment to Option A would unlock a concrete Pseudocode Contract. |
| L7 Axiological | PASS | INV-FERR-009 is a Stage 0 core invariant; the bug is real per the audit notes. |

**Verdict**: NEEDS WORK with 1 MAJOR (stale paths) + 1 MINOR (deferred contract) → action **EDIT**.

**Findings raised**:
- [FINDING-022] bd-4pna has STALE file paths — same pattern as FINDING-020. MAJOR.
- [FINDING-023] bd-4pna defers `## Pseudocode Contract` behind an undecided fix option. The bead's own analysis concludes Option A is preferable; the contract should be written for Option A and the deferral removed. MINOR.

#### bd-u5vi — Verify LIVE view retraction handling and Op ordering invariant

**Phase 1 verification**

| Check | Result |
|-------|--------|
| Code | **STALE PATHS (pattern: FINDING-020)**. `ferratomic-core/src/store/query.rs` and `ferratomic-core/src/store/tests.rs` both moved to `ferratomic-store/src/{query,tests}.rs` (verified: `ferratomic-store/src/query.rs` is 10KB, exists). |
| Spec | **MISMATCHED CITATION (MAJOR)**. The bead cites "INV-FERR-029 (LIVE Resolution Correctness), Level 2" in `spec/03-performance.md`. Actual spec content: INV-FERR-029 at `spec/03:500` is titled **"LIVE View Resolution"**; INV-FERR-032 at `spec/03:937` is titled **"LIVE Resolution Correctness"**. The bead has the title of INV-FERR-032 attached to the number INV-FERR-029. Either: (a) the intended INV is 032 (the number is wrong), or (b) the title is a typo (the title should be "LIVE View Resolution"). Given the bead's *content* (correctness of LIVE retraction handling under Card:Many and Op ordering — these are correctness properties), the intended INV is likely **032**. Supporting INV-FERR-005 at `spec/01:360` (Index Bijection) confirmed; but the prior Session 013 audit note says "ref 005 should be 012" — this annotation is also unresolved and should be revisited. |
| Dependencies | VALID — bd-bdvf, bd-oiqr (parent), bd-r3um (incoming gate). |
| Duplicates | UNIQUE. |

**Phase 2 lenses (8)**

| Lens | Result | Notes |
|------|--------|-------|
| L0 Epistemic Fit | PASS | Verification bead, V:TEST + V:PROP. Bug-template fields used correctly. |
| L1 Structural | PASS | Bug-template fields (Observed, Expected, Root Cause, Fix) all present. Audit Notes block from Session 013 preserved. |
| L2 Traceability | **FAIL** | The primary INV citation is mismatched (number ↔ title). Cannot trust the chain `Bead → INV-FERR-? → spec/03` until the citation is corrected. |
| L3 Postcondition Strength | PASS | 4 binary "Expected" items, 5 specific test names in Verification Plan. |
| L4 Scope Atomicity | PASS | 2 files, focused on LIVE query logic. |
| L5 Frame Adequacy | PASS-minor | 2 frame conditions; could be more explicit about what proptest infrastructure is reused. |
| L6 Compiler Test | PASS | N/A correctly stated. |
| L7 Axiological | PASS | LIVE resolution is core query semantics; correctness here is non-negotiable per INV-FERR-032/029. |

**Verdict**: NEEDS WORK with 1 MAJOR (citation mismatch) + 1 MAJOR (stale paths) + 1 MINOR (unresolved 005-vs-012 audit annotation) → action **EDIT**.

**Findings raised**:
- [FINDING-024] bd-u5vi has STALE file paths — same pattern as FINDING-020. MAJOR.
- [FINDING-025] bd-u5vi cites INV-FERR-029 with the title of INV-FERR-032 ("LIVE Resolution Correctness"). Number-title mismatch. **MAJOR**.
- [FINDING-026] bd-u5vi has an unresolved audit annotation ("ref 005 should be 012") from Session 013 that has not been actioned. Need to determine which INV is the correct supporting reference for the Op ordering claim. MINOR (FLAG for human if cannot resolve from primary sources alone).

#### bd-0lk8 — Fail-fast: Ed25519 verification throughput benchmark

**Phase 1 verification**

| Check | Result |
|-------|--------|
| Code | `experiments/` directory exists (currently contains only `index_scaling/` from bd-snnh). `experiments/ed25519_bench/` is correctly absent (NEW). `docs/research/2026-04-XX-ed25519-throughput.md` is correctly absent (NEW with placeholder date). |
| Spec | `docs/ideas/013-implementation-risk-vectors.md` exists (49KB). spec/05-federation.md exists ✓. ADR-FERR-031 — **EXISTS IN TWO PLACES**: `spec/05:5341` ("Database-Layer Signing", which is the bead's intended reference) AND `spec/09:2838` ("Wavelet Matrix Phase 4a Prerequisites — Rank/Select and Attribute Interning"). This is the duplicate-ADR-number defect that **bd-s56i** flags. The bead's intended reference is `spec/05:5341`, but the citation is ambiguous as written. |
| Dependencies | VALID — "None — can run NOW". bd-7ij (Phase 4b gate) is the incoming `blocks` edge, meaning bd-0lk8 must close before bd-7ij can. Correct: a fail-fast experiment must conclude before the gate it informs. |
| Duplicates | UNIQUE for Ed25519 throughput. |

**Phase 2 lenses (8)**

This bead uses an **experiment template** (Hypothesis / Methodology / Success Criteria / Failure Response / Time Budget / Risks Derisked) rather than the implementation template. Lenses applied accordingly.

| Lens | Result | Notes |
|------|--------|-------|
| L0 Epistemic Fit | PASS | Experiment bead, V:BENCH on real hardware. Correct method for a throughput hypothesis. |
| L1 Structural | PASS — substantive | All experiment-template fields present. Hypothesis is quantified, Success Criteria are measurable, Failure Response defines contingency, Risks Derisked traces to specific doc 013 sections. |
| L2 Traceability | PASS-minor | doc 013 §9.2, spec/05-federation.md, ADR-FERR-031 (Database-Layer Signing). The ADR-FERR-031 citation should disambiguate which of the two duplicate ADR-031s is intended (the §05 one). |
| L3 Postcondition Strength | PASS | 4 quantitative success criteria + 1 deliverable artifact. All measurable. |
| L4 Scope Atomicity | PASS | Single benchmark + report. ~1 day budget stated. |
| L5 Frame Adequacy | PASS | 3 explicit frame conditions including library configuration parity with planned production code. |
| L6 Compiler Test | PASS | N/A — benchmark code, no production type changes. |
| L7 Axiological | PASS | Fail-fast assumption validation BEFORE design commitment is the discipline for high-risk perf claims (per Tier 2 verification depth). |

**Verdict**: SOUND with 3 MINOR findings → action **EDIT** (light polish; no rewrite).

**Findings raised**:
- [FINDING-027] bd-0lk8 carries 3 phase labels (experiment, phase-4a5, phase-4c). The phase-4a5 label is questionable — the experiment informs Phase 4c federation transport design, not Phase 4a.5 implementation. Could be intentional multi-phase relevance, or could be a stale carry-over. MINOR.
- [FINDING-028] bd-0lk8 deliverable filename has unresolved placeholder "2026-04-XX". Should be templated or set on completion. MINOR.
- [FINDING-029] bd-0lk8 cites `ADR-FERR-031` without disambiguating which of the two duplicate ADR-031s is intended. **Cross-references SPEC AUDIT finding (still to come)** — the duplicate ADR number itself is a spec defect bd-s56i tracks. MINOR for this bead, MAJOR/CRITICAL for the spec.

### 4.3 P2 beads (16)

_Beads to audit_: bd-0n9k, bd-u2tx, bd-hlxr, bd-mklv, bd-lifv, bd-1rcm,
bd-sup6, bd-7dkk, bd-6j0r, bd-3t63, bd-h51f, bd-37za, bd-hcns, bd-1zxn,
bd-tck2, bd-8f4r.

_Per-bead findings to be filled in during Phase 1 execution._

### 4.4 P3 beads (1)

_Beads to audit_: bd-s4ne.

---

## 5. Bead Audit — Phase 4b (Track 2)

**Order**: P0 → P1 → P2 → P3 → P4, sequential. The wavelet matrix sub-graph
(bd-obo8 → gvil.1..11) is processed in spec-then-impl order.

### 5.1 P0 beads (~20)

_Beads to audit_: bd-qgxjl, bd-j1mp, bd-y1rs (EPIC), bd-m8ym, bd-51zo, bd-pg85,
bd-jolx, bd-no6b, bd-ena7, bd-o6io, bd-g1nd, bd-chu0, bd-hfzx, bd-q630,
bd-8uck, bd-vhgn, bd-lkdh, bd-obo8, bd-e58u, bd-4vwk.

_Per-bead findings to be filled in during Phase 1 execution._

### 5.2 P1 beads (~30+)

_Beads to audit_: bd-d6dl, bd-p8n3, bd-ipzu, bd-s56i, bd-9be8, bd-fw31,
bd-bmu2, bd-2cud, bd-ei8d, bd-v3gz, bd-9khc, bd-dwhr, bd-xlvp, bd-biw6,
bd-5bvd, bd-4k8s, bd-lzy2, bd-imwb, bd-59dc, bd-lfgv, bd-kt98, bd-7ij,
bd-t9h, bd-2rq, bd-26x, bd-r2u, bd-f74, bd-14b, bd-132, bd-400, bd-3gk,
bd-85j.14, bd-85j.13, bd-85j.12.

_Per-bead findings to be filled in during Phase 1 execution._

### 5.3 P2 beads (~25+)

_Beads to audit_: bd-o0suq, bd-xk2je, bd-j7akd, bd-wo07o, bd-dmqv, bd-iwz3,
bd-q188, bd-a7i0, bd-p7ie, bd-pdns, bd-2crm, bd-xr1f, bd-7hmv, bd-i4k2,
bd-f5hl, bd-keyt, bd-nhui, bd-2ac, bd-39r, bd-18a, bd-26q.

_Per-bead findings to be filled in during Phase 1 execution._

### 5.4 P3 beads (~9)

_Beads to audit_: bd-hq78, bd-xsr1, bd-39qx, bd-gc5e, bd-3p2x, bd-z6yo,
bd-12d2, bd-xopd, bd-l2v6.

### 5.5 P4 beads (1)

_Beads to audit_: bd-sg59.

---

## 6. Spec Audit — `spec/05 §23.8.5` (Phase 4a.5 Federation Foundations)

_Phases 1-6 of lifecycle/17 to be filled in during execution._

### 6.1 Structural inventory

_Per-INV table to be filled in during Phase 1 of spec audit._

### 6.2 Cross-reference integrity

_To be filled in during Phase 2 of spec audit._

### 6.3 Deep quality audit

_Per-INV findings via 7 lenses to be filled in during Phase 3 of spec audit._

### 6.4 Remediation log

_To be filled in during Phase 4 of spec audit._

---

## 7. Spec Audit — `spec/06` (Prolly Tree, INV-FERR-045..050)

_Phases 1-6 of lifecycle/17 to be filled in during execution._

---

## 8. Spec Audit — `spec/09` (Performance Architecture, INV-FERR-070..085)

_Phases 1-6 of lifecycle/17 to be filled in during execution._

Special check: `bd-s56i` flags duplicate ADR numbers — ADR-FERR-031/032/033
appear in BOTH `spec/05` and `spec/09`. This contradiction must be resolved
during Phase 4 remediation.

---

## 9. Cross-phase consolidation — patterns observed

_Updated incrementally as the audit progresses. Patterns are extracted from
per-bead findings to enable batch remediation in Phase 3._

### Pattern A — `bd-add` phantom dependency edge

**Description**: Multiple open Phase 4a.5 beads carry a `blocked-by` edge to
`bd-add`. `bd-add` was closed 2026-04-08 (PHASE 4A GATE CLOSED, commit
`732c3aa`, tag `v0.4.0-gate`). All such edges are PHANTOM and must be removed.

**Beads observed in P0-P1 audit so far** (3): bd-oiqr, bd-bdvf, bd-r3um.
**Likely affected** (not yet audited): most P2 4a.5 beads created before 2026-04-08.

**Phase 3 batch action**:
```bash
# Identify all open beads with bd-add as a blocker
br list --status=open --format=json | jq -r '... | select(.blockers[] == "bd-add") | .id' \
  | xargs -I{} br dep rm {} bd-add
```
or, more conservatively, enumerate explicitly during Phase 3 reconciliation
once the audit is complete.

### Pattern B — Stale file paths from pre-11-crate-decomp era

**Description**: Phase 4a.5 beads created before the 11-crate decomposition
(bd-cly9, ~session 016) reference file paths that no longer exist. Specifically:
- `ferratomic-core/src/store/*` → moved to `ferratomic-store/src/`
- `ferratomic-core/src/writer/*` → moved to `ferratomic-tx/src/`
- `ferratomic-core/src/schema_evolution.rs` → moved to `ferratomic-store/`

The remaining `ferratomic-core/src/` (db/, mmap.rs, observer.rs, topology.rs,
checkpoint.rs, transport.rs, anti_entropy.rs, backpressure.rs, lib.rs, snapshot.rs)
is still valid.

**Beads observed in P0-P1 audit so far** (3): bd-k5bv, bd-4pna, bd-u5vi.
**Likely affected** (not yet audited): bd-mklv, bd-lifv, bd-1rcm, bd-sup6, bd-7dkk,
bd-6j0r, bd-3t63, bd-h51f, bd-37za, bd-hcns, bd-1zxn, bd-tck2, bd-8f4r, bd-0n9k
(many of which were authored alongside bd-k5bv etc.).

**Cross-link**: bd-0n9k ("Update Phase 4a.5 bead file paths for 11-crate decomposition")
is the existing bead that should batch-fix this. FINDING-021 records the gap that
bd-0n9k does not currently enumerate which beads it covers, and stale-path beads
do not declare bd-0n9k as a precondition.

**Phase 3 batch action**: Audit bd-0n9k first. If its scope covers all stale-path
beads, treat the FINDING-020/022/024 instances as "deferred to bd-0n9k". If not,
either expand bd-0n9k or fix each bead individually.

### Pattern C — Internal numbering not bead-precise

**Description**: Phase 4a.5 beads use internal naming schemes that are not
self-resolvable from the bead alone:
- `B01-B17` — Phase 4a.5 bead labels (B01=bd-bdvf, B17=bd-r7ht, etc.)
- `V01-V03` — Phase 4a.5 verification beads
- `D1-D21` (sometimes higher) — Phase 4a.5 design decisions

**Beads observed in P0-P1 audit so far** (3): bd-r7ht (B01-B16), bd-r3um (V01-V03),
bd-qguw (D16-D21, B01-B09).

**Phase 3 batch action**: For each affected bead, replace internal labels with
br IDs (or include a translation table once at the top of the bead). Alternatively,
publish a single Phase 4a.5 internal-label-to-br-ID glossary in `docs/design/` and
have beads reference it.

### Pattern D — Mismatched citations / spec drift

**Description**: Beads cite spec elements where the cited number and the cited
title don't match. Indicates either spec content changed without bead update,
or a bead authoring typo.

**Beads observed in P0-P1 audit so far** (1): bd-u5vi cites
"INV-FERR-029 (LIVE Resolution Correctness)" but the title belongs to INV-FERR-032.

**Phase 3 action**: Per-bead resolution (not a batch — each mismatch must be
disambiguated by reading the spec to determine which the bead actually means).

### Pattern E — Missing template fields

**Description**: Beads with incomplete lab-grade template coverage. The empty
bdvf.13 body is the extreme case; many other beads are missing one or two
specific sections (Frame Conditions, Postcondition INV-trace, etc.).

**Beads observed in P0-P1 audit so far** (5): bdvf.13 (entire body empty —
critical), bd-oiqr (epic Child Beads + Progress), bd-bdvf (Frame Conditions),
bd-r3um (Frame Conditions; non-template Acceptance shorthand), bd-r7ht
(Postcondition INV-trace, Dependencies enumeration).

**Phase 3 action**: Per-bead remediation. bdvf.13 needs full rewrite from
template; others need additive patches.

### Pattern F — Duplicate ADR-FERR-031 (cross-cuts spec audit)

**Description**: ADR-FERR-031 appears at TWO distinct spec locations with
DIFFERENT content:
- `spec/05:5341` — "Database-Layer Signing" (cited by bd-qguw, bd-0lk8)
- `spec/09:2838` — "Wavelet Matrix Phase 4a Prerequisites — Rank/Select and Attribute Interning"

This is the defect bd-s56i tracks. It MUST be resolved during the spec audit
(Section 8) — not during the bead audit. Bead-side fix is to use unambiguous
file:line citations until the spec is corrected.

**Phase 3 action**: Spec audit Section 8 selects which ADR-031 keeps the number;
the other gets renumbered (likely to ADR-FERR-034 or higher). Then beads citing
the renumbered ADR get updated.

---

## 10. Findings Register (consolidated, severity-ordered)

_Populated as findings emerge. Format per `lifecycle/14` Phase 5 + `lifecycle/17`
Phase 3:_

```
### FINDING-NNN: <one-line description>
**Location**: <file/bead and field/section>
**Lens**: <which audit lens caught this>
**Severity**: CRITICAL | MAJOR | MINOR
**Evidence**: <what was observed>
**Expected**: <what the lab-grade standard requires>
**Fix**: <concrete remediation>
**Status**: open | fixed-in-place | filed-as-bd-XXX | flagged-for-human
```

### CRITICAL findings

_None recorded yet._

### MAJOR findings

#### FINDING-008 — bd-bdvf: Type mismatch (task with 13 children should be epic)
**Location**: bd-bdvf, `Type:` field
**Lens**: 1 (Structural Completeness) — Type field accuracy
**Severity**: MAJOR
**Evidence**: bd-bdvf has 13 parent-child children (bdvf.1 through bdvf.13). Per `lifecycle/14` lab-grade template, `task = atomic work, no children` and `epic = container, has children`. A bead with 13 children is structurally an epic regardless of how it was originally created.
**Expected**: `type=epic` for any bead with parent-child children.
**Fix**: `br update bd-bdvf --type epic` and add `## Child Beads` enumeration + `## Progress` section per epic template (12/13 closed; bottleneck = bdvf.13). The bdvf.1-12 closure history can be summarized: "12 of 13 children closed 2026-04-08; remaining: bdvf.13 (final convergence review)."
**Status**: open (Phase 3 reconciliation)

#### FINDING-020 — bd-k5bv: Stale file paths from pre-11-crate-decomp era
**Location**: bd-k5bv, `## Files` section
**Lens**: 1 (Structural Completeness) + Phase 1 Check 1 (Code reference accuracy)
**Severity**: MAJOR
**Evidence**: bd-k5bv lists these files which DO NOT EXIST at the cited locations after the 11-crate decomposition (bd-cly9):
- `ferratomic-core/src/store/mod.rs` → moved to `ferratomic-store/src/store.rs`
- `ferratomic-core/src/store/apply.rs` → moved to `ferratomic-store/src/apply.rs`
- `ferratomic-core/src/store/merge.rs` → moved to `ferratomic-store/src/merge.rs`
- `ferratomic-core/src/store/tests.rs` → moved to `ferratomic-store/src/tests.rs`
- `ferratomic-core/src/schema_evolution.rs` → moved to `ferratomic-store/src/schema_evolution.rs`
- `ferratomic-core/src/writer/mod.rs` → no `writer/` subdirectory in ferratomic-core; transaction logic moved to `ferratomic-tx/src/{commit,validate,lib}.rs`

The remaining paths (`ferratomic-core/src/{db/mod,db/transact,mmap}.rs`, `ferratom-clock/src/*`, `ferratom/src/lib.rs`) are still valid.

An agent following this bead would attempt to edit nonexistent files, fail, and either (a) ask for clarification (defeating the lab-grade-bead promise of zero questions), or (b) silently make wrong-place edits (creating drift).
**Expected**: Files list reflects current 11-crate layout.
**Fix**: Update Files section to:
```
- ferratom-clock/src/txid.rs (MODIFIED)
- ferratom-clock/src/lib.rs (MODIFIED)
- ferratom-clock/src/frontier.rs (MODIFIED)
- ferratom/src/lib.rs (MODIFIED)
- ferratom/src/clock/ (re-export — verify if any files need touching)
- ferratomic-store/src/store.rs (MODIFIED): genesis_agent → genesis_node
- ferratomic-store/src/apply.rs (MODIFIED): agent → node in create_tx_metadata
- ferratomic-store/src/merge.rs (MODIFIED): genesis_agent → genesis_node
- ferratomic-store/src/tests.rs (MODIFIED): test array updates
- ferratomic-store/src/schema_evolution.rs (MODIFIED): tx/agent → tx/origin, tx/coherence-override → tx/validation-override
- ferratomic-tx/src/commit.rs (MODIFIED): Transaction::new(node) — verify exact location
- ferratomic-tx/src/lib.rs (MODIFIED): re-exports
- ferratomic-core/src/db/mod.rs (MODIFIED): field/method names
- ferratomic-core/src/db/transact.rs (MODIFIED): variable names
- ferratomic-core/src/mmap.rs (MODIFIED): genesis_agent → genesis_node
- spec/00-preamble.md (MODIFIED): add C8 TEST to constraint table
- spec/05-federation.md (MODIFIED): agent → node in INV-FERR references
- ferratomic-verify/tests/* (MODIFIED): test references
```
**Status**: open (Phase 3 reconciliation). Note: Phase 3 will need to re-grep the workspace for `AgentId|tx/agent|genesis_agent|FromAgents|coherence-override` to enumerate every actually-affected file before rewriting the bead's Files list.

#### FINDING-022 — bd-4pna: STALE file paths (same pattern as FINDING-020)
**Location**: bd-4pna, `## Files` section
**Lens**: 1 (Structural) + Phase 1 Check 1
**Severity**: MAJOR
**Evidence**:
- `ferratomic-core/src/writer/commit.rs` — does not exist (`writer/` not in ferratomic-core; logic in `ferratomic-tx/src/{commit,validate}.rs`)
- `ferratomic-core/src/store/apply.rs` — moved to `ferratomic-store/src/apply.rs`
- `ferratomic-core/src/store/tests.rs` — moved to `ferratomic-store/src/tests.rs`
- `ferratomic-core/src/db/recover.rs` — exists ✓

3 of 4 paths stale. The Audit Notes block dates the most recent edit to Session 013, but the path drift was not addressed when the 11-crate decomposition landed (bd-cly9, session 016).
**Expected**: Files list reflects current 11-crate layout.
**Fix**: Update Files section:
```
- ferratomic-tx/src/commit.rs (READ then potentially MODIFIED — validate_datoms)
- ferratomic-tx/src/validate.rs (READ then potentially MODIFIED — validate_datoms helper)
- ferratomic-store/src/apply.rs (READ — confirm replay_entry is correct)
- ferratomic-core/src/db/recover.rs (READ — confirm from_checkpoint is correct)
- ferratomic-store/src/tests.rs (MODIFIED — add regression tests)
```
**Status**: open (Phase 3 reconciliation; will be batched with FINDING-020 stale-path fixes)

#### FINDING-024 — bd-u5vi: STALE file paths (pattern FINDING-020)
**Location**: bd-u5vi, `## Files` section
**Lens**: 1 + Phase 1 Check 1
**Severity**: MAJOR
**Evidence**: `ferratomic-core/src/store/query.rs` and `ferratomic-core/src/store/tests.rs` were both moved to `ferratomic-store/src/{query,tests}.rs` during the 11-crate decomposition. The `ferratomic-store/src/query.rs` file is currently 10KB.
**Expected**: Files list reflects current 11-crate layout.
**Fix**: Update Files section:
```
- ferratomic-store/src/query.rs (READ — audit live_resolve, live_values)
- ferratomic-store/src/tests.rs (MODIFIED — add regression tests)
```
**Status**: open (Phase 3 reconciliation; batched with FINDING-020/022)

#### FINDING-025 — bd-u5vi: INV citation mismatch — number 029, title of 032
**Location**: bd-u5vi, `## Specification Reference > Primary` field
**Lens**: 2 (Specification Traceability)
**Severity**: MAJOR
**Evidence**: Bead cites: "INV-FERR-029 (LIVE Resolution Correctness), Level 2 — Spec file: spec/03-performance.md (INV-FERR-029/032)". The spec file actually has:
- INV-FERR-029 at `spec/03:500` titled **"LIVE View Resolution"**
- INV-FERR-032 at `spec/03:937` titled **"LIVE Resolution Correctness"**

The bead has attached INV-FERR-032's title to INV-FERR-029's number. Inferring intent from bead content (correctness of LIVE retraction handling under Card:Many and Op ordering — these are *correctness* properties, matching INV-FERR-032's title), the intended INV is most likely **INV-FERR-032**.
**Expected**: Citation matches spec content. Either change number to 032 or change title to "LIVE View Resolution" — pick whichever matches the bead's actual semantic intent.
**Fix**: After confirming intent (recommend reading spec/03 §INV-FERR-029 and §INV-FERR-032 in full to determine which one the bead's verification work targets), update the citation. If both apply, cite both: "INV-FERR-029 (LIVE View Resolution) Level 1, and INV-FERR-032 (LIVE Resolution Correctness) Level 2".
**Status**: open (Phase 3 reconciliation)

#### FINDING-027 — bd-0lk8: 3 phase labels including potentially mismatched phase-4a5
**Location**: bd-0lk8, `Labels:` field
**Lens**: 5 (Frame Adequacy / phase coherence)
**Severity**: MINOR
**Evidence**: Labels = `experiment, phase-4a5, phase-4c`. The bead's `## Goal` says it informs "BEFORE Phase 4c commits to per-transaction signature verification". phase-4c is correct. phase-4a5 is questionable — Ed25519 throughput informs Phase 4c federation transport design, not Phase 4a.5 implementation work. The experiment label is correct.
**Expected**: Phase labels match the phase whose work depends on the bead's outcome.
**Fix**: Either remove `phase-4a5` (cleaner) OR document why both phases need this signal (e.g., "phase-4a5 because the signing primitives land in 4a.5 and we want to know if they'll be a bottleneck before phase-4c federation builds on them").
**Status**: open (Phase 3 reconciliation)

#### FINDING-028 — bd-0lk8: Deliverable filename has unresolved placeholder
**Location**: bd-0lk8, `## Files` and `## Success Criteria` item 5
**Lens**: 1 (Structural)
**Severity**: MINOR
**Evidence**: `docs/research/2026-04-XX-ed25519-throughput.md (NEW)` — the date placeholder `2026-04-XX` is not resolved. When the bead is executed, the agent must invent a date, creating drift between the bead's prediction and the actual artifact path.
**Expected**: Either set the date to "the date of execution" with a clear convention, OR specify the date now (e.g., 2026-04-09 if planned for tomorrow).
**Fix**: Replace with "2026-04-{today}" convention or set explicit target date.
**Status**: open (Phase 3 reconciliation)

#### FINDING-029 — bd-0lk8: ADR-FERR-031 citation is ambiguous due to duplicate ADR numbers
**Location**: bd-0lk8, `## Specification Reference > Supporting`
**Lens**: 2 (Traceability)
**Severity**: MINOR (for this bead) — but ROOTS in a CRITICAL spec defect
**Evidence**: bd-0lk8 cites `ADR-FERR-031 (Database-Layer Signing)`. The number ADR-FERR-031 exists in TWO places:
- `spec/05:5341` — "Database-Layer Signing" (intended)
- `spec/09:2838` — "Wavelet Matrix Phase 4a Prerequisites — Rank/Select and Attribute Interning" (collision)

The bead disambiguates by including the title, which is OK as a workaround. But the underlying defect — two distinct ADRs sharing a number — is what bd-s56i tracks. The spec audit (Section 8 of this report) must resolve which ADR-031 keeps the number and renumber the other.
**Expected**: Each ADR-FERR number is unique across the entire spec.
**Fix (this bead)**: Once the spec audit resolves the collision, update the bead's citation to include the unambiguous file:line.
**Fix (cross-cutting)**: See Section 8 spec audit; cross-reference bd-s56i.
**Status**: open (depends on spec audit Section 8)

#### FINDING-026 — bd-u5vi: Unresolved Session 013 audit annotation ("ref 005 should be 012")
**Location**: bd-u5vi, `## Specification Reference > Supporting`
**Lens**: 2 (Traceability)
**Severity**: MINOR
**Evidence**: bd-u5vi's Notes section says: "AUDIT PASS (Session 013). Phase 1: All 4 checks pass. ... Phase 2: 6 PASS, L2 minor (ref 005 should be 012), L3 marginal." This audit annotation flags the INV-FERR-005 supporting reference as potentially wrong (should be 012), but the bead body still cites 005 — the annotation has not been actioned.
**Expected**: Audit annotations are either applied to the bead body or formally rejected with reasoning. They should not linger as unresolved comments.
**Fix**: Verify which supporting INV is correct for the "Op ordering in im::OrdSet" claim. INV-FERR-005 (Index Bijection, `spec/01:360`) is about index correctness; INV-FERR-012 (Content-Addressed Identity) is about EntityId derivation. The Op ordering claim relates to neither directly — it relates to the Datom Ord impl, which is part of how indexes maintain their bijection. **Recommendation**: Cite INV-FERR-005 (it IS about index correctness which depends on Datom ordering) and reject the Session 013 annotation as incorrect. **Or FLAG** if not confident.
**Status**: FLAG-candidate (will resolve in Phase 3 by reading both INVs in full)

#### FINDING-023 — bd-4pna: Deferred Pseudocode Contract behind undecided fix option
**Location**: bd-4pna, `## Pseudocode Contract` section
**Lens**: 6 (Compiler Test) — contract specificity
**Severity**: MINOR
**Evidence**: "N/A — verification/fix bead, type changes depend on which fix option is chosen." But the bead's `## Fix` section says: "Option A is more principled — it validates the schema-defining datoms too." The bead has implicitly chosen Option A; the contract should be written for it.
**Expected**: Commit to Option A, write the Pseudocode Contract for the two-phase validation function (e.g., `fn pre_evolve_schema_for_validation(datoms: &[Datom], schema: &Schema) -> Result<Schema, FerraError>` plus the rewritten `validate_datoms` signature).
**Fix**: Author the Pseudocode Contract for Option A, including: (a) the new helper function signature, (b) the modified `validate_datoms` flow, (c) which file owns each piece (`ferratomic-tx/src/validate.rs`).
**Status**: open (Phase 3 reconciliation)

#### FINDING-021 — bd-k5bv: Overlaps with bd-0n9k's mandate
**Location**: bd-k5bv (and likely all open Phase 4a.5 beads with stale paths)
**Lens**: Phase 1 Check 4 (overlap detection)
**Severity**: MINOR
**Evidence**: bd-0n9k is titled "Update Phase 4a.5 bead file paths for 11-crate decomposition". Its mandate is exactly to fix what FINDING-020 describes for bd-k5bv. Yet bd-k5bv does not list bd-0n9k as a precondition, and bd-0n9k does not enumerate which Phase 4a.5 beads need updating. The two beads are conceptually linked but not graph-linked.
**Expected**: Either (a) bd-0n9k is a precondition of every stale-path Phase 4a.5 bead, OR (b) bd-0n9k owns the stale-path remediation centrally and other beads get updated as a side effect.
**Fix**: When auditing bd-0n9k (P2 cluster), evaluate whether its scope should be expanded to enumerate every affected bead AND whether it should be elevated to P1. For now, file the cross-link as an observation; final fix decision deferred to bd-0n9k audit.
**Status**: open (deferred to bd-0n9k audit, then Phase 3 reconciliation)

#### FINDING-010 — bd-bdvf: Acceptance criterion is a forward-reference to an empty child
**Location**: bd-bdvf, `Acceptance:` field
**Lens**: 3 (Postcondition Strength)
**Severity**: MAJOR (cascades from FINDING-012)
**Evidence**: bd-bdvf's Acceptance is "bdvf.13 closed cleanly with audit notes." But bdvf.13's body is empty (FINDING-012), so "closed cleanly" has no concrete predicate. The acceptance is unverifiable until bdvf.13 is rewritten.
**Expected**: Acceptance must enumerate the convergence review's binary postconditions: e.g., "Five lenses (Completeness, Soundness, Simplicity, Adversarial, Traceability) applied to every INV-FERR in §23.8.5; spec/README.md counts updated to match actual file content; every INV-FERR cited in §23.8.5 has a back-reference from its target."
**Fix**: After FINDING-012 rewrites bdvf.13, lift bdvf.13's postconditions into bd-bdvf's Acceptance section as the parent-aggregate criterion.
**Status**: open (depends on FINDING-012)

#### FINDING-012 — bd-bdvf.13: Body is **empty** (only title content)
**Location**: bd-bdvf.13, entire bead body
**Lens**: 1 (Structural Completeness), 2 (Traceability), 3 (Postcondition Strength), 5 (Frame Adequacy)
**Severity**: MAJOR
**Evidence**: `br show bd-bdvf.13` returns only metadata (id/title/owner/labels/dates) and dependency edges. There is no description body. None of the 13 lab-grade template fields are populated. The title alone carries the intent: "Five-lens convergence review (Completeness/Soundness/Simplicity/Adversarial/Traceability) + spec/README.md counts + bidirectional cross-refs". An agent loaded with only this bead cannot determine: which spec section to audit, which lenses produce which artifacts, what counts to verify, what "closed cleanly" means.
**Expected**: Lab-grade body with all required fields per `lifecycle/14` task template:
```
## Specification Reference
- Primary: spec/05 §23.8.5 (lines 4951-end), spec/README.md
- Methodology: lifecycle/16-spec-authoring.md §"Five-lens convergence protocol"
- Supporting: lifecycle/17-spec-audit.md (Phase 5 convergence verification)

## Preconditions
1. bdvf.1-12 closed (spec content + ADRs + Level 2 Rust contracts authored)
2. The current audit session's findings (this audit doc) have been remediated
3. spec/05 §23.8.5 reflects current Phase 4a.5 scope

## Postconditions
1. Lens "Completeness": every Phase 4a.5 INV-FERR (060, 061, 062, 063, 025b, 086) has all 6 verification layers populated. Verify: structural inventory check.
2. Lens "Soundness": every proof sketch cites a real mechanism (no "obvious"). Verify: grep for "obvious" / "by construction" produces zero matches in §23.8.5.
3. Lens "Simplicity": no spec section duplicates content from another section. Verify: cross-section diff.
4. Lens "Adversarial": every falsification condition is generator-searchable. Verify: each Falsification field has a concrete predicate, not "some invalid state".
5. Lens "Traceability": every INV cited from §23.8.5 has a back-reference at the cited target. Verify: bidirectional grep.
6. spec/README.md counts (86 invariants, 32 ADRs, 7 NEGs, 2 CIs) match actual file content. Verify: `grep -c "^### INV-FERR" spec/*.md` etc.
7. The five-lens convergence pass produces zero structural changes (idempotency). Verify: re-run pass returns nothing.

## Frame Conditions
1. No code modifications.
2. Only spec/05, spec/README.md, and this audit doc may be edited.
3. Existing Lean proofs must continue passing (`lake build`).

## Verification Plan
1. Run lifecycle/17 Phase 5 (convergence verification) script/checklist.
2. `cargo doc --workspace --no-deps -- -D warnings` (catches broken doc-comment cross-refs).
3. `lake build` (catches Lean theorem statement drift).

## Files
- `spec/05-federation.md` (potential edits to §23.8.5 if convergence finds gaps)
- `spec/README.md` (count updates)
- `docs/reviews/2026-04-08-phase-4a5-4b-audit.md` (status update)

## Dependencies
- Depends on: bd-bdvf (parent epic — provides scope)
- Depends on: this audit session's remediation (the convergence pass runs AFTER the audit + fixes)
- Blocks: bd-y1rs (spine reframe EPIC), bd-m8ym (canonical spec form)
```
**Fix**: Rewrite the bead body via `br update bd-bdvf.13 --description "$(cat <<'BODY' ... BODY)"` with the full lab-grade content above. The audit session can either author this rewrite directly during Phase 3 or file it as a separate remediation bead.
**Status**: open (Phase 3 reconciliation)



### MINOR findings

#### FINDING-001 — bd-r7ht: Dependencies prose enumerates 1 of 8 graph dependencies
**Location**: bd-r7ht, `## Dependencies` section
**Lens**: 1 (Structural Completeness) — `Dependencies [R]` field
**Severity**: MINOR
**Evidence**: Graph has 8 hard `blocked-by` edges (bd-bdvf, bd-hlxr, bd-mklv, bd-lifv, bd-sup6, bd-7dkk, bd-3t63, bd-h51f) plus the bd-oiqr EPIC parent. Bead prose lists only `bd-hlxr` by ID; the remaining 7 are absorbed under "all Phase 4a.5 types, signing, transport, selective merge, identity implemented and tested."
**Expected**: Lab-grade template requires `Depends on: <bead-id> — <what it produces that this consumes>` per dependency. Bidirectional and bead-precise.
**Fix**: Enumerate all 8 hard deps with one-line "what it produces" annotations. Lift the Blocks list (bd-r3um, bd-j1mp, bd-y1rs, bd-m8ym, bd-d6dl, bd-ipzu, bd-gc5e, bd-8o8t) into the prose as well.
**Status**: open (Phase 3 reconciliation)

#### FINDING-002 — bd-r7ht: Postcondition #8 lacks INV-FERR trace
**Location**: bd-r7ht, `## Postconditions` item 8
**Lens**: 3 (Postcondition Strength) — INV-tracing requirement
**Severity**: MINOR
**Evidence**: ":verification/* schema attributes installed and queryable. Verify: test assertion." carries no INV-FERR or ADR-FERR citation.
**Expected**: Every postcondition must trace to a primary source (INV-FERR or NEG-FERR) per `lifecycle/14` Lens 3, OR explicitly note "non-INV catalog requirement" with the source (e.g., `ADR-FERR-013 catalog schema`).
**Fix**: Trace #8 to `ADR-FERR-013` (Machine-Readable Invariant Catalog) which defines the `:verification/*` namespace. Strengthen Verify clause to include the specific schema attributes (e.g., `:verification/lean-status`, `:verification/proptest-passes`) and a query that returns them all.
**Status**: open (Phase 3 reconciliation)

#### FINDING-003 — bd-r7ht: Postcondition #9 lacks the concrete predicate body
**Location**: bd-r7ht, `## Postconditions` item 9
**Lens**: 3 (Postcondition Strength) — verifiability requirement
**Severity**: MINOR
**Evidence**: "Gate closure expressible as predicate: query for Stage 0 invariants missing lean-status returns empty. Verify: predicate query." The English description gives the predicate's intent but not its mechanical form.
**Expected**: An agent must be able to write the verification test from the postcondition alone, without inventing the query.
**Fix**: Include the actual Datalog/query body in the bead, e.g.:
```
?inv :stage 0
?inv :verification/lean-status :missing
;; result count must be 0
```
And cross-reference the test name (`test_bootstrap_gate_predicate` or similar).
**Status**: open (Phase 3 reconciliation)

#### FINDING-005 — bd-oiqr: Epic body missing `## Child Beads` enumeration
**Location**: bd-oiqr, bead description
**Lens**: 1 (Structural Completeness) — Epic-Specific Fields template
**Severity**: MINOR
**Evidence**: Lifecycle/14 epic template requires `## Child Beads` with `<bead-id>: <title> (status)` per child. bd-oiqr's 23 children are visible only via `br show`'s `Dependents:` graph metadata, not in the description body. If the dep graph is ever lost or the bead is read in plain-text isolation, the children are unrecoverable from the body alone.
**Expected**: Body contains explicit child enumeration matching the parent-child edges.
**Fix**: Add `## Child Beads` section listing all 23 children with current status. The information is already in the graph; this is a body↔graph synchronization fix.
**Status**: open (Phase 3 reconciliation)

#### FINDING-006 — bd-oiqr: Epic body missing `## Progress` tracking
**Location**: bd-oiqr, bead description
**Lens**: 1 (Structural Completeness) — Epic-Specific Fields template
**Severity**: MINOR
**Evidence**: Lifecycle/14 epic template requires `## Progress` with N/M children closed and current bottleneck. bd-oiqr has neither.
**Expected**: `## Progress` section: "0/23 children closed. Current bottleneck: bd-bdvf (highest centrality, blocks 4 federation children including the 8 leaf-type beads)."
**Fix**: Add `## Progress` section. Compute N/M from child statuses and identify the highest-centrality unclosed child as the bottleneck.
**Status**: open (Phase 3 reconciliation)

#### FINDING-009 — bd-bdvf: Phantom dependency edge to bd-add (same as FINDING-007 pattern)
**Location**: bd-bdvf → bd-add edge
**Lens**: Phase 1 Check 3 (Dependencies) — phantom edge detection
**Severity**: MINOR
**Evidence**: bd-bdvf is `blocked-by` bd-add. bd-add closed 2026-04-08 (Phase 4a gate).
**Expected**: Phantom edges to satisfied beads are removed in Phase 3.
**Fix**: `br dep rm bd-bdvf bd-add`. (Pattern: any open Phase 4a.5/4b bead with a `blocked-by` edge to bd-add will share this finding. Will batch all such removals.)
**Status**: open (Phase 3 reconciliation)

#### FINDING-011 — bd-bdvf: Frame Conditions section missing
**Location**: bd-bdvf, missing `## Frame Conditions` section
**Lens**: 1 (Structural Completeness), 5 (Frame Adequacy)
**Severity**: MINOR
**Evidence**: Lab-grade template requires `## Frame Conditions` even when "None — greenfield" or for docs beads.
**Expected**: Explicit frame: e.g., "1. No code modifications. 2. Only spec/05 §23.8.5 and spec/README.md may be edited. 3. Existing Lean proofs must continue passing."
**Fix**: Add `## Frame Conditions` section to bd-bdvf.
**Status**: open (Phase 3 reconciliation)

#### FINDING-014 — bd-r3um: Non-template `Acceptance` shorthand
**Location**: bd-r3um, `Acceptance:` section
**Lens**: 1 (Structural Completeness)
**Severity**: MINOR
**Evidence**: Single `Acceptance:` field combines what the lab-grade template separates into `## Postconditions` (binary, INV-traced criteria) and `## Verification Plan` (test names, commands).
**Expected**: Two distinct sections per template. Postconditions = the WHAT; Verification Plan = the HOW.
**Fix**: Split `Acceptance` into `## Postconditions` (items 1-2, 6-7: state predicates) and `## Verification Plan` (items 3-5: cargo commands). Optionally merge with FINDING-017's added Frame Conditions in a single update.
**Status**: open (Phase 3 reconciliation)

#### FINDING-015 — bd-r3um: Internal `V01-V03` numbering not resolvable from bead alone
**Location**: bd-r3um, `Acceptance:` item 2
**Lens**: 5 (Frame/Self-containment)
**Severity**: MINOR
**Evidence**: "All Phase 4a.5 verification beads (V01-V03) are closed." V01-V03 is an internal Phase 4a.5 verification bead numbering. An agent reading the bead in isolation cannot map V01-V03 to br IDs (likely bd-4pna, bd-u5vi, bd-u2tx based on titles, but not stated). Pattern matches FINDING-004 (bd-r7ht's B01-B16).
**Expected**: Use br IDs directly: "All of bd-4pna, bd-u5vi, bd-u2tx are closed."
**Fix**: Replace V01-V03 with the explicit br ID list.
**Status**: open (Phase 3 reconciliation)

#### FINDING-016 — bd-r3um: Phantom dependency edge to bd-add (pattern)
**Location**: bd-r3um → bd-add edge
**Lens**: Phase 1 Check 3
**Severity**: MINOR
**Evidence**: bd-r3um is `blocked-by` bd-add. bd-add closed 2026-04-08. **Pattern**: this is the third bead with this exact phantom edge (bd-oiqr, bd-bdvf, bd-r3um). Likely more open beads share it.
**Expected**: Phantom edges removed in Phase 3.
**Fix**: `br dep rm bd-r3um bd-add`. **Note**: I'll batch all bd-add phantom-edge removals when Phase 3 begins.
**Status**: open (Phase 3 reconciliation; pattern will be tracked across the audit)

#### FINDING-018 — bd-qguw: Postcondition #3 is a precondition phrased as a postcondition
**Location**: bd-qguw, `## Postconditions` item 3
**Lens**: 3 (Postcondition Strength) — postcondition vs precondition
**Severity**: MINOR
**Evidence**: "[INV-FERR-086] Format already in spec/05-federation.md. Verify: grep INV-FERR-086." This asserts a state that already holds before this bead's work (the spec was authored in bdvf.1). It's a precondition, not a postcondition — the bead's work doesn't *make* this true, it *depends on* it being true.
**Expected**: Move to `## Preconditions` as item 4 ("INV-FERR-086 spec content is finalized in spec/05 §23.8.5").
**Fix**: Renumber postconditions; move #3 to Preconditions.
**Status**: open (Phase 3 reconciliation)

#### FINDING-019 — bd-qguw: Internal D-/B- numbering not bead-precise
**Location**: bd-qguw, `## Specification Reference > Design decisions` and `## Why This Exists` and `## Dependencies > Depends on`
**Lens**: 5 (Self-containment) — same pattern as FINDING-004/015
**Severity**: MINOR
**Evidence**: References "D16, D17, D19, D21" (Phase 4a.5 design decisions) and "B01, B08, B09" (internal bead labels). An agent reading the bead in isolation cannot map these to br IDs without an external translation table. The Phase 4a.5 design decision document and the B-labels are internal session-016 artifacts.
**Expected**: Either inline the relevant design decision content, OR cross-reference by stable identifier (e.g., spec section + ADR number), OR replace B-labels with br IDs.
**Fix**: Replace `B01` → `bd-bdvf`, `B08` → `bd-3t63 (fingerprint)`, `B09` → `bd-6j0r (signing)`. For D16-21, either inline the decision name (e.g., "Decision: tx_entity = EntityId::from_content(canonical_bytes(tx_id))") or reference an ADR if one exists.
**Status**: open (Phase 3 reconciliation)

#### FINDING-017 — bd-r3um: Frame Conditions section missing
**Location**: bd-r3um, missing `## Frame Conditions`
**Lens**: 5 (Frame Adequacy)
**Severity**: MINOR
**Evidence**: No `## Frame Conditions` section. For routing/gate beads, frame is non-trivial: "no file modifications, no code touched, closure is a state change only" — and explicitly that closing this gate must NOT cascade-close any of its dependencies.
**Expected**: Explicit `## Frame Conditions` even when minimal.
**Fix**: Add: "1. No file modifications. 2. No code touched. 3. Closure is a metadata-only state change. 4. Closing this gate must not cascade-close dependencies — children must be closed individually with their own evidence."
**Status**: open (Phase 3 reconciliation)

#### FINDING-013 — bd-bdvf.13: Sibling `blocks` edge to bd-bdvf.12 has wrong relation type
**Location**: bd-bdvf.13 → bd-bdvf.12 edge
**Lens**: Phase 1 Check 3 (Dependencies)
**Severity**: MINOR
**Evidence**: bdvf.13 lists `bd-bdvf.12 (blocks)` as an outgoing dependency. The two are sibling-children of bd-bdvf, not a hierarchical block relationship. bdvf.12 is also closed (per bd-bdvf description), so the edge is also PHANTOM.
**Expected**: Sibling ordering should be expressed via either (a) shared parent's child enumeration order, or (b) explicit `depends_on` relation only when one sibling truly produces an artifact the other consumes. In this case, bdvf.12 was a precursor authoring step (convention docs); bdvf.13 is the audit pass. The dep is real but the bead is closed, so the edge is PHANTOM.
**Fix**: `br dep rm bd-bdvf.13 bd-bdvf.12`. Document sibling ordering in bd-bdvf's epic body (post-FINDING-008 conversion).
**Status**: open (Phase 3 reconciliation)

#### FINDING-007 — bd-oiqr: Phantom dependency edge to bd-add
**Location**: bd-oiqr → bd-add edge
**Lens**: Phase 1 Check 3 (Dependencies) — phantom edge detection
**Severity**: MINOR
**Evidence**: bd-oiqr is `blocked-by` bd-add per the dep graph. bd-add was closed 2026-04-08 ("PHASE 4A GATE CLOSED — composite 9.55-9.57/A+", commit `732c3aa`, tag `v0.4.0-gate`). The Phase 4a gate (which bd-add tracks) was the prerequisite for the Phase 4a.5 epic to start; that prerequisite is satisfied, so the edge is PHANTOM (closed-and-satisfied).
**Expected**: Phantom edges to closed-and-satisfied beads are removed during Phase 3 reconciliation.
**Fix**: `br dep rm bd-oiqr bd-add` (no other consequence — the work the edge represented is done).
**Status**: open (Phase 3 reconciliation)

#### FINDING-004 — bd-r7ht: Frame condition #1 uses internal `B01-B16` numbering
**Location**: bd-r7ht, `## Frame Conditions` item 1 ("No modification to any B01-B16 file")
**Lens**: 5 (Frame Condition Adequacy) — self-contained-bead requirement
**Severity**: MINOR
**Evidence**: An agent loaded with only AGENTS.md, the cited spec module, and this bead cannot resolve "B01-B16" to actual file paths or br IDs without an external translation table. The lab-grade standard says the bead alone is sufficient context.
**Expected**: Frame conditions reference br IDs and/or file paths directly.
**Fix**: Replace "any B01-B16 file" with the explicit list of files owned by the 8 dependency beads (or the file globs they cover, e.g., `ferratomic-store/src/identity.rs`, `ferratomic-store/src/transport/local.rs`, etc.). Alternatively, replace with "any file owned by an open dependency bead" if dependency edges are made authoritative.
**Status**: open (Phase 3 reconciliation)

---

## 11. Remediation Log

_Populated during Phase 3 of bead audit + Phase 4 of spec audit. Each entry
links to the finding it resolves._

| Action | Bead/INV | Justification (FINDING-NNN) | Status |
|--------|----------|----------------------------|--------|
| _none yet_ | | | |

---

## 12. New Beads Filed

_Findings that require their own scope (not in-place fixes) are filed as new
beads. Tracked here for traceability._

| New bead ID | Title | Source finding | Phase label | Priority |
|-------------|-------|---------------|-------------|----------|
| _none yet_ | | | | |

---

## 13. AFTER metrics

_Captured after Phase 4 of bead audit (graph integrity verification). Empty
until that phase runs._

| Metric | Before | After | Delta |
|--------|--------|-------|-------|
| _to be filled_ | | | |

---

## 14. Flags for Human Review

_Per `lifecycle/14` uncertainty protocol — items where the auditor lacks
primary-source evidence to act unilaterally are flagged here for the human
to resolve._

_None yet._

---

## 15. Session continuation

This audit is explicitly multi-session per the user's "DO NOT rush" directive.
Each session updates this document in place. The header shows current status.
The handoff section below identifies the next pickup point.

### Session 1 progress (2026-04-08, this session)

**Completed**:
- Phase 0 grounding (orientation, BEFORE metrics, gold-standard calibration)
- Bead audit Phase 1: 4a.5 P0 (1/1) and 4a.5 P1 (9/9) — **10 beads total**
- 29 findings recorded (1 MAJOR-pattern bdvf.13 empty, 7 MAJOR, 21 MINOR)
- 6 cross-phase patterns extracted to Section 9 for batch remediation

**Beads audited this session**:
1. bd-r7ht (P0) — SOUND, 4 MINOR
2. bd-oiqr (EPIC P1) — NEEDS WORK (epic template gaps), 3 findings
3. bd-bdvf (P1) — NEEDS WORK (type=task with 13 children, needs RECLASSIFY → epic), 4 findings
4. bd-bdvf.13 (P1) — **CRITICAL FAIL** (empty body, REWRITE), 2 findings
5. bd-r3um (P1 gate) — SOUND, 4 MINOR
6. bd-qguw (P1) — **EXEMPLARY** (gold standard), 2 MINOR
7. bd-k5bv (P1) — NEEDS WORK (stale paths), 2 findings
8. bd-4pna (P1) — NEEDS WORK (stale paths + deferred contract), 2 findings
9. bd-u5vi (P1) — NEEDS WORK (citation mismatch + stale paths), 3 findings
10. bd-0lk8 (P1) — SOUND, 3 MINOR

**Next pickup point** (Session 2):

**Phase**: Bead audit Phase 1 — Phase 4a.5 P2 beads (16 beads remaining)
**Next bead**: bd-0n9k (highest impact — owns the stale-path remediation per Pattern B)

**Recommended order for P2 cluster**:
1. **bd-0n9k** first (owns Pattern B — its scope determines whether stale-path findings get batched there or fixed individually)
2. Then leaf data-type beads in dependency order: bd-tck2, bd-8f4r, bd-37za, bd-hcns, bd-1zxn (these are the ferratom additive types)
3. Then signing/identity beads: bd-mklv, bd-6j0r, bd-3t63, bd-h51f
4. Then transport beads: bd-1rcm, bd-lifv
5. Then merge beads: bd-7dkk, bd-sup6
6. Then audit/integration beads: bd-u2tx, bd-hlxr
7. Finally bd-s4ne (P3 docs)

After 4a.5 cluster completes (~16 more beads), proceed to Phase 4b cluster
(85 beads, P0→P4 order). Then spec audits.

**Time/quality discipline**: Maintain per-bead lab-grade depth (5-15 min each).
Do not batch. Do not rush. The user explicitly directed multi-session pacing
("DO NOT rush to finish the beads within a single session").
