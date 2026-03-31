//! WAL recovery integration tests.
//!
//! INV-FERR-008, INV-FERR-014, INV-FERR-024 (in-memory backend).
//! Phase 4a: all tests passing against ferratomic-core implementation.

use std::io::Write;

use ferratom::{AgentId, Attribute, EntityId, Value};
use ferratomic_core::{db::Database, wal::Wal, writer::Transaction};
use tempfile::TempDir;

/// INV-FERR-008: Basic WAL write and recovery.
#[test]
#[allow(clippy::too_many_lines)]
// Test complexity justified — WAL roundtrip with deserialization verification
fn inv_ferr_008_wal_write_and_recover() {
    let dir = TempDir::new().expect("failed to create temp dir");
    let wal_path = dir.path().join("test.wal");

    let agent = AgentId::from_bytes([1u8; 16]);

    // Write one entry
    {
        let mut wal = Wal::create(&wal_path).expect("create WAL");
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

    // Recover and verify
    {
        let mut wal = Wal::open(&wal_path).expect("open WAL");
        let entries = wal.recover().expect("recovery failed");
        assert_eq!(
            entries.len(),
            1,
            "INV-FERR-008: expected 1 entry, got {}",
            entries.len()
        );

        // CR-027: Deserialize the recovered payload and verify datom content.
        // ADR-FERR-010: Deserialize as wire types, convert through trust boundary.
        let wire_datoms: Vec<ferratom::wire::WireDatom> = bincode::deserialize(&entries[0].payload)
            .expect("INV-FERR-008: recovered payload must deserialize as Vec<WireDatom>");
        let datoms: Vec<ferratom::Datom> = wire_datoms
            .into_iter()
            .map(ferratom::wire::WireDatom::into_trusted)
            .collect();
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

/// INV-FERR-014: Crash-then-transact roundtrip.
///
/// Full lifecycle: genesis → transact N datoms → crash (drop) → recover
/// from WAL → verify identical state → transact M more → verify epoch
/// advances correctly and all N+M user datoms are present.
#[test]
#[allow(clippy::too_many_lines)]
// Test complexity justified — multi-phase scenario (genesis, crash, recovery, post-recovery transact)
fn test_inv_ferr_014_crash_then_transact() {
    let dir = TempDir::new().expect("failed to create temp dir");
    let wal_path = dir.path().join("crash_roundtrip.wal");

    let agent = AgentId::from_bytes([42u8; 16]);

    // -- Phase 1: genesis + transact 3 user datoms ----------------------------
    let (pre_crash_datoms, pre_crash_epoch, pre_crash_schema) = {
        let db = Database::genesis_with_wal(&wal_path).expect("genesis_with_wal must succeed");

        for i in 0..3i64 {
            let tx = Transaction::new(agent)
                .assert_datom(
                    EntityId::from_content(format!("user-{i}").as_bytes()),
                    Attribute::from("user/name"),
                    Value::String(format!("User {i}").into()),
                )
                .commit_unchecked();
            db.transact(tx).expect("transact failed");
        }

        let snap = db.snapshot();
        let datoms: std::collections::BTreeSet<_> = snap.datoms().cloned().collect();
        let epoch = db.epoch();
        let schema = db.schema();
        (datoms, epoch, schema)
        // db drops here — simulates crash
    };

    assert_eq!(
        pre_crash_epoch, 3,
        "INV-FERR-014: 3 transactions must produce epoch 3"
    );

    // -- Phase 2: recover from WAL and verify identical state -----------------
    let recovered_db =
        Database::recover_from_wal(&wal_path).expect("recovery from WAL must succeed");

    let recovered_snap = recovered_db.snapshot();
    let recovered_datoms: std::collections::BTreeSet<_> =
        recovered_snap.datoms().cloned().collect();
    let recovered_epoch = recovered_db.epoch();
    let recovered_schema = recovered_db.schema();

    assert_eq!(
        recovered_epoch, pre_crash_epoch,
        "INV-FERR-014: recovered epoch must equal pre-crash epoch. \
         recovered={recovered_epoch}, expected={pre_crash_epoch}"
    );
    assert_eq!(
        recovered_schema, pre_crash_schema,
        "INV-FERR-014: recovered schema must be identical to pre-crash schema"
    );
    assert_eq!(
        recovered_datoms,
        pre_crash_datoms,
        "INV-FERR-014: recovered datoms must be identical to pre-crash datoms. \
         recovered={}, pre_crash={}",
        recovered_datoms.len(),
        pre_crash_datoms.len()
    );

    // -- Phase 3: transact 2 more datoms on recovered database ----------------
    for i in 3..5i64 {
        let tx = Transaction::new(agent)
            .assert_datom(
                EntityId::from_content(format!("user-{i}").as_bytes()),
                Attribute::from("user/name"),
                Value::String(format!("User {i}").into()),
            )
            .commit_unchecked();
        recovered_db
            .transact(tx)
            .expect("transact on recovered db failed");
    }

    // -- Phase 4: verify epoch advances and all datoms present ----------------
    let final_epoch = recovered_db.epoch();
    assert_eq!(
        final_epoch,
        recovered_epoch + 2,
        "INV-FERR-014: epoch must advance by 2 after 2 post-recovery transactions. \
         final={final_epoch}, expected={}",
        recovered_epoch + 2
    );

    let final_snap = recovered_db.snapshot();
    let final_datoms: std::collections::BTreeSet<_> = final_snap.datoms().cloned().collect();

    // All 5 user datoms must be findable by entity ID.
    for i in 0..5i64 {
        let expected_entity = EntityId::from_content(format!("user-{i}").as_bytes());
        assert!(
            final_datoms.iter().any(|d| d.entity() == expected_entity),
            "INV-FERR-014: user-{i} entity must be present in final state"
        );
    }

    // Final state must be a strict superset of the recovered state (only
    // additions, never deletions — append-only C1).
    assert!(
        final_datoms.is_superset(&recovered_datoms),
        "INV-FERR-014: post-recovery state must be a superset of recovered state. \
         final={}, recovered={}",
        final_datoms.len(),
        recovered_datoms.len()
    );
}

/// INV-FERR-008: WAL entry must precede snapshot visibility.
/// After commit, WAL recovery alone must reproduce all visible datoms,
/// epoch, and schema -- exact state equality.
#[test]
#[allow(clippy::too_many_lines)]
// Test complexity justified — full WAL lifecycle with exact state equality verification
fn inv_ferr_008_wal_entry_precedes_snapshot() {
    let dir = TempDir::new().expect("failed to create temp dir");
    let wal_path = dir.path().join("test.wal");

    let db = Database::genesis_with_wal(&wal_path).expect("failed to create store with WAL");

    let agent = AgentId::from_bytes([1u8; 16]);
    for i in 0..5i64 {
        let tx = Transaction::new(agent)
            .assert_datom(
                EntityId::from_content(format!("e{}", i).as_bytes()),
                Attribute::from("tx/provenance"),
                Value::String(format!("test-{i}").into()),
            )
            .commit(&db.schema())
            .expect("valid tx");
        db.transact(tx).expect("transact failed");
    }

    let snapshot_datoms: std::collections::BTreeSet<_> = db.snapshot().datoms().cloned().collect();
    let expected_epoch = db.epoch();
    let expected_schema = db.schema();

    // Recover from WAL alone (simulating crash + restart)
    let recovered_db = Database::recover_from_wal(&wal_path).expect("recovery failed");
    let recovered_datoms: std::collections::BTreeSet<_> =
        recovered_db.snapshot().datoms().cloned().collect();

    // INV-FERR-008: exact state equality -- datoms, epoch, schema.
    assert_eq!(
        snapshot_datoms,
        recovered_datoms,
        "INV-FERR-008: WAL recovery produced different datom set. \
         pre-crash={}, recovered={}",
        snapshot_datoms.len(),
        recovered_datoms.len()
    );
    assert_eq!(
        expected_epoch,
        recovered_db.epoch(),
        "INV-FERR-008: WAL recovery produced different epoch. \
         pre-crash={expected_epoch}, recovered={}",
        recovered_db.epoch()
    );
    assert_eq!(
        expected_schema,
        recovered_db.schema(),
        "INV-FERR-008: WAL recovery produced different schema"
    );
}

/// INV-FERR-014: Double-crash recovery with checkpoint in between.
///
/// Full lifecycle: genesis --> transact 3 datoms --> write checkpoint -->
/// transact 2 datoms --> drop (crash 1) --> `cold_start` (recover 1) -->
/// assert 5 user datoms present --> transact 1 datom --> drop (crash 2) -->
/// `cold_start` (recover 2) --> assert 6 user datoms present.
///
/// This exercises the critical path where a checkpoint exists but WAL
/// entries extend beyond it, and then a second crash occurs after
/// recovery, requiring another `cold_start` to restore state.
#[test]
#[allow(clippy::too_many_lines)]
// Test complexity justified — multi-phase scenario (4 phases: genesis, recovery 1, crash 2, recovery 2)
fn test_inv_ferr_014_double_crash() {
    use ferratomic_core::{
        checkpoint::write_checkpoint,
        storage::{cold_start, RecoveryLevel},
    };

    let dir = TempDir::new().expect("failed to create temp dir");
    let data_dir = dir.path().join("double_crash");
    std::fs::create_dir_all(&data_dir).expect("create data dir");

    let wal_path = data_dir.join("wal.log");
    let checkpoint_path = data_dir.join("checkpoint.chkp");

    let agent = AgentId::from_bytes([77u8; 16]);

    // -- Phase 1: genesis + transact 3 user datoms ----------------------------
    {
        let db = Database::genesis_with_wal(&wal_path).expect("genesis_with_wal must succeed");
        let schema = db.schema();

        for i in 0..3u64 {
            let tx = Transaction::new(agent)
                .assert_datom(
                    EntityId::from_content(format!("dc-entity-{i}").as_bytes()),
                    Attribute::from("db/doc"),
                    Value::String(format!("double-crash-{i}").into()),
                )
                .commit(&schema)
                .expect("INV-FERR-014: transaction must validate against schema");
            db.transact(tx)
                .expect("INV-FERR-014: transact must succeed");
        }

        // Write checkpoint after 3 transactions.
        let store_for_checkpoint = {
            let snap = db.snapshot();
            let mut s = ferratomic_core::store::Store::genesis();
            for d in snap.datoms() {
                s.insert(d);
            }
            s
        };
        write_checkpoint(&store_for_checkpoint, &checkpoint_path)
            .expect("INV-FERR-014: checkpoint write must succeed");

        // Transact 2 more datoms (these will be WAL-only, after checkpoint).
        for i in 3..5u64 {
            let tx = Transaction::new(agent)
                .assert_datom(
                    EntityId::from_content(format!("dc-entity-{i}").as_bytes()),
                    Attribute::from("db/doc"),
                    Value::String(format!("double-crash-{i}").into()),
                )
                .commit(&schema)
                .expect("INV-FERR-014: post-checkpoint transaction must validate");
            db.transact(tx)
                .expect("INV-FERR-014: post-checkpoint transact must succeed");
        }

        // db drops here — simulates crash 1
    }

    // -- Phase 2: cold_start recovery 1 — checkpoint + WAL --------------------
    let result1 = cold_start(&data_dir).expect("INV-FERR-014: first cold_start must succeed");

    assert_eq!(
        result1.level,
        RecoveryLevel::CheckpointPlusWal,
        "INV-FERR-014: first recovery must use CheckpointPlusWal \
         (checkpoint exists and WAL has entries beyond it)"
    );

    // Assert all 5 user datoms present after first recovery.
    {
        let snap = result1.database.snapshot();
        for i in 0..5u64 {
            let expected_entity = EntityId::from_content(format!("dc-entity-{i}").as_bytes());
            assert!(
                snap.datoms().any(|d| d.entity() == expected_entity),
                "INV-FERR-014: dc-entity-{i} must be present after first recovery"
            );
        }
    }

    // Assert epoch is correct after first recovery (5 transactions committed).
    let epoch_after_recovery1 = result1.database.epoch();
    assert!(
        epoch_after_recovery1 >= 5,
        "INV-FERR-014: epoch after first recovery must be >= 5 (5 transactions committed). \
         got={epoch_after_recovery1}"
    );

    // -- Phase 3: transact 1 more datom on recovered database, then crash 2 ---
    {
        let schema = result1.database.schema();
        let tx = Transaction::new(agent)
            .assert_datom(
                EntityId::from_content(b"dc-entity-5"),
                Attribute::from("db/doc"),
                Value::String("double-crash-5".into()),
            )
            .commit(&schema)
            .expect("INV-FERR-014: post-recovery transaction must validate");
        result1
            .database
            .transact(tx)
            .expect("INV-FERR-014: post-recovery transact must succeed");

        // result1.database drops here — simulates crash 2
    }

    // -- Phase 4: cold_start recovery 2 — verify all 6 user datoms -----------
    let result2 = cold_start(&data_dir).expect("INV-FERR-014: second cold_start must succeed");

    // Second recovery may be CheckpointPlusWal or WalOnly depending on
    // whether cold_start created a new WAL after recovery 1.
    assert!(
        result2.level == RecoveryLevel::CheckpointPlusWal
            || result2.level == RecoveryLevel::WalOnly,
        "INV-FERR-014: second recovery must be CheckpointPlusWal or WalOnly. \
         got={:?}",
        result2.level
    );

    // Assert all 6 user datoms present after second recovery.
    {
        let snap = result2.database.snapshot();
        for i in 0..6u64 {
            let expected_entity = EntityId::from_content(format!("dc-entity-{i}").as_bytes());
            assert!(
                snap.datoms().any(|d| d.entity() == expected_entity),
                "INV-FERR-014: dc-entity-{i} must be present after second recovery"
            );
        }
    }

    // Assert epoch is correct after second recovery (6 transactions total).
    let epoch_after_recovery2 = result2.database.epoch();
    assert!(
        epoch_after_recovery2 >= 6,
        "INV-FERR-014: epoch after second recovery must be >= 6 (6 transactions committed). \
         got={epoch_after_recovery2}"
    );

    // Epoch must not regress between recoveries.
    assert!(
        epoch_after_recovery2 >= epoch_after_recovery1,
        "INV-FERR-014: epoch must not regress between recoveries. \
         recovery1={epoch_after_recovery1}, recovery2={epoch_after_recovery2}"
    );
}

/// INV-FERR-024: `InMemoryBackend` supports `cold_start_with_backend`.
///
/// bd-7tb0: integration test verifying the `InMemoryBackend` trait implementation
/// works with the generic `cold_start_with_backend` path. Empty backend produces
/// genesis; backend with a checkpoint restores state.
#[test]
#[allow(clippy::too_many_lines)]
// Test complexity justified — two-phase in-memory backend cold_start verification
fn test_inv_ferr_024_in_memory_backend() {
    use ferratomic_core::storage::{
        cold_start_with_backend, InMemoryBackend, RecoveryLevel, StorageBackend,
    };

    // Phase 1: Empty backend produces genesis.
    let backend = InMemoryBackend::new();
    let result = cold_start_with_backend(&backend)
        .expect("INV-FERR-024: cold_start_with_backend must succeed on empty backend");

    assert_eq!(
        result.level,
        RecoveryLevel::Genesis,
        "INV-FERR-024: empty in-memory backend must produce genesis recovery level"
    );
    assert_eq!(
        result.database.epoch(),
        0,
        "INV-FERR-024: genesis database must have epoch 0"
    );

    // Phase 2: Write a checkpoint into the backend, then cold-start again.
    {
        let mut store = ferratomic_core::store::Store::genesis();
        let tx = ferratomic_core::writer::Transaction::new(AgentId::from_bytes([24u8; 16]))
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

    assert!(
        backend.checkpoint_exists(),
        "INV-FERR-024: checkpoint must exist after write"
    );

    let result2 = cold_start_with_backend(&backend)
        .expect("INV-FERR-024: cold_start_with_backend must succeed with checkpoint");

    assert_eq!(
        result2.level,
        RecoveryLevel::CheckpointOnly,
        "INV-FERR-024: backend with checkpoint must recover at CheckpointOnly level"
    );
    assert!(
        result2.database.snapshot().datoms().count() > 0,
        "INV-FERR-024: recovered database must contain datoms from checkpoint"
    );
}
