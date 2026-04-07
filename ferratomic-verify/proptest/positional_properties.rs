//! Positional content addressing property tests (INV-FERR-076).
//!
//! Verifies that `PositionalStore` produces identical results to the
//! `OrdSet`/`OrdMap`-based `Store` for all operations. Tests the five
//! acceptance criteria from the session 007 execution plan.

use std::collections::{BTreeSet, HashSet};

use ferratom::{Datom, EntityId};
use ferratomic_db::{
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

        // bd-j7qk: perm_*() returns Eytzinger (BFS) layout with n+1 elements
        // (sentinel at index 0). Use perm_*_sorted() to recover the original
        // sorted permutation for validation.
        for (name, sorted_perm) in [
            ("AEVT", ps.perm_aevt_sorted()),
            ("VAET", ps.perm_vaet_sorted()),
            ("AVET", ps.perm_avet_sorted()),
        ] {
            prop_assert_eq!(
                sorted_perm.len(), n,
                "INV-FERR-076: {} sorted permutation length {} != canonical length {}",
                name, sorted_perm.len(), n
            );
            let mut check: Vec<u32> = sorted_perm;
            check.sort_unstable();
            let expected: Vec<u32> = (0..n)
                .map(|i| u32::try_from(i).unwrap_or(u32::MAX))
                .collect();
            prop_assert_eq!(
                check, expected,
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

    // -----------------------------------------------------------------------
    // MPH entity lookup properties (contributes to INV-FERR-027)
    // -----------------------------------------------------------------------

    /// INV-FERR-027 / INV-FERR-076: entity_lookup finds every entity.
    ///
    /// For every unique entity in the store, `entity_lookup` must return
    /// `Some(position)` where the datom at that position has the queried entity.
    ///
    /// Falsification: an entity present in the canonical array is not found
    /// by `entity_lookup` (false negative).
    #[test]
    fn inv_ferr_027_entity_lookup_completeness(
        datoms in prop::collection::btree_set(arb_datom(), 1..200),
    ) {
        let ps = PositionalStore::from_datoms(datoms.into_iter());
        let unique = ps.unique_entity_ids();

        for eid in &unique {
            let pos = ps.entity_lookup(eid);
            prop_assert!(
                pos.is_some(),
                "INV-FERR-027: entity_lookup returned None for entity present in store"
            );
            let p = pos.expect("just asserted Some") as usize;
            prop_assert_eq!(
                ps.datom_at(p as u32).map(|d| d.entity()),
                Some(*eid),
                "INV-FERR-027: entity_lookup returned wrong position for entity"
            );
        }
    }

    /// INV-FERR-027 / INV-FERR-076: entity_lookup returns first position.
    ///
    /// The returned position must be the FIRST canonical position for the
    /// entity (smallest index where `canonical[i].entity() == eid`).
    ///
    /// Falsification: `entity_lookup` returns a position that is not the
    /// first occurrence of the entity in canonical order.
    #[test]
    fn inv_ferr_027_entity_lookup_first_position(
        datoms in prop::collection::btree_set(arb_datom(), 1..200),
    ) {
        let ps = PositionalStore::from_datoms(datoms.into_iter());

        for eid in &ps.unique_entity_ids() {
            if let Some(pos) = ps.entity_lookup(eid) {
                let p = pos as usize;
                // Must be in range.
                prop_assert!(p < ps.len());
                // Must have the correct entity.
                prop_assert_eq!(ps.datom_at(pos).map(|d| d.entity()), Some(*eid));
                // Must be the FIRST: no earlier position has this entity.
                if p > 0 {
                    prop_assert_ne!(
                        ps.datom_at(pos - 1).map(|d| d.entity()),
                        Some(*eid),
                        "INV-FERR-027: entity_lookup did not return the first position"
                    );
                }
            }
        }
    }

    /// INV-FERR-027: entity_lookup rejects absent entities.
    ///
    /// An `EntityId` not present in the canonical array must return `None`.
    ///
    /// Falsification: `entity_lookup` returns `Some` for a non-existent entity.
    #[test]
    fn inv_ferr_027_entity_lookup_absent_rejected(
        datoms in prop::collection::btree_set(arb_datom(), 0..100),
        absent_bytes in any::<[u8; 32]>(),
    ) {
        let ps = PositionalStore::from_datoms(datoms.into_iter());
        let absent = EntityId::from_bytes(absent_bytes);

        // bd-zp8g: Use HashSet for O(1) entity presence check instead of
        // linear scan via `.any()`.
        let entity_set: HashSet<EntityId> = ps.datoms().iter().map(|d| d.entity()).collect();
        if !entity_set.contains(&absent) {
            prop_assert_eq!(
                ps.entity_lookup(&absent),
                None,
                "INV-FERR-027: entity_lookup returned Some for absent entity"
            );
        }
    }

    /// INV-FERR-076: unique_entity_ids is sorted and unique.
    ///
    /// The result must be strictly sorted (no duplicates) and contain exactly
    /// the distinct entities from the canonical array.
    ///
    /// Falsification: unsorted, contains duplicates, or missing/extra entities.
    #[test]
    fn inv_ferr_076_unique_entity_ids_correct(
        datoms in prop::collection::btree_set(arb_datom(), 0..200),
    ) {
        let ps = PositionalStore::from_datoms(datoms.into_iter());
        let unique = ps.unique_entity_ids();

        // Strictly sorted.
        prop_assert!(
            unique.windows(2).all(|w| w[0] < w[1]),
            "INV-FERR-076: unique_entity_ids is not strictly sorted"
        );

        // Matches the distinct entity set from canonical.
        let expected: BTreeSet<EntityId> = ps.datoms().iter().map(|d| d.entity()).collect();
        let actual: BTreeSet<EntityId> = unique.iter().copied().collect();
        prop_assert_eq!(
            actual, expected,
            "INV-FERR-076: unique_entity_ids doesn't match canonical entity set"
        );
    }

    /// INV-FERR-027 / INV-FERR-076: entity_lookup correct after merge.
    ///
    /// After merging two stores, `entity_lookup` must find every entity
    /// in the merged result. The MPH is rebuilt from scratch on the merged
    /// canonical array — this test verifies the rebuild is correct.
    ///
    /// Falsification: an entity present in the merged store is not found
    /// by `entity_lookup` on the merged result.
    #[test]
    fn inv_ferr_027_entity_lookup_after_merge(
        a_datoms in prop::collection::btree_set(arb_datom(), 0..100),
        b_datoms in prop::collection::btree_set(arb_datom(), 0..100),
    ) {
        let ps_a = PositionalStore::from_datoms(a_datoms.into_iter());
        let ps_b = PositionalStore::from_datoms(b_datoms.into_iter());
        let merged = merge_positional(&ps_a, &ps_b);

        for eid in &merged.unique_entity_ids() {
            let pos = merged.entity_lookup(eid);
            prop_assert!(
                pos.is_some(),
                "INV-FERR-027: entity_lookup failed after merge for entity"
            );
            let p = pos.expect("asserted Some");
            prop_assert_eq!(
                merged.datom_at(p).map(ferratom::Datom::entity),
                Some(*eid),
                "INV-FERR-027: entity_lookup returned wrong position after merge"
            );
        }
    }

    // -----------------------------------------------------------------------
    // XOR homomorphic fingerprint properties (INV-FERR-074, bd-83j4)
    // -----------------------------------------------------------------------

    /// INV-FERR-074: fingerprint determinism.
    ///
    /// Two constructions from the same datom set produce identical fingerprints.
    ///
    /// Falsification: same input → different fingerprints.
    #[test]
    fn inv_ferr_074_fingerprint_deterministic(
        datoms in prop::collection::btree_set(arb_datom(), 0..200),
    ) {
        let ps_a = PositionalStore::from_datoms(datoms.iter().cloned());
        let ps_b = PositionalStore::from_datoms(datoms.into_iter());

        prop_assert_eq!(
            ps_a.fingerprint(), ps_b.fingerprint(),
            "INV-FERR-074: fingerprint determinism violated"
        );
    }

    /// INV-FERR-074 + INV-FERR-001: merge fingerprint commutativity.
    ///
    /// `merge(a,b).fingerprint() == merge(b,a).fingerprint()`.
    ///
    /// Falsification: merge order changes fingerprint.
    #[test]
    fn inv_ferr_074_fingerprint_commutative(
        a_datoms in prop::collection::btree_set(arb_datom(), 0..100),
        b_datoms in prop::collection::btree_set(arb_datom(), 0..100),
    ) {
        let ps_a = PositionalStore::from_datoms(a_datoms.into_iter());
        let ps_b = PositionalStore::from_datoms(b_datoms.into_iter());
        let merged_ab = merge_positional(&ps_a, &ps_b);
        let merged_ba = merge_positional(&ps_b, &ps_a);

        prop_assert_eq!(
            merged_ab.fingerprint(), merged_ba.fingerprint(),
            "INV-FERR-074: merge fingerprint not commutative"
        );
    }

    /// INV-FERR-074: homomorphic property over disjoint union.
    ///
    /// For disjoint stores A and B: `H(A ∪ B) = H(A) ⊕ H(B)`.
    /// Disjointness is guaranteed by partitioning a single generated set.
    ///
    /// Falsification: XOR of individual fingerprints differs from merged.
    #[test]
    fn inv_ferr_074_fingerprint_homomorphic_disjoint(
        datoms in prop::collection::btree_set(arb_datom(), 2..100),
    ) {
        let all: Vec<Datom> = datoms.into_iter().collect();
        let mid = all.len() / 2;
        let ps_a = PositionalStore::from_datoms(all[..mid].iter().cloned());
        let ps_b = PositionalStore::from_datoms(all[mid..].iter().cloned());
        let ps_merged = merge_positional(&ps_a, &ps_b);

        let xor_fp = xor_fingerprints(ps_a.fingerprint(), ps_b.fingerprint());
        prop_assert_eq!(
            ps_merged.fingerprint(), &xor_fp,
            "INV-FERR-074: homomorphic property violated for disjoint stores"
        );
    }

    /// INV-FERR-074: non-disjoint merge fingerprint formula.
    ///
    /// For stores A and B with guaranteed non-empty overlap:
    /// `H(A ∪ B) = H(A) ⊕ H(B) ⊕ H(A ∩ B)`.
    /// Overlap is guaranteed by injecting a shared subset into both stores.
    ///
    /// Falsification: the non-disjoint formula produces a different result
    /// than computing the fingerprint of the union directly.
    #[test]
    fn inv_ferr_074_fingerprint_nondisjoint(
        shared in prop::collection::btree_set(arb_datom(), 1..20),
        a_only in prop::collection::btree_set(arb_datom(), 0..30),
        b_only in prop::collection::btree_set(arb_datom(), 0..30),
    ) {
        // A = shared ∪ a_only, B = shared ∪ b_only → A ∩ B ⊇ shared.
        let a_datoms: BTreeSet<_> = shared.union(&a_only).cloned().collect();
        let b_datoms: BTreeSet<_> = shared.union(&b_only).cloned().collect();
        let union_datoms: BTreeSet<_> = a_datoms.union(&b_datoms).cloned().collect();
        let inter_datoms: BTreeSet<_> = a_datoms.intersection(&b_datoms).cloned().collect();

        let ps_a = PositionalStore::from_datoms(a_datoms.into_iter());
        let ps_b = PositionalStore::from_datoms(b_datoms.into_iter());
        let ps_union = PositionalStore::from_datoms(union_datoms.into_iter());
        let ps_inter = PositionalStore::from_datoms(inter_datoms.into_iter());

        // H(A ∪ B) = H(A) ⊕ H(B) ⊕ H(A ∩ B)
        let combined = xor_fingerprints(
            &xor_fingerprints(ps_a.fingerprint(), ps_b.fingerprint()),
            ps_inter.fingerprint(),
        );
        prop_assert_eq!(
            ps_union.fingerprint(), &combined,
            "INV-FERR-074: non-disjoint fingerprint formula violated"
        );
    }

    // -----------------------------------------------------------------------
    // Bloom filter entity_exists properties (INV-FERR-027, bd-218b)
    // -----------------------------------------------------------------------

    /// INV-FERR-027: entity_exists has zero false negatives.
    ///
    /// For every entity present in the store, `entity_exists` must return `true`.
    ///
    /// Falsification: an entity present in the canonical array returns `false`.
    #[test]
    fn inv_ferr_027_entity_exists_no_false_negatives(
        datoms in prop::collection::btree_set(arb_datom(), 1..200),
    ) {
        let ps = PositionalStore::from_datoms(datoms.into_iter());

        for eid in &ps.unique_entity_ids() {
            prop_assert!(
                ps.entity_exists(eid),
                "INV-FERR-027: entity_exists false negative for present entity"
            );
        }
    }

    /// INV-FERR-027: entity_exists rejects absent entities.
    ///
    /// An `EntityId` not in the store must return `false`.
    ///
    /// Falsification: `entity_exists` returns `true` for a non-existent entity.
    #[test]
    fn inv_ferr_027_entity_exists_absent_rejected(
        datoms in prop::collection::btree_set(arb_datom(), 0..100),
        absent_bytes in any::<[u8; 32]>(),
    ) {
        let ps = PositionalStore::from_datoms(datoms.into_iter());
        let absent = EntityId::from_bytes(absent_bytes);

        // bd-zp8g: Use HashSet for O(1) entity presence check instead of
        // linear scan via `.any()`.
        let entity_set: HashSet<EntityId> = ps.datoms().iter().map(|d| d.entity()).collect();
        if !entity_set.contains(&absent) {
            prop_assert!(
                !ps.entity_exists(&absent),
                "INV-FERR-027: entity_exists returned true for absent entity"
            );
        }
    }
}

/// XOR two 32-byte fingerprints (test helper for INV-FERR-074).
fn xor_fingerprints(a: &[u8; 32], b: &[u8; 32]) -> [u8; 32] {
    let mut result = [0u8; 32];
    for (r, (x, y)) in result.iter_mut().zip(a.iter().zip(b.iter())) {
        *r = x ^ y;
    }
    result
}
