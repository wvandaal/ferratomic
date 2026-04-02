//! WAL recovery integration tests.
//!
//! INV-FERR-008, INV-FERR-014, INV-FERR-024 (in-memory backend).
//! Phase 4a: all tests passing against ferratomic-core implementation.

use std::io::Write;

use ferratom::{AgentId, Attribute, EntityId, Value};
use ferratomic_core::{db::Database, wal::Wal, writer::Transaction};
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
    wal.append(1, &tx).expect("append failed");
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
            wal.append(i, &tx).expect("append failed");
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
            "{context}: user-{i} entity must be present"
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
    assert_eq!(expected.1, actual.1, "{label}: epoch mismatch");
    assert_eq!(expected.2, actual.2, "{label}: schema mismatch");
    assert_eq!(expected.0, actual.0, "{label}: datom set mismatch");
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

/// Write a checkpoint from the current database snapshot.
fn write_checkpoint_from_db(db: &Database, checkpoint_path: &std::path::Path) {
    use ferratomic_core::checkpoint::write_checkpoint;
    let snap = db.snapshot();
    let mut store = ferratomic_core::store::Store::genesis();
    for d in snap.datoms() {
        store.insert(d);
    }
    write_checkpoint(&store, checkpoint_path).expect("INV-FERR-014: checkpoint write must succeed");
}

/// Assert that all dc-entity-{0..count} entities are present in a database snapshot.
fn assert_dc_entities_present(db: &Database, count: u64, context: &str) {
    let snap = db.snapshot();
    for i in 0..count {
        let expected = EntityId::from_content(format!("dc-entity-{i}").as_bytes());
        assert!(
            snap.datoms().any(|d| d.entity() == expected),
            "{context}: dc-entity-{i} must be present"
        );
    }
}

/// INV-FERR-014: Double-crash recovery with checkpoint in between.
///
/// Full lifecycle: genesis --> transact 3 --> checkpoint --> transact 2 -->
/// crash 1 --> cold_start --> transact 1 --> crash 2 --> cold_start --> verify.
#[test]
fn test_inv_ferr_014_double_crash() {
    use ferratomic_core::storage::{cold_start, RecoveryLevel};

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
    assert!(
        result2.level == RecoveryLevel::CheckpointPlusWal
            || result2.level == RecoveryLevel::WalOnly,
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
fn write_test_checkpoint_to_backend(backend: &ferratomic_core::storage::InMemoryBackend) {
    use ferratomic_core::storage::StorageBackend;

    let mut store = ferratomic_core::store::Store::genesis();
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
    ferratomic_core::checkpoint::write_checkpoint_to_writer(&store, &mut writer)
        .expect("INV-FERR-024: write checkpoint to in-memory backend");
}

/// INV-FERR-024: `InMemoryBackend` supports `cold_start_with_backend`.
///
/// bd-7tb0: integration test verifying the `InMemoryBackend` trait implementation
/// works with the generic `cold_start_with_backend` path. Empty backend produces
/// genesis; backend with a checkpoint restores state.
#[test]
fn test_inv_ferr_024_in_memory_backend() {
    use ferratomic_core::storage::{
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
