use std::{collections::BTreeSet, sync::Arc};

use ferratom::{Attribute, Cardinality, Datom, EntityId, Op, TxId, Value, ValueType};

use super::*;
use crate::schema_evolution::{parse_cardinality, parse_value_type};

/// Helper: build a sample datom for testing.
fn sample_datom(seed: &str) -> Datom {
    Datom::new(
        EntityId::from_content(seed.as_bytes()),
        Attribute::from("test/name"),
        Value::String(Arc::from(seed)),
        TxId::new(1, 0, 0),
        Op::Assert,
    )
}

#[test]
fn test_from_datoms_preserves_set() {
    let mut set = BTreeSet::new();
    set.insert(sample_datom("a"));
    set.insert(sample_datom("b"));

    let store = Store::from_datoms(set.clone());
    let stored: BTreeSet<&Datom> = store.datom_set().iter().collect();
    let expected: BTreeSet<&Datom> = set.iter().collect();
    assert_eq!(stored, expected);
    assert_eq!(store.len(), 2);
}

#[test]
fn test_from_datoms_empty() {
    let store = Store::from_datoms(BTreeSet::new());
    assert!(store.is_empty());
    assert_eq!(store.len(), 0);
}

#[test]
fn test_inv_ferr_031_genesis_determinism() {
    let a = Store::genesis();
    let b = Store::genesis();
    assert_eq!(
        a.schema(),
        b.schema(),
        "INV-FERR-031: genesis() must produce identical schemas"
    );
    assert!(
        a.datom_set() == b.datom_set(),
        "INV-FERR-031: genesis() must produce identical datom sets"
    );
    assert_eq!(a.epoch(), b.epoch());
}

/// The 19 axiomatic attribute idents expected in genesis schema (INV-FERR-031).
const GENESIS_ATTRIBUTE_IDENTS: [&str; 19] = [
    "db/ident",
    "db/valueType",
    "db/cardinality",
    "db/doc",
    "db/unique",
    "db/isComponent",
    "db/resolutionMode",
    "db/latticeOrder",
    "db/lwwClock",
    "lattice/ident",
    "lattice/elements",
    "lattice/comparator",
    "lattice/bottom",
    "lattice/top",
    "tx/time",
    "tx/agent",
    "tx/provenance",
    "tx/rationale",
    "tx/coherence-override",
];

#[test]
fn test_inv_ferr_031_genesis_schema_has_19_attributes() {
    let store = Store::genesis();
    assert_eq!(
        store.schema().len(),
        19,
        "INV-FERR-031: genesis schema must have exactly 19 axiomatic attributes"
    );
    for ident in &GENESIS_ATTRIBUTE_IDENTS {
        assert!(
            store.schema().get(&Attribute::from(*ident)).is_some(),
            "INV-FERR-031: genesis schema missing expected attribute: {ident}"
        );
    }
}

#[test]
fn test_inv_ferr_005_index_bijection_from_datoms() {
    let mut set = BTreeSet::new();
    set.insert(sample_datom("x"));
    set.insert(sample_datom("y"));
    set.insert(sample_datom("z"));

    // bd-h2fz: from_datoms builds Positional repr (no OrdMap indexes).
    // Promote to OrdMap to verify index bijection via Indexes API.
    let mut store = Store::from_datoms(set);
    store.promote();
    let primary: BTreeSet<&Datom> = store.datoms().collect();
    let indexes = store.indexes().unwrap();
    let eavt: BTreeSet<&Datom> = indexes.eavt_datoms().collect();
    let aevt: BTreeSet<&Datom> = indexes.aevt_datoms().collect();
    let vaet: BTreeSet<&Datom> = indexes.vaet_datoms().collect();
    let avet: BTreeSet<&Datom> = indexes.avet_datoms().collect();

    assert_eq!(primary, eavt, "INV-FERR-005: EAVT must match primary");
    assert_eq!(primary, aevt, "INV-FERR-005: AEVT must match primary");
    assert_eq!(primary, vaet, "INV-FERR-005: VAET must match primary");
    assert_eq!(primary, avet, "INV-FERR-005: AVET must match primary");
}

#[test]
fn test_genesis_is_empty_of_datoms() {
    let store = Store::genesis();
    assert!(store.is_empty(), "genesis store must have zero datoms");
}

#[test]
fn test_snapshot_is_frozen() {
    let mut store = Store::from_datoms(BTreeSet::new());
    store.insert(&sample_datom("before"));

    let snap = store.snapshot();
    let snap_set_before: BTreeSet<&Datom> = snap.datoms().collect();

    store.insert(&sample_datom("after"));

    let snap_set_after: BTreeSet<&Datom> = snap.datoms().collect();
    assert_eq!(
        snap_set_before, snap_set_after,
        "INV-FERR-006: snapshot datom set must not change after later inserts"
    );
    assert_eq!(
        snap_set_before.len(),
        1,
        "snapshot should have exactly 1 datom"
    );
}

#[test]
fn test_parse_value_type_all_variants() {
    assert_eq!(
        parse_value_type("db.type/keyword"),
        Some(ValueType::Keyword)
    );
    assert_eq!(parse_value_type("db.type/string"), Some(ValueType::String));
    assert_eq!(parse_value_type("db.type/long"), Some(ValueType::Long));
    assert_eq!(parse_value_type("db.type/double"), Some(ValueType::Double));
    assert_eq!(
        parse_value_type("db.type/boolean"),
        Some(ValueType::Boolean)
    );
    assert_eq!(
        parse_value_type("db.type/instant"),
        Some(ValueType::Instant)
    );
    assert_eq!(parse_value_type("db.type/uuid"), Some(ValueType::Uuid));
    assert_eq!(parse_value_type("db.type/bytes"), Some(ValueType::Bytes));
    assert_eq!(parse_value_type("db.type/ref"), Some(ValueType::Ref));
    assert_eq!(parse_value_type("db.type/bigint"), Some(ValueType::BigInt));
    assert_eq!(parse_value_type("db.type/bigdec"), Some(ValueType::BigDec));
    assert_eq!(parse_value_type("db.type/unknown"), None);
}

#[test]
fn test_parse_cardinality_variants() {
    assert_eq!(
        parse_cardinality("db.cardinality/one"),
        Some(Cardinality::One)
    );
    assert_eq!(
        parse_cardinality("db.cardinality/many"),
        Some(Cardinality::Many)
    );
    assert_eq!(parse_cardinality("db.cardinality/unknown"), None);
}

/// INV-FERR-072: after transact, store is demoted back to Positional.
#[test]
fn test_inv_ferr_072_demote_after_transact() {
    use ferratom::AgentId;

    use crate::writer::Transaction;

    let mut store = Store::genesis();
    let agent = AgentId::from_bytes([1u8; 16]);
    let tx = Transaction::new(agent)
        .assert_datom(
            EntityId::from_content(b"e1"),
            Attribute::from("db/doc"),
            Value::String(Arc::from("test-demote")),
        )
        .commit(store.schema())
        .expect("valid tx");
    store.transact_test(tx).expect("transact ok");

    assert!(
        store.positional().is_some(),
        "INV-FERR-072: store must be Positional after transact (demoted)"
    );
}

/// INV-FERR-072: demotion preserves the datom set exactly.
#[test]
fn test_inv_ferr_072_demote_preserves_datoms() {
    use std::collections::BTreeSet;

    let d1 = sample_datom("alpha");
    let d2 = sample_datom("beta");
    let d3 = sample_datom("gamma");

    let mut store = Store::from_datoms(BTreeSet::new());
    store.insert(&d1);
    store.insert(&d2);
    store.insert(&d3);

    // Store is now OrdMap after inserts. Capture datom set.
    let before: BTreeSet<Datom> = store.datoms().cloned().collect();
    assert_eq!(before.len(), 3, "precondition: 3 datoms inserted");

    // Demote to Positional.
    store.demote();

    assert!(
        store.positional().is_some(),
        "INV-FERR-072: store must be Positional after demote"
    );

    let after: BTreeSet<Datom> = store.datoms().cloned().collect();
    assert_eq!(
        before, after,
        "INV-FERR-072: datom set must be identical after demotion cycle"
    );
}

/// INV-FERR-072: demote is a no-op on an already-Positional store.
#[test]
fn test_inv_ferr_072_demote_noop_on_positional() {
    let store_before = Store::genesis();
    let mut store = store_before.clone();
    store.demote();

    assert!(
        store.positional().is_some(),
        "INV-FERR-072: demote on Positional must remain Positional"
    );
    assert_eq!(
        store.len(),
        0,
        "genesis store remains empty after no-op demote"
    );
}

/// INV-FERR-014: `batch_replay` promotes once, replays all, demotes once.
#[test]
fn test_inv_ferr_014_batch_replay() {
    let d1 = Datom::new(
        EntityId::from_content(b"e1"),
        Attribute::from("test/name"),
        Value::String(Arc::from("one")),
        TxId::new(1, 0, 0),
        Op::Assert,
    );
    let d2 = Datom::new(
        EntityId::from_content(b"e2"),
        Attribute::from("test/name"),
        Value::String(Arc::from("two")),
        TxId::new(2, 0, 0),
        Op::Assert,
    );

    let mut store = Store::genesis();
    let entries = vec![(1_u64, vec![d1.clone()]), (2_u64, vec![d2.clone()])];
    store.batch_replay(&entries).expect("batch_replay ok");

    assert_eq!(
        store.epoch(),
        2,
        "INV-FERR-014: epoch must be 2 after two entries"
    );
    assert_eq!(store.len(), 2, "INV-FERR-014: two datoms replayed");
    assert!(
        store.positional().is_some(),
        "INV-FERR-072: store must be Positional after batch_replay"
    );
    assert!(
        store.datom_set().contains(&d1),
        "INV-FERR-014: first datom present"
    );
    assert!(
        store.datom_set().contains(&d2),
        "INV-FERR-014: second datom present"
    );
}

/// INV-FERR-014: `batch_replay` with empty entries is a no-op.
#[test]
fn test_inv_ferr_014_batch_replay_empty() {
    let mut store = Store::genesis();
    store.batch_replay(&[]).expect("empty batch_replay ok");
    assert_eq!(store.epoch(), 0, "epoch unchanged for empty batch");
    assert!(store.positional().is_some(), "still Positional");
}

/// bd-20j: Semilattice trait is usable via generic bounds.
#[test]
fn test_semilattice_trait_bound() {
    use ferratom::traits::Semilattice;

    fn requires_semilattice<T: Semilattice>(a: &T, b: &T) -> Result<T, ferratom::FerraError> {
        a.merge(b)
    }

    let a = Store::genesis();
    let b = Store::genesis();
    let merged = requires_semilattice(&a, &b).expect("merge should succeed");
    assert_eq!(
        merged.epoch(),
        0,
        "bd-20j: Semilattice merge of genesis stores"
    );
}

/// bd-20j: `ContentAddressed` trait is usable via generic bounds.
#[test]
fn test_content_addressed_trait_bound() {
    use ferratom::traits::ContentAddressed;

    fn requires_content_addressed<T: ContentAddressed>(x: &T) -> [u8; 32] {
        x.content_hash()
    }

    let datom = sample_datom("trait-test");
    let hash = requires_content_addressed(&datom);
    assert_ne!(
        hash, [0u8; 32],
        "bd-20j: ContentAddressed must produce non-zero hash"
    );
}
