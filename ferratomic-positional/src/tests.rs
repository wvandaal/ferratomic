//! Tests for positional store, LIVE bitvector, merge, and permutation layout.

mod positional_tests {
    use ferratom::{Attribute, Datom, EntityId, Op, TxId, Value};

    use crate::live::{
        build_live_bitvector, live_positions_for_test,
        live_positions_from_sorted_run_keys_for_test, live_positions_kernel,
    };

    fn proof_entity_id(id: u8) -> EntityId {
        let mut bytes = [0u8; 32];
        bytes[31] = id;
        EntityId::from_bytes(bytes)
    }

    fn canonical_positions(bits: &bitvec::prelude::BitVec<u64, bitvec::prelude::Lsb0>) -> Vec<u32> {
        bits.iter_ones()
            .map(|position| u32::try_from(position).unwrap_or(u32::MAX))
            .collect()
    }

    #[test]
    fn test_inv_ferr_029_live_bitvector_matches_kernel_positions() {
        let entity = proof_entity_id(0x29);
        let attr = Attribute::from("a");
        let canonical = vec![
            Datom::new(
                entity,
                attr.clone(),
                Value::Long(1),
                TxId::new(1, 0, 0),
                Op::Assert,
            ),
            Datom::new(
                entity,
                attr.clone(),
                Value::Long(1),
                TxId::new(2, 0, 0),
                Op::Retract,
            ),
            Datom::new(
                entity,
                attr.clone(),
                Value::Long(2),
                TxId::new(1, 0, 0),
                Op::Assert,
            ),
            Datom::new(entity, attr, Value::Long(2), TxId::new(2, 0, 0), Op::Assert),
        ];

        assert_eq!(
            canonical_positions(&build_live_bitvector(&canonical)),
            live_positions_kernel(&canonical),
            "INV-FERR-029: bitvector LIVE representation must reflect kernel live positions"
        );
    }

    #[test]
    fn test_inv_ferr_029_live_bitvector_respects_triple_boundaries() {
        let attr = Attribute::from("a");
        let canonical = vec![
            Datom::new(
                proof_entity_id(0x31),
                attr.clone(),
                Value::Long(1),
                TxId::new(1, 0, 0),
                Op::Assert,
            ),
            Datom::new(
                proof_entity_id(0x31),
                attr.clone(),
                Value::Long(1),
                TxId::new(2, 0, 0),
                Op::Retract,
            ),
            Datom::new(
                proof_entity_id(0x32),
                attr.clone(),
                Value::Long(1),
                TxId::new(1, 0, 0),
                Op::Assert,
            ),
            Datom::new(
                proof_entity_id(0x32),
                attr,
                Value::Long(1),
                TxId::new(2, 0, 0),
                Op::Assert,
            ),
        ];

        assert_eq!(
            canonical_positions(&build_live_bitvector(&canonical)),
            live_positions_kernel(&canonical),
            "INV-FERR-029: bitvector LIVE representation must track each triple independently"
        );
    }

    #[test]
    fn test_inv_ferr_029_datom_wrapper_matches_group_key_kernel() {
        let attr = Attribute::from("a");
        let canonical = vec![
            Datom::new(
                proof_entity_id(0x41),
                attr.clone(),
                Value::Long(1),
                TxId::new(1, 0, 0),
                Op::Assert,
            ),
            Datom::new(
                proof_entity_id(0x41),
                attr.clone(),
                Value::Long(1),
                TxId::new(2, 0, 0),
                Op::Retract,
            ),
            Datom::new(
                proof_entity_id(0x41),
                attr.clone(),
                Value::Long(2),
                TxId::new(1, 0, 0),
                Op::Assert,
            ),
            Datom::new(
                proof_entity_id(0x41),
                attr,
                Value::Long(2),
                TxId::new(2, 0, 0),
                Op::Assert,
            ),
        ];
        let proof_entries = [
            ((0x41_u8, 0_u8, 1_i64), Op::Assert),
            ((0x41_u8, 0_u8, 1_i64), Op::Retract),
            ((0x41_u8, 0_u8, 2_i64), Op::Assert),
            ((0x41_u8, 0_u8, 2_i64), Op::Assert),
        ];

        assert_eq!(
            live_positions_for_test(&canonical),
            live_positions_from_sorted_run_keys_for_test(&proof_entries),
            "INV-FERR-029: datom LIVE wrapper must agree with the sorted-run kernel"
        );
        assert_eq!(
            live_positions_kernel(&canonical),
            live_positions_from_sorted_run_keys_for_test(&proof_entries),
            "INV-FERR-029: canonical datom grouping must preserve the proof-kernel result"
        );
    }

    /// INV-FERR-027: binary search fallback path produces correct results.
    ///
    /// Tests `first_datom_position_for_entity` which is the fallback when
    /// MPH build fails. Verifies it returns the same positions as
    /// `entity_lookup` (which uses the MPH path).
    #[test]
    fn test_inv_ferr_027_binary_search_fallback() {
        use std::sync::Arc;

        use crate::store::PositionalStore;

        let attr = Attribute::from("db/doc");
        let datoms = vec![
            Datom::new(
                EntityId::from_content(b"a"),
                attr.clone(),
                Value::String(Arc::from("v1")),
                TxId::new(0, 0, 0),
                Op::Assert,
            ),
            Datom::new(
                EntityId::from_content(b"b"),
                attr.clone(),
                Value::String(Arc::from("v2")),
                TxId::new(0, 0, 0),
                Op::Assert,
            ),
            Datom::new(
                EntityId::from_content(b"c"),
                attr,
                Value::String(Arc::from("v3")),
                TxId::new(0, 0, 0),
                Op::Assert,
            ),
        ];
        let ps = PositionalStore::from_datoms(datoms.into_iter());

        // Test the binary search fallback directly.
        let eid_a = EntityId::from_content(b"a");
        let eid_b = EntityId::from_content(b"b");
        let eid_absent = EntityId::from_content(b"absent");

        let pos_a = ps.first_datom_position_for_entity(&eid_a);
        let pos_b = ps.first_datom_position_for_entity(&eid_b);
        let pos_absent = ps.first_datom_position_for_entity(&eid_absent);

        assert!(pos_a.is_some(), "entity a must be found by binary search");
        assert!(pos_b.is_some(), "entity b must be found by binary search");
        assert_eq!(pos_absent, None, "absent entity must return None");

        // Binary search must agree with MPH path.
        assert_eq!(pos_a, ps.entity_lookup(&eid_a));
        assert_eq!(pos_b, ps.entity_lookup(&eid_b));
        assert_eq!(pos_absent, ps.entity_lookup(&eid_absent));
    }

    // -----------------------------------------------------------------------
    // merge_sort_dedup regression tests (DEFECT-005, GOALS.md S6.9)
    // -----------------------------------------------------------------------

    /// Helper: build a sorted datom from entity seed + tx wall clock.
    fn make_datom(entity_seed: u8, tx_wall: u64) -> Datom {
        Datom::new(
            EntityId::from_content(&[entity_seed]),
            Attribute::from("db/doc"),
            Value::String(std::sync::Arc::from("v")),
            TxId::new(tx_wall, 0, 0),
            Op::Assert,
        )
    }

    /// Sort and dedup a vec of datoms (test helper).
    fn sorted_deduped(mut datoms: Vec<Datom>) -> Vec<Datom> {
        datoms.sort();
        datoms.dedup();
        datoms
    }

    #[test]
    fn test_inv_ferr_001_merge_sort_dedup_both_empty() {
        let result = crate::merge::merge_sort_dedup(&[], &[]);
        assert!(result.is_empty(), "INV-FERR-001: empty + empty = empty");
    }

    #[test]
    fn test_inv_ferr_001_merge_sort_dedup_one_empty() {
        let a = sorted_deduped(vec![make_datom(1, 0), make_datom(2, 0)]);
        let result_left = crate::merge::merge_sort_dedup(&a, &[]);
        let result_right = crate::merge::merge_sort_dedup(&[], &a);
        assert_eq!(result_left, a, "INV-FERR-001: a + empty = a");
        assert_eq!(result_right, a, "INV-FERR-001: empty + a = a");
    }

    #[test]
    fn test_inv_ferr_001_merge_sort_dedup_full_overlap() {
        let a = sorted_deduped(vec![make_datom(1, 0), make_datom(2, 0)]);
        let result = crate::merge::merge_sort_dedup(&a, &a);
        assert_eq!(result, a, "INV-FERR-003: a + a = a (idempotent)");
    }

    #[test]
    fn test_inv_ferr_001_merge_sort_dedup_full_disjoint() {
        let a = sorted_deduped(vec![make_datom(1, 0), make_datom(3, 0)]);
        let b = sorted_deduped(vec![make_datom(2, 0), make_datom(4, 0)]);
        let result = crate::merge::merge_sort_dedup(&a, &b);
        let expected = sorted_deduped(vec![
            make_datom(1, 0),
            make_datom(2, 0),
            make_datom(3, 0),
            make_datom(4, 0),
        ]);
        assert_eq!(result, expected, "INV-FERR-001: disjoint merge = union");
    }

    #[test]
    fn test_inv_ferr_001_merge_sort_dedup_single_element() {
        let a = sorted_deduped(vec![make_datom(1, 0)]);
        let b = sorted_deduped(vec![make_datom(2, 0)]);
        let result = crate::merge::merge_sort_dedup(&a, &b);
        assert_eq!(result.len(), 2);
        // Commutativity.
        let result_rev = crate::merge::merge_sort_dedup(&b, &a);
        assert_eq!(result, result_rev, "INV-FERR-001: commutativity");
    }

    #[test]
    fn test_inv_ferr_001_merge_sort_dedup_partial_overlap() {
        let a = sorted_deduped(vec![make_datom(1, 0), make_datom(2, 0), make_datom(3, 0)]);
        let b = sorted_deduped(vec![make_datom(2, 0), make_datom(3, 0), make_datom(4, 0)]);
        let result = crate::merge::merge_sort_dedup(&a, &b);
        let expected = sorted_deduped(vec![
            make_datom(1, 0),
            make_datom(2, 0),
            make_datom(3, 0),
            make_datom(4, 0),
        ]);
        assert_eq!(result, expected, "INV-FERR-001: partial overlap = union");
    }

    // -- from_sorted_with_live error path tests (INV-FERR-076) ----------------

    #[test]
    fn test_inv_ferr_076_from_sorted_with_live_length_mismatch() {
        use bitvec::prelude::{BitVec, Lsb0};

        use crate::store::PositionalStore;

        let datoms = vec![make_datom(1, 0)];
        let wrong_bits = BitVec::<u64, Lsb0>::repeat(false, 5); // length 5 != 1
        let result = PositionalStore::from_sorted_with_live(datoms, wrong_bits);
        assert!(
            result.is_err(),
            "INV-FERR-076: mismatched live_bits length must be rejected"
        );
    }

    #[test]
    fn test_inv_ferr_076_from_sorted_with_live_unsorted() {
        use bitvec::prelude::{BitVec, Lsb0};

        use crate::store::PositionalStore;

        // Intentionally unsorted: datom 2 before datom 1
        let datoms = vec![make_datom(2, 0), make_datom(1, 0)];
        let bits = BitVec::<u64, Lsb0>::repeat(true, 2);
        let result = PositionalStore::from_sorted_with_live(datoms, bits);
        assert!(
            result.is_err(),
            "INV-FERR-076: unsorted canonical must be rejected"
        );
    }
}

mod perm_tests {
    use ferratom::{Attribute, Datom, EntityId, Op, TxId, Value};

    use crate::perm::{layout_permutation, layout_search, layout_to_sorted};

    /// Helper: build a datom with a specific entity content for ordering.
    fn make_datom(content: &[u8]) -> Datom {
        Datom::new(
            EntityId::from_content(content),
            Attribute::from("db/doc"),
            Value::Bool(true),
            TxId::new(0, 1, 0),
            Op::Assert,
        )
    }

    /// Empty input produces a single-element sentinel array.
    /// Search on empty returns None.
    #[test]
    fn test_eytzinger_empty() {
        let result = layout_permutation(&[]);
        assert_eq!(
            result.len(),
            1,
            "INV-FERR-071: empty layout has sentinel only"
        );
        assert_eq!(result[0], u32::MAX, "INV-FERR-071: sentinel is u32::MAX");

        // Search on empty Eytzinger array returns None.
        let canonical: Vec<Datom> = Vec::new();
        let found = layout_search(&result, &canonical, |_d| std::cmp::Ordering::Equal);
        assert!(
            found.is_none(),
            "INV-FERR-071: search on empty returns None"
        );
    }

    /// Single element: [MAX, 0]. Search finds the element.
    #[test]
    fn test_eytzinger_single() {
        let result = layout_permutation(&[0]);
        assert_eq!(
            result,
            vec![u32::MAX, 0],
            "INV-FERR-071: single element layout"
        );

        // Search for the single element.
        let d = make_datom(b"alpha");
        let canonical = vec![d.clone()];
        let found = layout_search(&result, &canonical, |datom| datom.cmp(&d));
        assert!(found.is_some(), "INV-FERR-071: search finds single element");
    }

    /// Seven elements form a perfect binary tree of depth 3.
    ///
    /// Sorted: [0, 1, 2, 3, 4, 5, 6]
    /// BFS:    [MAX, 3, 1, 5, 0, 2, 4, 6]
    ///
    /// Tree:       3
    ///           /   \
    ///          1     5
    ///         / \   / \
    ///        0   2 4   6
    #[test]
    fn test_eytzinger_seven() {
        let sorted: Vec<u32> = (0..7).collect();
        let result = layout_permutation(&sorted);
        assert_eq!(
            result,
            vec![u32::MAX, 3, 1, 5, 0, 2, 4, 6],
            "INV-FERR-071: perfect binary tree BFS order"
        );
    }

    /// Round-trip: `layout_to_sorted(layout_permutation(sorted)) == sorted`.
    #[test]
    fn test_eytzinger_roundtrip() {
        for n in 0..=20 {
            let sorted: Vec<u32> = (0..n).collect();
            let bfs = layout_permutation(&sorted);
            let recovered = layout_to_sorted(&bfs);
            assert_eq!(
                recovered, sorted,
                "INV-FERR-071: round-trip failed for n={n}"
            );
        }
    }
}
