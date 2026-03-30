# 03 — Test Suite Writing (Red Phase TDD)

> **Purpose**: Write tests that define expected behavior. ALL tests must FAIL initially.
> Tests are written BEFORE implementation. A passing test with no implementation is a bug
> in the test.
>
> **DoF**: Low. The spec defines exactly what to test. Execute mechanically.

---

## Phase 0: Load Context

```bash
ms load spec-first-design -m --full    # Spec interpretation for test contracts
bv --robot-next                        # Top-priority pick
br update <id> --status in_progress    # Claim it
```

---

## Workflow

```
Read spec invariant (Level 2 contract + proptest strategy)
    --> Write proptest property
    --> Write integration test
    --> Run tests: ALL MUST FAIL (red phase)
    --> Commit failing tests
```

---

## Demonstration: INV-FERR-001 (Merge Commutativity)

### 1. Read the spec

From `spec/01-core-invariants.md`, INV-FERR-001 provides both the
proptest strategy and the falsification condition verbatim.

### 2. Write the generator

Generators live in `ferratomic-verify/proptest/`. One generator module
per domain type, shared across all property tests.

```rust
// ferratomic-verify/proptest/generators.rs

use ferratom::{Datom, EntityId, Attribute, Value, Op, TxId};
use proptest::prelude::*;

/// Arbitrary Datom. Covers the full value space with weighted distribution
/// toward edge cases (empty strings, zero values, retract ops).
pub fn arb_datom() -> impl Strategy<Value = Datom> {
    (
        arb_entity_id(),
        arb_attribute(),
        arb_value(),
        arb_tx_id(),
        prop_oneof![Just(Op::Assert), Just(Op::Retract)],
    )
        .prop_map(|(e, a, v, tx, op)| Datom::new(e, a, v, tx, op))
}

pub fn arb_entity_id() -> impl Strategy<Value = EntityId> {
    any::<[u8; 32]>().prop_map(EntityId::from_bytes)
}

pub fn arb_attribute() -> impl Strategy<Value = Attribute> {
    "[a-z][a-z0-9_/]{0,63}".prop_map(|s| Attribute::from(s))
}

pub fn arb_value() -> impl Strategy<Value = Value> {
    prop_oneof![
        any::<i64>().prop_map(Value::Long),
        any::<bool>().prop_map(Value::Bool),
        ".*".prop_map(Value::String),
        any::<f64>()
            .prop_filter("not NaN", |f| !f.is_nan())
            .prop_map(Value::Double),
    ]
}

pub fn arb_tx_id() -> impl Strategy<Value = TxId> {
    (any::<u64>(), any::<u32>(), any::<u16>())
        .prop_map(|(wall, counter, node)| TxId::new(wall, counter, node))
}

pub fn arb_store(max_datoms: usize) -> impl Strategy<Value = Store> {
    prop::collection::btree_set(arb_datom(), 0..max_datoms)
        .prop_map(Store::from_datoms)
}
```

### 3. Write the property test

```rust
// ferratomic-verify/proptest/crdt_properties.rs

use crate::generators::*;
use proptest::prelude::*;

proptest! {
    /// INV-FERR-001: merge(A, B) == merge(B, A) for all store pairs.
    ///
    /// Falsification: any pair (A, B) where the datom set of merge(A, B)
    /// differs from merge(B, A). Would indicate order-dependent operations
    /// in the merge path.
    #[test]
    fn inv_ferr_001_merge_commutativity(
        a in arb_store(100),
        b in arb_store(100),
    ) {
        let ab = merge(&a, &b);
        let ba = merge(&b, &a);

        prop_assert_eq!(
            ab.datom_set(),
            ba.datom_set(),
            "INV-FERR-001 violated: merge(A,B) != merge(B,A). \
             |A|={}, |B|={}, |A∪B|={}, |B∪A|={}",
            a.len(), b.len(), ab.len(), ba.len()
        );
    }
}
```

### 4. Write the integration test

```rust
// ferratomic-verify/integration/test_crdt.rs

/// INV-FERR-001: Concrete merge commutativity with known stores.
#[test]
fn inv_ferr_001_merge_commutes_concrete() {
    let a = Store::from_datoms(btreeset![
        datom!("e1", "name", "Alice", tx(1), assert),
        datom!("e2", "age", 30, tx(2), assert),
    ]);
    let b = Store::from_datoms(btreeset![
        datom!("e2", "age", 30, tx(2), assert),  // overlap
        datom!("e3", "role", "admin", tx(3), assert),
    ]);

    let ab = merge(&a, &b);
    let ba = merge(&b, &a);

    assert_eq!(
        ab.datom_set(), ba.datom_set(),
        "INV-FERR-001: merge commutativity violated on concrete stores"
    );
    assert_eq!(ab.len(), 3, "Union of 2+2 with 1 overlap = 3 datoms");
}
```

### 5. Run and confirm failure

```bash
CARGO_TARGET_DIR=/data/cargo-target cargo test --workspace 2>&1 | head -50
# Expected: FAIL (Store, merge, Datom types not yet implemented)
```

Every test must fail because the types and functions do not exist yet.
If a test passes, it tests nothing. Fix or delete it.

---

## Test Organization

```
ferratomic-verify/
  proptest/
    generators.rs       # Arbitrary instances for all core types
    crdt_properties.rs  # INV-FERR-001..004: algebraic CRDT laws
    index_properties.rs # INV-FERR-005..007: index consistency
    wal_properties.rs   # INV-FERR-008: WAL ordering
    schema_properties.rs# INV-FERR-009..011: schema validation
  integration/
    test_crdt.rs        # Concrete merge/transact scenarios
    test_snapshot.rs    # Snapshot isolation under concurrent reads
    test_recovery.rs    # WAL replay after crash simulation
    test_schema.rs      # Schema evolution with live data
  kani/                 # Bounded model checking (when ready)
  stateright/           # Protocol model checking (when ready)
```

---

## Test Naming Convention

```
inv_ferr_NNN_short_description
```

Examples:
- `inv_ferr_001_merge_commutativity`
- `inv_ferr_005_eavt_index_covers_all_datoms`
- `inv_ferr_008_wal_entry_precedes_snapshot`

Every test name starts with the invariant it verifies.
`grep -r inv_ferr_ ferratomic-verify/` must produce
a complete cross-reference of coverage.

---

## Failure Message Format

Every assertion must include the INV-FERR ID and enough context
to diagnose a failure without reading the test source:

```rust
assert_eq!(
    result, expected,
    "INV-FERR-005: datom {} present in store but missing from EAVT index. \
     Store size: {}, index size: {}",
    datom, store.len(), index.len()
);
```

---

## Checklist Per Invariant

For each INV-FERR you test:

1. Read Level 2 (implementation contract) and proptest strategy from spec
2. Write the generator if the type doesn't have one yet
3. Write the proptest property with 10,000+ cases
4. Write at least one concrete integration test with known inputs
5. Ensure failure messages cite INV-FERR-NNN
6. Run tests and confirm ALL FAIL
7. Commit: `test: red-phase tests for INV-FERR-NNN`
8. Close task: `br close <id> --reason "Red-phase tests committed"`

---

## What NOT To Do

- Do not write tests that pass. This is red phase.
- Do not implement types to make tests compile. That is Phase 3.
- Do not use `#[ignore]` to hide failures. Failures are the point.
- Do not test implementation details. Test the spec contract.
- Do not use `unwrap()` without a descriptive message in test code.
