//! Concrete CRDT integration tests.
//!
//! INV-FERR-001..004, INV-FERR-010, INV-FERR-012.
//! ALL TESTS MUST FAIL (red phase). Types are not yet implemented.

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

    let ab = merge(&a, &b);
    let ba = merge(&b, &a);

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

    let ab_c = merge(&merge(&a, &b), &c);
    let a_bc = merge(&a, &merge(&b, &c));

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

    let merged = merge(&store, &store);

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
        r1.insert(d.clone());
    }

    // Replica 2: reverse order
    let mut r2 = Store::genesis();
    for d in datoms.iter().rev() {
        r2.insert(d.clone());
    }

    // Replica 3: merge topology
    let mut r3a = Store::genesis();
    r3a.insert(datoms[0].clone());
    r3a.insert(datoms[2].clone());
    let mut r3b = Store::genesis();
    r3b.insert(datoms[1].clone());
    let r3 = merge(&r3a, &r3b);

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
    let merged = merge(&genesis, &foreign_store);
    assert!(
        merged.datom_set().contains(&foreign_datom),
        "INV-FERR-009/C4: merge dropped datom with unknown attribute. \
         Merge must be pure set union."
    );
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
