use std::path::Path;

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use ferratom::FerraError;
use ferratomic_db::{
    checkpoint::write_checkpoint,
    db::Database,
    storage::{self, checkpoint_path, wal_path, RecoveryLevel},
    store::Store,
};
use ferratomic_verify::bench_helpers::{schema_attrs, transact_batched, SCALE_INPUT_SIZES};
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Cold-start-specific helpers (not shared with other bench targets)
// ---------------------------------------------------------------------------

fn checkpoint_store(db: &Database) -> Store {
    let schema = db.schema();
    let attrs = schema_attrs(&schema);
    let datoms = db.snapshot().datoms().cloned().collect::<Vec<_>>();
    Store::from_checkpoint(db.epoch(), Store::genesis().genesis_agent(), attrs, datoms)
}

fn checkpoint_database(db: &Database, data_dir: &Path) -> Result<(), FerraError> {
    let store = checkpoint_store(db);
    write_checkpoint(&store, &checkpoint_path(data_dir))
}

fn split_checkpoint_and_wal(total_datoms: usize) -> (usize, usize) {
    let wal_datoms = (total_datoms / 5).max(1);
    let checkpoint_datoms = total_datoms.saturating_sub(wal_datoms);
    (checkpoint_datoms, wal_datoms)
}

fn prepare_cold_start_dir(total_datoms: usize) -> TempDir {
    let dir = tempfile::tempdir().expect("cold-start bench tempdir");
    let db = Database::genesis_with_wal(&wal_path(dir.path())).expect("create WAL-backed db");
    let (checkpoint_datoms, wal_datoms) = split_checkpoint_and_wal(total_datoms);

    transact_batched(&db, 0, checkpoint_datoms, 50).expect("seed checkpoint segment");
    checkpoint_database(&db, dir.path()).expect("write checkpoint");
    transact_batched(&db, checkpoint_datoms, wal_datoms, 50).expect("seed WAL delta");

    dir
}

// ---------------------------------------------------------------------------
// Benchmarks
// ---------------------------------------------------------------------------

fn bench_cold_start(c: &mut Criterion) {
    let mut group = c.benchmark_group("inv_ferr_028_cold_start");
    group.sample_size(10);

    for datom_count in SCALE_INPUT_SIZES {
        let dir = prepare_cold_start_dir(datom_count);

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
                    // bd-tnkm: The fixture seeds `datom_count` user datoms,
                    // but each transaction also adds 2 metadata datoms
                    // (`:tx/time`, `:tx/agent`), so recovered count > datom_count.
                    // Assert `>=` as a lower bound on set-union fidelity.
                    let recovered = result.database.snapshot().datoms().count();
                    assert!(
                        recovered >= datom_count,
                        "INV-FERR-014: recovered fixture must contain at least \
                         {datom_count} user datoms, got {recovered}",
                    );
                    black_box(recovered);
                });
            },
        );
    }

    group.finish();
}

/// INV-FERR-028, INV-FERR-014: benchmark checkpoint round-trip at specific
/// datom counts (1K and 10K).
///
/// Each iteration prepares a directory with a checkpoint plus WAL delta,
/// then measures cold-start recovery latency. The recovered database must
/// contain all seeded datoms, confirming durable round-trip fidelity.
fn bench_checkpoint_roundtrip(c: &mut Criterion) {
    let mut group = c.benchmark_group("inv_ferr_028_checkpoint_roundtrip");
    group.sample_size(10);

    let roundtrip_sizes: [usize; 2] = [1_000, 10_000];

    for datom_count in roundtrip_sizes {
        let dir = prepare_cold_start_dir(datom_count);
        let label = match datom_count {
            1_000 => "checkpoint_roundtrip_1k",
            10_000 => "checkpoint_roundtrip_10k",
            _ => "checkpoint_roundtrip",
        };

        group.throughput(Throughput::Elements(datom_count as u64));
        group.bench_with_input(
            BenchmarkId::new(label, datom_count),
            &datom_count,
            |b, &_datom_count| {
                b.iter(|| {
                    let result = storage::cold_start(dir.path()).expect("cold start benchmark");
                    assert_eq!(
                        result.level,
                        RecoveryLevel::CheckpointPlusWal,
                        "INV-FERR-028: roundtrip fixture must exercise checkpoint+WAL recovery",
                    );
                    // bd-tnkm: Same rationale as cold_start benchmark above.
                    // Metadata datoms inflate the count beyond `datom_count`.
                    let recovered = result.database.snapshot().datoms().count();
                    assert!(
                        recovered >= datom_count,
                        "INV-FERR-014: recovered roundtrip must contain at least \
                         {datom_count} user datoms, got {recovered}",
                    );
                    black_box(recovered);
                });
            },
        );
    }

    group.finish();
}

criterion_group!(benches, bench_cold_start, bench_checkpoint_roundtrip);
criterion_main!(benches);
