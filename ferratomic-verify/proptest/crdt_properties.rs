//! CRDT algebraic property tests for Ferratomic.
//!
//! Tests INV-FERR-001 (commutativity), INV-FERR-002 (associativity),
//! INV-FERR-003 (idempotency), INV-FERR-004 (monotonic growth),
//! INV-FERR-010 (convergence), INV-FERR-012 (content-addressed identity).
//!
//! ALL TESTS MUST FAIL (red phase). Types are not yet implemented.

use ferratom::{Datom, EntityId};
use ferratomic_core::merge::merge;
use ferratomic_core::store::Store;
use ferratomic_verify::generators::*;
use proptest::prelude::*;
use std::collections::BTreeSet;

/// Verify index bijection: primary set == each secondary index set.
fn verify_index_bijection(store: &Store) -> bool {
    let primary: BTreeSet<&Datom> = store.datoms().collect();
    let eavt: BTreeSet<&Datom> = store.indexes().eavt().iter().collect();
    let aevt: BTreeSet<&Datom> = store.indexes().aevt().iter().collect();
    let vaet: BTreeSet<&Datom> = store.indexes().vaet().iter().collect();
    let avet: BTreeSet<&Datom> = store.indexes().avet().iter().collect();

    primary == eavt && primary == aevt && primary == vaet && primary == avet
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
        let ab = merge(&a, &b);
        let ba = merge(&b, &a);

        prop_assert_eq!(
            ab.datom_set(),
            ba.datom_set(),
            "INV-FERR-001 violated: merge(A,B) != merge(B,A). \
             |A|={}, |B|={}, |A∪B|={}, |B∪A|={}",
            a.len(), b.len(), ab.len(), ba.len()
        );
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
        let ab_c = merge(&merge(&a, &b), &c);
        let a_bc = merge(&a, &merge(&b, &c));

        prop_assert_eq!(
            ab_c.datom_set(),
            a_bc.datom_set(),
            "INV-FERR-002 violated: merge(merge(A,B),C) != merge(A,merge(B,C)). \
             |A|={}, |B|={}, |C|={}",
            a.len(), b.len(), c.len()
        );
    }

    /// INV-FERR-003: merge(A, A) == A for all stores.
    ///
    /// Falsification: a store A where merge(A, A) differs from A in
    /// datom set, cardinality, or index state. Would indicate side effects.
    #[test]
    fn inv_ferr_003_merge_idempotency(
        store in arb_store(100),
    ) {
        let merged = merge(&store, &store);

        prop_assert_eq!(
            store.datom_set(),
            merged.datom_set(),
            "INV-FERR-003 violated: merge(A,A) != A. |A|={}, |merge(A,A)|={}",
            store.len(), merged.len()
        );
        prop_assert_eq!(
            store.len(),
            merged.len(),
            "INV-FERR-003 violated: cardinality changed. |A|={}, |merge(A,A)|={}",
            store.len(), merged.len()
        );
        // Also verify index state is preserved (spec falsification includes "different index state")
        prop_assert!(
            verify_index_bijection(&merged),
            "INV-FERR-003 violated: index bijection broken after self-merge. |A|={}",
            store.len()
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
        let pre_datoms = initial.datom_set().clone();
        let pre_len = initial.len();

        let mut store = initial;
        let _receipt = store.transact(tx)
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
        let merged = merge(&a, &b);

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

        for d in a.datom_set() {
            prop_assert!(
                merged.datom_set().contains(d),
                "INV-FERR-004 violated: datom from A missing in merge result"
            );
        }
        for d in b.datom_set() {
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
            r1.insert(d.clone());
        }

        let mut shuffled = datoms.clone();
        let mut rng = StdRng::seed_from_u64(perm_seed);
        shuffled.shuffle(&mut rng);
        for d in &shuffled {
            r2.insert(d.clone());
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

        let merged = merge(&a, &b);

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
}
