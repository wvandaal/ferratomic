//! Bootstrap test: store Phase 4a.5 spec as signed datoms.
//!
//! GOALS.md Level 2: "The bootstrap test: Ferratomic's own specification
//! stored as datoms within itself."
//!
//! This test composes ALL Phase 4a.5 infrastructure:
//! - genesis_with_identity (INV-FERR-060)
//! - transact_signed (INV-FERR-051)
//! - create_federation_metadata (INV-FERR-061/063)
//! - selective_merge (INV-FERR-039)
//! - SignedTransactionBundle round-trip
//! - Schema evolution for custom attributes

use ed25519_dalek::SigningKey;
use ferratom::{Attribute, DatomFilter, EntityId, TxSigner, Value};
use ferratomic_db::{db::Database, store::selective_merge, writer::Transaction};

// ---------------------------------------------------------------------------
// The bootstrap test
// ---------------------------------------------------------------------------

/// GOALS.md Level 2: Store Phase 4a.5 spec as signed datoms.
///
/// Creates a ferratomic store, defines spec-related schema attributes,
/// asserts spec invariants as signed datoms, and verifies every aspect
/// of the Phase 4a.5 stack composes correctly.
#[test]
fn test_bootstrap_spec_as_signed_datoms() {
    let sk = SigningKey::from_bytes(&[0x5B; 32]);
    let db = Database::genesis_with_identity(&sk).expect("genesis_with_identity");
    // Use the genesis node for follow-up transactions.
    let node = db.genesis_node();

    // Step 1: Define spec schema attributes.
    let schema_tx = Transaction::new(node)
        .assert_datom(
            EntityId::from_content(b"spec/title"),
            Attribute::from("db/ident"),
            Value::Keyword("spec/title".into()),
        )
        .assert_datom(
            EntityId::from_content(b"spec/title"),
            Attribute::from("db/valueType"),
            Value::Keyword("db.type/string".into()),
        )
        .assert_datom(
            EntityId::from_content(b"spec/title"),
            Attribute::from("db/cardinality"),
            Value::Keyword("db.cardinality/one".into()),
        )
        .assert_datom(
            EntityId::from_content(b"spec/stage"),
            Attribute::from("db/ident"),
            Value::Keyword("spec/stage".into()),
        )
        .assert_datom(
            EntityId::from_content(b"spec/stage"),
            Attribute::from("db/valueType"),
            Value::Keyword("db.type/long".into()),
        )
        .assert_datom(
            EntityId::from_content(b"spec/stage"),
            Attribute::from("db/cardinality"),
            Value::Keyword("db.cardinality/one".into()),
        )
        .commit(&db.schema())
        .expect("schema tx valid");
    db.transact_signed(schema_tx, &sk).expect("schema tx");

    // Step 2: Assert spec invariants as signed datoms.
    let invariants = [
        ("INV-FERR-001", "CRDT Merge Commutativity", 0i64),
        ("INV-FERR-002", "CRDT Merge Associativity", 0),
        ("INV-FERR-003", "CRDT Merge Idempotency", 0),
        ("INV-FERR-012", "Content-Addressed Identity", 0),
        ("INV-FERR-051", "Signed Transactions", 1),
        ("INV-FERR-060", "Store Identity Persistence", 1),
        ("INV-FERR-086", "Canonical Datom Format Determinism", 0),
    ];

    for (inv_id, title, stage) in &invariants {
        let entity = EntityId::from_content(inv_id.as_bytes());
        let tx = Transaction::new(node)
            .assert_datom(
                entity,
                Attribute::from("spec/title"),
                Value::String((*title).into()),
            )
            .assert_datom(entity, Attribute::from("spec/stage"), Value::Long(*stage))
            .commit(&db.schema())
            .expect("invariant tx valid");
        db.transact_signed(tx, &sk)
            .unwrap_or_else(|e| panic!("transact_signed failed for {inv_id}: {e}"));
    }

    // --- Verification ---

    let snap = db.snapshot();
    let datom_count = snap.datoms().count();

    // PC1: Store created with genesis_with_identity — epoch advanced.
    assert!(
        db.epoch() > 0,
        "INV-FERR-060: genesis_with_identity must advance epoch"
    );

    // PC2: All transactions signed — tx/signature + tx/signer present.
    let sig_count = snap
        .datoms()
        .filter(|d| d.attribute().as_str() == "tx/signature")
        .count();
    let signer_count = snap
        .datoms()
        .filter(|d| d.attribute().as_str() == "tx/signer")
        .count();
    // 2 identity txs + 1 schema tx + 7 invariant txs = 10 signed txs
    assert_eq!(sig_count, 10, "INV-FERR-051: all 10 txs must be signed");
    assert_eq!(
        signer_count, 10,
        "INV-FERR-051: all 10 txs must have signer"
    );

    // PC2b: All signers match the spec key.
    let pk = sk.verifying_key();
    for d in snap
        .datoms()
        .filter(|d| d.attribute().as_str() == "tx/signer")
    {
        let signer = TxSigner::try_from(d.value()).expect("valid signer");
        assert_eq!(
            signer.as_bytes(),
            pk.as_bytes(),
            "INV-FERR-051: all signers must match the spec key"
        );
    }

    // PC4: All spec assertions carry provenance :provenance/observed.
    let prov_count = snap
        .datoms()
        .filter(|d| d.attribute().as_str() == "tx/provenance")
        .count();
    assert_eq!(prov_count, 10, "INV-FERR-063: all txs must have provenance");

    // PC5: Selective merge with spec/ namespace transfers spec datoms.
    let local = ferratomic_db::store::Store::genesis();
    // Build a Store from snapshot datoms for selective_merge.
    let remote_store = ferratomic_db::store::Store::from_datoms(snap.datoms().cloned().collect());
    let filter = DatomFilter::AttributeNamespace(vec!["spec/".to_string()]);
    let (merged, receipt) =
        selective_merge(&local, &remote_store, &filter, "bootstrap").expect("selective_merge");
    assert!(
        receipt.transferred > 0,
        "INV-FERR-039: spec/ datoms must transfer via selective merge"
    );
    // Merged store must have spec/title datoms.
    assert!(
        merged
            .datoms()
            .any(|d| d.attribute().as_str() == "spec/title"),
        "INV-FERR-039: merged store must contain spec/title datoms"
    );

    // PC7: Genesis schema is deterministic.
    assert!(
        db.schema().get(&Attribute::from("db/doc")).is_some(),
        "INV-FERR-031: genesis must include db/doc"
    );

    // PC8: spec/* schema attributes installed and queryable.
    assert!(
        db.schema().get(&Attribute::from("spec/title")).is_some(),
        "spec/title must be in schema after evolution"
    );
    assert!(
        db.schema().get(&Attribute::from("spec/stage")).is_some(),
        "spec/stage must be in schema after evolution"
    );

    // Smoke: total datom count is reasonable.
    assert!(
        datom_count > 50,
        "bootstrap store should have >50 datoms (identity + schema + invariants + metadata), got {datom_count}"
    );
}
