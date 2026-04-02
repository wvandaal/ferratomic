//! Positional content addressing property tests (INV-FERR-076).
//!
//! Verifies that `PositionalStore` produces identical results to the
//! `OrdSet`/`OrdMap`-based `Store` for all operations. Tests the five
//! acceptance criteria from the session 007 execution plan.

use std::collections::BTreeSet;

use ferratom::Datom;
use ferratomic_core::{
    indexes::{AevtKey, AvetKey, EavtKey, VaetKey},
    merge::merge,
    positional::{merge_positional, PositionalStore},
    store::Store,
};
use ferratomic_verify::generators::*;
use proptest::prelude::*;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(10_000))]

    /// INV-FERR-076 acceptance #1: `PositionalStore.datoms()` == `Store.datoms()`.
    ///
    /// Both must contain the same datom set when built from the same input.
    ///
    /// Falsification: a datom present in one but missing from the other.
    #[test]
    fn inv_ferr_076_datoms_match_store(
        datoms in prop::collection::btree_set(arb_datom(), 0..200),
    ) {
        let store = Store::from_datoms(datoms.clone());
        let ps = PositionalStore::from_datoms(datoms.into_iter());

        let store_datoms: BTreeSet<&Datom> = store.datoms().collect();
        let ps_datoms: BTreeSet<&Datom> = ps.datoms().iter().collect();

        prop_assert_eq!(
            store_datoms, ps_datoms,
            "INV-FERR-076: PositionalStore datoms differ from Store. \
             store={}, positional={}",
            store.len(), ps.len()
        );
    }

    /// INV-FERR-076 acceptance #2: LIVE view equivalence.
    ///
    /// `PositionalStore.live_datoms()` must produce the same set of live
    /// `(entity, attribute, value)` triples as `Store.live_values()`.
    ///
    /// Falsification: a triple is live in one but dead in the other.
    #[test]
    fn inv_ferr_076_live_view_matches_store(
        datoms in prop::collection::btree_set(arb_datom(), 0..200),
    ) {
        let store = Store::from_datoms(datoms.clone());
        let ps = PositionalStore::from_datoms(datoms.into_iter());

        // Collect live (entity, attribute, value) triples from PositionalStore.
        let ps_live: BTreeSet<_> = ps.live_datoms()
            .map(|d| (d.entity(), d.attribute().clone(), d.value().clone()))
            .collect();

        // Collect live triples from Store via live_values per (e,a).
        let mut store_live: BTreeSet<_> = BTreeSet::new();
        for d in store.datoms() {
            if let Some(values) = store.live_values(d.entity(), d.attribute()) {
                for v in values.iter() {
                    store_live.insert((
                        d.entity(),
                        d.attribute().clone(),
                        v.clone(),
                    ));
                }
            }
        }

        let ps_count = ps_live.len();
        let store_count = store_live.len();
        prop_assert_eq!(
            ps_live, store_live,
            "INV-FERR-076: LIVE view differs. positional={}, store={}",
            ps_count, store_count
        );
    }

    /// INV-FERR-076 acceptance #3: `merge_positional` == `merge`.
    ///
    /// Merging two `PositionalStore`s must produce the same datom set
    /// as merging two `Store`s.
    ///
    /// Falsification: merged datom sets differ.
    #[test]
    fn inv_ferr_076_merge_matches_store(
        a_datoms in prop::collection::btree_set(arb_datom(), 0..100),
        b_datoms in prop::collection::btree_set(arb_datom(), 0..100),
    ) {
        let store_a = Store::from_datoms(a_datoms.clone());
        let store_b = Store::from_datoms(b_datoms.clone());
        let store_merged = merge(&store_a, &store_b)
            .expect("INV-FERR-076: merge must succeed");

        let ps_a = PositionalStore::from_datoms(a_datoms.into_iter());
        let ps_b = PositionalStore::from_datoms(b_datoms.into_iter());
        let ps_merged = merge_positional(&ps_a, &ps_b);

        let store_set: BTreeSet<&Datom> = store_merged.datoms().collect();
        let ps_set: BTreeSet<&Datom> = ps_merged.datoms().iter().collect();

        prop_assert_eq!(
            store_set, ps_set,
            "INV-FERR-076: merge_positional differs from Store::merge. \
             store={}, positional={}",
            store_merged.len(), ps_merged.len()
        );

        // INV-FERR-001: merge commutativity — direct test on PositionalStore.
        let ps_ba = merge_positional(&ps_b, &ps_a);
        prop_assert_eq!(
            ps_merged.datoms(), ps_ba.datoms(),
            "INV-FERR-001: merge_positional(a,b) != merge_positional(b,a)"
        );
    }

    /// INV-FERR-076 acceptance #4: LIVE bitvector length == canonical length.
    ///
    /// Falsification: bitvector has different length than canonical array.
    #[test]
    fn inv_ferr_076_live_bits_length(
        datoms in prop::collection::btree_set(arb_datom(), 0..200),
    ) {
        let ps = PositionalStore::from_datoms(datoms.into_iter());
        prop_assert_eq!(
            ps.live_bits_len(), ps.len(),
            "INV-FERR-076: LIVE bitvector length {} != canonical length {}",
            ps.live_bits_len(), ps.len()
        );
    }

    /// INV-FERR-076 acceptance #5: permutation arrays are valid.
    ///
    /// Each permutation must contain exactly `[0, n)` in some order.
    ///
    /// Falsification: duplicate positions, missing positions, or out-of-range.
    #[test]
    fn inv_ferr_076_permutations_valid(
        datoms in prop::collection::btree_set(arb_datom(), 0..200),
    ) {
        let ps = PositionalStore::from_datoms(datoms.into_iter());
        let n = ps.len();

        for (name, perm) in [
            ("AEVT", ps.perm_aevt()),
            ("VAET", ps.perm_vaet()),
            ("AVET", ps.perm_avet()),
        ] {
            prop_assert_eq!(
                perm.len(), n,
                "INV-FERR-076: {} permutation length {} != canonical length {}",
                name, perm.len(), n
            );
            let mut sorted: Vec<u32> = perm.to_vec();
            sorted.sort_unstable();
            let expected: Vec<u32> = (0..n)
                .map(|i| u32::try_from(i).unwrap_or(u32::MAX))
                .collect();
            prop_assert_eq!(
                sorted, expected,
                "INV-FERR-076: {} permutation is not a valid permutation of [0, {})",
                name, n
            );
        }
    }

    /// INV-FERR-076: positional determinism.
    ///
    /// Two constructions from the same datom set produce identical positions.
    ///
    /// Falsification: same input → different canonical arrays.
    #[test]
    fn inv_ferr_076_positional_determinism(
        datoms in prop::collection::btree_set(arb_datom(), 0..200),
    ) {
        let ps_a = PositionalStore::from_datoms(datoms.iter().cloned());
        let ps_b = PositionalStore::from_datoms(datoms.into_iter());

        prop_assert_eq!(
            ps_a.datoms(), ps_b.datoms(),
            "INV-FERR-076: positional determinism violated"
        );
        // Every datom is findable at its canonical position.
        for (p, d) in ps_a.datoms().iter().enumerate() {
            let pos = ps_a.position_of(d);
            let expected = u32::try_from(p).ok();
            prop_assert_eq!(
                pos, expected,
                "INV-FERR-076: position_of mismatch at index {}",
                p
            );
        }
    }

    /// INV-FERR-076: all four index lookups find every datom.
    ///
    /// Every datom must be findable via EAVT (canonical binary search)
    /// and AEVT, VAET, AVET (permuted binary search).
    ///
    /// Falsification: any lookup returns None or wrong datom.
    #[test]
    fn inv_ferr_076_all_index_lookups(
        datoms in prop::collection::btree_set(arb_datom(), 1..100),
    ) {
        let ps = PositionalStore::from_datoms(datoms.into_iter());

        for d in ps.datoms() {
            // EAVT: canonical binary search.
            let eavt = ps.eavt_get(&EavtKey::from_datom(d));
            prop_assert_eq!(
                eavt, Some(d),
                "INV-FERR-076: eavt_get failed for datom {:?}", d.entity()
            );
            // AEVT: permuted binary search.
            let aevt = ps.aevt_get(&AevtKey::from_datom(d));
            prop_assert_eq!(
                aevt, Some(d),
                "INV-FERR-076: aevt_get failed for datom {:?}", d.entity()
            );
            // VAET: permuted binary search.
            let vaet = ps.vaet_get(&VaetKey::from_datom(d));
            prop_assert_eq!(
                vaet, Some(d),
                "INV-FERR-076: vaet_get failed for datom {:?}", d.entity()
            );
            // AVET: permuted binary search.
            let avet = ps.avet_get(&AvetKey::from_datom(d));
            prop_assert_eq!(
                avet, Some(d),
                "INV-FERR-076: avet_get failed for datom {:?}", d.entity()
            );
        }
    }
}
