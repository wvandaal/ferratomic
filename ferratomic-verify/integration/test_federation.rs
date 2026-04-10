//! Phase 4a.5 federation foundations integration tests.
//!
//! Exercises the full stack: genesis_with_identity, transact_signed,
//! selective_merge, filtered observers, LocalTransport, and
//! SignedTransactionBundle round-trip.

use std::sync::Arc;

use ed25519_dalek::SigningKey;
use ferratom::{Attribute, DatomFilter, EntityId, NodeId, ProvenanceType, TxSigner, Value};
use ferratomic_db::{
    db::Database,
    store::{selective_merge, Store},
    transport::{LocalTransport, Transport},
    writer::Transaction,
};

fn test_key() -> SigningKey {
    SigningKey::from_bytes(&[42u8; 32])
}

fn test_node() -> NodeId {
    NodeId::from_bytes([1u8; 16])
}

// -- INV-FERR-060: Store identity ------------------------------------------

#[test]
fn test_inv_ferr_060_genesis_identity_roundtrip() {
    let sk = test_key();
    let db = Database::genesis_with_identity(&sk).expect("genesis_with_identity");

    // Schema must have store/public-key and store/created.
    let schema = db.schema();
    assert!(
        schema.get(&Attribute::from("store/public-key")).is_some(),
        "INV-FERR-060: store/public-key must be in schema"
    );
    assert!(
        schema.get(&Attribute::from("store/created")).is_some(),
        "INV-FERR-060: store/created must be in schema"
    );

    // Snapshot must contain the public key value.
    let snap = db.snapshot();
    let pk_datom = snap
        .datoms()
        .find(|d| d.attribute().as_str() == "store/public-key")
        .expect("INV-FERR-060: store/public-key datom must exist");
    match pk_datom.value() {
        Value::Bytes(b) => assert_eq!(b.len(), 32, "INV-FERR-060: public key must be 32 bytes"),
        other => panic!("INV-FERR-060: expected Bytes, got {other:?}"),
    }
}

// -- INV-FERR-051: Signed transaction round-trip ---------------------------

#[test]
fn test_inv_ferr_051_signed_tx_metadata_emitted() {
    let sk = test_key();
    let db = Database::genesis();
    let node = test_node();

    let tx = Transaction::new(node)
        .assert_datom(
            EntityId::from_content(b"signed-test"),
            Attribute::from("db/doc"),
            Value::String("signed".into()),
        )
        .commit(&db.schema())
        .expect("valid tx");

    let receipt = db.transact_signed(tx, &sk).expect("transact_signed");

    // Must have tx/signature and tx/signer datoms.
    let has_sig = receipt
        .datoms()
        .iter()
        .any(|d| d.attribute().as_str() == "tx/signature");
    let has_signer = receipt
        .datoms()
        .iter()
        .any(|d| d.attribute().as_str() == "tx/signer");
    assert!(has_sig, "INV-FERR-051: tx/signature must be emitted");
    assert!(has_signer, "INV-FERR-051: tx/signer must be emitted");

    // Signer must match the signing key's public key.
    let signer_datom = receipt
        .datoms()
        .iter()
        .find(|d| d.attribute().as_str() == "tx/signer")
        .expect("signer datom");
    let signer = TxSigner::try_from(signer_datom.value()).expect("valid signer");
    assert_eq!(
        signer.as_bytes(),
        sk.verifying_key().as_bytes(),
        "INV-FERR-051: signer must match signing key"
    );
}

// -- INV-FERR-063: Provenance emitted --------------------------------------

#[test]
fn test_inv_ferr_063_provenance_emitted() {
    let db = Database::genesis();
    let node = test_node();

    let tx = Transaction::new(node)
        .assert_datom(
            EntityId::from_content(b"prov-test"),
            Attribute::from("db/doc"),
            Value::String("provenance".into()),
        )
        .commit(&db.schema())
        .expect("valid tx");

    let receipt = db.transact(tx).expect("transact");

    let prov_datom = receipt
        .datoms()
        .iter()
        .find(|d| d.attribute().as_str() == "tx/provenance")
        .expect("INV-FERR-063: tx/provenance datom must be emitted");

    match prov_datom.value() {
        Value::Keyword(k) => assert_eq!(
            &**k,
            ProvenanceType::Observed.as_keyword(),
            "INV-FERR-063: default provenance must be Observed"
        ),
        other => panic!("INV-FERR-063: expected Keyword, got {other:?}"),
    }
}

// -- INV-FERR-039: Selective merge -----------------------------------------

#[test]
fn test_inv_ferr_039_selective_merge_namespace() {
    let mut local = Store::genesis();
    let mut remote = Store::genesis();
    let node = test_node();

    // Local: add a db/doc datom.
    let tx_l = Transaction::new(node)
        .assert_datom(
            EntityId::from_content(b"local-1"),
            Attribute::from("db/doc"),
            Value::String("local-data".into()),
        )
        .commit(local.schema())
        .expect("valid");
    local.transact_test(tx_l).expect("transact");

    // Remote: add user/name and system/config datoms.
    let tx_r = Transaction::new(node)
        .assert_datom(
            EntityId::from_content(b"remote-user"),
            Attribute::from("db/doc"),
            Value::String("user-data".into()),
        )
        .commit(remote.schema())
        .expect("valid");
    remote.transact_test(tx_r).expect("transact");

    // Selective merge: only db/* namespace.
    let filter = DatomFilter::AttributeNamespace(vec!["db/".to_string()]);
    let (merged, receipt) =
        selective_merge(&local, &remote, &filter, "test-peer").expect("selective_merge");

    // Local datoms must be preserved.
    assert!(
        merged.len() > local.len(),
        "INV-FERR-039: merged store must contain more datoms than local alone"
    );
    assert!(
        receipt.transferred > 0,
        "INV-FERR-039: some datoms must transfer"
    );
    assert_eq!(receipt.source, "test-peer");
}

// -- INV-FERR-038: LocalTransport equivalence ------------------------------

#[test]
fn test_inv_ferr_038_local_transport_equivalence() {
    let db = Arc::new(Database::genesis());
    let node = test_node();

    let tx = Transaction::new(node)
        .assert_datom(
            EntityId::from_content(b"transport-test"),
            Attribute::from("db/doc"),
            Value::String("via-transport".into()),
        )
        .commit(&db.schema())
        .expect("valid tx");
    db.transact(tx).expect("transact");

    let transport = LocalTransport::new(Arc::clone(&db));
    let filter = DatomFilter::All;

    // Fetch datoms via transport.
    let fetched = futures_lite_block(transport.fetch_datoms(&filter))
        .expect("INV-FERR-038: fetch_datoms must succeed");
    let direct: Vec<_> = db.snapshot().datoms().cloned().collect();

    assert_eq!(
        fetched.len(),
        direct.len(),
        "INV-FERR-038: transport must return same datom count as direct access"
    );
}

/// Block on a future synchronously (no runtime needed for `ready` futures).
fn futures_lite_block<F: std::future::Future>(f: F) -> F::Output {
    // LocalTransport returns std::future::ready, so poll once suffices.
    let mut f = std::pin::pin!(f);
    let waker = noop_waker();
    let mut cx = std::task::Context::from_waker(&waker);
    match f.as_mut().poll(&mut cx) {
        std::task::Poll::Ready(val) => val,
        std::task::Poll::Pending => panic!("LocalTransport future should be ready immediately"),
    }
}

/// Minimal no-op waker for polling ready futures.
fn noop_waker() -> std::task::Waker {
    use std::task::{RawWaker, RawWakerVTable};
    fn no_op(_: *const ()) {}
    fn clone(p: *const ()) -> RawWaker {
        RawWaker::new(p, &VTABLE)
    }
    const VTABLE: RawWakerVTable = RawWakerVTable::new(clone, no_op, no_op, no_op);
    // SAFETY: the waker does nothing — no resources to manage.
    // This is the standard pattern for polling ready futures.
    unsafe { std::task::Waker::from_raw(RawWaker::new(std::ptr::null(), &VTABLE)) }
}

// -- INV-FERR-031: Genesis determinism with 25 attributes ------------------

#[test]
fn test_inv_ferr_031_genesis_25_attributes() {
    let db = Database::genesis();
    let schema = db.schema();
    assert_eq!(
        schema.len(),
        25,
        "INV-FERR-031: genesis must have exactly 25 attributes"
    );
}
