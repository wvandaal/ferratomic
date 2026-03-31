use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use ferratomic_core::storage::{self, RecoveryLevel};

mod common;

fn bench_cold_start(c: &mut Criterion) {
    let mut group = c.benchmark_group("inv_ferr_028_cold_start");
    group.sample_size(10);

    for datom_count in common::SCALE_INPUT_SIZES {
        let dir = common::prepare_cold_start_dir(datom_count);

        group.throughput(Throughput::Elements(datom_count as u64));
        group.bench_with_input(
            BenchmarkId::new("checkpoint_plus_wal", datom_count),
            &datom_count,
            |b, &_datom_count| {
                b.iter(|| {
                    let result = storage::cold_start(dir.path()).expect("cold start benchmark");
                    assert_eq!(
                        result.level,
                        RecoveryLevel::CheckpointPlusWal,
                        "INV-FERR-028: prepared fixture must exercise checkpoint+WAL recovery",
                    );
                    let recovered = result.database.snapshot().datoms().count();
                    assert!(
                        recovered >= datom_count,
                        "INV-FERR-014: recovered fixture must contain all seeded datoms",
                    );
                    black_box(recovered);
                });
            },
        );
    }

    group.finish();
}

criterion_group!(benches, bench_cold_start);
criterion_main!(benches);
