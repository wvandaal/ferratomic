//! Criterion benchmarks for prolly tree operations.
//!
//! Validates the performance claims from INV-FERR-046 (build), INV-FERR-047
//! (diff), INV-FERR-048 (transfer), INV-FERR-049 (read) with empirical
//! measurements at 1K, 10K, and 100K scale.

use std::collections::BTreeMap;

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use ferratomic_db::prolly::{
    boundary::DEFAULT_PATTERN_WIDTH,
    build::build_prolly_tree,
    chunk::MemoryChunkStore,
    diff::{diff, diff_exact},
    read::read_prolly_tree,
    transfer::{ChunkTransfer, RecursiveTransfer},
};

fn make_kvs(n: u32) -> BTreeMap<Vec<u8>, Vec<u8>> {
    let mut kvs = BTreeMap::new();
    for i in 0..n {
        kvs.insert(i.to_be_bytes().to_vec(), vec![0u8; 32]);
    }
    kvs
}

fn bench_build(c: &mut Criterion) {
    let mut group = c.benchmark_group("prolly_build");
    for &size in &[1_000u32, 10_000, 100_000] {
        let kvs = make_kvs(size);
        group.bench_with_input(BenchmarkId::from_parameter(size), &kvs, |b, kvs| {
            b.iter(|| {
                let store = MemoryChunkStore::new();
                black_box(build_prolly_tree(kvs, &store, DEFAULT_PATTERN_WIDTH).unwrap());
            });
        });
    }
    group.finish();
}

fn bench_read(c: &mut Criterion) {
    let mut group = c.benchmark_group("prolly_read");
    for &size in &[1_000u32, 10_000, 100_000] {
        let kvs = make_kvs(size);
        let store = MemoryChunkStore::new();
        let root = build_prolly_tree(&kvs, &store, DEFAULT_PATTERN_WIDTH).unwrap();

        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, _| {
            b.iter(|| {
                black_box(read_prolly_tree(&root, &store).unwrap());
            });
        });
    }
    group.finish();
}

fn bench_diff_identical(c: &mut Criterion) {
    let mut group = c.benchmark_group("prolly_diff_identical");
    for &size in &[1_000u32, 10_000, 100_000] {
        let kvs = make_kvs(size);
        let store = MemoryChunkStore::new();
        let root = build_prolly_tree(&kvs, &store, DEFAULT_PATTERN_WIDTH).unwrap();

        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, _| {
            b.iter(|| {
                let count: usize = diff(&root, &root, &store)
                    .collect::<Result<Vec<_>, _>>()
                    .unwrap()
                    .len();
                black_box(count);
            });
        });
    }
    group.finish();
}

fn bench_diff_changed(c: &mut Criterion) {
    let mut group = c.benchmark_group("prolly_diff_d_changes");
    for &size in &[10_000u32, 100_000] {
        let kvs1 = make_kvs(size);
        let store = MemoryChunkStore::new();
        let root1 = build_prolly_tree(&kvs1, &store, DEFAULT_PATTERN_WIDTH).unwrap();

        // Modify 10 keys (d=10 changes in an n-entry store)
        let mut kvs2 = kvs1.clone();
        for i in 0u32..10 {
            let key = (i * (size / 10)).to_be_bytes().to_vec();
            kvs2.insert(key, vec![0xFFu8; 32]);
        }
        let root2 = build_prolly_tree(&kvs2, &store, DEFAULT_PATTERN_WIDTH).unwrap();

        group.bench_with_input(BenchmarkId::new("diff_raw", size), &size, |b, _| {
            b.iter(|| {
                let entries: Vec<_> = diff(&root1, &root2, &store)
                    .collect::<Result<Vec<_>, _>>()
                    .unwrap();
                black_box(entries.len());
            });
        });

        group.bench_with_input(BenchmarkId::new("diff_exact", size), &size, |b, _| {
            b.iter(|| {
                let entries = diff_exact(&root1, &root2, &store).unwrap();
                black_box(entries.len());
            });
        });
    }
    group.finish();
}

fn bench_transfer(c: &mut Criterion) {
    let mut group = c.benchmark_group("prolly_transfer");
    for &size in &[1_000u32, 10_000] {
        let kvs = make_kvs(size);
        let src = MemoryChunkStore::new();
        let root = build_prolly_tree(&kvs, &src, DEFAULT_PATTERN_WIDTH).unwrap();

        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, _| {
            b.iter(|| {
                let dst = MemoryChunkStore::new();
                let xfer = RecursiveTransfer;
                black_box(xfer.transfer(&src, &dst, &root).unwrap());
            });
        });
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_build,
    bench_read,
    bench_diff_identical,
    bench_diff_changed,
    bench_transfer,
);
criterion_main!(benches);
