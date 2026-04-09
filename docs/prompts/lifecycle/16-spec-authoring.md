# 16 Spec Authoring — Writing Invariants, ADRs, and Spec Sections

> **Purpose**: Author new specification content — INV-FERR invariants, ADR-FERR
> decisions, NEG-FERR negative requirements, and complete spec sections — at the
> quality level established by `spec/01-core-invariants.md`. Every invariant is
> born with all six verification layers populated. No aspirational claims.
>
> **DoF**: High (formalization) → Low (mechanization). Discover the algebraic
> structure first, then mechanize it into the spec template.
>
> **Cognitive mode**: Formal specification. You are writing the source of truth
> that all downstream artifacts — Lean proofs, tests, types, implementation — will
> be derived from. Errors here propagate through the entire refinement tower.
>
> **Model gate**: Opus 4.6 with /effort max or GPT 5.4 xhigh. Spec authoring
> requires sustained formal reasoning across algebraic, operational, and
> implementation domains simultaneously.

---

## When to Use This Prompt

- A new phase requires new invariants (e.g., Phase 4b prolly tree, Phase 4c federation)
- A cleanroom review discovers a spec gap (behavior exists in code but not in spec)
- An existing invariant needs Level 2 expansion (Level 0/1 exist but Level 2 is `todo!()`)
- A new ADR must be recorded
- A design decision creates a new negative requirement

**This prompt produces spec content** — markdown in `spec/NN-section.md`. It does
not produce code, tests, or proofs. Those are downstream artifacts created by
[02-lean-proofs.md](02-lean-proofs.md), [03-test-suite.md](03-test-suite.md),
and [05-implementation.md](05-implementation.md).

---

## Phase 0: Ground Yourself

```bash
# Orientation
cat AGENTS.md
cat spec/README.md

# Methodology
ms load spec-first-design -m --full

# Understand the target section
cat spec/<target-section>.md

# What work is this serving?
br show <bead-id>
bv --robot-next
```

**Checkpoint**: Before authoring any spec content, you must be able to answer:
- What upstream trace does this invariant refine? (SEED.md §N, INV-STORE-NNN, C-N)
- What algebraic structure governs this domain?
- What existing INV-FERR are adjacent? (Which invariants does this one depend on or enable?)
- What is the next available INV-FERR ID? (Read the spec section to determine)

---

## The INV-FERR Template

This is the gold standard, extracted from `spec/01-core-invariants.md`. Every
new invariant must have ALL of these fields. An invariant missing any field is
incomplete — it fails the convergence protocol.

```markdown
### INV-FERR-NNN: Name

**Traces to**: <upstream: SEED.md §N, C-N, L-N, INV-STORE-NNN, INV-FERR-NNN>
**Verification**: <layers: `V:PROP`, `V:KANI`, `V:LEAN`, `V:MODEL`, `V:TYPE`>
**Stage**: <0 (MVP) | 1 (Production) | 2 (Future)>

#### Level 0 (Algebraic Law)
```
∀ ...:
  <formal predicate>

Proof: <algebraic proof sketch citing transition rules or set-theoretic
       properties. NOT "by construction" or "obvious" — state the mechanism.>
```

#### Level 1 (State Invariant)
<Operational meaning for all reachable states. What this invariant means
in practice. Why it matters. What would go wrong without it. 3-6 sentences.>

#### Level 2 (Implementation Contract)
```rust
<Rust code showing the contract. Use BTreeSet/BTreeMap as conceptual
illustrations per spec/README.md note. Include Kani harness if V:KANI.>
```

**Falsification**: <Specific counterexample shape. "Any X such that Y."
Describe the CONCRETE input that would violate this invariant. An invariant
that cannot be falsified is not an invariant — it's a wish.>

**proptest strategy**:
```rust
<Complete proptest code block using generators from ferratomic-verify.
Must actually test the falsification condition, not a weaker property.>
```

**Lean theorem**:
```lean
<Lean 4 theorem statement + proof (or `sorry` with tracking bead ID).
Must correspond to the Level 0 algebraic law, not a different property.>
```
```

### Field-by-Field Guidance

**Traces to**: Follow the chain upward. Every INV-FERR refines something.
- If it refines a STORE lattice law → cite L-N and INV-STORE-NNN
- If it refines a constraint → cite C-N
- If it refines a SEED.md design commitment → cite SEED.md §N
- If it refines another INV-FERR → cite INV-FERR-NNN
- Bidirectional: also add a cross-reference in the upstream invariant's section

**Verification layers**: Only list layers you are providing content for.
If there is no Stateright model for this invariant, do not list `V:MODEL`.
If you write a `sorry` in Lean, still list `V:LEAN` but note the sorry
in the theorem block.
New verification tags from GOALS.md §6: `V:MIRI` (MIRI UB detection),
`V:FUZZ` (fuzz testing), `V:MUTANT` (mutation testing), `V:FAULT`
(FaultInjectingBackend). Use these when the invariant touches unsafe
boundaries, deserialization, or durability paths.

**Stage**: 0 = required for current phase (Phase 4a). 1 = required for
production (Phase 4b+). 2 = designed now, implemented later. Stage determines
whether missing Level 2 is a gap (Stage 0) or an expected deferral (Stage 1+).

**Level 0**: The algebraic law. This is the mathematical statement that must
hold. Use standard notation (∀, ∃, ∈, ⊆, ∪, etc.). The proof sketch must
cite the specific mechanism — which transition rules preserve the property,
which set-theoretic identity applies, or which algebraic structure guarantees it.

**Level 1**: The operational interpretation. Translate the math into what it
means for running systems. Include: what reachable states are covered, what
failure modes it prevents, what other invariants depend on it, and what would
go wrong in production if this invariant were violated.

**Level 2**: The Rust contract. This is conceptual — it uses `BTreeSet`/`BTreeMap`
per the spec README note (actual implementation uses `im::OrdSet`/`im::OrdMap`
per ADR-FERR-001). Include the function signature, key logic, and a Kani
harness if `V:KANI` is listed. For future-phase invariants, use
`todo!("Phase Nb")` in the body — but still provide the type signature.

**Falsification**: The counterexample shape. "Any pair of stores `(A, B)` where..."
This must be specific enough that a proptest generator could search for it.
If you cannot describe a concrete falsification, the invariant is unfalsifiable.

**proptest strategy**: Working code using generators from `ferratomic-verify/src/generators.rs`.
Must test the falsification condition — if the falsification says "any X where
merge(A,B) ≠ merge(B,A)", the proptest must generate random A and B and assert
equality. Weak strategies (testing a subset of the property) are worse than
missing strategies (they provide false confidence).

**Lean theorem**: Must correspond 1:1 to the Level 0 algebraic law. If Level 0
says `∀ A,B: merge(A,B) = merge(B,A)`, the Lean theorem must prove exactly that
on `DatomStore := Finset Datom`, using the definitions from `spec/00-preamble.md §23.0.4`.

---

## The ADR-FERR Template

```markdown
### ADR-FERR-NNN: Name

**Traces to**: INV-FERR-NNN, INV-FERR-MMM
**Stage**: 0 | 1 | 2

**Problem**: <What needs to be decided. One paragraph.>

**Options**:

| Option | Description | Pros | Cons |
|--------|-------------|------|------|
| A: Name | <description> | <pros> | <cons> |
| B: Name | <description> | <pros> | <cons> |
| C: Name | <description> | <pros> | <cons> |

**Decision**: **Option X: Name**

<Rationale. Why this option. Which invariants it enables or preserves.
Quantified tradeoffs where possible.>

**Rejected**: <Why each alternative was rejected. Specific, not vague.>

**Consequence**: <Impact on implementation. What changes in the codebase.
Which modules are affected. What new constraints apply.>

**Source**: <Trace to SEED.md or upstream spec.>
```

---

## The NEG-FERR Template

```markdown
### NEG-FERR-NNN: Name

**Traces to**: INV-FERR-NNN
**Stage**: 0 | 1 | 2

<What the system must NOT do. State as a universal negative:
"No operation ever..." or "Under no circumstances does..."
Include the operational consequence of violation.>
```

---

## Authoring Protocol

### Step 1: Identify the Upstream Trace (High DoF)

Before writing any spec text, answer: "What algebraic structure governs this?"

```bash
# Read the upstream specification this invariant refines
cat spec/00-preamble.md    # Constraints, lattice laws, STORE mapping
cat spec/<relevant-section>.md  # Adjacent invariants
```

Identify:
- Which constraint (C1-C8) does this invariant serve?
- Which lattice law (L1-L5) does it preserve?
- Which existing INV-FERR does it extend, enable, or depend on?
- What algebraic property is being formalized?

### Step 2: Formalize Level 0 (High DoF)

Write the algebraic law FIRST. This is the anchor — Level 1 and Level 2
are derived from it, not the other way around.

Ask: "What is the simplest mathematical statement that captures this property?"

- Use set-theoretic notation where the property is about sets
- Use order-theoretic notation where the property is about ordering
- Use function-theoretic notation where the property is about mappings
- State the proof mechanism (not "by construction" — which specific property?)

### Step 3: Derive Level 1 (Structured DoF)

Translate Level 0 into operational meaning. Ask:

- "What does this look like in a running system?"
- "What goes wrong if this invariant is violated?"
- "Which other invariants depend on this one?"
- "Under what conditions might a naive implementation violate this?"

### Step 4: Specify Level 2 (Low DoF)

Write the Rust contract. This is mechanical — derive it from Level 0/1.

- Function signature: what types, what parameters
- Core logic: how the property is maintained
- Kani harness: bounded model checking of the property
- For future-phase invariants: provide the signature, `todo!()` the body

### Step 5: Write Falsification (Structured DoF)

Ask: "What input would prove this invariant wrong?"

The falsification must be:
- **Specific**: not "some invalid input" but "any pair (A, B) where property P fails"
- **Searchable**: a proptest generator could look for it
- **Diagnostic**: if found, it tells you WHAT is wrong (not just that something is)

### Step 6: Write proptest Strategy (Low DoF)

Mechanical derivation from the falsification condition. The proptest searches
for the counterexample described in the falsification. Use generators from
`ferratomic-verify/src/generators.rs`.

### Step 7: Write Lean Theorem (Low DoF)

Mechanical translation of Level 0 into Lean 4. Use the definitions from
`spec/00-preamble.md §23.0.4` (`DatomStore := Finset Datom`, `merge := union`).

- If the proof is straightforward (direct application of mathlib): provide it
- If the proof is non-trivial: provide the statement with `sorry` and file
  a bead for the proof: `br create --title "Prove INV-FERR-NNN in Lean" --type task`

### Step 8: Add Cross-References (Low DoF)

- Update the Traces-to field with all upstream references
- Add a back-reference in any invariant this one extends or depends on
- Update `spec/README.md` if the INV count changed
- Verify the INV-FERR ID doesn't collide with existing IDs

### Six-Dimension Scoring (GOALS.md §7)

Before finalizing any non-trivial invariant, ADR, or spec section, score it across all six dimensions (1-10 each). Document the composite. Any dimension below 7.0 → reconsider. Correctness below 10.0 → cannot ship. See GOALS.md §7.4 for a worked example (INV-FERR-049 rewrite: pre/post scores across all six dimensions).

---

## The Five-Lens Convergence Protocol

After drafting, review the new spec content through five sequential single-lens
passes. One lens per pass. Do not combine.

### Lens 1: Completeness
> "What fields are missing?"

Check every INV-FERR against the template. Every field must be populated.
`sorry` and `todo!("Phase Nb")` are acceptable for Stage 1+ invariants —
but only if a tracking bead exists.

### Lens 2: Soundness
> "Are the proof sketches actually correct?"

For each Level 0 proof: does the cited mechanism actually preserve the property?
Could you construct a counterexample that the proof sketch doesn't account for?
Is the Lean theorem statement logically equivalent to the Level 0 law?

### Lens 3: Simplicity
> "Is this the simplest mathematical structure that works?"

Could the Level 0 be stated more concisely? Is the algebraic structure
correctly identified — or over-complicated? Does Level 2 introduce unnecessary
implementation detail beyond what the contract requires?

### Lens 4: Adversarial
> "How would an adversary break each invariant?"

For each falsification condition: is it truly the strongest counterexample?
Could a weaker violation exist that the falsification doesn't cover? Does the
proptest strategy actually search the right space?

### Lens 5: Traceability
> "Does every thread trace through every layer?"

INV-FERR → Level 0 → Level 1 → Level 2 → falsification → proptest → Lean.
Is this chain unbroken? Does the proptest test the same property the Lean
theorem proves? Does the falsification match the Level 0 law's negation?

**Converged** when a pass produces zero structural changes.

---

## Demonstration: Authoring INV-FERR-046a

Scenario: The bead `bd-400` requests "Add INV-FERR-046a: rolling hash
determinism and algorithm specification" for the prolly tree section.

### Step 1: Upstream trace

Reading `spec/06-prolly-tree.md`: INV-FERR-046 (History Independence) requires
that the same key-value set produces the same tree structure. This depends on
the chunk boundaries being deterministic. INV-FERR-046a formalizes that
sub-property: the rolling hash function that determines boundaries is
deterministic and algorithm-specified.

Upstream: INV-FERR-046, C2 (content-addressed), ADR-FERR-008 (prolly tree block store).

### Step 2: Level 0

```
∀ key_sequence K, ∀ implementations I₁ I₂ conforming to this spec:
  boundaries(I₁, K) = boundaries(I₂, K)

Where boundaries(I, K) is the set of positions in K where I's rolling
hash function produces a chunk boundary.

Proof: The rolling hash algorithm, window size, and boundary predicate are
fully specified (not implementation-defined). Given identical input bytes
in identical order, the hash state machine produces identical output.
Determinism follows from the function being pure (no hidden state beyond
the rolling window).
```

### Step 3: Level 1

The rolling hash function that determines prolly tree chunk boundaries must be
algorithm-specified — not implementation-defined or platform-dependent. Any two
conforming implementations processing the same sorted key sequence must produce
identical chunk boundary positions. This is the sub-property that makes
INV-FERR-046 (history independence) achievable: if boundaries vary between
implementations, the same key-value set produces different tree structures,
breaking content-addressing (INV-FERR-045) and O(d) diff (INV-FERR-047).

### Step 4: Level 2

```rust
/// Rolling hash for prolly tree chunk boundaries.
/// Algorithm: Buzhash with 32-byte window, 64-bit state.
/// Boundary predicate: `hash & MASK == 0` where MASK = (1 << B) - 1
/// and B is the target fan-out exponent (default: 8, giving ~256 keys/chunk).
///
/// INV-FERR-046a: This function is pure — same input always produces same output.
pub fn rolling_boundary(window: &[u8; 32]) -> bool {
    let hash = buzhash64(window);
    hash & BOUNDARY_MASK == 0
}

const BOUNDARY_MASK: u64 = (1 << 8) - 1; // ~256 keys per chunk average

#[kani::proof]
#[kani::unwind(5)]
fn rolling_hash_determinism() {
    let window: [u8; 32] = kani::any();
    let r1 = rolling_boundary(&window);
    let r2 = rolling_boundary(&window);
    assert_eq!(r1, r2); // pure function: same input → same output
}
```

### Step 5: Falsification

Any byte sequence `W` where `rolling_boundary(W)` returns different results
across two invocations, or across two conforming implementations. This would
indicate hidden state (mutable internal buffer, platform-dependent hash, or
non-deterministic tie-breaking). Concretely: find `W` such that
`rolling_boundary(W) != rolling_boundary(W)`.

### Step 6: proptest

```rust
proptest! {
    #[test]
    fn rolling_hash_deterministic(
        window in prop::array::uniform32(any::<u8>()),
    ) {
        let r1 = rolling_boundary(&window);
        let r2 = rolling_boundary(&window);
        prop_assert_eq!(r1, r2,
            "INV-FERR-046a: rolling hash must be deterministic");
    }
}
```

### Step 7: Lean

```lean
/-- The rolling hash boundary predicate is a pure function:
    same input always produces the same output. -/
theorem rolling_boundary_deterministic (w : Fin 32 → UInt8) :
    rolling_boundary w = rolling_boundary w := rfl
```

### Step 8: Cross-references

- INV-FERR-046a traces to: INV-FERR-046 (parent), C2, ADR-FERR-008
- INV-FERR-046 gains: "Sub-property INV-FERR-046a formalizes boundary determinism."

---

## Integration with Other Prompts

| Situation | Follow-up prompt |
|-----------|-----------------|
| Invariant authored, needs Lean proof beyond `sorry` | [02-lean-proofs.md](02-lean-proofs.md) |
| Invariant authored, needs tests | [03-test-suite.md](03-test-suite.md) |
| Invariant authored, needs implementation | [05-implementation.md](05-implementation.md) |
| Spec section complete, needs review | [17-spec-audit.md](17-spec-audit.md) |
| Spec gap found during authoring | [08-task-creation.md](08-task-creation.md) |
| Spec contradiction found | [12-deep-analysis.md](12-deep-analysis.md) |

---

## What NOT To Do

- Do not write Level 2 before Level 0. The algebraic law is the anchor.
  Implementation contracts derived from vague intentions produce code that
  "works" but doesn't provably satisfy anything.
- Do not write aspirational invariants. "The system is correct" with no proof
  sketch is a parasitic constraint — it consumes attention while contributing
  nothing. Every invariant must be falsifiable.
- Do not write a proof sketch that says "obvious" or "by construction." State
  the mechanism: which transition rule preserves it, which set identity applies,
  which algebraic property guarantees it.
- Do not write a proptest that tests a weaker property than the Level 0 law
  states. If Level 0 says "for all stores A, B" and the proptest only generates
  stores with < 5 datoms, the test provides false confidence.
- Do not write a Lean theorem that proves a different property than Level 0
  states. The Lean theorem and Level 0 must be logical equivalents over their
  respective models.
- Do not skip cross-references. Orphan invariants — those with no upstream
  trace and no downstream dependents — indicate either a spec gap (the
  invariant is real but disconnected) or spec bloat (the invariant is unnecessary).
- Do not author spec content for a phase without reading the entire existing
  section first. Context prevents ID collisions, contradictions, and redundancy.
- Do not edit spec files without verifying that the change reduces or maintains
  drift. New content must be consistent with all existing invariants.

**Knowledge Organization Rule**: Prescriptive content (invariants, ADRs, decisions) goes in `spec/` or `GOALS.md` — NEVER only in `docs/ideas/`. Idea docs may explore but must reference canonical sources as authoritative. See AGENTS.md Knowledge Organization Rule.
