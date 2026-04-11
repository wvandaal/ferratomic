//! Scale benchmark: prolly tree at 1M and 10M entries.
//!
//! Answers: what is the actual bottleneck at target scale?
//! Measures build time, read time, diff time, transfer time, and peak memory.

use std::{collections::BTreeMap, time::Instant};

use ferratomic_db::prolly::{
    boundary::DEFAULT_PATTERN_WIDTH,
    build::build_prolly_tree,
    chunk::{ChunkStore, MemoryChunkStore},
    diff::{diff, diff_exact},
    read::{read_prolly_tree, read_prolly_tree_vec},
    transfer::{ChunkTransfer, RecursiveTransfer},
};

fn make_kvs(n: u32) -> BTreeMap<Vec<u8>, Vec<u8>> {
    let mut kvs = BTreeMap::new();
    for i in 0..n {
        kvs.insert(i.to_be_bytes().to_vec(), vec![0u8; 32]);
    }
    kvs
}

fn bytes_in_store(store: &MemoryChunkStore) -> usize {
    store
        .all_addrs()
        .unwrap()
        .iter()
        .map(|addr| store.get_chunk(addr).unwrap().map_or(0, |c| c.len()))
        .sum()
}

fn run_scale_test(n: u32) {
    println!("\n============================================================");
    println!("  PROLLY TREE SCALE TEST: {n} entries");
    println!("============================================================\n");

    // ── BUILD ──────────────────────────────────────────────────
    println!("[1/5] Building {n}-entry tree...");
    let store = MemoryChunkStore::new();
    let kvs = make_kvs(n);
    let t0 = Instant::now();
    let root = build_prolly_tree(&kvs, &store, DEFAULT_PATTERN_WIDTH).expect("build must succeed");
    let build_time = t0.elapsed();
    let store_bytes = bytes_in_store(&store);
    let chunk_count = store.all_addrs().unwrap().len();
    println!(
        "  Build:  {:.2?}  ({} chunks, {:.2} MB total chunk data)",
        build_time,
        chunk_count,
        store_bytes as f64 / (1024.0 * 1024.0),
    );

    // ── READ (BTreeMap vs Vec) ──────────────────────────────
    println!("[2/6] Reading full tree (BTreeMap)...");
    let t0 = Instant::now();
    let recovered = read_prolly_tree(&root, &store).expect("read btree");
    let read_btree_time = t0.elapsed();
    assert_eq!(recovered.len(), n as usize);
    println!(
        "  Read BTreeMap: {:.2?}  ({} entries)",
        read_btree_time,
        recovered.len()
    );
    drop(recovered);

    println!("       Reading full tree (Vec)...");
    let t0 = Instant::now();
    let recovered_vec = read_prolly_tree_vec(&root, &store).expect("read vec");
    let read_vec_time = t0.elapsed();
    assert_eq!(recovered_vec.len(), n as usize);
    let speedup = read_btree_time.as_secs_f64() / read_vec_time.as_secs_f64();
    println!(
        "  Read Vec:      {:.2?}  ({:.1}x faster)",
        read_vec_time, speedup,
    );
    drop(recovered_vec);

    // ── DIFF IDENTICAL (O(1) fast path) ───────────────────────
    println!("[3/5] Diff identical trees (O(1) fast path)...");
    let t0 = Instant::now();
    for _ in 0..1000 {
        let count: usize = diff(&root, &root, &store)
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
            .len();
        assert_eq!(count, 0);
    }
    let diff_identical_time = t0.elapsed() / 1000;
    println!(
        "  Diff identical: {:.2?} per call (1000 iterations)",
        diff_identical_time
    );

    // ── DIFF WITH d CHANGES ───────────────────────────────────
    let d = 100u32;
    println!("[4/5] Diff with d={d} changes...");
    let mut kvs2 = kvs.clone();
    for i in 0..d {
        let key = (i * (n / d)).to_be_bytes().to_vec();
        kvs2.insert(key, vec![0xFFu8; 32]);
    }
    let root2 = build_prolly_tree(&kvs2, &store, DEFAULT_PATTERN_WIDTH).expect("build modified");

    // Raw diff
    let t0 = Instant::now();
    let raw_entries: Vec<_> = diff(&root, &root2, &store)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let raw_diff_time = t0.elapsed();
    println!(
        "  Diff raw:   {:.2?}  ({} entries, includes phantoms)",
        raw_diff_time,
        raw_entries.len(),
    );

    // Exact diff
    let t0 = Instant::now();
    let exact_entries = diff_exact(&root, &root2, &store).unwrap();
    let exact_diff_time = t0.elapsed();
    println!(
        "  Diff exact: {:.2?}  ({} entries, exact symmetric diff)",
        exact_diff_time,
        exact_entries.len(),
    );

    // ── TRANSFER ──────────────────────────────────────────────
    println!("[5/5] Full transfer to empty store...");
    let dst = MemoryChunkStore::new();
    let xfer = RecursiveTransfer;
    let t0 = Instant::now();
    let result = xfer.transfer(&store, &dst, &root).unwrap();
    let transfer_time = t0.elapsed();
    println!(
        "  Transfer:   {:.2?}  ({} chunks, {:.2} MB, {} skipped)",
        transfer_time,
        result.chunks_transferred,
        result.bytes_transferred as f64 / (1024.0 * 1024.0),
        result.chunks_skipped,
    );

    // Incremental transfer (only d changes)
    let dst2 = MemoryChunkStore::new();
    xfer.transfer(&store, &dst2, &root).unwrap(); // baseline
    let t0 = Instant::now();
    let incr = xfer.transfer(&store, &dst2, &root2).unwrap();
    let incr_time = t0.elapsed();
    println!(
        "  Incremental: {:.2?}  ({} new chunks, {} skipped)",
        incr_time, incr.chunks_transferred, incr.chunks_skipped,
    );

    // ── SUMMARY ───────────────────────────────────────────────
    println!("\n  SUMMARY ({n} entries):");
    println!("  ├─ Build:          {build_time:.2?}");
    println!("  ├─ Read BTreeMap:  {read_btree_time:.2?}");
    println!("  ├─ Read Vec:       {read_vec_time:.2?}  ({speedup:.1}x faster)");
    println!("  ├─ Diff identical: {diff_identical_time:.2?}");
    println!(
        "  ├─ Diff d={d}:     {exact_diff_time:.2?}  ({} entries)",
        exact_entries.len()
    );
    println!(
        "  ├─ Transfer full:  {transfer_time:.2?}  ({:.2} MB)",
        result.bytes_transferred as f64 / (1024.0 * 1024.0)
    );
    println!(
        "  ├─ Transfer incr:  {incr_time:.2?}  ({} new chunks)",
        incr.chunks_transferred
    );
    println!(
        "  └─ Store size:     {:.2} MB in {} chunks",
        store_bytes as f64 / (1024.0 * 1024.0),
        chunk_count
    );
}

fn main() {
    run_scale_test(1_000_000);
    run_scale_test(10_000_000);
}
