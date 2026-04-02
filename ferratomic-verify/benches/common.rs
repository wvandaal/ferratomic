// Bench helper module — individual items are used across multiple criterion bench
// targets (cold_start, merge_throughput, read_latency, etc.) but each target
// only uses a subset. Every item is used by at least one bench target.

use std::{collections::BTreeSet, path::Path, sync::Arc};

use ferratom::{
    AgentId, Attribute, AttributeDef, Datom, EntityId, FerraError, Op, Schema, TxId, Value,
};
use ferratomic_core::{
    checkpoint::write_checkpoint,
    db::Database,
    indexes::EavtKey,
    storage::{checkpoint_path, wal_path},
    store::Store,
    writer::{Committed, Transaction},
};
use tempfile::TempDir;

pub const SCALE_INPUT_SIZES: [usize; 3] = [1_000, 10_000, 100_000];
pub const WRITE_TRANSACTION_COUNTS: [usize; 2] = [1_000, 10_000];

const BENCH_AGENT_BYTES: [u8; 16] = [7u8; 16];
const DOC_ATTRIBUTE: &str = "db/doc";

pub fn bench_agent() -> AgentId {
    AgentId::from_bytes(BENCH_AGENT_BYTES)
}

pub fn doc_entity(index: usize) -> EntityId {
    EntityId::from_content(format!("entity-{index}").as_bytes())
}

pub fn doc_value(index: usize) -> Value {
    Value::String(Arc::from(format!("document-{index}").as_str()))
}

pub fn doc_datom(index: usize) -> Datom {
    Datom::new(
        doc_entity(index),
        Attribute::from(DOC_ATTRIBUTE),
        doc_value(index),
        TxId::new(index as u64 + 1, 0, 1),
        Op::Assert,
    )
}

pub fn build_store(count: usize) -> Store {
    build_shifted_store(0, count)
}

pub fn build_shifted_store(start: usize, count: usize) -> Store {
    let datoms = (start..start + count)
        .map(doc_datom)
        .collect::<BTreeSet<_>>();
    Store::from_datoms(datoms)
}

pub fn build_database(count: usize) -> Database {
    Database::from_store(build_store(count))
}

pub fn lookup_key(index: usize) -> EavtKey {
    EavtKey::from_datom(&doc_datom(index))
}

pub fn logical_user_bytes(start: usize, count: usize) -> usize {
    (start..start + count).map(serialized_datom_len).sum()
}

pub fn measure_write_amplification(tx_count: usize) -> f64 {
    let dir = tempfile::tempdir().expect("write amplification bench tempdir");
    let db = Database::genesis_with_wal(&wal_path(dir.path())).expect("create WAL-backed db");
    transact_batched(&db, 0, tx_count, 1).expect("write amplification transact range");

    let wal_bytes = std::fs::metadata(wal_path(dir.path()))
        .expect("WAL metadata")
        .len() as f64;
    let logical_bytes = logical_user_bytes(0, tx_count) as f64;
    wal_bytes / logical_bytes
}

pub fn prepare_cold_start_dir(total_datoms: usize) -> TempDir {
    let dir = tempfile::tempdir().expect("cold-start bench tempdir");
    let db = Database::genesis_with_wal(&wal_path(dir.path())).expect("create WAL-backed db");
    let (checkpoint_datoms, wal_datoms) = split_checkpoint_and_wal(total_datoms);

    transact_batched(&db, 0, checkpoint_datoms, 50).expect("seed checkpoint segment");
    checkpoint_database(&db, dir.path()).expect("write checkpoint");
    transact_batched(&db, checkpoint_datoms, wal_datoms, 50).expect("seed WAL delta");

    dir
}

fn serialized_datom_len(index: usize) -> usize {
    bincode::serialize(&doc_datom(index))
        .expect("serialize benchmark datom")
        .len()
}

fn checkpoint_database(db: &Database, data_dir: &Path) -> Result<(), FerraError> {
    let store = checkpoint_store(db);
    write_checkpoint(&store, &checkpoint_path(data_dir))
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

fn schema_attrs(schema: &Schema) -> Vec<(String, AttributeDef)> {
    schema
        .iter()
        .map(|(attr, def)| (attr.as_str().to_owned(), def.clone()))
        .collect()
}

fn split_checkpoint_and_wal(total_datoms: usize) -> (usize, usize) {
    let wal_datoms = (total_datoms / 5).max(1);
    let checkpoint_datoms = total_datoms.saturating_sub(wal_datoms);
    (checkpoint_datoms, wal_datoms)
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
