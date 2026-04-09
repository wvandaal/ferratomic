//! Schema validation integration tests.
//!
//! INV-FERR-009, INV-FERR-019 (error Display), INV-FERR-023 (no unsafe),
//! INV-FERR-031.
//! Phase 4a: all tests passing against ferratomic-db implementation.

use ferratom::{Attribute, EntityId, FerraError, NodeId, Value};
use ferratomic_db::{
    store::Store,
    writer::{Building, Transaction, TxValidationError},
};

/// Build a transaction that defines a new schema attribute with the given ident,
/// value type, and cardinality on the given entity.
fn build_define_attribute_tx(
    node: NodeId,
    entity: EntityId,
    ident: &str,
    value_type: &str,
    cardinality: &str,
) -> Transaction<Building> {
    Transaction::new(node)
        .assert_datom(
            entity,
            Attribute::from("db/ident"),
            Value::Keyword(ident.into()),
        )
        .assert_datom(
            entity,
            Attribute::from("db/valueType"),
            Value::Keyword(value_type.into()),
        )
        .assert_datom(
            entity,
            Attribute::from("db/cardinality"),
            Value::Keyword(cardinality.into()),
        )
}

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
    let node = NodeId::from_bytes([1u8; 16]);

    let tx = Transaction::new(node).assert_datom(
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
    let node = NodeId::from_bytes([1u8; 16]);

    // :db/ident expects Keyword, but we pass Long
    let tx = Transaction::new(node).assert_datom(
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
    let node = NodeId::from_bytes([1u8; 16]);
    let new_attr_entity = EntityId::from_content(b"new-attr-entity");

    // First transaction: define a new attribute
    let define_tx = build_define_attribute_tx(
        node,
        new_attr_entity,
        "user/email",
        "db.type/string",
        "db.cardinality/one",
    );
    let committed = define_tx
        .commit(store.schema())
        .expect("define tx should succeed");
    store
        .transact_test(committed)
        .expect("transact should succeed");

    // Second transaction: use the newly defined attribute
    let use_tx = Transaction::new(node).assert_datom(
        EntityId::from_content(b"user-1"),
        Attribute::from("user/email"),
        Value::String("alice@example.com".into()),
    );
    let committed = use_tx
        .commit(store.schema())
        .expect("use tx should succeed");
    store
        .transact_test(committed)
        .expect("transact should succeed");

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
    let node = NodeId::from_bytes([1u8; 16]);

    // Transaction with one valid and one invalid datom
    let tx = Transaction::new(node)
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

/// INV-FERR-009: Invalid `db/valueType` keywords are rejected atomically.
#[test]
fn test_inv_ferr_009_reject_invalid_value_type() {
    let mut store = Store::genesis();
    let node = NodeId::from_bytes([1u8; 16]);
    let attr_entity = EntityId::from_content(b"invalid-value-type");

    let define_tx = Transaction::new(node)
        .assert_datom(
            attr_entity,
            Attribute::from("db/ident"),
            Value::Keyword("user/bad-value-type".into()),
        )
        .assert_datom(
            attr_entity,
            Attribute::from("db/valueType"),
            Value::Keyword("db.type/invalid".into()),
        )
        .assert_datom(
            attr_entity,
            Attribute::from("db/cardinality"),
            Value::Keyword("db.cardinality/one".into()),
        );

    let committed = define_tx
        .commit(store.schema())
        .expect("meta-schema datoms are well-typed keywords");
    let result = store.transact_test(committed);

    assert!(
        matches!(
            result,
            Err(FerraError::SchemaViolation {
                ref attribute,
                ref expected,
                ref got,
            }) if attribute == "db/valueType"
                && expected == "recognized db.type/* keyword"
                && got == "db.type/invalid"
        ),
        "INV-FERR-009: invalid db/valueType keyword was not rejected. result={result:?}"
    );
}

/// INV-FERR-009: Invalid `db/cardinality` keywords are rejected atomically.
#[test]
fn test_inv_ferr_009_reject_invalid_cardinality() {
    let mut store = Store::genesis();
    let node = NodeId::from_bytes([2u8; 16]);
    let attr_entity = EntityId::from_content(b"invalid-cardinality");

    let define_tx = Transaction::new(node)
        .assert_datom(
            attr_entity,
            Attribute::from("db/ident"),
            Value::Keyword("user/bad-cardinality".into()),
        )
        .assert_datom(
            attr_entity,
            Attribute::from("db/valueType"),
            Value::Keyword("db.type/string".into()),
        )
        .assert_datom(
            attr_entity,
            Attribute::from("db/cardinality"),
            Value::Keyword("db.cardinality/triple".into()),
        );

    let committed = define_tx
        .commit(store.schema())
        .expect("meta-schema datoms are well-typed keywords");
    let result = store.transact_test(committed);

    assert!(
        matches!(
            result,
            Err(FerraError::SchemaViolation {
                ref attribute,
                ref expected,
                ref got,
            }) if attribute == "db/cardinality"
                && expected == "recognized db.cardinality/* keyword"
                && got == "db.cardinality/triple"
        ),
        "INV-FERR-009: invalid db/cardinality keyword was not rejected. result={result:?}"
    );
}

/// Construct one instance of every `FerraError` variant for exhaustive testing.
fn build_all_ferra_error_variants() -> Vec<FerraError> {
    vec![
        FerraError::WalWrite("test wal write".into()),
        FerraError::WalRead("test wal read".into()),
        FerraError::CheckpointCorrupted {
            expected: "abc123".into(),
            actual: "def456".into(),
        },
        FerraError::CheckpointWrite("test checkpoint write".into()),
        FerraError::Io {
            kind: "Other".into(),
            message: "test io error".into(),
        },
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
        FerraError::SignatureInvalid {
            tx_description: "test signature".into(),
        },
        FerraError::TransportError("test transport".into()),
        FerraError::TruncatedChunk,
        FerraError::TrailingBytes,
        FerraError::NonCanonicalChunk,
        FerraError::EmptyChunk,
        FerraError::UnknownCodecTag(0x42),
        FerraError::NotImplemented("test feature"),
    ]
}

/// Assert fields valid for message-only FerraError variants.
fn assert_message_variant_valid(msg: &str, variant: &str) {
    assert!(!msg.is_empty(), "{} message should not be empty", variant);
}

/// Assert non-empty attribute, plus distinct left/right for dual-field variants.
fn assert_attr_non_empty(attribute: &str, variant: &str) {
    assert!(
        !attribute.is_empty(),
        "{} attribute should not be empty",
        variant
    );
}

/// Assert struct-field FerraError variants with attribute + pair fields.
fn assert_attr_pair_distinct(attr: &str, left: &str, right: &str, variant: &str) {
    assert_attr_non_empty(attr, variant);
    assert_ne!(left, right, "{} fields should differ", variant);
}

/// Exhaustive match on every `FerraError` variant -- no wildcards.
/// Adding a new variant will cause a compile error here.
fn assert_ferra_error_fields_valid(error: &FerraError) {
    match error {
        FerraError::WalWrite(msg) => assert_message_variant_valid(msg, "WalWrite"),
        FerraError::WalRead(msg) => assert_message_variant_valid(msg, "WalRead"),
        FerraError::CheckpointCorrupted { expected, actual } => {
            assert_ne!(expected, actual, "CheckpointCorrupted expected != actual");
        }
        FerraError::CheckpointWrite(msg) => assert_message_variant_valid(msg, "CheckpointWrite"),
        FerraError::Io { kind, message } => {
            assert_message_variant_valid(kind, "Io kind");
            assert_message_variant_valid(message, "Io message");
        }
        FerraError::UnknownAttribute { attribute } => {
            assert_attr_non_empty(attribute, "UnknownAttribute")
        }
        FerraError::SchemaViolation {
            attribute,
            expected,
            got,
        } => {
            assert_attr_pair_distinct(attribute, expected, got, "SchemaViolation");
        }
        FerraError::EmptyTransaction
        | FerraError::Backpressure
        | FerraError::TruncatedChunk
        | FerraError::TrailingBytes
        | FerraError::NonCanonicalChunk
        | FerraError::EmptyChunk => {}
        FerraError::SchemaIncompatible {
            attribute,
            left,
            right,
        } => {
            assert_attr_pair_distinct(attribute, left, right, "SchemaIncompatible");
        }
        FerraError::PeerUnreachable { addr, reason } => {
            assert_message_variant_valid(addr, "PeerUnreachable addr");
            assert_message_variant_valid(reason, "PeerUnreachable reason");
        }
        FerraError::InvariantViolation { invariant, details } => {
            assert!(
                invariant.starts_with("INV-FERR-"),
                "InvariantViolation naming"
            );
            assert_message_variant_valid(details, "InvariantViolation details");
        }
        FerraError::SignatureInvalid { tx_description } => {
            assert_message_variant_valid(tx_description, "SignatureInvalid tx_description");
        }
        FerraError::TransportError(msg) => {
            assert_message_variant_valid(msg, "TransportError");
        }
        FerraError::UnknownCodecTag(tag) => {
            assert_ne!(*tag, 0x01, "0x01 is a known codec tag (DatomPair)");
        }
        FerraError::NotImplemented(msg) => {
            assert!(!msg.is_empty(), "NotImplemented must have a message");
        }
    }
}

/// INV-FERR-019: Every `FerraError` variant can be constructed and exhaustively matched.
///
/// This test enumerates ALL variants of `FerraError` without using a wildcard (`_ =>`).
/// If a new variant is added to the enum, this test will fail to compile until it is
/// updated -- which is the point.
#[test]
fn test_inv_ferr_019_error_exhaustiveness() {
    let variants = build_all_ferra_error_variants();

    for error in &variants {
        assert_ferra_error_fields_valid(error);
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
        20,
        "INV-FERR-019: expected 20 FerraError variants -- update this test if variants are added"
    );
}

/// Build simple (message-only and unit) FerraError variants for Display testing.
fn build_simple_ferra_error_variants() -> Vec<(&'static str, FerraError)> {
    vec![
        ("WalWrite", FerraError::WalWrite("disk full".into())),
        ("WalRead", FerraError::WalRead("corrupt frame".into())),
        (
            "CheckpointCorrupted",
            FerraError::CheckpointCorrupted {
                expected: "aabb".into(),
                actual: "ccdd".into(),
            },
        ),
        (
            "CheckpointWrite",
            FerraError::CheckpointWrite("io error".into()),
        ),
        (
            "Io",
            FerraError::Io {
                kind: "PermissionDenied".into(),
                message: "permission denied".into(),
            },
        ),
        (
            "UnknownAttribute",
            FerraError::UnknownAttribute {
                attribute: "ghost/attr".into(),
            },
        ),
        ("EmptyTransaction", FerraError::EmptyTransaction),
        ("Backpressure", FerraError::Backpressure),
        ("TruncatedChunk", FerraError::TruncatedChunk),
        ("TrailingBytes", FerraError::TrailingBytes),
        ("NonCanonicalChunk", FerraError::NonCanonicalChunk),
        ("EmptyChunk", FerraError::EmptyChunk),
        ("UnknownCodecTag", FerraError::UnknownCodecTag(0x42)),
        ("NotImplemented", FerraError::NotImplemented("test feature")),
    ]
}

/// Build struct-field FerraError variants for Display testing.
fn build_struct_ferra_error_variants() -> Vec<(&'static str, FerraError)> {
    vec![
        (
            "SchemaViolation",
            FerraError::SchemaViolation {
                attribute: "user/age".into(),
                expected: "Long".into(),
                got: "String".into(),
            },
        ),
        (
            "PeerUnreachable",
            FerraError::PeerUnreachable {
                addr: "10.0.0.1:9000".into(),
                reason: "timeout".into(),
            },
        ),
        (
            "SchemaIncompatible",
            FerraError::SchemaIncompatible {
                attribute: "user/name".into(),
                left: "String".into(),
                right: "Keyword".into(),
            },
        ),
        (
            "InvariantViolation",
            FerraError::InvariantViolation {
                invariant: "INV-FERR-007".into(),
                details: "epoch overflow".into(),
            },
        ),
    ]
}

/// Build all named FerraError variants for Display testing.
fn build_named_ferra_error_variants() -> Vec<(&'static str, FerraError)> {
    let mut variants = build_simple_ferra_error_variants();
    variants.extend(build_struct_ferra_error_variants());
    variants
}

/// Assert that a FerraError's Display and Error outputs are non-empty and identical.
fn assert_display_and_error_match(name: &str, error: &FerraError) {
    let display = format!("{error}");
    assert!(
        !display.is_empty(),
        "INV-FERR-019: Display for {} must produce non-empty output",
        name
    );
    let as_error: &dyn std::error::Error = error;
    let error_str = format!("{as_error}");
    assert!(
        !error_str.is_empty(),
        "INV-FERR-019: Error trait for {} must produce non-empty output",
        name
    );
    assert_eq!(
        display, error_str,
        "INV-FERR-019: Display and Error output must match for {}",
        name
    );
}

/// INV-FERR-019: Every `FerraError` variant produces non-empty `Display` output.
///
/// Constructs every variant and verifies that `fmt::Display` and
/// `std::error::Error` produce meaningful, non-empty strings. This is the
/// Display-focused integration complement to the exhaustive-match test above.
///
/// bd-oizy: raises INV-FERR-019 verification depth to 4+ layers.
#[test]
fn test_inv_ferr_019_error_display_all_variants() {
    let variants = build_named_ferra_error_variants();
    for (name, error) in &variants {
        assert_display_and_error_match(name, error);
    }
}

/// INV-FERR-023: All workspace crates contain `#![forbid(unsafe_code)]`.
///
/// Compile-time defense: the `forbid(unsafe_code)` attribute makes `unsafe`
/// blocks a hard error. This integration test reads the source files to
/// verify the attribute is present -- a belt-and-suspenders check that
/// catches accidental removal even before compilation.
///
/// bd-oizy: raises INV-FERR-023 verification depth to 4+ layers.
#[test]
fn test_inv_ferr_023_forbid_unsafe_code() {
    // All five crate lib.rs files that must enforce unsafe code policy.
    let lib_files = [
        concat!(env!("CARGO_MANIFEST_DIR"), "/../ferratom-clock/src/lib.rs"),
        concat!(env!("CARGO_MANIFEST_DIR"), "/../ferratom/src/lib.rs"),
        concat!(env!("CARGO_MANIFEST_DIR"), "/../ferratomic-core/src/lib.rs"),
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../ferratomic-datalog/src/lib.rs"
        ),
        concat!(env!("CARGO_MANIFEST_DIR"), "/src/lib.rs"),
    ];

    for path in &lib_files {
        let content = std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("INV-FERR-023: cannot read {}: {}", path, e));

        // ADR-FERR-020: ferratomic-db uses #![deny(unsafe_code)] instead of
        // #![forbid(unsafe_code)] to allow the localized mmap.rs unsafe boundary.
        // The mmap module has #![allow(unsafe_code)] as the sole exception.
        // All other crates retain #![forbid(unsafe_code)].
        let is_core = path.contains("ferratomic-core");
        if is_core {
            assert!(
                content.contains("#![deny(unsafe_code)]"),
                "INV-FERR-023 / ADR-FERR-020: ferratomic-db must have \
                 #![deny(unsafe_code)] (not forbid, per ADR-FERR-020 mmap exception).",
            );
        } else {
            assert!(
                content.contains("#![forbid(unsafe_code)]"),
                "INV-FERR-023 violated: {path} is missing #![forbid(unsafe_code)]. \
                 Every non-core crate must forbid unsafe code.",
            );
        }
    }
}
