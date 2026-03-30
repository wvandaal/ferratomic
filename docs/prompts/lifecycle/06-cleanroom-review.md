# 06 — Cleanroom Review & Audit

> **Purpose**: Post-implementation adversarial review. Find what is wrong.
> You are not here to confirm correctness. You are here to find defects.
>
> **DoF**: High (discovery). Explore freely. Question everything.

---

## Phase 0: Load Context

```bash
ms load prompt-optimization -m --pack 2000  # Review/audit skill
bv --robot-next                             # Top-priority pick
br update <id> --status in_progress         # Claim it
```

---

## Mindset

Assume the implementation has bugs. Your job is to find them.
Every module is guilty until proven innocent by evidence
(passing tests, matching proofs, spec alignment).

---

## 8 Review Phases

Execute in order. Each phase produces findings or a clean bill.
Do not skip phases even if earlier ones found nothing.

### Phase 1: Algebraic Correctness

Does the implementation preserve the algebraic laws?

- For each INV-FERR-001..004: does the code structurally match set union?
- Are there any order-dependent operations in merge paths?
- Does transact preserve monotonicity (new datoms only, never remove)?
- Do index rebuilds produce identical results for identical inputs?

### Phase 2: Invariant Integrity

For every INV-FERR referenced in doc comments:

- Is the claim true? Trace the code path. Can you construct an input
  that violates the stated invariant?
- Does the falsification condition from the spec have a corresponding test?
- Are there invariants claimed in comments but not tested?
- Are there invariants tested but not claimed in comments?

### Phase 3: Type-Theoretic Analysis

Do the types encode what they claim?

- Can any type represent an invalid state? (e.g., `EntityId` from arbitrary bytes
  that aren't a BLAKE3 hash)
- Are there `pub` fields that bypass constructors?
- Is typestate used where lifecycle phases exist?
- Are enum matches exhaustive (no `_ =>` wildcards)?
- Are error types categorized correctly (retryable vs caller-bug vs our-bug)?

### Phase 4: Performance

Does the implementation meet the spec's performance contracts?

- INV-FERR-025: Are index lookups O(log n)?
- INV-FERR-026: Is write amplification bounded?
- Are there O(n) operations hidden inside O(1) interfaces?
- Are allocations minimized? (Clone where borrow suffices? Vec where iterators suffice?)
- Is the fast path for self-merge (INV-FERR-003) actually taken?

### Phase 5: Test Adequacy

Do the tests actually verify the invariants?

- Run `CARGO_TARGET_DIR=/data/cargo-target cargo test --workspace`. All pass?
- Are proptest case counts >= 10,000 for algebraic laws?
- Do failure messages cite INV-FERR IDs?
- Are there invariants with no test? (Cross-reference spec vs test names.)
- Are there tests that can never fail? (Tautological assertions, trivial inputs.)
- Do integration tests cover error paths, not just happy paths?

### Phase 6: Error Handling

Can the system fail gracefully?

- Are all `?` propagations correct? (Not swallowing context, not converting
  specific errors to generic ones.)
- Are there any `unwrap()` or `expect()` in production code?
- Does `FerraError::InvariantViolation` contain enough context to diagnose?
- Are I/O errors (disk full, permission denied) handled or silently propagated?

### Phase 7: Documentation

Does the documentation match reality?

- Do doc comments describe what the function DOES, not what it SHOULD do?
- Are INV-FERR references accurate? (The cited invariant matches the function's role.)
- Are there aspirational docs describing unimplemented behavior?
- Does `AGENTS.md` reflect the current phase and constraints?

### Phase 8: Defect Register

Compile all findings into a defect register. Every finding gets a record.

---

## Defect Format

```markdown
### [SEVERITY] DEFECT-NNN: Short title

**Location**: `crate/src/module.rs:42`
**Traces to**: INV-FERR-NNN
**Evidence**: [what you observed]
**Expected**: [what the spec/proof/test requires]
**Fix**: [concrete action]
```

Severity levels:
- **CRITICAL**: Invariant violation. Algebraic law broken. Data loss possible.
- **MAJOR**: Correctness issue that does not violate a named invariant but
  will cause bugs under specific conditions.
- **MINOR**: Code quality issue. Suboptimal but correct.
- **STYLE**: Naming, formatting, documentation. No behavioral impact.

---

## Demonstration: One Defect at Each Severity

### [CRITICAL] DEFECT-001: merge drops datoms when stores share entity IDs

**Location**: `ferratomic-core/src/merge.rs:38`
**Traces to**: INV-FERR-001
**Evidence**: When both stores contain datoms for entity `e1` but with
different attributes, the merge deduplicates by entity ID instead of
by the full 5-tuple `[e, a, v, tx, op]`. Store A has `[e1, "name", ...]`,
store B has `[e1, "age", ...]`. After merge, only one survives.
**Expected**: Both datoms preserved. Datom identity is the full 5-tuple
(INV-FERR-012), not just the entity ID.
**Fix**: Change deduplication key from `EntityId` to `Datom` (the full tuple).

### [MAJOR] DEFECT-002: WAL fsync ordering not enforced on ext4

**Location**: `ferratomic-core/src/wal.rs:91`
**Traces to**: INV-FERR-008
**Evidence**: The code calls `file.sync_all()` but does not fsync the
parent directory. On ext4 with `data=writeback`, the WAL entry may not
be durable before the snapshot references it.
**Expected**: Two-fsync barrier: (1) fsync WAL file, (2) fsync parent directory.
**Fix**: Add `File::open(parent_dir)?.sync_all()?` after WAL fsync.

### [MINOR] DEFECT-003: Clone in merge loop where borrow suffices

**Location**: `ferratomic-core/src/merge.rs:45`
**Traces to**: INV-FERR-025 (performance)
**Evidence**: `datom.clone()` inside the merge loop allocates for every
datom in the smaller store. BTreeSet::insert takes ownership, but we
could use `Cow` or restructure to avoid the clone for datoms already present.
**Expected**: Avoid allocation for datoms that are already in the set.
**Fix**: Check membership before cloning: `if !datoms.contains(datom) { datoms.insert(datom.clone()); }`

### [STYLE] DEFECT-004: Inconsistent doc comment format

**Location**: `ferratom/src/datom.rs:12`
**Traces to**: Documentation standards (AGENTS.md)
**Evidence**: Doc comment says "Returns the entity" without citing INV-FERR-012.
**Expected**: All doc comments on public items cite the relevant INV-FERR.
**Fix**: Add `(INV-FERR-012)` to the doc comment.

---

## Output

The review produces a single document: the defect register.

Structure:
1. **Summary**: N critical, N major, N minor, N style findings.
2. **Critical findings** (must fix before merge).
3. **Major findings** (should fix before merge).
4. **Minor findings** (fix when convenient).
5. **Style findings** (batch with other cleanup).
6. **Clean phases**: list any phases that found zero defects.

File defects as beads issues:
```bash
br create --title "DEFECT-001: merge drops datoms on shared entity IDs" \
  --type bug --priority 0
```

CRITICAL = P0. MAJOR = P1. MINOR = P2. STYLE = P3.

---

## What NOT To Do

- Do not confirm correctness. Find defects.
- Do not stop after finding one bug. Complete all 8 phases.
- Do not suggest improvements that aren't defects. This is a review, not a wishlist.
- Do not file defects without evidence. "This looks wrong" is not a finding.
  Show the input that triggers the bug or the spec clause that is violated.
- Do not fix defects during review. File them. Fixing is a separate task
  with its own test-implement-verify cycle.
