//! WAL recovery integration tests.
//!
//! INV-FERR-006 (snapshot isolation), INV-FERR-008 (WAL durability),
//! INV-FERR-013 (checkpoint identity), INV-FERR-014 (recovery correctness),
//! INV-FERR-024 (in-memory backend).
//! Phase 4a: all tests passing against ferratomic-db implementation.

use std::io::Write;

use ferratom::{AgentId, Attribute, EntityId, Value};
use ferratomic_db::{db::Database, wal::Wal, writer::Transaction};
use tempfile::TempDir;

/// Write a single test datom into a new WAL at the given path.
fn write_single_wal_entry(wal_path: &std::path::Path, agent: AgentId) {
    let mut wal = Wal::create(wal_path).expect("create WAL");
    let tx = Transaction::new(agent)
        .assert_datom(
            EntityId::from_content(b"e1"),
            Attribute::from("user/name"),
            Value::String("Alice".into()),
        )
        .commit_unchecked();
    let payload = bincode::serialize(tx.datoms()).expect("serialize tx");
    wal.append_raw(1, &payload).expect("append failed");
    wal.fsync().expect("fsync failed");
}

/// Recover a WAL and deserialize the first entry's datoms via wire types.
fn recover_and_deserialize_datoms(wal_path: &std::path::Path) -> Vec<ferratom::Datom> {
    let mut wal = Wal::open(wal_path).expect("open WAL");
    let entries = wal.recover().expect("recovery failed");
    assert_eq!(
        entries.len(),
        1,
        "INV-FERR-008: expected 1 entry, got {}",
        entries.len()
    );
    let wire_datoms: Vec<ferratom::wire::WireDatom> = bincode::deserialize(&entries[0].payload)
        .expect("INV-FERR-008: recovered payload must deserialize as Vec<WireDatom>");
    wire_datoms
        .into_iter()
        .map(ferratom::wire::WireDatom::into_trusted)
        .collect()
}

/// INV-FERR-008: Basic WAL write and recovery.
#[test]
fn inv_ferr_008_wal_write_and_recover() {
    let dir = TempDir::new().expect("failed to create temp dir");
    let wal_path = dir.path().join("test.wal");
    let agent = AgentId::from_bytes([1u8; 16]);

    write_single_wal_entry(&wal_path, agent);

    let datoms = recover_and_deserialize_datoms(&wal_path);
    assert_eq!(
        datoms.len(),
        1,
        "INV-FERR-008: recovered entry must contain exactly 1 datom, got {}",
        datoms.len()
    );
    assert_eq!(
        datoms[0].entity(),
        EntityId::from_content(b"e1"),
        "INV-FERR-008: recovered datom entity must match original"
    );
    assert_eq!(
        datoms[0].attribute().as_str(),
        "user/name",
        "INV-FERR-008: recovered datom attribute must match original"
    );
    assert_eq!(
        *datoms[0].value(),
        Value::String("Alice".into()),
        "INV-FERR-008: recovered datom value must match original"
    );
}

/// Regression: bd-32t — WAL payload content roundtrip preserves datom attributes.
///
/// Migrated from ferratomic-wal unit tests during crate extraction (bd-bc41).
/// The WAL crate tests raw byte roundtrip; this test verifies the full
/// serialize-write-recover-deserialize path through the ADR-FERR-010 trust boundary.
#[test]
fn test_bug_bd_32t_payload_content_roundtrip() {
    let dir = TempDir::new().expect("failed to create temp dir");
    let wal_path = dir.path().join("test.wal");
    let agent = AgentId::from_bytes([1u8; 16]);

    write_single_wal_entry(&wal_path, agent);
    let datoms = recover_and_deserialize_datoms(&wal_path);

    assert!(!datoms.is_empty(), "bd-32t: payload must contain datoms");
    assert_eq!(
        datoms[0].attribute().as_str(),
        "user/name",
        "bd-32t: datom attribute must survive WAL roundtrip"
    );
}

/// INV-FERR-008: Crash mid-write — incomplete entry truncated on recovery.
#[test]
fn inv_ferr_008_crash_mid_write_recovery() {
    let dir = TempDir::new().expect("failed to create temp dir");
    let wal_path = dir.path().join("test.wal");

    let agent = AgentId::from_bytes([1u8; 16]);

    // Write 3 complete entries
    {
        let mut wal = Wal::create(&wal_path).expect("create WAL");
        for i in 1u64..=3 {
            let tx = Transaction::new(agent)
                .assert_datom(
                    EntityId::from_content(format!("e{}", i).as_bytes()),
                    Attribute::from("test/data"),
                    Value::Long(i as i64),
                )
                .commit_unchecked();
            let payload = bincode::serialize(tx.datoms()).expect("serialize tx");
            wal.append_raw(i, &payload).expect("append failed");
        }
        wal.fsync().expect("fsync failed");
    }

    // Simulate crash: append garbage bytes
    {
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&wal_path)
            .expect("open for crash sim");
        file.write_all(&[0xDE, 0xAD, 0xBE, 0xEF, 0x00, 0x01, 0x02])
            .expect("write garbage");
    }

    // Recovery must preserve 3 complete entries, discard garbage
    {
        let mut wal = Wal::open(&wal_path).expect("open WAL");
        let entries = wal.recover().expect("recovery failed");
        assert_eq!(
            entries.len(),
            3,
            "INV-FERR-008: crash recovery wrong count. expected=3, got={}",
            entries.len()
        );
    }
}

/// Transact N unchecked user-name datoms into a database, using the given
/// entity prefix and index range.
fn transact_user_datoms(db: &Database, agent: AgentId, range: std::ops::Range<i64>) {
    for i in range {
        let tx = Transaction::new(agent)
            .assert_datom(
                EntityId::from_content(format!("user-{i}").as_bytes()),
                Attribute::from("user/name"),
                Value::String(format!("User {i}").into()),
            )
            .commit_unchecked();
        db.transact(tx).expect("transact failed");
    }
}

/// Capture the current database state as (datoms, epoch, schema).
fn capture_db_state(
    db: &Database,
) -> (
    std::collections::BTreeSet<ferratom::Datom>,
    u64,
    ferratom::Schema,
) {
    let snap = db.snapshot();
    let datoms = snap.datoms().cloned().collect();
    (datoms, db.epoch(), db.schema())
}

/// Assert that a set of datoms contains entities for all indices in the range.
fn assert_entities_present(
    datoms: &std::collections::BTreeSet<ferratom::Datom>,
    range: std::ops::Range<i64>,
    context: &str,
) {
    for i in range {
        let expected_entity = EntityId::from_content(format!("user-{i}").as_bytes());
        assert!(
            datoms.iter().any(|d| d.entity() == expected_entity),
            "{}: user-{} entity must be present",
            context,
            i
        );
    }
}

/// INV-FERR-014: Crash-then-transact roundtrip.
///
/// Full lifecycle: genesis -> transact N datoms -> crash (drop) -> recover
/// from WAL -> verify identical state -> transact M more -> verify epoch
/// advances correctly and all N+M user datoms are present.
#[test]
fn test_inv_ferr_014_crash_then_transact() {
    let dir = TempDir::new().expect("failed to create temp dir");
    let wal_path = dir.path().join("crash_roundtrip.wal");
    let agent = AgentId::from_bytes([42u8; 16]);

    // Phase 1: genesis + transact 3 user datoms, then crash (drop)
    let (pre_crash_datoms, pre_crash_epoch, pre_crash_schema) = {
        let db = Database::genesis_with_wal(&wal_path).expect("genesis_with_wal must succeed");
        transact_user_datoms(&db, agent, 0..3);
        capture_db_state(&db)
        // db drops here -- simulates crash
    };
    assert_eq!(
        pre_crash_epoch, 3,
        "INV-FERR-014: 3 txns must produce epoch 3"
    );

    // Phase 2: recover from WAL and verify identical state
    let recovered_db =
        Database::recover_from_wal(&wal_path).expect("recovery from WAL must succeed");
    let (recovered_datoms, recovered_epoch, recovered_schema) = capture_db_state(&recovered_db);
    assert_eq!(
        recovered_epoch, pre_crash_epoch,
        "INV-FERR-014: recovered epoch mismatch"
    );
    assert_eq!(
        recovered_schema, pre_crash_schema,
        "INV-FERR-014: recovered schema mismatch"
    );
    assert_eq!(
        recovered_datoms, pre_crash_datoms,
        "INV-FERR-014: recovered datoms mismatch"
    );

    // Phase 3: transact 2 more datoms on recovered database
    transact_user_datoms(&recovered_db, agent, 3..5);

    // Phase 4: verify epoch advances and all datoms present
    let (final_datoms, final_epoch, _) = capture_db_state(&recovered_db);
    assert_eq!(
        final_epoch,
        recovered_epoch + 2,
        "INV-FERR-014: epoch must advance by 2 after 2 post-recovery txns"
    );
    assert_entities_present(&final_datoms, 0..5, "INV-FERR-014");
    assert!(
        final_datoms.is_superset(&recovered_datoms),
        "INV-FERR-014: post-recovery state must be a superset of recovered state"
    );
}

/// Transact N schema-validated datoms into a database with the given prefix
/// and attribute.
fn transact_validated_datoms(
    db: &Database,
    agent: AgentId,
    prefix: &str,
    attribute: &str,
    count: i64,
) {
    for i in 0..count {
        let tx = Transaction::new(agent)
            .assert_datom(
                EntityId::from_content(format!("{prefix}{i}").as_bytes()),
                Attribute::from(attribute),
                Value::String(format!("test-{i}").into()),
            )
            .commit(&db.schema())
            .expect("valid tx");
        db.transact(tx).expect("transact failed");
    }
}

/// Assert that two database states are identical (datoms, epoch, schema).
fn assert_db_state_equal(
    label: &str,
    expected: &(
        std::collections::BTreeSet<ferratom::Datom>,
        u64,
        ferratom::Schema,
    ),
    actual: &(
        std::collections::BTreeSet<ferratom::Datom>,
        u64,
        ferratom::Schema,
    ),
) {
    assert_eq!(expected.1, actual.1, "{}: epoch mismatch", label);
    assert_eq!(expected.2, actual.2, "{}: schema mismatch", label);
    assert_eq!(expected.0, actual.0, "{}: datom set mismatch", label);
}

/// INV-FERR-008: WAL entry must precede snapshot visibility.
/// After commit, WAL recovery alone must reproduce all visible datoms,
/// epoch, and schema -- exact state equality.
#[test]
fn inv_ferr_008_wal_entry_precedes_snapshot() {
    let dir = TempDir::new().expect("failed to create temp dir");
    let wal_path = dir.path().join("test.wal");
    let db = Database::genesis_with_wal(&wal_path).expect("failed to create store with WAL");
    let agent = AgentId::from_bytes([1u8; 16]);

    transact_validated_datoms(&db, agent, "e", "tx/provenance", 5);
    let pre_crash = capture_db_state(&db);

    // Recover from WAL alone (simulating crash + restart)
    let recovered_db = Database::recover_from_wal(&wal_path).expect("recovery failed");
    let recovered = capture_db_state(&recovered_db);

    assert_db_state_equal("INV-FERR-008", &pre_crash, &recovered);
}

/// Transact N schema-validated datoms with a "dc-entity-" prefix into a database.
fn transact_dc_datoms(db: &Database, agent: AgentId, range: std::ops::Range<u64>) {
    let schema = db.schema();
    for i in range {
        let tx = Transaction::new(agent)
            .assert_datom(
                EntityId::from_content(format!("dc-entity-{i}").as_bytes()),
                Attribute::from("db/doc"),
                Value::String(format!("double-crash-{i}").into()),
            )
            .commit(&schema)
            .expect("INV-FERR-014: transaction must validate");
        db.transact(tx)
            .expect("INV-FERR-014: transact must succeed");
    }
}

/// Write a checkpoint from the current database state.
///
/// INV-FERR-013: the checkpoint must faithfully preserve the store's epoch,
/// schema, genesis_agent, datom set, and LIVE metadata. Uses
/// `Database::store_for_checkpoint()` which clones the actual Store,
/// preserving all metadata. This ensures the checkpoint epoch matches the
/// database epoch — cold start recovery uses the checkpoint epoch to
/// compute the WAL delta.
fn write_checkpoint_from_db(db: &Database, checkpoint_path: &std::path::Path) {
    use ferratomic_db::checkpoint::write_checkpoint;
    let store = db.store_for_checkpoint();
    write_checkpoint(&store, checkpoint_path).expect("INV-FERR-013: checkpoint write must succeed");
}

/// Assert that all dc-entity-{0..count} entities are present in a database snapshot.
fn assert_dc_entities_present(db: &Database, count: u64, context: &str) {
    let snap = db.snapshot();
    for i in 0..count {
        let expected = EntityId::from_content(format!("dc-entity-{i}").as_bytes());
        assert!(
            snap.datoms().any(|d| d.entity() == expected),
            "{}: dc-entity-{} must be present",
            context,
            i
        );
    }
}

/// INV-FERR-014: Double-crash recovery with checkpoint in between.
///
/// Full lifecycle: genesis --> transact 3 --> checkpoint --> transact 2 -->
/// crash 1 --> cold_start --> transact 1 --> crash 2 --> cold_start --> verify.
#[test]
fn test_inv_ferr_014_double_crash() {
    use ferratomic_db::storage::{cold_start, RecoveryLevel};

    let dir = TempDir::new().expect("failed to create temp dir");
    let data_dir = dir.path().join("double_crash");
    std::fs::create_dir_all(&data_dir).expect("create data dir");
    let wal_path = data_dir.join("wal.log");
    let checkpoint_path = data_dir.join("checkpoint.chkp");
    let agent = AgentId::from_bytes([77u8; 16]);

    // Phase 1: genesis + 3 txns + checkpoint + 2 more txns, then crash
    double_crash_phase1(&wal_path, &checkpoint_path, agent);

    // Phase 2: first cold_start recovery
    let result1 = cold_start(&data_dir).expect("INV-FERR-014: first cold_start must succeed");
    assert_eq!(result1.level, RecoveryLevel::CheckpointPlusWal);
    assert_dc_entities_present(&result1.database, 5, "INV-FERR-014 recovery 1");
    let epoch1 = result1.database.epoch();
    assert!(
        epoch1 >= 5,
        "INV-FERR-014: epoch after recovery 1 must be >= 5, got {epoch1}"
    );

    // Phase 3: transact 1 more, then crash 2
    double_crash_phase3(&result1.database, agent);

    // Phase 4: second cold_start recovery
    let result2 = cold_start(&data_dir).expect("INV-FERR-014: second cold_start must succeed");
    // bd-auqy: After crash 2, the checkpoint from Phase 1 still exists and
    // the WAL contains post-checkpoint transactions from Phase 3. Recovery
    // MUST use CheckpointPlusWal to replay the WAL delta on top of the
    // checkpoint. WalOnly would discard the checkpoint; CheckpointOnly would
    // lose the post-checkpoint transactions.
    assert_eq!(
        result2.level,
        RecoveryLevel::CheckpointPlusWal,
        "INV-FERR-014: recovery 2 must use CheckpointPlusWal (checkpoint exists \
         with post-checkpoint WAL delta from Phase 3), got {:?}",
        result2.level
    );
    assert_dc_entities_present(&result2.database, 6, "INV-FERR-014 recovery 2");
    let epoch2 = result2.database.epoch();
    assert!(
        epoch2 >= 6,
        "INV-FERR-014: epoch after recovery 2 must be >= 6, got {epoch2}"
    );
    assert!(
        epoch2 >= epoch1,
        "INV-FERR-014: epoch must not regress between recoveries"
    );
}

/// Phase 1 of double-crash test: genesis, 3 txns, checkpoint, 2 more txns.
fn double_crash_phase1(
    wal_path: &std::path::Path,
    checkpoint_path: &std::path::Path,
    agent: AgentId,
) {
    let db = Database::genesis_with_wal(wal_path).expect("genesis_with_wal must succeed");
    transact_dc_datoms(&db, agent, 0..3);
    write_checkpoint_from_db(&db, checkpoint_path);
    transact_dc_datoms(&db, agent, 3..5);
    // db drops here -- simulates crash 1
}

/// Phase 3 of double-crash test: transact 1 more datom, then crash.
fn double_crash_phase3(db: &Database, agent: AgentId) {
    let schema = db.schema();
    let tx = Transaction::new(agent)
        .assert_datom(
            EntityId::from_content(b"dc-entity-5"),
            Attribute::from("db/doc"),
            Value::String("double-crash-5".into()),
        )
        .commit(&schema)
        .expect("INV-FERR-014: post-recovery transaction must validate");
    db.transact(tx)
        .expect("INV-FERR-014: post-recovery transact must succeed");
    // db drops after this function returns -- simulates crash 2
}

/// Write a test checkpoint into an `InMemoryBackend`.
fn write_test_checkpoint_to_backend(backend: &ferratomic_db::storage::InMemoryBackend) {
    use ferratomic_db::storage::StorageBackend;

    let mut store = ferratomic_db::store::Store::genesis();
    let tx = Transaction::new(AgentId::from_bytes([24u8; 16]))
        .assert_datom(
            EntityId::from_content(b"in-mem-test-entity"),
            Attribute::from("db/doc"),
            Value::String("in-memory-backend-test".into()),
        )
        .commit_unchecked();
    store
        .transact_test(tx)
        .expect("INV-FERR-024: transact into store for checkpoint");

    let mut writer = backend
        .open_checkpoint_writer()
        .expect("INV-FERR-024: open checkpoint writer");
    ferratomic_db::checkpoint::write_checkpoint_to_writer(&store, &mut writer)
        .expect("INV-FERR-024: write checkpoint to in-memory backend");
}

/// INV-FERR-024: `InMemoryBackend` supports `cold_start_with_backend`.
///
/// bd-7tb0: integration test verifying the `InMemoryBackend` trait implementation
/// works with the generic `cold_start_with_backend` path. Empty backend produces
/// genesis; backend with a checkpoint restores state.
#[test]
fn test_inv_ferr_024_in_memory_backend() {
    use ferratomic_db::storage::{
        cold_start_with_backend, InMemoryBackend, RecoveryLevel, StorageBackend,
    };

    // Phase 1: Empty backend produces genesis.
    let backend = InMemoryBackend::new();
    let result = cold_start_with_backend(&backend)
        .expect("INV-FERR-024: cold_start_with_backend must succeed on empty backend");
    assert_eq!(result.level, RecoveryLevel::Genesis);
    assert_eq!(
        result.database.epoch(),
        0,
        "INV-FERR-024: genesis epoch must be 0"
    );

    // Phase 2: Write a checkpoint, then cold-start again.
    write_test_checkpoint_to_backend(&backend);
    assert!(
        backend.checkpoint_exists(),
        "INV-FERR-024: checkpoint must exist after write"
    );

    let result2 = cold_start_with_backend(&backend)
        .expect("INV-FERR-024: cold_start_with_backend must succeed with checkpoint");
    assert_eq!(result2.level, RecoveryLevel::CheckpointOnly);
    assert!(
        result2.database.snapshot().datoms().count() > 0,
        "INV-FERR-024: recovered database must contain datoms from checkpoint"
    );
}

// =========================================================================
// bd-7fub.19.2 — Triple-crash with WAL truncation
// =========================================================================

/// Transact N schema-validated datoms with a "tc-entity-" prefix into a database.
fn transact_tc_datoms(db: &Database, agent: AgentId, range: std::ops::Range<u64>) {
    let schema = db.schema();
    for i in range {
        let tx = Transaction::new(agent)
            .assert_datom(
                EntityId::from_content(format!("tc-entity-{i}").as_bytes()),
                Attribute::from("db/doc"),
                Value::String(format!("triple-crash-{i}").into()),
            )
            .commit(&schema)
            .expect("INV-FERR-014: transaction must validate");
        db.transact(tx)
            .expect("INV-FERR-014: transact must succeed");
    }
}

/// Assert that all tc-entity-{0..count} entities are present in a database snapshot.
fn assert_tc_entities_present(db: &Database, count: u64, context: &str) {
    let snap = db.snapshot();
    for i in 0..count {
        let expected = EntityId::from_content(format!("tc-entity-{i}").as_bytes());
        assert!(
            snap.datoms().any(|d| d.entity() == expected),
            "{}: tc-entity-{} must be present",
            context,
            i
        );
    }
}

/// INV-FERR-014: Triple-crash with WAL truncation.
///
/// bd-7fub.19.2: Tests that recovery correctly handles three sequential crashes
/// with a WAL truncation mid-frame between crash 2 and crash 3. The truncated
/// frame must be discarded (CRC will not validate), and the third recovery must
/// produce a stable state identical to the second recovery.
///
/// Scenario:
/// 1. Genesis -> 3 txns -> checkpoint -> 2 more txns -> crash 1
/// 2. Cold start recovery 1 -> verify all 5 datoms
/// 3. 2 more txns -> WAL truncation (cut into last frame) -> crash 2
/// 4. Cold start recovery 2 -> at least 5 datoms (pre-truncation state)
/// 5. Crash 3 (clean drop from recovered state)
/// 6. Cold start recovery 3 -> identical state to recovery 2
#[test]
fn test_inv_ferr_014_triple_crash_wal_truncation() {
    use ferratomic_db::storage::{cold_start, RecoveryLevel};

    let dir = TempDir::new().expect("failed to create temp dir");
    let data_dir = dir.path().join("triple_crash");
    std::fs::create_dir_all(&data_dir).expect("create data dir");
    let wal_path = data_dir.join("wal.log");
    let checkpoint_path = data_dir.join("checkpoint.chkp");
    let agent = AgentId::from_bytes([0xAA; 16]);

    // Phase 1: genesis -> 3 txns -> checkpoint -> 2 more txns -> crash 1
    {
        let db = Database::genesis_with_wal(&wal_path)
            .expect("INV-FERR-014: genesis_with_wal must succeed");
        transact_tc_datoms(&db, agent, 0..3);
        write_checkpoint_from_db(&db, &checkpoint_path);
        transact_tc_datoms(&db, agent, 3..5);
        // db drops here -- simulates crash 1
    }

    // Phase 2: cold start recovery 1 -> verify all 5 datoms present
    let recovery1_datom_count;
    let recovery1_epoch;
    {
        let result1 = cold_start(&data_dir).expect("INV-FERR-014: first cold_start must succeed");
        assert_eq!(
            result1.level,
            RecoveryLevel::CheckpointPlusWal,
            "INV-FERR-014: first recovery must use checkpoint+WAL path"
        );
        assert_tc_entities_present(&result1.database, 5, "INV-FERR-014 recovery 1");
        recovery1_epoch = result1.database.epoch();
        assert!(
            recovery1_epoch >= 5,
            "INV-FERR-014: epoch after recovery 1 must be >= 5, got {recovery1_epoch}"
        );

        // Phase 3: transact 2 more -> WAL truncation -> crash 2
        transact_tc_datoms(&result1.database, agent, 5..7);
        recovery1_datom_count = result1.database.snapshot().datoms().count();
        // db drops here -- simulates crash 2
    }

    // Between crash 2 and recovery 2: truncate WAL to cut into the last frame.
    // The WAL frame format has an 18-byte header + payload + 4-byte CRC = 22
    // bytes minimum per frame. Truncating by 7 bytes removes part of the last
    // frame's CRC or payload, so recovery will discard the incomplete frame.
    {
        let wal_len = std::fs::metadata(&wal_path)
            .expect("INV-FERR-014: WAL file must exist")
            .len();
        // The minimum frame size is 22 bytes (18 header + 4 CRC, zero payload).
        // Truncating by 7 bytes is guaranteed to damage only the last frame
        // because 7 < 22 (minimum frame size).
        assert!(
            wal_len > 22,
            "INV-FERR-014: WAL must contain at least one complete frame, got {wal_len} bytes"
        );
        // Truncation amount (7) < minimum frame size (22 = 18 header + 4 CRC),
        // so only the last frame is damaged. This is a compile-time invariant.
        let truncated_len = wal_len - 7;
        let file = std::fs::OpenOptions::new()
            .write(true)
            .open(&wal_path)
            .expect("INV-FERR-014: open WAL for truncation");
        file.set_len(truncated_len)
            .expect("INV-FERR-014: WAL truncation must succeed");
    }

    // Phase 4: cold start recovery 2 -> at least 5 datoms survive
    let recovery2_datoms;
    let recovery2_epoch;
    {
        let result2 = cold_start(&data_dir).expect("INV-FERR-014: second cold_start must succeed");
        // bd-auqy: Recovery 2 follows a WAL truncation (crash 2 + truncation
        // in Phase 3). The checkpoint alone predates the WAL delta, so
        // recovery MUST replay the (truncated) WAL to reconstruct state.
        // `CheckpointOnly` would silently lose all post-checkpoint datoms.
        assert_eq!(
            result2.level,
            RecoveryLevel::CheckpointPlusWal,
            "INV-FERR-014: recovery 2 must use CheckpointPlusWal (WAL delta \
             exists after truncation), got {:?}",
            result2.level
        );
        assert_tc_entities_present(&result2.database, 5, "INV-FERR-014 recovery 2");
        recovery2_epoch = result2.database.epoch();
        assert!(
            recovery2_epoch >= 5,
            "INV-FERR-014: epoch after recovery 2 must be >= 5, got {recovery2_epoch}"
        );
        // The truncated frame's datoms may be lost, so the recovered datom
        // count may be less than what was in memory before crash 2.
        let recovery2_count = result2.database.snapshot().datoms().count();
        assert!(
            recovery2_count <= recovery1_datom_count,
            "INV-FERR-014: recovery 2 datom count ({recovery2_count}) must not \
             exceed pre-crash-2 count ({recovery1_datom_count}) since frame was truncated"
        );
        recovery2_datoms = result2
            .database
            .snapshot()
            .datoms()
            .cloned()
            .collect::<std::collections::BTreeSet<_>>();
        // db drops here -- simulates crash 3
    }

    // Phase 5-6: crash 3 (clean) -> cold start recovery 3 -> same state as recovery 2
    {
        let result3 = cold_start(&data_dir).expect("INV-FERR-014: third cold_start must succeed");
        assert!(
            result3.level == RecoveryLevel::CheckpointPlusWal
                || result3.level == RecoveryLevel::CheckpointOnly,
            "INV-FERR-014: recovery 3 must use checkpoint path, got {:?}",
            result3.level
        );
        let recovery3_epoch = result3.database.epoch();
        let recovery3_datoms: std::collections::BTreeSet<_> =
            result3.database.snapshot().datoms().cloned().collect();
        assert_eq!(
            recovery3_epoch, recovery2_epoch,
            "INV-FERR-014: recovery 3 epoch must equal recovery 2 epoch \
             (stable state after truncation)"
        );
        assert_eq!(
            recovery3_datoms, recovery2_datoms,
            "INV-FERR-014: recovery 3 datom set must equal recovery 2 datom set \
             (idempotent recovery after clean crash)"
        );
    }
}

// =========================================================================
// bd-7fub.19.4 — Power-cut simulation (atomic rename)
// =========================================================================

/// Transact N schema-validated datoms with a "pc-entity-" prefix into a database.
fn transact_pc_datoms(db: &Database, agent: AgentId, range: std::ops::Range<u64>) {
    let schema = db.schema();
    for i in range {
        let tx = Transaction::new(agent)
            .assert_datom(
                EntityId::from_content(format!("pc-entity-{i}").as_bytes()),
                Attribute::from("db/doc"),
                Value::String(format!("power-cut-{i}").into()),
            )
            .commit(&schema)
            .expect("INV-FERR-014: transaction must validate");
        db.transact(tx)
            .expect("INV-FERR-014: transact must succeed");
    }
}

/// Assert that all pc-entity-{0..count} entities are present in a database snapshot.
fn assert_pc_entities_present(db: &Database, count: u64, context: &str) {
    let snap = db.snapshot();
    for i in 0..count {
        let expected = EntityId::from_content(format!("pc-entity-{i}").as_bytes());
        assert!(
            snap.datoms().any(|d| d.entity() == expected),
            "{}: pc-entity-{} must be present",
            context,
            i
        );
    }
}

/// INV-FERR-014 + INV-FERR-013: Power-cut simulation (atomic rename).
///
/// bd-7fub.19.4: Tests that the checkpoint write path uses atomic rename so
/// that if a power-cut occurs after writing the new checkpoint to a temp file
/// but before the rename completes, the original checkpoint survives intact
/// and cold start recovers from it.
///
/// Scenario:
/// 1. Genesis -> 3 txns -> write checkpoint
/// 2. Verify checkpoint valid (cold start)
/// 3. 2 more txns -> write checkpoint to TEMP file (no rename)
/// 4. Simulate power-cut: temp file exists but was never renamed
/// 5. Cold start -> must recover from ORIGINAL checkpoint
/// 6. Verify: recovered state matches pre-second-checkpoint state
#[test]
fn test_inv_ferr_014_power_cut_atomic_rename() {
    use ferratomic_db::storage::cold_start;

    let dir = TempDir::new().expect("failed to create temp dir");
    let data_dir = dir.path().join("power_cut");
    std::fs::create_dir_all(&data_dir).expect("create data dir");
    let wal_path = data_dir.join("wal.log");
    let checkpoint_path = data_dir.join("checkpoint.chkp");
    let temp_checkpoint_path = data_dir.join("checkpoint.chkp.tmp");
    let agent = AgentId::from_bytes([0xBB; 16]);

    // Phase 1: genesis -> 3 txns -> write checkpoint to canonical path
    let pre_second_checkpoint_datoms: std::collections::BTreeSet<ferratom::Datom>;
    {
        let db = Database::genesis_with_wal(&wal_path)
            .expect("INV-FERR-014: genesis_with_wal must succeed");
        transact_pc_datoms(&db, agent, 0..3);
        write_checkpoint_from_db(&db, &checkpoint_path);

        // Phase 2: verify checkpoint is valid by cold starting
        // (We just wrote it; verify by checking it exists.)
        assert!(
            checkpoint_path.exists(),
            "INV-FERR-013: checkpoint file must exist after write"
        );

        // Capture state before second checkpoint
        pre_second_checkpoint_datoms = db.snapshot().datoms().cloned().collect();

        // Phase 3: transact 2 more -> write second checkpoint to TEMP file only
        transact_pc_datoms(&db, agent, 3..5);

        // Write the checkpoint to the temp path using write_checkpoint_to_writer.
        // This simulates a checkpoint write that completes data write but the
        // atomic rename never happens (power-cut before rename).
        {
            // Use store_for_checkpoint() — the same correct pattern as the
            // primary write_checkpoint_from_db helper. Avoids the genesis+insert
            // anti-pattern that produces epoch-0 checkpoints (now fixed via store_for_checkpoint()).
            let temp_store = db.store_for_checkpoint();
            let temp_file = std::fs::File::create(&temp_checkpoint_path)
                .expect("test setup: create temp checkpoint file");
            let mut buf_writer = std::io::BufWriter::new(temp_file);
            ferratomic_db::checkpoint::write_checkpoint_to_writer(&temp_store, &mut buf_writer)
                .expect("test setup: write checkpoint to temp file");
        }

        assert!(
            temp_checkpoint_path.exists(),
            "INV-FERR-014: temp checkpoint file must exist"
        );

        // db drops here -- simulates power-cut crash
    }

    // Phase 5: cold start from the data dir. The cold_start function looks
    // for checkpoint at "checkpoint.chkp", NOT "checkpoint.chkp.tmp". The
    // temp file is invisible to the recovery cascade.
    {
        let result =
            cold_start(&data_dir).expect("INV-FERR-014: cold_start after power-cut must succeed");

        // Verify recovery actually loaded the checkpoint (not genesis or WAL-only).
        assert!(
            result.level == ferratomic_db::storage::RecoveryLevel::CheckpointPlusWal
                || result.level == ferratomic_db::storage::RecoveryLevel::CheckpointOnly,
            "INV-FERR-013: recovery must load the original checkpoint, got {:?}",
            result.level
        );

        // Phase 6: verify recovered state matches pre-second-checkpoint state.
        // The original checkpoint captured only 3 pc-entity datoms. The WAL
        // contains all 5 txns, so recovery replays WAL delta after the
        // checkpoint epoch. However, the ORIGINAL checkpoint's datoms must
        // all be present.
        assert_pc_entities_present(
            &result.database,
            3,
            "INV-FERR-013: original checkpoint datoms must survive power-cut",
        );

        // bd-w30y: Verify WAL delta replay: entities 3 and 4 (post-checkpoint
        // transactions) must also be present. The WAL contains all 5 txns;
        // recovery replays entries after the checkpoint epoch.
        assert_pc_entities_present(
            &result.database,
            5,
            "INV-FERR-014: WAL delta replay must recover post-checkpoint entities",
        );

        // The temp file at checkpoint.chkp.tmp must NOT have been loaded.
        // Verify the original checkpoint was the one used by confirming
        // that the checkpoint file on disk is the same as the original
        // (its size has not changed to the larger second checkpoint).
        let original_size = std::fs::metadata(&checkpoint_path)
            .expect("INV-FERR-013: checkpoint must still exist")
            .len();
        let temp_size = std::fs::metadata(&temp_checkpoint_path)
            .expect("INV-FERR-014: temp file must still exist on disk")
            .len();
        // The temp file has 5 txns of datoms, the original has 3.
        // They cannot be the same size (more datoms = larger checkpoint).
        // This is a structural assertion that the original checkpoint was
        // not overwritten.
        assert_ne!(
            original_size, temp_size,
            "INV-FERR-013: original checkpoint and temp checkpoint must differ in size, \
             confirming the original was not overwritten"
        );

        // The original checkpoint's datoms are a subset of the recovered state
        // (recovery replays WAL delta on top of the checkpoint).
        let recovered_datoms: std::collections::BTreeSet<_> =
            result.database.snapshot().datoms().cloned().collect();
        assert!(
            recovered_datoms.is_superset(&pre_second_checkpoint_datoms),
            "INV-FERR-013: recovered state must be a superset of original checkpoint state"
        );
    }
}

// =========================================================================
// bd-7fub.19.5 — ENOSPC simulation (disk full)
// =========================================================================

/// INV-FERR-014: ENOSPC simulation via partial WAL frame write.
///
/// bd-7fub.19.5: The effect of ENOSPC during a WAL write is a partially-written
/// frame that fails CRC validation. This test simulates that artifact directly:
/// write 3 committed datoms, then append a partial WAL frame header (magic
/// bytes + partial epoch but not a full frame) to the WAL, then recover.
/// Recovery must preserve the 3 committed datoms and discard the partial frame.
#[test]
fn test_inv_ferr_014_enospc_wal_truncation() {
    let dir = TempDir::new().expect("failed to create temp dir");
    let wal_path = dir.path().join("enospc.wal");
    let agent = AgentId::from_bytes([0xCC; 16]);

    // Phase 1: genesis -> 3 txns via WAL -> fsync
    let pre_enospc_datoms: std::collections::BTreeSet<ferratom::Datom>;
    {
        let db = Database::genesis_with_wal(&wal_path)
            .expect("INV-FERR-014: genesis_with_wal must succeed");
        transact_user_datoms(&db, agent, 0..3);
        pre_enospc_datoms = db.snapshot().datoms().cloned().collect();
        // db drops here -- WAL is fsynced by transact
    }

    // Phase 2: simulate ENOSPC by appending a partial WAL frame header.
    // WAL magic is b"FERR" (4 bytes) + version 0x0001 (2 bytes LE) +
    // partial epoch (only 3 of 8 bytes). This is 9 bytes total -- not
    // a complete frame header (which is 18 bytes), so the CRC cannot
    // validate and recovery must discard it.
    {
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&wal_path)
            .expect("test setup: open WAL for ENOSPC simulation");
        // WAL magic: b"FERR"
        file.write_all(b"FERR")
            .expect("test setup: write magic bytes");
        // Version: 0x0001 LE
        file.write_all(&1u16.to_le_bytes())
            .expect("test setup: write version");
        // Partial epoch: only 3 of 8 bytes (simulates ENOSPC mid-write)
        file.write_all(&[0x04, 0x00, 0x00])
            .expect("test setup: write partial epoch");
        // No flush/fsync -- simulates the crash after partial write
    }

    // Phase 3: recover from WAL -> verify exactly the 3 committed datoms survive
    {
        let recovered_db = Database::recover_from_wal(&wal_path)
            .expect("INV-FERR-014: recovery after ENOSPC must succeed");
        let recovered_datoms: std::collections::BTreeSet<_> =
            recovered_db.snapshot().datoms().cloned().collect();
        assert_eq!(
            recovered_datoms, pre_enospc_datoms,
            "INV-FERR-014: recovery after ENOSPC must produce identical datom set \
             to pre-ENOSPC state (partial frame discarded)"
        );
        let recovered_epoch = recovered_db.epoch();
        assert_eq!(
            recovered_epoch, 3,
            "INV-FERR-014: recovered epoch must be 3 (3 committed txns, \
             partial frame discarded), got {recovered_epoch}"
        );
    }
}

// =========================================================================
// bd-7fub.19.6 — Concurrent snapshot + crash + recovery
// =========================================================================

/// INV-FERR-014 + INV-FERR-006: Snapshot survives crash and recovery.
///
/// bd-7fub.19.6: Tests that snapshot isolation holds across a crash boundary:
/// a snapshot taken at epoch 3 sees only 3 user datoms even after 2 more
/// are transacted (INV-FERR-006), and that recovery after crash restores
/// all 5 committed datoms (INV-FERR-014). The original snapshot is
/// in-memory only and is lost on crash -- this is expected.
///
/// Scenario:
/// 1. Genesis -> 3 txns
/// 2. Take snapshot (snap1) at epoch 3
/// 3. 2 more txns -> epoch 5
/// 4. Verify snap1 still sees only 3 user datoms (INV-FERR-006)
/// 5. Crash (drop DB)
/// 6. Cold start recovery -> all 5 user datoms present
/// 7. New snapshot on recovered DB -> sees all 5 datoms
/// 8. snap1 is gone (in-memory only) -- expected behavior
#[test]
fn test_inv_ferr_014_snapshot_survives_crash_recovery() {
    let dir = TempDir::new().expect("failed to create temp dir");
    let wal_path = dir.path().join("snap_crash.wal");
    let agent = AgentId::from_bytes([0xDD; 16]);

    // Pre-crash: genesis -> 3 txns -> snapshot -> 2 more txns -> verify snapshot
    {
        let db = Database::genesis_with_wal(&wal_path)
            .expect("INV-FERR-014: genesis_with_wal must succeed");
        transact_user_datoms(&db, agent, 0..3);

        // Step 2: take snapshot at epoch 3
        let snap1 = db.snapshot();
        let snap1_epoch = snap1.epoch();
        assert_eq!(
            snap1_epoch, 3,
            "INV-FERR-006: snapshot taken after 3 txns must have epoch 3"
        );

        // Step 3: transact 2 more datoms (epoch advances to 5)
        transact_user_datoms(&db, agent, 3..5);
        let current_epoch = db.epoch();
        assert_eq!(
            current_epoch, 5,
            "INV-FERR-007: epoch must be 5 after 5 total txns"
        );

        // Step 4: verify snap1 still sees only the 3 original user datoms
        // (INV-FERR-006: snapshot frozen at capture time)
        let snap1_datoms: std::collections::BTreeSet<_> = snap1.datoms().cloned().collect();
        // Count check: 3 user datoms + genesis meta-schema datoms.
        // The exact count depends on genesis, so we verify the user datom
        // count by checking that snap1 has FEWER datoms than the current store.
        let current_datom_count = db.snapshot().datoms().count();
        assert!(
            snap1_datoms.len() < current_datom_count,
            "INV-FERR-006: snapshot at epoch 3 must have fewer datoms than store at epoch 5"
        );
        for i in 0..3 {
            let expected = EntityId::from_content(format!("user-{i}").as_bytes());
            assert!(
                snap1_datoms.iter().any(|d| d.entity() == expected),
                "INV-FERR-006: snap1 must contain user-{i}"
            );
        }
        // snap1 must NOT contain the datoms from the later 2 transactions
        for i in 3..5 {
            let unexpected = EntityId::from_content(format!("user-{i}").as_bytes());
            assert!(
                !snap1_datoms.iter().any(|d| d.entity() == unexpected),
                "INV-FERR-006: snap1 must NOT contain user-{i} (transacted after snapshot)"
            );
        }

        // db drops here -- simulates crash. snap1 is in-memory only and is lost.
    }

    // Step 6: cold start recovery -> all 5 committed datoms must be present
    let recovered_db = Database::recover_from_wal(&wal_path)
        .expect("INV-FERR-014: recovery after snapshot+crash must succeed");
    let (recovered_datoms, recovered_epoch, _) = capture_db_state(&recovered_db);
    assert_eq!(
        recovered_epoch, 5,
        "INV-FERR-014: recovered epoch must be 5, got {recovered_epoch}"
    );
    assert_entities_present(
        &recovered_datoms,
        0..5,
        "INV-FERR-014: all 5 user datoms must survive crash+recovery",
    );

    // Step 7: new snapshot on recovered DB sees all 5 datoms
    let snap_recovered = recovered_db.snapshot();
    let snap_recovered_datoms: std::collections::BTreeSet<_> =
        snap_recovered.datoms().cloned().collect();
    assert_eq!(
        snap_recovered.epoch(),
        5,
        "INV-FERR-006: snapshot on recovered DB must have epoch 5"
    );
    for i in 0..5 {
        let expected = EntityId::from_content(format!("user-{i}").as_bytes());
        assert!(
            snap_recovered_datoms.iter().any(|d| d.entity() == expected),
            "INV-FERR-014: recovered snapshot must contain user-{i}"
        );
    }

    // Step 8: The original snap1 is gone -- it was in-memory only.
    // This is a documentation assertion, not a code assertion, because
    // snap1 went out of scope when the db was dropped. The fact that we
    // can't reference it here IS the proof that it's gone.
}
