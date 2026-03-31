# Refinement Chains

> Structured correctness arguments for Ferratomic's critical execution paths.
>
> Each chain transforms an abstract specification into executable code through
> a sequence of refinement steps, where each step is justified by a named
> refinement law and a proof obligation that maps to a specific INV-FERR.
>
> **Methodology**: Morgan's refinement calculus (1990). Notation follows
> the specification statement `w: [pre, post]` with refinement ordering `sqsubseteq`.
> See spec/07-refinement.md for the coupling invariant that connects
> the Lean model to the Rust implementation.

---

## 1. Transaction Pipeline (`Database::transact`)

### Abstract Specification

```
transact : (Database, Transaction<Committed>) -> Result<TxReceipt, FerraError>

w = {datoms, indexes, schema, epoch, wal}

w: [valid(tx) /\ epoch = e /\ datoms = D /\ schema = S,
    datoms' = D  union  stamp(tx, e+1, tx.agent)  union  tx_meta(e+1, tx.agent)
    /\ epoch' = e + 1
    /\ schema' = evolve(S, datoms' \ D)
    /\ indexes' = project(datoms')
    /\ durable(datoms' \ D)
    /\ visible(datoms', epoch') ]

-- Postcondition components:
-- (a) datoms grow by stamped user datoms + tx metadata    [INV-FERR-004]
-- (b) epoch strictly advances                              [INV-FERR-007]
-- (c) schema evolves from new datoms                       [INV-FERR-009]
-- (d) indexes track primary                                [INV-FERR-005]
-- (e) new datoms are durable before visibility             [INV-FERR-008]
-- (f) new state is atomically visible to readers           [INV-FERR-006]
```

### Refinement Chain

#### Step 0 -> 1: Introduce write serialization (Law 4.2 — alternation)

```
w: [pre, post]
  sqsubseteq  {INV-FERR-021, INV-FERR-007}
if try_lock(write_lock) ->
    w: [pre /\ lock_held, post]
[] ~try_lock(write_lock) ->
    Err(Backpressure)
fi

Proof obligation:
  pre => try_lock(write_lock) \/ ~try_lock(write_lock)
  -- Trivially true: try_lock always returns one or the other.
  -- INV-FERR-021: Backpressure is a typed error, not a block.
  -- INV-FERR-007: Lock ensures at most one writer, so epoch
  --   ordering is strict (no concurrent epoch assignment).
```

**Code**: `db.rs:277-280` — `self.write_lock.try_lock().map_err(|_| FerraError::Backpressure)`

---

#### Step 1 -> 2: Separate mutation from publication (Law 3.3 — sequential composition)

```
w: [pre /\ lock_held, post]
  sqsubseteq  {INV-FERR-006}
w_internal: [pre /\ lock_held,
             new_store = apply(clone(current), tx)];
w_visible:  [new_store = apply(clone(current), tx),
             visible(new_store) /\ durable(delta(new_store, current))]

Mid-predicate: new_store contains the complete new state but is NOT yet
  visible to readers. current is the old state still served to readers.

Proof obligation:
  The mid-predicate does not contain initial variables (Morgan Law 8.4).
  INV-FERR-006: readers loading between steps 1 and 2 see the OLD state,
    because ArcSwap still points to current. Snapshot isolation is
    maintained by the separation of mutation from publication.
```

**Code**: `db.rs:284-286` — `Store::clone(&current)` + `new_store.transact(transaction)`

---

#### Step 2 -> 3: Decompose apply into sub-steps (Law 3.3 — nested sequential composition)

```
w_internal: [pre /\ lock_held, new_store = apply(clone(current), tx)]
  sqsubseteq  {INV-FERR-007, INV-FERR-015, INV-FERR-004, INV-FERR-009, INV-FERR-005}

-- Step 2a: Advance epoch
epoch: [epoch = e, epoch' = e + 1]
  -- INV-FERR-007: checked_add prevents overflow.
  -- Proof obligation: e < u64::MAX (epoch never overflows in practice;
  --   InvariantViolation error if it does).

-- Step 2b: Stamp datoms with real TxId
stamped: [epoch' = e + 1 /\ tx.datoms = [d1..dn],
          stamped = [d1[tx:=TxId(e+1, 0, agent)]..dn[tx:=TxId(e+1, 0, agent)]]]
  -- INV-FERR-015: replaces placeholder TxId(0,0,0) with monotonically
  --   increasing real TxId. The epoch component guarantees ordering.
  -- Proof obligation: TxId::with_agent(e+1, 0, agent) > all previous TxIds
  --   because e+1 > e and epoch is the dominant sort key in TxId::Ord.

-- Step 2c: Create tx metadata datoms
all_datoms: [stamped, all_datoms = stamped ++ [tx/time, tx/agent]]
  -- INV-FERR-004: |all_datoms| >= |stamped| + 2, guaranteeing strict growth
  --   even if all user datoms are duplicates.
  -- Proof obligation: tx_entity is fresh (content-addressed from epoch + agent,
  --   both unique to this transaction).

-- Step 2d: Evolve schema
schema: [all_datoms, schema' = evolve(schema, all_datoms)]
  -- INV-FERR-009: scans for (E, db/ident, _), (E, db/valueType, _),
  --   (E, db/cardinality, _) triples and installs new attributes.
  -- Proof obligation: evolve is monotone (never removes attributes).

-- Step 2e: Insert into primary + indexes
datoms, indexes: [all_datoms /\ schema',
                  datoms' = datoms union all_datoms
                  /\ indexes' = project(datoms')]
  -- INV-FERR-005: bijection maintained by inserting each datom into
  --   both the primary OrdSet and all four index OrdSets.
  -- INV-FERR-003: duplicate insertion is a no-op (OrdSet semantics).
  -- Proof obligation: for each d in all_datoms:
  --   d in datoms' /\ d in eavt' /\ d in aevt' /\ d in vaet' /\ d in avet'
```

**Code**: `store.rs:362-441` — the `Store::transact` method body.

---

#### Step 3 -> 4: WAL before publication (Law 3.3 — sequential composition with ordering constraint)

```
w_visible: [new_store ready, visible(new_store) /\ durable(delta)]
  sqsubseteq  {INV-FERR-008}
wal: [new_store ready /\ delta = diff(new_store, current),
      wal_contains(delta) /\ fsynced(wal)];
      -- Mid-predicate: durable(WAL(T)) -- the WAL is fsynced.
swap: [fsynced(wal),
       visible(new_store)]

The critical ordering constraint (INV-FERR-008):
  durable(WAL(T)) BEFORE visible(SNAP(e))

Proof obligation:
  The WAL append + fsync completes BEFORE ArcSwap::store(). This is
  enforced by sequential execution within the write lock (no async,
  no reordering). The Rust memory model guarantees that the fsync
  system call returns only after data is on stable storage.

  If the WAL step fails (IO error), the function returns Err and the
  ArcSwap is never updated. Readers continue seeing the old state.
  The transaction is lost, but no committed data is lost. This is the
  "fail-safe" property: failure modes preserve existing state.
```

**Code**: `db.rs:296-303` (WAL), `db.rs:308` (ArcSwap store)

---

#### Step 4 -> 5: Atomic publication (Law 1.3 — assignment)

```
swap: [fsynced(wal), visible(new_store)]
  sqsubseteq  {INV-FERR-006, NEG-FERR-004}
self.current.store(Arc::new(new_store))

Proof obligation (Morgan Law 1.3 / assignment):
  fsynced(wal) /\ new_store = apply(clone(current), tx)
  => visible(new_store)[current \ Arc::new(new_store)]

  ArcSwap::store is an atomic pointer swap. After this line:
  - All subsequent snapshot() calls load the new Arc<Store>.
  - Existing readers holding old Arc<Store> are unaffected (reference
    counted; im::OrdSet nodes are shared, not copied).
  - NEG-FERR-004: no reader obtains a stale snapshot after publication.
```

**Code**: `db.rs:308` — `self.current.store(Arc::new(new_store))`

---

### Complete Chain Summary

```
SPEC: w: [valid(tx) /\ epoch = e, datoms' /\ epoch' /\ durable /\ visible]
  sqsubseteq [Law 4.2, INV-FERR-021]          -- write serialization
  sqsubseteq [Law 3.3, INV-FERR-006]          -- separate mutation from publication
  sqsubseteq [Law 3.3, INV-FERR-007/015/004/009/005]  -- decompose apply
  sqsubseteq [Law 3.3, INV-FERR-008]          -- WAL before publication
  sqsubseteq [Law 1.3, INV-FERR-006/NEG-004]  -- atomic assignment
CODE: try_lock; clone+stamp+meta+schema+insert; wal+fsync; arcswap.store
```

Each `sqsubseteq` is justified by a refinement law whose proof obligation is
discharged by the cited invariant. The composition is valid by transitivity
of refinement.

---

## 2. Crash Recovery (`Database::recover`)

### Abstract Specification

```
recover : (checkpoint_path, wal_path) -> Result<Database, FerraError>

w = {store}

w: [exists valid checkpoint at checkpoint_path
    /\ exists valid WAL at wal_path,
    store' = last_committed_state_before_crash]

-- Where last_committed_state is defined as:
--   genesis
--   |> apply(tx_1) |> apply(tx_2) |> ... |> apply(tx_n)
-- for all transactions tx_1..tx_n that were successfully committed
-- (i.e., WAL fsynced) before the crash.
```

### Refinement Chain

#### Step 0 -> 1: Decompose into checkpoint + WAL replay (Law 3.3)

```
w: [pre, store' = last_committed]
  sqsubseteq  {INV-FERR-013, INV-FERR-014}
base: [valid(checkpoint),
       base_store = load(checkpoint)
       /\ base_store.epoch = checkpoint_epoch];
replay: [base_store /\ valid(wal),
         store' = base_store |> apply_wal_entries(epoch > checkpoint_epoch)]

Mid-predicate:
  base_store = state_at_epoch(checkpoint_epoch)
  -- INV-FERR-013 guarantees: load(checkpoint(S)) = S
  -- Therefore base_store faithfully represents the state at checkpoint time.

Proof obligation:
  last_committed = base_store |> apply(entries after checkpoint_epoch)
  -- This holds because:
  --   (a) checkpoint captures ALL datoms up to checkpoint_epoch [INV-FERR-013]
  --   (b) WAL contains ALL entries after checkpoint_epoch [INV-FERR-008]
  --   (c) The union of these two sets = all committed datoms [INV-FERR-014]
  -- This is exactly Morgan's sequential composition law with the
  -- checkpoint epoch as the mid-predicate boundary.
```

**Code**: `db.rs:142-167` — `Database::recover`

---

#### Step 1a: Load checkpoint (Law 1.3 with data refinement)

```
base: [valid(checkpoint), base_store = load(checkpoint)]
  sqsubseteq  {INV-FERR-013}
-- Verify BLAKE3 hash (tamper detection)
-- Parse header (magic, version, epoch)
-- Deserialize JSON payload to (schema, genesis_agent, datoms)
-- Reconstruct Store via from_checkpoint (rebuilds indexes)
base_store := load_checkpoint(checkpoint_path)?

Proof obligation (INV-FERR-013 — round-trip identity):
  For all valid stores S:
    load_checkpoint(write_checkpoint(S)) = S
  Specifically:
    datom_set equality, epoch equality, schema equality,
    index bijection after reconstruction (INV-FERR-005).
```

**Code**: `checkpoint.rs:157-183` — `load_checkpoint`

---

#### Step 1b: Replay WAL suffix (Law 5.5 — iteration)

```
replay: [base_store, store' = base_store |> entries_after(checkpoint_epoch)]
  sqsubseteq  {INV-FERR-014, INV-FERR-008}
var store := base_store;
for entry in wal.recover()? {
    if entry.epoch > checkpoint_epoch {
        let datoms = deserialize(entry.payload)?;
        for datom in datoms {
            store.insert(datom);  -- INV-FERR-004: monotonic growth
        }
    }
}

Invariant: store = base_store |> entries_up_to(current_entry)
Variant: remaining_entries (decreases by 1 each iteration, bounded below by 0)

Proof obligation:
  (a) INV-FERR-008: every committed transaction has a corresponding valid
      WAL frame (fsynced before epoch advance).
  (b) INV-FERR-014: wal.recover() truncates incomplete frames, so only
      fully-written entries are replayed. Incomplete = uncommitted.
  (c) Recovered datoms carry real TxIds (stamped before WAL write),
      so direct insertion produces identical state to original transact.
  (d) INV-FERR-003: duplicate insertion is idempotent. If a datom from
      the WAL is already in the checkpoint, re-insertion is a no-op.
```

**Code**: `db.rs:148-159` — WAL replay loop in `Database::recover`

---

### Recovery Chain Summary

```
SPEC: w: [valid checkpoint /\ valid WAL, store' = last_committed]
  sqsubseteq [Law 3.3, INV-FERR-013/014]   -- checkpoint + WAL decomposition
  sqsubseteq [Law 1.3, INV-FERR-013]        -- load checkpoint
  sqsubseteq [Law 5.5, INV-FERR-014/008]    -- replay WAL suffix (iteration)
CODE: load_checkpoint(path)?; for entry in wal: if epoch > cp_epoch: insert(datoms)
```

The **shared mid-predicate** at the WAL boundary connects the transaction
chain and the recovery chain:

```
Transaction side:                Recovery side:
... stamp; insert;               load(checkpoint) = state_at(cp_epoch);
WAL.append(delta); WAL.fsync;    for entry in WAL where epoch > cp_epoch:
ArcSwap.store(new)                 store.insert(entry.datoms)
       |                                    |
       v                                    v
  durable(WAL(T))     ===     WAL contains entry
```

The WAL is the serialization boundary where "transaction correctness" hands
off to "recovery correctness." INV-FERR-008 (fsync ordering) is the proof
obligation that bridges them: what the transaction made durable, recovery
can replay.

---

## 3. CRDT Merge (`merge(A, B)`)

### Abstract Specification

```
merge : (Store, Store) -> Store

w = {result}

w: [true,
    result.datoms = A.datoms union B.datoms
    /\ result.schema = A.schema union B.schema
    /\ result.epoch = max(A.epoch, B.epoch)]
```

### Refinement Chain

#### Step 0 -> 1: Set union via im::OrdSet (Law 1.3 with data refinement)

```
w: [true, result.datoms = A union B]
  sqsubseteq  {INV-FERR-001, INV-FERR-002, INV-FERR-003, ADR-FERR-001}
result.datoms := A.datoms.clone().union(B.datoms.clone())

Proof obligation (data refinement — coupling invariant CI):
  CI(Finset, OrdSet) => Finset.union(A, B) = to_finset(OrdSet.union(A, B))
  -- im::OrdSet.union implements set union correctly.
  -- INV-FERR-001: OrdSet.union is commutative (same elements either way).
  -- INV-FERR-002: OrdSet.union is associative (structural sharing is order-independent).
  -- INV-FERR-003: OrdSet.union is idempotent (inserting existing = no-op).

Algebraic properties inherited from set union:
  -- Commutativity: A union B = B union A [INV-FERR-001]
  -- Associativity: (A union B) union C = A union (B union C) [INV-FERR-002]
  -- Idempotency: A union A = A [INV-FERR-003]
  -- Monotonicity: A subset (A union B) [INV-FERR-004]
```

**Code**: `store.rs:307-338` — `Store::from_merge`

#### Step 1 -> 2: Rebuild indexes from merged primary (Law 3.3)

```
result.indexes: [result.datoms, indexes' = project(result.datoms)]
  sqsubseteq  {INV-FERR-005}
result.indexes := Indexes::from_primary(&result.datoms)

Proof obligation:
  Every datom in result.datoms appears in all four indexes.
  No datom in any index is absent from result.datoms.
  -- INV-FERR-005: bijection by construction (from_primary iterates
  --   the primary set and inserts each datom into all indexes).
```

**Code**: `store.rs:329` — `Indexes::from_primary(&merged_datoms)`

#### Step 2 -> 3: Schema merge + epoch max (Law 1.3)

```
result.schema: [A.schema, B.schema,
                result.schema = A.schema union B.schema]
result.epoch:  [A.epoch, B.epoch,
                result.epoch = max(A.epoch, B.epoch)]
  sqsubseteq  {INV-FERR-043, INV-FERR-007}

Proof obligation:
  -- INV-FERR-043: shared attributes must have identical definitions.
  --   debug_assert! checks this at merge time.
  -- INV-FERR-007: max epoch preserves the "most advanced" timeline.
  --   After merge, the epoch is at least as large as both inputs,
  --   so subsequent transact produces a strictly larger epoch.
```

**Code**: `store.rs:317-326` — schema union + `max(a.epoch, b.epoch)`

---

## Appendix: Refinement Law Reference

| Law | Name | Morgan Reference | Application |
|-----|------|-----------------|-------------|
| 1.1 | Strengthen postcondition | Ch. 1, p. 10 | Deriving stronger guarantees from weaker specs |
| 1.2 | Weaken precondition | Ch. 1, p. 12 | Handling more initial states |
| 1.3 | Assignment | Ch. 5, p. 55 | Final code: `w := E` implements `w: [pre, post]` |
| 3.3 | Sequential composition | Ch. 3, p. 31 | Decomposing into steps with mid-predicates |
| 4.2 | Alternation (if-fi) | Ch. 4, p. 39 | Branching on guards (try_lock success/failure) |
| 5.5 | Iteration (do-od) | Ch. 5, p. 60 | Loops with invariant + variant (WAL replay) |
| 17.15 | Data refinement (specification) | Ch. 17, p. 203 | Abstract -> concrete type with coupling invariant |

## Appendix: Invariant Cross-Reference

| Refinement Step | Invariants Used | Chain |
|----------------|-----------------|-------|
| Write serialization | INV-FERR-021, INV-FERR-007 | Transaction |
| Mutation/publication separation | INV-FERR-006 | Transaction |
| Epoch advance | INV-FERR-007 | Transaction |
| TxId stamping | INV-FERR-015 | Transaction |
| Tx metadata creation | INV-FERR-004 | Transaction |
| Schema evolution | INV-FERR-009 | Transaction |
| Index insertion | INV-FERR-005, INV-FERR-003 | Transaction |
| WAL append + fsync | INV-FERR-008 | Transaction, Recovery |
| Atomic swap | INV-FERR-006, NEG-FERR-004 | Transaction |
| Checkpoint load | INV-FERR-013 | Recovery |
| WAL replay iteration | INV-FERR-014, INV-FERR-008 | Recovery |
| Datom set union | INV-FERR-001/002/003/004, ADR-FERR-001 | Merge |
| Index rebuild | INV-FERR-005 | Merge |
| Schema union + epoch max | INV-FERR-043, INV-FERR-007 | Merge |
