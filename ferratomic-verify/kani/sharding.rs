//! Sharding Kani harnesses.
//!
//! Covers INV-FERR-017.

use std::collections::BTreeSet;

use ferratom::Datom;
use ferratomic_core::{merge::merge, store::Store};

/// Deterministic entity-hash shard assignment.
fn shard_id(datom: &Datom, shard_count: usize) -> usize {
    let entity_hash = datom.entity().as_bytes();
    let hash_u64 = u64::from_le_bytes(
        entity_hash[0..8]
            .try_into()
            .expect("INV-FERR-017: entity hash prefix must provide eight bytes"),
    );
    (hash_u64 % shard_count as u64) as usize
}

/// Partition a store into entity-local shards.
fn shard(store: &Store, shard_count: usize) -> Vec<Store> {
    let mut shards: Vec<BTreeSet<Datom>> = (0..shard_count).map(|_| BTreeSet::new()).collect();

    for datom in store.datoms() {
        let idx = shard_id(datom, shard_count);
        shards[idx].insert(datom.clone());
    }

    shards.into_iter().map(Store::from_datoms).collect()
}

/// Recompose a sharded store by set union.
fn unshard(shards: &[Store]) -> Store {
    shards.iter().fold(Store::empty(), |acc, s| merge(&acc, s))
}

/// INV-FERR-017: sharding followed by unsharding is identity.
#[kani::proof]
#[kani::unwind(8)]
fn shard_equivalence() {
    let datoms: BTreeSet<Datom> = kani::any();
    kani::assume(datoms.len() <= 4);
    let shard_count: usize = kani::any();
    kani::assume(shard_count > 0 && shard_count <= 4);

    let store = Store::from_datoms(datoms.clone());
    let shards = shard(&store, shard_count);
    let recomposed = unshard(&shards);

    assert_eq!(store.datom_set(), recomposed.datom_set());
}

/// INV-FERR-017: shards form a pairwise-disjoint partition.
#[kani::proof]
#[kani::unwind(8)]
fn shard_disjointness() {
    let datoms: BTreeSet<Datom> = kani::any();
    kani::assume(datoms.len() <= 4);
    let shard_count: usize = kani::any();
    kani::assume(shard_count >= 2 && shard_count <= 4);

    let store = Store::from_datoms(datoms);
    let shards = shard(&store, shard_count);

    for i in 0..shards.len() {
        for j in (i + 1)..shards.len() {
            let intersection: BTreeSet<_> = shards[i]
                .datom_set()
                .intersection(shards[j].datom_set())
                .collect();
            assert!(
                intersection.is_empty(),
                "INV-FERR-017: shards {} and {} share datoms",
                i,
                j
            );
        }
    }
}
