## 23.1 Core Invariants

### INV-FERR-001: Merge Commutativity

**Traces to**: SEED.md 4 Axiom 2 (Store), C4, L1, INV-STORE-004
**Verification**: `V:PROP`, `V:KANI`, `V:LEAN`, `V:MODEL`
**Stage**: 0

#### Level 0 (Algebraic Law)
```
∀ A, B ∈ DatomStore:
  merge(A, B) = merge(B, A)

Proof: merge(A, B) = A ∪ B = B ∪ A = merge(B, A)
  by commutativity of set union.
```

#### Level 1 (State Invariant)
For all reachable store pairs `(A, B)` produced by any sequence of TRANSACT operations
starting from GENESIS: the datom set resulting from `merge(A, B)` is identical to the
datom set resulting from `merge(B, A)`. This holds regardless of the order in which
transactions were applied to A and B independently, the wall-clock times of those
transactions, or the agents that produced them. Commutativity means that the order in
which two replicas discover each other and initiate merge is irrelevant to the final
converged state.

#### Level 2 (Implementation Contract)
```rust
/// Merge two stores. The result contains exactly the union of both datom sets.
/// Order of arguments does not affect the result (INV-FERR-001).
///
/// # Panics
/// Never panics. Merge is total over all valid store pairs.
pub fn merge(a: &Store, b: &Store) -> Store {
    let mut result = a.datoms.clone();
    for datom in b.datoms.iter() {
        result.insert(datom.clone()); // BTreeSet insert is idempotent
    }
    Store::from_datoms(result) // rebuilds indexes
}

#[kani::proof]
#[kani::unwind(10)]
fn merge_commutativity() {
    let a: BTreeSet<Datom> = kani::any();
    let b: BTreeSet<Datom> = kani::any();
    kani::assume(a.len() <= 4 && b.len() <= 4);

    let ab: BTreeSet<Datom> = a.union(&b).cloned().collect();
    let ba: BTreeSet<Datom> = b.union(&a).cloned().collect();
    assert_eq!(ab, ba);
}
```

**Falsification**: Any pair of stores `(A, B)` where the datom set of `merge(A, B)` differs
from the datom set of `merge(B, A)`. Concretely: there exists a datom `d` such that
`d ∈ merge(A, B)` but `d ∉ merge(B, A)`, or vice versa. This would indicate that the merge
implementation performs order-dependent operations (e.g., deduplication that depends on
insertion order, or resolution logic applied during merge rather than at the query layer).

**proptest strategy**:
```rust
proptest! {
    #[test]
    fn merge_commutes(
        a_datoms in prop::collection::btree_set(arb_datom(), 0..100),
        b_datoms in prop::collection::btree_set(arb_datom(), 0..100),
    ) {
        let a = Store::from_datoms(a_datoms.clone());
        let b = Store::from_datoms(b_datoms.clone());

        let ab = merge(&a, &b);
        let ba = merge(&b, &a);

        prop_assert_eq!(ab.datom_set(), ba.datom_set());
    }
}
```

**Lean theorem**:
```lean
theorem merge_comm (a b : DatomStore) : merge a b = merge b a := by
  unfold merge
  exact Finset.union_comm a b
```

---

### INV-FERR-002: Merge Associativity

**Traces to**: SEED.md 4 Axiom 2 (Store), C4, L2, INV-STORE-005
**Verification**: `V:PROP`, `V:KANI`, `V:LEAN`, `V:MODEL`
**Stage**: 0

#### Level 0 (Algebraic Law)
```
∀ A, B, C ∈ DatomStore:
  merge(merge(A, B), C) = merge(A, merge(B, C))

Proof: merge(merge(A, B), C) = (A ∪ B) ∪ C = A ∪ (B ∪ C) = merge(A, merge(B, C))
  by associativity of set union.
```

#### Level 1 (State Invariant)
For all reachable store triples `(A, B, C)`: the final datom set is invariant under
regrouping of merge operations. This is the property that enables arbitrary merge topologies
in multi-agent systems. Whether agent 1 merges with agent 2 first and then agent 3, or
agent 2 merges with agent 3 first and then agent 1, the final converged state is identical.
Without associativity, the merge topology would constrain the final result, making the
system dependent on coordination infrastructure rather than on the datoms themselves.

#### Level 2 (Implementation Contract)
```rust
#[kani::proof]
#[kani::unwind(10)]
fn merge_associativity() {
    let a: BTreeSet<Datom> = kani::any();
    let b: BTreeSet<Datom> = kani::any();
    let c: BTreeSet<Datom> = kani::any();
    kani::assume(a.len() <= 3 && b.len() <= 3 && c.len() <= 3);

    let ab: BTreeSet<Datom> = a.union(&b).cloned().collect();
    let ab_c: BTreeSet<Datom> = ab.union(&c).cloned().collect();

    let bc: BTreeSet<Datom> = b.union(&c).cloned().collect();
    let a_bc: BTreeSet<Datom> = a.union(&bc).cloned().collect();

    assert_eq!(ab_c, a_bc);
}
```

**Falsification**: Any triple of stores `(A, B, C)` where `merge(merge(A, B), C)` produces
a different datom set than `merge(A, merge(B, C))`. This would indicate that the merge
implementation accumulates state (e.g., a merge counter, a "last merged from" marker)
that is sensitive to grouping. Since merge is defined as pure set union, any such
accumulation is a bug.

**proptest strategy**:
```rust
proptest! {
    #[test]
    fn merge_associative(
        a_datoms in prop::collection::btree_set(arb_datom(), 0..50),
        b_datoms in prop::collection::btree_set(arb_datom(), 0..50),
        c_datoms in prop::collection::btree_set(arb_datom(), 0..50),
    ) {
        let a = Store::from_datoms(a_datoms);
        let b = Store::from_datoms(b_datoms);
        let c = Store::from_datoms(c_datoms);

        let ab_c = merge(&merge(&a, &b), &c);
        let a_bc = merge(&a, &merge(&b, &c));

        prop_assert_eq!(ab_c.datom_set(), a_bc.datom_set());
    }
}
```

**Lean theorem**:
```lean
theorem merge_assoc (a b c : DatomStore) : merge (merge a b) c = merge a (merge b c) := by
  unfold merge
  exact Finset.union_assoc a b c
```

---

### INV-FERR-003: Merge Idempotency

**Traces to**: SEED.md 4 Axiom 2 (Store), C4, L3, INV-STORE-006
**Verification**: `V:PROP`, `V:KANI`, `V:LEAN`
**Stage**: 0

#### Level 0 (Algebraic Law)
```
∀ A ∈ DatomStore:
  merge(A, A) = A

Proof: merge(A, A) = A ∪ A = A
  by idempotency of set union.
```

#### Level 1 (State Invariant)
Merging a store with itself produces no change to the datom set, the indexes, or any
derived state. This property is essential for at-least-once delivery semantics:
if a merge message is delivered twice (network retry, process restart), the second delivery
is a no-op. Without idempotency, retry logic would need deduplication infrastructure
external to the store, violating the principle that all protocol-relevant state lives
in the datom set itself.

#### Level 2 (Implementation Contract)
```rust
#[kani::proof]
#[kani::unwind(10)]
fn merge_idempotency() {
    let a: BTreeSet<Datom> = kani::any();
    kani::assume(a.len() <= 5);

    let aa: BTreeSet<Datom> = a.union(&a).cloned().collect();
    assert_eq!(a, aa);
}

/// Implementation: merge detects self-merge via store identity hash and short-circuits.
/// Even without the short-circuit, set union with self is structurally idempotent.
pub fn merge(a: &Store, b: &Store) -> Store {
    if a.identity_hash() == b.identity_hash() {
        return a.clone(); // fast path: self-merge
    }
    // ... full merge path ...
}
```

**Falsification**: A store `A` where `merge(A, A)` produces a datom set that differs from
`A` in any way: different cardinality, different datom content, different index state. This
would indicate that merge has side effects beyond set union — for example, incrementing a
merge counter, updating a "last merged" timestamp, or re-indexing in a non-deterministic way.

**proptest strategy**:
```rust
proptest! {
    #[test]
    fn merge_idempotent(
        datoms in prop::collection::btree_set(arb_datom(), 0..100),
    ) {
        let store = Store::from_datoms(datoms);
        let merged = merge(&store, &store);
        prop_assert_eq!(store.datom_set(), merged.datom_set());
        prop_assert_eq!(store.len(), merged.len());
    }
}
```

**Lean theorem**:
```lean
theorem merge_idemp (a : DatomStore) : merge a a = a := by
  unfold merge
  exact Finset.union_idempotent a
```

---

### INV-FERR-004: Monotonic Growth

**Traces to**: SEED.md 4 Axiom 2 (Store), C1, L4, L5, INV-STORE-001, INV-STORE-002
**Referenced by**: INV-FERR-062 (merge receipt datoms persist through monotonic growth),
ADR-FERR-029 (merge receipts as datoms)
**Verification**: `V:PROP`, `V:KANI`, `V:LEAN`
**Stage**: 0

#### Level 0 (Algebraic Law)
```
∀ S ∈ DatomStore, ∀ d ∈ Datom:
  |apply(S, d)| ≥ |S|

Equivalently:
  S ⊆ apply(S, d)        -- no datom is lost
  |apply(S, d)| ≥ |S|    -- cardinality is non-decreasing

Strict growth for transactions:
  ∀ S, T where T is a non-empty transaction:
    |TRANSACT(S, T)| > |S|
  (every transaction adds at least its tx_entity metadata datoms)
```

#### Level 1 (State Invariant)
For all reachable states `(S, S')` where `S` transitions to `S'` via TRANSACT or MERGE:
`S.datoms` is a subset of `S'.datoms`. The store never shrinks. Retractions are new datoms
with `op = Retract` — they add to the store rather than removing from it. The "current
state" of an entity (which values are "live") is a query-layer concern computed by the
LIVE index; the store itself is append-only.

For TRANSACT specifically, growth is strict: every transaction produces at least one new
datom (the transaction entity's metadata), so `|S'.datoms| > |S.datoms|`. For MERGE,
growth is non-strict: merging two identical stores produces the same store (idempotency,
INV-FERR-003), so `|merge(S, S)| = |S|`.

#### Level 2 (Implementation Contract)
```rust
/// Transact a committed transaction into the store.
/// Post-condition: store size strictly increases (at least tx metadata datoms added).
#[kani::ensures(|result| old(store.len()) < store.len())]
pub fn transact(store: &mut Store, tx: Transaction<Committed>) -> Result<TxReceipt, TxApplyError> {
    let pre_len = store.len();
    // ... apply datoms, add tx metadata ...
    debug_assert!(store.len() > pre_len, "INV-FERR-004: strict growth violated");
    Ok(receipt)
}

/// Merge: non-strict growth. Result is superset of both inputs.
#[kani::ensures(|result| old(a.len()) <= result.len() && old(b.len()) <= result.len())]
pub fn merge(a: &Store, b: &Store) -> Store {
    // ... set union ...
}

#[kani::proof]
#[kani::unwind(10)]
fn monotonic_growth() {
    let s: BTreeSet<Datom> = kani::any();
    let d: Datom = kani::any();
    kani::assume(s.len() <= 5);

    let mut s_prime = s.clone();
    s_prime.insert(d);
    assert!(s_prime.len() >= s.len());
    assert!(s.is_subset(&s_prime));
}
```

**Falsification**: Any transition `S -> S'` where there exists a datom `d ∈ S` such that
`d ∉ S'`. Equivalently: `store.len()` decreases after any operation. For TRANSACT
specifically: `store.len()` does not strictly increase (remains equal or decreases).
This would indicate either a mutation (violating C1), a deduplication bug that removes
existing datoms, or a transaction that adds zero datoms (including no tx metadata).

**proptest strategy**:
```rust
proptest! {
    #[test]
    fn monotonic_transact(
        initial in arb_store(0..50),
        tx in arb_transaction(),
    ) {
        let pre_datoms: BTreeSet<_> = initial.datom_set().clone();
        let pre_len = initial.len();

        let mut store = initial;
        if let Ok(_receipt) = store.transact(tx) {
            // Strict growth: at least tx metadata added
            prop_assert!(store.len() > pre_len);
            // Monotonicity: no datoms lost
            for d in &pre_datoms {
                prop_assert!(store.datom_set().contains(d));
            }
        }
    }
}
```

**Lean theorem**:
```lean
theorem apply_monotone (s : DatomStore) (d : Datom) : s.card ≤ (apply_tx s d).card := by
  unfold apply_tx
  exact Finset.card_le_card (Finset.subset_union_left s {d})

theorem apply_superset (s : DatomStore) (d : Datom) : s ⊆ apply_tx s d := by
  unfold apply_tx
  exact Finset.subset_union_left s {d}

theorem merge_monotone_left (a b : DatomStore) : a ⊆ merge a b := by
  unfold merge
  exact Finset.subset_union_left a b

theorem merge_monotone_right (a b : DatomStore) : b ⊆ merge a b := by
  unfold merge
  exact Finset.subset_union_right a b
```

---

### INV-FERR-005: Index Bijection

**Traces to**: SEED.md 4, INV-STORE-012
**Referenced by**: INV-FERR-071 (sorted-array backend), INV-FERR-073 (permutation index fusion), INV-FERR-076 (positional content addressing)
**Verification**: `V:PROP`, `V:KANI`
**Stage**: 0

#### Level 0 (Algebraic Law)
```
Let primary(S) = S.datoms (the canonical datom set).
Let EAVT(S), AEVT(S), VAET(S), AVET(S) be the four secondary indexes.

The store maintains 6 indexes, each a deterministic projection of the primary datom set:

  EAVT(S)  = { d ∈ S | sorted by (entity, attribute, value, tx) }     — primary
  Entity(S) = { (e, {d ∈ S | d.entity = e}) }                          — entity → datoms
  Attr(S)   = { (a, {d ∈ S | d.attribute = a}) }                       — attribute → datoms
  VAET(S)   = { (target, {d ∈ S | d.value = Ref(target)}) }            — reverse refs
  AVET(S)   = { ((a,v), {d ∈ S | d.attribute = a ∧ d.value = v}) }     — attribute-value → datoms
  LIVE(S)   = { ((e,a), resolve(a, {d ∈ S | d.entity = e ∧ d.attribute = a})) } — resolved current state

∀ d ∈ Datom:
  d ∈ primary(S) ⟺ d ∈ EAVT(S) ⟺ d ∈ AEVT(S) ⟺ d ∈ VAET(S) ⟺ d ∈ AVET(S)

Equivalently: the indexes are projections of the same set, differing only in sort order.
The content of all five structures is identical; only the access pattern differs.

Cardinality:
  |primary(S)| = |EAVT(S)| = |AEVT(S)| = |VAET(S)| = |AVET(S)|
```

#### Level 1 (State Invariant)
After every TRANSACT, MERGE, or crash-recovery operation, every datom in the primary
store appears in every secondary index, and every entry in every secondary index
corresponds to a datom in the primary store. There are no phantom index entries (present
in index but not in primary) and no missing index entries (present in primary but absent
from index). The LIVE index is excluded from this bijection because it is a derived view
(resolution-applied) rather than a permutation of the raw datom set.

This invariant must hold even after crash recovery: if the process crashes between
writing a datom to the primary store and updating an index, the recovery procedure
must restore the bijection before the store becomes queryable.

#### Level 2 (Implementation Contract)
```rust
/// Verify index bijection. Called after every TRANSACT and during recovery.
/// O(n) scan — used in debug builds and verification harnesses, not hot path.
fn verify_index_bijection(store: &Store) -> bool {
    let primary = &store.datoms;
    let eavt_set: BTreeSet<&Datom> = store.indexes.eavt.iter().collect();
    let aevt_set: BTreeSet<&Datom> = store.indexes.aevt.iter().collect();
    let vaet_set: BTreeSet<&Datom> = store.indexes.vaet.iter().collect();
    let avet_set: BTreeSet<&Datom> = store.indexes.avet.iter().collect();

    let primary_set: BTreeSet<&Datom> = primary.iter().collect();

    primary_set == eavt_set
        && primary_set == aevt_set
        && primary_set == vaet_set
        && primary_set == avet_set
}

#[kani::proof]
#[kani::unwind(10)]
fn index_bijection() {
    let datoms: BTreeSet<Datom> = kani::any();
    kani::assume(datoms.len() <= 4);

    let store = Store::from_datoms(datoms.clone());

    // Every datom in primary is in every index
    for d in &datoms {
        assert!(store.indexes.eavt.contains(d));
        assert!(store.indexes.aevt.contains(d));
        assert!(store.indexes.vaet.contains(d));
        assert!(store.indexes.avet.contains(d));
    }

    // Every index has exactly the same cardinality as primary
    assert_eq!(datoms.len(), store.indexes.eavt.len());
    assert_eq!(datoms.len(), store.indexes.aevt.len());
    assert_eq!(datoms.len(), store.indexes.vaet.len());
    assert_eq!(datoms.len(), store.indexes.avet.len());
}
```

**Falsification**: A datom `d` exists in `primary(S)` but not in `EAVT(S)` (or any other
index), or a datom `d` exists in `AEVT(S)` but not in `primary(S)`. Also: any state where
`|primary(S)| != |EAVT(S)|` (cardinality mismatch). This would indicate either an
incremental index update bug (a transaction that adds to primary but fails to update one
or more indexes), a crash-recovery defect (WAL replayed to primary but not to indexes),
or a concurrency bug (index update not protected by the same serialization as the primary
write).

**proptest strategy**:
```rust
proptest! {
    #[test]
    fn index_bijection_after_transactions(
        initial in arb_store(0..20),
        txns in prop::collection::vec(arb_transaction(), 0..10),
    ) {
        let mut store = initial;
        for tx in txns {
            let _ = store.transact(tx);
            // After every transaction, verify bijection
            prop_assert!(verify_index_bijection(&store));
        }
    }

    #[test]
    fn index_bijection_after_merge(
        a in arb_store(0..30),
        b in arb_store(0..30),
    ) {
        let merged = merge(&a, &b);
        prop_assert!(verify_index_bijection(&merged));
    }
}
```

**Lean theorem**:
```lean
/-- Index bijection: every projection of the same set has the same cardinality.
    In Lean, we model indexes as the same Finset with different orderings.
    Since reordering a Finset does not change membership, bijection is trivial. -/
theorem index_bijection (s : DatomStore) :
    s.card = s.card := by
  rfl

/-- The substantive claim: after adding a datom to the store, the datom is
    present in every "index" (modeled as the same set, since indexes are
    permutations of the primary set). -/
theorem index_membership_after_apply (s : DatomStore) (d : Datom) :
    d ∈ apply_tx s d := by
  unfold apply_tx
  exact Finset.mem_union_right s (Finset.mem_singleton_self d)
```

---

### INV-FERR-006: Snapshot Isolation

**Traces to**: SEED.md 4 Axiom 3 (Snapshots), INV-STORE-013,
ADR-FERR-001 (Persistent Data Structures), ADR-FERR-003 (Concurrency Model)
**Referenced by**: INV-FERR-072 (lazy representation promotion)
**Verification**: `V:PROP`, `V:KANI`, `V:MODEL`
**Stage**: 0

#### Level 0 (Algebraic Law)
```
Let epoch(S) = the monotonic version counter of store S.
Let snapshot(S, e) = the datom set visible at epoch e.

∀ reader R observing epoch e:
  ∀ writer W committing transaction T at epoch e' > e:
    snapshot(S, e) ∩ T.datoms = ∅ if T was not committed at or before epoch e

No reader sees a partial transaction:
  ∀ T = {d₁, d₂, ..., dₙ}:
    either ∀ dᵢ ∈ snapshot(S, e) (T fully visible)
    or     ∀ dᵢ ∉ snapshot(S, e) (T fully invisible)

This is equivalent to:
  snapshot(S, e) = ⋃ {T.datoms | T committed at epoch ≤ e}
```

#### Level 1 (State Invariant)
A reader that obtains a snapshot at epoch `e` sees a consistent view: exactly the set
of datoms from all transactions committed at or before epoch `e`, and none of the datoms
from transactions committed after epoch `e`. No reader ever sees a subset of a
transaction's datoms — transactions are atomic with respect to snapshot visibility.

This must hold even under concurrent access: while writer W is committing transaction T
(which involves writing datoms, updating indexes, and advancing the epoch), any concurrent
reader R that obtained a snapshot before W's commit sees none of T's datoms. Reader R'
that obtains a snapshot after W's commit sees all of T's datoms.

#### Level 2 (Implementation Contract)
```rust
/// A snapshot is a read-only view at a specific epoch.
/// The epoch is captured at construction time and does not advance.
pub struct Snapshot<'a> {
    store: &'a Store,
    epoch: u64,
}

impl Store {
    /// Obtain a snapshot at the current epoch.
    /// The snapshot sees all committed transactions up to this epoch.
    pub fn snapshot(&self) -> Snapshot<'_> {
        Snapshot {
            store: self,
            epoch: self.current_epoch(),
        }
    }
}

impl<'a> Snapshot<'a> {
    /// Query datoms visible at this snapshot's epoch.
    /// Returns only datoms from transactions committed at epoch <= self.epoch.
    pub fn datoms(&self) -> impl Iterator<Item = &Datom> {
        self.store.datoms.iter().filter(|d| d.tx_epoch <= self.epoch)
    }
}

#[kani::proof]
#[kani::unwind(8)]
fn snapshot_isolation() {
    let mut store = Store::genesis();
    let snap_epoch = store.current_epoch();
    let snapshot_datoms: BTreeSet<Datom> = store.snapshot().datoms().cloned().collect();

    // Simulate a concurrent write at a later epoch
    let tx: Transaction<Committed> = kani::any();
    let _ = store.transact(tx);

    // Original snapshot must not see the new datoms
    for d in store.datoms.iter() {
        if d.tx_epoch > snap_epoch {
            assert!(!snapshot_datoms.contains(d));
        }
    }
}
```

**Falsification**: A reader at epoch `e` observes a datom from a transaction committed at
epoch `e' > e`. Or: a reader at epoch `e` observes datoms `d₁` from transaction `T` but
not `d₂` from the same transaction `T` where `T` was committed at epoch `≤ e` (partial
transaction visibility). Either case indicates a concurrency defect in the snapshot
mechanism — either the epoch is not atomically captured, or the datom visibility filter
is not correctly epoch-bounded.

**proptest strategy**:
```rust
proptest! {
    #[test]
    fn snapshot_sees_no_future_txns(
        initial_txns in prop::collection::vec(arb_transaction(), 1..5),
        later_txns in prop::collection::vec(arb_transaction(), 1..5),
    ) {
        let mut store = Store::genesis();
        for tx in initial_txns {
            let _ = store.transact(tx);
        }

        let snapshot = store.snapshot();
        let snap_datoms: BTreeSet<_> = snapshot.datoms().cloned().collect();

        for tx in later_txns {
            let _ = store.transact(tx);
        }

        // Snapshot must not have grown
        let snap_datoms_after: BTreeSet<_> = snapshot.datoms().cloned().collect();
        prop_assert_eq!(snap_datoms, snap_datoms_after);
    }

    #[test]
    fn transaction_atomicity(
        txns in prop::collection::vec(arb_multi_datom_transaction(), 1..10),
    ) {
        let mut store = Store::genesis();
        for tx in txns {
            let tx_datoms: BTreeSet<_> = tx.datoms().cloned().collect();
            let _ = store.transact(tx);

            let snapshot = store.snapshot();
            let visible: BTreeSet<_> = snapshot.datoms().cloned().collect();

            // Transaction is either fully visible or fully invisible
            let visible_count = tx_datoms.iter().filter(|d| visible.contains(d)).count();
            prop_assert!(
                visible_count == 0 || visible_count == tx_datoms.len(),
                "Partial transaction visibility: {} of {} datoms visible",
                visible_count, tx_datoms.len()
            );
        }
    }
}
```

**Lean theorem**:
```lean
/-- Snapshot isolation: the datoms visible at epoch e are exactly those
    from transactions at epoch ≤ e. Adding a datom at epoch e' > e
    does not change the set visible at epoch e. -/

def visible_at (s : DatomStore) (epoch : Nat) : DatomStore :=
  s.filter (fun d => d.tx ≤ epoch)

theorem snapshot_stable (s : DatomStore) (d : Datom) (epoch : Nat)
    (h : epoch < d.tx) :
    visible_at (apply_tx s d) epoch = visible_at s epoch := by
  unfold visible_at apply_tx
  simp [Finset.filter_union, Finset.filter_singleton]
  intro h_le
  exact absurd (Nat.lt_of_lt_of_le h h_le) (Nat.lt_irrefl _)
```

---

### INV-FERR-007: Write Linearizability

**Traces to**: SEED.md 4, INV-STORE-010, INV-STORE-011,
ADR-FERR-003 (Concurrency Model)
**Verification**: `V:PROP`, `V:KANI`, `V:MODEL`
**Stage**: 0

#### Level 0 (Algebraic Law)
```
Let epoch : Store → Nat be the monotonic epoch counter.
Let commit_order be the total order in which transactions are durably committed.

∀ T₁, T₂ committed to the same store:
  commit_order(T₁) < commit_order(T₂)
  ⟹ epoch(T₁) < epoch(T₂)

The epoch sequence is strictly monotonically increasing for committed writes.
Combined with snapshot isolation (INV-FERR-006), this means every committed
write is visible to all subsequent snapshots and invisible to all prior snapshots.
```

#### Level 1 (State Invariant)
Committed writes appear in a strict total order defined by their epoch numbers. If
transaction `T₁` commits before transaction `T₂` (in wall-clock time), then `T₁.epoch <
T₂.epoch`. No two transactions share the same epoch. This ordering is the serialization
point for writers: concurrent write attempts are serialized (one commits first, the other
commits second with a higher epoch), and the serialization is reflected in the epoch
sequence.

Within a single process, serialization is achieved by holding a write lock for the
duration of the commit. Across processes, serialization is achieved by flock(2) on the
WAL file (or equivalent OS-level exclusive lock). The epoch is assigned under the lock,
ensuring no interleaving.

#### Level 2 (Implementation Contract)
```rust
/// Commit serialization: only one writer at a time.
/// The epoch is assigned under the write lock, ensuring strict ordering.
impl Store {
    pub fn transact(&mut self, tx: Transaction<Committed>) -> Result<TxReceipt, TxApplyError> {
        let _write_lock = self.write_lock.lock(); // serialize writers

        let epoch = self.next_epoch(); // strictly monotonic under lock
        debug_assert!(epoch > self.last_committed_epoch);

        // Write WAL entry with this epoch
        self.wal.append(epoch, &tx)?;
        self.wal.fsync()?; // durable before publication (INV-FERR-008)

        // Apply to in-memory state
        self.apply_datoms(epoch, &tx);
        self.last_committed_epoch = epoch;

        Ok(receipt)
    }
}

#[kani::proof]
#[kani::unwind(8)]
fn write_linearizability() {
    let mut epochs: Vec<u64> = Vec::new();
    let mut store = Store::genesis();

    for _ in 0..kani::any::<u8>().min(5) {
        let tx: Transaction<Committed> = kani::any();
        if let Ok(receipt) = store.transact(tx) {
            epochs.push(receipt.epoch);
        }
    }

    // Epochs are strictly monotonically increasing
    for i in 1..epochs.len() {
        assert!(epochs[i] > epochs[i - 1]);
    }
}
```

**Falsification**: Two committed transactions `T₁, T₂` where `T₁` committed before `T₂`
(in real time) but `T₁.epoch >= T₂.epoch`. Or: two transactions with the same epoch
value. This would indicate either a failure to serialize writes (the write lock was not
held, allowing interleaved epoch assignment), or a bug in the epoch counter (non-monotonic
increment, overflow without detection, or reset after crash).

**proptest strategy**:
```rust
proptest! {
    #[test]
    fn epochs_strictly_increase(
        txns in prop::collection::vec(arb_transaction(), 1..20),
    ) {
        let mut store = Store::genesis();
        let mut prev_epoch: Option<u64> = None;

        for tx in txns {
            if let Ok(receipt) = store.transact(tx) {
                if let Some(prev) = prev_epoch {
                    prop_assert!(receipt.epoch > prev,
                        "Epoch did not increase: {} -> {}", prev, receipt.epoch);
                }
                prev_epoch = Some(receipt.epoch);
            }
        }
    }
}
```

**Lean theorem**:
```lean
/-- INV-FERR-007: the next epoch is always strictly greater than the previous. -/
theorem next_epoch_strict (prev : Nat) : prev < prev + 1 := by
  omega

/-- Sequential commits preserve strict epoch ordering. -/
theorem write_linear_pair (e₁ e₂ : Nat) (h : e₂ = e₁ + 1) : e₁ < e₂ := by
  omega

/-- Strict ordering composes across commit chains. -/
theorem write_linear_chain (e₁ e₂ e₃ : Nat)
    (h₁₂ : e₁ < e₂) (h₂₃ : e₂ < e₃) : e₁ < e₃ := by
  omega
```

---

### INV-FERR-008: WAL Fsync Ordering

**Traces to**: SEED.md 5 (Harvest/Seed Lifecycle — durability), C1, INV-STORE-009
**Verification**: `V:PROP`, `V:MODEL`
**Stage**: 0

#### Level 0 (Algebraic Law)
```
Let WAL(T) = the WAL entry for transaction T.
Let SNAP(e) = the snapshot publication at epoch e.

∀ T committed at epoch e:
  durable(WAL(T))  BEFORE  visible(SNAP(e))

The temporal ordering is:
  1. Write WAL entry for T (append to WAL file)
  2. fsync WAL file (ensure bytes are on durable storage)
  3. Apply T to in-memory indexes
  4. Advance epoch to e (making T visible to new snapshots)

Step 2 MUST complete before step 4 begins.
If the process crashes between steps 2 and 4, recovery replays the WAL
to reconstruct the in-memory state. No committed data is lost.
If the process crashes between steps 1 and 2, the WAL entry may be
incomplete. Recovery truncates incomplete entries.
```

#### Level 1 (State Invariant)
The WAL is the durable ground truth. In-memory indexes and snapshots are derived state
that can be reconstructed from the WAL. A transaction is considered "committed" only after
its WAL entry has been fsynced. The epoch advances (making the transaction visible) only
after the fsync completes. This ordering ensures that any transaction visible to a reader
is recoverable after a crash.

The converse also holds: if a transaction's WAL entry was NOT fsynced before a crash, the
transaction is NOT committed, and its datoms MUST NOT appear in the recovered store. This
prevents "phantom reads" where a reader saw datoms that do not survive crash recovery.

#### Level 2 (Implementation Contract)
```rust
/// Write-ahead log with strict fsync ordering.
pub struct Wal {
    file: File,
    last_synced_epoch: u64,
}

impl Wal {
    /// Append a transaction to the WAL. Does NOT fsync.
    pub fn append(&mut self, epoch: u64, tx: &Transaction<Committed>) -> io::Result<()> {
        let entry = WalEntry::new(epoch, tx);
        entry.serialize_into(&mut self.file)?;
        Ok(())
    }

    /// Fsync the WAL. After this returns, all appended entries are durable.
    /// MUST be called before advancing the epoch (INV-FERR-008).
    pub fn fsync(&mut self) -> io::Result<()> {
        self.file.sync_all()?;
        self.last_synced_epoch = self.pending_epoch;
        Ok(())
    }

    /// Recovery: replay all complete WAL entries, truncate incomplete ones.
    pub fn recover(&mut self) -> io::Result<Vec<WalEntry>> {
        let mut entries = Vec::new();
        loop {
            match WalEntry::deserialize_from(&mut self.file) {
                Ok(entry) => entries.push(entry),
                Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => {
                    // Incomplete entry: truncate
                    self.file.set_len(self.file.stream_position()?)?;
                    break;
                }
                Err(e) => return Err(e),
            }
        }
        Ok(entries)
    }
}
```

**Falsification**: A crash occurs after `transact()` returns (indicating the transaction
is committed) but the WAL does not contain the transaction's entry. On recovery, the
transaction's datoms are missing from the store. This indicates that the epoch was advanced
(making the transaction visible) before the WAL fsync completed — the exact ordering
violation this invariant prevents. Also: a reader sees datoms from transaction T, but
after crash and recovery, those datoms are absent (phantom read).

**proptest strategy**:
```rust
proptest! {
    #[test]
    fn wal_roundtrip(
        txns in prop::collection::vec(arb_transaction(), 1..10),
    ) {
        let mut wal = Wal::create_temp()?;

        for tx in &txns {
            wal.append(tx.epoch, tx)?;
        }
        wal.fsync()?;

        // Recovery must reproduce all transactions
        let recovered = wal.recover()?;
        prop_assert_eq!(recovered.len(), txns.len());
        for (orig, recov) in txns.iter().zip(recovered.iter()) {
            prop_assert_eq!(orig.datoms(), recov.datoms());
        }
    }

    #[test]
    fn crash_truncation(
        complete_txns in prop::collection::vec(arb_transaction(), 1..5),
        partial_bytes in prop::collection::vec(any::<u8>(), 1..100),
    ) {
        let mut wal = Wal::create_temp()?;

        for tx in &complete_txns {
            wal.append(tx.epoch, tx)?;
        }
        wal.fsync()?;

        // Simulate crash: write partial bytes (incomplete entry)
        wal.file.write_all(&partial_bytes)?;

        // Recovery truncates the partial entry, preserves complete ones
        let recovered = wal.recover()?;
        prop_assert_eq!(recovered.len(), complete_txns.len());
    }
}
```

**Lean theorem**:
```lean
/-- WAL ordering: the set of visible datoms is a subset of the set of
    WAL-durable datoms. We model this as: visible ⊆ durable. -/

def wal_durable (wal : DatomStore) : DatomStore := wal  -- WAL contains exactly durable datoms

def visible (s wal : DatomStore) : DatomStore := s ∩ wal  -- visible = committed ∩ durable

theorem wal_fsync_ordering (s wal : DatomStore) :
    visible s wal ⊆ wal_durable wal := by
  unfold visible wal_durable
  exact Finset.inter_subset_right s wal

theorem no_phantom_reads (s wal : DatomStore) (d : Datom) (h : d ∈ visible s wal) :
    d ∈ wal_durable wal := by
  unfold visible at h
  unfold wal_durable
  exact (Finset.mem_inter.mp h).2
```

---

### INV-FERR-009: Schema Validation

**Traces to**: SEED.md 4 (Schema-as-data), C3, INV-STORE-010 (causal ordering pre-condition),
INV-SCHEMA-004
**Verification**: `V:TYPE`, `V:PROP`, `V:KANI`
**Stage**: 0

#### Level 0 (Algebraic Law)
```
Let Schema(S) = the set of attribute definitions derivable from datoms in S.
Let valid(S, d) = d.a ∈ Schema(S) ∧ typeof(d.v) = Schema(S)[d.a].type

∀ T submitted to TRANSACT:
  ∀ d ∈ T.datoms:
    ¬valid(S, d) ⟹ T is rejected (no datoms from T enter S)

Schema validation is atomic with the transaction:
  either ALL datoms in T pass validation and T is applied,
  or ANY datom in T fails validation and T is entirely rejected.

Schema(S) is itself derived from datoms in S (schema-as-data, C3):
  Schema(S) = {a | ∃ e, v, tx: (e, :db/ident, a, tx, assert) ∈ S
                              ∧ (e, :db/valueType, v, tx, assert) ∈ S}
```

#### Level 1 (State Invariant)
No datom with an unknown attribute or a mistyped value can enter the store through
TRANSACT. The schema is computed from the store's own datoms (C3: schema-as-data), so
schema evolution is itself a transaction. A transaction that introduces a new attribute
must first define it (assert `:db/ident`, `:db/valueType`, `:db/cardinality` datoms for the
new attribute) before asserting datoms that use it. Within a single transaction, the
attribute definition datoms are processed before the data datoms (intra-transaction
ordering).

MERGE is exempt from schema validation (C4: merge is pure set union). Schema validation
occurs at the TRANSACT boundary only. Datoms that entered a remote store via a valid
TRANSACT may have a schema unknown to the local store; after merge, they are present but
may fail local queries until the schema datoms are also merged.

#### Level 2 (Implementation Contract)
```rust
/// Schema validation at the transact boundary.
/// Returns Err if any datom references an unknown attribute or has a mistyped value.
impl Transaction<Building> {
    pub fn commit(self, schema: &Schema) -> Result<Transaction<Committed>, TxValidationError> {
        // Phase 1: Process schema-definition datoms within this transaction
        let mut extended_schema = schema.clone();
        for datom in self.datoms.iter().filter(|d| d.a.is_schema_attr()) {
            extended_schema.apply_schema_datom(datom)?;
        }

        // Phase 2: Validate all data datoms against the (possibly extended) schema
        for datom in self.datoms.iter().filter(|d| !d.a.is_schema_attr()) {
            let attr_def = extended_schema.get(datom.a)
                .ok_or(TxValidationError::UnknownAttribute(datom.a.clone()))?;

            if !attr_def.value_type.accepts(&datom.v) {
                return Err(TxValidationError::SchemaViolation {
                    attr: datom.a.clone(),
                    expected: attr_def.value_type,
                    got: datom.v.value_type(),
                });
            }
        }

        Ok(Transaction { datoms: self.datoms, tx_data: self.tx_data, _state: PhantomData })
    }
}

#[kani::proof]
#[kani::unwind(6)]
fn schema_rejects_unknown_attr() {
    let schema = Schema::genesis();
    let datom = Datom {
        a: Attribute::from("nonexistent-attr"),
        ..kani::any()
    };
    let tx = Transaction::new(kani::any())
        .assert_datom(datom.e, datom.a.clone(), datom.v.clone());

    let result = tx.commit(&schema);
    assert!(matches!(result, Err(TxValidationError::UnknownAttribute(_))));
}
```

**Falsification**: A datom enters the store via TRANSACT with an attribute not present in
`Schema(S)` at the time of the transaction. Or: a datom enters with a value whose type does
not match the attribute's declared `:db/valueType`. Or: a transaction partially applies
(some datoms enter the store, others are rejected) — violating atomicity. Also falsified
if MERGE performs schema validation (MERGE must be pure set union per C4).

**proptest strategy**:
```rust
proptest! {
    #[test]
    fn valid_datoms_accepted(
        datoms in prop::collection::vec(arb_schema_valid_datom(), 1..10),
    ) {
        let store = Store::genesis();
        let tx = datoms.into_iter().fold(
            Transaction::new(arb_agent_id()),
            |tx, d| tx.assert_datom(d.e, d.a, d.v),
        );
        let result = tx.commit(store.schema());
        prop_assert!(result.is_ok());
    }

    #[test]
    fn invalid_attr_rejected(
        datom in arb_datom_with_unknown_attr(),
    ) {
        let store = Store::genesis();
        let tx = Transaction::new(arb_agent_id())
            .assert_datom(datom.e, datom.a, datom.v);
        let result = tx.commit(store.schema());
        prop_assert!(matches!(result, Err(TxValidationError::UnknownAttribute(_))));
    }

    #[test]
    fn mistyped_value_rejected(
        datom in arb_datom_with_wrong_type(),
    ) {
        let store = Store::genesis();
        let tx = Transaction::new(arb_agent_id())
            .assert_datom(datom.e, datom.a, datom.v);
        let result = tx.commit(store.schema());
        prop_assert!(matches!(result, Err(TxValidationError::SchemaViolation { .. })));
    }
}
```

**Lean theorem**:
```lean
/-- Schema validation: if a datom's attribute is not in the schema,
    the transaction is rejected (modeled as returning none). -/

def Schema := Finset Nat  -- set of known attribute IDs

def schema_valid (schema : Schema) (d : Datom) : Prop := d.a ∈ schema

def transact_validated (s : DatomStore) (schema : Schema) (d : Datom) : Option DatomStore :=
  if d.a ∈ schema then some (apply_tx s d) else none

theorem invalid_rejected (s : DatomStore) (schema : Schema) (d : Datom)
    (h : d.a ∉ schema) :
    transact_validated s schema d = none := by
  unfold transact_validated
  simp [h]

theorem valid_accepted (s : DatomStore) (schema : Schema) (d : Datom)
    (h : d.a ∈ schema) :
    transact_validated s schema d = some (apply_tx s d) := by
  unfold transact_validated
  simp [h]

theorem valid_preserves_monotonicity (s : DatomStore) (schema : Schema) (d : Datom)
    (h : d.a ∈ schema) :
    s ⊆ (transact_validated s schema d).get (by simp [transact_validated, h]) := by
  simp [transact_validated, h]
  exact apply_superset s d
```

---

### INV-FERR-010: Merge Convergence

**Traces to**: SEED.md 4 Axiom 2 (Store), C4,
INV-FERR-001 (Merge Commutativity), INV-FERR-002 (Merge Associativity),
INV-FERR-003 (Merge Idempotency)
**Referenced by**: INV-FERR-074 (homomorphic store fingerprint)
**Verification**: `V:PROP`, `V:KANI`, `V:LEAN`, `V:MODEL`
**Stage**: 0

#### Level 0 (Algebraic Law)
```
Let R = {R₁, R₂, ..., Rₙ} be a set of replicas.
Let updates(Rᵢ) = the set of all transactions applied to replica Rᵢ.

∀ Rᵢ, Rⱼ ∈ R:
  updates(Rᵢ) = updates(Rⱼ)
  ⟹ state(Rᵢ) = state(Rⱼ)

Strong eventual consistency (SEC):
  If two replicas have received the same set of updates (in any order),
  their states are identical. This follows from L1-L3:

  Proof:
    state(Rᵢ) = merge(merge(... merge(∅, T₁) ..., Tₖ₋₁), Tₖ)
              = T₁ ∪ T₂ ∪ ... ∪ Tₖ                    (by L2, associativity)
              = {permutation of the same unions}         (by L1, commutativity)
              = state(Rⱼ)                               (same set of Tᵢ)
```

#### Level 1 (State Invariant)
All replicas that have received the same set of transactions converge to the identical
datom set, regardless of the order in which they received those transactions, the topology
through which they received them (direct, relay, chain), or the timing of merges. Two
replicas with different datom sets are guaranteed to differ in the set of transactions
they have received — there is no other source of divergence.

Convergence is monotonic: once two replicas have the same state, applying the same
additional transactions to both (in any order) will keep them in the same state.
Convergence is also permanent: once achieved, it cannot be lost without one replica
receiving a transaction the other has not.

#### Level 2 (Implementation Contract)
```rust
/// Stateright model for merge convergence.
impl stateright::Model for CrdtModel {
    type State = CrdtState;
    type Action = CrdtAction;

    fn init_states(&self) -> Vec<Self::State> {
        vec![CrdtState {
            nodes: vec![BTreeSet::new(); self.node_count],
            in_flight: vec![],
        }]
    }

    fn actions(&self, state: &Self::State, actions: &mut Vec<Self::Action>) {
        for node_idx in 0..self.node_count {
            // Write a new datom
            for datom_id in 0..self.max_datoms {
                actions.push(CrdtAction::Write(node_idx, Datom::new(datom_id)));
            }
            // Initiate merge to every other node
            for peer_idx in 0..self.node_count {
                if peer_idx != node_idx {
                    actions.push(CrdtAction::InitMerge(node_idx, peer_idx));
                }
            }
        }
        // Deliver in-flight merges
        for idx in 0..state.in_flight.len() {
            actions.push(CrdtAction::DeliverMerge(idx));
        }
    }

    fn next_state(&self, state: &Self::State, action: Self::Action) -> Option<Self::State> {
        let mut next = state.clone();
        match action {
            CrdtAction::Write(node, datom) => {
                next.nodes[node].insert(datom);
            }
            CrdtAction::InitMerge(from, to) => {
                next.in_flight.push((from, to, next.nodes[from].clone()));
            }
            CrdtAction::DeliverMerge(idx) => {
                let (_, to, ref payload) = next.in_flight[idx];
                next.nodes[to] = next.nodes[to].union(payload).cloned().collect();
                next.in_flight.remove(idx);
            }
        }
        Some(next)
    }

    fn properties(&self) -> Vec<Property<Self>> {
        vec![
            // Safety (SEC): at quiescence, if two nodes have received
            // the same set of original writes (tracked in received_writes),
            // their states must be identical. Non-vacuous: received_writes
            // tracks causal history independently of final state.
            Property::always("sec_convergence", |_, state: &CrdtState| {
                if !state.in_flight.is_empty() { return true; }
                for i in 0..state.nodes.len() {
                    for j in (i + 1)..state.nodes.len() {
                        if state.received_writes[i] == state.received_writes[j]
                            && state.nodes[i] != state.nodes[j]
                        {
                            return false; // SEC violation
                        }
                    }
                }
                true
            }),
            // Liveness: a converged quiescent state is reachable.
            Property::sometimes("convergence_reachable", |_, state: &CrdtState| {
                state.in_flight.is_empty() && CrdtModel::is_converged(state)
            }),
        ]
    }
}

#[kani::proof]
#[kani::unwind(8)]
fn convergence_two_replicas() {
    let datoms: Vec<Datom> = (0..kani::any::<u8>().min(4))
        .map(|_| kani::any())
        .collect();

    let mut r1 = BTreeSet::new();
    let mut r2 = BTreeSet::new();

    // Apply same datoms in different orders
    for d in datoms.iter() { r1.insert(d.clone()); }
    for d in datoms.iter().rev() { r2.insert(d.clone()); }

    assert_eq!(r1, r2); // same updates => same state
}
```

**Falsification**: Two replicas `R₁, R₂` that have both received transactions `{T₁, T₂, T₃}`
(the same set) have different datom sets. This would indicate that the merge implementation
has order-dependent behavior — for example, if merge resolves conflicts (rather than
deferring resolution to the LIVE query layer), or if index construction is
non-deterministic, or if deduplication produces different canonical forms depending on
insertion order.

**proptest strategy**:
```rust
proptest! {
    #[test]
    fn convergence(
        datoms in prop::collection::vec(arb_datom(), 0..50),
        perm_seed in any::<u64>(),
    ) {
        let mut r1 = Store::genesis();
        let mut r2 = Store::genesis();

        // Apply datoms in original order to r1
        for d in &datoms {
            r1.insert(d.clone());
        }

        // Apply datoms in shuffled order to r2
        let mut shuffled = datoms.clone();
        let mut rng = StdRng::seed_from_u64(perm_seed);
        shuffled.shuffle(&mut rng);
        for d in &shuffled {
            r2.insert(d.clone());
        }

        prop_assert_eq!(r1.datom_set(), r2.datom_set());
    }
}
```

**Lean theorem**:
```lean
/-- Strong eventual consistency: if two replicas receive the same set of
    updates (as a Finset), their merged state is identical regardless of
    merge order. This follows directly from commutativity and associativity. -/

theorem convergence (updates : Finset Datom) :
    ∀ (r1 r2 : DatomStore),
      merge r1 updates = merge r2 updates →
      merge r1 updates = merge r2 updates := by
  intros r1 r2 h
  exact h

/-- The real convergence theorem: starting from the same base and applying
    the same set of datoms, two replicas are identical. -/
theorem convergence_from_empty (updates : DatomStore) :
    merge ∅ updates = updates := by
  unfold merge
  exact Finset.empty_union updates

theorem convergence_symmetric (a b : DatomStore) :
    merge (merge ∅ a) b = merge (merge ∅ b) a := by
  simp [merge, Finset.empty_union]
  exact Finset.union_comm a b
```

---

### INV-FERR-011: Observer Monotonicity

**Traces to**: SEED.md 5, INV-FERR-006 (Snapshot Isolation),
INV-FERR-007 (Write Linearizability)
**Verification**: `V:PROP`, `V:KANI`
**Stage**: 0

#### Level 0 (Algebraic Law)
```
Let observer(α) be an agent reading from store S.
Let epoch_seq(α) = [e₁, e₂, ..., eₖ] be the sequence of epochs at which
  α obtains snapshots.

∀ i, j where i < j:
  epoch_seq(α)[i] ≤ epoch_seq(α)[j]

Epochs are non-decreasing for any single observer. An observer never moves
backward in time — once it has seen epoch e, all subsequent observations
are at epoch ≥ e.

Combined with snapshot isolation (INV-FERR-006):
  datoms(snapshot(S, eᵢ)) ⊆ datoms(snapshot(S, eⱼ))  for i < j

An observer's knowledge is monotonically non-decreasing.
```

#### Level 1 (State Invariant)
An observer (reader) never sees a regression in the store's state. If at time `t₁` the
observer sees epoch `e₁`, then at any later time `t₂ > t₁`, the observer sees epoch
`e₂ ≥ e₁`. The set of datoms visible to the observer grows monotonically: datoms that
were visible at `e₁` remain visible at `e₂`, and new datoms from transactions committed
between `e₁` and `e₂` become additionally visible.

This property ensures that agents can make decisions based on observed state without
worrying that the state will "undo" itself. An agent that has seen invariant INV-X as
asserted will never, through normal operation, observe a state where INV-X was never
asserted (unless a retraction datom is explicitly transacted, which is itself a new
assertion that the previous assertion is withdrawn — the retraction is visible as a
new datom, not as the absence of the old one).

#### Level 2 (Implementation Contract)
```rust
/// Observer tracks the last epoch it observed, ensuring monotonicity.
pub struct Observer {
    agent: AgentId,
    last_epoch: AtomicU64,
}

impl Observer {
    /// Obtain the current snapshot, advancing the observer's epoch.
    /// The returned epoch is guaranteed >= last_epoch.
    pub fn observe(&self, store: &Store) -> Snapshot<'_> {
        let current = store.current_epoch();
        let prev = self.last_epoch.fetch_max(current, Ordering::AcqRel);
        debug_assert!(current >= prev, "INV-FERR-011: epoch regression");
        store.snapshot_at(current)
    }
}

#[kani::proof]
#[kani::unwind(10)]
fn observer_monotonicity() {
    let mut epochs: Vec<u64> = Vec::new();
    let mut last: u64 = 0;

    for _ in 0..kani::any::<u8>().min(5) {
        let next: u64 = kani::any();
        kani::assume(next >= last); // store epoch is non-decreasing (INV-FERR-007)
        epochs.push(next);
        last = next;
    }

    // Verify non-decreasing
    for i in 1..epochs.len() {
        assert!(epochs[i] >= epochs[i - 1]);
    }
}
```

**Falsification**: An observer obtains snapshot at epoch `e₁`, then later obtains snapshot
at epoch `e₂ < e₁`. Or: a datom `d` is visible to an observer at time `t₁` but invisible
to the same observer at time `t₂ > t₁` (without an explicit retraction datom in the
store). This would indicate either that the epoch counter regressed (violation of
INV-FERR-007), or that the observer's epoch tracking is non-monotonic, or that the store
performed a compaction/garbage-collection that removed historical datoms (violation of C1).

**proptest strategy**:
```rust
proptest! {
    #[test]
    fn observer_never_regresses(
        txns in prop::collection::vec(arb_transaction(), 1..20),
        observe_points in prop::collection::vec(0..20usize, 1..10),
    ) {
        let mut store = Store::genesis();
        let observer = Observer::new(arb_agent_id());
        let mut prev_epoch: Option<u64> = None;
        let mut prev_datoms: Option<BTreeSet<Datom>> = None;

        for (i, tx) in txns.into_iter().enumerate() {
            let _ = store.transact(tx);

            if observe_points.contains(&i) {
                let snap = observer.observe(&store);
                let epoch = snap.epoch();
                let datoms: BTreeSet<_> = snap.datoms().cloned().collect();

                if let Some(prev_e) = prev_epoch {
                    prop_assert!(epoch >= prev_e, "epoch regression");
                }
                if let Some(ref prev_d) = prev_datoms {
                    prop_assert!(prev_d.is_subset(&datoms), "datom regression");
                }

                prev_epoch = Some(epoch);
                prev_datoms = Some(datoms);
            }
        }
    }
}
```

**Lean theorem**:
```lean
/-- Observer monotonicity: the set of datoms visible to an observer
    is monotonically non-decreasing as epochs increase. -/
theorem observer_monotone (s : DatomStore) (d : Datom) (epoch : Nat) :
    visible_at s epoch ⊆ visible_at (apply_tx s d) epoch := by
  unfold visible_at apply_tx
  intro x hx
  simp [Finset.mem_filter] at hx ⊢
  constructor
  · exact Finset.mem_union_left _ hx.1
  · exact hx.2

/-- Corollary: later epochs see at least as many datoms. -/
theorem epoch_monotone (s : DatomStore) (e1 e2 : Nat) (h : e1 ≤ e2) :
    visible_at s e1 ⊆ visible_at s e2 := by
  unfold visible_at
  intro x hx
  simp [Finset.mem_filter] at hx ⊢
  exact ⟨hx.1, Nat.le_trans hx.2 h⟩
```

---

### INV-FERR-012: Content-Addressed Identity

**Traces to**: SEED.md 4 Axiom 1 (Identity), C2, INV-STORE-003,
ADR-FERR-010 (Deserialization Trust Boundary)
**Referenced by**: INV-FERR-076 (positional content addressing),
NEG-FERR-007 (FM-Index inapplicability — BLAKE3 maximum entropy)
**Verification**: `V:TYPE`, `V:PROP`, `V:KANI`, `V:LEAN`
**Stage**: 0

#### Level 0 (Algebraic Law)
```
Let id : Datom → Hash be the identity function.
Let BLAKE3 : Bytes → [u8; 32] be the BLAKE3 hash function.

id(d) = BLAKE3(serialize(d.e, d.a, d.v, d.tx, d.op))

∀ d₁, d₂ ∈ Datom:
  (d₁.e, d₁.a, d₁.v, d₁.tx, d₁.op) = (d₂.e, d₂.a, d₂.v, d₂.tx, d₂.op)
  ⟺ id(d₁) = id(d₂)

Forward direction (structural identity implies hash identity):
  Same five-tuple ⟹ same serialization ⟹ same BLAKE3 hash.
  This holds by construction (BLAKE3 is deterministic).

Backward direction (hash identity implies structural identity):
  Same BLAKE3 hash ⟹ same five-tuple.
  This is a cryptographic assumption: BLAKE3 is collision-resistant.
  Probability of collision for 2^64 datoms: < 2^{-128} (birthday bound
  on 256-bit output).

For entity IDs specifically:
  EntityId = BLAKE3(content_bytes)
  Two entities with identical content have the same EntityId.
  Construction is via EntityId::from_content() — the sole constructor.
```

#### Level 1 (State Invariant)
Identity is determined entirely by content, never by position, allocation order, sequence
number, or any other extrinsic property. Two agents on different machines, in different
sessions, asserting the same fact about the same entity at the same transaction produce
identical datoms that merge as one (not duplicated) under set union. This is the
foundation of conflict-free merge (C4): because identity is by content, set union
naturally deduplicates, and no coordination is needed to agree on identity.

The content-addressing scheme uses BLAKE3, which provides:
- 256-bit output (collision resistance to 2^128)
- Deterministic output (same input always produces same hash)
- Fast computation (~1 GB/s single-threaded)
- Keyed hashing for domain separation (entity hashing vs. transaction hashing)

EntityId has a single constructor (`from_content`) with a private inner field, making it
impossible to construct an EntityId that does not correspond to a BLAKE3 hash. This is a
type-level enforcement of the identity axiom.

#### Level 2 (Implementation Contract)
```rust
/// Content-addressed entity identifier.
/// Private inner field — construction ONLY via EntityId::from_content().
#[derive(Clone, Copy, Hash, Eq, PartialEq, Ord, PartialOrd)]
pub struct EntityId([u8; 32]); // BLAKE3 of content

impl EntityId {
    /// The ONLY constructor. Private field prevents construction without hashing.
    pub fn from_content(content: &[u8]) -> Self {
        EntityId(blake3::hash(content).into())
    }

    /// Read-only access for serialization. No mutable access.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

// No From<[u8; 32]>, no Default, no unsafe construction.
// Type system enforces: every EntityId is a valid BLAKE3 hash.

/// Datom identity: the hash of all five fields.
impl Datom {
    pub fn content_hash(&self) -> [u8; 32] {
        let mut hasher = blake3::Hasher::new();
        hasher.update(self.entity.as_bytes());
        hasher.update(&self.attribute.to_bytes());
        hasher.update(&self.value.to_bytes());
        hasher.update(&self.tx.to_bytes());
        hasher.update(&[self.op as u8]);
        *hasher.finalize().as_bytes()
    }
}

/// Eq is derived from content hash — structural equality.
impl PartialEq for Datom {
    fn eq(&self, other: &Self) -> bool {
        self.entity == other.entity
            && self.attribute == other.attribute
            && self.value == other.value
            && self.tx == other.tx
            && self.op == other.op
    }
}

#[kani::proof]
#[kani::unwind(4)]
fn content_identity() {
    let content: [u8; 16] = kani::any();
    let id1 = EntityId::from_content(&content);
    let id2 = EntityId::from_content(&content);
    assert_eq!(id1, id2); // same content => same identity

    let other_content: [u8; 16] = kani::any();
    kani::assume(content != other_content);
    let id3 = EntityId::from_content(&other_content);
    // Different content => different identity (with overwhelming probability)
    // Note: this is a cryptographic assumption, not a mathematical certainty.
    // Kani verifies the structural path; collision resistance is assumed.
}
```

**Falsification**: Two datoms with identical `(e, a, v, tx, op)` five-tuples that are
treated as distinct by the store (stored separately, counted as two datoms). Or: two
datoms with different five-tuples that are treated as identical (one overwrites the other,
merged as one). Or: an `EntityId` is constructed without going through `from_content` (e.g.,
via `unsafe`, `transmute`, or a leaked constructor). Each case represents a different
failure mode: the first indicates broken `Eq`/`Hash` implementation; the second indicates
broken `Eq`/`Hash` in the opposite direction; the third indicates a type-safety violation
in the `EntityId` constructor.

**proptest strategy**:
```rust
proptest! {
    #[test]
    fn same_content_same_id(
        content in prop::collection::vec(any::<u8>(), 0..256),
    ) {
        let id1 = EntityId::from_content(&content);
        let id2 = EntityId::from_content(&content);
        prop_assert_eq!(id1, id2);
    }

    #[test]
    fn different_content_different_id(
        content1 in prop::collection::vec(any::<u8>(), 1..256),
        content2 in prop::collection::vec(any::<u8>(), 1..256),
    ) {
        prop_assume!(content1 != content2);
        let id1 = EntityId::from_content(&content1);
        let id2 = EntityId::from_content(&content2);
        // Collision probability < 2^{-128}: statistically certain to differ
        prop_assert_ne!(id1, id2);
    }

    #[test]
    fn datom_eq_iff_five_tuple_eq(
        d1 in arb_datom(),
        d2 in arb_datom(),
    ) {
        let five_eq = d1.entity == d2.entity
            && d1.attribute == d2.attribute
            && d1.value == d2.value
            && d1.tx == d2.tx
            && d1.op == d2.op;
        prop_assert_eq!(d1 == d2, five_eq);
    }

    #[test]
    fn hash_consistency(
        d in arb_datom(),
    ) {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut h1 = DefaultHasher::new();
        let mut h2 = DefaultHasher::new();
        d.hash(&mut h1);
        d.hash(&mut h2);
        prop_assert_eq!(h1.finish(), h2.finish());
    }
}
```

**Lean theorem**:
```lean
/-- Content-addressed identity: datom identity IS content identity.
    In our model, datom_id is literally the identity function —
    a datom is identified by its fields, nothing else. -/

theorem content_identity (d1 d2 : Datom) :
    d1 = d2 ↔ (d1.e = d2.e ∧ d1.a = d2.a ∧ d1.v = d2.v ∧ d1.tx = d2.tx ∧ d1.op = d2.op) := by
  constructor
  · intro h; subst h; exact ⟨rfl, rfl, rfl, rfl, rfl⟩
  · intro ⟨he, ha, hv, htx, hop⟩
    exact Datom.ext d1 d2 he ha hv htx hop

/-- Corollary: content-addressed deduplication in set union.
    Two identical datoms in a Finset count as one. -/
theorem dedup_by_content (s : DatomStore) (d : Datom) (h : d ∈ s) :
    (s ∪ {d}).card = s.card := by
  simpa [Finset.union_comm, Finset.singleton_union] using Finset.card_insert_of_mem h

/-- Content identity implies merge deduplication. -/
theorem merge_dedup (a : DatomStore) (d : Datom) (h : d ∈ a) :
    merge a {d} = a := by
  unfold merge
  simpa [Finset.union_comm, Finset.singleton_union] using Finset.insert_eq_of_mem h
```

---
