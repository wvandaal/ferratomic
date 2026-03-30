# 08 Task Creation & Crystallization

> **Purpose**: Create properly specified beads issues with dependency edges.
> **DoF**: Low. Task format is precisely defined.
> **Cognitive mode**: Specification (not discovery, not implementation).

---

## Phase 0: Load Context

```bash
ms load spec-first-design -m --full   # Spec decomposition for acceptance criteria
```

---

## Required Fields

Every issue needs ALL of these. Missing fields = incomplete crystallization.

| Field | Flag | Notes |
|-------|------|-------|
| Title | `--title` | Verb-first, under 80 chars. "Implement X", "Fix Y", "Test Z". |
| Type | `--type` | `task`, `bug`, `feature`, `epic`, `question`, `docs` |
| Priority | `--priority` | 0=critical, 1=high, 2=medium, 3=low, 4=backlog |
| Label | `--label` | Phase: `phase-1`, `phase-2`, `phase-3`, `phase-4a`, `phase-4b`, `phase-4c` |
| Description | `--description` | Structured body (see template below) |

### Description Template

```
**What**: One sentence. What changes when this is done?
**Why**: Which INV-FERR, ADR-FERR, or NEG-FERR does this serve?
**Acceptance**: Numbered list. Each item is pass/fail verifiable.
**File(s)**: Which file(s) will be created or modified.
**Depends on**: Issue IDs this blocks on (if any).
```

---

## Creating a Single Task

```bash
br create \
  --title "Implement HLC tick() monotonicity" \
  --type task \
  --priority 2 \
  --label "phase-4a" \
  --description "$(cat <<'BODY'
**What**: HybridClock::tick() returns strictly increasing TxId even under NTP regression.
**Why**: INV-FERR-015 (HLC monotonicity).
**Acceptance**:
1. tick() returns TxId > all previously issued TxIds.
2. NTP wall-clock regression does not cause TxId regression.
3. Logical counter increments when wall-clock is stale.
4. proptest: 10,000 random tick sequences are strictly ordered.
**File(s)**: ferratom/src/clock.rs
**Depends on**: None (leaf type, no internal deps).
BODY
)"
```

## Wiring Dependency Edges

Dependencies encode "X cannot start until Y is done." Use them for:
- Implementation depends on type definitions
- Tests depend on the function they test
- Integration depends on unit components

```bash
# Add a dependency: br-15 depends on br-12
br dep add br-15 br-12

# Verify: br-15 should NOT appear in ready list until br-12 is closed
br ready
```

**Rule**: Only add real dependencies. "Nice to have first" is not a dependency.
A depends on B means A literally cannot be implemented without B being done.

---

## Demonstration: Epic with 3 Child Tasks

Scenario: Implement snapshot isolation (INV-FERR-006, INV-FERR-007, INV-FERR-020).

```bash
# Step 1: Create the epic
br create \
  --title "EPIC: Snapshot isolation" \
  --type epic \
  --priority 1 \
  --label "phase-4a" \
  --description "$(cat <<'BODY'
**What**: Readers get consistent point-in-time views. Writers are serialized.
**Why**: INV-FERR-006 (snapshot isolation), INV-FERR-007 (write linearizability),
INV-FERR-020 (observer monotonicity).
**Acceptance**: All three child tasks pass. Stateright model verifies.
BODY
)"
# Output: Created br-50

# Step 2: Create child tasks
br create \
  --title "Implement Snapshot struct with Arc<StoreInner>" \
  --type task \
  --priority 1 \
  --label "phase-4a" \
  --parent br-50 \
  --description "$(cat <<'BODY'
**What**: Snapshot wraps Arc<StoreInner> for zero-copy consistent reads.
**Why**: INV-FERR-006 (snapshot isolation).
**Acceptance**:
1. Snapshot::read() returns data as of creation time.
2. Concurrent writes do not affect existing snapshots.
3. Snapshot is Send + Sync (can cross thread boundaries).
**File(s)**: ferratomic-core/src/snapshot.rs
**Depends on**: None.
BODY
)"
# Output: Created br-51

br create \
  --title "Implement WriterActor with mpsc serialization" \
  --type task \
  --priority 1 \
  --label "phase-4a" \
  --parent br-50 \
  --description "$(cat <<'BODY'
**What**: Single-writer serializes all mutations through mpsc channel.
**Why**: INV-FERR-007 (write linearizability).
**Acceptance**:
1. All writes go through WriterActor channel.
2. Concurrent write requests are serialized (total order).
3. Group commit batches writes within a configurable window.
**File(s)**: ferratomic-core/src/writer.rs
**Depends on**: br-51 (Snapshot struct must exist first).
BODY
)"
# Output: Created br-52

br create \
  --title "Stateright model for snapshot + writer interaction" \
  --type task \
  --priority 1 \
  --label "phase-4a" \
  --parent br-50 \
  --description "$(cat <<'BODY'
**What**: Model-check snapshot isolation under all interleavings.
**Why**: INV-FERR-006, INV-FERR-007 verified by exhaustive state exploration.
**Acceptance**:
1. Stateright explores all states without property violation.
2. Model covers: concurrent read+write, concurrent write+write, snapshot after close.
**File(s)**: ferratomic-verify/stateright/snapshot_model.rs
**Depends on**: br-51, br-52 (model tests the interaction).
BODY
)"
# Output: Created br-53

# Step 3: Wire dependency edges
br dep add br-52 br-51    # WriterActor depends on Snapshot
br dep add br-53 br-51    # Stateright model depends on Snapshot
br dep add br-53 br-52    # Stateright model depends on WriterActor

# Step 4: Verify the dependency graph
bv --robot-triage
# Should show:
# - br-51 as READY (no blockers)
# - br-52 blocked by br-51
# - br-53 blocked by br-51, br-52
# - br-51 recommended as next action (unblocks the most downstream work)

br ready
# Should show only br-51 (the only unblocked task)
```

---

## Task Quality Checklist

Before creating any task, verify:

- [ ] Title starts with a verb and is under 80 chars
- [ ] Description has all 5 sections (What/Why/Acceptance/Files/Depends)
- [ ] Each acceptance criterion is binary (pass or fail, no "mostly works")
- [ ] At least one acceptance criterion references an INV-FERR
- [ ] File paths are specific (not "somewhere in ferratomic-core")
- [ ] Dependencies are real (not aspirational ordering preferences)
- [ ] Priority reflects impact, not effort

---

## Bulk Operations

When crystallizing a batch of related tasks (e.g., from a design session):

```bash
# Create all tasks first, capture IDs
# Then wire all dependency edges
# Then verify the full graph

bv --robot-triage          # Ranked recommendations
bv --robot-plan            # Parallel execution tracks
bv --robot-insights        # Graph metrics (cycles, critical path)
```

**Warning**: `bv --robot-insights` will flag dependency cycles. Fix them immediately --
cycles mean your decomposition has a circular dependency, which is a design error.
