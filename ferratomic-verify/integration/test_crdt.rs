//! Concrete CRDT integration tests.
//!
//! INV-FERR-001..004, INV-FERR-009/C4, INV-FERR-010, INV-FERR-012,
//! INV-FERR-017 (shard equivalence), INV-FERR-022 (anti-entropy),
//! INV-FERR-030 (replica filter), INV-FERR-031 (genesis determinism).
//! Phase 4a: all tests passing against ferratomic-core implementation.

use ferratom::{AgentId, Attribute, Datom, EntityId, Op, TxId, Value};
use ferratomic_core::merge::merge;
use ferratomic_core::store::Store;
use std::collections::BTreeSet;

/// INV-FERR-001: Concrete merge commutativity with known stores.
#[test]
fn inv_ferr_001_merge_commutes_concrete() {
    let a = Store::from_datoms(BTreeSet::from([
        Datom::new(
            EntityId::from_content(b"e1"),
            Attribute::from("user/name"),
            Value::String("Alice".into()),
            TxId::new(1, 0, 0),
            Op::Assert,
        ),
        Datom::new(
            EntityId::from_content(b"e2"),
            Attribute::from("user/age"),
            Value::Long(30),
            TxId::new(2, 0, 0),
            Op::Assert,
        ),
    ]));

    let b = Store::from_datoms(BTreeSet::from([
        Datom::new(
            EntityId::from_content(b"e2"),
            Attribute::from("user/age"),
            Value::Long(30),
            TxId::new(2, 0, 0),
            Op::Assert,
        ), // overlap with a
        Datom::new(
            EntityId::from_content(b"e3"),
            Attribute::from("user/role"),
            Value::String("admin".into()),
            TxId::new(3, 0, 0),
            Op::Assert,
        ),
    ]));

    let ab = merge(&a, &b).expect("INV-FERR-001: merge(A,B) must succeed");
    let ba = merge(&b, &a).expect("INV-FERR-001: merge(B,A) must succeed");

    assert_eq!(
        ab.datom_set(),
        ba.datom_set(),
        "INV-FERR-001: merge commutativity violated on concrete stores"
    );
    assert_eq!(ab.len(), 3, "Union of 2+2 with 1 overlap = 3 datoms");
}

/// INV-FERR-002: Concrete merge associativity with three stores.
#[test]
fn inv_ferr_002_merge_associates_concrete() {
    let a = Store::from_datoms(BTreeSet::from([Datom::new(
        EntityId::from_content(b"e1"),
        Attribute::from("user/name"),
        Value::String("Alice".into()),
        TxId::new(1, 0, 0),
        Op::Assert,
    )]));

    let b = Store::from_datoms(BTreeSet::from([Datom::new(
        EntityId::from_content(b"e2"),
        Attribute::from("user/name"),
        Value::String("Bob".into()),
        TxId::new(2, 0, 0),
        Op::Assert,
    )]));

    let c = Store::from_datoms(BTreeSet::from([Datom::new(
        EntityId::from_content(b"e3"),
        Attribute::from("user/name"),
        Value::String("Carol".into()),
        TxId::new(3, 0, 0),
        Op::Assert,
    )]));

    let ab_c = merge(
        &merge(&a, &b).expect("INV-FERR-002: merge(A,B) must succeed"),
        &c,
    ).expect("INV-FERR-002: merge(AB,C) must succeed");
    let a_bc = merge(
        &a,
        &merge(&b, &c).expect("INV-FERR-002: merge(B,C) must succeed"),
    ).expect("INV-FERR-002: merge(A,BC) must succeed");

    assert_eq!(
        ab_c.datom_set(),
        a_bc.datom_set(),
        "INV-FERR-002: merge associativity violated"
    );
    assert_eq!(ab_c.len(), 3, "Three disjoint stores merge to 3 datoms");
}

/// INV-FERR-003: Concrete merge idempotency.
#[test]
fn inv_ferr_003_merge_idempotent_concrete() {
    let store = Store::from_datoms(BTreeSet::from([
        Datom::new(
            EntityId::from_content(b"e1"),
            Attribute::from("user/name"),
            Value::String("Alice".into()),
            TxId::new(1, 0, 0),
            Op::Assert,
        ),
        Datom::new(
            EntityId::from_content(b"e1"),
            Attribute::from("user/name"),
            Value::String("Alice".into()),
            TxId::new(1, 0, 0),
            Op::Retract,
        ),
    ]));

    let merged = merge(&store, &store).expect("INV-FERR-003: self-merge must succeed");

    assert_eq!(
        store.datom_set(),
        merged.datom_set(),
        "INV-FERR-003: merge idempotency violated"
    );
    assert_eq!(
        store.len(),
        merged.len(),
        "INV-FERR-003: cardinality changed"
    );
}

/// INV-FERR-004: Transact strictly grows the store.
#[test]
fn inv_ferr_004_transact_grows_store() {
    let mut store = Store::genesis();
    let pre_len = store.len();

    // Use genesis-schema attribute (db/doc accepts String) instead of
    // user/name which is not in genesis schema.
    let tx = ferratomic_core::writer::Transaction::new(AgentId::from_bytes([0u8; 16]))
        .assert_datom(
            EntityId::from_content(b"new-entity"),
            Attribute::from("db/doc"),
            Value::String("Test".into()),
        );

    let committed = tx
        .commit(store.schema())
        .expect("INV-FERR-004: valid tx rejected");
    let result = store.transact(committed);
    assert!(
        result.is_ok(),
        "INV-FERR-004: transact failed unexpectedly"
    );
    assert!(
        store.len() > pre_len,
        "INV-FERR-004: store did not grow. pre={}, post={}",
        pre_len,
        store.len()
    );
}

/// INV-FERR-010: Three replicas converge when they receive the same datoms.
#[test]
fn inv_ferr_010_convergence_three_replicas() {
    let datoms = vec![
        Datom::new(
            EntityId::from_content(b"e1"),
            Attribute::from("user/name"),
            Value::String("Alice".into()),
            TxId::new(1, 0, 0),
            Op::Assert,
        ),
        Datom::new(
            EntityId::from_content(b"e2"),
            Attribute::from("user/name"),
            Value::String("Bob".into()),
            TxId::new(2, 0, 0),
            Op::Assert,
        ),
        Datom::new(
            EntityId::from_content(b"e3"),
            Attribute::from("user/name"),
            Value::String("Carol".into()),
            TxId::new(3, 0, 0),
            Op::Assert,
        ),
    ];

    // Replica 1: forward order
    let mut r1 = Store::genesis();
    for d in datoms.iter() {
        r1.insert(d);
    }

    // Replica 2: reverse order
    let mut r2 = Store::genesis();
    for d in datoms.iter().rev() {
        r2.insert(d);
    }

    // Replica 3: merge topology
    let mut r3a = Store::genesis();
    r3a.insert(&datoms[0]);
    r3a.insert(&datoms[2]);
    let mut r3b = Store::genesis();
    r3b.insert(&datoms[1]);
    let r3 = merge(&r3a, &r3b).expect("INV-FERR-010: merge must succeed");

    assert_eq!(
        r1.datom_set(),
        r2.datom_set(),
        "INV-FERR-010: replicas 1 and 2 diverged"
    );
    assert_eq!(
        r1.datom_set(),
        r3.datom_set(),
        "INV-FERR-010: replicas 1 and 3 diverged"
    );
}

/// INV-FERR-009 / C4: Merge accepts datoms with unknown attributes.
/// Merge is pure set union — no schema validation at merge time.
#[test]
fn inv_ferr_009_merge_exempt_from_schema() {
    let genesis = Store::genesis();

    // Create a store with a datom whose attribute is NOT in genesis schema
    let foreign_datom = Datom::new(
        EntityId::from_content(b"foreign-entity"),
        Attribute::from("unknown/foreign_attr"),
        Value::String("foreign value".into()),
        TxId::new(999, 0, 1),
        Op::Assert,
    );
    let foreign_store = Store::from_datoms(BTreeSet::from([foreign_datom.clone()]));

    // Merge must preserve the foreign datom — no schema filtering
    let merged = merge(&genesis, &foreign_store)
        .expect("INV-FERR-009: merge with unknown attrs must succeed");
    assert!(
        merged.datom_set().contains(&foreign_datom),
        "INV-FERR-009/C4: merge dropped datom with unknown attribute. \
         Merge must be pure set union."
    );
}

/// INV-FERR-017: Shard partition + union = original store.
///
/// Partition a concrete store into N shards by entity hash. Verify the union
/// of all shards equals the original store.
#[test]
fn inv_ferr_017_shard_equivalence_concrete() {
    let datoms = BTreeSet::from([
        Datom::new(
            EntityId::from_content(b"shard-e1"),
            Attribute::from("user/name"),
            Value::String("Alice".into()),
            TxId::new(1, 0, 0),
            Op::Assert,
        ),
        Datom::new(
            EntityId::from_content(b"shard-e2"),
            Attribute::from("user/name"),
            Value::String("Bob".into()),
            TxId::new(2, 0, 0),
            Op::Assert,
        ),
        Datom::new(
            EntityId::from_content(b"shard-e3"),
            Attribute::from("user/name"),
            Value::String("Carol".into()),
            TxId::new(3, 0, 0),
            Op::Assert,
        ),
        Datom::new(
            EntityId::from_content(b"shard-e1"),
            Attribute::from("user/age"),
            Value::Long(30),
            TxId::new(4, 0, 0),
            Op::Assert,
        ),
    ]);
    let store = Store::from_datoms(datoms);
    let shard_count = 3usize;

    // Partition by entity.
    let mut shards: Vec<BTreeSet<&Datom>> = (0..shard_count).map(|_| BTreeSet::new()).collect();
    for d in store.datoms() {
        let entity = d.entity();
        let bytes = entity.as_bytes();
        let mut buf = [0u8; 8];
        let len = bytes.len().min(8);
        buf[..len].copy_from_slice(&bytes[..len]);
        let shard_id = (u64::from_le_bytes(buf) as usize) % shard_count;
        shards[shard_id].insert(d);
    }

    // Union of shards = original.
    let union: BTreeSet<&Datom> = shards.iter().flat_map(|s| s.iter().copied()).collect();
    let primary: BTreeSet<&Datom> = store.datoms().collect();
    assert_eq!(
        union, primary,
        "INV-FERR-017: shard union != original store"
    );

    // Total cardinality preserved.
    let total: usize = shards.iter().map(|s| s.len()).sum();
    assert_eq!(
        total,
        store.len(),
        "INV-FERR-017: shard cardinality mismatch"
    );

    // Shards are disjoint.
    for i in 0..shard_count {
        for j in (i + 1)..shard_count {
            let overlap: Vec<_> = shards[i].intersection(&shards[j]).collect();
            assert!(
                overlap.is_empty(),
                "INV-FERR-017: shards {} and {} overlap by {} datoms",
                i, j, overlap.len()
            );
        }
    }
}

/// INV-FERR-012: Content-addressed identity — same content, same EntityId.
#[test]
fn inv_ferr_012_same_content_same_id() {
    let content = b"hello world";
    let id1 = EntityId::from_content(content);
    let id2 = EntityId::from_content(content);
    assert_eq!(
        id1, id2,
        "INV-FERR-012: same content produced different EntityIds"
    );

    let id3 = EntityId::from_content(b"different content");
    assert_ne!(
        id1, id3,
        "INV-FERR-012: different content produced same EntityId"
    );
}

/// INV-FERR-004: Transact 3x via Database, assert store size only grows.
///
/// bd-7tb0: integration test at the Database level (not Store). Uses the
/// full Database::genesis() -> db.transact() -> db.snapshot() path.
#[test]
fn test_inv_ferr_004_monotonic_growth_database() {
    use ferratomic_core::db::Database;
    use ferratomic_core::writer::Transaction;

    let db = Database::genesis();
    let agent = AgentId::from_bytes([4u8; 16]);

    let mut prev_count = db.snapshot().datoms().count();

    for i in 0..3i64 {
        let tx = Transaction::new(agent)
            .assert_datom(
                EntityId::from_content(format!("growth-e{i}").as_bytes()),
                Attribute::from("db/doc"),
                Value::String(format!("growth-{i}").into()),
            )
            .commit(&db.schema())
            .expect("INV-FERR-004: valid tx must commit");
        db.transact(tx)
            .expect("INV-FERR-004: transact must succeed");

        let current_count = db.snapshot().datoms().count();
        assert!(
            current_count > prev_count,
            "INV-FERR-004: store must strictly grow after transact. \
             iteration={i}, prev={prev_count}, current={current_count}"
        );
        prev_count = current_count;
    }

    // Final size must be strictly greater than genesis.
    assert!(
        prev_count > 0,
        "INV-FERR-004: database must contain datoms after 3 transactions"
    );
}

/// INV-FERR-022: NullAntiEntropy compiles and diff returns empty.
///
/// bd-7tb0: verifies the anti-entropy trait boundary works at the
/// integration level. NullAntiEntropy is the no-op default for
/// single-node operation.
#[test]
fn test_inv_ferr_022_anti_entropy_trait() {
    use ferratomic_core::anti_entropy::{AntiEntropy, NullAntiEntropy};

    let ae = NullAntiEntropy;
    let mut store = ferratomic_core::store::Store::genesis();

    // diff must return Ok with empty vec.
    let diff = ae
        .diff(&store)
        .expect("INV-FERR-022: NullAntiEntropy::diff must succeed");
    assert!(
        diff.is_empty(),
        "INV-FERR-022: NullAntiEntropy diff must return empty vec, got {} bytes",
        diff.len()
    );

    // apply_diff must succeed and leave store unchanged.
    let epoch_before = store.epoch();
    let len_before = store.len();
    ae.apply_diff(&mut store, &diff)
        .expect("INV-FERR-022: NullAntiEntropy::apply_diff must succeed");
    assert_eq!(
        store.epoch(),
        epoch_before,
        "INV-FERR-022: apply_diff must not change epoch"
    );
    assert_eq!(
        store.len(),
        len_before,
        "INV-FERR-022: apply_diff must not change store size"
    );

    // apply_diff with non-empty bytes must also be a no-op.
    ae.apply_diff(&mut store, &[0xCA, 0xFE, 0xBA, 0xBE])
        .expect("INV-FERR-022: apply_diff with arbitrary bytes must succeed");
    assert_eq!(
        store.epoch(),
        epoch_before,
        "INV-FERR-022: apply_diff with arbitrary bytes must not change epoch"
    );
}

/// INV-FERR-030: AcceptAll replica filter passes all datoms.
///
/// bd-7tb0: verifies the ReplicaFilter trait boundary at integration level.
/// AcceptAll is the default full-replica behavior.
#[test]
fn test_inv_ferr_030_replica_filter() {
    use ferratomic_core::topology::{AcceptAll, ReplicaFilter};

    let filter = AcceptAll;

    // Build several diverse datoms and verify all are accepted.
    let datoms = vec![
        Datom::new(
            EntityId::from_content(b"filter-e1"),
            Attribute::from("user/name"),
            Value::String("Alice".into()),
            TxId::new(1, 0, 0),
            Op::Assert,
        ),
        Datom::new(
            EntityId::from_content(b"filter-e2"),
            Attribute::from("user/age"),
            Value::Long(30),
            TxId::new(2, 0, 0),
            Op::Assert,
        ),
        Datom::new(
            EntityId::from_content(b"filter-e3"),
            Attribute::from("user/name"),
            Value::String("Bob".into()),
            TxId::new(3, 0, 0),
            Op::Retract,
        ),
    ];

    for (i, datom) in datoms.iter().enumerate() {
        assert!(
            filter.accepts(datom),
            "INV-FERR-030: AcceptAll must accept datom {i}, got false"
        );
    }

    // Verify AcceptAll is Send + Sync (required by trait bound).
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<AcceptAll>();
}

/// INV-FERR-031: Two genesis databases produce identical datom sets.
///
/// bd-7tb0: integration-level genesis determinism test. Creates two
/// Database instances via genesis(), verifies identical schemas, epochs,
/// and datom sets.
#[test]
fn test_inv_ferr_031_genesis_determinism() {
    use ferratomic_core::db::Database;

    let db1 = Database::genesis();
    let db2 = Database::genesis();

    // Epochs must be identical (both 0).
    assert_eq!(
        db1.epoch(),
        db2.epoch(),
        "INV-FERR-031: genesis databases must have identical epochs"
    );

    // Schemas must be identical.
    assert_eq!(
        db1.schema(),
        db2.schema(),
        "INV-FERR-031: genesis databases must have identical schemas"
    );

    // Datom sets must be identical.
    let snap1 = db1.snapshot();
    let snap2 = db2.snapshot();
    let datoms1: BTreeSet<_> = snap1.datoms().cloned().collect();
    let datoms2: BTreeSet<_> = snap2.datoms().cloned().collect();
    assert_eq!(
        datoms1, datoms2,
        "INV-FERR-031: genesis databases must have identical datom sets"
    );
}
