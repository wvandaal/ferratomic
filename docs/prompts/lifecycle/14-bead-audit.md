# 14 Bead Audit & Specification Hardening

> **Purpose**: Systematically audit every open bead against primary sources and
> transform each into a lab-grade implementation specification. The end state is
> an issue graph where any bead can be handed to an agent with a clean context
> window and executed to zero-defect completion without a single clarifying question.
>
> **DoF**: Varies by phase. High (verification) → Structured (assessment) → Low (remediation).
>
> **Cognitive mode**: Adversarial verification, then surgical editing.
> Assume every bead is wrong until proven correct by primary source evidence.
>
> **Model gate**: Opus 4.6 with /effort max or GPT 5.4 xhigh. The per-bead
> verification against multiple primary sources demands sustained precision.
>
> **Execution constraint**: The auditing agent MUST perform all work itself,
> sequentially, one bead at a time. Do NOT delegate bead auditing to subagents.
> The entire value of this prompt is that a single agent accumulates cross-bead
> context — which invariants are referenced by multiple beads, which file sets
> overlap, which dependency edges are missing. Subagents lose this accumulated
> context and produce locally-correct but globally-inconsistent results. The
> Session 006 Kani incident (7 API drift bugs) originated from exactly this
> pattern: independent agents making locally-reasonable decisions that diverged
> from each other. The auditing agent IS the integration point.

---

## When to Use This Prompt

- After a swarm has completed a large body of work (beads may be stale)
- Before starting a new phase (ensure the task graph is clean)
- After a cleanroom review files many new defect beads (they need hardening)
- When velocity stalls and agents keep asking clarifying questions
- Periodic hygiene (e.g., before every phase gate decision)

**This prompt modifies beads but does NOT write code.** It produces a clean,
lab-grade issue graph. Code changes happen in follow-up sessions.

---

## The Lab-Grade Standard

A bead meets lab-grade standard when it satisfies this predicate:

> An agent loaded with only AGENTS.md, the referenced spec module, and this bead
> can execute the work to completion, verify its own output, and close the bead —
> without reading any other bead, without asking a clarifying question, and without
> making a judgment call about scope, approach, or correctness criteria.

This is a high bar. Most beads in most projects do not meet it. Every bead in
THIS project must.

### The Lab-Grade Bead Template

Every bead that passes audit conforms to this structure. Fields marked **[R]** are
required for all beads. Fields marked **[E]** are required for epics only.
Fields marked **[T]** are required for tasks and bugs only.

```markdown
## Title  [R]
Verb-first, specific, under 80 chars.
"Implement X" | "Fix Y" | "Prove Z" | "Test W"
Not "X improvements" | "Y stuff" | "Look into Z"

## Type  [R]
bug | task | feature | epic | docs

## Priority  [R]
P0-P4, calibrated to DOWNSTREAM IMPACT, not effort:
- P0: Invariant violation, data loss, blocks all progress
- P1: Spec divergence, blocks 2+ downstream beads
- P2: Quality gap, blocks 0-1 downstream beads
- P3: Cosmetic, no downstream impact
- P4: Backlog — valid but not yet scheduled

## Phase Label  [R]
phase-1 | phase-2 | phase-3 | phase-4a | phase-4b | phase-4c | phase-4d

---

## Specification Reference  [T]
The exact primary source that defines what "correct" means for this work.
Not a vague gesture — a specific, dereferenceable pointer.

- **Primary invariant**: INV-FERR-NNN
  - **Level cited**: 0 (algebraic law) | 1 (state invariant) | 2 (Rust contract)
  - **Spec file**: `spec/NN-section.md`, Section X.Y heading
- **Supporting**: ADR-FERR-NNN, NEG-FERR-NNN (if applicable)
- **Design doc**: `docs/design/FILE.md` (if applicable)

The agent reads AGENTS.md, the spec file at the cited section, and this bead.
That is the complete context. If the agent needs anything else, the bead is
underspecified.

## Preconditions  [T]
What must be true BEFORE this work begins. Each precondition is a verifiable
predicate, not a hope.

1. `<bead-id>` is closed (produces: <what it produces that this consumes>)
2. `<module/type>` exists at `<path>` (structural prerequisite)
3. `<spec section>` is finalized (no pending changes to the cited invariant)

If there are no preconditions, state "None — leaf task."

## Postconditions  [T]
What must be true AFTER this work completes. These are the acceptance criteria.
Every postcondition is:
- **Binary**: pass or fail, no "mostly" or "improved"
- **Verifiable**: by running a command, reading a file, or checking a predicate
- **Traced**: to a specific INV-FERR level or NEG-FERR

Format:
1. [INV-FERR-NNN] <predicate>. **Verify**: `<command or file check>`.
2. [NEG-FERR-NNN] <negative predicate>. **Verify**: `<command>`.
3. `cargo test --workspace` passes with 0 failures.
4. `cargo clippy --workspace -- -D warnings` produces 0 warnings.

The agent knows they are done when ALL postconditions hold simultaneously.

## Frame Conditions  [T]
What this work must NOT change. Critical for parallel agent execution.

1. <module/file> must not be modified (owned by another bead or agent)
2. <public API surface> must remain unchanged (no signature changes)
3. <existing tests> must continue passing (regression guard)
4. <invariant> must not be weakened (the fix must not break a stronger guarantee)

If there are no frame constraints, state "None — greenfield."

## Refinement Sketch  [T]
A refinement-calculus-style description connecting the spec to the implementation.
Not pseudocode — a statement of WHAT changes and WHY it satisfies the invariant.

- **Abstract** (spec says): <formal property in plain language or notation>
- **Concrete** (code must): <what the Rust implementation expresses>
- **Coupling** (verified by): <how to confirm the refinement preserves the invariant>

For bug-type beads, replace with:
- **Observed**: <current behavior with evidence>
- **Expected**: <correct behavior per INV-FERR-NNN>
- **Root cause**: <why the current code produces wrong behavior>
- **Fix**: <minimal change that restores the invariant>

## Pseudocode Contract  [T]
Exact Rust type definitions, function signatures, and enum dispatch patterns for
every type this bead introduces or modifies. This is NOT implementation — it is a
**type-level contract** that eliminates all agent judgment calls about ownership,
mutability, return types, visibility, and dispatch.

**The Compilability Test**: Read ONLY this section. Could you write a `.rs` file
with all the type definitions, struct fields, function signatures (with `todo!()`
bodies), and trait impls? If not, the contract is incomplete.

Why this matters: Agents that make "reasonable" type choices without full context
produce code that compiles but silently violates invariants. The Session 006 Kani
incident (7 API drift bugs from `cfg(kani)` gating) and the `Arc<PositionalStore>`
vs `PositionalStore` ownership question demonstrate this. A wrong choice in a
foundation bead propagates to every downstream bead.

Format:

```rust
// --- New types ---

/// <doc comment citing INV-FERR-NNN>
pub struct NewType {
    /// <field doc: what it represents, invariant it maintains>
    pub(crate) field_a: ExactType,     // specify Arc<T> vs T vs Box<T>
    field_b: OtherType,                 // private by default — state it
}

// --- Modified signatures ---

impl ExistingType {
    /// <what changed and why>
    /// Was: `pub fn method(&self) -> OldReturn`
    /// Now:
    pub fn method(&self) -> NewReturn {  // &self vs &mut self: specified
        todo!()
    }
}

// --- Enum dispatch ---

/// All match arms enumerated. Agent implements each arm, never adds/removes arms.
match value {
    Variant::A(inner) => { /* <what A does — one sentence> */ todo!() }
    Variant::B { field } => { /* <what B does> */ todo!() }
    Variant::C => { /* <what C does> */ todo!() }
    // NO wildcard `_ =>` — every variant named
}

// --- Trait impls ---

impl TraitName for NewType {
    fn trait_method(&self, param: ParamType) -> ReturnType { todo!() }
}
```

**5 rules — every bead that touches Rust types MUST resolve these:**

| Decision | Why it matters | Bead must specify |
|----------|---------------|-------------------|
| `Arc<T>` vs `T` vs `Box<T>` | Determines O(1) vs O(n) clone, shared vs exclusive ownership | Exact wrapper for every struct field and return type |
| `&self` vs `&mut self` | Determines whether method callable on shared references | Every new or changed method signature |
| Return type of changed methods | A method returning `&T` when `T` may not exist is a compile error | Every method whose return type changes or whose invariants change |
| `pub` vs `pub(crate)` vs private | Module boundary contract for downstream consumers | Every new struct, field, method, and function |
| Enum match arms | Missing arm = compile error; wrong arm = silent invariant violation | Every match/if-let on an enum this bead touches |

For beads that do not introduce or modify Rust types (e.g., docs, pure test beads,
Lean proofs), state "N/A — no type changes."

## Verification Plan  [T]
How to confirm the postconditions hold. Specific enough to execute mechanically.

1. **Test**: `test_inv_ferr_NNN_<description>` in `<path>`. Strategy: <unit/prop/integration>.
2. **Build**: `CARGO_TARGET_DIR=/data/cargo-target cargo check --workspace`
3. **Lint**: `CARGO_TARGET_DIR=/data/cargo-target cargo clippy --workspace -- -D warnings`
4. **Cross-check**: Read `spec/NN-section.md` INV-FERR-NNN Level 2 and confirm code matches.
5. **MIRI**: `cargo +nightly miri test <test_name>` (if bead touches unsafe or deserialization)
6. **Fuzz**: `cargo fuzz run <target> -- -max_total_time=60` (if bead touches parsing/deserialization)
7. **Full gates**: GOALS.md §6.8 (all 11 gates must pass)

## Files  [T]
Exact paths that will be created or modified, with a one-line summary of what
changes in each file. This defines the agent's blast radius.

- `<crate>/src/<module>.rs`: <what changes>
- `<crate>/tests/<test>.rs`: <new test or modified test>

An agent working on this bead touches ONLY these files. If the fix requires
touching other files, the bead's scope is wrong — split it.

## Dependencies  [R]
Bidirectional awareness of the task graph neighborhood.

- **Depends on**: `<bead-id>` — <what it produces that this consumes>
- **Blocks**: `<bead-id>` — <what this produces that those consume>
- If no dependencies: "Leaf task — no predecessors, no successors."

---

## Epic-Specific Fields  [E]

Epics do NOT have postconditions, files, or verification plans.
Epics are containers. Their children have the specifics.

## Child Beads
- `<bead-id>`: <title> (status: open|closed)
- `<bead-id>`: <title> (status: open|closed)

## Completion Criterion
This epic closes when ALL child beads are closed.

## Progress
- N/M children closed
- Current bottleneck: `<bead-id>` (<why it's the bottleneck>)
```

---

## Phase 0: Ground Yourself

```bash
# Orientation
cat AGENTS.md
cat spec/README.md

# Methodology
ms load spec-first-design -m --pack 2000

# Current bead graph state — record these as BEFORE metrics
bv --robot-triage
bv --robot-insights
bv --robot-alerts
bv --robot-suggest
bv --robot-priority

# Full bead inventory
br list --status=open
br list --status=closed | tail -20   # Recent closures for context
```

**Checkpoint**: Before proceeding, you must know:
- Total open bead count
- Priority distribution (P0/P1/P2/P3/P4)
- Type distribution (bug/task/feature/epic/docs)
- Cycle count (must be 0)
- Alert count
- The project's current phase and True North

---

## Phase 1: Primary Source Verification (High DoF)

**Objective**: For every open bead, verify every claim against primary sources.
Do not assess quality yet — just verify facts. This is forensic, not editorial.

### Protocol: Per-Bead Verification

For each open bead, in priority order (P0 first, then P1, etc.):

```bash
br show <id>
```

Then apply these 4 verification checks:

#### Check 1: Does the referenced code exist and match the description?

If the bead references a file, line number, function, or behavior:

```bash
# Read the referenced location
# Does it exist? Does it say what the bead claims?
```

Record: CONFIRMED | STALE (code changed) | INVALID (never existed) | COMPLETED (work already done)

#### Check 2: Does the referenced invariant exist and match?

If the bead cites INV-FERR-NNN, ADR-FERR-NNN, or NEG-FERR-NNN:

```bash
# Read the spec file containing the cited invariant
# Does the invariant say what the bead claims?
# Is the bead's proposed work actually what the invariant requires?
```

Record: CONFIRMED | MISMATCHED (bead misinterprets spec) | ABSENT (invariant doesn't exist)

#### Check 3: Do declared dependencies hold?

```bash
br show <dep-id>   # For each dependency
# Is the dependency still open? (If closed, the edge may be phantom)
# Is the dependency real? (Would this bead LITERALLY fail without it?)
```

Record per edge: VALID | PHANTOM (dep is closed/satisfied) | ASPIRATIONAL (nice ordering but not required)

#### Check 4: Is there duplicate or overlapping work?

```bash
# Search for beads with similar titles, same INV-FERR, or same file paths
br list --status=open | grep -i "<keyword>"
```

Record: UNIQUE | OVERLAPS-WITH <bead-id> (describe overlap)

### Output: Verification Register

A table with one row per open bead:

| Bead | Code Check | Spec Check | Dep Check | Dup Check | Verdict |
|------|-----------|-----------|----------|----------|---------|
| bd-xxx | CONFIRMED | CONFIRMED | 2 VALID, 1 PHANTOM | UNIQUE | SOUND |
| bd-yyy | STALE | MISMATCHED | 1 ASPIRATIONAL | OVERLAPS bd-zzz | NEEDS WORK |
| bd-zzz | COMPLETED | CONFIRMED | all VALID | UNIQUE | CLOSE |

Verdicts:
- **SOUND**: All checks pass. Proceed to Phase 2 quality assessment.
- **NEEDS WORK**: One or more checks failed. Will be remediated in Phase 3.
- **CLOSE**: Work is done or bead is invalid. Will be closed in Phase 3.
- **FLAG**: Uncertainty encountered. Will be escalated to human in Phase 3.

---

## Phase 2: Quality Assessment (Structured DoF)

**Objective**: For each SOUND bead from Phase 1, assess whether it meets the
lab-grade standard. For each NEEDS WORK bead, diagnose what's wrong.

### The 8 Audit Lenses

Apply each lens to each bead. Record pass/fail per lens.

#### Lens 0: Epistemic Fit

**Before checking completeness, ask: is this the RIGHT verification method for
this invariant?** A bead that prescribes the wrong method will produce work that
compiles but verifies nothing. This lens catches that BEFORE an agent wastes
time writing vacuous proofs or redundant tests.

For each verification method the bead prescribes, check whether the invariant's
algebraic structure matches the method's domain of validity:

| Method | Domain of validity | NOT valid for |
|--------|-------------------|---------------|
| **Lean** | Algebraic properties of Finset/set operations: commutativity, monotonicity, subset, cardinality preservation, homomorphism. Properties expressible as equalities or inclusions on the abstract `DatomStore := Finset Datom` model. | Type system properties (unsafe, Result), performance thresholds (latency, write amplification), crash non-determinism (which WAL entries survive), rate limiting (backpressure), architecture properties (substrate agnosticism). |
| **Stateright** | Properties under concurrent/crash interleavings: convergence under message reordering, recovery correctness under crash timing, state machine safety and liveness. Properties that require exploring ALL action sequences. | Algebraic identities provable by Finset rewriting (use Lean). Properties of a single deterministic computation (use proptest). |
| **Kani** | Bounded model checking of Rust code paths: exhaustive verification of small input spaces, path coverage for error handling, contract verification with `kani::any()`. | Properties that require unbounded inputs, real I/O, or timing. Properties already provable by Lean at the algebraic level (Kani adds implementation-level confidence, not algebraic proof). |
| **Proptest** | Statistical confidence on concrete Rust implementations: 10K+ random inputs verify the implementation matches the spec's algebraic law. The conformance bridge between Lean model and Rust code. | Universal proofs (use Lean). Properties expressible as `A ∪ B = B ∪ A` where Finset.union_comm is a one-liner — don't spend 10K random cases on a rewriting identity. |
| **V:TYPE** | Rust compiler enforces it: trait bounds, `Result<T,E>` totality, safe callable surface per GOALS.md §6.2, type-state patterns. Zero runtime verification needed — the compiler IS the verifier. | Runtime behavior, algebraic properties, performance characteristics. |
| **V:MIRI** | Undefined behavior detection | When bead touches unsafe code, FFI boundaries, or pointer arithmetic | `cargo +nightly miri test` |
| **V:FUZZ** | Edge case discovery via coverage-guided fuzzing | Deserialization, WAL parsing, checkpoint loading, wire type decoding | `cargo fuzz run <target> -- -max_total_time=60` |
| **V:MUTANT** | Test strength verification via mutation analysis | Any bead with proptest coverage — verifies assertions catch defects | `cargo mutants --file <path>` |
| **V:FAULT** | Adversarial storage fault tolerance | Durability, recovery, checkpoint integrity under TornWrite/PowerCut/IoError/DiskFull/BitFlip | FaultInjectingBackend in proptest |

**A bead FAILS epistemic fit if:**
- It prescribes Lean for a property the Finset model cannot encode (rate limiting,
  crash timing, type system enforcement). The resulting theorem would be vacuously
  true or trivially `rfl` on identity functions.
- It prescribes Stateright for a pure algebraic identity (merge commutativity).
  The model checker would explore millions of states to verify what `Finset.union_comm`
  proves in one line.
- It prescribes proptest for a property already proven universally by Lean.
  Statistical confidence is weaker than a proof — use proptest for the
  **conformance bridge** (Lean predicts, Rust confirms), not as a substitute.
- It prescribes Kani for a property that requires unbounded reasoning or real I/O.
- It prescribes V:TYPE for a runtime behavioral property that the compiler cannot check.

**Evidence from this project**: Session 011 audited 9 "Lean proof" beads and found
7 were epistemically wrong. They would have produced compilable-but-vacuous theorems:
- INV-FERR-019 (error exhaustiveness): Lean can't verify Rust's type system
- INV-FERR-021 (backpressure): Lean has no concept of rate or time
- INV-FERR-023 (no unsafe): Lean can't verify the GOALS.md §6.2 containment policy
- INV-FERR-024 (substrate agnosticism): vacuously true — Finset IS substrate-agnostic

The correct action for a mismatched bead: reclassify it to the RIGHT method,
or close it if the property is already verified by the correct method elsewhere.

#### Lens 1: Structural Completeness

Does the bead have all required fields from the lab-grade template?

- [ ] Title: verb-first, specific, <80 chars
- [ ] Type: correct (bug=broken behavior, task=new work, feature=new capability)
- [ ] Priority: calibrated to downstream impact
- [ ] Phase label: matches current project phase
- [ ] Specification reference: exact INV-FERR with Level cited
- [ ] Preconditions: verifiable predicates
- [ ] Postconditions: binary, verifiable, INV-traced
- [ ] Frame conditions: stated (even if "none — greenfield")
- [ ] Refinement sketch or bug analysis
- [ ] Pseudocode Contract (if bead introduces/modifies Rust types) or "N/A"
- [ ] Verification plan: specific test names, commands
- [ ] Files: exact paths with change descriptions
- [ ] Dependencies: bidirectional (depends-on AND blocks)

Each missing field is a finding. Count the gaps.

#### Lens 2: Specification Traceability

Every bead must trace to a primary source. The chain must be unbroken:

```
Bead → INV-FERR-NNN (specific Level) → spec/NN-section.md → algebraic law
```

If ANY link in this chain is missing, the bead is ungrounded. An ungrounded
bead is a bead where "correct" is a matter of opinion rather than proof.

Check: Can you follow the chain from the bead to an algebraic law in < 3 hops?

#### Lens 3: Postcondition Strength

Are the postconditions strong enough to verify the work?

- **Strong**: "INV-FERR-005: for every datom in the primary set, all 4 secondary
  indexes contain that datom. Verify: `test_inv_ferr_005_index_bijection` passes."
- **Weak**: "Indexes are correct." (How? Correct per what definition? Which test?)
- **Absent**: No postconditions at all.

A postcondition is strong when an agent can write the test FROM the postcondition
alone, without reading any other context.

**Performance-to-type tracing**: Postconditions that reference performance
characteristics MUST trace to a specific type choice in the Pseudocode Contract.
A performance claim without a type anchor is unverifiable:

- **Strong**: "O(1) snapshot via `Arc::clone` (INV-FERR-006). The `Arc<StoreInner>`
  in the Pseudocode Contract guarantees this — `Arc::clone` is a reference count
  increment, not a data copy."
- **Weak**: "O(1) snapshot." (How? Clone semantics depend on whether the field is
  `Arc<T>`, `Box<T>`, or `T` — each has different clone cost.)
- **Strong**: "O(log n) lookup with ~4 cache misses (INV-FERR-071). The
  `Vec<(K, V)>` in the Pseudocode Contract guarantees contiguous memory layout."
- **Weak**: "Fast lookup." (Fast compared to what? Depends on the data structure.)

#### Lens 4: Scope Atomicity

Can one agent complete this in one focused session?

Heuristics:
- Touches <= 3 files: likely atomic
- Touches 4-6 files: borderline — verify they're all in one module
- Touches 7+ files: almost certainly needs splitting
- Has > 8 postconditions: likely multiple concerns bundled
- Description contains "and also" or "additionally": likely needs splitting
- Is labeled "epic" but has no children: needs decomposition

#### Lens 5: Frame Condition Adequacy

For parallel agent execution, frame conditions prevent collisions.

- Does the bead name specific files it will NOT touch?
- If another open bead modifies the same file, is there an explicit ordering?
- Could an agent working on this bead accidentally break another agent's work?

Cross-reference: check all other open beads for file overlap.

#### Lens 6: The Compiler Test (Pseudocode Contract Verification)

For every bead that introduces or modifies Rust types, apply all 6 sub-checks.
A bead **FAILS** if an implementing agent must make ANY of the following choices —
each is a judgment call that risks silent invariant violation.

**Sub-check 6a: Type Resolution**
Every struct field, function parameter, and return type is FULLY SPECIFIED with
exact types. No placeholders, no "appropriate type", no "similar to X".

- PASS: `field: Arc<RwLock<PositionalStore>>` — no ambiguity
- FAIL: `field: PositionalStore` when the bead doesn't state whether shared
  ownership is required — agent must decide Arc vs owned vs Box

**Sub-check 6b: Signature Resolution**
Every new or modified function signature specifies: receiver (`&self`, `&mut self`,
`self`, or none), all parameters with types, return type, and error type.

- PASS: `pub(crate) fn promote(&mut self) -> Result<(), FerraError>`
- FAIL: `fn promote(...)` — agent must decide mutability, visibility, error handling

**Sub-check 6c: Match Pattern Completeness**
Every `match`, `if let`, or pattern-dispatch that the bead's code will touch has
ALL arms enumerated with a one-sentence description of what each arm does.

- PASS: `AdaptiveIndexes::SortedVec(sv) => { /* promote to OrdMap */ }`
  `AdaptiveIndexes::OrdMap(om) => { /* already promoted, no-op */ }`
- FAIL: "Handle all variants of AdaptiveIndexes" — agent must discover the variants

**Sub-check 6d: Lifetime Resolution**
If any signature involves borrowed data (`&`, `&mut`, named lifetimes), the bead
specifies the lifetime relationship. If the bead introduces a struct with references,
it states the lifetime parameter.

- PASS: `pub fn datom_at<'a>(&'a self, pos: u32) -> &'a Datom`
- FAIL: `fn datom_at(&self, pos: u32) -> &Datom` when elision is ambiguous

**Sub-check 6e: API Compatibility**
If the bead modifies an existing function's signature or return type, it states
the OLD signature and the NEW signature. Any callers affected by the change are
listed in the Files section.

- PASS: `Was: pub fn indexes(&self) -> &Indexes`
  `Now: pub fn indexes(&self) -> Option<&Indexes>` — callers: `store/query.rs`, `store/merge.rs`
- FAIL: "Change indexes() to handle the case where indexes don't exist" — agent
  must choose the return type and find all callers

**Sub-check 6f: Module Wiring**
If the bead creates a new module or type, it states: which module declares it,
which modules import it, and whether it's re-exported from `lib.rs`.

- PASS: "Declare `PositionalStore` in `ferratomic-positional/src/lib.rs`.
  Re-export from crate root. Import in `ferratomic-store/src/mod.rs`."
- FAIL: "Add the PositionalStore type" — agent must decide where to put it

For beads that do not touch Rust types (docs, Lean proofs, pure config changes),
Lens 6 is automatically PASS — note "N/A: no type changes."

#### Lens 7: Axiological Alignment

Does this bead serve True North?

> Ferratomic provides the universal substrate: an append-only datom store with
> content-addressed identity, CRDT merge, indexed random access, and
> cloud-scale distribution.

- Does the work strengthen a named invariant?
- Does the work directly advance the current phase?
- Is there a credible path from this bead to a user-visible capability?

**GOALS.md §7 check**: Design-decision beads must have a six-dimension score. Missing score = finding.

A bead that passes all 8 lenses is lab-grade. Record the lens results.

### Output: Quality Register

Extend the Phase 1 table with lens results:

| Bead | Verdict | L0 | L1 | L2 | L3 | L4 | L5 | L6 | L7 | Gap Count | Action |
|------|---------|----|----|----|----|----|----|----|----|---------|--------|--------|
| bd-xxx | SOUND | P | P | P | P | P | P | P | P | 0 | NONE |
| bd-yyy | NEEDS WORK | **F** | F | P | F | P | F | F | P | 5 | RECLASSIFY |
| bd-zzz | CLOSE | — | — | — | — | — | — | — | — | — | CLOSE |

Action categories:
- **NONE**: Lab-grade. No changes needed.
- **EDIT**: 1-2 lens failures. Add missing fields.
- **REWRITE**: 3+ lens failures. Rebuild from the lab-grade template.
- **RECLASSIFY**: Lens 0 failure. The bead prescribes the wrong verification method. Change the method to match the invariant's epistemic domain, or close if the property is already verified elsewhere by the correct method.
- **SPLIT**: Lens 4 failure. Decompose into atomic sub-beads.
- **MERGE**: Overlapping with another bead. Consolidate.
- **CLOSE**: Work done, invalid, or duplicate.
- **FLAG**: Requires human judgment (see uncertainty protocol).

---

## Phase 3: Reconciliation (Low DoF)

**Objective**: Execute changes in a precise order. Every change is justified by
evidence from Phases 1-2. No editorial discretion — the findings dictate the actions.

### Execution Order

Process in this exact order. Each step reduces noise before the next adds precision.

#### Step 1: Deduplicate

Before any other reconciliation, identify and resolve duplicate beads. Duplicates
arise from: (a) multiple agents filing the same finding independently, (b) a
cleanroom audit filing defects that overlap with prior session work, (c) beads
created at different granularities that cover the same code path.

**Detection protocol**:

```bash
# Exact title duplicates
br list --status=open | sed 's/.*- //' | sort | uniq -d

# Near-duplicates: same INV-FERR, same file path, or same HI/CR/ME tag
br list --status=open | grep -e "<keyword>" | sort
```

For each duplicate pair (or cluster):

1. **Identify the canonical bead**: the one with the fuller lab-grade description,
   more precise postconditions, and properly wired dependency edges. If one was
   hardened during a bead audit and the other was not, the hardened one is canonical.

2. **Transfer any unique information** from the duplicate into the canonical bead.
   If the duplicate has a dependency edge, test name, or file reference that the
   canonical lacks, add it to the canonical bead's description.

3. **Close the duplicate** with a structured reason. **Never delete** — always close
   with a traceable link to the canonical bead:

```bash
br close <duplicate-id> --reason "Duplicate of <canonical-id> (<title>). \
Superseded: <canonical-id> has <what makes it better: lab-grade description, \
full spec ref, postconditions, verification plan>. \
Deduplicated during bead audit <date>."
```

4. **Verify** the canonical bead's dependency graph is complete — any beads that
   depended on the closed duplicate must now depend on the canonical bead:

```bash
# Check if anything depended on the closed duplicate
br show <duplicate-id>  # Look for "Dependents:" section before closing
# If so, rewire:
br dep add <dependent> <canonical-id>
```

**Rules**:
- Never close both beads in a duplicate pair — exactly one survives.
- Never silently merge descriptions — the close reason must explain what was superseded and why.
- If two beads have the same title but genuinely different scope (e.g., one is the
  bug fix, the other is the regression test), they are NOT duplicates — they are
  siblings. Verify before closing.
- After deduplication, re-run `br list --status=open | sed 's/.*- //' | sort | uniq -d`
  to confirm zero remaining duplicates.

#### Step 2: Close Invalid Beads

For each CLOSE verdict:

```bash
br close <id> --reason "<evidence from Phase 1>"
```

Valid close reasons:
- "Completed: work done in commit <hash>"
- "Obsolete: premise invalidated by <spec change or code change>"
- "Duplicate of bd-XXX: <overlap description>"
- "Invalid: described behavior cannot be reproduced; <evidence>"

#### Step 3: Fix Factual Errors

For each bead with STALE or MISMATCHED in Phase 1:

Update the bead description to match current reality. Cite the evidence:
```bash
br update <id> --description "$(cat <<'BODY'
<corrected description with current file paths, line numbers, and invariant references>
BODY
)"
```

#### Step 3: Repair Dependency Graph

```bash
# Remove phantom edges (depend on closed beads that are satisfied)
br dep rm <bead> <closed-dep>

# Remove aspirational edges (nice ordering but not required)
br dep rm <bead> <aspirational-dep>

# Add missing edges (discovered in Phase 1 Check 3 or Phase 2 Lens 5)
br dep add <bead> <missing-dep>
```

#### Step 4: Recalibrate Priorities

For each bead flagged by `bv --robot-priority` or Phase 2:

```bash
br update <id> --priority <new-priority>
```

Priority rules:
- P0 requires: invariant violation OR data loss OR blocks all progress
- A bead's priority must be >= the highest priority of any bead it blocks
- A bead with 0 downstream dependents cannot be P0 unless it's a correctness bug

#### Step 5: Split Oversized Beads

For each SPLIT action:

1. Create child beads following the lab-grade template
2. Wire dependency edges between children
3. Convert the original to an epic (or close it if children fully cover the scope)

```bash
# Create child beads
br create --title "<specific sub-task>" --type task --priority <N> \
  --label "phase-<X>" --description "$(cat <<'BODY'
<full lab-grade template>
BODY
)"

# Wire edges
br dep add <child-2> <child-1>

# Convert parent to epic or close
br update <parent-id> --type epic
# OR
br close <parent-id> --reason "Split into bd-AAA, bd-BBB, bd-CCC"
```

#### Step 6: Harden to Lab-Grade

For each EDIT or REWRITE action, transform the bead to meet the full lab-grade
template. This is the core work of the audit.

For EDIT (1-2 missing fields): add the missing sections.

For REWRITE (3+ missing fields): rebuild the bead from scratch using this protocol:

1. **Read the primary source**: Open the cited spec file, find the invariant.
   Read Level 0 (algebraic law), Level 1 (state invariant), Level 2 (Rust contract).
2. **Read the referenced code**: Open the file, find the function or module.
   Understand what currently exists.
3. **Write the specification reference**: Exact INV-FERR, exact Level, exact section.
4. **Write preconditions**: What must be true before? Check dependency beads.
5. **Write postconditions**: Derive from the spec Level 2 contract. Each postcondition
   maps to a specific clause of the invariant. Include the verification command.
6. **Write frame conditions**: Check what other open beads touch nearby files.
   Ensure no collisions.
7. **Write the refinement sketch**: Abstract (from spec Level 0/1) → Concrete
   (from spec Level 2) → Coupling (from proptest strategy or Lean theorem in the spec).
8. **Write the Pseudocode Contract**: If the bead introduces or modifies Rust types,
   extract exact type definitions, function signatures, and enum match patterns from
   the spec Level 2 contract and the existing codebase. Apply the Compilability Test:
   could an agent write a `.rs` file with `todo!()` bodies from ONLY this section?
   Resolve all 5 judgment-call failure modes (ownership, mutability, return types,
   visibility, match arms). If no type changes, write "N/A — no type changes."
9. **Write the verification plan**: Test name, test location, test strategy.
   Derive from the proptest strategy in the spec if one exists.
10. **Write files**: List every file that changes. If > 3 files, reconsider scope.
11. **Write dependencies**: Check `bv --robot-insights` for graph neighborhood.

```bash
br update <id> --description "$(cat <<'BODY'
<full lab-grade template filled in from primary source verification>
BODY
)"
```

#### Step 7: Flag Uncertainties

For each FLAG action, output a structured question for the human:

```markdown
### FLAG: bd-<id> — <title>

**Uncertainty**: <what you don't know>
**Options**:
  A. <option and consequence>
  B. <option and consequence>
**Evidence examined**: <what you checked>
**Recommendation**: <which option you'd pick if forced, and why>
```

Collect all flags. Present them as a batch at the end of Phase 3, NOT inline
during execution. The human reviews all flags together.

---

## Phase 4: Graph Integrity Verification (Low DoF)

**Objective**: After all individual changes, verify the graph as a whole is sound.

```bash
# Re-run all graph checks
bv --robot-triage         # Compare against Phase 0 baseline
bv --robot-insights       # Verify: 0 cycles, healthy metrics
bv --robot-alerts         # Verify: 0 alerts
bv --robot-suggest        # Verify: suggestion count decreased
bv --robot-priority       # Verify: 0 priority misalignments
```

### Graph-Level Checks

Execute each check. Record pass/fail.

#### Check 1: Zero Cycles
Cycles mean circular dependencies — a decomposition error.
If any exist after remediation, something went wrong in Step 3.

#### Check 2: No Orphan Beads
Every open bead either belongs to an epic or stands alone with explicit
justification (leaf task with no dependents AND no parent is suspicious).

#### Check 3: No Phantom Edges
No dependency edges pointing to closed or nonexistent beads.

#### Check 4: No Priority Inversions
No high-priority bead depending on a low-priority bead without the
low-priority bead being elevated.

#### Check 5: Phase Coherence
Phase-4b beads do not depend on phase-4c beads (phase ordering violation).
Beads labeled for a completed phase are closed or relabeled.

#### Check 6: Epic Completeness
Every open epic has at least one open child. Every child's phase label is
consistent with the epic's phase label.

#### Check 7: File Disjointness (Parallel Safety)
No two ready (unblocked) beads modify the same file unless they have an
explicit ordering dependency. Collisions here will cause agent conflicts.

```bash
# Check for file overlaps among ready beads
br ready  # List all ready beads
# For each pair, compare file lists
```

#### Check 8: Ready Queue Health
After all changes, verify:
- `br ready` returns a non-empty set (work is unblocked)
- Ready beads are the correct entry points for the current phase
- Ready beads are all lab-grade (they will be picked up first)

### Output: Graph Health Report

| Check | Before | After | Status |
|-------|--------|-------|--------|
| Cycles | 0 | 0 | PASS |
| Orphans | N | N | PASS/FAIL |
| Phantom edges | N | 0 | PASS |
| Priority inversions | N | 0 | PASS |
| Phase coherence | — | — | PASS/FAIL |
| Epic completeness | — | — | PASS/FAIL |
| File disjointness | — | — | PASS/FAIL |
| Ready queue | N beads | M beads | PASS |

---

## Phase 5: Summary & Handoff (Low DoF)

### 5.1 Reconciliation Log

| Action | Count | Examples |
|--------|-------|---------|
| Closed (completed) | N | bd-xxx, bd-yyy |
| Closed (invalid) | N | bd-xxx |
| Closed (duplicate) | N | bd-xxx (dup of bd-yyy) |
| Factual corrections | N | bd-xxx (line numbers updated) |
| Dependency edges added | N | bd-xxx → bd-yyy |
| Dependency edges removed | N | bd-xxx ↛ bd-yyy |
| Priority changes | N | bd-xxx P0→P1 |
| Split into children | N | bd-xxx → bd-aaa + bd-bbb |
| Hardened to lab-grade | N | bd-xxx (added postconditions + verification plan) |
| Flagged for human | N | bd-xxx (see flags below) |

### 5.2 Before/After Metrics

| Metric | Before | After | Delta |
|--------|--------|-------|-------|
| Open beads | N | M | -K |
| Ready beads | N | M | +/- |
| Lab-grade beads | N% | M% | +K% |
| Graph alerts | N | M | -K |
| Priority inversions | N | 0 | -N |
| Missing edges | N | 0 | -N |

### 5.3 Flags for Human Review

Present all FLAG items here, batched and ordered by priority.

### 5.4 Flush

```bash
br sync --flush-only   # Export to JSONL (no git operations)
```

---

## Uncertainty Protocol

When you encounter ambiguity during the audit, follow this decision tree:

### Proceed Autonomously When:

- **Correcting a stale line number** that's off by < 10 lines (verifiable by reading the file)
- **Adding a missing dependency** that's explicit in the description text
- **Correcting type** when evidence is unambiguous (bead describes a broken behavior → bug, not task)
- **Closing a bead** when the exact work appears in a committed, tested, passing implementation
- **Adding a missing INV-FERR reference** when the mapping is 1:1 and unambiguous
- **Removing a phantom edge** to a closed bead whose work is complete
- **Adjusting priority by 1 level** when bv --robot-priority recommends it with evidence

### Stop and Flag When:

- **Two beads contradict each other** (different solutions for the same problem)
- **A bead's scope overlaps with an ADR-FERR** (relitigating settled decisions requires human sign-off)
- **Priority change spans > 2 levels** (P4→P1 or P0→P3 — may reflect deliberate human choice)
- **Cannot reproduce a described bug** and cannot determine if it's fixed or never existed
- **The bead references external knowledge** you don't have access to
- **Closing a bead would leave a spec gap** with no replacement (the invariant would be untracked)
- **Splitting a bead requires design decisions** about decomposition boundaries

**The general principle**: Act when you have primary-source evidence. Flag when
acting requires inference about intent.

---

## Demonstration: One Bead Hardened

**Before** (underspecified cleanroom finding):

```
bd-m9h: proptest case count below 10K threshold
P2, type: bug, label: phase-4a
Description: "Some proptest suites use cases(1000) instead of the
required 10,000 minimum per AGENTS.md testing standards."
```

**Audit findings**:
- Lens 1 (Structure): FAIL — missing 8 of 12 fields
- Lens 2 (Traceability): FAIL — no INV-FERR reference
- Lens 3 (Postconditions): FAIL — no acceptance criteria
- Lens 4 (Scope): PASS — clearly atomic
- Lens 5 (Frame): FAIL — no frame conditions
- Lens 6 (Executability): FAIL — which suites? Which files?
- Lens 7 (Alignment): PASS — serves verification depth

**After** (lab-grade):

```
bd-m9h: Raise proptest case count to 10K minimum in all verification suites
P2, type: bug, label: phase-4a

## Specification Reference
- Primary: INV-FERR-001 through INV-FERR-018 (all proptest-verified invariants)
  - Level cited: Level 2 (Rust contract — proptest strategies section)
  - Spec file: spec/01-core-invariants.md, "V:PROP" entries
- Supporting: AGENTS.md "Testing Standards" — "10,000+ cases for any
  function involving CRDT operations, ordering, or identity"

## Preconditions
None — leaf task, no structural prerequisites.

## Postconditions
1. [AGENTS.md] Every proptest suite in ferratomic-verify/tests/proptest/
   uses `proptest!(ProptestConfig { cases: 10_000, .. }, ...)` or higher.
   Verify: `grep -r "cases" ferratomic-verify/tests/proptest/ --include="*.rs"`
   shows no value below 10_000.
2. [INV-FERR-001..003] CRDT law property tests run 10K+ cases.
   Verify: `cargo test -p ferratomic-verify semilattice` passes.
3. `cargo test --workspace` passes with 0 failures.
4. `cargo clippy --workspace -- -D warnings` produces 0 warnings.

## Frame Conditions
1. Test logic must not change — only the case count configuration.
2. No changes to workspace crates (this is verify-only).
3. Existing test assertions must remain identical.

## Refinement Sketch
- Abstract: Property-based tests require sufficient sample size to achieve
  statistical confidence in algebraic laws (commutativity over 10K random
  pairs vs 1K provides ~10x defect detection probability for rare edge cases).
- Concrete: Change `ProptestConfig { cases: 1000, .. }` to
  `ProptestConfig { cases: 10_000, .. }` in each affected file.
- Coupling: Before/after test results must be identical (same properties
  tested, only sample size changes). Any NEW failures at 10K that didn't
  appear at 1K are real bugs exposed by the larger sample — file as
  separate beads, do not suppress.

## Pseudocode Contract
N/A — no type changes. This bead modifies only proptest configuration constants.

## Verification Plan
1. Test: Run full proptest suite with `cargo test -p ferratomic-verify`.
   Expect: all tests pass (longer runtime is expected and acceptable).
2. Search: `grep -rn "cases.*1000\|cases.*100[^0]" ferratomic-verify/`
   returns 0 matches.
3. Cross-check: Read each proptest file, confirm case count >= 10_000.

## Files
- `ferratomic-verify/tests/proptest/semilattice_properties.rs`: case count
- `ferratomic-verify/tests/proptest/clock_properties.rs`: case count
- `ferratomic-verify/tests/proptest/store_properties.rs`: case count
  (enumerate all affected files during execution — search first, then edit)

## Dependencies
- Depends on: None
- Blocks: None (independent quality improvement)
```

---

## Demonstration: Pseudocode Contract (Type-Changing Bead)

**Before** (underspecified):

```
bd-xyz: Wire PositionalStore into Store for cold start
P1, type: task, label: phase-4a
Description: "Make Store use PositionalStore for cold-start-loaded stores.
The store should use sorted arrays when loaded from checkpoint and switch
to OrdMap when mutated."
```

**Lens 6 (Compiler Test) failures**:
- 6a FAIL: `PositionalStore` — owned? Arc? Where is it stored?
- 6b FAIL: No method signatures. How does `indexes()` change?
- 6c FAIL: `AdaptiveIndexes` — how many variants? What does each arm do?
- 6e FAIL: `indexes()` return type changes — old and new not stated
- 6f FAIL: Where is `AdaptiveIndexes` declared? What re-exports it?

**After** (lab-grade Pseudocode Contract):

```
## Pseudocode Contract

// --- ferratomic-store/src/adaptive.rs (NEW) ---

/// Two-mode index storage: sorted arrays for cold start, OrdMap for mutation.
/// INV-FERR-072: promotion preserves datom set and query results.
pub(crate) enum AdaptiveIndexes {
    /// Read-optimized after cold start. O(log n) lookup, ~4 cache misses.
    /// O(n) clone — acceptable for read-only stores.
    SortedVec(SortedVecIndexes),
    /// Write-optimized after first mutation. O(log n) lookup, ~18 cache misses.
    /// O(1) clone via structural sharing (Arc internally).
    OrdMap(OrdMapIndexes),
}

impl AdaptiveIndexes {
    /// Promote from SortedVec to OrdMap. Called exactly once, on first mutation.
    /// O(n log n) one-time cost. After this call, self is always OrdMap.
    /// INV-FERR-072: content(before) == content(after).
    pub(crate) fn promote(&mut self) {
        // match *self {
        //   AdaptiveIndexes::SortedVec(sv) => { convert to OrdMap, replace self }
        //   AdaptiveIndexes::OrdMap(_) => { no-op — already promoted }
        // }
        todo!()
    }

    /// Query the EAVT index regardless of current mode.
    /// Was: pub fn eavt(&self) -> &OrdMap<EavtKey, Datom>
    /// Now: returns an opaque iterator — callers must not depend on OrdMap.
    pub(crate) fn eavt_range(&self, range: impl RangeBounds<EavtKey>)
        -> Box<dyn Iterator<Item = &Datom> + '_>
    {
        // match self {
        //   AdaptiveIndexes::SortedVec(sv) => { binary search + slice iter }
        //   AdaptiveIndexes::OrdMap(om) => { OrdMap range iter }
        // }
        todo!()
    }
}

// --- ferratomic-store/src/mod.rs (MODIFIED) ---

impl Store {
    /// Was: pub fn indexes(&self) -> &Indexes
    /// Now: indexes are accessed through query methods, not directly.
    /// Callers that called store.indexes().eavt().range(..) must use
    /// store.eavt_range(..) instead.
    /// REMOVED: pub fn indexes(&self) -> &Indexes
    /// ADDED:
    pub fn eavt_range(&self, range: impl RangeBounds<EavtKey>)
        -> Box<dyn Iterator<Item = &Datom> + '_>
    {
        todo!()
    }
}
```

This contract eliminates all 5 Lens 6 failures: the agent knows the exact enum,
the exact match arms, the exact method signatures, the exact visibility, and the
exact callers affected by the `indexes()` removal.

---

## What NOT To Do

- Do not audit beads without reading primary sources. "This looks wrong" is not
  a finding. Show the spec text, the code, the commit.
- Do not simplify scope during audit. If the spec says it, the bead must track it.
  The audit hardens beads; it does not negotiate requirements downward.
- Do not make assumptions about intent. If you cannot determine why a bead exists
  or what "correct" means, FLAG it — do not guess.
- Do not fix code during the audit. The audit produces a clean task graph.
  Code changes happen in subsequent execution sessions.
- Do not batch changes mentally. Update each bead immediately after completing
  its assessment. This prevents drift between your findings and the bead state.
- Do not skip beads because they "look fine." Apply all 8 lenses to every bead.
  The purpose of a systematic protocol is to catch what quick judgment misses.
- Do not close beads that represent real spec gaps just because the current code
  works. If INV-FERR-NNN requires X and the bead tracks X, the bead stays open
  until X is implemented and verified — regardless of whether the absence of X
  causes a visible bug today.
- Do not rewrite beads to be shorter. Lab-grade beads are as long as they need
  to be. A 50-line bead that an agent can execute blind is better than a 5-line
  bead that spawns 3 clarifying questions.
