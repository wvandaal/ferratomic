use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use ferratomic_db::{db::Database, storage::wal_path};
use ferratomic_verify::bench_helpers::{doc_datom, transact_batched, WRITE_TRANSACTION_COUNTS};

// ---------------------------------------------------------------------------
// Write-amplification-specific helpers (not shared with other bench targets)
// ---------------------------------------------------------------------------

fn serialized_datom_len(index: usize) -> usize {
    bincode::serialize(&doc_datom(index))
        .expect("serialize benchmark datom")
        .len()
}

fn logical_user_bytes(start: usize, count: usize) -> usize {
    (start..start + count).map(serialized_datom_len).sum()
}

fn measure_write_amplification(tx_count: usize) -> f64 {
    let dir = tempfile::tempdir().expect("write amplification bench tempdir");
    let db = Database::genesis_with_wal(&wal_path(dir.path())).expect("create WAL-backed db");
    transact_batched(&db, 0, tx_count, 1).expect("write amplification transact range");

    let wal_bytes = std::fs::metadata(wal_path(dir.path()))
        .expect("WAL metadata")
        .len() as f64;
    let logical_bytes = logical_user_bytes(0, tx_count) as f64;
    wal_bytes / logical_bytes
}

// ---------------------------------------------------------------------------
// Benchmarks
// ---------------------------------------------------------------------------

fn bench_write_amplification(c: &mut Criterion) {
    let mut group = c.benchmark_group("inv_ferr_026_write_amplification");
    group.sample_size(10);

    for tx_count in WRITE_TRANSACTION_COUNTS {
        let logical_bytes = logical_user_bytes(0, tx_count) as u64;
        let baseline_ratio = measure_write_amplification(tx_count);
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
                b.iter(|| black_box(measure_write_amplification(tx_count)));
            },
        );
    }

    group.finish();
}

criterion_group!(benches, bench_write_amplification);
criterion_main!(benches);
