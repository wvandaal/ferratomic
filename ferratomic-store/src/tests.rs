use std::{collections::BTreeSet, sync::Arc};

use ferratom::{Attribute, Cardinality, Datom, EntityId, NodeId, Op, TxId, Value, ValueType};
use ferratomic_tx::Transaction;

use crate::{
    schema_evolution::{parse_cardinality, parse_value_type},
    store::{Store, TxReceipt},
};

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

/// The 25 axiomatic attribute idents expected in genesis schema (INV-FERR-031).
const GENESIS_ATTRIBUTE_IDENTS: [&str; 25] = [
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
    "tx/derivation-input",
    "tx/derivation-rule",
    "tx/derivation-source",
    "tx/origin",
    "tx/predecessor",
    "tx/provenance",
    "tx/rationale",
    "tx/signature",
    "tx/signer",
    "tx/time",
    "tx/validation-override",
];

#[test]
fn test_inv_ferr_031_genesis_schema_has_25_attributes() {
    let store = Store::genesis();
    assert_eq!(
        store.schema().len(),
        25,
        "INV-FERR-031: genesis schema must have exactly 25 axiomatic attributes"
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
    use ferratom::NodeId;
    use ferratomic_tx::Transaction;

    let mut store = Store::genesis();
    let node = NodeId::from_bytes([1u8; 16]);
    let tx = Transaction::new(node)
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

// -----------------------------------------------------------------------
// INV-FERR-001..003: Merge edge cases (bd-lg6m)
// -----------------------------------------------------------------------

/// bd-lg6m: merge(empty, empty) must be empty.
#[test]
fn test_inv_ferr_001_merge_empty_empty() {
    let a = Store::from_datoms(BTreeSet::new());
    let b = Store::from_datoms(BTreeSet::new());
    let merged = Store::from_merge(&a, &b);
    assert!(
        merged.is_empty(),
        "INV-FERR-001: merge(empty, empty) must be empty"
    );
}

/// bd-lg6m: merge(empty, X) == merge(X, empty) == X.
#[test]
fn test_inv_ferr_001_merge_empty_nonempty() {
    let empty = Store::from_datoms(BTreeSet::new());
    let mut datoms = BTreeSet::new();
    datoms.insert(sample_datom("merge-edge"));
    let nonempty = Store::from_datoms(datoms.clone());

    let ab = Store::from_merge(&empty, &nonempty);
    let ba = Store::from_merge(&nonempty, &empty);

    let expected: BTreeSet<&Datom> = datoms.iter().collect();
    assert_eq!(
        ab.datom_set().iter().collect::<BTreeSet<_>>(),
        expected,
        "INV-FERR-001: merge(empty, X) must equal X"
    );
    assert_eq!(
        ba.datom_set().iter().collect::<BTreeSet<_>>(),
        expected,
        "INV-FERR-001: merge(X, empty) must equal X"
    );
}

/// bd-lg6m: merge(X, X) == X (idempotence).
#[test]
fn test_inv_ferr_003_merge_self_idempotent() {
    let mut datoms = BTreeSet::new();
    datoms.insert(sample_datom("self-merge"));
    let store = Store::from_datoms(datoms);
    let merged = Store::from_merge(&store, &store);

    let original: BTreeSet<&Datom> = store.datom_set().iter().collect();
    let result: BTreeSet<&Datom> = merged.datom_set().iter().collect();
    assert_eq!(original, result, "INV-FERR-003: merge(X, X) must equal X");
}

// -----------------------------------------------------------------------
// INV-FERR-074: XOR homomorphic store fingerprint (bd-83j4)
// -----------------------------------------------------------------------

/// bd-83j4: empty store fingerprint is XOR identity [0; 32].
#[test]
fn test_inv_ferr_074_fingerprint_empty() {
    let store = Store::from_datoms(BTreeSet::new());
    assert_eq!(
        store.fingerprint(),
        Some(&[0u8; 32]),
        "INV-FERR-074: empty store fingerprint must be XOR identity"
    );
}

/// bd-83j4: non-empty store fingerprint is non-zero.
#[test]
fn test_inv_ferr_074_fingerprint_nonempty() {
    let mut datoms = BTreeSet::new();
    datoms.insert(sample_datom("fp-test"));
    let store = Store::from_datoms(datoms);
    let fp = store.fingerprint();
    assert!(
        fp.is_some(),
        "INV-FERR-074: Positional store must have fingerprint"
    );
    assert_ne!(
        fp,
        Some(&[0u8; 32]),
        "INV-FERR-074: non-empty store fingerprint must not be zero"
    );
}

/// bd-83j4: `Store::fingerprint()` dispatch -- `Some` for Positional, `None` for `OrdMap`.
#[test]
fn test_inv_ferr_074_fingerprint_dispatch() {
    let mut datoms = BTreeSet::new();
    datoms.insert(sample_datom("dispatch"));
    let mut store = Store::from_datoms(datoms);

    // Positional: fingerprint available.
    assert!(
        store.fingerprint().is_some(),
        "Positional must have fingerprint"
    );

    // Promote to OrdMap: fingerprint unavailable.
    store.promote();
    assert!(
        store.fingerprint().is_none(),
        "OrdMap must not have fingerprint"
    );

    // Demote back: fingerprint recomputed.
    store.demote();
    assert!(
        store.fingerprint().is_some(),
        "Demoted (Positional) must have fingerprint"
    );
}

/// bd-83j4: singleton store fingerprint equals the datom's content hash.
#[test]
fn test_inv_ferr_074_fingerprint_singleton() {
    let datom = sample_datom("singleton-fp");
    let mut datoms = BTreeSet::new();
    datoms.insert(datom.clone());
    let store = Store::from_datoms(datoms);
    assert_eq!(
        store.fingerprint(),
        Some(&datom.content_hash()),
        "INV-FERR-074: singleton fingerprint must equal content_hash"
    );
}

// -----------------------------------------------------------------------
// INV-FERR-027: Bloom filter entity_exists (bd-218b)
// -----------------------------------------------------------------------

/// bd-218b: `entity_exists` returns true for present entities.
#[test]
fn test_inv_ferr_027_entity_exists_present() {
    let datom = sample_datom("bloom-present");
    let mut datoms = BTreeSet::new();
    datoms.insert(datom.clone());
    let store = Store::from_datoms(datoms);
    let ps = store
        .positional()
        .expect("INV-FERR-027: store must be Positional after from_datoms");
    assert!(
        ps.entity_exists(&datom.entity()),
        "INV-FERR-027: entity_exists must return true for present entity"
    );
}

/// bd-218b: `entity_exists` returns false for absent entities.
#[test]
fn test_inv_ferr_027_entity_exists_absent() {
    let datom = sample_datom("bloom-absent");
    let mut datoms = BTreeSet::new();
    datoms.insert(datom);
    let store = Store::from_datoms(datoms);
    let ps = store
        .positional()
        .expect("INV-FERR-027: store must be Positional after from_datoms");
    let absent = ferratom::EntityId::from_content(b"definitely-not-here");
    assert!(
        !ps.entity_exists(&absent),
        "INV-FERR-027: entity_exists must return false for absent entity"
    );
}

/// bd-218b: `entity_exists` on empty store returns false.
#[test]
fn test_inv_ferr_027_entity_exists_empty() {
    let store = Store::from_datoms(BTreeSet::new());
    let ps = store
        .positional()
        .expect("INV-FERR-027: store must be Positional after from_datoms");
    let any_eid = ferratom::EntityId::from_content(b"anything");
    assert!(
        !ps.entity_exists(&any_eid),
        "INV-FERR-027: entity_exists on empty store must return false"
    );
}

#[test]
fn test_inv_ferr_007_epoch_overflow_rejected() {
    let mut store = Store::genesis();
    store.epoch = u64::MAX;
    let tx = Transaction::new(NodeId::from_bytes([1u8; 16]))
        .assert_datom(
            EntityId::from_content(b"overflow"),
            Attribute::from("db/doc"),
            Value::String("test".into()),
        )
        .commit_unchecked();
    let result = store.transact_test(tx);
    assert!(
        result.is_err(),
        "INV-FERR-007: transact at epoch u64::MAX must return Err"
    );
}

/// INV-FERR-072: `batch_splice_transact` applies multiple transactions in a
/// single merge, producing identical results to individual transacts.
#[test]
fn test_inv_ferr_072_batch_splice_epochs_and_datoms() {
    let (store, receipts) = batch_splice_3tx_fixture();

    assert_eq!(receipts.len(), 3, "INV-FERR-072: one receipt per tx");
    assert_eq!(receipts[0].epoch(), 1, "INV-FERR-007: epoch 1");
    assert_eq!(receipts[1].epoch(), 2, "INV-FERR-007: epoch 2");
    assert_eq!(receipts[2].epoch(), 3, "INV-FERR-007: epoch 3");
    assert_eq!(store.epoch(), 3, "INV-FERR-007: final epoch 3");

    // tx1: 1 user + 2 meta, tx2: 3 schema + 2 meta, tx3: 1 user + 2 meta = 11
    assert_eq!(store.len(), 11, "INV-FERR-072: all datoms present");
}

/// INV-FERR-072 + INV-FERR-009: schema evolution works across batch.
#[test]
fn test_inv_ferr_072_batch_splice_schema_and_repr() {
    let (store, _) = batch_splice_3tx_fixture();

    assert!(
        store.schema().get(&Attribute::from("user/email")).is_some(),
        "INV-FERR-009: schema evolution in batch must install user/email"
    );
    assert!(
        store.positional().is_some(),
        "INV-FERR-072: store must be Positional after batch_splice_transact"
    );
}

/// Shared fixture: genesis store + 3 transactions via `batch_splice_transact`.
fn batch_splice_3tx_fixture() -> (Store, Vec<TxReceipt>) {
    let mut store = Store::genesis();
    let node = NodeId::from_bytes([10u8; 16]);

    let tx1 = Transaction::new(node)
        .assert_datom(
            EntityId::from_content(b"batch-e1"),
            Attribute::from("db/doc"),
            Value::String(Arc::from("batch-doc-1")),
        )
        .commit(store.schema())
        .expect("tx1 valid");

    let tx2 = Transaction::new(node)
        .assert_datom(
            EntityId::from_content(b"attr-user-email"),
            Attribute::from("db/ident"),
            Value::Keyword("user/email".into()),
        )
        .assert_datom(
            EntityId::from_content(b"attr-user-email"),
            Attribute::from("db/valueType"),
            Value::Keyword("db.type/string".into()),
        )
        .assert_datom(
            EntityId::from_content(b"attr-user-email"),
            Attribute::from("db/cardinality"),
            Value::Keyword("db.cardinality/one".into()),
        )
        .commit(store.schema())
        .expect("tx2 valid");

    let tx3 = Transaction::new(node)
        .assert_datom(
            EntityId::from_content(b"batch-e3"),
            Attribute::from("db/doc"),
            Value::String(Arc::from("batch-doc-3")),
        )
        .commit(store.schema())
        .expect("tx3 valid");

    let batches = vec![
        (tx1.into_datoms(), TxId::with_node(100, 0, node)),
        (tx2.into_datoms(), TxId::with_node(200, 0, node)),
        (tx3.into_datoms(), TxId::with_node(300, 0, node)),
    ];

    let receipts = store
        .batch_splice_transact(batches)
        .expect("batch must succeed");
    (store, receipts)
}

/// INV-FERR-029: `live_apply` with a lower `TxId` is a no-op — the causal map
/// retains the higher `TxId` and the `live_set` is unchanged.
#[test]
fn test_inv_ferr_029_live_apply_noop_lower_txid() {
    use im::OrdSet;

    let entity = EntityId::from_content(b"live-noop-entity");
    let attr = Attribute::from("db/doc");
    let value = Value::String(Arc::from("hello"));

    // Insert a datom with TxId=2 (higher).
    let d_high = Datom::new(
        entity,
        attr.clone(),
        value.clone(),
        TxId::new(2, 0, 0),
        Op::Assert,
    );
    let mut store = Store::from_datoms(std::collections::BTreeSet::new());
    store.live_apply(&d_high);

    // Verify initial causal state.
    let key = (entity, attr.clone());
    let causal_before = store.live_causal.get(&key).cloned();
    let live_before: Option<OrdSet<Value>> = store.live_set.get(&key).cloned();
    assert!(
        live_before.is_some(),
        "INV-FERR-029: value must be LIVE after Assert"
    );

    // Apply a datom with TxId=1 (lower) for the same (entity, attribute, value).
    let d_low = Datom::new(
        entity,
        attr.clone(),
        value.clone(),
        TxId::new(1, 0, 0),
        Op::Assert,
    );
    store.live_apply(&d_low);

    // Causal map must still show TxId=2 (the higher one wins).
    let causal_after = store.live_causal.get(&key).cloned();
    assert_eq!(
        causal_before, causal_after,
        "INV-FERR-029: live_apply with lower TxId must not change causal map"
    );

    // live_set must be unchanged.
    let live_after: Option<OrdSet<Value>> = store.live_set.get(&key).cloned();
    assert_eq!(
        live_before, live_after,
        "INV-FERR-029: live_apply with lower TxId must not change live_set"
    );
}

/// INV-FERR-072: `batch_splice_transact` with empty batch is a no-op.
#[test]
fn test_inv_ferr_072_batch_splice_transact_empty() {
    let mut store = Store::genesis();
    let receipts = store
        .batch_splice_transact(Vec::new())
        .expect("empty batch must succeed");
    assert!(
        receipts.is_empty(),
        "INV-FERR-072: empty batch must return empty receipts"
    );
    assert_eq!(store.epoch(), 0, "epoch unchanged for empty batch");
}

/// INV-FERR-072: `batch_splice_transact` rejects empty transactions within batch.
#[test]
fn test_inv_ferr_072_batch_splice_transact_rejects_empty_tx() {
    let mut store = Store::genesis();
    let node = NodeId::from_bytes([11u8; 16]);
    let batches = vec![(Vec::new(), TxId::with_node(100, 0, node))];
    let result = store.batch_splice_transact(batches);
    assert!(
        result.is_err(),
        "INV-FERR-072: batch with empty transaction must return Err"
    );
}

/// INV-FERR-072: `batch_splice_transact` `OrdMap` fallback path.
#[test]
fn test_inv_ferr_072_batch_splice_ordmap_fallback() {
    let mut store = Store::genesis();
    let node = NodeId::from_bytes([12u8; 16]);
    // Force OrdMap representation
    store.promote();
    assert!(store.positional().is_none(), "must be OrdMap after promote");

    let tx = Transaction::new(node)
        .assert_datom(
            EntityId::from_content(b"ordmap-test"),
            Attribute::from("db/doc"),
            Value::String(Arc::from("via-ordmap")),
        )
        .commit(store.schema())
        .expect("tx valid");

    let batches = vec![(tx.into_datoms(), TxId::with_node(100, 0, node))];
    let receipts = store
        .batch_splice_transact(batches)
        .expect("batch must succeed");
    assert_eq!(receipts.len(), 1);
    // After batch on OrdMap, store should be demoted back to Positional
    assert!(
        store.positional().is_some(),
        "INV-FERR-072: OrdMap fallback must demote after batch"
    );
}

/// INV-FERR-072: batch produces identical datom set to sequential transacts.
#[test]
fn test_inv_ferr_072_batch_equals_sequential() {
    let node = NodeId::from_bytes([13u8; 16]);

    // Build two identical transaction sets
    let make_txs = |schema: &ferratom::Schema| -> Vec<Transaction<ferratomic_tx::Committed>> {
        vec![
            Transaction::new(node)
                .assert_datom(
                    EntityId::from_content(b"eq-1"),
                    Attribute::from("db/doc"),
                    Value::String(Arc::from("one")),
                )
                .commit(schema)
                .expect("tx1"),
            Transaction::new(node)
                .assert_datom(
                    EntityId::from_content(b"eq-2"),
                    Attribute::from("db/doc"),
                    Value::String(Arc::from("two")),
                )
                .commit(schema)
                .expect("tx2"),
        ]
    };

    // Sequential path
    let mut seq_store = Store::genesis();
    for tx in make_txs(seq_store.schema()) {
        seq_store.transact_test(tx).expect("seq transact");
    }

    // Batch path
    let mut batch_store = Store::genesis();
    let txs = make_txs(batch_store.schema());
    let batches: Vec<_> = txs
        .into_iter()
        .enumerate()
        .map(|(i, tx)| {
            (
                tx.into_datoms(),
                TxId::with_node(u64::try_from(i + 1).unwrap_or(0), 0, node),
            )
        })
        .collect();
    batch_store.batch_splice_transact(batches).expect("batch");

    // Compare datom sets (not epochs -- those differ by construction)
    let seq_datoms: BTreeSet<_> = seq_store.datoms().cloned().collect();
    let batch_datoms: BTreeSet<_> = batch_store.datoms().cloned().collect();
    assert_eq!(
        seq_datoms, batch_datoms,
        "INV-FERR-072: batch must produce same datom set as sequential"
    );
}
