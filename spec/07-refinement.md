## 23.11 Refinement Tower

The Ferratomic development methodology follows a strict phase ordering where each
phase refines the previous one. This section formalizes the **coupling invariants**
that connect adjacent layers of the refinement tower, making explicit the relationship
that ADR-FERR-007 (Lean-Rust bridge) acknowledges is currently bridged by proptest.

**Traces to**: ADR-FERR-007 (Lean-Rust Bridge), ADR-FERR-001 (Persistent Data Structures),
all INV-FERR-001 through INV-FERR-012 (properties proven in Lean, implemented in Rust).

**Methodology**: Morgan's data refinement (Ch. 17) and Back & von Wright's refinement
calculus (Ch. 26-27). See `docs/design/REFINEMENT_CHAINS.md` for the operational
application to Ferratomic's execution paths.

---

### The Refinement Tower

```
Level 0: SPECIFICATION (spec/ — this document)
  | refinement_0: spec formalizes domain intent
  v
Level 1: LEAN MODEL (ferratomic-verify/lean/ — Finset Datom)
  | refinement_1: data refinement via CI_lean_rust
  v
Level 2: RUST TYPES (ferratom/ — EntityId, Datom, Value, Schema)
  | refinement_2: implementation refinement via typestate encoding
  v
Level 3: RUST CODE (ferratomic-core/ — Store, Database, WAL, Checkpoint)
```

Each arrow is a formal refinement step (`sqsubseteq`). A property proven at Level 1
transfers to Level 3 if the coupling invariants at each boundary are maintained by
every operation. The chain is valid by transitivity of refinement.

---

### CI-FERR-001: Lean-Rust Coupling Invariant

**Stage**: 0
**Verification**: `V:PROP` (proptest bridging today), `V:LEAN` (target: mechanized proof)

#### Level 0 (Algebraic Definition)

```
Let L : Finset Datom         -- the Lean model (abstract store)
Let R : Store                -- the Rust implementation (concrete store)
Let to_finset : OrdSet -> Finset  -- the abstraction function

CI(L, R) :=
  (1)  to_finset(R.datoms) = L
  (2)  forall idx in {eavt, aevt, vaet, avet}:
         to_finset(R.indexes.idx) = L
  (3)  R.epoch = |{tx committed to L}|
  (4)  R.schema = derive_schema(L)

Where:
  to_finset is the abstraction function (Morgan's af(c)):
    maps im::OrdSet<Datom> to Finset Datom by forgetting tree structure.
  derive_schema scans L for schema-defining datom triples:
    (E, db/ident, Keyword(name)) /\ (E, db/valueType, Keyword(type))
    /\ (E, db/cardinality, Keyword(card))
```

**Conjuncts explained**:

1. **Datom set identity**: The Rust primary set, when viewed as a mathematical set
   (ignoring B-tree structure, memory layout, reference counts), equals the Lean
   `Finset Datom`. This is the fundamental adequacy condition: the concrete
   representation faithfully represents the abstract state.

2. **Index bijection**: Every secondary index, viewed as a set, equals the primary.
   This is INV-FERR-005 expressed as a component of the coupling invariant rather
   than a standalone invariant. In the refinement framework, INV-FERR-005 is a
   **data-type invariant** (Morgan's `dti(c)`) — a property of the concrete
   representation that has no analogue in the abstract model (Lean has no indexes).

3. **Epoch correspondence**: The Rust epoch counter equals the number of committed
   transactions in the Lean model. This connects the concrete counter to the
   abstract transaction history.

4. **Schema derivation**: The Rust schema is derivable from the Lean datom set
   via the schema-as-data bootstrap (C3). Schema is not independent state — it
   is a derived view of the datom set.

#### Level 1 (State Invariant)

For every reachable state of the system — produced by any interleaving of genesis,
transact, merge, checkpoint, and recover operations — the Lean model and the Rust
implementation agree on the datom set, epoch, and schema.

The coupling invariant is **established** by genesis (both Lean and Rust produce the
empty datom set with 19 meta-schema attributes) and **preserved** by every operation.

Preservation obligations (one per operation):

| Operation | Obligation |
|-----------|-----------|
| `genesis()` | `CI(empty_finset, Store::genesis())` — both produce identical empty stores |
| `transact(tx)` | `CI(L, R) /\ valid(tx) => CI(lean_transact(L, tx), rust_transact(R, tx))` |
| `merge(A, B)` | `CI(La, Ra) /\ CI(Lb, Rb) => CI(lean_merge(La, Lb), rust_merge(Ra, Rb))` |
| `snapshot(e)` | `CI(L, R) => lean_snapshot(L, e) = to_finset(rust_snapshot(R, e).datoms)` |
| `checkpoint + load` | `CI(L, R) => CI(L, load_checkpoint(write_checkpoint(R)))` |
| `recover(wal)` | `CI(L, R) => CI(L, recover_from_wal(wal_of(R)))` |

The transact obligation is the most complex because transact performs multiple
sub-operations (epoch advance, TxId stamping, schema evolution, index update).
See `docs/design/REFINEMENT_CHAINS.md` for the decomposed refinement chain.

#### Level 2 (Implementation Contract)

The coupling invariant is currently enforced by three mechanisms:

**Structural enforcement (compile-time)**:
- `EntityId([u8; 32])` — private inner field prevents non-content-addressed construction.
- `Transaction<Committed>` — typestate prevents accessing datoms before validation.
- `Store.datoms` — private field prevents external mutation.
- `#![forbid(unsafe_code)]` — no backdoor around type system.

**proptest bridge (runtime, probabilistic)**:
```rust
// ferratomic-verify/proptest/crdt_properties.rs
// Every proptest implicitly checks CI by performing the same operation
// on both a BTreeSet (proxy for Finset) and a Store (concrete).
proptest! {
    fn inv_ferr_001_merge_commutativity(
        a in arb_store(50), b in arb_store(50)
    ) {
        let ab = merge(&a, &b);
        let ba = merge(&b, &a);
        prop_assert_eq!(ab.datom_set(), ba.datom_set());
        // Implicit CI check: Store::from_datoms builds from BTreeSet
        // (abstract), and the result is checked via datom_set() which
        // returns the OrdSet contents (concrete). Agreement = CI holds.
    }
}
```

**Lean proofs (abstract, mechanized)**:
```lean
-- ferratomic-verify/lean/Ferratomic/Store.lean
-- All theorems operate on Finset Datom (the abstract model).
-- CI_lean_rust would formalize the bridge:
--
-- theorem ci_transact_preserved (L : DatomStore) (d : Datom)
--     (R : RustStore) (h : CI L R) :
--     CI (apply L d) (rust_apply R d) := by
--   -- Proof: apply = Finset.insert, rust_apply = OrdSet.insert
--   -- Both add d to their respective sets.
--   -- to_finset(OrdSet.insert(R.datoms, d)) = Finset.insert(L, d)
--   -- follows from the representation invariant of OrdSet.
--   sorry  -- TODO: mechanize when OrdSet model is defined in Lean
```

**Target state**: Mechanized Lean proof of CI preservation for all operations,
eliminating the proptest bridge as a correctness-critical element. Proptest
remains as a regression test but is no longer load-bearing for correctness.

---

### CI-FERR-002: Type-Level Refinement (Curry-Howard Encoding)

**Stage**: 0
**Verification**: Rust compiler (structural)

#### Level 0 (Algebraic Definition)

```
The ferratom types encode propositions via the Curry-Howard correspondence.
Each type's cardinality equals the number of valid states for that concept.

Type           | Proposition encoded                    | Cardinality
-------------- | -------------------------------------- | -----------
EntityId       | Identity is BLAKE3(content)            | 2^256 (hash space)
Attribute      | Name is interned (O(1) clone)          | countable
Value          | Exactly 11 value domains                | sum of 11 variants
Op             | Assert or Retract (no third option)     | 2
Datom          | Immutable 5-tuple fact                  | product of 5 fields
TxId           | Causally ordered timestamp              | (u64, u32, AgentId)
Schema         | Attribute -> Definition partial function| finite map
Store          | Append-only datom set with invariants   | (OrdSet, Indexes, Schema, u64)
```

The refinement from abstract to concrete is:

```
Lean (Finset Datom)                     Rust (Store)
  |                                       |
  | Datom = 5-field structure             | Datom = struct with private fields
  | DatomStore = Finset Datom             | Store = OrdSet<Datom> + Indexes
  | merge = Finset.union                  | merge = OrdSet.union + index rebuild
  | apply = Finset.insert                 | transact = stamp + insert + schema evolve
  |                                       |
  | (no indexes, no WAL, no epoch)        | (indexes, WAL, epoch, schema)
```

The concrete side has ADDITIONAL structure (indexes, WAL, epoch counter) that
does not exist in the abstract model. In refinement calculus terms, these are
**auxiliary concrete variables** introduced during data refinement. The coupling
invariant constrains them:
- Indexes must be in bijection with the primary set (INV-FERR-005)
- Epoch must equal the transaction count (INV-FERR-007)
- WAL must contain all committed transactions (INV-FERR-008)

These constraints are the **data-type invariant** `dti(c)` from Morgan's
Law 17.15. They must be maintained by every operation but have no
abstract counterpart.

#### Level 1 (State Invariant)

Every public type in the `ferratom` crate admits exactly the valid states for its
domain concept. Invalid states are unrepresentable at compile time (not caught at
runtime). This is the Curry-Howard principle: the type IS the proof that the value
is valid.

Examples of invalid states prevented by the type system:

| Invalid state | How prevented |
|--------------|---------------|
| EntityId not derived from content | `from_content()` is the only production constructor; `from_bytes()` is test-gated |
| Datom with mutated field | All fields are private; no `&mut self` methods exist |
| Transaction accessing datoms before commit | `datoms()` exists only on `Transaction<Committed>`, not `Transaction<Building>` |
| Float value without total ordering | `Value::Double(OrderedFloat<f64>)` provides Eq + Ord + Hash |
| Schema with duplicate attribute names | `Schema` uses `HashMap<Attribute, AttributeDef>` (key uniqueness by construction) |

#### Level 2 (Implementation Contract)

The type-level refinement is enforced by Rust's type system and visibility rules.
No runtime checks are needed for these properties. The compiler rejects programs
that attempt to construct invalid states.

```rust
// This does NOT compile — EntityId::from_bytes is test-gated:
#[cfg(not(any(test, feature = "test-utils")))]
let bad_id = EntityId::from_bytes([0u8; 32]);  // ERROR: method not found

// This does NOT compile — datoms() is only on Committed:
let tx = Transaction::<Building>::new(agent);
let datoms = tx.datoms();  // ERROR: no method `datoms` on Transaction<Building>

// This does NOT compile — Datom fields are private:
let d = Datom::new(e, a, v, tx, op);
d.entity = other_id;  // ERROR: field `entity` is private
```

---

### Refinement Verification Strategy

The refinement tower is verified by three complementary mechanisms:

| Boundary | Mechanism | Strength | Gap |
|----------|-----------|----------|-----|
| Level 0 -> 1 | Human review (spec matches Lean statements) | Informal | Spec-Lean isomorphism not mechanized |
| Level 1 -> 2 | CI-FERR-001 (coupling invariant, proptest today) | Probabilistic | Not a proof; 10,000 cases may miss edge cases |
| Level 2 -> 3 | Rust type system + CI-FERR-002 | Structural | Complete for type-level properties; does not cover runtime behavior |

**Target**: Mechanize the Level 1 -> 2 boundary in Lean (CI-FERR-001), reducing
the overall gap to:
- Level 0 -> 1: human review (inherently informal — specs are natural language + math)
- Level 1 -> 3: mechanized proof (Lean proves abstract properties transfer to concrete)

This is the strongest achievable result: the spec-to-Lean boundary is always
informal (you cannot prove that a natural-language spec "means" what the Lean
formalization says), but everything downstream of Lean would be mechanically verified.
