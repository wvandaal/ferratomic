//! Index consistency and shard property tests.
//!
//! Tests INV-FERR-005 (index bijection), INV-FERR-006 (snapshot isolation),
//! INV-FERR-007 (write linearizability), INV-FERR-017 (shard equivalence),
//! INV-FERR-025 (index backend interchangeability), INV-FERR-027 (read latency),
//! INV-FERR-071 (sorted-array index backend).
//!
//! Phase 4a: all tests passing against ferratomic-core implementation.

use std::collections::BTreeSet;

use ferratom::Datom;
use ferratomic_core::{
    indexes::{EavtKey, IndexBackend, SortedVecBackend, SortedVecIndexes},
    merge::merge,
    store::Store,
};
use ferratomic_verify::generators::*;
use im::OrdMap;
use proptest::prelude::*;

/// Verify index bijection: primary set == each secondary index set.
fn verify_index_bijection(store: &Store) -> bool {
    let primary: BTreeSet<&Datom> = store.datoms().collect();
    let eavt: BTreeSet<&Datom> = store.indexes().eavt_datoms().collect();
    let aevt: BTreeSet<&Datom> = store.indexes().aevt_datoms().collect();
    let vaet: BTreeSet<&Datom> = store.indexes().vaet_datoms().collect();
    let avet: BTreeSet<&Datom> = store.indexes().avet_datoms().collect();

    primary == eavt && primary == aevt && primary == vaet && primary == avet
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(10_000))]

    /// INV-FERR-005: After every transaction, all indexes match primary.
    ///
    /// Falsification: datom in primary but missing from an index, or
    /// phantom entry in index not in primary.
    #[test]
    fn inv_ferr_005_index_bijection_after_transactions(
        initial in arb_store(20),
        txns in prop::collection::vec(arb_transaction(), 0..10),
    ) {
        let mut store = initial;
        for tx in txns {
            store.transact_test(tx)
                .expect("INV-FERR-005: transact must succeed for committed tx");
            prop_assert!(
                verify_index_bijection(&store),
                "INV-FERR-005 violated: index bijection broken after transact. \
                 store size={}",
                store.len()
            );
        }
    }

    /// INV-FERR-005: After merge, all indexes match primary.
    #[test]
    fn inv_ferr_005_index_bijection_after_merge(
        a in arb_store(30),
        b in arb_store(30),
    ) {
        let merged = merge(&a, &b).expect("INV-FERR-005: merge must succeed");
        prop_assert!(
            verify_index_bijection(&merged),
            "INV-FERR-005 violated: index bijection broken after merge. \
             |A|={}, |B|={}, |merged|={}",
            a.len(), b.len(), merged.len()
        );
    }

    /// INV-FERR-005: Runtime bijection check via Store::verify_bijection
    /// and explicit 4-index cardinality comparison against primary set.
    ///
    /// bd-zws: generates random stores and verifies that
    /// (a) Store::verify_bijection returns Ok, and
    /// (b) all 4 secondary indexes have exactly the same count as primary.
    #[test]
    fn test_inv_ferr_005_index_bijection(
        store in arb_store(50),
    ) {
        // (a) The indexes verify_bijection must succeed.
        prop_assert!(
            store.indexes().verify_bijection(),
            "INV-FERR-005: index bijection violated for a valid store"
        );

        // (b) Explicit 4-index cardinality check against primary.
        let primary_count = store.len();
        let eavt_count = store.indexes().eavt().len();
        let aevt_count = store.indexes().aevt().len();
        let vaet_count = store.indexes().vaet().len();
        let avet_count = store.indexes().avet().len();

        prop_assert_eq!(
            eavt_count, primary_count,
            "INV-FERR-005: EAVT count ({}) != primary count ({})",
            eavt_count, primary_count
        );
        prop_assert_eq!(
            aevt_count, primary_count,
            "INV-FERR-005: AEVT count ({}) != primary count ({})",
            aevt_count, primary_count
        );
        prop_assert_eq!(
            vaet_count, primary_count,
            "INV-FERR-005: VAET count ({}) != primary count ({})",
            vaet_count, primary_count
        );
        prop_assert_eq!(
            avet_count, primary_count,
            "INV-FERR-005: AVET count ({}) != primary count ({})",
            avet_count, primary_count
        );
    }

    /// INV-FERR-006: Snapshot sees no future transactions.
    ///
    /// Falsification: snapshot grows after later transactions are committed.
    #[test]
    fn inv_ferr_006_snapshot_sees_no_future_txns(
        initial_txns in prop::collection::vec(arb_transaction(), 1..5),
        later_txns in prop::collection::vec(arb_transaction(), 1..5),
    ) {
        let mut store = Store::genesis();
        for tx in initial_txns {
            store.transact_test(tx)
                .expect("INV-FERR-006: initial transact must succeed");
        }

        let snapshot = store.snapshot();
        let snap_datoms: BTreeSet<_> = snapshot.datoms().cloned().collect();

        for tx in later_txns {
            store.transact_test(tx)
                .expect("INV-FERR-006: later transact must succeed");
        }

        let snap_datoms_after: BTreeSet<_> = snapshot.datoms().cloned().collect();
        let before_len = snap_datoms.len();
        let after_len = snap_datoms_after.len();
        prop_assert_eq!(
            snap_datoms,
            snap_datoms_after,
            "INV-FERR-006 violated: snapshot changed after later transactions. \
             before={}, after={}",
            before_len,
            after_len
        );
    }

    /// INV-FERR-006: Transaction atomicity — full or nothing visibility.
    ///
    /// Falsification: reader sees subset of a transaction's datoms.
    #[test]
    fn inv_ferr_006_transaction_atomicity(
        txns in prop::collection::vec(arb_multi_datom_transaction(), 1..10),
    ) {
        let mut store = Store::genesis();
        for tx in txns {
            // ME-020: Use receipt datoms (post-stamp) instead of tx datoms
            // (pre-stamp with placeholder TxId). The original assertion was
            // tautological because pre-stamp datoms never match post-stamp
            // ones in the snapshot, so visible_count was always 0.
            let receipt = store.transact_test(tx)
                .expect("INV-FERR-006: transact must succeed for committed tx");
            let stamped_datoms: BTreeSet<_> = receipt.datoms().iter().cloned().collect();

            let snapshot = store.snapshot();
            let visible: BTreeSet<_> = snapshot.datoms().cloned().collect();

            let visible_count = stamped_datoms.iter().filter(|d| visible.contains(d)).count();
            prop_assert_eq!(
                visible_count,
                stamped_datoms.len(),
                "INV-FERR-006 violated: partial transaction visibility. \
                 {} of {} post-stamp datoms visible",
                visible_count,
                stamped_datoms.len()
            );
        }
    }

    /// INV-FERR-017: Shard partition + union = original store.
    ///
    /// Partition a store by entity hash modulo N shards. The union of all
    /// shards must equal the original store exactly.
    ///
    /// Falsification: a datom is missing from all shards, or appears in two
    /// shards, or the union differs from the original.
    #[test]
    fn inv_ferr_017_shard_equivalence(
        store in arb_store(50),
        shard_count in 2usize..8,
    ) {
        use std::collections::BTreeSet;

        // Partition datoms by entity hash modulo shard_count.
        let mut shards: Vec<BTreeSet<&ferratom::Datom>> =
            (0..shard_count).map(|_| BTreeSet::new()).collect();

        for d in store.datoms() {
            let shard_id = {
                let entity = d.entity();
                let entity_bytes = entity.as_bytes();
                // Use first 8 bytes of entity as a u64 hash for sharding.
                let mut buf = [0u8; 8];
                let len = entity_bytes.len().min(8);
                buf[..len].copy_from_slice(&entity_bytes[..len]);
                (u64::from_le_bytes(buf) as usize) % shard_count
            };
            shards[shard_id].insert(d);
        }

        // Property 1: Union of all shards = original store (coverage).
        let union: BTreeSet<&ferratom::Datom> = shards.iter().flat_map(|s| s.iter().copied()).collect();
        let primary: BTreeSet<&ferratom::Datom> = store.datoms().collect();
        let union_len = union.len();
        let primary_len = primary.len();
        prop_assert_eq!(
            union, primary,
            "INV-FERR-017 violated: shard union differs from original store. \
             union_size={}, primary_size={}",
            union_len, primary_len
        );

        // Property 2: Shards are disjoint.
        for i in 0..shard_count {
            for j in (i + 1)..shard_count {
                let overlap: Vec<_> = shards[i].intersection(&shards[j]).collect();
                prop_assert!(
                    overlap.is_empty(),
                    "INV-FERR-017 violated: shards {} and {} share {} datoms",
                    i, j, overlap.len()
                );
            }
        }

        // Property 3: Total cardinality is preserved.
        let total: usize = shards.iter().map(|s| s.len()).sum();
        prop_assert_eq!(
            total, store.len(),
            "INV-FERR-017 violated: total shard cardinality {} != store cardinality {}",
            total, store.len()
        );
    }

    /// INV-FERR-007: Committed epochs are strictly monotonically increasing.
    ///
    /// Falsification: two transactions with same epoch, or epoch decreases.
    #[test]
    fn inv_ferr_007_epochs_strictly_increase(
        txns in prop::collection::vec(arb_transaction(), 1..20),
    ) {
        let mut store = Store::genesis();
        let mut prev_epoch: Option<u64> = None;

        for tx in txns {
            let receipt = store.transact_test(tx)
                .expect("INV-FERR-007: transact must succeed for committed tx");
            if let Some(prev) = prev_epoch {
                prop_assert!(
                    receipt.epoch() > prev,
                    "INV-FERR-007 violated: epoch did not increase. \
                     prev={}, current={}",
                    prev,
                    receipt.epoch()
                );
            }
            prev_epoch = Some(receipt.epoch());
        }
    }

    /// INV-FERR-025: IndexBackend<OrdMap> insert/get round-trip.
    ///
    /// For any sequence of datoms, every datom inserted into an OrdMap-backed
    /// index can be retrieved by its key. The backend_len matches the number
    /// of unique keys inserted.
    ///
    /// Falsification: an inserted datom cannot be retrieved by its key, or
    /// backend_len disagrees with the number of unique keys.
    #[test]
    fn inv_ferr_025_index_backend_roundtrip(
        datoms in prop::collection::vec(arb_datom(), 1..100),
    ) {
        let mut backend: OrdMap<EavtKey, Datom> = OrdMap::new();

        // Insert all datoms.
        for d in &datoms {
            let key = EavtKey::from_datom(d);
            backend.backend_insert(key, d.clone());
        }

        // Every datom must be retrievable by its key.
        for d in &datoms {
            let key = EavtKey::from_datom(d);
            let retrieved = backend.backend_get(&key);
            prop_assert!(
                retrieved.is_some(),
                "INV-FERR-025 violated: inserted datom not found by key. \
                 entity={:?}, attr={:?}",
                d.entity(), d.attribute()
            );
            // The retrieved datom must equal the original.
            prop_assert_eq!(
                retrieved.expect("already checked"),
                d,
                "INV-FERR-025 violated: retrieved datom differs from inserted"
            );
        }

        // backend_len must equal the number of unique keys.
        let unique_keys: BTreeSet<_> = datoms.iter()
            .map(EavtKey::from_datom)
            .collect();
        prop_assert_eq!(
            backend.backend_len(),
            unique_keys.len(),
            "INV-FERR-025 violated: backend_len {} != unique keys {}",
            backend.backend_len(),
            unique_keys.len()
        );

        // backend_is_empty must agree with len.
        prop_assert_eq!(
            backend.backend_is_empty(),
            unique_keys.is_empty(),
            "INV-FERR-025 violated: is_empty disagrees with len"
        );
    }

    /// INV-FERR-027: Read latency — lookup in a store with datoms finds inserted datoms.
    ///
    /// This is a correctness test for index-backed lookups: after inserting
    /// N datoms (up to 1000) into a store, every datom can be found via the
    /// EAVT index. The index ordering enables O(log n + k) range scans.
    ///
    /// Falsification: an inserted datom is absent from the EAVT index.
    #[test]
    fn inv_ferr_027_read_latency_lookup(
        datoms in prop::collection::vec(arb_datom(), 1..100),
    ) {
        let store = Store::from_datoms(datoms.iter().cloned().collect());

        // Every inserted datom must be findable in the EAVT index.
        let eavt_datoms: BTreeSet<&Datom> = store.indexes().eavt_datoms().collect();
        for d in &datoms {
            prop_assert!(
                eavt_datoms.contains(d),
                "INV-FERR-027 violated: datom not found in EAVT index after insert. \
                 entity={:?}, attr={:?}",
                d.entity(), d.attribute()
            );
        }

        // The store must also find every datom in its primary set.
        for d in &datoms {
            prop_assert!(
                store.datom_set().contains(d),
                "INV-FERR-027 violated: datom not found in primary set after insert. \
                 entity={:?}",
                d.entity()
            );
        }
    }

    /// INV-FERR-071: SortedVecBackend produces identical results to OrdMap.
    ///
    /// For any sequence of datoms, both backends return the same values for
    /// get, len, and values iteration order after sort.
    ///
    /// Falsification: any operation returns different results between backends.
    #[test]
    fn inv_ferr_071_sorted_vec_equiv_ordmap(
        datoms in prop::collection::vec(arb_datom(), 1..200),
    ) {
        let mut svb: SortedVecBackend<EavtKey, Datom> = SortedVecBackend::default();
        let mut om: OrdMap<EavtKey, Datom> = OrdMap::new();

        for d in &datoms {
            let key = EavtKey::from_datom(d);
            svb.backend_insert(key, d.clone());
            om.backend_insert(EavtKey::from_datom(d), d.clone());
        }
        svb.sort();

        // Same length (unique key count).
        prop_assert_eq!(
            svb.backend_len(), om.backend_len(),
            "INV-FERR-071: backend_len differs. SortedVec={}, OrdMap={}",
            svb.backend_len(), om.backend_len()
        );

        // Same get results for every inserted key.
        for d in &datoms {
            let key = EavtKey::from_datom(d);
            prop_assert_eq!(
                svb.backend_get(&key), om.backend_get(&key),
                "INV-FERR-071: backend_get differs for datom {:?}",
                d.entity()
            );
        }

        // Same values in iteration order (both sorted by key).
        let svb_vals: Vec<&Datom> = svb.backend_values().collect();
        let om_vals: Vec<&Datom> = om.backend_values().collect();
        prop_assert_eq!(
            svb_vals, om_vals,
            "INV-FERR-071: backend_values iteration order differs"
        );
    }

    /// INV-FERR-071: SortedVecIndexes full pipeline matches OrdMap Indexes.
    ///
    /// Build both index types from the same datom set and verify bijection,
    /// cardinality, datom set equality, and iteration ORDER across indexes.
    ///
    /// Falsification: any index differs between SortedVec and OrdMap backends.
    #[test]
    fn inv_ferr_071_sorted_vec_indexes_full_pipeline(
        store in arb_store(50),
    ) {
        let mut sv: SortedVecIndexes = SortedVecIndexes::from_datoms(store.datoms());
        sv.sort_all();

        prop_assert_eq!(
            sv.len(), store.indexes().len(),
            "INV-FERR-071: SortedVecIndexes len != OrdMap Indexes len"
        );
        prop_assert!(
            sv.verify_bijection(),
            "INV-FERR-071: SortedVecIndexes bijection violated"
        );

        // EAVT: ordered iteration must match (catches sort-order bugs).
        let sv_eavt: Vec<_> = sv.eavt_datoms().collect();
        let om_eavt: Vec<_> = store.indexes().eavt_datoms().collect();
        prop_assert_eq!(sv_eavt, om_eavt,
            "INV-FERR-071: EAVT iteration order differs");

        // Remaining indexes: set equality (ordering is Ord-derived, same argument).
        let sv_aevt: BTreeSet<_> = sv.aevt_datoms().collect();
        let om_aevt: BTreeSet<_> = store.indexes().aevt_datoms().collect();
        prop_assert_eq!(sv_aevt, om_aevt, "INV-FERR-071: AEVT datom sets differ");

        let sv_vaet: BTreeSet<_> = sv.vaet_datoms().collect();
        let om_vaet: BTreeSet<_> = store.indexes().vaet_datoms().collect();
        prop_assert_eq!(sv_vaet, om_vaet, "INV-FERR-071: VAET datom sets differ");

        let sv_avet: BTreeSet<_> = sv.avet_datoms().collect();
        let om_avet: BTreeSet<_> = store.indexes().avet_datoms().collect();
        prop_assert_eq!(sv_avet, om_avet, "INV-FERR-071: AVET datom sets differ");
    }

}
