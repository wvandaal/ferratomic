use std::{path::Path, sync::Arc};

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use ferratom::{AgentId, Attribute, AttributeDef, EntityId, FerraError, Schema, Value};
use ferratomic_core::{
    checkpoint::write_checkpoint,
    db::Database,
    storage::{self, checkpoint_path, wal_path, RecoveryLevel},
    store::Store,
    writer::{Committed, Transaction},
};
use tempfile::TempDir;

const SCALE_INPUT_SIZES: [usize; 3] = [1_000, 10_000, 100_000];

const BENCH_AGENT_BYTES: [u8; 16] = [7u8; 16];
const DOC_ATTRIBUTE: &str = "db/doc";

fn bench_agent() -> AgentId {
    AgentId::from_bytes(BENCH_AGENT_BYTES)
}

fn doc_entity(index: usize) -> EntityId {
    EntityId::from_content(format!("entity-{index}").as_bytes())
}

fn doc_value(index: usize) -> Value {
    Value::String(Arc::from(format!("document-{index}").as_str()))
}

fn build_committed_batch(
    schema: &Schema,
    agent: AgentId,
    start: usize,
    count: usize,
) -> Result<Transaction<Committed>, FerraError> {
    let mut tx = Transaction::new(agent);
    for index in start..start + count {
        tx = tx.assert_datom(
            doc_entity(index),
            Attribute::from(DOC_ATTRIBUTE),
            doc_value(index),
        );
    }
    tx.commit(schema).map_err(Into::into)
}

fn transact_batched(
    db: &Database,
    start: usize,
    total_datoms: usize,
    batch_size: usize,
) -> Result<(), FerraError> {
    let schema = db.schema();
    let agent = bench_agent();
    let end = start + total_datoms;
    let chunk_size = batch_size.max(1);
    let mut next = start;

    while next < end {
        let chunk_len = (end - next).min(chunk_size);
        let tx = build_committed_batch(&schema, agent, next, chunk_len)?;
        db.transact(tx)?;
        next += chunk_len;
    }

    Ok(())
}

fn schema_attrs(schema: &Schema) -> Vec<(String, AttributeDef)> {
    schema
        .iter()
        .map(|(attr, def)| (attr.as_str().to_owned(), def.clone()))
        .collect()
}

fn checkpoint_store(db: &Database) -> Store {
    let schema = db.schema();
    let schema_attrs = schema_attrs(&schema);
    let datoms = db.snapshot().datoms().cloned().collect::<Vec<_>>();
    Store::from_checkpoint(
        db.epoch(),
        Store::genesis().genesis_agent(),
        schema_attrs,
        datoms,
    )
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
