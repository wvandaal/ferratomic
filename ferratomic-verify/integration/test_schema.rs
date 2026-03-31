//! Schema validation integration tests.
//!
//! INV-FERR-009, INV-FERR-031.
//! Phase 4a: all tests passing against ferratomic-core implementation.

use ferratom::{AgentId, Attribute, EntityId, Value};
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

/// INV-FERR-019: Every FerraError variant can be constructed and exhaustively matched.
///
/// This test enumerates ALL variants of `FerraError` without using a wildcard (`_ =>`).
/// If a new variant is added to the enum, this test will fail to compile until it is
/// updated — which is the point.
#[test]
fn test_inv_ferr_019_error_exhaustiveness() {
    use ferratom::FerraError;

    let variants: Vec<FerraError> = vec![
        FerraError::WalWrite("test wal write".into()),
        FerraError::WalRead("test wal read".into()),
        FerraError::CheckpointCorrupted {
            expected: "abc123".into(),
            actual: "def456".into(),
        },
        FerraError::CheckpointWrite("test checkpoint write".into()),
        FerraError::Io("test io error".into()),
        FerraError::UnknownAttribute {
            attribute: "test/attr".into(),
        },
        FerraError::SchemaViolation {
            attribute: "test/attr".into(),
            expected: "String".into(),
            got: "Long".into(),
        },
        FerraError::EmptyTransaction,
        FerraError::Backpressure,
        FerraError::PeerUnreachable {
            addr: "127.0.0.1:9000".into(),
            reason: "connection refused".into(),
        },
        FerraError::SchemaIncompatible {
            attribute: "test/attr".into(),
            left: "String".into(),
            right: "Long".into(),
        },
        FerraError::InvariantViolation {
            invariant: "INV-FERR-005".into(),
            details: "test invariant violation".into(),
        },
    ];

    // Exhaustive match on every variant — no wildcards.
    // Adding a new FerraError variant will cause a compile error here.
    for error in &variants {
        match error {
            FerraError::WalWrite(msg) => {
                assert!(!msg.is_empty(), "WalWrite message should not be empty");
            }
            FerraError::WalRead(msg) => {
                assert!(!msg.is_empty(), "WalRead message should not be empty");
            }
            FerraError::CheckpointCorrupted { expected, actual } => {
                assert_ne!(
                    expected, actual,
                    "CheckpointCorrupted expected != actual"
                );
            }
            FerraError::CheckpointWrite(msg) => {
                assert!(!msg.is_empty(), "CheckpointWrite message should not be empty");
            }
            FerraError::Io(msg) => {
                assert!(!msg.is_empty(), "Io message should not be empty");
            }
            FerraError::UnknownAttribute { attribute } => {
                assert!(
                    !attribute.is_empty(),
                    "UnknownAttribute attribute should not be empty"
                );
            }
            FerraError::SchemaViolation {
                attribute,
                expected,
                got,
            } => {
                assert!(!attribute.is_empty(), "SchemaViolation attribute should not be empty");
                assert_ne!(
                    expected, got,
                    "SchemaViolation expected != got"
                );
            }
            FerraError::EmptyTransaction => {
                // Unit variant — construction is sufficient proof.
            }
            FerraError::SchemaIncompatible {
                attribute,
                left,
                right,
            } => {
                assert!(!attribute.is_empty(), "SchemaIncompatible attribute should not be empty");
                assert_ne!(
                    left, right,
                    "SchemaIncompatible left != right"
                );
            }
            FerraError::Backpressure => {
                // Unit variant — construction is sufficient proof.
            }
            FerraError::PeerUnreachable { addr, reason } => {
                assert!(!addr.is_empty(), "PeerUnreachable addr should not be empty");
                assert!(!reason.is_empty(), "PeerUnreachable reason should not be empty");
            }
            FerraError::InvariantViolation { invariant, details } => {
                assert!(
                    invariant.starts_with("INV-FERR-"),
                    "InvariantViolation invariant should follow naming convention"
                );
                assert!(!details.is_empty(), "InvariantViolation details should not be empty");
            }
        }
    }

    // Verify Display impl works for every variant (INV-FERR-019: typed errors).
    for error in &variants {
        let display = format!("{error}");
        assert!(
            !display.is_empty(),
            "INV-FERR-019: Display impl must produce non-empty output for all variants"
        );
    }

    // Verify std::error::Error impl (every variant is a valid Error).
    for error in &variants {
        let as_error: &dyn std::error::Error = error;
        let _ = format!("{as_error}");
    }

    assert_eq!(
        variants.len(),
        12,
        "INV-FERR-019: expected 12 FerraError variants — update this test if variants are added"
    );
}
