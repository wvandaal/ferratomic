//! CRDT algebraic property tests for Ferratomic.
//!
//! Tests INV-FERR-001 (commutativity), INV-FERR-002 (associativity),
//! INV-FERR-003 (idempotency), INV-FERR-004 (monotonic growth),
//! INV-FERR-010 (convergence), INV-FERR-012 (content-addressed identity),
//! INV-FERR-031 (genesis determinism).
//!
//! Phase 4a: all tests passing against ferratomic-db implementation.

use std::collections::BTreeSet;

use ferratom::{Datom, EntityId};
use ferratomic_db::{merge::merge, store::Store};
use ferratomic_verify::generators::{self, *};
use proptest::prelude::*;

/// Verify index bijection: delegates to shared helper in generators.
/// INV-FERR-005: All four indexes must match the primary datom set.
fn verify_index_bijection(store: &Store) -> bool {
    generators::verify_index_bijection(store)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(10_000))]

    /// INV-FERR-001: merge(A, B) == merge(B, A) for all store pairs.
    ///
    /// Falsification: any pair (A, B) where the datom set of merge(A, B)
    /// differs from merge(B, A). Would indicate order-dependent operations
    /// in the merge path.
    #[test]
    fn inv_ferr_001_merge_commutativity(
        a in arb_store(100),
        b in arb_store(100),
    ) {
        let ab = merge(&a, &b).expect("INV-FERR-001: merge(A,B) must succeed");
        let ba = merge(&b, &a).expect("INV-FERR-001: merge(B,A) must succeed");

        let (a_len, b_len, ab_len, ba_len) = (a.len(), b.len(), ab.len(), ba.len());
        // bd-2jx: compare datom sets, schema length, and epoch for equality.
        // Store does not implement PartialEq; compare components instead.
        prop_assert_eq!(
            ab.datom_set(), ba.datom_set(),
            "INV-FERR-001 violated: merge(A,B).datom_set != merge(B,A).datom_set. \
             |A|={}, |B|={}, |A∪B|={}, |B∪A|={}",
            a_len, b_len, ab_len, ba_len
        );
        prop_assert_eq!(
            ab.schema().len(), ba.schema().len(),
            "INV-FERR-001 violated: merge(A,B).schema != merge(B,A).schema"
        );
        prop_assert_eq!(
            ab.epoch(), ba.epoch(),
            "INV-FERR-001 violated: merge(A,B).epoch != merge(B,A).epoch"
        );
        // INV-FERR-029: LIVE state commutativity.
        for d in ab.datoms() {
            prop_assert_eq!(
                ab.live_values(d.entity(), d.attribute()),
                ba.live_values(d.entity(), d.attribute()),
                "INV-FERR-001/029: LIVE values diverged after commuted merge"
            );
        }
    }

    /// INV-FERR-002: merge(merge(A, B), C) == merge(A, merge(B, C)).
    ///
    /// Falsification: any triple (A, B, C) where regrouping changes result.
    /// Would indicate accumulated state in merge (counters, markers).
    #[test]
    fn inv_ferr_002_merge_associativity(
        a in arb_store(50),
        b in arb_store(50),
        c in arb_store(50),
    ) {
        let ab_c = merge(
            &merge(&a, &b).expect("INV-FERR-002: merge(A,B) must succeed"),
            &c,
        ).expect("INV-FERR-002: merge(AB,C) must succeed");
        let a_bc = merge(
            &a,
            &merge(&b, &c).expect("INV-FERR-002: merge(B,C) must succeed"),
        ).expect("INV-FERR-002: merge(A,BC) must succeed");

        let (a_len, b_len, c_len) = (a.len(), b.len(), c.len());
        // bd-2jx: compare datom sets, schema length, and epoch for equality.
        // Store does not implement PartialEq; compare components instead.
        prop_assert_eq!(
            ab_c.datom_set(), a_bc.datom_set(),
            "INV-FERR-002 violated: merge(merge(A,B),C).datom_set != merge(A,merge(B,C)).datom_set. \
             |A|={}, |B|={}, |C|={}",
            a_len, b_len, c_len
        );
        prop_assert_eq!(
            ab_c.schema().len(), a_bc.schema().len(),
            "INV-FERR-002 violated: schema mismatch"
        );
        prop_assert_eq!(
            ab_c.epoch(), a_bc.epoch(),
            "INV-FERR-002 violated: epoch mismatch"
        );
        // INV-FERR-029: LIVE state associativity.
        for d in ab_c.datoms() {
            prop_assert_eq!(
                ab_c.live_values(d.entity(), d.attribute()),
                a_bc.live_values(d.entity(), d.attribute()),
                "INV-FERR-002/029: LIVE values diverged after regrouped merge"
            );
        }
    }

    /// INV-FERR-003: merge(A, A) == A for all stores.
    ///
    /// Falsification: a store A where merge(A, A) differs from A in
    /// datom set, cardinality, or index state. Would indicate side effects.
    #[test]
    fn inv_ferr_003_merge_idempotency(
        store in arb_store(100),
    ) {
        let merged = merge(&store, &store).expect("INV-FERR-003: self-merge must succeed");

        let store_len = store.len();
        let merged_len = merged.len();
        // Also verify index state is preserved (spec falsification includes "different index state")
        prop_assert!(
            verify_index_bijection(&merged),
            "INV-FERR-003 violated: index bijection broken after self-merge. |A|={}",
            store_len
        );
        // bd-2jx: compare datom sets, schema length, and epoch for equality.
        // Store does not implement PartialEq; compare components instead.
        prop_assert_eq!(
            store.datom_set(), merged.datom_set(),
            "INV-FERR-003 violated: merge(A,A).datom_set != A.datom_set. |A|={}, |merge(A,A)|={}",
            store_len, merged_len
        );
        prop_assert_eq!(
            store.schema().len(), merged.schema().len(),
            "INV-FERR-003 violated: schema mismatch after self-merge"
        );
        prop_assert_eq!(
            store.epoch(), merged.epoch(),
            "INV-FERR-003 violated: epoch mismatch after self-merge"
        );
    }

    /// INV-FERR-004: |transact(S, T)| > |S| for non-empty T.
    /// Strict growth: every transaction adds at least tx metadata.
    ///
    /// Falsification: store.len() does not increase after transact.
    #[test]
    fn inv_ferr_004_monotonic_growth_transact(
        initial in arb_store(50),
        tx in arb_transaction(),
    ) {
        let pre_datoms: std::collections::BTreeSet<ferratom::Datom> = initial.datoms().cloned().collect();
        let pre_len = initial.len();

        let mut store = initial;
        let _receipt = store.transact_test(tx)
            .expect("INV-FERR-004: transact must succeed for committed tx");

        // Strict growth
        prop_assert!(
            store.len() > pre_len,
            "INV-FERR-004 violated: store did not grow after transact. \
             pre={}, post={}",
            pre_len, store.len()
        );
        // Monotonicity: no datoms lost
        for d in &pre_datoms {
            prop_assert!(
                store.datom_set().contains(d),
                "INV-FERR-004 violated: datom lost after transact: {:?}",
                d
            );
        }
    }

    /// INV-FERR-004: |merge(A, B)| >= max(|A|, |B|).
    /// Non-strict growth: merge result is superset of both inputs.
    #[test]
    fn inv_ferr_004_monotonic_growth_merge(
        a in arb_store(50),
        b in arb_store(50),
    ) {
        let merged = merge(&a, &b).expect("INV-FERR-004: merge must succeed");

        prop_assert!(
            merged.len() >= a.len(),
            "INV-FERR-004 violated: merge result smaller than A. \
             |A|={}, |merge|={}",
            a.len(), merged.len()
        );
        prop_assert!(
            merged.len() >= b.len(),
            "INV-FERR-004 violated: merge result smaller than B. \
             |B|={}, |merge|={}",
            b.len(), merged.len()
        );

        for d in a.datom_set().iter() {
            prop_assert!(
                merged.datom_set().contains(d),
                "INV-FERR-004 violated: datom from A missing in merge result"
            );
        }
        for d in b.datom_set().iter() {
            prop_assert!(
                merged.datom_set().contains(d),
                "INV-FERR-004 violated: datom from B missing in merge result"
            );
        }
    }

    /// INV-FERR-010: Same updates => same state, regardless of application order.
    /// Strong eventual consistency (SEC).
    ///
    /// Falsification: two replicas with same update set diverge.
    #[test]
    fn inv_ferr_010_convergence(
        datoms in prop::collection::vec(arb_datom(), 0..50),
        perm_seed in any::<u64>(),
    ) {
        use rand::seq::SliceRandom;
        use rand::rngs::StdRng;
        use rand::SeedableRng;

        let mut r1 = Store::genesis();
        let mut r2 = Store::genesis();

        for d in &datoms {
            r1.insert(d);
        }

        let mut shuffled = datoms.clone();
        let mut rng = StdRng::seed_from_u64(perm_seed);
        shuffled.shuffle(&mut rng);
        for d in &shuffled {
            r2.insert(d);
        }

        prop_assert_eq!(
            r1.datom_set(),
            r2.datom_set(),
            "INV-FERR-010 violated: same datoms in different order produced \
             different states. |datoms|={}, |r1|={}, |r2|={}",
            datoms.len(), r1.len(), r2.len()
        );
    }

    /// INV-FERR-012: Content-addressed identity.
    /// Same content => same EntityId.
    #[test]
    fn inv_ferr_012_content_addressed_identity(
        content in prop::collection::vec(any::<u8>(), 0..1024),
    ) {
        let id1 = EntityId::from_content(&content);
        let id2 = EntityId::from_content(&content);

        prop_assert_eq!(
            id1, id2,
            "INV-FERR-012 violated: same content produced different EntityIds"
        );
    }

    /// INV-FERR-012: Different content => different EntityId (collision resistance).
    #[test]
    fn inv_ferr_012_collision_resistance(
        content_a in prop::collection::vec(any::<u8>(), 1..512),
        content_b in prop::collection::vec(any::<u8>(), 1..512),
    ) {
        prop_assume!(content_a != content_b);

        let id_a = EntityId::from_content(&content_a);
        let id_b = EntityId::from_content(&content_b);

        prop_assert_ne!(
            id_a, id_b,
            "INV-FERR-012: BLAKE3 collision detected (astronomically unlikely). \
             |a|={}, |b|={}",
            content_a.len(), content_b.len()
        );
    }

    /// INV-FERR-009 / C4: Merge is exempt from schema validation.
    /// Merge is pure set union — datoms with unknown attributes MUST survive merge.
    /// A merge that validates schema would violate C4.
    ///
    /// Falsification: a datom present in store B (with unknown attribute)
    /// is absent from merge(A, B).
    #[test]
    fn inv_ferr_009_merge_exempt_from_schema(
        a in arb_store(30),
        unknown_datoms in prop::collection::vec(arb_datom_with_unknown_attr(), 1..20),
    ) {
        // Build store B with datoms whose attributes are NOT in genesis schema
        let b = Store::from_datoms(unknown_datoms.iter().cloned().collect());

        let merged = merge(&a, &b).expect("INV-FERR-009: merge must succeed");

        // Every datom from B must be in the merged result — merge does NOT filter
        for d in &unknown_datoms {
            prop_assert!(
                merged.datom_set().contains(d),
                "INV-FERR-009/C4 violated: merge rejected datom with unknown attribute. \
                 Merge must be pure set union, not schema-validated. attr={:?}",
                d.attribute()
            );
        }
    }

    /// INV-FERR-031: Two independent genesis() calls produce identical stores.
    ///
    /// Genesis determinism: the 19 axiomatic meta-schema attributes, empty
    /// datom set, epoch 0, and genesis agent are all fixed. Any deviation
    /// between two calls indicates non-determinism in schema construction.
    ///
    /// The proptest input is a dummy seed — we want 10,000 repetitions to
    /// stress any source of non-determinism (hashmap iteration order,
    /// thread-local state, etc.).
    ///
    /// Falsification: two genesis() calls produce stores that differ in
    /// schema, epoch, datom set, or genesis agent.
    #[test]
    fn inv_ferr_031_genesis_determinism(
        _seed in any::<u64>(),
    ) {
        let g1 = Store::genesis();
        let g2 = Store::genesis();

        // Datom sets must be identical (both empty).
        prop_assert_eq!(
            g1.datom_set(), g2.datom_set(),
            "INV-FERR-031 violated: genesis datom sets differ"
        );

        // Epoch must be identical (both 0).
        prop_assert_eq!(
            g1.epoch(), g2.epoch(),
            "INV-FERR-031 violated: genesis epochs differ. g1={}, g2={}",
            g1.epoch(), g2.epoch()
        );

        // Schema must have identical attributes.
        prop_assert_eq!(
            g1.schema().len(), g2.schema().len(),
            "INV-FERR-031 violated: genesis schema lengths differ. g1={}, g2={}",
            g1.schema().len(), g2.schema().len()
        );

        // Each attribute in g1 must exist with the same definition in g2.
        for (attr, def) in g1.schema().iter() {
            let g2_def = g2.schema().get(attr);
            prop_assert!(
                g2_def.is_some(),
                "INV-FERR-031 violated: attribute {:?} in g1 but not g2",
                attr
            );
            prop_assert_eq!(
                def, g2_def.expect("already checked"),
                "INV-FERR-031 violated: attribute {:?} has different definition",
                attr
            );
        }

        // Genesis agent must be identical.
        prop_assert_eq!(
            g1.genesis_agent(), g2.genesis_agent(),
            "INV-FERR-031 violated: genesis agents differ"
        );
    }
}

/// Build a store with a single assert datom at the given tx epoch.
fn store_with_assert(
    entity: EntityId,
    attribute: &ferratom::Attribute,
    value: &ferratom::Value,
    tx_epoch: u64,
) -> Store {
    Store::from_datoms(BTreeSet::from([Datom::new(
        entity,
        attribute.clone(),
        value.clone(),
        ferratom::TxId::new(tx_epoch, 0, 0),
        ferratom::Op::Assert,
    )]))
}

/// Build a store with an assert at one epoch and a retract at another.
fn store_with_assert_and_retract(
    entity: EntityId,
    attribute: &ferratom::Attribute,
    value: &ferratom::Value,
    assert_epoch: u64,
    retract_epoch: u64,
) -> Store {
    Store::from_datoms(BTreeSet::from([
        Datom::new(
            entity,
            attribute.clone(),
            value.clone(),
            ferratom::TxId::new(assert_epoch, 0, 0),
            ferratom::Op::Assert,
        ),
        Datom::new(
            entity,
            attribute.clone(),
            value.clone(),
            ferratom::TxId::new(retract_epoch, 0, 0),
            ferratom::Op::Retract,
        ),
    ]))
}

/// Check whether a value is live after merging two stores.
fn is_value_live_after_merge(
    a: &Store,
    b: &Store,
    entity: EntityId,
    attribute: &ferratom::Attribute,
    value: &ferratom::Value,
) -> bool {
    let merged = merge(a, b).expect("merge must succeed");
    merged
        .live_values(entity, attribute)
        .is_some_and(|vals| vals.contains(value))
}

/// INV-FERR-029: merge_causal homomorphism -- adversarial cross-store retraction.
///
/// Verifies that merge_causal produces correct LIVE resolution when one store
/// asserts a value and another store retracts it. This is the exact scenario
/// that proved the naive LIVE set union incorrect (bd-glir).
#[test]
fn inv_ferr_029_merge_causal_cross_retraction() {
    let entity = EntityId::from_content(b"cross-retract-e1");
    let attribute = ferratom::Attribute::from("user/name");
    let value = ferratom::Value::String("contested".into());

    // Case 1: B's retract (tx=20) > A's assert (tx=10) -- value NOT live.
    let a1 = store_with_assert(entity, &attribute, &value, 10);
    let b1 = store_with_assert_and_retract(entity, &attribute, &value, 5, 20);
    assert!(
        !is_value_live_after_merge(&a1, &b1, entity, &attribute, &value),
        "INV-FERR-029: retract at tx=20 must override assert at tx=10"
    );

    // Case 2: A's assert (tx=30) > B's retract (tx=20) -- value IS live.
    let a2 = store_with_assert(entity, &attribute, &value, 30);
    let b2 = store_with_assert_and_retract(entity, &attribute, &value, 5, 20);
    assert!(
        is_value_live_after_merge(&a2, &b2, entity, &attribute, &value),
        "INV-FERR-029: assert at tx=30 must override retract at tx=20"
    );

    // Case 3: Commutativity holds for both cases.
    let merged1 = merge(&a1, &b1).expect("merge must succeed");
    let merged1_rev = merge(&b1, &a1).expect("reverse merge must succeed");
    assert_eq!(
        merged1.live_values(entity, &attribute),
        merged1_rev.live_values(entity, &attribute),
        "INV-FERR-001/029: cross-retraction merge must be commutative"
    );
    let merged2 = merge(&a2, &b2).expect("merge must succeed");
    let merged2_rev = merge(&b2, &a2).expect("reverse merge must succeed");
    assert_eq!(
        merged2.live_values(entity, &attribute),
        merged2_rev.live_values(entity, &attribute),
        "INV-FERR-001/029: cross-retraction merge must be commutative"
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(10_000))]

    /// ME-017 / INV-FERR-043: Merge of stores with conflicting attribute
    /// definitions is deterministic and commutative.
    ///
    /// Generates two stores where the same attribute name has different
    /// `ValueType` definitions. Merge must:
    /// 1. Not panic or return Err.
    /// 2. Be commutative: merge(A,B).schema == merge(B,A).schema.
    /// 3. Produce a deterministic result (the definition that sorts first wins).
    ///
    /// Falsification: merge(A,B).schema != merge(B,A).schema for conflicting schemas.
    #[test]
    fn inv_ferr_043_schema_conflict_merge_commutativity(
        seed in any::<u64>(),
    ) {

        use ferratom::{AgentId, Attribute, Value};

        // Build two genesis stores with divergent schema definitions.
        let mut a = Store::genesis();
        let mut b = Store::genesis();

        // Store A: define "user/email" as String
        let tx_a = ferratomic_db::writer::Transaction::new(AgentId::from_bytes([1u8; 16]))
            .assert_datom(
                EntityId::from_content(format!("attr-email-{seed}").as_bytes()),
                Attribute::from("db/ident"),
                Value::Keyword("user/email".into()),
            )
            .assert_datom(
                EntityId::from_content(format!("attr-email-{seed}").as_bytes()),
                Attribute::from("db/valueType"),
                Value::Keyword("db.type/string".into()),
            )
            .assert_datom(
                EntityId::from_content(format!("attr-email-{seed}").as_bytes()),
                Attribute::from("db/cardinality"),
                Value::Keyword("db.cardinality/one".into()),
            )
            .commit(a.schema())
            .expect("valid schema tx a");
        a.transact_test(tx_a).expect("transact a ok");

        // Store B: define "user/email" as Keyword (conflicting type!)
        let tx_b = ferratomic_db::writer::Transaction::new(AgentId::from_bytes([2u8; 16]))
            .assert_datom(
                EntityId::from_content(format!("attr-email-b-{seed}").as_bytes()),
                Attribute::from("db/ident"),
                Value::Keyword("user/email".into()),
            )
            .assert_datom(
                EntityId::from_content(format!("attr-email-b-{seed}").as_bytes()),
                Attribute::from("db/valueType"),
                Value::Keyword("db.type/keyword".into()),
            )
            .assert_datom(
                EntityId::from_content(format!("attr-email-b-{seed}").as_bytes()),
                Attribute::from("db/cardinality"),
                Value::Keyword("db.cardinality/one".into()),
            )
            .commit(b.schema())
            .expect("valid schema tx b");
        b.transact_test(tx_b).expect("transact b ok");

        // Both stores define "user/email" with different types.
        prop_assert!(
            a.schema().get(&Attribute::from("user/email")).is_some(),
            "store A must have user/email"
        );
        prop_assert!(
            b.schema().get(&Attribute::from("user/email")).is_some(),
            "store B must have user/email"
        );

        // Merge must succeed (not panic, not Err).
        let ab = merge(&a, &b).expect("merge(A,B) must not fail");
        let ba = merge(&b, &a).expect("merge(B,A) must not fail");

        // INV-FERR-001: merge commutativity extends to schema.
        prop_assert_eq!(
            ab.schema().len(),
            ba.schema().len(),
            "INV-FERR-043: merge(A,B).schema.len() must equal merge(B,A).schema.len()"
        );

        // The resolved definition must be identical regardless of merge order.
        let ab_email = ab.schema().get(&Attribute::from("user/email"));
        let ba_email = ba.schema().get(&Attribute::from("user/email"));
        prop_assert_eq!(
            ab_email,
            ba_email,
            "INV-FERR-043: conflicting schema resolution must be commutative. \
             merge(A,B)={:?}, merge(B,A)={:?}",
            ab_email,
            ba_email
        );

        // INV-FERR-001: datom set commutativity holds even with schema conflicts.
        prop_assert_eq!(
            ab.datom_set(),
            ba.datom_set(),
            "INV-FERR-001: datom sets must be identical regardless of merge order"
        );
    }

    // -----------------------------------------------------------------
    // Overlap merge tests (bd-vd5d)
    // -----------------------------------------------------------------

    /// INV-FERR-001..004: merge of stores with controlled overlap must
    /// satisfy all CRDT properties. The overlap guarantees non-empty
    /// intersection, exercising merge dedup and LIVE resolution.
    #[test]
    fn inv_ferr_001_004_merge_with_overlap(
        (a, b) in arb_store_with_overlap(50, 0.3),
    ) {
        // Commutativity (INV-FERR-001).
        let ab = merge(&a, &b).expect("merge(A,B) must succeed");
        let ba = merge(&b, &a).expect("merge(B,A) must succeed");
        prop_assert_eq!(
            ab.datom_set(),
            ba.datom_set(),
            "INV-FERR-001: merge commutativity with overlap"
        );

        // Idempotency (INV-FERR-003).
        let ab_ab = merge(&ab, &ab).expect("self-merge must succeed");
        prop_assert_eq!(
            ab.datom_set(),
            ab_ab.datom_set(),
            "INV-FERR-003: merge idempotency with overlap"
        );

        // Monotonic growth (INV-FERR-004): merged set is superset of both inputs.
        let merged_set = ab.datom_set();
        for d in a.datoms() {
            prop_assert!(
                merged_set.contains(d),
                "INV-FERR-004: merged store must contain all datoms from A"
            );
        }
        for d in b.datoms() {
            prop_assert!(
                merged_set.contains(d),
                "INV-FERR-004: merged store must contain all datoms from B"
            );
        }
    }
}
