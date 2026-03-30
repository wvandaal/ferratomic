//! Snapshot isolation integration tests.
//!
//! INV-FERR-006, INV-FERR-007, INV-FERR-011.
//! ALL TESTS MUST FAIL (red phase). Types are not yet implemented.

use std::sync::{Arc, Mutex};

use ferratom::{AgentId, Attribute, EntityId, Value};
use ferratomic_core::{
    db::Database,
    observer::{DatomObserver, Observer},
    store::Store,
    writer::Transaction,
};

#[derive(Debug, Clone, PartialEq, Eq)]
enum ObserverEvent {
    Commit { epoch: u64, count: usize },
    Catchup { from_epoch: u64, count: usize },
}

struct RecordingObserver {
    events: Arc<Mutex<Vec<ObserverEvent>>>,
}

impl RecordingObserver {
    fn new(events: Arc<Mutex<Vec<ObserverEvent>>>) -> Self {
        Self { events }
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
        "integration-recorder"
    }
}

/// INV-FERR-006: Snapshot is stable — does not change after later writes.
#[test]
fn inv_ferr_006_snapshot_stability() {
    let mut store = Store::genesis();
    let agent = AgentId::from_bytes([1u8; 16]);

    // Commit first transaction
    let tx1 = Transaction::new(agent.clone())
        .assert_datom(
            EntityId::from_content(b"e1"),
            Attribute::from("db/doc"),
            Value::String("Alice".into()),
        )
        .commit(store.schema())
        .expect("valid tx");
    store.transact(tx1).expect("transact failed");

    // Take snapshot
    let snap = store.snapshot();
    let snap_count = snap.datoms().count();

    // Commit second transaction AFTER snapshot
    let tx2 = Transaction::new(agent)
        .assert_datom(
            EntityId::from_content(b"e2"),
            Attribute::from("db/doc"),
            Value::String("Bob".into()),
        )
        .commit(store.schema())
        .expect("valid tx");
    store.transact(tx2).expect("transact failed");

    // Snapshot must NOT see the second transaction
    let snap_count_after = snap.datoms().count();
    assert_eq!(
        snap_count, snap_count_after,
        "INV-FERR-006: snapshot changed after later transaction. \
         before={}, after={}",
        snap_count, snap_count_after
    );
}

/// INV-FERR-006: Concurrent reads don't see in-progress writes.
#[test]
fn inv_ferr_006_concurrent_read_write() {
    let mut store = Store::genesis();
    let agent = AgentId::from_bytes([1u8; 16]);

    // Snapshot before any user transactions
    let snap_before = store.snapshot();
    let count_before = snap_before.datoms().count();

    // Commit a transaction
    let tx = Transaction::new(agent)
        .assert_datom(
            EntityId::from_content(b"e1"),
            Attribute::from("db/doc"),
            Value::String("Alice".into()),
        )
        .commit(store.schema())
        .expect("valid tx");
    store.transact(tx).expect("transact failed");

    // Snapshot after transaction
    let snap_after = store.snapshot();
    let count_after = snap_after.datoms().count();

    // snap_before must not have changed
    assert_eq!(
        count_before,
        snap_before.datoms().count(),
        "INV-FERR-006: pre-transaction snapshot was mutated"
    );

    // snap_after must see the new datoms
    assert!(
        count_after > count_before,
        "INV-FERR-006: post-transaction snapshot missing new datoms. \
         before={}, after={}",
        count_before,
        count_after
    );
}

/// INV-FERR-007: Epochs are strictly monotonically increasing.
#[test]
fn inv_ferr_007_epoch_ordering() {
    let mut store = Store::genesis();
    let agent = AgentId::from_bytes([1u8; 16]);

    let mut epochs = Vec::new();
    for i in 0..5i64 {
        let tx = Transaction::new(agent.clone())
            .assert_datom(
                EntityId::from_content(format!("e{}", i).as_bytes()),
                Attribute::from("tx/provenance"),
                Value::String(format!("test-{i}").into()),
            )
            .commit(store.schema())
            .expect("valid tx");
        let receipt = store.transact(tx).expect("transact failed");
        epochs.push(receipt.epoch());
    }

    for i in 1..epochs.len() {
        assert!(
            epochs[i] > epochs[i - 1],
            "INV-FERR-007: epoch did not strictly increase. \
             epoch[{}]={}, epoch[{}]={}",
            i - 1,
            epochs[i - 1],
            i,
            epochs[i]
        );
    }
}

/// INV-FERR-011: Observer epoch is monotonically non-decreasing.
#[test]
fn inv_ferr_011_observer_epoch_monotonic() {
    let mut store = Store::genesis();
    let agent = AgentId::from_bytes([1u8; 16]);
    let observer = Observer::new(AgentId::from_bytes([2u8; 16]));

    let mut prev_epoch = 0u64;

    for i in 0..10i64 {
        let tx = Transaction::new(agent.clone())
            .assert_datom(
                EntityId::from_content(format!("e{}", i).as_bytes()),
                Attribute::from("tx/provenance"),
                Value::String(format!("test-{i}").into()),
            )
            .commit(store.schema())
            .expect("valid tx");
        store.transact(tx).expect("transact failed");

        let snap = observer.observe(&store);
        let epoch = snap.epoch();

        assert!(
            epoch >= prev_epoch,
            "INV-FERR-011: observer epoch regressed. prev={}, current={}",
            prev_epoch,
            epoch
        );
        prev_epoch = epoch;
    }
}

/// INV-FERR-011: registering after writes triggers catch-up delivery.
#[test]
fn inv_ferr_011_database_observer_catchup_delivery() {
    let db = Database::genesis();
    let agent = AgentId::from_bytes([3u8; 16]);

    for i in 0..2i64 {
        let tx = Transaction::new(agent)
            .assert_datom(
                EntityId::from_content(format!("catchup-e{}", i).as_bytes()),
                Attribute::from("db/doc"),
                Value::String(format!("catchup-{i}").into()),
            )
            .commit(&db.schema())
            .expect("valid tx");
        db.transact(tx).expect("transact failed");
    }

    let events = Arc::new(Mutex::new(Vec::new()));
    let observer = Box::new(RecordingObserver::new(Arc::clone(&events)));
    db.register_observer(observer)
        .expect("observer registration should succeed");

    let recorded = events.lock().expect("events lock");
    assert!(
        matches!(recorded.as_slice(), [ObserverEvent::Catchup { from_epoch: 0, count }] if *count > 0),
        "INV-FERR-011: observer must receive catchup delivery after late registration, got {:?}",
        *recorded
    );
}

/// INV-FERR-011: registered observers receive post-commit delivery.
#[test]
fn inv_ferr_011_database_observer_commit_delivery() {
    let db = Database::genesis();
    let events = Arc::new(Mutex::new(Vec::new()));
    let observer = Box::new(RecordingObserver::new(Arc::clone(&events)));
    db.register_observer(observer)
        .expect("observer registration should succeed");

    let tx = Transaction::new(AgentId::from_bytes([4u8; 16]))
        .assert_datom(
            EntityId::from_content(b"observer-db"),
            Attribute::from("db/doc"),
            Value::String("observer".into()),
        )
        .commit(&db.schema())
        .expect("valid tx");
    db.transact(tx).expect("transact failed");

    let recorded = events.lock().expect("events lock");
    assert!(
        recorded.iter().any(|event| {
            matches!(event, ObserverEvent::Commit { epoch: 1, count } if *count > 0)
        }),
        "INV-FERR-011: registered observer must receive on_commit, got {:?}",
        *recorded
    );
}
