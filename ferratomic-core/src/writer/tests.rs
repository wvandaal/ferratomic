use std::sync::Arc;

use ferratom::{AgentId, Attribute, EntityId, Op, Schema, TxId, Value};

use super::{validate::value_matches_type, *};

/// Helper: build a minimal schema with one String attribute.
fn test_schema() -> Schema {
    use std::collections::HashMap;

    use ferratom::{AttributeDef, Cardinality, ResolutionMode};

    let mut attrs = HashMap::new();
    attrs.insert(
        Attribute::from("user/name"),
        AttributeDef::new(
            ferratom::ValueType::String,
            Cardinality::One,
            ResolutionMode::Lww,
            None,
        ),
    );
    attrs.insert(
        Attribute::from("user/age"),
        AttributeDef::new(
            ferratom::ValueType::Long,
            Cardinality::One,
            ResolutionMode::Lww,
            None,
        ),
    );
    Schema::from_attrs(attrs)
}

#[test]
fn test_transaction_new_is_empty() {
    let agent = AgentId::from_bytes([0u8; 16]);
    let tx = Transaction::new(agent);
    let committed = tx.commit_unchecked();
    assert!(
        committed.datoms().is_empty(),
        "new transaction should have no datoms"
    );
}

#[test]
fn test_transaction_assert_datom_accumulates() {
    let agent = AgentId::from_bytes([1u8; 16]);
    let entity = EntityId::from_content(b"test");
    let tx = Transaction::new(agent)
        .assert_datom(
            entity,
            Attribute::from("user/name"),
            Value::String("Alice".into()),
        )
        .assert_datom(entity, Attribute::from("user/age"), Value::Long(30));

    let committed = tx.commit_unchecked();
    assert_eq!(committed.datoms().len(), 2, "should have 2 datoms");
}

#[test]
fn test_transaction_agent_preserved() {
    let agent = AgentId::from_bytes([42u8; 16]);
    let tx = Transaction::new(agent).commit_unchecked();
    assert_eq!(
        tx.agent(),
        agent,
        "committed transaction should preserve agent"
    );
}

#[test]
fn test_inv_ferr_009_commit_valid() {
    let schema = test_schema();
    let agent = AgentId::from_bytes([0u8; 16]);
    let entity = EntityId::from_content(b"e1");

    let tx = Transaction::new(agent).assert_datom(
        entity,
        Attribute::from("user/name"),
        Value::String("Bob".into()),
    );

    let result = tx.commit(&schema);
    assert!(
        result.is_ok(),
        "INV-FERR-009: valid datom should be accepted"
    );
}

#[test]
fn test_inv_ferr_009_commit_unknown_attribute() {
    let schema = test_schema();
    let agent = AgentId::from_bytes([0u8; 16]);
    let entity = EntityId::from_content(b"e1");

    let tx = Transaction::new(agent).assert_datom(
        entity,
        Attribute::from("nonexistent/attr"),
        Value::String("test".into()),
    );

    let result = tx.commit(&schema);
    assert!(
        matches!(result, Err(TxValidationError::UnknownAttribute(_))),
        "INV-FERR-009: unknown attribute should be rejected, got {result:?}"
    );
}

#[test]
fn test_inv_ferr_009_commit_wrong_type() {
    let schema = test_schema();
    let agent = AgentId::from_bytes([0u8; 16]);
    let entity = EntityId::from_content(b"e1");

    let tx =
        Transaction::new(agent).assert_datom(entity, Attribute::from("user/name"), Value::Long(42));

    let result = tx.commit(&schema);
    assert!(
        matches!(result, Err(TxValidationError::SchemaViolation { .. })),
        "INV-FERR-009: wrong value type should be rejected, got {result:?}"
    );
}

#[test]
fn test_inv_ferr_006_atomic_rejection() {
    let schema = test_schema();
    let agent = AgentId::from_bytes([0u8; 16]);

    let tx = Transaction::new(agent)
        .assert_datom(
            EntityId::from_content(b"e1"),
            Attribute::from("user/name"),
            Value::String("valid".into()),
        )
        .assert_datom(
            EntityId::from_content(b"e2"),
            Attribute::from("bogus/attr"),
            Value::Long(0),
        );

    let result = tx.commit(&schema);
    assert!(
        result.is_err(),
        "INV-FERR-006: transaction with one invalid datom must be fully rejected"
    );
}

#[test]
fn test_datoms_have_placeholder_tx_id() {
    let agent = AgentId::from_bytes([0u8; 16]);
    let entity = EntityId::from_content(b"e1");

    let committed = Transaction::new(agent)
        .assert_datom(
            entity,
            Attribute::from("user/name"),
            Value::String("x".into()),
        )
        .commit_unchecked();

    let datom = &committed.datoms()[0];
    let placeholder = TxId::new(0, 0, 0);
    assert_eq!(
        datom.tx(),
        placeholder,
        "datoms should carry placeholder TxId before store assigns real one"
    );
}

#[test]
fn test_datoms_have_op_assert() {
    let agent = AgentId::from_bytes([0u8; 16]);
    let entity = EntityId::from_content(b"e1");

    let committed = Transaction::new(agent)
        .assert_datom(
            entity,
            Attribute::from("user/name"),
            Value::String("x".into()),
        )
        .commit_unchecked();

    assert_eq!(
        committed.datoms()[0].op(),
        Op::Assert,
        "assert_datom should produce Op::Assert"
    );
}

#[test]
fn test_value_matches_type_all_variants() {
    let pairs: Vec<(Value, ferratom::ValueType)> = vec![
        (Value::Keyword(Arc::from("k")), ferratom::ValueType::Keyword),
        (Value::String(Arc::from("s")), ferratom::ValueType::String),
        (Value::Long(0), ferratom::ValueType::Long),
        (
            Value::Double(ferratom::NonNanFloat::new(1.0).unwrap()),
            ferratom::ValueType::Double,
        ),
        (Value::Bool(true), ferratom::ValueType::Boolean),
        (Value::Instant(0), ferratom::ValueType::Instant),
        (Value::Uuid([0; 16]), ferratom::ValueType::Uuid),
        (
            Value::Bytes(Arc::from(vec![0u8])),
            ferratom::ValueType::Bytes,
        ),
        (
            Value::Ref(EntityId::from_bytes([0; 32])),
            ferratom::ValueType::Ref,
        ),
        (Value::BigInt(0), ferratom::ValueType::BigInt),
        (Value::BigDec(0), ferratom::ValueType::BigDec),
    ];

    for (value, vtype) in &pairs {
        assert!(
            value_matches_type(value, vtype),
            "value_matches_type should return true for matching pair: {value:?} vs {vtype:?}"
        );
    }
}

#[test]
fn test_value_matches_type_rejects_mismatches() {
    assert!(
        !value_matches_type(&Value::String(Arc::from("s")), &ferratom::ValueType::Long),
        "String should not match Long"
    );
    assert!(
        !value_matches_type(&Value::Long(42), &ferratom::ValueType::String),
        "Long should not match String"
    );
}

#[test]
fn test_commit_unchecked_accepts_anything() {
    let agent = AgentId::from_bytes([0u8; 16]);

    let committed = Transaction::new(agent)
        .assert_datom(
            EntityId::from_content(b"e"),
            Attribute::from("totally/made-up"),
            Value::Long(999),
        )
        .commit_unchecked();

    assert_eq!(committed.datoms().len(), 1);
}

#[test]
fn test_builder_chaining() {
    let agent = AgentId::from_bytes([0u8; 16]);
    let entity = EntityId::from_content(b"e");

    let mut tx = Transaction::new(agent);
    for i in 0..5 {
        tx = tx.assert_datom(entity, Attribute::from("user/name"), Value::Long(i));
    }
    let committed = tx.commit_unchecked();
    assert_eq!(committed.datoms().len(), 5);
}

/// Regression: bd-79n — `retract_datom` creates `Op::Retract` datoms.
#[test]
fn test_bug_bd_79n_retract_datom() {
    let agent = AgentId::from_bytes([0u8; 16]);
    let entity = EntityId::from_content(b"e1");

    let committed = Transaction::new(agent)
        .retract_datom(
            entity,
            Attribute::from("user/name"),
            Value::String("old_value".into()),
        )
        .commit_unchecked();

    assert_eq!(committed.datoms().len(), 1, "should have 1 datom");
    assert_eq!(
        committed.datoms()[0].op(),
        Op::Retract,
        "bd-79n: retract_datom must produce Op::Retract"
    );
}

/// Regression: bd-79n — mixed assert and retract transaction.
#[test]
fn test_bug_bd_79n_mixed_assert_retract() {
    let agent = AgentId::from_bytes([0u8; 16]);
    let entity = EntityId::from_content(b"e1");

    let committed = Transaction::new(agent)
        .assert_datom(
            entity,
            Attribute::from("user/name"),
            Value::String("new_value".into()),
        )
        .retract_datom(
            entity,
            Attribute::from("user/name"),
            Value::String("old_value".into()),
        )
        .commit_unchecked();

    assert_eq!(committed.datoms().len(), 2, "should have assert + retract");
    assert_eq!(committed.datoms()[0].op(), Op::Assert);
    assert_eq!(committed.datoms()[1].op(), Op::Retract);
}
