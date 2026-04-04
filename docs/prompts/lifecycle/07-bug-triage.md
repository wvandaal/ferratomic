# 07 Bug Triage & Resolution

> **Purpose**: Observe a defect, crystallize it, fix it, prove it won't recur.
> **DoF**: High for diagnosis, Low for fix.
> **Cognitive mode**: Forensic analysis then surgical repair.

---

## Severity Classification

| Level | Criteria | Response |
|-------|----------|----------|
| **CRITICAL** | C1/C2/C4 violation, data loss, INV-FERR falsification | Stop all work. Fix immediately. |
| **MAJOR** | Wrong result, spec divergence, test gap, performance regression >10% | Fix before session end. |
| **MINOR** | Cosmetic, doc error, suboptimal but correct behavior | File issue, fix when convenient. |

**Rule**: If you aren't sure, classify one level higher.

---

## Phase 0: Load Context

```bash
ms load spec-first-design -m --full   # Formal analysis of invariant violations
```

---

## Protocol

### 1. Observe (high DoF -- understand before acting)

```bash
# What failed?
CARGO_TARGET_DIR=/data/cargo-target cargo test --workspace 2>&1 | head -80

# What does the code actually do?
# Read the failing function. Read its callers. Read its spec reference.
# Trace the data flow from input to wrong output.
```

Ask three questions before touching code:
- Which INV-FERR does this violate? (If none, is it actually a bug?)
- What is the minimal reproducer?
- Why did existing tests miss this?

### 2. Crystallize (low DoF -- create the beads issue)

```bash
br create \
  --title "BUG: <what is wrong>" \
  --type bug \
  --priority <0-4> \
  --label "phase-4a" \
  --description "$(cat <<'BODY'
**Observed**: <what happens>
**Expected**: <what should happen per INV-FERR-NNN>
**Reproducer**: <minimal steps or test name>
**Root cause**: <hypothesis or TBD>
**Affected INV**: INV-FERR-NNN
BODY
)"
```

### 3. Root Cause Analysis

Work backward from the symptom:

1. **Reproduce**: Write a failing test FIRST. Name it `test_bug_<issue_id>_<description>`.
2. **Isolate**: Binary search the call chain. Which function first produces wrong output?
3. **Identify**: Is it a logic error, a missing case, a wrong assumption, or a spec misread?
4. **Trace**: Which INV-FERR is violated? If the bug reveals a spec gap, file a separate issue.

### 4. Fix (low DoF -- minimal correct change)

```bash
br update <id> --status in_progress
```

- Fix the root cause, not the symptom.
- The fix must not violate any other INV-FERR.
- If the fix touches more than 3 files, reconsider -- you may be fixing the wrong thing.

### 5. Verify

```bash
# The regression test you wrote in step 3 now passes
CARGO_TARGET_DIR=/data/cargo-target cargo test --workspace
CARGO_TARGET_DIR=/data/cargo-target cargo clippy --workspace -- -D warnings

# Close the issue
br close <id> --reason "Fixed: <one-line summary>"
```

**Full gate verification** (if the fix is non-trivial):
```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy --workspace --lib -- -D warnings \
  -D clippy::unwrap_used -D clippy::expect_used -D clippy::panic
cargo test --workspace
cargo deny check
```

### 6. Regression Guard

Every bug fix MUST include a test named `test_bug_<issue_id>_<description>`.
This test must fail without the fix and pass with it. No exceptions.

- **Fuzz corpus**: If the bug was found via fuzzing, add the crashing input to `fuzz/corpus/`. If the bug is in a deserialization/WAL/checkpoint path, consider adding a fuzz target if one doesn't exist.

See GOALS.md §6.9 for the full regression discipline.

---

## Demonstration: Full Lifecycle of a MAJOR Bug

**Discovery**: `cargo test` shows `test_inv_ferr_005_index_consistency` failing.
The EAVT index returns a datom that the AEVT index doesn't contain.

```bash
# Step 1: Observe
# Read the test output. The assertion shows:
#   EAVT contains [e1, :name, "alice", tx1, Assert]
#   AEVT lookup for :name returns [] (empty)
# Read store.rs apply_datoms(). Read the AEVT insertion logic.
# Hypothesis: AEVT insertion uses attribute.clone() but Attribute
# uses Arc<str> with interning -- a new Arc<str>("name") != the interned one.

# Step 2: Crystallize
br create \
  --title "BUG: AEVT index missing datoms after apply_datoms" \
  --type bug \
  --priority 1 \
  --label "phase-4a" \
  --description "$(cat <<'BODY'
**Observed**: Datoms inserted via apply_datoms appear in EAVT but not AEVT.
**Expected**: All four indexes contain every datom (INV-FERR-005).
**Reproducer**: test_inv_ferr_005_index_consistency
**Root cause**: AEVT key uses un-interned Attribute clone; equality check fails.
**Affected INV**: INV-FERR-005 (index bijection)
BODY
)"
# Output: Created br-42

# Step 3: Root cause analysis
br update br-42 --status in_progress

# Write the regression test:
# tests/bugs/test_bug_br42_aevt_missing_datoms.rs
# fn test_bug_br42_aevt_missing_datoms() {
#     let store = Store::empty();
#     let datom = make_test_datom("entity1", ":name", "alice");
#     let store = store.apply_datoms(&[datom.clone()])?;
#     // This is the bug: AEVT lookup returns empty
#     let found = store.aevt_lookup(&datom.attribute());
#     assert!(!found.is_empty(),
#         "INV-FERR-005: datom in EAVT must also appear in AEVT");
# }

# Confirm: test fails without fix (red).

# Step 4: Fix
# In store.rs, apply_datoms():
#   - was:   aevt.insert(AevtKey::new(attr.clone(), ...))
#   - fixed: aevt.insert(AevtKey::new(self.intern_attribute(attr), ...))
# One line changed. Root cause: un-interned attribute in AEVT key.

# Step 5: Verify
CARGO_TARGET_DIR=/data/cargo-target cargo test --workspace    # All pass
CARGO_TARGET_DIR=/data/cargo-target cargo clippy --workspace -- -D warnings

br close br-42 --reason "Fixed: AEVT index now uses interned Attribute for key equality"
```

**Post-fix check**: Run `bv --robot-triage` to see if this bug reveals other issues.

---

## Stop Conditions

- Bug reveals a spec contradiction -> file a spec issue, don't guess the resolution.
- Fix requires changing a public API -> escalate. API changes need an ADR.
- Fix touches > 3 files -> decompose. Follow [12-deep-analysis.md](12-deep-analysis.md) first.
- Root cause is in a dependency (im, blake3, etc.) -> file upstream issue, implement workaround.
- You can't reproduce it -> document the investigation, mark the issue with label "flaky".
