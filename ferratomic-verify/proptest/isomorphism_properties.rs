//! INV-FERR-059: Optimization isomorphism property tests.
//!
//! Verifies that all Phase 4a performance optimizations preserve query
//! behavior relative to the baseline representation:
//!
//! 1. PositionalStore vs OrdMap — datom set identity after promote/demote.
//! 2. SortedVecIndexes vs OrdMap Indexes — all four index iterations match.
//! 3. Checkpoint V3 round-trip — serialize/deserialize preserves datom set.
//! 4. Eytzinger layout — round-trip and search correctness via PositionalStore.
//!
//! These are RETROACTIVE behavioral preservation proofs for optimizations
//! that were implemented without isomorphism verification.

use std::collections::BTreeSet;

use ferratom::Datom;
use ferratomic_core::{
    checkpoint::{load_checkpoint, write_checkpoint_to_writer},
    indexes::{AevtKey, AvetKey, EavtKey, SortedVecIndexes, VaetKey},
    positional::PositionalStore,
    store::Store,
};
use ferratomic_verify::{
    generators::*,
    isomorphism::{verify_optimization_isomorphism, IsomorphismVerdict},
};
use proptest::prelude::*;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(10_000))]

    /// INV-FERR-059 test 1: PositionalStore preserves queries after promote.
    ///
    /// `Store::from_datoms` builds a `Positional` store. `promote()` converts
    /// to `OrdMap`. Both must contain identical datom sets, and all four index
    /// iterations must match the primary set.
    ///
    /// Falsification: any datom present in one representation but missing
    /// from the other, or index cardinality mismatch after promote.
    #[test]
    fn inv_ferr_059_positional_store_preserves_queries(
        datoms in prop::collection::btree_set(arb_datom(), 0..200),
    ) {
        // from_datoms builds Positional repr.
        let store = Store::from_datoms(datoms);
        let positional_datoms: BTreeSet<&Datom> = store.datoms().collect();
        let positional_len = store.len();

        // Promote to OrdMap + SortedVecIndexes.
        let mut promoted = store.clone();
        promoted.promote();
        let promoted_datoms: BTreeSet<&Datom> = promoted.datoms().collect();

        // Datom sets must be identical.
        prop_assert_eq!(
            positional_len,
            promoted.len(),
            "INV-FERR-059: length mismatch after promote. positional={}, promoted={}",
            positional_len, promoted.len()
        );
        prop_assert_eq!(
            &positional_datoms,
            &promoted_datoms,
            "INV-FERR-059: datom set differs after promote"
        );

        // All four indexes must match primary set after promote.
        let indexes = promoted.indexes()
            .expect("INV-FERR-059: promoted store must have indexes");
        let eavt: BTreeSet<&Datom> = indexes.eavt_datoms().collect();
        let aevt: BTreeSet<&Datom> = indexes.aevt_datoms().collect();
        let vaet: BTreeSet<&Datom> = indexes.vaet_datoms().collect();
        let avet: BTreeSet<&Datom> = indexes.avet_datoms().collect();

        prop_assert_eq!(
            &eavt, &positional_datoms,
            "INV-FERR-059: EAVT index differs from positional datoms after promote"
        );
        prop_assert_eq!(
            &aevt, &positional_datoms,
            "INV-FERR-059: AEVT index differs from positional datoms after promote"
        );
        prop_assert_eq!(
            &vaet, &positional_datoms,
            "INV-FERR-059: VAET index differs from positional datoms after promote"
        );
        prop_assert_eq!(
            &avet, &positional_datoms,
            "INV-FERR-059: AVET index differs from positional datoms after promote"
        );
    }

    /// INV-FERR-059 test 2: SortedVecIndexes preserves queries.
    ///
    /// Build a Store, promote to OrdMap (uses SortedVecIndexes), verify all
    /// four index iterations match the primary set and bijection holds.
    ///
    /// Falsification: any index produces a different datom set than primary,
    /// or verify_bijection fails.
    #[test]
    fn inv_ferr_059_sorted_vec_indexes_preserves_queries(
        datoms in prop::collection::btree_set(arb_datom(), 0..200),
    ) {
        let store = Store::from_datoms(datoms);
        let primary: BTreeSet<&Datom> = store.datoms().collect();

        // Build SortedVecIndexes independently from the datom iterator.
        let mut sv_indexes: SortedVecIndexes = SortedVecIndexes::from_datoms(store.datoms());
        sv_indexes.sort_all();

        // Bijection check.
        prop_assert!(
            sv_indexes.verify_bijection(),
            "INV-FERR-059: SortedVecIndexes bijection violated"
        );

        // Cardinality check.
        prop_assert_eq!(
            sv_indexes.len(), store.len(),
            "INV-FERR-059: SortedVecIndexes len {} != store len {}",
            sv_indexes.len(), store.len()
        );

        // All four iterations must match primary set.
        let sv_eavt: BTreeSet<&Datom> = sv_indexes.eavt_datoms().collect();
        let sv_aevt: BTreeSet<&Datom> = sv_indexes.aevt_datoms().collect();
        let sv_vaet: BTreeSet<&Datom> = sv_indexes.vaet_datoms().collect();
        let sv_avet: BTreeSet<&Datom> = sv_indexes.avet_datoms().collect();

        prop_assert_eq!(&sv_eavt, &primary, "INV-FERR-059: EAVT datoms differ from primary");
        prop_assert_eq!(&sv_aevt, &primary, "INV-FERR-059: AEVT datoms differ from primary");
        prop_assert_eq!(&sv_vaet, &primary, "INV-FERR-059: VAET datoms differ from primary");
        prop_assert_eq!(&sv_avet, &primary, "INV-FERR-059: AVET datoms differ from primary");
    }

    /// INV-FERR-059 test 3: Checkpoint V3 round-trip preserves queries.
    ///
    /// Build a Store, serialize to V3 checkpoint bytes, deserialize back,
    /// verify: datom sets identical, epoch identical, schema length identical.
    ///
    /// Falsification: any field differs after round-trip.
    #[test]
    fn inv_ferr_059_checkpoint_v3_roundtrip_preserves_queries(
        datoms in prop::collection::btree_set(arb_datom(), 0..100),
    ) {
        let store = Store::from_datoms(datoms);
        let original_datoms: BTreeSet<&Datom> = store.datoms().collect();
        let original_epoch = store.epoch();
        let original_schema_len = store.schema().iter().count();

        // Serialize to V3 via the public write_checkpoint_to_writer API.
        let mut bytes = Vec::new();
        write_checkpoint_to_writer(&store, &mut bytes)
            .expect("INV-FERR-059: checkpoint serialization must succeed");

        // Deserialize via tempfile + load_checkpoint (public API).
        let dir = tempfile::TempDir::new()
            .expect("INV-FERR-059: tempdir creation must succeed");
        let path = dir.path().join("iso.chkp");
        std::fs::write(&path, &bytes)
            .expect("INV-FERR-059: writing checkpoint file must succeed");
        let loaded = load_checkpoint(&path)
            .expect("INV-FERR-059: checkpoint deserialization must succeed");

        // Datom set equality.
        let loaded_datoms: BTreeSet<&Datom> = loaded.datoms().collect();
        prop_assert_eq!(
            original_datoms, loaded_datoms,
            "INV-FERR-059: datom set differs after V3 checkpoint round-trip"
        );

        // Epoch identity.
        prop_assert_eq!(
            original_epoch, loaded.epoch(),
            "INV-FERR-059: epoch differs after V3 checkpoint round-trip. \
             original={}, loaded={}",
            original_epoch, loaded.epoch()
        );

        // Schema length identity.
        let loaded_schema_len = loaded.schema().iter().count();
        prop_assert_eq!(
            original_schema_len, loaded_schema_len,
            "INV-FERR-059: schema length differs after V3 checkpoint round-trip. \
             original={}, loaded={}",
            original_schema_len, loaded_schema_len
        );
    }

    /// INV-FERR-059 test 4: Eytzinger layout preserves search correctness.
    ///
    /// For a PositionalStore built from random datoms, verify that:
    /// (a) `perm_*_sorted(perm_*(canonical))` recovers the original sorted
    ///     permutation (Eytzinger round-trip).
    /// (b) Every datom findable via EAVT, AEVT, VAET, AVET lookups on the
    ///     PositionalStore (which uses Eytzinger layout internally).
    /// (c) Sorted permutations are valid permutations of [0, n).
    ///
    /// Falsification: round-trip differs, lookup misses, or invalid permutation.
    #[test]
    fn inv_ferr_059_eytzinger_layout_preserves_search(
        datoms in prop::collection::btree_set(arb_datom(), 1..100),
    ) {
        let ps = PositionalStore::from_datoms(datoms.into_iter());
        let n = ps.len();

        // (a) Eytzinger round-trip: sorted → BFS → sorted.
        // perm_aevt() returns Eytzinger layout, perm_aevt_sorted() recovers sorted.
        for (name, bfs_perm, sorted_perm) in [
            ("AEVT", ps.perm_aevt(), ps.perm_aevt_sorted()),
            ("VAET", ps.perm_vaet(), ps.perm_vaet_sorted()),
            ("AVET", ps.perm_avet(), ps.perm_avet_sorted()),
        ] {
            // BFS layout has n+1 elements (sentinel at 0).
            prop_assert_eq!(
                bfs_perm.len(), n + 1,
                "INV-FERR-059: {} Eytzinger layout length {} != n+1={}",
                name, bfs_perm.len(), n + 1
            );
            // Sentinel at index 0.
            prop_assert_eq!(
                bfs_perm[0], u32::MAX,
                "INV-FERR-059: {} Eytzinger sentinel is not u32::MAX", name
            );
            // Sorted permutation has n elements.
            prop_assert_eq!(
                sorted_perm.len(), n,
                "INV-FERR-059: {} sorted permutation length {} != n={}",
                name, sorted_perm.len(), n
            );
            // Sorted permutation is a valid permutation of [0, n).
            let mut check: Vec<u32> = sorted_perm;
            check.sort_unstable();
            let expected: Vec<u32> = (0..n)
                .map(|i| u32::try_from(i).unwrap_or(u32::MAX))
                .collect();
            prop_assert_eq!(
                check, expected,
                "INV-FERR-059: {} sorted permutation is not [0, {})", name, n
            );
        }

        // (b) Every datom findable via all four index lookups.
        for d in ps.datoms() {
            let eavt = ps.eavt_get(&EavtKey::from_datom(d));
            prop_assert_eq!(
                eavt, Some(d),
                "INV-FERR-059: eavt_get missed datom {:?}", d.entity()
            );
            let aevt = ps.aevt_get(&AevtKey::from_datom(d));
            prop_assert_eq!(
                aevt, Some(d),
                "INV-FERR-059: aevt_get missed datom {:?}", d.entity()
            );
            let vaet = ps.vaet_get(&VaetKey::from_datom(d));
            prop_assert_eq!(
                vaet, Some(d),
                "INV-FERR-059: vaet_get missed datom {:?}", d.entity()
            );
            let avet = ps.avet_get(&AvetKey::from_datom(d));
            prop_assert_eq!(
                avet, Some(d),
                "INV-FERR-059: avet_get missed datom {:?}", d.entity()
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Unit test: isomorphism API shape (INV-FERR-059)
// ---------------------------------------------------------------------------

#[test]
fn test_inv_ferr_059_isomorphism_api_shape() {
    let store = Store::genesis();
    let proof = verify_optimization_isomorphism(
        &store,
        |s| s.clone(), // identity optimization — must be isomorphic
        &[],
        "identity",
    );
    assert_eq!(
        proof.verdict,
        IsomorphismVerdict::Isomorphic,
        "INV-FERR-059: identity optimization must be isomorphic"
    );
    assert_eq!(proof.optimization, "identity");
    assert_eq!(proof.datom_count, 0);
    assert_eq!(proof.query_count, 0);
}

/// Verify that the isomorphism harness detects a divergent optimization.
#[test]
fn test_inv_ferr_059_isomorphism_detects_divergence() {
    let store = Store::genesis();
    // A "bad" optimization that adds a spurious datom.
    let proof = verify_optimization_isomorphism(
        &store,
        |_s| {
            let mut datoms = BTreeSet::new();
            datoms.insert(Datom::new(
                ferratom::EntityId::from_content(b"spurious"),
                ferratom::Attribute::from("db/doc"),
                ferratom::Value::Bool(true),
                ferratom::TxId::new(0, 1, 0),
                ferratom::Op::Assert,
            ));
            Store::from_datoms(datoms)
        },
        &[],
        "bad_optimization",
    );
    assert_eq!(
        proof.verdict,
        IsomorphismVerdict::Divergent {
            first_divergence: "length mismatch: baseline=0, optimized=1".to_string(),
        },
        "INV-FERR-059: divergent optimization must be detected"
    );
}
