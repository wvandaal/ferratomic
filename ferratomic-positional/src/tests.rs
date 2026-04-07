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

mod soa_columnar_tests {
    use ferratom::{Attribute, Datom, EntityId, Op, TxId, Value};

    use crate::store::PositionalStore;

    fn proof_entity_id(id: u8) -> EntityId {
        let mut bytes = [0u8; 32];
        bytes[31] = id;
        EntityId::from_bytes(bytes)
    }

    /// Build a small canonical store with varied entities, tx, and ops.
    fn build_test_store() -> PositionalStore {
        let attr = Attribute::from("test/attr");
        let datoms = vec![
            Datom::new(
                proof_entity_id(0x10),
                attr.clone(),
                Value::Long(1),
                TxId::new(1, 0, 0),
                Op::Assert,
            ),
            Datom::new(
                proof_entity_id(0x10),
                attr.clone(),
                Value::Long(1),
                TxId::new(2, 0, 0),
                Op::Retract,
            ),
            Datom::new(
                proof_entity_id(0x20),
                attr.clone(),
                Value::Long(2),
                TxId::new(3, 0, 0),
                Op::Assert,
            ),
            Datom::new(
                proof_entity_id(0x30),
                attr,
                Value::Long(3),
                TxId::new(4, 0, 0),
                Op::Assert,
            ),
        ];
        PositionalStore::from_datoms(datoms.into_iter())
    }

    /// INV-FERR-078: entity column matches `canonical[p].entity()` for all positions.
    #[test]
    fn test_soa_col_entities_matches_canonical() {
        let ps = build_test_store();
        let col = ps.col_entities();
        assert_eq!(
            col.len(),
            ps.len(),
            "INV-FERR-078: entity column length must equal canonical length"
        );
        for (p, datom) in ps.datoms().iter().enumerate() {
            assert_eq!(
                col[p],
                datom.entity(),
                "INV-FERR-078: col_entities[{p}] must match canonical[{p}].entity()"
            );
        }
    }

    /// INV-FERR-078: transaction column matches `canonical[p].tx()` for all positions.
    #[test]
    fn test_soa_col_txids_matches_canonical() {
        let ps = build_test_store();
        let col = ps.col_txids();
        assert_eq!(
            col.len(),
            ps.len(),
            "INV-FERR-078: txid column length must equal canonical length"
        );
        for (p, datom) in ps.datoms().iter().enumerate() {
            assert_eq!(
                col[p],
                datom.tx(),
                "INV-FERR-078: col_txids[{p}] must match canonical[{p}].tx()"
            );
        }
    }

    /// INV-FERR-078: op column matches `canonical[p].op()` for all positions.
    #[test]
    fn test_soa_col_ops_matches_canonical() {
        let ps = build_test_store();
        let col = ps.col_ops();
        assert_eq!(
            col.len(),
            ps.len(),
            "INV-FERR-078: op column length must equal canonical length"
        );
        for (p, datom) in ps.datoms().iter().enumerate() {
            let expected = datom.op() == Op::Assert;
            assert_eq!(
                col[p], expected,
                "INV-FERR-078: col_ops[{p}] must be true iff canonical[{p}].op() == Assert"
            );
        }
    }

    /// Empty store: all columnar columns are empty.
    #[test]
    fn test_soa_columns_empty_store() {
        let ps = PositionalStore::from_datoms(std::iter::empty());
        assert!(
            ps.col_entities().is_empty(),
            "INV-FERR-078: empty store entity column must be empty"
        );
        assert!(
            ps.col_txids().is_empty(),
            "INV-FERR-078: empty store txid column must be empty"
        );
        assert!(
            ps.col_ops().is_empty(),
            "INV-FERR-078: empty store op column must be empty"
        );
    }

    /// Clone preserves initialized columnar columns.
    #[test]
    fn test_soa_columns_clone_preserves_init() {
        let ps = build_test_store();
        // Force lazy initialization of all columns.
        let _ = ps.col_entities();
        let _ = ps.col_txids();
        let _ = ps.col_ops();

        let cloned = ps.clone();
        assert_eq!(
            cloned.col_entities(),
            ps.col_entities(),
            "INV-FERR-078: cloned entity column must match original"
        );
        assert_eq!(
            cloned.col_txids(),
            ps.col_txids(),
            "INV-FERR-078: cloned txid column must match original"
        );
        assert_eq!(
            *cloned.col_ops(),
            *ps.col_ops(),
            "INV-FERR-078: cloned op column must match original"
        );
    }

    /// INV-FERR-078: `build_col_attrs` produces an attribute column matching canonical.
    #[test]
    fn test_soa_build_col_attrs_matches_canonical() {
        use ferratom::AttributeIntern;

        let datoms: Vec<Datom> = (0..10u8)
            .map(|i| {
                let mut bytes = [0u8; 32];
                bytes[31] = i;
                Datom::new(
                    EntityId::from_bytes(bytes),
                    Attribute::from("test/attr"),
                    Value::Long(i64::from(i)),
                    TxId::new(1, 0, 0),
                    Op::Assert,
                )
            })
            .collect();
        let ps = PositionalStore::from_datoms(datoms.into_iter());
        let intern =
            AttributeIntern::from_attributes(vec![Attribute::from("test/attr")]).expect("intern");

        let col = ps.build_col_attrs(&intern);
        assert_eq!(
            col.len(),
            ps.len(),
            "col_attrs must have same length as canonical"
        );
        for opt_id in &col {
            assert!(opt_id.is_some(), "all attributes should be interned");
        }
    }
}

mod perm_txid_tests {
    use ferratom::{Attribute, Datom, EntityId, Op, TxId, Value};

    use crate::store::PositionalStore;

    fn proof_entity_id(id: u8) -> EntityId {
        let mut bytes = [0u8; 32];
        bytes[31] = id;
        EntityId::from_bytes(bytes)
    }

    /// Build a store with datoms at distinct `TxId` values to exercise temporal ordering.
    fn build_temporal_store() -> PositionalStore {
        let attr = Attribute::from("test/attr");
        let datoms = vec![
            Datom::new(
                proof_entity_id(0x10),
                attr.clone(),
                Value::Long(1),
                TxId::new(5, 0, 0),
                Op::Assert,
            ),
            Datom::new(
                proof_entity_id(0x20),
                attr.clone(),
                Value::Long(2),
                TxId::new(1, 0, 0),
                Op::Assert,
            ),
            Datom::new(
                proof_entity_id(0x30),
                attr.clone(),
                Value::Long(3),
                TxId::new(3, 0, 0),
                Op::Assert,
            ),
            Datom::new(
                proof_entity_id(0x40),
                attr,
                Value::Long(4),
                TxId::new(2, 0, 0),
                Op::Assert,
            ),
        ];
        PositionalStore::from_datoms(datoms.into_iter())
    }

    /// bd-3ta0: sorted `TxId` permutation contains all positions [0, N).
    #[test]
    fn test_perm_txid_covers_all_positions() {
        let ps = build_temporal_store();
        let n = ps.len();
        let sorted = ps.perm_txid_sorted();

        assert_eq!(
            sorted.len(),
            n,
            "bd-3ta0: TxId permutation must have exactly N entries"
        );

        let mut positions: Vec<u32> = sorted.clone();
        positions.sort_unstable();
        let expected: Vec<u32> = (0..u32::try_from(n).unwrap_or(u32::MAX)).collect();
        assert_eq!(
            positions, expected,
            "bd-3ta0: TxId permutation must be a permutation of [0, N)"
        );
    }

    /// bd-3ta0: datoms accessed through the permutation are in `TxId` order.
    #[test]
    fn test_perm_txid_sorted_by_txid() {
        let ps = build_temporal_store();
        let sorted = ps.perm_txid_sorted();

        let txids: Vec<TxId> = sorted
            .iter()
            .filter_map(|&pos| ps.datom_at(pos).map(ferratom::Datom::tx))
            .collect();
        for window in txids.windows(2) {
            assert!(
                window[0] <= window[1],
                "bd-3ta0: datoms via TxId permutation must be in non-decreasing TxId order, \
                 got {:?} > {:?}",
                window[0],
                window[1]
            );
        }
    }

    /// bd-3ta0: empty store produces a valid (empty) `TxId` permutation.
    #[test]
    fn test_perm_txid_empty_store() {
        let ps = PositionalStore::from_datoms(std::iter::empty());
        let sorted = ps.perm_txid_sorted();
        assert!(
            sorted.is_empty(),
            "bd-3ta0: empty store TxId permutation must be empty"
        );
        // Eytzinger layout: sentinel-only array.
        let eytzinger = ps.perm_txid();
        assert_eq!(
            eytzinger.len(),
            1,
            "bd-3ta0: empty Eytzinger layout has sentinel only"
        );
        assert_eq!(eytzinger[0], u32::MAX, "bd-3ta0: sentinel is u32::MAX");
    }

    /// bd-3ta0: Eytzinger round-trip preserves sorted order.
    #[test]
    fn test_perm_txid_eytzinger_roundtrip() {
        let ps = build_temporal_store();
        let eytzinger = ps.perm_txid();
        let recovered = crate::perm::layout_to_sorted(eytzinger);
        let sorted = ps.perm_txid_sorted();
        assert_eq!(
            recovered, sorted,
            "bd-3ta0: Eytzinger round-trip must recover sorted `TxId` permutation"
        );
    }

    /// INV-FERR-081: duplicate `TxId` ordering is deterministic via tiebreaker.
    #[test]
    fn test_perm_txid_duplicate_txids_deterministic() {
        // 4 datoms with only 2 distinct TxIds.
        let mut bytes = [0u8; 32];

        bytes[31] = 1;
        let d1 = Datom::new(
            EntityId::from_bytes(bytes),
            Attribute::from("a"),
            Value::Long(1),
            TxId::new(5, 0, 0),
            Op::Assert,
        );
        bytes[31] = 2;
        let d2 = Datom::new(
            EntityId::from_bytes(bytes),
            Attribute::from("a"),
            Value::Long(2),
            TxId::new(5, 0, 0),
            Op::Assert,
        );
        bytes[31] = 3;
        let d3 = Datom::new(
            EntityId::from_bytes(bytes),
            Attribute::from("b"),
            Value::Long(3),
            TxId::new(3, 0, 0),
            Op::Assert,
        );
        bytes[31] = 4;
        let d4 = Datom::new(
            EntityId::from_bytes(bytes),
            Attribute::from("b"),
            Value::Long(4),
            TxId::new(3, 0, 0),
            Op::Assert,
        );

        let datoms = vec![d1, d2, d3, d4];
        let ps = PositionalStore::from_datoms(datoms.into_iter());
        let sorted = ps.perm_txid_sorted();

        // Verify TxId non-decreasing
        for w in sorted.windows(2) {
            let tx_a = ps.datom_at(w[0]).map(ferratom::Datom::tx);
            let tx_b = ps.datom_at(w[1]).map(ferratom::Datom::tx);
            assert!(tx_a <= tx_b, "INV-FERR-081: TxId must be non-decreasing");
        }

        // Verify deterministic: build again, same result
        let ps2 = PositionalStore::from_datoms(ps.datoms().iter().cloned());
        assert_eq!(
            ps2.perm_txid_sorted(),
            sorted,
            "INV-FERR-081: perm_txid must be deterministic"
        );
    }
}

mod incremental_live_tests {
    use ferratom::{Attribute, Datom, EntityId, Op, TxId, Value};

    use crate::{
        chunk_fingerprints::ChunkFingerprints,
        live::{build_live_bitvector, rebuild_live_incremental_for_test},
    };

    fn proof_entity_id(id: u8) -> EntityId {
        let mut bytes = [0u8; 32];
        bytes[31] = id;
        EntityId::from_bytes(bytes)
    }

    fn make_datom(entity_seed: u8, tx_wall: u64, op: Op) -> Datom {
        Datom::new(
            proof_entity_id(entity_seed),
            Attribute::from("db/doc"),
            Value::Long(i64::from(entity_seed)),
            TxId::new(tx_wall, 0, 0),
            op,
        )
    }

    fn make_datom_with_value(entity_seed: u8, value: i64, tx_wall: u64, op: Op) -> Datom {
        Datom::new(
            proof_entity_id(entity_seed),
            Attribute::from("db/doc"),
            Value::Long(value),
            TxId::new(tx_wall, 0, 0),
            op,
        )
    }

    /// INV-FERR-080: incremental rebuild on empty canonical produces empty bitvector.
    #[test]
    fn test_inv_ferr_080_empty_canonical() {
        let old_fps = ChunkFingerprints::from_canonical(&[], 4);
        let new_fps = ChunkFingerprints::from_canonical(&[], 4);
        let result = rebuild_live_incremental_for_test(&[], 4, &old_fps, &new_fps);
        let expected = build_live_bitvector(&[]);
        assert_eq!(
            result, expected,
            "INV-FERR-080: empty canonical produces empty LIVE bitvector"
        );
    }

    /// INV-FERR-080: identical fingerprints (no changes) produces same result
    /// as full rebuild.
    #[test]
    fn test_inv_ferr_080_identical_fingerprints() {
        let mut datoms: Vec<Datom> = (0..8u8).map(|i| make_datom(i, 1, Op::Assert)).collect();
        datoms.sort();
        datoms.dedup();

        let fps = ChunkFingerprints::from_canonical(&datoms, 4);
        let result = rebuild_live_incremental_for_test(&datoms, 4, &fps, &fps);
        let expected = build_live_bitvector(&datoms);
        assert_eq!(
            result, expected,
            "INV-FERR-080: identical fingerprints must produce same LIVE as full rebuild"
        );
    }

    /// INV-FERR-080: fallback when canonical length changed (Phase 4a path).
    /// After `merge_sort_dedup` inserts new datoms, the canonical grows.
    /// The function must fall back to full rebuild and produce correct results.
    #[test]
    fn test_inv_ferr_080_length_change_fallback() {
        let mut old_datoms: Vec<Datom> = (0..4u8).map(|i| make_datom(i, 1, Op::Assert)).collect();
        old_datoms.sort();
        old_datoms.dedup();

        let mut new_datoms: Vec<Datom> = (0..8u8).map(|i| make_datom(i, 1, Op::Assert)).collect();
        new_datoms.sort();
        new_datoms.dedup();

        let old_fps = ChunkFingerprints::from_canonical(&old_datoms, 4);
        let new_fps = ChunkFingerprints::from_canonical(&new_datoms, 4);

        let result = rebuild_live_incremental_for_test(&new_datoms, 4, &old_fps, &new_fps);
        let expected = build_live_bitvector(&new_datoms);
        assert_eq!(
            result, expected,
            "INV-FERR-080: length change must fall back to full rebuild"
        );
    }

    /// INV-FERR-080: mixed Assert/Retract patterns produce correct LIVE
    /// through the incremental path.
    #[test]
    fn test_inv_ferr_080_assert_retract_mix() {
        let mut datoms = vec![
            make_datom_with_value(1, 10, 1, Op::Assert),
            make_datom_with_value(1, 10, 2, Op::Retract),
            make_datom_with_value(2, 20, 1, Op::Assert),
            make_datom_with_value(3, 30, 1, Op::Assert),
            make_datom_with_value(3, 30, 2, Op::Assert),
            make_datom_with_value(4, 40, 1, Op::Assert),
            make_datom_with_value(5, 50, 1, Op::Assert),
            make_datom_with_value(5, 50, 2, Op::Retract),
        ];
        datoms.sort();
        datoms.dedup();

        let fps = ChunkFingerprints::from_canonical(&datoms, 4);
        let result = rebuild_live_incremental_for_test(&datoms, 4, &fps, &fps);
        let expected = build_live_bitvector(&datoms);
        assert_eq!(
            result, expected,
            "INV-FERR-080: mixed assert/retract must match full rebuild"
        );
    }

    /// INV-FERR-080: `chunk_size` mismatch triggers fallback.
    #[test]
    fn test_inv_ferr_080_chunk_size_mismatch_fallback() {
        let mut datoms: Vec<Datom> = (0..8u8).map(|i| make_datom(i, 1, Op::Assert)).collect();
        datoms.sort();
        datoms.dedup();

        let old_fps = ChunkFingerprints::from_canonical(&datoms, 4);
        let new_fps = ChunkFingerprints::from_canonical(&datoms, 8);

        // chunk_size parameter differs from old_fps chunk_size -> fallback
        let result = rebuild_live_incremental_for_test(&datoms, 8, &old_fps, &new_fps);
        let expected = build_live_bitvector(&datoms);
        assert_eq!(
            result, expected,
            "INV-FERR-080: `chunk_size` mismatch must fall back correctly"
        );
    }

    /// INV-FERR-080: single-datom canonical works correctly.
    #[test]
    fn test_inv_ferr_080_single_datom() {
        let datoms = vec![make_datom(1, 1, Op::Assert)];
        let fps = ChunkFingerprints::from_canonical(&datoms, 4);

        let result = rebuild_live_incremental_for_test(&datoms, 4, &fps, &fps);
        let expected = build_live_bitvector(&datoms);
        assert_eq!(
            result, expected,
            "INV-FERR-080: single datom must produce correct LIVE"
        );
    }

    /// INV-FERR-080: all-retract canonical produces empty LIVE set.
    #[test]
    fn test_inv_ferr_080_all_retract() {
        let mut datoms: Vec<Datom> = (0..4u8).map(|i| make_datom(i, 1, Op::Retract)).collect();
        datoms.sort();
        datoms.dedup();

        let fps = ChunkFingerprints::from_canonical(&datoms, 4);
        let result = rebuild_live_incremental_for_test(&datoms, 4, &fps, &fps);
        let expected = build_live_bitvector(&datoms);
        assert_eq!(
            result, expected,
            "INV-FERR-080: all-retract must produce no live bits"
        );
        assert_eq!(
            result.count_ones(),
            0,
            "INV-FERR-080: no datoms should be live when all are retracts"
        );
    }

    /// INV-FERR-080: (entity, attribute, value) group spanning chunk boundary.
    /// Two datoms with the same (e,a,v) but different tx, straddling a
    /// chunk boundary, must produce correct LIVE regardless of the split.
    #[test]
    fn test_inv_ferr_080_group_spans_chunk_boundary() {
        // chunk_size=2, 4 datoms. The (e=1, a, v=10) group at positions 1-2
        // spans the boundary between chunk 0 and chunk 1.
        let mut datoms = vec![
            make_datom_with_value(0, 0, 1, Op::Assert),
            make_datom_with_value(1, 10, 1, Op::Assert),
            make_datom_with_value(1, 10, 2, Op::Retract),
            make_datom_with_value(2, 20, 1, Op::Assert),
        ];
        datoms.sort();
        datoms.dedup();

        let fps = ChunkFingerprints::from_canonical(&datoms, 2);
        let result = rebuild_live_incremental_for_test(&datoms, 2, &fps, &fps);
        let expected = build_live_bitvector(&datoms);
        assert_eq!(
            result, expected,
            "INV-FERR-080: group spanning chunk boundary must be handled correctly"
        );
    }

    /// INV-FERR-080: proptest -- incremental rebuild always equals full rebuild.
    ///
    /// Generates arbitrary sorted, deduplicated datom arrays and verifies
    /// that `rebuild_live_incremental` produces bit-identical results to
    /// `build_live_bitvector` for every chunk size power of 2 in {1, 2, 4}.
    mod proptests {
        use proptest::prelude::*;

        use super::*;

        /// Generate a sorted, deduplicated datom vector of bounded size.
        fn arb_sorted_datoms() -> impl Strategy<Value = Vec<Datom>> {
            // Up to 64 datoms with small entity space to encourage (e,a,v) collisions.
            proptest::collection::vec(
                (
                    0..16u8,
                    0..4i64,
                    0..4u64,
                    prop_oneof![Just(Op::Assert), Just(Op::Retract)],
                ),
                0..64,
            )
            .prop_map(|raw| {
                let mut datoms: Vec<Datom> = raw
                    .into_iter()
                    .map(|(e, v, tx, op)| make_datom_with_value(e, v, tx, op))
                    .collect();
                datoms.sort();
                datoms.dedup();
                datoms
            })
        }

        proptest! {
            #[test]
            fn test_inv_ferr_080_incremental_equals_full(
                datoms in arb_sorted_datoms(),
                chunk_size_exp in 0..3u32, // 2^0=1, 2^1=2, 2^2=4
            ) {
                let chunk_size = 1usize << chunk_size_exp;
                let fps = ChunkFingerprints::from_canonical(&datoms, chunk_size);

                let incremental = rebuild_live_incremental_for_test(
                    &datoms, chunk_size, &fps, &fps,
                );
                let full = build_live_bitvector(&datoms);
                prop_assert_eq!(
                    incremental, full,
                    "INV-FERR-080: incremental must be bit-identical to full rebuild"
                );
            }

            /// Proptest: incremental with length-change fallback also matches.
            /// Simulates the Phase 4a fallback path where old and new
            /// canonical arrays have different lengths.
            #[test]
            fn test_inv_ferr_080_fallback_equals_full(
                old_datoms in arb_sorted_datoms(),
                extra_datoms in arb_sorted_datoms(),
            ) {
                // Merge old + extra to get new canonical that may differ in length.
                let mut new_datoms = old_datoms.clone();
                new_datoms.extend(extra_datoms);
                new_datoms.sort();
                new_datoms.dedup();

                let chunk_size = 4;
                let old_fps = ChunkFingerprints::from_canonical(&old_datoms, chunk_size);
                let new_fps = ChunkFingerprints::from_canonical(&new_datoms, chunk_size);

                let incremental = rebuild_live_incremental_for_test(
                    &new_datoms, chunk_size, &old_fps, &new_fps,
                );
                let full = build_live_bitvector(&new_datoms);
                prop_assert_eq!(
                    incremental, full,
                    "INV-FERR-080: fallback path must be bit-identical to full rebuild"
                );
            }
        }
    }
}

mod chunk_fingerprint_tests {
    use ferratom::{Attribute, Datom, EntityId, Op, TxId, Value};

    use crate::{
        chunk_fingerprints::ChunkFingerprints, fingerprint::compute_fingerprint,
        store::PositionalStore,
    };

    fn test_datom(id: u8, tx: u64) -> Datom {
        let mut bytes = [0u8; 32];
        bytes[31] = id;
        Datom::new(
            EntityId::from_bytes(bytes),
            Attribute::from("test/attr"),
            Value::Long(i64::from(id)),
            TxId::new(tx, 0, 0),
            Op::Assert,
        )
    }

    #[test]
    fn test_inv_ferr_079_empty_store() {
        let cf = ChunkFingerprints::from_canonical(&[], 64);
        assert_eq!(cf.num_chunks(), 0, "INV-FERR-079: empty store has 0 chunks");
        assert_eq!(
            cf.store_fingerprint(),
            [0u8; 32],
            "INV-FERR-079: empty store fingerprint is identity"
        );
    }

    #[test]
    fn test_inv_ferr_079_single_chunk() {
        let datoms: Vec<Datom> = (0..10u8).map(|i| test_datom(i, 1)).collect();
        let ps = PositionalStore::from_datoms(datoms.into_iter());
        let cf = ChunkFingerprints::from_canonical(ps.datoms(), 64);

        assert_eq!(
            cf.num_chunks(),
            1,
            "INV-FERR-079: 10 datoms in chunk_size=64 → 1 chunk"
        );
        assert_eq!(
            cf.store_fingerprint(),
            *ps.fingerprint(),
            "INV-FERR-079: single-chunk fingerprint == store fingerprint"
        );
    }

    #[test]
    fn test_inv_ferr_079_decomposition_multi_chunk() {
        // 10 datoms, chunk_size=4 → 3 chunks (4+4+2).
        let datoms: Vec<Datom> = (0..10u8).map(|i| test_datom(i, 1)).collect();
        let ps = PositionalStore::from_datoms(datoms.into_iter());
        let cf = ChunkFingerprints::from_canonical(ps.datoms(), 4);

        assert_eq!(
            cf.num_chunks(),
            3,
            "INV-FERR-079: 10 datoms / chunk_size=4 → 3 chunks"
        );
        let manual_fp = compute_fingerprint(ps.datoms());
        assert_eq!(
            cf.store_fingerprint(),
            manual_fp,
            "INV-FERR-079: XOR of chunk fingerprints must equal store fingerprint"
        );
    }

    #[test]
    fn test_inv_ferr_079_decomposition_via_accessor() {
        let datoms: Vec<Datom> = (0..50u8).map(|i| test_datom(i, 1)).collect();
        let ps = PositionalStore::from_datoms(datoms.into_iter());
        let cf = ps.chunk_fingerprints();

        assert_eq!(
            cf.store_fingerprint(),
            *ps.fingerprint(),
            "INV-FERR-079: lazy-built chunk fingerprints decompose store fingerprint"
        );
    }

    #[test]
    fn test_inv_ferr_079_diff_identical_stores() {
        let datoms: Vec<Datom> = (0..20u8).map(|i| test_datom(i, 1)).collect();
        let ps = PositionalStore::from_datoms(datoms.into_iter());
        let cf_a = ChunkFingerprints::from_canonical(ps.datoms(), 8);
        let cf_b = ChunkFingerprints::from_canonical(ps.datoms(), 8);

        assert!(
            cf_a.diff_chunks(&cf_b).is_empty(),
            "INV-FERR-079: identical stores have zero differing chunks"
        );
    }

    #[test]
    fn test_inv_ferr_079_diff_detects_changes() {
        let datoms_a: Vec<Datom> = (0..8u8).map(|i| test_datom(i, 1)).collect();
        let datoms_b: Vec<Datom> = (0..8u8)
            .map(|i| {
                if i == 5 {
                    test_datom(55, 1) // different datom in second chunk
                } else {
                    test_datom(i, 1)
                }
            })
            .collect();
        let ps_a = PositionalStore::from_datoms(datoms_a.into_iter());
        let ps_b = PositionalStore::from_datoms(datoms_b.into_iter());
        let cf_a = ChunkFingerprints::from_canonical(ps_a.datoms(), 4);
        let cf_b = ChunkFingerprints::from_canonical(ps_b.datoms(), 4);

        let diffs = cf_a.diff_chunks(&cf_b);
        assert!(
            !diffs.is_empty(),
            "INV-FERR-079: different stores must have differing chunks"
        );
    }

    #[test]
    fn test_inv_ferr_079_insert_hash_marks_dirty() {
        let datoms: Vec<Datom> = (0..8u8).map(|i| test_datom(i, 1)).collect();
        let mut cf = ChunkFingerprints::from_canonical(&datoms, 4);

        // Initially no dirty chunks.
        assert_eq!(
            cf.dirty_chunks().count(),
            0,
            "INV-FERR-079: fresh chunk fingerprints have no dirty chunks"
        );

        // Insert a hash into chunk 0.
        let hash = datoms[0].content_hash();
        cf.insert_hash(1, &hash);
        let dirty: Vec<usize> = cf.dirty_chunks().collect();
        assert_eq!(
            dirty,
            vec![0],
            "INV-FERR-079: insert at position 1 dirties chunk 0"
        );

        // Clear dirty.
        cf.clear_dirty();
        assert_eq!(
            cf.dirty_chunks().count(),
            0,
            "INV-FERR-079: clear_dirty resets all dirty flags"
        );
    }

    #[test]
    fn test_inv_ferr_079_insert_updates_single_chunk() {
        let datoms: Vec<Datom> = (0..8u8).map(|i| test_datom(i, 1)).collect();
        let mut cf = ChunkFingerprints::from_canonical(&datoms, 4);
        let original_fp = cf.store_fingerprint();

        // XOR the same hash twice = identity (XOR is self-inverse).
        let hash = [0xABu8; 32];
        cf.insert_hash(2, &hash);
        cf.insert_hash(2, &hash);
        assert_eq!(
            cf.store_fingerprint(),
            original_fp,
            "INV-FERR-079: double XOR is identity"
        );
    }

    #[test]
    fn test_inv_ferr_079_diff_different_sizes() {
        let datoms_small: Vec<Datom> = (0..4u8).map(|i| test_datom(i, 1)).collect();
        let datoms_large: Vec<Datom> = (0..12u8).map(|i| test_datom(i, 1)).collect();
        let cf_small = ChunkFingerprints::from_canonical(&datoms_small, 4);
        let cf_large = ChunkFingerprints::from_canonical(&datoms_large, 4);

        let diffs = cf_small.diff_chunks(&cf_large);
        // The first chunk might match (same datoms), extra chunks always differ.
        assert!(
            diffs.len() >= 2,
            "INV-FERR-079: extra chunks in larger store must appear as diffs"
        );
    }
}
