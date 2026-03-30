# 11 Federation & Integration Testing

> **Purpose**: Verify cross-store, cross-crate, and distributed correctness.
> **DoF**: High for test design, Low for test execution.
> **Cognitive mode**: Adversarial verification (find the failure, not the success).

---

## Phase 0: Load Context

```bash
ms load rust-formal-engineering -m --full   # Algebraic properties of CRDT merge + transport
```

---

## What Federation Testing Verifies

Federation (spec/05-federation.md, INV-FERR-037 through INV-FERR-044) introduces
cross-store interactions. Every property that holds for one store must still hold
when two or more stores interact through transports, merges, and queries.

The three categories:

| Category | What breaks | INV-FERR |
|----------|------------|----------|
| **Transport transparency** | Query results differ by transport | 038 |
| **Selective merge correctness** | Merge violates CRDT properties or leaks filtered datoms | 039, 040, 043 |
| **Partition tolerance** | Temporary disconnection causes data loss or divergence | 036, 041 |

---

## Multi-Store Test Setup

Every federation test starts with this pattern:

```rust
fn setup_two_stores() -> (Store, Store) {
    let schema = test_schema();  // Shared schema (INV-FERR-043)
    let store_a = Store::empty_with_schema(schema.clone());
    let store_b = Store::empty_with_schema(schema);

    // Shared history: both stores start from the same genesis
    let genesis_datoms = vec![
        make_datom("e1", ":name", "shared-entity"),
    ];
    let store_a = store_a.apply_datoms(&genesis_datoms).unwrap();
    let store_b = store_b.apply_datoms(&genesis_datoms).unwrap();

    // Diverge: each store gets unique writes
    let store_a = store_a.apply_datoms(&vec![
        make_datom("e2", ":name", "alice"),
        make_datom("e2", ":role", "engineer"),
    ]).unwrap();

    let store_b = store_b.apply_datoms(&vec![
        make_datom("e3", ":name", "bob"),
        make_datom("e3", ":role", "designer"),
    ]).unwrap();

    (store_a, store_b)
}
```

**Invariant**: After `merge(a, b)`, the result contains ALL datoms from both stores.
No datom is invented. No datom is lost. This is the CRDT guarantee (C4).

---

## Transport Transparency Testing (INV-FERR-038)

The same query against the same store MUST produce identical results regardless
of which transport carries the request.

```rust
#[test]
fn test_inv_ferr_038_transport_transparency() {
    let store = populated_store();  // 1000+ datoms
    let query = Query::find_by_attribute(":name");

    // Execute via LocalTransport (in-process, zero-copy)
    let local = LocalTransport::new(store.clone());
    let result_local = local.query(&query).unwrap();

    // Execute via LoopbackTransport (serialize -> deserialize round-trip)
    let loopback = LoopbackTransport::new(store.clone());
    let result_loopback = loopback.query(&query).unwrap();

    // Results must be identical
    assert_eq!(
        result_local, result_loopback,
        "INV-FERR-038: transport must not affect query results"
    );
}
```

**Test matrix**: Run every query type (point, range, pattern) across every transport.
The cross-product IS the test suite.

---

## Selective Merge Verification (INV-FERR-039)

Selective merge transfers a SUBSET of datoms from source to destination.
Five properties must hold:

```rust
#[test]
fn test_selective_merge_properties() {
    let (store_a, store_b) = setup_two_stores();
    let filter = DatomFilter::AttributeNamespace(":role");

    let merged = store_a.selective_merge(&store_b, &filter).unwrap();

    // 1. Filtered datoms present
    let role_datoms_b: Vec<_> = store_b.datoms()
        .filter(|d| d.attribute().starts_with(":role"))
        .collect();
    for datom in &role_datoms_b {
        assert!(merged.contains(datom),
            "INV-FERR-039: filtered datom from source must appear in result");
    }

    // 2. Non-filtered datoms from source NOT present (unless already in dest)
    let name_only_in_b: Vec<_> = store_b.datoms()
        .filter(|d| d.attribute().starts_with(":name"))
        .filter(|d| !store_a.contains(d))
        .collect();
    for datom in &name_only_in_b {
        assert!(!merged.contains(datom),
            "INV-FERR-039: non-filtered datom must not leak through selective merge");
    }

    // 3. All original dest datoms preserved
    for datom in store_a.datoms() {
        assert!(merged.contains(datom),
            "Selective merge must not drop destination datoms");
    }

    // 4. Idempotent
    let merged_again = merged.selective_merge(&store_b, &filter).unwrap();
    assert_eq!(merged.datom_count(), merged_again.datom_count(),
        "Selective merge must be idempotent");

    // 5. CRDT monotonicity
    assert!(merged.datom_count() >= store_a.datom_count(),
        "C4: merge result must be >= either input (monotonic)");
}
```

---

## Demonstration: Federated Query Test

**Scenario**: Two stores, fan-out query, merge results, verify correctness.

```rust
/// INV-FERR-037: Federated query returns the union of matching datoms
/// from all participating stores.
#[test]
fn test_federated_query_correctness() {
    // Setup: two stores with overlapping and unique data
    let schema = test_schema();
    let mut federation = Federation::new();

    let store_a = Store::empty_with_schema(schema.clone());
    let store_a = store_a.apply_datoms(&vec![
        make_datom("e1", ":name", "alice"),
        make_datom("e1", ":dept", "engineering"),
        make_datom("e2", ":name", "charlie"),         // unique to A
    ]).unwrap();

    let store_b = Store::empty_with_schema(schema);
    let store_b = store_b.apply_datoms(&vec![
        make_datom("e1", ":name", "alice"),            // overlap with A
        make_datom("e3", ":name", "bob"),              // unique to B
        make_datom("e3", ":dept", "design"),
    ]).unwrap();

    let id_a = federation.add_store(store_a.clone(), LocalTransport::new);
    let id_b = federation.add_store(store_b.clone(), LocalTransport::new);

    // Fan-out query: find all :name datoms
    let query = Query::find_by_attribute(":name");
    let result = federation.query(&query, &[id_a, id_b]).unwrap();

    // Verify: result is the union of both stores' matches
    assert_eq!(result.datom_count(), 3,
        "INV-FERR-037: federated query must return union (alice + charlie + bob)");

    // Verify: per-store metadata present
    assert_eq!(result.store_responses().len(), 2,
        "Each participating store must have a StoreResponse");
    assert!(result.store_responses().iter().all(|r| r.is_complete()),
        "INV-FERR-041: all stores responded (no partial results)");

    // Verify: no invented datoms
    for datom in result.datoms() {
        assert!(store_a.contains(&datom) || store_b.contains(&datom),
            "C4: federated query must not invent datoms");
    }

    // Verify: transport transparency -- same result via LoopbackTransport
    let mut fed_loopback = Federation::new();
    fed_loopback.add_store(store_a, LoopbackTransport::new);
    fed_loopback.add_store(store_b, LoopbackTransport::new);
    let result_loopback = fed_loopback.query(&query, &[id_a, id_b]).unwrap();
    assert_eq!(result.datoms().collect::<Vec<_>>(),
               result_loopback.datoms().collect::<Vec<_>>(),
        "INV-FERR-038: transport must not affect federated query results");
}
```

---

## Partition Tolerance Testing (INV-FERR-036, INV-FERR-041)

Simulate network partitions and verify graceful degradation:

```rust
#[test]
fn test_partition_tolerance() {
    let (store_a, store_b) = setup_two_stores();
    let mut federation = Federation::new();
    let id_a = federation.add_store(store_a, LocalTransport::new);
    let id_b = federation.add_store(store_b, PartitionedTransport::new);

    // Store B is partitioned (all requests timeout)
    let query = Query::find_by_attribute(":name");
    let result = federation.query(&query, &[id_a, id_b]).unwrap();

    // Partial result from store A only
    assert!(result.store_responses()[0].is_complete());
    assert!(result.store_responses()[1].is_partial(),
        "INV-FERR-041: timed-out store flagged as partial");

    // Result contains A's datoms, not B's
    assert!(result.datom_count() > 0, "Available store data returned");
}
```

---

## VKN Signature Verification (INV-FERR-044)

If namespace isolation is enabled, verify that stores cannot access datoms
outside their authorized namespaces:

```rust
#[test]
fn test_namespace_isolation() {
    let store = populated_store_with_namespaces();
    let filter = DatomFilter::AttributeNamespace(":secret/");

    // Store with VKN for ":public/" only
    let restricted = store.with_namespace_restriction(&[":public/"]);

    // Query for :secret/ datoms must return empty
    let result = restricted.query(&Query::find_by_attribute(":secret/key")).unwrap();
    assert_eq!(result.datom_count(), 0,
        "INV-FERR-044: namespace isolation must prevent cross-namespace access");
}
```

---

## Running Federation Tests

```bash
CARGO_TARGET_DIR=/data/cargo-target cargo test -p ferratomic-verify
CARGO_TARGET_DIR=/data/cargo-target cargo clippy --workspace -- -D warnings
```

---

## Filing Findings

When a federation test reveals a defect, file it immediately:

```bash
br create \
  --title "BUG: selective merge leaks non-filtered datoms" \
  --type bug --priority 1 --label "phase-4c" \
  --description "$(cat <<'BODY'
**Observed**: Selective merge with filter :role/ includes :name/ datoms from source.
**Expected**: Only :role/ datoms transferred (INV-FERR-039).
**Reproducer**: test_selective_merge_properties assertion 2.
**Affected INV**: INV-FERR-039
BODY
)"
```

For complex multi-invariant failures, decompose first with [12-deep-analysis.md](12-deep-analysis.md).
After fixing, run [06-cleanroom-review.md](06-cleanroom-review.md) on the federation module.

---

## Test Organization

```
ferratomic-verify/integration/
  federation.rs           # Multi-node merge, convergence, anti-entropy
  federated_query.rs      # Fan-out, selective merge, transport transparency
  partition.rs            # Partition tolerance, partial results, recovery
ferratomic-verify/proptest/
  federation.rs           # Property-based: idempotency, monotonicity, no invention
```

Each test file name maps to a spec section. Each test function name includes
the INV-FERR it verifies.
