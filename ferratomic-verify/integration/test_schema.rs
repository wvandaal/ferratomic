//! Schema validation integration tests.
//!
//! INV-FERR-009, INV-FERR-031.
//! ALL TESTS MUST FAIL (red phase). Types are not yet implemented.

use ferratom::{AgentId, Attribute, EntityId, Op, TxId, Value};
use ferratomic_core::store::Store;
use ferratomic_core::writer::{Transaction, TxValidationError};

/// INV-FERR-009 + INV-FERR-031: Genesis schema has axiomatic meta-schema attributes.
/// All genesis() calls produce identical schemas.
#[test]
fn inv_ferr_009_genesis_schema() {
    let store1 = Store::genesis();
    let store2 = Store::genesis();

    assert_eq!(
        store1.schema(),
        store2.schema(),
        "INV-FERR-031: genesis() produced different schemas"
    );

    let schema = store1.schema();
    assert!(
        schema.get(&Attribute::from("db/ident")).is_some(),
        "INV-FERR-009: genesis schema missing :db/ident"
    );
    assert!(
        schema.get(&Attribute::from("db/valueType")).is_some(),
        "INV-FERR-009: genesis schema missing :db/valueType"
    );
    assert!(
        schema.get(&Attribute::from("db/cardinality")).is_some(),
        "INV-FERR-009: genesis schema missing :db/cardinality"
    );
    assert!(
        schema.get(&Attribute::from("db/doc")).is_some(),
        "INV-FERR-009: genesis schema missing :db/doc"
    );
}

/// INV-FERR-009: Transaction with unknown attribute is rejected.
#[test]
fn inv_ferr_009_reject_unknown_attribute() {
    let store = Store::genesis();
    let agent = AgentId::from_bytes([1u8; 16]);

    let tx = Transaction::new(agent).assert_datom(
        EntityId::from_content(b"e1"),
        Attribute::from("nonexistent/attribute"),
        Value::String("test".into()),
    );

    let result = tx.commit(store.schema());
    assert!(
        matches!(result, Err(TxValidationError::UnknownAttribute(_))),
        "INV-FERR-009: unknown attribute was not rejected. result={:?}",
        result
    );
}

/// INV-FERR-009: Transaction with mistyped value is rejected.
#[test]
fn inv_ferr_009_reject_wrong_type() {
    let store = Store::genesis();
    let agent = AgentId::from_bytes([1u8; 16]);

    // :db/ident expects Keyword, but we pass Long
    let tx = Transaction::new(agent).assert_datom(
        EntityId::from_content(b"e1"),
        Attribute::from("db/ident"),
        Value::Long(42),
    );

    let result = tx.commit(store.schema());
    assert!(
        matches!(result, Err(TxValidationError::SchemaViolation { .. })),
        "INV-FERR-009: wrong value type was not rejected. result={:?}",
        result
    );
}

/// INV-FERR-009: Schema evolution — define new attribute then use it.
#[test]
fn inv_ferr_009_schema_evolution() {
    let mut store = Store::genesis();
    let agent = AgentId::from_bytes([1u8; 16]);
    let new_attr_entity = EntityId::from_content(b"new-attr-entity");

    // First transaction: define a new attribute
    let define_tx = Transaction::new(agent.clone())
        .assert_datom(
            new_attr_entity.clone(),
            Attribute::from("db/ident"),
            Value::Keyword("user/email".into()),
        )
        .assert_datom(
            new_attr_entity.clone(),
            Attribute::from("db/valueType"),
            Value::Keyword("db.type/string".into()),
        )
        .assert_datom(
            new_attr_entity,
            Attribute::from("db/cardinality"),
            Value::Keyword("db.cardinality/one".into()),
        );

    let committed = define_tx
        .commit(store.schema())
        .expect("define tx should succeed");
    store.transact(committed).expect("transact should succeed");

    // Second transaction: use the newly defined attribute
    let use_tx = Transaction::new(agent).assert_datom(
        EntityId::from_content(b"user-1"),
        Attribute::from("user/email"),
        Value::String("alice@example.com".into()),
    );

    let committed = use_tx
        .commit(store.schema())
        .expect("use tx should succeed");
    store.transact(committed).expect("transact should succeed");

    // Verify the datom is in the store
    let snap = store.snapshot();
    let has_email = snap.datoms().any(|d| {
        d.attribute() == &Attribute::from("user/email")
            && d.value() == &Value::String("alice@example.com".into())
    });
    assert!(
        has_email,
        "INV-FERR-009: newly defined attribute datom not found in store"
    );
}

/// INV-FERR-009: Schema validation is atomic — partial transaction rejected entirely.
#[test]
fn inv_ferr_009_atomic_rejection() {
    let store = Store::genesis();
    let agent = AgentId::from_bytes([1u8; 16]);

    // Transaction with one valid and one invalid datom
    let tx = Transaction::new(agent)
        .assert_datom(
            EntityId::from_content(b"e1"),
            Attribute::from("db/doc"),
            Value::String("valid doc".into()),
        )
        .assert_datom(
            EntityId::from_content(b"e2"),
            Attribute::from("nonexistent/attr"),
            Value::String("invalid".into()),
        );

    let result = tx.commit(store.schema());
    assert!(
        result.is_err(),
        "INV-FERR-009: transaction with one invalid datom was not fully rejected"
    );
}
