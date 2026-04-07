//! CI-FERR-002: Type-Level Refinement Tower (Curry-Howard Encoding).
//!
//! Verifies that Rust types in the `ferratom` crate faithfully represent
//! the algebraic specification. Each type's cardinality equals the number
//! of valid states for its domain concept, and structural properties
//! (immutability, interning, ordering) are preserved.
//!
//! CI-FERR-002 (spec/07-refinement.md): "The ferratom types encode
//! propositions via the Curry-Howard correspondence. Each type's
//! cardinality equals the number of valid states for that concept."
//!
//! Tests are split into deterministic cardinality checks (`#[test]`) and
//! property-based structural checks (`proptest!` with 10,000 cases).

use std::sync::Arc;

use ferratom::{AgentId, Attribute, Datom, EntityId, NonNanFloat, Op, TxId, Value};
use proptest::prelude::*;

// ---------------------------------------------------------------------------
// Deterministic cardinality and structural checks
// ---------------------------------------------------------------------------

/// CI-FERR-002 / EntityId cardinality: `EntityId` is exactly 32 bytes.
///
/// The spec requires EntityId to inhabit 2^256 (the BLAKE3 hash space),
/// which is represented by exactly 32 bytes of storage. This is a
/// compile-time property verified at runtime via `size_of`.
#[test]
fn ci_ferr_002_entity_id_cardinality() {
    assert_eq!(
        std::mem::size_of::<EntityId>(),
        32,
        "CI-FERR-002: EntityId must be exactly 32 bytes (BLAKE3 output width)"
    );

    // Verify that from_content produces a value whose backing bytes
    // are exactly 32 bytes long.
    let eid = EntityId::from_content(b"cardinality test");
    assert_eq!(
        eid.as_bytes().len(),
        32,
        "CI-FERR-002: EntityId::from_content must produce a 32-byte value"
    );
}

/// CI-FERR-002 / Value variant count: Value is a sum type with exactly 11 variants.
///
/// The spec requires: Keyword, String, Long, Double, Bool, Instant, Uuid,
/// Bytes, Ref, BigInt, BigDec. We construct each variant and verify Debug
/// output is non-empty (round-trip through the Debug trait).
#[test]
fn ci_ferr_002_value_variant_count() {
    let variants: Vec<Value> = vec![
        Value::Keyword(Arc::from("db.type/string")),
        Value::String(Arc::from("hello")),
        Value::Long(42),
        Value::Double(NonNanFloat::new(1.5).expect("CI-FERR-002: 1.5 is a valid non-NaN float")),
        Value::Bool(true),
        Value::Instant(1_000_000),
        Value::Uuid([0u8; 16]),
        Value::Bytes(Arc::from(vec![1u8, 2, 3])),
        Value::Ref(EntityId::from_content(b"ref-target")),
        Value::BigInt(999_999_999_i128),
        Value::BigDec(123_456_i128),
    ];

    // Exactly 11 variants per CI-FERR-002 spec table.
    assert_eq!(
        variants.len(),
        11,
        "CI-FERR-002: Value must have exactly 11 variants"
    );

    // Each variant round-trips through Debug (derived) without panic.
    for (i, variant) in variants.iter().enumerate() {
        let debug_repr = format!("{variant:?}");
        assert!(
            !debug_repr.is_empty(),
            "CI-FERR-002: Value variant {} must have a non-empty Debug representation",
            i
        );
    }

    // Verify all variants are distinct from each other (sum type, not aliases).
    for i in 0..variants.len() {
        for j in (i + 1)..variants.len() {
            assert_ne!(
                std::mem::discriminant(&variants[i]),
                std::mem::discriminant(&variants[j]),
                "CI-FERR-002: Value variants {} and {} must have distinct discriminants",
                i,
                j
            );
        }
    }

    // Compile-time exhaustiveness enforcement: an exhaustive match
    // (no wildcard) on Value. Adding a 12th variant to Value produces
    // a compile error at this match site. This is the compile-time
    // guard that the manual vec construction above cannot provide.
    for variant in &variants {
        let _label = match variant {
            Value::Keyword(_) => "keyword",
            Value::String(_) => "string",
            Value::Long(_) => "long",
            Value::Double(_) => "double",
            Value::Bool(_) => "bool",
            Value::Instant(_) => "instant",
            Value::Uuid(_) => "uuid",
            Value::Bytes(_) => "bytes",
            Value::Ref(_) => "ref",
            Value::BigInt(_) => "bigint",
            Value::BigDec(_) => "bigdec",
        };
    }
}

/// CI-FERR-002 / Op cardinality: Op has exactly 2 variants (Assert, Retract).
///
/// Exhaustive match without wildcard — adding a third variant to `Op`
/// will produce a compile error at this match site, which is the point.
#[test]
fn ci_ferr_002_op_cardinality() {
    let ops = [Op::Assert, Op::Retract];

    // Exactly 2 variants.
    assert_eq!(ops.len(), 2, "CI-FERR-002: Op must have exactly 2 variants");

    // Exhaustive match: no wildcard. A third variant causes a compile error.
    for op in &ops {
        let label = match op {
            Op::Assert => "assert",
            Op::Retract => "retract",
        };
        assert!(
            !label.is_empty(),
            "CI-FERR-002: Op variant must have a non-empty label"
        );
    }

    // The two variants are distinct.
    assert_ne!(
        Op::Assert,
        Op::Retract,
        "CI-FERR-002: Assert and Retract must be distinct"
    );

    // Debug round-trip for both variants.
    assert!(
        !format!("{:?}", Op::Assert).is_empty(),
        "CI-FERR-002: Op::Assert Debug must be non-empty"
    );
    assert!(
        !format!("{:?}", Op::Retract).is_empty(),
        "CI-FERR-002: Op::Retract Debug must be non-empty"
    );
}

/// CI-FERR-002 / AgentId fixed-size: `AgentId` is exactly 16 bytes.
///
/// The spec requires a 16-byte agent identifier. This is a structural
/// property verified via `size_of`.
#[test]
fn ci_ferr_002_agent_id_fixed_size() {
    assert_eq!(
        std::mem::size_of::<AgentId>(),
        16,
        "CI-FERR-002: AgentId must be exactly 16 bytes"
    );

    // Verify the backing bytes are exactly 16 bytes.
    let agent = AgentId::from_bytes([0xABu8; 16]);
    assert_eq!(
        agent.as_bytes().len(),
        16,
        "CI-FERR-002: AgentId::as_bytes must return exactly 16 bytes"
    );
}

// ---------------------------------------------------------------------------
// Property-based structural checks
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(10_000))]

    /// CI-FERR-002 / EntityId cardinality (property): `from_content` always
    /// produces a 32-byte value, regardless of input content length.
    #[test]
    fn ci_ferr_002_entity_id_from_content_always_32_bytes(
        content in prop::collection::vec(any::<u8>(), 0..2048),
    ) {
        let eid = EntityId::from_content(&content);
        prop_assert_eq!(
            eid.as_bytes().len(),
            32,
            "CI-FERR-002: EntityId::from_content must produce 32 bytes for input of length {}",
            content.len()
        );
    }

    /// CI-FERR-002 / Datom 5-tuple structure: A Datom constructed from
    /// (entity, attribute, value, tx, op) returns those exact components
    /// via its accessors.
    ///
    /// This is the product-type cardinality check: the Datom is a faithful
    /// 5-tuple with no information loss or transformation in the accessors.
    #[test]
    fn ci_ferr_002_datom_5_tuple_structure(
        entity_bytes in any::<[u8; 32]>(),
        attr_name in "[a-z][a-z0-9_]{0,15}/[a-z][a-z0-9_]{0,31}",
        value_long in any::<i64>(),
        tx_physical in any::<u64>(),
        tx_logical in any::<u32>(),
        tx_agent_seed in any::<u16>(),
        is_assert in any::<bool>(),
    ) {
        let entity = EntityId::from_bytes(entity_bytes);
        let attribute = Attribute::from(attr_name.as_str());
        let value = Value::Long(value_long);
        let tx = TxId::new(tx_physical, tx_logical, tx_agent_seed);
        let op = if is_assert { Op::Assert } else { Op::Retract };

        let datom = Datom::new(entity, attribute.clone(), value.clone(), tx, op);

        // Each accessor returns the exact component used in construction.
        prop_assert_eq!(
            datom.entity(), entity,
            "CI-FERR-002: Datom::entity() must return the entity used in construction"
        );
        prop_assert_eq!(
            datom.attribute(), &attribute,
            "CI-FERR-002: Datom::attribute() must return the attribute used in construction"
        );
        prop_assert_eq!(
            datom.value(), &value,
            "CI-FERR-002: Datom::value() must return the value used in construction"
        );
        prop_assert_eq!(
            datom.tx(), tx,
            "CI-FERR-002: Datom::tx() must return the TxId used in construction"
        );
        prop_assert_eq!(
            datom.op(), op,
            "CI-FERR-002: Datom::op() must return the Op used in construction"
        );
    }

    /// CI-FERR-002 / Attribute interning: Two Attributes constructed from the
    /// same string are equal, and clone preserves equality.
    ///
    /// The spec requires Arc semantics: O(1) clone. We verify the observable
    /// consequence — clone produces an equal value — without accessing the
    /// internal Arc (private field).
    #[test]
    fn ci_ferr_002_attribute_interning(
        name in "[a-z][a-z0-9_]{0,15}/[a-z][a-z0-9_]{0,31}",
    ) {
        let a = Attribute::from(name.as_str());
        let b = Attribute::from(name.as_str());

        // Two Attributes from the same string are equal.
        prop_assert_eq!(
            &a, &b,
            "CI-FERR-002: Attributes from the same string must be equal"
        );

        // Clone preserves equality (O(1) Arc clone semantics).
        let cloned = a.clone();
        prop_assert_eq!(
            &a, &cloned,
            "CI-FERR-002: Attribute clone must produce an equal value"
        );

        // The string content is preserved through as_str().
        prop_assert_eq!(
            a.as_str(), name.as_str(),
            "CI-FERR-002: Attribute::as_str() must return the original string"
        );

        // Display produces the same string.
        prop_assert_eq!(
            format!("{a}"), name,
            "CI-FERR-002: Attribute Display must produce the original string"
        );
    }

    /// CI-FERR-002 / TxId ordering: TxId ordering is lexicographic on
    /// (physical, logical, agent). Construct two TxIds and verify that
    /// the ordering contract holds.
    ///
    /// INV-FERR-015: The total order enables HLC monotonicity.
    #[test]
    fn ci_ferr_002_tx_id_ordering(
        phys_a in any::<u64>(),
        log_a in any::<u32>(),
        agent_a in any::<u16>(),
        phys_b in any::<u64>(),
        log_b in any::<u32>(),
        agent_b in any::<u16>(),
    ) {
        let tx_a = TxId::new(phys_a, log_a, agent_a);
        let tx_b = TxId::new(phys_b, log_b, agent_b);

        // Reconstruct expected ordering: lexicographic on (physical, logical, agent).
        let agent_id_a = AgentId::from_seed(agent_a);
        let agent_id_b = AgentId::from_seed(agent_b);
        let expected = phys_a
            .cmp(&phys_b)
            .then_with(|| log_a.cmp(&log_b))
            .then_with(|| agent_id_a.cmp(&agent_id_b));

        prop_assert_eq!(
            tx_a.cmp(&tx_b),
            expected,
            "CI-FERR-002: TxId ordering must be lexicographic on (physical, logical, agent). \
             a=({}, {}, {}), b=({}, {}, {})",
            phys_a, log_a, agent_a, phys_b, log_b, agent_b
        );

        // Verify accessor consistency.
        prop_assert_eq!(
            tx_a.physical(), phys_a,
            "CI-FERR-002: TxId::physical() must return the physical component"
        );
        prop_assert_eq!(
            tx_a.logical(), log_a,
            "CI-FERR-002: TxId::logical() must return the logical component"
        );
        prop_assert_eq!(
            tx_a.agent(), agent_id_a,
            "CI-FERR-002: TxId::agent() must return the agent component"
        );
    }
}
