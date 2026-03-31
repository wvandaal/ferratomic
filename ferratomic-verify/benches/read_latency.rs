use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};

mod common;

fn bench_read_latency(c: &mut Criterion) {
    let mut group = c.benchmark_group("inv_ferr_027_read_latency_eavt");

    for datom_count in common::SCALE_INPUT_SIZES {
        let store = common::build_store(datom_count);
        let key = common::lookup_key(datom_count / 2);

        group.throughput(Throughput::Elements(datom_count as u64));
        group.bench_with_input(
            BenchmarkId::new("eavt_get", datom_count),
            &datom_count,
            |b, &_datom_count| {
                b.iter(|| {
                    let datom = store.indexes().eavt().get(black_box(&key));
                    assert!(
                        datom.is_some(),
                        "INV-FERR-027: benchmark lookup key must exist"
                    );
                    black_box(datom);
                });
            },
        );
    }

    group.finish();
}

criterion_group!(benches, bench_read_latency);
criterion_main!(benches);
