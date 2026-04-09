//! Proptest generators for all Ferratomic core types.
//!
//! Arbitrary instances for domain types used by property-based tests.
//! Generators cover all value variants with weighted distribution
//! (random arms weight 10, edge-case arms weight 1).
//!
//! ## Usage
//!
//! ```rust,ignore
//! use ferratomic_verify::generators::*;
//! use proptest::prelude::*;
//!
//! proptest! {
//!     fn my_property(datom in arb_datom()) {
//!         // ...
//!     }
//! }
//! ```

use std::collections::BTreeSet;

use ferratom::{Attribute, Datom, EntityId, NodeId, Op, TxId, Value};
use ferratomic_db::store::Store;
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
    "[a-z][a-z0-9_]{0,15}/[a-z][a-z0-9_]{0,31}".prop_map(|s| Attribute::from(s.as_str()))
}

/// Construct a `Value::Double` from a known non-NaN `f64`.
///
/// Falls back to `Value::Long(0)` if the value is somehow NaN,
/// satisfying NEG-FERR-001 (no panics) without `expect`/`unwrap`.
/// All callers pass compile-time constants that are provably non-NaN.
fn double_edge(f: f64) -> Value {
    match ferratom::NonNanFloat::new(f) {
        Some(nn) => Value::Double(nn),
        // Fallback: should never be reached for the constants below.
        None => Value::Long(0),
    }
}

/// Arbitrary Value: all 11 variant types with weighted distribution.
///
/// Random generators get weight 10 each; edge-case `Just(...)` arms get
/// weight 1 each, preventing edge cases from diluting random coverage.
///
/// INV-FERR-012 (Content-Addressed Identity): edge cases verify that
/// BLAKE3 hashing handles all representable values correctly, including
/// -0.0, infinities, empty strings, and integer extremes.
///
/// Wraps generated primitives into `Arc`/`NonNanFloat` for Value enum compatibility.
pub fn arb_value() -> impl Strategy<Value = Value> {
    use std::sync::Arc;
    prop_oneof![
        // --- Random generation (weight 10 each) ---
        10 => any::<i64>().prop_map(Value::Long),
        10 => any::<bool>().prop_map(Value::Bool),
        10 => ".*".prop_map(|s| Value::String(Arc::from(s.as_str()))),
        10 => any::<f64>().prop_filter_map("not NaN", |f| {
            ferratom::NonNanFloat::new(f).map(Value::Double)
        }),
        10 => "[a-z][a-z0-9_/]{0,63}".prop_map(|s| Value::Keyword(Arc::from(s.as_str()))),
        10 => any::<i64>().prop_map(Value::Instant),
        10 => any::<[u8; 16]>().prop_map(Value::Uuid),
        10 => prop::collection::vec(any::<u8>(), 0..256).prop_map(|v| Value::Bytes(Arc::from(v))),
        10 => arb_entity_id().prop_map(Value::Ref),
        10 => any::<i128>().prop_map(Value::BigInt),
        10 => any::<i128>().prop_map(Value::BigDec),
        // --- Edge cases (bd-tj8r, INV-FERR-012) — weight 1 each ---
        // Double: -0.0 (distinct bit pattern from +0.0)
        1 => Just(double_edge(-0.0)),
        // Double: positive infinity
        1 => Just(double_edge(f64::INFINITY)),
        // Double: negative infinity
        1 => Just(double_edge(f64::NEG_INFINITY)),
        // Double: smallest positive normal
        1 => Just(double_edge(f64::MIN_POSITIVE)),
        // Double: f64 extremes (bd-b5mw)
        1 => Just(double_edge(f64::MAX)),
        1 => Just(double_edge(f64::MIN)),
        // String: empty string
        1 => Just(Value::String(Arc::from(""))),
        // Keyword: empty-ish keyword
        1 => Just(Value::Keyword(Arc::from(""))),
        // Long: extremes
        1 => Just(Value::Long(i64::MIN)),
        1 => Just(Value::Long(i64::MAX)),
        1 => Just(Value::Long(0)),
        // Instant: extremes (epoch boundaries)
        1 => Just(Value::Instant(i64::MIN)),
        1 => Just(Value::Instant(i64::MAX)),
        1 => Just(Value::Instant(0)),
        // Uuid: all zeros and all ones
        1 => Just(Value::Uuid([0u8; 16])),
        1 => Just(Value::Uuid([0xFF; 16])),
        // Bytes: empty
        1 => Just(Value::Bytes(Arc::from(Vec::<u8>::new().as_slice()))),
        // Ref: boundary EntityIds (bd-wdcz)
        1 => Just(Value::Ref(EntityId::from_bytes([0u8; 32]))),
        1 => Just(Value::Ref(EntityId::from_bytes([0xFF; 32]))),
        // BigInt: extremes
        1 => Just(Value::BigInt(i128::MIN)),
        1 => Just(Value::BigInt(i128::MAX)),
        1 => Just(Value::BigInt(0)),
        // BigDec: extremes
        1 => Just(Value::BigDec(i128::MIN)),
        1 => Just(Value::BigDec(i128::MAX)),
        1 => Just(Value::BigDec(0)),
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

/// Arbitrary NodeId: 16-byte node identifier for testing.
pub fn arb_node_id() -> impl Strategy<Value = NodeId> {
    any::<[u8; 16]>().prop_map(NodeId::from_bytes)
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
    prop::collection::btree_set(arb_datom(), 0..max_datoms).prop_map(Store::from_datoms)
}

/// Arbitrary pair of Stores with controlled overlap (bd-vd5d).
///
/// Generates two stores that share `overlap_fraction` of their datoms,
/// ensuring non-empty intersection. This exercises merge dedup,
/// LIVE resolution, and schema conflict paths.
///
/// Strategy: generate a shared datom pool, partition into three sets
/// (A-only, B-only, shared), where `overlap_fraction` controls the
/// proportion of pool datoms that appear in both stores.
///
/// INV-FERR-001..004: merge must handle overlapping datom sets correctly.
pub fn arb_store_with_overlap(
    max_datoms: usize,
    overlap_fraction: f64,
) -> impl Strategy<Value = (Store, Store)> {
    // Clamp overlap to [0.0, 1.0].
    let overlap = overlap_fraction.clamp(0.0, 1.0);
    // Need at least 3 datoms in the pool to guarantee non-trivial partitions.
    let pool_size = max_datoms.max(3);

    prop::collection::vec(arb_datom(), pool_size..=pool_size).prop_map(move |pool| {
        let overlap_count = ((pool.len() as f64 * overlap) as usize).max(1);
        let remaining = pool.len().saturating_sub(overlap_count);
        let half = remaining / 2;

        let mut a_datoms = BTreeSet::new();
        let mut b_datoms = BTreeSet::new();

        // First `overlap_count` datoms go into both stores (shared).
        for d in pool.iter().take(overlap_count) {
            a_datoms.insert(d.clone());
            b_datoms.insert(d.clone());
        }

        // Next `half` go to A only.
        for d in pool.iter().skip(overlap_count).take(half) {
            a_datoms.insert(d.clone());
        }

        // Remainder go to B only.
        for d in pool.iter().skip(overlap_count + half) {
            b_datoms.insert(d.clone());
        }

        (Store::from_datoms(a_datoms), Store::from_datoms(b_datoms))
    })
}

/// Arbitrary committed Transaction (bypasses schema for testing).
pub fn arb_transaction(
) -> impl Strategy<Value = ferratomic_db::writer::Transaction<ferratomic_db::writer::Committed>> {
    (arb_node_id(), prop::collection::vec(arb_datom(), 1..20)).prop_map(|(node, datoms)| {
        let mut tx = ferratomic_db::writer::Transaction::new(node);
        for d in datoms {
            tx = tx.assert_datom(d.entity(), d.attribute().clone(), d.value().clone());
        }
        tx.commit_unchecked() // bypass schema for testing
    })
}

/// Arbitrary multi-datom Transaction (at least 2 datoms).
/// Used for testing transaction atomicity (INV-FERR-006).
pub fn arb_multi_datom_transaction(
) -> impl Strategy<Value = ferratomic_db::writer::Transaction<ferratomic_db::writer::Committed>> {
    (arb_node_id(), prop::collection::vec(arb_datom(), 2..20)).prop_map(|(node, datoms)| {
        let mut tx = ferratomic_db::writer::Transaction::new(node);
        for d in datoms {
            tx = tx.assert_datom(d.entity(), d.attribute().clone(), d.value().clone());
        }
        tx.commit_unchecked()
    })
}

// ---------------------------------------------------------------------------
// Schema-targeted generators (INV-FERR-009)
// ---------------------------------------------------------------------------

/// Arbitrary datom valid against the genesis schema.
pub fn arb_schema_valid_datom() -> impl Strategy<Value = Datom> {
    let schema_attrs = prop_oneof![
        Just(("db/ident", Value::Keyword("test/attr".into()))),
        Just(("db/valueType", Value::Keyword("db.type/string".into()))),
        Just((
            "db/cardinality",
            Value::Keyword("db.cardinality/one".into())
        )),
        Just(("db/doc", Value::String("test doc".into()))),
    ];
    (arb_entity_id(), schema_attrs, arb_tx_id())
        .prop_map(|(e, (attr, val), tx)| Datom::new(e, Attribute::from(attr), val, tx, Op::Assert))
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

// ---------------------------------------------------------------------------
// Shared test helpers
// ---------------------------------------------------------------------------

/// Verify index bijection: primary set == each secondary index set.
///
/// INV-FERR-005: All four indexes (EAVT, AEVT, VAET, AVET) must contain
/// exactly the same datom set as the primary store.
///
/// bd-h2fz: promotes a clone to OrdMap if needed (Positional stores
/// have no OrdMap indexes, but their permutation arrays are built
/// from the same canonical sort, so bijection is by construction).
pub fn verify_index_bijection(store: &Store) -> bool {
    let mut promoted = store.clone();
    promoted.promote();
    let primary: BTreeSet<&Datom> = promoted.datoms().collect();
    // bd-0k2k: promote() on a Store always produces indexes. If this
    // ever returns None, there is a bug in Store::promote(). Returning
    // false here triggers a proptest failure with a clear message.
    let Some(indexes) = promoted.indexes() else {
        return false;
    };
    let eavt: BTreeSet<&Datom> = indexes.eavt_datoms().collect();
    let aevt: BTreeSet<&Datom> = indexes.aevt_datoms().collect();
    let vaet: BTreeSet<&Datom> = indexes.vaet_datoms().collect();
    let avet: BTreeSet<&Datom> = indexes.avet_datoms().collect();

    primary == eavt && primary == aevt && primary == vaet && primary == avet
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
    (arb_entity_id(), mismatched_pairs, arb_tx_id())
        .prop_map(|(e, (attr, val), tx)| Datom::new(e, Attribute::from(attr), val, tx, Op::Assert))
}
