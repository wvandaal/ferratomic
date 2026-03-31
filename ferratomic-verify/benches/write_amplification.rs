use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};

mod common;

fn bench_write_amplification(c: &mut Criterion) {
    let mut group = c.benchmark_group("inv_ferr_026_write_amplification");
    group.sample_size(10);

    for tx_count in common::WRITE_TRANSACTION_COUNTS {
        let logical_bytes = common::logical_user_bytes(0, tx_count) as u64;
        let baseline_ratio = common::measure_write_amplification(tx_count);
        if baseline_ratio >= 10.0 {
            eprintln!(
                "INV-FERR-026 soft threshold exceeded at {tx_count} tx: WA={baseline_ratio:.3}x"
            );
        }

        group.throughput(Throughput::Bytes(logical_bytes));
        group.bench_with_input(
            BenchmarkId::from_parameter(tx_count),
            &tx_count,
            |b, &tx_count| {
                b.iter(|| black_box(common::measure_write_amplification(tx_count)));
            },
        );
    }

    group.finish();
}

criterion_group!(benches, bench_write_amplification);
criterion_main!(benches);
