//! Concrete CRDT integration tests.
//!
//! INV-FERR-001..004, INV-FERR-009/C4, INV-FERR-010, INV-FERR-012,
//! INV-FERR-017 (shard equivalence), INV-FERR-022 (anti-entropy),
//! INV-FERR-030 (replica filter), INV-FERR-031 (genesis determinism).
//! Phase 4a: all tests passing against ferratomic-db implementation.

use std::collections::BTreeSet;

use ferratom::{Attribute, Datom, EntityId, NodeId, Op, TxId, Value};
use ferratomic_db::{merge::merge, store::Store};

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
    )
    .expect("INV-FERR-002: merge(AB,C) must succeed");
    let a_bc = merge(
        &a,
        &merge(&b, &c).expect("INV-FERR-002: merge(B,C) must succeed"),
    )
    .expect("INV-FERR-002: merge(A,BC) must succeed");

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
    let tx = ferratomic_db::writer::Transaction::new(NodeId::from_bytes([0u8; 16])).assert_datom(
        EntityId::from_content(b"new-entity"),
        Attribute::from("db/doc"),
        Value::String("Test".into()),
    );

    let committed = tx
        .commit(store.schema())
        .expect("INV-FERR-004: valid tx rejected");
    let result = store.transact_test(committed);
    assert!(result.is_ok(), "INV-FERR-004: transact failed unexpectedly");
    assert!(
        store.len() > pre_len,
        "INV-FERR-004: store did not grow. pre={}, post={}",
        pre_len,
        store.len()
    );
}

/// Build three test datoms for convergence tests (Alice, Bob, Carol).
fn build_convergence_datoms() -> [Datom; 3] {
    [
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
    ]
}

/// INV-FERR-010: Three replicas converge when they receive the same datoms.
#[test]
fn inv_ferr_010_convergence_three_replicas() {
    let datoms = build_convergence_datoms();

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
    let r3 = build_merged_replica(&datoms[0], &datoms[2], &datoms[1]);

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

/// Build a replica by inserting two datoms into one store, one into another,
/// then merging them.
fn build_merged_replica(first: &Datom, second: &Datom, third: &Datom) -> Store {
    let mut part_a = Store::genesis();
    part_a.insert(first);
    part_a.insert(second);
    let mut part_b = Store::genesis();
    part_b.insert(third);
    merge(&part_a, &part_b).expect("INV-FERR-010: merge must succeed")
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

/// Partition a store's datoms into N shards by entity hash.
fn partition_into_shards(store: &Store, shard_count: usize) -> Vec<BTreeSet<&Datom>> {
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
    shards
}

/// Assert that all shards are pairwise disjoint.
fn assert_shards_disjoint(shards: &[BTreeSet<&Datom>]) {
    for i in 0..shards.len() {
        for j in (i + 1)..shards.len() {
            let overlap: Vec<_> = shards[i].intersection(&shards[j]).collect();
            assert!(
                overlap.is_empty(),
                "INV-FERR-017: shards {} and {} overlap by {} datoms",
                i,
                j,
                overlap.len()
            );
        }
    }
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
    let shards = partition_into_shards(&store, 3);

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

    assert_shards_disjoint(&shards);
}

/// INV-FERR-012: Content-addressed identity -- same content, same `EntityId`.
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
    use ferratomic_db::{db::Database, writer::Transaction};

    let db = Database::genesis();
    let node = NodeId::from_bytes([4u8; 16]);

    let mut prev_count = db.snapshot().datoms().count();

    for i in 0..3i64 {
        let tx = Transaction::new(node)
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

/// INV-FERR-022: `NullAntiEntropy` compiles and diff returns empty.
///
/// bd-7tb0: verifies the anti-entropy trait boundary works at the
/// integration level. `NullAntiEntropy` is the no-op default for
/// single-node operation.
#[test]
fn test_inv_ferr_022_anti_entropy_trait() {
    use ferratomic_db::anti_entropy::{AntiEntropy, NullAntiEntropy};

    let ae = NullAntiEntropy;
    let mut store = ferratomic_db::store::Store::genesis();

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

/// INV-FERR-030: `AcceptAll` replica filter passes all datoms.
///
/// bd-7tb0: verifies the ReplicaFilter trait boundary at integration level.
/// AcceptAll is the default full-replica behavior.
#[test]
fn test_inv_ferr_030_replica_filter() {
    use ferratomic_db::topology::{AcceptAll, ReplicaFilter};

    let filter = AcceptAll;

    // Build several diverse datoms and verify all are accepted.
    let datoms = [
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
            "INV-FERR-030: AcceptAll must accept datom {}, got false",
            i
        );
    }

    // Verify AcceptAll is Send + Sync (required by trait bound).
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<AcceptAll>();
}

/// Build the convergence test datoms spanning 3 entities with mixed ops.
fn build_convergence_index_datoms() -> [Datom; 6] {
    [
        Datom::new(
            EntityId::from_content(b"conv-e1"),
            Attribute::from("user/name"),
            Value::String("Alice".into()),
            TxId::new(1, 0, 0),
            Op::Assert,
        ),
        Datom::new(
            EntityId::from_content(b"conv-e1"),
            Attribute::from("user/age"),
            Value::Long(30),
            TxId::new(2, 0, 0),
            Op::Assert,
        ),
        Datom::new(
            EntityId::from_content(b"conv-e2"),
            Attribute::from("user/name"),
            Value::String("Bob".into()),
            TxId::new(3, 0, 0),
            Op::Assert,
        ),
        Datom::new(
            EntityId::from_content(b"conv-e2"),
            Attribute::from("user/role"),
            Value::String("admin".into()),
            TxId::new(4, 0, 0),
            Op::Assert,
        ),
        Datom::new(
            EntityId::from_content(b"conv-e3"),
            Attribute::from("user/name"),
            Value::String("Carol".into()),
            TxId::new(5, 0, 0),
            Op::Assert,
        ),
        Datom::new(
            EntityId::from_content(b"conv-e3"),
            Attribute::from("user/name"),
            Value::String("Carol".into()),
            TxId::new(6, 0, 0),
            Op::Retract,
        ),
    ]
}

/// Build forward-order and reverse-order stores from the given datoms.
///
/// bd-5zc4: `SortedVecBackend` defers sorting after `insert()`.
/// `ensure_indexes_sorted()` is called after all inserts to restore
/// sorted order before convergence assertions.
fn build_forward_and_reverse_stores(datoms: &[Datom]) -> (Store, Store) {
    let mut forward = Store::genesis();
    for d in datoms {
        forward.insert(d);
    }
    // bd-5zc4: SortedVecBackend defers sorting after insert().
    // Must sort before querying indexes for convergence checks.
    forward.ensure_indexes_sorted();
    let mut reverse = Store::genesis();
    for d in datoms.iter().rev() {
        reverse.insert(d);
    }
    reverse.ensure_indexes_sorted();
    (forward, reverse)
}

/// Assert that EAVT index iteration matches field-by-field between two stores.
fn assert_eavt_field_convergence(forward: &Store, reverse: &Store) {
    let eavt_f: Vec<&Datom> = forward.indexes().unwrap().eavt_datoms().collect();
    let eavt_r: Vec<&Datom> = reverse.indexes().unwrap().eavt_datoms().collect();
    assert_eq!(
        eavt_f.len(),
        eavt_r.len(),
        "INV-FERR-010: EAVT index cardinality diverged"
    );
    for (i, (f, r)) in eavt_f.iter().zip(eavt_r.iter()).enumerate() {
        assert_eq!(
            f.entity(),
            r.entity(),
            "INV-FERR-010: EAVT entity diverged at position {}",
            i
        );
        assert_eq!(
            f.attribute(),
            r.attribute(),
            "INV-FERR-010: EAVT attribute diverged at position {}",
            i
        );
        assert_eq!(
            f.value(),
            r.value(),
            "INV-FERR-010: EAVT value diverged at position {}",
            i
        );
        assert_eq!(
            f.tx(),
            r.tx(),
            "INV-FERR-010: EAVT tx diverged at position {}",
            i
        );
    }
}

/// Assert that a secondary index (AEVT, AVET, or VAET) matches between two stores.
fn assert_secondary_index_convergence(
    forward_datoms: Vec<&Datom>,
    reverse_datoms: Vec<&Datom>,
    index_name: &str,
) {
    assert_eq!(
        forward_datoms.len(),
        reverse_datoms.len(),
        "INV-FERR-010: {} index cardinality diverged",
        index_name
    );
    for (i, (f, r)) in forward_datoms.iter().zip(reverse_datoms.iter()).enumerate() {
        assert_eq!(
            f, r,
            "INV-FERR-010: {} index diverged at position {}",
            index_name, i
        );
    }
}

/// INV-FERR-010: Convergence verified at all four index levels.
///
/// bd-7fub.23.1: Two stores with identical datoms applied in different
/// insertion order must produce identical EAVT, AEVT, AVET, and VAET
/// index iteration sequences. Field-by-field datom comparison.
#[test]
fn test_inv_ferr_010_convergence_index_level() {
    let datoms = build_convergence_index_datoms();
    let (forward, reverse) = build_forward_and_reverse_stores(&datoms);

    assert_eq!(
        forward.datom_set(),
        reverse.datom_set(),
        "INV-FERR-010: primary datom sets must be identical regardless of insertion order"
    );

    assert_eavt_field_convergence(&forward, &reverse);

    assert_secondary_index_convergence(
        forward.indexes().unwrap().aevt_datoms().collect(),
        reverse.indexes().unwrap().aevt_datoms().collect(),
        "AEVT",
    );
    assert_secondary_index_convergence(
        forward.indexes().unwrap().avet_datoms().collect(),
        reverse.indexes().unwrap().avet_datoms().collect(),
        "AVET",
    );
    assert_secondary_index_convergence(
        forward.indexes().unwrap().vaet_datoms().collect(),
        reverse.indexes().unwrap().vaet_datoms().collect(),
        "VAET",
    );
}

/// Build the shard-merge test datoms across 4 entities.
fn build_shard_merge_datoms() -> BTreeSet<Datom> {
    BTreeSet::from([
        Datom::new(
            EntityId::from_content(b"shard-m1"),
            Attribute::from("user/name"),
            Value::String("Alice".into()),
            TxId::new(1, 0, 0),
            Op::Assert,
        ),
        Datom::new(
            EntityId::from_content(b"shard-m1"),
            Attribute::from("user/age"),
            Value::Long(28),
            TxId::new(2, 0, 0),
            Op::Assert,
        ),
        Datom::new(
            EntityId::from_content(b"shard-m2"),
            Attribute::from("user/name"),
            Value::String("Bob".into()),
            TxId::new(3, 0, 0),
            Op::Assert,
        ),
        Datom::new(
            EntityId::from_content(b"shard-m3"),
            Attribute::from("user/role"),
            Value::String("admin".into()),
            TxId::new(4, 0, 0),
            Op::Assert,
        ),
        Datom::new(
            EntityId::from_content(b"shard-m4"),
            Attribute::from("user/name"),
            Value::String("Dave".into()),
            TxId::new(5, 0, 0),
            Op::Assert,
        ),
    ])
}

/// Partition a store into shard stores by entity hash, then merge all shards.
fn partition_and_merge_shards(original: &Store, shard_count: usize) -> Store {
    let mut shard_datoms: Vec<BTreeSet<Datom>> =
        (0..shard_count).map(|_| BTreeSet::new()).collect();
    for d in original.datoms() {
        let entity = d.entity();
        let bytes = entity.as_bytes();
        let mut buf = [0u8; 8];
        let len = bytes.len().min(8);
        buf[..len].copy_from_slice(&bytes[..len]);
        let shard_id = (u64::from_le_bytes(buf) as usize) % shard_count;
        shard_datoms[shard_id].insert(d.clone());
    }
    let shard_stores: Vec<Store> = shard_datoms.into_iter().map(Store::from_datoms).collect();
    let mut merged = shard_stores[0].clone();
    for shard in &shard_stores[1..] {
        merged = merge(&merged, shard).expect("INV-FERR-017: shard merge must succeed");
    }
    merged
}

/// Assert that all datoms from the original store are present in the merged store.
fn assert_all_datoms_present(original: &Store, merged: &Store) {
    for d in original.datoms() {
        assert!(
            merged.datom_set().contains(d),
            "INV-FERR-017: datom missing from merged result: entity={:?}, attr={}, val={:?}",
            d.entity(),
            d.attribute().as_str(),
            d.value()
        );
    }
}

/// INV-FERR-017: Shard partition + merge = original store.
///
/// bd-7fub.23.2: Partition a store into shards by entity hash, create
/// Store objects from each shard, merge all shards via Store::from_merge,
/// verify the merged result equals the original at the datom level.
#[test]
fn test_inv_ferr_017_shard_union_via_merge() {
    let original = Store::from_datoms(build_shard_merge_datoms());
    let merged = partition_and_merge_shards(&original, 3);

    assert_eq!(
        merged.datom_set(),
        original.datom_set(),
        "INV-FERR-017: merged shards must equal original store at datom level"
    );
    assert_eq!(
        merged.len(),
        original.len(),
        "INV-FERR-017: merged shard cardinality must match original"
    );
    assert_all_datoms_present(&original, &merged);
}

/// INV-FERR-031: Two genesis databases produce identical datom sets.
///
/// bd-7tb0: integration-level genesis determinism test. Creates two
/// Database instances via genesis(), verifies identical schemas, epochs,
/// and datom sets.
#[test]
fn test_inv_ferr_031_genesis_determinism() {
    use ferratomic_db::db::Database;

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
