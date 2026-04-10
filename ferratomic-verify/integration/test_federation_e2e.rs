//! Federation end-to-end tests.
//!
//! These tests exercise multi-store composition scenarios that cross
//! the boundaries between signing, transport, merge, WAL recovery,
//! and checkpoint — the integration surfaces where Phase 4a.5 bugs hide.

use std::{collections::BTreeSet, sync::Arc};

use ed25519_dalek::SigningKey;
use ferratom::{Attribute, DatomFilter, EntityId, SignedTransactionBundle, TxSigner, Value};
use ferratomic_db::{
    db::Database,
    store::{selective_merge, Store},
    transport::{LocalTransport, Transport},
    writer::Transaction,
};
use tempfile::TempDir;

fn key_a() -> SigningKey {
    SigningKey::from_bytes(&[0xAA; 32])
}

fn key_b() -> SigningKey {
    SigningKey::from_bytes(&[0xBB; 32])
}

/// Poll a ready future (LocalTransport always returns ready).
fn block_on<F: std::future::Future>(f: F) -> F::Output {
    let mut f = std::pin::pin!(f);
    let waker = noop_waker();
    let mut cx = std::task::Context::from_waker(&waker);
    match f.as_mut().poll(&mut cx) {
        std::task::Poll::Ready(val) => val,
        std::task::Poll::Pending => panic!("expected ready future"),
    }
}

fn noop_waker() -> std::task::Waker {
    use std::task::{RawWaker, RawWakerVTable};
    fn no_op(_: *const ()) {}
    fn clone(p: *const ()) -> RawWaker {
        RawWaker::new(p, &VTABLE)
    }
    const VTABLE: RawWakerVTable = RawWakerVTable::new(clone, no_op, no_op, no_op);
    unsafe { std::task::Waker::from_raw(RawWaker::new(std::ptr::null(), &VTABLE)) }
}

// ---------------------------------------------------------------------------
// E2E 1: Diamond merge — two stores diverge, merge, verify both signers
// ---------------------------------------------------------------------------

#[test]
fn test_e2e_diamond_merge_two_signers() {
    let sk_a = key_a();
    let sk_b = key_b();
    let pk_a = sk_a.verifying_key();
    let pk_b = sk_b.verifying_key();

    // Two independent databases from genesis.
    let db_a = Database::genesis();
    let db_b = Database::genesis();
    let node_a = db_a.genesis_node();
    let node_b = db_b.genesis_node();

    // Store A: signed transaction with key A.
    let tx_a = Transaction::new(node_a)
        .assert_datom(
            EntityId::from_content(b"sensor-1"),
            Attribute::from("db/doc"),
            Value::String("temperature=22C".into()),
        )
        .commit(&db_a.schema())
        .expect("tx_a valid");
    db_a.transact_signed(tx_a, &sk_a).expect("transact_a");

    // Store B: signed transaction with key B.
    let tx_b = Transaction::new(node_b)
        .assert_datom(
            EntityId::from_content(b"sensor-2"),
            Attribute::from("db/doc"),
            Value::String("humidity=45%".into()),
        )
        .commit(&db_b.schema())
        .expect("tx_b valid");
    db_b.transact_signed(tx_b, &sk_b).expect("transact_b");

    // Merge: union of both stores.
    let store_a = Store::from_datoms(db_a.snapshot().datoms().cloned().collect());
    let store_b = Store::from_datoms(db_b.snapshot().datoms().cloned().collect());
    let merged = ferratomic_db::merge::merge(&store_a, &store_b).expect("merge");

    // The merged store contains datoms from both.
    let merged_datoms: BTreeSet<_> = merged.datoms().cloned().collect();
    assert!(
        merged.len() > store_a.len(),
        "merged must contain more datoms than A alone"
    );
    assert!(
        merged.len() > store_b.len(),
        "merged must contain more datoms than B alone"
    );

    // Both signers present in merged store.
    let signers: Vec<TxSigner> = merged
        .datoms()
        .filter(|d| d.attribute().as_str() == "tx/signer")
        .filter_map(|d| TxSigner::try_from(d.value()).ok())
        .collect();
    let signer_bytes: BTreeSet<[u8; 32]> = signers.iter().map(|s| *s.as_bytes()).collect();
    assert!(
        signer_bytes.contains(pk_a.as_bytes()),
        "merged store must contain signer A"
    );
    assert!(
        signer_bytes.contains(pk_b.as_bytes()),
        "merged store must contain signer B"
    );

    // INV-FERR-001: merge is commutative.
    let merged_ba = ferratomic_db::merge::merge(&store_b, &store_a).expect("merge_ba");
    let ba_datoms: BTreeSet<_> = merged_ba.datoms().cloned().collect();
    assert_eq!(
        merged_datoms, ba_datoms,
        "INV-FERR-001: merge(A,B) must equal merge(B,A)"
    );
}

// ---------------------------------------------------------------------------
// E2E 2: WAL recovery preserves signed transaction metadata
// ---------------------------------------------------------------------------

#[test]
fn test_e2e_wal_recovery_preserves_signatures() {
    let dir = TempDir::new().expect("tempdir");
    let wal_path = dir.path().join("test.wal");
    let sk = key_a();

    // Create DB with WAL, transact signed.
    {
        let db = Database::genesis_with_wal(&wal_path).expect("genesis_with_wal");
        let node = db.genesis_node();

        let tx = Transaction::new(node)
            .assert_datom(
                EntityId::from_content(b"wal-entity"),
                Attribute::from("db/doc"),
                Value::String("survives-crash".into()),
            )
            .commit(&db.schema())
            .expect("valid tx");
        db.transact_signed(tx, &sk).expect("transact_signed");

        // Verify signature exists before "crash".
        let snap = db.snapshot();
        assert!(
            snap.datoms()
                .any(|d| d.attribute().as_str() == "tx/signature"),
            "signature must exist before crash"
        );
        // db drops here — simulates crash
    }

    // Recover from WAL.
    let recovered = Database::recover_from_wal(&wal_path).expect("recover");
    let snap = recovered.snapshot();

    // Signature and signer must survive WAL recovery.
    assert!(
        snap.datoms()
            .any(|d| d.attribute().as_str() == "tx/signature"),
        "INV-FERR-051: tx/signature must survive WAL recovery"
    );
    assert!(
        snap.datoms().any(|d| d.attribute().as_str() == "tx/signer"),
        "INV-FERR-051: tx/signer must survive WAL recovery"
    );

    // Provenance must survive.
    assert!(
        snap.datoms()
            .any(|d| d.attribute().as_str() == "tx/provenance"),
        "INV-FERR-063: tx/provenance must survive WAL recovery"
    );

    // The user datom must survive.
    assert!(
        snap.datoms().any(|d| {
            d.attribute().as_str() == "db/doc"
                && matches!(d.value(), Value::String(s) if &**s == "survives-crash")
        }),
        "user datom must survive WAL recovery"
    );
}

// ---------------------------------------------------------------------------
// E2E 3: LocalTransport → fetch_signed_transactions → verify bundles
// ---------------------------------------------------------------------------

#[test]
fn test_e2e_transport_signed_bundle_round_trip() {
    let sk = key_a();
    let pk = sk.verifying_key();
    let db = Arc::new(Database::genesis());
    let node = db.genesis_node();

    // Transact two signed transactions.
    let tx1 = Transaction::new(node)
        .assert_datom(
            EntityId::from_content(b"transport-e1"),
            Attribute::from("db/doc"),
            Value::String("first".into()),
        )
        .commit(&db.schema())
        .expect("tx1");
    db.transact_signed(tx1, &sk).expect("signed tx1");

    let tx2 = Transaction::new(node)
        .assert_datom(
            EntityId::from_content(b"transport-e2"),
            Attribute::from("db/doc"),
            Value::String("second".into()),
        )
        .commit(&db.schema())
        .expect("tx2");
    db.transact_signed(tx2, &sk).expect("signed tx2");

    // Fetch via LocalTransport.
    let transport = LocalTransport::new(Arc::clone(&db));
    let bundles: Vec<SignedTransactionBundle> =
        block_on(transport.fetch_signed_transactions(&DatomFilter::All))
            .expect("fetch_signed_transactions");

    // Should have at least 2 signed bundles (from our txs).
    let signed_bundles: Vec<_> = bundles.iter().filter(|b| b.is_signed()).collect();
    assert!(
        signed_bundles.len() >= 2,
        "at least 2 signed bundles expected, got {}",
        signed_bundles.len()
    );

    // Each signed bundle's signer must match our key.
    for bundle in &signed_bundles {
        let signer = bundle.signer().expect("signed bundle has signer");
        assert_eq!(
            signer.as_bytes(),
            pk.as_bytes(),
            "INV-FERR-051: bundle signer must match signing key"
        );
    }

    // Each signed bundle must have user datoms.
    for bundle in &signed_bundles {
        assert!(
            !bundle.datoms().is_empty(),
            "signed bundle must have user datoms"
        );
    }
}

// ---------------------------------------------------------------------------
// E2E 4: Selective merge preserves local, transfers filtered remote
// ---------------------------------------------------------------------------

#[test]
fn test_e2e_selective_merge_cross_store() {
    let sk_expert = key_a();
    let db_expert = Database::genesis();
    let node = db_expert.genesis_node();

    // Expert store: define and populate user/* attributes.
    let schema_tx = Transaction::new(node)
        .assert_datom(
            EntityId::from_content(b"user/skill"),
            Attribute::from("db/ident"),
            Value::Keyword("user/skill".into()),
        )
        .assert_datom(
            EntityId::from_content(b"user/skill"),
            Attribute::from("db/valueType"),
            Value::Keyword("db.type/string".into()),
        )
        .assert_datom(
            EntityId::from_content(b"user/skill"),
            Attribute::from("db/cardinality"),
            Value::Keyword("db.cardinality/one".into()),
        )
        .commit(&db_expert.schema())
        .expect("schema tx");
    db_expert
        .transact_signed(schema_tx, &sk_expert)
        .expect("schema");

    let data_tx = Transaction::new(node)
        .assert_datom(
            EntityId::from_content(b"alice"),
            Attribute::from("user/skill"),
            Value::String("rust-expert".into()),
        )
        .commit(&db_expert.schema())
        .expect("data tx");
    db_expert
        .transact_signed(data_tx, &sk_expert)
        .expect("data");

    // Novice store: empty genesis.
    let novice = Store::genesis();
    let novice_len = novice.len();

    // Selective merge: only user/* from expert.
    let expert_store = Store::from_datoms(db_expert.snapshot().datoms().cloned().collect());
    let filter = DatomFilter::AttributeNamespace(vec!["user/".to_string()]);
    let (merged, receipt) =
        selective_merge(&novice, &expert_store, &filter, "expert").expect("merge");

    // Local datoms preserved.
    assert!(
        merged.len() > novice_len,
        "merged must grow beyond novice baseline"
    );

    // Only user/* transferred — no db/* or tx/* from expert.
    assert!(receipt.transferred > 0, "user/* datoms must transfer");
    assert!(
        receipt.filtered_out > 0,
        "non-user/* datoms must be filtered"
    );

    // Verify the skill datom arrived.
    assert!(
        merged.datoms().any(|d| {
            d.attribute().as_str() == "user/skill"
                && matches!(d.value(), Value::String(s) if &**s == "rust-expert")
        }),
        "user/skill datom must be in merged store"
    );

    // Idempotent: merging again changes nothing.
    let (merged2, receipt2) =
        selective_merge(&merged, &expert_store, &filter, "expert").expect("merge2");
    assert_eq!(
        merged.len(),
        merged2.len(),
        "INV-FERR-039: idempotent merge must not grow"
    );
    assert_eq!(
        receipt2.transferred, 0,
        "INV-FERR-039: idempotent merge transfers 0"
    );
}

// ---------------------------------------------------------------------------
// E2E 5: Full federation round-trip — sign, transport, merge, verify
// ---------------------------------------------------------------------------

#[test]
fn test_e2e_full_federation_round_trip() {
    let sk = key_a();

    // Source: genesis with identity + data.
    let source = Arc::new(Database::genesis_with_identity(&sk).expect("genesis_with_identity"));
    let node = source.genesis_node();

    let tx = Transaction::new(node)
        .assert_datom(
            EntityId::from_content(b"fed-entity"),
            Attribute::from("db/doc"),
            Value::String("federated-value".into()),
        )
        .commit(&source.schema())
        .expect("tx");
    source.transact_signed(tx, &sk).expect("transact");

    // Transport: fetch all datoms.
    let transport = LocalTransport::new(Arc::clone(&source));
    let remote_datoms = block_on(transport.fetch_datoms(&DatomFilter::All)).expect("fetch");

    // Build a store from transported datoms.
    let remote_store = Store::from_datoms(remote_datoms.into_iter().collect());

    // Destination: empty genesis.
    let dest = Store::genesis();

    // Selective merge: all datoms.
    let (merged, receipt) = selective_merge(&dest, &remote_store, &DatomFilter::All, "source")
        .expect("selective_merge");

    assert!(receipt.transferred > 0, "datoms must transfer");

    // The federated value arrived.
    assert!(
        merged.datoms().any(|d| {
            d.attribute().as_str() == "db/doc"
                && matches!(d.value(), Value::String(s) if &**s == "federated-value")
        }),
        "federated value must arrive in destination"
    );

    // The source's identity (store/public-key) arrived.
    assert!(
        merged
            .datoms()
            .any(|d| d.attribute().as_str() == "store/public-key"),
        "INV-FERR-060: store identity must survive federation"
    );

    // Signatures arrived.
    let sig_count = merged
        .datoms()
        .filter(|d| d.attribute().as_str() == "tx/signature")
        .count();
    assert!(
        sig_count > 0,
        "INV-FERR-051: signatures must survive federation"
    );
}
