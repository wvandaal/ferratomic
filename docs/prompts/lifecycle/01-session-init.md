# 01 — Session Init & Context Recovery

> **Purpose**: Cold-start orientation. You know nothing about ferratomic.
> After this protocol, you can articulate the project's purpose, core invariants,
> current phase, and your specific task — or you keep reading until you can.
>
> **DoF**: High (discovery). Do not write code during this phase.

---

## Step 1: Understand the Project

Read these files in order. Stop when you can answer the checkpoint questions below.

```bash
cat QUICKSTART.md          # Project identity, current phase, key document pointers
cat AGENTS.md              # Build commands, hard constraints, quality gates, crate map
cat GOALS.md               # Value hierarchy, success criteria, defensive standards (§6)
cat spec/README.md         # Spec module index (canonical INV/ADR/NEG counts)
```

**Checkpoint** (answer all before proceeding):
1. What is ferratomic's core algebraic property?
2. What are the 5 development phases and which phase are we in?
3. What does `#![forbid(unsafe_code)]` mean and which crates enforce it?
4. Name 3 hard constraints (C1, C2, C4) from memory.

If you cannot answer all four, re-read AGENTS.md sections "True North" and "Hard Constraints."

---

## Step 1.5: Staleness Check

Verify that the documents you just read are consistent with the current project state.
Stale docs are actively parasitic — they waste your context on wrong information.

```bash
# Compare QUICKSTART.md phase status against actual beads state
bv --robot-next    # What does the project ACTUALLY say is next?

# Check spec counts match spec/README.md (the single source of truth)
head -6 spec/README.md   # Canonical INV/ADR/NEG counts
```

If QUICKSTART.md, AGENTS.md, or README.md contain stale phase status, wrong
invariant counts, or outdated crate maps: **fix them NOW before proceeding.**
This is not optional cleanup — stale agent-facing docs corrupt every subsequent
agent's context. See Step 2 of 09-continuation.md for the canonical update protocol.

---

## Step 2: Understand the Execution Frontier

```bash
br ready                   # Actionable tasks (no blockers)
bv --robot-next            # Top-priority pick with claim command
bv --robot-triage          # Full ranked recommendations + health
```

Read the output. Understand what work is available, what blocks what,
and where the project's energy should go. Do not start working yet.

---

## Step 3: Load Methodology

Load exactly ONE skill matching your cognitive phase:

```bash
# Discovery (understanding spec, modeling):
ms load spec-first-design -m --full

# Implementation (writing Rust, proving theorems):
ms load rust-formal-engineering -m --full

# Review (auditing, finding defects):
ms load prompt-optimization -m --pack 2000
```

Never load more than one full skill at a time. If output becomes generic
or name-drops frameworks without depth, you have too many skills loaded.

---

## Step 4: Claim Your Task

Once you know what to do:

```bash
br update <id> --status in_progress
```

Read the spec module referenced by the task. For example, if the task
references INV-FERR-001 through INV-FERR-012, read `spec/01-core-invariants.md`.
If it references INV-FERR-013 through INV-FERR-024, read `spec/02-concurrency.md`.

| Spec module | INV-FERR range | Focus |
|-------------|----------------|-------|
| `spec/01-core-invariants.md` | 001-012 | CRDT, indexes, snapshots, WAL, schema, identity |
| `spec/02-concurrency.md` | 013-024 | Checkpoint, recovery, HLC, sharding, atomicity |
| `spec/03-performance.md` | 025-032 | Index backend, write amp, latency, cold start |
| `spec/04-decisions-and-constraints.md` | 033-036 | ADRs, NEGs, cross-shard, partitions |
| `spec/05-federation.md` | 037-044 | Federation, selective merge, transport, migration |
| `spec/06-prolly-tree.md` | 045-050 | Content-addressed trees, history independence, diff |

---

## Step 5: Verify Your Understanding

Before writing any code, state in your own words:
- What invariant(s) you are implementing/verifying
- What the Level 0 algebraic law says
- What the Level 2 Rust contract looks like
- What would falsify this invariant

If any answer is "I'm not sure," go back to the spec. The spec has
the algebraic law, the Rust contract, the falsification condition,
the proptest strategy, and the Lean theorem for every single invariant.

---

## Crate Map (for orientation)

```
ferratom/           Core types. Datom, EntityId, Value, Schema. ZERO deps.
ferratomic-core/    Engine. Store, indexes, WAL, snapshots, merge.
ferratomic-datalog/ Query. Datalog parser + evaluator.
ferratomic-verify/  Proofs + tests. Lean 4, proptest, Kani, Stateright.
```

Dependency: `ferratom <-- ferratomic-core <-- ferratomic-datalog`. Acyclic.

---

## Build Commands

```bash
CARGO_TARGET_DIR=/data/cargo-target cargo check --workspace
CARGO_TARGET_DIR=/data/cargo-target cargo clippy --workspace -- -D warnings
CARGO_TARGET_DIR=/data/cargo-target cargo test --workspace
cd ferratomic-verify/lean && lake build   # Lean proofs
```

---

## What NOT To Do

- Do not write code before completing Steps 1-5.
- Do not load multiple ms skills simultaneously.
- Do not skip the checkpoint questions in Step 1.
- Do not start on a task without claiming it (`br update`).
- Do not assume you know the spec. Read it. Every time.

---

## Transition

When you can articulate purpose, invariants, phase, and task:
proceed to the lifecycle prompt matching your phase.

| Phase | Prompt |
|-------|--------|
| 1: Lean proofs | `02-lean-proofs.md` |
| 2: Tests (red) | `03-test-suite.md` |
| 3: Types | `04-type-definition.md` |
| 4: Implementation | `05-implementation.md` |
| Review | `06-cleanroom-review.md` |
