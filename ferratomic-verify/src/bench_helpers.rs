//! Shared benchmark helper functions.
//!
//! Provides datom factories, store builders, and constants used across
//! multiple Criterion bench targets. Lives in the library (not a local
//! `mod common`) so each bench binary imports compiled code rather than
//! re-compiling a copy — eliminating per-binary `dead_code` warnings.
//!
//! **Library code constraint**: no `unwrap()`, `expect()`, or `panic!()`.
//! All fallible operations return `Result<T, FerraError>`.

use std::{collections::BTreeSet, sync::Arc};

use ferratom::{
    AgentId, Attribute, AttributeDef, Datom, EntityId, FerraError, Op, Schema, TxId, Value,
};
use ferratomic_db::{
    db::Database,
    indexes::EavtKey,
    store::Store,
    writer::{Committed, Transaction},
};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Standard benchmark scale factors: 1K, 10K, 100K datoms.
pub const SCALE_INPUT_SIZES: [usize; 3] = [1_000, 10_000, 100_000];

/// Transaction counts for write-path benchmarks.
pub const WRITE_TRANSACTION_COUNTS: [usize; 2] = [1_000, 10_000];

/// Fixed agent identity for benchmark transactions.
pub const BENCH_AGENT_BYTES: [u8; 16] = [7u8; 16];

/// Attribute name used by all benchmark datoms.
pub const DOC_ATTRIBUTE: &str = "db/doc";

// ---------------------------------------------------------------------------
// Datom factories
// ---------------------------------------------------------------------------

/// Returns the fixed benchmark agent identity.
pub fn bench_agent() -> AgentId {
    AgentId::from_bytes(BENCH_AGENT_BYTES)
}

/// Content-addressed entity ID for benchmark index `index`.
pub fn doc_entity(index: usize) -> EntityId {
    EntityId::from_content(format!("entity-{index}").as_bytes())
}

/// String value for benchmark index `index`.
pub fn doc_value(index: usize) -> Value {
    Value::String(Arc::from(format!("document-{index}").as_str()))
}

/// Complete benchmark datom at the given index.
pub fn doc_datom(index: usize) -> Datom {
    Datom::new(
        doc_entity(index),
        Attribute::from(DOC_ATTRIBUTE),
        doc_value(index),
        TxId::new(index as u64 + 1, 0, 1),
        Op::Assert,
    )
}

/// EAVT lookup key for the datom at `index`.
pub fn lookup_key(index: usize) -> EavtKey {
    EavtKey::from_datom(&doc_datom(index))
}

// ---------------------------------------------------------------------------
// Store / Database builders
// ---------------------------------------------------------------------------

/// Build a `Store` from `count` datoms starting at index `start`.
///
/// Returns a `Positional`-representation store (no indexes promoted).
/// Callers needing `OrdMap` representation should call `store.promote()`
/// on the result.
pub fn build_shifted_store(start: usize, count: usize) -> Store {
    let datoms = (start..start + count)
        .map(doc_datom)
        .collect::<BTreeSet<_>>();
    Store::from_datoms(datoms)
}

/// Build a `Store` from `count` datoms starting at index 0.
pub fn build_store(count: usize) -> Store {
    build_shifted_store(0, count)
}

/// Build a `Database` wrapping a store of `count` datoms.
pub fn build_database(count: usize) -> Database {
    Database::from_store(build_store(count))
}

// ---------------------------------------------------------------------------
// Transaction helpers
// ---------------------------------------------------------------------------

/// Build a committed transaction batch spanning `[start..start+count)`.
pub fn build_committed_batch(
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

/// Transact `total_datoms` datoms in batches of `batch_size`, starting at
/// index `start`.
pub fn transact_batched(
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

// ---------------------------------------------------------------------------
// Schema / checkpoint helpers
// ---------------------------------------------------------------------------

/// Extract schema attribute definitions as owned `(name, def)` pairs.
pub fn schema_attrs(schema: &Schema) -> Vec<(String, AttributeDef)> {
    schema
        .iter()
        .map(|(attr, def)| (attr.as_str().to_owned(), def.clone()))
        .collect()
}
