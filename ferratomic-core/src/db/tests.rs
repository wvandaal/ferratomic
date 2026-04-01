use std::sync::{Arc as StdArc, Mutex as StdMutex};

use ferratom::{AgentId, Attribute, EntityId, Value};

use super::*;
use crate::{observer::DatomObserver, wal::Wal, writer::Transaction};

#[derive(Debug, Clone, PartialEq, Eq)]
enum ObserverEvent {
    Commit { epoch: u64, count: usize },
    Catchup { from_epoch: u64, count: usize },
}

struct RecordingObserver {
    name: &'static str,
    events: StdArc<StdMutex<Vec<ObserverEvent>>>,
}

impl RecordingObserver {
    fn new(name: &'static str, events: StdArc<StdMutex<Vec<ObserverEvent>>>) -> Self {
        Self { name, events }
    }
}

impl DatomObserver for RecordingObserver {
    fn on_commit(&self, epoch: u64, datoms: &[ferratom::Datom]) {
        self.events
            .lock()
            .expect("observer commit events lock")
            .push(ObserverEvent::Commit {
                epoch,
                count: datoms.len(),
            });
    }

    fn on_catchup(&self, from_epoch: u64, datoms: &[ferratom::Datom]) {
        self.events
            .lock()
            .expect("observer catchup events lock")
            .push(ObserverEvent::Catchup {
                from_epoch,
                count: datoms.len(),
            });
    }

    fn name(&self) -> &str {
        self.name
    }
}

/// INV-FERR-031: genesis produces a deterministic database.
#[test]
fn test_inv_ferr_031_genesis_determinism() {
    let db1 = Database::genesis();
    let db2 = Database::genesis();
    assert_eq!(
        db1.epoch(),
        db2.epoch(),
        "INV-FERR-031: genesis databases must have identical epochs"
    );
    let s1 = db1.snapshot();
    let s2 = db2.snapshot();
    assert_eq!(
        s1.epoch(),
        s2.epoch(),
        "INV-FERR-031: genesis snapshots must have identical epochs"
    );
}

/// INV-FERR-006: snapshot isolation -- a snapshot taken before a write
/// does not see the write's effects.
#[test]
fn test_inv_ferr_006_snapshot_isolation() {
    let db = Database::genesis();
    let before = db.snapshot();

    let agent = AgentId::from_bytes([1u8; 16]);
    let schema = db.schema();
    let tx = Transaction::new(agent)
        .assert_datom(
            EntityId::from_content(b"e1"),
            Attribute::from("db/doc"),
            Value::String("hello".into()),
        )
        .commit(&schema);

    match tx {
        Ok(committed) => {
            let result = db.transact(committed);
            assert!(
                result.is_ok(),
                "INV-FERR-007: transact on genesis db must succeed"
            );

            let after = db.snapshot();

            assert_eq!(
                before.epoch(),
                0,
                "INV-FERR-006: pre-write snapshot epoch must be 0"
            );
            assert_eq!(
                after.epoch(),
                1,
                "INV-FERR-007: post-write snapshot epoch must be 1"
            );
        }
        Err(error) => panic!("Transaction commit failed unexpectedly: {error}"),
    }
}

/// INV-FERR-007: epoch strictly increases with each transact.
#[test]
fn test_inv_ferr_007_epoch_monotonicity() {
    let db = Database::genesis();
    assert_eq!(db.epoch(), 0, "INV-FERR-031: genesis epoch is 0");

    let agent = AgentId::from_bytes([2u8; 16]);
    let schema = db.schema();

    for iteration in 1u64..=3 {
        let tx = Transaction::new(agent)
            .assert_datom(
                EntityId::from_content(format!("e{iteration}").as_bytes()),
                Attribute::from("db/doc"),
                Value::String(format!("doc-{iteration}").into()),
            )
            .commit(&schema);

        match tx {
            Ok(committed) => {
                let receipt = db.transact(committed);
                match receipt {
                    Ok(result) => assert_eq!(
                        result.epoch(),
                        iteration,
                        "INV-FERR-007: epoch must equal {iteration} after transaction {iteration}"
                    ),
                    Err(error) => panic!("transact failed on iteration {iteration}: {error}"),
                }
            }
            Err(error) => panic!("commit failed on iteration {iteration}: {error}"),
        }
    }

    assert_eq!(
        db.epoch(),
        3,
        "INV-FERR-007: final epoch must be 3 after 3 transactions"
    );
}

/// INV-FERR-006: `from_store` preserves the store's state.
#[test]
fn test_inv_ferr_006_from_store() {
    let store = Store::genesis();
    let epoch = store.epoch();
    let db = Database::from_store(store);
    assert_eq!(
        db.epoch(),
        epoch,
        "INV-FERR-006: from_store must preserve epoch"
    );
}

#[test]
fn test_inv_ferr_011_register_observer_delivers_catchup() {
    let db = Database::genesis();
    let agent = AgentId::from_bytes([7u8; 16]);
    let schema = db.schema();

    for index in 0..2i64 {
        let tx = Transaction::new(agent)
            .assert_datom(
                EntityId::from_content(format!("catchup-{index}").as_bytes()),
                Attribute::from("db/doc"),
                Value::String(format!("doc-{index}").into()),
            )
            .commit(&schema)
            .expect("valid tx");
        db.transact(tx).expect("transact succeeds");
    }

    let events = StdArc::new(StdMutex::new(Vec::new()));
    let observer = Box::new(RecordingObserver::new("catchup", StdArc::clone(&events)));
    db.register_observer(observer)
        .expect("observer registration succeeds");

    let recorded = events.lock().expect("events lock");
    assert!(
        matches!(recorded.as_slice(), [ObserverEvent::Catchup { from_epoch: 0, count }] if *count > 0),
        "register_observer must catch up existing state, got {:?}",
        *recorded
    );
}

#[test]
fn test_inv_ferr_011_transact_notifies_registered_observer() {
    let db = Database::genesis();
    let events = StdArc::new(StdMutex::new(Vec::new()));
    let observer = Box::new(RecordingObserver::new("commit", StdArc::clone(&events)));
    db.register_observer(observer)
        .expect("observer registration succeeds");

    let schema = db.schema();
    let tx = Transaction::new(AgentId::from_bytes([8u8; 16]))
        .assert_datom(
            EntityId::from_content(b"observer-commit"),
            Attribute::from("db/doc"),
            Value::String("observed".into()),
        )
        .commit(&schema)
        .expect("valid tx");
    db.transact(tx).expect("transact succeeds");

    let recorded = events.lock().expect("events lock");
    assert!(
        recorded.iter().any(|event| {
            matches!(event, ObserverEvent::Commit { epoch: 1, count } if *count > 0)
        }),
        "registered observer must receive commit notification, got {:?}",
        *recorded
    );
}

/// Regression: bd-2w9 -- Database with WAL writes WAL before epoch advance.
#[test]
fn test_bug_bd_2w9_wal_written_on_transact() {
    let dir = tempfile::TempDir::new().unwrap();
    let wal_path = dir.path().join("test.wal");

    let db = Database::genesis_with_wal(&wal_path).unwrap();
    let agent = AgentId::from_bytes([1u8; 16]);
    let schema = db.schema();

    let tx = Transaction::new(agent)
        .assert_datom(
            EntityId::from_content(b"e1"),
            Attribute::from("db/doc"),
            Value::String("hello from wal".into()),
        )
        .commit(&schema)
        .expect("valid tx");

    db.transact(tx).expect("transact should succeed");

    let mut wal = Wal::open(&wal_path).expect("WAL must exist");
    let entries = wal.recover().expect("recovery must succeed");
    assert_eq!(
        entries.len(),
        1,
        "bd-2w9: WAL must contain 1 entry after 1 transact"
    );
    assert_eq!(entries[0].epoch, 1, "bd-2w9: WAL entry epoch must be 1");
}

/// Regression: bd-2w9 -- `recover_from_wal` restores state.
#[test]
fn test_bug_bd_2w9_recover_from_wal() {
    let dir = tempfile::TempDir::new().unwrap();
    let wal_path = dir.path().join("test.wal");

    {
        let db = Database::genesis_with_wal(&wal_path).unwrap();
        let agent = AgentId::from_bytes([1u8; 16]);
        let schema = db.schema();

        for index in 0..3i64 {
            let tx = Transaction::new(agent)
                .assert_datom(
                    EntityId::from_content(format!("e{index}").as_bytes()),
                    Attribute::from("db/doc"),
                    Value::String(format!("doc-{index}").into()),
                )
                .commit(&schema)
                .expect("valid tx");
            db.transact(tx).expect("transact ok");
        }
    }

    let recovered = Database::recover_from_wal(&wal_path).expect("recovery must succeed");
    let snap = recovered.snapshot();

    assert!(
        snap.datoms().count() > 0,
        "bd-2w9: recovered database must have datoms"
    );
}

/// Regression: bd-85v1 -- release bijection canary returns a typed error.
#[cfg(feature = "release_bijection_check")]
#[test]
fn test_bug_bd_85v1_bijection_canary_returns_invariant_violation() {
    use std::sync::atomic::Ordering;

    use ferratom::FerraError;

    use crate::indexes::Indexes;

    let db = Database::genesis();
    let agent = AgentId::from_bytes([9u8; 16]);
    let schema = db.schema();

    let seed_tx = Transaction::new(agent)
        .assert_datom(
            EntityId::from_content(b"seed"),
            Attribute::from("db/doc"),
            Value::String("seed".into()),
        )
        .commit(&schema)
        .expect("seed transaction should commit");
    db.transact(seed_tx).expect("seed transact should succeed");

    let current = db.current.load();
    let mut corrupted = Store::clone(&current);
    corrupted.indexes = Indexes::from_datoms(std::iter::empty());
    db.current.store(StdArc::new(corrupted));
    db.transaction_count.store(99, Ordering::Release);

    let tx = Transaction::new(agent)
        .assert_datom(
            EntityId::from_content(b"trigger"),
            Attribute::from("db/doc"),
            Value::String("trigger".into()),
        )
        .commit(&schema)
        .expect("trigger transaction should commit");

    let result = db.transact(tx);
    assert!(
        matches!(
            result,
            Err(FerraError::InvariantViolation { invariant, .. })
                if invariant == "INV-FERR-005"
        ),
        "bd-85v1: release bijection canary must return InvariantViolation, got {result:?}"
    );
}
