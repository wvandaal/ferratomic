use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use ferratomic_core::merge::merge;

mod common;

fn bench_merge_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("inv_ferr_001_merge_throughput");

    for datom_count in common::SCALE_INPUT_SIZES {
        let left = common::build_store(datom_count);
        let right = common::build_shifted_store(datom_count / 2, datom_count);
        let expected_len = datom_count + datom_count / 2;

        group.throughput(Throughput::Elements(expected_len as u64));
        group.bench_with_input(
            BenchmarkId::new("overlap_50pct", datom_count),
            &datom_count,
            |b, &_datom_count| {
                b.iter(|| {
                    let merged = merge(black_box(&left), black_box(&right))
                        .expect("schemas compatible in benchmark");
                    assert_eq!(
                        merged.len(),
                        expected_len,
                        "INV-FERR-001: merge throughput fixture must preserve set union cardinality",
                    );
                    black_box(merged);
                });
            },
        );
    }

    group.finish();
}

criterion_group!(benches, bench_merge_throughput);
criterion_main!(benches);
