# 15 Prompt Forge — Designing Lifecycle Prompts

> **Purpose**: Design a new lifecycle prompt from first principles. The output is a
> prompt that meets the quality standard of this ecosystem — grounded in primary
> sources, formally structured, and producing zero-ambiguity execution for agents.
>
> **DoF**: Varies. High (research/formalization) → Structured (specification) → Low (drafting) → High (convergence).
>
> **Cognitive mode**: Meta-design. You are not doing the work — you are designing
> the instrument that will cause an agent to do the work correctly.
>
> **Model gate**: Opus 4.6 with /effort max or GPT 5.4 xhigh. Prompt design
> requires sustained reasoning about reasoning — shallow models produce shallow prompts.

---

## When to Use This Prompt

- A new recurring task has emerged that would benefit from a structured prompt
- An existing ad-hoc prompt is producing inconsistent or low-quality agent output
- A new project phase requires a new execution workflow
- You need to codify an expert methodology into a reproducible agent protocol

**This prompt produces a lifecycle prompt file.** It does not produce code, tasks,
or analysis. The output is `docs/prompts/lifecycle/NN-name.md`.

---

## The Core Insight

A prompt is not a list of instructions. It is a **field configuration over the
model's activation manifold**. Instructions interact non-linearly: a formatting
constraint can suppress analytical depth; a demonstration can activate capabilities
that no constraint can reach. Prompt design is therefore a design discipline, not
a writing task.

Three consequences follow:

1. **Structure > Content.** Rearranging identical components changes output quality
   more than rewriting them. Optimize arrangement before polishing prose.

2. **Demonstrate > Enumerate.** One worked example encodes format, style, depth,
   tone, and domain simultaneously — more information per token than any constraint
   cluster. Replace constraint clusters with demonstrations wherever possible.

3. **Activate > Lecture.** Point the model at knowledge it already has rather than
   re-teaching it. "What invariants could this violate?" activates deep reasoning.
   "Check that each invariant is satisfied" activates surface compliance.

---

## Phase 0: Ground Yourself

Before designing a prompt, you must deeply understand the task it will drive,
the project it will operate in, and the methodology it will encode.

```bash
# Project orientation
cat AGENTS.md
cat spec/README.md

# Methodology for prompt design
ms load prompt-optimization -m --full

# Methodology for the task domain (pick ONE matching the domain)
ms load spec-first-design -m --pack 2000       # If the task is analytical/formal
ms load rust-formal-engineering -m --pack 2000  # If the task is implementation
```

Study the existing prompt ecosystem:
```bash
cat docs/prompts/lifecycle/README.md   # Index and workflow diagram
```

Read 2-3 existing prompts that are structurally similar to what you're designing.
The strongest exemplars are `06-cleanroom-review.md` (adversarial audit), `07-bug-triage.md`
(diagnosis + fix), `13-progress-review.md` (multi-phase assessment), and
`14-bead-audit.md` (verification + transformation).

---

## Phase 1: Understand the Task (High DoF)

Before writing any prompt text, formalize the task domain. This is the single
most powerful intervention for prompt quality — it prevents the mid-DoF saddle
where output is neither precise enough nor free enough.

### 1.1 Interview the User

Do not assume you understand the task. Ask these questions explicitly. Do not
proceed until you have clear answers.

**Required questions:**

1. **What is the goal?** What state of the world exists after this prompt is
   used successfully that didn't exist before?

2. **What is the output artifact?** What specific document, code, or state
   transformation does the prompt produce?

3. **Who executes this prompt?** What model capability is required? What context
   will the executing agent have (clean window? mid-conversation? loaded with
   prior prompts)?

4. **What are the constraints?** Hard limits, quality standards, forbidden
   approaches, tool requirements.

5. **When is this prompt used?** What triggers it? What precedes it in the
   workflow? What follows it?

6. **What does failure look like?** If the prompt produces bad output, what
   specifically goes wrong? Name the failure modes.

7. **What does the user's current approach look like?** If they have an ad-hoc
   prompt or verbal instruction, read it. It encodes implicit requirements that
   the user may not articulate as explicit constraints.

**Optional questions** (ask if answers to required questions suggest complexity):

8. **Are there tradeoffs?** Constraints that tension against each other?
9. **What has been tried before?** What didn't work and why?
10. **Is this Ferratomic-specific or generalizable?** Scope determines abstraction level.

### 1.2 Research Primary Sources

Read every document the prompt will reference. The prompt cannot be more precise
than your understanding of the domain.

```bash
# Read relevant spec sections
cat spec/NN-section.md

# Read relevant code
cat <crate>/src/<module>.rs

# Read relevant design docs
cat docs/design/<doc>.md

# Search for prior art
cass search "<task domain>" --robot --fields minimal --limit 10

# Check procedural memory
cm context "<task domain>" --json --limit 5
```

### 1.3 Formalize the Task

Answer these questions in writing. They become the prompt's specification.

- **State space**: What are the inputs? What are the outputs? What quality
  dimensions matter?
- **DoF commitment**: Should the prompt target high DoF (exploration/discovery)
  or low DoF (execution/compliance)? If the task has distinct phases, which DoF
  for each phase?
- **Cognitive mode**: Name the mode. "Adversarial verification." "Forensic
  analysis." "Empirical science." "Specification." The name shapes the agent's
  approach more than paragraphs of instruction.
- **What does the model already know?** List domain knowledge you can ACTIVATE
  rather than TEACH. Every concept the model already understands is a constraint
  you don't need to write.
- **Integration points**: Where does this prompt sit in the lifecycle? What feeds
  it? What does it feed?

### Output: Task Specification

A structured document (not the prompt itself) containing: goal, artifact, executor,
constraints, trigger, failure modes, DoF commitment, cognitive mode, integration points.
This is the prompt's blueprint.

---

## Phase 2: Design the Prompt Architecture (Structured DoF)

With the task specification in hand, design the prompt's structure before writing
its content.

### 2.1 Phase Decomposition

Every non-trivial prompt decomposes into phases. Determine:

- How many phases? (Typical: 3-6)
- What is the DoF progression? (Usually: decreasing, or U-shaped with verification at the end)
- What artifact does each phase produce?
- What checkpoint gates the transition between phases?

**Principle**: Each phase has ONE cognitive mode. Measurement does not overlap
with judgment. Judgment does not overlap with planning. Mixing modes produces
the mid-DoF saddle.

### 2.2 Constraint Budget

List every constraint you think the prompt needs. Then apply the **removal test**
to each one: "If I remove this constraint, does the output quality decrease?"

The typical trajectory:
- Start with 10-15 candidate constraints
- Remove 4-7 that the model already knows (parasitic — lecturing, not activating)
- Replace 2-3 constraint clusters with a single demonstration
- End with 4-8 constraints + 1-2 demonstrations

**Check for coherence**: Do any surviving constraints tension against each other?
If so, resolve the tension with a demonstration showing the desired tradeoff.

### 2.3 Demonstration Selection

Choose ONE primary demonstration that encodes the maximum number of principles
simultaneously. The ideal demonstration:

- Uses a concrete, real example from the project (not hypothetical)
- Shows the FULL workflow from input to output
- Includes exact file paths, exact commands, exact expected output
- Is 40-80 lines (shorter = underspecified; longer = attention-expensive)
- For graduated-output prompts: shows 2+ quality levels (good/bad, before/after,
  multi-severity)

### 2.4 Anti-Pattern Inventory

Identify 5-8 specific failure modes for this prompt. Each anti-pattern:

- Names a concrete mistake (not vague principle)
- States what goes wrong if the agent does this
- Pairs with a positive instruction elsewhere in the prompt

Anti-patterns serve as **ecosystem boundaries** — they prevent the agent from
collapsing this prompt's cognitive mode into an adjacent prompt's mode.

### 2.5 Uncertainty Boundaries

For any prompt involving judgment, define:

- **Proceed autonomously when**: The agent has primary-source evidence.
- **Stop and flag when**: Acting requires inference about intent.

This protocol prevents two failure modes: paralysis (flagging everything) and
overreach (guessing when evidence is ambiguous).

### Output: Prompt Architecture

A structural blueprint: phase list with DoF/mode/artifact/gate for each,
constraint list (post-removal-test), demonstration choice, anti-pattern list,
uncertainty boundaries.

---

## Phase 3: Draft the Prompt (Low DoF)

With the architecture designed, write the prompt. Follow the ecosystem's invariant
structure — deviating from it makes the prompt feel foreign and reduces agent trust.

### The Prompt DNA

Every lifecycle prompt in this ecosystem shares this structural skeleton:

```markdown
# NN — Title

> **Purpose**: One sentence.
> **DoF**: Level. Constraint sentence.
> **Cognitive mode**: Named mode.
> [**Model gate**: Requirement.]

---

## When to Use This Prompt
[3-5 bullet triggers]
[Scope statement: what this prompt does NOT produce]

---

## Phase 0: Ground Yourself
[Skill loading + bv/br context + checkpoint]

---

## Phase N: [Name] ([DoF level])
**Objective**: One sentence.
[Content: protocol, checklists, tables]

---

## Demonstration: [Concrete Example]
[40-80 lines. Full workflow. Real INV-FERR. Exact paths and commands.]

---

## Integration with Other Prompts
[Table mapping findings → follow-up prompts]

---

## [Stop Conditions / Uncertainty Protocol]
[When to escalate vs proceed]

---

## What NOT To Do
[5-8 specific anti-patterns, verb-first]
```

### Writing Principles

**Structure first.** Write all section headers and one-line summaries before
filling in any section. This is the prompt's skeleton — get it right before
adding flesh.

**Demonstrate early in the drafting process.** Write the demonstration BEFORE
writing the surrounding protocol. The demonstration is the ground truth; the
protocol is the generalization. If you write protocol first, you risk a
demonstration that doesn't fit.

**Use activation language.** Instead of "Check that the merge operation is
commutative" (lecturing), write "Which algebraic laws must this merge preserve?
Verify each one." (activating). The agent knows what commutativity is — point
it at the question, don't recite the definition.

**Name your cognitive modes explicitly.** "Forensic analysis then surgical repair"
tells the agent more about HOW to think than any list of steps tells it WHAT to do.

**Include exact tool commands.** Every `bash` block in the prompt should be
copy-pasteable. No pseudocode, no `<placeholder>` without explanation.

### Output: Draft Prompt

The complete prompt file, following the DNA structure.

---

## Phase 4: Convergence (High DoF)

The draft is not the final artifact. Apply 5 sequential single-lens review passes.
Each pass uses ONE focused lens. Do not combine lenses — distributing review across
time avoids the attention competition of stacking in space.

### Pass 1: Completeness

> "What is missing?"

- Does every phase have a named artifact and a checkpoint/gate?
- Does the demonstration cover the full workflow?
- Are all integration points documented?
- Is there a "What NOT To Do" section?
- Are tool commands complete and copy-pasteable?

### Pass 2: Parsimony

> "What is parasitic?"

Apply the constraint removal test to every constraint, instruction, and section:
- If removing it does not hurt output quality, remove it
- If the model already knows it, it's lecturing — remove it or convert to an activation question
- If a constraint cluster can be replaced by the demonstration, replace it

**This is the most important pass.** Overprompting past k\* produces generic,
hedged output. Fewer well-chosen constraints always beat more loosely-specified ones.

### Pass 3: Coherence

> "Do any constraints fight each other?"

- Does "be thorough" conflict with "be concise"?
- Does "check everything" conflict with the time budget?
- Does "don't make assumptions" conflict with "proceed autonomously"?

For each tension: resolve it. Usually by scoping one constraint ("be concise IN
THE EXECUTIVE SUMMARY; be thorough IN THE GAP REGISTER") or by replacing both
with a demonstration that shows the desired balance.

### Pass 4: Structure

> "Is the arrangement optimal?"

- Is the demonstration close to the top? (It sets the quality basin early)
- Is the anti-pattern section at the bottom? (Boundaries after positive guidance)
- Does the DoF progress logically across phases?
- Could sections be reordered for better flow?

Test: rearrange 2-3 sections mentally. Would any reordering improve clarity?

### Pass 5: Activation

> "Does this point at deep knowledge or lecture surface knowledge?"

For each instruction in the prompt:
- Is it activating? ("What invariants could this violate?" — deep)
- Or lecturing? ("Invariants are properties that must hold" — surface)

Convert lectures to activations. If the model needs domain knowledge it genuinely
lacks, provide it as context (a quoted spec passage), not as instruction.

### Convergence Criterion

The prompt has converged when a pass produces zero structural changes. Content
polish (word choice, formatting) does not count as structural. Typically converges
in 3-5 passes.

### Output: Final Prompt

The converged prompt file, ready for `docs/prompts/lifecycle/NN-name.md`.

---

## Phase 5: Ecosystem Integration (Low DoF)

### 5.1 Update the README

Add the new prompt to `docs/prompts/lifecycle/README.md`:
- Add row to the prompt index table
- Update the workflow diagram if the new prompt creates a new pathway

### 5.2 Verify Cross-References

- All markdown links to other prompts use relative paths and resolve correctly
- All referenced tools (`br`, `bv`, `ms`, `cargo`) are correct commands
- All referenced files (`spec/NN-section.md`, etc.) exist

### 5.3 Verify Self-Consistency

The prompt must pass its own quality standard. If the prompt defines a checklist,
apply that checklist to the prompt itself. If it defines anti-patterns, verify the
prompt does not exhibit them.

---

## Demonstration: From Ad-Hoc Instruction to Lifecycle Prompt

The following shows the transformation of an ad-hoc user instruction into a
lifecycle prompt, annotated with which phase produced which design decision.

### BEFORE (ad-hoc instruction)

```
Check over each bead super carefully-- are you sure it makes sense?
Is it optimal? Could we change anything to make the system work better
for users? If so, revise the beads. It's a lot easier and faster to
operate in "plan space" before we start implementing these things!
Do this for all open beads, not just the ones you made. Make sure you
always ground your changes in the primary source documents and code.
Do not simplify or cut scope. Do not make assumptions. If you run into
any uncertainty, stop and ask me for clarification. Be meticulous and
methodical and always think from first principles.
```

### Phase 1 findings (task formalization)

| Question | Answer | Design decision |
|----------|--------|----------------|
| Goal? | Clean, executable task graph | Named artifact: "reconciliation log" |
| Output? | Hardened beads + graph health report | Multi-artifact output |
| Constraints? | "Ground in primary sources", "don't simplify", "don't assume" | These become Phase 1 (verification), uncertainty protocol |
| Failure modes? | Agent makes changes without checking sources; agent closes beads that represent real gaps | These become anti-patterns |
| DoF? | High (discovery) then Low (mechanical edits) | Phase decomposition with DoF progression |
| What does user already do well? | "Plan space" insight — edit the plan, not the code | This becomes the header's cognitive mode framing |

### Phase 2 findings (architecture)

Constraint removal test applied to the ad-hoc prompt:
- "Be meticulous and methodical" → **REMOVE** (the model knows this; the phase structure enforces it)
- "Think from first principles" → **REMOVE** (the grounding protocol enforces it)
- "Ground in primary sources" → **KEEP** (core requirement — becomes Phase 1's entire purpose)
- "Don't simplify or cut scope" → **KEEP** (non-obvious — becomes anti-pattern)
- "Don't make assumptions" → **KEEP** (becomes uncertainty protocol)
- "Stop and ask for clarification" → **KEEP** (becomes FLAG mechanism)

Result: 4 surviving constraints + 7-lens audit framework + before/after demonstration.

### AFTER (lifecycle prompt 14-bead-audit.md)

```
5 phases: Ground → Verify → Assess → Reconcile → Verify Graph → Summarize
Lab-Grade Standard: defined as a single predicate
7 Audit Lenses: Structural, Traceability, Postcondition Strength,
  Atomicity, Frame Conditions, Executability, Axiological Alignment
Uncertainty Protocol: proceed (evidence) vs flag (inference about intent)
Demonstration: bd-m9h before/after transformation (68 lines)
8 anti-patterns grounded in specific failure modes
Before/after metrics for verification
```

The ad-hoc prompt's 8 sentences became a 791-line structured instrument. The
transformation was not "add more words" — it was formalize the task domain (Phase 1),
decompose into phases with distinct cognitive modes (Phase 2), draft following the
ecosystem's structural DNA (Phase 3), and converge through single-lens passes (Phase 4).

---

## Integration with Other Prompts

| Situation | Follow-up |
|-----------|-----------|
| New prompt designed | Update `README.md` index |
| Prompt produces inconsistent results | Re-run Phase 4 convergence passes |
| User reports prompt failure mode | Add to anti-pattern section |
| New tool/methodology available | Update Phase 0 skill loading |
| Prompt scope creeps | Split using this meta-prompt |

---

## What NOT To Do

- Do not write the prompt before formalizing the task (Phase 1). Jumping to text
  produces mid-DoF prompts that are neither precise nor free. Formalization is the
  single most powerful intervention for prompt quality.
- Do not lecture the model about things it already knows. "Invariants must be
  checked" wastes tokens. "Which invariants could this violate?" activates the
  same capability at zero attention cost.
- Do not add constraints without the removal test. Every constraint that survives
  must demonstrably improve output when present and degrade it when absent. If you
  cannot demonstrate the degradation, the constraint is parasitic.
- Do not skip the demonstration. A prompt without a worked example is a prompt
  where the agent must guess what "good" looks like. The demonstration IS the
  quality specification.
- Do not combine cognitive modes within a phase. "Measure AND judge AND plan" in
  one phase produces the mid-DoF saddle. "Measure. Then judge. Then plan." in
  three phases activates each mode fully.
- Do not make the prompt longer than it needs to be. The prompt competes for the
  agent's attention with the actual task. Every unnecessary sentence is a tax on
  the agent's output quality. If a section doesn't survive the removal test,
  delete it.
- Do not design in isolation from the ecosystem. Every prompt connects to others.
  Check: what feeds this prompt? What does this prompt feed? Is the output artifact
  compatible with the consuming prompt's input format?
- Do not publish without self-consistency check. The prompt must pass its own
  quality standard. If it defines anti-patterns, it must not exhibit them. If it
  requires demonstrations, it must contain one.
