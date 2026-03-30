//! WAL recovery integration tests.
//!
//! INV-FERR-008.
//! ALL TESTS MUST FAIL (red phase). Types are not yet implemented.

use ferratom::{AgentId, Attribute, Datom, EntityId, Op, TxId, Value};
use ferratomic_core::store::Store;
use ferratomic_core::wal::Wal;
use ferratomic_core::writer::Transaction;
use std::io::Write;
use tempfile::TempDir;

/// INV-FERR-008: Basic WAL write and recovery.
#[test]
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
            let tx = Transaction::new(agent.clone())
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

/// INV-FERR-008: WAL entry must precede snapshot visibility.
/// After commit, WAL recovery alone must reproduce all visible datoms.
#[test]
fn inv_ferr_008_wal_entry_precedes_snapshot() {
    let dir = TempDir::new().expect("failed to create temp dir");
    let wal_path = dir.path().join("test.wal");

    let mut store =
        Store::genesis_with_wal(&wal_path).expect("failed to create store with WAL");

    let agent = AgentId::from_bytes([1u8; 16]);
    for i in 0..5i64 {
        let tx = Transaction::new(agent.clone())
            .assert_datom(
                EntityId::from_content(format!("e{}", i).as_bytes()),
                Attribute::from("test/data"),
                Value::Long(i),
            )
            .commit(store.schema())
            .expect("valid tx");
        store.transact(tx).expect("transact failed");
    }

    let snapshot_datoms: std::collections::BTreeSet<_> =
        store.snapshot().datoms().cloned().collect();

    // Recover from WAL alone (simulating crash + restart)
    let recovered_store = Store::recover_from_wal(&wal_path).expect("recovery failed");
    let recovered_datoms: std::collections::BTreeSet<_> =
        recovered_store.snapshot().datoms().cloned().collect();

    assert_eq!(
        snapshot_datoms, recovered_datoms,
        "INV-FERR-008: WAL recovery produced different state. \
         pre-crash={}, recovered={}",
        snapshot_datoms.len(),
        recovered_datoms.len()
    );
}
