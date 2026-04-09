# 17 Spec Audit — Verification and Hardening of Specification Content

> **Purpose**: Systematically audit every invariant, ADR, and negative requirement
> in one or more spec sections against the six-layer quality standard. Find
> structural gaps, internal contradictions, broken cross-references, and
> weak falsification conditions. Harden each element to the quality level
> of `spec/01-core-invariants.md`.
>
> **DoF**: High (adversarial discovery) → Structured (assessment) → Low (remediation).
>
> **Cognitive mode**: Adversarial verification. Assume every invariant is
> incomplete until proven otherwise by structural evidence. You are not here
> to confirm the spec is correct — you are here to find where it is wrong,
> weak, or inconsistent.
>
> **Model gate**: Opus 4.6 with /effort max or GPT 5.4 xhigh. Cross-referencing
> 55+ invariants against 6 verification layers and multiple spec sections
> demands sustained formal precision.

---

## When to Use This Prompt

- After a spec authoring session completes a batch of new invariants
- Before a phase gate decision (is the spec for Phase N complete?)
- After implementation reveals spec gaps (code exists without spec backing)
- Periodic hygiene (e.g., quarterly spec health check)
- When a progress review (13) scores Verification Depth below B

**This prompt modifies spec files** — it adds missing fields, strengthens weak
sections, and fixes cross-references. It does NOT produce code, tests, or
Lean proofs. Those downstream artifacts are created by other lifecycle prompts.

---

## The Lab-Grade Spec Standard

A spec invariant meets lab-grade standard when it satisfies this predicate:

> All six verification layers are populated with content that is mutually
> consistent: the Lean theorem proves the Level 0 law, the proptest tests
> the falsification condition, the Level 2 contract implements the Level 1
> operational description, and every cross-reference resolves to a real
> element. An agent reading only this invariant and its upstream traces
> can write a correct test, a correct implementation, and a correct Lean
> proof without consulting any other source.

### The Six Verification Layers

| Layer | Content | Quality Predicate |
|-------|---------|-------------------|
| Level 0 | Algebraic law with proof sketch | Proof cites specific mechanism (not "obvious") |
| Level 1 | Operational meaning | 3+ sentences; states consequences of violation |
| Level 2 | Rust contract + Kani harness | Compiles conceptually; uses spec types |
| Falsification | Counterexample shape | Specific enough for a generator to search |
| proptest | Working proptest code | Tests the falsification condition, not a weaker property |
| Lean | Theorem statement + proof | Logically equivalent to Level 0 over Finset Datom model |

An invariant missing any layer is structurally incomplete. An invariant where
layers contradict each other is internally inconsistent. Both are findings.

**Cross-reference with GOALS.md §6**: When auditing an invariant's `**Verification**` tags, check whether the invariant should also prescribe `V:MIRI`, `V:FUZZ`, `V:MUTANT`, or `V:FAULT` per GOALS.md §6.4. Invariants touching unsafe boundaries, deserialization, or durability paths should have the corresponding dynamic analysis tags.

---

## Phase 0: Ground Yourself

```bash
# Orientation
cat AGENTS.md
cat spec/README.md

# Methodology
ms load spec-first-design -m --full

# Scope: which spec section(s) to audit
cat spec/<target-section>.md

# Cross-reference: what implementation exists for these invariants
grep -r "INV-FERR" --include="*.rs" ferratomic-*/src/ ferratom/src/ ferratom-clock/src/
```

**Checkpoint**: Before auditing, you must know:
- Total invariant count in the target section
- Which INV-FERR are Stage 0 (must be complete) vs Stage 1+ (may have deferrals)
- The gold standard format (re-read INV-FERR-001 in `spec/01-core-invariants.md`)
- The Lean foundation model definitions (`spec/00-preamble.md §23.0.4`)

---

## Phase 1: Structural Inventory (Low DoF)

**Objective**: Enumerate every spec element and check for the presence (not
quality) of each verification layer. Pure inventory — no judgment.

For each INV-FERR in the target section:

| Check | How | Record |
|-------|-----|--------|
| Traces-to present? | Read header | ✓ / — |
| Verification tags present? | Read header | List which tags |
| Stage present? | Read header | 0 / 1 / 2 |
| Level 0 present? | Search for `#### Level 0` | ✓ / — |
| Level 0 has proof sketch? | Read Level 0 block | ✓ / "obvious" / — |
| Level 1 present? | Search for `#### Level 1` | ✓ / — |
| Level 2 present? | Search for `#### Level 2` | ✓ / `todo!()` / — |
| Falsification present? | Search for `**Falsification**` | ✓ / — |
| proptest present? | Search for `**proptest strategy**` | ✓ / — |
| Lean present? | Search for `**Lean theorem**` | ✓ / `sorry` / — |

For each ADR-FERR: check Problem, Options table, Decision, Rejected, Consequence, Source.

For each NEG-FERR: check Traces-to, Stage, description.

### Output: Structural Inventory Table

| ID | Traces | Tags | Stage | L0 | L0-proof | L1 | L2 | Falsify | proptest | Lean | Gaps |
|----|--------|------|-------|----|----------|----|----|---------|----------|------|------|
| 001 | ✓ | 4 | 0 | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | 0 |
| 045 | ✓ | 3 | 1 | ✓ | ✓ | ✓ | todo | ✓ | — | sorry | 2 |

---

## Phase 2: Cross-Reference Integrity (Structured DoF)

**Objective**: Verify that every reference resolves and every relationship
is bidirectional.

### Check 1: Upstream Traces

For each INV-FERR's "Traces to" field:
- Does each cited INV-FERR/INV-STORE/C-N/L-N actually exist?
- Does the cited element's scope match the citing invariant's claim?
- Is the refinement direction correct? (INV-FERR refines INV-STORE, not vice versa)

### Check 2: Bidirectional Cross-References

For each INV-FERR that cites another INV-FERR:
- Does the cited invariant acknowledge the citation? (Back-reference)
- If A says "Traces to: INV-FERR-B" but B doesn't mention A, that's an orphan reference.

### Check 3: Code References

```bash
# For each INV-FERR in the target section:
grep -r "INV-FERR-NNN" --include="*.rs" .
```

- Does the implementation reference this invariant in doc comments?
- Does the invariant's Level 2 correspond to actual code patterns?
- Are there code references to INV-FERR IDs that don't exist in the spec?

### Check 4: Inter-Section Consistency

For invariants that span multiple sections (e.g., INV-FERR-005 in §23.1
referenced by INV-FERR-046 in §23.9):
- Are the claims consistent? Does §23.9's reference to §23.1 match what §23.1 says?
- Has the referenced invariant changed since the reference was written?

### Output: Cross-Reference Report

List every broken reference, orphan reference, and missing back-reference.

---

## Phase 3: Deep Quality Audit (High DoF)

**Objective**: For each invariant, assess the QUALITY of each verification layer.
This is adversarial — actively try to break each invariant.

Apply these 7 audit lenses to each INV-FERR. Lenses are applied sequentially to
each invariant, not in batch (one invariant at a time, all lenses).

### Lens 1: Algebraic Soundness

Is the Level 0 law mathematically correct?

- Does the proof sketch cite a real mechanism?
- Could you construct a counterexample the proof doesn't account for?
- Is the algebraic structure correctly identified?
- Does the universal quantifier cover the right domain? ("for all stores"
  vs "for all non-empty stores" — the difference matters)

### Lens 2: Level 0 ↔ Level 2 Consistency

Does the Rust contract actually implement the algebraic law?

- If Level 0 says `∀ A,B: f(A,B) = f(B,A)`, does Level 2's function
  structurally guarantee this?
- Does Level 2 introduce implementation details that could violate Level 0?
  (e.g., order-dependent operations in a contract for a commutative law)
- Does Level 2 use the correct types? (BTreeSet for spec, im::OrdSet for impl)

### Lens 3: Falsification Adequacy

Is the falsification condition strong enough?

- Is it the negation of Level 0? (If Level 0 says ∀x: P(x), falsification
  should be ∃x: ¬P(x) — not some weaker condition)
- Is it specific enough for a generator to search? ("Some invalid state"
  is not searchable. "Any pair (A,B) where |merge(A,B)| < |A|" is.)
- Does it cover edge cases? (Empty stores, single-element stores,
  stores with duplicate entities, maximum-size stores)

### Lens 4: proptest ↔ Falsification Correspondence

Does the proptest actually test the falsification condition?

- Does the proptest generate inputs in the same domain as the falsification?
- Does the assertion check the same predicate as the falsification describes?
- Is the generator's domain broad enough? (If falsification mentions
  "any sequence of operations" but proptest only generates single operations,
  the test is weaker than the claim)
- Are case counts adequate? (≥10K for algebraic laws per AGENTS.md)

### Lens 5: Lean ↔ Level 0 Correspondence

Does the Lean theorem prove the same property as Level 0?

- Is the theorem statement logically equivalent to Level 0 over the Lean model?
- Does the proof use the correct definitions from `00-preamble.md §23.0.4`?
- If the proof has `sorry`: is there a tracking bead? What is the blocker?
- Is the theorem vacuously true? (Preconditions that can never be satisfied
  make the theorem true but meaningless)
- Is it a tautology? (e.g., `∀ x: f(x) = f(x)` proves nothing about f's behavior)

### Lens 6: Stage ↔ Completeness Consistency

Is the invariant's completeness appropriate for its stage?

| Stage | Level 2 | proptest | Lean | Expected |
|-------|---------|----------|------|----------|
| 0 | Required | Required | Required (sorry OK with bead) | Fully specified |
| 1 | `todo!()` OK | Optional | `sorry` OK with bead | Structure present, detail deferred |
| 2 | Optional | Optional | Optional | Placeholder acceptable |

A Stage 0 invariant with missing proptest is a gap.
A Stage 1 invariant with `todo!("Phase 4b")` is expected — not a gap.

### Lens 7: Internal Contradiction

Does this invariant contradict any other invariant in the same or adjacent section?

- Could both invariants be simultaneously satisfied?
- Do they make incompatible claims about the same operation?
- Do they impose incompatible constraints on the same type?

### Output: Audit Finding Register

For each finding:

```markdown
### FINDING-NNN: <one-line description>

**Location**: `spec/NN-section.md`, INV-FERR-NNN, <field>
**Lens**: <which audit lens caught this>
**Severity**: CRITICAL | MAJOR | MINOR
**Evidence**: <what you observed>
**Expected**: <what the lab-grade standard requires>
**Fix**: <concrete remediation>
```

Severity levels:
- **CRITICAL**: Algebraic error, internal contradiction, or Lean/Level0 mismatch.
  The invariant may state something false.
- **MAJOR**: Missing verification layer for a Stage 0 invariant, weak falsification,
  proptest that doesn't test the claimed property.
- **MINOR**: Missing back-reference, formatting inconsistency, weak Level 1 description.

---

## Phase 4: Remediation (Low DoF)

**Objective**: Fix findings in severity order. Every change must be justified
by a specific finding from Phase 3.

### Execution Order

1. **CRITICAL findings first.** Algebraic errors and contradictions could
   invalidate downstream proofs and implementations.
2. **MAJOR findings.** Missing layers and weak falsifications.
3. **MINOR findings.** Cross-references and formatting.

### Remediation Protocol

For each finding:

1. Read the invariant in full
2. Read any referenced upstream invariants
3. Apply the fix described in the finding
4. Re-check the fixed invariant against the lens that caught it
5. Verify no new cross-reference breaks were introduced

### Adding Missing Layers

When adding a missing proptest, Lean theorem, or falsification to an existing
invariant, follow the authoring protocol from [16-spec-authoring.md](16-spec-authoring.md):

- proptest: derive from the falsification condition
- Lean: translate Level 0 into Lean 4 using `00-preamble.md §23.0.4` definitions
- Falsification: negate Level 0

### Updating spec/README.md

If the audit changes the invariant count, ADR count, or NEG count, update
`spec/README.md` accordingly.

---

## Phase 5: Convergence Verification (Structured DoF)

After remediation, run the five-lens convergence protocol from
[16-spec-authoring.md](16-spec-authoring.md) on the remediated section:

1. **Completeness**: Every invariant has all 6 layers
2. **Soundness**: Proof sketches cite mechanisms
3. **Simplicity**: No unnecessary complexity
4. **Adversarial**: Falsifications are strong
5. **Traceability**: Cross-references are bidirectional

The section has converged when a pass produces zero structural changes.

### Final Verification

```bash
# Verify spec/README.md counts are accurate
grep -c "### INV-FERR" spec/<target-section>.md
grep -c "### ADR-FERR" spec/<target-section>.md
grep -c "### NEG-FERR" spec/<target-section>.md

# Verify no broken INV-FERR references
grep -oP "INV-FERR-\d+" spec/<target-section>.md | sort -u
# Cross-check: every referenced ID exists somewhere in spec/
```

---

## Phase 6: Summary (Low DoF)

### Audit Report

```markdown
## Spec Audit Report — <section name> — YYYY-MM-DD

**Scope**: spec/<file>.md (INV-FERR-NNN through INV-FERR-MMM)
**Reviewer**: <model>

### Inventory
- Invariants audited: N
- ADRs audited: N
- NEGs audited: N

### Findings
- CRITICAL: N
- MAJOR: N
- MINOR: N

### Remediation
- Findings fixed: N/M
- Layers added: N (proptest: X, Lean: Y, falsification: Z)
- Cross-references repaired: N
- Findings deferred (with bead): N

### Quality Assessment
- Lab-grade invariants: N/M (before → after)
- Average layer completeness: N/6 (before → after)
- Cross-reference integrity: PASS/FAIL

### Remaining Gaps
<List any unfixed findings with bead IDs>
```

---

## Demonstration: Auditing One Invariant

**Target**: INV-FERR-025 (Index Backend Interchangeability) in `spec/03-performance.md`

### Phase 1 (inventory):

| Field | Present? |
|-------|---------|
| Traces-to | ✓ (C8, ADRS SR-001/SR-002 — but SR-001/SR-002 don't exist in FERR namespace) |
| Verification | ✓ (V:TYPE, V:PROP) |
| Stage | ✓ (0) |
| Level 0 | ✓ |
| Level 0 proof | — (Level 0 states the property but has no proof sketch) |
| Level 1 | ✓ |
| Level 2 | ✓ |
| Falsification | — |
| proptest | — |
| Lean | — |
| **Gaps** | **4** (no proof sketch, no falsification, no proptest, no Lean) |

### Phase 3 (deep audit):

**Lens 1 (Soundness)**: Level 0 states `∀ B₁, B₂ implementing IndexBackend:
result(ops, B₁) = result(ops, B₂)`. But the proof mechanism is unstated.
The property holds because all backends implement the same trait contract,
but this needs to be cited explicitly: "By trait contract: all implementations
satisfy the IndexBackend specification. Behavioral equivalence follows from
the trait being the only interface through which backends are accessed."

**Lens 2 (L0↔L2)**: Level 2 provides the trait definition and a BTreeMapBackend
impl, but doesn't show a SECOND backend to verify the equivalence claim.
The Level 0 quantifies over all pairs of backends; Level 2 demonstrates only one.

**Lens 3 (Falsification)**: Missing. Should be: "Any two IndexBackend
implementations `B₁, B₂` and operation sequence `ops` where `result(ops, B₁) ≠ result(ops, B₂)`."

**Lens 5 (Lean)**: Missing. Would require modeling IndexBackend as a structure
with axioms — non-trivial. Acceptable as `sorry` for Stage 0 with tracking bead.

### Findings:

```
FINDING-001: INV-FERR-025 Level 0 has no proof sketch
Location: spec/03-performance.md, INV-FERR-025, Level 0
Lens: 1 (Algebraic Soundness)
Severity: MAJOR
Evidence: Level 0 states the property but offers no mechanism.
Expected: Proof sketch citing trait contract as the mechanism.
Fix: Add proof sketch: "By trait contract adherence: all implementations
satisfy the IndexBackend specification, which fully determines observable
behavior. The trait interface is the ONLY access path to backend state,
preventing backend-specific behavior from leaking."

FINDING-002: INV-FERR-025 missing falsification, proptest, and Lean
Location: spec/03-performance.md, INV-FERR-025, multiple fields
Lens: 6 (Stage ↔ Completeness)
Severity: MAJOR (Stage 0 invariant with 3 missing layers)
Evidence: Stage 0 invariant with only Level 0/1/2. No falsification,
no proptest, no Lean.
Fix: Add all three layers per 16-spec-authoring protocol.

FINDING-003: INV-FERR-025 traces to non-existent ADRS SR-001/SR-002
Location: spec/03-performance.md, INV-FERR-025, Traces-to
Lens: Cross-reference integrity (Phase 2)
Severity: MINOR
Evidence: "ADRS SR-001, SR-002" — these IDs are not in the FERR namespace.
Likely stale references to an upstream spec.
Fix: Replace with the correct FERR-namespace references or remove.
```

---

## Integration with Other Prompts

| Situation | Follow-up prompt |
|-----------|-----------------|
| Audit finds missing invariants (spec gap) | [16-spec-authoring.md](16-spec-authoring.md) |
| Audit finds code-spec contradiction | [12-deep-analysis.md](12-deep-analysis.md) |
| Audit finds missing Lean proofs | [02-lean-proofs.md](02-lean-proofs.md) |
| Audit finds missing tests | [03-test-suite.md](03-test-suite.md) |
| Audit complete, need progress assessment | [13-progress-review.md](13-progress-review.md) |
| Findings need to be filed as beads | [08-task-creation.md](08-task-creation.md) |

---

## What NOT To Do

- Do not audit without reading the gold standard first. Re-read INV-FERR-001
  in `spec/01-core-invariants.md` before every audit session. It calibrates
  your quality expectations.
- Do not accept "obvious" or "by construction" as proof sketches. Every proof
  must name the mechanism. If the mechanism is truly obvious, naming it costs
  one sentence. If it's not, the proof is incomplete.
- Do not accept proptest strategies that test weaker properties than Level 0
  states. A proptest for merge commutativity that only tests stores with ≤3
  datoms is weaker than the ∀ in Level 0. Note the weakness as a finding.
- Do not accept Lean theorems that are vacuously true or tautological. A theorem
  with preconditions that can never be satisfied proves nothing. A theorem that
  says `f(x) = f(x)` proves nothing about f's actual behavior.
- Do not fix findings without tracing back to the primary source. If you're
  adding a falsification condition, derive it from the Level 0 law's negation —
  do not invent one.
- Do not audit spec content without checking the implementation. A spec invariant
  that is structurally perfect but contradicted by the actual code is worse than
  one with missing fields — it provides false confidence.
- Do not conflate Stage 1 deferrals with Stage 0 gaps. `todo!("Phase 4b")` in a
  Stage 1 Level 2 is expected and correct. The same `todo!()` in a Stage 0
  Level 2 is a finding.
