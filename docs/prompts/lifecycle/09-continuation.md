# 09 Session Continuation & Handoff

> **Purpose**: End a session cleanly. Generate a self-contained continuation prompt.
> **DoF**: Low. Structured output, precise format.
> **Cognitive mode**: Summarization and handoff.

---

## End-of-Session Protocol

Execute these steps IN ORDER. Do not skip any step.

### Step 1: File Issues for Remaining Work

Any unfinished work, discovered bugs, or deferred decisions become beads issues.
Do not carry uncommitted knowledge in your head -- crystallize it.
Follow the format in [08-task-creation.md](08-task-creation.md).

```bash
# For each piece of remaining work:
br create --title "..." --type task --priority N --label "phase-Na" \
  --description "..."

# Wire any dependency edges
br dep add <new-id> <existing-id>
```

### Step 2: Update Issue Status

```bash
# Close completed work
br close <id> --reason "Done: <one-line summary>"

# Mark in-progress work (if stopping mid-task)
br update <id> --status in_progress

# Export to JSONL (no git operations)
br sync --flush-only
```

### Step 3: Quality Gates (if code changed)

```bash
# All eleven must pass. No exceptions.

# Gate 1: Formatting
CARGO_TARGET_DIR=/data/cargo-target cargo fmt --all -- --check

# Gate 2: Lint (all targets)
CARGO_TARGET_DIR=/data/cargo-target cargo clippy --workspace --all-targets -- -D warnings

# Gate 3: NEG-FERR-001 — no unwrap/expect/panic in production code
CARGO_TARGET_DIR=/data/cargo-target cargo clippy --workspace --lib -- -D warnings \
  -D clippy::unwrap_used -D clippy::expect_used -D clippy::panic

# Gate 4: Tests
CARGO_TARGET_DIR=/data/cargo-target cargo test --workspace

# Gate 5: Supply chain audit
CARGO_TARGET_DIR=/data/cargo-target cargo deny check

# Gate 6: INV-FERR-023 — #![forbid(unsafe_code)] verified in all crate roots

# Gate 7: Documentation builds without warnings
CARGO_TARGET_DIR=/data/cargo-target cargo doc --workspace --no-deps -- -D warnings

# Gate 8: File complexity limits (500 LOC, clippy.toml thresholds)

# Gate 9: Lean proofs (0 sorry) — unconditional
cd ferratomic-verify/lean && lake build

# Gate 10: MIRI (pure-logic subset)
CARGO_TARGET_DIR=/data/cargo-target cargo +nightly miri test

# Gate 11: Coverage >= thresholds (no regression)
```

All eleven must pass. If any fails, fix before continuing.

### Step 4: Update Canonical Documents

**Stale agent-facing docs are actively parasitic** — they waste every future
agent's context on wrong information. Before committing, verify and update:

```bash
# 1. Check spec/README.md counts (the single source of truth)
head -6 spec/README.md

# 2. If you added/removed INV-FERR, ADR-FERR, or NEG-FERR:
#    - Update the count in spec/README.md line 6
#    - Update the module table in spec/README.md if new entries affect it
#    - Do NOT update counts in QUICKSTART.md, AGENTS.md, or README.md —
#      they should reference spec/README.md, not hardcode numbers

# 3. If phase status changed (gate closed, new phase started):
#    - Update QUICKSTART.md "Current Phase" section
#    - Update AGENTS.md Phase Ordering table status column
#    - Update README.md phase list

# 4. If crate structure changed (new crates, renames):
#    - Update AGENTS.md Crate Map
#    - Update README.md Crate Map table
#    - Update Cargo.toml [workspace] members

# 5. If you added new design docs or lifecycle prompts:
#    - Update docs/prompts/lifecycle/README.md
#    - Update AGENTS.md Specification pointers
```

The rule: `spec/README.md` is the single source of truth for counts.
`QUICKSTART.md` references it, never duplicates it. `AGENTS.md` and `README.md`
may contain counts in their public-facing text but must be updated when they drift.

### Step 5: Commit and Push

```bash
git add -A
git commit -m "feat: <summary of session work>"
git push origin main
git push origin main:master    # Keep master synchronized
```

Work is NOT done until `git push` succeeds.

### Step 5: Generate Continuation Prompt

Write the continuation prompt to stdout (or a file if requested).
This is the ONLY artifact the successor agent needs beyond `QUICKSTART.md`.

---

## Continuation Prompt Format

The continuation prompt must follow this exact structure:

```markdown
# Ferratomic Continuation -- Session NNN

> Generated: YYYY-MM-DD
> Last commit: <hash> "<message>"
> Branch: main

## Read First

1. `QUICKSTART.md` -- project orientation
2. `AGENTS.md` -- guidelines and constraints
3. `spec/README.md` -- load only the spec modules you need

## Session Summary

### Completed
- <what was done, with issue IDs if applicable>

### Decisions Made
- <any ADR-level decisions, with rationale>

### Bugs Found
- <discovered defects, with issue IDs>

### Stopping Point
<Exactly where you stopped. Which file, which function, which line.
What was the last thing you verified working? What was the next
thing you were about to do?>

## Next Execution Scope

### Primary Task
<The single most important thing the successor should do.
Include the beads issue ID and the specific acceptance criteria.>

### Ready Queue
```bash
br ready          # Show unblocked issues
bv --robot-next   # Top pick with reasoning
```

### Dependency Context
<Which tasks block which. What must finish before what.
Only include if the dependency graph is non-obvious.>

## Hard Constraints

- Safe callable surface: `#![forbid(unsafe_code)]` by default; internal unsafe permitted only when firewalled behind safe APIs, mission-critical, and ADR-documented (GOALS.md §6.2)
- No `unwrap()` in production code
- `CARGO_TARGET_DIR=/data/cargo-target`
- Phase N+1 cannot start until Phase N passes isomorphism check
- Full defensive engineering standards: GOALS.md §6
- <any session-specific constraints discovered during work>

## Stop Conditions

Stop and escalate to the user if:
- <condition 1>
- <condition 2>
- <any session-specific escalation triggers>
```

---

## Demonstration: Full Handoff

```bash
# Step 1: File remaining work
br create \
  --title "Implement WAL chain hash verification on recovery" \
  --type task --priority 2 --label "phase-4a" \
  --description "$(cat <<'BODY'
**What**: WAL recovery verifies chain hash continuity.
**Why**: INV-FERR-008 (WAL ordering).
**Acceptance**:
1. Recovery detects and rejects tampered WAL frames.
2. Chain hash break at frame N discards frames N+ and logs warning.
**File(s)**: ferratomic-wal/src/wal.rs
**Depends on**: br-30 (WAL append implementation).
BODY
)"

# Step 2: Update status
br close br-28 --reason "Done: Snapshot struct with Arc<StoreInner>"
br close br-29 --reason "Done: WriterActor mpsc serialization"
br update br-30 --status in_progress
br sync --flush-only

# Step 3: Quality gates
CARGO_TARGET_DIR=/data/cargo-target cargo check --workspace && \
CARGO_TARGET_DIR=/data/cargo-target cargo clippy --workspace -- -D warnings && \
CARGO_TARGET_DIR=/data/cargo-target cargo fmt --check && \
CARGO_TARGET_DIR=/data/cargo-target cargo test --workspace

# Step 4: Update canonical docs (if counts/phase/crates changed)
# e.g., head -6 spec/README.md  # verify counts
# e.g., update QUICKSTART.md phase status if gate closed

# Step 5: Commit and push
git add -A
git commit -m "feat: snapshot isolation + writer serialization (br-28, br-29)"
git push origin main && git push origin main:master

# Step 6: Output the continuation prompt (to stdout)
```

The successor agent receives the continuation prompt + `QUICKSTART.md` and is
productive within 5 minutes. No Slack messages. No "where did you leave off?"
The continuation prompt IS the handoff.

---

## Common Mistakes

- **Forgetting to update canonical docs**: The #1 cause of stale QUICKSTART.md,
  AGENTS.md, and README.md. If you changed invariant counts, phase status, or
  crate structure, Step 4 is MANDATORY.
- **Forgetting `br sync --flush-only`**: Beads state exists only in memory until flushed.
- **Pushing code that doesn't compile**: Always run quality gates BEFORE commit.
- **Vague stopping points**: "I was working on store.rs" is useless. "I finished
  `apply_datoms` and verified all 4 indexes update. Next: implement `merge` starting
  from the `Semilattice` trait impl on line 142" is useful.
- **Not pushing**: Local commits are invisible. Push is mandatory.
- **Leaving in-progress tasks unmarked**: The successor won't know what's mid-flight.
