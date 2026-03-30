//! Proptest generators for all Ferratomic core types.
//!
//! Arbitrary instances for domain types used by property-based tests.
//! Generators cover all value variants with uniform distribution.
//!
//! ## Usage
//!
//! ```rust
//! use ferratomic_verify::generators::*;
//! use proptest::prelude::*;
//!
//! proptest! {
//!     #[test]
//!     fn my_property(datom in arb_datom()) {
//!         // ...
//!     }
//! }
//! ```

use ferratom::{
    AgentId, Attribute, Datom, EntityId, Op, TxId, Value,
};
use ferratomic_core::store::Store;
use proptest::prelude::*;

// ---------------------------------------------------------------------------
// Leaf type generators
// ---------------------------------------------------------------------------

/// Arbitrary EntityId: 32-byte identifier for testing.
/// In production, EntityId = BLAKE3(content) per INV-FERR-012.
/// For generators, we use arbitrary bytes to cover the full ID space.
pub fn arb_entity_id() -> impl Strategy<Value = EntityId> {
    any::<[u8; 32]>().prop_map(EntityId::from_bytes)
}

/// Arbitrary Attribute: namespace/name format.
pub fn arb_attribute() -> impl Strategy<Value = Attribute> {
    "[a-z][a-z0-9_]{0,15}/[a-z][a-z0-9_]{0,31}"
        .prop_map(|s| Attribute::from(s.as_str()))
}

/// Arbitrary Value: all 9 variant types with uniform distribution.
pub fn arb_value() -> impl Strategy<Value = Value> {
    prop_oneof![
        any::<i64>().prop_map(Value::Long),
        any::<bool>().prop_map(Value::Bool),
        ".*".prop_map(Value::String),
        any::<f64>()
            .prop_filter("not NaN", |f| !f.is_nan())
            .prop_map(Value::Double),
        "[a-z][a-z0-9_/]{0,63}".prop_map(Value::Keyword),
        any::<i64>().prop_map(Value::Instant),
        any::<[u8; 16]>().prop_map(Value::Uuid),
        prop::collection::vec(any::<u8>(), 0..256).prop_map(Value::Bytes),
        arb_entity_id().prop_map(Value::Ref),
    ]
}

/// Arbitrary Op: Assert or Retract.
pub fn arb_op() -> impl Strategy<Value = Op> {
    prop_oneof![Just(Op::Assert), Just(Op::Retract)]
}

/// Arbitrary TxId: HLC timestamp components.
/// INV-FERR-015: HLC monotonicity.
pub fn arb_tx_id() -> impl Strategy<Value = TxId> {
    (any::<u64>(), any::<u32>(), any::<u16>())
        .prop_map(|(wall, counter, node)| TxId::new(wall, counter, node))
}

/// Arbitrary AgentId.
pub fn arb_agent_id() -> impl Strategy<Value = AgentId> {
    any::<[u8; 16]>().prop_map(AgentId::from_bytes)
}

// ---------------------------------------------------------------------------
// Composite type generators
// ---------------------------------------------------------------------------

/// Arbitrary Datom: the 5-tuple atomic fact.
/// INV-FERR-012: Content-addressed identity (Eq/Hash/Ord on all 5 fields).
/// INV-FERR-018: Immutable after creation.
pub fn arb_datom() -> impl Strategy<Value = Datom> {
    (
        arb_entity_id(),
        arb_attribute(),
        arb_value(),
        arb_tx_id(),
        arb_op(),
    )
        .prop_map(|(e, a, v, tx, op)| Datom::new(e, a, v, tx, op))
}

/// Arbitrary Store with up to `max_datoms` datoms.
/// INV-FERR-001..004: CRDT semilattice properties.
pub fn arb_store(max_datoms: usize) -> impl Strategy<Value = Store> {
    prop::collection::btree_set(arb_datom(), 0..max_datoms)
        .prop_map(Store::from_datoms)
}

/// Arbitrary committed Transaction (bypasses schema for testing).
pub fn arb_transaction(
) -> impl Strategy<Value = ferratomic_core::writer::Transaction<ferratomic_core::writer::Committed>>
{
    (arb_agent_id(), prop::collection::vec(arb_datom(), 1..20)).prop_map(
        |(agent, datoms)| {
            let mut tx = ferratomic_core::writer::Transaction::new(agent);
            for d in datoms {
                tx = tx.assert_datom(d.entity(), d.attribute().clone(), d.value().clone());
            }
            tx.commit_unchecked() // bypass schema for testing
        },
    )
}

/// Arbitrary multi-datom Transaction (at least 2 datoms).
/// Used for testing transaction atomicity (INV-FERR-006).
pub fn arb_multi_datom_transaction(
) -> impl Strategy<Value = ferratomic_core::writer::Transaction<ferratomic_core::writer::Committed>>
{
    (arb_agent_id(), prop::collection::vec(arb_datom(), 2..20)).prop_map(
        |(agent, datoms)| {
            let mut tx = ferratomic_core::writer::Transaction::new(agent);
            for d in datoms {
                tx = tx.assert_datom(d.entity(), d.attribute().clone(), d.value().clone());
            }
            tx.commit_unchecked()
        },
    )
}

// ---------------------------------------------------------------------------
// Schema-targeted generators (INV-FERR-009)
// ---------------------------------------------------------------------------

/// Arbitrary datom valid against the genesis schema.
pub fn arb_schema_valid_datom() -> impl Strategy<Value = Datom> {
    let schema_attrs = prop_oneof![
        Just(("db/ident", Value::Keyword("test/attr".into()))),
        Just(("db/valueType", Value::Keyword("db.type/string".into()))),
        Just(("db/cardinality", Value::Keyword("db.cardinality/one".into()))),
        Just(("db/doc", Value::String("test doc".into()))),
    ];
    (arb_entity_id(), schema_attrs, arb_tx_id()).prop_map(|(e, (attr, val), tx)| {
        Datom::new(e, Attribute::from(attr), val, tx, Op::Assert)
    })
}

/// Arbitrary datom with an attribute NOT in the genesis schema.
/// INV-FERR-009: Must be rejected by transact.
/// Generates varied unknown attribute names (prefix "zztest/" is provably
/// absent from genesis, which uses "db/" namespace).
pub fn arb_datom_with_unknown_attr() -> impl Strategy<Value = Datom> {
    (
        arb_entity_id(),
        "[a-z]{1,8}".prop_map(|suffix| Attribute::from(format!("zztest/{}", suffix).as_str())),
        arb_value(),
        arb_tx_id(),
    )
        .prop_map(|(e, attr, v, tx)| Datom::new(e, attr, v, tx, Op::Assert))
}

/// Arbitrary datom with a value type that doesn't match the attribute's declared type.
/// INV-FERR-009: Must be rejected by transact.
/// Generates varied (attribute, wrong-value-type) pairs to cover multiple schema attrs.
pub fn arb_datom_with_wrong_type() -> impl Strategy<Value = Datom> {
    // Each pair: (known attribute, a value type that DOES NOT match its declared type)
    let mismatched_pairs = prop_oneof![
        // :db/ident expects Keyword — give it Long
        Just(("db/ident", Value::Long(42))),
        // :db/valueType expects Keyword — give it Bool
        Just(("db/valueType", Value::Bool(true))),
        // :db/cardinality expects Keyword — give it String
        Just(("db/cardinality", Value::String("wrong".into()))),
        // :db/doc expects String — give it Long
        Just(("db/doc", Value::Long(99))),
    ];
    (arb_entity_id(), mismatched_pairs, arb_tx_id()).prop_map(|(e, (attr, val), tx)| {
        Datom::new(e, Attribute::from(attr), val, tx, Op::Assert)
    })
}
