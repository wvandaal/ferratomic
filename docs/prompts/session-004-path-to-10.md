# Ferratomic Session 004 — Path to 10.0: Phase 4a Gate Closure

> **Scope**: Close Phase 4a at 10.0/A+ across all 10 quality vectors.
> **Mandate**: Zero-defect cleanroom standard. Gate cannot close without confirmed 10.0.
> **Method**: Bead-driven execution. Every task is pre-specified to lab-grade.
> **State**: Build broken (4 clippy/test errors). Gate chain blocked. 102 beads filed.
> **Critical rule**: The gate (bd-add) depends on T11-012 (re-review confirms 10.0).
>   It is structurally impossible to close the gate without a 10.0 progress review.

---

## Phase 0: Context Recovery

1. Read `AGENTS.md`
2. Read `docs/reviews/2026-04-01-path-to-10.0.md` — the 102-item roadmap (this is your map)
3. Run `br ready` — see what's actionable now
4. Run `bv --robot-triage` — confirm graph health
5. `export CARGO_TARGET_DIR=/data/cargo-target`

**Checkpoint**: Before any code, verify:
- You understand the bead dependency graph: Tier 0 → Tier 0.5 → Tiers 2-10 → Spec audit → Cleanroom → Re-review → Gate
- `br show bd-7fub.1.1` returns a full lab-grade description (if not, read the path-to-10.0 doc)
- The build is currently BROKEN (4 P0 bugs at top of ready queue)

---

## Phase 1: Fix the Build (Tier 0)

**Goal**: All 5 quality gates pass. Estimated: 20 minutes.

The ready queue starts with 4 P0 bugs. Each has a full lab-grade description
in its bead. Execute them in parallel (disjoint files):

```bash
br ready | head -4    # Show the 4 P0 bugs
br show bd-7fub.1.1   # Read each bead's full specification before working
```

After fixing all 4:

```bash
br close bd-7fub.1.1 --reason "Fixed: publish_and_check returns () not Result"
br close bd-7fub.1.2 --reason "Fixed: removed 3 unused imports from db/tests.rs"
br close bd-7fub.1.3 --reason "Fixed: ref got in test_schema.rs:212,256"
br close bd-7fub.1.4 --reason "Fixed: cloned_ref_to_slice_refs source"
```

Then verify (bd-7fub.1.5):

```bash
CARGO_TARGET_DIR=/data/cargo-target cargo check --workspace --all-targets
CARGO_TARGET_DIR=/data/cargo-target cargo clippy --workspace --all-targets -- -D warnings
CARGO_TARGET_DIR=/data/cargo-target cargo clippy --workspace --lib -- -D warnings \
  -D clippy::unwrap_used -D clippy::expect_used -D clippy::panic
cargo fmt --all -- --check
PROPTEST_CASES=1000 CARGO_TARGET_DIR=/data/cargo-target cargo test --workspace
```

All 5 must exit 0. Then:

```bash
br close bd-7fub.1.5 --reason "All 5 quality gates pass"
br close bd-lplt --reason "Full regression: 287 tests pass, 0 failures"
```

**Gate**: Do not proceed to Phase 2 until all 5 commands exit 0.

---

## Phase 2: Close Phase 4a Defects (Tier 0.5)

**Goal**: Close 3 known defects that count against Code Quality and Performance scoring.

These are independent — run as parallel agents with disjoint file sets:

| Bead | File | What |
|------|------|------|
| bd-glir | `store/merge.rs` | Replace `build_live_set(datoms.iter())` with `merge_live_sets(&a.live_set, &b.live_set)` |
| bd-emt3 | `observer.rs` | Remove `recent.iter().cloned().collect()` clone in `publish` |
| bd-9h0o | `checkpoint.rs` | Bump Length field from u32 to u64, update HEADER_SIZE 18→22, version 1→2 |

Each bead has a full lab-grade description. Read it with `br show <id>` before starting.

After each fix: run the 5 quality gates, close the bead with evidence.

**Gate**: All 3 defect beads closed. `br list --status=open --labels=phase-4a | grep bug` returns < 5 results.

---

## Phase 3: Verification Depth (Tiers 2-7)

**Goal**: Every Phase 4a INV-FERR at 4+ independent verification layers.

This is the largest phase (~5-6 sessions). Work the beads from the ready queue.
Organize as parallel swarm with disjoint file sets:

### Agent allocation (recommended)

| Agent | Focus | Beads | Files (exclusive) |
|-------|-------|-------|-------------------|
| A | Lean proofs | bd-7fub.2.11..2.19 | `lean/Ferratomic/*.lean` |
| B | Kani harnesses | bd-7fub.14.11..14.17 | `src/kani/*.rs` |
| C | Stateright models | bd-7fub.6.1..6.8 | `src/stateright_models/*.rs` |
| D | Integration tests | bd-7fub.6.9..6.12 + bd-7fub.23.1..23.2 | `integration/*.rs` |
| E | Durability tests | bd-7fub.19.1..19.6 | `integration/test_recovery.rs`, `proptest/fault_recovery_properties.rs` |
| F | CI-FERR + completeness | bd-7fub.14.7..14.10 + bd-7fub.23.3 | `proptest/conformance.rs`, `invariant_catalog.rs` |

Each agent reads its bead with `br show <id>`, executes the specification, closes the bead.

**Lean proof protocol**: Use [02-lean-proofs.md](lifecycle/02-lean-proofs.md).
If a proof requires `sorry` after reasonable effort, stop and escalate — may indicate spec error.

**Agents MUST NOT run cargo.** The orchestrator runs build/test ONCE after all agents complete.

**Gate**: After all agents finish, orchestrator runs full verification:
```bash
CARGO_TARGET_DIR=/data/cargo-target cargo check --workspace --all-targets
CARGO_TARGET_DIR=/data/cargo-target cargo clippy --workspace --all-targets -- -D warnings
PROPTEST_CASES=1000 CARGO_TARGET_DIR=/data/cargo-target cargo test --workspace
cd ferratomic-verify/lean && lake build   # 0 sorry
```

---

## Phase 4: Documentation + Polish (Tiers 5, 8, 9, 10)

**Goal**: Every trait contract documented, every error variant has recovery guidance,
every O(n) operation justified, every function traces to named INV-FERR.

Parallel swarm, disjoint files:

| Agent | Beads | Focus |
|-------|-------|-------|
| G | bd-7fub.3.13..3.20 | Performance thresholds + O(n) documentation |
| H | bd-7fub.4.7..4.13 | Trait contract docs + API/wire boundary audits |
| I | bd-7fub.12.1..12.6 | Error recovery guidance + usage examples |
| J | bd-7fub.15.1..15.6 | Axiological traceability + spec audit |

**Spec audit (bd-7fub.15.5)** uses [17-spec-audit.md](lifecycle/17-spec-audit.md).
This must complete before the cleanroom review.

**Gate**: `cargo doc --workspace` clean. `#![deny(missing_docs)]` passes.

---

## Phase 5: Gate Chain

**Goal**: Tag the release and prepare for cleanroom review.

```bash
br close bd-y1w5 --reason "Tagged v0.4.0-gate"
git tag v0.4.0-gate -m "Phase 4a gate closure candidate"
```

Install pre-commit hook (bd-7fub.18):
```bash
cat > .git/hooks/pre-commit << 'HOOK'
#!/bin/bash
export CARGO_TARGET_DIR=/data/cargo-target
cargo check --workspace --all-targets && \
cargo clippy --workspace --all-targets -- -D warnings && \
cargo fmt --all -- --check
HOOK
chmod +x .git/hooks/pre-commit
br close bd-7fub.18 --reason "Pre-commit hook installed, tested"
```

Pin toolchain (bd-7fub.24):
```bash
cat > rust-toolchain.toml << 'TOML'
[toolchain]
channel = "nightly-2026-02-20"
TOML
br close bd-7fub.24 --reason "Toolchain pinned to nightly-2026-02-20"
```

---

## Phase 6: Cleanroom Review Loop

**This is where 10.0 is won or lost.** The loop runs until the re-review confirms 10.0.

### Step 1: Cleanroom review (bd-7fub.22.3)

Use [06-cleanroom-review.md](lifecycle/06-cleanroom-review.md). Adversarial — actively try
to break every invariant. Document findings.

```bash
br close bd-7fub.22.3 --reason "Cleanroom review complete: N findings documented"
```

### Step 2: File findings (bd-7fub.22.8)

Every defect becomes a bead with lab-grade description per [08-task-creation.md](lifecycle/08-task-creation.md).

```bash
br create --title "CR-NNN: <finding>" --type bug --priority N --labels "phase-4a" \
  --description "..."
br close bd-7fub.22.8 --reason "Filed N findings as beads"
```

### Step 3: Close findings (bd-7fub.22.9)

Fix every finding. Run quality gates after each fix.

```bash
br close bd-7fub.22.9 --reason "All N findings closed and verified"
```

### Step 4: Re-review (bd-7fub.22.10)

Run [13-progress-review.md](lifecycle/13-progress-review.md) deep mode.

```bash
# This is the gate predicate:
# IF composite == 10.0 on all 10 vectors:
br close bd-7fub.22.10 --reason "Progress review: 10.0/A+ confirmed. All 10 vectors at 10.0."
br close bd-add --reason "Phase 4a gate: CLOSED. 10.0/A+ confirmed by T11-012."

# IF composite < 10.0:
# DO NOT close bd-7fub.22.10. File remaining gaps. Return to Step 1.
```

**This loop is the structural guarantee.** The gate cannot close without 10.0.

---

## Demonstration: Executing One Bead

Here is the complete workflow for one bead, from ready queue to closure:

```bash
# 1. Pick from ready queue
br ready | head -1
# → bd-7fub.1.1: Fix clippy unnecessary_wraps in production code

# 2. Read the full specification
br show bd-7fub.1.1
# → Spec ref: NEG-FERR-001
# → File: ferratomic-core/src/db/transact.rs:146
# → Fix: Change return type from Result<(), FerraError> to ()
# → Postcondition: cargo clippy --workspace --lib -- -D warnings exits 0

# 3. Read the code
# (read transact.rs, find publish_and_check at line 146)

# 4. Make the change
# (change return type, remove Ok(()) at line 169, update call sites)

# 5. Verify the postcondition
CARGO_TARGET_DIR=/data/cargo-target cargo clippy --workspace --lib -- -D warnings
# → exits 0 ✓

# 6. Close the bead with evidence
br close bd-7fub.1.1 --reason "Fixed: publish_and_check returns () not Result<(), FerraError>. clippy --lib passes."

# 7. Check what's next
br ready | head -3
```

That's it. Read the bead. Do what it says. Verify the postcondition. Close with evidence. Next.

---

## Swarm Orchestration

For parallel execution across Phases 3-4, use [multi-agent-swarm-workflow](../../AGENTS.md) principles:

1. **Assign disjoint file sets.** Two agents NEVER edit the same file.
2. **Agents do NOT run cargo.** Orchestrator builds once after all complete.
3. **Communicate via beads.** If Agent A's fix breaks Agent B's assumption, file a bead.
4. **No worktrees.** `isolation: "worktree"` is FORBIDDEN (corrupts .beads/ state).
5. **Session end**: Each agent runs [09-continuation.md](lifecycle/09-continuation.md).

### Orchestrator checklist (after each swarm batch)

```bash
CARGO_TARGET_DIR=/data/cargo-target cargo check --workspace --all-targets
CARGO_TARGET_DIR=/data/cargo-target cargo clippy --workspace --all-targets -- -D warnings
CARGO_TARGET_DIR=/data/cargo-target cargo clippy --workspace --lib -- -D warnings \
  -D clippy::unwrap_used -D clippy::expect_used -D clippy::panic
cargo fmt --all -- --check
PROPTEST_CASES=1000 CARGO_TARGET_DIR=/data/cargo-target cargo test --workspace
bv --robot-alerts    # Must be 0
```

---

## Stop Conditions

Stop and escalate to the user if:
- A Lean proof requires `sorry` after reasonable effort (may indicate spec error)
- The cleanroom review finds a correctness bug (INV-FERR violation in production code)
- Any quality gate cannot be made to pass after 3 attempts
- Composite score stalls below 9.5 after 2 iterations of the cleanroom loop
- A bead's postcondition contradicts another bead's frame condition (graph design error)

---

## Hard Constraints

- `#![forbid(unsafe_code)]` in all crates
- No `unwrap()` or `expect()` in production code
- `export CARGO_TARGET_DIR=/data/cargo-target` (NOT /tmp)
- Zero clippy suppressions — fix root cause, never `#[allow(...)]`
- Phase 4a gate (bd-add) depends on T11-012. Gate CANNOT close without 10.0.
- Agents MUST NOT run cargo — orchestrator only
- NO worktrees — disjoint file sets only
- Every commit passes all 5 quality gates (pre-commit hook enforces this)

---

## Success Criteria

The session succeeds when:

```
$ bv --robot-triage | jq '.triage.quick_ref.open_count'
0  (all Phase 4a beads closed)

$ br show bd-add
✓ CLOSED — "Phase 4a gate: CLOSED. 10.0/A+ confirmed by T11-012."

$ br show bd-7fub.22.10
✓ CLOSED — "Progress review: 10.0/A+ confirmed."
```

Phase 4a is done. Phase 4b begins.
