//! Hard threshold assertions for performance invariants.
//!
//! INV-FERR-026: Write amplification < 10x.
//! INV-FERR-027: EAVT read latency usable at scale.
//! INV-FERR-028: Cold start recovery < 5s for 100K datoms.
//!
//! These are pass/fail `#[test]` functions, NOT Criterion benchmarks.
//! They enforce spec-defined performance bounds as regression gates.

use std::time::Instant;

use ferratom::{AgentId, Attribute, EntityId, Value};
use ferratomic_core::checkpoint::write_checkpoint;
use ferratomic_core::db::Database;
use ferratomic_core::indexes::EavtKey;
use ferratomic_core::storage::cold_start;
use ferratomic_core::store::Store;
use ferratomic_core::writer::Transaction;

// ---------------------------------------------------------------------------
// Constants: spec-defined thresholds (generous for CI)
// ---------------------------------------------------------------------------

/// INV-FERR-026: maximum write amplification ratio.
/// Spec target is < 10x; we use exactly 10.0 as the hard ceiling.
const MAX_WRITE_AMPLIFICATION: f64 = 10.0;

/// INV-FERR-027: maximum average read latency per EAVT lookup.
/// 1 ms = 1_000_000 ns. Generous for CI machines under load.
const MAX_READ_LATENCY_NS: u128 = 1_000_000;

/// INV-FERR-028: maximum cold start recovery time in seconds.
/// Spec target is < 5s for 100K datoms in release builds. We test
/// with 1K datoms in debug builds and keep the 5s ceiling. This
/// catches O(n^2) pathologies while remaining CI-friendly.
const MAX_COLD_START_SECS: u64 = 5;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a genesis Database and transact `count` datoms into it via
/// schema-valid transactions (one datom per tx for worst-case WA).
///
/// Returns the Database and the total logical bytes written (sum of
/// serialized datom sizes before WAL framing).
fn build_store_with_datoms(count: usize) -> Store {
    let mut store = Store::genesis();
    let agent = AgentId::from_bytes([1u8; 16]);

    for i in 0..count {
        let tx = Transaction::new(agent)
            .assert_datom(
                EntityId::from_content(format!("entity-{i}").as_bytes()),
                Attribute::from("db/doc"),
                Value::String(format!("value-{i}").into()),
            )
            .commit_unchecked();
        store
            .transact(tx)
            .unwrap_or_else(|e| panic!("transact {i} failed: {e}"));
    }

    store
}

/// Build a Database with WAL and transact `count` datoms, returning
/// the WAL file size and the sum of logical datom payload sizes.
fn measure_write_amplification(count: usize) -> f64 {
    let dir = tempfile::TempDir::new().expect("create temp dir for WA test");
    let wal_path = dir.path().join("wa_test.wal");

    let db = Database::genesis_with_wal(&wal_path)
        .expect("INV-FERR-026: genesis_with_wal must succeed");
    let agent = AgentId::from_bytes([2u8; 16]);

    let mut logical_bytes: u64 = 0;

    for i in 0..count {
        let entity = EntityId::from_content(format!("wa-entity-{i}").as_bytes());
        let attr = Attribute::from("db/doc");
        let val = Value::String(format!("wa-value-{i}").into());

        // Approximate logical size: entity(32) + attr(~6) + value(~12) + tx(14) + op(1) = ~65
        // We use serde_json serialization of a single datom as the logical unit.
        let datom = ferratom::Datom::new(
            entity.clone(),
            attr.clone(),
            val.clone(),
            ferratom::TxId::new(0, 0, 0),
            ferratom::Op::Assert,
        );
        let serialized = serde_json::to_vec(&[&datom])
            .expect("serialize datom for logical size");
        logical_bytes += serialized.len() as u64;

        let tx = Transaction::new(agent)
            .assert_datom(entity, attr, val)
            .commit_unchecked();
        db.transact(tx)
            .unwrap_or_else(|e| panic!("INV-FERR-026: transact {i} failed: {e}"));
    }

    // Physical bytes = WAL file size on disk.
    let wal_size = std::fs::metadata(&wal_path)
        .expect("INV-FERR-026: WAL file must exist after transactions")
        .len();

    if logical_bytes == 0 {
        return 0.0;
    }

    wal_size as f64 / logical_bytes as f64
}

/// Prepare a data directory with a checkpoint containing `count` datoms.
/// Returns the path to the temp directory (kept alive by the TempDir handle).
fn prepare_cold_start_dir(count: usize) -> tempfile::TempDir {
    let dir = tempfile::TempDir::new().expect("create temp dir for cold start");

    let store = build_store_with_datoms(count);

    let checkpoint_path = dir.path().join("checkpoint.chkp");
    write_checkpoint(&store, &checkpoint_path)
        .expect("INV-FERR-028: write_checkpoint must succeed");

    dir
}

// ---------------------------------------------------------------------------
// Threshold tests
// ---------------------------------------------------------------------------

/// INV-FERR-026: Write amplification must be below 10x.
///
/// Write amplification = (physical WAL bytes) / (logical datom payload bytes).
/// The WAL adds per-frame overhead (magic, version, epoch, length, CRC = 22 bytes)
/// plus transaction metadata datoms (tx/time, tx/agent). With small datoms
/// (~65 bytes logical), the overhead is significant but must stay below 10x.
#[test]
fn threshold_inv_ferr_026_write_amplification() {
    let wa_ratio = measure_write_amplification(1000);

    assert!(
        wa_ratio < MAX_WRITE_AMPLIFICATION,
        "INV-FERR-026: write amplification {wa_ratio:.2}x exceeds \
         threshold {MAX_WRITE_AMPLIFICATION}x. \
         WAL framing + tx metadata overhead is too high."
    );

    // Sanity: WA should be at least 1.0 (physical >= logical).
    assert!(
        wa_ratio >= 1.0,
        "INV-FERR-026: write amplification {wa_ratio:.2}x is below 1.0, \
         which indicates a measurement bug."
    );
}

/// INV-FERR-027: EAVT index lookups must average below 1ms each.
///
/// Builds a store with 10K datoms, then performs 1000 point lookups
/// in the EAVT index (the primary read path). Measures wall-clock time
/// and asserts the average is below the threshold.
#[test]
fn threshold_inv_ferr_027_read_latency() {
    let datom_count = 10_000;
    let lookup_count = 1_000;

    let store = build_store_with_datoms(datom_count);

    // Collect entity IDs we inserted for deterministic lookups.
    let lookup_entities: Vec<EntityId> = (0..lookup_count)
        .map(|i| EntityId::from_content(format!("entity-{i}").as_bytes()))
        .collect();

    let eavt = store.indexes().eavt();

    // Warm up: one lookup to fault in pages / warm caches.
    let warmup_key = EavtKey(
        lookup_entities[0],
        Attribute::from("db/doc"),
        Value::String("value-0".into()),
        ferratom::TxId::new(0, 0, 0),
        ferratom::Op::Assert,
    );
    let _ = eavt.get_prev(&warmup_key);

    // Timed lookups: use get_prev which finds the nearest entry <= key.
    // This exercises the O(log n) tree traversal path that real queries use.
    let start = Instant::now();

    for i in 0..lookup_count {
        let key = EavtKey(
            lookup_entities[i],
            Attribute::from("db/doc"),
            Value::String(format!("value-{i}").into()),
            ferratom::TxId::new(0, 0, 0),
            ferratom::Op::Assert,
        );
        // get_prev returns the greatest key <= the query key.
        // This is the standard EAVT range scan entry point.
        let result = eavt.get_prev(&key);
        // Prevent the compiler from optimizing away the lookup.
        std::hint::black_box(result);
    }

    let elapsed = start.elapsed();
    let avg_ns = elapsed.as_nanos() / lookup_count as u128;

    assert!(
        avg_ns < MAX_READ_LATENCY_NS,
        "INV-FERR-027: average EAVT lookup latency {avg_ns}ns exceeds \
         threshold {MAX_READ_LATENCY_NS}ns (1ms). \
         {lookup_count} lookups across {datom_count} datoms took {elapsed:?} total."
    );
}

/// INV-FERR-028: Cold start recovery must complete in under 5 seconds.
///
/// Prepares a checkpoint with 1K datoms (scaled down from spec's 100K
/// for debug-build test speed), then times `cold_start()`. The 5s
/// threshold is extremely generous at this scale -- it validates the
/// recovery path works and doesn't have O(n^2) or worse pathologies.
/// The full 100K target should be verified in release-mode benchmarks.
#[test]
fn threshold_inv_ferr_028_cold_start() {
    let datom_count = 1_000;

    let dir = prepare_cold_start_dir(datom_count);

    let start = Instant::now();
    let result = cold_start(dir.path())
        .expect("INV-FERR-028: cold_start must succeed with valid checkpoint");
    let elapsed = start.elapsed();

    // Verify we actually recovered data, not just a genesis.
    assert!(
        result.database.epoch() > 0,
        "INV-FERR-028: recovered database must have epoch > 0, got {}. \
         Cold start may have fallen through to genesis.",
        result.database.epoch()
    );

    assert!(
        elapsed.as_secs() < MAX_COLD_START_SECS,
        "INV-FERR-028: cold start took {elapsed:?}, exceeds \
         threshold {MAX_COLD_START_SECS}s. \
         Recovery of {datom_count} datoms from checkpoint is too slow."
    );
}
