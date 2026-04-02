use std::sync::Arc;

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use ferratom::{AgentId, Attribute, Datom, EntityId, FerraError, Op, Schema, TxId, Value};
use ferratomic_core::{
    db::Database,
    storage::wal_path,
    writer::{Committed, Transaction},
};

const WRITE_TRANSACTION_COUNTS: [usize; 2] = [1_000, 10_000];

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

fn doc_datom(index: usize) -> Datom {
    Datom::new(
        doc_entity(index),
        Attribute::from(DOC_ATTRIBUTE),
        doc_value(index),
        TxId::new(index as u64 + 1, 0, 1),
        Op::Assert,
    )
}

fn serialized_datom_len(index: usize) -> usize {
    bincode::serialize(&doc_datom(index))
        .expect("serialize benchmark datom")
        .len()
}

fn logical_user_bytes(start: usize, count: usize) -> usize {
    (start..start + count).map(serialized_datom_len).sum()
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
