//! Sharding Kani harnesses.
//!
//! Covers INV-FERR-017.

use std::collections::BTreeSet;

use ferratom::Datom;
use ferratomic_db::{merge::merge, store::Store};

use super::helpers::concrete_datom_set;
#[cfg(not(kani))]
use super::kani;

/// Deterministic entity-hash shard assignment.
fn shard_id(datom: &Datom, shard_count: usize) -> usize {
    let entity = datom.entity();
    let entity_hash = entity.as_bytes();
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
    shards
        .iter()
        .fold(Store::from_datoms(BTreeSet::new()), |acc, s| {
            merge(&acc, s).expect("INV-FERR-017: unshard merge must succeed")
        })
}

/// INV-FERR-017: sharding followed by unsharding is identity.
#[cfg_attr(kani, kani::proof)]
#[cfg_attr(kani, kani::unwind(8))]
#[cfg_attr(not(kani), test)]
#[cfg_attr(not(kani), ignore = "requires Kani verifier")]
fn shard_equivalence() {
    let count: u8 = kani::any();
    kani::assume(count <= 4);
    let datoms = concrete_datom_set(count);
    let shard_count: usize = kani::any();
    kani::assume(shard_count > 0 && shard_count <= 4);

    let store = Store::from_datoms(datoms.clone());
    let shards = shard(&store, shard_count);
    let recomposed = unshard(&shards);

    assert!(
        store.datom_set() == recomposed.datom_set(),
        "INV-FERR-017: shard/unshard round-trip must preserve datom set"
    );
}

/// INV-FERR-017: shards form a pairwise-disjoint partition.
#[cfg_attr(kani, kani::proof)]
#[cfg_attr(kani, kani::unwind(8))]
#[cfg_attr(not(kani), test)]
#[cfg_attr(not(kani), ignore = "requires Kani verifier")]
fn shard_disjointness() {
    let count: u8 = kani::any();
    kani::assume(count <= 4);
    let datoms = concrete_datom_set(count);
    let shard_count: usize = kani::any();
    kani::assume((2..=4).contains(&shard_count));

    let store = Store::from_datoms(datoms);
    let shards = shard(&store, shard_count);

    for i in 0..shards.len() {
        for j in (i + 1)..shards.len() {
            // bd-h2fz: datom_set() returns DatomSetView, not OrdSet.
            // Convert to BTreeSet<&Datom> for intersection check.
            let set_i: BTreeSet<&Datom> = shards[i].datom_set().iter().collect();
            let set_j: BTreeSet<&Datom> = shards[j].datom_set().iter().collect();
            let intersection: BTreeSet<_> = set_i.intersection(&set_j).collect();
            assert!(
                intersection.is_empty(),
                "INV-FERR-017: shards {} and {} share datoms",
                i,
                j
            );
        }
    }
}
