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

> **Staleness notice**: Code module references are current as of 2026-04-06.
> Line numbers are intentionally omitted -- use module paths and function
> names for navigation. The algebraic structure (refinement laws,
> mid-predicates, proof obligations) is stable; the concrete data types
> implementing the abstract operations evolve across phases.

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
if write_limiter.try_acquire() /\ try_lock(write_lock) ->
    w: [pre /\ lock_held /\ tx_id = clock.tick(), post]
[] ~write_limiter.try_acquire() \/ ~try_lock(write_lock) ->
    Err(Backpressure)
fi

Proof obligation:
  pre => (try_acquire /\ try_lock) \/ (~try_acquire \/ ~try_lock)
  -- Trivially true: both arms are exhaustive.
  -- INV-FERR-021: WriteLimiter pre-check prevents thundering herd on
  --   the write Mutex. Backpressure is a typed error, not a block.
  -- INV-FERR-007: Lock ensures at most one writer, so epoch
  --   ordering is strict (no concurrent epoch assignment).
  -- INV-FERR-015: HLC tick under the write lock produces a causally
  --   ordered TxId passed to Store::transact.
```

**Code**: `db/transact.rs` -- `WriteLimiter::try_acquire` + `acquire_write_lock_and_tick`

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

**Code**: `db/transact.rs` -- `Store::clone(&current)` + `new_store.transact(transaction, tx_id)`

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

-- Step 2b: Stamp datoms with HLC-derived TxId
stamped: [tx_id = clock.tick() /\ tx.datoms = [d1..dn],
          stamped = [d1[tx:=tx_id]..dn[tx:=tx_id]]]
  -- INV-FERR-015/016: replaces placeholder TxId(0,0,0) with the HLC-derived
  --   tx_id passed from Database::transact (ticked under the write lock).
  --   HLC monotonicity guarantees causal ordering across transactions.
  -- Proof obligation: tx_id > all previous TxIds because HLC.tick() is
  --   monotonic (INV-FERR-015) and causally consistent (INV-FERR-016).

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

-- Step 2e: Promote, insert into primary + indexes, demote
datoms, indexes: [all_datoms /\ schema',
                  datoms' = datoms union all_datoms
                  /\ indexes' = project(datoms')]
  -- promote(): Positional -> OrdMap (OrdSet + SortedVecIndexes) on first write.
  -- INV-FERR-005: bijection maintained by inserting each datom into
  --   both the primary OrdSet and all four SortedVecIndexes (sorted Vec backends).
  -- INV-FERR-003: duplicate insertion is a no-op (OrdSet semantics).
  -- INV-FERR-029: live_apply maintains the causal LIVE lattice incrementally.
  -- demote(): OrdMap -> Positional after transact for ns-level read performance
  --   (INV-FERR-072). Rebuilds PositionalStore from OrdSet in O(n).
  -- Proof obligation: for each d in all_datoms:
  --   d in datoms' /\ d in eavt' /\ d in aevt' /\ d in vaet' /\ d in avet'
```

**Code**: `store/apply.rs` -- `Store::transact` (promote, stamp, insert, live_apply, demote).

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

**Code**: `db/transact.rs` -- `write_wal` (WAL append + fsync), `publish_and_check` (ArcSwap store)

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

**Code**: `db/transact.rs` -- `publish_and_check` calls `self.current.store(Arc::new(new_store))`

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

**Code**: `db/recover.rs` -- `Database::recover`

---

#### Step 1a: Load checkpoint (Law 1.3 with data refinement)

```
base: [valid(checkpoint), base_store = load(checkpoint)]
  sqsubseteq  {INV-FERR-013, ADR-FERR-010}
-- Verify BLAKE3 hash (tamper detection)
-- Parse header: dispatch on magic bytes (V2 "CHKP" or V3 "CHK3")
-- Deserialize bincode payload to (schema, genesis_agent, datoms)
-- ADR-FERR-010: wire types (WireCheckpointPayload / V3PayloadRead)
--   cross the trust boundary via into_trusted()
-- Reconstruct Store via from_checkpoint / from_checkpoint_v3
--   (V3 preserves pre-sorted datoms + LIVE bitvector)
base_store := load_checkpoint(checkpoint_path)?

Proof obligation (INV-FERR-013 — round-trip identity):
  For all valid stores S:
    load_checkpoint(write_checkpoint(S)) = S
  Specifically:
    datom_set equality, epoch equality, schema equality,
    index bijection after reconstruction (INV-FERR-005).
```

**Code**: `checkpoint.rs` -- `load_checkpoint` (V2/V3 format dispatch)

---

#### Step 1b: Replay WAL suffix (Law 5.5 — iteration)

```
replay: [base_store, store' = base_store |> entries_after(checkpoint_epoch)]
  sqsubseteq  {INV-FERR-014, INV-FERR-008, ADR-FERR-010}
var store := base_store;
for entry in wal.recover()? {
    if entry.epoch > checkpoint_epoch {
        -- ADR-FERR-010: deserialize as WireDatom, convert via into_trusted()
        let wire_datoms = bincode::deserialize(entry.payload)?;
        let datoms = wire_datoms.map(WireDatom::into_trusted);
        store.replay_entry(entry.epoch, &datoms)?;
        -- replay_entry: inserts datoms, advances epoch, evolves schema
        -- (INV-FERR-009: schema-defining datoms in WAL are re-installed)
    }
}

Invariant: store = base_store |> entries_up_to(current_entry)
Variant: remaining_entries (decreases by 1 each iteration, bounded below by 0)

Proof obligation:
  (a) INV-FERR-008: every committed transaction has a corresponding valid
      WAL frame (fsynced before epoch advance).
  (b) INV-FERR-014: wal.recover() truncates incomplete frames, so only
      fully-written entries are replayed. Incomplete = uncommitted.
  (c) ADR-FERR-010: WireDatom -> into_trusted() crosses the trust boundary.
      CRC was already verified by Wal::recover().
  (d) Recovered datoms carry real TxIds (stamped before WAL write),
      so replay_entry produces identical state to original transact.
  (e) INV-FERR-003: duplicate insertion is idempotent. If a datom from
      the WAL is already in the checkpoint, re-insertion is a no-op.
  (f) INV-FERR-009: replay_entry calls evolve_schema per entry, preventing
      schema loss across crash recovery.
```

**Code**: `db/recover.rs` -- WAL replay loop in `Database::recover`

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

#### Step 0 -> 1: Set union via 4-way repr match (Law 1.3 with data refinement)

```
w: [true, result.datoms = A union B]
  sqsubseteq  {INV-FERR-001, INV-FERR-002, INV-FERR-003, INV-FERR-029, ADR-FERR-001}
result := merge_repr(A.repr, B.repr)
  -- 4-way match on (A.repr, B.repr):
  --   (Positional, Positional) -> merge_positional(A, B)  [O(n+m) merge-sort]
  --   (Positional, OrdMap) | (OrdMap, Positional) -> merge_sort_dedup [O(n+m)]
  --   (OrdMap, OrdMap) -> merge_sort_dedup [O(n+m)]
  -- Result is always Positional (cache-optimal read representation).
result.live_causal := merge_causal(A.live_causal, B.live_causal)
  -- INV-FERR-029: causal LIVE merge via per-key max(TxId). Lattice homomorphism:
  --   merge_causal(LIVE(A), LIVE(B)) = LIVE(A union B)

Proof obligation (data refinement — coupling invariant CI):
  CI(Finset, PositionalStore) => Finset.union(A, B) = to_finset(merge_positional(A, B))
  -- merge_positional and merge_sort_dedup both produce a sorted, deduplicated
  --   Vec<Datom> that is semantically equivalent to set union.
  -- INV-FERR-001: merge_sort_dedup is commutative (sorted merge, same elements).
  -- INV-FERR-002: merge_sort_dedup is associative (order-independent).
  -- INV-FERR-003: dedup ensures idempotency (duplicates removed).

Algebraic properties inherited from set union:
  -- Commutativity: A union B = B union A [INV-FERR-001]
  -- Associativity: (A union B) union C = A union (B union C) [INV-FERR-002]
  -- Idempotency: A union A = A [INV-FERR-003]
  -- Monotonicity: A subset (A union B) [INV-FERR-004]
```

**Code**: `store/merge.rs` -- `Store::from_merge`, `merge_repr` (4-way match), `merge_causal`

#### Step 1 -> 2: Indexes intrinsic to PositionalStore (Law 3.3)

```
result.indexes: [result.datoms, indexes' = project(result.datoms)]
  sqsubseteq  {INV-FERR-005, INV-FERR-076}
-- Indexes are intrinsic to PositionalStore: permutation arrays for each
-- sort order (EAVT, AEVT, VAET, AVET) are lazily constructed on first access
-- via OnceLock. No explicit Indexes::from_primary call.
-- INV-FERR-076: positional representation preserves bijection by construction.

Proof obligation:
  Every datom in result.datoms appears in all four permutation indexes.
  No index entry references a position absent from result.datoms.
  -- INV-FERR-005: bijection by construction (PositionalStore builds
  --   permutation arrays from the canonical sorted datom slice).
```

**Code**: `store/merge.rs` -- `merge_repr` returns `PositionalStore` (indexes are lazy permutation arrays)

#### Step 2 -> 3: Schema merge + epoch max + genesis agent (Law 1.3)

```
result.schema: [A.schema, B.schema,
                result.schema = A.schema union B.schema]
result.epoch:  [A.epoch, B.epoch,
                result.epoch = max(A.epoch, B.epoch)]
result.genesis_agent: [A.genesis_agent, B.genesis_agent,
                       result.genesis_agent = min(A.genesis_agent, B.genesis_agent)]
  sqsubseteq  {INV-FERR-043, INV-FERR-007}

Proof obligation:
  -- INV-FERR-043: conflicting schema definitions (same attribute, different
  --   type/cardinality) are resolved deterministically by keeping the
  --   Ord-minimal definition: min(a,b) == min(b,a) preserves commutativity.
  --   Every conflict is recorded as a SchemaConflict audit trail entry.
  -- INV-FERR-007: max epoch preserves the "most advanced" timeline.
  --   After merge, the epoch is at least as large as both inputs,
  --   so subsequent transact produces a strictly larger epoch.
  -- HI-014: genesis_agent is min(a, b) -- deterministic, commutative.
  -- INV-FERR-029: live_set is derived from the merged live_causal lattice.
```

**Code**: `store/merge.rs` -- `merge_schemas` (Ord-minimal conflict resolution + SchemaConflict audit trail),
`from_merge` (max epoch, min genesis_agent, derive_live_set)

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
| Write serialization (two-phase gate) | INV-FERR-021, INV-FERR-007, INV-FERR-015 | Transaction |
| Mutation/publication separation | INV-FERR-006 | Transaction |
| Epoch advance | INV-FERR-007 | Transaction |
| TxId stamping (HLC-derived) | INV-FERR-015, INV-FERR-016 | Transaction |
| Tx metadata creation | INV-FERR-004 | Transaction |
| Schema evolution | INV-FERR-009 | Transaction |
| Promote/insert/demote cycle | INV-FERR-005, INV-FERR-003, INV-FERR-029, INV-FERR-072 | Transaction |
| WAL append + fsync | INV-FERR-008 | Transaction, Recovery |
| Atomic swap | INV-FERR-006, NEG-FERR-004 | Transaction |
| Checkpoint load (V2/V3 dispatch) | INV-FERR-013, ADR-FERR-010 | Recovery |
| WAL replay (replay_entry) | INV-FERR-014, INV-FERR-008, INV-FERR-009, ADR-FERR-010 | Recovery |
| Datom set union (4-way repr match) | INV-FERR-001/002/003/004, INV-FERR-029, ADR-FERR-001 | Merge |
| Indexes (PositionalStore intrinsic) | INV-FERR-005, INV-FERR-076 | Merge |
| Schema union + epoch max + genesis_agent | INV-FERR-043, INV-FERR-007 | Merge |
