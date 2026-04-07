//! Schema and identity Kani harnesses.
//!
//! Covers INV-FERR-009 and INV-FERR-012.

use ferratom::{AgentId, Attribute, EntityId, Value};
use ferratomic_db::{
    store::Store,
    writer::{Transaction, TxValidationError},
};

#[cfg(not(kani))]
use super::kani;

/// INV-FERR-009: unknown attributes are rejected at commit time.
#[cfg_attr(kani, kani::proof)]
#[cfg_attr(kani, kani::unwind(6))]
#[cfg_attr(not(kani), test)]
#[cfg_attr(not(kani), ignore = "requires Kani verifier")]
fn schema_rejects_unknown_attr() {
    let store = Store::genesis();
    let tx = Transaction::new(AgentId::from_bytes([0u8; 16])).assert_datom(
        EntityId::from_content(b"kani-unknown-attr"),
        Attribute::from("nonexistent/attr"),
        Value::String("value".into()),
    );

    let result = tx.commit(store.schema());
    assert!(matches!(
        result,
        Err(TxValidationError::UnknownAttribute(_))
    ));
}

/// INV-FERR-012: identical content produces identical entity identities.
#[cfg_attr(kani, kani::proof)]
#[cfg_attr(kani, kani::unwind(4))]
#[cfg_attr(not(kani), test)]
#[cfg_attr(not(kani), ignore = "requires Kani verifier")]
fn content_identity() {
    let content: [u8; 16] = kani::any();
    let id1 = EntityId::from_content(&content);
    let id2 = EntityId::from_content(&content);
    assert_eq!(id1, id2);

    let other_content: [u8; 16] = kani::any();
    kani::assume(content != other_content);
    let _id3 = EntityId::from_content(&other_content);
}
